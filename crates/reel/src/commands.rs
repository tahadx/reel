use crate::config;
use crate::paths::{self, Paths};
use crate::state;
use crate::util;
use crate::webcam;
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::json;
use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, UNIX_EPOCH};

const PLUGIN_ID: &str = "io.github.tahadx.reel";

/// How long a checked result is trusted. The shell asks once an hour; a restart
/// inside that window reads the cache instead of fetching again.
const UPDATE_CHECK_TTL: i64 = 3600;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct UpdateReport {
    current: String,
    latest: String,
    available: bool,
    behind: u64,
    checked_at: i64,
    error: String,
}

fn installed_plugin_dir() -> PathBuf {
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."));
    let config_home = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| home.join(".config"));
    config_home.join("omarchy/plugins").join(PLUGIN_ID)
}

fn update_cache_is_fresh(report: &UpdateReport, current: &str, now: i64) -> bool {
    report.current == current && now.saturating_sub(report.checked_at) < UPDATE_CHECK_TTL
}

fn read_update_cache(paths: &Paths) -> Option<UpdateReport> {
    let text = fs::read_to_string(&paths.update).ok()?;
    serde_json::from_str(&text).ok()
}

/// Read the version out of the fetched manifest, so the panel can name the
/// release the user would get rather than only saying "an update".
fn remote_version(dir: &Path) -> String {
    let dir_text = dir.display().to_string();
    let Some(text) = util::run_stdout_within(
        "git",
        &["-C", &dir_text, "show", "FETCH_HEAD:manifest.json"],
        Duration::from_secs(10),
    ) else {
        return String::new();
    };
    serde_json::from_str::<Value>(&text)
        .ok()
        .and_then(|value| value.get("version")?.as_str().map(str::to_string))
        .unwrap_or_default()
}

/// Fetch the remote revision and compare it with the installed checkout. The
/// comparison is the one `omarchy plugin update` uses: how many commits HEAD is
/// behind the fetched remote head. Failures become a report, not an error, so a
/// background check never interrupts the panel or the log.
fn fetch_update_report(current: &str, checked_at: i64) -> UpdateReport {
    let mut report = UpdateReport {
        current: current.to_string(),
        latest: String::new(),
        available: false,
        behind: 0,
        checked_at,
        error: String::new(),
    };

    let dir = installed_plugin_dir();
    if !dir.is_dir() {
        report.error = "Reel is not installed as an Omarchy plugin.".into();
        return report;
    }
    if !dir.join(".git").exists() {
        report.error = "This install is not a git checkout.".into();
        return report;
    }

    let dir_text = dir.display().to_string();
    let fetched = util::run_stdout_within_env(
        "git",
        &["-C", &dir_text, "fetch", "--quiet", "origin", "HEAD"],
        &[
            ("GIT_TERMINAL_PROMPT", "0"),
            ("GIT_SSH_COMMAND", "ssh -oBatchMode=yes"),
        ],
        Duration::from_secs(20),
    );
    if fetched.is_none() {
        report.error = "The remote could not be reached.".into();
        return report;
    }

    let behind = util::run_stdout_within(
        "git",
        &["-C", &dir_text, "rev-list", "--count", "HEAD..FETCH_HEAD"],
        Duration::from_secs(10),
    )
    .and_then(|out| out.trim().parse::<u64>().ok());
    let Some(behind) = behind else {
        report.error = "The remote revision could not be read.".into();
        return report;
    };

    report.behind = behind;
    report.available = behind > 0;
    if report.available {
        report.latest = remote_version(&dir);
    }
    report
}

