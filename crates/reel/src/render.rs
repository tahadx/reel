//! The v1 render base: one ffmpeg pass that applies the project camera and
//! re-encodes the take. Canvas, cursor, ripples, webcam, and exports are later
//! branches; this file owns the camera path and the ffmpeg plumbing they build
//! on.

use crate::cli::RenderArgs;
use crate::finalize;
use crate::paths::{self, Paths};
use crate::project::{self, Camera, Keyframe, Project};
use anyhow::{bail, Context, Result};
use std::fs;
use std::path::Path;

/// Above this length the per-keyframe expressions are replaced by a generated
/// `sendcmd` script, as the render contract requires.
pub const EXPRESSION_LIMIT: usize = 64 * 1024;

/// ffmpeg's expression parser nests `if()` calls and gives up somewhere around
/// 500 levels. A dense hand-edited project can collapse many keyframes onto the
/// same output frame, producing a short but deeply nested expression, so keep
/// well clear of that limit and use the `sendcmd` path instead.
pub const EXPRESSION_KEYFRAME_LIMIT: usize = 128;

#[derive(Debug)]
pub struct CameraFilter {
    /// The ffmpeg video filter graph.
    pub graph: String,
    /// Present when the graph reads a `sendcmd` script.
    pub sendcmd: Option<String>,
}

pub fn run(paths: &Paths, args: RenderArgs) -> Result<()> {
    let project_path = args.project;
    let project = Project::load(&project_path)?;
    let input = project.take_path(&project_path);
    if !input.is_file() {
        bail!(
            "the take for this project is missing at {}",
            input.display()
        );
    }
    let output = args
        .output
        .unwrap_or_else(|| project::rendered_path(&input));
    if output == input {
        bail!("a re-render cannot overwrite the original take");
    }
    render_take(paths, &project, &input, &output, 0.0, false)?;
    println!("{}", output.display());
    Ok(())
}

/// Render `input` to `output` with the project's camera. `trim_start` is only
/// non-zero for the hidden save pass, which reads the raw capture; a re-render
/// already points at the saved take.
pub fn render_take(
    paths: &Paths,
    project: &Project,
    input: &Path,
    output: &Path,
    trim_start: f64,
    noise_cancellation: bool,
) -> Result<()> {
    if !input.is_file() {
        bail!("recording is missing at {}", input.display());
    }
    let width = project.source.width;
    let height = project.source.height;
    if width == 0 || height == 0 {
        bail!("the project has no source width and height");
    }
    let fps = if project.source.fps.is_finite() && project.source.fps > 0.0 {
        project.source.fps
    } else {
        60.0
    };

    let sendcmd_path = paths
        .runtime
        .join(format!("render-{}.sendcmd", std::process::id()));
    let camera = camera_filter(
        &project.camera,
        width,
        height,
        fps,
        project.source.duration,
        &sendcmd_path,
    );
    if let Some(script) = camera.as_ref().and_then(|filter| filter.sendcmd.as_ref()) {
        fs::write(&sendcmd_path, script)
            .with_context(|| format!("cannot write {}", sendcmd_path.display()))?;
    }

    let work = finalize::work_path(input, output);
    let ext = output
        .extension()
        .map(|ext| ext.to_string_lossy().to_string())
        .unwrap_or_default();
    let trim = if trim_start.is_finite() && trim_start > 0.0 {
        Some(trim_start)
    } else {
        None
    };
    let audio_present = util_has_audio(input);
    let audio_filter = finalize::audio_filter(noise_cancellation);
    let graph = camera.as_ref().map(|filter| filter.graph.clone());

    let attempt = |use_audio: bool| -> bool {
        let mut args: Vec<String> = vec!["-y".into()];
        if let Some(trim) = trim {
            args.extend(["-ss".into(), format!("{trim:.3}")]);
        }
        args.extend(["-i".into(), input.display().to_string()]);
        if let Some(graph) = &graph {
            args.extend(["-vf".into(), graph.clone()]);
        }
        args.extend([
            "-c:v".into(),
            "libx264".into(),
            "-preset".into(),
            "veryfast".into(),
            "-crf".into(),
            "20".into(),
            "-pix_fmt".into(),
            "yuv420p".into(),
        ]);
        if use_audio {
            args.extend([
                "-af".into(),
                audio_filter.clone(),
                "-c:a".into(),
                "aac".into(),
                "-b:a".into(),
                "192k".into(),
                "-ar".into(),
                "48000".into(),
            ]);
        } else {
            args.push("-an".into());
        }
        if ext == "mp4" {
            args.extend(["-movflags".into(), "+faststart".into()]);
        }
        args.push(work.display().to_string());
        finalize::run_ffmpeg(paths, &args)
    };

    let mut ok = attempt(audio_present);
    if !ok && audio_present {
        paths::log(
            paths,
            &format!("render: retrying without audio for {}", input.display()),
        );
        ok = attempt(false);
    }

    if ok && finalize::valid_video(&work) {
        fs::rename(&work, output)
            .with_context(|| format!("cannot move the rendered take to {}", output.display()))?;
        paths::remove_quiet(&sendcmd_path);
        Ok(())
    } else {
        paths::remove_quiet(&work);
        paths::log(
            paths,
            &format!(
                "render: {} for {}",
                if ok {
                    "output failed validation"
                } else {
                    "ffmpeg failed"
                },
                input.display()
            ),
        );
        bail!("the render pass failed")
    }
}

