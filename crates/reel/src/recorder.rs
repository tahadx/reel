use crate::cli::StartArgs;
use crate::config;
use crate::finalize::{factor_str, label};
use crate::paths::{self, Lock, Paths};
use crate::state;
use crate::target;
use crate::util;
use crate::webcam;
use crate::zoom::{self, Mode};
use anyhow::{Context, Result};
use serde_json::{json, Value};
use std::fs::{self, OpenOptions};
use std::path::Path;
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

/// Everything the picker and capture process need. It is resolved once at the
/// start of a take so the recording cannot silently change mid-flow.
#[derive(Debug, Clone)]
struct Settings {
    desktop: bool,
    mic: bool,
    cursor: bool,
    webcam: String,
    webcam_device: String,
    slowmo: String,
    countdown: i64,
    fps: i64,
    quality: String,
    codec: String,
    resolution: String,
    container: String,
    output_dir: String,
    filename_prefix: String,
    auto_zoom: zoom::Settings,
}

impl Settings {
    fn to_json(&self) -> Value {
        json!({
            "desktopAudio": self.desktop,
            "microphoneAudio": self.mic,
            "cursor": self.cursor,
            "webcam": self.webcam,
            "webcamDevice": self.webcam_device,
            "slowmo": self.slowmo,
            "countdown": self.countdown,
            "fps": self.fps,
            "quality": self.quality,
            "codec": self.codec,
            "resolution": self.resolution,
            "container": self.container,
            "outputDir": self.output_dir,
            "filenamePrefix": self.filename_prefix,
            "autoZoom": self.auto_zoom.mode.as_str(),
            "followMouse": self.auto_zoom.follow_mouse,
            "focusAutoZoom": self.auto_zoom.focus_auto_zoom,
        })
    }
}

fn effective(option: &Option<String>, settings: &Value, key: &str, fallback: &str) -> String {
    match option {
        Some(value) if !value.is_empty() => value.clone(),
        _ => config::cfg_in(settings, key, fallback),
    }
}

fn quality_level(raw: &str) -> &'static str {
    match raw.trim().to_ascii_lowercase().as_str() {
        "very_low" | "1000" => "very_low",
        "low" | "2000" => "low",
        "medium" | "4000" => "medium",
        "high" | "8000" => "high",
        "ultra" | "32000" => "ultra",
        _ => "very_high",
    }
}

/// gpu-screen-recorder only names four presets. The two quieter levels use the
/// medium preset with an explicit QP override, which keeps QP mode (and avoids
/// the VAAPI CBR failures at high frame rates).
fn quality_preset(quality: &str) -> &'static str {
    match quality {
        "very_low" | "low" | "medium" => "medium",
        "high" => "high",
        "ultra" => "ultra",
        _ => "very_high",
    }
}

fn quality_qp_override(quality: &str, codec: &str) -> Option<i64> {
    let qp = match quality {
        "very_low" => 45,
        "low" => 40,
        _ => return None,
    };
    // gpu-screen-recorder uses the same H.264-equivalent scale for H.264 and
    // HEVC, and a 4x scale for AV1.
    Some(if codec == "av1" { qp * 4 } else { qp })
}

fn resolve_settings(paths: &Paths, options: &StartArgs) -> Result<Settings> {
    // Read reel.json once and resolve every option from it. The old path
    // reloaded and re-parsed the file for each option, which was wasteful even
    // if each read was small.
    let stored = config::load(paths);
    let desktop = util::normalize_switch(&effective(
        &options.desktop_audio,
        &stored,
        "desktopAudio",
        "on",
    ));
    let mic = util::normalize_switch(&effective(
        &options.microphone_audio,
        &stored,
        "microphoneAudio",
        "off",
    ));
    let cursor = util::normalize_switch(&effective(&options.cursor, &stored, "cursor", "on"));
    let webcam = util::pick_choice(
        &effective(&options.webcam, &stored, "webcam", "off"),
        "off",
        &["off", "small", "medium", "large"],
    );
    let webcam_device = effective(&options.webcam_device, &stored, "webcamDevice", "");
    let slowmo = util::pick_choice(
        label(&effective(&options.slowmo, &stored, "slowmo", "off")),
        "off",
        &["off", "0.5x", "0.25x"],
    );
    let countdown =
        util::at_least_int(&effective(&options.countdown, &stored, "countdown", "0"), 0).min(60);
    let fps = util::pick_choice(
        &effective(&options.fps, &stored, "fps", "60"),
        "60",
        &["30", "60", "120"],
    )
    .parse::<i64>()
    .unwrap_or(60);
    let quality = quality_level(&effective(
        &options.quality,
        &stored,
        "quality",
        "very_high",
    ))
    .to_string();
    let codec = util::pick_choice(
        &effective(&options.codec, &stored, "codec", "auto"),
        "auto",
        &["auto", "h264", "hevc", "av1"],
    );
    let resolution = util::pick_choice(
        &effective(&options.resolution, &stored, "resolution", "0x0"),
        "0x0",
        &["0x0", "3840x2160", "2560x1440", "1920x1080", "1280x720"],
    );
    let container = util::pick_choice(
        &effective(&options.container, &stored, "container", "mp4"),
        "mp4",
        &["mp4", "mkv"],
    );
    let configured_dir = effective(&options.output_dir, &stored, "outputDir", "");
    let dir = if configured_dir.is_empty() {
        util::default_output_dir().display().to_string()
    } else {
        util::expand_home(&configured_dir)
    };
    let filename_prefix = effective(&options.filename_prefix, &stored, "filenamePrefix", "");
    let mut auto_zoom = zoom::settings_from(&stored);
    if let Some(ref mode_str) = options.auto_zoom {
        auto_zoom.mode = zoom::Mode::from_value(Some(&serde_json::Value::String(mode_str.clone())));
    }
    if let Some(ref val) = options.follow_mouse {
        auto_zoom.follow_mouse = util::normalize_switch(val);
    }
    if let Some(ref val) = options.focus_auto_zoom {
        auto_zoom.focus_auto_zoom = util::normalize_switch(val);
    }

    Ok(Settings {
        desktop,
        mic,
        cursor,
        webcam,
        webcam_device,
        slowmo,
        countdown,
        fps,
        quality,
        codec,
        resolution,
        container,
        output_dir: dir,
        filename_prefix,
        auto_zoom,
    })
}

