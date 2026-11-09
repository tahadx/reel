# approved cursor features

**approved:** 2026-11-09
**status:** approved for implementation; not started.
**approval record:** this folder.
**scope:** all four user-facing features are approved together, but each keeps
its own branch, worktree, verification gate, and merge decision.

read `plans/shared-foundation.md` before touching cursor, telemetry, project,
render, capture, or settings code. do not start implementation until the
orchestrator gives the kickoff.

## feature register

| feature | file | branch | worktree | proposal | status |
| --- | --- | --- | --- | --- | --- |
| shared cursor foundation | `shared-cursor-foundation.md` | `feat/cursor-foundation` (to create) | new worktree | `plans/shared-foundation.md` | approved, not started |
| smooth cursor | `smooth-cursor.md` | `feat/smooth-cursor` | `/home/taha/Projects/oss/reel-smooth-cursor` | `plans/proposals/smooth-cursor.md` | approved, not started |
| cursor motion blur | `cursor-motion-blur.md` | `feat/motion-blur` | `/home/taha/Projects/oss/reel-motion-blur` | `plans/proposals/cursor-motion-blur.md` | approved, not started |
| click effects | `click-effects.md` | `feat/click-effects` | `/home/taha/Projects/oss/reel-click-effects` | `plans/proposals/click-effects.md` | approved, not started |
| editable cursor styles | `editable-cursor-styles.md` | `feat/cursor-styles` | `/home/taha/Projects/oss/reel-cursor-styles` | `plans/proposals/cursor-styles.md` | approved, not started |

the shared foundation is a coordination branch, not a fifth user-facing feature.
it lands first so the feature branches do not invent competing telemetry
parsers, project schemas, camera transforms, or ffmpeg graphs.

## shared decisions recorded at approval

- **pointer-free plate:** keep one for synthetic takes so style, smoothing, and
  blur can be changed after saving. the project stores the plate path; renders
  use the plate and never double-apply effects.
- **manual live zoom:** disable it for synthetic-cursor takes in v1, or fall back
  to the baked pointer. recording the live viewport is a later job.
- **custom cursor assets:** png only, selected through config/cli. no svg, no
  native file picker in v1.
- **defaults:** `cursorStyle=baked`, all new effects off. today's output is
  unchanged unless the user opts in.
- **ui:** the existing tabs are recording, visuals, and shortcuts. pointer
  capture and pointer effects live in visuals. no fifth tab.

## implementation order

1. create and land the shared foundation branch.
2. smooth cursor on the foundation.
3. cursor styles on the shared cursor renderer.
4. motion blur on the shared smoothed track.
5. click effects can start alongside the foundation once the shared telemetry
   and `RenderPlan` interfaces are stable; it does not require a synthetic
   pointer.

no branch may merge automatically. each feature needs its own live desktop test,
auto-zoom/manual-zoom compatibility check, and explicit merge approval.
