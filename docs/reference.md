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
| `reel trash <file>` | Move a take to the trash |
| `reel config <command>` | Read or write settings |
| `reel preview [--json] [--note-only]` | Capture a still of the focused monitor |
| `reel render --project <path> [--output <path>]` | Render a take again from its project file |
| `reel webcams [--json]` | List the cameras available for the webcam corner |
| `reel output` | Print the recordings folder |
| `reel doctor` | Check tools and print diagnostics |
| `reel update [--binds]` | Update the plugin and rerun its installer |
| `reel update --check [--json] [--force]` | Check whether a newer commit is available |

A take cannot start until the previous take has finished saving: while the
save pass runs, `reel start` and `reel toggle` refuse with a message, the
panel's record button sits dimmed, and every shortcut that would record is
held. A crashed save job is released by the usual reconcile, so the gate never
sticks.

`reel toggle` accepts the same options as `reel start`. `reel stop` fails when
no take is active. `reel trash` accepts only files inside the recordings folder.

### Start options

| Option | Values | Default |
| --- | --- | --- |
| `--desktop-audio` | `on`, `off` | `on` |
| `--microphone-audio` | `on`, `off` | `off` |
| `--noise-reduction` | `on`, `off` | `off` |
| `--cursor` | `on`, `off` | `on` |
| `--webcam` | `off`, `small`, `medium`, `large` | `off` |
| `--webcam-device` | camera path, such as `/dev/video0` | first camera |
| `--countdown` | seconds, clamped to 0–60 | `0` |
| `--fps` | `30`, `60`, `120` | `60` |
| `--quality` | `very_low`, `low`, `medium`, `high`, `very_high`, `ultra` | `very_high` |
| `--codec` | `auto`, `h264`, `hevc`, `av1` | `auto` |
| `--resolution` | `0x0`, `3840x2160`, `2560x1440`, `1920x1080`, `1280x720` | `0x0` |
| `--container` | `mp4`, `mkv` | `mp4` |
| `--output-dir` | folder path; `~` expands | XDG Videos `/Reel` |
| `--filename-prefix` | text prepended to the timestamp | empty (`reel-`) |
| `--manual-zoom` | `on`, `off` | `on` |
| `--auto-zoom` | `on`, `off` | `off` |

`reel update` updates the installed Omarchy plugin, then runs the installer from
the updated checkout. If the Hyprland shortcut block is missing, the installer
asks before adding it. `--binds` adds or refreshes it without prompting. From an
open panel, `Shift+U` runs the same command in a floating terminal so the
updater and its prompt stay visible while the plugin reloads.

Stop an active take before installing or updating. The plugin reload removes the
shell process that keeps the recorder's telemetry listener alive, so the take
would keep recording without pointer data and auto zoom would stop for the rest
of it. The installer refuses while Reel is recording or saving.

`reel update --check` compares the installed checkout with the remote head and
reports whether an update is waiting. It contacts the plugin's git origin, so it
is the only network access Reel makes. The shell service runs it once when it
starts and then once an hour; a result is cached for the hour, so a restart
inside that window reads the cache instead of fetching again. `--force` ignores
the cache, and `--json` prints the result for the panel. An update waiting shows
as an accent dot on the bar mark and a newer version with an accent arrow in the
panel header; that readout checks again when the install is current, and reads
`Retry` when the last check failed.

`0x0` keeps the selected crop or monitor at native size.
`--microphone-audio` also has the alias `--mic`.
`reel preview --note-only` resolves the monitor and note without writing a still.

Examples:

```sh
reel start --countdown 3
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
reel config apply '{"fps": 60}'
```

If the JSON argument to `config apply` is omitted, Reel reads it from stdin.

`webcamDevice` is empty by default, which records the first camera Reel finds.
Settings -> **Webcam device** cycles Auto plus the cameras from `reel webcams`
and stores the camera's `/dev/v4l/by-id` link, which stays the same when cameras
are unplugged or plugged in a different order (a `/dev/videoN` node is stored
only when udev made no link). The list is read when the panel opens and again
each time settings open, so a camera plugged in while the shell runs appears
without a reload. A chosen camera
that is unplugged stays selected and shows as unplugged; a take started while it
is missing uses the first camera found and says so in a notification. The device
only matters while **Webcam corner** is on.

