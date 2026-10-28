use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::fs::File;
use std::io::{BufRead, BufReader, BufWriter, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use crate::util;

/// How auto zoom should be produced.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Off,
    /// Record full frames plus a keyframe sidecar, then apply the camera move in
    /// the ffmpeg save pass. Smooth, non-destructive, works on any compositor.
    Post,
    /// Drive the compositor magnifier live while recording. Experimental.
    Live,
}

impl Mode {
    pub fn from_value(value: Option<&Value>) -> Mode {
        let raw = match value {
            Some(Value::Bool(true)) => "post_process".to_string(),
            Some(Value::Bool(false)) => "off".to_string(),
            Some(Value::Null) | None => "post_process".to_string(),
            Some(Value::String(text)) => text.clone(),
            Some(other) => other.to_string(),
        };
        match raw.trim().to_ascii_lowercase().as_str() {
            "off" | "false" | "no" | "0" | "none" => Mode::Off,
            "live" | "live_compositor" | "compositor" => Mode::Live,
            // Anything else (including default "post_process", "post", "on", "") is Post.
            _ => Mode::Post,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Mode::Off => "off",
            Mode::Post => "post_process",
            Mode::Live => "live_compositor",
        }
    }
}



/// Everything the tracker needs. Resolved once when the take starts.
#[derive(Debug, Clone)]
pub struct Settings {
    pub mode: Mode,
    pub max_zoom: f64,
    pub typing_zoom: f64,
    pub omega: f64,
    pub idle_timeout: Duration,
    pub unzoom_speed: f64,
    pub follow_mouse: bool,
    pub focus_auto_zoom: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            mode: Mode::Post,
            max_zoom: 1.9,
            typing_zoom: 1.75,
            omega: 6.0,
            idle_timeout: Duration::from_millis(6000),
            unzoom_speed: 2600.0,
            follow_mouse: true,
            focus_auto_zoom: true,
        }
    }
}

fn switch(value: Option<&Value>) -> bool {
    match value {
        Some(Value::Bool(flag)) => *flag,
        Some(Value::String(text)) => util::normalize_switch(text),
        Some(Value::Number(number)) => number.as_f64().unwrap_or(0.0) != 0.0,
        _ => true,
    }
}

fn clamp_zoom(value: f64, fallback: f64) -> f64 {
    if value.is_finite() && value >= 1.05 {
        value.min(3.0)
    } else {
        fallback
    }
}

/// Read the auto zoom block out of an already-loaded settings object.
pub fn settings_from(settings: &Value) -> Settings {
    let mut out = Settings::default();

    let raw = match (settings.get("autoZoom"), settings.get("autoZoomMode")) {
        (Some(Value::String(s)), _) if s == "live" || s == "live_compositor" || s == "post_process" || s == "off" => {
            Some(Value::String(s.clone()))
        }
        (_, Some(Value::String(s))) if s == "live" || s == "live_compositor" || s == "post_process" || s == "off" => {
            Some(Value::String(s.clone()))
        }
        (Some(val), _) if !val.as_str().unwrap_or("").is_empty() => Some(val.clone()),
        (_, Some(val)) if !val.as_str().unwrap_or("").is_empty() => Some(val.clone()),
        _ => None,
    };
    out.mode = Mode::from_value(raw.as_ref());

    if let Some(value) = settings.get("autoZoomMax").and_then(Value::as_f64) {
        out.max_zoom = clamp_zoom(value, out.max_zoom);
    }
    if let Some(value) = settings.get("autoZoomSpeed").and_then(Value::as_f64) {
        out.unzoom_speed = value.clamp(200.0, 8000.0);
    }
    if let Some(value) = settings.get("autoZoomIdle").and_then(Value::as_f64) {
        out.idle_timeout = Duration::from_millis((value.max(800.0) as u64).min(20_000));
    }
    out.follow_mouse = switch(settings.get("followMouse"));
    out.focus_auto_zoom = if let Some(val) = settings.get("focusAutoZoom") {
        switch(Some(val))
    } else if let Some(val) = settings.get("followKeyboard") {
        // Migration: "off"/"false" → false, anything else ("window"/"caret"/true) → true
        match val {
            Value::Bool(b) => *b,
            Value::String(s) => !matches!(s.trim().to_ascii_lowercase().as_str(), "off" | "false" | "none" | "0"),
            _ => true,
        }
    } else {
        true
    };
    out
}

// ---------- keyframes ----------------------------------------------------

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct Keyframe {
    /// Seconds since the take started.
    pub t: f64,
    /// Focus point in logical desktop coordinates.
    pub x: f64,
    pub y: f64,
    /// 1.0 is the whole frame.
    pub z: f64,
}

pub fn read_keyframes(path: &Path) -> Vec<Keyframe> {
    let Ok(text) = std::fs::read_to_string(path) else {
        return Vec::new();
    };
    text.lines()
        .filter_map(|line| serde_json::from_str::<Keyframe>(line.trim()).ok())
        .filter(|frame| frame.t.is_finite() && frame.z.is_finite())
        .collect()
}

// ---------- capture geometry --------------------------------------------

/// A logical desktop rectangle: where the captured pixels live on screen.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Geometry {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

struct Monitor {
    name: String,
    x: f64,
    y: f64,
    width: f64,
    height: f64,
    scale: f64,
}

fn monitors() -> Vec<Monitor> {
    let Some(text) =
        util::run_stdout_within("hyprctl", &["monitors", "-j"], Duration::from_secs(2))
    else {
        return Vec::new();
    };
    let Ok(Value::Array(list)) = serde_json::from_str::<Value>(&text) else {
        return Vec::new();
    };
    list.iter()
        .filter_map(|entry| {
            Some(Monitor {
                name: entry.get("name")?.as_str()?.to_string(),
                x: entry.get("x")?.as_f64()?,
                y: entry.get("y")?.as_f64()?,
                width: entry.get("width")?.as_f64()?,
                height: entry.get("height")?.as_f64()?,
                scale: entry.get("scale").and_then(Value::as_f64).unwrap_or(1.0),
            })
        })
        .collect()
}

fn logical(monitor: &Monitor) -> Geometry {
    let scale = if monitor.scale > 0.1 { monitor.scale } else { 1.0 };
    Geometry {
        x: monitor.x,
        y: monitor.y,
        width: monitor.width / scale,
        height: monitor.height / scale,
    }
}

fn split_at_sign(text: &str) -> Option<(f64, &str)> {
    let bytes = text.as_bytes();
    for index in 1..bytes.len() {
        if bytes[index] == b'+' {
            let value: f64 = text[..index].parse().ok()?;
            return Some((value, &text[index + 1..]));
        }
    }
    None
}

fn parse_region(body: &str) -> Option<Geometry> {
    let (width, rest) = body.split_once('x')?;
    let width: f64 = width.parse().ok()?;
    let (height, rest) = split_at_sign(rest)?;
    let (x, y) = split_at_sign(rest)?;
    let y: f64 = y.parse().ok()?;
    if width < 8.0 || height < 8.0 {
        return None;
    }
    Some(Geometry {
        x,
        y,
        width,
        height,
    })
}

/// Resolve a saved capture target to the logical rectangle it covers.
pub fn geometry_for(target: &str) -> Option<Geometry> {
    let list = monitors();
    if let Some(name) = target.strip_prefix("monitor:") {
        return list.iter().find(|m| m.name == name).map(logical);
    }
    if let Some(body) = target.strip_prefix("region:") {
        return parse_region(body);
    }
    // Portal and unknown targets: assume the primary monitor was captured.
    list.first().map(logical)
}

/// Critically damped spring (zero overshoot), sampled at the tracker rate.
#[derive(Debug, Clone, Copy)]
pub struct Spring {
    pub pos: f64,
    pub vel: f64,
}

impl Spring {
    pub fn new(value: f64) -> Self {
        Self {
            pos: value,
            vel: 0.0,
        }
    }

