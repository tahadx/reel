//! The small pieces of mouse auto zoom that are shared by the recorder, the
//! save pass, and the live zoom controls.
//!
//! Reel's auto zoom is a save-pass camera: the take records pointer evidence and
//! the save pass turns it into keyframes. That means the rules about what counts
//! as a click, which clicks are inside the capture, how a follow target is
//! filtered, and when a scroll extends a shot all live here as pure functions so
//! they can be tested without a compositor or an ffmpeg run.
//!
//! The live manual zoom still owns the compositor camera while recording. It
//! uses [`Spring`] for its transition, and [`ZoomSession`] is the policy any
//! future live automation must consult before a timeout may reset a manual
//! zoom. The save pass never touches the compositor factor.

use std::f64::consts::LN_2;

/// A click has to be released within this long to count as a click rather than
/// a hold. A long press is usually a context menu, a text selection, or a drag.
pub const CLICK_MAX_HOLD_SECS: f64 = 0.280;
/// The pointer may wobble this far between press and release and still be a
/// click. Anything farther is a drag.
pub const CLICK_MAX_TRAVEL_PX: f64 = 15.0;
/// A drag cancels an armed click once it travels this far while held. The
/// release gate above is stricter for ordinary clicks; this constant documents
/// the drag line and is used when a release is missing from a truncated log.
pub const DRAG_CANCEL_PX: f64 = 20.0;
/// A zoom-in settles in roughly four half-lives, about 720 ms.
pub const ZOOM_IN_HALF_LIFE_SECS: f64 = 0.180;
/// Zooming out is deliberately a little slower so the context returns gently.
pub const ZOOM_OUT_HALF_LIFE_SECS: f64 = 0.260;
/// Below this the spring is considered settled, in zoom units.
pub const SPRING_EPSILON: f64 = 0.001;

/// One press/release position in telemetry time.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Press {
    pub t: f64,
    pub x: f64,
    pub y: f64,
}

impl Press {
    pub fn new(t: f64, x: f64, y: f64) -> Self {
        Self { t, x, y }
    }
}

/// A short, still press/release pair is a click. A drag or a hold is not.
pub fn qualifies_as_click(press: Press, release: Press) -> bool {
    if !(press.t.is_finite()
        && press.x.is_finite()
        && press.y.is_finite()
        && release.t.is_finite()
        && release.x.is_finite()
        && release.y.is_finite())
    {
        return false;
    }
    let held = release.t - press.t;
    if held < 0.0 || held > CLICK_MAX_HOLD_SECS + 1e-9 {
        return false;
    }
    let dx = release.x - press.x;
    let dy = release.y - press.y;
    dx * dx + dy * dy <= CLICK_MAX_TRAVEL_PX * CLICK_MAX_TRAVEL_PX + 1e-9
}

/// Whether a press that never reported a release was a drag. Used only for a
/// truncated telemetry log: a press with no release and no movement is kept so
/// an interrupted log does not silently lose the take's clicks.
pub fn unpaired_press_is_click(press: Press, samples: &[(f64, f64, f64)]) -> bool {
    if !(press.t.is_finite() && press.x.is_finite() && press.y.is_finite()) {
        return false;
    }
    for &(t, x, y) in samples {
        if t < press.t - 1e-9 || t > press.t + CLICK_MAX_HOLD_SECS + 1e-9 {
            continue;
        }
        if !(x.is_finite() && y.is_finite()) {
            continue;
        }
        let dx = x - press.x;
        let dy = y - press.y;
        if dx * dx + dy * dy > DRAG_CANCEL_PX * DRAG_CANCEL_PX + 1e-9 {
            return false;
        }
    }
    true
}

/// The three capture shapes. The save pass only sees capture-local pixels, but
/// the mode is kept with the bounds so tests can cover a window and an area
/// explicitly instead of assuming a monitor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CaptureMode {
    Monitor,
    Window,
    Area,
}

impl CaptureMode {
    pub fn from_kind(kind: &str) -> Self {
        match kind {
            "window" => Self::Window,
            "region" | "area" => Self::Area,
            _ => Self::Monitor,
        }
    }
}

