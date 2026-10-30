#!/bin/bash
# Installer/uninstaller regression test. Everything happens under a throwaway
# HOME; the real user configuration and recordings are never touched.
set -euo pipefail

REPO="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
TEST_ROOT="$(mktemp -d "${TMPDIR:-/tmp}/reel-install-test.XXXXXX")"
export TEST_ROOT

cleanup() {
  rm -rf -- "$TEST_ROOT"
}
trap cleanup EXIT

fail() {
  echo "install.test: $*" >&2
  exit 1
}

assert_file() {
  [[ -f "$1" ]] || fail "missing file: $1"
}

assert_no_file() {
  [[ ! -e "$1" && ! -L "$1" ]] || fail "unexpected file: $1"
}

export HOME="$TEST_ROOT/home"
export XDG_CONFIG_HOME="$HOME/.config"
export XDG_STATE_HOME="$HOME/.state"
mkdir -p -- "$HOME/.config/hypr" "$HOME/.local/bin" "$TEST_ROOT/bin"

# The installer checks for the recorder tools. Shim them so the test does not
# depend on the machine's packages, and keep Omarchy/Hyprland out of the run.
for tool in gpu-screen-recorder ffmpeg ffprobe; do
  cat >"$TEST_ROOT/bin/$tool" <<'EOF'
#!/bin/sh
exit 0
EOF
  chmod 755 -- "$TEST_ROOT/bin/$tool"
done
cat >"$TEST_ROOT/bin/omarchy" <<'EOF'
#!/bin/sh
printf '%s\n' "$*" >>"${TEST_ROOT:?}/omarchy.log"
case "${1:-} ${2:-}" in
"plugin list") printf '%s\n' "io.github.tahadx.reel enabled" ;;
esac
exit 0
EOF
cat >"$TEST_ROOT/bin/omarchy-shell" <<'EOF'
#!/bin/sh
printf '%s\n' "$*" >>"${TEST_ROOT:?}/omarchy-shell.log"
exit 0
EOF
cat >"$TEST_ROOT/bin/hyprctl" <<'EOF'
#!/bin/sh
printf '%s\n' "$*" >>"${TEST_ROOT:?}/hyprctl.log"
exit 0
EOF
chmod 755 -- "$TEST_ROOT/bin/omarchy" "$TEST_ROOT/bin/omarchy-shell" \
  "$TEST_ROOT/bin/hyprctl"
export PATH="$HOME/.local/bin:$TEST_ROOT/bin:$PATH"
unset HYPRLAND_INSTANCE_SIGNATURE

PLUGIN_DIR="$XDG_CONFIG_HOME/omarchy/plugins/io.github.tahadx.reel"
BINDINGS="$XDG_CONFIG_HOME/hypr/bindings.lua"
USER_BIND='o.bind("SUPER + Return", "Terminal", "foot")'
# No trailing newline: the installer must not glue its block onto this line.
printf '%s' "$USER_BIND" >"$BINDINGS"

"$REPO/install.sh" --yes >"$TEST_ROOT/install-1.log"

