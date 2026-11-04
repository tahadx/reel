use crate::cli::FinalizeArgs;
use crate::config;
use crate::paths::{self, Paths};
use crate::project;
use crate::render;
use crate::state;
use crate::telemetry;
use crate::util;
use anyhow::{bail, Context, Result};
use serde_json::{json, Value};
use std::fs::{self, OpenOptions};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

/// Wait for a child without letting a wedged passthrough hold the save
/// forever. A 10 minute cap is far beyond a normal trim/loudnorm pass.
fn run_with_timeout(mut command: Command, timeout: Duration) -> bool {
    let Ok(mut child) = command.spawn() else {
        return false;
    };
    let deadline = Instant::now() + timeout;
    loop {
        match child.try_wait() {
            Ok(Some(status)) => return status.success(),
            Ok(None) if Instant::now() < deadline => {
                thread::sleep(Duration::from_millis(50));
            }
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return false;
            }
        }
    }
}

pub fn make_preview(file: &Path, preview: &Path, seek: f64) -> bool {
    if !file.is_file() {
        return false;
    }
    let mut command = Command::new("ffmpeg");
    command
        .args(["-y", "-ss", &format!("{seek:.3}"), "-i"])
        .arg(file)
        .args(["-frames:v", "1", "-q:v", "3"])
        .arg(preview)
        .args(["-loglevel", "error"])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    run_with_timeout(command, Duration::from_secs(30))
}

fn first_gop_has_discard(file: &Path) -> bool {
    util::run_stdout_within(
        "ffprobe",
        &[
            "-v",
            "error",
            "-select_streams",
            "v:0",
            "-read_intervals",
            "%+0.2",
            "-show_entries",
            "packet=flags",
            "-of",
            "csv=p=0",
            &file.display().to_string(),
        ],
        Duration::from_secs(20),
    )
    .map(|out| out.contains('D'))
    .unwrap_or(false)
}

pub(crate) fn work_path(file: &Path, output: &Path) -> PathBuf {
    let ext = file
        .extension()
        .map(|e| e.to_string_lossy().to_string())
        .unwrap_or_default();
    let stem = output.with_extension("").to_string_lossy().to_string();
    PathBuf::from(format!("{stem}.part.{}.{ext}", std::process::id()))
}

pub(crate) fn run_ffmpeg(paths: &Paths, args: &[String]) -> bool {
    paths::log(paths, &format!("finalize: ffmpeg {}", args.join(" ")));
    let Ok(log) = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&paths.log)
    else {
        return false;
    };
    let err = match log.try_clone() {
        Ok(file) => file,
        Err(_) => return false,
    };
    let mut command = Command::new("ffmpeg");
    command
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::from(log))
        .stderr(Stdio::from(err));
    run_with_timeout(command, Duration::from_secs(600))
}

/// A finished file must have a video stream and a real duration. Checking the
/// work file before the rename means a broken MP4 can never replace the raw
/// take that the recorder actually wrote.
pub(crate) fn valid_video(file: &Path) -> bool {
    if !file.is_file() || fs::metadata(file).map(|m| m.len()).unwrap_or(0) == 0 {
        return false;
    }
    let video = util::run_stdout_within(
        "ffprobe",
        &[
            "-v",
            "error",
            "-select_streams",
            "v:0",
            "-show_entries",
            "stream=codec_type",
            "-of",
            "csv=p=0",
            &file.display().to_string(),
        ],
        Duration::from_secs(20),
    )
    .map(|out| out.contains("video"))
    .unwrap_or(false);
    video && util::duration_of(file) > 0.0
}

