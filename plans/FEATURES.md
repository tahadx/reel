# Approved cursor features

**Approved:** 2026-11-09
**Implementation status:** Approved for implementation; **not started.**
**Issue policy:** No GitHub Issues. This file is the approval record and feature
register. Do not open an issue for these features.
**Scope:** All four features below are approved together, but each still has its
own branch, worktree, verification gate, and merge decision.

Read `plans/SHARED-FOUNDATION.md` before touching cursor, telemetry, project,
render, capture, or settings code. Do not start implementation until the
orchestrator gives the kickoff.

## Feature register

| Feature | Branch | Worktree | Proposal | Status |
| --- | --- | --- | --- | --- |
| Shared cursor foundation | `feat/cursor-foundation` (to be created) | new worktree | `plans/SHARED-FOUNDATION.md` | Approved, not started |
| Smooth Cursor | `feat/smooth-cursor` | `/home/taha/Projects/oss/reel-smooth-cursor` | `plans/proposals/smooth-cursor.md` | Approved, not started |
| Cursor Motion Blur | `feat/motion-blur` | `/home/taha/Projects/oss/reel-motion-blur` | `plans/proposals/cursor-motion-blur.md` | Approved, not started |
| Click Effects | `feat/click-effects` | `/home/taha/Projects/oss/reel-click-effects` | `plans/proposals/click-effects.md` | Approved, not started |
| Editable Cursor Styles | `feat/cursor-styles` | `/home/taha/Projects/oss/reel-cursor-styles` | `plans/proposals/cursor-styles.md` | Approved, not started |

The shared foundation is a coordination branch, not a fifth user-facing feature.
It lands first so the feature branches do not invent competing telemetry
parsers, project schemas, camera transforms, or ffmpeg graphs.

## Shared decisions recorded at approval

These are the recommended defaults from the proposal review. Change them here
before implementation if they are wrong.

- **Pointer-free plate:** keep one for synthetic takes so style, smoothing, and
  blur can be changed after saving. The project stores the plate path; renders
  use the plate and never double-apply effects.
- **Manual live zoom:** disable it for synthetic-cursor takes in v1, or fall
  back to the baked pointer. Recording the live viewport is a later job.
- **Custom cursor assets:** PNG only, selected through config/CLI. No SVG, no
  native file picker in v1.
- **Defaults:** `cursorStyle=baked`, all new effects off. Today’s output is
  unchanged unless the user opts in.
- **UI:** the existing tabs are Recording, Visuals, and Shortcuts. Pointer
  capture and pointer effects live in Visuals. No fifth tab.

## 1. Shared cursor foundation

**Goal:** one pointer-data and post-processing base for every cursor feature.

**Scope:**
- Rich telemetry reader: preserve `move`, `sync`, `button` down/up, button
  identity, tap/physical source, capture facts.
- `TimeMap`: `trimStart`, video-start mark, pause/resume cuts, saved-take time.
- `PointerTrack`: ordered samples, segment splitting, `position_at`,
  `velocity_at`; raw telemetry stays authoritative.
- Project schema: additive `cursor`/`ripples` fields, unknown fields preserved.
- Synthetic capture contract: strip the pointer only after a gated telemetry
  listener is live; force the hardware cursor; fall back to baked on failure.
- `RenderPlan`: one ffmpeg pass, old `-vf` path unchanged when nothing is on.
  Order: camera → click effects → cursor/blur → future webcam/canvas.
- Plate retention and re-render guard.
- Shared tests and fixtures for time, coordinates, trim, pauses, and missing
  telemetry.

**Acceptance:** existing tests pass; a baked take renders exactly as today; a
synthetic take can be rendered twice from the same project and telemetry without
double-applying an effect; no new dependency.

## 2. Smooth Cursor

**Goal:** reconstruct a natural pointer path from raw telemetry without changing
the recording data.

**Scope:**
- Synthetic pointer mode: `cursorStyle=baked|synthetic`.
- Smoothing presets: `off`, `precise`, `balanced`, `cinematic`.
- PCHIP reconstruction plus symmetric speed-adaptive EMA; no systematic lag.
- Click anchors are hard constraints; smoothing relaxes around clicks.
- Frame-time sampling on the saved-take clock; cursor composited after the
  camera at a constant screen size.
- Disable switch is `cursorSmoothing=off`.

**Acceptance:** raw telemetry is byte-identical after save; click positions are
exact on the nearest frame; stationary jitter is reduced without overshoot;
30/60/120 fps outputs agree at common timestamps; baked mode is unchanged.

## 3. Cursor Motion Blur

**Goal:** velocity/direction-aware cursor blur that stays sharp at rest.

**Scope:**
- Opt-in `cursorMotionBlur=off|low|medium|high`.
- Synthetic pointer required; baked takes are not blurred.
- Shutter-based exposure, exponential decay, output-pixel smear cap, stationary
  deadband, segment-safe sampling.
- Shared cursor layer; if ASS ghost events are too heavy for long takes, add a
  raw RGBA tile behind the same `RenderPlan`.
- Composes with smooth cursor: blur consumes the smoothed track.
- Re-render reads resolved project values, not live settings.

**Acceptance:** stationary cursor is sharp; fast moves blur in the direction of
travel without screen-wide smear; 30/60/120 fps are stable; auto zoom and blur
compose; disabled mode is a no-op.

## 4. Click Effects

**Goal:** restrained click feedback at the exact click location.

**Scope:**
- Preserve left/right button down/up, tap/physical source, drags, and pause
  markers in telemetry.
- Normalize `ClickEvent`s in saved-take time; store them in `project.ripples`.
- Ring, pulse, and highlight styles; left/right differentiation; duration,
  size, intensity, button filter.
- Generated ASS vector events after the camera; optional PNG/custom layers later.
- Independently configurable and disableable; works over a baked pointer.
- No screen-pixel click detection.

**Acceptance:** left/right identity is correct; click centres match within 1–2
px at 1× and under zoom; effects are gone by `t0 + duration`; trim/pause mapping
is correct; disabled mode leaves the render unchanged; missing libass never
loses a take.

## 5. Editable Cursor Styles

**Goal:** separate recorded cursor data from its visual representation so a
saved synthetic take can be restyled without recording again.

**Scope:**
- Opt-in `cursorStyle=synthetic`; baked remains the default.
- Appearance spec in `project.cursor`: shape/preset, size, color, outline,
  shadow, opacity, press scale, optional zoom scaling, custom PNG asset and
  hotspot.
- Built-in shapes as original ASS vector drawings; custom PNG through ffmpeg
  overlay.
- Size proportional to output height for 1080p/1440p/4K and high-DPI.
- Existing baked takes are refused for restyling; there is no clean plate.
- Re-render from the project and raw telemetry, not live config.

**Acceptance:** one synthetic cursor, never a double pointer; telemetry fallback
is safe; style changes survive re-render; auto-zoom transform matches within one
output pixel; PNG validation rejects symlinks, non-PNG, oversized files, and
missing assets; old projects load unchanged.

## Implementation order

1. Create and land the shared foundation branch.
2. Smooth Cursor on the foundation.
3. Cursor Styles on the shared cursor renderer.
4. Motion Blur on the shared smoothed track.
5. Click Effects can start alongside the foundation once the shared telemetry
   and `RenderPlan` interfaces are stable; it does not require a synthetic
   pointer.

No branch may merge automatically. Each feature needs its own live desktop test,
auto-zoom/manual-zoom compatibility check, and explicit merge approval.
