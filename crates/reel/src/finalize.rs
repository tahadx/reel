use crate::cli::FinalizeArgs;
use crate::paths::{self, Paths};
use crate::state;
use crate::util;
use anyhow::{Context, Result};
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

    let mut file = args.file;
    let mut output = args.output.unwrap_or_else(|| file.clone());
    let original_file = file.display().to_string();
    let mut quarantined = output != file;

    // Defense in depth for a state file written before `output` existed: a
    // countdown capture must never be finalized in place. `stop` normally
    // quarantines it first; if an older or malformed caller did not, move it
    // aside here rather than risk leaving pre-roll footage under the ordinary
    // take name after a failed trim.
    if !quarantined && args.trim_start > 0.0 && !paths::is_untrimmed(&file) {
        match paths::quarantine_capture(&file) {
            Ok(capture) => {
                output = file;
                file = capture;
                quarantined = true;
            }
            Err(err) => {
                let message = format!(
                    "The countdown capture could not be quarantined before saving: {err}. \
                     The original file was left untouched at {}.",
                    file.display()
                );
                let _ = state::write(
                    paths,
                    &state::idle_with_error_preserving_last(paths, &message),
                );
                util::notify("Reel", Some(&message), "critical");
                anyhow::bail!(message);
            }
        }
    }

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
    let pass = match args.project.as_ref() {
        Some(project_file) => match crate::project::Project::load(project_file) {
            Ok(project) => {
                if let Some(telemetry) = args.telemetry.as_ref() {
                    // The telemetry pass owns reading this file. Until it is
                    // merged, the project camera is the only render input.
                    paths::log(
                        paths,
                        &format!(
                            "finalize: telemetry {} is not read in this build; using the project camera",
                            telemetry.display()
                        ),
                    );
                }
                crate::render::render_take(
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
        None => finalize_video(
            paths,
            &file,
            args.trim_start,
            &output,
            args.noise_cancellation,
        ),
    };
    if let Err(err) = &pass {
        paths::log(paths, &format!("finalize: save pass: {err:#}"));
    }
    if pass.is_ok() {
        // The trimmed take now owns the normal name. The raw countdown capture
        // must not linger as a second, shippable copy.
        if quarantined {
            paths::remove_quiet(&file);
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
