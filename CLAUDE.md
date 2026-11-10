# CLAUDE.md

Reel is an Omarchy bar plugin for screen recording (`io.github.tahadx.reel`): a QML bar widget and panel backed by a static Rust engine in `crates/reel`. The panel starts, pauses, discards, and manages takes; the engine owns capture, finalize, config, and state.

This repo is the plugin root. `omarchy plugin add` clones it directly into `~/.config/omarchy/plugins/io.github.tahadx.reel/`, so `manifest.json`, `Panel.qml`, `install.sh`, `LICENSE`, `README.md`, `preview.png`, and `bin/` must stay at the repo root.

## Read these first

| File                | What it is                                                                             | When to read it                             |
| ------------------- | -------------------------------------------------------------------------------------- | ------------------------------------------- |
| `docs/design.md`    | **The design system.** Tokens, layout, panel anatomy, motion, copy, interaction rules. | Before any panel or visual change           |
| `docs/reference.md` | CLI, config keys, capture behavior, save pass, troubleshooting.                        | Before changing engine behavior or settings |
| `README.md`         | User-facing features, shortcuts, install/update, marketplace copy.                     | Before changing a user-visible feature      |
| `manifest.json`     | Plugin id, kinds, settings schema, defaults.                                           | Before adding or changing a setting         |

## Omarchy skills

Load only the skill matching your current task before touching code (do not load all of them). If your agent has no skill loader, read `~/.local/share/octomarchy/skills/<name>/SKILL.md` if present locally.

| Area                                              | Skill                  |
| ------------------------------------------------- | ---------------------- |
| Plugin manifest, kinds, entry points, marketplace | `omarchy-plugins`      |
| Omarchy CLI, themes, bar layout, user config      | `omarchy-system`       |
| QML, Quickshell, bar widgets, services            | `qml-quickshell`       |
| Panel and bar UI, spacing, states, motion         | `ui-ux`                |
| Hyprland binds, window rules, monitors            | `hyprland`             |
| JavaScript modules next to QML                    | `javascript-qml`       |
| Marketplace security review                       | `plugin-security`      |
| GitHub, PRs, marketplace issue workflow           | `github`               |
| Branches, commits, plugin updates                 | `git-workflow`         |
| Small diffs, validation, definition of done       | `software-engineering` |

## Architecture: an Omarchy bar plugin

Reel runs **inside `omarchy-shell`**, Omarchy's long-lived Quickshell process, as
the plugin `io.github.tahadx.reel`. It has two entry points: `Panel.qml` (kind
`bar-widget`) and `Service.qml` (kind `service`). `Panel.qml` extends `qs.Ui`'s
`Panel`, so the shell owns the window, summon/hide, and key handling; the panel
is not an app of its own.

QML and the engine talk over **process boundaries**. `Service.qml` runs
`reel status --json` and `reel recent --json --limit 20` on a timer through
Quickshell `Process`, which keeps the bar timer honest while the panel is closed.
The CLI owns the recording; QML only drives it and reads state.

The only socket is the engine's: `$XDG_RUNTIME_DIR/reel/recorder.sock`, which
`reel` uses to drive `gpu-screen-recorder` for pause/resume and live zoom. QML
never opens it.

```text
bar widget / panel    Panel.qml + qs.Ui Panel + qs.Commons
        │  Quickshell Process: reel <command> --json   (no socket on this hop)
        ▼
bin/reel wrapper → bin/dist/reel-native   (Rust engine, crates/reel)
        │  Unix socket: $XDG_RUNTIME_DIR/reel/recorder.sock
        ▼
gpu-screen-recorder / ffmpeg   (capture, then finalize save pass)
        │
        └── config   ~/.config/omarchy/reel.json
            runtime  $XDG_RUNTIME_DIR/reel/{state.json,reel.log,recorder.sock}
            takes    output dir
```

**Keep dependencies near zero:** QML, plain JS, and the shipped Rust engine,
using only what Omarchy already ships (Quickshell, Qt, bash,
`gpu-screen-recorder`, `ffmpeg`). No new QML import, Node runtime, npm package,
or native module without asking first.

To learn the host, read the shell's own code in `$OMARCHY_PATH/shell/`
(`/usr/share/omarchy/shell`):

- `shell.qml` — plugin loading, `summon`/`hide`/`toggle`, and the scoped `shell`
  API
- `Ui/Panel.qml`, `Ui/PanelHero.qml`, `Ui/PanelActionButton.qml`,
  `Ui/PanelController.qml` — the base panel Reel extends; read these before
  changing layout or focus behavior
