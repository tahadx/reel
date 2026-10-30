# Hyprland bindings

Reel does not need any Hyprland bindings: the bar widget, the panel, and
touchpad pinch zoom all work without them. The installer can add the bindings
below to `~/.config/hypr/bindings.lua`; this page is the same marked block for
anyone who prefers to add them by hand.

## Add them by hand

1. Append the block below to `~/.config/hypr/bindings.lua`.
2. Run `hyprctl reload`.
3. To remove them later, delete everything from `-- Reel: begin.` through
   `-- Reel: end.` and run `hyprctl reload` again.

Keep the two marker lines. `install.sh` and `bin/uninstall` use them to replace
or remove only this block, without touching the rest of the file.

```lua
-- Reel: begin. Written by install.sh; delete this block to remove the binds.
-- Keycodes keep these binds working after switching keyboard layouts.
o.bind("SUPER + code:27", "Reel start/stop", "reel toggle")
o.bind("SUPER + SHIFT + code:27", "Reel panel", "omarchy-shell shell toggle io.github.tahadx.reel '{}'")
o.bind("SUPER + ALT + code:32", "Reel pause/resume", "reel toggle-pause")

-- Zoom controls (Apple-style: non-conflicting Ctrl+Alt, mouse wheel, & touchpad gestures)
o.bind("CTRL + ALT + equal", "Reel zoom in", "reel zoom-in")
o.bind("CTRL + ALT + minus", "Reel zoom out", "reel zoom-out")
o.bind("CTRL + ALT + 0", "Reel zoom reset", "reel zoom-reset")
o.bind("CTRL + ALT + 8", "Reel zoom toggle", "reel zoom-toggle")
o.bind("CTRL + ALT + mouse_up", "Reel zoom in", "reel zoom-in --step 0.15")
o.bind("CTRL + ALT + mouse_down", "Reel zoom out", "reel zoom-out --step 0.15")

-- Keycodes for zoom shortcuts (works across any active keyboard layout)
o.bind("CTRL + ALT + code:21", "Reel zoom in", "reel zoom-in")
o.bind("CTRL + ALT + code:20", "Reel zoom out", "reel zoom-out")
o.bind("CTRL + ALT + code:19", "Reel zoom reset", "reel zoom-reset")
o.bind("CTRL + ALT + code:17", "Reel zoom toggle", "reel zoom-toggle")
-- Reel: end.
```

## What each bind does

| Shortcut | Action |
| --- | --- |
| `Super + R` | Start or stop a take |
| `Super + Shift + R` | Open or close the Reel panel |
| `Super + Alt + O` | Pause or resume |
| `Ctrl + Alt + =` / `Ctrl + Alt + -` | Zoom in / out during a take |
| `Ctrl + Alt + 0` | Reset zoom to 1× |
| `Ctrl + Alt + 8` | Toggle between 1× and the zoom depth |
| `Ctrl + Alt + Scroll` | Zoom in / out during a take |

The block binds physical keycodes as well as keysyms, so the shortcuts keep
working after switching keyboard layouts. The zoom binds only act while a take
is recording and **Manual zoom** is on. Touchpad pinch is not in this block:
Reel registers the two-finger pinch gesture when a take starts.

`omarchy plugin update` never edits `bindings.lua`. After updating, re-run the
plugin's `install.sh` or replace the marked block with the one above to pick up
new commands.
