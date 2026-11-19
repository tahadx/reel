use crate::autozoom::{self, Spring};
use crate::paths::Paths;
use crate::util;
use std::fs;
use std::path::Path;
use std::time::Duration;

/// The live zoom factor mirrored to the runtime directory.
///
/// The compositor owns the actual zoom, including the pinch and wheel paths
/// that run entirely inside Hyprland's Lua config. The save pass still has to
/// place clicks in the recorded frame, which is the magnified picture, so every
/// zoom step writes the current factor here and telemetry reads it per event.
/// Callers that do not hold a `Paths` pass the already-resolved file.
pub fn read_recording_zoom(path: &Path) -> f64 {
    let text = fs::read_to_string(path).unwrap_or_default();
    text.trim()
        .parse::<f64>()
        .ok()
        .filter(|zoom| zoom.is_finite() && *zoom >= 1.0)
        .map(|zoom| zoom.clamp(1.0, 4.0))
        .unwrap_or(1.0)
}

/// Mirror the live zoom factor for telemetry. A temp file and rename keep a
/// reader from seeing a half-written number, and the caller never fails a zoom
/// because the mirror could not be written.
pub fn write_recording_zoom(paths: &Paths, zoom: f64) {
    let zoom = if zoom.is_finite() {
        zoom.clamp(1.0, 4.0)
    } else {
        1.0
    };
    let temp = paths.zoom_factor.with_extension("tmp");
    if fs::write(&temp, format!("{zoom:.4}")).is_ok() {
        let _ = fs::rename(&temp, &paths.zoom_factor);
    }
}

