# shared cursor foundation

**status:** approved for implementation; not started.
**branch:** `feat/cursor-foundation` (to be created)
**design:** `plans/shared-foundation.md`
**approval:** `plans/features/index.md`

## goal

one pointer-data and post-processing base for every cursor feature.

## scope

- rich telemetry reader: preserve `move`, `sync`, `button` down/up, button
  identity, tap/physical source, capture facts.
- `TimeMap`: `trimStart`, video-start mark, pause/resume cuts, saved-take time.
- `PointerTrack`: ordered samples, segment splitting, `position_at`,
  `velocity_at`; raw telemetry stays authoritative.
- project schema: additive `cursor`/`ripples` fields, unknown fields preserved.
- synthetic capture contract: strip the pointer only after a gated telemetry
  listener is live; force the hardware cursor; fall back to baked on failure.
- `RenderPlan`: one ffmpeg pass, old `-vf` path unchanged when nothing is on.
  order: camera -> click effects -> cursor/blur -> future webcam/canvas.
- plate retention and re-render guard.
- shared tests and fixtures for time, coordinates, trim, pauses, and missing
  telemetry.

## acceptance

existing tests pass; a baked take renders exactly as today; a synthetic take can
be rendered twice from the same project and telemetry without double-applying an
effect; no new dependency.
