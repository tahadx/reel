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
| `reel audio-devices [--json]` | List audio capture devices available to the active recorder |
| `reel trash <file>` | Move a take to the trash |
| `reel thumbnails [--limit N]` | Generate missing thumbnails for recent takes |
| `reel rename <file> <name>` | Rename a take, keeping its preview and sidecars |
| `reel config <command>` | Read or write settings |
| `reel preview [--json] [--note-only]` | Capture a still of the focused monitor |
| `reel render --project <path> [--output <path>]` | Render a take again from its project file |
| `reel webcams [--json]` | List the cameras available for the webcam corner |
| `reel output` | Print the recordings folder |
| `reel doctor` | Check tools and print diagnostics |
| `reel update [--binds]` | Update the plugin, rerun its installer, and restart the shell |
| `reel update --check [--json] [--force]` | Check whether a newer commit is available |

A finished take does not own the engine while it is saved. `reel stop` hands
the take to its own save job and returns, so `reel start` and `reel toggle` can
begin the next take immediately; any number of save jobs can run beside the
recorder. `reel status --json` lists live jobs under `saves`, and `reel status`
reports `Saving` while at least one job is still running. A crashed save job is
released by the usual reconcile, so the panel never keeps a stale saving row.

`reel toggle` accepts the same options as `reel start`. `reel stop` fails when
no take is active. `reel trash` accepts only files inside the recordings folder.
`reel rename` keeps the recording extension, refuses names that already exist or
belong to another folder, and moves the preview, telemetry, project, zoom, and
rendered files with the take. Only finished takes can be renamed. Names ending
in `-untrimmed` or containing `.capture` or `.part.` are reserved.

### Start options

| Option | Values | Default |
| --- | --- | --- |
| `--target` | `portal`, `monitor:NAME`, or `region:WxH+X+Y` | picker |
| `--desktop-audio` | `on`, `off` | `on` |
| `--microphone-audio` | `on`, `off` | `off` |
| `--noise-reduction` | `on`, `off` | `off` |
| `--desktop-device` | audio device ID from `reel audio-devices --json` | empty (`default_output`) |
| `--microphone-device` | audio device ID from `reel audio-devices --json` | empty (`default_input`) |
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
| `--click-effects` | `off`, `ripple`, `halo` | `off` |

`reel update` updates the installed Omarchy plugin, then runs the installer from
the updated checkout. The installer restarts the shell so the new QML is loaded.
If the Hyprland shortcut block is missing, the installer asks before adding it.
`--binds` adds or refreshes it without prompting. From an open panel, `Shift+U`
runs the same command in a floating terminal so the updater and its prompt stay
visible while the plugin reloads. The update fails with a clear message rather
than claiming success if the shell cannot be restarted.

Stop an active take before installing or updating. The plugin reload removes the
shell process that keeps the recorder's telemetry listener alive, so the take
would keep recording without pointer data and auto zoom would stop for the rest
of it. The installer refuses while Reel is recording or saving.

`reel update --check` compares the installed checkout with the remote head and
reports whether an update is waiting. It contacts the plugin's git origin, so it
is the only network access Reel makes. If the effective origin URL (after git's
`insteadOf` rewrites) embeds a credential or a query token, the check refuses to
fetch: git would put that URL in the remote helper's argv, where another local
account could read it. Git's remote-helper URL forms (`https::…`, `ext::…`)
are refused too, since the background check has no reason to use them. Keep
secrets in a git credential helper. The shell service runs the check once when
it starts and then once an hour; a result is
cached for the hour, so a restart inside that window reads the cache instead of
fetching again. `--force` ignores
the cache, and `--json` prints the result for the panel. An update waiting shows
as an accent dot on the bar mark and a newer version with an accent arrow in the
panel header; that readout checks again when the install is current, and reads
`Retry` when the last check failed.

`0x0` keeps the selected crop or monitor at native size.
`--microphone-audio` also has the alias `--mic`.
`reel preview --note-only` resolves the monitor and note without writing a still.

`--target` skips the picker. Without it, Reel opens the picker, and an output
that `gpu-screen-recorder --list-capture-options` does not list as a KMS
monitor falls back to `portal`: the system dialog chooses the source, and an
area take records the whole output. This is the path for a headless Hyprland
output created with `hyprctl output create headless`. `--target portal` selects
it explicitly, which is also useful in scripts.