/// Check whether a newer Reel is available. The hourly service call uses the
/// cached result inside the hour; `force` (the panel's manual check) always
/// contacts the remote. Prints JSON for the panel or one line for a terminal.
pub fn check_update(paths: &Paths, force: bool, as_json: bool) -> Result<()> {
    let current = env!("CARGO_PKG_VERSION").to_string();
    let now = paths::now_secs();

    let cached = read_update_cache(paths);
    let fresh = if force {
        None
    } else {
        cached.filter(|report| update_cache_is_fresh(report, &current, now))
    };

    let report = match fresh {
        Some(report) => report,
        None => {
            let report = fetch_update_report(&current, now);
            if let Ok(body) = serde_json::to_string(&report) {
                let _ = paths::write_atomic(&paths.update, &body);
            }
            report
        }
    };

    if as_json {
        println!("{}", serde_json::to_string(&report)?);
        return Ok(());
    }

    if !report.error.is_empty() {
        println!("Could not check for updates: {}", report.error);
    } else if report.available {
        if report.latest.is_empty() || report.latest == report.current {
            println!(
                "An update is available (current {}). Run `reel update`.",
                report.current
            );
        } else {
            println!(
                "Reel {} is available (current {}). Run `reel update`.",
                report.latest, report.current
            );
        }
    } else {
        println!("Reel {} is up to date.", report.current);
    }
    Ok(())
}

