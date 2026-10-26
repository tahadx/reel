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
- Slow motion at half or quarter speed; the save pass retimes video and audio
  together.
- Desktop audio, microphone, pointer capture, 30/60/120 fps, quality presets,
  codec and resolution limits, MP4/MKV, and a webcam corner overlay.
- Live preview of the next take and a bar mark for recording, paused, and
  saving states.
- Recording history with previews, open in Omacut (or `mpv`), an actions menu
  with slow-motion copy, share, and trash, plus path copy.
- Keyboard-first panel.

## Requirements

- Omarchy with the Quattro shell (`omarchy-shell`).
- `gpu-screen-recorder` and `ffmpeg` (which provides `ffprobe`).
- Omarchy's `omarchy-capture-region` picker and `slurp` for selection.
- `grim` for still previews.
- `mpv` for the webcam overlay and history playback fallback.
- `wl-clipboard` if you want the history path-copy action.
- LocalSend for sharing takes through `omarchy share`.
- Optional: `omacut` for opening takes in its trimmer.
- Linux x86_64 for the prebuilt engine. Other architectures must build from
  source; see [Development](docs/DEVELOPMENT.md).

```sh
sudo pacman -S --needed gpu-screen-recorder ffmpeg grim slurp mpv wl-clipboard
```

`omacut` is optional: without it, history takes open in `mpv` instead.

## Install

```sh
omarchy plugin add https://github.com/tahadx/reel --enable
~/.config/omarchy/plugins/io.github.tahadx.reel/install.sh
```

The installer checks the core dependencies, links the `reel` command into
`~/.local/bin`, and enables the widget. It asks before adding optional
Hyprland shortcuts; `--yes` adds them and `--no-binds` skips them.

### Default shortcuts

These are the shortcuts Reel offers to install. **They are not set by
default**: nothing is written to your Hyprland config unless you answer `y` at
the installer prompt or run it with `--yes`.

| Shortcut | Action |
| --- | --- |
| `Super + R` | Start or stop a take |
| `Super + Shift + R` | Open or close the Reel panel |
| `Super + Alt + O` | Pause or resume |

They are added as a marked block in `~/.config/hypr/bindings.lua`; run
`hyprctl reload` after adding them. The block uses keycodes, so the shortcuts
keep working after switching keyboard layouts. Delete the block to remove them.

If `~/.local/bin` is not on your `PATH`, add it. Re-running `install.sh` is
safe. Update with `omarchy plugin update io.github.tahadx.reel`.

CLI options, files, uninstall, and troubleshooting are in
[docs/REFERENCE.md](docs/REFERENCE.md).

## Usage and keyboard shortcuts

Click the Reel mark in the bar to open the panel. Left-click toggles it,
right-click starts or pauses, and middle-click opens the recordings folder.

1. Press **Record** or click the preview.
2. Click a window or monitor, or drag an area.
3. After the countdown (0 by default), Reel records. The panel closes so it
   does not appear in the take.

Pause or resume from the panel, the bar's right-click action, or `p`. Open
history from the clock icon in the header or with `Shift+H`.

A window is recorded as its **visible screen rectangle**, not an isolated
toplevel surface. Overlapping windows and window movement can appear in the
take. Monitors and dragged areas record exactly those screen pixels.

| Key | Action |
| --- | --- |
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
| `x` / `Delete` | Trash the selected take (press twice) |
| `m` | Save a 0.5× slow-motion copy of the selected take |
| `u` | Share the selected take with LocalSend |
| `c` | Copy the selected take's path |
| `o` | Open the recordings folder |

The settings page covers all capture options and the filename prefix. Turn off
**Open takes in Omacut** there to open history takes in `mpv` instead.

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
[Release builds and architecture notes](docs/DEVELOPMENT.md) cover
`bin/refresh-dist` and the release process.

## License

MIT.
