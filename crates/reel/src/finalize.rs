use crate::cli::FinalizeArgs;
use crate::paths::{self, Paths};
use crate::state;
use crate::util;
use anyhow::{Context, Result};
use serde_json::{json, Value};
use std::fs::{self, OpenOptions};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// `off`, `0.5x`, `0.25x`.
pub fn label(value: &str) -> &'static str {
    match factor(value) {
        0.5 => "0.5x",
        0.25 => "0.25x",
        _ => "off",
    }
}

/// `1`, `0.5`, `0.25` — the number used by ffmpeg.
pub fn factor_str(value: &str) -> &'static str {
    match factor(value) {
        0.5 => "0.5",
        0.25 => "0.25",
        _ => "1",
    }
}

pub fn factor(value: &str) -> f64 {
    match value.to_ascii_lowercase().as_str() {
        "0.5" | "0.5x" | "half" => 0.5,
        "0.25" | "0.25x" | "quarter" => 0.25,
        _ => 1.0,
    }
}

fn atempo(value: &str) -> Option<&'static str> {
    match factor(value) {
        0.5 => Some("atempo=0.5"),
        0.25 => Some("atempo=0.5,atempo=0.5"),
        _ => None,
    }
}

pub fn make_preview(file: &Path, preview: &Path) -> bool {
    if !file.is_file() {
        return false;
    }
    let status = Command::new("ffmpeg")
        .args(["-y", "-ss", "0.2", "-i"])
        .arg(file)
        .args(["-frames:v", "1", "-q:v", "3"])
        .arg(preview)
        .args(["-loglevel", "error"])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
    status.map(|s| s.success()).unwrap_or(false)
}

fn first_gop_has_discard(file: &Path) -> bool {
    util::run_stdout(
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
    )
    .map(|out| out.contains('D'))
    .unwrap_or(false)
}

fn work_path(file: &Path, output: Option<&Path>) -> PathBuf {
    let ext = file
        .extension()
        .map(|e| e.to_string_lossy().to_string())
        .unwrap_or_default();
    if let Some(output) = output {
        let stem = output.with_extension("").to_string_lossy().to_string();
        PathBuf::from(format!("{stem}.part.{}.{ext}", std::process::id()))
    } else {
        let stem = file.with_extension("").to_string_lossy().to_string();
        PathBuf::from(format!("{stem}-processed.{ext}"))
    }
}

fn run_ffmpeg(paths: &Paths, args: &[String]) -> bool {
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
    Command::new("ffmpeg")
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::from(log))
        .stderr(Stdio::from(err))
        .status()
        .map(|status| status.success())
        .unwrap_or(false)
}

/// A finished file must have a video stream and a real duration. Checking the
/// work file before the rename means a broken MP4 can never replace the raw
/// take that the recorder actually wrote.
fn valid_video(file: &Path) -> bool {
    if !file.is_file() || fs::metadata(file).map(|m| m.len()).unwrap_or(0) == 0 {
        return false;
    }
    let video = util::run_stdout(
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
    )
    .map(|out| out.contains("video"))
    .unwrap_or(false);
    video && util::duration_of(file) > 0.0
}

/// Trim the first frame, mute the PipeWire pop, normalize the audio, and either
/// stream-copy the video or re-encode it. With an output path the source is
/// left untouched.
pub fn finalize_video(
    paths: &Paths,
    file: &Path,
    slowmo: &str,
    output: Option<&Path>,
) -> Result<()> {
    if !file.is_file() {
        anyhow::bail!("recording is missing");
    }
    let ext = file
        .extension()
        .map(|e| e.to_string_lossy().to_string())
        .unwrap_or_default();
    let work = work_path(file, output);

    let factor = factor(slowmo);
    let mut needs_video_reencode = false;
    let mut video_filter = String::new();
    if factor != 1.0 {
        needs_video_reencode = true;
        video_filter = format!("setpts={:.4}*PTS", 1.0 / factor);
    } else if first_gop_has_discard(file) {
        needs_video_reencode = true;
    }

    let mut audio_present = util::has_audio_stream(file);
    let src_duration = util::duration_of(file);
    let mut trim = "0.1";
    if src_duration > 0.0 && src_duration < 0.8 {
        paths::log(
            paths,
            &format!("finalize: short take ({src_duration}s), dropping audio"),
        );
        audio_present = false;
        if src_duration < 0.4 {
            trim = "0";
        }
    }

    let mut audio_filter = String::from(
        "volume=enable='lt(t,0.4)':volume=0,afade=t=in:st=0.4:d=0.05,loudnorm=I=-14:TP=-1.5:LRA=11",
    );
    if let Some(atempo) = atempo(slowmo) {
        audio_filter.push(',');
        audio_filter.push_str(atempo);
    }

    let attempt = |use_audio: bool| -> bool {
        let mut args: Vec<String> = vec![
            "-y".into(),
            "-ss".into(),
            trim.into(),
            "-i".into(),
            file.display().to_string(),
        ];
        if needs_video_reencode {
            if !video_filter.is_empty() {
                args.extend([
                    "-vf".into(),
                    video_filter.clone(),
                    "-fps_mode".into(),
                    "vfr".into(),
                ]);
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
        let destination = output.unwrap_or(file);
        fs::rename(&work, destination).with_context(|| {
            format!("cannot move the finished take to {}", destination.display())
        })?;
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

/// The hidden `__finalize` process spawned by `stop`.
pub fn run_job(paths: &Paths, args: FinalizeArgs) -> Result<()> {
    // Keep the take's framing with its result so the panel can show the same
    // area after the recording has finished.
    let target = if args.target.is_empty() {
        Value::Null
    } else {
        Value::String(args.target)
    };
    let target_kind = if args.target_kind.is_empty() {
        Value::Null
    } else {
        Value::String(args.target_kind)
    };
    let target_label = if args.target_label.is_empty() {
        Value::Null
    } else {
        Value::String(args.target_label)
    };

    let file = args.file;
    let mut error = String::new();
    if let Err(err) = finalize_video(paths, &file, &args.slowmo, None) {
        let _ = err;
        error = "The take was saved, but the final pass failed.".into();
    }

    let duration = util::duration_of(&file);
    let size = fs::metadata(&file).map(|m| m.len()).unwrap_or(0);

    let mut preview = args.preview;
    if preview.is_empty() || !Path::new(&preview).is_file() {
        let generated = PathBuf::from(format!("{}-preview.jpg", file.with_extension("").display()));
        if make_preview(&file, &generated) {
            preview = generated.display().to_string();
        } else {
            preview.clear();
        }
    }

    let last = json!({
        "path": file.display().to_string(),
        "preview": preview,
        "target": target,
        "targetKind": target_kind,
        "targetLabel": target_label,
        "duration": duration,
        "size": size,
        "startedAt": args.started_at,
        "finishedAt": paths::now_secs(),
        "slowmo": label(&args.slowmo),
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