- `plugins/panels/`, `plugins/bar/` — small plugins built the same way, one with
  a bar widget and a panel
- `Commons/{Style,Color,Util}.qml` — tokens; use these instead of a private
  palette
- `default/agents/skills/omarchy/plugins.md` — plugin safety rules; use
  `textFormat: Text.PlainText` for any dynamic text

Rules that follow from this:

- **State & paths:** user config lives in `~/.config/omarchy/reel.json`; runtime
  state, the recorder socket, and logs live under `$XDG_RUNTIME_DIR/reel/`.
- **Scope:** the panel is the only interface. No timeline, no editor, no separate
  window. Save-pass effects belong in the save pass (`finalize.rs` /
  `render.rs`) and must be re-runnable from a project file once that lands.
- **Never block the shell:** QML must survive an engine error. Parse defensively,
  show `lastError`, never wait synchronously on a command.
- **Static binary:** `bin/dist/reel-native` is built by `bin/refresh-dist` for
  `x86_64-unknown-linux-musl` with pinned linker and path-remap flags. Its
  fingerprint covers `Cargo.toml`, `Cargo.lock`, `rust-toolchain.toml`, and all
  `crates/reel` sources; pre-push and CI verify exact bytes. Never hand-edit it.
  The rebuild step is in Verification below.

## Repo layout

```text
manifest.json            plugin registration, kinds, settings schema
Panel.qml                bar widget, panel, settings, history, take menu
Service.qml, Model.js    shell service and history shaping
Countdown.qml            countdown overlay
install.sh               dependency check, engine install, bar widget, binds
crates/reel/src/         Rust engine (cli.rs, config.rs, recorder.rs, finalize.rs, render.rs)
bin/                     lint-qml, test-qml, refresh-dist, check-release, stamp-version, reel
docs/                    design.md (UI system) and reference.md (CLI/config)
tests/                   shell and QML tests (install.test.sh)
preview.png              marketplace preview (shows the panel only)
```

## Ground rules & hard constraints

1. **Theme is the brand.** Use `Commons.Color.*`, `Style.*`, and `Util.alpha()`. Never add a private palette or hardcoded UI colors.
2. **Fixed frame, moving content.** The hero, actions, and settings tabs never scroll; only the page body scrolls.
3. **Panel, manifest, and engine stay in sync.** Every new setting requires a `manifest.json` schema entry, a `Panel.qml` row, and `crates/reel` support in the same change. Never render a UI row the engine cannot honor.
4. **Pointer behavior is sacred.** `cursor` on keeps today's baked pointer and adds pointer telemetry; `cursor` off keeps today's no-pointer take. There is no capture mode. Cursor synthesis is a render choice (`cursorStyle=baked|synthetic`), and it strips the pointer only after telemetry starts.
5. **Zero new heavy dependencies.** Keep the engine static and self-contained. Do not add `wgpu`, a new workspace crate, or external runtime dependencies without asking first.
6. **No symlinks anywhere.** `omarchy plugin validate .` rejects symlinks inside the plugin tree (`AGENTS.md` must remain a plain file).
7. **Never edit installed files.** Do not touch `/usr/share/omarchy` or `~/.config/omarchy/plugins/io.github.tahadx.reel/` directly. Edit the checkout and run `./install.sh`.
8. **Git & push discipline.** Keep diffs small. Never push, force-push, or rewrite git history unless the user explicitly asks.
9. **Sync docs in the same commit.** Update `docs/design.md` for panel/UI changes, and `docs/reference.md` + `README.md` for CLI, config, or user-facing behavior changes. If docs and code disagree, fix both.

## Security & privacy

- **No shell interpolation:** Never interpolate user text into a shell command or `sh -c`. Pass queries, filenames, paths, and prompts via `stdin` or discrete `argv` elements.
- **Path validation:** Reject empty, `NUL`, and newline values whenever a path is expected.
- **Safe QML text rendering:** Always use `Text.PlainText` for dynamic or user-controlled strings; never use `Text.RichText`.
- **Permissions & network:** Preserve `0700` runtime/preview directory permissions and atomic file writes. No telemetry, analytics, or credential handling. The single network access is the update check (`reel update --check`): a `git fetch` against the plugin's own origin, at most once an hour or when the user asks. Nothing else may make a request.
- **Log & repo hygiene:** Log actions, never payloads, secrets, tokens, or environment dumps. Treat `~/.config/omarchy/reel.json`, recordings, and local notes as private—never commit them or paste them into issues/screenshots.

## Known pitfalls

