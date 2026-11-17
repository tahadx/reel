#!/bin/bash
# Reel installer: verifies the recorder dependencies, self-configures the
# shipped engine, and enables the bar widget. Shortcuts are optional and never
# added unless you explicitly accept them. Idempotent; safe to re-run, including
# from the installed plugin directory. It refuses while a take is recording,
# because the plugin reload stops the recorder's telemetry listener.
#
# Usage: ./install.sh [--yes] [--no-binds]
set -euo pipefail

HERE="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
PLUGIN_ID="io.github.tahadx.reel"
CONFIG_HOME="${XDG_CONFIG_HOME:-$HOME/.config}"
PLUGIN_DIR="$CONFIG_HOME/omarchy/plugins/$PLUGIN_ID"
BINDINGS="$CONFIG_HOME/hypr/bindings.lua"

YES=0
BINDS=1
for arg in "$@"; do
  case "$arg" in
  --yes | -y) YES=1 ;;
  --no-binds) BINDS=0 ;;
  -h | --help)
    sed -n '2,7p' "$0" | sed 's/^# \{0,1\}//'
    exit 0
    ;;
  *)
    echo "install.sh: unknown option: $arg" >&2
    exit 1
    ;;
  esac
done

ask() {
  # ask "question" -- true with --yes or an explicit yes. Empty/non-interactive
  # answers skip, so installing normally never changes Hyprland bindings.
  local prompt="$1" answer
  (( YES == 1 )) && return 0
  if read -r -t 30 -p "$prompt [y/N] " answer; then
    [[ "$answer" == "y" || "$answer" == "Y" ]]
  else
    return 1
  fi
}

have() { command -v "$1" >/dev/null 2>&1; }

echo "== Reel: dependencies =="
missing=()
for tool in gpu-screen-recorder ffmpeg ffprobe; do
  have "$tool" || missing+=("$tool")