fn require_tools() -> Result<()> {
    if !util::have("gpu-screen-recorder") {
        util::notify(
            "Reel",
            Some("gpu-screen-recorder is missing. Install it with: omarchy pkg add gpu-screen-recorder"),
            "critical",
        );
        anyhow::bail!(
            "gpu-screen-recorder is missing. Install it with: omarchy pkg add gpu-screen-recorder"
        );
    }
    for tool in ["ffmpeg", "ffprobe"] {
        if !util::have(tool) {
            anyhow::bail!("{tool} is missing.");
        }
    }
    Ok(())
}

fn prepare_output_dir(dir: &str) -> Result<()> {
    fs::create_dir_all(dir)
        .with_context(|| format!("Cannot create the recordings folder: {dir}"))?;
    let probe = Path::new(dir).join(format!(".reel-write-{}", std::process::id()));
    match OpenOptions::new().create_new(true).write(true).open(&probe) {
        Ok(_) => {
            paths::remove_quiet(&probe);
            Ok(())
        }
        Err(err) => anyhow::bail!("The recordings folder is not writable: {dir} ({err})"),
    }
}

fn available_audio() -> Option<String> {
    // Bounded: enumerating devices must never stall a take.
    util::run_stdout_within(
        "gpu-screen-recorder",
        &["--list-audio-devices"],
        Duration::from_secs(3),
    )
}

fn validate_audio(settings: &Settings) -> Result<()> {
    if !settings.desktop && !settings.mic {
        return Ok(());
    }
    let Some(devices) = available_audio() else {
        return Ok(());
    };
    if settings.desktop
        && !devices
            .lines()
            .any(|line| line.starts_with("default_output|"))
    {
        anyhow::bail!(
            "No system audio output is available. Choose another audio source or turn desktop audio off."
        );
    }
    if settings.mic
        && !devices
            .lines()
            .any(|line| line.starts_with("default_input|"))
    {
        anyhow::bail!(
            "No microphone is available. Choose another audio source or turn the microphone off."
        );
    }
    Ok(())
}

/// Hyprland can composite the pointer into the scanout when it falls back to a
/// software cursor. gpu-screen-recorder then has no pointer to remove, even
/// with `-cursor no`. Force the hardware cursor for the take so it stays on a
/// separate plane and out of the captured framebuffer.
#[derive(Debug, Clone, Copy)]
struct HardwareCursor {
    original: i64,
}

fn hardware_cursor_marker(paths: &Paths) -> std::path::PathBuf {
    paths.runtime.join("cursor-mode")
}

fn hyprland_cursor_mode() -> Option<i64> {
    let text = util::run_stdout(
        "hyprctl",
        &["getoption", "cursor:no_hardware_cursors", "-j"],
    )?;
    let value: Value = serde_json::from_str(&text).ok()?;
    value.get("int")?.as_i64()
}

fn set_hyprland_cursor_mode(mode: i64) -> bool {
    let lua = format!("hl.config({{ cursor = {{ no_hardware_cursors = {mode} }} }})");
    if util::run_ok("hyprctl", &["eval", &lua]) {
        return true;
    }
    // Older Hyprland releases use the non-Lua keyword interface.
    util::run_ok(
        "hyprctl",
        &["keyword", "cursor:no_hardware_cursors", &mode.to_string()],
    )
}

