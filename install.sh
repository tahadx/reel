#!/bin/bash
# Reel installer: verifies the recorder dependencies, self-configures the
# shipped engine, and enables the bar widget. Shortcuts are optional and never
# added unless you explicitly accept them. Idempotent; safe to re-run, including
# from the installed plugin directory.
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
  echo "  Install with: sudo pacman -S --needed ${missing[*]}" >&2
  exit 1
fi
for tool in slurp mpv grim omarchy-capture-region omacut; do
  have "$tool" || echo "  optional tool missing: $tool"
done
echo "  ok"

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
  install -m 0644 "$HERE/Model.js" "$PLUGIN_DIR/Model.js"
  install -m 0755 "$HERE/bin/reel" "$PLUGIN_DIR/bin/reel"
  install -m 0755 "$HERE/bin/configure" "$PLUGIN_DIR/bin/configure"
  install -m 0755 "$HERE/bin/uninstall" "$PLUGIN_DIR/bin/uninstall"
  install -m 0755 "$HERE/bin/dist/reel-native" "$PLUGIN_DIR/bin/dist/reel-native"
  echo "  copied into $PLUGIN_DIR"
  # The first configure ran from the clone; relink to the installed wrapper so
  # ~/.local/bin keeps working if the checkout moves or is deleted.
  "$PLUGIN_DIR/bin/configure"
else
  echo "  already installed in $PLUGIN_DIR"
fi

echo "== Reel: plugin =="
state="$(omarchy plugin list 2>/dev/null | awk -v id="$PLUGIN_ID" '$1 == id { print $2; exit }')"
if [[ "$state" == "enabled" ]]; then
  echo "  already enabled."
else
  omarchy plugin enable "$PLUGIN_ID" --section right
  echo "  enabled in the right bar section."
fi
omarchy-shell -q shell rescanPlugins || true

echo "== Reel: shortcuts (optional) =="
if ((BINDS == 0)); then
  echo "  skipped (--no-binds)."
else
  BIND_BLOCK=$(
    cat <<'LUA'
-- Reel: begin. Written by install.sh; delete this block to remove the binds.
o.bind("SUPER + R", "Reel start/stop", "reel toggle")
o.bind("SUPER + SHIFT + R", "Reel panel", "omarchy-shell shell toggle io.github.tahadx.reel '{}'")
o.bind("SUPER + ALT + O", "Reel pause/resume", "reel toggle-pause")
-- Reel: end.
LUA
  )
  if [[ -f "$BINDINGS" ]] \
    && grep -qxF -- 'o.bind("SUPER + R", "Reel start/stop", "reel toggle")' "$BINDINGS" \
    && grep -qxF -- 'o.bind("SUPER + SHIFT + R", "Reel panel", "omarchy-shell shell toggle io.github.tahadx.reel '\''{}'\''")' "$BINDINGS" \
    && grep -qxF -- 'o.bind("SUPER + ALT + O", "Reel pause/resume", "reel toggle-pause")' "$BINDINGS"; then
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
    echo "  added. Super+R records, Super+Shift+R opens the panel, Super+Alt+O pauses."
  else
    echo "  not added. These are optional; if you want them, add this to"
    echo "  $BINDINGS and run \`hyprctl reload\`:"
    echo
    printf '%s\n' "$BIND_BLOCK" | sed 's/^/    /'
  fi
fi

echo
echo "Done. Click the Reel icon in the bar, or press Super+Shift+R if you added the shortcut."
