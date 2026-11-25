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
# Keep the test off the live runtime directory. Otherwise `reel status` reads a
# take that is recording on the desktop and the installer refuses to run.
export XDG_RUNTIME_DIR="$TEST_ROOT/run"
# The installer test runs on the developer's live desktop. A failed probe must
# stay in the log, not become a notification.
export REEL_NO_NOTIFY=1
mkdir -p -- "$XDG_RUNTIME_DIR" "$HOME/.config/hypr" "$HOME/.local/bin" "$TEST_ROOT/bin"

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
#!/bin/bash
printf '%s\n' "$*" >>"${TEST_ROOT:?}/omarchy.log"
if [[ -n ${REEL_TEST_SHELL_DOWN:-} && -f ${TEST_ROOT:?}/shell-down ]]; then
  case "${1:-} ${2:-}" in
  "plugin list")
    printf '%s\n' "omarchy-shell is not running" >&2
    exit 1
    ;;
  esac
fi
case "${1:-} ${2:-}" in
"plugin list") printf '%s\n' "io.github.tahadx.reel enabled" ;;
"restart shell")
  [[ -n ${REEL_TEST_SHELL_STUCK:-} ]] || rm -f "${TEST_ROOT:?}/shell-down"
  ;;
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
cat >"$TEST_ROOT/bin/omarchy-notification-send" <<'EOF'
#!/bin/sh
printf '%s\n' "$*" >>"${TEST_ROOT:?}/notification.log"
exit 0
EOF
chmod 755 -- "$TEST_ROOT/bin/omarchy" "$TEST_ROOT/bin/omarchy-shell" \
  "$TEST_ROOT/bin/hyprctl" "$TEST_ROOT/bin/omarchy-notification-send"
export PATH="$HOME/.local/bin:$TEST_ROOT/bin:$PATH"
unset HYPRLAND_INSTANCE_SIGNATURE

PLUGIN_DIR="$XDG_CONFIG_HOME/omarchy/plugins/io.github.tahadx.reel"
BINDINGS="$XDG_CONFIG_HOME/hypr/bindings.lua"
USER_BIND='o.bind("SUPER + Return", "Terminal", "foot")'

# The copy-paste block in docs/bindings.md must be exactly what install.sh
# writes, or the documentation silently hands out a stale shortcut set.
sed -n "/^[[:space:]]*cat <<'LUA'$/,/^LUA$/p" "$REPO/install.sh" | sed '1d;$d' \
  >"$TEST_ROOT/install-binds.lua"
# shellcheck disable=SC2016  # the backticks are Markdown fences, not commands
sed -n '/^```lua$/,/^```$/p' "$REPO/docs/bindings.md" | sed '1d;$d' \
  >"$TEST_ROOT/docs-binds.lua"
diff -u "$TEST_ROOT/install-binds.lua" "$TEST_ROOT/docs-binds.lua" \
  || fail "docs/bindings.md drifted from install.sh"

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
assert_file "$PLUGIN_DIR/bin/clipboard"
[[ -x "$PLUGIN_DIR/bin/clipboard" ]] || fail "bounded clipboard helper is not executable"
assert_file "$PLUGIN_DIR/GuardedInput.qml"
assert_file "$PLUGIN_DIR/THIRD-PARTY.md"
assert_file "$PLUGIN_DIR/install.sh"
# A development install records the checkout so `reel update` can reinstall
# from it instead of git-updating the copied tree.
assert_file "$PLUGIN_DIR/.reel-dev-source"
DEV_SOURCE="$(sed -n '1p' "$PLUGIN_DIR/.reel-dev-source")"
[[ "$DEV_SOURCE" == "$REPO" ]] || fail "dev install recorded '$DEV_SOURCE', not '$REPO'"
grep -qxF -- 'Panel.qml' "$PLUGIN_DIR/.reel-dev-source" \
  || fail "dev install marker is missing Panel.qml"
grep -qxF -- 'bin/dist/reel-native' "$PLUGIN_DIR/.reel-dev-source" \
  || fail "dev install marker is missing the shipped engine"