Examples:

```sh
reel start --countdown 3
reel start --microphone-audio on
reel start --target portal
reel toggle-pause
reel stop
```

## Capture backends

Reel records with `gpu-screen-recorder` when it is installed. If it is missing,
or it exits before producing a frame — a GPU the recorder does not support, for
example — Reel falls back to `wf-recorder` and encodes on the CPU, the same
order Omarchy's own recorder uses. The take then goes through the same save
pass as a GPU take.

The CPU fallback has a narrower capture surface:

- `--target portal` is GPU-only. Monitor and area takes work.
- Audio is one stream. With desktop and microphone both on, Reel mixes them
  through a temporary null sink so the take still carries both; the sink is
  unloaded when the take stops.
- `--resolution` is GPU-only. The fallback records at native size.
- Pause, resume, and the recorder IPC socket are unavailable. The panel labels
  the control **No pause** for that take, and `reel pause` says so.
- **Record the pointer** still drives telemetry, and **Manual zoom** still
  works because it runs in the compositor. `wf-recorder` always includes the
  pointer in its capture, so a fallback take has the pointer baked in even when
  **Record the pointer** is off.
- Quality presets map to libx264, libx265, or libsvtav1 with a CRF; expect a
  larger file than a hardware take at the same preset.

On AMD Polaris GPUs, even a `gpu-screen-recorder` take is encoded on the CPU:
that generation's VCE ring can hang mid-recording and take the compositor's
VRAM with it, so Reel avoids starting hardware encoding there.

`reel doctor` reports which recorder Reel will use. The take's log line starts
with `capture: backend=...`.

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

The panel's filename-prefix and rename fields accept typing and bounded paste
only. Every standard paste binding, including the dedicated Paste key, is
intercepted before Qt's native handling. Paste runs `bin/clipboard`, which
stops at 4096 bytes with a hard deadline; the field then refuses line breaks,
control characters, and text over 200 characters. Qt's native paste is
intercepted so an oversized or streaming clipboard never enters the shell
process.

`webcamDevice` is empty by default, which records the first camera Reel finds.
Settings -> **Input devices** -> **Camera** lists the first available camera
plus the cameras from `reel webcams` and stores the camera's `/dev/v4l/by-id`
link, which stays the same when cameras are unplugged or plugged in a different
order (a `/dev/videoN` node is stored only when udev made no link). The list is
read when the panel opens, when settings open, and whenever the Devices page
opens, so a camera plugged in while the shell runs appears without a reload. A
chosen camera that is unplugged stays selected and shows as unplugged; a take
started while it is missing uses the first camera found and says so in a
notification. The device only matters while **Webcam corner** is on.

`noiseCancellation` is off by default and appears in Settings → **Recording** as
Noise reduction.
When it is on and the take captured the microphone, the save pass applies
FFT-based noise reduction before normalizing the audio. It is a save-pass
filter, so it does not add latency while recording and cannot clean an already
finished take.

**Record the pointer** (`cursor`) lives in Settings → **Visuals** and controls
pointer capture. When it is on, Reel
also records local pointer movement and clicks for later save-pass work. This
needs read access to `/dev/input`; add your user to the `input` group
(`sudo usermod -aG input "$USER"`) and log in again. If the devices cannot be
read, the take still records and simply has no pointer telemetry.

## Auto zoom

**Auto zoom** sits below **Manual zoom** in Settings → **Visuals**, a single
on/off switch that is off by default. The panel sends the switch with every
take, so the take uses the setting you are looking at. When it is on, clicks
during the take become short zoom blocks in the saved file. A touchpad tap counts too when the compositor has
tap-to-click on; a physical click always counts. The magnification is 2×; set
another allowed depth with `reel config set autoZoomDepth 1.5` (or `2.5`). The
switch itself is `reel config set zoomOnClicks on`; `autoZoom` in old files
belongs to the retired live-zoom build and is ignored.

The save pass builds the camera from the take's telemetry, not from the live
zoom:

- A press starts a block. Every button down inside the capture is a zoom
  anchor, so a text selection, a window drag, or a slider scrub starts a zoom
  that follows the pointer. Left, right, and middle presses all count.