/// A parseable Lua string literal for a path.
fn lua_path(path: &Path) -> String {
    path.display()
        .to_string()
        .replace('\\', "\\\\")
        .replace('\'', "\\'")
}

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
/// monitor captures; see `camera_lua`. A successful step also mirrors the
/// factor for telemetry, so a click that follows it lands in the recorded crop.
pub fn set_compositor_zoom(paths: &Paths, zoom: f64, follow: bool) -> bool {
    let applied = util::run_ok("hyprctl", &["eval", &camera_lua(zoom, follow)]);
    if applied {
        write_recording_zoom(paths, zoom);
    }
    applied
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

/// Where a capture-local pointer position is drawn in a frame that already
/// contains the compositor's live zoom.
///
/// A monitor take runs the attached camera (`zoom_detached_camera = false`,
/// `zoom_rigid = true`): Hyprland places the scene at `z * (p - pointer) +
/// center` and clamps the box so no empty space enters the output. The pointer
/// is the click itself, so a click maps without needing any camera history.
/// Window and area takes run a detached camera whose position Reel does not
/// record; their points are left alone rather than guessed.
pub fn recorded_point(
    x: f64,
    y: f64,
    zoom: f64,
    width: f64,
    height: f64,
    attached: bool,
) -> (f64, f64) {
    if !attached
        || !(x.is_finite()
            && y.is_finite()
            && zoom.is_finite()
            && width.is_finite()
            && height.is_finite())
        || zoom <= 1.001
        || width <= 0.0
        || height <= 0.0
    {
        return (x, y);
    }
    let z = zoom.max(1.0);
    let axis = |p: f64, dim: f64| {
        // The scene box starts at center - z * pointer and is shifted back
        // inside the output when the magnified frame would expose an edge.
        let offset = (dim / 2.0 - z * p).clamp(dim * (1.0 - z), 0.0);
        (offset + z * p).clamp(0.0, dim)
    };
    (axis(x, width), axis(y, height))
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

/// The compositor side of recording zoom.
///
/// Wheel zoom runs inside the compositor rather than through a Hyprland keybind
/// for two reasons: Hyprland only forwards one wheel bind every 300 ms
/// (`binds:scroll_event_delay`), and a bind cannot tell how far the wheel moved.
/// A wheel event therefore nudges a target, and a short timer glides the camera
/// toward it, which is the same continuous motion the pinch handler produces.
/// The binds are registered only for the take; outside a take Ctrl+scroll keeps
/// its normal meaning in applications.
fn recording_zoom_lua(paths: &Paths, max_zoom: f64, follow: bool) -> String {
    let max_z = max_zoom.clamp(1.1, 4.0);
    let detached = if follow { "false" } else { "true" };
    let rigid = if follow { "true" } else { "false" };
    let mirror = lua_path(&paths.zoom_factor);
    format!(
        r#"
        -- Wheel zoom: a wheel event moves a target, and a 16 ms timer eases the
        -- camera toward it. This keeps Ctrl+scroll smooth like the pinch gesture
        -- instead of stepping or waiting for Hyprland's scroll bind delay.
        _G.reel_wheel_delay = hl.get_config('binds:scroll_event_delay')
        hl.config({{ binds = {{ scroll_event_delay = 0 }} }})

        -- The save pass runs after the take and needs the zoom that was live
        -- when each click happened. Mirror every applied factor for telemetry.
        -- Temp file plus rename keeps a reader from seeing half a number.
        _G.reel_zoom_mirror = function(z)
            local f = io.open('{mirror}.tmp', 'w')
            if f then
                f:write(string.format('%.4f', z))
                f:close()
                os.rename('{mirror}.tmp', '{mirror}')
            end
        end
        _G.reel_zoom_mirror(1.0)

        _G.reel_wheel_current = 1.0
        _G.reel_wheel_target = 1.0

        _G.reel_wheel_apply = function(z)
            hl.config({{ cursor = {{ zoom_factor = z, zoom_detached_camera = {detached}, zoom_rigid = {rigid} }} }})
            _G.reel_zoom_mirror(z)
        end

        _G.reel_wheel_tick = function()
            local delta = _G.reel_wheel_target - _G.reel_wheel_current
            if math.abs(delta) < 0.002 then
                _G.reel_wheel_current = _G.reel_wheel_target
                _G.reel_wheel_apply(_G.reel_wheel_current)
                if _G.reel_wheel_timer then
                    _G.reel_wheel_timer:set_enabled(false)
                end
                return
            end
            _G.reel_wheel_current = _G.reel_wheel_current + delta * 0.35
            _G.reel_wheel_apply(_G.reel_wheel_current)
        end

        _G.reel_wheel_zoom = function(direction)
            -- A keyboard zoom or a reset may have moved the camera since the
            -- last notch; start the glide from where the picture really is.
            local actual = hl.get_config('cursor:zoom_factor') or 1.0
            if math.abs(actual - _G.reel_wheel_current) > 0.02 then
                _G.reel_wheel_current = actual
                _G.reel_wheel_target = actual
            end
            _G.reel_wheel_target = math.max(1.0, math.min({max_z:.2}, _G.reel_wheel_target + direction * 0.15))
            if _G.reel_wheel_timer then
                _G.reel_wheel_timer:set_enabled(true)
            else
                _G.reel_wheel_timer = hl.timer(_G.reel_wheel_tick, {{ timeout = 16, type = 'repeat' }})
            end
            _G.reel_wheel_tick()
        end

        _G.reel_wheel_binds = {{
            hl.bind('CTRL + mouse_up', function() _G.reel_wheel_zoom(1) end, {{ description = 'Reel zoom in' }}),
            hl.bind('CTRL + mouse_down', function() _G.reel_wheel_zoom(-1) end, {{ description = 'Reel zoom out' }}),
        }}

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
                                _G.reel_zoom_mirror(z)
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
                    _G.reel_zoom_mirror(z)
                    mode = 'undecided'
                end,
                ['end'] = function(ev)
                    local current = hl.get_config('cursor:zoom_factor') or 1.0
                    local z = current <= 1.01 and 1.0 or current
                    hl.config({{ cursor = {{ zoom_factor = z, zoom_detached_camera = {detached}, zoom_rigid = {rigid} }} }})
                    _G.reel_zoom_mirror(z)
                    mode = 'undecided'
                end
            }}
        end

        hl.gesture({{ fingers = 2, direction = 'pinch', action = _G.reel_zoom_handler() }})
        hl.config({{ cursor = {{ zoom_factor = 1.0, zoom_detached_camera = true, zoom_rigid = false }} }})
        _G.reel_zoom_mirror(1.0)
    "#
    )
}

/// Remove the per-take wheel binds and restore the user's scroll delay.
fn clear_zoom_lua() -> &'static str {
    r#"
    if _G.reel_wheel_timer then
        _G.reel_wheel_timer:set_enabled(false)
        _G.reel_wheel_timer = nil
    end
    if _G.reel_wheel_binds then
        for _, bind in ipairs(_G.reel_wheel_binds) do
            if bind then bind:remove() end
        end
        _G.reel_wheel_binds = nil
    end
    if _G.reel_wheel_delay ~= nil then
        hl.config({ binds = { scroll_event_delay = _G.reel_wheel_delay } })
        _G.reel_wheel_delay = nil
    end
    _G.reel_wheel_zoom = nil
    _G.reel_wheel_tick = nil
    _G.reel_wheel_apply = nil
    _G.reel_wheel_current = nil
    _G.reel_wheel_target = nil
    _G.reel_zoom_handler = nil
    "#
}