impl HardwareCursor {
    fn force(paths: &Paths) -> Option<Self> {
        std::env::var_os("HYPRLAND_INSTANCE_SIGNATURE")?;
        let original = hyprland_cursor_mode()?;
        if original == 0 {
            return None;
        }

        // Leave a marker behind so a failed start or a stopped recording can
        // always put the user's compositor setting back.
        let marker = hardware_cursor_marker(paths);
        paths::write_atomic(&marker, &original.to_string()).ok()?;
        if !set_hyprland_cursor_mode(0) {
            paths::remove_quiet(&marker);
            return None;
        }

        paths::log(
            paths,
            "capture: using a hardware cursor so the pointer is not baked into the frame",
        );
        Some(Self { original })
    }

    fn restore(self, paths: &Paths) {
        restore_hardware_cursor(paths, self.original);
    }
}

fn restore_hardware_cursor(paths: &Paths, original: i64) {
    if set_hyprland_cursor_mode(original) {
        paths::log(
            paths,
            &format!("cursor: restored no_hardware_cursors={original}"),
        );
    }
    paths::remove_quiet(&hardware_cursor_marker(paths));
}

/// Restore a mode left behind by a take that did not finish cleanly.
fn restore_cursor_mode(paths: &Paths) {
    let marker = hardware_cursor_marker(paths);
    let Ok(text) = fs::read_to_string(&marker) else {
        return;
    };
    match text.trim().parse::<i64>() {
        Ok(original) => restore_hardware_cursor(paths, original),
        Err(_) => paths::remove_quiet(&marker),
    }
}

fn cancelled(paths: &Paths) -> bool {
    paths.cancel.exists()
}

/// Start the detached auto-zoom tracker for a take. It has to be its own
/// process: `reel start` exits as soon as the encoder is running, and threads
/// do not outlive it.
fn start_zoom_tracker(paths: &Paths, file: &str, target: &str, settings: &zoom::Settings) {
    if settings.mode == Mode::Off {
        return;
    }
    let Ok(exe) = std::env::current_exe() else {
        return;
    };
    let sentinel = paths.runtime.join("zoom.active");
    let pid_file = paths.runtime.join("zoom.pid");
    let sidecar = Path::new(file).with_extension("zoom.json");

    paths::remove_quiet(&sidecar);
    let _ = paths::write_atomic(&sentinel, "");

    let child = Command::new(exe)
        .arg("__zoom")
        .arg("--sentinel")
        .arg(&sentinel)
        .arg("--sidecar")
        .arg(&sidecar)
        .arg("--target")
        .arg(target)
        .arg("--max-zoom")
        .arg(format!("{:.3}", settings.max_zoom))
        .arg("--typing-zoom")
        .arg(format!("{:.3}", settings.typing_zoom))
        .arg("--mode")
        .arg(settings.mode.as_str())
        .arg("--follow-mouse")
        .arg(if settings.follow_mouse { "on" } else { "off" })
        .arg("--focus-auto-zoom")
        .arg(if settings.focus_auto_zoom { "on" } else { "off" })
        .arg("--idle-timeout")
        .arg(settings.idle_timeout.as_millis().to_string())
        .arg("--unzoom-speed")
        .arg(format!("{:.1}", settings.unzoom_speed))
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn();

    match child {
        Ok(child) => {
            let _ = paths::write_pid(&pid_file, child.id());
            paths::log(
                paths,
                &format!(
                    "zoom: tracking {} up to {:.2}x ({})",
                    file,
                    settings.max_zoom,
                    settings.mode.as_str()
                ),
            );
        }
        Err(err) => paths::log(paths, &format!("zoom: cannot start the tracker: {err}")),
    }
}

/// Stop the tracker and give it a moment to flush the last keyframes.
fn stop_zoom_tracker(paths: &Paths) {
    let sentinel = paths.runtime.join("zoom.active");
    let pid_file = paths.runtime.join("zoom.pid");
    let pid = paths::read_pid(&pid_file).unwrap_or(0);
    paths::remove_quiet(&sentinel);

    if pid > 0 && paths::alive(pid) {
        wait_until(Duration::from_millis(1500), || !paths::alive(pid));
        if paths::alive(pid) {
            paths::kill(pid, libc::SIGTERM);
            wait_until(Duration::from_millis(500), || !paths::alive(pid));
        }
    }
    paths::remove_quiet(&pid_file);
    let _ = zoom::reset_compositor_zoom();
}

/// Poll with a short, backing-off delay until `ready` or the deadline. The
/// first checks are deliberately close together: gpu-screen-recorder usually
/// closes its file in well under 100 ms, and a fixed 100 ms poll made every
/// start and stop feel a beat slower.
fn wait_until(timeout: Duration, mut ready: impl FnMut() -> bool) -> bool {
    let deadline = Instant::now() + timeout;
    let mut delay = Duration::from_millis(5);
    loop {
        if ready() {
            return true;
        }
        if Instant::now() >= deadline {
            return false;
        }
        thread::sleep(delay);
        delay = (delay * 2).min(Duration::from_millis(50));
    }
}

/// Wait for a recorder we did not spawn. After the recorder exits it is
/// reaped by its own parent, so `alive` turns false; our own children would
/// linger as zombies instead, and must go through `wait_for_child`.
fn wait_for_exit(pid: i32, timeout: Duration) -> bool {
    wait_until(timeout, || !paths::alive(pid))
}

