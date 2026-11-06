//! Pointer and click telemetry.
//!
//! Every line is one JSON object in `session.telemetry.jsonl`. Events are read
//! from evdev and converted to capture-local pixels with the same facts the
//! encoder used, so later render passes can align effects with the recorded
//! frame.
//!
//! The listener is a thread in the process that launched the encoder. That
//! process stays alive while telemetry is active (see `recorder::run_take`) so
//! the listener can stop on the same clock as the video.

use crate::paths::{self, Paths};
use crate::target;
use crate::util;
use anyhow::{Context, Result};
use serde_json::{json, Value};
use std::fs::{File, OpenOptions};
use std::io::{Read, Write};
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::os::unix::io::AsRawFd;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

const VERSION: u64 = 1;
const CLOCK_NAME: &str = "CLOCK_MONOTONIC";

const EV_SYN: u16 = 0x00;
const EV_KEY: u16 = 0x01;
const EV_REL: u16 = 0x02;
const EV_ABS: u16 = 0x03;
const REL_X: u16 = 0x00;
const REL_Y: u16 = 0x01;
const ABS_X: u16 = 0x00;
const ABS_Y: u16 = 0x01;
const ABS_MT_POSITION_X: u16 = 0x35;
const ABS_MT_POSITION_Y: u16 = 0x36;
const SYN_REPORT: u16 = 0x00;
const BTN_LEFT: u16 = 0x110;
const BTN_RIGHT: u16 = 0x111;
const BTN_MIDDLE: u16 = 0x112;
const BTN_TOOL_DOUBLETAP: u16 = 0x14d;
const BTN_TOOL_TRIPLETAP: u16 = 0x14e;
const BTN_TOOL_QUADTAP: u16 = 0x14f;
const BTN_TOOL_QUINTTAP: u16 = 0x148;
const BTN_TOUCH: u16 = 0x14a;

/// The longest contact that still counts as a tap.
const TAP_MAX_SECS: f64 = 0.3;
/// Contact movement (in raw touchpad units) that turns a tap into a drag.
const TAP_SLOP: f64 = 60.0;
/// Wait for the compositor to move the cursor onto a tap before reading it.
const TAP_SETTLE: Duration = Duration::from_millis(15);
/// Capability bitmaps are queried in 64-bit words. Eight words cover event
/// codes up to 511, including BTN_TOUCH.
const CAP_WORDS: usize = 8;

const BY_ID: &str = "/dev/input/by-id";
const EVENT_DIR: &str = "/dev/input";
const SYNC_INTERVAL: Duration = Duration::from_millis(100);
const POLL_TIMEOUT_MS: libc::c_int = 10;

/// A short reason for a telemetry source that could not start. The contract
/// names the first three; the last two cover targets telemetry cannot map.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceError {
    InputGroup,
    NoDevice,
    OpenFailed,
    NoMonitor,
    UnsupportedTarget,
}

impl SourceError {
    pub fn code(self) -> &'static str {
        match self {
            SourceError::InputGroup => "input-group",
            SourceError::NoDevice => "no-device",
            SourceError::OpenFailed => "open-failed",
            SourceError::NoMonitor => "no-monitor",
            SourceError::UnsupportedTarget => "unsupported-target",
        }
    }

    /// The fix a user can act on, or `None` when there is nothing to do.
    pub fn hint(self) -> Option<&'static str> {
        match self {
            SourceError::InputGroup => Some("add your user to the input group, then log in again"),
            _ => None,
        }
    }
}

/// The source rectangle and the encoded frame it becomes.
///
/// `x`, `y`, `scale`, and `transform` are the compositor facts for debugging
/// and re-calibration. `width` and `height` are the encoded video dimensions.
#[derive(Debug, Clone)]
pub struct CaptureSpace {
    pub kind: String,
    pub target: String,
    pub monitor: String,
    pub x: i64,
    pub y: i64,
    pub source_width: f64,
    pub source_height: f64,
    pub width: u32,
    pub height: u32,
    pub scale: f64,
    pub transform: i64,
}

impl CaptureSpace {
    /// Resolve the encoded frame and the logical-to-pixel mapping for a target.
    pub fn resolve(
        target: &str,
        kind: &str,
        monitor_name: &str,
        resolution: &str,
    ) -> Result<Self, SourceError> {
        let monitors = target::monitors();
        if monitors.is_empty() {
            return Err(SourceError::NoMonitor);
        }
        let encoded = parse_resolution(resolution);
        if let Some(name) = target.strip_prefix("monitor:") {
            let monitor = monitors
                .iter()
                .find(|monitor| monitor.name == name)
                .ok_or(SourceError::NoMonitor)?;
            let (source_width, source_height) = monitor_pixels(monitor);
            if source_width <= 0.0 || source_height <= 0.0 {
                return Err(SourceError::NoMonitor);
            }
            let (width, height) = encoded.unwrap_or((source_width as u32, source_height as u32));
            return Ok(Self {
                kind: kind.to_string(),
                target: target.to_string(),
                monitor: name.to_string(),
                x: monitor.x,
                y: monitor.y,
                source_width,
                source_height,
                width,
                height,
                scale: positive_scale(monitor.scale),
                transform: monitor.transform,
            });
        }

        let body = match target.strip_prefix("region:") {
            Some(body) => body,
            None => return Err(SourceError::UnsupportedTarget),
        };
        let (region_width, region_height, x, y) =
            target::parse_geometry(body).ok_or(SourceError::UnsupportedTarget)?;
        let scale = monitor_for_region(&monitors, x, y, region_width as i64, region_height as i64)
            .map(|monitor| positive_scale(monitor.scale))
            .unwrap_or(1.0);
        let (source_width, source_height) =
            region_pixels(x, y, region_width as i64, region_height as i64, scale);
        if source_width <= 0.0 || source_height <= 0.0 {
            return Err(SourceError::UnsupportedTarget);
        }
        let (width, height) = encoded.unwrap_or((source_width as u32, source_height as u32));
        let monitor = if monitor_name.is_empty() {
            monitor_for_region(&monitors, x, y, region_width as i64, region_height as i64)
                .map(|monitor| monitor.name.clone())
                .unwrap_or_default()
        } else {
            monitor_name.to_string()
        };
        let transform = monitors
            .iter()
            .find(|candidate| candidate.name == monitor)
            .map(|candidate| candidate.transform)
            .unwrap_or(0);
        Ok(Self {
            kind: kind.to_string(),
            target: target.to_string(),
            monitor,
            x,
            y,
            source_width,
            source_height,
            width,
            height,
            scale,
            transform,
        })
    }

    /// A placeholder for a target telemetry cannot map, used only so a failed
    /// start still writes a well-formed `start` line.
    pub fn unsupported(kind: &str, target: &str, monitor: &str) -> Self {
        Self {
            kind: kind.to_string(),
            target: target.to_string(),
            monitor: monitor.to_string(),
            x: 0,
            y: 0,
            source_width: 0.0,
            source_height: 0.0,
            width: 0,
            height: 0,
            scale: 1.0,
            transform: 0,
        }
    }