/// The rectangle a click has to land in, in the same coordinate space as the
/// encoded frame. For a monitor this starts at zero; window and area captures
/// use the same local frame because telemetry maps the pointer before it writes
/// a click.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CaptureBounds {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

impl CaptureBounds {
    pub fn new(x: f64, y: f64, width: f64, height: f64) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }

    /// The encoded frame's own rectangle.
    pub fn frame(width: f64, height: f64) -> Self {
        Self::new(0.0, 0.0, width, height)
    }

    pub fn is_valid(&self) -> bool {
        self.x.is_finite()
            && self.y.is_finite()
            && self.width.is_finite()
            && self.height.is_finite()
            && self.width > 0.0
            && self.height > 0.0
    }

    /// Strict containment: the right and bottom edges belong to the next pixel,
    /// so a click exactly at `width`/`height` is outside the capture.
    pub fn contains(&self, x: f64, y: f64) -> bool {
        self.is_valid()
            && x.is_finite()
            && y.is_finite()
            && x >= self.x
            && x < self.x + self.width
            && y >= self.y
            && y < self.y + self.height
    }

    /// Clamp a viewport centre so the crop stays inside the capture at `zoom`.
    /// The minimum and maximum are the requested centre limits:
    /// `x + width / (2 * zoom)` and `x + width - width / (2 * zoom)`.
    pub fn clamp_center(&self, x: f64, y: f64, zoom: f64) -> (f64, f64) {
        if !self.is_valid() {
            return (x, y);
        }
        let z = finite_zoom(zoom);
        let fallback = (self.x + self.width / 2.0, self.y + self.height / 2.0);
        let cx = if x.is_finite() { x } else { fallback.0 };
        let cy = if y.is_finite() { y } else { fallback.1 };

        let half_w = self.width / (2.0 * z);
        let half_h = self.height / (2.0 * z);
        let min_x = self.x + half_w;
        let max_x = self.x + self.width - half_w;
        let min_y = self.y + half_h;
        let max_y = self.y + self.height - half_h;
        (
            if min_x <= max_x {
                cx.clamp(min_x, max_x)
            } else {
                fallback.0
            },
            if min_y <= max_y {
                cy.clamp(min_y, max_y)
            } else {
                fallback.1
            },
        )
    }

    /// The crop rectangle for a centre and zoom. The centre is clamped first, so
    /// the result can never be negative or extend past the capture.
    #[cfg(test)]
    pub fn crop(&self, center: (f64, f64), zoom: f64) -> (f64, f64, f64, f64) {
        let z = finite_zoom(zoom);
        let (cx, cy) = self.clamp_center(center.0, center.1, z);
        let width = self.width / z;
        let height = self.height / z;
        let x = (cx - width / 2.0).clamp(self.x, self.x + self.width - width);
        let y = (cy - height / 2.0).clamp(self.y, self.y + self.height - height);
        (x, y, width, height)
    }
}

fn finite_zoom(zoom: f64) -> f64 {
    if zoom.is_finite() {
        zoom.clamp(1.0, 4.0)
    } else {
        1.0
    }
}

/// A critically damped second-order spring. Retargeting changes only the
/// target; the current value and velocity are kept, so a click in the middle of
/// a zoom-out cannot make the camera jump.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Spring {
    value: f64,
    velocity: f64,
    target: f64,
    half_life: f64,
}

impl Spring {
    pub fn new(value: f64, half_life: f64) -> Self {
        let value = if value.is_finite() { value } else { 0.0 };
        Self {
            value,
            velocity: 0.0,
            target: value,
            half_life: valid_half_life(half_life),
        }
    }

    pub fn value(&self) -> f64 {
        self.value
    }

    /// Move the target without disturbing the state that is already in flight.
    pub fn retarget(&mut self, target: f64, half_life: f64) {
        if target.is_finite() {
            self.target = target;
        }
        self.half_life = valid_half_life(half_life);
    }

    /// Advance the exact critically damped solution by `dt` seconds.
    pub fn step(&mut self, dt: f64) {
        if !dt.is_finite() || dt <= 0.0 {
            return;
        }
        let omega = LN_2 / self.half_life;
        let decay = (-omega * dt).exp();
        let a = self.value - self.target;
        let b = self.velocity + omega * a;
        let next = self.target + (a + b * dt) * decay;
        let next_velocity = (b - omega * (a + b * dt)) * decay;
        if next.is_finite() {
            self.value = next;
        }
        if next_velocity.is_finite() {
            self.velocity = next_velocity;
        }
    }