/// Wait for a child of this process, reaping it. `try_wait` reports a zombie
/// as exited, which `kill(pid, 0)` does not.
fn wait_for_child(child: &mut std::process::Child, timeout: Duration) -> bool {
    wait_until(timeout, || matches!(child.try_wait(), Ok(Some(_))))
}

/// Wait until the recorder has produced its first file, exited, or the take
/// was cancelled. A cold encoder can take a second or more to get there.
fn wait_for_capture_file(paths: &Paths, file: &Path, pid: i32, timeout: Duration) -> bool {
    // A dead recorder or an aborted take will never produce the file.
    wait_until(timeout, || {
        file.exists() || cancelled(paths) || !paths::alive(pid)
    });
    file.exists()
}

/// Stop a recorder this process spawned: ask politely, then insist, and reap.
fn stop_child(child: &mut std::process::Child, grace: Duration) {
    let pid = child.id() as i32;
    paths::kill(pid, libc::SIGINT);
    if wait_for_child(child, grace) {
        return;
    }
    paths::kill(pid, libc::SIGKILL);
    // Reap it if it goes; do not block forever on an uninterruptible process.
    let _ = wait_for_child(child, Duration::from_secs(1));
}

fn fail_start(paths: &Paths, message: &str) {
    let _ = state::write(
        paths,
        &state::idle_with_error_preserving_last(paths, message),
    );
    paths::remove_quiet(&paths.starting);
    util::notify("Reel", Some(message), "critical");
    paths::log(paths, &format!("error: {message}"));
}

fn start_guard(paths: &Paths) -> Result<String> {
    let _lock = Lock::acquire(paths)?;
    let current = state::status(paths);
    if matches!(current.as_str(), "recording" | "paused" | "starting") {
        anyhow::bail!("Reel is already busy. Stop the current take first.");
    }
    paths::remove_quiet(&paths.cancel);
    Ok(current)
}

/// Open the picker and return the choice. A cancelled pick is `Ok(None)`.
fn choose_target(paths: &Paths) -> Result<Option<target::Choice>> {
    {
        let _lock = Lock::acquire(paths)?;
        let now = paths::now_secs();
        let _ = paths::write_pid(&paths.starting, std::process::id());
        let _ = state::write(
            paths,
            &json!({
                "status": "starting",
                "stage": "select",
                "startedAt": now,
            }),
        );
    }

    util::debug("start: opening the capture picker");
    let choice = match target::pick() {
        Ok(Some(choice)) => choice,
        Ok(None) => {
            paths::log(paths, "start: target selection cancelled");
            let _ = state::write(paths, &state::idle_preserving_last(paths));
            paths::remove_quiet(&paths.starting);
            paths::remove_quiet(&paths.cancel);
            return Ok(None);
        }
        Err(err) => {
            paths::log(paths, &format!("start: {err}"));
            let _ = state::write(paths, &state::idle_with_error_preserving_last(paths, &err));
            paths::remove_quiet(&paths.starting);
            paths::remove_quiet(&paths.cancel);
            util::notify("Reel", Some(&err), "critical");
            return Err(anyhow::anyhow!(err));
        }
    };

    if cancelled(paths) {
        let _ = state::write(paths, &state::idle_preserving_last(paths));
        paths::remove_quiet(&paths.starting);
        paths::remove_quiet(&paths.cancel);
        return Ok(None);
    }

    util::debug(&format!("start: picked {}", choice.target));
    Ok(Some(choice))
}

fn unique_recording(dir: &str, container: &str, prefix: &str) -> String {
    let prefix = sanitize_prefix(prefix);
    let base = format!("{prefix}{}", paths::file_stamp());
    let mut candidate = format!("{dir}/{base}.{container}");
    let mut suffix = 2;
    while Path::new(&candidate).exists() {
        candidate = format!("{dir}/{base}-{suffix}.{container}");
        suffix += 1;
    }
    candidate
}

/// Keep the user's prefix readable but make it safe as one path component.
fn sanitize_prefix(raw: &str) -> String {
    let cleaned: String = raw
        .trim()
        .chars()
        .map(|ch| match ch {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' | '\0' => '-',
            ch if ch.is_control() => '-',
            ch => ch,
        })
        .take(80)
        .collect();
    let cleaned = cleaned.trim_matches(|ch: char| matches!(ch, '.' | ' ' | '-'));
    if cleaned.is_empty() {
        "reel-".into()
    } else if cleaned.ends_with('-') || cleaned.ends_with('_') || cleaned.ends_with(' ') {
        cleaned.to_string()
    } else {
        format!("{cleaned}-")
    }
}