`noiseCancellation` is off by default and appears in Settings → **Media** as
Noise reduction.
When it is on and the take captured the microphone, the save pass applies
FFT-based noise reduction before normalizing the audio. It is a save-pass
filter, so it does not add latency while recording and cannot clean an already
finished take.

**Record the pointer** (`cursor`) controls pointer capture. When it is on, Reel
also records local pointer movement and clicks for later save-pass work. This
needs read access to `/dev/input`; add your user to the `input` group
(`sudo usermod -aG input "$USER"`) and log in again. If the devices cannot be
read, the take still records and simply has no pointer telemetry.

## Auto zoom

**Auto zoom** sits below **Manual zoom** in Settings → **Zoom**, a single
on/off switch that is off by default. The panel sends the switch with every
take, so the take uses the setting you are looking at. When it is on, clicks
during the take become short zoom blocks in the saved file. A touchpad tap counts too when the compositor has
tap-to-click on; a physical click always counts. The magnification is 2×; set
another allowed depth with `reel config set autoZoomDepth 1.5` (or `2.5`). The
switch itself is `reel config set zoomOnClicks on`; `autoZoom` in old files
belongs to the retired live-zoom build and is ignored.

The save pass builds the camera from the take's telemetry, not from the live
zoom:

- Clicks within 2.5 seconds and 350 pixels of each other become one block, and
  the block aims at their centroid.
- If the click landed on a quiet patch while the screen changed in one compact
  place during the block — the text you were typing, the caret blinking next to
  it, a panel that opened — the block follows the change instead. A tap that
  only focuses a window clicks wherever the pointer was parked, so this keeps
  the zoom on the work rather than on an empty area. A click that changes the
  picture where it landed keeps its place.
- Each block eases in over 500 ms before the first click, holds through the
  last click plus 900 ms, and eases out over 500 ms. Velocity is zero at every
  keyframe.
- Blocks that overlap in time stay zoomed and pan between the two areas, so a
  quick click somewhere else does not flash back to 1x for a millisecond. The
  pan takes about 450 ms and ends on the new click; a shift smaller than 12% of
  the visible frame is ignored, so a click already inside the view cannot make
  the camera twitch.
- The viewport is clamped inside the capture, so a zoom never shows empty
  space.
- Keyframe times are saved-take time: the countdown is subtracted, so a zoom
  stays on its click after trimming.

Only takes recorded with **Record the pointer** on have telemetry, so a take
started with the pointer off never zooms. If the telemetry is missing or
unreadable the take saves normally, without a zoom and without a project file.

