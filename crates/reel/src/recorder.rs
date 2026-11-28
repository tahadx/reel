use crate::cli::StartArgs;
use crate::config;
use crate::paths::{self, Lock, Paths};
use crate::project;
use crate::state;
use crate::target;
use crate::telemetry;
use crate::util;
use crate::webcam;
use crate::wf_recorder;
use anyhow::{Context, Result};
use serde_json::{json, Value};
use std::fs::{self, OpenOptions};
use std::os::unix::process::ExitStatusExt;
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
    noise_cancellation: bool,
    desktop_device: String,
    microphone_device: String,
    cursor: bool,
    webcam: String,
    webcam_device: String,
    countdown: i64,
    fps: i64,
    quality: String,
    codec: String,
    resolution: String,
    container: String,
    output_dir: String,
    filename_prefix: String,
    manual_zoom: bool,
    zoom_max: f64,
    auto_zoom: bool,
    auto_zoom_depth: f64,
    click_effects: String,
}

impl Settings {
    fn to_json(&self) -> Value {
        json!({
            "desktopAudio": self.desktop,
            "microphoneAudio": self.mic,
            "noiseCancellation": self.noise_cancellation,
            "desktopDevice": self.desktop_device,
            "microphoneDevice": self.microphone_device,
            "cursor": self.cursor,
            "webcam": self.webcam,
            "webcamDevice": self.webcam_device,
            "countdown": self.countdown,
            "fps": self.fps,
            "quality": self.quality,
            "codec": self.codec,
            "resolution": self.resolution,
            "container": self.container,
            "outputDir": self.output_dir,
            "filenamePrefix": self.filename_prefix,
            "manualZoom": self.manual_zoom,
            "zoomMax": self.zoom_max,
            "autoZoom": self.auto_zoom,
            "autoZoomDepth": self.auto_zoom_depth,
            "clickEffects": self.click_effects,
        })
    }

    fn effective_desktop_device(&self) -> &str {
        if self.desktop_device.is_empty() {
            "default_output"
        } else {
            &self.desktop_device
        }
    }

    fn effective_microphone_device(&self) -> &str {
        if self.microphone_device.is_empty() {
            "default_input"
        } else {
            &self.microphone_device
        }
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

/// AMD Polaris GPUs can hang their VCE 3.4 encode ring mid-recording and lose
/// compositor VRAM when the ensuing GPU reset fails (#9555). The recorder's
/// `-fallback-cpu-encoding` cannot recover from a GPU that has already hung, so
/// avoid starting hardware encoding there at all. These are amdgpu's
/// CHIP_POLARIS10, 11, 12 and CHIP_VEGAM ids, read from sysfs so probing does
/// not wake a suspended dGPU.
const POLARIS_IDS: &[&str] = &[
    "0x67c0", "0x67c1", "0x67c2", "0x67c4", "0x67c7", "0x67c8", "0x67c9", "0x67ca", "0x67cc",
    "0x67cf", "0x67d0", "0x67d4", "0x67df", "0x6fdf", "0x67e0", "0x67e1", "0x67e3", "0x67e7",
    "0x67e8", "0x67e9", "0x67eb", "0x67ef", "0x67ff", "0x6980", "0x6981", "0x6985", "0x6986",
    "0x6987", "0x698f", "0x6995", "0x6997", "0x699f", "0x694c", "0x694e", "0x694f",
];

fn polaris_present(root: &Path) -> bool {
    let Ok(devices) = fs::read_dir(root) else {
        return false;
    };
    for device in devices.flatten() {
        let path = device.path();
        let Ok(vendor) = fs::read_to_string(path.join("vendor")) else {
            continue;
        };
        if vendor.trim() != "0x1002" {
            continue;
        }
        let Ok(id) = fs::read_to_string(path.join("device")) else {
            continue;
        };
        if POLARIS_IDS.contains(&id.trim()) {
            return true;
        }
    }
    false
}

fn needs_cpu_encoder() -> bool {
    let root = std::env::var_os("REEL_PCI_DEVICES_PATH")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::path::PathBuf::from("/sys/bus/pci/devices"));
    polaris_present(&root)
}

/// Whether manual zoom should use the stable push camera for this take.
///
/// Push is opt-in. Auto zoom and click effects need the live frame's click
/// positions, which are exact only under the attached follow camera, so those
/// takes keep follow even when the user chose push.
fn manual_zoom_push(follow: bool, auto_zoom: bool, click_effects: &str, stored: &Value) -> bool {
    follow
        && !auto_zoom
        && click_effects == "off"
        && !config::zoom_follows_pointer(stored.get("zoomFollow"))
}

/// The take's auto zoom: the panel's per-take argument first, then reel.json.
/// The panel passes what the user is looking at, so a stale `autoZoom` left in
/// the file can never zoom a take the panel says is off.
fn auto_zoom_choice(option: &Option<String>, stored: &Value) -> bool {
    match option
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        Some(value) => config::auto_zoom(&json!({ "autoZoom": value })),
        None => config::auto_zoom(stored),
    }
}

/// The take's click effect: the per-take argument first, then reel.json. A
/// snapshot is stored with the take so a setting changed mid-recording cannot
/// restyle the save.
fn click_effects_choice(option: &Option<String>, stored: &Value) -> String {
    match option
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        Some(value) => config::click_effects(&json!({ "clickEffects": value })),
        None => config::click_effects(stored),
    }
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
        "off",
    ));
    let mic = util::normalize_switch(&effective(
        &options.microphone_audio,
        &stored,
        "microphoneAudio",
        "off",
    ));
    let noise_cancellation = util::normalize_switch(&effective(
        &options.noise_cancellation,
        &stored,
        "noiseCancellation",
        "off",
    ));
    let desktop_device = effective(&options.desktop_device, &stored, "desktopDevice", "");
    let microphone_device = effective(&options.microphone_device, &stored, "microphoneDevice", "");
    let cursor = util::normalize_switch(&effective(&options.cursor, &stored, "cursor", "on"));
    let webcam = util::pick_choice(
        &effective(&options.webcam, &stored, "webcam", "off"),
        "off",
        &["off", "small", "medium", "large"],
    );
    let webcam_device = effective(&options.webcam_device, &stored, "webcamDevice", "");
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
    let manual_zoom = util::normalize_switch(&effective(
        &options.manual_zoom,
        &stored,
        "manualZoom",
        "on",
    ));
    let zoom_max = options
        .zoom_max
        .or_else(|| config::zoom_max_value(stored.get("zoomMax")))
        .unwrap_or(2.0)
        .clamp(1.1, 4.0);
    // Auto zoom is a save-pass choice, but it is snapshotted with the take so
    // changing the setting mid-recording cannot change this save.
    let auto_zoom = auto_zoom_choice(&options.auto_zoom, &stored);
    let auto_zoom_depth = config::auto_zoom_depth(&stored);
    let click_effects = click_effects_choice(&options.click_effects, &stored);

    Ok(Settings {
        desktop,
        mic,
        noise_cancellation,
        desktop_device,
        microphone_device,
        cursor,
        webcam,
        webcam_device,
        countdown,
        fps,
        quality,
        codec,
        resolution,
        container,
        output_dir: dir,
        filename_prefix,
        manual_zoom,
        zoom_max,
        auto_zoom,
        auto_zoom_depth,
        click_effects,
    })
}

/// The capture engine for a take. `Gpu` is preferred exactly as before; `Wf`
/// is Omarchy's CPU fallback, used when the GPU recorder is missing or dies
/// before its first frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Backend {
    Gpu,
    Wf,
}

impl Backend {
    fn id(self) -> &'static str {
        match self {
            Backend::Gpu => "gpu-screen-recorder",
            Backend::Wf => "wf-recorder",
        }
    }

    fn can_pause(self) -> bool {
        matches!(self, Backend::Gpu)
    }
}

/// Recorders in the order they are tried, mirroring Omarchy's own script.
/// Neither installed resolves to an empty list so `require_tools` can report
/// both names at once.
fn recorder_backends() -> Vec<Backend> {
    let mut backends = Vec::new();
    if util::have("gpu-screen-recorder") {
        backends.push(Backend::Gpu);
    }
    if wf_recorder::available() {
        backends.push(Backend::Wf);
    }
    backends
}