/// Count down, start the webcam, launch the encoder, and move to `recording`
/// only after the encoder really produced a file.
fn run_take(paths: &Paths, settings: &Settings, choice: &target::Choice) -> Result<bool> {
    let target = &choice.target;
    if let Err(err) = target::validate_target(target) {
        fail_start(paths, &err);
        return Err(anyhow::anyhow!(err));
    }

    {
        let _lock = Lock::acquire(paths)?;
        let _ = paths::write_pid(&paths.starting, std::process::id());
        let now = paths::now_secs();
        let _ = state::write(
            paths,
            &json!({
                "status": "starting",
                "stage": "countdown",
                "target": target,
                "targetKind": &choice.kind,
                "targetLabel": &choice.label,
                "targetWarning": &choice.warning,
                "monitor": &choice.monitor,
                "settings": settings.to_json(),
                "startedAt": now,
            }),
        );
    }

    paths::remove_quiet(&paths.cancel);

    let mut tick = settings.countdown;
    while tick > 0 {
        if cancelled(paths) {
            break;
        }
        util::osd(&tick.to_string(), 850);
        thread::sleep(Duration::from_secs(1));
        tick -= 1;
    }

    if cancelled(paths) {
        webcam::cleanup(paths);
        let _ = state::write(paths, &state::idle_preserving_last(paths));
        paths::remove_quiet(&paths.starting);
        paths::remove_quiet(&paths.cancel);
        return Ok(false);
    }

    if !webcam::start(paths, target, &settings.webcam, &settings.webcam_device) {
        // The recorder still runs without a camera.
    }

    if cancelled(paths) {
        webcam::cleanup(paths);
        let _ = state::write(paths, &state::idle_preserving_last(paths));
        paths::remove_quiet(&paths.starting);
        paths::remove_quiet(&paths.cancel);
        return Ok(false);
    }

    let file = unique_recording(
        &settings.output_dir,
        &settings.container,
        &settings.filename_prefix,
    );
    let slowmo_value = factor_str(&settings.slowmo);
    let capture_fps = if slowmo_value != "1" && settings.fps < 120 {
        120
    } else {
        settings.fps
    };

    let mut capture: Vec<String> = vec![
        "-w".into(),
        String::new(),
        "-f".into(),
        capture_fps.to_string(),
        "-fm".into(),
        "cfr".into(),
        "-k".into(),
        settings.codec.clone(),
        "-q".into(),
        quality_preset(&settings.quality).into(),
        "-bm".into(),
        "qp".into(),
        "-cursor".into(),
        if settings.cursor {
            "yes".into()
        } else {
            "no".into()
        },
        "-fallback-cpu-encoding".into(),
        "yes".into(),
        "-ipc".into(),
        paths.socket.display().to_string(),
        "-o".into(),
        file.clone(),
    ];

    match target.as_str() {
        _ if target.starts_with("monitor:") => {
            capture[1] = target["monitor:".len()..].to_string();
        }
        _ if target.starts_with("region:") => {
            // Geometry goes straight into -w. gpu-screen-recorder 6.1.4
            // segfaults on the -w region + -region pair.
            capture[1] = target["region:".len()..].to_string();
        }
        "portal" => {
            capture[1] = "portal".into();
        }
        _ => {
            anyhow::bail!("The picker returned an unsupported target: {target}");
        }
    }
    if settings.resolution != "0x0" {
        capture.extend(["-s".into(), settings.resolution.clone()]);
    }

    let mut devices = String::new();
    if settings.desktop {
        devices.push_str("default_output");
    }
    if let Some(qp) = quality_qp_override(&settings.quality, &settings.codec) {
        capture.extend(["-ffmpeg-video-opts".into(), format!("qp={qp}")]);
    }

    if settings.mic {
        if !devices.is_empty() {
            devices.push('|');
        }
        devices.push_str("default_input");
    }
    if !devices.is_empty() {
        capture.extend(["-a".into(), devices, "-ac".into(), "aac".into()]);
    }

    util::debug(&format!(
        "capture: launching gpu-screen-recorder for {file}"
    ));
    paths::remove_quiet(&paths.socket);
    paths::remove_quiet(&paths.pid_file);
    paths::reset_log(paths);
    paths::log(
        paths,
        &format!(
            "capture: target={} fps={} slowmo={} cursor={} output={}",
            target,
            capture_fps,
            slowmo_value,
            if settings.cursor { "on" } else { "off" },
            file
        ),
    );

    let hardware_cursor = if !settings.cursor && target != "portal" {
        HardwareCursor::force(paths)
    } else {
        None
    };

    let log = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&paths.log)
        .context("cannot open the Reel log")?;
    let err = log.try_clone()?;
    let child = Command::new("gpu-screen-recorder")
        .args(&capture)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::from(err))
        .spawn();
    let mut child = match child {
        Ok(child) => child,
        Err(err) => {
            if let Some(cursor) = hardware_cursor {
                cursor.restore(paths);
            }
            webcam::cleanup(paths);
            fail_start(
                paths,
                &format!(
                    "The recorder did not start: {err}. See {}",
                    paths.log.display()
                ),
            );
            return Err(err.into());
        }
    };
    let pid = child.id();
    util::debug(&format!("capture: recorder pid {pid}"));
    let _ = paths::write_pid(&paths.pid_file, pid);

    // The encoder is running now, so the take has started even though its
    // first file can take a moment to appear (a cold GPU can need a second or
    // more). Announce the capture stage and start the clock here instead of
    // leaving the bar on the "starting" placeholder until the file shows up.
    let started_at = paths::now_secs();
    {
        let _lock = Lock::acquire(paths).ok();
        let _ = state::write(
            paths,
            &json!({
                "status": "starting",
                "stage": "capture",
                "pid": pid,
                "file": &file,
                "target": target,
                "targetKind": &choice.kind,
                "targetLabel": &choice.label,
                "targetWarning": &choice.warning,
                "monitor": &choice.monitor,
                "settings": settings.to_json(),
                "startedAt": started_at,
            }),
        );
    }

    let ready_file =
        wait_for_capture_file(paths, Path::new(&file), pid as i32, Duration::from_secs(15));

    // A stop during the warmup writes the cancel marker; the take never got
    // going, so tear it down quietly instead of reporting a start failure.
    if cancelled(paths) {
        stop_child(&mut child, Duration::from_secs(4));
        if let Some(cursor) = hardware_cursor {
            cursor.restore(paths);
        }
        webcam::cleanup(paths);
        paths::remove_quiet(&paths.pid_file);
        paths::remove_quiet(&paths.socket);
        paths::remove_quiet(Path::new(&file));
        let _ = state::write(paths, &state::idle_preserving_last(paths));
        paths::remove_quiet(&paths.starting);
        paths::remove_quiet(&paths.cancel);
        return Ok(false);
    }

    if !ready_file {
        stop_child(&mut child, Duration::from_secs(2));
        if let Some(cursor) = hardware_cursor {
            cursor.restore(paths);
        }
        webcam::cleanup(paths);
        let detail = fs::read_to_string(&paths.log).ok().and_then(|log| {
            log.lines()
                .rev()
                .map(str::trim)
                .find(|line| !line.is_empty() && !line.starts_with('['))
                .map(str::to_string)
        });
        let message = match detail {
            Some(detail) => format!(
                "The recorder did not start: {detail}. See {}",
                paths.log.display()
            ),
            None => format!("The recorder did not start. See {}", paths.log.display()),
        };
        fail_start(paths, &message);
        paths::remove_quiet(&paths.pid_file);
        paths::remove_quiet(Path::new(&file));
        return Err(anyhow::anyhow!(message));
    }

    {
        let _lock = Lock::acquire(paths).ok();
        // Keep the clock started at the capture stage so the bar timer does not
        // jump back to zero when the encoder's file finally appears.
        let _ = state::write(
            paths,
            &json!({
                "status": "recording",
                "pid": pid,
                "file": file,
                "target": target,
                "targetKind": &choice.kind,
                "targetLabel": &choice.label,
                "targetWarning": &choice.warning,
                "monitor": &choice.monitor,
                "quality": settings.quality,
                "codec": settings.codec,
                "slowmo": settings.slowmo,
                "desktopAudio": settings.desktop,
                "microphoneAudio": settings.mic,
                "cursor": settings.cursor,
                "webcam": settings.webcam,
                "fps": capture_fps,
                "startedAt": started_at,
                "paused": false,
                "pausedTotal": 0,
                "pausedAt": 0,
                "settings": settings.to_json(),
            }),
        );
    }
    start_zoom_tracker(paths, &file, &target, &settings.auto_zoom);
    paths::remove_quiet(&paths.starting);
    paths::remove_quiet(&paths.cancel);
    println!("{file}");
    Ok(true)
}