/// Update the installed Omarchy plugin, then run the installer from the
/// updated checkout. The installer is version-matched to the engine it just
/// fetched, so there is no second download or curl-to-shell step.
pub fn update(paths: &Paths, binds: bool) -> Result<()> {
    state::reconcile(paths);
    match state::status(paths).as_str() {
        "idle" => {}
        "finalizing" => anyhow::bail!("Reel is still saving the last take. Try again in a moment."),
        _ => anyhow::bail!("Stop the current take before updating Reel."),
    }

    const ID: &str = PLUGIN_ID;
    let plugin_dir = installed_plugin_dir();
    if !plugin_dir.is_dir() {
        anyhow::bail!(
            "Reel is not installed as an Omarchy plugin at {}",
            plugin_dir.display()
        );
    }
    if !util::have("omarchy") {
        anyhow::bail!("The omarchy command is not on PATH.");
    }

    println!("== Reel: update ==");
    println!("  updating {ID}");
    let status = Command::new("omarchy")
        .args(["plugin", "update", ID, "--yes"])
        .status()
        .context("could not run omarchy plugin update")?;
    if !status.success() {
        anyhow::bail!("omarchy plugin update failed for {ID}");
    }

    let installer = plugin_dir.join("install.sh");
    if !installer.is_file() {
        anyhow::bail!(
            "the updated plugin has no installer at {}",
            installer.display()
        );
    }
    let mut command = Command::new("bash");
    command.arg(&installer);
    // Without --binds, leave the installer interactive. It only asks when the
    // shortcut block is missing; an existing block is left alone.
    if binds {
        command.arg("--yes");
    }
    let status = command
        .status()
        .context("could not run the updated Reel installer")?;
    if !status.success() {
        anyhow::bail!("the updated Reel installer failed");
    }
    // The cached check described the old checkout. Drop it so the next panel
    // look can confirm the new version without waiting for the hour.
    paths::remove_quiet(&paths.update);
    println!("Reel updated.");
    Ok(())
}

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

    // A capture whose quarantine was denied keeps its ordinary name, so the
    // name alone cannot rule it out. The durable ledger is the other half.
    let withheld = crate::quarantine::blocked(paths);

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
            // Raw captures that still carry the countdown never belong in
            // history. They stay on disk under a `-untrimmed` name and the
            // failure notice points at them. Half-written save passes are
            // skipped for the same reason.
            if paths::is_untrimmed(&path) || paths::is_partial(&path) {
                return None;
            }
            if withheld.contains(&crate::quarantine::key(&path)) {
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

pub fn webcams(as_json: bool) -> Result<()> {
    let found = webcam::devices();
    if as_json {
        let cameras: Vec<_> = found
            .iter()
            .map(|camera| json!({ "id": camera.id, "path": camera.path, "label": camera.label }))
            .collect();
        println!("{}", serde_json::to_string(&json!({ "cameras": cameras }))?);
    } else if found.is_empty() {
        println!("No cameras found.");
    } else {
        for camera in &found {
            println!("{}\t{}", camera.id, camera.label);
        }
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

/// The zoom depth resolved for the active take. The state carries the value the
/// take started with; the config is a fallback for a missing state file.
fn configured_zoom_max(paths: &Paths) -> f64 {
    let from_state = state::read(paths)
        .and_then(|st| st.get("settings").and_then(|s| s.get("zoomMax")).cloned())
        .and_then(|value| config::zoom_max_value(Some(&value)));
    let from_config = config::zoom_max_value(config::load(paths).get("zoomMax"));
    from_state.or(from_config).unwrap_or(2.0)
}

/// Whether the live camera follows the pointer during this take. Only monitor
/// captures do; a window or area keeps the camera detached so the pointer is
/// not pulled out of a small frame.
fn follow_pointer(paths: &Paths) -> bool {
    state::read(paths)
        .and_then(|st| state::field_str(&st, "targetKind"))
        .as_deref()
        == Some("monitor")
}

/// One step up, capped at the configured depth. A zoom that is already above a
/// lowered cap is left where it is: this only limits increases.
fn zoom_in_target(current: f64, step: f64, cap: f64) -> f64 {
    let cap = cap.clamp(1.1, 4.0);
    if current >= cap {
        return current;
    }
    (current + step).min(cap)
}

pub fn zoom_in(paths: &Paths, step: f64) -> Result<()> {
    if !is_manual_zoom_active(paths) {
        return Ok(());
    }
    let current = crate::zoom::compositor_zoom();
    let step = if step > 0.01 { step } else { 0.25 };
    let target = zoom_in_target(current, step, configured_zoom_max(paths));
    let immediate = step <= 0.16;
    let new_z = crate::zoom::animate_compositor_zoom(target, immediate, follow_pointer(paths));
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
    let new_z = crate::zoom::animate_compositor_zoom(target, immediate, follow_pointer(paths));
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

/// The target of a toggle. An explicit factor wins; without one, "zoom in"
/// means the configured depth rather than a hard-coded 2x.
fn zoom_toggle_target(current: f64, factor: f64, cap: f64) -> f64 {
    let factor = if factor > 1.05 { factor } else { cap };
    if current > 1.05 {
        1.0
    } else {
        factor.clamp(1.1, 4.0)
    }
}

pub fn zoom_toggle(paths: &Paths, factor: f64) -> Result<()> {
    if !is_manual_zoom_active(paths) {
        return Ok(());
    }
    let current = crate::zoom::compositor_zoom();
    let target = zoom_toggle_target(current, factor, configured_zoom_max(paths));
    let new_z = crate::zoom::animate_compositor_zoom(target, false, follow_pointer(paths));
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

fn pan_delta(direction: &str, distance: f64) -> Option<(f64, f64)> {
    match direction {
        "left" => Some((-distance, 0.0)),
        "right" => Some((distance, 0.0)),
        "up" => Some((0.0, -distance)),
        "down" => Some((0.0, distance)),
        _ => None,
    }
}

/// The recorded target, so a pan cannot walk the view past the capture.
fn pan_bounds(paths: &Paths) -> Option<(f64, f64, f64, f64)> {
    let state = state::read(paths)?;
    let target = state::field_str(&state, "target")?;
    let (x, y, width, height) = crate::target::geometry(&target)?;
    Some((x as f64, y as f64, width as f64, height as f64))
}

/// Pan the zoomed view by one step, or keep panning until the key is released
/// when `hold` is set. The zoom factor is never touched.
pub fn zoom_pan(
    paths: &Paths,
    direction: &str,
    step: f64,
    fast: bool,
    hold: bool,
    stop: bool,
) -> Result<()> {
    let stop_marker = paths.runtime.join("zoom-pan.stop");
    let holder = paths.runtime.join("zoom-pan.pid");

    if stop {
        let _ = paths::write_atomic(&stop_marker, "");
        if let Some(pid) = paths::read_pid(&holder) {
            let deadline = std::time::Instant::now() + Duration::from_millis(400);
            while std::time::Instant::now() < deadline && paths::alive(pid) {
                std::thread::sleep(Duration::from_millis(10));
            }
        }
        paths::remove_quiet(&stop_marker);
        paths::remove_quiet(&holder);
        return Ok(());
    }

    if !is_manual_zoom_active(paths) {
        return Ok(());
    }
    // A window or area capture keeps the camera detached, so moving the pointer
    // would move the cursor without panning the view. Panning is monitor-only.
    if !follow_pointer(paths) {
        return Ok(());
    }
    if crate::zoom::compositor_zoom() <= 1.01 {
        return Ok(());
    }
    let configured = config::cfg(paths, "panStep", "60")
        .trim()
        .parse::<f64>()
        .unwrap_or(60.0);
    let base = if step > 1.0 { step } else { configured };
    let distance = if fast { base * 3.0 } else { base };
    let Some((dx, dy)) = pan_delta(direction, distance) else {
        anyhow::bail!("zoom-pan expects left, right, up, or down.");
    };
    let bounds = pan_bounds(paths);

    if !hold {
        if crate::zoom::pan_view(dx, dy, bounds) {
            println!("Reel zoom: pan {direction}");
        }
        return Ok(());
    }

    // Hold: one process pans at ~30 Hz until the key comes up. A tap still
    // gets a full step because the first move happens immediately.
    let _ = paths::write_pid(&holder, std::process::id());
    paths::remove_quiet(&stop_marker);
    let (hold_x, hold_y) = pan_delta(direction, (distance / 3.0).max(6.0)).unwrap_or((dx, dy));
    let mut first = true;
    let mut ticks: u32 = 0;
    loop {
        if stop_marker.exists() || state::status(paths) != "recording" {
            break;
        }
        // A reset to 1x during the hold leaves nothing to pan. Probe a few
        // times a second rather than spawning hyprctl on every 33 ms tick.
        if ticks.is_multiple_of(10) && crate::zoom::compositor_zoom() <= 1.01 {
            break;
        }
        ticks = ticks.wrapping_add(1);
        let (step_x, step_y) = if first { (dx, dy) } else { (hold_x, hold_y) };
        if !crate::zoom::pan_view(step_x, step_y, bounds) {
            break;
        }
        first = false;
        std::thread::sleep(Duration::from_millis(33));
    }
    paths::remove_quiet(&holder);
    paths::remove_quiet(&stop_marker);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_paths(root: &Path) -> Paths {
        let paths = Paths::for_test(root);
        fs::create_dir_all(&paths.config_dir).unwrap();
        paths
    }

    fn temp_root(label: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "reel-{label}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ))
    }

    #[test]
    fn an_update_cache_only_lasts_an_hour_for_the_same_release() {
        let report = UpdateReport {
            current: "0.3.0".into(),
            latest: "0.3.1".into(),
            available: true,
            behind: 2,
            checked_at: 1000,
            error: String::new(),
        };
        assert!(update_cache_is_fresh(&report, "0.3.0", 1000 + 3599));
        assert!(!update_cache_is_fresh(&report, "0.3.0", 1000 + 3600));
        // A new engine version means the cached answer describes the old
        // checkout, so it must not be reused.
        assert!(!update_cache_is_fresh(&report, "0.3.1", 1000 + 10));
    }

    #[test]
    fn update_reports_round_trip_through_the_cache_format() {
        let report = UpdateReport {
            current: "0.3.0".into(),
            latest: "0.3.1".into(),
            available: true,
            behind: 2,
            checked_at: 1000,
            error: String::new(),
        };
        let body = serde_json::to_string(&report).unwrap();
        assert!(body.contains("\"checkedAt\":1000"), "{body}");
        let parsed: UpdateReport = serde_json::from_str(&body).unwrap();
        assert_eq!(parsed, report);
    }

    #[test]
    fn untrimmed_footage_stays_out_of_recent_takes() {
        let root = temp_root("recent");
        let videos = root.join("Videos");
        fs::create_dir_all(&videos).unwrap();
        let paths = test_paths(&root);
        fs::write(
            &paths.config,
            format!("{{\"outputDir\":\"{}\"}}", videos.display()),
        )
        .unwrap();
        fs::write(videos.join("reel-1.mp4"), b"take").unwrap();
        fs::write(videos.join("reel-2-untrimmed.mp4"), b"raw").unwrap();
        fs::write(videos.join("reel-3.part.4242.mp4"), b"part").unwrap();
        fs::write(videos.join("reel-4.capture.99.mp4"), b"capture").unwrap();

        let takes = recent_takes(&paths, 10);
        assert_eq!(takes.len(), 1);
        assert!(takes[0].path.ends_with("reel-1.mp4"));
        assert!(!paths::is_partial(Path::new("reel-1.mp4")));
        assert!(paths::is_partial(Path::new("reel-1.part.42.mp4")));
        assert!(paths::is_partial(Path::new("reel-1.capture.mp4")));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn a_withheld_capture_is_not_a_recent_take() {
        let root = temp_root("withheld");
        let videos = root.join("Videos");
        fs::create_dir_all(&videos).unwrap();
        let paths = test_paths(&root);
        fs::write(
            &paths.config,
            format!("{{\"outputDir\":\"{}\"}}", videos.display()),
        )
        .unwrap();
        // A capture whose rename was denied keeps its ordinary name.
        let file = videos.join("reel-9.mp4");
        fs::write(&file, b"raw").unwrap();

        crate::quarantine::record(&paths, &file, "countdown");
        assert!(recent_takes(&paths, 10).is_empty());

        crate::quarantine::clear(&paths, &file);
        assert_eq!(recent_takes(&paths, 10).len(), 1);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn zoom_in_stops_at_the_configured_depth() {
        assert_eq!(zoom_in_target(1.0, 0.25, 3.0), 1.25);
        assert_eq!(zoom_in_target(2.75, 0.25, 3.0), 3.0);
        assert_eq!(zoom_in_target(3.0, 0.25, 3.0), 3.0);
    }

    #[test]
    fn a_lowered_cap_does_not_force_a_decrease() {
        // An existing zoom above a newly lowered cap stays put; only increases
        // are capped.
        assert_eq!(zoom_in_target(3.5, 0.25, 2.0), 3.5);
        assert_eq!(zoom_in_target(1.9, 0.25, 2.0), 2.0);
    }

    #[test]
    fn zoom_toggle_uses_the_configured_depth() {
        assert_eq!(zoom_toggle_target(1.0, 0.0, 3.0), 3.0);
        assert_eq!(zoom_toggle_target(1.0, 0.0, 1.5), 1.5);
        // Already zoomed: toggle goes back to full frame.
        assert_eq!(zoom_toggle_target(2.2, 0.0, 3.0), 1.0);
        // An explicit factor still wins.
        assert_eq!(zoom_toggle_target(1.0, 1.8, 3.0), 1.8);
    }

    #[test]
    fn pan_directions_map_to_steps() {
        assert_eq!(pan_delta("left", 60.0), Some((-60.0, 0.0)));
        assert_eq!(pan_delta("right", 60.0), Some((60.0, 0.0)));
        assert_eq!(pan_delta("up", 60.0), Some((0.0, -60.0)));
        assert_eq!(pan_delta("down", 60.0), Some((0.0, 60.0)));
        assert_eq!(pan_delta("sideways", 60.0), None);
    }

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