fn require_tools() -> Result<Vec<Backend>> {
    let backends = recorder_backends();
    if backends.is_empty() {
        let message = "No screen recorder is installed. Install gpu-screen-recorder with Omarchy's package installer, or wf-recorder for CPU capture.";
        util::notify("Reel", Some(message), "critical");
        anyhow::bail!(message);
    }
    for tool in ["ffmpeg", "ffprobe"] {
        if !util::have(tool) {
            anyhow::bail!("{tool} is missing.");
        }
    }
    Ok(backends)
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

#[derive(Debug, Clone, serde::Serialize)]
pub struct AudioDevice {
    pub id: String,
    pub label: String,
    pub kind: String,
}

pub fn available_audio_raw() -> Option<String> {
    // Bounded: enumerating devices must never stall a take.
    util::run_stdout_within(
        "gpu-screen-recorder",
        &["--list-audio-devices"],
        Duration::from_secs(3),
    )
}

/// Audio ids in the shape the panel already parses. The GPU recorder's list is
/// preferred because its ids are what a GPU take records; without it, the CPU
/// fallback's PulseAudio sources are the same node names.
fn audio_devices_raw() -> Option<String> {
    if let Some(raw) = available_audio_raw() {
        return Some(raw);
    }
    if !wf_recorder::available() {
        return None;
    }
    let devices = wf_recorder::audio_devices();
    if devices.is_empty() {
        return None;
    }
    Some(
        devices
            .into_iter()
            .map(|(id, label, _kind)| format!("{id}|{label}"))
            .collect::<Vec<_>>()
            .join("\n"),
    )
}

pub fn list_audio_devices() -> Vec<AudioDevice> {
    let Some(raw) = audio_devices_raw() else {
        return Vec::new();
    };
    raw.lines()
        .filter_map(|line| {
            let line = line.trim();
            if line.is_empty() {
                return None;
            }
            let (id, label) = match line.split_once('|') {
                Some((id, label)) => (id.trim().to_string(), label.trim().to_string()),
                None => (line.to_string(), line.to_string()),
            };
            if id.is_empty() {
                return None;
            }
            let kind =
                if id == "default_output" || id.ends_with(".monitor") || id.contains("output") {
                    "output"
                } else if id == "default_input"
                    || id.contains("input")
                    || id.contains("source")
                    || id.starts_with("alsa_input")
                    || id.starts_with("bluez_input")
                {
                    "input"
                } else {
                    "unknown"
                }
                .to_string();
            Some(AudioDevice { id, label, kind })
        })
        .collect()
}

fn device_exists(devices: &str, id: &str) -> bool {
    devices.lines().any(|line| {
        let line = line.trim();
        line == id
            || line
                .strip_prefix(id)
                .is_some_and(|rest| rest.starts_with('|'))
    })
}

fn validate_audio(settings: &Settings) -> Result<()> {
    if !settings.desktop && !settings.mic {
        return Ok(());
    }
    let Some(devices) = audio_devices_raw() else {
        return Ok(());
    };
    if settings.desktop {
        let id = settings.effective_desktop_device();
        if !device_exists(&devices, id) {
            if settings.desktop_device.is_empty() {
                anyhow::bail!(
                    "No system audio output is available. Choose another audio source or turn desktop audio off."
                );
            }
            anyhow::bail!(
                "The selected desktop audio device is not available: {id}. Choose another device or turn desktop audio off."
            );
        }
    }
    if settings.mic {
        let id = settings.effective_microphone_device();
        if !device_exists(&devices, id) {
            if settings.microphone_device.is_empty() {
                anyhow::bail!(
                    "No microphone is available. Choose another audio source or turn the microphone off."
                );
            }
            anyhow::bail!(
                "The selected microphone device is not available: {id}. Choose another device or turn the microphone off."
            );
        }
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

/// Keep a desktop-audio monitor stream open while the target is being chosen.
/// PipeWire suspends an idle sink monitor; when gpu-screen-recorder opens it
/// at the last moment, the first frame can stall for seconds while the sink
/// resumes. Starting the stream here overlaps that wake-up with the picker.
struct AudioWarmup {
    child: std::process::Child,
    started: Instant,
}

impl AudioWarmup {
    fn start(settings: &Settings) -> Option<Self> {
        if !settings.desktop {
            return None;
        }
        let target = desktop_monitor(settings)?;
        // parec actually pulls samples from a monitor source, which resumes
        // it. pw-cat (observed on PipeWire 1.6) attaches but leaves the
        // source suspended, so it is only the fallback.
        let mut command = if util::have("parec") {
            let mut command = Command::new("parec");
            command
                .arg(format!("--device={target}"))
                .arg("--raw")
                .arg("--format=s16le")
                .arg("--rate=48000")
                .arg("--channels=2");
            command
        } else if util::have("pw-cat") {
            let mut command = Command::new("pw-cat");
            command
                .arg("--record")
                .arg("--target")
                .arg(&target)
                .arg("/dev/null");
            command
        } else {
            return None;
        };
        let child = command
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .ok()?;
        util::debug(&format!("start: warming desktop audio on {target}"));
        Some(Self {
            child,
            started: Instant::now(),
        })
    }

    /// Long enough for a suspended sink to resume. When the picker took
    /// longer than this the stream is already warm and this returns at once.
    fn ready(&mut self) {
        let minimum = Duration::from_millis(350);
        let elapsed = self.started.elapsed();
        if elapsed < minimum {
            thread::sleep(minimum - elapsed);
        }
    }
}

impl Drop for AudioWarmup {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// The PipeWire source that matches the chosen desktop audio device. A
/// monitor source is used directly; a sink is mapped to its monitor.
fn desktop_monitor(settings: &Settings) -> Option<String> {
    let device = settings.effective_desktop_device();
    if device == "default_output" {
        let sink = util::run_stdout_within("pactl", &["get-default-sink"], Duration::from_secs(1))?;
        if sink.is_empty() {
            return None;
        }
        return Some(format!("{sink}.monitor"));
    }
    // Application capture and combined sources have no single monitor to
    // warm; the encoder's own path is still correct, just not pre-warmed.
    if device.starts_with("app:") || device.contains('|') {
        return None;
    }
    if device.ends_with(".monitor") {
        Some(device.to_string())
    } else {
        Some(format!("{device}.monitor"))
    }
}

fn cancelled(paths: &Paths) -> bool {
    paths.cancel.exists()
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

/// Whether the installed recorder can tell us when the first frame lands.
/// Older builds only create the container, which is not proof that capture
/// has begun.
fn first_frame_marker_supported() -> bool {
    static SUPPORTED: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *SUPPORTED.get_or_init(|| {
        util::run_stdout_within_any("gpu-screen-recorder", &["--help"], Duration::from_secs(2))
            .is_some_and(|help| help.contains("write-first-frame-ts"))
    })
}

fn capture_ready(file: &Path, first_frame: Option<&Path>) -> bool {
    match first_frame {
        Some(marker) => marker.exists(),
        None => file.exists(),
    }
}

/// Read the monotonic microseconds of the first captured frame from the
/// recorder's marker. The file is written by gpu-screen-recorder next to the
/// capture and contains `monotonic_microsec<TAB>realtime_microsec`. A retry
/// covers the moment between the file appearing and its contents landing.
fn read_first_frame_us(marker: &Path) -> Option<u64> {
    for _ in 0..20 {
        if let Ok(text) = fs::read_to_string(marker) {
            // The file starts with a column header; the timestamp is the first
            // numeric line.
            for line in text.lines() {
                if let Some(field) = line.split_whitespace().next() {
                    if let Ok(value) = field.parse::<u64>() {
                        return Some(value);
                    }
                }
            }
        }
        thread::sleep(Duration::from_millis(5));
    }
    None
}

/// Wait until the recorder has really captured its first frame, exited, or the
/// take was cancelled. The container alone is written before any pixels, so
/// only the first-frame marker proves the take is live. A cold encoder can
/// take several seconds to get there.
///
/// The result says whether the capture is live and, when the recorder provides
/// it, the first frame's monotonic timestamp.
fn wait_for_capture_file(
    paths: &Paths,
    file: &Path,
    first_frame: Option<&Path>,
    pid: i32,
    timeout: Duration,
) -> (bool, Option<u64>) {
    // A dead recorder or an aborted take will never produce the marker.
    wait_until(timeout, || {
        capture_ready(file, first_frame) || cancelled(paths) || !paths::alive(pid)
    });
    let ready = capture_ready(file, first_frame);
    let first_frame_us = if ready {
        first_frame.and_then(read_first_frame_us)
    } else {
        None
    };
    if let Some(marker) = first_frame {
        paths::remove_quiet(marker);
    }
    (ready, first_frame_us)
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

/// Kill a recorder that never produced a frame. gpu-screen-recorder
/// segfaults in its encoder flush when interrupted before its first frame;
/// there is no take to save at that point, so SIGKILL is both safe and quiet.
fn kill_child(child: &mut std::process::Child) {
    let pid = child.id() as i32;
    paths::kill(pid, libc::SIGKILL);
    let _ = wait_for_child(child, Duration::from_secs(1));
}

fn fail_start(paths: &Paths, message: &str) {
    crate::zoom::disable_recording_zoom(paths);
    let _ = state::write(
        paths,
        &state::idle_with_error_preserving_last(paths, message),
    );
    paths::remove_quiet(&paths.starting);
    util::notify("Reel", Some(message), "critical");
    paths::log(paths, &format!("error: {message}"));
}

fn start_guard(paths: &Paths) -> Result<String> {
    start_guard_within(paths, Duration::from_secs(12))
}

fn start_guard_within(paths: &Paths, wait: Duration) -> Result<String> {
    // "finalizing" is the short encoder shutdown before the save job exists.
    // A record press during that beat waits for it instead of disabling the
    // button: the lock is released between checks so `stop` can finish.
    let deadline = Instant::now() + wait;
    loop {
        let lock = Lock::acquire(paths)?;
        // Clear a dead job first so a crashed save cannot leave stale
        // bookkeeping beside the engine. Live save jobs do not block a take.
        state::reconcile(paths);
        let current = state::status(paths);
        match current.as_str() {
            "recording" | "paused" | "starting" => {
                anyhow::bail!("Reel is already busy. Stop the current take first.")
            }
            "finalizing" => {
                drop(lock);
                if Instant::now() >= deadline {
                    anyhow::bail!("Reel is still finishing the last take. Try again in a moment.");
                }
                thread::sleep(Duration::from_millis(100));
            }
            _ => {
                // Claim the take while the lock is still held. Releasing it
                // first let two closely spaced presses both see "idle", run
                // the picker, and launch two encoders for the same target.
                // The second press now sees this reservation and is refused.
                paths::write_pid(&paths.starting, std::process::id())?;
                state::write(
                    paths,
                    &json!({
                        "status": "starting",
                        "stage": "select",
                        "startedAt": paths::now_secs(),
                    }),
                )?;
                paths::remove_quiet(&paths.cancel);
                drop(lock);
                return Ok(current);
            }
        }
    }
}

/// Resolve the chosen target. An explicit `--target` skips the picker; a
/// cancelled pick is `Ok(None)`.
fn choose_target(paths: &Paths, options: &StartArgs) -> Result<Option<target::Choice>> {
    // `start_guard` already reserved the take and wrote the "select" state
    // while holding the lock.
    let picked = match options.target.as_deref() {
        Some(raw) => {
            util::debug(&format!("start: explicit target {raw}"));
            target::resolve(raw).map(Some)
        }
        None => {
            util::debug("start: opening the capture picker");
            target::pick()
        }
    };
    let choice = match picked {
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
    while Path::new(&candidate).exists()
        || paths::untrimmed_path(Path::new(&candidate)).exists()
        || paths::capture_path(Path::new(&candidate)).exists()
    {
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

/// The GPU recorder's arguments. Kept separate so the fallback loop can build
/// them only for a GPU attempt.
fn gpu_capture_args(
    settings: &Settings,
    target: &str,
    capture_file: &str,
    socket: &Path,
    write_first_frame: bool,
    cpu_encoder: bool,
) -> Result<Vec<String>> {
    let mut capture: Vec<String> = vec![
        "-w".into(),
        String::new(),
        "-f".into(),
        settings.fps.to_string(),
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
    ];
    if write_first_frame {
        capture.extend(["-write-first-frame-ts".into(), "yes".into()]);
    }
    if cpu_encoder {
        capture.extend(["-encoder".into(), "cpu".into()]);
    }
    capture.extend([
        "-ipc".into(),
        socket.display().to_string(),
        "-o".into(),
        capture_file.to_string(),
    ]);

    match target {
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
        devices.push_str(settings.effective_desktop_device());
    }
    if let Some(qp) = quality_qp_override(&settings.quality, &settings.codec) {
        capture.extend(["-ffmpeg-video-opts".into(), format!("qp={qp}")]);
    }
    if settings.mic {
        if !devices.is_empty() {
            devices.push('|');
        }
        devices.push_str(settings.effective_microphone_device());
    }
    if !devices.is_empty() {
        capture.extend(["-a".into(), devices, "-ac".into(), "aac".into()]);
    }
    Ok(capture)
}

/// A recorder that really produced a frame, plus the clocks its telemetry
/// listener needs.
struct Launched {
    child: std::process::Child,
    backend: Backend,
    pid: u32,
    t0: Instant,
    t0_ns: u64,
    first_frame_us: Option<u64>,
    estimate: Option<f64>,
}

/// Count down, start the webcam, launch the encoder, and move to `recording`
/// only after the encoder really captured its first frame.
fn run_take(
    paths: &Paths,
    settings: &Settings,
    choice: &target::Choice,
    warmup: &mut Option<AudioWarmup>,
    backends: &[Backend],
    gpu_first_frame_marker: bool,
) -> Result<bool> {
    let target = &choice.target;
    if target == "portal" && !backends.contains(&Backend::Gpu) {
        let message = "Portal capture needs gpu-screen-recorder. The CPU fallback can record a monitor or an area instead.";
        fail_start(paths, message);
        return Err(anyhow::anyhow!(message));
    }
    if let Err(err) = target::validate_target(target) {
        fail_start(paths, &err);
        return Err(anyhow::anyhow!(err));
    }
    // A portal fallback has a warning the picker never got to show, because
    // the panel closes before recording. Say why the system dialog appears.
    if choice.kind == "portal" && !choice.warning.is_empty() {
        util::notify(
            "Reel",
            Some("This output has no KMS connector, so Reel records through the system portal picker."),
            "normal",
        );
    }

    // The target is chosen now, but the webcam may still take a moment to
    // open. Replace the "select" stage so the bar stops saying it is waiting
    // for a selection while the take is being prepared.
    {
        let _lock = Lock::acquire(paths).ok();
        let _ = state::write(
            paths,
            &json!({
                "status": "starting",
                "stage": "prepare",
                "target": target,
                "targetKind": &choice.kind,
                "targetLabel": &choice.label,
                "targetWarning": &choice.warning,
                "monitor": &choice.monitor,
                "settings": settings.to_json(),
                "startedAt": paths::now_secs(),
            }),
        );
    }

    paths::remove_quiet(&paths.cancel);

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
    // Always record into a hidden capture name. The save pass publishes the
    // ordinary take name only after the camera and audio pass, so the take the
    // user can see is never the raw, un-zoomed capture.
    let capture_file = paths::capture_path(Path::new(&file)).display().to_string();
    // gpu-screen-recorder writes `<output>.ts` when the first frame is
    // encoded. It is the only honest signal that capture has begun: the
    // container appears earlier, before any pixels exist.
    let first_frame = gpu_first_frame_marker.then(|| format!("{capture_file}.ts"));
    let capture_fps = settings.fps;

    // Keep the previous take's log instead of truncating it; a failed save is
    // often investigated after the next take starts. This rotates only when
    // the log grows large.
    paths::reset_log(paths);

    // Pointer telemetry follows the existing "Record the pointer" setting.
    let capture_space = if settings.cursor {
        match telemetry::CaptureSpace::resolve(
            target,
            &choice.kind,
            &choice.monitor,
            &settings.resolution,
        ) {
            Ok(space) => Some(space),
            Err(err) => {
                paths::log(
                    paths,
                    &format!(
                        "telemetry: cannot map {target} ({}); the take continues without pointer data",
                        err.code()
                    ),
                );
                None
            }
        }
    } else {
        None
    };
    // A new take always starts with an empty runtime file, so a failed save
    // from the previous take cannot be mistaken for this one.
    telemetry::clear(paths);
    let mut prepared = capture_space
        .as_ref()
        .map(|space| telemetry::Prepared::new(paths, space));
    let telemetry_ready = prepared
        .as_ref()
        .map(telemetry::Prepared::available)
        .unwrap_or(false);
    if settings.cursor && !telemetry_ready {
        let code = prepared
            .as_ref()
            .and_then(telemetry::Prepared::error)
            .or(capture_space
                .as_ref()
                .map(|_| telemetry::SourceError::NoDevice))
            .unwrap_or(telemetry::SourceError::UnsupportedTarget);
        paths::log(
            paths,
            &format!(
                "telemetry: unavailable ({}); the take continues without pointer data{}",
                code.code(),
                code.hint()
                    .map(|hint| format!(" — {hint}"))
                    .unwrap_or_default()
            ),
        );
    }

    paths::remove_quiet(&paths.socket);
    paths::remove_quiet(&paths.pid_file);

    let log = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&paths.log)
        .context("cannot open the Reel log")?;
    // Wait for the audio monitor to resume before the encoder opens it, so a
    // suspended sink cannot delay the first frame. A picker interaction
    // usually covers this wait completely.
    if let Some(warmup) = warmup.as_mut() {
        warmup.ready();
    }

    // Try the GPU recorder first, then the CPU fallback, exactly as Omarchy's
    // own recorder does. A recorder that dies or captures no frame releases
    // the take to the next one; only when every backend fails does the start
    // fail.
    let mut hardware_cursor: Option<HardwareCursor> = None;
    let mut last_error: Option<String> = None;
    let mut launched: Option<Launched> = None;
    let cpu_encoder = backends.contains(&Backend::Gpu) && needs_cpu_encoder();
    if cpu_encoder {
        paths::log(
            paths,
            "capture: AMD Polaris detected; encoding on the CPU to avoid a VCE hang",
        );
    }

    for backend in backends {
        if *backend == Backend::Wf && target == "portal" {
            continue;
        }

        paths::remove_quiet(Path::new(&capture_file));
        paths::remove_quiet(Path::new(&format!("{capture_file}.ts")));

        let (args, marker) = match backend {
            Backend::Gpu => {
                if !settings.cursor && target != "portal" {
                    hardware_cursor = HardwareCursor::force(paths);
                }
                (
                    gpu_capture_args(
                        settings,
                        target,
                        &capture_file,
                        &paths.socket,
                        first_frame.is_some(),
                        cpu_encoder,
                    )?,
                    first_frame.as_deref(),
                )
            }
            Backend::Wf => {
                let desktop = settings
                    .desktop
                    .then(|| settings.effective_desktop_device());
                let microphone = settings.mic.then(|| settings.effective_microphone_device());
                let audio = wf_recorder::audio(paths, desktop, microphone);
                for side in &audio.missing {
                    util::notify(
                        "Reel",
                        Some(&format!(
                            "Recording without {side} audio. The CPU fallback could not find that source."
                        )),
                        "normal",
                    );
                }
                if !settings.cursor {
                    // wf-recorder asks the compositor for the overlay cursor
                    // unconditionally, so this take cannot honor cursor=off.
                    paths::log(
                        paths,
                        "capture: wf-recorder always includes the pointer; cursor=off cannot be honored",
                    );
                    util::notify(
                        "Reel",
                        Some("The CPU fallback always includes the pointer in this take."),
                        "normal",
                    );
                }
                let args = match wf_recorder::capture_args(&wf_recorder::Capture {
                    target,
                    fps: capture_fps,
                    codec: &settings.codec,
                    quality: &settings.quality,
                    file: &capture_file,
                    audio: audio.device.as_deref(),
                }) {
                    Ok(args) => args,
                    Err(err) => {
                        wf_recorder::cleanup_audio(paths);
                        last_error = Some(err);
                        continue;
                    }
                };
                (args, None)
            }
        };

        let executable = match backend {
            Backend::Gpu => "gpu-screen-recorder",
            Backend::Wf => wf_recorder::BINARY,
        };
        util::debug(&format!(
            "capture: launching {executable} for {capture_file}"
        ));
        paths::log(
            paths,
            &format!(
                "capture: backend={} target={} fps={} cursor={} telemetry={} autoZoom={} output={}",
                backend.id(),
                target,
                capture_fps,
                if settings.cursor { "on" } else { "off" },
                if telemetry_ready { "on" } else { "off" },
                if settings.auto_zoom { "on" } else { "off" },
                capture_file
            ),
        );

        let mut command = Command::new(executable);
        command
            .args(&args)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::from(log.try_clone()?));

        // T0 is the moment the encoder launches. Both T0 and every event use
        // CLOCK_MONOTONIC through `Instant`.
        let (t0, t0_ns) = telemetry::clock_start();
        let mut child = match command.spawn() {
            Ok(child) => child,
            Err(err) => {
                last_error = Some(format!("{} did not start: {err}", backend.id()));
                paths::log(
                    paths,
                    &format!("capture: {} did not start: {err}", backend.id()),
                );
                if *backend == Backend::Wf {
                    wf_recorder::cleanup_audio(paths);
                }
                if let Some(cursor) = hardware_cursor.take() {
                    cursor.restore(paths);
                }
                continue;
            }
        };
        let pid = child.id();
        util::debug(&format!("capture: recorder pid {pid}"));
        let _ = paths::write_pid(&paths.pid_file, pid);

        // The recorder is running now, so the take has started even though its
        // first file can take a moment to appear (a cold GPU can need a second
        // or more). Announce the capture stage and start the clock here instead
        // of leaving the bar on the "starting" placeholder until the file
        // shows up.
        {
            let _lock = Lock::acquire(paths).ok();
            let _ = state::write(
                paths,
                &json!({
                    "status": "starting",
                    "stage": "warmup",
                    "pid": pid,
                    "file": &capture_file,
                    "output": &file,
                    "target": target,
                    "targetKind": &choice.kind,
                    "targetLabel": &choice.label,
                    "targetWarning": &choice.warning,
                    "monitor": &choice.monitor,
                    "backend": backend.id(),
                    "canPause": backend.can_pause(),
                    "telemetry": telemetry_ready,
                    "settings": settings.to_json(),
                    "startedAt": paths::now_secs(),
                }),
            );
        }

        let (ready_file, first_frame_us) = wait_for_capture_file(
            paths,
            Path::new(&capture_file),
            marker.map(Path::new),
            pid as i32,
            Duration::from_secs(15),
        );
        let ready_at = Instant::now();
        // The encoder now owns the monitor; the warm-up stream is done.
        warmup.take();

        // A stop during the warmup writes the cancel marker; the take never
        // got going, so tear it down quietly instead of reporting a start
        // failure.
        if cancelled(paths) {
            if ready_file {
                stop_child(&mut child, Duration::from_secs(4));
            } else {
                kill_child(&mut child);
            }
            if *backend == Backend::Wf {
                wf_recorder::cleanup_audio(paths);
            }
            if let Some(cursor) = hardware_cursor.take() {
                cursor.restore(paths);
            }
            webcam::cleanup(paths);
            paths::remove_quiet(&paths.pid_file);
            paths::remove_quiet(&paths.socket);
            paths::remove_quiet(Path::new(&capture_file));
            paths::remove_quiet(Path::new(&file));
            let _ = state::write(paths, &state::idle_preserving_last(paths));
            paths::remove_quiet(&paths.starting);
            paths::remove_quiet(&paths.cancel);
            return Ok(false);
        }

        if !ready_file {
            // No frame was captured, so there is nothing to flush; killing it
            // also avoids the GPU encoder's shutdown crash in this state.
            kill_child(&mut child);
            if *backend == Backend::Wf {
                wf_recorder::cleanup_audio(paths);
            }
            if let Some(cursor) = hardware_cursor.take() {
                cursor.restore(paths);
            }
            paths::remove_quiet(&paths.pid_file);
            paths::remove_quiet(Path::new(&capture_file));
            paths::remove_quiet(Path::new(&format!("{capture_file}.ts")));
            last_error = Some(format!("{} did not produce a frame", backend.id()));
            paths::log(
                paths,
                &format!(
                    "capture: {} did not produce a frame; trying the next recorder",
                    backend.id()
                ),
            );
            continue;
        }

        launched = Some(Launched {
            child,
            backend: *backend,
            pid,
            t0,
            t0_ns,
            first_frame_us,
            estimate: Some(ready_at.saturating_duration_since(t0).as_secs_f64()),
        });
        break;
    }

    let Some(Launched {
        mut child,
        backend,
        pid,
        t0,
        t0_ns,
        first_frame_us,
        estimate,
    }) = launched
    else {
        if let Some(cursor) = hardware_cursor.take() {
            cursor.restore(paths);
        }
        webcam::cleanup(paths);
        wf_recorder::cleanup_audio(paths);
        paths::remove_quiet(&paths.pid_file);
        paths::remove_quiet(&paths.socket);
        paths::remove_quiet(Path::new(&capture_file));
        paths::remove_quiet(Path::new(&file));
        let detail = last_error.unwrap_or_else(|| "The recorder did not start".into());
        let message = format!("{detail}. See {}", paths.log.display());
        fail_start(paths, &message);
        return Err(anyhow::anyhow!(message));
    };

    // Telemetry's t=0 is the encoder launch; the video's t=0 is the first
    // frame. The GPU recorder states that moment in its marker; the CPU
    // fallback has no marker, so the file appearing is the best estimate.
    let first_frame_offset = first_frame_us
        .map(|us| us as f64 / 1_000_000.0 - t0_ns as f64 / 1_000_000_000.0)
        .or(estimate);

    // The encoder is live. Start the listener when the pointer setting is on;
    // otherwise leave the take exactly as it is today.
    let mut listener = None;
    if settings.cursor {
        match (prepared.take(), capture_space.as_ref()) {
            (Some(prepared), Some(space)) if telemetry_ready => {
                listener = prepared.start(space.clone(), capture_fps, t0, t0_ns);
                if listener.is_none() {
                    paths::log(
                        paths,
                        "telemetry: listener did not start; the take has no pointer data",
                    );
                }
            }
            (Some(prepared), Some(space)) => {
                let code = prepared.error().unwrap_or(telemetry::SourceError::NoDevice);
                prepared.record_failure(space, capture_fps, code);
            }
            (_, Some(space)) => {
                let code = telemetry::SourceError::UnsupportedTarget;
                telemetry::record_unavailable(paths, space, capture_fps, code);
            }
            (_, None) => {
                let space =
                    telemetry::CaptureSpace::unsupported(&choice.kind, target, &choice.monitor);
                telemetry::record_unavailable(
                    paths,
                    &space,
                    capture_fps,
                    telemetry::SourceError::UnsupportedTarget,
                );
            }
        }
    }
    let telemetry_on = listener.is_some();
    if let (Some(listener), Some(offset)) = (listener.as_ref(), first_frame_offset) {
        if offset.is_finite() && offset >= 0.0 {
            listener.note_video_start(offset);
        }
    }

    // The encoder is live now. Show the countdown against a running capture,
    // then trim those seconds from the file so the take starts at zero.
    let countdown = settings.countdown.max(0);
    let trim_start = if countdown > 0 {
        {
            let _lock = Lock::acquire(paths).ok();
            let _ = state::write(
                paths,
                &json!({
                    "status": "starting",
                    "stage": "countdown",
                    "pid": pid,
                    "file": &capture_file,
                    "output": &file,
                    "target": target,
                    "targetKind": &choice.kind,
                    "targetLabel": &choice.label,
                    "targetWarning": &choice.warning,
                    "monitor": &choice.monitor,
                    "backend": backend.id(),
                    "canPause": backend.can_pause(),
                    "telemetry": telemetry_on,
                    "settings": settings.to_json(),
                    "startedAt": paths::now_secs(),
                }),
            );
        }

        let mut tick = countdown;
        while tick > 0 {
            if cancelled(paths) {
                break;
            }
            thread::sleep(Duration::from_secs(1));
            tick -= 1;
        }

        if cancelled(paths) {
            stop_child(&mut child, Duration::from_secs(4));
            stop_listener(&mut listener, "cancelled", None);
            if let Some(cursor) = hardware_cursor {
                cursor.restore(paths);
            }
            webcam::cleanup(paths);
            wf_recorder::cleanup_audio(paths);
            paths::remove_quiet(&paths.pid_file);
            paths::remove_quiet(&paths.socket);
            paths::remove_quiet(Path::new(&capture_file));
            paths::remove_quiet(Path::new(&file));
            let _ = state::write(paths, &state::idle_preserving_last(paths));
            paths::remove_quiet(&paths.starting);
            paths::remove_quiet(&paths.cancel);
            return Ok(false);
        }

        countdown as f64 + 0.1
    } else {
        0.0
    };
    let recording_started_at = paths::now_secs();

    {
        let _lock = Lock::acquire(paths).ok();
        // The countdown has already been recorded and will be trimmed from
        // the saved file, so the visible clock starts at zero here.
        let _ = state::write(
            paths,
            &json!({
                "status": "recording",
                "pid": pid,
                "file": &capture_file,
                "output": &file,
                "target": target,
                "targetKind": &choice.kind,
                "targetLabel": &choice.label,
                "targetWarning": &choice.warning,
                "monitor": &choice.monitor,
                "backend": backend.id(),
                "canPause": backend.can_pause(),
                "telemetry": telemetry_on,
                "quality": settings.quality,
                "codec": settings.codec,
                "desktopAudio": settings.desktop,
                "microphoneAudio": settings.mic,
                "noiseCancellation": settings.noise_cancellation,
                "cursor": settings.cursor,
                "webcam": settings.webcam,
                "fps": capture_fps,
                "startedAt": recording_started_at,
                "trimStart": trim_start,
                "paused": false,
                "pausedTotal": 0,
                "pausedAt": 0,
                "settings": settings.to_json(),
            }),
        );
    }
    paths::remove_quiet(&paths.starting);
    paths::remove_quiet(&paths.cancel);
    if settings.manual_zoom {
        let follow = choice.kind == "monitor";
        let stored = config::load(paths);
        let push = manual_zoom_push(follow, settings.auto_zoom, &settings.click_effects, &stored);
        crate::zoom::enable_recording_zoom(paths, settings.zoom_max, follow, push);
    }
    println!("{file}");

    // When telemetry is running, keep this process alive so the listener can
    // watch the encoder's lifetime and close the telemetry file. Otherwise
    // return now, exactly as before.
    if let Some(listener) = listener {
        let reason = match child.wait() {
            Ok(status)
                if status.success()
                    || matches!(status.signal(), Some(libc::SIGINT) | Some(libc::SIGTERM)) =>
            {
                "stopped"
            }
            _ => "error",
        };
        listener.stop(reason, None);
        // `stop` normally restored these already. If the encoder exited on its
        // own, the supervisor is the last one that can put the desktop back.
        if hardware_cursor_marker(paths).exists() {
            if let Some(cursor) = hardware_cursor {
                cursor.restore(paths);
            }
        }
        webcam::cleanup(paths);
        wf_recorder::cleanup_audio(paths);
    }
    Ok(true)
}

/// Close the telemetry file on an early exit. Dropping the listener also
/// writes a terminal line, but naming the reason is more useful.
fn stop_listener(listener: &mut Option<telemetry::Listener>, reason: &str, error: Option<&str>) {
    if let Some(listener) = listener.take() {
        listener.stop(reason, error);
    }
}

/// Select a target, then count down and record without an intermediate step.
pub fn start(paths: &Paths, options: StartArgs) -> Result<bool> {
    let settings = resolve_settings(paths, &options)?;
    let backends = require_tools()?;
    prepare_output_dir(&settings.output_dir)?;
    let _ = start_guard(paths)?;
    // A crashed fallback take can leave its temporary audio mix loaded.
    wf_recorder::cleanup_audio(paths);

    // Wake the desktop-audio monitor before the picker opens: a cold sink
    // otherwise stalls the encoder's first frame for seconds, and the picker
    // is time the user is spending anyway.
    let mut warmup = AudioWarmup::start(&settings);
    // The check is a short call, and doing it before the picker keeps it off
    // the post-selection start path. Only the GPU recorder writes the marker.
    let gpu_first_frame_marker = backends.contains(&Backend::Gpu) && first_frame_marker_supported();

    // Check the audio devices before opening the picker, but only once the
    // "already busy" state is ruled out. The probe is a single fast call, and
    // failing here means a missing microphone is reported immediately instead
    // of after the user has already drawn a region.
    if let Err(err) = validate_audio(&settings) {
        fail_start(paths, &err.to_string());
        return Err(err);
    }
    restore_cursor_mode(paths);

    match choose_target(paths, &options)? {
        Some(choice) => run_take(
            paths,
            &settings,
            &choice,
            &mut warmup,
            &backends,
            gpu_first_frame_marker,
        ),
        None => Ok(false),
    }
}

/// Whether this take's recorder can pause. Older state files predate the flag
/// and were all GPU takes, so a missing field stays pausable unless the
/// backend itself says otherwise.
fn recorder_can_pause(state: &Value) -> bool {
    match state.get("canPause").and_then(Value::as_bool) {
        Some(can_pause) => can_pause,
        None => state.get("backend").and_then(Value::as_str) != Some(wf_recorder::BINARY),
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
    if !recorder_can_pause(&current) {
        let message = "This take is recording with the CPU fallback, which cannot pause.";
        util::notify("Reel", Some(message), "critical");
        anyhow::bail!(message);
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
    crate::zoom::disable_recording_zoom(paths);
    Ok(())
}

pub fn resume(paths: &Paths) -> Result<()> {
    let _lock = Lock::acquire(paths)?;
    let current = state::read_or_idle(paths);
    let status = state::field_str(&current, "status").unwrap_or_default();
    if status == "recording" {
        return Ok(());
    }
    if !recorder_can_pause(&current) {
        let message = "This take is recording with the CPU fallback, which cannot pause.";
        util::notify("Reel", Some(message), "critical");
        anyhow::bail!(message);
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

    let manual_zoom = next
        .get("settings")
        .and_then(|s| s.get("manualZoom"))
        .map(|v| {
            v.as_bool().unwrap_or(false)
                || v.as_str() == Some("on")
                || v.as_str() == Some("true")
                || v.as_str() == Some("1")
        })
        .unwrap_or(false);
    if manual_zoom {
        let zoom_max = config::zoom_max_value(next.get("settings").and_then(|s| s.get("zoomMax")))
            .unwrap_or(2.0);
        let follow = state::field_str(&next, "targetKind").as_deref() == Some("monitor");
        let empty_settings = serde_json::Value::Null;
        let take_settings = next.get("settings").unwrap_or(&empty_settings);
        let stored = config::load(paths);
        let push = manual_zoom_push(
            follow,
            config::auto_zoom(take_settings),
            &config::click_effects(take_settings),
            &stored,
        );
        crate::zoom::enable_recording_zoom(paths, zoom_max, follow, push);
    }

    Ok(())
}

pub fn toggle_pause(paths: &Paths) -> Result<()> {
    match state::status(paths).as_str() {
        "paused" => resume(paths),
        "recording" => pause(paths),
        _ => anyhow::bail!("Reel is not recording."),
    }
}

/// Remove every artifact Reel keeps for a take: the capture, its zoom sidecar,
/// cached previews, and an old in-folder preview.
fn remove_take_artifacts(paths: &Paths, path: &Path) {
    let stem = path.with_extension("").display().to_string();
    let ext = path
        .extension()
        .map(|e| e.to_string_lossy().to_string())
        .unwrap_or_default();
    paths::remove_quiet(path);
    paths::remove_quiet(Path::new(&format!("{}.ts", path.display())));
    paths::remove_quiet(&paths::telemetry_path(path));
    paths::remove_quiet(&project::project_path(path));
    paths::remove_quiet(&paths::legacy_telemetry_path(path));
    paths::remove_quiet(&project::legacy_project_path(path));
    paths::remove_quiet(&path.with_extension("zoom.json"));
    paths::remove_quiet(&paths.preview_path(path));
    paths::remove_quiet(&paths.cache_preview_path(path));
    paths::remove_quiet(&paths::legacy_preview_path(path));
    paths::remove_quiet(Path::new(&format!("{stem}-processed.{ext}")));
    fs::remove_dir(paths::sidecar_dir(path)).ok();
}

/// Resolve the raw capture and final output for a take stopped by possibly
/// newer code. Older state files only carry `file`; a countdown take from that
/// version recorded straight into the ordinary name, so quarantine it before
/// any fallible finalization.
fn resolve_capture_paths(
    paths: &Paths,
    file: String,
    output: String,
    trim_start: f64,
) -> Result<(String, String)> {
    let output = output.trim().to_string();
    let in_place = output.is_empty() || output == file;
    let capture = Path::new(&file);

    if trim_start > 0.0 && in_place && !file.is_empty() && !paths::is_untrimmed(capture) {
        let quarantined = crate::quarantine::capture(paths, capture).with_context(|| {
            format!(
                "cannot move {} out of the ordinary take list",
                capture.display()
            )
        })?;
        let output = if output.is_empty() { file } else { output };
        return Ok((quarantined.display().to_string(), output));
    }

    if output.is_empty() {
        if paths::is_untrimmed(capture) {
            let trimmed = paths::trimmed_path(capture).display().to_string();
            return Ok((file, trimmed));
        }
        return Ok((file.clone(), file));
    }

    Ok((file, output))
}

pub fn stop(paths: &Paths, discard: bool) -> Result<()> {
    let (
        status,
        pid,
        file,
        output,
        started_at,
        target,
        target_kind,
        target_label,
        trim_start,
        noise_cancellation,
        telemetry_ran,
        auto_zoom,
        auto_zoom_depth,
        click_effects,
    );
    {
        let _lock = Lock::acquire(paths)?;
        let current = state::read_or_idle(paths);
        status = state::field_str(&current, "status").unwrap_or_default();
        pid = state::field_i64(&current, "pid").unwrap_or(0) as i32;
        file = state::field_str(&current, "file").unwrap_or_default();
        output = state::field_str(&current, "output")
            .unwrap_or_default()
            .trim()
            .to_string();
        started_at = state::field_i64(&current, "startedAt").unwrap_or_else(paths::now_secs);
        target = state::field_str(&current, "target").unwrap_or_default();
        target_kind = state::field_str(&current, "targetKind").unwrap_or_default();
        target_label = state::field_str(&current, "targetLabel").unwrap_or_default();
        trim_start = state::field_str(&current, "trimStart")
            .unwrap_or_default()
            .parse::<f64>()
            .unwrap_or(0.0);

        // The save pass must not read the live settings later: a new take may
        // already be running by the time it starts. Resolve this take's save
        // options now and hand them over with the job.
        let take_settings = current.get("settings").cloned().unwrap_or(Value::Null);
        auto_zoom = crate::config::auto_zoom(&take_settings);
        auto_zoom_depth = crate::config::auto_zoom_depth(&take_settings);
        click_effects = crate::config::click_effects(&take_settings);
        let enabled =
            |value: &Value, key: &str| value.get(key).and_then(Value::as_bool).unwrap_or(false);
        let mic_on =
            enabled(&current, "microphoneAudio") || enabled(&take_settings, "microphoneAudio");
        let noise_on =
            enabled(&current, "noiseCancellation") || enabled(&take_settings, "noiseCancellation");
        noise_cancellation = mic_on && noise_on;
        // Telemetry is the save pass's only pointer/click source. The flag is
        // written by the recorder while the listener is live.
        telemetry_ran = current
            .get("telemetry")
            .and_then(Value::as_bool)
            .unwrap_or(false);

        if status == "starting" {
            let _ = paths::write_atomic(&paths.cancel, "");
            let _ = util::run_ok("pkill", &["-x", "slurp"]);
            crate::zoom::disable_recording_zoom(paths);
            return Ok(());
        }
        if status != "recording" && status != "paused" {
            crate::zoom::disable_recording_zoom(paths);
            anyhow::bail!("Reel is not recording.");
        }
        if !file.is_empty() && Path::new(&file).is_file() && !discard {
            let mut next = state::with_status(current, "finalizing");
            state::set(
                &mut next,
                "finalizeAt",
                serde_json::json!(paths::now_secs()),
            );
            state::set(&mut next, "finalizePid", serde_json::json!(0));
            let _ = state::write(paths, &next);
        }
    }

    paths::kill(pid, libc::SIGINT);
    crate::zoom::disable_recording_zoom(paths);
    if !wait_for_exit(pid, Duration::from_secs(8)) {
        paths::log(paths, "stop: encoder ignored SIGINT, escalating to SIGKILL");
        paths::kill(pid, libc::SIGKILL);
    }

    restore_cursor_mode(paths);
    webcam::cleanup(paths);
    wf_recorder::cleanup_audio(paths);
    paths::remove_quiet(&paths.pid_file);
    paths::remove_quiet(&paths.socket);

    if discard {
        if !file.is_empty() {
            remove_take_artifacts(paths, Path::new(&file));
        }
        if !output.is_empty() && output != file {
            remove_take_artifacts(paths, Path::new(&output));
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

    // Older state files predate the `output` field. A countdown take from
    // that version recorded straight into the ordinary name, so an engine
    // upgrade landing mid-take must move the raw capture aside before any
    // fallible save pass. Otherwise a failed trim would leave the pre-roll
    // visible in history and available to share.
    let (file, output) = match resolve_capture_paths(paths, file, output, trim_start) {
        Ok(paths) => paths,
        Err(err) => {
            let message = format!(
                "The countdown capture could not be quarantined before saving: {err}. \
                 The original file is kept at the recorded path and withheld from history \
                 and sharing until it can be moved aside."
            );
            let _ = state::write(
                paths,
                &state::idle_with_error_keeping_capture(paths, &message),
            );
            paths::remove_quiet(&paths.starting);
            paths::remove_quiet(&paths.cancel);
            util::notify("Reel", Some(&message), "critical");
            return Err(err);
        }
    };

    // Register the save before spawning it. The job record outlives this
    // command, and a new take may start while several of these run together.
    let job_id = crate::saves::create(
        paths,
        Path::new(&file),
        Path::new(&output),
        trim_start,
        started_at,
    )
    .ok();
    // The runtime telemetry file is cleared by the next take, so the save pass
    // gets its own copy instead of reading a file the next recorder overwrites.
    let telemetry = match job_id.as_deref() {
        Some(id) if telemetry_ran && paths.session_telemetry.is_file() => {
            crate::saves::snapshot_telemetry(paths, id)
        }
        _ => None,
    };

    // Do not render a preview here: it delayed the stop response for every
    // take. The finalize child renders one from the raw capture before its
    // save pass, so a failed or interrupted save still leaves a thumbnail.
    let preview = paths.preview_path(Path::new(&output));

    let job = util::spawn_finalize(
        paths,
        Path::new(&file),
        Path::new(&output),
        &preview,
        trim_start,
        started_at,
        &target,
        &target_kind,
        &target_label,
        noise_cancellation,
        telemetry.as_deref(),
        job_id.as_deref(),
        auto_zoom,
        auto_zoom_depth,
        &click_effects,
    );
    match job {
        Ok(job_pid) => {
            if let Some(id) = job_id.as_deref() {
                crate::saves::set_pid(paths, id, job_pid);
            }
            paths::log(paths, &format!("stop: save job {job_pid} for {output}"));
        }
        Err(err) => {
            if let Some(id) = job_id.as_deref() {
                crate::saves::remove(paths, id);
            }
            paths::log(paths, &format!("stop: cannot spawn save job: {err}"));
            let message = if output != file {
                format!("The take could not be saved. Untrimmed footage is at {file}.")
            } else {
                "The take was captured, but the save job could not start.".into()
            };
            let _ = state::write(
                paths,
                &state::idle_with_error_preserving_last(paths, &message),
            );
            util::notify("Reel", Some(&message), "critical");
        }
    }
    // The encoder is gone and the job owns the rest. Release the engine so the
    // next take can start; the save keeps running in its own process.
    {
        let _lock = Lock::acquire(paths).ok();
        if let Some(current) = state::read(paths) {
            let status = state::field_str(&current, "status").unwrap_or_default();
            if status == "finalizing" {
                let _ = state::write(paths, &state::idle_preserving_last(paths));
            }
        }
    }
    paths::remove_quiet(&paths.starting);
    paths::remove_quiet(&paths.cancel);
    println!("{output}");
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
    use std::path::PathBuf;

    fn temp_root(label: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "reel-recorder-{label}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ))
    }

    #[test]
    fn the_cpu_fallback_is_not_pausable() {
        assert!(!recorder_can_pause(&json!({ "backend": "wf-recorder" })));
        assert!(!recorder_can_pause(&json!({ "canPause": false })));
        assert!(recorder_can_pause(
            &json!({ "backend": "gpu-screen-recorder" })
        ));
        assert!(recorder_can_pause(&json!({})));
    }

    #[test]
    fn detects_amd_polaris_from_sysfs() {
        let root = temp_root("polaris");
        let device = root.join("0000:01:00.0");
        fs::create_dir_all(&device).unwrap();
        fs::write(device.join("vendor"), "0x1002\n").unwrap();
        fs::write(device.join("device"), "0x67df\n").unwrap();
        assert!(polaris_present(&root));

        fs::write(device.join("device"), "0x73df\n").unwrap();
        assert!(!polaris_present(&root));
        assert!(!polaris_present(&root.join("missing")));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn a_denied_quarantine_keeps_the_ordinary_name() {
        // SAFETY: geteuid has no memory effects and cannot fail.
        if unsafe { libc::geteuid() } == 0 {
            return;
        }
        use std::os::unix::fs::PermissionsExt;

        let root = temp_root("denied");
        let videos = root.join("Videos");
        fs::create_dir_all(&videos).unwrap();
        let paths = Paths::for_test(&root);
        let file = videos.join("reel-legacy.mp4");
        fs::write(&file, b"raw").unwrap();
        fs::set_permissions(&videos, fs::Permissions::from_mode(0o555)).unwrap();

        let result = resolve_capture_paths(&paths, file.display().to_string(), String::new(), 3.0);
        assert!(result.is_err());
        assert!(file.is_file());
        assert!(crate::quarantine::is_blocked(&paths, &file));

        fs::set_permissions(&videos, fs::Permissions::from_mode(0o755)).unwrap();
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn a_writable_folder_quarantines_the_capture() {
        let root = temp_root("writable");
        let videos = root.join("Videos");
        fs::create_dir_all(&videos).unwrap();
        let paths = Paths::for_test(&root);
        let file = videos.join("reel-legacy.mp4");
        fs::write(&file, b"raw").unwrap();

        let (capture, output) =
            resolve_capture_paths(&paths, file.display().to_string(), String::new(), 3.0).unwrap();
        assert!(capture.ends_with("-untrimmed.mp4"));
        assert_eq!(output, file.display().to_string());
        assert!(Path::new(&capture).is_file());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn a_take_waits_for_the_previous_encoder_to_stop() {
        let root = temp_root("finalizing");
        fs::create_dir_all(&root).unwrap();
        let paths = Paths::for_test(&root);
        let alive = std::process::id() as i64;
        state::write(
            &paths,
            &json!({
                "status": "finalizing",
                "finalizePid": alive,
                "finalizeAt": paths::now_secs(),
            }),
        )
        .unwrap();

        let error = start_guard_within(&paths, Duration::from_millis(150))
            .unwrap_err()
            .to_string();
        assert!(error.contains("still finishing"), "{error}");
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn a_start_press_waits_for_the_encoder_to_finish() {
        let root = temp_root("start-waits");
        fs::create_dir_all(&root).unwrap();
        let paths = Paths::for_test(&root);
        let alive = std::process::id() as i64;
        state::write(
            &paths,
            &json!({
                "status": "finalizing",
                "finalizePid": alive,
                "finalizeAt": paths::now_secs(),
            }),
        )
        .unwrap();

        let idle = Paths::for_test(&root);
        let releaser = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(250));
            let _ = state::write(&idle, &state::idle());
        });
        let result = start_guard_within(&paths, Duration::from_secs(2));
        releaser.join().unwrap();
        assert!(result.is_ok(), "{result:?}");
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn the_container_alone_is_not_a_started_capture() {
        let root = temp_root("first-frame");
        fs::create_dir_all(&root).unwrap();
        let paths = Paths::for_test(&root);
        let file = root.join("take.capture.mp4");
        let marker = root.join("take.capture.mp4.ts");
        fs::write(&file, b"header").unwrap();

        assert!(
            !wait_for_capture_file(
                &paths,
                &file,
                Some(marker.as_path()),
                std::process::id() as i32,
                Duration::from_millis(50),
            )
            .0
        );
        fs::write(
            &marker,
            b"monotonic_microsec\trealtime_microsec\n1234567\t7654321\n",
        )
        .unwrap();
        let (ready, first_frame_us) = wait_for_capture_file(
            &paths,
            &file,
            Some(marker.as_path()),
            std::process::id() as i32,
            Duration::from_millis(50),
        );
        assert!(ready);
        assert_eq!(first_frame_us, Some(1_234_567));
        assert!(!marker.exists(), "the marker is only a startup signal");
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn a_second_start_press_is_refused_before_the_picker() {
        let root = temp_root("start-race");
        fs::create_dir_all(&root).unwrap();
        let paths = Paths::for_test(&root);
        state::write(&paths, &state::idle()).unwrap();

        assert!(start_guard(&paths).is_ok());
        assert_eq!(state::status(&paths), "starting");
        let error = start_guard(&paths).unwrap_err().to_string();
        assert!(error.contains("already busy"), "{error}");
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn a_running_save_does_not_block_a_new_take() {
        let root = temp_root("saves");
        fs::create_dir_all(&root).unwrap();
        let paths = Paths::for_test(&root);
        state::write(&paths, &state::idle()).unwrap();
        let file = root.join("take.capture.mp4");
        let output = root.join("take.mp4");
        for _ in 0..10 {
            let id = crate::saves::create(&paths, &file, &output, 0.0, paths::now_secs()).unwrap();
            crate::saves::set_pid(&paths, &id, std::process::id());
        }

        assert_eq!(crate::saves::count(&paths), 10);
        assert!(start_guard(&paths).is_ok());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn the_noise_reduction_argument_wins_over_a_stale_config() {
        let root = temp_root("noise");
        fs::create_dir_all(&root).unwrap();
        let paths = Paths::for_test(&root);
        fs::create_dir_all(&paths.config_dir).unwrap();
        fs::write(
            &paths.config,
            r#"{"microphoneAudio":"on","noiseCancellation":"off"}"#,
        )
        .unwrap();

        let options = StartArgs {
            microphone_audio: Some("on".into()),
            noise_cancellation: Some("on".into()),
            ..Default::default()
        };
        let settings = resolve_settings(&paths, &options).unwrap();
        assert!(settings.mic);
        assert!(settings.noise_cancellation);

        let stored = resolve_settings(
            &paths,
            &StartArgs {
                microphone_audio: Some("on".into()),
                ..Default::default()
            },
        )
        .unwrap();
        assert!(!stored.noise_cancellation);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn the_auto_zoom_argument_wins_over_a_stale_config() {
        let stored = json!({ "autoZoom": "on" });
        assert!(!auto_zoom_choice(&Some("off".into()), &stored));
        assert!(auto_zoom_choice(&Some("on".into()), &stored));
        assert!(auto_zoom_choice(&Some("  ".into()), &stored));
        assert!(auto_zoom_choice(&None, &stored));
    }

    #[test]
    fn manual_zoom_uses_push_only_when_opted_in_and_clicks_do_not_need_follow() {
        // Follow is the default; push is an explicit choice.
        assert!(!manual_zoom_push(true, false, "off", &json!({})));
        let push = json!({ "zoomFollow": "push" });
        assert!(manual_zoom_push(true, false, "off", &push));
        // Auto zoom and click effects need the attached camera, so they win.
        assert!(!manual_zoom_push(true, true, "off", &push));
        assert!(!manual_zoom_push(true, false, "ripple", &push));
        // A window or area take is never push; it is already detached.
        assert!(!manual_zoom_push(false, false, "off", &push));
        assert!(!manual_zoom_push(
            true,
            false,
            "off",
            &json!({ "zoomFollow": "follow" })
        ));
    }

    #[test]
    fn the_click_effect_argument_wins_over_a_stale_config() {
        let stored = json!({ "clickEffects": "halo" });
        assert_eq!(
            click_effects_choice(&Some("ripple".into()), &stored),
            "ripple"
        );
        assert_eq!(click_effects_choice(&Some("off".into()), &stored), "off");
        assert_eq!(click_effects_choice(&Some("pulse".into()), &stored), "off");
        assert_eq!(click_effects_choice(&Some("  ".into()), &stored), "halo");
        assert_eq!(click_effects_choice(&None, &stored), "halo");
        assert_eq!(
            click_effects_choice(&None, &json!({ "clickEffects": "sparkle" })),
            "off"
        );
    }

    #[test]
    fn settings_serialize_for_the_state_file() {
        let settings = Settings {
            desktop: true,
            mic: false,
            noise_cancellation: true,
            desktop_device: String::new(),
            microphone_device: String::new(),
            cursor: true,
            webcam: "off".into(),
            webcam_device: String::new(),
            countdown: 3,
            fps: 60,
            quality: "very_high".into(),
            codec: "auto".into(),
            resolution: "0x0".into(),
            container: "mp4".into(),
            output_dir: "/tmp".into(),
            filename_prefix: "reel-".into(),
            manual_zoom: true,
            zoom_max: 2.0,
            auto_zoom: true,
            auto_zoom_depth: 2.5,
            click_effects: "ripple".into(),
        };
        let value = settings.to_json();
        assert_eq!(value["fps"], 60);
        assert_eq!(value["desktopAudio"], true);
        assert_eq!(value["microphoneAudio"], false);
        assert_eq!(value["noiseCancellation"], true);
        assert_eq!(value["desktopDevice"], "");
        assert_eq!(value["microphoneDevice"], "");
        assert_eq!(value["manualZoom"], true);
        assert_eq!(value["zoomMax"], 2.0);
        assert_eq!(value["clickEffects"], "ripple");
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
    fn a_legacy_countdown_take_is_quarantined_before_finalize() {
        let root = std::env::temp_dir().join(format!(
            "reel-recorder-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        fs::create_dir_all(&root).unwrap();
        let paths = Paths::for_test(&root);
        let file = root.join("reel-1.mp4");
        fs::write(&file, b"raw").unwrap();

        let (capture, output) =
            resolve_capture_paths(&paths, file.display().to_string(), String::new(), 3.0).unwrap();

        assert_eq!(output, file.display().to_string());
        assert_eq!(capture, paths::untrimmed_path(&file).display().to_string());
        assert!(!file.exists());
        assert!(Path::new(&capture).is_file());

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn a_legacy_plain_take_keeps_its_name() {
        let root = std::env::temp_dir().join(format!(
            "reel-recorder-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        fs::create_dir_all(&root).unwrap();
        let paths = Paths::for_test(&root);
        let file = root.join("reel-1.mp4");
        fs::write(&file, b"raw").unwrap();

        let (capture, output) =
            resolve_capture_paths(&paths, file.display().to_string(), String::new(), 0.0).unwrap();

        assert_eq!(capture, file.display().to_string());
        assert_eq!(output, file.display().to_string());
        assert!(file.is_file());

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn an_existing_output_is_left_alone() {
        let paths = Paths::for_test(Path::new("/tmp/reel-unused"));
        assert_eq!(
            resolve_capture_paths(
                &paths,
                "/videos/take-untrimmed.mp4".into(),
                "/videos/take.mp4".into(),
                3.0,
            )
            .unwrap(),
            (
                "/videos/take-untrimmed.mp4".into(),
                "/videos/take.mp4".into()
            )
        );
    }

    #[test]
    fn removing_a_take_also_removes_its_telemetry_sidecar() {
        let root = std::env::temp_dir().join(format!(
            "reel-recorder-discard-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        let videos = root.join("Videos");
        fs::create_dir_all(&videos).unwrap();
        let paths = Paths::for_test(&root);
        let take = videos.join("reel-1.mp4");
        fs::write(&take, b"video").unwrap();
        let sidecar = paths::telemetry_path(&take);
        paths::ensure_sidecar_dir(&take).unwrap();
        fs::write(&sidecar, b"{}\n").unwrap();
        let project = project::project_path(&take);
        fs::write(&project, b"{}\n").unwrap();
        let legacy_telemetry = paths::legacy_telemetry_path(&take);
        fs::write(&legacy_telemetry, b"{}\n").unwrap();
        let legacy_project = project::legacy_project_path(&take);
        fs::write(&legacy_project, b"{}\n").unwrap();

        remove_take_artifacts(&paths, &take);

        assert!(!take.exists());
        assert!(!sidecar.exists());
        assert!(!project.exists());
        assert!(!legacy_telemetry.exists());
        assert!(!legacy_project.exists());
        assert!(!paths::sidecar_dir(&take).exists());
        let _ = fs::remove_dir_all(root);
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