/// Select a target, then count down and record without an intermediate step.
pub fn start(paths: &Paths, options: StartArgs) -> Result<bool> {
    let settings = resolve_settings(paths, &options)?;
    require_tools()?;
    prepare_output_dir(&settings.output_dir)?;
    let _ = start_guard(paths)?;

    // Check the audio devices before opening the picker, but only once the
    // "already busy" state is ruled out. The probe is a single fast call, and
    // failing here means a missing microphone is reported immediately instead
    // of after the user has already drawn a region.
    if let Err(err) = validate_audio(&settings) {
        fail_start(paths, &err.to_string());
        return Err(err);
    }
    restore_cursor_mode(paths);

    match choose_target(paths)? {
        Some(choice) => run_take(paths, &settings, &choice),
        None => Ok(false),
    }
}

fn send_pause(paths: &Paths, paused: bool) -> bool {
    use std::io::{BufRead, BufReader, Write};
    use std::os::unix::net::UnixStream;

    let Ok(mut stream) = UnixStream::connect(&paths.socket) else {
        return false;
    };
    // gpu-screen-recorder answers with one JSON line and leaves the
    // connection open. Reading to EOF therefore waits for the timeout on
    // every pause; read just the reply line so the action is immediate.
    let _ = stream.set_read_timeout(Some(Duration::from_millis(500)));
    let _ = stream.set_write_timeout(Some(Duration::from_millis(500)));
    let payload = format!("{{\"id\":1,\"name\":\"set-paused\",\"data\":{paused}}}\n");
    if stream.write_all(payload.as_bytes()).is_err() {
        return false;
    }
    let mut reply = String::new();
    let _ = BufReader::new(stream).read_line(&mut reply);
    reply.contains("\"result\":\"ok\"")
}

