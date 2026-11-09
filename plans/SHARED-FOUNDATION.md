# Shared cursor-data and post-processing foundation

**Status:** Approved for implementation on 2026-11-09. Not started. No GitHub
Issue; the approval record is `plans/FEATURES.md`.

This is the common design. Feature branches must consume this foundation rather
than inventing four cursor parsers or four ffmpeg graphs.

## 1. One pointer-data model

New shared module (name to be agreed): `crates/reel/src/pointer.rs`.

```rust
pub struct PointerSample {
    pub t: f64,          // raw telemetry time
    pub x: f64,          // capture pixels
    pub y: f64,
    pub kind: SampleKind, // Move | Sync | Button
    pub button: Option<Button>,
    pub state: Option<PressState>,
    pub source: SampleSource, // Evdev | Compositor | Tap
}

pub struct ClickEvent {
    pub t: f64,          // saved-take seconds
    pub x: f64,
    pub y: f64,
    pub button: Button,
    pub kind: ClickKind, // Click | Tap | Drag | Hold
    pub duration: f64,
    pub count: u8,
}

pub struct TimeMap {
    pub trim_start: f64,
    pub video_start: f64,
    pub pauses: Vec<PauseSpan>,
}

pub struct PointerTrack { /* sorted samples, segments, width, height */ }

impl PointerTrack {
    pub fn position_at(&self, t: f64) -> Option<(f64, f64)>;
    pub fn velocity_at(&self, t: f64) -> Option<(f64, f64)>;
}
```

Rules:

- Raw telemetry stays authoritative; smoothing/blur/appearance are derived.
- Malformed lines never fail a save.
- Interpolate only inside a segment; hold or hide across gaps/teleports.
- No feature parses telemetry independently.

## 2. One clock

`project::read_telemetry` becomes the rich reader. It keeps the existing
`clicks` vector for auto zoom, and adds samples, buttons, pauses, pointer mode,
capture facts, and the video-start mark.

Additive telemetry records:

- `start.pointer`: `baked | hidden | none`.
- `video`: first captured-frame mark on the monotonic clock.
- `pause` / `resume`: a separate append-only runtime marks file is acceptable
  because the pause command is a different process from the listener.

`TimeMap::saved(raw)` subtracts `video_start`, `trim_start`, and paused spans.
The same map feeds auto zoom, cursor, blur, and click effects.

## 3. One coordinate transform

```text
view_w = W_s / zoom
view_h = H_s / zoom
left   = clamp(cx - view_w / 2, 0, W_s - view_w)
top    = clamp(cy - view_h / 2, 0, H_s - view_h)
out_x  = (src_x - left) * zoom * W_o / W_s
out_y  = (src_y - top ) * zoom * H_o / H_s
```

Use the same camera sample as the renderer. For the `sendcmd` fallback, use its
even-rounded integer viewport. Default to a screen-sized cursor after the
camera; keep a project option for scene-space scaling if needed.

## 4. Project schema (additive v1)

```json
"cursor": {
  "enabled": true,
  "capture": "hidden",
  "style": "synthetic",
  "shape": "arrow",
  "size": "medium",
  "color": "auto",
  "outline": "auto",
  "shadow": "soft",
  "opacity": 1.0,
  "hideIdle": false,
  "smooth": true,
  "smoothing": "balanced",
  "scaleWithZoom": false,
  "motionBlur": {
    "enabled": true, "intensity": "medium", "shutter": 0.5,
    "maxSmear": 32.0, "decay": 0.18, "samples": 8, "deadband": 60.0
  },
  "cuts": [4.1, 7.3]
},
"ripples": {
  "enabled": true, "style": "ring", "durationMs": 320,
  "size": "medium", "intensity": "normal", "buttons": "both",
  "events": [ { "t": 1.234, "x": 100.5, "y": 200.25, "button": "left", "kind": "click", "count": 1 } ]
}
```

- `capture` is the authoritative fact: `baked | hidden | none`. Never synthesize
  over `baked`.
- Old projects default to `baked`; unknown fields are preserved.
- `plate` and `rendered` fields may be added for non-destructive re-render.

## 5. Capture contract

- `cursor=on`, `cursorStyle=baked`: exactly today. `-cursor yes`, telemetry on.
- `cursor=on`, synthetic requested, telemetry listener gated live:
  `-cursor no`, force hardware cursor, telemetry on; save pass draws the cursor.
- Synthetic must never strip the pointer before the listener is proven live.
  If telemetry fails, fall back to baked and record `capture: "baked"`.
- `cursor=off`: unchanged, no pointer and no telemetry.
- Manual live zoom: v1 either disables live zoom for synthetic takes or records
  the live viewport. Recommend disable/fallback for v1 and document it.

## 6. Render composition

One `RenderPlan` / `EffectGraph`, one ffmpeg pass. Keep the current `-vf` path
byte-for-byte when no effect is enabled.

Order: camera -> click effects -> synthetic cursor (with blur) -> future
webcam/canvas.

Backends:

- Built-in cursor shapes and click effects: generated ASS/libass vector events.
- Custom cursor PNG: second input + `overlay` + `sendcmd`.
- Motion blur: prefer reusing the cursor layer; if ghost ASS events are too
  heavy for long takes, add a raw RGBA tile overlay behind the same plan. A
  benchmark spike decides this; do not build a separate pipeline.

`finalize` must render when any effect is enabled, not only when the camera
zooms. A failed effect must never lose the take; fall back to trim/loudness and
log it. Do not write a project that claims an effect the saved file lacks.

## 7. Non-destructive re-render

Current state: raw capture deleted, `reel render` re-applies camera to the saved
take. That cannot support restyling after save. Approved: retain a pointer-free
plate for synthetic takes, store `plate` in the project, and render from it.
Storage roughly doubles synthetic takes. If the plate is ever unavailable,
document restyling as a save-time choice, not a re-render guarantee.

## 8. UI and settings

The existing **Visuals** tab keeps Manual zoom and Auto zoom at the top and is
the home for pointer capture and all pointer appearance/effect rows:

- Manual zoom
- Auto zoom
- Record the pointer
- Pointer style (Recorded / Arrow / Minimal / Hand / Dot / Ring / Custom)
- Pointer smoothing (Off / Precise / Balanced / Cinematic)
- Pointer motion blur (Off / Low / Medium / High)
- Click effects (Off / On) + style/duration/size/intensity/buttons
- Pointer size, color, outline, shadow, hide when idle

The **Recording** tab contains Audio, Video, Capture, and Saving sections.
**Output** is folded into Recording → Saving: saved notification, filename
prefix, and the recordings path. The tabs are Recording, Visuals, and
Shortcuts, plus History; `1–3` and `Shift+H` are the shortcuts. `Record the
pointer` lives in Visuals. Defaults preserve today's output: baked pointer, all
effects off. All keys must exist in `config.rs`, `manifest.json`, and `Panel.qml`
in the same change.

## 9. Verification

Every feature must pass the repo's full bar: fmt, clippy, workspace tests,
`bin/refresh-dist`, QML lint/tests, installer test, release check, plugin
validation. Tests stay offscreen with `REEL_NO_NOTIFY=1`. Add deterministic
fixtures for pointer paths, clicks, trim, pauses, 30/60/120 fps, 1080p/4K,
edges, missing telemetry, and disabled effects.
