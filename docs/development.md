# Development

Reel is a Rust engine plus an Omarchy QML plugin. The plugin ships a prebuilt
engine, so a normal install does not need Rust.

## Layout

- `Cargo.toml` and `crates/reel/` — the engine and CLI.
- `Panel.qml`, `GuardedInput.qml`, `Service.qml`, and `Model.js` — the bar
  widget, bounded-paste text field, background service, and pure JavaScript
  helpers.
- `bin/dist/reel-native` — the committed x86_64 musl engine.
- `bin/dist/reel-native.fingerprint` — source fingerprint for the local hook.
- `bin/refresh-dist` — rebuilds the committed engine from this tree.
- `bin/clipboard` — the bounded clipboard reader used by the name fields.
- `THIRD-PARTY.md` and `licenses/` — notices for the linked engine.
- `bin/lint-qml`, `bin/test-qml`, and `tests/` — QML and installer checks.

## Build and test

The Rust toolchain is pinned in `rust-toolchain.toml`. Rustup installs it. The
shipped build also needs the `x86_64-unknown-linux-musl` target.

```sh
cargo build --release
cargo test --workspace
bin/lint-qml
bin/test-qml
bash tests/install.test.sh
```

`cargo build --release` builds a host binary for local work. `bin/lint-qml`
checks QML syntax; unresolved Omarchy imports are expected outside the shell.
`bin/test-qml` runs the QML tests offscreen. `tests/install.test.sh` exercises
the installer.

Formatting, clippy, the tests above, and the shipped-engine check run in CI.
Enable the local hooks once:

```sh
git config core.hooksPath .githooks
```

`pre-commit` rebuilds `bin/dist` and stages the binary and fingerprint whenever
a commit stages engine inputs, so the engine is never a separate manual commit.
`pre-push` mirrors the remaining checks.

## Shipped engine

The pre-commit hook runs this automatically when engine inputs are staged:

```sh
bin/refresh-dist
```

It builds `reel` for `x86_64-unknown-linux-musl`, copies the binary to
`bin/dist/reel-native`, writes a source fingerprint, and prints its version.
The hook stages both files in the same commit as the source change.

`bin/refresh-dist` hides the toolchain's `rust-src` directory for the build and
restores it immediately. Without that, rustc embeds local std source paths and
the binary differs from CI even though the sources match.

CI is the byte-level authority. It rebuilds the committed engine on its pinned
runner and fails unless the rebuilt bytes are identical to `bin/dist/reel-native`.
The workflow records the engine SHA-256 in the job summary, so the shipped
binary has a public reproducibility witness tied to the commit. If a local
build differs from the CI build, download the `reel-dist` artifact from that CI
run and commit it.

The local pre-push hook compares the source fingerprint instead of bytes,
because host toolchains and linkers can produce different binaries. The build
pins the toolchain's `rust-lld`, normalizes source and Cargo paths, disables the
linker build ID, and relies on Cargo's own `strip = true` instead of the host
`strip`.

The static musl build does not depend on the host glibc.

The committed engine targets x86_64. On another architecture, build the crate
for the host, place the binary at `bin/dist/reel-native`, and run `./install.sh`.

## Development install

From a clone:

```sh
./install.sh
```

This copies the runtime files into
`~/.config/omarchy/plugins/io.github.tahadx.reel` and relinks
`~/.local/bin/reel` to the installed wrapper. It is the only install path that
copies files; `omarchy plugin add` and updates already place them in the plugin
directory. Re-running it is safe.

The copied tree is a development install, not a git checkout. `reel update` and
`reel update --check` use Omarchy's plugin updater, so they reject it the same way
they reject any non-git or dirty plugin. To receive updates, install Reel as a
normal Omarchy plugin, which is a clean git checkout.

If a development install is left on a local branch, `reel swap-main` stashes
local changes, saves any local `main` commits on a backup branch, checks out
`main` tracking `origin/main`, and reinstalls. Run it from a terminal; it prompts
before changing the checkout.

After QML changes, restart the shell so the loaded panel is replaced:

```sh
omarchy-restart-shell
```