Every saved take with telemetry writes `<take-stem>.project.json` into a hidden
`.reel/` folder beside the video, even when auto zoom is off; that file holds
the capture facts and the camera path. The raw log is kept beside it as
`<take-stem>.telemetry.jsonl`. The folder is owner-only and both files are
removed when the take is trashed. Edit the project and run
`reel render --project <path>` to change the zoom without recording again; see
[Re-rendering a take](#re-rendering-a-take).

## Re-rendering a take

A saved take with pointer telemetry carries a `project.json` in the hidden
`.reel/` folder beside the video. It is the render contract: `reel render` reads
it instead of the live settings, so a hand edit is all it takes to change the
camera and render again.

```sh
reel render --project ~/Videos/Reel/.reel/reel-2026-11-03_12-00-00.project.json
reel render --project .reel/take.project.json --output take-wide.mp4
```

The default output is `<take-stem>.rendered.<ext>`, next to the take. A
re-render never replaces the original file.

The `camera` object holds the zoom path. `style` is `off` or `auto` (projects
written before the modes were folded away may say `subtle`, `cinematic`, or
`close`; they all read as `auto`). `depth` is the magnification (`1.5`, `2.0`,
or `2.5`), and `keyframes` is an ordered list of `t`, `zoom`, `x`, `y`:

```json
"camera": {
  "style": "auto",
  "depth": 2.0,
  "easing": "smoothstep",
  "keyframes": [
    { "t": 0.0, "zoom": 1.0, "x": 1280.0, "y": 720.0 },
    { "t": 1.5, "zoom": 2.0, "x": 1900.0, "y": 620.0 },
    { "t": 3.7, "zoom": 1.0, "x": 1280.0, "y": 720.0 }
  ]
}
```

`t` is seconds in the saved take. `x` and `y` are the viewport centre in source
capture pixels. Keyframes are sorted by `t`, `zoom` is at least `1`, and every
viewport is clamped so the frame never shows space outside the capture. Values
between keyframes use smoothstep, so the camera is still at each keyframe.
`style: "off"` disables the zoom even when keyframes are present.

Unknown fields are preserved, and missing fields fall back to the v1 defaults.
`trimStart` is only used by the hidden save pass; a re-render reads the saved
take and does not trim it again.

When the project is missing or has no camera keyframes, `reel render` encodes
the take without a zoom. It uses the same audio filter and validation as the
save pass, and the same project and inputs produce the same bytes.

## Save pass

A take is written by the encoder to `<take>.capture.<ext>`, then finished: the
first frame is dropped (or the countdown is trimmed), the camera from auto zoom
is applied, the PipeWire pop is muted, the audio is normalized, and noise
reduction runs when it was on. The ordinary `<take>.<ext>` name is published
only when that pass succeeds, so history never shows an un-zoomed capture.
If the camera pass itself fails, Reel retries with the trim and loudness pass
and still publishes the take without a zoom; the failure stays in the log.

When the capture's first GOP is flagged for discard, only the head of the video
is re-encoded, up to the next keyframe. Everything after that is stream-copied
and joined with the processed audio, so a long take does not re-encode the
whole file just to trim its head.

## Runtime files

| Path | Contents |
| --- | --- |
| `~/.config/omarchy/reel.json` | Settings shared by the panel and CLI |
| `$XDG_RUNTIME_DIR/reel/state.json` | Live state the bar watches |
| `$XDG_RUNTIME_DIR/reel/last.json` | Metadata for the last completed take |
| `$XDG_RUNTIME_DIR/reel/reel.log` | Recorder and save-pass log; kept across takes and rotated to `reel.log.1` at 4 MB |
| `$XDG_RUNTIME_DIR/reel/update.json` | Cached update check, trusted for one hour |
| `$XDG_RUNTIME_DIR/reel/recorder.sock` | Pause/resume IPC to the recorder |
| `$XDG_RUNTIME_DIR/reel/session.telemetry.jsonl` | Pointer telemetry while Record the pointer is on |
| `$XDG_RUNTIME_DIR/reel/next-preview-*.jpg` | Still produced by `reel preview` |
| `$XDG_RUNTIME_DIR/reel/render-*.sendcmd` | Generated camera script; removed after a successful render |
| `$XDG_CACHE_HOME/reel/previews` | History and notification thumbnails |
| `~/.local/state/omarchy/reel/pending-quarantine.json` | Captures withheld from history until their countdown is moved aside |
| `~/Videos/Reel` | Default recordings folder |
| `~/Videos/Reel/.reel` | Hidden project and telemetry sidecars for saved takes |

If `XDG_RUNTIME_DIR` is unset, the runtime files live under
`~/.local/state/omarchy/reel/run` instead. The default output folder follows
the XDG Videos directory, so it may not be `~/Videos`.

Takes are named `<prefix>-YYYY-MM-DD_HH-MM-SS.<container>`. While a take is
being recorded and saved, its raw file is `<prefix>-YYYY-MM-DD_HH-MM-SS.capture.<container>`
and the ordinary name appears at the end of the save pass. An empty prefix
uses `reel-`; repeated names get `-2`, `-3`, and so on. When pointer telemetry
is on, Reel records it in the runtime folder and copies it into the hidden
`.reel` folder beside the saved video as `<take-stem>.telemetry.jsonl`, with the
render contract beside it as `<take-stem>.project.json`. JPEG previews are kept
in `$XDG_CACHE_HOME/reel/previews`, not beside the recordings. Previews written
by older Reel versions are moved into the cache when history is first read.

## Optional Hyprland shortcuts

`install.sh` can add these to `~/.config/hypr/bindings.lua`. Answer `y` at the
prompt, run the installer with `--yes`, or add the marked block yourself and
run `hyprctl reload`. Use `--no-binds` to skip them. The generated binds use
key names, so they follow the active keyboard layout.

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

The full block, ready to copy, is in [Hyprland bindings](bindings.md).

Re-running the installer replaces the marked block, so an install from before
the zoom commands gains them at the next run. It does not touch the rest of the
file.

## Manual zoom

Manual zoom is enabled by the **Manual zoom** setting (on by default) and only
acts while a take is recording. Pausing or stopping resets the compositor to
1×. Zoom depth and pan step have no panel rows; they stay config-only.

- Touchpad pinch zooms at the pointer and is registered when a take starts.
  Once zoomed in, a pinch that reads as a pan moves the viewport instead.
- `Ctrl + Alt + =` / `-` step by 0.25×, `Ctrl + Alt + 0` resets, and
  `Ctrl + Alt + 8` toggles between 1× and the zoom depth.
- `Ctrl + Alt + Scroll` steps by 0.15× per wheel event.
- Zoom depth is 2× by default. `reel config set zoomMax 1.5` (or `2.5`, `3`)
  changes it; pinch, keyboard steps, and the toggle all stop at that depth. The
  engine clamps to 1×–4× regardless. Raising or lowering the depth never snaps
  an existing zoom back on its own.

### Panning

Follow mode is for **monitor** takes. While zoomed, the camera is attached to
the pointer: moving the mouse pans the view and keeps the cursor near the centre
of the crop. This is a tight, one-to-one pan with no smoothing, so it cannot
feel floaty.

Window and area takes keep the camera detached while zoomed. The pointer keeps
its real position, so a take of a small window in a corner still shows the
pointer inside the frame instead of pulling it toward the screen centre. Pinch
zoom still works there; only panning is monitor-only, and `zoom-pan` does
nothing for a window or area.

- `Ctrl + Alt + ←/→/↑/↓` pans from the keyboard. A tap moves 60 px by default
  (`reel config set panStep 30` or `120` changes it), holding keeps panning at
  a steady speed, and `Shift` covers three times as much. The view moves
  immediately and the cursor stays over the point being panned toward. A hold
  stops as soon as the zoom returns to 1×.
- Panning never changes the zoom factor, so the depth stays locked.
- The pan target is clamped to the recorded monitor, so the view cannot walk off
  the capture. The compositor additionally clamps it to the monitor.
- Returning to 1×, pausing, or stopping restores the detached camera, which is
  the normal desktop state.
- A pinch that reads as a pan uses the same pointer movement, so gesture zoom
  and manual pan do not fight each other.

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
  at the plugin's `bin/dist/reel-native`. See [Development](development.md).
- **The panel does not appear after an update** — restart the shell:
  `omarchy-restart-shell`.
- **The picker does not open** — `omarchy-capture-region` or `slurp` is
  missing. `reel doctor` lists what it found.
- **`reel start` refuses to start** — it validates the output folder and audio
  devices before opening the picker. Check `reel output` and the error shown in
  the panel.
- **A save-pass effect has no pointer data** — pointer telemetry could not read
  `/dev/input`. Add your user to the `input` group and log in again. The reason
  is written to the log and the take stays usable.
- **A take has no audio** — takes shorter than 0.8 seconds are saved silent
  rather than failing the loudness pass. For longer takes, check that the
  selected desktop-output or microphone source exists.
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
- **The update mark never appears** — checks run at most once an hour and need
  network access to the plugin's git origin. Run `reel update --check` to see
  the current answer or the connection error.
- **The save pass failed** — the original recording is kept and the error is
  written to the log. A take recorded with a countdown stays under an
  `-untrimmed` name, and history skips it, until the trim succeeds; the error
  names the file so the overlay footage is never offered as an ordinary take.
  A countdown take started by an older Reel and stopped after an engine update
  is moved to the same `-untrimmed` name before the save pass runs.
  If the recordings folder is read-only and that move is denied, Reel leaves the
  raw file at its ordinary name but records it as withheld, so history and Share
  skip it. The entry clears, and the file moves aside, on the first check after
  the folder is writable again.
  `reel doctor` checks the tools used by the save pass.

## Uninstall

From the installed plugin directory:

```sh
~/.config/omarchy/plugins/io.github.tahadx.reel/bin/uninstall
omarchy plugin remove io.github.tahadx.reel
```

`bin/uninstall` removes only the `~/.local/bin/reel` link and the marked
shortcut block. Recordings, settings, and runtime state are untouched.
