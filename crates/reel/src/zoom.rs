use crate::autozoom::{self, Spring};
use crate::paths::Paths;
use crate::util;
use std::fs;
use std::path::Path;
use std::time::Duration;

/// The live zoom factor mirrored to the runtime directory.
///
/// The compositor owns the actual zoom, including the pinch and wheel paths
/// that run entirely inside Hyprland's Lua config. Every applied step writes
/// the current factor here, and telemetry stamps each event with it so a saved
/// take keeps an accurate record of the live depth. Callers that do not hold a
/// `Paths` pass the already-resolved file.
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
/// A monitor take uses Hyprland's attached rigid camera: the point under the
/// pointer is pinned to the middle of the crop, so the thing being zoomed into
/// always has the whole frame around it. A loose camera would leave that point
/// under the cursor instead, which pushes it against the edge of the crop as
/// soon as the cursor nears the screen edge or the view is panned.
/// `zoomFollow=push` opts into the detached camera, where the frame holds still
/// and only pans when the pointer reaches the edge. A window or area capture
/// keeps the camera detached either way: the pointer holds its real position,
/// which keeps it inside a small capture. At 1x the camera is detached, the
/// normal desktop state.
fn camera_lua(zoom: f64, follow: bool, push: bool) -> String {
    let z = zoom.clamp(1.0, 4.0);
    let zoomed = z > 1.01;
    let attached = follow && !push && zoomed;
    format!(
        "hl.config({{ cursor = {{ zoom_factor = {z:.3}, zoom_detached_camera = {}, zoom_rigid = {} }} }})",
        !attached,
        attached,
    )
}