    pub fn settled(&self, epsilon: f64) -> bool {
        (self.value - self.target).abs() <= epsilon.abs().max(1e-9)
            && self.velocity.abs() <= epsilon.abs().max(1e-9)
    }
}

fn valid_half_life(half_life: f64) -> f64 {
    if half_life.is_finite() && half_life > 0.0 {
        half_life
    } else {
        ZOOM_IN_HALF_LIFE_SECS
    }
}

/// Half-life for a zoom transition: slower on the way out.
pub fn zoom_half_life(current: f64, target: f64) -> f64 {
    if target > current {
        ZOOM_IN_HALF_LIFE_SECS
    } else {
        ZOOM_OUT_HALF_LIFE_SECS
    }
}

/// Three springs that keep the zoom and the viewport centre together.
///
/// The save pass writes keyframes rather than running a live camera, so this
/// model exists for the spring tests and for any future live automation.
#[cfg(test)]
#[derive(Debug, Clone, Copy)]
pub struct CameraSpring {
    pub scale: Spring,
    pub x: Spring,
    pub y: Spring,
    bounds: CaptureBounds,
}

#[cfg(test)]
impl CameraSpring {
    pub fn new(scale: f64, x: f64, y: f64, bounds: CaptureBounds) -> Self {
        let scale = finite_zoom(scale);
        let (x, y) = bounds.clamp_center(x, y, scale);
        Self {
            scale: Spring::new(scale, ZOOM_IN_HALF_LIFE_SECS),
            x: Spring::new(x, ZOOM_IN_HALF_LIFE_SECS),
            y: Spring::new(y, ZOOM_IN_HALF_LIFE_SECS),
            bounds,
        }
    }

    /// Retarget without zeroing any velocity. A click during a zoom-out simply
    /// changes where the camera is heading.
    pub fn retarget(&mut self, scale: f64, x: f64, y: f64) {
        let scale = finite_zoom(scale);
        let half_life = zoom_half_life(self.scale.value(), scale);
        let (x, y) = self.bounds.clamp_center(x, y, scale);
        self.scale.retarget(scale, half_life);
        self.x.retarget(x, half_life);
        self.y.retarget(y, half_life);
    }

    pub fn step(&mut self, dt: f64) -> (f64, f64, f64) {
        self.scale.step(dt);
        let scale = finite_zoom(self.scale.value());
        if (scale - self.scale.value()).abs() > 1e-9 {
            // The compositor cannot go below 1x or above 4x; stop pushing.
            self.scale.value = scale;
            self.scale.velocity = 0.0;
        }
        self.x.step(dt);
        self.y.step(dt);
        let (x, y) = self
            .bounds
            .clamp_center(self.x.value(), self.y.value(), scale);
        // Hitting a capture edge is a hard stop; leave the other axis alone and
        // never let a rejection turn into a NaN or an out-of-frame crop.
        if (x - self.x.value()).abs() > 1e-9 {
            self.x.value = x;
            self.x.velocity = 0.0;
        }
        if (y - self.y.value()).abs() > 1e-9 {
            self.y.value = y;
            self.y.velocity = 0.0;
        }
        (scale, self.x.value(), self.y.value())
    }
}

/// One scroll-wheel or touchpad scroll report in telemetry time.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ScrollTick {
    pub t: f64,
    pub x: f64,
    pub y: f64,
    /// Positive is up/away, negative is down/towards. High-resolution reports
    /// are normalised so one mouse notch is `1.0`.
    pub delta: f64,
}

/// Who currently owns the camera.
#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ZoomMode {
    Auto,
    Manual,
}

/// The strict ownership rule for manual zoom.
///
/// A manual action always wins. While manual zoom is above 1x, an auto-zoom
/// timeout is not allowed to return the camera to 1x; only an explicit
/// `Ctrl+Alt+0` (or a manual step back to 1x) releases the camera to auto mode.
///
/// The save pass never owns the live compositor camera, so this policy has no
/// caller in the save pass today. It is the contract the unit tests pin for the
/// live automation path.
#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ZoomSession {
    mode: ZoomMode,
    manual_scale: f64,
    hold_until: f64,
}

