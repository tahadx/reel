use crate::paths::Paths;
use crate::util;
use std::time::Duration;

/// Reads current live compositor zoom factor from Hyprland.
pub fn compositor_zoom() -> f64 {
    let out = util::run_stdout_within(
        "hyprctl",
        &["getoption", "cursor:zoom_factor"],
        Duration::from_millis(400),
    )
    .unwrap_or_default();

    for line in out.lines() {
        if let Some(rest) = line.strip_prefix("float:") {
            if let Ok(val) = rest.trim().parse::<f64>() {
                if val.is_finite() && val >= 1.0 {
                    return val;
                }
            }
        }
    }
    1.0
}

/// The live zoom camera.
///
/// While a monitor is recorded the camera follows the pointer and is rigid, so
/// moving the pointer pans the view and the pointer stays near the centre of
/// the crop. A window or area capture keeps the camera detached instead: the
/// pointer holds its real position, which is what keeps it inside a small
/// capture. At 1x the camera is detached again, the normal desktop state.
fn camera_lua(zoom: f64, follow: bool) -> String {
    let z = zoom.clamp(1.0, 4.0);
    let zoomed = z > 1.01;
    let detached = !follow || !zoomed;
    let rigid = follow && zoomed;
    format!(
        "hl.config({{ cursor = {{ zoom_factor = {z:.3}, zoom_detached_camera = {}, zoom_rigid = {} }} }})",
        detached, rigid,
    )
}

/// Sets live compositor zoom factor in Hyprland. `follow` is true only for
/// monitor captures; see `camera_lua`.
pub fn set_compositor_zoom(zoom: f64, follow: bool) -> bool {
    util::run_ok("hyprctl", &["eval", &camera_lua(zoom, follow)])
}

/// Pointer position in compositor coordinates.
pub fn cursor_position() -> Option<(f64, f64)> {
    let out = util::run_stdout_within("hyprctl", &["cursorpos"], Duration::from_millis(400))?;
    let (x, y) = out.trim().split_once(',')?;
    Some((x.trim().parse().ok()?, y.trim().parse().ok()?))
}

/// Where a pan step lands, kept inside `bounds` (the recorded target) when it
/// is known so the view cannot walk off the capture.
pub fn pan_target(
    x: f64,
    y: f64,
    dx: f64,
    dy: f64,
    bounds: Option<(f64, f64, f64, f64)>,
) -> (f64, f64) {
    let mut next_x = x + dx;
    let mut next_y = y + dy;
    if let Some((bx, by, bw, bh)) = bounds {
        if bw > 0.0 && bh > 0.0 {
            next_x = next_x.clamp(bx, bx + bw - 1.0);
            next_y = next_y.clamp(by, by + bh - 1.0);
        }
    }
    (next_x, next_y)
}

/// Pan the live view by moving the pointer while the camera is attached. The
/// zoom factor is never touched, so a pan cannot change the depth by accident.
pub fn pan_view(dx: f64, dy: f64, bounds: Option<(f64, f64, f64, f64)>) -> bool {
    let Some((x, y)) = cursor_position() else {
        return false;
    };
    let (next_x, next_y) = pan_target(x, y, dx, dy, bounds);
    let lua = format!(
        "hl.config({{ cursor = {{ zoom_detached_camera = false, zoom_rigid = true }} }})\nhl.dispatch(hl.dsp.cursor.move({{ x = {next_x:.1}, y = {next_y:.1} }}))"
    );
    util::run_ok("hyprctl", &["eval", &lua])
}

const ZOOM_MARKER: &str = "zoom_gestures";
/// Marker content for a take that did not override a user gesture.
const MARKER_OURS: &str = "1";
/// Marker content for a take that had to remove a user pinch binding first.
const MARKER_PRIOR: &str = "prior";

/// Removes the two-finger pinch gesture. Returns true when one was registered,
/// which also means the user had their own binding for Reel to override.
fn unset_pinch_gesture() -> bool {
    util::run_ok(
        "hyprctl",
        &[
            "eval",
            "hl.gesture({ fingers = 2, direction = 'pinch', action = 'unset' })",
        ],
    )
}

/// Re-registers the gestures declared in the user's Lua config. Reloading is
/// the only way to recover a pinch binding; Hyprland has no gesture getter.
fn restore_config_gestures() {
    let _ = util::run_ok("hyprctl", &["reload"]);
}

fn marker_restores_prior(contents: &str) -> bool {
    contents.trim() == MARKER_PRIOR
}

