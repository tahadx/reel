# editable cursor styles

**status:** approved for implementation; not started.
**branch:** `feat/cursor-styles`
**worktree:** `/home/taha/Projects/oss/reel-cursor-styles`
**proposal:** `plans/proposals/cursor-styles.md`
**approval:** `plans/features/index.md`

## goal

separate recorded cursor data from its visual representation so a saved
synthetic take can be restyled without recording again.

## scope

- opt-in `cursorStyle=synthetic`; baked remains the default.
- appearance spec in `project.cursor`: shape/preset, size, color, outline,
  shadow, opacity, press scale, optional zoom scaling, custom png asset and
  hotspot.
- built-in shapes as original ass vector drawings; custom png through ffmpeg
  overlay.
- size proportional to output height for 1080p/1440p/4k and high-dpi.
- existing baked takes are refused for restyling; there is no clean plate.
- re-render from the project and raw telemetry, not live config.

## acceptance

one synthetic cursor, never a double pointer; telemetry fallback is safe; style
changes survive re-render; auto-zoom transform matches within one output pixel;
png validation rejects symlinks, non-png, oversized files, and missing assets;
old projects load unchanged.