    /// Logical compositor coordinates to capture-local pixels. Not clamped:
    /// consumers test the frame bounds themselves.
    pub fn to_capture(&self, logical_x: f64, logical_y: f64) -> (f64, f64) {
        if self.source_width <= 0.0 || self.source_height <= 0.0 {
            return (0.0, 0.0);
        }
        let local_x = (logical_x - self.x as f64) * self.scale;
        let local_y = (logical_y - self.y as f64) * self.scale;
        let x = local_x * self.width as f64 / self.source_width;
        let y = local_y * self.height as f64 / self.source_height;
        (round_px(x), round_px(y))
    }

    /// A sane starting point when the compositor cannot report the pointer.
    fn centre_logical(&self) -> (f64, f64) {
        let scale = if self.scale > 0.0 { self.scale } else { 1.0 };
        (
            self.x as f64 + self.source_width / scale / 2.0,
            self.y as f64 + self.source_height / scale / 2.0,
        )
    }
}

fn positive_scale(scale: f64) -> f64 {
    if scale > 0.0 {
        scale
    } else {
        1.0
    }
}

fn monitor_pixels(monitor: &target::Monitor) -> (f64, f64) {
    if monitor.transform % 2 != 0 {
        (monitor.height as f64, monitor.width as f64)
    } else {
        (monitor.width as f64, monitor.height as f64)
    }
}

fn monitor_for_region(
    monitors: &[target::Monitor],
    x: i64,
    y: i64,
    width: i64,
    height: i64,
) -> Option<&target::Monitor> {
    let cx = x + width / 2;
    let cy = y + height / 2;
    monitors.iter().find(|monitor| {
        let (pixel_width, pixel_height) = monitor_pixels(monitor);
        // `monitor.x/y` and the picker rectangle are logical; the monitor's
        // pixel size is physical, so divide by its fractional scale.
        let scale = positive_scale(monitor.scale);
        let logical_width = pixel_width / scale;
        let logical_height = pixel_height / scale;
        cx >= monitor.x
            && (cx as f64) < monitor.x as f64 + logical_width
            && cy >= monitor.y
            && (cy as f64) < monitor.y as f64 + logical_height
    })
}

/// Match the compositor's boundary rounding so the region size agrees with the
/// pixels `gpu-screen-recorder` crops.
fn region_pixels(x: i64, y: i64, width: i64, height: i64, scale: f64) -> (f64, f64) {
    let left = (x as f64 * scale).round();
    let right = ((x + width) as f64 * scale).round();
    let top = (y as f64 * scale).round();
    let bottom = ((y + height) as f64 * scale).round();
    ((right - left).max(0.0), (bottom - top).max(0.0))
}

fn parse_resolution(raw: &str) -> Option<(u32, u32)> {
    let raw = raw.trim();
    if raw.is_empty() || raw == "0x0" {
        return None;
    }
    let (width, height) = raw.split_once('x')?;
    let width = width.trim().parse::<u32>().ok()?;
    let height = height.trim().parse::<u32>().ok()?;
    (width > 0 && height > 0).then_some((width, height))
}

/// One evdev event, laid out exactly as the kernel writes it.
#[repr(C)]
struct InputEvent {
    _time: libc::timeval,
    kind: u16,
    code: u16,
    value: i32,
}

/// What a device can report. A mouse is relative, a touchpad and a touchscreen
/// are absolute (and may report contact through `BTN_TOUCH`).
#[derive(Debug, Clone, Copy, Default)]
struct PointerCaps {
    relative: bool,
    absolute: bool,
    touch: bool,
}

struct Device {
    file: File,
    name: String,
    caps: PointerCaps,
    tap: TapState,
}

/// The in-progress contact on a touch device, used to turn a tap into a click.
///
/// Taps never reach evdev: libinput turns them into a button press above the
/// device node. Watching `BTN_TOUCH` is the only way to see them, so a short,
/// still, single-finger contact is emitted as a left click at the cursor.
#[derive(Debug, Default)]
struct TapState {
    down_at: Option<Instant>,
    /// Per-axis raw position bounds during the contact.
    x: Option<(f64, f64)>,
    y: Option<(f64, f64)>,
    multi: bool,
    /// A real BTN_LEFT arrived, so this contact is already a click.
    physical: bool,
}

impl TapState {
    fn begin(&mut self) {
        self.down_at = Some(Instant::now());
        self.x = None;
        self.y = None;
        self.multi = false;
        self.physical = false;
    }

    fn track_x(&mut self, value: f64) {
        track(&mut self.x, value);
    }

    fn track_y(&mut self, value: f64) {
        track(&mut self.y, value);
    }

    fn travelled(&self) -> f64 {
        let span =
            |bounds: Option<(f64, f64)>| bounds.map(|(min, max)| (max - min).abs()).unwrap_or(0.0);
        span(self.x).hypot(span(self.y))
    }

    fn is_tap(&self) -> bool {
        self.down_at
            .map(|down| down.elapsed().as_secs_f64() <= TAP_MAX_SECS)
            .unwrap_or(false)
            && !self.multi
            && !self.physical
            && self.travelled() <= TAP_SLOP
    }
}

fn track(bounds: &mut Option<(f64, f64)>, value: f64) {
    let (min, max) = bounds.get_or_insert((value, value));
    *min = min.min(value);
    *max = max.max(value);
}

/// A take's telemetry file, opened and waiting for `T0`.
struct TelemetryWriter {
    file: File,
    t0: Instant,
    t0_ns: u64,
}

impl TelemetryWriter {
    fn t(&self) -> f64 {
        seconds(self.t0.elapsed().as_secs_f64())
    }

    fn write(&mut self, value: &Value) {
        if let Ok(line) = serde_json::to_string(value) {
            // File has no userspace buffer, but flushing after every line is
            // part of the contract: a crash may lose at most this event.
            let _ = writeln!(self.file, "{line}");
            let _ = self.file.flush();
        }
    }
}

/// Everything needed to start the listener, resolved before the encoder
/// launches; if telemetry cannot start the take continues without pointer data.
pub struct Prepared {
    file: Option<File>,
    devices: Vec<Device>,
    error: Option<SourceError>,
}

impl Prepared {
    /// Open the runtime file and the pointer devices. Never fails the take.
    pub fn new(paths: &Paths, space: &CaptureSpace) -> Prepared {
        let file = OpenOptions::new()
            .create(true)
            .truncate(true)
            .write(true)
            .open(&paths.session_telemetry)
            .ok();
        let (devices, error) = discover_devices(Path::new(BY_ID), Path::new(EVENT_DIR));
        if file.is_none() && error.is_none() {
            return Prepared {
                file: None,
                devices: Vec::new(),
                error: Some(SourceError::OpenFailed),
            };
        }
        let error = if devices.is_empty() {
            Some(error.unwrap_or(SourceError::NoDevice))
        } else {
            None
        };
        if error.is_some() {
            paths::log(
                paths,
                &format!(
                    "telemetry: pointer source unavailable ({}), capture space {}",
                    error.map(SourceError::code).unwrap_or("unknown"),
                    space.target
                ),
            );
        } else {
            let names: Vec<&str> = devices.iter().map(|device| device.name.as_str()).collect();
            paths::log(paths, &format!("telemetry: watching {}", names.join(", ")));
        }
        Prepared {
            file,
            devices,
            error,
        }
    }