# The shipped engine reads that marker: a fresh copy is current, and a stale
# copied file is offered as an update.
CHECK_JSON="$(reel update --check --json)"
[[ "$CHECK_JSON" == *'"available":false'* ]] \
  || fail "dev update check did not see a current install: $CHECK_JSON"
[[ "$CHECK_JSON" == *'"error":""'* ]] \
  || fail "dev update check reported an error: $CHECK_JSON"
printf '\n// stale\n' >>"$PLUGIN_DIR/Panel.qml"
CHECK_JSON="$(reel update --check --json)"
[[ "$CHECK_JSON" == *'"available":true'* ]] \
  || fail "dev update check missed a stale copied file: $CHECK_JSON"
# The update action must reinstall from the checkout instead of asking git to
# fast-forward the copied tree, and it must make the check current again.
reel update --binds >"$TEST_ROOT/update-dev.log"
grep -q 'development install from' "$TEST_ROOT/update-dev.log" \
  || fail "dev update did not report the development checkout"
CHECK_JSON="$(reel update --check --json)"
[[ "$CHECK_JSON" == *'"available":false'* ]] \
  || fail "dev update did not replace the stale copied file: $CHECK_JSON"
[[ -x "$PLUGIN_DIR/install.sh" ]] || fail "installed installer is not executable"
cmp -s "$REPO/install.sh" "$PLUGIN_DIR/install.sh" \
  || fail "installed installer differs from the checkout"
assert_file "$PLUGIN_DIR/licenses/rust-toolchain/NOTICE.md"
[[ "$(reel --version)" == "$WANT_VERSION" ]] \
  || fail "reel --version does not report the manifest version"
grep -qxF -- "$USER_BIND" "$BINDINGS" || fail "installer changed the user binding"
grep -qxF -- '-- Reel: begin. Written by install.sh; delete this block to remove the binds.' \
  "$BINDINGS" || fail "installer did not add the marked block"
grep -qxF -- 'o.bind("SUPER + R", "Reel start/stop", "reel toggle")' "$BINDINGS" \
  || fail "installer did not add the start/stop binding"
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
grep -qxF -- 'restart shell' "$TEST_ROOT/omarchy.log" || fail "installer did not restart the shell"
assert_no_file "$TEST_ROOT/hyprctl.log"

BACKUPS_BEFORE="$(find "$XDG_CONFIG_HOME/hypr" -maxdepth 1 -name 'bindings.lua.bak.*' | wc -l)"

# Older installs left agent instruction files in the runtime tree. A rerun
# must remove them.
printf 'stale\n' >"$PLUGIN_DIR/AGENTS.md"
printf 'stale\n' >"$PLUGIN_DIR/CLAUDE.md"
"$REPO/install.sh" --yes >"$TEST_ROOT/install-2.log"
assert_no_file "$PLUGIN_DIR/AGENTS.md"
assert_no_file "$PLUGIN_DIR/CLAUDE.md"

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
grep -qxF -- 'o.bind("SUPER + R", "Reel start/stop", "reel toggle")' "$BINDINGS" \
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

# The bounded clipboard helper must cap the bytes a fake owner can emit and
# give up on a stalled owner.
cat >"$TEST_ROOT/bin/wl-paste" <<'EOF'
#!/bin/sh
exec yes x
EOF
chmod 755 -- "$TEST_ROOT/bin/wl-paste"
set +e
"$REPO/bin/clipboard" >"$TEST_ROOT/clipboard.out" 2>/dev/null
CLIP_CODE=$?
set -e
[[ "$CLIP_CODE" -eq 141 ]] || fail "bounded paste exited $CLIP_CODE for oversized text"
[[ "$(wc -c <"$TEST_ROOT/clipboard.out")" -eq 4097 ]] \
  || fail "bounded paste did not cap its output at 4097 bytes"
cat >"$TEST_ROOT/bin/wl-paste" <<'EOF'
#!/bin/sh
sleep 3
EOF
chmod 755 -- "$TEST_ROOT/bin/wl-paste"
set +e
"$REPO/bin/clipboard" >/dev/null 2>&1
CLIP_CODE=$?
set -e
[[ "$CLIP_CODE" -eq 124 ]] || fail "bounded paste exited $CLIP_CODE for a stalled clipboard"
rm -f -- "$TEST_ROOT/bin/wl-paste"

