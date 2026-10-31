# Development

Reel is a Rust engine plus an Omarchy QML plugin. The plugin ships a prebuilt
engine, so a normal install does not need Rust.

## Layout

- `Cargo.toml` and `crates/reel/` — the engine and CLI.
- `Panel.qml`, `Service.qml`, and `Model.js` — the bar widget, background
  service, and pure JavaScript helpers.
- `bin/dist/reel-native` — the committed x86_64 musl engine.
- `bin/refresh-dist`, `bin/stamp-version`, and `bin/check-release` — release
  tooling.
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
The pre-push hook mirrors them:

```sh
git config core.hooksPath .githooks
```

## Shipped engine

Run `bin/refresh-dist` after every change under `crates/`:

```sh
bin/refresh-dist
```

It builds `reel` for `x86_64-unknown-linux-musl`, copies the binary to
`bin/dist/reel-native`, strips it, and prints its version. Commit the refreshed
binary in the same commit as the source change. CI and the pre-push hook fail
on a stale engine.

The release profile strips symbols, and the static musl build does not depend
on the host glibc. `bin/check-release` verifies that the manifest and crate
versions agree, that the engine is executable on this machine, and that it
reports the manifest version.

## Release

1. Stamp the version with `bin/stamp-version X.Y.Z`.
2. Rebuild the shipped engine with `bin/refresh-dist`.
3. Commit both files and the engine, tag the commit `vX.Y.Z`, and push the tag.

The release workflow rebuilds the engine from the tag and refuses to publish
if `manifest.json` or `crates/reel/Cargo.toml` disagree with it. It uploads a
`reel-<version>-linux-x86_64` asset and its checksum.

The committed engine and release workflow target x86_64. On another
architecture, build the crate for the host, place the binary at
`bin/dist/reel-native`, and run `./install.sh`.

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

After QML changes, restart the shell so the loaded panel is replaced:

```sh
omarchy-restart-shell
```
