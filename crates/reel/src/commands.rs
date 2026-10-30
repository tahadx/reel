use crate::config;
use crate::finalize;
use crate::paths::{self, Paths};
use crate::state;
use crate::util;
use anyhow::{Context, Result};
use serde_json::json;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::UNIX_EPOCH;

pub fn status(paths: &Paths, as_json: bool) -> Result<()> {
    if as_json {
        println!("{}", serde_json::to_string(&state::status_json(paths))?);
        return Ok(());
    }

    state::reconcile(paths);
    let status = state::status(paths);
    match status.as_str() {
        "recording" => println!("Recording {}", util::format_duration(state::elapsed(paths))),
        "paused" => println!("Paused {}", util::format_duration(state::elapsed(paths))),
        "finalizing" => println!("Saving"),
        "starting" => println!("Starting"),
        _ => println!("Idle"),
    }
    Ok(())
}

struct Take {
    path: PathBuf,
    preview: String,
    duration: f64,
    size: u64,
    mtime: i64,
}

/// The cached preview for a take, adopting a preview written next to the
/// recording by an older Reel version when possible.
fn take_preview(paths: &Paths, file: &Path) -> Option<PathBuf> {
    let cached = paths.preview_path(file);
    if cached.is_file() {
        return Some(cached);
    }

    let legacy = paths::legacy_preview_path(file);
    if !legacy.is_file() {
        return None;
    }
    if fs::rename(&legacy, &cached).is_ok() {
        return Some(cached);
    }
    if fs::copy(&legacy, &cached).is_ok() {
        paths::remove_quiet(&legacy);
        return Some(cached);
    }
    Some(legacy)
}

fn recent_takes(paths: &Paths, limit: usize) -> Vec<Take> {
    let dir = config::output_dir(paths);
    let Ok(entries) = fs::read_dir(&dir) else {
        return Vec::new();
    };

    let mut files: Vec<(i64, PathBuf)> = entries
        .flatten()
        .filter_map(|entry| {
            let path = entry.path();
            if !path.is_file() {
                return None;
            }
            let ext = path.extension()?.to_string_lossy().to_ascii_lowercase();
            if ext != "mp4" && ext != "mkv" {
                return None;
            }
            let modified = entry
                .metadata()
                .ok()
                .and_then(|meta| meta.modified().ok())
                .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
                .map(|duration| duration.as_secs() as i64)
                .unwrap_or(0);
            Some((modified, path))
        })
        .collect();

    files.sort_by_key(|file| std::cmp::Reverse(file.0));
    files.truncate(limit);

    files
        .into_iter()
        .map(|(mtime, path)| {
            let preview = take_preview(paths, &path)
                .map(|path| path.display().to_string())
                .unwrap_or_default();
            let size = fs::metadata(&path).map(|meta| meta.len()).unwrap_or(0);
            Take {
                duration: util::duration_of(&path),
                preview,
                size,
                mtime,
                path,
            }
        })
        .collect()
}

pub fn recent(paths: &Paths, as_json: bool, limit: usize) -> Result<()> {
    let takes = recent_takes(paths, limit);
    if as_json {
        let recordings: Vec<_> = takes
            .iter()
            .map(|take| {
                json!({
                    "path": take.path.display().to_string(),
                    "preview": take.preview,
                    "duration": take.duration,
                    "size": take.size,
                    "mtime": take.mtime,
                })
            })
            .collect();
        println!(
            "{}",
            serde_json::to_string(&json!({ "recordings": recordings }))?
        );
    } else if takes.is_empty() {
        println!("No recordings yet.");
    } else {
        for take in &takes {
            println!("{}\t{}", take.path.display(), take.duration);
        }
    }
    Ok(())
}

pub fn slowmo(paths: &Paths, file: PathBuf, factor: &str) -> Result<()> {
    if !file.is_file() {
        anyhow::bail!("Usage: reel slowmo <recording> [0.5|0.25]");
    }
    let value = finalize::factor(factor);
    if value == 1.0 {
        anyhow::bail!("Slow motion needs 0.5 or 0.25.");
    }
    let ext = file
        .extension()
        .map(|ext| ext.to_string_lossy().to_string())
        .unwrap_or_default();
    let stem = file.with_extension("").display().to_string();
    let factor = finalize::factor_str(factor);
    let output = PathBuf::from(format!("{stem}-slowmo-{factor}x.{ext}"));
    let preview = paths.preview_path(&output);

    finalize::finalize_video(paths, &file, factor, Some(&output))
        .map_err(|_| anyhow::anyhow!("The slow-motion copy could not be written."))?;
    let _ = finalize::make_preview(&output, &preview);

    if util::notifications_on(paths) && util::have("omarchy-notification-send") {
        let mut cmd = Command::new("omarchy-notification-send");
        cmd.arg("Slow motion saved")
            .arg(util::file_stem_name(&output))
            .arg("-t")
            .arg("8000");
        if preview.is_file() {
            cmd.arg("--image").arg(&preview);
        }
        cmd.arg("--exec").arg("mpv").arg("--").arg(&output);
        let _ = cmd
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    }

    println!("{}", output.display());
    Ok(())
}