    pub fn available(&self) -> bool {
        self.error.is_none() && !self.devices.is_empty()
    }

    pub fn error(&self) -> Option<SourceError> {
        self.error
    }

    /// Write `start` with `source:none` and a `stop` line, then leave the file
    /// for debugging. Used when pointer telemetry cannot start.
    pub fn record_failure(mut self, space: &CaptureSpace, fps: i64, error: SourceError) {
        let Some(file) = self.file.take() else {
            return;
        };
        let mut writer = TelemetryWriter {
            file,
            t0: Instant::now(),
            t0_ns: monotonic_ns(),
        };
        let code = error.code();
        writer.write(&start_value(space, fps, "none", Some(code), writer.t0_ns));
        let t = writer.t();
        writer.write(&stop_value(t, "error", Some(code)));
    }

    /// Begin the listener. `t0` is the moment the encoder was launched.
    pub fn start(
        mut self,
        space: CaptureSpace,
        fps: i64,
        t0: Instant,
        t0_ns: u64,
    ) -> Option<Listener> {
        let file = self.file.take()?;
        if self.error.is_some() || self.devices.is_empty() {
            return None;
        }
        let writer = Arc::new(Mutex::new(TelemetryWriter { file, t0, t0_ns }));
        writer
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .write(&start_value(&space, fps, "evdev", None, t0_ns));

        let stop = Arc::new(AtomicBool::new(false));
        let devices = std::mem::take(&mut self.devices);
        let handle = {
            let writer = Arc::clone(&writer);
            let stop = Arc::clone(&stop);
            thread::Builder::new()
                .name("reel-telemetry".into())
                .spawn(move || run_listener(devices, space, writer, stop))
        };
        match handle {
            Ok(handle) => Some(Listener {
                stop,
                handle: Some(handle),
                writer,
            }),
            Err(_) => {
                let mut writer = writer
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner());
                let t = writer.t();
                writer.write(&stop_value(t, "error", Some("open-failed")));
                None
            }
        }
    }
}

/// The live listener. Dropping it without an explicit stop still writes a
/// terminal line, so a failed take never leaves a file open-ended.
pub struct Listener {
    stop: Arc<AtomicBool>,
    handle: Option<JoinHandle<()>>,
    writer: Arc<Mutex<TelemetryWriter>>,
}

impl Listener {
    pub fn stop(mut self, reason: &str, error: Option<&str>) {
        self.finish(reason, error);
    }

    fn finish(&mut self, reason: &str, error: Option<&str>) {
        let Some(handle) = self.handle.take() else {
            return;
        };
        self.stop.store(true, Ordering::SeqCst);
        let _ = handle.join();
        let mut writer = self
            .writer
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let t = writer.t();
        writer.write(&stop_value(t, reason, error));
    }
}

impl Drop for Listener {
    fn drop(&mut self) {
        if self.handle.is_some() {
            self.finish("error", Some("aborted"));
        }
    }
}

/// Copy the runtime file into the take's hidden sidecar folder. The runtime
/// copy is kept for debugging and cleared by the next take. The finalize pass
/// calls this after a successful save.
pub fn persist(paths: &Paths, take: &Path) -> Result<PathBuf> {
    let source = &paths.session_telemetry;
    if !source.is_file() {
        anyhow::bail!("no telemetry was recorded for this take");
    }
    paths::ensure_sidecar_dir(take)?;
    let target = paths::telemetry_path(take);
    std::fs::copy(source, &target)
        .with_context(|| format!("cannot copy telemetry to {}", target.display()))?;
    let _ = std::fs::set_permissions(&target, std::fs::Permissions::from_mode(0o600));
    paths::remove_quiet(&paths::legacy_telemetry_path(take));
    Ok(target)
}

/// A new take clears the runtime file, so a failed save cannot be mistaken for
/// the take that follows it. Every take calls this.
pub fn clear(paths: &Paths) {
    paths::remove_quiet(&paths.session_telemetry);
}

/// Write a diagnostic `start`/`stop` pair when telemetry cannot map its source
/// at all (for example a portal take). The take continues without pointer data.
pub fn record_unavailable(paths: &Paths, space: &CaptureSpace, fps: i64, error: SourceError) {
    let Ok(file) = OpenOptions::new()
        .create(true)
        .truncate(true)
        .write(true)
        .open(&paths.session_telemetry)
    else {
        return;
    };
    let mut writer = TelemetryWriter {
        file,
        t0: Instant::now(),
        t0_ns: monotonic_ns(),
    };
    let code = error.code();
    writer.write(&start_value(space, fps, "none", Some(code), writer.t0_ns));
    let t = writer.t();
    writer.write(&stop_value(t, "error", Some(code)));
}

fn discover_devices(by_id: &Path, event_dir: &Path) -> (Vec<Device>, Option<SourceError>) {
    let by_id_paths = read_dir_sorted(by_id, |name| name.ends_with("-event-mouse"));
    let event_paths = read_dir_sorted(event_dir, |name| {
        name.strip_prefix("event")
            .map(|rest| !rest.is_empty() && rest.chars().all(|ch| ch.is_ascii_digit()))
            .unwrap_or(false)
    });

    let mut seen = std::collections::BTreeSet::new();
    let mut permission_denied = false;
    let mut opened_any = false;
    let mut candidates = 0;
    // Watch both: by-id only lists USB mice, so a laptop touchpad (an i2c
    // device) exists only as a raw event node. Always scanning both also keeps
    // a sleeping mouse from hiding the touchpad.
    let mut devices = open_candidates(
        &by_id_paths,
        &mut seen,
        &mut permission_denied,
        &mut opened_any,
        &mut candidates,
    );
    devices.extend(open_candidates(
        &event_paths,
        &mut seen,
        &mut permission_denied,
        &mut opened_any,
        &mut candidates,
    ));
    if !devices.is_empty() {
        return (devices, None);
    }
    let error = classify_source_error(permission_denied, candidates, opened_any);
    (Vec::new(), Some(error))
}