fn util_has_audio(input: &Path) -> bool {
    crate::util::has_audio_stream(input)
}

/// Build the camera graph for a project camera. Small keyframe paths use
/// `zoompan` expressions over the output frame number; a path whose expressions
/// exceed `EXPRESSION_LIMIT` switches to a generated `sendcmd` script driving
/// `crop`, then scales back to the source size.
pub fn camera_filter(
    camera: &Camera,
    width: u32,
    height: u32,
    fps: f64,
    duration: f64,
    sendcmd_path: &Path,
) -> Option<CameraFilter> {
    if !camera.needs_zoom() {
        return None;
    }
    let z = piecewise(&camera.keyframes, fps, |frame| frame.zoom);
    let center_x = piecewise(&camera.keyframes, fps, |frame| frame.x);
    let center_y = piecewise(&camera.keyframes, fps, |frame| frame.y);
    // `zoom` in the x/y expressions is the zoompan filter's current z, so the
    // top-left follows the eased zoom without repeating the z expression.
    let x = format!("max(0,min(({center_x})-iw/(2*zoom),iw-iw/zoom))");
    let y = format!("max(0,min(({center_y})-ih/(2*zoom),ih-ih/zoom))");

    if camera.keyframes.len() <= EXPRESSION_KEYFRAME_LIMIT
        && z.len().max(x.len()).max(y.len()) <= EXPRESSION_LIMIT
    {
        let fps_text = number(fps);
        return Some(CameraFilter {
            graph: format!("zoompan=z='{z}':x='{x}':y='{y}':d=1:s={width}x{height}:fps={fps_text}"),
            sendcmd: None,
        });
    }

    let script = sendcmd_script(camera, width, height, fps, duration);
    Some(CameraFilter {
        graph: format!(
            "sendcmd=f='{}',crop=w={width}:h={height}:x=0:y=0,scale={width}:{height}",
            escape_filter_value(sendcmd_path)
        ),
        sendcmd: Some(script),
    })
}

/// A smoothstep interpolation for one camera field over output frame numbers.
///
/// The expression is a chain of `if(lt(on, frame), segment, ...)`, so its size
/// grows with the number of keyframes rather than the take duration.
fn piecewise(frames: &[Keyframe], fps: f64, value: impl Fn(&Keyframe) -> f64) -> String {
    if frames.is_empty() {
        return "1".into();
    }
    if frames.len() == 1 {
        return number(value(&frames[0]));
    }
    let marks: Vec<f64> = frames
        .iter()
        .map(|frame| (frame.t * fps).round().max(0.0))
        .collect();
    let mut expr = number(value(frames.last().expect("not empty")));
    for i in (0..frames.len() - 1).rev() {
        let start = marks[i];
        let end = marks[i + 1];
        let segment = if end > start {
            let from = value(&frames[i]);
            let to = value(&frames[i + 1]);
            let u = format!("(on-{})/{}", number(start), number(end - start));
            format!(
                "({}+({}-{})*{u}*{u}*(3-2*{u}))",
                number(from),
                number(to),
                number(from)
            )
        } else {
            number(value(&frames[i + 1]))
        };
        expr = format!("if(lt(on,{}),{segment},({expr}))", number(end));
    }
    format!(
        "if(lt(on,{}),{},({expr}))",
        number(marks[0]),
        number(value(&frames[0]))
    )
}

