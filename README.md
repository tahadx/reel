# Reel

Reel is a screen recorder for [Omarchy](https://omarchy.org). It lives in the
bar, records a window, monitor, or selected area, and keeps your takes in a
history you can open, trim with Omacut, or copy its path.

It uses `gpu-screen-recorder` for capture and keeps its state separate from
Omarchy's stock recorder, so both can be used without stepping on each other.

## Features

- Pick a window, monitor, or region with Omarchy's picker; recording starts
  after the countdown.
- Pause and resume without ending the file.
- Manual zoom during a take with touchpad pinch, `Ctrl+Alt` keyboard binds, or
  `Ctrl+Alt+Scroll`, capped by a configurable zoom depth.
- Optional auto zoom in the save pass: clicks become smooth camera blocks, and
  every take with telemetry writes an editable `project.json` for re-rendering.
- Desktop audio, microphone, optional microphone noise reduction, pointer
  capture, 30/60/120 fps, quality presets, codec and resolution limits, MP4/MKV,
  and a webcam corner overlay with a camera picker.
- A bar mark for recording, paused, and saving states.
- Recording history with previews, open in `mpv`, cut in Omacut, an actions
  menu with share and trash, path copy, and drag-out to drop the video into
  another app.
- Keyboard-first panel.
- An update check: the bar and panel mark a waiting update, and the header
  button checks again on demand.

## Requirements

- Omarchy with the Quattro shell (`omarchy-shell`).
- `gpu-screen-recorder` and `ffmpeg` (which provides `ffprobe`).
- Omarchy's `omarchy-capture-region` picker and `slurp` for selection.
- `grim` for still previews.
- `mpv` for history playback and the webcam overlay.
- `wl-clipboard` if you want the history path-copy action.
- LocalSend for sharing takes through `omarchy share`.
- Optional: `omacut` for cutting takes.
- Linux x86_64 for the prebuilt engine. Other architectures must build from
  source; see [Development](docs/development.md).

Install these with Omarchy's package installer: `gpu-screen-recorder`,
`ffmpeg`, `grim`, `slurp`, `mpv`, and `wl-clipboard`.

`omacut` is optional: takes still open in `mpv` without it, but the Cut action
needs it.

## Install

```sh
omarchy plugin add https://github.com/tahadx/reel --enable
~/.config/omarchy/plugins/io.github.tahadx.reel/install.sh
```

The installer checks the core dependencies, links the `reel` command into
`~/.local/bin`, and enables the widget. It asks before adding optional
Hyprland shortcuts; `--yes` adds them and `--no-binds` skips them.

### Updating

```sh
reel update
```

From an open panel, `Shift+U` runs the same command in a floating terminal, so
the updater and its shortcut prompt stay visible while the plugin reloads.

Reel also checks for updates by itself: once when the shell starts and then once
an hour. A waiting update shows as an accent dot on the bar mark and a version
pill in the panel header that turns accent. The pill checks again when Reel is
current, so a manual refresh never needs a terminal. The check contacts only the
plugin's own git origin; a result is cached for the hour.

`reel update` runs `omarchy plugin update io.github.tahadx.reel --yes`, then runs
the installer from the freshly updated plugin checkout. It relinks the `reel`
command, rechecks dependencies, and reloads the widget. It never downloads a
second copy of the installer.

By default it leaves `~/.config/hypr/bindings.lua` alone when the Reel block is
already there. If the block is missing, the installer asks before adding it.
Pass `--binds` to add or refresh the optional shortcuts without prompting:

```sh
reel update --binds
```

Installs older than the update command need the two steps once:

```sh
omarchy plugin update io.github.tahadx.reel
~/.config/omarchy/plugins/io.github.tahadx.reel/install.sh
```

Re-running the installer is safe. It only writes the shortcut block when you
accept at the prompt or pass `--yes`; `--no-binds` skips it explicitly.

### Default shortcuts

These are the shortcuts Reel offers to install. **They are not set by
default**: nothing is written to your Hyprland config unless you answer `y` at
the installer prompt or run it with `--yes`.

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

The full block, ready to copy, is in
[docs/bindings.md](docs/bindings.md).

Touchpad pinch zoom needs no shortcut: Reel registers it at the start of each
take. The zoom binds only act while a take is recording.

They are added as a marked block in `~/.config/hypr/bindings.lua`; run
`hyprctl reload` after adding them. The block uses key names, so the shortcuts
follow the active keyboard layout. Delete the block to remove them.

If `~/.local/bin` is not on your `PATH`, add it.

CLI options, files, uninstall, and troubleshooting are in
[docs/reference.md](docs/reference.md). The visual language is in
[docs/design.md](docs/design.md).

## Usage and keyboard shortcuts

Click the Reel mark in the bar to open the panel. Left-click toggles it,
right-click starts or pauses, and middle-click opens the recordings folder.

1. Press **Record** or click the preview.
2. Click a window or monitor, or drag an area.
3. After the countdown (0 by default), Reel records. The panel closes so it
   does not appear in the take.

Pause or resume from the panel, the bar's right-click action, or `p`. Open
history from the clock icon in the header or with `Shift+H`. Drag a take row
into another app to copy the video file there.

A window is recorded as its **visible screen rectangle**, not an isolated
toplevel surface. Overlapping windows and window movement can appear in the
take. Monitors and dragged areas record exactly those screen pixels.

| Key | Action |
| --- | --- |
| `Shift+U` | Update Reel in a terminal |
| `↑` / `↓` or `j` / `k` | Move between rows |
| `←` / `→` or `h` / `l` | Move horizontally; cycle the focused option |
| `Enter` / `Space` | Activate the selected action |
| `Tab` / `Shift+Tab` | Move between sections |
| `Esc` | Cancel the picker or countdown; go back, then close |
| `q` | Close the panel |
| `Shift+H` | Open or close history |
| `r` | Start or stop a take |
| `p` | Pause or resume |
| `d` | Discard the current take (press twice) |
| `s` | Open or close settings |
| `a` | Open the selected take's actions menu |
| `t` | Cut the selected take in Omacut |
| `x` / `Delete` | Trash the selected take (press twice) |
| `u` | Share the selected take with LocalSend |
| `c` | Copy the selected take's path |
| `o` | Open the recordings folder |

### Zoom during a take

Manual zoom is on by default (Settings → **Manual zoom**). It only acts while a
take is recording; pausing or stopping resets the view to 1×.

- Touchpad pinch zooms at the pointer. Once already zoomed, a pinch that reads
  as a pan moves the viewport instead of changing the factor.
- `Ctrl + Alt + =` / `Ctrl + Alt + -` zoom in and out, `Ctrl + Alt + 0` resets,
  and `Ctrl + Alt + 8` toggles between 1× and the zoom depth.
- `Ctrl + Alt + Scroll` zooms in and out.
- Monitor takes follow the pointer: moving the mouse pans the view and keeps the
  cursor near the middle of the crop. Window and area takes keep the camera
  detached, so the pointer stays where it really is instead of being pulled to
  the screen centre and out of a small capture. `Ctrl + Alt + ←/→/↑/↓` nudges a
  monitor take's view from the keyboard (hold to keep moving, `Shift` for a
  longer step), and **Pan step** in Settings sets how far one nudge goes.
  Panning never changes the zoom depth, and it stays inside the recorded
  monitor.