pub fn trash(paths: &Paths, file: PathBuf) -> Result<()> {
    if !file.is_file() {
        anyhow::bail!("Usage: reel trash <recording>");
    }
    let dir = config::output_dir(paths);
    let real_dir = fs::canonicalize(&dir).unwrap_or(dir.clone());
    let real_file = fs::canonicalize(&file)?;
    if !real_file.starts_with(&real_dir) {
        anyhow::bail!("Refusing to delete a file outside the recordings folder.");
    }

    let preview = paths.preview_path(&file);
    let legacy_preview = paths::legacy_preview_path(&file);
    let trashed = util::have("gio")
        && Command::new("gio")
            .arg("trash")
            .arg("--")
            .arg(&file)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map(|status| status.success())
            .unwrap_or(false);
    if !trashed {
        fs::remove_file(&file).with_context(|| format!("cannot remove {}", file.display()))?;
    }
    paths::remove_quiet(&preview);
    paths::remove_quiet(&legacy_preview);
    Ok(())
}

/// A still of the focused monitor. The take itself is framed later in the
/// picker, so this is only a rough look at the screen before recording.
pub fn preview(paths: &Paths, as_json: bool, note_only: bool) -> Result<()> {
    let name = crate::target::focused_monitor()
        .ok_or_else(|| anyhow::anyhow!("Reel could not read the monitor layout from Hyprland."))?;
    let target = format!("monitor:{name}");
    let (x, y, width, height) = crate::target::geometry(&target)
        .ok_or_else(|| anyhow::anyhow!("Reel could not read the target geometry."))?;
    if width <= 0 || height <= 0 {
        anyhow::bail!("The next-take preview has no area to capture.");
    }
    let note = "Pick a window, area, or monitor at record time";

    if note_only {
        if as_json {
            println!(
                "{}",
                serde_json::to_string(&json!({
                    "path": "",
                    "target": target,
                    "width": width,
                    "height": height,
                    "note": note,
                }))?
            );
        } else {
            println!("{note}");
        }
        return Ok(());
    }

    if !util::have("grim") {
        anyhow::bail!("grim is missing, so Reel cannot draw the next-take preview.");
    }

    let millis = paths::now_millis();
    let destination = paths.runtime.join(format!("next-preview-{millis}.jpg"));
    if let Ok(entries) = fs::read_dir(&paths.runtime) {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            if name.starts_with("next-preview-")
                && name.ends_with(".jpg")
                && entry.path() != destination
            {
                let _ = fs::remove_file(entry.path());
            }
        }
    }

    let scale = if width > 720 {
        720.0 / width as f64
    } else {
        1.0
    };
    let region = format!("{x},{y} {width}x{height}");
    let captured = Command::new("grim")
        .arg("-g")
        .arg(&region)
        .arg("-s")
        .arg(format!("{scale:.4}"))
        .arg("-t")
        .arg("jpeg")
        .arg("-q")
        .arg("85")
        .arg(&destination)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|status| status.success())
        .unwrap_or(false);
    if !captured || !destination.is_file() {
        anyhow::bail!("The next-take preview could not be captured.");
    }

    if as_json {
        println!(
            "{}",
            serde_json::to_string(&json!({
                "path": destination.display().to_string(),
                "target": target,
                "width": width,
                "height": height,
                "note": note,
            }))?
        );
    } else {
        println!("{}", destination.display());
    }
    Ok(())
}

pub fn output(paths: &Paths) {
    println!("{}", config::output_dir(paths).display());
}

pub fn doctor(paths: &Paths) {
    println!("reel state: {}", paths.state.display());
    println!("reel output: {}", config::output_dir(paths).display());
    for tool in [
        "gpu-screen-recorder",
        "ffmpeg",
        "ffprobe",
        "hyprctl",
        "mpv",
        "slurp",
        "grim",
        "hyprpicker",
        "jq",
        "v4l2-ctl",
        "omarchy-capture-region",
        "omarchy-osd",
        "omarchy-notification-send",
    ] {
        if util::have(tool) {
            println!("  ok      {tool}");
        } else {
            println!("  missing {tool}");
        }
    }
    if util::have("gpu-screen-recorder") {
        if let Some(version) = util::run_stdout("gpu-screen-recorder", &["--version"]) {
            if let Some(first) = version.lines().next() {
                println!("  version {first}");
            }
        }
        if let Some(monitors) = util::run_stdout("gpu-screen-recorder", &["--list-monitors"]) {
            for line in monitors.lines().filter(|line| !line.trim().is_empty()) {
                println!("  monitor {line}");
            }
        }
        if let Some(audio) = util::run_stdout("gpu-screen-recorder", &["--list-audio-devices"]) {
            for line in audio
                .lines()
                .filter(|line| line.starts_with("default_"))
                .take(4)
            {
                println!("  audio   {line}");
            }
        }
    }
}