impl Default for ZoomSession {
    fn default() -> Self {
        Self {
            mode: ZoomMode::Auto,
            manual_scale: 1.0,
            hold_until: 0.0,
        }
    }
}

#[allow(dead_code)]
impl ZoomSession {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn mode(&self) -> ZoomMode {
        self.mode
    }

    pub fn manual_scale(&self) -> f64 {
        self.manual_scale
    }

    /// Record a manual zoom action. At 1x the camera is back under auto
    /// control, which is what makes `Ctrl+Alt+0` a real release.
    pub fn manual_zoom(&mut self, scale: f64) {
        let scale = finite_zoom(scale);
        if scale > 1.0 + 1e-9 {
            self.mode = ZoomMode::Manual;
            self.manual_scale = scale;
        } else {
            self.release();
        }
    }

    pub fn hold_auto_until(&mut self, t: f64) {
        if t.is_finite() && t > self.hold_until {
            self.hold_until = t;
        }
    }

    /// The scale an auto-zoom timeout may apply at `now`, or `None` when manual
    /// ownership blocks it.
    pub fn auto_timeout(&mut self, now: f64) -> Option<f64> {
        if !now.is_finite() || now < self.hold_until - 1e-9 {
            return None;
        }
        match self.mode {
            ZoomMode::Manual if self.manual_scale > 1.0 + 1e-9 => None,
            _ => Some(1.0),
        }
    }

