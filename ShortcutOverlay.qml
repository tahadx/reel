import QtQuick
import Quickshell
import Quickshell.Io
import Quickshell.Wayland
import qs.Commons
import qs.Commons as Commons
import "Shortcuts.js" as Shortcuts

// A visual-only toast for the Omarchy shortcuts pressed during a take.
//
// The recording state comes from the live service. The key hook is registered
// in Hyprland at runtime (no config-file edits) only while a take is active and
// the Visuals -> Show shortcuts setting is on. A Lua script matches raw key
// events against the same bind list the Super+K menu reads and writes one line
// here; this component only draws it.
Item {
  id: root

  property var service: null

  readonly property string home: Quickshell.env("HOME") || ""
  readonly property string runtimeDir: {
    var runtime = Quickshell.env("XDG_RUNTIME_DIR") || ""
    if (runtime) return runtime + "/reel"
    return (Quickshell.env("XDG_STATE_HOME") || root.home + "/.local/state") + "/omarchy/reel/run"
  }
  readonly property string configPath: root.home + "/.config/omarchy/reel.json"
  readonly property string hookPath: root.home
    + "/.config/omarchy/plugins/io.github.tahadx.reel/bin/shortcut-hook.lua"
  readonly property string mapPath: root.runtimeDir + "/shortcut-map.tsv"
  readonly property string eventPath: root.runtimeDir + "/shortcut-event"

  readonly property bool recording: root.service !== null
    && (root.service.active || root.service.capturing)
  property bool settingOn: false
  readonly property bool enabled: root.settingOn && root.recording

  property string bindsText: ""
  property string keymapText: ""
  property string pendingMap: ""
  property string keys: ""
  property string label: ""
  property string lastSeq: ""
  property bool showing: false

  function settingEnabled(value) {
    if (value === true) return true
    var text = String(value === undefined || value === null ? "" : value).toLowerCase()
    return text === "on" || text === "true" || text === "yes" || text === "1"
  }

  function applyConfig(text) {
    var parsed
    try {
      parsed = JSON.parse(String(text || "{}"))
    } catch (e) {
      parsed = ({})
    }
    root.settingOn = !!parsed && settingEnabled(parsed.showShortcuts)
  }

  function startHook() {
    if (!root.enabled) return
    if (root.bindsText === "" || root.keymapText === "") {
      if (!bindsProc.running) bindsProc.running = true
      if (!keymapProc.running) keymapProc.running = true
      return
    }
    var map = Shortcuts.buildMap(root.bindsText, root.keymapText)
    if (map === "") return
    root.pendingMap = map
    if (!mapWriter.running) mapWriter.running = true
  }

  function applyEvent(text) {
    var event = Shortcuts.parseEvent(text)
    if (event.keys === "" || event.label === "") return
    if (event.seq === root.lastSeq) return
    root.lastSeq = event.seq
    root.keys = event.keys
    root.label = event.label
    root.showing = true
    hideTimer.restart()
  }

  onEnabledChanged: {
    if (root.enabled) {
      root.lastSeq = ""
      root.startHook()
    } else {
      root.showing = false
      root.pendingMap = ""
      unregisterProc.running = true
      refreshTimer.stop()
    }
  }

  Component.onDestruction: {
    if (_registered) unregisterProc.running = true
  }
  property bool _registered: false

  // The user's saved choice. The engine writes this key through the same panel
  // row that writes every other setting, so a missing key means "off".
  FileView {
    id: configFile
    path: root.configPath
    watchChanges: true
    printErrors: false
    onFileChanged: reload()
    onLoaded: root.applyConfig(text())
    onLoadFailed: root.settingOn = false
  }

  Timer {
    interval: 1000
    repeat: true
    running: !configFile.loaded
    onTriggered: configFile.reload()
  }

  // The bind list is the one Omarchy's keybindings menu shows. Rebuild it at
  // the start of every take and slowly while one runs, so a changed binding
  // cannot leave the overlay showing stale keys.
  Timer {
    id: refreshTimer
    interval: 60000
    repeat: true
    running: root.enabled
    onTriggered: root.startHook()
  }

  Process {
    id: bindsProc
    command: ["hyprctl", "binds", "-j"]
    stdout: StdioCollector {
      waitForEnd: true
      onStreamFinished: {
        root.bindsText = String(text || "")
        root.startHook()
      }
    }
    stderr: StdioCollector { waitForEnd: true }
  }

  Process {
    id: keymapProc
    // xkbcli waits for stdin EOF; Quickshell leaves the pipe open, so give it
    // /dev/null or the map is never built and the hook never registers.
    command: ["sh", "-c", "xkbcli compile-keymap </dev/null"]
    stdout: StdioCollector {
      waitForEnd: true
      onStreamFinished: {
        root.keymapText = String(text || "")
        root.startHook()
      }
    }
    stderr: StdioCollector { waitForEnd: true }
  }

  // Write via argv, never a shell string: the map is plain data and tee only
  // ever sees a fixed path from this plugin's own runtime directory.
  Process {
    id: mapWriter
    command: ["tee", root.mapPath]
    stdinEnabled: true
    onStarted: {
      if (root.pendingMap === "") return
      mapWriter.write(root.pendingMap)
      root.pendingMap = ""
      mapWriter.stdinEnabled = false
    }
    onExited: function(code) {
      if (code === 0) registerProc.register()
    }
  }

  Process {
    id: registerProc
    stdout: StdioCollector { waitForEnd: true }
    stderr: StdioCollector { waitForEnd: true }
    function register() {
      if (!root.enabled) return
      if (running) running = false
      command = ["hyprctl", "eval", "dofile(" + JSON.stringify(root.hookPath) + ")"]
      running = true
      root._registered = true
    }
  }

  Process {
    id: unregisterProc
    command: ["hyprctl", "eval",
      "if _G.reel_shortcuts then _G.reel_shortcuts:remove(); _G.reel_shortcuts=nil end"]
    stdout: StdioCollector { waitForEnd: true }
    stderr: StdioCollector { waitForEnd: true }
    onExited: root._registered = false
  }

  FileView {
    id: eventFile
    path: root.eventPath
    watchChanges: true
    printErrors: false
    onFileChanged: reload()
    onLoaded: root.applyEvent(text())
    onLoadFailed: {}
  }

  Timer {
    interval: 1000
    repeat: true
    running: root.enabled && !eventFile.loaded
    onTriggered: eventFile.reload()
  }

  Timer {
    id: hideTimer
    interval: 1800
    repeat: false
    onTriggered: root.showing = false
  }

  // One surface per monitor, mapped only while the feature is live. An empty
  // input region keeps the toast visual-only: it never takes clicks or keys.
  Variants {
    model: Quickshell.screens

    PanelWindow {
      id: surface
      required property var modelData
      screen: modelData
      visible: root.enabled
      color: "transparent"
      exclusionMode: ExclusionMode.Ignore
      WlrLayershell.namespace: "reel-shortcuts"
      WlrLayershell.layer: WlrLayer.Overlay
      WlrLayershell.keyboardFocus: WlrKeyboardFocus.None
      // PanelWindow anchors only cover the four screen edges. Span the
      // bottom edge and centre the card inside the surface instead.
      anchors { bottom: true; left: true; right: true }
      // Room for the bottom gap and the entry slide, so the card is never
      // clipped by its own layer surface.
      implicitHeight: card.height + Style.space(56)
      mask: Region {}

      Item {
        id: cardWrap
        anchors.horizontalCenter: parent.horizontalCenter
        anchors.bottom: parent.bottom
        anchors.bottomMargin: Style.space(40)
        width: card.width
        height: card.height
        opacity: 0
        transform: Translate {
          id: lift
          y: Style.space(10)
        }

        states: [
          State {
            name: "shown"
            PropertyChanges { target: cardWrap; opacity: 1; restoreEntryValues: false }
            PropertyChanges { target: lift; y: 0; restoreEntryValues: false }
          },
          State {
            name: "hidden"
            PropertyChanges { target: cardWrap; opacity: 0; restoreEntryValues: false }
            PropertyChanges { target: lift; y: Style.space(10); restoreEntryValues: false }
          }
        ]
        state: root.showing ? "shown" : "hidden"

        transitions: Transition {
          from: "*"
          to: "*"
          NumberAnimation {
            target: cardWrap
            property: "opacity"
            duration: Style.duration(160)
            easing.type: Easing.Bezier
            easing.bezierCurve: Style.easeOut
          }
          NumberAnimation {
            target: lift
            property: "y"
            duration: Style.duration(200)
            easing.type: Easing.Bezier
            easing.bezierCurve: Style.easeOut
          }
        }

        Rectangle {
          id: card
          width: Math.max(Style.space(180), column.implicitWidth + Style.space(40))
          height: column.implicitHeight + Style.spacing.lg * 2
          radius: Style.cornerRadius
          color: Util.alpha(Commons.Color.background, 0.94)
          border.width: Math.max(1, Style.spacing.hairline)
          border.color: Util.alpha(Commons.Color.foreground, 0.16)

          Rectangle {
            width: Math.max(2, Style.spacing.xxs)
            height: card.height - Style.spacing.xl * 2
            anchors.left: parent.left
            anchors.leftMargin: Style.spacing.md
            anchors.verticalCenter: parent.verticalCenter
            radius: width / 2
            color: Commons.Color.accent
          }

          Column {
            id: column
            anchors.centerIn: parent
            spacing: Style.spacing.xs

            Text {
              id: keyText
              anchors.horizontalCenter: parent.horizontalCenter
              textFormat: Text.PlainText
              text: root.keys
              color: Commons.Color.accent
              font.family: Style.font.family
              font.pixelSize: Style.font.body
              font.bold: true
            }

            Text {
              id: labelText
              anchors.horizontalCenter: parent.horizontalCenter
              textFormat: Text.PlainText
              text: root.label
              color: Util.alpha(Commons.Color.foreground, 0.82)
              font.family: Style.font.family
              font.pixelSize: Style.font.bodySmall
            }
          }
        }
      }
    }
  }
}