/// Per-frame crop commands for the large-path fallback. `sendcmd` runs before
/// `crop`, so the command at 0.0 applies to the first frame.
pub fn sendcmd_script(camera: &Camera, width: u32, height: u32, fps: f64, duration: f64) -> String {
    let fps = if fps.is_finite() && fps > 0.0 {
        fps
    } else {
        60.0
    };
    let last = camera
        .keyframes
        .last()
        .map(|frame| frame.t)
        .unwrap_or_default();
    let duration = if duration.is_finite() && duration > 0.0 {
        duration
    } else {
        last + 1.0
    };
    let frames = (duration * fps).ceil().max(1.0) as u64;

    let source_w = width as f64;
    let source_h = height as f64;
    let mut script = String::new();
    let mut previous: Option<(u32, u32, i64, i64)> = None;
    for frame in 0..=frames {
        let t = frame as f64 / fps;
        let (zoom, cx, cy) = camera.viewport_at(t, source_w, source_h);
        let view_w = even_dimension(source_w / zoom, width);
        let view_h = even_dimension(source_h / zoom, height);
        let max_x = i64::from(width.saturating_sub(view_w));
        let max_y = i64::from(height.saturating_sub(view_h));
        let x = ((cx - f64::from(view_w) / 2.0).round() as i64).clamp(0, max_x);
        let y = ((cy - f64::from(view_h) / 2.0).round() as i64).clamp(0, max_y);
        if previous == Some((view_w, view_h, x, y)) {
            continue;
        }
        previous = Some((view_w, view_h, x, y));
        let stamp = format!("{t:.6}");
        script.push_str(&format!(
            "{stamp} crop w {view_w};\n{stamp} crop h {view_h};\n\
             {stamp} crop x {x};\n{stamp} crop y {y};\n"
        ));
    }
    script
}

/// yuv420p needs even dimensions, and a crop cannot be larger than the source.
fn even_dimension(value: f64, max: u32) -> u32 {
    let max = (max & !1).max(2);
    let mut dimension = value.round() as i64;
    dimension = dimension.clamp(2, i64::from(max));
    let mut dimension = dimension as u32;
    if dimension & 1 == 1 {
        dimension -= 1;
    }
    dimension.max(2)
}

fn escape_filter_value(path: &Path) -> String {
    path.display()
        .to_string()
        .replace('\\', "\\\\")
        .replace('\'', "\\'")
}