fn open_candidates(
    paths: &[PathBuf],
    seen: &mut std::collections::BTreeSet<PathBuf>,
    permission_denied: &mut bool,
    opened_any: &mut bool,
    candidates: &mut usize,
) -> Vec<Device> {
    let mut devices = Vec::new();
    for path in paths {
        let key = std::fs::canonicalize(path).unwrap_or_else(|_| path.clone());
        if !seen.insert(key) {
            continue;
        }
        *candidates += 1;
        match OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NONBLOCK)
            .open(path)
        {
            Ok(file) => {
                *opened_any = true;
                if let Some(caps) = pointer_caps(&file) {
                    devices.push(Device {
                        file,
                        name: device_name(path),
                        caps,
                        tap: TapState::default(),
                    });
                }
            }
            Err(err) => {
                if err.kind() == std::io::ErrorKind::PermissionDenied {
                    *permission_denied = true;
                }
            }
        }
    }
    devices
}

fn read_dir_sorted(dir: &Path, keep: impl Fn(&str) -> bool) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut paths: Vec<PathBuf> = entries
        .flatten()
        .filter_map(|entry| {
            let name = entry.file_name().to_string_lossy().to_string();
            keep(&name).then(|| entry.path())
        })
        .collect();
    paths.sort();
    paths
}

fn classify_source_error(
    permission_denied: bool,
    candidates: usize,
    opened_any: bool,
) -> SourceError {
    if permission_denied {
        SourceError::InputGroup
    } else if candidates == 0 || opened_any {
        SourceError::NoDevice
    } else {
        SourceError::OpenFailed
    }
}

/// What a pointer device can report, or `None` when the node is not a pointer.
///
/// A mouse reports `EV_REL` deltas; a laptop touchpad and a touchscreen report
/// `EV_ABS` positions and may only expose taps through `BTN_TOUCH`. The old
/// check required `EV_REL`, which silently skipped every touchpad.
fn pointer_caps(file: &File) -> Option<PointerCaps> {
    let types = capability_bits(file, 0);
    if !has_code(&types, EV_KEY) {
        return None;
    }

    let relative = has_code(&types, EV_REL) && {
        let bits = capability_bits(file, EV_REL as u32);
        has_code(&bits, REL_X) && has_code(&bits, REL_Y)
    };
    let absolute = has_code(&types, EV_ABS) && {
        let bits = capability_bits(file, EV_ABS as u32);
        (has_code(&bits, ABS_X) || has_code(&bits, ABS_MT_POSITION_X))
            && (has_code(&bits, ABS_Y) || has_code(&bits, ABS_MT_POSITION_Y))
    };
    if !relative && !absolute {
        return None;
    }

    let keys = capability_bits(file, EV_KEY as u32);
    let touch = has_code(&keys, BTN_TOUCH);
    if !touch && !has_code(&keys, BTN_LEFT) {
        return None;
    }
    Some(PointerCaps {
        relative,
        absolute,
        touch,
    })
}

fn capability_bits(file: &File, ev: u32) -> [u64; CAP_WORDS] {
    let mut bits = [0u64; CAP_WORDS];
    let len = std::mem::size_of_val(&bits);
    let rc = unsafe { libc::ioctl(file.as_raw_fd(), eviocgbit(ev, len), bits.as_mut_ptr()) };
    if rc < 0 {
        [0u64; CAP_WORDS]
    } else {
        bits
    }
}

fn has_code(bits: &[u64; CAP_WORDS], code: u16) -> bool {
    let word = usize::from(code) / 64;
    let bit = usize::from(code) % 64;
    bits.get(word)
        .map(|word| word & (1u64 << bit) != 0)
        .unwrap_or(false)
}

/// The human name of an event node, for the take log.
fn device_name(path: &Path) -> String {
    let node = path
        .canonicalize()
        .ok()
        .and_then(|path| {
            path.file_name()
                .map(|name| name.to_string_lossy().to_string())
        })
        .or_else(|| {
            path.file_name()
                .map(|name| name.to_string_lossy().to_string())
        })
        .unwrap_or_default();
    std::fs::read_to_string(
        Path::new("/sys/class/input")
            .join(&node)
            .join("device/name"),
    )
    .map(|name| name.trim().to_string())
    .ok()
    .filter(|name| !name.is_empty())
    .unwrap_or(node)
}

/// Linux `_IOC(_IOC_READ, 'E', 0x20 + ev, len)`, the EVIOCGBIT request.
///
/// The request type differs by libc: glibc takes a `c_ulong`, musl takes a
/// `c_int`. `libc::Ioctl` follows the build target.
fn eviocgbit(ev: u32, len: usize) -> libc::Ioctl {
    let read: u64 = 2;
    let request =
        (read << 30) | ((len as u64) << 16) | ((u64::from(b'E')) << 8) | (0x20 + u64::from(ev));
    request as libc::Ioctl
}

struct PointerTracker {
    x: f64,
    y: f64,
}

impl PointerTracker {
    fn known(x: f64, y: f64) -> Self {
        Self { x, y }
    }

    fn apply(&mut self, dx: f64, dy: f64) {
        self.x += dx;
        self.y += dy;
    }

    fn resync(&mut self, x: f64, y: f64) {
        self.x = x;
        self.y = y;
    }

    fn position(&self) -> (f64, f64) {
        (self.x, self.y)
    }
}

fn run_listener(
    mut devices: Vec<Device>,
    space: CaptureSpace,
    writer: Arc<Mutex<TelemetryWriter>>,
    stop: Arc<AtomicBool>,
) {
    // The initial position is not a movement; record one sync so consumers can
    // distinguish a fresh start from a corrected drift. The compositor may be
    // missing on a non-Hyprland session, so fall back to the frame centre.
    let (mut tracker, initial_sync) = match cursor_position() {
        Some((x, y)) => (PointerTracker::known(x, y), Some((x, y))),
        None => {
            let (x, y) = space.centre_logical();
            (PointerTracker::known(x, y), None)
        }
    };
    if let Some((x, y)) = initial_sync {
        let (x, y) = space.to_capture(x, y);
        let mut writer = writer
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let t = writer.t();
        writer.write(&sync_value(t, x, y, "hyprctl"));
    }

    let mut next_sync = Instant::now();
    let mut dx = 0.0;
    let mut dy = 0.0;
    // Taps are synthesized above evdev by the compositor, so only look for
    // them when the compositor turns taps into clicks.
    let taps_enabled = tap_to_click();

    while !stop.load(Ordering::SeqCst) && !devices.is_empty() {
        let mut fds: Vec<libc::pollfd> = devices
            .iter()
            .map(|device| libc::pollfd {
                fd: device.file.as_raw_fd(),
                events: libc::POLLIN,
                revents: 0,
            })
            .collect();
        let ready =
            unsafe { libc::poll(fds.as_mut_ptr(), fds.len() as libc::nfds_t, POLL_TIMEOUT_MS) };
        if ready < 0 {
            let error = std::io::Error::last_os_error();
            if error.kind() == std::io::ErrorKind::Interrupted {
                continue;
            }
            break;
        }

        let mut dead = Vec::new();
        for (index, fd) in fds.iter().enumerate() {
            if fd.revents & (libc::POLLERR | libc::POLLHUP | libc::POLLNVAL) != 0 {
                dead.push(index);
                continue;
            }
            if fd.revents & libc::POLLIN == 0 {
                continue;
            }
            match read_device(&mut devices[index]) {
                Ok((events, count)) => {
                    let caps = devices[index].caps;
                    let tap = &mut devices[index].tap;
                    for event in &events[..count] {
                        handle_event(
                            event,
                            &space,
                            &mut tracker,
                            tap,
                            caps,
                            taps_enabled,
                            &writer,
                            &cursor_position,
                            &mut dx,
                            &mut dy,
                        );
                    }
                }
                Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => {}
                Err(err) if err.kind() == std::io::ErrorKind::Interrupted => {}
                Err(_) => dead.push(index),
            }
        }
        dead.sort_unstable();
        dead.dedup();
        for index in dead.into_iter().rev() {
            devices.remove(index);
        }

        // Resync at about 10 Hz. The compositor position is authoritative;
        // integrating relative events between resyncs keeps motion smooth.
        if Instant::now() >= next_sync {
            if let Some((x, y)) = cursor_position() {
                tracker.resync(x, y);
                let (x, y) = space.to_capture(x, y);
                let mut writer = writer
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner());
                let t = writer.t();
                writer.write(&sync_value(t, x, y, "hyprctl"));
            }
            next_sync = Instant::now() + SYNC_INTERVAL;
        }
    }
}