- A click outside the recorded monitor, window, or area is ignored completely.
  Window and area takes use the encoded rectangle, so moving the window during
  the take cannot make an outside click pull the camera into the frame.
- Clicks within 2.5 seconds and 350 pixels of each other become one block, and
  the block aims at their centroid.
- If the click landed on a quiet patch while the screen changed in one compact
  place during the block — the text you were typing, the caret blinking next to
  it, a panel that opened — the block follows the change instead. A tap that
  only focuses a window clicks wherever the pointer was parked, so this keeps
  the zoom on the work rather than on an empty area. A click that changes the
  picture where it landed keeps its place.
- Pointer movement after a press is followed continuously, which is what keeps
  a drag — selecting text, moving a window, drawing a line — inside the zoomed
  frame until the pointer settles.
- Scrolling inside the zoomed viewport keeps the block alive. Each wheel or
  touchpad tick holds the camera until 1.2 s after the last tick, so reading
  through a page does not zoom out mid-scroll.
- Each block eases in with the same critically damped spring as manual zoom:
  a 0.11 s half-life settling over up to 1.2 s. The move starts up to 1.2 s
  before the first click, so the click lands as the spring comes to rest rather
  than at the end of a 500 ms snap. It then holds through the last click plus
  900 ms and eases back over as much of the gap before the next block as
  fits, up to 1.2 s, leaving a short rest at 1x when there is room. The click
  timing and depth are unchanged. Velocity is zero at every keyframe.
- Blocks that overlap in time stay zoomed and pan between the two areas, so a
  quick click somewhere else does not flash back to 1x for a millisecond. The
  pan takes about 450 ms and ends on the new click; a shift smaller than 12% of
  the visible frame is ignored, so a click already inside the view cannot make
  the camera twitch.
- The viewport is clamped inside the capture at every depth, including a click
  on the top-left or bottom-right pixel. A click exactly on the right or bottom
  edge is outside the half-open frame and is ignored rather than producing a
  negative crop.
- Keyframe times are saved-take time: the countdown is subtracted, so a zoom
  stays on its click after trimming.
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

## Click effects

**Click effects** sits below **Auto zoom** in Settings -> **Visuals**. It is off
by default; the choices are **Ripple** and **Halo**. The panel writes the choice
with every take (`reel config set clickEffects ripple` also sets it), and the
save pass draws the effect at each short tap (whether or not auto zoom is on):
a left or right release within 280 ms and 15 px, inside the capture. A drag
zooms but does not draw an effect. Touchpad taps count when tap-to-click is on.
Clicks outside a window or area take are ignored, so the effect is never drawn
outside the recorded rectangle.

- **Ripple** is the signature effect: a soft accent halo, a crisp accented ring
  with a dark hairline, a thin trailing accent echo, and a lighter accent core
  that fades first.
- **Halo** is a soft, lens-like accent bloom with no hard ring. It suits calm
  demos.

The effect color comes from the active Omarchy theme's `accent`, read from
`~/.local/state/omarchy/current/theme/colors.toml` (or the older
`~/.config/omarchy/current/theme/colors.toml`). The accent ring and its dark
outline keep the effect legible on both light and dark content. The resolved
color is saved in the project so a later re-render keeps the look it was
recorded with, even after a theme switch.

Click effects are built from the same pointer telemetry as auto zoom, so they
need **Record the pointer** on. Without `/dev/input` access the take still saves,
just without effects. Effects and auto zoom can be used together: the shapes are
composited before the camera pass, so a zoom moves and scales the effect with
the picture under it.

When **Manual zoom** is on, the compositor has already magnified the recorded
frames. Reel mirrors the live zoom factor into the runtime folder so telemetry
can stamp each press. A monitor take uses the attached loose camera, so the
clicked content stays under the pointer and the click is already at the
position the recorded frame shows; the save pass uses that position directly
for click effects and auto zoom. Window and area takes keep a detached camera
whose position telemetry does not record, so their points stay at their real
positions rather than being guessed. A take that opts into the stable
`zoomFollow push` camera has no save-pass clicks unless **Auto zoom** or **Click
effects** is on; those settings force the attached camera for that take.