- **Legacy config keys:** Older `~/.config/omarchy/reel.json` files may contain deprecated keys (`autoZoom`, `followMouse`, `spotlight`, `mode`). Ignore them silently—do not reuse those key names or delete user data.
- **Countdown quarantine:** Countdown captures are saved as `-untrimmed`. Keep them out of the history model and never trim them in place.
- **QML caching:** Opening the panel alone may serve cached QML. Run `omarchy-restart-shell` (or `./install.sh` when manifest/installer changes) before manual testing.
- **PATH shadowing:** `reel` on `PATH` may point to the installed plugin binary rather than your local build. Verify which binary you are invoking.
- **Single live desktop:** `./install.sh`, `omarchy theme set`, and `omarchy-restart-shell` mutate the live desktop session. Never run them from two checkouts simultaneously.

## Verification & Definition of Done

A change is done only when all applicable checks below pass (these mirror the pre-push hook and CI):

```sh
# 1. Rust checks
cargo fmt --check -p reel
cargo clippy -p reel --all-targets -- -D warnings
cargo test --workspace --quiet

# 2. Rebuild static binary (REQUIRED if crates/reel, Cargo.*, or rust-toolchain changed)
bin/refresh-dist

# 3. QML, installer, release, and marketplace validation
bin/lint-qml
bin/test-qml
bash tests/install.test.sh
bin/check-release
omarchy plugin validate .
```

Tests must stay off the live desktop. `bin/test-qml` forces the offscreen Qt
platform, and the test scripts set `REEL_NO_NOTIFY=1` so a failure path cannot
pop a notification. Keep both in any new test path; never let a test open a real
panel or window or call `omarchy-notification-send`.

### Live desktop testing (when UI or shell integration changes)

```sh
# Inspect runtime & CLI state
reel config
reel status
reel config apply '{"cursor":"off"}'

# Capture panel screenshot
omarchy-shell io.github.tahadx.reel open
grim /tmp/reel-panel.png
omarchy-shell io.github.tahadx.reel close
```

- **Surface coverage:** Test the panel, countdown, history, take menu, and settings tabs when modifying shared row or cursor code.
- **Synthetic input:** Only send keys with `wtype` after confirming the panel has focus; early keystrokes leak into the underlying window.
- **Debugging:** Inspect `$XDG_RUNTIME_DIR/reel/reel.log` and `reel status` if a take or panel state misbehaves.
- **Final report:** Always list files changed, commands run, verification evidence, and any deferred work.

## Commits

Every commit must have a description body, not just a subject line.

Subject: `type(area): summary` (50 characters or less). `type` is one of `feat`,
`fix`, `docs`, `refactor`, `test`, `chore`, `build`. `area` is the part touched:
`panel`, `settings`, `history`, `engine`, `finalize`, `recorder`, `cli`, `dist`,
`docs`, `ci`, `install`, `marketplace`. Then the summary. Body: what was wrong,
what the fix does, and how it was verified (build, tests,
`omarchy plugin validate .`, live panel check).

## Release & update flow

- **User update:** `reel update` (or `Shift+U` in the panel) runs `omarchy plugin update io.github.tahadx.reel --yes`, then `./install.sh` from the updated checkout. The installer ends with `omarchy restart shell` so the new QML is loaded.
- **Versioning:** `bin/version` derives the version from the git tag; `bin/stamp-version <semver>` writes it into `manifest.json` and `crates/reel/Cargo.toml`.
- **Cutting a release** (steps 1–4 always; step 5 only when the user asks to publish):
  1. `bin/stamp-version <semver>`
  2. `bin/refresh-dist`
  3. `bin/check-release` — verifies semver, plugin id/schema, entry points, crate version, and ELF validity
  4. Commit and tag `v<semver>`
  5. Push the tag `v<semver>` only when explicitly asked; CI re-stamps from the tag and fails on any mismatch

## Marketplace

- Required at the repo root: `LICENSE`, `README.md`, `manifest.json`, `preview.png`.
- `preview.png` shows the panel only, not the full desktop.
- `omarchy plugin validate .` must pass, and the tree may hold no symlinks.
- No secrets, logs, recordings, or local config in the committed tree.
- The plugin id is `io.github.tahadx.reel`; keep it stable.
- Marketplace verification issue: `omacom/omarchy-plugin-marketplace#13634`.

## Status

- **Shipped:** settings tabs (Recording, Visuals, Shortcuts), fixed tab strip, save-pass noise reduction, `Shift+U` update shortcut, countdown quarantine, history actions (open/cut/share/trash/copy path), live/manual zoom, pause/resume, cursor capture, webcam corner overlay, and bar states.
- **Open:** marketplace verification, maintainer review pending.