fn read_device(device: &mut Device) -> std::io::Result<([InputEvent; 64], usize)> {
    let mut events: [InputEvent; 64] = unsafe { std::mem::zeroed() };
    let bytes = unsafe {
        std::slice::from_raw_parts_mut(
            events.as_mut_ptr() as *mut u8,
            std::mem::size_of_val(&events),
        )
    };
    let read = device.file.read(bytes)?;
    let count = read / std::mem::size_of::<InputEvent>();
    Ok((events, count))
}

#[allow(clippy::too_many_arguments)]
fn handle_event(
    event: &InputEvent,
    space: &CaptureSpace,
    tracker: &mut PointerTracker,
    tap: &mut TapState,
    caps: PointerCaps,
    taps_enabled: bool,
    writer: &Arc<Mutex<TelemetryWriter>>,
    cursor: &dyn Fn() -> Option<(f64, f64)>,
    dx: &mut f64,
    dy: &mut f64,
) {
    match event.kind {
        EV_REL => match event.code {
            REL_X => *dx += f64::from(event.value),
            REL_Y => *dy += f64::from(event.value),
            _ => {}
        },
        EV_ABS => match event.code {
            ABS_X | ABS_MT_POSITION_X => tap.track_x(f64::from(event.value)),
            ABS_Y | ABS_MT_POSITION_Y => tap.track_y(f64::from(event.value)),
            _ => {}
        },
        EV_KEY => {
            // A tap only exists as a contact: libinput turns it into a button
            // press above evdev, so watch BTN_TOUCH and synthesize the click.
            if event.code == BTN_TOUCH {
                match event.value {
                    1 => tap.begin(),
                    0 if tap.down_at.is_some() => {
                        let click = taps_enabled && caps.touch && tap.is_tap();
                        tap.down_at = None;
                        if click {
                            emit_tap(writer, space, tracker, cursor);
                        }
                    }
                    _ => {}
                }
                return;
            }
            if matches!(
                event.code,
                BTN_TOOL_DOUBLETAP | BTN_TOOL_TRIPLETAP | BTN_TOOL_QUADTAP | BTN_TOOL_QUINTTAP
            ) {
                if event.value >= 1 {
                    tap.multi = true;
                }
                return;
            }
            let Some(button) = button_name(event.code) else {
                return;
            };
            let state = match event.value {
                0 => "up",
                1 => "down",
                _ => return,
            };
            if state == "down" {
                tap.physical = true;
            }
            flush_move(*dx, *dy, tracker, space, writer);
            *dx = 0.0;
            *dy = 0.0;
            let (x, y) = pointer_position(tracker, space, caps.absolute && !caps.relative, cursor);
            let mut writer = writer
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            let t = writer.t();
            writer.write(&button_value(t, button, state, x, y));
        }
        EV_SYN if event.code == SYN_REPORT => {
            flush_move(*dx, *dy, tracker, space, writer);
            *dx = 0.0;
            *dy = 0.0;
        }
        _ => {}
    }
}

/// Where the pointer is right now, in capture pixels. An absolute device has
/// no deltas to integrate, so ask the compositor and fall back to the last
/// synced position. A relative device uses the tracked position.
fn pointer_position(
    tracker: &PointerTracker,
    space: &CaptureSpace,
    absolute: bool,
    cursor: &dyn Fn() -> Option<(f64, f64)>,
) -> (f64, f64) {
    if absolute {
        if let Some((x, y)) = cursor() {
            return space.to_capture(x, y);
        }
    }
    let (x, y) = tracker.position();
    space.to_capture(x, y)
}

/// Emit one left click for a tap. The contact had no evdev button event, so the
/// compositor is asked for the cursor position the tap moved to.
fn emit_tap(
    writer: &Arc<Mutex<TelemetryWriter>>,
    space: &CaptureSpace,
    tracker: &PointerTracker,
    cursor: &dyn Fn() -> Option<(f64, f64)>,
) {
    let click_t = {
        let writer = writer
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        writer.t()
    };
    // The compositor sees the same kernel event after this process; give it a
    // moment to move the cursor onto the tap before reading the position.
    std::thread::sleep(TAP_SETTLE);
    let (x, y) = pointer_position(tracker, space, true, cursor);
    let mut writer = writer
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    writer.write(&button_value(click_t, "left", "down", x, y));
    writer.write(&button_value(click_t, "left", "up", x, y));
}

/// Whether the compositor turns touchpad taps into clicks. Taps never produce
/// a button event on the device node, so synthesizing one when taps are off
/// would invent clicks the user did not make.
fn tap_to_click() -> bool {
    util::run_stdout_within(
        "hyprctl",
        &["getoption", "input:touchpad:tap-to-click"],
        Duration::from_millis(300),
    )
    .map(|output| output.contains("bool: true"))
    .unwrap_or(false)
}

fn flush_move(
    dx: f64,
    dy: f64,
    tracker: &mut PointerTracker,
    space: &CaptureSpace,
    writer: &Arc<Mutex<TelemetryWriter>>,
) {
    if dx == 0.0 && dy == 0.0 {
        return;
    }
    tracker.apply(dx, dy);
    let (x, y) = tracker.position();
    let (x, y) = space.to_capture(x, y);
    let mut writer = writer
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let t = writer.t();
    writer.write(&move_value(t, x, y));
}

fn button_name(code: u16) -> Option<&'static str> {
    match code {
        BTN_LEFT => Some("left"),
        BTN_RIGHT => Some("right"),
        BTN_MIDDLE => Some("middle"),
        _ => None,
    }
}