/// Trim the first frame, mute the PipeWire pop, optionally reduce microphone
/// noise, normalize the audio, and either stream-copy the video or re-encode
/// it. The source is left untouched until the finished file has been validated
/// and moved into place.
pub fn finalize_video(
    paths: &Paths,
    file: &Path,
    trim_start: f64,
    output: &Path,
    noise_cancellation: bool,
) -> Result<()> {
    if !file.is_file() {
        anyhow::bail!("recording is missing");
    }
    let ext = file
        .extension()
        .map(|e| e.to_string_lossy().to_string())
        .unwrap_or_default();
    let work = work_path(file, output);

    let needs_video_reencode = first_gop_has_discard(file);

    let mut audio_present = util::has_audio_stream(file);
    let src_duration = util::duration_of(file);
    let mut trim = if trim_start > 0.0 {
        format!("{trim_start:.3}")
    } else {
        "0.1".to_string()
    };
    if src_duration > 0.0 && src_duration < 0.8 {
        paths::log(
            paths,
            &format!("finalize: short take ({src_duration}s), dropping audio"),
        );
        audio_present = false;
        if src_duration < 0.4 {
            trim = "0".into();
        }
    }

    let audio_filter = audio_filter(noise_cancellation);

    let attempt = |use_audio: bool| -> bool {
        let mut args: Vec<String> = vec![
            "-y".into(),
            "-ss".into(),
            trim.clone(),
            "-i".into(),
            file.display().to_string(),
        ];
        if needs_video_reencode {
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
        } else {
            args.extend(["-c:v".into(), "copy".into()]);
        }
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
        run_ffmpeg(paths, &args)
    };

    let mut ok = attempt(audio_present);
    if !ok && audio_present {
        paths::log(
            paths,
            &format!("finalize: retrying without audio for {}", file.display()),
        );
        ok = attempt(false);
    }

    if ok && valid_video(&work) {
        fs::rename(&work, output)
            .with_context(|| format!("cannot move the finished take to {}", output.display()))?;
        Ok(())
    } else {
        paths::remove_quiet(&work);
        paths::log(
            paths,
            &format!(
                "finalize: {} for {}",
                if ok {
                    "output failed validation"
                } else {
                    "ffmpeg failed"
                },
                file.display()
            ),
        );
        anyhow::bail!("the final pass failed")
    }
}

pub(crate) fn audio_filter(noise_cancellation: bool) -> String {
    let mut filter = String::from("volume=enable='lt(t,0.4)':volume=0,afade=t=in:st=0.4:d=0.05");
    if noise_cancellation {
        // Opt-in and only requested for takes that captured the microphone.
        // Denoise before loudnorm so the normalizer measures clean speech.
        filter.push_str(",afftdn=nf=-30");
    }
    filter.push_str(",loudnorm=I=-14:TP=-1.5:LRA=11");
    filter
}

/// Move a countdown capture aside before the save pass. Split out so the
/// denied-rename path is testable without running ffmpeg.
fn quarantine_before_save(
    paths: &Paths,
    file: &mut PathBuf,
    output: &mut PathBuf,
    trim_start: f64,
    quarantined: &mut bool,
) -> Result<()> {
    if *quarantined || trim_start <= 0.0 || paths::is_untrimmed(file) {
        return Ok(());
    }
    match crate::quarantine::capture(paths, file) {
        Ok(capture) => {
            *output = file.clone();
            *file = capture;
            *quarantined = true;
            Ok(())
        }
        Err(err) => {
            let message = format!(
                "The countdown capture could not be quarantined before saving: {err}. \
                 The original file is kept at {} and withheld from history and sharing \
                 until it can be moved aside.",
                file.display()
            );
            let _ = state::write(
                paths,
                &state::idle_with_error_keeping_capture(paths, &message),
            );
            util::notify("Reel", Some(&message), "critical");
            anyhow::bail!(message);
        }
    }
}

/// The auto zoom choices for this take, read from its settings snapshot before
/// the save pass touches state.
struct TakeOptions {
    style: &'static str,
    depth: f64,
}

fn take_options(paths: &Paths) -> TakeOptions {
    let settings = state::read(paths)
        .and_then(|state| state.get("settings").cloned())
        .unwrap_or(Value::Null);
    TakeOptions {
        style: config::auto_zoom_style(&settings),
        depth: config::auto_zoom_depth(&settings),
    }
}