/// Activate manual zoom for recording: enables live tracking, registers
/// touchpad pinch gestures, and binds Ctrl+scroll for smooth wheel zoom.
///
/// `follow` is true for monitor captures, where the camera tracks the pointer.
/// Window and area captures keep the camera detached so the pointer keeps its
/// real position instead of being pulled toward the screen centre.
pub fn enable_recording_zoom(paths: &Paths, max_zoom: f64, follow: bool) {
    let marker = paths.runtime.join(ZOOM_MARKER);

    // A marker from a take that crashed still points at Reel's gesture. Undo it
    // first so the probe below sees the user's own binding rather than ours.
    if marker.exists() {
        disable_recording_zoom(paths);
    }

    // Hyprland rejects a second gesture on the same fingers and direction, so a
    // user's pinch binding has to come off for the take. Remember that and put
    // it back when the take ends.
    let had_prior = unset_pinch_gesture();

    // Start the mirror at 1x before the Lua can apply a step, so a click that
    // races the take start still reads a sane factor.
    write_recording_zoom(paths, 1.0);
    let lua = recording_zoom_lua(paths, max_zoom, follow);
    if util::run_ok("hyprctl", &["eval", &lua]) {
        let state = if had_prior { MARKER_PRIOR } else { MARKER_OURS };
        let _ = std::fs::write(&marker, state);
    } else {
        // A failed eval may have registered the wheel binds before the error.
        // Clean up so the next take does not inherit half of the controller.
        let _ = util::run_ok("hyprctl", &["eval", clear_zoom_lua()]);
        if had_prior {
            // Registration failed after the user's gesture came off; put it back.
            restore_config_gestures();
        }
    }
}

/// Deactivate manual zoom: unsets touchpad pinch gestures and restores default cursor options.
pub fn disable_recording_zoom(paths: &Paths) {
    let marker = paths.runtime.join(ZOOM_MARKER);
    if marker.exists() {
        let contents = std::fs::read_to_string(&marker).unwrap_or_default();
        let _ = unset_pinch_gesture();
        let cleaned = util::run_ok("hyprctl", &["eval", clear_zoom_lua()]);
        if !cleaned {
            crate::paths::log(paths, "zoom: take cleanup failed");
        }
        if marker_restores_prior(&contents) {
            restore_config_gestures();
        }
        let _ = std::fs::remove_file(&marker);
    }
    write_recording_zoom(paths, 1.0);
    reset_compositor_zoom(paths);
}

/// Smoothly animates compositor zoom factor to target value.
pub fn animate_compositor_zoom(paths: &Paths, target: f64, immediate: bool, follow: bool) -> f64 {
    let target = target.clamp(1.0, 4.0);
    let current = compositor_zoom();

    if (current - target).abs() < 0.01 {
        set_compositor_zoom(paths, target, follow);
        return target;
    }

    if immediate {
        set_compositor_zoom(paths, target, follow);
        return target;
    }

    // Critically damped spring, not a fixed step count: the camera has one
    // velocity through the whole move, so a new key press during a zoom-out
    // changes only where it is heading. The loop is bounded so a compositor
    // that stops answering cannot hold the command forever.
    let half_life = autozoom::zoom_half_life(current, target);
    let mut spring = Spring::new(current, half_life);
    spring.retarget(target, half_life);
    let step = Duration::from_millis(14);
    let mut elapsed = Duration::ZERO;
    while !spring.settled(autozoom::SPRING_EPSILON) && elapsed < Duration::from_millis(1200) {
        spring.step(step.as_secs_f64());
        set_compositor_zoom(paths, spring.value(), follow);
        std::thread::sleep(step);
        elapsed += step;
    }
    set_compositor_zoom(paths, target, follow);
    target
}