fn cursor_position() -> Option<(f64, f64)> {
    let text = util::run_stdout_within("hyprctl", &["cursorpos"], Duration::from_millis(300))?;
    let (x, y) = text.trim().split_once(',')?;
    let x = x.trim().parse::<f64>().ok()?;
    let y = y.trim().parse::<f64>().ok()?;
    (x.is_finite() && y.is_finite()).then_some((x, y))
}

/// `T0` and its nanosecond value on the same clock. Both are CLOCK_MONOTONIC.
pub fn clock_start() -> (Instant, u64) {
    let instant = Instant::now();
    (instant, monotonic_ns())
}

fn monotonic_ns() -> u64 {
    // SAFETY: clock_gettime writes into a timespec owned by this thread.
    unsafe {
        let mut ts: libc::timespec = std::mem::zeroed();
        if libc::clock_gettime(libc::CLOCK_MONOTONIC, &mut ts) == 0 {
            ts.tv_sec as u64 * 1_000_000_000 + ts.tv_nsec as u64
        } else {
            0
        }
    }
}

fn seconds(raw: f64) -> f64 {
    if !raw.is_finite() || raw < 0.0 {
        return 0.0;
    }
    (raw * 1_000_000.0).round() / 1_000_000.0
}

fn round_px(raw: f64) -> f64 {
    if raw.is_finite() {
        (raw * 100.0).round() / 100.0
    } else {
        0.0
    }
}

fn start_value(
    space: &CaptureSpace,
    fps: i64,
    source: &str,
    error: Option<&str>,
    t0_ns: u64,
) -> Value {
    json!({
        "v": VERSION,
        "type": "start",
        "t": 0.0,
        "t0_ns": t0_ns,
        "clock": CLOCK_NAME,
        "capture": {
            "kind": space.kind,
            "target": space.target,
            "monitor": space.monitor,
            "x": space.x,
            "y": space.y,
            "width": space.width,
            "height": space.height,
            "scale": space.scale,
            "transform": space.transform,
        },
        "fps": fps,
        "source": source,
        "error": error,
    })
}

fn move_value(t: f64, x: f64, y: f64) -> Value {
    json!({ "v": VERSION, "type": "move", "t": seconds(t), "x": x, "y": y })
}

fn button_value(t: f64, button: &str, state: &str, x: f64, y: f64) -> Value {
    json!({
        "v": VERSION,
        "type": "button",
        "t": seconds(t),
        "button": button,
        "state": state,
        "x": x,
        "y": y,
    })
}

fn sync_value(t: f64, x: f64, y: f64, source: &str) -> Value {
    json!({
        "v": VERSION,
        "type": "sync",
        "t": seconds(t),
        "x": x,
        "y": y,
        "source": source,
    })
}

