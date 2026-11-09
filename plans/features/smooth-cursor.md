# smooth cursor

**status:** approved for implementation; not started.
**branch:** `feat/smooth-cursor`
**worktree:** `/home/taha/Projects/oss/reel-smooth-cursor`
**proposal:** `plans/proposals/smooth-cursor.md`
**approval:** `plans/features/index.md`

## goal

reconstruct a natural pointer path from raw telemetry without changing the
recording data.

## scope

- synthetic pointer mode: `cursorStyle=baked|synthetic`.
- smoothing presets: `off`, `precise`, `balanced`, `cinematic`.
- pchip reconstruction plus symmetric speed-adaptive ema; no systematic lag.
- click anchors are hard constraints; smoothing relaxes around clicks.
- frame-time sampling on the saved-take clock; cursor composited after the
  camera at a constant screen size.
- disable switch is `cursorSmoothing=off`.

## acceptance

raw telemetry is byte-identical after save; click positions are exact on the
nearest frame; stationary jitter is reduced without overshoot; 30/60/120 fps
outputs agree at common timestamps; baked mode is unchanged.