fn number(value: f64) -> String {
    if !value.is_finite() {
        return "0".into();
    }
    let value = if value.abs() < 5e-7 { 0.0 } else { value };
    let mut text = format!("{value:.6}");
    while text.contains('.') && text.ends_with('0') {
        text.pop();
    }
    if text.ends_with('.') {
        text.pop();
    }
    if text == "-0" {
        text = "0".into();
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project::{Camera, Keyframe};

    fn camera(frames: Vec<Keyframe>) -> Camera {
        Camera {
            style: "cinematic".into(),
            depth: 2.0,
            easing: "smoothstep".into(),
            keyframes: frames,
            ..Camera::default()
        }
    }

    #[test]
    fn a_small_path_uses_zoompan_expressions() {
        let camera = camera(vec![
            Keyframe::new(0.0, 1.0, 320.0, 180.0),
            Keyframe::new(1.0, 2.0, 400.0, 200.0),
            Keyframe::new(2.0, 1.0, 320.0, 180.0),
        ]);
        let filter = camera_filter(
            &camera,
            640,
            360,
            60.0,
            2.0,
            Path::new("/run/user/1/reel/render.sendcmd"),
        )
        .expect("a filter");

        assert!(filter.graph.starts_with("zoompan="));
        assert!(filter.graph.contains("s=640x360"));
        assert!(filter.graph.contains("fps=60"));
        assert!(filter.graph.contains("zoom"));
        assert!(filter.sendcmd.is_none());
    }

    #[test]
    fn no_zoom_means_no_camera_filter() {
        let still = camera(vec![
            Keyframe::new(0.0, 1.0, 320.0, 180.0),
            Keyframe::new(2.0, 1.0, 320.0, 180.0),
        ]);
        assert!(camera_filter(&still, 640, 360, 60.0, 2.0, Path::new("/tmp/never")).is_none());

        let off = Camera::default();
        assert!(!off.needs_zoom());
    }

    #[test]
    fn a_single_keyframe_holds_a_constant_zoom() {
        let camera = camera(vec![Keyframe::new(1.0, 2.0, 480.0, 180.0)]);
        let filter =
            camera_filter(&camera, 640, 360, 30.0, 3.0, Path::new("/tmp/never")).expect("a filter");
        assert!(filter.graph.starts_with("zoompan="));
        assert!(filter.graph.contains("z='2'"));
    }

    #[test]
    fn style_off_disables_zoomed_keyframes() {
        let mut camera = camera(vec![
            Keyframe::new(0.0, 1.0, 320.0, 180.0),
            Keyframe::new(1.0, 2.0, 320.0, 180.0),
        ]);
        camera.style = "off".into();
        assert!(camera_filter(&camera, 640, 360, 60.0, 2.0, Path::new("/tmp/never")).is_none());
    }

    #[test]
    fn a_large_path_switches_to_sendcmd() {
        let mut frames = Vec::new();
        let mut zoom = 1.0;
        for step in 0..1500 {
            frames.push(Keyframe::new(
                step as f64 * 0.1,
                zoom,
                300.0 + step as f64,
                180.0,
            ));
            zoom = if zoom > 1.5 { 1.0 } else { 2.0 };
        }
        let filter = camera_filter(
            &camera(frames),
            640,
            360,
            60.0,
            60.0,
            Path::new("/run/user/1/reel/render.sendcmd"),
        )
        .expect("a filter");

        assert!(filter.graph.starts_with("sendcmd=f='"));
        assert!(filter.graph.contains("scale=640:360"));
        let script = filter.sendcmd.expect("a sendcmd script");
        assert!(script.contains("crop w"));
        assert!(script.contains("0.000000 crop w"));
    }

    #[test]
    fn the_sendcmd_fallback_never_crops_outside_the_source() {
        let camera = camera(vec![
            Keyframe::new(0.0, 1.0, 0.0, 0.0),
            Keyframe::new(1.0, 2.0, 0.0, 0.0),
            Keyframe::new(2.0, 1.0, 640.0, 360.0),
        ]);
        let script = sendcmd_script(&camera, 640, 360, 10.0, 2.0);
        let mut saw_zoom = false;
        for line in script.lines() {
            let parts: Vec<&str> = line.split_whitespace().collect();
            assert!(parts[0].parse::<f64>().is_ok());
            assert_eq!(parts[1], "crop");
            let value: i64 = parts[3].trim_end_matches(';').parse().unwrap();
            match parts[2] {
                "w" => {
                    assert!((2..=640).contains(&value));
                    assert_eq!(value % 2, 0);
                    if value < 640 {
                        saw_zoom = true;
                    }
                }
                "h" => {
                    assert!((2..=360).contains(&value));
                    assert_eq!(value % 2, 0);
                }
                "x" => assert!(value >= 0),
                "y" => assert!(value >= 0),
                other => panic!("unexpected command {other}"),
            }
        }
        assert!(saw_zoom);
    }

    #[test]
    fn expression_numbers_are_compact() {
        assert_eq!(number(60.0), "60");
        assert_eq!(number(1.5), "1.5");
        assert_eq!(number(-0.0), "0");
        assert_eq!(number(f64::NAN), "0");
        assert_eq!(number(0.000001), "0.000001");
    }
}