/// Build the project for a take from its telemetry.
///
/// Every saved take with telemetry gets one, even when auto zoom is off: the
/// file records the capture facts, the trim, and the camera style so a later
/// `reel render` does not need live settings. The keyframe times are saved-take
/// time (`t - trimStart`, floored at 0).
fn project_from_telemetry(
    telemetry_file: &Path,
    input: &Path,
    output: &Path,
    trim_start: f64,
    options: &TakeOptions,
    target: &str,
    target_kind: &str,
) -> Result<project::Project> {
    let telemetry = project::read_telemetry(telemetry_file)?;
    let width = telemetry.width;
    let height = telemetry.height;
    if width == 0 || height == 0 {
        bail!(
            "the telemetry at {} has no capture size",
            telemetry_file.display()
        );
    }
    let trim = if trim_start.is_finite() {
        trim_start.max(0.0)
    } else {
        0.0
    };
    let captured = util::duration_of(input);
    let raw = if captured > 0.0 {
        captured
    } else {
        telemetry.stop_t
    };
    let duration = (raw - trim).max(0.0);
    let fps = if telemetry.fps.is_finite() && telemetry.fps > 0.0 {
        telemetry.fps
    } else {
        60.0
    };

    let mut project = project::Project::for_take(output, width, height, fps, duration);
    project.trim_start = trim;
    // The `start` record is the authoritative capture; the recorder's stop
    // arguments are the fallback for a log without one.
    if telemetry.capture.kind.is_empty() {
        project.capture = project::Capture {
            kind: target_kind.to_string(),
            target: target.to_string(),
            monitor: String::new(),
            extra: Default::default(),
        };
    } else {
        project.capture = telemetry.capture;
        if !target.is_empty() {
            project.capture.target = target.to_string();
        }
        if !target_kind.is_empty() {
            project.capture.kind = target_kind.to_string();
        }
    }
    project.telemetry = paths::telemetry_path(output)
        .file_name()
        .map(|name| name.to_string_lossy().to_string())
        .unwrap_or_default();
    project.camera = project::build_camera(
        &telemetry.clicks,
        options.style,
        options.depth,
        width as f64,
        height as f64,
        trim,
    );
    Ok(project)
}

/// Write `<take>.project.json`. A write failure is logged and never fails the
/// save: the take is already valid and a project is only useful for re-renders.
fn write_project(paths: &Paths, take: &Path, project: &project::Project) -> Option<PathBuf> {
    let path = project::project_path(take);
    match project::Project::save(&path, project) {
        Ok(()) => {
            paths::log(
                paths,
                &format!(
                    "finalize: project written to {} ({} keyframes)",
                    path.display(),
                    project.camera.keyframes.len()
                ),
            );
            Some(path)
        }
        Err(err) => {
            paths::log(
                paths,
                &format!("finalize: cannot write {}: {err}", path.display()),
            );
            None
        }
    }
}

/// Copy pointer telemetry beside a successfully saved take. A copy failure is
/// logged and never fails the save; the runtime file stays for debugging.
fn persist_telemetry(paths: &Paths, take: &Path, telemetry_ran: bool) -> bool {
    if !telemetry_ran {
        return false;
    }
    if !take.is_file() {
        paths::log(
            paths,
            &format!(
                "finalize: telemetry not copied because {} is missing",
                take.display()
            ),
        );
        return false;
    }
    match telemetry::persist(paths, take) {
        Ok(path) => {
            paths::log(
                paths,
                &format!("finalize: telemetry copied to {}", path.display()),
            );
            true
        }
        Err(err) => {
            paths::log(
                paths,
                &format!(
                    "finalize: telemetry copy failed for {}: {err}",
                    take.display()
                ),
            );
            false
        }
    }
}

