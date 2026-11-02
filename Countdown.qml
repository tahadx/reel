import QtQuick
import QtQuick.Shapes
import Quickshell
import Quickshell.Io
import Quickshell.Wayland
import qs.Commons
import qs.Commons as Commons
import qs.Ui

// A focused countdown ring shown between the capture picker and the encoder.
// The recorder writes the countdown start to state.json; this overlay follows
// the same clock and disappears the moment the take moves on.
Item {
  id: root

  property bool active: false
  property int remaining: 0
  property int total: 0
  property real startedAt: 0
  property real progress: 1
  property real visualStart: 0
  property real visualTotal: 0
  property real visualStep: 0

  readonly property string home: Quickshell.env("HOME") || ""
  readonly property string runtimeDir: {
    var runtime = Quickshell.env("XDG_RUNTIME_DIR") || ""
    if (runtime) return runtime + "/reel"
    return (Quickshell.env("XDG_STATE_HOME") || root.home + "/.local/state") + "/omarchy/reel/run"
  }
  readonly property string statePath: root.runtimeDir + "/state.json"

  readonly property color accent: Commons.Color.accent
  readonly property color foreground: Commons.Color.foreground
  readonly property color background: Commons.Color.background

  function applyState(text) {
    try {
      var state = JSON.parse(String(text || "{}") || "{}")
      var count = Number(state.settings && state.settings.countdown ? state.settings.countdown : 0)
      var started = Number(state.startedAt || 0)
      if (state.status === "starting" && state.stage === "countdown" && count > 0 && started > 0) {
        var fresh = !root.active || root.total !== count || root.startedAt !== started
        root.total = count
        root.startedAt = started
        if (fresh) {
          root.remaining = count
          root.progress = 1
          root.visualStart = 0
          root.visualTotal = 0
          root.visualStep = 0
        }
        root.active = true
        root.beginVisual()
        return
      }
    } catch (e) {
      // Treat a partial write as no countdown.
    }
    root.active = false
  }

  function beginVisual() {
    if (!root.active || root.visualStart > 0 || !overlay.contentRevealed) return
    var now = Date.now() / 1000
    var left = root.total - (now - root.startedAt)
    if (left <= 0.1) {
      root.active = false
      return
    }
    root.visualStart = now
    root.visualTotal = left
    root.visualStep = left / root.total
    root.remaining = root.total
    root.progress = 1
  }

  function advance() {
    if (root.visualStart <= 0 || root.visualTotal <= 0) return
    var elapsed = Date.now() / 1000 - root.visualStart
    if (elapsed >= root.visualTotal) {
      root.active = false
      return
    }
    var display = Math.max(1, root.total - Math.floor(elapsed / root.visualStep))
    if (display !== root.remaining) {
      root.remaining = display
      pulse.restart()
    }
    var span = root.visualTotal
    root.progress = span > 0 ? Math.max(0, Math.min(1, (span - elapsed) / span)) : 0
  }

  FileView {
    id: stateFile
    path: root.statePath
    watchChanges: true
    printErrors: false
    onFileChanged: reload()
    onLoaded: root.applyState(text())
    onLoadFailed: root.applyState("")
  }

  Timer {
    id: frameTimer
    interval: 16
    repeat: true
    running: root.active
    onTriggered: root.advance()
  }

  OverlayWindow {
    id: overlay
    shown: root.active
    WlrLayershell.namespace: "reel-countdown"

    onContentRevealedChanged: root.beginVisual()

    Rectangle {
      anchors.fill: parent
      color: Util.alpha(root.background, 0.28)
    }

    Item {
      anchors.centerIn: parent
      width: Style.space(200)
      height: width

      Rectangle {
        anchors.fill: ring
        anchors.margins: -Style.space(6)
        radius: width / 2
        color: Util.alpha("#000000", 0.24)
      }

      Rectangle {
        anchors.fill: ring
        anchors.margins: -Style.space(2)
        radius: width / 2
        color: Util.alpha("#000000", 0.18)
      }

      Rectangle {
        id: ring
        anchors.fill: parent
        radius: width / 2
        color: Util.alpha(root.background, 0.94)
        border.width: Math.max(1, Style.space(2))
        border.color: Util.alpha(root.foreground, 0.16)
      }

      Shape {
        anchors.fill: ring
        ShapePath {
          strokeColor: root.accent
          strokeWidth: Math.max(2, Style.space(3))
          fillColor: "transparent"
          capStyle: ShapePath.RoundCap
          PathAngleArc {
            centerX: ring.width / 2
            centerY: ring.height / 2
            radiusX: ring.width / 2 - Style.space(4)
            radiusY: ring.height / 2 - Style.space(4)
            startAngle: -90
            sweepAngle: 360 * root.progress
          }
        }
      }

      Column {
        anchors.centerIn: parent
        spacing: Style.space(2)

        Text {
          id: numberText
          anchors.horizontalCenter: parent.horizontalCenter
          textFormat: Text.PlainText
          text: String(root.remaining)
          color: root.accent
          font.family: Style.font.family
          font.pixelSize: Math.round(Style.font.display * 2.2)
          font.bold: true
          transformOrigin: Item.Center
        }

        Text {
          anchors.horizontalCenter: parent.horizontalCenter
          textFormat: Text.PlainText
          text: "Recording starts"
          color: Util.alpha(root.foreground, 0.68)
          font.family: Style.font.family
          font.pixelSize: Style.font.bodySmall
        }
      }
    }
  }

  SequentialAnimation {
    id: pulse
    NumberAnimation {
      target: numberText
      property: "scale"
      from: 1.12
      to: 1
      duration: 190
      easing.type: Easing.OutCubic
    }
  }
}
