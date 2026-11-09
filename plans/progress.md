# Reel cursor-effects program — progress overview

Tracked coordination file (`plans/` is public). The four features are approved
for implementation, but **no feature code has started**. The approval record is
`plans/features/index.md`; the shared design is `plans/shared-foundation.md`.

## Current state

- `main` carries the shared UI scaffold and the update-restart fix.
- Feature worktrees carry the same scaffold plus their proposal and branch brief.
- Every worktree is clean.
- No feature implementation.
- Do not start feature code until the orchestrator gives the kickoff.

## Worktrees and branches

| Feature | Branch | Worktree | Proposal | Status |
| --- | --- | --- | --- | --- |
| Shared cursor foundation | `feat/cursor-foundation` (to create) | new worktree | `plans/shared-foundation.md` | Approved, not started |
| Cursor Motion Blur | `feat/motion-blur` | `/home/taha/Projects/oss/reel-motion-blur` | `plans/proposals/cursor-motion-blur.md` | Approved, not started |
| Smooth Cursor | `feat/smooth-cursor` | `/home/taha/Projects/oss/reel-smooth-cursor` | `plans/proposals/smooth-cursor.md` | Approved, not started |
| Click Effects | `feat/click-effects` | `/home/taha/Projects/oss/reel-click-effects` | `plans/proposals/click-effects.md` | Approved, not started |
| Editable Cursor Styles | `feat/cursor-styles` | `/home/taha/Projects/oss/reel-cursor-styles` | `plans/proposals/cursor-styles.md` | Approved, not started |

The foundation branch does not exist yet. Create it before feature work so the
shared interfaces land once.

## Approval record

- Approved on 2026-11-09: shared cursor foundation, Smooth Cursor, Cursor Motion
  Blur, Click Effects, and Editable Cursor Styles.
- `plans/features/index.md` is the feature register and acceptance record.
- Shared defaults: pointer-free plate retained, manual live zoom disabled or
  falling back for synthetic takes in v1, PNG-only custom cursor assets,
  baked/off defaults, and the existing Recording/Visuals/Shortcuts tabs.
- No implementation has started.

## Baseline findings that drive the shared design

- The pointer is baked into captures today. `gpu-screen-recorder` gets
  `-cursor yes` when `cursor` is on; when off, Reel forces a hardware cursor so
  the pointer is not composited into the frame.
- Telemetry already records `move`, `button` (left/right/middle, down/up),
  `sync` at ~10 Hz, and `stop`, but `project::read_telemetry` keeps only
  button-down clicks. It discards movement, button identity, releases, and
  syncs.
- `project.json` has placeholder `cursor` and `ripples` blocks, but the renderer
  ignores them. `render.rs` is a single ffmpeg pass that applies only the
  auto-zoom camera (`zoompan`, or `sendcmd` + `crop`).
- The save pass writes a project for every take with telemetry, but skips
  `render_take` when the camera has no zoom. Any new overlay must change that
  decision.
- Auto zoom uses saved-take time (`raw t - trimStart`). Pause/resume is not
  recorded in telemetry; the video-start offset is not marked either. This is a
  latent alignment bug for auto zoom and a blocker for frame-accurate pointer
  effects.
- Manual zoom is live Hyprland compositor zoom. Its viewport is not stored in
  the project, so a synthetic pointer cannot be positioned inside a manually
  zoomed frame without new metadata or a restriction.
- Rotated monitors are under-mapped: `CaptureSpace::to_capture` scales but does
  not rotate coordinates. Synthetic effects would inherit the error.
- The raw capture is deleted after a successful save, and `reel render` applies
  the camera to the already-rendered take. Real non-destructive restyling needs
  a retained pointer-free plate and an already-rendered guard.

## Shared foundation

Must land once, before feature branches merge. Details in
`plans/shared-foundation.md`.

1. **Pointer data model + time map**: one parser for `move`/`sync`/`button`,
   button identity, tap/physical source, clicks; `trimStart`, video-start mark,
   pause/resume cuts; saved-take time helper. Raw telemetry stays authoritative.
2. **Coordinate model**: one `Camera::viewport_at` / source-to-output transform
   shared by cursor, blur, clicks, and future layers.
3. **Project contract**: additive `Cursor`/`Ripples` fields (`capture`, `style`,
   `smoothing`, `motionBlur`, `events`, appearance, `cuts`, `telemetryPath`,
   `plate`); unknown fields preserved; no v2 bump yet.
4. **Capture contract**: `cursor=on` + `baked` stays today's behavior. Synthetic
   mode strips the pointer only after a gated telemetry listener is live, forces
   the hardware cursor, and falls back to baked if telemetry cannot start.
5. **Render plan**: one ffmpeg pass, old `-vf` path kept when nothing is enabled.
   Order: camera → click effects → synthetic cursor/blur → future webcam/canvas.
   Vector layers via generated ASS; custom PNG via overlay; motion blur may need
   a raw RGBA tile overlay if ASS ghost benchmarks are too heavy.
6. **Non-destructive re-render**: retain a pointer-free plate for synthetic
   takes (`.reel/<stem>.plate.<ext>`), store it in the project, and render from
   the plate instead of double-applying effects.
7. **UI surface**: the tabs are Recording, Visuals, and Shortcuts. Pointer
   capture and pointer effects live in Visuals. Output is folded into Recording
   → Saving. Defaults preserve today: baked pointer, all effects off.

## Implementation order

1. Create the shared foundation branch and land the interfaces.
2. Smooth Cursor.
3. Cursor Styles.
4. Motion Blur.
5. Click Effects can run alongside the foundation once shared telemetry and
   `RenderPlan` are stable.

No automatic merges. Each feature needs its own live desktop test, auto-zoom and
manual-zoom compatibility check, and explicit merge approval.