/// Activate manual zoom for recording: enables live tracking and registers touchpad pinch gestures.
///
/// `follow` is true for monitor captures, where the camera tracks the pointer.
/// Window and area captures keep the camera detached so the pointer keeps its
/// real position instead of being pulled toward the screen centre.
pub fn enable_recording_zoom(paths: &Paths, max_zoom: f64, follow: bool) {
    let marker = paths.runtime.join(ZOOM_MARKER);
    let max_z = max_zoom.clamp(1.1, 4.0);
    let detached = if follow { "false" } else { "true" };
    let rigid = if follow { "true" } else { "false" };

    // A marker from a take that crashed still points at Reel's gesture. Undo it
    // first so the probe below sees the user's own binding rather than ours.
    if marker.exists() {
        disable_recording_zoom(paths);
    }

    // Hyprland rejects a second gesture on the same fingers and direction, so a
    // user's pinch binding has to come off for the take. Remember that and put
    // it back when the take ends.
    let had_prior = unset_pinch_gesture();

    let lua = format!(
        r#"
        _G.reel_zoom_handler = function()
            local start_z = 1.0
            local last_z = 1.0
            local mode = 'zoom'
            local follow = {follow}
            local accum_dx = 0
            local accum_dy = 0

            return {{
                start = function(ev)
                    start_z = hl.get_config('cursor:zoom_factor') or 1.0
                    last_z = start_z
                    -- A detached camera has no pan gesture: pinch only zooms.
                    mode = (start_z <= 1.02 or not follow) and 'zoom' or 'undecided'
                    accum_dx = 0
                    accum_dy = 0
                end,
                update = function(ev)
                    if not ev then return end

                    local dx = (ev.delta and ev.delta.x) or 0
                    local dy = (ev.delta and ev.delta.y) or 0
                    local scale = ev.scale or 1.0
                    local scale_diff = math.abs(scale - 1.0)

                    accum_dx = accum_dx + dx
                    accum_dy = accum_dy + dy
                    local pan_dist = math.sqrt(accum_dx * accum_dx + accum_dy * accum_dy)

                    -- Intent arbitration: at 1.0x, zoom responds immediately with zero deadzone.
                    -- When already zoomed in, disambiguate smoothly between zoom and viewport pan.
                    if mode == 'undecided' then
                        if scale_diff >= 0.008 then
                            mode = 'zoom'
                        elseif pan_dist >= 8.0 and pan_dist > (scale_diff * 400.0) then
                            mode = 'pan'
                            hl.config({{ cursor = {{ zoom_detached_camera = false, zoom_rigid = true }} }})
                        end
                    end

                    if mode == 'zoom' then
                        if scale > 0 then
                            local z = math.max(1.0, math.min({max_z:.2}, start_z * scale))
                            if math.abs(z - last_z) >= 0.002 then
                                last_z = z
                                hl.config({{ cursor = {{ zoom_factor = z, zoom_detached_camera = {detached}, zoom_rigid = {rigid} }} }})
                            end
                        end
                    elseif mode == 'pan' then
                        if dx ~= 0 or dy ~= 0 then
                            local p = hl.get_cursor_pos()
                            if p and p.x and p.y then
                                hl.dispatch(hl.dsp.cursor.move({{
                                    x = p.x + dx,
                                    y = p.y + dy
                                }}))
                            end
                        end
                    end
                end,
                finish = function(ev)
                    local current = hl.get_config('cursor:zoom_factor') or 1.0
                    local z = current <= 1.01 and 1.0 or current
                    hl.config({{ cursor = {{ zoom_factor = z, zoom_detached_camera = {detached}, zoom_rigid = {rigid} }} }})
                    mode = 'undecided'
                end,
                ['end'] = function(ev)
                    local current = hl.get_config('cursor:zoom_factor') or 1.0
                    local z = current <= 1.01 and 1.0 or current
                    hl.config({{ cursor = {{ zoom_factor = z, zoom_detached_camera = {detached}, zoom_rigid = {rigid} }} }})
                    mode = 'undecided'
                end
            }}
        end

        hl.gesture({{ fingers = 2, direction = 'pinch', action = _G.reel_zoom_handler() }})
        hl.config({{ cursor = {{ zoom_factor = 1.0, zoom_detached_camera = true, zoom_rigid = false }} }})
    "#
    );
    if util::run_ok("hyprctl", &["eval", &lua]) {
        let state = if had_prior { MARKER_PRIOR } else { MARKER_OURS };
        let _ = std::fs::write(&marker, state);
    } else if had_prior {
        // Registration failed after the user's gesture came off; put it back.
        restore_config_gestures();
    }
}