done
if ((${#missing[@]})); then
  echo "  missing: ${missing[*]}" >&2
  echo "  Install the missing packages with Omarchy's package installer: ${missing[*]}" >&2
  exit 1
fi
for tool in slurp mpv grim omarchy-capture-region omacut; do
  have "$tool" || echo "  optional tool missing: $tool"
done
echo "  ok"

# A plugin reload unloads the panel process that keeps `reel start` and its
# telemetry listener alive. The encoder would keep recording without pointer
# data, so auto zoom would stop for the rest of the take. Refuse instead.
if have reel; then
  current="$(reel status 2>/dev/null | head -n 1 || true)"
  case "$current" in
  Recording* | Paused* | Saving* | Starting*)
    echo "install.sh: Reel is ${current}. Stop the take before installing." >&2
    exit 1
    ;;
  esac
fi

echo "== Reel: command line =="
"$HERE/bin/configure"

echo "== Reel: plugin files =="
if [[ "$HERE" != "$PLUGIN_DIR" ]]; then
  # Development path: a clone installs into the plugin directory Omarchy loads.
  # The marketplace/clone flow already has the files where they belong.
  mkdir -p -- "$PLUGIN_DIR/bin/dist"
  install -m 0644 "$HERE/manifest.json" "$PLUGIN_DIR/manifest.json"
  install -m 0644 "$HERE/Panel.qml" "$PLUGIN_DIR/Panel.qml"
  install -m 0644 "$HERE/Service.qml" "$PLUGIN_DIR/Service.qml"
  install -m 0644 "$HERE/Countdown.qml" "$PLUGIN_DIR/Countdown.qml"
  install -m 0644 "$HERE/Model.js" "$PLUGIN_DIR/Model.js"
  install -m 0644 "$HERE/GuardedInput.qml" "$PLUGIN_DIR/GuardedInput.qml"
  install -m 0755 "$HERE/bin/reel" "$PLUGIN_DIR/bin/reel"
  install -m 0755 "$HERE/bin/clipboard" "$PLUGIN_DIR/bin/clipboard"
  install -m 0755 "$HERE/bin/configure" "$PLUGIN_DIR/bin/configure"
  install -m 0755 "$HERE/bin/uninstall" "$PLUGIN_DIR/bin/uninstall"
  install -m 0755 "$HERE/bin/dist/reel-native" "$PLUGIN_DIR/bin/dist/reel-native"
  # The installed copy runs on later updates and re-installs, so it must be the
  # same installer as this checkout rather than the one that was cloned.
  install -m 0755 "$HERE/install.sh" "$PLUGIN_DIR/install.sh"
  # The standalone engine is redistributed, so its notices travel with it.
  install -m 0644 "$HERE/LICENSE" "$PLUGIN_DIR/LICENSE"
  install -m 0644 "$HERE/THIRD-PARTY.md" "$PLUGIN_DIR/THIRD-PARTY.md"
  rm -rf -- "$PLUGIN_DIR/licenses"
  cp -a -- "$HERE/licenses" "$PLUGIN_DIR/licenses"
  # Older installs copied agent instruction files into the runtime tree. They
  # must not be auto-activated by the shell's agents; the checkout keeps its
  # guide under docs/contributing/ instead.
  rm -f -- "$PLUGIN_DIR/AGENTS.md" "$PLUGIN_DIR/CLAUDE.md"
  rm -rf -- "$PLUGIN_DIR/.agents" "$PLUGIN_DIR/.claude" "$PLUGIN_DIR/.codex" "$PLUGIN_DIR/.gemini"
  echo "  copied into $PLUGIN_DIR"
  # The first configure ran from the clone; relink to the installed wrapper so
  # ~/.local/bin keeps working if the checkout moves or is deleted.
  "$PLUGIN_DIR/bin/configure"
else
  echo "  already installed in $PLUGIN_DIR"
fi

echo "== Reel: plugin =="
# `omarchy plugin list` asks the running shell for its registry. Installing
# from a TTY or another session can race a shell restart, and the command then
# exits nonzero. With `set -e` that used to stop the installer here with no
# message, before the shortcuts and shell restart steps. Record the failure,
# finish the install, and verify or enable the plugin once the shell is up.
SHELL_DOWN=0
if ! PLUGIN_LIST="$(omarchy plugin list 2>&1)"; then
  SHELL_DOWN=1
  echo "  omarchy-shell is not reachable yet:"
  if [[ -n "$PLUGIN_LIST" ]]; then
    printf '%s\n' "$PLUGIN_LIST" | sed 's/^/    /'
  else
    echo "    omarchy plugin list returned no answer."
  fi
  echo "  The shell restart below will load Reel; its state is checked then."
fi
if ((SHELL_DOWN == 0)); then
  state="$(printf '%s\n' "$PLUGIN_LIST" | awk -v id="$PLUGIN_ID" '$1 == id { print $2; exit }')"
  if [[ "$state" == "enabled" ]]; then
    echo "  already enabled."
  elif ! omarchy plugin enable "$PLUGIN_ID" --section right; then
    echo "install.sh: could not enable $PLUGIN_ID in the bar." >&2
    echo "  Start the shell with \`omarchy restart shell\`, then re-run this installer." >&2
    exit 1
  else
    echo "  enabled in the right bar section."
  fi
fi
omarchy-shell -q shell rescanPlugins || true

echo "== Reel: shortcuts (optional) =="
if ((BINDS == 0)); then
  echo "  skipped (--no-binds)."
else
  BIND_BLOCK=$(
    cat <<'LUA'
-- Reel: begin. Written by install.sh; delete this block to remove the binds.
-- Key names follow the active keyboard layout.
o.bind("SUPER + R", "Reel start/stop", "reel toggle")
o.bind("SUPER + SHIFT + R", "Reel panel", { panel = "io.github.tahadx.reel" })
o.bind("SUPER + ALT + O", "Reel pause/resume", "reel toggle-pause")

-- Zoom controls (Apple-style: non-conflicting Ctrl+Alt, mouse wheel, & touchpad gestures)
o.bind("CTRL + ALT + equal", "Reel zoom in", "reel zoom-in")
o.bind("CTRL + ALT + minus", "Reel zoom out", "reel zoom-out")
o.bind("CTRL + ALT + 0", "Reel zoom reset", "reel zoom-reset")
o.bind("CTRL + ALT + 8", "Reel zoom toggle", "reel zoom-toggle")
o.bind("CTRL + ALT + mouse_up", "Reel zoom in", "reel zoom-in --step 0.15")
o.bind("CTRL + ALT + mouse_down", "Reel zoom out", "reel zoom-out --step 0.15")

-- Keycodes for zoom shortcuts (works across any active keyboard layout)
o.bind("CTRL + ALT + code:21", "Reel zoom in", "reel zoom-in")
o.bind("CTRL + ALT + code:20", "Reel zoom out", "reel zoom-out")
o.bind("CTRL + ALT + code:19", "Reel zoom reset", "reel zoom-reset")
o.bind("CTRL + ALT + code:17", "Reel zoom toggle", "reel zoom-toggle")

-- Pan the zoomed view; hold to keep moving, Shift to cover more ground
o.bind_hold("CTRL + ALT + code:113", "Reel pan left", "reel zoom-pan left --hold", "Reel pan left release", "reel zoom-pan --stop")
o.bind_hold("CTRL + ALT + code:114", "Reel pan right", "reel zoom-pan right --hold", "Reel pan right release", "reel zoom-pan --stop")
o.bind_hold("CTRL + ALT + code:111", "Reel pan up", "reel zoom-pan up --hold", "Reel pan up release", "reel zoom-pan --stop")
o.bind_hold("CTRL + ALT + code:116", "Reel pan down", "reel zoom-pan down --hold", "Reel pan down release", "reel zoom-pan --stop")
o.bind_hold("CTRL + ALT + SHIFT + code:113", "Reel pan left fast", "reel zoom-pan left --hold --fast", "Reel pan left fast release", "reel zoom-pan --stop")
o.bind_hold("CTRL + ALT + SHIFT + code:114", "Reel pan right fast", "reel zoom-pan right --hold --fast", "Reel pan right fast release", "reel zoom-pan --stop")
o.bind_hold("CTRL + ALT + SHIFT + code:111", "Reel pan up fast", "reel zoom-pan up --hold --fast", "Reel pan up fast release", "reel zoom-pan --stop")
o.bind_hold("CTRL + ALT + SHIFT + code:116", "Reel pan down fast", "reel zoom-pan down --hold --fast", "Reel pan down fast release", "reel zoom-pan --stop")
-- Reel: end.
LUA
  )
  if [[ -f "$BINDINGS" ]] \
    && grep -qxF -- 'o.bind("SUPER + R", "Reel start/stop", "reel toggle")' "$BINDINGS" \
    && grep -qxF -- 'o.bind("SUPER + SHIFT + R", "Reel panel", { panel = "io.github.tahadx.reel" })' "$BINDINGS" \
    && grep -qxF -- 'o.bind("CTRL + ALT + equal", "Reel zoom in", "reel zoom-in")' "$BINDINGS" \
    && grep -qxF -- 'o.bind("CTRL + ALT + code:21", "Reel zoom in", "reel zoom-in")' "$BINDINGS" \
    && grep -qxF -- 'o.bind_hold("CTRL + ALT + code:113", "Reel pan left", "reel zoom-pan left --hold", "Reel pan left release", "reel zoom-pan --stop")' "$BINDINGS"; then
    echo "  Reel shortcuts are already present."
  elif ask "Add or update the optional Reel shortcuts?"; then
    mkdir -p -- "$(dirname -- "$BINDINGS")"
    if [[ -f "$BINDINGS" ]]; then
      cp -- "$BINDINGS" "$BINDINGS.bak.$(date +%s)"
      existing=$(sed '/^-- Reel: begin\./,/^-- Reel: end\.$/d' "$BINDINGS")
      printf '%s\n' "$existing" >"$BINDINGS"
    fi
    printf '\n%s\n' "$BIND_BLOCK" >>"$BINDINGS"
    if have hyprctl && [[ -n "${HYPRLAND_INSTANCE_SIGNATURE:-}" ]]; then
      hyprctl reload >/dev/null 2>&1 || true
    fi
    echo "  added. Super+R records, Super+Shift+R opens panel, Ctrl+Alt+=/- and Ctrl+Alt+Scroll zoom. Ctrl+Scroll also zooms without these binds."
  else
    echo "  not added. These are optional; if you want them, add this to"
    echo "  $BINDINGS and run \`hyprctl reload\`:"
    echo
    printf '%s\n' "$BIND_BLOCK" | sed 's/^/    /'
  fi
fi

echo "== Reel: shell =="
if ! omarchy restart shell; then
  echo "install.sh: the shell did not restart. Run \`omarchy restart shell\` to load Reel." >&2
  exit 1
fi
echo "  restarted."

# The plugin step above could not ask the shell anything. Now that it is up,
# finish that step rather than leaving a cloned plugin disabled, and fail
# loudly if the shell still does not answer.
if ((SHELL_DOWN == 1)); then
  if ! PLUGIN_LIST="$(omarchy plugin list 2>&1)"; then
    echo "install.sh: omarchy-shell is still not answering after the restart." >&2
    if [[ -n "$PLUGIN_LIST" ]]; then
      printf '%s\n' "$PLUGIN_LIST" | sed 's/^/  /' >&2
    fi
    echo "  Reel's files and command are installed, but the bar widget could not be enabled." >&2
    echo "  Start the shell and run: omarchy plugin enable $PLUGIN_ID --section right" >&2
    exit 1
  fi
  state="$(printf '%s\n' "$PLUGIN_LIST" | awk -v id="$PLUGIN_ID" '$1 == id { print $2; exit }')"
  if [[ "$state" == "enabled" ]]; then
    echo "  Reel is enabled."
  elif ! omarchy plugin enable "$PLUGIN_ID" --section right; then
    echo "install.sh: could not enable $PLUGIN_ID after the shell restarted." >&2
    echo "  Run: omarchy plugin enable $PLUGIN_ID --section right" >&2
    exit 1
  else
    echo "  enabled in the right bar section."
  fi
fi

echo
echo "Done. Click the Reel icon in the bar, or press Super+Shift+R if you added the shortcut."