/// The hidden `__finalize` process spawned by `stop`.
pub fn run_job(paths: &Paths, args: FinalizeArgs) -> Result<()> {
    // Keep the take's framing with its result so the panel can show the same
    // area after the recording has finished.
    let target = if args.target.is_empty() {
        Value::Null
    } else {
        Value::String(args.target.clone())
    };
    let target_kind = if args.target_kind.is_empty() {
        Value::Null
    } else {
        Value::String(args.target_kind.clone())
    };
    let target_label = if args.target_label.is_empty() {
        Value::Null
    } else {
        Value::String(args.target_label)
    };

    // Read the listener flag before touching state. The recording state is
    // still `finalizing` here and carries whether telemetry actually ran.
    let telemetry_ran = state::read(paths)
        .and_then(|state| state.get("telemetry").and_then(Value::as_bool))
        .unwrap_or(false);
    let options = take_options(paths);
    // `--telemetry` is explicit; otherwise use the take's runtime log when the
    // recorder actually started the listener. A missing file is not an error:
    // the take simply saves without a camera.
    let telemetry_file = args
        .telemetry
        .clone()
        .filter(|path| path.is_file())
        .or_else(|| {
            (telemetry_ran && paths.session_telemetry.is_file())
                .then(|| paths.session_telemetry.clone())
        });

    let mut file = args.file;
    let mut output = args.output.unwrap_or_else(|| file.clone());
    let original_file = file.display().to_string();
    let mut quarantined = output != file;

    // Defense in depth for a state file written before `output` existed: a
    // countdown capture must never be finalized in place. `stop` normally
    // quarantines it first; if an older or malformed caller did not, move it
    // aside here rather than risk leaving pre-roll footage under the ordinary
    // take name after a failed trim.
    quarantine_before_save(
        paths,
        &mut file,
        &mut output,
        args.trim_start,
        &mut quarantined,
    )?;

    // Publish the job pid before touching ffmpeg. The service can then tell a
    // live save from one that died and left the bar on "Saving".
    if let Some(current) = state::read(paths) {
        let status = state::field_str(&current, "status").unwrap_or_default();
        let current_file = state::field_str(&current, "file").unwrap_or_default();
        if status == "finalizing"
            && (current_file == original_file || current_file == file.display().to_string())
        {
            let mut next = current;
            if quarantined {
                state::set(&mut next, "file", json!(file.display().to_string()));
                state::set(&mut next, "output", json!(output.display().to_string()));
            }
            state::set(&mut next, "finalizePid", json!(std::process::id()));
            let _ = state::write(paths, &next);
        }
    }

    // Render the thumbnail before the save pass. It comes from the raw capture
    // either way, and generating it first means history keeps an image even if
    // the save is interrupted. An existing
    // cache file from an earlier pass is reused.
    let mut preview = args.preview;
    if preview.is_empty() || !Path::new(&preview).is_file() {
        let generated = paths.preview_path(&output);
        if make_preview(&file, &generated, args.trim_start + 0.2) {
            preview = generated.display().to_string();
        } else {
            preview.clear();
        }
    }

    let mut error = String::new();
    let mut saved = output.clone();
    let mut built_project: Option<project::Project> = None;
    let pass = match args.project.as_ref() {
        Some(project_file) => match project::Project::load(project_file) {
            Ok(mut project) => {
                // A project handed to the save pass can be a skeleton: fill its
                // camera from `--telemetry` when it has none of its own.
                if let Some(telemetry_file) = telemetry_file.as_deref() {
                    if project.camera.keyframes.is_empty() && project.camera.style != "off" {
                        match project_from_telemetry(
                            telemetry_file,
                            &file,
                            &output,
                            project.trim_start,
                            &options,
                            &args.target,
                            &args.target_kind,
                        ) {
                            Ok(built) => project.camera = built.camera,
                            Err(err) => paths::log(
                                paths,
                                &format!("finalize: telemetry not applied: {err:#}"),
                            ),
                        }
                    }
                }
                render::render_take(
                    paths,
                    &project,
                    &file,
                    &output,
                    project.trim_start,
                    args.noise_cancellation,
                )
            }
            Err(err) => Err(err),
        },
        None => match telemetry_file.as_deref() {
            Some(telemetry_file) => match project_from_telemetry(
                telemetry_file,
                &file,
                &output,
                args.trim_start,
                &options,
                &args.target,
                &args.target_kind,
            ) {
                Ok(project) => {
                    paths::log(
                        paths,
                        &format!(
                            "finalize: auto zoom style={} depth={} keyframes={}",
                            project.camera.style,
                            project.camera.depth,
                            project.camera.keyframes.len()
                        ),
                    );
                    let rendered = render::render_take(
                        paths,
                        &project,
                        &file,
                        &output,
                        project.trim_start,
                        args.noise_cancellation,
                    );
                    if rendered.is_ok() {
                        built_project = Some(project);
                    }
                    rendered
                }
                Err(err) => {
                    // Missing or unusable telemetry must never cost the take.
                    paths::log(
                        paths,
                        &format!("finalize: telemetry skipped, saving without a zoom: {err:#}"),
                    );
                    finalize_video(
                        paths,
                        &file,
                        args.trim_start,
                        &output,
                        args.noise_cancellation,
                    )
                }
            },
            None => finalize_video(
                paths,
                &file,
                args.trim_start,
                &output,
                args.noise_cancellation,
            ),
        },
    };
    if let Err(err) = &pass {
        paths::log(paths, &format!("finalize: save pass: {err:#}"));
    }
    let mut telemetry_saved = false;
    if pass.is_ok() {
        // The trimmed take now owns the normal name. The raw countdown capture
        // must not linger as a second, shippable copy.
        if quarantined {
            paths::remove_quiet(&file);
        }
        // The work file has been validated and renamed into place. An explicit
        // `--telemetry` file that is the runtime log is moved beside the take
        // even when state no longer carries the listener flag.
        let telemetry_recorded =
            telemetry_ran || telemetry_file.as_deref() == Some(paths.session_telemetry.as_path());
        telemetry_saved = persist_telemetry(paths, &output, telemetry_recorded);
        // The project is written last: it points at the take and at the
        // telemetry file that was just copied, and a failed save must not
        // leave a project describing a take that was never produced.
        if let Some(project) = &built_project {
            write_project(paths, &output, project);
        }
    } else if quarantined {
        // The raw capture is already named `-untrimmed`, and the recent list
        // skips that suffix. Point the user at it instead of offering it as a
        // normal take.
        saved = file.clone();
        error = format!(
            "The countdown could not be trimmed. Untrimmed footage is at {}.",
            file.display()
        );
    } else {
        error = "The take was saved, but the final pass failed.".into();
    }

    let duration = util::duration_of(&saved);
    let size = fs::metadata(&saved).map(|m| m.len()).unwrap_or(0);

    let last = json!({
        "path": saved.display().to_string(),
        "preview": preview,
        "target": target,
        "targetKind": target_kind,
        "targetLabel": target_label,
        "duration": duration,
        "size": size,
        "telemetry": telemetry_saved,
        "untrimmed": quarantined && !error.is_empty(),
        "startedAt": args.started_at,
        "finishedAt": paths::now_secs(),
        "error": error,
    });
    paths::write_atomic(&paths.last, &serde_json::to_string(&last)?)?;

    if let Some(current) = state::read(paths) {
        let status = state::field_str(&current, "status").unwrap_or_default();
        let current_file = state::field_str(&current, "file").unwrap_or_default();
        let owns_state = (status == "finalizing" && current_file == file.display().to_string())
            || (status == "idle" && current_file.is_empty());
        if owns_state {
            let next = state::idle_with_last(&last);
            let _ = state::write(paths, &next);
        }
    }

    if !error.is_empty() {
        util::notify("Reel", Some(&error), "critical");
        anyhow::bail!(error);
    }

    let seconds = duration.round() as i64;
    let minutes = format!("{:02}:{:02}", seconds / 60, seconds % 60);
    paths::log(
        paths,
        &format!(
            "finalize: saved {} ({}s, {} bytes)",
            file.display(),
            seconds,
            size
        ),
    );

    if util::notifications_on(paths) && util::have("omarchy-notification-send") {
        let body = minutes.clone();
        let mut cmd = Command::new("omarchy-notification-send");
        cmd.arg("Recording saved").arg(body).arg("-t").arg("10000");
        if !preview.is_empty() {
            cmd.arg("--image").arg(&preview);
        }
        cmd.arg("--exec").arg("mpv").arg("--").arg(&file);
        let _ = cmd
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_denied_quarantine_stops_the_save_and_keeps_the_capture() {
        // SAFETY: geteuid has no memory effects and cannot fail.
        if unsafe { libc::geteuid() } == 0 {
            return;
        }
        use std::os::unix::fs::PermissionsExt;

        let root = std::env::temp_dir().join(format!(
            "reel-finalize-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        let videos = root.join("Videos");
        fs::create_dir_all(&videos).unwrap();
        let paths = Paths::for_test(&root);
        let file = videos.join("reel-legacy.mp4");
        fs::write(&file, b"raw").unwrap();
        state::write(
            &paths,
            &json!({
                "status": "finalizing",
                "file": file.display().to_string(),
                "trimStart": 3.0,
            }),
        )
        .unwrap();
        fs::set_permissions(&videos, fs::Permissions::from_mode(0o555)).unwrap();

        let mut capture = file.clone();
        let mut output = PathBuf::new();
        let mut quarantined = false;
        let err = quarantine_before_save(&paths, &mut capture, &mut output, 3.0, &mut quarantined)
            .unwrap_err();

        assert!(err.to_string().contains("quarantined"));
        assert!(!quarantined);
        assert!(file.is_file());
        assert!(crate::quarantine::is_blocked(&paths, &file));
        let state = state::read(&paths).unwrap();
        assert_eq!(state::field_str(&state, "status").as_deref(), Some("idle"));
        assert_eq!(
            state::field_str(&state, "file").as_deref(),
            Some(file.display().to_string().as_str())
        );
        assert!(state::field_str(&state, "error")
            .unwrap()
            .contains("withheld"));

        fs::set_permissions(&videos, fs::Permissions::from_mode(0o755)).unwrap();
        let _ = fs::remove_dir_all(root);
    }

    fn temp_root(label: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "reel-finalize-{label}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ))
    }

    fn telemetry_fixture() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/telemetry-basic.jsonl")
    }

    #[test]
    fn a_telemetry_take_builds_a_project_in_saved_take_time() {
        let root = temp_root("project");
        let videos = root.join("Videos");
        fs::create_dir_all(&videos).unwrap();
        let take = videos.join("reel-2.mp4");
        fs::write(&take, b"video").unwrap();

        let options = TakeOptions {
            style: "cinematic",
            depth: 2.0,
        };
        let project = project_from_telemetry(
            &telemetry_fixture(),
            &take,
            &take,
            // One second of countdown was trimmed off the front.
            1.0,
            &options,
            "monitor:DP-1",
            "monitor",
        )
        .unwrap();

        assert_eq!(project.take, "reel-2.mp4");
        assert_eq!(project.source.width, 2560);
        assert_eq!(project.source.height, 1440);
        assert_eq!(project.trim_start, 1.0);
        assert_eq!(project.telemetry, "reel-2.telemetry.jsonl");
        assert_eq!(project.capture.kind, "monitor");
        assert_eq!(project.capture.target, "monitor:DP-1");
        assert_eq!(project.camera.style, "cinematic");
        assert_eq!(project.camera.depth, 2.0);

        // The click at t=1.5 lands at 0.5 in the saved take, and the 2x
        // viewport is clamped inside the 2560x1440 capture.
        let peak = project
            .camera
            .keyframes
            .iter()
            .find(|frame| frame.zoom > 1.9)
            .expect("a zoomed keyframe");
        assert!((peak.t - 0.5).abs() < 1e-6);
        assert!((peak.x - 640.0).abs() < 1e-6);
        assert!((peak.y - 360.0).abs() < 1e-6);

        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn auto_zoom_off_still_records_the_project_without_keyframes() {
        let root = temp_root("project-off");
        let videos = root.join("Videos");
        fs::create_dir_all(&videos).unwrap();
        let take = videos.join("reel-3.mp4");
        fs::write(&take, b"video").unwrap();

        let project = project_from_telemetry(
            &telemetry_fixture(),
            &take,
            &take,
            0.0,
            &TakeOptions {
                style: "off",
                depth: 2.0,
            },
            "",
            "",
        )
        .unwrap();

        assert_eq!(project.camera.style, "off");
        assert!(project.camera.keyframes.is_empty());
        assert!(!project.camera.needs_zoom());
        // The project still carries the capture facts for a later re-render.
        assert_eq!(project.source.width, 2560);
        assert_eq!(project.capture.monitor, "DP-1");

        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn telemetry_without_a_capture_size_has_no_project() {
        let root = temp_root("project-nosize");
        fs::create_dir_all(&root).unwrap();
        let telemetry = root.join("take.telemetry.jsonl");
        fs::write(
            &telemetry,
            b"{\"v\":1,\"type\":\"button\",\"t\":1.0,\"state\":\"down\",\"x\":5.0,\"y\":6.0}\n",
        )
        .unwrap();
        let take = root.join("reel-4.mp4");
        fs::write(&take, b"video").unwrap();

        let err = project_from_telemetry(
            &telemetry,
            &take,
            &take,
            0.0,
            &TakeOptions {
                style: "cinematic",
                depth: 2.0,
            },
            "",
            "",
        )
        .unwrap_err();
        assert!(err.to_string().contains("capture size"));
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn the_project_file_is_written_next_to_the_take() {
        let root = temp_root("project-write");
        let videos = root.join("Videos");
        fs::create_dir_all(&videos).unwrap();
        let paths = Paths::for_test(&root);
        let take = videos.join("reel-5.mp4");
        fs::write(&take, b"video").unwrap();

        let project = project_from_telemetry(
            &telemetry_fixture(),
            &take,
            &take,
            0.0,
            &TakeOptions {
                style: "subtle",
                depth: 1.5,
            },
            "",
            "",
        )
        .unwrap();
        let written = write_project(&paths, &take, &project).expect("a project path");
        assert_eq!(written, videos.join("reel-5.project.json"));

        let reloaded = project::Project::load(&written).unwrap();
        assert_eq!(reloaded.take, "reel-5.mp4");
        assert_eq!(reloaded.camera.style, "subtle");
        assert_eq!(reloaded.camera.depth, 1.5);
        assert!(reloaded.camera.needs_zoom());
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn telemetry_is_copied_for_a_successful_save() {
        let root = temp_root("telemetry-copy");
        let videos = root.join("Videos");
        fs::create_dir_all(&videos).unwrap();
        let paths = Paths::for_test(&root);
        let take = videos.join("reel-1.mp4");
        fs::write(&take, b"video").unwrap();
        fs::write(&paths.session_telemetry, b"{}\n").unwrap();

        assert!(persist_telemetry(&paths, &take, true));
        assert!(paths::telemetry_path(&take).is_file());
        assert!(
            paths.session_telemetry.is_file(),
            "the runtime copy stays for debugging"
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn telemetry_is_not_copied_when_it_did_not_run() {
        let root = temp_root("telemetry-off");
        let videos = root.join("Videos");
        fs::create_dir_all(&videos).unwrap();
        let paths = Paths::for_test(&root);
        let take = videos.join("reel-1.mp4");
        fs::write(&take, b"video").unwrap();
        fs::write(&paths.session_telemetry, b"{}\n").unwrap();

        assert!(!persist_telemetry(&paths, &take, false));
        assert!(!paths::telemetry_path(&take).exists());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn a_missing_runtime_file_never_fails_the_save() {
        let root = temp_root("telemetry-missing");
        let videos = root.join("Videos");
        fs::create_dir_all(&videos).unwrap();
        let paths = Paths::for_test(&root);
        let take = videos.join("reel-1.mp4");
        fs::write(&take, b"video").unwrap();

        assert!(!persist_telemetry(&paths, &take, true));
        assert!(!paths::telemetry_path(&take).exists());
        assert!(take.is_file(), "the take itself is untouched");
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn audio_filter_only_denoises_when_requested() {
        let plain = audio_filter(false);
        assert!(plain.contains("loudnorm"));
        assert!(!plain.contains("afftdn"));

        let denoised = audio_filter(true);
        assert!(denoised.contains("afftdn=nf=-30"));
        assert!(denoised.contains("loudnorm"));
        assert!(denoised.find("afftdn").unwrap() < denoised.find("loudnorm").unwrap());
    }
}