pub fn is_manual_zoom_active(paths: &Paths) -> bool {
    state::reconcile(paths);
    let Some(st) = state::read(paths) else {
        return false;
    };
    is_manual_zoom_active_in_state(&st)
}

pub fn is_manual_zoom_active_in_state(st: &serde_json::Value) -> bool {
    let status = state::field_str(st, "status").unwrap_or_default();
    if status != "recording" {
        return false;
    }
    if let Some(settings) = st.get("settings") {
        if let Some(mz) = settings.get("manualZoom") {
            if let Some(b) = mz.as_bool() {
                return b;
            }
            if let Some(s) = mz.as_str() {
                return s == "on" || s == "true" || s == "1";
            }
        }
    }
    false
}

pub fn zoom_in(paths: &Paths, step: f64) -> Result<()> {
    if !is_manual_zoom_active(paths) {
        return Ok(());
    }
    let current = crate::zoom::compositor_zoom();
    let step = if step > 0.01 { step } else { 0.25 };
    let target = (current + step).min(4.0);
    let immediate = step <= 0.16;
    let new_z = crate::zoom::animate_compositor_zoom(target, immediate);
    println!("Reel zoom: {:.2}x", new_z);
    Ok(())
}

pub fn zoom_out(paths: &Paths, step: f64) -> Result<()> {
    if !is_manual_zoom_active(paths) {
        return Ok(());
    }
    let current = crate::zoom::compositor_zoom();
    let step = if step > 0.01 { step } else { 0.25 };
    let target = (current - step).max(1.0);
    let immediate = step <= 0.16;
    let new_z = crate::zoom::animate_compositor_zoom(target, immediate);
    println!("Reel zoom: {:.2}x", new_z);
    Ok(())
}

pub fn zoom_reset(paths: &Paths) -> Result<()> {
    if !is_manual_zoom_active(paths) {
        return Ok(());
    }
    crate::zoom::reset_compositor_zoom();
    println!("Reel zoom: 1.00x");
    Ok(())
}

pub fn zoom_toggle(paths: &Paths, factor: f64) -> Result<()> {
    if !is_manual_zoom_active(paths) {
        return Ok(());
    }
    let current = crate::zoom::compositor_zoom();
    let factor = if factor > 1.05 { factor } else { 2.0 };
    let target = if current > 1.05 { 1.0 } else { factor };
    let new_z = crate::zoom::animate_compositor_zoom(target, false);
    println!("Reel zoom: {:.2}x", new_z);
    Ok(())
}

pub fn zoom_status(_paths: &Paths, as_json: bool) -> Result<()> {
    let current = crate::zoom::compositor_zoom();
    if as_json {
        println!(
            "{}",
            serde_json::json!({ "zoom": current, "active": current > 1.01 })
        );
    } else {
        println!("Reel zoom: {:.2}x", current);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manual_zoom_active_only_when_recording() {
        // Idle state -> inactive
        let idle_state = serde_json::json!({
            "status": "idle",
            "settings": { "manualZoom": true }
        });
        assert!(!is_manual_zoom_active_in_state(&idle_state));

        // Paused state -> inactive (zoom resets on pause)
        let paused_state = serde_json::json!({
            "status": "paused",
            "settings": { "manualZoom": true }
        });
        assert!(!is_manual_zoom_active_in_state(&paused_state));

        // Recording with manualZoom = false -> inactive
        let rec_off_state = serde_json::json!({
            "status": "recording",
            "settings": { "manualZoom": false }
        });
        assert!(!is_manual_zoom_active_in_state(&rec_off_state));

        // Recording with manualZoom = true -> active
        let rec_on_state = serde_json::json!({
            "status": "recording",
            "settings": { "manualZoom": true }
        });
        assert!(is_manual_zoom_active_in_state(&rec_on_state));

        // Recording with manualZoom = "on" -> active
        let rec_str_state = serde_json::json!({
            "status": "recording",
            "settings": { "manualZoom": "on" }
        });
        assert!(is_manual_zoom_active_in_state(&rec_str_state));
    }
}