/// Sets live compositor zoom factor in Hyprland. `follow` is true only for
/// monitor captures, and `push` selects the stable detached camera for a
/// monitor take; see `camera_lua`. A successful step also mirrors the factor
/// for telemetry.
pub fn set_compositor_zoom(paths: &Paths, zoom: f64, follow: bool, push: bool) -> bool {
    let applied = util::run_ok("hyprctl", &["eval", &camera_lua(zoom, follow, push)]);
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
/// A monitor take runs the attached rigid camera (`zoom_detached_camera =
/// false`, `zoom_rigid = true`): Hyprland places the scene at `z * (p -
/// pointer) + center` and clamps the box so no empty space enters the output.
/// The pointer is the click itself, so a click maps without needing any camera
/// history. Window and area takes run a detached camera whose position Reel
/// does not record; their points are left alone rather than guessed.
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
/// The rigid camera keeps the point under the pointer centred in the crop, so
/// moving the pointer pans by the same amount at every depth; no gain is
/// needed, and the subject cannot drift into the edge of the frame.
pub fn pan_view(dx: f64, dy: f64, bounds: Option<(f64, f64, f64, f64)>) -> bool {
    let Some((x, y)) = cursor_position() else {
        return false;
    };
    let (next_x, next_y) = pan_target(x, y, dx, dy, bounds);
    util::run_ok("hyprctl", &["eval", &pan_lua(next_x, next_y)])
}

fn pan_lua(x: f64, y: f64) -> String {
    format!(
        "hl.config({{ cursor = {{ zoom_detached_camera = false, zoom_rigid = true }} }})\nhl.dispatch(hl.dsp.cursor.move({{ x = {x:.1}, y = {y:.1} }}))"
    )
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
/// Every manual input (wheel, keyboard, pinch, reset) moves one target, and a
/// 16 ms timer eases the applied factor toward it. Doing the easing inside the
/// compositor keeps the motion at frame rate and avoids spawning `hyprctl` for
/// every step. Wheel zoom also has to run here for two reasons: Hyprland only
/// forwards one wheel bind every 300 ms (`binds:scroll_event_delay`), and a bind
/// cannot tell how far the wheel moved. The binds are registered only for the
/// take; outside a take Ctrl+scroll keeps its normal meaning in applications.
fn recording_zoom_lua(paths: &Paths, max_zoom: f64, follow: bool, push: bool) -> String {
    let max_z = max_zoom.clamp(1.1, 4.0);
    let mirror = lua_path(&paths.zoom_factor);
    format!(
        r#"
        _G.reel_zoom_delay = hl.get_config('binds:scroll_event_delay')
        hl.config({{ binds = {{ scroll_event_delay = 0 }} }})

        local max_z = {max_z:.2}
        local follow = {follow}
        local push = {push}
        -- Critically damped springs. A half-life is the time to cover half the
        -- remaining distance under the spring's acceleration; the whole move
        -- reads as a slow ease-in/ease-out instead of a step with a fade-out.
        -- Pinch is short so it stays under the fingers; reset is long because
        -- it is a deliberate return to the full frame.
        local wheel_half_life = 0.11
        local key_half_life = 0.15
        local pinch_half_life = 0.06
        local reset_half_life = 0.16
        local spring_dt = 0.016

        -- Telemetry stamps every event with the zoom that was live when it
        -- happened. Mirror each applied factor; temp file plus rename keeps a
        -- reader from seeing half a number.
        _G.reel_zoom_mirror = function(z)
            local f = io.open('{mirror}.tmp', 'w')
            if f then
                f:write(string.format('%.4f', z))
                f:close()
                os.rename('{mirror}.tmp', '{mirror}')
            end
        end

        _G.reel_zoom_current = 1.0
        _G.reel_zoom_target = 1.0
        _G.reel_zoom_velocity = 0.0
        _G.reel_zoom_half_life = wheel_half_life

        _G.reel_zoom_stop = function()
            if _G.reel_zoom_timer then
                _G.reel_zoom_timer:set_enabled(false)
            end
        end

        _G.reel_zoom_apply = function(z)
            _G.reel_zoom_current = z
            -- Follow is Hyprland's attached rigid camera: the point under the
            -- pointer is pinned to the middle of the crop, so the subject has
            -- the whole frame around it instead of sitting on the edge. Push is
            -- the detached camera that holds still and pans only at the edge.
            -- At 1x and for window/area takes the camera is detached.
            local attached = follow and not push and z > 1.01
            hl.config({{ cursor = {{ zoom_factor = z, zoom_detached_camera = not attached, zoom_rigid = attached }} }})
            _G.reel_zoom_mirror(z)
        end

        _G.reel_zoom_tick = function()
            -- Exact critically damped step. Retargeting keeps the velocity, so
            -- a new input changes only where the camera is heading; it never
            -- restarts the motion or zeroes what is already in flight.
            local omega = 0.6931471805599453 / _G.reel_zoom_half_life
            local error = _G.reel_zoom_current - _G.reel_zoom_target
            local velocity = _G.reel_zoom_velocity + omega * error
            local decay = math.exp(-omega * spring_dt)
            local next_value = _G.reel_zoom_target + (error + velocity * spring_dt) * decay
            local next_velocity = (velocity - omega * (error + velocity * spring_dt)) * decay

            if next_value < 1.0 then
                next_value = 1.0
                next_velocity = 0.0
            elseif next_value > max_z then
                next_value = max_z
                next_velocity = 0.0
            end

            _G.reel_zoom_velocity = next_velocity
            _G.reel_zoom_apply(next_value)

            if math.abs(next_value - _G.reel_zoom_target) < 0.001
                and math.abs(next_velocity) < 0.02 then
                _G.reel_zoom_velocity = 0.0
                _G.reel_zoom_apply(_G.reel_zoom_target)
                _G.reel_zoom_stop()
            end
        end

        -- A keyboard step, a reset, or a user's own zoom bind may have moved
        -- the camera since the last tick. Start from where the picture is.
        _G.reel_zoom_sync = function()
            local actual = hl.get_config('cursor:zoom_factor') or _G.reel_zoom_current
            if math.abs(actual - _G.reel_zoom_current) > 0.02 then
                _G.reel_zoom_current = actual
                _G.reel_zoom_target = actual
                _G.reel_zoom_velocity = 0.0
            end
        end

        _G.reel_zoom_start = function()
            if _G.reel_zoom_timer then
                _G.reel_zoom_timer:set_enabled(true)
            else
                _G.reel_zoom_timer = hl.timer(_G.reel_zoom_tick, {{ timeout = 16, type = 'repeat' }})
            end
            _G.reel_zoom_tick()
        end

        _G.reel_zoom_to = function(target, half_life)
            _G.reel_zoom_sync()
            target = math.max(1.0, math.min(max_z, target))
            -- Keep momentum when the new goal is in the same direction, but
            -- stop cleanly on a reversal instead of letting the old velocity
            -- carry the camera past the new goal.
            local old_error = _G.reel_zoom_target - _G.reel_zoom_current
            local new_error = target - _G.reel_zoom_current
            if old_error * new_error < 0 then
                _G.reel_zoom_velocity = 0.0
            end
            _G.reel_zoom_half_life = half_life or wheel_half_life
            _G.reel_zoom_target = target
            _G.reel_zoom_start()
        end

        _G.reel_zoom_step = function(delta, half_life)
            _G.reel_zoom_sync()
            -- Step from the target, not the applied factor, so rapid notches
            -- accumulate instead of each one restarting from the same place.
            _G.reel_zoom_to(_G.reel_zoom_target + delta, half_life)
        end

        _G.reel_zoom_key_step = function(delta)
            _G.reel_zoom_step(delta, key_half_life)
        end

        _G.reel_zoom_toggle = function(factor)
            _G.reel_zoom_sync()
            local goal = 1.0
            if _G.reel_zoom_target <= 1.05 then
                goal = math.max(1.1, math.min(max_z, factor or max_z))
            end
            _G.reel_zoom_to(goal, reset_half_life)
        end

        _G.reel_zoom_settle = function()
            _G.reel_zoom_sync()
            local goal = _G.reel_zoom_target
            if goal <= 1.01 then
                goal = 1.0
            end
            _G.reel_zoom_to(goal, wheel_half_life)
        end

        _G.reel_wheel_zoom = function(direction)
            _G.reel_zoom_step(direction * 0.15)
        end

        _G.reel_wheel_binds = {{
            hl.bind('CTRL + mouse_up', function() _G.reel_wheel_zoom(1) end, {{ description = 'Reel zoom in' }}),
            hl.bind('CTRL + mouse_down', function() _G.reel_wheel_zoom(-1) end, {{ description = 'Reel zoom out' }}),
        }}

        _G.reel_zoom_handler = function()
            local start_z = 1.0
            local mode = 'zoom'
            local accum_dx = 0
            local accum_dy = 0

            return {{
                start = function(ev)
                    _G.reel_zoom_sync()
                    start_z = _G.reel_zoom_current
                    -- The push camera is detached, so a pinch only zooms. Pan
                    -- is part of the opt-in follow camera.
                    mode = (start_z <= 1.02 or not follow or push) and 'zoom' or 'undecided'
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
                            local z = math.max(1.0, math.min(max_z, start_z * scale))
                            if math.abs(z - _G.reel_zoom_target) >= 0.002 then
                                _G.reel_zoom_to(z, pinch_half_life)
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
                    _G.reel_zoom_settle()
                    mode = 'undecided'
                end,
                ['end'] = function(ev)
                    _G.reel_zoom_settle()
                    mode = 'undecided'
                end
            }}
        end

        hl.gesture({{ fingers = 2, direction = 'pinch', action = _G.reel_zoom_handler() }})
        _G.reel_zoom_apply(1.0)
    "#
    )
}

/// Remove the per-take wheel binds and restore the user's scroll delay.
fn clear_zoom_lua() -> &'static str {
    r#"
    if _G.reel_zoom_timer then
        _G.reel_zoom_timer:set_enabled(false)
        _G.reel_zoom_timer = nil
    end
    if _G.reel_wheel_binds then
        for _, bind in ipairs(_G.reel_wheel_binds) do
            if bind then bind:remove() end
        end
        _G.reel_wheel_binds = nil
    end
    if _G.reel_zoom_delay ~= nil then
        hl.config({ binds = { scroll_event_delay = _G.reel_zoom_delay } })
        _G.reel_zoom_delay = nil
    end
    _G.reel_wheel_zoom = nil
    _G.reel_zoom_handler = nil
    _G.reel_zoom_mirror = nil
    _G.reel_zoom_current = nil
    _G.reel_zoom_target = nil
    _G.reel_zoom_velocity = nil
    _G.reel_zoom_half_life = nil
    _G.reel_zoom_stop = nil
    _G.reel_zoom_apply = nil
    _G.reel_zoom_tick = nil
    _G.reel_zoom_sync = nil
    _G.reel_zoom_start = nil
    _G.reel_zoom_to = nil
    _G.reel_zoom_step = nil
    _G.reel_zoom_key_step = nil
    _G.reel_zoom_toggle = nil
    _G.reel_zoom_settle = nil
    "#
}

/// Activate manual zoom for recording: enables live tracking, registers
/// touchpad pinch gestures, and binds Ctrl+scroll for smooth wheel zoom.
///
/// `follow` is true for monitor captures. A monitor take uses the stable
/// detached push camera unless `push` is false, in which case it uses the
/// attached rigid camera that keeps the subject centred. Window and area
/// captures keep the camera detached
/// so the pointer keeps its real position instead of being pulled toward the
/// screen centre.
pub fn enable_recording_zoom(paths: &Paths, max_zoom: f64, follow: bool, push: bool) {
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
    let lua = recording_zoom_lua(paths, max_zoom, follow, push);
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

/// Whether the per-take compositor controller is installed. The marker is
/// written only after its Lua has loaded, so a command that runs during a take
/// can choose the in-compositor glide instead of the Rust fallback.
pub fn recording_zoom_installed(paths: &Paths) -> bool {
    paths.runtime.join(ZOOM_MARKER).exists()
}

/// Ask the installed compositor controller to ease toward `target`. The glide
/// runs on the compositor's 16 ms timer, so the command returns immediately
/// instead of blocking on one `hyprctl` per frame.
///
/// Returns false when no take controller is installed; the caller then uses
/// the Rust fallback.
pub fn glide_compositor_zoom(paths: &Paths, target: f64, slow: bool) -> bool {
    if !recording_zoom_installed(paths) || !target.is_finite() {
        return false;
    }
    let target = target.clamp(1.0, 4.0);
    let half_life = if slow { "0.16" } else { "0.11" };
    let lua = format!("if _G.reel_zoom_to then _G.reel_zoom_to({target:.3}, {half_life}) end");
    util::run_ok("hyprctl", &["eval", &lua])
}

/// Add one step to the controller's target, so rapid key presses or wheel
/// notches accumulate instead of each restarting from the applied factor.
pub fn step_compositor_zoom(paths: &Paths, delta: f64) -> bool {
    if !recording_zoom_installed(paths) || !delta.is_finite() || delta.abs() < 1e-9 {
        return false;
    }
    // Key steps are deliberate, so they ease a little more slowly than a
    // wheel notch. The target still accumulates under key repeat.
    let lua = format!("if _G.reel_zoom_key_step then _G.reel_zoom_key_step({delta:.3}) end");
    util::run_ok("hyprctl", &["eval", &lua])
}

/// Toggle between 1x and `factor` (or the take's configured depth when no
/// factor is given), using the controller's target rather than a half-applied
/// glide.
pub fn toggle_compositor_zoom(paths: &Paths, factor: Option<f64>) -> bool {
    if !recording_zoom_installed(paths) {
        return false;
    }
    let factor = factor
        .filter(|value| value.is_finite() && *value > 1.05)
        .map(|value| format!("{:.3}", value.clamp(1.1, 4.0)))
        .unwrap_or_else(|| "nil".to_string());
    let lua = format!("if _G.reel_zoom_toggle then _G.reel_zoom_toggle({factor}) end");
    util::run_ok("hyprctl", &["eval", &lua])
}

/// Smoothly animates compositor zoom factor to target value.
///
/// This is the fallback used when the per-take compositor controller is not
/// installed, for example during cleanup after the take has stopped.
pub fn animate_compositor_zoom(
    paths: &Paths,
    target: f64,
    immediate: bool,
    follow: bool,
    push: bool,
) -> f64 {
    let target = target.clamp(1.0, 4.0);
    let current = compositor_zoom();

    if (current - target).abs() < 0.01 {
        set_compositor_zoom(paths, target, follow, push);
        return target;
    }

    if immediate {
        set_compositor_zoom(paths, target, follow, push);
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
        set_compositor_zoom(paths, spring.value(), follow, push);
        std::thread::sleep(step);
        elapsed += step;
    }
    set_compositor_zoom(paths, target, follow, push);
    target
}

/// Reset compositor zoom back to 1.0x (full frame) and restore default cursor options.
pub fn reset_compositor_zoom(paths: &Paths) -> bool {
    // During a take the controller owns the camera: let it ease home instead of
    // starting a second animation from the CLI.
    if glide_compositor_zoom(paths, 1.0, true) {
        return true;
    }
    let current = compositor_zoom();
    // At 1x the camera is always detached, so the follow mode no longer matters.
    if (current - 1.0).abs() > 0.01 {
        animate_compositor_zoom(paths, 1.0, false, false, false);
    } else {
        set_compositor_zoom(paths, 1.0, false, false);
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
    fn monitor_zoom_pins_the_subject_to_the_middle_of_the_crop() {
        let zoomed = super::camera_lua(2.0, true, false);
        assert!(zoomed.contains("zoom_detached_camera = false"));
        // The rigid camera pins the point under the pointer to the middle of
        // the crop. A loose camera would leave it under the cursor and push it
        // into the edge of the frame as the cursor moved.
        assert!(zoomed.contains("zoom_rigid = true"));
        let full = super::camera_lua(1.0, true, false);
        assert!(full.contains("zoom_detached_camera = true"));
        assert!(full.contains("zoom_rigid = false"));
    }

    #[test]
    fn monitor_zoom_can_opt_into_the_stable_push_camera() {
        let zoomed = super::camera_lua(2.0, true, true);
        assert!(zoomed.contains("zoom_detached_camera = true"));
        assert!(zoomed.contains("zoom_rigid = false"));
    }

    #[test]
    fn window_and_area_captures_keep_the_camera_detached() {
        // A detached camera keeps the pointer at its real position, so it stays
        // inside a small window or area capture instead of drifting to the
        // screen centre.
        let zoomed = super::camera_lua(2.0, false, false);
        assert!(zoomed.contains("zoom_detached_camera = true"));
        assert!(zoomed.contains("zoom_rigid = false"));
    }

    #[test]
    fn panning_keeps_the_camera_rigid_so_the_subject_does_not_reach_the_edge() {
        // A pan moves the pointer that the rigid camera centres on, so every
        // step keeps the subject in the middle of the crop. The old loose
        // camera needed a >1 gain and let the subject drift into the edge.
        let lua = super::pan_lua(120.5, 80.0);
        assert!(lua.contains("zoom_rigid = true"));
        assert!(lua.contains("zoom_detached_camera = false"));
        assert!(lua.contains("hl.dsp.cursor.move({ x = 120.5, y = 80.0 })"));
        assert!(!lua.contains('*'));
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
    fn the_controller_helpers_do_not_touch_the_compositor_without_a_take() {
        let root = std::env::temp_dir().join(format!(
            "reel-zoom-controller-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        std::fs::create_dir_all(&root).unwrap();
        let paths = crate::paths::Paths::for_test(&root);
        assert!(!super::recording_zoom_installed(&paths));
        assert!(!super::step_compositor_zoom(&paths, 0.25));
        assert!(!super::toggle_compositor_zoom(&paths, None));
        assert!(!super::glide_compositor_zoom(&paths, 1.0, false));
        // A malformed target is refused before the marker is even consulted.
        std::fs::write(root.join(super::ZOOM_MARKER), super::MARKER_OURS).unwrap();
        assert!(super::recording_zoom_installed(&paths));
        assert!(!super::glide_compositor_zoom(&paths, f64::NAN, false));
        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn wheel_zoom_is_bound_only_for_the_take() {
        let paths = crate::paths::Paths::for_test(std::path::Path::new("/tmp/reel-zoom-test"));
        let lua = super::recording_zoom_lua(&paths, 2.0, true, false);
        // Every applied step mirrors the factor telemetry reads.
        assert!(lua.contains("_G.reel_zoom_mirror(z)"));
        assert!(lua.contains("zoom-factor"));
        // Plain Ctrl+scroll, no Alt, and a spring instead of a keybind dispatch
        // so every wheel notch reaches the camera.
        assert!(lua.contains("hl.bind('CTRL + mouse_up'"));
        assert!(lua.contains("hl.bind('CTRL + mouse_down'"));
        assert!(lua.contains("scroll_event_delay = 0"));
        assert!(lua.contains("hl.timer(_G.reel_zoom_tick"));
        assert!(lua.contains("_G.reel_zoom_apply(1.0)"));
        assert!(lua.contains("local follow = true"));
        assert!(lua.contains("local push = false"));
        assert!(lua.contains("local wheel_half_life = 0.11"));
        assert!(lua.contains("local key_half_life = 0.15"));
        assert!(lua.contains("_G.reel_zoom_velocity"));
        assert!(lua.contains("_G.reel_zoom_key_step"));
        assert!(lua.contains("zoom_rigid = attached"));

        let push = super::recording_zoom_lua(&paths, 2.0, true, true);
        assert!(push.contains("local push = true"));
    }

    #[test]
    fn wheel_zoom_cleanup_removes_its_binds_and_restores_the_delay() {
        let cleanup = super::clear_zoom_lua();
        assert!(cleanup.contains("bind:remove()"));
        assert!(cleanup.contains("scroll_event_delay = _G.reel_zoom_delay"));
        assert!(cleanup.contains("_G.reel_zoom_handler = nil"));
        assert!(cleanup.contains("_G.reel_zoom_key_step = nil"));
        assert!(cleanup.contains("_G.reel_zoom_velocity = nil"));
        assert!(cleanup.contains("_G.reel_zoom_half_life = nil"));
        assert!(cleanup.contains("_G.reel_zoom_timer = nil"));
    }
}