/// Reset compositor zoom back to 1.0x (full frame) and restore default cursor options.
pub fn reset_compositor_zoom(paths: &Paths) -> bool {
    let current = compositor_zoom();
    // At 1x the camera is always detached, so the follow mode no longer matters.
    if (current - 1.0).abs() > 0.01 {
        animate_compositor_zoom(paths, 1.0, false, false);
    } else {
        set_compositor_zoom(paths, 1.0, false);
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

    #[test]
    fn a_click_follows_the_attached_zoom_camera() {
        // At 2x with room around the pointer, Hyprland centres the clicked
        // content, so the effect belongs at the centre of the recorded frame.
        assert_eq!(
            super::recorded_point(1440.0, 810.0, 2.0, 2880.0, 1620.0, true),
            (1440.0, 810.0)
        );
        // A click near the right edge cannot be centred: the camera clamps to
        // the output, exactly like Hyprland's zoom transform.
        assert_eq!(
            super::recorded_point(2644.0, 1010.0, 2.0, 2880.0, 1620.0, true),
            (2408.0, 810.0)
        );
        assert_eq!(
            super::recorded_point(1440.0, 1010.0, 2.0, 2880.0, 1620.0, true),
            (1440.0, 810.0)
        );
    }

    #[test]
    fn a_point_outside_the_zoom_camera_stays_inside_the_frame() {
        // The corner click with the zoom pushed to the cap: the clamp keeps
        // every recorded position within the encoded frame.
        let (x, y) = super::recorded_point(0.0, 0.0, 2.0, 2880.0, 1620.0, true);
        assert_eq!((x, y), (0.0, 0.0));
        let (x, y) = super::recorded_point(2879.0, 1619.0, 2.0, 2880.0, 1620.0, true);
        assert!((0.0..2880.0).contains(&x) && (0.0..1620.0).contains(&y));
    }

    #[test]
    fn a_plain_frame_or_a_detached_camera_leaves_the_point_alone() {
        let point = (120.0, 340.0);
        assert_eq!(
            super::recorded_point(point.0, point.1, 1.0, 2880.0, 1620.0, true),
            point
        );
        assert_eq!(
            super::recorded_point(point.0, point.1, 2.5, 2880.0, 1620.0, false),
            point
        );
        assert_eq!(
            super::recorded_point(point.0, point.1, f64::NAN, 2880.0, 1620.0, true),
            point
        );
    }

    #[test]
    fn the_zoom_mirror_round_trips_and_rejects_junk() {
        let root = std::env::temp_dir().join(format!(
            "reel-zoom-mirror-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        std::fs::create_dir_all(&root).unwrap();
        let paths = crate::paths::Paths::for_test(&root);
        assert_eq!(super::read_recording_zoom(&paths.zoom_factor), 1.0);
        super::write_recording_zoom(&paths, 2.5);
        assert_eq!(super::read_recording_zoom(&paths.zoom_factor), 2.5);
        // A partial mirror or a hand-edited junk value reads as 1x rather than
        // scrambling every click.
        std::fs::write(&paths.zoom_factor, "not a zoom").unwrap();
        assert_eq!(super::read_recording_zoom(&paths.zoom_factor), 1.0);
        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn wheel_zoom_is_bound_only_for_the_take() {
        let paths = crate::paths::Paths::for_test(std::path::Path::new("/tmp/reel-zoom-test"));
        let lua = super::recording_zoom_lua(&paths, 2.0, true);
        // Every wheel and pinch step mirrors the factor telemetry reads.
        assert!(lua.contains("_G.reel_zoom_mirror(1.0)"));
        assert!(lua.contains("_G.reel_zoom_mirror(z)"));
        assert!(lua.contains("zoom-factor"));
        // Plain Ctrl+scroll, no Alt, and a short ease instead of a keybind
        // dispatch so every wheel notch reaches the camera.
        assert!(lua.contains("hl.bind('CTRL + mouse_up'"));
        assert!(lua.contains("hl.bind('CTRL + mouse_down'"));
        assert!(lua.contains("scroll_event_delay = 0"));
        assert!(lua.contains("hl.timer(_G.reel_wheel_tick"));
        assert!(lua.contains("math.min(2.00"));
    }

    #[test]
    fn wheel_zoom_cleanup_removes_its_binds_and_restores_the_delay() {
        let cleanup = super::clear_zoom_lua();
        assert!(cleanup.contains("bind:remove()"));
        assert!(cleanup.contains("scroll_event_delay = _G.reel_wheel_delay"));
        assert!(cleanup.contains("_G.reel_zoom_handler = nil"));
    }
}