**Zoom depth** in Settings caps the maximum magnification (1.5×–3×, 2× by
default). The keyboard and scroll controls come from the optional shortcut
block; touchpad pinch is registered by Reel when a take starts. If you already
have your own two-finger pinch binding, Reel overrides it for the take and
reloads your config afterwards so it comes back.

**Noise reduction** is off by default. Turn it on in Settings → **Media**
with the microphone enabled; Reel reduces steady background noise during the
save pass, before the take is normalized. It only runs for takes that captured
the microphone. It uses FFmpeg's noise reduction, so EasyEffects is not
required.

**Record the pointer** controls pointer capture. When it is on, Reel also keeps
local pointer telemetry for later save-pass work. That needs read access to
`/dev/input`; add your user to the `input` group and log in again. If the
devices cannot be read, the take still records without pointer telemetry.

**Auto zoom** is the first row in Settings → **Zoom**, a single on/off switch
that is off by default. Turn it on and clicks (or touchpad taps, when
tap-to-click is on) become short 2× zoom blocks in the saved file. When a
click only focuses a window, the block follows the part of the screen that
changes — the text you are typing and the caret next to it — instead of zooming
into an empty area.
The pointer is still recorded in this version and scales with the zoom. Reel
needs read access to `/dev/input` to see the clicks; without it the take still
saves, just without the automatic zoom.

Every saved take with telemetry writes a `project.json` next to it, even when
auto zoom is off. `reel render --project <path>` renders the take again with the
camera path in that file. The default output is `<take-stem>.rendered.<ext>`,
and the original take is never replaced. The project schema and the keyframe
fields are in [docs/reference.md](docs/reference.md#re-rendering-a-take).

The settings page covers all capture options and the filename prefix. History
takes open in `mpv`; **Cut** sends the selected take to Omacut for trimming.

Takes are saved in the recordings folder, `~/Videos/Reel` by default. Closing
the panel with `q` or `Esc` does not stop an active take; use `r` or the stop
action.

## Development

Build and test from the repository root:

```sh
cargo build --release
cargo test --workspace
bin/lint-qml
bin/test-qml
```

The plugin ships `bin/dist/reel-native`, so users do not need a Rust toolchain.
[Release builds and architecture notes](docs/development.md) cover
`bin/refresh-dist` and the release process.

## License

MIT.