# A shell that is not reachable during the plugin step must not stop the
# installer silently: the files still install, the restart brings the shell up,
# and the plugin state is checked afterwards.
touch "$TEST_ROOT/shell-down"
REEL_TEST_SHELL_DOWN=1 "$REPO/install.sh" --yes >"$TEST_ROOT/install-shell-down.log"
grep -q 'omarchy-shell is not reachable yet' "$TEST_ROOT/install-shell-down.log" \
  || fail "installer did not report an unreachable shell"
grep -q 'Reel is enabled' "$TEST_ROOT/install-shell-down.log" \
  || fail "installer did not recheck the plugin after the restart"
grep -qxF -- 'restart shell' "$TEST_ROOT/omarchy.log" \
  || fail "installer did not restart a shell that was down"

# If the shell still cannot answer after the restart, the installer has to say
# so and exit nonzero instead of claiming success.
touch "$TEST_ROOT/shell-down"
set +e
REEL_TEST_SHELL_DOWN=1 REEL_TEST_SHELL_STUCK=1 \
  "$REPO/install.sh" --yes >"$TEST_ROOT/install-shell-stuck.log" 2>&1
SHELL_STUCK_CODE=$?
set -e
[[ "$SHELL_STUCK_CODE" -ne 0 ]] || fail "installer succeeded with no shell after the restart"
grep -q 'still not answering after the restart' "$TEST_ROOT/install-shell-stuck.log" \
  || fail "installer did not explain the failed post-restart plugin check"
rm -f -- "$TEST_ROOT/shell-down"

# gpu-screen-recorder is the recommended path, but wf-recorder alone must
# satisfy the dependency check: that is the CPU fallback Reel ships. Build a
# PATH with the utilities the installer needs and no system recorder in it, so
# the check sees exactly what this test provides.
MINBIN="$TEST_ROOT/minbin"
mkdir -p -- "$MINBIN"
for tool in awk cat cp dirname grep head install ln mkdir rm sed readlink; do
  ln -s -- "$(command -v "$tool")" "$MINBIN/$tool"
done
ln -s -- "$TEST_ROOT/bin/omarchy" "$MINBIN/omarchy"
ln -s -- "$TEST_ROOT/bin/omarchy-shell" "$MINBIN/omarchy-shell"
ln -s -- "$TEST_ROOT/bin/hyprctl" "$MINBIN/hyprctl"
ln -s -- "$TEST_ROOT/bin/omarchy-notification-send" "$MINBIN/omarchy-notification-send"
ln -s -- "$TEST_ROOT/bin/ffmpeg" "$MINBIN/ffmpeg"
ln -s -- "$TEST_ROOT/bin/ffprobe" "$MINBIN/ffprobe"

cat >"$MINBIN/wf-recorder" <<'EOF'
#!/bin/sh
exit 0
EOF
chmod 755 -- "$MINBIN/wf-recorder"
PATH="$MINBIN:$HOME/.local/bin" "$REPO/install.sh" --yes >"$TEST_ROOT/install-wf-only.log"
grep -q '== Reel: dependencies ==' "$TEST_ROOT/install-wf-only.log" \
  || fail "installer did not reach the dependency check with wf-recorder only"

rm -f -- "$MINBIN/wf-recorder"
set +e
PATH="$MINBIN:$HOME/.local/bin" "$REPO/install.sh" --yes \
  >"$TEST_ROOT/install-no-recorder.log" 2>&1
NO_RECORDER_CODE=$?
set -e
[[ "$NO_RECORDER_CODE" -ne 0 ]] || fail "installer succeeded with no recorder installed"
grep -q 'missing: gpu-screen-recorder or wf-recorder' "$TEST_ROOT/install-no-recorder.log" \
  || fail "installer did not name both recorder options when neither is installed"

# The test must stay silent on the live desktop even if a probe fails.
assert_no_file "$TEST_ROOT/notification.log"

echo "install.test: ok"
