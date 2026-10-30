# Reel reference

Detailed reference for the Reel CLI, settings, runtime files, troubleshooting,
and uninstall. See the [README](../README.md) for installation and panel usage.

## CLI

The panel is a front end for the `reel` command. The command is also usable on
its own:

| Command | Action |
| --- | --- |
| `reel` | Show the current state (same as `reel status`) |
| `reel start [options]` | Pick a target and start recording |
| `reel toggle [options]` | Start a take, or stop the active one |
| `reel stop [--discard]` | Stop and save, or stop and discard |
| `reel pause` | Pause the active take |
| `reel resume` | Resume a paused take |
| `reel toggle-pause` | Pause or resume |
| `reel status [--json]` | Print the current state |
| `reel recent [--json] [--limit N]` | List recent takes; the default limit is 8 |
| `reel slowmo <file> [0.5\|0.25]` | Write a slow-motion copy; the default factor is 0.5 |
| `reel trash <file>` | Move a take to the trash |
| `reel config <command>` | Read or write settings |
| `reel preview [--json] [--note-only]` | Capture a still of the focused monitor |
| `reel output` | Print the recordings folder |
| `reel doctor` | Check tools and print diagnostics |

`reel toggle` accepts the same options as `reel start`. `reel stop` fails when
no take is active. `reel trash` accepts only files inside the recordings folder.

### Start options

| Option | Values | Default |
| --- | --- | --- |
| `--desktop-audio` | `on`, `off` | `on` |
| `--microphone-audio` | `on`, `off` | `off` |
| `--cursor` | `on`, `off` | `on` |
| `--webcam` | `off`, `small`, `medium`, `large` | `off` |
| `--webcam-device` | camera path, such as `/dev/video0` | first camera |
| `--slowmo` | `off`, `0.5`, `0.25` | `off` |
| `--countdown` | seconds, clamped to 0–60 | `0` |
| `--fps` | `30`, `60`, `120` | `60` |
| `--quality` | `very_low`, `low`, `medium`, `high`, `very_high`, `ultra` | `very_high` |
| `--codec` | `auto`, `h264`, `hevc`, `av1` | `auto` |
| `--resolution` | `0x0`, `3840x2160`, `2560x1440`, `1920x1080`, `1280x720` | `0x0` |
| `--container` | `mp4`, `mkv` | `mp4` |
| `--output-dir` | folder path; `~` expands | XDG Videos `/Reel` |
| `--filename-prefix` | text prepended to the timestamp | empty (`reel-`) |

`0x0` keeps the selected crop or monitor at native size. Enabling slow motion
raises the capture rate to 120 fps when needed, then retimes video and audio in
the save pass. `--microphone-audio` also has the alias `--mic`, and `--slowmo`
also accepts `0.5x`/`half` and `0.25x`/`quarter`. `reel preview --note-only`
resolves the monitor and note without writing a still.

Examples:

```sh
reel start --slowmo 0.5 --countdown 0
reel start --microphone-audio on
reel toggle-pause
reel stop
```

## Settings

Settings live in `~/.config/omarchy/reel.json`. The panel writes its capture
settings there before the next take, so a change made in the panel is what
`reel start` uses. Unset keys fall back to the defaults above.

| Command | Action |
| --- | --- |
| `reel config path` | Print the settings file path |
| `reel config show` | Print the stored settings |
| `reel config init` | Write default settings if the file does not exist |
| `reel config set <key> <value>` | Set one key; the value is parsed as JSON when possible |
| `reel config apply [json]` | Merge a JSON object into the settings |

```sh
reel config path
reel config set filenamePrefix Demo
reel config apply '{"slowmo": "0.5"}'
```

If the JSON argument to `config apply` is omitted, Reel reads it from stdin.

## Runtime files

| Path | Contents |
| --- | --- |
| `~/.config/omarchy/reel.json` | Settings shared by the panel and CLI |
| `$XDG_RUNTIME_DIR/reel/state.json` | Live state the bar watches |
| `$XDG_RUNTIME_DIR/reel/last.json` | Metadata for the last completed take |
| `$XDG_RUNTIME_DIR/reel/reel.log` | Recorder and save-pass log |
| `$XDG_RUNTIME_DIR/reel/recorder.sock` | Pause/resume IPC to the recorder |
| `$XDG_RUNTIME_DIR/reel/next-preview-*.jpg` | Still produced by `reel preview` |
| `$XDG_CACHE_HOME/reel/previews` | History and notification thumbnails |
| `~/Videos/Reel` | Default recordings folder |

If `XDG_RUNTIME_DIR` is unset, the runtime files live under
`~/.local/state/omarchy/reel/run` instead. The default output folder follows
the XDG Videos directory, so it may not be `~/Videos`.