pub fn pause(paths: &Paths) -> Result<()> {
    let _lock = Lock::acquire(paths)?;
    let current = state::read_or_idle(paths);
    let status = state::field_str(&current, "status").unwrap_or_default();
    if status == "paused" {
        return Ok(());
    }
    let pid = state::field_i64(&current, "pid").unwrap_or(0) as i32;
    if status != "recording" || !paths::alive(pid) {
        anyhow::bail!("Reel is not recording.");
    }

    let mut ok = send_pause(paths, true);
    if !ok {
        ok = paths::kill(pid, libc::SIGUSR2);
    }
    if !ok {
        util::notify("Reel", Some("This recorder cannot pause."), "critical");
        anyhow::bail!("This recorder cannot pause.");
    }

    let now = paths::now_secs();
    let mut next = current;
    state::set(&mut next, "status", json!("paused"));
    state::set(&mut next, "paused", json!(true));
    state::set(&mut next, "pausedAt", json!(now));
    state::write(paths, &next)?;
    Ok(())
}

pub fn resume(paths: &Paths) -> Result<()> {
    let _lock = Lock::acquire(paths)?;
    let current = state::read_or_idle(paths);
    let status = state::field_str(&current, "status").unwrap_or_default();
    if status == "recording" {
        return Ok(());
    }
    let pid = state::field_i64(&current, "pid").unwrap_or(0) as i32;
    if status != "paused" || !paths::alive(pid) {
        anyhow::bail!("Reel is not paused.");
    }

    let mut ok = send_pause(paths, false);
    if !ok {
        ok = paths::kill(pid, libc::SIGUSR2);
    }
    if !ok {
        util::notify("Reel", Some("This recorder cannot resume."), "critical");
        anyhow::bail!("This recorder cannot resume.");
    }

    let now = paths::now_secs();
    let paused_total = state::field_i64(&current, "pausedTotal").unwrap_or(0);
    let paused_at = state::field_i64(&current, "pausedAt").unwrap_or(0);
    let mut next = current;
    state::set(&mut next, "status", json!("recording"));
    state::set(&mut next, "paused", json!(false));
    state::set(&mut next, "pausedAt", json!(0));
    state::set(
        &mut next,
        "pausedTotal",
        json!(paused_total + now - paused_at),
    );
    state::write(paths, &next)?;
    Ok(())
}

pub fn toggle_pause(paths: &Paths) -> Result<()> {
    match state::status(paths).as_str() {
        "paused" => resume(paths),
        "recording" => pause(paths),
        _ => anyhow::bail!("Reel is not recording."),
    }
}

pub fn stop(paths: &Paths, discard: bool) -> Result<()> {
    let (status, pid, file, slowmo, started_at, target, target_kind, target_label);
    {
        let _lock = Lock::acquire(paths)?;
        let current = state::read_or_idle(paths);
        status = state::field_str(&current, "status").unwrap_or_default();
        pid = state::field_i64(&current, "pid").unwrap_or(0) as i32;
        file = state::field_str(&current, "file").unwrap_or_default();
        slowmo = state::field_str(&current, "slowmo").unwrap_or_else(|| "off".into());
        started_at = state::field_i64(&current, "startedAt").unwrap_or_else(paths::now_secs);
        target = state::field_str(&current, "target").unwrap_or_default();
        target_kind = state::field_str(&current, "targetKind").unwrap_or_default();
        target_label = state::field_str(&current, "targetLabel").unwrap_or_default();

        if status == "starting" {
            let _ = paths::write_atomic(&paths.cancel, "");
            let _ = util::run_ok("pkill", &["-x", "slurp"]);
            return Ok(());
        }
        if status != "recording" && status != "paused" {
            anyhow::bail!("Reel is not recording.");
        }
        if !file.is_empty() && Path::new(&file).is_file() && !discard {
            let next = state::with_status(current, "finalizing");
            let _ = state::write(paths, &next);
        }
    }

    paths::kill(pid, libc::SIGINT);
    if !wait_for_exit(pid, Duration::from_secs(8)) {
        paths::log(paths, "stop: encoder ignored SIGINT, escalating to SIGKILL");
        paths::kill(pid, libc::SIGKILL);
    }

    restore_cursor_mode(paths);
    stop_zoom_tracker(paths);
    webcam::cleanup(paths);
    paths::remove_quiet(&paths.pid_file);
    paths::remove_quiet(&paths.socket);

    if discard {
        if !file.is_empty() {
            let path = Path::new(&file);
            let stem = path.with_extension("").display().to_string();
            let ext = path
                .extension()
                .map(|e| e.to_string_lossy().to_string())
                .unwrap_or_default();
            paths::remove_quiet(path);
            paths::remove_quiet(&paths.preview_path(path));
            paths::remove_quiet(&paths::legacy_preview_path(path));
            paths::remove_quiet(Path::new(&format!("{stem}-processed.{ext}")));
            paths::remove_quiet(&path.with_extension("zoom.json"));
        }
        let _ = state::write(paths, &state::idle_preserving_last(paths));
        return Ok(());
    }

    if file.is_empty() || !Path::new(&file).is_file() {
        let message = "The recording did not produce a file.";
        let _ = state::write(
            paths,
            &state::idle_with_error_preserving_last(paths, message),
        );
        paths::remove_quiet(&paths.starting);
        paths::remove_quiet(&paths.cancel);
        util::notify("Reel", Some(message), "critical");
        anyhow::bail!("{message}");
    }

    let size = fs::metadata(&file).map(|m| m.len()).unwrap_or(0);
    if size == 0 {
        let message = "The recording produced an empty file.";
        let _ = state::write(
            paths,
            &state::idle_with_error_preserving_last(paths, message),
        );
        paths::remove_quiet(&paths.starting);
        paths::remove_quiet(&paths.cancel);
        util::notify("Reel", Some(message), "critical");
        anyhow::bail!("{message}");
    }

    // Do not render a preview here: it delayed the stop response for every
    // take. The finalize child renders one from the raw capture before its
    // save pass, so a failed or interrupted save still leaves a thumbnail.
    let preview = paths.preview_path(Path::new(&file));

    let job = util::spawn_finalize(
        paths,
        Path::new(&file),
        &slowmo,
        &preview,
        started_at,
        &target,
        &target_kind,
        &target_label,
    );
    match job {
        Ok(job_pid) => paths::log(paths, &format!("stop: finalize job {job_pid} for {file}")),
        Err(err) => paths::log(paths, &format!("stop: cannot spawn finalize: {err}")),
    }
    paths::remove_quiet(&paths.starting);
    paths::remove_quiet(&paths.cancel);
    println!("{file}");
    Ok(())
}