Telemetry starts at encoder launch, but the video starts at its first captured
frame. A cold encoder can take seconds to produce that frame, so the first
frame's timestamp is written into the telemetry and the save pass subtracts the
gap. A click is therefore placed on the frame it happened on, not several
seconds later on different content.

The project stores the effect under `ripples`:

```json
"ripples": {
  "enabled": true,
  "style": "ripple",
  "color": "#8a9a7b",
  "clicks": [
    { "t": 0.8, "x": 1900.0, "y": 620.0 },
    { "t": 2.4, "x": 1280.0, "y": 720.0 }
  ]
}
```

`style` is `ripple` or `halo`; an unknown style, the retired `pulse` style, and
`enabled: false` all render nothing. `color` is a `#rrggbb` accent (empty uses
the live theme), and `clicks` are saved-take seconds with source capture pixels.
Edit any of these and run `reel render` to change the effect without recording
again. The save pass removes its generated ASS script after a successful
render; it is not a runtime file you need to manage.

## Re-rendering a take

A saved take with pointer telemetry carries a `project.json` in the hidden
`.reel/` folder beside the video. It is the render contract: `reel render` reads
it instead of the live settings, so a hand edit is all it takes to change the
camera and click effects and render again.

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
  "easing": "spring",
  "keyframes": [
    { "t": 0.0, "zoom": 1.0, "x": 1280.0, "y": 720.0 },
    { "t": 1.5, "zoom": 2.0, "x": 1900.0, "y": 620.0 },
    { "t": 3.7, "zoom": 1.0, "x": 1280.0, "y": 720.0 }
  ]
}
```

`t` is seconds in the saved take. `x` and `y` are the viewport centre in source
capture pixels. Keyframes are sorted by `t`, `zoom` is at least `1`, and every
viewport is clamped so the frame never shows space outside the capture. New
auto zoom writes `easing: "spring"`: values between keyframes use the manual
zoom's critically damped spring curve, and the camera is still at every
keyframe. A normal move spans up to 1.2 s so it reads like the live spring;
shorter gaps use a shorter span. Projects written before the spring (and any
unknown easing value) keep the historical `"smoothstep"` path. `style: "off"`
disables the zoom even when keyframes are present.

The `ripples` object holds the click effects: `enabled`, `style`
(`ripple` or `halo`), `color` (a `#rrggbb` accent, or empty for the live
theme), and `clicks` (`t`, `x`, `y` in saved-take time and source pixels). The
save pass writes the resolved accent into `color`, so a re-render keeps the
look it was recorded with. See [Click effects](#click-effects) for the styles
and the exact click rules.

Unknown fields are preserved, and missing fields fall back to the v1 defaults.
`trimStart` is only used by the hidden save pass; a re-render reads the saved
take and does not trim it again.

When the project is missing or has no camera keyframes and no clicks, `reel
render` encodes the take without a zoom or effects. It uses the same audio
filter and validation as the save pass, and the same project and inputs produce
the same bytes.

## Save pass

A take is written by the encoder to `<take>.capture.<ext>`, then finished: the
first frame is dropped (or the countdown is trimmed), the click effects and the
camera from auto zoom are applied, the PipeWire pop is muted, the audio is
normalized, and noise reduction runs when it was on. The ordinary
`<take>.<ext>` name is published only when that pass succeeds, so history never
shows an un-zoomed capture.
If the render pass itself fails, Reel retries with the trim and loudness pass
and still publishes the take without the zoom or effects; the failure stays in
the log.

When the capture's first GOP is flagged for discard, only the head of the video
is re-encoded, up to the next keyframe. Everything after that is stream-copied
and joined with the processed audio, so a long take does not re-encode the
whole file just to trim its head.

## Runtime files

| Path | Contents |
| --- | --- |
| `~/.config/omarchy/reel.json` | Settings shared by the panel and CLI |
| `$XDG_RUNTIME_DIR/reel/state.json` | Live state the bar watches |
| `$XDG_RUNTIME_DIR/reel/saves` | One small record per running save pass |
| `$XDG_RUNTIME_DIR/reel/last.json` | Metadata for the last completed take |
| `$XDG_RUNTIME_DIR/reel/reel.log` | Recorder and save-pass log; kept across takes and rotated to `reel.log.1` at 4 MB |
| `$XDG_RUNTIME_DIR/reel/update.json` | Cached update check, trusted for one hour |
| `$XDG_RUNTIME_DIR/reel/recorder.sock` | Pause/resume IPC to `gpu-screen-recorder` |
| `$XDG_RUNTIME_DIR/reel/pa.modules` | Temporary PulseAudio modules for the CPU fallback's combined audio |
| `$XDG_RUNTIME_DIR/reel/session.telemetry.jsonl` | Pointer telemetry while Record the pointer is on |
| `$XDG_RUNTIME_DIR/reel/zoom-factor` | Live manual zoom factor mirrored for telemetry |
| `$XDG_RUNTIME_DIR/reel/next-preview-*.jpg` | Still produced by `reel preview` |
| `$XDG_RUNTIME_DIR/reel/render-*.sendcmd` | Generated camera script; removed after a successful render |
| `$XDG_RUNTIME_DIR/reel/render-*.ass` | Generated click-effect script; removed after a successful render |
| `~/.reel/previews` | History and notification thumbnails; survives cache cleanup |
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
in `~/.reel/previews`, not in the cache directory and not beside the recordings,
so clearing the cache no longer removes history thumbnails. Previews written by
older Reel versions, including ones left in `$XDG_CACHE_HOME/reel/previews`, are
moved into that folder on the first run after upgrading. Previews lost before
that change are rebuilt from the recording: the panel runs `reel thumbnails`
when History opens. Removing a take also removes its preview, even when the
recording was already deleted from disk.

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
| `Ctrl + Scroll` | Zoom in / out during a take (built in) |
| `Ctrl + Alt + Scroll` | Zoom in / out during a take, from the optional block |
| `Ctrl + Alt + ←/→/↑/↓` | Pan the zoomed view during a take (hold to keep moving) |
| `Ctrl + Alt + Shift + ←/→/↑/↓` | Pan three times as far |

The full block, ready to copy, is in [Hyprland bindings](bindings.md).

Re-running the installer replaces the marked block, so an install from before
the zoom commands gains them at the next run. It does not touch the rest of the
file.

## Manual zoom

Manual zoom is enabled by the **Manual zoom** setting (on by default) and only
acts while a take is recording. Pausing or stopping resets the compositor to
1×. Zoom depth, follow mode, and pan step have no panel rows; they stay
config-only.

- Touchpad pinch zooms at the pointer and is registered when a take starts.
  Once zoomed in, a pinch that reads as a pan moves the viewport instead.
- `Ctrl + Alt + =` / `-` step by 0.25×, `Ctrl + Alt + 0` resets, and
  `Ctrl + Alt + 8` toggles between 1× and the zoom depth.
- `Ctrl + Scroll` zooms in and out the way the pinch does: each notch moves a
  shared target and a 16 ms in-compositor timer eases the applied factor toward
  it. Reel registers the binds for the take only, so Ctrl+scroll keeps its
  normal meaning in applications outside a take. `Ctrl + Alt + Scroll` steps by
  0.15× per wheel event when the optional shortcut block is installed.
- Scale changes share one critically damped spring: wheel notches, key steps,
  pinch, toggle, and reset move a target, and a 16 ms timer advances the spring.
  The half-life is 110 ms for a wheel notch, 150 ms for a key step, 60 ms for a
  pinch, and 160 ms for reset. A new input changes only the destination and
  keeps the momentum; a reversal stops the old motion cleanly instead of
  overshooting. The Rust fallback, used only when the compositor controller is
  not installed, uses the same shape with the older 180/260 ms half-lives.
- `zoomFollow` is `follow` by default and has no panel row. `follow` is the
  attached loose camera: the content under the pointer stays under it, so the
  cursor keeps moving normally instead of being pinned to the middle, and the
  view follows at `(zoom - 1) / zoom` of the pointer's movement. `reel config
  set zoomFollow push` opts into the detached camera that holds the frame still
  and pans only when the pointer reaches the edge. A take with **Auto zoom** or
  **Click effects** on always uses the attached camera, because the save pass
  needs to place those clicks in the recorded frame exactly.
- Manual zoom owns the camera while it is above 1×. An automatic timeout may
  not reset it; only `Ctrl + Alt + 0`, or stepping manually back to 1×, returns
  control to the automatic camera. The save-pass auto zoom is a separate
  keyframe path and never touches the live compositor factor.
- Zoom depth is 2× by default. `reel config set zoomMax 1.5` (or `2.5`, `3`)
  changes it; pinch, keyboard steps, and the toggle all stop at that depth. The
  engine clamps to 1×–4× regardless. Raising or lowering the depth never snaps
  an existing zoom back on its own.

### Panning

The default camera is **follow** for monitor takes. While zoomed, Hyprland
keeps the camera attached but **loose**: the content under the pointer stays
under it, so the cursor moves normally on screen and the view follows at
`(zoom - 1) / zoom` of the pointer's movement — half at 2×. The pointer is not
pulled to the middle of the crop. The new spring smooths the zoom depth, and the
camera moves with it as the frame glides in.

Set `zoomFollow push` for the stable detached camera instead. The frame holds
still while the pointer moves and pans only when the pointer reaches the edge,
so the picture never chases every mouse move.

Window and area takes always keep the camera detached while zoomed. The pointer
keeps its real position, so a take of a small window in a corner still shows the
pointer inside the frame instead of pulling it toward the screen centre. Pinch
zoom still works there; only panning is monitor-only, and `zoom-pan` does
nothing for a window or area.

- `Ctrl + Alt + ←/→/↑/↓` pans from the keyboard in **follow** mode. A tap pans
  the view about 60 px at the default 2× (`reel config set panStep 30` or `120`
  changes it), holding keeps panning at a steady speed, and `Shift` covers three
  times as much. The pointer moves far enough that the loose camera travels the
  whole step; at a very shallow zoom the pointer step is capped, so the view
  moves a little less. A hold stops as soon as the zoom returns to 1×. The push
  camera has no attached pan; move the pointer to the edge to pan.
- Panning never changes the zoom factor, so the depth stays locked.
- The pan target is clamped to the recorded monitor, so the view cannot walk off
  the capture. The compositor additionally clamps it to the monitor.
- Returning to 1×, pausing, or stopping restores the detached camera, which is
  the normal desktop state.
- In **follow** mode, a pinch that reads as a pan uses the same pointer movement
  and camera, so gesture zoom and manual pan do not fight each other. In push
  mode a pinch only zooms.

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
- **The installer stopped at `== Reel: plugin ==`** — older builds exited
  silently when `omarchy-shell` was not running. Update to the current installer
  and re-run it; it now finishes the install, restarts the shell, and verifies
  the plugin afterwards. If the shell still cannot answer, it exits with the
  failing command and the `omarchy plugin enable` line to run.
- **The picker does not open** — `omarchy-capture-region` or `slurp` is
  missing. `reel doctor` lists what it found.
- **`reel start` refuses to start** — it validates the output folder and audio
  devices before opening the picker. Check `reel output` and the error shown in
  the panel.
- **A take is recording with the CPU fallback** — `gpu-screen-recorder` is
  missing or could not capture this GPU, so Reel is using `wf-recorder`.
  `reel doctor` and the take's `capture: backend=...` log line confirm it.
  Pause/resume, portal targets, and resolution limits are unavailable on that
  path; a GPU take uses them again.
- **Takes fail on a headless output** — KMS capture only sees real DRM
  connectors, so a headless output has nothing to capture. Reel detects an
  output missing from `gpu-screen-recorder --list-capture-options` and records
  through the desktop portal instead: choose the output in the system dialog,
  and an area take becomes a full-output take. If the portal is unavailable,
  enable xdg-desktop-portal screen sharing or use a physical output. To skip the
  picker entirely, run `reel start --target portal`. Portal capture needs
  `gpu-screen-recorder`; the CPU fallback cannot use it.
- **A save-pass effect has no pointer data** — pointer telemetry could not read
  `/dev/input`. Add your user to the `input` group and log in again. The reason
  is written to the log and the take stays usable.
- **Click effects do not appear** — they are built from the same telemetry as
  auto zoom, so **Record the pointer** must be on and `/dev/input` readable. A
  drag, hold, or click outside the capture does not qualify. If the effect
  render itself failed, the log says so and the take is published without the
  effect rather than losing the camera or the footage.
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