fn stop_value(t: f64, reason: &str, error: Option<&str>) -> Value {
    json!({
        "v": VERSION,
        "type": "stop",
        "t": seconds(t),
        "reason": reason,
        "error": error,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    fn monitor_space() -> CaptureSpace {
        CaptureSpace {
            kind: "monitor".into(),
            target: "monitor:DP-1".into(),
            monitor: "DP-1".into(),
            x: 0,
            y: 0,
            source_width: 2560.0,
            source_height: 1440.0,
            width: 2560,
            height: 1440,
            scale: 1.5,
            transform: 0,
        }
    }

    fn temp_file(name: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!(
            "reel-telemetry-{}-{}-{}",
            std::process::id(),
            name,
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        std::fs::create_dir_all(&root).unwrap();
        root.join("session.telemetry.jsonl")
    }

    #[test]
    fn start_line_matches_the_contract() {
        let space = monitor_space();
        let value = start_value(&space, 60, "evdev", None, 123_456_789);
        assert_eq!(value["v"], 1);
        assert_eq!(value["type"], "start");
        assert_eq!(value["t"], 0.0);
        assert_eq!(value["t0_ns"], 123_456_789);
        assert_eq!(value["clock"], "CLOCK_MONOTONIC");
        assert_eq!(value["capture"]["kind"], "monitor");
        assert_eq!(value["capture"]["target"], "monitor:DP-1");
        assert_eq!(value["capture"]["monitor"], "DP-1");
        assert_eq!(value["capture"]["width"], 2560);
        assert_eq!(value["capture"]["height"], 1440);
        assert_eq!(value["capture"]["scale"], 1.5);
        assert_eq!(value["capture"]["transform"], 0);
        assert_eq!(value["fps"], 60);
        assert_eq!(value["source"], "evdev");
        assert!(value["error"].is_null());
    }

    #[test]
    fn event_lines_match_the_contract() {
        let move_line = move_value(1.234_567_8, 100.5, 200.25);
        assert_eq!(move_line["type"], "move");
        assert_eq!(move_line["t"], 1.234_568);
        assert_eq!(move_line["x"], 100.5);
        assert_eq!(move_line["y"], 200.25);

        let button = button_value(1.5, "left", "down", 100.5, 200.25);
        assert_eq!(button["type"], "button");
        assert_eq!(button["button"], "left");
        assert_eq!(button["state"], "down");

        let sync = sync_value(2.0, 100.0, 200.0, "hyprctl");
        assert_eq!(sync["type"], "sync");
        assert_eq!(sync["source"], "hyprctl");

        let stop = stop_value(10.0, "stopped", None);
        assert_eq!(stop["type"], "stop");
        assert_eq!(stop["reason"], "stopped");
        assert!(stop["error"].is_null());
    }

    #[test]
    fn the_fixture_is_a_complete_stream() {
        let fixture = include_str!("../tests/fixtures/telemetry-basic.jsonl");
        let lines: Vec<Value> = fixture
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        assert_eq!(lines.first().unwrap()["type"], "start");
        assert_eq!(lines.last().unwrap()["type"], "stop");
        assert!(lines.iter().all(|line| line["v"] == 1));
        let kinds: Vec<&str> = lines
            .iter()
            .map(|line| line["type"].as_str().unwrap())
            .collect();
        assert_eq!(kinds, vec!["start", "move", "button", "sync", "stop"]);
        assert!(lines
            .iter()
            .all(|line| { line["t"].as_f64().map(|t| t >= 0.0).unwrap_or(false) }));
    }

    #[test]
    fn a_known_monitor_target_is_within_two_pixels() {
        let space = monitor_space();
        // Logical (853.33, 480) is physical (1280, 720) at 1.5x.
        let (x, y) = space.to_capture(853.33, 480.0);
        assert!((x - 1280.0).abs() <= 2.0, "x was {x}");
        assert!((y - 720.0).abs() <= 2.0, "y was {y}");
    }

    #[test]
    fn a_region_and_a_resolution_cap_still_land_within_two_pixels() {
        let mut region = monitor_space();
        region.kind = "region".into();
        region.x = 100;
        region.y = 50;
        region.source_width = 1200.0;
        region.source_height = 900.0;
        region.width = 1200;
        region.height = 900;
        let (x, y) = region.to_capture(500.0, 350.0);
        assert!((x - 600.0).abs() <= 2.0, "x was {x}");
        assert!((y - 450.0).abs() <= 2.0, "y was {y}");

        let mut capped = monitor_space();
        capped.width = 1920;
        capped.height = 1080;
        let (x, y) = capped.to_capture(853.33, 480.0);
        assert!((x - 960.0).abs() <= 2.0, "x was {x}");
        assert!((y - 540.0).abs() <= 2.0, "y was {y}");
    }

    #[test]
    fn an_offset_monitor_maps_to_capture_local_pixels() {
        let mut space = monitor_space();
        space.x = 1920;
        space.scale = 1.0;
        space.source_width = 2560.0;
        space.source_height = 1440.0;
        space.width = 2560;
        space.height = 1440;
        let (x, y) = space.to_capture(2000.0, 300.0);
        assert_eq!((x, y), (80.0, 300.0));
    }

    #[test]
    fn a_transformed_monitor_keeps_its_encoded_size() {
        let mut space = monitor_space();
        space.transform = 1;
        space.source_width = 1440.0;
        space.source_height = 2560.0;
        space.width = 1440;
        space.height = 2560;
        let (x, y) = space.to_capture(100.0, 200.0);
        assert!((x - 150.0).abs() <= 2.0, "x was {x}");
        assert!((y - 300.0).abs() <= 2.0, "y was {y}");
    }

    #[test]
    fn time_is_rounded_to_six_places_and_never_negative() {
        assert_eq!(seconds(-1.0), 0.0);
        assert_eq!(seconds(f64::NAN), 0.0);
        assert_eq!(seconds(0.000_000_4), 0.0);
        assert_eq!(seconds(1.234_567_8), 1.234_568);
    }

    #[test]
    fn resync_stops_drift_over_sixty_seconds() {
        // The device reports 0.4% too much movement; without resync that is
        // 24 px of drift over 60 s. The 10 Hz compositor resync clamps it.
        let mut tracker = PointerTracker::known(0.0, 0.0);
        let (mut truth_x, mut truth_y) = (0.0, 0.0);
        let mut raw_x = 0.0;
        for _ in 0..600 {
            for _ in 0..10 {
                truth_x += 1.0;
                truth_y += 0.5;
                tracker.apply(1.004, 0.502);
                raw_x += 1.004;
            }
            tracker.resync(truth_x, truth_y);
        }
        let (x, y) = tracker.position();
        assert!((x - truth_x).abs() <= 2.0, "x drift was {}", x - truth_x);
        assert!((y - truth_y).abs() <= 2.0, "y drift was {}", y - truth_y);
        assert!(
            raw_x - truth_x > 2.0,
            "the test needs real drift to correct"
        );
    }

    fn test_writer(label: &str) -> (PathBuf, Arc<Mutex<TelemetryWriter>>) {
        let path = temp_file(label);
        let file = OpenOptions::new()
            .create(true)
            .truncate(true)
            .write(true)
            .open(&path)
            .unwrap();
        let writer = Arc::new(Mutex::new(TelemetryWriter {
            file,
            t0: Instant::now(),
            t0_ns: 1,
        }));
        (path, writer)
    }

    fn event(kind: u16, code: u16, value: i32) -> InputEvent {
        InputEvent {
            _time: unsafe { std::mem::zeroed() },
            kind,
            code,
            value,
        }
    }

    fn read_lines(path: &Path) -> Vec<Value> {
        std::fs::read_to_string(path)
            .unwrap()
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect()
    }

    fn flat_space() -> CaptureSpace {
        let mut space = monitor_space();
        space.scale = 1.0;
        space.source_width = 2560.0;
        space.source_height = 1440.0;
        space
    }

    fn mouse_caps() -> PointerCaps {
        PointerCaps {
            relative: true,
            absolute: false,
            touch: false,
        }
    }

    fn touchpad_caps() -> PointerCaps {
        PointerCaps {
            relative: false,
            absolute: true,
            touch: true,
        }
    }

    #[test]
    fn relative_events_and_a_click_become_move_and_button_lines() {
        let (path, writer) = test_writer("events");
        let space = flat_space();
        let mut tracker = PointerTracker::known(100.0, 200.0);
        let (mut dx, mut dy) = (0.0, 0.0);
        let mut tap = TapState::default();
        let cursor = || None;

        for event in [
            event(EV_REL, REL_X, 10),
            event(EV_REL, REL_Y, 5),
            event(EV_SYN, SYN_REPORT, 0),
            event(EV_KEY, BTN_LEFT, 1),
        ] {
            handle_event(
                &event,
                &space,
                &mut tracker,
                &mut tap,
                mouse_caps(),
                true,
                &writer,
                &cursor,
                &mut dx,
                &mut dy,
            );
        }
        drop(writer);

        let lines = read_lines(&path);
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0]["type"], "move");
        assert_eq!(lines[0]["x"], 110.0);
        assert_eq!(lines[0]["y"], 205.0);
        assert_eq!(lines[1]["type"], "button");
        assert_eq!(lines[1]["button"], "left");
        assert_eq!(lines[1]["state"], "down");
        assert_eq!(lines[1]["x"], 110.0);
        assert_eq!(lines[1]["y"], 205.0);
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn a_touchpad_tap_becomes_a_left_click_at_the_cursor() {
        // A tap never produces BTN_LEFT on the event node; only BTN_TOUCH is
        // visible, so the listener synthesizes the click.
        let (path, writer) = test_writer("tap");
        let space = flat_space();
        let mut tracker = PointerTracker::known(0.0, 0.0);
        let (mut dx, mut dy) = (0.0, 0.0);
        let mut tap = TapState::default();
        let cursor = || Some((640.0, 480.0));

        for event in [
            event(EV_ABS, ABS_X, 1200),
            event(EV_ABS, ABS_Y, 800),
            event(EV_KEY, BTN_TOUCH, 1),
            event(EV_SYN, SYN_REPORT, 0),
            event(EV_ABS, ABS_X, 1201),
            event(EV_KEY, BTN_TOUCH, 0),
        ] {
            handle_event(
                &event,
                &space,
                &mut tracker,
                &mut tap,
                touchpad_caps(),
                true,
                &writer,
                &cursor,
                &mut dx,
                &mut dy,
            );
        }
        drop(writer);

        let lines = read_lines(&path);
        assert_eq!(lines.len(), 2, "a tap is a press and a release");
        assert_eq!(lines[0]["type"], "button");
        assert_eq!(lines[0]["button"], "left");
        assert_eq!(lines[0]["state"], "down");
        assert_eq!(lines[0]["x"], 640.0);
        assert_eq!(lines[0]["y"], 480.0);
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn a_drag_or_a_second_finger_is_not_a_tap() {
        for (label, middle) in [
            (
                "drag",
                vec![event(EV_ABS, ABS_X, 1200), event(EV_ABS, ABS_X, 1800)],
            ),
            ("two fingers", vec![event(EV_KEY, BTN_TOOL_DOUBLETAP, 1)]),
        ] {
            let (path, writer) = test_writer(label);
            let space = flat_space();
            let mut tracker = PointerTracker::known(0.0, 0.0);
            let (mut dx, mut dy) = (0.0, 0.0);
            let mut tap = TapState::default();
            let cursor = || Some((640.0, 480.0));

            let mut events = vec![event(EV_KEY, BTN_TOUCH, 1)];
            events.extend(middle);
            events.push(event(EV_KEY, BTN_TOUCH, 0));
            for event in events {
                handle_event(
                    &event,
                    &space,
                    &mut tracker,
                    &mut tap,
                    touchpad_caps(),
                    true,
                    &writer,
                    &cursor,
                    &mut dx,
                    &mut dy,
                );
            }
            drop(writer);
            assert!(
                read_lines(&path).is_empty(),
                "{label} must not become a click"
            );
            let _ = std::fs::remove_dir_all(path.parent().unwrap());
        }
    }

    #[test]
    fn a_physical_click_does_not_also_emit_a_tap() {
        let (path, writer) = test_writer("physical");
        let space = flat_space();
        let mut tracker = PointerTracker::known(0.0, 0.0);
        let (mut dx, mut dy) = (0.0, 0.0);
        let mut tap = TapState::default();
        let cursor = || Some((640.0, 480.0));

        for event in [
            event(EV_KEY, BTN_TOUCH, 1),
            event(EV_KEY, BTN_LEFT, 1),
            event(EV_KEY, BTN_LEFT, 0),
            event(EV_KEY, BTN_TOUCH, 0),
        ] {
            handle_event(
                &event,
                &space,
                &mut tracker,
                &mut tap,
                touchpad_caps(),
                true,
                &writer,
                &cursor,
                &mut dx,
                &mut dy,
            );
        }
        drop(writer);

        let lines = read_lines(&path);
        assert_eq!(lines.len(), 2, "press and release, not four lines");
        assert_eq!(lines[0]["state"], "down");
        assert_eq!(lines[1]["state"], "up");
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn taps_stay_silent_when_tap_to_click_is_off() {
        let (path, writer) = test_writer("tap-off");
        let space = flat_space();
        let mut tracker = PointerTracker::known(0.0, 0.0);
        let (mut dx, mut dy) = (0.0, 0.0);
        let mut tap = TapState::default();
        let cursor = || Some((640.0, 480.0));

        for event in [event(EV_KEY, BTN_TOUCH, 1), event(EV_KEY, BTN_TOUCH, 0)] {
            handle_event(
                &event,
                &space,
                &mut tracker,
                &mut tap,
                touchpad_caps(),
                false,
                &writer,
                &cursor,
                &mut dx,
                &mut dy,
            );
        }
        drop(writer);
        assert!(read_lines(&path).is_empty());
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn the_writer_flushes_every_line() {
        let path = temp_file("flush");
        let file = OpenOptions::new()
            .create(true)
            .truncate(true)
            .write(true)
            .open(&path)
            .unwrap();
        let mut writer = TelemetryWriter {
            file,
            t0: Instant::now(),
            t0_ns: 1,
        };
        writer.write(&move_value(1.0, 2.0, 3.0));
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.ends_with('\n'));
        let parsed: Value = serde_json::from_str(text.trim()).unwrap();
        assert_eq!(parsed["type"], "move");
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn a_failed_source_writes_start_then_stop() {
        let path = temp_file("failure");
        let file = OpenOptions::new()
            .create(true)
            .truncate(true)
            .write(true)
            .open(&path)
            .unwrap();
        let prepared = Prepared {
            file: Some(file),
            devices: Vec::new(),
            error: Some(SourceError::InputGroup),
        };
        prepared.record_failure(&monitor_space(), 60, SourceError::InputGroup);
        let text = std::fs::read_to_string(&path).unwrap();
        let lines: Vec<Value> = text
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0]["type"], "start");
        assert_eq!(lines[0]["source"], "none");
        assert_eq!(lines[0]["error"], "input-group");
        assert_eq!(lines[1]["type"], "stop");
        assert_eq!(lines[1]["reason"], "error");
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn persisting_copies_the_runtime_file_next_to_the_take() {
        let root = std::env::temp_dir().join(format!(
            "reel-telemetry-persist-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        std::fs::create_dir_all(&root).unwrap();
        let paths = Paths::for_test(&root);
        let videos = root.join("Videos");
        std::fs::create_dir_all(&videos).unwrap();
        std::fs::write(&paths.session_telemetry, b"{}\n").unwrap();
        let take = videos.join("reel-1.mp4");
        let legacy = paths::legacy_telemetry_path(&take);
        std::fs::write(&legacy, b"old\n").unwrap();
        let saved = persist(&paths, &take).unwrap();
        assert_eq!(saved, videos.join(".reel/reel-1.telemetry.jsonl"));
        assert!(!legacy.exists(), "old visible sidecar is cleaned up");
        assert!(saved.is_file());
        assert!(
            paths.session_telemetry.is_file(),
            "the runtime copy stays for debugging"
        );
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn source_errors_name_the_fix() {
        assert_eq!(SourceError::InputGroup.code(), "input-group");
        assert!(SourceError::InputGroup.hint().is_some());
        assert_eq!(SourceError::NoDevice.code(), "no-device");
        assert_eq!(SourceError::OpenFailed.code(), "open-failed");
        assert_eq!(
            classify_source_error(true, 2, false),
            SourceError::InputGroup
        );
        assert_eq!(
            classify_source_error(false, 0, false),
            SourceError::NoDevice
        );
        assert_eq!(
            classify_source_error(false, 2, false),
            SourceError::OpenFailed
        );
        assert_eq!(classify_source_error(false, 2, true), SourceError::NoDevice);
    }

    #[test]
    fn empty_input_directories_report_no_device() {
        let root = std::env::temp_dir().join(format!(
            "reel-telemetry-empty-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        let by_id = root.join("by-id");
        let events = root.join("event");
        std::fs::create_dir_all(&by_id).unwrap();
        std::fs::create_dir_all(&events).unwrap();
        let (devices, error) = discover_devices(&by_id, &events);
        assert!(devices.is_empty());
        assert_eq!(error, Some(SourceError::NoDevice));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn the_input_event_layout_matches_the_kernel() {
        assert_eq!(std::mem::size_of::<InputEvent>() % 8, 0);
        #[cfg(target_pointer_width = "64")]
        assert_eq!(std::mem::size_of::<InputEvent>(), 24);
    }

    #[test]
    fn resolution_caps_are_parsed_strictly() {
        assert_eq!(parse_resolution("0x0"), None);
        assert_eq!(parse_resolution(""), None);
        assert_eq!(parse_resolution("1920x1080"), Some((1920, 1080)));
        assert_eq!(parse_resolution("nonsense"), None);
        assert_eq!(parse_resolution("0x1080"), None);
    }
}