pub fn toggle(paths: &Paths, options: StartArgs) -> Result<bool> {
    match state::status(paths).as_str() {
        "starting" => {
            let _lock = Lock::acquire(paths)?;
            let _ = paths::write_atomic(&paths.cancel, "");
            let _ = util::run_ok("pkill", &["-x", "slurp"]);
            let _ = state::write(paths, &state::idle_preserving_last(paths));
            Ok(true)
        }
        "recording" | "paused" => {
            stop(paths, false)?;
            Ok(true)
        }
        _ => start(paths, options),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_serialize_for_the_state_file() {
        let settings = Settings {
            desktop: true,
            mic: false,
            cursor: true,
            webcam: "off".into(),
            webcam_device: String::new(),
            slowmo: "off".into(),
            countdown: 3,
            fps: 60,
            quality: "very_high".into(),
            codec: "auto".into(),
            resolution: "0x0".into(),
            container: "mp4".into(),
            output_dir: "/tmp".into(),
            filename_prefix: "reel-".into(),
            auto_zoom: zoom::Settings::default(),
        };
        let value = settings.to_json();
        assert_eq!(value["fps"], 60);
        assert_eq!(value["desktopAudio"], true);
        assert_eq!(value["microphoneAudio"], false);
        assert_eq!(value["focusAutoZoom"], true);
    }

    #[test]
    fn quality_levels_keep_words_and_accept_old_numbers() {
        assert_eq!(quality_level("very_high"), "very_high");
        assert_eq!(quality_level("8000"), "high");
        assert_eq!(quality_level("nonsense"), "very_high");
        assert_eq!(quality_preset("very_low"), "medium");
        assert_eq!(quality_preset("high"), "high");
        assert_eq!(quality_qp_override("low", "h264"), Some(40));
        assert_eq!(quality_qp_override("low", "av1"), Some(160));
        assert_eq!(quality_qp_override("medium", "h264"), None);
    }

    #[test]
    fn filename_prefix_is_one_safe_component() {
        assert_eq!(sanitize_prefix(""), "reel-");
        assert_eq!(sanitize_prefix("Demo"), "Demo-");
        assert_eq!(sanitize_prefix("Bad/name"), "Bad-name-");
        assert_eq!(sanitize_prefix("Already-"), "Already-");
        assert_eq!(sanitize_prefix("..."), "reel-");
    }

    #[test]
    fn wait_until_returns_as_soon_as_ready() {
        let started = Instant::now();
        assert!(wait_until(Duration::from_secs(5), || true));
        assert!(started.elapsed() < Duration::from_millis(500));
    }

    #[test]
    fn wait_until_keeps_polling_until_ready() {
        let mut calls = 0;
        assert!(wait_until(Duration::from_secs(5), || {
            calls += 1;
            calls == 3
        }));
        assert_eq!(calls, 3);
    }

    #[test]
    fn wait_until_gives_up_at_the_deadline() {
        let started = Instant::now();
        assert!(!wait_until(Duration::from_millis(20), || false));
        assert!(started.elapsed() >= Duration::from_millis(20));
    }
}