    pub fn step(&mut self, target: f64, dt: f64, omega: f64) {
        let delta = self.pos - target;
        let decay = (-omega * dt).exp();
        self.pos = target + (delta + (self.vel + omega * delta) * dt) * decay;
        self.vel = (self.vel - (self.vel + omega * delta) * omega * dt) * decay;
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WindowRect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

pub fn hypr_active_window_rect() -> Option<WindowRect> {
    let out = util::run_stdout_within("hyprctl", &["activewindow", "-j"], Duration::from_millis(700))?;
    let value: Value = serde_json::from_str(&out).ok()?;
    let at = value.get("at")?.as_array()?;
    let size = value.get("size")?.as_array()?;
    let x = at.first()?.as_f64()?;
    let y = at.get(1)?.as_f64()?;
    let width = size.first()?.as_f64()?;
    let height = size.get(1)?.as_f64()?;
    if width < 8.0 || height < 8.0 {
        return None;
    }
    Some(WindowRect { x, y, width, height })
}

/// Dynamic Fit-to-Window Zoom Calculation:
/// Computes ideal zoom factor Z so the active window plus border margin P fits
/// inside the capture area. If the window is nearly fullscreen, stays at 1.0x.
/// Fit the active window into the capture area showing 100% of the window and borders.
pub fn calculate_window_framing(
    win: WindowRect,
    geometry: Geometry,
    max_zoom: f64,
    padding: f64,
) -> (f64, f64, f64) {
    if win.width <= 16.0
        || win.height <= 16.0
        || geometry.width <= 16.0
        || geometry.height <= 16.0
    {
        let cx = geometry.x + geometry.width / 2.0;
        let cy = geometry.y + geometry.height / 2.0;
        return (cx, cy, 1.0);
    }

    let is_full_w = win.width >= 0.85 * geometry.width;
    let is_full_h = win.height >= 0.85 * geometry.height;

    // 1. Truly fullscreen or maximized window (fills both width and height):
    // Stay cleanly at 1.0x full view at capture center, no arbitrary crop.
    if is_full_w && is_full_h {
        let cx = geometry.x + geometry.width / 2.0;
        let cy = geometry.y + geometry.height / 2.0;
        return (cx, cy, 1.0);
    }

    let padded_w = win.width + 2.0 * padding;
    let padded_h = win.height + 2.0 * padding;
    let z_fit_w = geometry.width / padded_w.max(1.0);
    let z_fit_h = geometry.height / padded_h.max(1.0);

    // Zoom factor that preserves 100% of the active window and borders
    // without cropping either dimension (just like on smaller windows):
    let z_fit = z_fit_w.min(z_fit_h);

    if z_fit < 1.06 {
        let cx = geometry.x + geometry.width / 2.0;
        let cy = geometry.y + geometry.height / 2.0;
        return (cx, cy, 1.0);
    }

    let z = z_fit.clamp(1.0, max_zoom);
    let view_w = geometry.width / z;
    let view_h = geometry.height / z;

    let center_x = (win.x + win.width / 2.0).clamp(
        geometry.x + view_w / 2.0,
        geometry.x + geometry.width - view_w / 2.0,
    );
    let center_y = (win.y + win.height / 2.0).clamp(
        geometry.y + view_h / 2.0,
        geometry.y + geometry.height - view_h / 2.0,
    );
    (center_x, center_y, z)
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CaretDeadzone {
    pub anchor_x: f64,
    pub anchor_y: f64,
}

impl CaretDeadzone {
    pub fn new(x: f64, y: f64) -> Self {
        Self {
            anchor_x: x,
            anchor_y: y,
        }
    }

    /// Updates the deadzone anchor based on caret position.
    /// The deadzone is an elongated horizontal box (30% viewport width, 16% height).
    pub fn update(
        &mut self,
        caret_x: f64,
        caret_y: f64,
        viewport_w: f64,
        viewport_h: f64,
    ) -> (f64, f64) {
        let half_w = viewport_w * 0.15;
        let half_h = viewport_h * 0.08;

        let dx = caret_x - self.anchor_x;
        let dy = caret_y - self.anchor_y;

        // Discontinuous jumps (e.g. clicked in a different field or paragraph): re-center anchor
        if dx.abs() > viewport_w * 0.55 || dy.abs() > viewport_h * 0.5 {
            self.anchor_x = caret_x;
            self.anchor_y = caret_y;
            return (self.anchor_x, self.anchor_y);
        }

        if dx.abs() > half_w {
            self.anchor_x = if dx > 0.0 {
                caret_x - half_w
            } else {
                caret_x + half_w
            };
        }
        if dy.abs() > half_h {
            self.anchor_y = if dy > 0.0 {
                caret_y - half_h
            } else {
                caret_y + half_h
            };
        }
        (self.anchor_x, self.anchor_y)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FocusSource {
    IdleCenter,
    MouseDwell,
    MouseClick,
    WindowFraming,
    CaretTyping,
}

#[derive(Debug, Clone)]
pub struct CameraController {
    pub sx: Spring,
    pub sy: Spring,
    pub sz: Spring,
    pub geometry: Geometry,
    pub center_x: f64,
    pub center_y: f64,
    pub settings: Settings,
    pub target_x: f64,
    pub target_y: f64,
    pub target_z: f64,
    pub deadzone: Option<CaretDeadzone>,
    pub last_interaction: Instant,
    pub last_typing: Instant,
    pub last_mouse_pos: (f64, f64),
    pub mouse_in_control: bool,
    pub last_source: FocusSource,
    pub last_valid_window: Option<(WindowRect, Instant)>,
    pub last_window_rect: Option<WindowRect>,
}

impl CameraController {
    pub fn new(geometry: Geometry, settings: Settings) -> Self {
        let center_x = geometry.x + geometry.width / 2.0;
        let center_y = geometry.y + geometry.height / 2.0;
        Self {
            sx: Spring::new(center_x),
            sy: Spring::new(center_y),
            sz: Spring::new(1.0),
            geometry,
            center_x,
            center_y,
            target_x: center_x,
            target_y: center_y,
            target_z: 1.0,
            deadzone: None,
            last_interaction: Instant::now(),
            last_typing: Instant::now() - Duration::from_secs(60),
            last_mouse_pos: (center_x, center_y),
            mouse_in_control: settings.follow_mouse && !settings.focus_auto_zoom,
            last_source: FocusSource::IdleCenter,
            last_valid_window: None,
            last_window_rect: None,
            settings,
        }
    }

    pub fn step(
        &mut self,
        dt: f64,
        now: Instant,
        cursor: (f64, f64),
        cursor_speed: f64,
        clicked: Option<(f64, f64, Instant)>,
        typed: bool,
        active_win: Option<(WindowRect, Instant)>,
        caret_pos: Option<(f64, f64, Instant)>,
    ) -> (f64, f64, f64, FocusSource) {
        // Track valid active window and detect genuine window focus changes
        if let Some((win, at)) = active_win {
            let is_new = self.last_window_rect.map_or(true, |prev| prev != win);
            if is_new {
                self.last_window_rect = Some(win);
                self.last_interaction = now;
                self.mouse_in_control = false;
            }
            self.last_valid_window = Some((win, at));
        }

        // 1. Typing activity or window focus change takes priority
        if typed {
            self.last_typing = now;
            self.last_interaction = now;
            self.mouse_in_control = false;
        }

        // 2. Intentional mouse movement hands priority back to mouse if follow_mouse is ON
        if self.settings.follow_mouse {
            let moved_dist = (cursor.0 - self.last_mouse_pos.0).hypot(cursor.1 - self.last_mouse_pos.1);
            let click_fresh = clicked.is_some_and(|(_, _, at)| now.duration_since(at) < Duration::from_millis(600));
            let typing_elapsed = now.duration_since(self.last_typing);

            if click_fresh || moved_dist > 35.0 || (cursor_speed > 30.0 && typing_elapsed > Duration::from_millis(2200)) {
                if moved_dist > 10.0 || click_fresh {
                    self.last_mouse_pos = cursor;
                    self.last_interaction = now;
                    self.mouse_in_control = true;
                }
            }
        } else {
            // When follow_mouse is OFF: mouse can NEVER gain control
            self.mouse_in_control = false;
        }

        // 3. Determine target
        let typing_active = now.duration_since(self.last_typing) < Duration::from_millis(1800);
        let mut source = FocusSource::IdleCenter;

        // Window hold: if active_win is transiently None (e.g. during a compositor focus handover),
        // hold the previous valid window for up to 600ms so the camera glides smoothly between windows
        // rather than plunging to 1.0x fullscreen and re-zooming.
        let effective_win = active_win
            .map(|(w, _)| w)
            .or_else(|| {
                self.last_valid_window
                    .filter(|(_, at)| now.duration_since(*at) < Duration::from_millis(600))
                    .map(|(w, _)| w)
            });

        let win_framing = effective_win.map(|win| {
            calculate_window_framing(win, self.geometry, self.settings.max_zoom, 28.0)
        });
        let is_unzoomable = win_framing.is_some_and(|(_, _, wz)| wz <= 1.06);

        if !self.mouse_in_control && self.settings.focus_auto_zoom {
            let mut handled = false;

            if is_unzoomable {
                // Window is fullscreen, maximized, or in a multi-window layout spanning screen dimensions:
                // Camera must NEVER zoom in (do not zoom into caret or crop the window).
                // Always stay full frame at 1.0x.
                self.deadzone = None;
                self.target_x = self.center_x;
                self.target_y = self.center_y;
                self.target_z = 1.0;
                source = FocusSource::WindowFraming;
                handled = true;
            } else {
                // State C: User is typing in a smaller window → zoom in & smoothly follow the caret
                if typing_active {
                    if let Some((cx, cy, cat)) = caret_pos {
                        if now.duration_since(cat) < Duration::from_millis(1500) {
                            let z_win = win_framing.map_or(1.0, |(_, _, wz)| wz);
                            let tz = (z_win * 1.5)
                                .max(self.settings.typing_zoom)
                                .min(self.settings.max_zoom);
                            let vw = self.geometry.width / tz;
                            let vh = self.geometry.height / tz;

                            let (dz_x, dz_y) = match self.deadzone {
                                Some(ref mut dz) => dz.update(cx, cy, vw, vh),
                                None => {
                                    let dz = CaretDeadzone::new(cx, cy);
                                    self.deadzone = Some(dz);
                                    (cx, cy)
                                }
                            };

                            let mut tx = dz_x;
                            let mut ty = dz_y;

                            // Window-Bounded Clamping: clamp zoomed caret viewport to active window's padded bounds
                            if let Some(win) = effective_win {
                                let pad = 28.0;
                                let pad_w = win.width + 2.0 * pad;
                                let pad_h = win.height + 2.0 * pad;

                                if vw <= pad_w {
                                    let min_tx = win.x - pad + vw / 2.0;
                                    let max_tx = win.x + win.width + pad - vw / 2.0;
                                    tx = tx.clamp(min_tx, max_tx);
                                }
                                if vh <= pad_h {
                                    let min_ty = win.y - pad + vh / 2.0;
                                    let max_ty = win.y + win.height + pad - vh / 2.0;
                                    ty = ty.clamp(min_ty, max_ty);
                                }
                            }

                            // Also clamp to overall capture geometry:
                            tx = tx.clamp(
                                self.geometry.x + vw / 2.0,
                                self.geometry.x + self.geometry.width - vw / 2.0,
                            );
                            ty = ty.clamp(
                                self.geometry.y + vh / 2.0,
                                self.geometry.y + self.geometry.height - vh / 2.0,
                            );

                            self.target_x = tx;
                            self.target_y = ty;
                            self.target_z = tz;
                            source = FocusSource::CaretTyping;
                            handled = true;
                        }
                    }
                }

                // State B: Window is active (not typing or caret unavailable) → frame the full active window
                if !handled {
                    self.deadzone = None;
                    if let Some((wx, wy, wz)) = win_framing {
                        self.target_x = wx;
                        self.target_y = wy;
                        self.target_z = wz;
                        source = FocusSource::WindowFraming;
                        handled = true;
                    }
                }
            }

            // State A: No active window (or hold expired) → full screen at 1.0x
            if !handled {
                self.deadzone = None;
                self.target_x = self.center_x;
                self.target_y = self.center_y;
                self.target_z = 1.0;
                source = FocusSource::IdleCenter;
            }
        } else if self.mouse_in_control && self.settings.follow_mouse {
            self.deadzone = None;
            let click_fresh = clicked.is_some_and(|(_, _, at)| now.duration_since(at) < Duration::from_millis(600));

            if is_unzoomable {
                // Window is fullscreen, maximized, or in a large split layout:
                // Camera must NEVER zoom in (whether clicking, dwelling, or moving anywhere on screen).
                // Always stay full frame at 1.0x so 100% of the window is visible.
                self.target_x = self.center_x;
                self.target_y = self.center_y;
                self.target_z = 1.0;
                source = if click_fresh { FocusSource::MouseClick } else { FocusSource::MouseDwell };
            } else {
                // Determine if the mouse interaction is within an active window
                let win_under_cursor = effective_win.filter(|win| {
                    cursor.0 >= win.x - 12.0
                        && cursor.0 <= win.x + win.width + 12.0
                        && cursor.1 >= win.y - 12.0
                        && cursor.1 <= win.y + win.height + 12.0
                });

                if let Some(win) = win_under_cursor {
                    let (wx, wy, wz) = calculate_window_framing(win, self.geometry, self.settings.max_zoom, 28.0);
                    let allowed_z = wz.min(self.settings.max_zoom);
                    if click_fresh {
                        let (cx, cy, _) = clicked.unwrap();
                        let view_w = self.geometry.width / allowed_z;
                        let view_h = self.geometry.height / allowed_z;
                        let pad = 28.0;
                        let min_tx = (win.x + win.width + pad - view_w / 2.0).min(wx);
                        let max_tx = (win.x - pad + view_w / 2.0).max(wx);
                        let min_ty = (win.y + win.height + pad - view_h / 2.0).min(wy);
                        let max_ty = (win.y - pad + view_h / 2.0).max(wy);
                        self.target_x = cx.clamp(min_tx.min(max_tx), min_tx.max(max_tx));
                        self.target_y = cy.clamp(min_ty.min(max_ty), min_ty.max(max_ty));
                        self.target_z = allowed_z;
                        source = FocusSource::MouseClick;
                    } else if cursor_speed > self.settings.unzoom_speed {
                        self.target_x = cursor.0;
                        self.target_y = cursor.1;
                        self.target_z = 1.05;
                        source = FocusSource::MouseDwell;
                    } else if now.duration_since(self.last_interaction) < self.settings.idle_timeout {
                        let view_w = self.geometry.width / allowed_z;
                        let view_h = self.geometry.height / allowed_z;
                        let pad = 28.0;
                        let min_tx = (win.x + win.width + pad - view_w / 2.0).min(wx);
                        let max_tx = (win.x - pad + view_w / 2.0).max(wx);
                        let min_ty = (win.y + win.height + pad - view_h / 2.0).min(wy);
                        let max_ty = (win.y - pad + view_h / 2.0).max(wy);
                        self.target_x = cursor.0.clamp(min_tx.min(max_tx), min_tx.max(max_tx));
                        self.target_y = cursor.1.clamp(min_ty.min(max_ty), min_ty.max(max_ty));
                        self.target_z = allowed_z;
                        source = FocusSource::MouseDwell;
                    } else {
                        // Mouse idle fallback
                        if self.settings.focus_auto_zoom {
                            self.target_x = wx;
                            self.target_y = wy;
                            self.target_z = wz;
                            source = FocusSource::WindowFraming;
                            self.mouse_in_control = false;
                        } else {
                            self.target_x = self.center_x;
                            self.target_y = self.center_y;
                            self.target_z = 1.0;
                            source = FocusSource::IdleCenter;
                            self.mouse_in_control = false;
                        }
                    }
                } else {
                    // Free desktop pointer interaction (no window under cursor and no unzoomable window)
                    if click_fresh {
                        let (cx, cy, _) = clicked.unwrap();
                        self.target_x = cx;
                        self.target_y = cy;
                        self.target_z = self.settings.max_zoom;
                        source = FocusSource::MouseClick;
                    } else if cursor_speed > self.settings.unzoom_speed {
                        self.target_x = cursor.0;
                        self.target_y = cursor.1;
                        self.target_z = 1.05;
                        source = FocusSource::MouseDwell;
                    } else if now.duration_since(self.last_interaction) < self.settings.idle_timeout {
                        self.target_x = cursor.0;
                        self.target_y = cursor.1;
                        self.target_z = self.settings.max_zoom;
                        source = FocusSource::MouseDwell;
                    } else {
                        // Mouse idle fallback: settle to active window (or full screen)
                        let win_rect = effective_win;
                        if self.settings.focus_auto_zoom {
                            if let Some(win) = win_rect {
                                let (wx, wy, wz) = calculate_window_framing(
                                    win, self.geometry, self.settings.max_zoom, 28.0,
                                );
                                self.target_x = wx;
                                self.target_y = wy;
                                self.target_z = wz;
                                source = FocusSource::WindowFraming;
                            } else {
                                self.target_x = self.center_x;
                                self.target_y = self.center_y;
                                self.target_z = 1.0;
                                source = FocusSource::IdleCenter;
                            }
                        } else {
                            self.target_x = self.center_x;
                            self.target_y = self.center_y;
                            self.target_z = 1.0;
                            source = FocusSource::IdleCenter;
                        }
                        self.mouse_in_control = false;
                    }
                }
            }
        } else {
            // Default idle fallback
            self.deadzone = None;
            self.target_x = self.center_x;
            self.target_y = self.center_y;
            self.target_z = 1.0;
            source = FocusSource::IdleCenter;
        }
        // 4. Viewport clamping so the camera NEVER leaves the capture screen
        let view_w = self.geometry.width / self.target_z;
        let view_h = self.geometry.height / self.target_z;
        self.target_x = self.target_x.clamp(
            self.geometry.x + view_w / 2.0,
            self.geometry.x + self.geometry.width - view_w / 2.0,
        );
        self.target_y = self.target_y.clamp(
            self.geometry.y + view_h / 2.0,
            self.geometry.y + self.geometry.height - view_h / 2.0,
        );

        // 5. Spring step
        self.sx.step(self.target_x, dt, self.settings.omega);
        self.sy.step(self.target_y, dt, self.settings.omega);
        self.sz.step(self.target_z, dt, self.settings.omega * 0.9);

        self.last_source = source;
        (self.sx.pos, self.sy.pos, self.sz.pos, source)
    }
}

fn hypr_cursor() -> Option<(f64, f64)> {
    let out = util::run_stdout_within("hyprctl", &["cursorpos", "-j"], Duration::from_millis(700))?;
    let value: Value = serde_json::from_str(&out).ok()?;
    Some((value.get("x")?.as_f64()?, value.get("y")?.as_f64()?))
}

#[allow(dead_code)]
pub fn hypr_active_window() -> Option<(f64, f64, f64)> {
    let rect = hypr_active_window_rect()?;
    let mon = monitors().first().map(logical).unwrap_or(Geometry {
        x: 0.0,
        y: 0.0,
        width: 1920.0,
        height: 1080.0,
    });
    Some(calculate_window_framing(rect, mon, 2.0, 32.0))
}

/// Queries the D-Bus AT-SPI2 accessibility bus for active text caret screen coordinates
pub fn query_atspi_caret() -> Option<(f64, f64)> {
    let script = r#"
import gi
gi.require_version("Atspi", "2.0")
from gi.repository import Atspi

desktop = Atspi.get_desktop(0)

def find_caret(obj, depth=0):
    if depth > 12:
        return None
    try:
        st = obj.get_state_set()
        if st.contains(Atspi.StateType.FOCUSED):
            txt = obj.get_text_iface()
            if txt:
                offset = txt.get_caret_offset()
                if offset >= 0:
                    ext = txt.get_character_extents(offset, Atspi.CoordType.SCREEN)
                    if ext.x > 0 or ext.y > 0:
                        return (ext.x, ext.y)
    except Exception:
        pass
    for k in range(obj.get_child_count()):
        child = obj.get_child_at_index(k)
        if child:
            res = find_caret(child, depth + 1)
            if res:
                return res
    return None

for i in range(desktop.get_child_count()):
    app = desktop.get_child_at_index(i)
    if not app:
        continue
    coords = find_caret(app)
    if coords:
        print(f"{coords[0]},{coords[1]}")
        break
"#;
    let out = util::run_stdout_within(
        "/usr/bin/python",
        &["-c", script],
        Duration::from_millis(500),
    )?;
    let line = out.lines().next()?;
    let (xs, ys) = line.split_once(',')?;
    let x: f64 = xs.trim().parse().ok()?;
    let y: f64 = ys.trim().parse().ok()?;
    if x > 0.0 && y > 0.0 {
        Some((x, y))
    } else {
        None
    }
}

pub fn is_typing_key(code: u16) -> bool {
    if code >= 0x100 {
        return false;
    }
    !matches!(
        code,
        29 | 97        // Ctrl
        | 42 | 54      // Shift
        | 56 | 100     // Alt
        | 125 | 126    // Super / Meta
        | 58 | 69 | 70  // Caps, Num, Scroll Lock
        | 59..=68      // F1-F10
        | 87..=88      // F11-F12
        | 113..=116    // Volume / Power
    )
}

fn set_compositor_zoom(zoom: f64) -> bool {
    let z = format!("{:.3}", zoom.max(1.0));
    let lua = format!("hl.config({{ cursor = {{ zoom_factor = {z} }} }})");
    if util::run_ok("hyprctl", &["eval", &lua]) {
        return true;
    }
    util::run_ok(
        "hyprctl",
        &["keyword", "cursor:zoom_factor", &z],
    )
}

pub fn reset_compositor_zoom() -> bool {
    set_compositor_zoom(1.0)
}

/// Follow window focus changes. Keyboard navigation between windows retargets
/// the camera immediately, before any typing starts.
fn window_watcher(running: Arc<AtomicBool>, hint: Arc<Mutex<Option<(WindowRect, Instant)>>>) {
    let Ok(runtime) = std::env::var("XDG_RUNTIME_DIR") else {
        return;
    };
    let Ok(signature) = std::env::var("HYPRLAND_INSTANCE_SIGNATURE") else {
        return;
    };
    let socket = PathBuf::from(format!(
        "{runtime}/hypr/{signature}/.socket2.sock"
    ));

    let mut last_known_rect: Option<WindowRect> = None;

    while running.load(Ordering::Relaxed) {
        let Ok(stream) = UnixStream::connect(&socket) else {
            let rect = hypr_active_window_rect();
            if let Ok(mut slot) = hint.lock() {
                if let Some(r) = rect {
                    last_known_rect = Some(r);
                    *slot = Some((r, Instant::now()));
                } else if last_known_rect.is_none() {
                    *slot = None;
                }
            }
            thread::sleep(Duration::from_millis(500));
            continue;
        };
        let _ = stream.set_read_timeout(Some(Duration::from_millis(300)));

        if let Some(rect) = hypr_active_window_rect() {
            last_known_rect = Some(rect);
            if let Ok(mut slot) = hint.lock() {
                *slot = Some((rect, Instant::now() - Duration::from_secs(60)));
            }
        }

        let mut last_poll = Instant::now();
        for line in BufReader::new(stream).lines() {
            if !running.load(Ordering::Relaxed) {
                break;
            }
            let Ok(event) = line else {
                if last_poll.elapsed() > Duration::from_millis(500) {
                    last_poll = Instant::now();
                    let rect = hypr_active_window_rect();
                    if let Ok(mut slot) = hint.lock() {
                        if let Some(r) = rect {
                            let changed = last_known_rect.map_or(true, |prev| prev != r);
                            if changed {
                                last_known_rect = Some(r);
                                *slot = Some((r, Instant::now()));
                            }
                        } else {
                            last_known_rect = None;
                            *slot = None;
                        }
                    }
                }
                continue;
            };

            if event.starts_with("activewindow>>")
                || event.starts_with("activewindowv2>>")
                || event.starts_with("movewindow>>")
                || event.starts_with("fullscreen>>")
                || event.starts_with("workspace>>")
                || event.starts_with("focusedmon>>")
                || event.starts_with("closewindow>>")
                || event.starts_with("openwindow>>")
                || event.starts_with("changefloatingmode>>")
            {
                last_poll = Instant::now();
                let mut rect = hypr_active_window_rect();
                // If rect is None on activewindow handover, retry after a brief pause
                // to allow the compositor to finish mapping the newly focused window.
                if rect.is_none() && (event.starts_with("activewindow>>") || event.starts_with("activewindowv2>>")) {
                    thread::sleep(Duration::from_millis(35));
                    rect = hypr_active_window_rect();
                }

                if let Ok(mut slot) = hint.lock() {
                    if let Some(r) = rect {
                        last_known_rect = Some(r);
                        *slot = Some((r, Instant::now()));
                    } else if event.starts_with("closewindow>>") || event.starts_with("workspace>>") {
                        // Truly closed window or switched workspace to empty
                        last_known_rect = None;
                        *slot = None;
                    }
                }
            }
        }
    }
}

/// Clicks and keystrokes, when /dev/input is readable. Without the `input`
/// group this degrades to mouse dwell only, which still zooms.
fn input_watcher(
    running: Arc<AtomicBool>,
    mut devices: Vec<evdev::Device>,
    click: Arc<Mutex<Option<(f64, f64, Instant)>>>,
    typing: Arc<Mutex<Option<Instant>>>,
) {
    if devices.is_empty() {
        return;
    }

    // evdev 0.12 has no non-blocking mode, so wait on the raw descriptors and
    // only read the ones that are actually ready. A short timeout keeps the
    // loop responsive when the take ends.
    use std::os::unix::io::AsRawFd;
    let mut fds: Vec<libc::pollfd> = devices
        .iter()
        .map(|device| libc::pollfd {
            fd: device.as_raw_fd(),
            events: libc::POLLIN,
            revents: 0,
        })
        .collect();

    while running.load(Ordering::Relaxed) {
        let ready = unsafe {
            libc::poll(
                fds.as_mut_ptr(),
                fds.len() as libc::nfds_t,
                100,
            )
        };
        if ready <= 0 {
            continue;
        }
        for (index, device) in devices.iter_mut().enumerate() {
            if fds[index].revents & libc::POLLIN == 0 {
                continue;
            }
            let Ok(events) = device.fetch_events() else {
                continue;
            };
            for event in events {
                let evdev::InputEventKind::Key(key) = event.kind() else {
                    continue;
                };
                if event.value() != 1 && event.value() != 2 {
                    continue;
                }
                let code = key.code();
                if (0x110..=0x117).contains(&code) {
                    let pos = hypr_cursor().unwrap_or((960.0, 540.0));
                    if let Ok(mut slot) = click.lock() {
                        *slot = Some((pos.0, pos.1, Instant::now()));
                    }
                } else if is_typing_key(code) {
                    if let Ok(mut slot) = typing.lock() {
                        *slot = Some(Instant::now());
                    }
                }
            }
        }
    }
}

fn caret_worker(
    running: Arc<AtomicBool>,
    typing: Arc<Mutex<Option<Instant>>>,
    caret: Arc<Mutex<Option<(f64, f64, Instant)>>>,
) {
    let mut last_query = Instant::now() - Duration::from_secs(10);
    while running.load(Ordering::Relaxed) {
        let typed_recently = typing
            .lock()
            .ok()
            .and_then(|slot| *slot)
            .is_some_and(|at| at.elapsed() < Duration::from_millis(2000));

        if typed_recently && last_query.elapsed() > Duration::from_millis(150) {
            last_query = Instant::now();
            if let Some((cx, cy)) = query_atspi_caret() {
                if let Ok(mut slot) = caret.lock() {
                    *slot = Some((cx, cy, Instant::now()));
                }
            }
        }
        thread::sleep(Duration::from_millis(40));
    }
}

fn read_status() -> Option<String> {
    // The tracker outlives `reel start`; the state file is the clean way to
    // notice that the take ended without a stop (crash, kill, logout).
    let runtime = std::env::var_os("XDG_RUNTIME_DIR")?;
    let path = PathBuf::from(runtime).join("reel/state.json");
    let text = std::fs::read_to_string(path).ok()?;
    let value: Value = serde_json::from_str(&text).ok()?;
    Some(value.get("status").and_then(Value::as_str)?.to_string())
}

/// The detached tracker process: samples the pointer and writes one keyframe
/// per tick until `sentinel` disappears or the take stops being recorded.
pub fn run_tracker(
    sentinel: &Path,
    sidecar: &Path,
    target: &str,
    settings: Settings,
) -> std::io::Result<()> {
    let mut out = if settings.mode == Mode::Post {
        let file = File::create(sidecar)?;
        Some(BufWriter::with_capacity(32 * 1024, file))
    } else {
        None
    };

    let running = Arc::new(AtomicBool::new(true));
    let window: Arc<Mutex<Option<(WindowRect, Instant)>>> = Arc::new(Mutex::new(None));
    let click: Arc<Mutex<Option<(f64, f64, Instant)>>> = Arc::new(Mutex::new(None));
    let typing: Arc<Mutex<Option<Instant>>> = Arc::new(Mutex::new(None));
    let caret: Arc<Mutex<Option<(f64, f64, Instant)>>> = Arc::new(Mutex::new(None));

    let (input_devices, input_ok) = match std::panic::catch_unwind(|| {
        evdev::enumerate()
            .map(|(_, device)| device)
            .filter(|device| {
                device.supported_keys().is_some_and(|keys| {
                    keys.contains(evdev::Key::BTN_LEFT) || keys.contains(evdev::Key::KEY_A)
                })
            })
            .collect::<Vec<_>>()
    }) {
        Ok(devs) if !devs.is_empty() => (devs, true),
        _ => (Vec::new(), false),
    };

    if !input_ok {
        if let Some(runtime) = std::env::var_os("XDG_RUNTIME_DIR") {
            let log = PathBuf::from(runtime).join("reel/reel.log");
            paths_log(
                &log,
                "zoom: /dev/input is not readable; keyboard typing auto-zoom inactive. Run: sudo usermod -aG input $USER && sudo setfacl -m u:$USER:rw /dev/input/event*",
            );
        }
        if settings.focus_auto_zoom && util::have("omarchy-notification-send") {
            let _ = Command::new("omarchy-notification-send")
                .arg("Reel Auto-Zoom")
                .arg("/dev/input is not readable. Run: sudo usermod -aG input $USER")
                .arg("-t")
                .arg("7000")
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn();
        }
    }

    let mut workers = Vec::new();
    if settings.focus_auto_zoom {
        let flag = running.clone();
        let hint = window.clone();
        workers.push(thread::spawn(move || window_watcher(flag, hint)));

        // Always start the caret worker when focus_auto_zoom is ON — typing
        // zooms to caret as part of the unified state machine.
        {
            let flag = running.clone();
            let keys = typing.clone();
            let target_caret = caret.clone();
            workers.push(thread::spawn(move || caret_worker(flag, keys, target_caret)));
        }
    }
    {
        let flag = running.clone();
        let clicks = click.clone();
        let keys = typing.clone();
        workers.push(thread::spawn(move || input_watcher(flag, input_devices, clicks, keys)));
    }

    let geometry = geometry_for(target).unwrap_or_else(|| Geometry {
        x: 0.0,
        y: 0.0,
        width: 1920.0,
        height: 1080.0,
    });
    let mut controller = CameraController::new(geometry, settings);

    let started = Instant::now();
    let dt = 1.0 / 30.0;
    let tick = Duration::from_secs_f64(dt);

    let mut cursor = hypr_cursor().unwrap_or((controller.center_x, controller.center_y));
    let mut previous = cursor;
    let mut live = 1.0;

    let mut last_state_check = Instant::now();
    let mut seen_recording = false;
    let mut paused = false;
    let mut time = 0.0;
    let mut last_tick = started;

    while sentinel.exists() {
        let now = Instant::now();

        if now.duration_since(last_state_check) > Duration::from_millis(300) {
            last_state_check = now;
            match read_status().as_deref() {
                Some("paused") => paused = true,
                Some("recording") | Some("starting") | Some("finalizing") => {
                    paused = false;
                    seen_recording = true;
                }
                Some(_) => {
                    if seen_recording {
                        break;
                    }
                }
                None => {}
            }
        }

        if paused {
            last_tick = now;
            thread::sleep(tick);
            continue;
        }

        let mut speed = 0.0;
        if let Some(fresh) = hypr_cursor() {
            let moved = (fresh.0 - previous.0).hypot(fresh.1 - previous.1);
            previous = cursor;
            cursor = fresh;
            speed = moved / dt;
        }

        let clicked = click
            .lock()
            .ok()
            .and_then(|slot| *slot)
            .filter(|(_, _, at)| now.duration_since(*at) < Duration::from_millis(600));

        let typed = typing
            .lock()
            .ok()
            .and_then(|slot| *slot)
            .is_some_and(|at| now.duration_since(at) < Duration::from_millis(150));

        let active_win = window.lock().ok().and_then(|slot| *slot);
        let caret_pos = caret.lock().ok().and_then(|slot| *slot);

        let (x, y, z, _) = controller.step(
            dt,
            now,
            cursor,
            speed,
            clicked,
            typed,
            active_win,
            caret_pos,
        );

        time += now.duration_since(last_tick).as_secs_f64().min(0.25);
        last_tick = now;

        if let Some(ref mut writer) = out {
            let _ = writeln!(
                writer,
                "{{\"t\":{time:.3},\"x\":{x:.2},\"y\":{y:.2},\"z\":{z:.4}}}"
            );
            let _ = writer.flush();
        }

        if controller.settings.mode == Mode::Live && (z - live).abs() > 0.02 {
            live = z;
            set_compositor_zoom(live);
        }

        if let Some(rest) = tick.checked_sub(now.elapsed()) {
            thread::sleep(rest);
        }
    }

    running.store(false, Ordering::Relaxed);
    for worker in workers {
        let _ = worker.join();
    }
    if controller.settings.mode == Mode::Live {
        set_compositor_zoom(1.0);
    }
    if let Some(ref mut writer) = out {
        let _ = writer.flush();
    }
    Ok(())
}

// ---------- ffmpeg camera pass ------------------------------------------

#[derive(Debug, Clone, Copy)]
pub struct VideoInfo {
    pub width: u32,
    pub height: u32,
    pub fps: f64,
}

pub fn video_info(file: &Path) -> Option<VideoInfo> {
    let text = util::run_stdout_within(
        "ffprobe",
        &[
            "-v",
            "error",
            "-select_streams",
            "v:0",
            "-show_entries",
            "stream=width,height,r_frame_rate",
            "-of",
            "csv=p=0",
            &file.display().to_string(),
        ],
        Duration::from_secs(10),
    )?;
    let line = text.lines().next()?;
    let mut parts = line.split(',');
    let width: u32 = parts.next()?.trim().parse().ok()?;
    let height: u32 = parts.next()?.trim().parse().ok()?;
    let rate = parts.next().unwrap_or("60/1").trim();
    let fps = match rate.split_once('/') {
        Some((num, den)) => {
            let num: f64 = num.parse().ok()?;
            let den: f64 = den.parse().ok()?;
            if den > 0.0 {
                num / den
            } else {
                60.0
            }
        }
        None => rate.parse().unwrap_or(60.0),
    };
    if width < 16 || height < 16 {
        return None;
    }
    Some(VideoInfo {
        width,
        height,
        fps: if fps > 1.0 { fps } else { 60.0 },
    })
}

fn build_bst_expr(segments: &[(i64, i64, f64, f64)], prec_val: usize, prec_delta: usize) -> String {
    if segments.is_empty() {
        return "1".into();
    }
    if segments.len() == 1 {
        let (on0, on1, v0, v1) = segments[0];
        let span = (on1 - on0).max(1) as f64;
        let delta = v1 - v0;
        if delta.abs() < 0.00001 {
            return format!("{v0:.prec$}", prec = prec_val);
        }
        return format!(
            "{v0:.pv$}+({delta:.pd$})*(on-{on0})/{span:.1}",
            pv = prec_val,
            pd = prec_delta
        );
    }
    let mid = segments.len() / 2;
    let split_on = segments[mid].0;
    let left = build_bst_expr(&segments[..mid], prec_val, prec_delta);
    let right = build_bst_expr(&segments[mid..], prec_val, prec_delta);
    format!("if(lt(on,{split_on}),{left},{right})")
}

fn scale_expr(points: &[(i64, f64, f64, f64)]) -> (String, String, String) {
    if points.is_empty() {
        return ("1".into(), "0".into(), "0".into());
    }
    if points.len() == 1 {
        return (
            format!("{:.5}", points[0].1),
            format!("{:.2}", points[0].2),
            format!("{:.2}", points[0].3),
        );
    }

    let mut z_segs = Vec::with_capacity(points.len() + 1);
    let mut x_segs = Vec::with_capacity(points.len() + 1);
    let mut y_segs = Vec::with_capacity(points.len() + 1);

    if points[0].0 > 0 {
        z_segs.push((0, points[0].0, points[0].1, points[0].1));
        x_segs.push((0, points[0].0, points[0].2, points[0].2));
        y_segs.push((0, points[0].0, points[0].3, points[0].3));
    }

    for i in 0..points.len() - 1 {
        let (on0, z0, x0, y0) = points[i];
        let (on1, z1, x1, y1) = points[i + 1];
        z_segs.push((on0, on1, z0, z1));
        x_segs.push((on0, on1, x0, x1));
        y_segs.push((on0, on1, y0, y1));
    }

    let last = points.last().unwrap();
    let tail_on = last.0 + 1_000_000;
    z_segs.push((last.0, tail_on, last.1, last.1));
    x_segs.push((last.0, tail_on, last.2, last.2));
    y_segs.push((last.0, tail_on, last.3, last.3));

    let z = build_bst_expr(&z_segs, 5, 6);
    let x = build_bst_expr(&x_segs, 2, 3);
    let y = build_bst_expr(&y_segs, 2, 3);

    (z, x, y)
}

/// Ramer–Douglas–Peucker over (frame, zoom, x, y). The camera curve is smooth,
/// so long still stretches collapse to a couple of anchors while every zoom
/// transition keeps its shape. This keeps the filter expression proportional to
/// the number of moves, not to the length of the take.
fn simplify(
    points: &[(i64, f64, f64, f64)],
    tolerance: f64,
    max: usize,
) -> Vec<(i64, f64, f64, f64)> {
    if points.len() <= 2 {
        return points.to_vec();
    }
    // Normalise the channels so one tolerance covers zoom and pixels.
    let distance = |a: (i64, f64, f64, f64), b: (i64, f64, f64, f64), p: (i64, f64, f64, f64)| {
        let span = (b.0 - a.0).max(1) as f64;
        let f = ((p.0 - a.0) as f64 / span).clamp(0.0, 1.0);
        let dz = (p.1 - (a.1 + (b.1 - a.1) * f)).abs() / 0.0025;
        let dx = (p.2 - (a.2 + (b.2 - a.2) * f)).abs() / 0.6;
        let dy = (p.3 - (a.3 + (b.3 - a.3) * f)).abs() / 0.6;
        dz.max(dx).max(dy)
    };

    let mut tolerance = tolerance;
    loop {
        let mut keep = vec![false; points.len()];
        keep[0] = true;
        keep[points.len() - 1] = true;
        let mut stack = vec![(0usize, points.len() - 1)];
        while let Some((a, b)) = stack.pop() {
            if b <= a + 1 {
                continue;
            }
            let mut worst = 0.0;
            let mut worst_at = a;
            for index in a + 1..b {
                let d = distance(points[a], points[b], points[index]);
                if d > worst {
                    worst = d;
                    worst_at = index;
                }
            }
            if worst > tolerance {
                keep[worst_at] = true;
                stack.push((a, worst_at));
                stack.push((worst_at, b));
            }
        }
        let kept: Vec<(i64, f64, f64, f64)> = points
            .iter()
            .zip(keep.iter())
            .filter(|(_, keep)| **keep)
            .map(|(point, _)| *point)
            .collect();
        if kept.len() <= max || tolerance > 64.0 {
            return kept;
        }
        tolerance *= 1.6;
    }
}

/// Build the `zoompan` camera move from the recorded curve. Returns the filter
/// graph, or `None` when the take never actually zoomed.
pub fn filter_script(
    frames: &[Keyframe],
    geometry: Geometry,
    video: VideoInfo,
) -> Option<String> {
    if frames.len() < 2 || geometry.width <= 1.0 || geometry.height <= 1.0 {
        return None;
    }
    let scale_x = video.width as f64 / geometry.width;
    let scale_y = video.height as f64 / geometry.height;

    let mapped: Vec<(f64, f64, f64, f64)> = frames
        .iter()
        .map(|frame| {
            let x = ((frame.x - geometry.x) * scale_x).clamp(0.0, video.width as f64);
            let y = ((frame.y - geometry.y) * scale_y).clamp(0.0, video.height as f64);
            (frame.t, x, y, frame.z.clamp(1.0, 3.0))
        })
        .collect();

    if mapped.iter().all(|(_, _, _, z)| *z < 1.02) {
        return None;
    }

    // One anchor per recorded sample, then only the corners that matter.
    let mut dense: Vec<(i64, f64, f64, f64)> = Vec::with_capacity(mapped.len());
    for (t, x, y, z) in &mapped {
        let on = (*t * video.fps).round() as i64;
        if dense.last().map(|p| p.0) == Some(on) {
            // Two samples inside one frame: the later one wins.
            *dense.last_mut().unwrap() = (on, *z, *x, *y);
        } else {
            dense.push((on, *z, *x, *y));
        }
    }
    let points = simplify(&dense, 1.0, 400);
    if points.len() < 2 {
        return None;
    }

    let (z, px, py) = scale_expr(&points);
    let x = format!("max(0,min(({px})-(iw/zoom)/2,iw-iw/zoom))");
    let y = format!("max(0,min(({py})-(ih/zoom)/2,ih-ih/zoom))");
    Some(format!(
        "zoompan=z='{z}':x='{x}':y='{y}':d=1:s={w}x{h}:fps={fps:.6}",
        w = video.width,
        h = video.height,
        fps = video.fps
    ))
}

fn run_ffmpeg(args: &[String], log: Option<&Path>) -> bool {
    let mut command = Command::new("ffmpeg");
    command.args(args).stdin(Stdio::null());
    match log.and_then(|path| {
        std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .ok()
    }) {
        Some(stream) => {
            let err = stream.try_clone();
            command.stdout(Stdio::from(stream));
            if let Ok(err) = err {
                command.stderr(Stdio::from(err));
            }
        }
        None => {
            command.stdout(Stdio::null()).stderr(Stdio::null());
        }
    }
    command
        .status()
        .map(|status| status.success())
        .unwrap_or(false)
}

/// Generate a smooth cinematic camera zoom curve for takes without a recorded telemetry sidecar.
pub fn synthetic_keyframes(video: VideoInfo, duration: f64, max_zoom: f64) -> Vec<Keyframe> {
    let geometry = Geometry {
        x: 0.0,
        y: 0.0,
        width: video.width as f64,
        height: video.height as f64,
    };
    let mut settings = Settings::default();
    settings.max_zoom = max_zoom.clamp(1.15, 2.5);
    settings.typing_zoom = settings.max_zoom;
    let mut controller = CameraController::new(geometry, settings);

    let d = if duration.is_finite() && duration > 0.5 {
        duration
    } else {
        5.0
    };
    let dt = 1.0 / 30.0;
    let total_steps = (d / dt).ceil() as usize;

    let mut frames = Vec::with_capacity(total_steps + 1);
    let zoom_start = 0.5_f64.min(d * 0.15);
    let zoom_end = (d - 1.0_f64).max(d * 0.75);

    let start_time = Instant::now();
    for i in 0..=total_steps {
        let t = i as f64 * dt;
        let now = start_time + Duration::from_secs_f64(t);
        let typing = t >= zoom_start && t <= zoom_end;
        let caret = if typing {
            Some((controller.center_x, controller.center_y, now))
        } else {
            None
        };
        let (x, y, z, _) = controller.step(
            dt,
            now,
            (controller.center_x, controller.center_y),
            0.0,
            None,
            typing,
            None,
            caret,
        );
        frames.push(Keyframe { t, x, y, z });
    }
    frames
}

/// Render a camera zoom move from `input` to `output` using `frames` keyframe curve.
pub fn render_zoom_to(
    input: &Path,
    output: &Path,
    frames: &[Keyframe],
    target: &str,
    log: Option<&Path>,
) -> bool {
    if frames.is_empty() {
        if let Some(log) = log {
            paths_log(log, "zoom: no keyframes provided");
        }
        return false;
    }
    let Some(geometry) = geometry_for(target) else {
        if let Some(log) = log {
            paths_log(log, "zoom: cannot resolve capture geometry");
        }
        return false;
    };
    let Some(video) = video_info(input) else {
        if let Some(log) = log {
            paths_log(log, "zoom: cannot read video info from input");
        }
        return false;
    };
    let Some(script) = filter_script(frames, geometry, video) else {
        if let Some(log) = log {
            paths_log(log, "zoom: no zoom motion in keyframes");
        }
        return false;
    };
    if let Some(log) = log {
        paths_log(
            log,
            &format!(
                "zoom: {} keyframes, geometry {}x{}+{}+{}, video {}x{}@{:.2}, filter {} bytes",
                frames.len(),
                geometry.width,
                geometry.height,
                geometry.x,
                geometry.y,
                video.width,
                video.height,
                video.fps,
                script.len()
            ),
        );
    }

    let script_path = input.with_extension(format!("zoomfilter.{}.txt", std::process::id()));
    if std::fs::write(&script_path, format!("{script}\n")).is_err() {
        return false;
    }

    let work_output = output.with_extension(format!(
        "part.{}.{}",
        std::process::id(),
        output
            .extension()
            .map(|ext| ext.to_string_lossy().to_string())
            .unwrap_or_else(|| "mp4".into())
    ));

    let movflags = matches!(
        output.extension().and_then(|ext| ext.to_str()),
        Some("mp4") | Some("mov")
    );

    let mut ok = false;
    for flag in ["-/filter:v", "-filter_script:v"] {
        for audio in ["copy", "aac"] {
            let mut args: Vec<String> = vec![
                "-y".into(),
                "-i".into(),
                input.display().to_string(),
                flag.into(),
                script_path.display().to_string(),
                "-sws_flags".into(),
                "lanczos+accurate_rnd".into(),
                "-map".into(),
                "0:v:0".into(),
                "-map".into(),
                "0:a?".into(),
                "-c:v".into(),
                "libx264".into(),
                "-preset".into(),
                "fast".into(),
                "-crf".into(),
                "15".into(),
                "-pix_fmt".into(),
                "yuv420p".into(),
            ];
            if audio == "aac" {
                args.extend(["-c:a".into(), "aac".into(), "-b:a".into(), "192k".into()]);
            } else {
                args.extend(["-c:a".into(), "copy".into()]);
            }
            if movflags {
                args.extend(["-movflags".into(), "+faststart".into()]);
            }
            args.push(work_output.display().to_string());
            if run_ffmpeg(&args, log) {
                ok = true;
                break;
            }
        }
        if ok {
            break;
        }
    }

    let _ = std::fs::remove_file(&script_path);

    let rendered = ok
        && work_output.is_file()
        && std::fs::metadata(&work_output).map(|m| m.len()).unwrap_or(0) > 0;

    if rendered {
        if std::fs::rename(&work_output, output).is_ok() {
            if let Some(log) = log {
                paths_log(log, &format!("zoom: wrote finished zoom to {}", output.display()));
            }
            return true;
        }
    }

    let _ = std::fs::remove_file(&work_output);
    if let Some(log) = log {
        paths_log(log, "zoom: camera pass failed");
    }
    false
}

/// Apply the recorded camera move to `file` in place. Returns whether the file
/// was re-rendered. The sidecar is removed on success.
pub fn apply_pass(
    file: &Path,
    sidecar: &Path,
    target: &str,
    log: Option<&Path>,
) -> bool {
    let frames = read_keyframes(sidecar);
    if frames.is_empty() {
        if let Some(log) = log {
            paths_log(log, "zoom: no keyframes, keeping the raw take");
        }
        let _ = std::fs::remove_file(sidecar);
        return false;
    }

    let temp_output = file.with_extension(format!(
        "zoom.{}.{}",
        std::process::id(),
        file.extension()
            .map(|ext| ext.to_string_lossy().to_string())
            .unwrap_or_else(|| "mp4".into())
    ));

    let rendered = render_zoom_to(file, &temp_output, &frames, target, log);
    if rendered {
        let _ = std::fs::rename(&temp_output, file);
        let _ = std::fs::remove_file(sidecar);
        if let Some(log) = log {
            paths_log(log, "zoom: applied the camera pass in place");
        }
        true
    } else {
        let _ = std::fs::remove_file(&temp_output);
        let _ = std::fs::remove_file(sidecar);
        if let Some(log) = log {
            paths_log(log, "zoom: camera pass failed, keeping raw take");
        }
        false
    }
}

fn paths_log(log: &Path, message: &str) {
    if let Ok(mut file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(log)
    {
        let _ = writeln!(file, "[{}] {message}", crate::paths::log_stamp());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn info() -> VideoInfo {
        VideoInfo {
            width: 1920,
            height: 1080,
            fps: 60.0,
        }
    }

    fn frame(t: f64, z: f64) -> Keyframe {
        Keyframe {
            t,
            x: 960.0,
            y: 540.0,
            z,
        }
    }

    #[test]
    fn mode_parses_switches_and_legacy_words() {
        assert_eq!(Mode::from_value(None), Mode::Post);
        assert_eq!(Mode::from_value(Some(&Value::Bool(false))), Mode::Off);
        assert_eq!(Mode::from_value(Some(&Value::String("off".into()))), Mode::Off);
        assert_eq!(Mode::from_value(Some(&Value::Bool(true))), Mode::Post);
        assert_eq!(
            Mode::from_value(Some(&Value::String("live_compositor".into()))),
            Mode::Live
        );
        assert_eq!(
            Mode::from_value(Some(&Value::String("mouse".into()))),
            Mode::Post
        );
    }

    #[test]
    fn settings_read_zoom_keys_with_bounds() {
        let value = serde_json::json!({
            "autoZoom": "post_process",
            "autoZoomMax": 9.0,
            "autoZoomIdle": 100.0,
            "followMouse": "off"
        });
        let settings = settings_from(&value);
        assert_eq!(settings.mode, Mode::Post);
        assert_eq!(settings.max_zoom, 3.0);
        assert_eq!(settings.idle_timeout, Duration::from_millis(800));
        assert!(!settings.follow_mouse);
    }

    #[test]
    fn flat_curve_needs_no_render() {
        let frames = vec![frame(0.0, 1.0), frame(1.0, 1.0), frame(2.0, 1.0)];
        let geometry = Geometry {
            x: 0.0,
            y: 0.0,
            width: 1920.0,
            height: 1080.0,
        };
        assert!(filter_script(&frames, geometry, info()).is_none());
    }

    #[test]
    fn zoom_curve_becomes_a_zoompan_graph() {
        let frames = vec![frame(0.0, 1.0), frame(1.0, 1.8), frame(2.0, 1.8)];
        let geometry = Geometry {
            x: 0.0,
            y: 0.0,
            width: 1920.0,
            height: 1080.0,
        };
        let script = filter_script(&frames, geometry, info()).expect("a script");
        assert!(script.starts_with("zoompan="));
        assert!(script.contains("d=1:s=1920x1080"));
        assert!(script.contains("fps=60.000000"));
        assert!(script.contains("if(lt(on,"));
    }

    #[test]
    fn region_targets_parse_with_negative_origins() {
        let body = "1280x720+-100+40";
        let geometry = parse_region(body).expect("geometry");
        assert_eq!(geometry.width, 1280.0);
        assert_eq!(geometry.height, 720.0);
        assert_eq!(geometry.x, -100.0);
        assert_eq!(geometry.y, 40.0);
    }

    #[test]
    fn cursor_is_mapped_into_video_pixels() {
        // A 2x scaled monitor: 1728x972 logical becomes 3456x1944 pixels.
        let geometry = Geometry {
            x: 0.0,
            y: 0.0,
            width: 1728.0,
            height: 972.0,
        };
        let video = VideoInfo {
            width: 3456,
            height: 1944,
            fps: 60.0,
        };
        let frames = vec![
            Keyframe {
                t: 0.0,
                x: 0.0,
                y: 0.0,
                z: 1.0,
            },
            Keyframe {
                t: 1.0,
                x: 864.0,
                y: 486.0,
                z: 2.0,
            },
        ];
        let script = filter_script(&frames, geometry, video).expect("script");
        // Centre of the monitor is the centre of the video.
        assert!(script.contains("1728.00") && script.contains("972.00"));
    }

    #[test]
    fn typing_keys_filter_modifiers() {
        // A, Z, 1, Space, Enter, Backspace should be typing keys
        assert!(is_typing_key(30)); // KEY_A
        assert!(is_typing_key(44)); // KEY_Z
        assert!(is_typing_key(2));  // KEY_1
        assert!(is_typing_key(57)); // KEY_SPACE
        assert!(is_typing_key(28)); // KEY_ENTER
        assert!(is_typing_key(14)); // KEY_BACKSPACE

        // Modifiers and F-keys should not be typing keys
        assert!(!is_typing_key(29));  // KEY_LEFTCTRL
        assert!(!is_typing_key(42));  // KEY_LEFTSHIFT
        assert!(!is_typing_key(56));  // KEY_LEFTALT
        assert!(!is_typing_key(125)); // KEY_LEFTMETA (Super)
        assert!(!is_typing_key(59));  // KEY_F1
    }

    #[test]
    fn spring_smoothly_approaches_target() {
        let mut s = Spring::new(1.0);
        for _ in 0..60 {
            s.step(2.0, 1.0 / 60.0, 6.0);
        }
        assert!(s.pos > 1.9 && s.pos <= 2.0);
    }

    #[test]
    fn synthetic_keyframes_produce_valid_filter_script() {
        let video = info();
        let frames = synthetic_keyframes(video, 4.0, 1.8);
        assert!(!frames.is_empty());
        let geometry = Geometry {
            x: 0.0,
            y: 0.0,
            width: 1920.0,
            height: 1080.0,
        };
        let script = filter_script(&frames, geometry, video);
        assert!(script.is_some());
        let s = script.unwrap();
        assert!(s.contains("zoompan="));
        assert!(s.contains("fps=60"));
    }

    #[test]
    fn window_framing_zoom_calculation_with_border_padding() {
        let geometry = Geometry {
            x: 0.0,
            y: 0.0,
            width: 1920.0,
            height: 1080.0,
        };

        // 1. Fullscreen window should never crop: returns 1.0x at center
        let fullscreen = WindowRect {
            x: 0.0,
            y: 0.0,
            width: 1920.0,
            height: 1080.0,
        };
        let (cx, cy, z) = calculate_window_framing(fullscreen, geometry, 2.0, 32.0);
        assert_eq!(z, 1.0);
        assert_eq!(cx, 960.0);
        assert_eq!(cy, 540.0);

        // 2. Centered window: 1000x600 padded by 32px is 1064x664
        // z_fit = min(1920/1064, 1080/664) = min(1.8045, 1.6265) = 1.6265
        let floating = WindowRect {
            x: 460.0,
            y: 240.0,
            width: 1000.0,
            height: 600.0,
        };
        let (cx, cy, z) = calculate_window_framing(floating, geometry, 2.0, 32.0);
        assert!((z - 1.6265).abs() < 0.01);
        assert_eq!(cx, 960.0);
        assert_eq!(cy, 540.0);

        // 3. Side-by-side vertical split windows (e.g. 701x760 on 1440x810 display)
        // Since height spans the screen, zooming would crop the top/bottom: stays cleanly at 1.0x
        let geo_1440 = Geometry {
            x: 0.0,
            y: 0.0,
            width: 1440.0,
            height: 810.0,
        };
        let left_tiled = WindowRect {
            x: 12.0,
            y: 38.0,
            width: 701.0,
            height: 760.0,
        };
        let (lcx, lcy, lz) = calculate_window_framing(left_tiled, geo_1440, 1.9, 28.0);
        assert_eq!(lz, 1.0, "Two large side-by-side windows cannot be zoomed without cropping height; must stay at 1.0x");
        assert_eq!(lcx, 720.0);
        assert_eq!(lcy, 405.0);

        let right_tiled = WindowRect {
            x: 727.0,
            y: 38.0,
            width: 701.0,
            height: 760.0,
        };
        let (rcx, rcy, rz) = calculate_window_framing(right_tiled, geo_1440, 1.9, 28.0);
        assert_eq!(rz, 1.0);
        assert_eq!(rcx, 720.0);
        assert_eq!(rcy, 405.0);

        // 4. Maximized window (1416x760 on 1440x810) fills both width & height: 1.0x at center
        let max_1440 = WindowRect {
            x: 12.0,
            y: 38.0,
            width: 1416.0,
            height: 760.0,
        };
        let (mcx, mcy, mz) = calculate_window_framing(max_1440, geo_1440, 1.9, 28.0);
        assert_eq!(mz, 1.0);
        assert_eq!(mcx, 720.0);
        assert_eq!(mcy, 405.0);
    }

    #[test]
    fn focus_auto_zoom_off_ignores_typing_and_stays_idle() {
        let geometry = Geometry {
            x: 0.0,
            y: 0.0,
            width: 1920.0,
            height: 1080.0,
        };
        let mut settings = Settings::default();
        settings.follow_mouse = false;
        settings.focus_auto_zoom = false;
        settings.typing_zoom = 2.0;

        let mut controller = CameraController::new(geometry, settings);
        let win = WindowRect {
            x: 60.0,
            y: 40.0,
            width: 1800.0,
            height: 1000.0,
        };
        let now = Instant::now();

        // While typing, when focus_auto_zoom is OFF, camera MUST stay at 1.0x
        controller.step(
            0.033,
            now,
            (0.0, 0.0),
            0.0,
            None,
            true, // is_typing == true
            Some((win, now)),
            Some((300.0, 250.0, now)),
        );

        assert_eq!(controller.target_z, 1.0);
        assert_eq!(controller.last_source, FocusSource::IdleCenter);
        assert!((controller.target_x - 960.0).abs() < 2.0);
        assert!((controller.target_y - 540.0).abs() < 2.0);
    }

    #[test]
    fn focus_auto_zoom_window_framing_when_not_typing() {
        let geometry = Geometry {
            x: 0.0,
            y: 0.0,
            width: 1920.0,
            height: 1080.0,
        };
        let mut settings = Settings::default();
        settings.follow_mouse = false;
        settings.focus_auto_zoom = true; // Unified focus auto zoom
        settings.typing_zoom = 2.0;

        let mut controller = CameraController::new(geometry, settings);
        let win = WindowRect {
            x: 60.0,
            y: 40.0,
            width: 1800.0,
            height: 1000.0,
        };
        let now = Instant::now();

        // When not typing, camera MUST show full window framing
        controller.step(
            0.033,
            now,
            (0.0, 0.0),
            0.0,
            None,
            false, // is_typing == false
            Some((win, now)),
            None,
        );

        assert!(controller.target_z >= 1.0 && controller.target_z <= 1.05);
        assert_eq!(controller.last_source, FocusSource::WindowFraming);
        assert!((controller.target_x - 960.0).abs() < 2.0);
        assert!((controller.target_y - 540.0).abs() < 2.0);
    }

    #[test]
    fn small_window_fits_window_and_borders() {
        let geometry = Geometry {
            x: 0.0,
            y: 0.0,
            width: 1920.0,
            height: 1080.0,
        };
        let win = WindowRect {
            x: 560.0,
            y: 290.0,
            width: 800.0,
            height: 500.0,
        };
        let (cx, cy, z) = calculate_window_framing(win, geometry, 2.0, 28.0);
        // w_req = 800 + 56 = 856 -> 1920/856 = 2.24
        // h_req = 500 + 56 = 556 -> 1080/556 = 1.94
        // z_fit = min(2.24, 1.94) = 1.94
        assert!((z - 1.94).abs() < 0.02);
        assert_eq!(cx, 960.0);
        assert_eq!(cy, 540.0);
    }

    #[test]
    fn fullscreen_window_typing_and_idle_transition_stays_1x() {
        let geometry = Geometry {
            x: 0.0,
            y: 0.0,
            width: 1920.0,
            height: 1080.0,
        };
        let mut settings = Settings::default();
        settings.follow_mouse = false;
        settings.focus_auto_zoom = true;
        settings.max_zoom = 2.0;
        settings.typing_zoom = 2.0;

        let mut controller = CameraController::new(geometry, settings);
        let win = WindowRect {
            x: 60.0,
            y: 40.0,
            width: 1800.0,
            height: 1000.0,
        };
        let start = Instant::now();

        // 1. WHILE TYPING with caret at (300, 250): window is fullscreen/maximized,
        // so camera MUST stay at 1.0x with full window framing.
        controller.step(
            0.033,
            start,
            (0.0, 0.0),
            0.0,
            None,
            true, // typing active
            Some((win, start)),
            Some((300.0, 250.0, start)),
        );
        assert_eq!(controller.target_z, 1.0, "typing in fullscreen window must not zoom");
        assert_eq!(controller.last_source, FocusSource::WindowFraming);
        assert_eq!(controller.target_x, 960.0);
        assert_eq!(controller.target_y, 540.0);

        // 2. ONCE TYPING STOPS past hold threshold (e.g. 2.5s later):
        // remains full active window at 1.0x
        let later = start + Duration::from_millis(2500);
        controller.step(
            0.033,
            later,
            (0.0, 0.0),
            0.0,
            None,
            false, // typing stopped
            Some((win, start)),
            None,
        );
        assert_eq!(controller.target_z, 1.0);
        assert_eq!(controller.last_source, FocusSource::WindowFraming);
        assert_eq!(controller.target_x, 960.0);
        assert_eq!(controller.target_y, 540.0);
    }

    #[test]
    fn small_window_typing_and_idle_transition() {
        let geometry = Geometry {
            x: 0.0,
            y: 0.0,
            width: 1920.0,
            height: 1080.0,
        };
        let mut settings = Settings::default();
        settings.follow_mouse = false;
        settings.focus_auto_zoom = true;
        settings.max_zoom = 2.0;
        settings.typing_zoom = 2.0;

        let mut controller = CameraController::new(geometry, settings);
        let win = WindowRect {
            x: 200.0,
            y: 150.0,
            width: 800.0,
            height: 500.0,
        };
        let start = Instant::now();

        // While typing in small window with caret at (300, 250): camera zooms to caret (2.0x)
        controller.step(
            0.033,
            start,
            (0.0, 0.0),
            0.0,
            None,
            true, // typing active
            Some((win, start)),
            Some((300.0, 250.0, start)),
        );
        assert_eq!(controller.target_z, 2.0);
        assert_eq!(controller.last_source, FocusSource::CaretTyping);
        assert!(controller.target_x >= 400.0 && controller.target_x <= 600.0);
    }

    #[test]
    fn follow_mouse_off_ignores_mouse_and_fullscreen_stays_1x() {
        let geometry = Geometry {
            x: 0.0,
            y: 0.0,
            width: 1920.0,
            height: 1080.0,
        };
        let mut settings = Settings::default();
        settings.follow_mouse = false;
        settings.focus_auto_zoom = true;

        let mut controller = CameraController::new(geometry, settings);

        // Moving mouse across the screen should be completely ignored
        let now = Instant::now();
        controller.step(
            0.033,
            now,
            (100.0, 100.0),
            600.0,
            None,
            false,
            None,
            None,
        );
        assert_eq!(controller.target_z, 1.0);
        assert_eq!(controller.target_x, 960.0);
        assert_eq!(controller.target_y, 540.0);
        assert!(!controller.mouse_in_control);

        // Mouse click should also be completely ignored
        controller.step(
            0.033,
            now,
            (100.0, 100.0),
            0.0,
            Some((100.0, 100.0, now)),
            false,
            None,
            None,
        );
        assert_eq!(controller.target_z, 1.0);
        assert_eq!(controller.target_x, 960.0);
        assert_eq!(controller.target_y, 540.0);

        // Fullscreen window active should keep zoom at 1.0x
        let fullscreen = WindowRect {
            x: 0.0,
            y: 0.0,
            width: 1920.0,
            height: 1080.0,
        };
        controller.step(
            0.033,
            now,
            (100.0, 100.0),
            0.0,
            None,
            false,
            Some((fullscreen, now)),
            None,
        );
        assert_eq!(controller.target_z, 1.0);
        assert_eq!(controller.target_x, 960.0);
        assert_eq!(controller.target_y, 540.0);
    }

    #[test]
    fn caret_tracking_deadzone_and_fallback_to_window() {
        let geometry = Geometry {
            x: 0.0,
            y: 0.0,
            width: 1920.0,
            height: 1080.0,
        };
        let mut settings = Settings::default();
        settings.follow_mouse = false;
        settings.focus_auto_zoom = true;
        settings.typing_zoom = 1.8;

        let mut controller = CameraController::new(geometry, settings);

        // Initial typing at caret (500, 500)
        let now = Instant::now();
        controller.step(
            0.033,
            now,
            (0.0, 0.0),
            0.0,
            None,
            true,
            None,
            Some((500.0, 500.0, now)),
        );
        assert_eq!(controller.last_source, FocusSource::CaretTyping);
        let anchor_x = controller.target_x;
        let anchor_y = controller.target_y;

        // Next character typed at (512, 500) within the horizontal deadzone: anchor does NOT jitter
        controller.step(
            0.033,
            now,
            (0.0, 0.0),
            0.0,
            None,
            true,
            None,
            Some((512.0, 500.0, now)),
        );
        assert_eq!(controller.target_x, anchor_x);
        assert_eq!(controller.target_y, anchor_y);

        // Caret moves across line to (750, 500) exceeding deadzone: anchor moves smoothly
        controller.step(
            0.033,
            now,
            (0.0, 0.0),
            0.0,
            None,
            true,
            None,
            Some((750.0, 500.0, now)),
        );
        assert!(controller.target_x > anchor_x);

        // When caret is unavailable (None) while typing in terminal, keeps framing the full active window (State B)
        let win = WindowRect {
            x: 400.0,
            y: 300.0,
            width: 800.0,
            height: 500.0,
        };
        controller.step(
            0.033,
            now,
            (0.0, 0.0),
            0.0,
            None,
            true, // is_typing == true
            Some((win, now)),
            None, // caret is None
        );
        assert_eq!(controller.last_source, FocusSource::WindowFraming);
        let (_, _, expected_wz) = calculate_window_framing(win, geometry, controller.settings.max_zoom, 28.0);
        assert!((controller.target_z - expected_wz).abs() < 0.01);
        assert!((controller.target_x - 800.0).abs() < 1.0);
        assert!((controller.target_y - 550.0).abs() < 1.0);

        // When typing stops, smoothly returns to active window framing (Z_window)
        let later = now + Duration::from_millis(2500);
        controller.step(
            0.033,
            later,
            (0.0, 0.0),
            0.0,
            None,
            false, // typing stopped
            Some((win, now)),
            None,
        );
        assert_eq!(controller.last_source, FocusSource::WindowFraming);
        assert!((controller.target_x - 800.0).abs() < 1.0);
        assert!((controller.target_y - 550.0).abs() < 1.0);
    }

    #[test]
    fn focus_auto_zoom_full_lifecycle_and_live_post_parity() {
        let geometry = Geometry {
            x: 0.0,
            y: 0.0,
            width: 1920.0,
            height: 1080.0,
        };
        let mut settings = Settings::default();
        settings.follow_mouse = false;
        settings.focus_auto_zoom = true;
        settings.max_zoom = 2.0;
        settings.typing_zoom = 1.7;

        let mut live_controller = CameraController::new(geometry, settings.clone());
        let mut post_controller = CameraController::new(geometry, settings.clone());

        let win_a = WindowRect {
            x: 100.0,
            y: 100.0,
            width: 800.0,
            height: 600.0,
        };
        let win_b = WindowRect {
            x: 900.0,
            y: 200.0,
            width: 900.0,
            height: 700.0,
        };

        let start = Instant::now();
        let dt = 1.0 / 30.0;

        struct StepEvent {
            t_offset: f64,
            typed: bool,
            active_win: Option<WindowRect>,
            caret_pos: Option<(f64, f64)>,
        }

        let mut timeline: Vec<StepEvent> = Vec::new();

        // 1. No Window (0.0s to 1.0s) -> State A: Fullscreen 1.0x
        for i in 0..30 {
            timeline.push(StepEvent {
                t_offset: i as f64 * dt,
                typed: false,
                active_win: None,
                caret_pos: None,
            });
        }

        // 2. Window A Opens (1.0s to 2.5s) -> State B: Frames full Window A
        for i in 30..75 {
            timeline.push(StepEvent {
                t_offset: i as f64 * dt,
                typed: false,
                active_win: Some(win_a),
                caret_pos: None,
            });
        }

        // 3. Switch to Window B (2.5s to 4.0s) -> State B: Frames full Window B
        for i in 75..120 {
            timeline.push(StepEvent {
                t_offset: i as f64 * dt,
                typed: false,
                active_win: Some(win_b),
                caret_pos: None,
            });
        }

        // 4. User Types in Window B with Caret (4.0s to 5.5s) -> State C: Zooms into Caret
        for i in 120..165 {
            timeline.push(StepEvent {
                t_offset: i as f64 * dt,
                typed: true,
                active_win: Some(win_b),
                caret_pos: Some((1200.0, 450.0)),
            });
        }

        // 5. User Stops Typing for 2.5s (5.5s to 8.5s) -> State D: Back to full Window B
        for i in 165..255 {
            timeline.push(StepEvent {
                t_offset: i as f64 * dt,
                typed: false,
                active_win: Some(win_b),
                caret_pos: None,
            });
        }

        // 6. Window B Closes (8.5s to 10.0s) -> State A: Zooms out to 1.0x Fullscreen
        for i in 255..300 {
            timeline.push(StepEvent {
                t_offset: i as f64 * dt,
                typed: false,
                active_win: None,
                caret_pos: None,
            });
        }

        let mut live_trajectory = Vec::new();
        let mut post_trajectory = Vec::new();

        for ev in &timeline {
            let now = start + Duration::from_secs_f64(ev.t_offset);
            let active_win_arg = ev.active_win.map(|w| (w, now));
            let caret_arg = ev.caret_pos.map(|(cx, cy)| (cx, cy, now));

            let (lx, ly, lz, lsrc) = live_controller.step(
                dt,
                now,
                (960.0, 540.0),
                0.0,
                None,
                ev.typed,
                active_win_arg,
                caret_arg,
            );
            live_trajectory.push((lx, ly, lz, lsrc));

            let (px, py, pz, psrc) = post_controller.step(
                dt,
                now,
                (960.0, 540.0),
                0.0,
                None,
                ev.typed,
                active_win_arg,
                caret_arg,
            );
            post_trajectory.push((px, py, pz, psrc));
        }

        // Verify Strict 1:1 Parity between Live and Post
        assert_eq!(live_trajectory.len(), post_trajectory.len());
        for i in 0..live_trajectory.len() {
            let (lx, ly, lz, lsrc) = live_trajectory[i];
            let (px, py, pz, psrc) = post_trajectory[i];
            assert_eq!(lsrc, psrc, "Source mismatch at step {i}");
            assert!((lx - px).abs() < 1e-9, "X mismatch at step {i}");
            assert!((ly - py).abs() < 1e-9, "Y mismatch at step {i}");
            assert!((lz - pz).abs() < 1e-9, "Z mismatch at step {i}");
        }

        // Verify State Lifecycle milestones:
        // 1. Initial State A: IdleCenter at 1.0x
        assert_eq!(live_trajectory[0].3, FocusSource::IdleCenter);
        assert_eq!(live_controller.target_z, 1.0);

        // 2. Window A open at step 70: WindowFraming
        assert_eq!(live_trajectory[70].3, FocusSource::WindowFraming);
        let (_, _, expected_za) = calculate_window_framing(win_a, geometry, 2.0, 28.0);
        assert!((live_trajectory[70].2 - expected_za).abs() < 0.1);

        // 3. Switch to Window B at step 115: WindowFraming
        assert_eq!(live_trajectory[115].3, FocusSource::WindowFraming);
        let (_, _, expected_zb) = calculate_window_framing(win_b, geometry, 2.0, 28.0);
        assert!((live_trajectory[115].2 - expected_zb).abs() < 0.1);

        // 4. Typing in Window B at step 160: CaretTyping at higher zoom
        assert_eq!(live_trajectory[160].3, FocusSource::CaretTyping);
        assert!(live_trajectory[160].2 > expected_zb);

        // 5. Idle after typing at step 250: Back to WindowFraming
        assert_eq!(live_trajectory[250].3, FocusSource::WindowFraming);
        assert!((live_trajectory[250].2 - expected_zb).abs() < 0.05);

        // 6. Window B closes at step 299: Returns to IdleCenter at 1.0x
        assert_eq!(live_trajectory[299].3, FocusSource::IdleCenter);
        assert!((live_trajectory[299].2 - 1.0).abs() < 0.05);
    }

    #[test]
    fn regression_1_small_floating_window() {
        let geo = Geometry {
            x: 0.0,
            y: 0.0,
            width: 1920.0,
            height: 1080.0,
        };
        let win = WindowRect {
            x: 660.0,
            y: 340.0,
            width: 600.0,
            height: 400.0,
        };
        let (cx, cy, z) = calculate_window_framing(win, geo, 1.9, 28.0);
        assert_eq!(z, 1.9); // max zoom
        assert_eq!(cx, 960.0);
        assert_eq!(cy, 540.0);

        // Entire padded window must comfortably fit inside viewport
        let vw = geo.width / z;
        let vh = geo.height / z;
        assert!(cx - vw / 2.0 <= win.x - 28.0);
        assert!(cx + vw / 2.0 >= win.x + win.width + 28.0);
        assert!(cy - vh / 2.0 <= win.y - 28.0);
        assert!(cy + vh / 2.0 >= win.y + win.height + 28.0);
    }

    #[test]
    fn regression_2_maximized_window() {
        let geo = Geometry {
            x: 0.0,
            y: 0.0,
            width: 1440.0,
            height: 810.0,
        };
        let win = WindowRect {
            x: 12.0,
            y: 38.0,
            width: 1416.0,
            height: 760.0,
        };
        let (cx, cy, z) = calculate_window_framing(win, geo, 1.9, 28.0);
        assert_eq!(z, 1.0);
        assert_eq!(cx, 720.0);
        assert_eq!(cy, 405.0);
    }

    #[test]
    fn regression_3_two_equal_side_by_side_windows() {
        let geo = Geometry {
            x: 0.0,
            y: 0.0,
            width: 1440.0,
            height: 810.0,
        };
        // Two side-by-side windows spanning screen height (760 on 810 display)
        let left = WindowRect {
            x: 12.0,
            y: 38.0,
            width: 701.0,
            height: 760.0,
        };
        let right = WindowRect {
            x: 727.0,
            y: 38.0,
            width: 701.0,
            height: 760.0,
        };
        let (lcx, lcy, lz) = calculate_window_framing(left, geo, 1.9, 28.0);
        let (rcx, rcy, rz) = calculate_window_framing(right, geo, 1.9, 28.0);

        // When windows span height, zooming would crop top/bottom prompts/bars.
        // It must cleanly stay at 1.0x full view at center so 100% of both windows are visible.
        assert_eq!(lz, 1.0);
        assert_eq!(rz, 1.0);
        assert_eq!(lcx, 720.0);
        assert_eq!(rcx, 720.0);
        assert_eq!(lcy, 405.0);
        assert_eq!(rcy, 405.0);

        // However, side-by-side windows that DO fit in both dimensions (e.g. 701x373)
        // must smoothly zoom in and frame 100% of the window without cropping.
        let left_small = WindowRect {
            x: 12.0,
            y: 38.0,
            width: 701.0,
            height: 373.0,
        };
        let (scx, scy, sz) = calculate_window_framing(left_small, geo, 1.9, 28.0);
        assert!(sz >= 1.85 && sz <= 1.90);
        let vw = geo.width / sz;
        let vh = geo.height / sz;
        assert!(scx - vw / 2.0 >= 0.0 && scx + vw / 2.0 <= geo.width);
        assert!(scy - vh / 2.0 >= 0.0 && scy + vh / 2.0 <= geo.height);
    }

    #[test]
    fn regression_4_two_stacked_vertical_windows() {
        let geo = Geometry {
            x: 0.0,
            y: 0.0,
            width: 1440.0,
            height: 810.0,
        };
        // Two stacked windows spanning screen width (1416 on 1440 display)
        let top = WindowRect {
            x: 12.0,
            y: 38.0,
            width: 1416.0,
            height: 373.0,
        };
        let bottom = WindowRect {
            x: 12.0,
            y: 425.0,
            width: 1416.0,
            height: 373.0,
        };
        let (tcx, tcy, tz) = calculate_window_framing(top, geo, 1.9, 28.0);
        let (bcx, bcy, bz) = calculate_window_framing(bottom, geo, 1.9, 28.0);

        // When windows span width, zooming would crop left/right edges.
        // It must cleanly stay at 1.0x full view at center so 100% of both windows are visible.
        assert_eq!(tz, 1.0);
        assert_eq!(bz, 1.0);
        assert_eq!(tcx, 720.0);
        assert_eq!(bcx, 720.0);
        assert_eq!(tcy, 405.0);
        assert_eq!(bcy, 405.0);

        // Stacked windows that DO fit in both dimensions (e.g. 700x373)
        // must smoothly zoom in and frame 100% of the window without cropping.
        let top_small = WindowRect {
            x: 370.0,
            y: 38.0,
            width: 700.0,
            height: 373.0,
        };
        let (scx, scy, sz) = calculate_window_framing(top_small, geo, 1.9, 28.0);
        assert!(sz >= 1.85 && sz <= 1.90);
        let vw = geo.width / sz;
        let vh = geo.height / sz;
        assert!(scx - vw / 2.0 >= 0.0 && scx + vw / 2.0 <= geo.width);
        assert!(scy - vh / 2.0 >= 0.0 && scy + vh / 2.0 <= geo.height);
    }

    #[test]
    fn regression_5_quarter_tiled_windows() {
        let geo = Geometry {
            x: 0.0,
            y: 0.0,
            width: 1440.0,
            height: 810.0,
        };
        let quadrants = [
            WindowRect { x: 12.0, y: 38.0, width: 701.0, height: 373.0 },
            WindowRect { x: 727.0, y: 38.0, width: 701.0, height: 373.0 },
            WindowRect { x: 12.0, y: 425.0, width: 701.0, height: 373.0 },
            WindowRect { x: 727.0, y: 425.0, width: 701.0, height: 373.0 },
        ];

        for (i, win) in quadrants.iter().enumerate() {
            let (cx, cy, z) = calculate_window_framing(*win, geo, 1.9, 28.0);
            assert!(z >= 1.85 && z <= 1.90, "Quadrant {i} should zoom ~1.88, got {z}");
            let vw = geo.width / z;
            let vh = geo.height / z;
            assert!(cx - vw / 2.0 >= 0.0 && cx + vw / 2.0 <= geo.width);
            assert!(cy - vh / 2.0 >= 0.0 && cy + vh / 2.0 <= geo.height);
        }
    }

    #[test]
    fn regression_6_unequal_and_asymmetric_tiled_windows() {
        let geo = Geometry {
            x: 0.0,
            y: 0.0,
            width: 1440.0,
            height: 810.0,
        };
        // 60/40 Master-Stack split where both panes are full-height (760 on 810 display)
        let master = WindowRect {
            x: 12.0,
            y: 38.0,
            width: 864.0,
            height: 760.0,
        };
        let stack = WindowRect {
            x: 888.0,
            y: 38.0,
            width: 540.0,
            height: 760.0,
        };

        let (mcx, mcy, mz) = calculate_window_framing(master, geo, 1.9, 28.0);
        let (scx, scy, sz) = calculate_window_framing(stack, geo, 1.9, 28.0);
        // Both span height: stay at 1.0x so neither window has its top/bottom cut off
        assert_eq!(mz, 1.0);
        assert_eq!(sz, 1.0);
        assert_eq!(mcx, 720.0);
        assert_eq!(scx, 720.0);
        assert_eq!(mcy, 405.0);
        assert_eq!(scy, 405.0);

        // When the stack pane contains 2 smaller stacked windows (540x370),
        // they fit comfortably without cropping height or width:
        let stack_top = WindowRect {
            x: 888.0,
            y: 38.0,
            width: 540.0,
            height: 370.0,
        };
        let (stcx, stcy, stz) = calculate_window_framing(stack_top, geo, 1.9, 28.0);
        assert!(stz >= 1.85 && stz <= 1.90);
        assert!(stcx > 720.0);
        assert!(stcy < 405.0);
    }

    #[test]
    fn regression_7_active_windows_near_capture_edges() {
        // Monitor offset at (100, 50)
        let geo = Geometry {
            x: 100.0,
            y: 50.0,
            width: 1920.0,
            height: 1080.0,
        };
        let top_left = WindowRect { x: 100.0, y: 50.0, width: 400.0, height: 300.0 };
        let bottom_right = WindowRect { x: 1620.0, y: 830.0, width: 400.0, height: 300.0 };

        for win in [top_left, bottom_right] {
            let (cx, cy, z) = calculate_window_framing(win, geo, 1.9, 28.0);
            let vw = geo.width / z;
            let vh = geo.height / z;
            assert!(cx - vw / 2.0 >= geo.x - 1e-6);
            assert!(cx + vw / 2.0 <= geo.x + geo.width + 1e-6);
            assert!(cy - vh / 2.0 >= geo.y - 1e-6);
            assert!(cy + vh / 2.0 <= geo.y + geo.height + 1e-6);
        }
    }

    #[test]
    fn regression_8_different_capture_aspect_ratios() {
        // 21:9 Ultrawide (2560x1080)
        let ultrawide = Geometry { x: 0.0, y: 0.0, width: 2560.0, height: 1080.0 };
        // Full height window (1000 on 1080): stays at 1.0x to avoid cropping height
        let left_uw = WindowRect { x: 20.0, y: 40.0, width: 1240.0, height: 1000.0 };
        let (cx, _, z) = calculate_window_framing(left_uw, ultrawide, 1.9, 28.0);
        assert_eq!(z, 1.0);
        assert_eq!(cx, 1280.0);

        // Floating / half-height window in ultrawide (fits cleanly):
        let float_uw = WindowRect { x: 40.0, y: 40.0, width: 1200.0, height: 500.0 };
        let (fcx, _, fz) = calculate_window_framing(float_uw, ultrawide, 1.9, 28.0);
        assert!(fz >= 1.8 && fz <= 1.9);
        assert!(fcx < 1280.0);

        // 9:16 Vertical Monitor (1080x1920)
        let vertical = Geometry { x: 0.0, y: 0.0, width: 1080.0, height: 1920.0 };
        // Full width window (1040 on 1080): stays at 1.0x to avoid cropping width
        let top_vert = WindowRect { x: 20.0, y: 40.0, width: 1040.0, height: 900.0 };
        let (_, cy, z_v) = calculate_window_framing(top_vert, vertical, 1.9, 28.0);
        assert_eq!(z_v, 1.0);
        assert_eq!(cy, 960.0);

        // Floating / narrower window in vertical monitor:
        let float_vert = WindowRect { x: 40.0, y: 40.0, width: 500.0, height: 900.0 };
        let (_, fcy, fz_v) = calculate_window_framing(float_vert, vertical, 1.9, 28.0);
        assert!(fz_v >= 1.8 && fz_v <= 1.9);
        assert!(fcy < 960.0);

        // 1:1 Square (1200x1200)
        let square = Geometry { x: 0.0, y: 0.0, width: 1200.0, height: 1200.0 };
        let win_sq = WindowRect { x: 20.0, y: 40.0, width: 560.0, height: 1120.0 };
        let (cx_s, _, z_s) = calculate_window_framing(win_sq, square, 1.9, 28.0);
        assert_eq!(z_s, 1.0);
        assert_eq!(cx_s, 600.0);
    }

    #[test]
    fn regression_9_window_focus_switch_does_not_unzoom_during_transient_gap() {
        let geo = Geometry {
            x: 0.0,
            y: 0.0,
            width: 1440.0,
            height: 810.0,
        };
        let mut settings = Settings::default();
        settings.follow_mouse = false;
        settings.focus_auto_zoom = true;
        settings.max_zoom = 1.9;

        let mut controller = CameraController::new(geo, settings);
        let win_a = WindowRect { x: 12.0, y: 38.0, width: 701.0, height: 373.0 };
        let win_b = WindowRect { x: 727.0, y: 38.0, width: 701.0, height: 373.0 };

        let start = Instant::now();
        let dt = 1.0 / 30.0;

        // Step 1: In Window A
        let now = start;
        controller.step(dt, now, (0.0, 0.0), 0.0, None, false, Some((win_a, now)), None);
        let za = controller.target_z;
        assert!(za >= 1.80);
        assert_eq!(controller.last_source, FocusSource::WindowFraming);

        // Step 2-4: Compositor focus handover: active_win is transiently None for 100ms
        for i in 1..=3 {
            let t = start + Duration::from_secs_f64(i as f64 * dt);
            controller.step(dt, t, (0.0, 0.0), 0.0, None, false, None, None);
            // CRITICAL: target_z must NOT collapse to 1.0x during the focus switch!
            assert_eq!(controller.target_z, za, "target_z should hold framing at tick {i}");
            assert_eq!(controller.last_source, FocusSource::WindowFraming);
        }

        // Step 5: Window B arrives at t=133ms
        let t_b = start + Duration::from_millis(133);
        controller.step(dt, t_b, (0.0, 0.0), 0.0, None, false, Some((win_b, t_b)), None);
        assert_eq!(controller.target_z, za, "Window B should maintain identical zoom factor");
        assert!(controller.target_x > 720.0, "Camera should now pan directly toward Window B");
        assert_eq!(controller.last_source, FocusSource::WindowFraming);
    }

    #[test]
    fn regression_10_follow_mouse_in_two_windows_and_smaller_windows() {
        let geo = Geometry {
            x: 0.0,
            y: 0.0,
            width: 1440.0,
            height: 810.0,
        };
        let mut settings = Settings::default();
        settings.follow_mouse = true;
        settings.focus_auto_zoom = true;
        settings.max_zoom = 1.9;

        let mut controller = CameraController::new(geo, settings);
        let two_win_left = WindowRect { x: 12.0, y: 38.0, width: 701.0, height: 760.0 };

        let now = Instant::now();
        // Moving mouse inside one of two side-by-side windows:
        // Must stay at 1.0x (does NOT zoom in to 1.9x and crop the window!)
        controller.step(
            0.033,
            now,
            (200.0, 300.0),
            10.0,
            None,
            false,
            Some((two_win_left, now)),
            None,
        );
        assert_eq!(controller.target_z, 1.0, "follow_mouse in large split window must stay at 1.0x");
        assert_eq!(controller.target_x, 720.0);
        assert_eq!(controller.target_y, 405.0);

        // Clicking inside one of two side-by-side windows:
        // Must stay at 1.0x (does NOT zoom in to 1.9x and crop!)
        let click_t = now + Duration::from_millis(100);
        controller.step(
            0.033,
            click_t,
            (200.0, 300.0),
            0.0,
            Some((200.0, 300.0, click_t)),
            false,
            Some((two_win_left, click_t)),
            None,
        );
        assert_eq!(controller.target_z, 1.0, "click in large split window must stay at 1.0x");

        // Small floating window:
        let small_win = WindowRect { x: 370.0, y: 200.0, width: 700.0, height: 373.0 };
        let small_t = click_t + Duration::from_millis(200);
        controller.step(
            0.033,
            small_t,
            (500.0, 300.0),
            10.0,
            None,
            false,
            Some((small_win, small_t)),
            None,
        );
        assert!(controller.target_z >= 1.85, "follow_mouse in small window should zoom to fit: got {}", controller.target_z);
        // Viewport must never crop small_win:
        let vw = geo.width / controller.target_z;
        let vh = geo.height / controller.target_z;
        assert!(controller.target_x - vw / 2.0 <= small_win.x);
        assert!(controller.target_x + vw / 2.0 >= small_win.x + small_win.width);
        assert!(controller.target_y - vh / 2.0 <= small_win.y);
        assert!(controller.target_y + vh / 2.0 >= small_win.y + small_win.height);
    }

    #[test]
    fn regression_11_fullscreen_window_never_zooms_with_typing_or_mouse() {
        let geo = Geometry {
            x: 0.0,
            y: 0.0,
            width: 1728.0,
            height: 972.0,
        };
        let mut settings = Settings::default();
        settings.follow_mouse = true;
        settings.focus_auto_zoom = true;
        settings.max_zoom = 2.0;
        settings.typing_zoom = 1.75;

        let mut controller = CameraController::new(geo, settings);
        // Single tiled / maximized / fullscreen terminal occupying the workspace
        let win = WindowRect {
            x: 12.0,
            y: 38.0,
            width: 1704.0,
            height: 922.0,
        };

        let now = Instant::now();

        // 1. Mouse moving inside window: must stay at 1.0x at capture center
        controller.step(0.033, now, (500.0, 400.0), 20.0, None, false, Some((win, now)), None);
        assert_eq!(controller.target_z, 1.0);
        assert_eq!(controller.target_x, 864.0);
        assert_eq!(controller.target_y, 486.0);

        // 2. Mouse dwelling inside window: must stay at 1.0x at capture center
        let t1 = now + Duration::from_millis(50);
        controller.step(0.033, t1, (500.0, 400.0), 0.0, None, false, Some((win, t1)), None);
        assert_eq!(controller.target_z, 1.0);

        // 3. Mouse clicking inside window: must stay at 1.0x at capture center
        let t2 = t1 + Duration::from_millis(50);
        controller.step(0.033, t2, (500.0, 400.0), 0.0, Some((500.0, 400.0, t2)), false, Some((win, t2)), None);
        assert_eq!(controller.target_z, 1.0);

        // 4. Mouse moving to Omarchy top bar (outside window bounds) and clicking: must stay at 1.0x
        let t3 = t2 + Duration::from_millis(50);
        controller.step(0.033, t3, (500.0, 15.0), 25.0, Some((500.0, 15.0, t3)), false, Some((win, t3)), None);
        assert_eq!(controller.target_z, 1.0, "clicking top bar with fullscreen window active must stay at 1.0x");
        assert_eq!(controller.target_x, 864.0);
        assert_eq!(controller.target_y, 486.0);

        // 5. Typing with caret active in fullscreen window: must stay at 1.0x
        let t4 = t3 + Duration::from_millis(100);
        controller.step(0.033, t4, (500.0, 400.0), 0.0, None, true, Some((win, t4)), Some((200.0, 800.0, t4)));
        assert_eq!(controller.target_z, 1.0, "typing in fullscreen window must stay at 1.0x");
        assert_eq!(controller.target_x, 864.0);
        assert_eq!(controller.target_y, 486.0);

        // 6. True fullscreen window (at: 0,0, size: 1728x972): must also stay at 1.0x
        let true_fullscreen = WindowRect {
            x: 0.0,
            y: 0.0,
            width: 1728.0,
            height: 972.0,
        };
        let t5 = t4 + Duration::from_millis(100);
        controller.step(0.033, t5, (200.0, 200.0), 10.0, None, true, Some((true_fullscreen, t5)), Some((200.0, 200.0, t5)));
        assert_eq!(controller.target_z, 1.0, "true fullscreen must never zoom");
        assert_eq!(controller.target_x, 864.0);
        assert_eq!(controller.target_y, 486.0);
    }

    /// End-to-end: a curve with real zoom must actually change the pixels.
    #[test]
    fn camera_pass_renders_the_zoom_curve() {
        if !util::have("ffmpeg") || !util::have("ffprobe") {
            return;
        }
        let dir = std::env::temp_dir().join(format!("reel-zoom-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("temp dir");
        let file = dir.join("take.mp4");
        let made = Command::new("ffmpeg")
            .args([
                "-y",
                "-f",
                "lavfi",
                "-i",
                "testsrc2=size=320x180:rate=30:duration=2",
                "-c:v",
                "libx264",
                "-pix_fmt",
                "yuv420p",
            ])
            .arg(&file)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map(|status| status.success())
            .unwrap_or(false);
        if !made {
            return;
        }
        let before = dir.join("before.png");
        grab_frame(&file, 1.5, &before);

        let sidecar = file.with_extension("zoom.json");
        let mut text = String::new();
        for step in 0..=250 {
            let t = step as f64 * 0.007;
            let z = 1.0 + (t / 0.9).min(1.0);
            text.push_str(&format!(
                "{{\"t\":{t:.3},\"x\":160.0,\"y\":90.0,\"z\":{z:.4}}}\n"
            ));
        }
        std::fs::write(&sidecar, text).expect("sidecar");

        let ok = apply_pass(
            &file,
            &sidecar,
            "region:320x180+0+0",
            Some(&dir.join("pass.log")),
        );
        if !ok {
            let log = std::fs::read_to_string(dir.join("pass.log")).unwrap_or_default();
            panic!("the camera pass failed; log:\n{log}");
        }
        assert!(!sidecar.exists());
        assert!(file.is_file());

        let after = dir.join("after.png");
        grab_frame(&file, 1.5, &after);
        let psnr = psnr(&before, &after).expect("psnr");
        // A real crop would be very different; an unchanged frame is ~40 dB.
        assert!(psnr < 25.0, "the camera pass did not zoom: {psnr} dB");
        let _ = std::fs::remove_dir_all(&dir);
    }

    fn grab_frame(file: &Path, at: f64, out: &Path) {
        let _ = Command::new("ffmpeg")
            .args(["-y", "-ss", &format!("{at:.3}"), "-i"])
            .arg(file)
            .args(["-frames:v", "1"])
            .arg(out)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    }

    fn psnr(a: &Path, b: &Path) -> Option<f64> {
        let output = util::run(
            "ffmpeg",
            &[
                "-hide_banner",
                "-i",
                &a.display().to_string(),
                "-i",
                &b.display().to_string(),
                "-lavfi",
                "psnr",
                "-f",
                "null",
                "-",
            ],
        )?;
        // ffmpeg reports the summary on stderr.
        let tail = output.stderr.rsplit("average:").next()?;
        tail.split_whitespace()
            .next()?
            .trim_end_matches(|c: char| !c.is_ascii_digit() && c != '.')
            .parse()
            .ok()
    }
}

#[cfg(test)]
mod dump {
    use super::*;

    #[test]
    fn dump_real_trace() {
        let Ok(text) = std::fs::read_to_string("/tmp/opencode/traceC.json") else {
            return;
        };
        let frames: Vec<Keyframe> = text
            .lines()
            .filter_map(|line| serde_json::from_str::<Keyframe>(line).ok())
            .collect();
        let geometry = Geometry {
            x: 760.0,
            y: 420.0,
            width: 640.0,
            height: 360.0,
        };
        let video = VideoInfo {
            width: 1066,
            height: 600,
            fps: 60.0,
        };
        let script = filter_script(&frames, geometry, video).expect("script");
        println!("LEN={}", script.len());
        println!("ANCHORS={}", script.matches("gte(on,").count());
        println!("HEAD={}", &script[..script.len().min(400)]);
        println!("TAIL={}", &script[script.len().saturating_sub(300)..]);
    }
}