Takes are named `<prefix>-YYYY-MM-DD_HH-MM-SS.<container>`. An empty prefix
uses `reel-`; repeated names get `-2`, `-3`, and so on. JPEG previews are kept
in `$XDG_CACHE_HOME/reel/previews`, not beside the recordings. Previews written
by older Reel versions are moved into the cache when history is first read.

## Optional Hyprland shortcuts

`install.sh` can add these to `~/.config/hypr/bindings.lua`. Answer `y` at the
prompt, run the installer with `--yes`, or add the marked block yourself and
run `hyprctl reload`. Use `--no-binds` to skip them. The generated binds use
keycodes, so they keep working after switching keyboard layouts.

| Shortcut | Action |
| --- | --- |
| `Super + R` | Start or stop a take |
| `Super + Shift + R` | Open or close the Reel panel |
| `Super + Alt + O` | Pause or resume |
| `Ctrl + Alt + =` / `Ctrl + Alt + -` | Zoom in / out during a take |
| `Ctrl + Alt + 0` | Reset zoom to 1× |
| `Ctrl + Alt + 8` | Toggle between 1× and the zoom depth |
| `Ctrl + Alt + Scroll` | Zoom in / out during a take |

The full block, ready to copy, is in [Hyprland bindings](BINDINGS.md).

Re-running the installer replaces the marked block, so an install from before
the zoom commands gains them at the next run. It does not touch the rest of the
file.

## Manual zoom

Manual zoom is enabled by the **Manual zoom** setting (on by default) and only
acts while a take is recording. Pausing or stopping resets the compositor to
1×.

- Touchpad pinch zooms at the pointer and is registered when a take starts.
  Once zoomed in, a pinch that reads as a pan moves the viewport instead.
- `Ctrl + Alt + =` / `-` step by 0.25×, `Ctrl + Alt + 0` resets, and
  `Ctrl + Alt + 8` toggles between 1× and the zoom depth.
- `Ctrl + Alt + Scroll` steps by 0.15× per wheel event.
- **Zoom depth** caps the maximum factor at 1.5×, 2×, 2.5×, or 3×; the engine
  clamps to 1×–4× regardless.

The keyboard and scroll bindings come from the optional shortcut block. Pinch
does not. If the user already had a two-finger pinch gesture, Hyprland rejects a
second one, so Reel removes it for the take, records that it did, and runs
`hyprctl reload` when the take ends to restore the binding from the user's
config.

## Troubleshooting

Start with the diagnostics and the recorder log:

```sh
reel doctor
reel status
tail -f "${XDG_RUNTIME_DIR:-$HOME/.local/state/omarchy/reel/run}/reel/reel.log"
```

- **`reel: command not found`** — run the installed plugin's `bin/configure`,
  or add `~/.local/bin` to your `PATH`.
- **The bar says the native engine is missing** — the plugin is built for
  linux-x86_64. Reinstall it, or build the engine for your machine and place it
  at the plugin's `bin/dist/reel-native`. See [Development](DEVELOPMENT.md).
- **The panel does not appear after an update** — restart the shell:
  `omarchy-restart-shell`.
- **The picker does not open** — `omarchy-capture-region` or `slurp` is
  missing. `reel doctor` lists what it found.
- **`reel start` refuses to start** — it validates the output folder and audio
  devices before opening the picker. Check `reel output` and the error shown in
  the panel.
- **A take has no audio** — takes shorter than 0.8 seconds are saved silent
  rather than failing the loudness pass. For longer takes, check that the
  selected desktop-output or microphone source exists.
- **Slow motion looks like normal speed** — slow motion needs a 120 fps capture.
  Reel raises the capture rate automatically when the setting is enabled.
  Existing takes can be converted with `reel slowmo <file> 0.5`.
- **Zoom shortcuts do nothing** — zoom only acts while a take is recording, and
  the `Ctrl + Alt` binds are optional: re-run `install.sh` after an update and
  accept the shortcut prompt (or add the block by hand). Touchpad pinch does
  not need a bind. Check that **Manual zoom** is on in Settings.
- **Pinch stops working after a take** — Reel unsets its gesture when the take
  ends. If a custom pinch binding was overridden for the take, the config is
  reloaded to restore it; if it is still missing, run `hyprctl reload`.
- **The live preview is black** — the panel uses the compositor's screencopy
  support (`wlr-screencopy` or `ext-image-copy-capture`). On Hyprland this
  should work; restart the shell if the panel was loaded before the compositor
  was ready.
- **The save pass failed** — the original recording is kept and the error is
  written to the log. `reel doctor` checks the tools used by the save pass.

## Uninstall

From the installed plugin directory:

```sh
~/.config/omarchy/plugins/io.github.tahadx.reel/bin/uninstall
omarchy plugin remove io.github.tahadx.reel
```

`bin/uninstall` removes only the `~/.local/bin/reel` link and the marked
shortcut block. Recordings, settings, and runtime state are untouched.
