# click effects

**status:** approved for implementation; not started.
**branch:** `feat/click-effects`
**worktree:** `/home/taha/Projects/oss/reel-click-effects`
**proposal:** `plans/proposals/click-effects.md`
**approval:** `plans/features/index.md`

## goal

restrained click feedback at the exact click location.

## scope

- preserve left/right button down/up, tap/physical source, drags, and pause
  markers in telemetry.
- normalize `ClickEvent`s in saved-take time; store them in `project.ripples`.
- ring, pulse, and highlight styles; left/right differentiation; duration, size,
  intensity, button filter.
- generated ass vector events after the camera; optional png/custom layers later.
- independently configurable and disableable; works over a baked pointer.
- no screen-pixel click detection.

## acceptance

left/right identity is correct; click centres match within 1-2 px at 1x and
under zoom; effects are gone by `t0 + duration`; trim/pause mapping is correct;
disabled mode leaves the render unchanged; missing libass never loses a take.