/// Deactivate manual zoom: unsets touchpad pinch gestures and restores default cursor options.
pub fn disable_recording_zoom(paths: &Paths) {
    let marker = paths.runtime.join(ZOOM_MARKER);
    if marker.exists() {
        let contents = std::fs::read_to_string(&marker).unwrap_or_default();
        let _ = unset_pinch_gesture();
        let _ = util::run_ok("hyprctl", &["eval", "_G.reel_zoom_handler = nil"]);
        if marker_restores_prior(&contents) {
            restore_config_gestures();
        }
        let _ = std::fs::remove_file(&marker);
    }
    reset_compositor_zoom();
}

/// Smoothly animates compositor zoom factor to target value.
pub fn animate_compositor_zoom(target: f64, immediate: bool, follow: bool) -> f64 {
    let target = target.clamp(1.0, 4.0);
    let current = compositor_zoom();

    if (current - target).abs() < 0.01 {
        set_compositor_zoom(target, follow);
        return target;
    }

    if immediate {
        set_compositor_zoom(target, follow);
        return target;
    }

    // 6-step smooth ease-out micro-animation (~84ms total)
    let steps = 6;
    for i in 1..steps {
        let f = i as f64 / steps as f64;
        let t = 3.0 * f * f - 2.0 * f * f * f; // smoothstep
        let z = current + (target - current) * t;
        set_compositor_zoom(z, follow);
        std::thread::sleep(Duration::from_millis(14));
    }
    set_compositor_zoom(target, follow);
    target
}

/// Reset compositor zoom back to 1.0x (full frame) and restore default cursor options.
pub fn reset_compositor_zoom() -> bool {
    let current = compositor_zoom();
    // At 1x the camera is always detached, so the follow mode no longer matters.
    if (current - 1.0).abs() > 0.01 {
        animate_compositor_zoom(1.0, false, false);
    } else {
        set_compositor_zoom(1.0, false);
    }
    true
}

// ---------- unit tests --------------------------------------------------

#[cfg(test)]
mod tests {
    #[test]
    fn parse_float_zoom() {
        let text = "float: 1.500000\nset: true\n";
        let mut parsed = 1.0;
        for line in text.lines() {
            if let Some(rest) = line.strip_prefix("float:") {
                if let Ok(val) = rest.trim().parse::<f64>() {
                    parsed = val;
                }
            }
        }
        assert_eq!(parsed, 1.5);
    }

    #[test]
    fn zoom_clamp_bounds() {
        let low = 0.5f64.clamp(1.0, 4.0);
        assert_eq!(low, 1.0);
        let high = 5.5f64.clamp(1.0, 4.0);
        assert_eq!(high, 4.0);
        let normal = 2.2f64.clamp(1.0, 4.0);
        assert_eq!(normal, 2.2);
    }

    #[test]
    fn marker_marks_overridden_user_gesture() {
        assert!(super::marker_restores_prior("prior"));
        assert!(super::marker_restores_prior("prior\n"));
        assert!(!super::marker_restores_prior("1"));
        assert!(!super::marker_restores_prior(""));
    }

    #[test]
    fn the_camera_follows_the_pointer_only_while_zoomed() {
        let zoomed = super::camera_lua(2.0, true);
        assert!(zoomed.contains("zoom_detached_camera = false"));
        assert!(zoomed.contains("zoom_rigid = true"));
        let full = super::camera_lua(1.0, true);
        assert!(full.contains("zoom_detached_camera = true"));
        assert!(full.contains("zoom_rigid = false"));
    }

    #[test]
    fn window_and_area_captures_keep_the_camera_detached() {
        // A detached camera keeps the pointer at its real position, so it stays
        // inside a small window or area capture instead of drifting to the
        // screen centre.
        let zoomed = super::camera_lua(2.0, false);
        assert!(zoomed.contains("zoom_detached_camera = true"));
        assert!(zoomed.contains("zoom_rigid = false"));
    }

    #[test]
    fn pan_steps_stay_inside_the_recording() {
        let bounds = Some((0.0, 0.0, 200.0, 200.0));
        assert_eq!(
            super::pan_target(100.0, 100.0, -50.0, 0.0, bounds),
            (50.0, 100.0)
        );
        assert_eq!(
            super::pan_target(10.0, 10.0, -50.0, -50.0, bounds),
            (0.0, 0.0)
        );
        assert_eq!(
            super::pan_target(10.0, 10.0, 500.0, 500.0, bounds),
            (199.0, 199.0)
        );
        assert_eq!(super::pan_target(10.0, 10.0, 5.0, 5.0, None), (15.0, 15.0));
    }
}