MANIFEST_VERSION="$(sed -n 's/^[[:space:]]*"version": "\([^"]*\)".*/\1/p' "$REPO/manifest.json" | head -n 1)"
WANT_VERSION="reel $MANIFEST_VERSION"

assert_file "$HOME/.local/bin/reel"
[[ -L "$HOME/.local/bin/reel" ]] || fail "reel is not a symlink"
[[ "$(readlink -- "$HOME/.local/bin/reel")" == "$PLUGIN_DIR/bin/reel" ]] \
  || fail "reel points at $(readlink -- "$HOME/.local/bin/reel"), not the plugin wrapper"
assert_file "$PLUGIN_DIR/bin/dist/reel-native"
[[ -x "$PLUGIN_DIR/bin/dist/reel-native" ]] || fail "shipped engine is not executable"
[[ "$(reel --version)" == "$WANT_VERSION" ]] \
  || fail "reel --version does not report the manifest version"
grep -qxF -- "$USER_BIND" "$BINDINGS" || fail "installer changed the user binding"
grep -qxF -- '-- Reel: begin. Written by install.sh; delete this block to remove the binds.' \
  "$BINDINGS" || fail "installer did not add the marked block"
grep -qxF -- 'o.bind("SUPER + code:27", "Reel start/stop", "reel toggle")' "$BINDINGS" \
  || fail "installer did not add the layout-independent start/stop binding"
grep -qxF -- 'o.bind("CTRL + ALT + equal", "Reel zoom in", "reel zoom-in")' "$BINDINGS" \
  || fail "installer did not add the zoom-in binding"
grep -qxF -- 'o.bind("CTRL + ALT + code:21", "Reel zoom in", "reel zoom-in")' "$BINDINGS" \
  || fail "installer did not add the layout-independent zoom-in binding"
grep -qxF -- 'o.bind("CTRL + ALT + mouse_up", "Reel zoom in", "reel zoom-in --step 0.15")' "$BINDINGS" \
  || fail "installer did not add the scroll zoom binding"
[[ "$(grep -c '^-- Reel: begin' "$BINDINGS")" -eq 1 ]] || fail "binding block duplicated"
[[ "$(tail -c 1 -- "$BINDINGS" | wc -l)" -eq 1 ]] || fail "bindings file lost its final newline"
assert_file "$BINDINGS.bak."*
grep -q 'plugin enable' "$TEST_ROOT/omarchy.log" && fail "installer enabled an enabled plugin"
assert_no_file "$TEST_ROOT/hyprctl.log"

BACKUPS_BEFORE="$(find "$XDG_CONFIG_HOME/hypr" -maxdepth 1 -name 'bindings.lua.bak.*' | wc -l)"

"$REPO/install.sh" --yes >"$TEST_ROOT/install-2.log"

[[ "$(grep -c '^-- Reel: begin' "$BINDINGS")" -eq 1 ]] || fail "rerun duplicated the block"
BACKUPS_AFTER="$(find "$XDG_CONFIG_HOME/hypr" -maxdepth 1 -name 'bindings.lua.bak.*' | wc -l)"
[[ "$BACKUPS_BEFORE" -eq "$BACKUPS_AFTER" ]] || fail "rerun made another backup"
grep -q 'already present' "$TEST_ROOT/install-2.log" || fail "rerun is not quiet about the binding"
assert_no_file "$TEST_ROOT/hyprctl.log"

# An install from before the zoom commands only has the three SUPER binds. The
# next run must replace the block so the new commands show up.
sed -i '/CTRL + ALT/d' "$BINDINGS"
"$REPO/install.sh" --yes >"$TEST_ROOT/install-upgrade.log"
grep -qxF -- 'o.bind("CTRL + ALT + equal", "Reel zoom in", "reel zoom-in")' "$BINDINGS" \
  || fail "installer did not upgrade a pre-zoom shortcut block"
grep -qxF -- 'o.bind("CTRL + ALT + code:21", "Reel zoom in", "reel zoom-in")' "$BINDINGS" \
  || fail "upgraded block is missing the layout-independent zoom binding"
[[ "$(grep -c '^-- Reel: begin' "$BINDINGS")" -eq 1 ]] || fail "upgrade duplicated the block"
assert_no_file "$TEST_ROOT/hyprctl.log"

"$REPO/bin/uninstall" >"$TEST_ROOT/uninstall-1.log"

assert_no_file "$HOME/.local/bin/reel"
grep -qxF -- "$USER_BIND" "$BINDINGS" || fail "uninstaller damaged the user binding"
if grep -q '^-- Reel: begin' "$BINDINGS"; then
  fail "uninstaller left the Reel block"
fi
grep -q 'omarchy plugin remove io.github.tahadx.reel' "$TEST_ROOT/uninstall-1.log" \
  || fail "uninstaller does not print the final plugin removal command"

# A second uninstall is a no-op, not an error.
"$REPO/bin/uninstall" >"$TEST_ROOT/uninstall-2.log"

# Reinstalling into a HOME without a bindings file must create it instead of
# failing on a missing source, and must not make a backup for a file that did
# not exist.
BACKUPS_BEFORE="$(find "$XDG_CONFIG_HOME/hypr" -maxdepth 1 -name 'bindings.lua.bak.*' | wc -l)"
rm -f -- "$BINDINGS"
"$REPO/install.sh" --yes >"$TEST_ROOT/install-3.log"
assert_file "$BINDINGS"
grep -qxF -- 'o.bind("SUPER + code:27", "Reel start/stop", "reel toggle")' "$BINDINGS" \
  || fail "installer did not create a missing bindings file"
BACKUPS_AFTER="$(find "$XDG_CONFIG_HOME/hypr" -maxdepth 1 -name 'bindings.lua.bak.*' | wc -l)"
[[ "$BACKUPS_BEFORE" -eq "$BACKUPS_AFTER" ]] \
  || fail "installer made a backup when there was no bindings file"

# Only Reel-shaped symlinks are ours; an unrelated binary with "reel" in its
# target must survive cleanup.
"$REPO/bin/uninstall" >/dev/null
ln -s -- "$TEST_ROOT/my-reel-helper" "$HOME/.local/bin/reel"
"$REPO/bin/uninstall" >/dev/null
[[ -L "$HOME/.local/bin/reel" ]] || fail "uninstaller removed an unrelated symlink"

echo "install.test: ok"
