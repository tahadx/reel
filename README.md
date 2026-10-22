# Reel

A screen recorder for [Omarchy](https://omarchy.org): a bar widget, a
keyboard-first recording history, and a small CLI that owns the recording. It records
a crop, a window, or a monitor with the audio and framing you choose, can pause
mid-take, and can save a take in slow motion.

Reel uses `gpu-screen-recorder` for capture and keeps its state separate from
Omarchy's stock recorder, so both can be used without stepping on each other.

## Features

- **One capture flow** — press Record and Omarchy's picker opens. Click a
  window, click a monitor, or drag an area; Reel records exactly that.
- **No extra confirmation** — the moment you pick a target, Reel counts down
  and starts recording. There is no separate Start screen.
- **Honest framing** — a clicked window is recorded as its visible bounds, not
  an isolated toplevel surface. Overlapping windows and window movement can
  appear in the take.
- **Pause and resume** without ending the file.
- **Slow motion** at 0.5× or 0.25×: capture runs at 120 fps and the save pass
  retimes video and audio together.
- **All the stock recorder options** — desktop audio, microphone, pointer,
  countdown, frame rate, quality, codec, resolution cap, container, output
  folder — plus a webcam corner overlay.
- **A live preview** at the top of the panel. Click it, or press Record, to open
  the picker.
- **A recording history page**: recent takes, scrollable, with play,
  slow-motion copy, path copy, and trash actions. The most recent take appears
  at the top.
- **A bar mark that tells the state at a glance**: a screen-and-dot mark when
  idle, a live clock (with a pulsing dot) while recording, a pause glyph when
  paused, and `saving` while the take is being finished.
- **Safe finalization**: the save pass probes its own output before replacing
  the raw take. A failed finalizer leaves the original file in place, and a
  name collision gets a numeric suffix instead of overwriting an older take.
- **A clean save pass**: drops the first frame, mutes the capture-open pop,
  loudness-normalizes the audio, and stream-copies the video when it can.
- **Keyboard-first**: every panel action has a key. Hyprland shortcuts are
  optional and shown by the installer, but never added automatically.

## Requirements

- Omarchy with the Quattro shell (`omarchy-shell`).
- `gpu-screen-recorder` (tested with 6.1.4), `ffmpeg`, `ffprobe`.
- Omarchy's capture helpers (`omarchy-capture-region`, `slurp`, `grim`), and
  `mpv` for the webcam overlay.
- Linux x86_64 for the prebuilt engine. The engine is statically linked
  against musl, so it does not depend on your glibc version. Other
  architectures must build the engine from source; see Development.

```sh
sudo pacman -S --needed gpu-screen-recorder ffmpeg grim slurp mpv
```

The plugin ships its engine at `bin/dist/reel-native`, so installing Reel does
not need a Rust toolchain. The installed wrapper runs that engine, which owns
the picker, countdown, recording, and save flow.

## Install

```sh
omarchy plugin add https://github.com/tahadx/reel --enable
~/.config/omarchy/plugins/io.github.tahadx.reel/install.sh
```

`omarchy plugin add` puts Reel on the bar. The first time the bar loads it,
`bin/configure` links the `reel` command into `~/.local/bin` from the prebuilt
engine shipped at `bin/dist/reel-native`. `install.sh` runs that setup if it has
not happened yet and asks before adding the optional shortcuts.
`install.sh --yes` adds them without asking; `--no-binds` skips them.

Re-running `install.sh` is safe. If `~/.local/bin` is not on your PATH, add it
or link the wrapper somewhere else.

To update: `omarchy plugin update io.github.tahadx.reel`, then re-run
`install.sh` only if you want to change shortcuts.

To uninstall: `bin/uninstall` in the plugin directory, then
`omarchy plugin remove io.github.tahadx.reel`. Recordings are untouched.

### Optional shortcuts

The installer does not add these automatically. Answer `y` at the prompt, or
add the block yourself to `~/.config/hypr/bindings.lua` and run `hyprctl reload`.

| Shortcut | Action |
| --- | --- |
| `Super + R` | Start or stop a take |
| `Super + Shift + R` | Open or close the Reel panel |
| `Super + Alt + O` | Pause or resume |

They are written as a marked `-- Reel: begin … -- Reel: end` block, so
re-running the installer replaces it cleanly and deleting the block removes it.

## Development

The engine is a Cargo workspace, matching the layout used by Catch:

- `Cargo.toml` — workspace manifest and release profile,
- `crates/reel/` — the recording engine and CLI,
- `bin/dist/reel-native` — the committed engine the plugin ships.

Build and test from the repo root:

```sh
cargo test --workspace
cargo build --release
bin/refresh-dist
```

`bin/refresh-dist` builds `-p reel` for `x86_64-unknown-linux-musl`, copies
`target/x86_64-unknown-linux-musl/release/reel` to `bin/dist/reel-native`,
strips it, and prints its version. **Run it after any `crates/` change and
commit the refreshed binary in the same commit.** The plugin never builds on
the user's machine; a stale binary is a bug, and CI and the pre-push hook both
fail on one.

The toolchain is pinned in `rust-toolchain.toml` and the release profile strips
symbols, so CI can rebuild the committed binary byte for byte. A static musl
build also means the engine runs the same on Arch, Ubuntu, and the CI runner.

`bin/stamp-version X.Y.Z` writes a version into `manifest.json` and
`crates/reel/Cargo.toml`; run it before tagging a release. `bin/version`
resolves the version from `$REEL_VERSION`, an exact tag, or the nearest tag
plus commits.

A clone can still be installed into the plugin directory for development:

```sh
./install.sh
```

That path copies the runtime files into
`~/.config/omarchy/plugins/io.github.tahadx.reel` and relinks
`~/.local/bin/reel` to the installed wrapper. It is the only path that copies
files; the marketplace install already has them in place.

The engine is prebuilt for linux-x86_64. On another architecture, build it with
`bin/refresh-dist` and commit the resulting `bin/dist/reel-native`, or use the
development install above with a locally built engine.

## Using the panel

Click the Reel icon in the bar to open it. Left-click starts or stops,
right-click starts or pauses, middle-click opens the recordings folder.

Inside the panel:

1. Press Record or click the preview. The panel closes and the picker opens.
2. Click a window or monitor, or drag an area.
3. Reel counts down and starts recording immediately. The panel stays closed
   so the setup UI is not in the take.

The live preview follows the current take: while a window or area is being
recorded it stays cropped to those bounds, then returns to the full screen when
the take ends.

| Key | Action |
| --- | --- |
| `←` `→` `↑` `↓` / `j` `k` | Move the cursor |
| `Enter` / `Space` | Activate (start, stop, save, play) |
| `Tab` / `Shift + Tab` | Move between sections |
| `Esc` | Cancel a selection or countdown; otherwise back, then close |
| `q` | Close the panel |
| `h` | Open or close history |
| `r` | Start or stop a take |
| `p` | Pause or resume |
| `d` | Discard the current take |
| `s` | Open or close settings |
| `x` / `Delete` | Trash the selected take |
| `m` | Save a slow-motion copy of the selected take |
| `c` | Copy the selected take's path |
| `o` | Open the recordings folder |

The first row is the action row, then the live preview and quick settings. The
clock-shaped history icon sits to the left of the gear in the panel header and
opens your recording history; the gear opens a settings page with every option in the
manifest and an editable filename prefix.

Escape and closing the panel never stop an active take. Use the stop action,
the bar's recording control, or the shortcut.

## Capture

Every take uses the same picker:

- **Click a window** to record its visible rectangle.
- **Click a monitor** to record that whole output.
- **Drag an area** to record just that crop.

Picking a target starts the take. The panel and the `reel toggle` shortcut use
the same picker and the same automatic countdown.

## CLI

The panel is a view over the CLI, and the CLI is usable on its own:

```sh
reel start [options]              # pick a target, then record
reel toggle                       # start, or stop the active take
reel stop [--discard]
reel pause | resume | toggle-pause
reel status [--json]
reel recent [--json] [--limit N]
reel slowmo <recording> [0.5|0.25]
reel trash <recording>
reel config [path|show|init|set <key> <value>|apply <json>]
reel preview [--json]
reel output
reel doctor
```

`reel start` accepts `--desktop-audio on|off`, `--microphone-audio on|off`,
`--cursor on|off`, `--webcam off|small|medium|large`, `--webcam-device`,
`--slowmo off|0.5|0.25`, `--countdown SECONDS`, `--fps 30|60|120`,
`--quality very_low|low|medium|high|very_high|ultra`, `--codec auto|h264|hevc|av1`,
`--resolution 0x0|3840x2160|2560x1440|1920x1080|1280x720`,
`--container mp4|mkv`, `--output-dir PATH`, and `--filename-prefix TEXT`.

`reel start` validates the settings and output folder before opening the
picker, so a broken setup fails before you draw a selection.

Settings live in `~/.config/omarchy/reel.json`. The panel keeps that file in
step with the widget's own settings, so a flag you change in the panel is what
the next `reel start` uses.

Examples:

```sh
reel start --slowmo 0.5 --countdown 0
reel start --microphone-audio on
reel toggle-pause
reel stop
```

## Files

| Path | Contents |
| --- | --- |
| `~/.config/omarchy/reel.json` | Settings shared by the panel and CLI |
| `$XDG_RUNTIME_DIR/reel/state.json` | Live state the bar watches |
| `$XDG_RUNTIME_DIR/reel/reel.log` | Recorder and save-pass log |
| `$XDG_RUNTIME_DIR/reel/recorder.sock` | Pause/resume IPC to the recorder |
| `$XDG_RUNTIME_DIR/reel/next-preview-*.jpg` | Still produced by `reel preview` |
| `~/Videos/Reel` | Default recordings folder |

Takes are named `reel-YYYY-MM-DD_HH-MM-SS.mp4` by default; set a filename
prefix in the settings page (or `reel config set filenamePrefix Demo`) to make
them `Demo-YYYY-MM-DD_HH-MM-SS.mp4`. A numeric suffix is added if two takes
would land in the same second, so an existing recording is never overwritten.
A preview frame sits next to each take in the history page and the saved notification.

## Troubleshooting

```sh
reel doctor        # checks every tool the recorder needs
reel status        # what the recorder thinks it is doing
tail -f "${XDG_RUNTIME_DIR:-/tmp}/reel/reel.log"
```

- **The bar says the command is missing** — `reel` is not on `PATH`. Run the
  plugin's `bin/configure` (it links the wrapper into `~/.local/bin`) or add
  `~/.local/bin` to your profile.
- **Is the native engine active?** — the wrapper runs `bin/reel-native` when a
  development checkout has one, otherwise `bin/dist/reel-native`. `reel doctor`
  prints the state path and every tool it uses.
- **The installer says the shipped engine is missing or cannot execute** — the
  plugin is built for linux-x86_64. Reinstall it, or build the engine from
  source with `bin/refresh-dist` and run `bin/configure` again.
- **The panel does not appear after an update** — restart the shell:
  `omarchy-restart-shell`.
- **Slow motion looks like normal speed** — the take must be captured at
  120 fps; Reel switches the capture rate automatically when slow motion is
  enabled. Re-saving an old take with `reel slowmo` also works.
- **A very short take has no audio** — takes under a second are saved silent
  rather than failing the loudness pass.
- **The preview is black** — the panel needs `wlr-screencopy` or
  `ext-image-copy-capture` support from the compositor. On Hyprland 0.56 it
  works; run `reel doctor` if the preview stays empty.

## Uninstall

```sh
~/.config/omarchy/plugins/io.github.tahadx.reel/bin/uninstall
omarchy plugin remove io.github.tahadx.reel
```

`bin/uninstall` removes only the `~/.local/bin/reel` symlink and the marked
`-- Reel: begin … -- Reel: end` shortcut block. Recordings, settings, and state
are untouched.

## License

MIT.
