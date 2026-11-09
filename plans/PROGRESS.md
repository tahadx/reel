# Reel cursor-effects program — progress overview

Tracked coordination file (`plans/` is public). Main checkout remains untouched;
no production code, pushes, or GitHub Issues exist yet.

## Worktrees and branches

| Feature | Branch | Worktree | Proposal | Status |
| --- | --- | --- | --- | --- |
| Cursor Motion Blur | `feat/motion-blur` | `/home/taha/Projects/oss/reel-motion-blur` | `plans/proposals/cursor-motion-blur.md` | Investigated, awaiting approval |
| Smooth Cursor | `feat/smooth-cursor` | `/home/taha/Projects/oss/reel-smooth-cursor` | `plans/proposals/smooth-cursor.md` | Investigated, awaiting approval |
| Click Effects | `feat/click-effects` | `/home/taha/Projects/oss/reel-click-effects` | `plans/proposals/click-effects.md` | Investigated, awaiting approval |
| Editable Cursor Styles | `feat/cursor-styles` | `/home/taha/Projects/oss/reel-cursor-styles` | `plans/proposals/cursor-styles.md` | Investigated, awaiting approval |

All four worktrees are based on `main` at `e5cfe3c`, branch verified, no tracked
changes. Proposals are local-only and are not committed.

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

## Shared foundation proposed by the orchestrator

This must land once, before feature branches merge. Details in
`plans/SHARED-FOUNDATION.md`.

1. **Pointer data model + time map**: one parser for `move`/`sync`/`button`,
   button identity, tap/physical source, clicks; `trimStart`, video-start mark,
   pause/resume cuts; saved-take time helper. Raw telemetry stays authoritative.
2. **Coordinate model**: one `Camera::viewport_at` / source-to-output transform
   shared by cursor, blur, clicks, and future layers.
3. **Project contract**: additive `Cursor`/`Ripples` fields (`capture`, `style`,
   `smoothing`, `motionBlur`, `events`, appearance, `cuts`, `telemetryPath`);
   unknown fields preserved; no v2 bump yet.
4. **Capture contract**: `cursor=on` + `baked` stays today's behavior. Synthetic
   mode strips the pointer only after a gated telemetry listener is live, forces
   the hardware cursor, and falls back to baked if telemetry cannot start.
5. **Render plan**: one ffmpeg pass, old `-vf` path kept when nothing is enabled.
   Order: camera -> click effects -> synthetic cursor/blur -> future webcam/canvas.
   Vector layers via generated ASS; custom PNG via overlay; motion blur may need
   a raw RGBA tile overlay if ASS ghost benchmarks are too heavy.
6. **Non-destructive re-render**: retain a pointer-free plate for synthetic
   takes (`.reel/<stem>.plate.<ext>`), store it in the project, and render from
   the plate instead of double-applying effects.
7. **UI surface**: rename the **Zoom** tab to **Visuals**, keep Manual zoom and
   Auto zoom there, and move pointer capture plus all pointer appearance/effect
   rows into it. Remove the **Output** tab and fold its contents into **Media**
   under a new **Saving** section (saved notification, filename prefix,
   recordings path). The tab count is now three settings tabs plus History;
   `1–3` and `Shift+H` are the shortcuts. Defaults preserve today: baked
   pointer, all effects off.

## Open shared decisions (not feature approvals)

- Retain a pointer-free plate for synthetic takes (recommended) or accept that
  restyling is a one-shot save-pass choice?
- For v1, disable live manual zoom on synthetic takes (recommended) or record
  the live viewport as new metadata?
- Custom cursor assets: PNG-only with config/CLI selection acceptable?
- New settings default: all effects off, cursor style `baked` (recommended).
