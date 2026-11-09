# cursor motion blur

**status:** approved for implementation; not started.
**branch:** `feat/motion-blur`
**worktree:** `/home/taha/Projects/oss/reel-motion-blur`
**proposal:** `plans/proposals/cursor-motion-blur.md`
**approval:** `plans/features/index.md`

## goal

velocity/direction-aware cursor blur that stays sharp at rest.

## scope

- opt-in `cursorMotionBlur=off|low|medium|high`.
- synthetic pointer required; baked takes are not blurred.
- shutter-based exposure, exponential decay, output-pixel smear cap, stationary
  deadband, segment-safe sampling.
- shared cursor layer; if ass ghost events are too heavy for long takes, add a
  raw rgba tile behind the same `RenderPlan`.
- composes with smooth cursor: blur consumes the smoothed track.
- re-render reads resolved project values, not live settings.

## acceptance

stationary cursor is sharp; fast moves blur in the direction of travel without
screen-wide smear; 30/60/120 fps are stable; auto zoom and blur compose; disabled
mode is a no-op.