    /// Explicit release back to the automatic camera.
    pub fn release(&mut self) {
        self.mode = ZoomMode::Auto;
        self.manual_scale = 1.0;
        self.hold_until = 0.0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn press(t: f64, x: f64, y: f64) -> Press {
        Press::new(t, x, y)
    }

    #[test]
    fn a_short_still_click_qualifies() {
        assert!(qualifies_as_click(
            press(1.0, 100.0, 100.0),
            press(1.12, 106.0, 104.0)
        ));
    }

    #[test]
    fn a_held_or_moved_press_is_not_a_click() {
        // A 300 ms hold is a context menu or selection.
        assert!(!qualifies_as_click(
            press(1.0, 100.0, 100.0),
            press(1.30, 100.0, 100.0)
        ));
        // A fast drag can be short; distance alone must reject it.
        assert!(!qualifies_as_click(
            press(1.0, 100.0, 100.0),
            press(1.12, 140.0, 100.0)
        ));
        // A 20 px drag is well past the 15 px click slop.
        assert!(!qualifies_as_click(
            press(1.0, 100.0, 100.0),
            press(1.12, 120.0, 100.0)
        ));
    }

    #[test]
    fn an_unpaired_press_is_kept_only_when_nothing_moved() {
        let samples = vec![(1.01, 101.0, 100.0), (1.2, 140.0, 100.0)];
        assert!(!unpaired_press_is_click(press(1.0, 100.0, 100.0), &samples));
        let still = vec![(1.01, 101.0, 100.0), (1.2, 102.0, 101.0)];
        assert!(unpaired_press_is_click(press(1.0, 100.0, 100.0), &still));
    }

    #[test]
    fn capture_bounds_reject_edges_and_outside_clicks() {
        let bounds = CaptureBounds::frame(1000.0, 800.0);
        assert!(bounds.contains(0.0, 0.0));
        assert!(bounds.contains(999.999, 799.999));
        // The right/bottom edge is outside; the next pixel starts there.
        assert!(!bounds.contains(1000.0, 400.0));
        assert!(!bounds.contains(400.0, 800.0));
        assert!(!bounds.contains(-0.01, 400.0));
        assert!(!bounds.contains(400.0, 800.01));
        assert!(!bounds.contains(f64::NAN, 0.0));
    }

    #[test]
    fn window_and_area_bounds_keep_their_own_origin() {
        let window = CaptureBounds::new(200.0, 100.0, 640.0, 480.0);
        assert!(!window.contains(100.0, 300.0));
        assert!(window.contains(200.0, 100.0));
        assert!(window.contains(839.0, 579.0));
        let (x, y) = window.clamp_center(0.0, 0.0, 2.0);
        assert_eq!((x, y), (360.0, 220.0));
    }

    #[test]
    fn a_click_on_any_edge_never_makes_a_bad_crop() {
        let bounds = CaptureBounds::frame(1000.0, 800.0);
        for (x, y) in [(0.0, 0.0), (999.0, 0.0), (0.0, 799.0), (999.0, 799.0)] {
            let (cx, cy) = bounds.clamp_center(x, y, 2.0);
            let (left, top, width, height) = bounds.crop((cx, cy), 2.0);
            assert!(left >= 0.0 && top >= 0.0, "crop at {x},{y}: {left},{top}");
            assert!(left + width <= 1000.0 + 1e-9, "crop at {x},{y} overflows");
            assert!(top + height <= 800.0 + 1e-9, "crop at {x},{y} overflows");
        }
    }

    #[test]
    fn the_spring_retarget_keeps_position_and_velocity() {
        let mut spring = Spring::new(1.0, ZOOM_IN_HALF_LIFE_SECS);
        spring.retarget(2.0, ZOOM_IN_HALF_LIFE_SECS);
        for _ in 0..4 {
            spring.step(0.016);
        }
        let value = spring.value;
        let velocity = spring.velocity;
        assert!(value > 1.0 && velocity > 0.0);

        // A click during the zoom-out changes only the destination.
        spring.retarget(1.2, ZOOM_OUT_HALF_LIFE_SECS);
        assert_eq!(spring.value, value);
        assert_eq!(spring.velocity, velocity);
        spring.step(0.001);
        assert!(spring.value.is_finite() && spring.velocity.is_finite());
        assert!((spring.value - value).abs() < 0.01);
    }

    #[test]
    fn a_spring_outlives_a_retarget_in_the_middle_of_a_zoom_out() {
        let mut camera = CameraSpring::new(2.0, 500.0, 400.0, CaptureBounds::frame(1000.0, 800.0));
        camera.retarget(1.0, 180.0, 120.0);
        for _ in 0..10 {
            camera.step(0.016);
        }
        let (scale, x, y) = camera.step(0.001);
        assert!((1.0..2.0).contains(&scale));
        assert!(x.is_finite() && y.is_finite());

        // A click while the camera is heading back to 1x: no snap, just a new
        // destination. The camera may keep drifting out for a frame because
        // velocity is continuous, which is exactly the point.
        let before = camera.step(0.001);
        camera.retarget(2.0, 900.0, 700.0);
        let after = camera.step(0.001);
        assert!(after.0 <= 2.0);
        assert!((after.0 - before.0).abs() < 0.05);
        assert!(after.1.is_finite() && after.2.is_finite());
    }

    #[test]
    fn a_spring_at_a_capture_edge_does_not_go_nan_or_negative() {
        let bounds = CaptureBounds::frame(640.0, 360.0);
        let mut camera = CameraSpring::new(1.0, 320.0, 180.0, bounds);
        camera.retarget(2.0, -1000.0, 5000.0);
        for _ in 0..240 {
            let (scale, x, y) = camera.step(1.0 / 60.0);
            assert!(scale.is_finite() && (1.0..=4.0).contains(&scale));
            let (left, top, width, height) = bounds.crop((x, y), scale);
            assert!(left >= 0.0 && top >= 0.0);
            assert!(left + width <= 640.0 + 1e-9);
            assert!(top + height <= 360.0 + 1e-9);
        }
    }

    #[test]
    fn a_manual_zoom_cannot_be_reset_by_an_auto_timeout() {
        let mut session = ZoomSession::new();
        session.hold_auto_until(1.0);
        assert_eq!(session.auto_timeout(2.0), Some(1.0));

        session.manual_zoom(2.0);
        assert_eq!(session.mode(), ZoomMode::Manual);
        session.hold_auto_until(10.0);
        assert_eq!(session.auto_timeout(100.0), None);

        // Stepping back to 1x, or Ctrl+Alt+0, is the only release.
        session.manual_zoom(1.0);
        assert_eq!(session.mode(), ZoomMode::Auto);
        assert_eq!(session.auto_timeout(100.0), Some(1.0));
    }
}
