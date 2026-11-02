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
-- Key names follow the active keyboard layout.
o.bind("SUPER + R", "Reel start/stop", "reel toggle")
o.bind("SUPER + SHIFT + R", "Reel panel", { panel = "io.github.tahadx.reel" })
o.bind("SUPER + ALT + O", "Reel pause/resume", "reel toggle-pause")

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

-- Pan the zoomed view; hold to keep moving, Shift to cover more ground
o.bind_hold("CTRL + ALT + code:113", "Reel pan left", "reel zoom-pan left --hold", "Reel pan left release", "reel zoom-pan --stop")
o.bind_hold("CTRL + ALT + code:114", "Reel pan right", "reel zoom-pan right --hold", "Reel pan right release", "reel zoom-pan --stop")
o.bind_hold("CTRL + ALT + code:111", "Reel pan up", "reel zoom-pan up --hold", "Reel pan up release", "reel zoom-pan --stop")
o.bind_hold("CTRL + ALT + code:116", "Reel pan down", "reel zoom-pan down --hold", "Reel pan down release", "reel zoom-pan --stop")
o.bind_hold("CTRL + ALT + SHIFT + code:113", "Reel pan left fast", "reel zoom-pan left --hold --fast", "Reel pan left fast release", "reel zoom-pan --stop")
o.bind_hold("CTRL + ALT + SHIFT + code:114", "Reel pan right fast", "reel zoom-pan right --hold --fast", "Reel pan right fast release", "reel zoom-pan --stop")
o.bind_hold("CTRL + ALT + SHIFT + code:111", "Reel pan up fast", "reel zoom-pan up --hold --fast", "Reel pan up fast release", "reel zoom-pan --stop")
o.bind_hold("CTRL + ALT + SHIFT + code:116", "Reel pan down fast", "reel zoom-pan down --hold --fast", "Reel pan down fast release", "reel zoom-pan --stop")
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
| `Ctrl + Alt + ←/→/↑/↓` | Pan the zoomed view during a take (hold to keep moving) |
| `Ctrl + Alt + Shift + ←/→/↑/↓` | Pan three times as far |

The block uses key names for Reel's Super shortcuts and physical keycodes for
the zoom and pan binds, so zoom keeps working after switching keyboard layouts.
The zoom binds only act while a take is recording and **Manual zoom** is on.
Touchpad pinch is not in this block: Reel registers the two-finger pinch gesture
when a take starts.

`omarchy plugin update` never edits `bindings.lua`. After updating, re-run the
plugin's `install.sh` or replace the marked block with the one above to pick up
new commands.
