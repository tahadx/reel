import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import Quickshell
import Quickshell.Hyprland
import Quickshell.Io
import Quickshell.Wayland
import qs.Commons
import qs.Commons as Commons
import qs.Ui
import "Model.js" as Model

// Reel: a bar widget, recording history, and a small settings page. The recording
// itself belongs to bin/reel; everything here drives it and reads its state.
Panel {
  id: root
  moduleName: "io.github.tahadx.reel"
  ipcTarget: "io.github.tahadx.reel"
  manageIpc: false

  readonly property string home: Quickshell.env("HOME") || ""
  readonly property var service: root.bar && root.bar.shell
    ? root.bar.shell.serviceFor(root.moduleName) : null

  readonly property var state: root.service ? root.service.state : Model.emptyState()
  readonly property var recents: root.service ? root.service.recents : []
  readonly property var visibleRecents: root.recents
  readonly property int elapsed: root.service ? root.service.elapsed : 0
  readonly property string status: Model.status(root.state)
  readonly property bool active: Model.isActive(root.state)
  readonly property bool starting: root.status === "starting"
  // The encoder is launched and warming up: the take runs, but the state only
  // becomes "recording" once its first file exists.
  readonly property bool capturing: root.starting
    && String(root.state.stage || "") === "capture"
  readonly property bool paused: root.status === "paused"
  readonly property bool finalizing: root.status === "finalizing"
  readonly property bool busy: Model.isBusy(root.state)
  readonly property bool barTimer: root.status === "recording" || root.starting || root.finalizing
  readonly property string outputDir: root.service && root.service.outputDir !== ""
    ? root.service.outputDir
    : root.home + "/Videos/Reel"

  readonly property color foreground: root.bar ? root.bar.foreground : Commons.Color.foreground
  readonly property color accent: Commons.Color.accent
  readonly property color urgent: Commons.Color.urgent
  readonly property color dim: Commons.Color.muted
  readonly property color accentSoft: Util.alpha(root.accent, 0.16)
  readonly property color urgentSoft: Util.alpha(root.urgent, 0.16)
  readonly property string fontFamily: root.bar ? root.bar.fontFamily : Style.font.family
  readonly property color onAccent: root.inkOn(root.accent)
  readonly property color onUrgent: root.inkOn(root.urgent)

  readonly property var defaults: ({
    "desktopAudio": "on",
    "microphoneAudio": "off",
    "cursor": "on",
    "webcam": "off",
    "slowmo": "off",
    "countdown": 0,
    "fps": 60,
    "quality": "very_high",
    "codec": "auto",
    "resolution": "0x0",
    "container": "mp4",
    "notifications": "off",
    "openWithOmacut": "on",
    "outputDir": "",
    "webcamDevice": ""
  })

  readonly property var primaryScreen: Quickshell.screens.length > 0 ? Quickshell.screens[0] : null
  readonly property var quickSettingRows: [
    { key: "desktopAudio", label: "Desktop audio", glyph: "󰕾" },
    { key: "microphoneAudio", label: "Microphone", glyph: "󰍬" },
    { key: "webcam", label: "Webcam corner", glyph: "󰄀" },
    { key: "cursor", label: "Record the pointer", glyph: "󰍽" },
    { key: "slowmo", label: "Slow motion", glyph: "󰔟" },
    { key: "countdown", label: "Countdown", glyph: "󰔛" }
  ]
  readonly property var advancedSettingRows: [
    { key: "fps", label: "Frame rate", glyph: "󰈫" },
    { key: "quality", label: "Quality", glyph: "󰓅" },
    { key: "codec", label: "Video codec", glyph: "󰈙" },
    { key: "resolution", label: "Resolution cap", glyph: "󰍹" },
    { key: "container", label: "Container", glyph: "󰉋" },
    { key: "notifications", label: "Saved notification", glyph: "󰂚" },
    { key: "openWithOmacut", label: "Open takes in Omacut", glyph: "󰐊" }
  ]

  // ---------- panel state -------------------------------------------------

  property string page: "home"
  property bool showShortcuts: false
  property bool typingPrefix: false
  property string focusSection: "primary"
  property int selectedIndex: 0
  property bool cursorActive: false
  property int settingsIndex: 0
  property bool hasOmacut: false

  property string notice: ""
  property bool noticeUrgent: false
  property string discardArmedPath: ""
  property string trashArmedPath: ""
  property bool discardArmed: false
  property bool cancelling: false
  property string pendingConfig: ""
  property string pushedSignature: ""
  property var lastActionArgs: []
  property string controlAction: ""
  property string pendingNotice: ""

  // The take actions menu lives at panel level rather than inside each row so
  // the keyboard cursor keeps working while it is open. It anchors to the
  // row's menu button and floats over the history list.
  property bool takeMenuOpen: false
  property var takeMenuRow: null
  property int takeMenuIndex: 0
  property real takeMenuX: 0
  property real takeMenuY: 0
  readonly property real takeMenuWidth: Style.space(198)
  readonly property real takeMenuRowHeight: Style.space(32)
  readonly property real takeMenuPadding: Style.space(4)
  readonly property var takeMenuBorderSpec: Border.flat(root.dim, 1)
  readonly property var takeMenuItems: [
    { key: "slowmo", label: "Slow motion", glyph: "󰔟" },
    { key: "share", label: "Share", glyph: "󰒗" },
    {
      key: "trash",
      label: root.trashArmedPath !== "" && root.takeMenuRow
        && root.trashArmedPath === String(root.takeMenuRow.path || "")
        ? "Confirm delete" : "Delete",
      glyph: "󰆴",
      urgent: root.trashArmedPath !== "" && root.takeMenuRow
        && root.trashArmedPath === String(root.takeMenuRow.path || "")
    }
  ]
  readonly property real takeMenuHeight: root.takeMenuRowHeight * root.takeMenuItems.length
    + root.takeMenuPadding * 2
    + Border.top(root.takeMenuBorderSpec) + Border.bottom(root.takeMenuBorderSpec)

  // A take can be armed for a second press, but not for long.
  Timer {
    id: armTimer
    interval: 3500
    repeat: false
    onTriggered: {
      root.discardArmed = false
      root.discardArmedPath = ""
      root.trashArmedPath = ""
    }
  }

  Timer {
    id: noticeTimer
    interval: 4500
    repeat: false
    onTriggered: root.notice = ""
  }

  function setNotice(text, urgent) {
    root.notice = String(text || "")
    root.noticeUrgent = !!urgent
    if (root.notice !== "") noticeTimer.restart()
  }

  // ---------- colours -----------------------------------------------------

  function channelLinear(value) {
    return value <= 0.04045 ? value / 12.92 : Math.pow((value + 0.055) / 1.055, 2.4)
  }
  function relLuminance(color) {
    return 0.2126 * root.channelLinear(color.r) + 0.7152 * root.channelLinear(color.g)
      + 0.0722 * root.channelLinear(color.b)
  }
  function contrastRatio(a, b) {
    var hi = Math.max(a, b)
    var lo = Math.min(a, b)
    return (hi + 0.05) / (lo + 0.05)
  }
  function inkOn(fill) {
    var base = root.relLuminance(fill)
    var background = Commons.Color.background
    return root.contrastRatio(base, root.relLuminance(background))
      >= root.contrastRatio(base, root.relLuminance(root.foreground)) ? background : root.foreground
  }

  // ---------- settings ----------------------------------------------------

  function valueString(key, fallback) {
    var value = root.setting(key, fallback)
    return value === undefined || value === null || value === "" ? String(fallback) : String(value)
  }

  function optionList(key) {
    switch (key) {
    case "desktopAudio": return ["on", "off"]
    case "microphoneAudio": return ["on", "off"]
    case "cursor": return ["on", "off"]
    case "webcam": return ["off", "small", "medium", "large"]
    case "slowmo": return ["off", "0.5", "0.25"]
    case "countdown": return ["0", "3", "5", "10"]
    case "fps": return ["30", "60", "120"]
    case "quality": return ["very_low", "low", "medium", "high", "very_high", "ultra"]
    case "codec": return ["auto", "h264", "hevc", "av1"]
    case "resolution": return ["0x0", "3840x2160", "2560x1440", "1920x1080", "1280x720"]
    case "container": return ["mp4", "mkv"]
    case "notifications": return ["on", "off"]
    case "openWithOmacut": return ["on", "off"]
    default: return [String(root.defaults[key] || "")]
    }
  }

  function qualityWord(value) {
    switch (String(value)) {
    case "1000": return "very_low"
    case "2000": return "low"
    case "4000": return "medium"
    case "8000": return "high"
    case "16000": return "very_high"
    case "32000": return "ultra"
    default: return ""
    }
  }

  function currentValue(key) {
    var list = root.optionList(key)
    var raw = root.valueString(key, root.defaults[key])
    if (key === "quality") {
      var migrated = root.qualityWord(raw)
      if (migrated !== "") return migrated
    }
    if (list.indexOf(raw) >= 0) return raw
    // Older configs stored booleans. Read those as on/off rather than
    // snapping the row to its fallback.
    if ((key === "desktopAudio" || key === "microphoneAudio" || key === "cursor"
        || key === "notifications" || key === "openWithOmacut")
        && (list.indexOf("on") >= 0)) {
      return Model.truthy(raw) ? "on" : "off"
    }
    var fallback = String(root.defaults[key])
    return list.indexOf(fallback) >= 0 ? fallback : list[0]
  }

  function valueLabel(key, value) {
    switch (key) {
    case "desktopAudio":
    case "microphoneAudio":
    case "cursor":
    case "notifications":
    case "openWithOmacut":
      return value === "on" ? "On" : "Off"
    case "webcam":
      return value === "off" ? "Off" : value.charAt(0).toUpperCase() + value.slice(1)
    case "slowmo":
      return value === "off" ? "Off" : value + "x"
    case "countdown":
      return value === "0" ? "Off" : value + "s"
    case "fps":
      return value + " fps"
    case "quality":
      return value === "very_low" ? "Very low"
        : value === "low" ? "Low"
        : value === "medium" ? "Medium"
        : value === "high" ? "High"
        : value === "very_high" ? "Very high"
        : "Ultra"
    case "codec":
      return value === "auto" ? "Auto" : value === "h264" ? "H.264" : value.toUpperCase()
    case "resolution":
      return value === "0x0" ? "Native"
        : value === "3840x2160" ? "4K"
        : value === "2560x1440" ? "1440p"
        : value === "1920x1080" ? "1080p"
        : "720p"
    case "container":
      return value.toUpperCase()
    default:
      return String(value)
    }
  }

  function writeSetting(key, value) {
    var entry = { id: root.moduleName }
    var source = root.settings || ({})
    for (var k in source) if (k !== "id") entry[k] = source[k]
    entry[key] = value
    root.settings = entry

    var signatureEntries = ({})
    for (var s in entry) if (s !== "id") signatureEntries[s] = entry[s]
    root.pushedSignature = JSON.stringify(signatureEntries)

    if (root.bar && root.bar.shell && typeof root.bar.shell.updateEntryInline === "function")
      root.bar.shell.updateEntryInline(root.moduleName, entry)
    root.queueConfig(entry)
  }

  function queueConfig(entry) {
    var payload = ({})
    for (var k in entry) if (k !== "id") payload[k] = entry[k]
    root.pendingConfig = JSON.stringify(payload)
    root.flushConfig()
  }

  function flushConfig() {
    if (root.pendingConfig === "" || configProc.running) return
    var payload = root.pendingConfig
    root.pendingConfig = ""
    configProc.command = ["reel", "config", "apply", payload]
    configProc.running = true
  }

  function cycleOption(key, direction) {
    var list = root.optionList(key)
    var next = Model.cycle(list, root.currentValue(key), direction)
    var value = (key === "countdown" || key === "fps") ? Number(next) : next
    root.writeSetting(key, value)
  }

  function syncConfig() {
    var payload = ({})
    var source = root.settings || ({})
    for (var k in source) if (k !== "id") payload[k] = source[k]
    var signature = JSON.stringify(payload)
    if (signature === root.pushedSignature) return
    root.pushedSignature = signature
    root.queueConfig(source)
  }

  // ---------- labels ------------------------------------------------------

  function targetLabel(target) {
    if (root.state && root.state.targetLabel) return String(root.state.targetLabel)
    var value = String(target || "")
    if (value.indexOf("monitor:") === 0) return value.slice(8)
    if (value.indexOf("region:") === 0) {
      var match = /^region:(\d+)x(\d+)\+(-?\d+)\+(-?\d+)$/.exec(value)
      if (match) return match[1] + "×" + match[2] + " at " + match[3] + "," + match[4]
    }
    return value === "portal" ? "portal" : "Capture"
  }

  // Crop the preview only for the take that is running now. Once the take
  // ends, go back to the focused monitor instead of showing an older selection.
  function previewTarget() {
    var state = root.state || ({})
    var status = String(state.status || "")
    if (status !== "recording" && status !== "paused" && status !== "starting") return ""
    return state.target ? String(state.target) : ""
  }

  function previewPlan() {
    var screens = Quickshell.screens || []
    var target = root.previewTarget()
    if (target !== "") {
      var state = root.state || ({})
      return Model.capturePlan(target, screens, String(state.targetKind || ""))
    }
    var focused = Hyprland.focusedMonitor ? String(Hyprland.focusedMonitor.name || "") : ""
    return Model.idlePlan(screens, focused)
  }

  function previewTitle(plan) {
    if (!plan) return "Preview unavailable"
    if (plan.mode === "idle") return "Full monitor preview"
    var state = root.state || ({})
    if (root.previewTarget() !== "" && state.targetLabel) {
      var label = String(state.targetLabel)
      return plan.kind === "monitor" ? label.replace(/ · focused$/, "") : label
    }
    return plan.title ? String(plan.title) : "Capture target"
  }

  function audioSummary() {
    var desktop = root.currentValue("desktopAudio") === "on"
    var mic = root.currentValue("microphoneAudio") === "on"
    if (desktop && mic) return "System audio + microphone"
    if (desktop) return "System audio"
    if (mic) return "Microphone"
    return "No audio"
  }

  function destinationLabel() {
    return Model.shortPath(root.currentValue("outputDir") || root.outputDir, root.home)
  }

  function barGlyph() {
    // Idle and recording use the drawn icon or the timer alone. Only pause
    // keeps a glyph, because the bar should say "paused" at a glance.
    return root.paused ? "󰏤" : ""
  }

  function barLabel() {
    if (root.finalizing) return "saving"
    if (root.starting) {
      var stage = String(root.state.stage || "")
      if (stage === "capture") return Model.clock(root.elapsed)
      return stage === "countdown" ? "…" : "pick"
    }
    if (root.paused) return ""
    if (root.active) return Model.clock(root.elapsed)
    return ""
  }

  function barTooltip() {
    if (root.finalizing) return "Reel · saving the take"
    if (root.paused) return "Reel · paused at " + Model.clock(root.elapsed) + " · right-click to resume"
    if (root.capturing) return "Reel · recording " + Model.clock(root.elapsed)
    if (root.starting) return "Reel · waiting for a selection"
    if (root.active) return "Reel · recording " + Model.clock(root.elapsed) + " · right-click to pause"
    return "Reel · click to open, right-click to record"
  }

  function stateMeta() {
    if (root.page === "settings") return "settings · s toggles · esc goes back · q closes"
    if (root.page === "history") {
      if (root.visibleRecents.length === 0) return "no takes yet · open the recordings folder · esc goes back"
      return root.visibleRecents.length + " takes · enter opens · a actions · esc goes back"
    }
    if (root.finalizing) return "trimming the first frame · normalizing audio"
    if (root.paused) return "paused at " + Model.clock(root.elapsed) + " · press p to resume"
    if (root.starting) {
      var stage = String(root.state.stage || "select")
      if (stage === "capture") return Model.clock(root.elapsed) + " · recording"
      if (stage === "countdown") return "starting in a moment"
      return "click a window or monitor, or drag an area"
    }
    if (root.active) return Model.clock(root.elapsed) + " · recording"
    return "ready · pick a window, area, or monitor · " + root.currentValue("fps") + " fps"
  }

  function primaryLabel() {
    if (root.finalizing) return "Saving…"
    if (root.starting) return "Cancel"
    if (root.active) return "Stop & save"
    return "Record"
  }

  function primaryGlyph() {
    if (root.finalizing) return "󰔟"
    if (root.starting) return "󰅖"
    if (root.active) return "󰓛"
    return "󰻂"
  }

  // ---------- actions -----------------------------------------------------

  function startArgs() {
    return [
      "reel", "start",
      "--desktop-audio", root.currentValue("desktopAudio"),
      "--microphone-audio", root.currentValue("microphoneAudio"),
      "--cursor", root.currentValue("cursor"),
      "--webcam", root.currentValue("webcam"),
      "--webcam-device", root.valueString("webcamDevice", ""),
      "--slowmo", root.currentValue("slowmo"),
      "--countdown", String(root.currentValue("countdown")),
      "--fps", String(root.currentValue("fps")),
      "--quality", root.currentValue("quality"),
      "--codec", root.currentValue("codec"),
      "--resolution", root.currentValue("resolution"),
      "--container", root.currentValue("container"),
      "--output-dir", root.valueString("outputDir", ""),
      "--filename-prefix", root.valueString("filenamePrefix", "")
    ]
  }

  // Open the picker, then count down and record. The panel closes first so the
  // picker owns the screen; it stays closed while the take runs.
  function start() {
    if (controlProc.running) {
      root.setNotice("Reel is still finishing the last command.")
      return
    }
    root.startImmediate()
  }

  // The IPC path goes through the same picker without a panel press.
  function startImmediate() {
    if (controlProc.running) return
    root.cancelling = false
    root.controlAction = "start"
    root.lastActionArgs = root.startArgs()
    root.close()
    controlProc.command = root.lastActionArgs
    controlProc.running = true
  }

  function stop() {
    if (root.finalizing) return
    root.cancelling = root.starting
    if (stopProc.running) return
    stopProc.command = ["reel", "stop"]
    stopProc.running = true
  }

  function discard() {
    if (!root.active) return
    if (!root.discardArmed) {
      root.discardArmed = true
      root.setNotice("Discard this take? Press discard again within a few seconds.", true)
      armTimer.restart()
      return
    }
    root.discardArmed = false
    root.discardNow()
  }

  // The armed path protects the mouse and keyboard. IPC is already an explicit
  // instruction, so it goes straight through.
  function discardNow() {
    root.cancelling = false
    if (stopProc.running) return
    stopProc.command = ["reel", "stop", "--discard"]
    stopProc.running = true
  }

  function togglePause() {
    if (!root.active) return
    if (controlProc.running) {
      root.setNotice("Reel is still finishing the last command.")
      return
    }
    root.cancelling = false
    root.controlAction = "pause"
    controlProc.command = ["reel", "toggle-pause"]
    controlProc.running = true
  }

  function toggleRecording() {
    if (root.active || root.starting) root.stop()
    else if (!root.finalizing) root.start()
  }

  function openVideo(row) {
    var command = Model.openVideoCommand(row && row.path, root.hasOmacut,
      root.currentValue("openWithOmacut") === "on")
    if (command.length > 0) Quickshell.execDetached(command)
  }

  function openFolder() {
    Quickshell.execDetached(["xdg-open", root.outputDir])
  }

  function copyPath(path) {
    if (!path) return
    Quickshell.execDetached(["wl-copy", String(path)])
    root.setNotice("Path copied.")
  }

  function slowmo(row) {
    if (!row || !row.path) return
    if (slowmoProc.running) return
    slowmoProc.command = ["reel", "slowmo", String(row.path), "0.5"]
    slowmoProc.running = true
    root.setNotice("Building a half-speed copy…")
  }

  function share(row) {
    if (!row || !row.path) return
    if (shareProc.running) return
    shareProc.command = ["omarchy", "share", "file", String(row.path)]
    shareProc.running = true
    root.setNotice("Sharing " + Model.takeTitle(row) + "…")
  }

  // ---------- take actions menu ------------------------------------------

  function openTakeMenu(row, rowIndex, anchor) {
    if (!row || !anchor) return
    var surface = keyboardPanel.loadedContent
    if (!surface) return
    root.cursorActive = true
    root.focusSection = "recents"
    root.selectedIndex = rowIndex

    var point = anchor.mapToItem(surface, 0, 0)
    var margin = Style.space(8)
    var gap = Style.space(4)
    var x = point.x + anchor.width - root.takeMenuWidth
    x = Math.max(margin, Math.min(x, surface.width - root.takeMenuWidth - margin))
    var y = point.y + anchor.height + gap
    if (y + root.takeMenuHeight > surface.height - margin)
      y = point.y - root.takeMenuHeight - gap
    y = Math.max(margin, Math.min(y, surface.height - root.takeMenuHeight - margin))

    root.takeMenuX = x
    root.takeMenuY = y
    root.takeMenuRow = row
    root.takeMenuIndex = 0
    root.takeMenuOpen = true
  }

  function closeTakeMenu() {
    if (!root.takeMenuOpen) return
    root.takeMenuOpen = false
    root.takeMenuRow = null
    root.takeMenuIndex = 0
  }

  function moveTakeMenu(delta) {
    if (!root.takeMenuOpen || delta === 0) return
    var count = root.takeMenuItems.length
    root.takeMenuIndex = (root.takeMenuIndex + delta + count) % count
  }

  function activateTakeMenu() {
    var row = root.takeMenuRow
    if (!root.takeMenuOpen || !row) return
    var item = root.takeMenuItems[root.takeMenuIndex]
    var key = item ? String(item.key || "") : ""
    if (key === "slowmo") {
      root.closeTakeMenu()
      root.slowmo(row)
    } else if (key === "share") {
      root.closeTakeMenu()
      root.share(row)
    } else if (key === "trash") {
      var path = String(row.path || "")
      if (path === "") return
      if (root.trashArmedPath !== path) {
        root.trashArmedPath = path
        root.setNotice("Choose Delete again to move " + Model.takeTitle(row) + " to the trash.", true)
        armTimer.restart()
        return
      }
      root.closeTakeMenu()
      root.trashTake(row)
    }
  }

  function selectedTake() {
    if (root.page !== "history" || root.focusSection !== "recents") return null
    var index = root.selectedIndex - 1
    if (index < 0 || index >= root.visibleRecents.length) return null
    return root.visibleRecents[index]
  }

  function trashTake(row) {
    if (!row || !row.path) return
    if (root.trashArmedPath !== String(row.path)) {
      root.trashArmedPath = String(row.path)
      root.setNotice("Press x again to move " + Model.takeTitle(row) + " to the trash.", true)
      armTimer.restart()
      return
    }
    root.trashArmedPath = ""
    if (trashProc.running) return
    trashProc.command = ["reel", "trash", String(row.path)]
    trashProc.running = true
  }

  function trashSelected() {
    root.trashTake(root.selectedTake())
  }

  function slowmoSelected() {
    var row = root.selectedTake()
    if (row) root.slowmo(row)
  }

  function shareSelected() {
    var row = root.selectedTake()
    if (row) root.share(row)
  }

  function primaryAction(index) {
    if (index === 0) {
      if (root.finalizing) return
      if (root.active || root.starting) root.stop()
      else root.start()
    } else if (index === 1) {
      root.togglePause()
    } else if (index === 2) {
      root.discard()
    }
  }

  // ---------- cursor ------------------------------------------------------

  readonly property var sections: root.page === "history"
    ? ["header", "recents"]
    : ["header", "primary", "quick"]

  function sectionCount(section) {
    switch (section) {
    case "header": return 2
    case "primary": return 3
    case "quick": return root.quickSettingRows.length
    case "recents": return Math.max(1, root.visibleRecents.length + 1)
    default: return 0
    }
  }

  function moveSection(delta) {
    if (root.page === "settings") {
      root.moveSettings(delta, 0)
      return
    }
    var index = root.sections.indexOf(root.focusSection)
    if (index < 0) index = 0
    index = (index + delta + root.sections.length) % root.sections.length
    root.focusSection = root.sections[index]
    root.selectedIndex = delta > 0 ? 0 : root.sectionCount(root.focusSection) - 1
  }

  function moveCursor(dx, dy) {
    if (root.page === "settings") {
      root.moveSettings(dy, dx)
      return
    }
    if (!root.cursorActive) {
      root.cursorActive = true
      return
    }
    if (dx !== 0) {
      root.adjust(dx)
      return
    }
    if (dy === 0) return
    if (root.focusSection === "quick" || root.focusSection === "recents") {
      var count = root.sectionCount(root.focusSection)
      var next = root.selectedIndex + dy
      if (next >= 0 && next < count) {
        root.selectedIndex = next
        return
      }
    }
    root.moveSection(dy)
  }

  function adjust(direction) {
    if (root.focusSection === "header") {
      var headers = root.sectionCount("header")
      root.selectedIndex = (root.selectedIndex + direction + headers) % headers
      return
    }
    if (root.focusSection === "primary") {
      var count = root.sectionCount(root.focusSection)
      root.selectedIndex = (root.selectedIndex + direction + count) % count
      return
    }
    if (root.focusSection === "quick") {
      var row = root.quickSettingRows[root.selectedIndex]
      if (row) root.cycleOption(row.key, direction)
      return
    }
  }

  function activateCursor() {
    if (root.page === "settings") {
      root.activateSettings()
      return
    }
    switch (root.focusSection) {
    case "header":
      if (root.selectedIndex === 0) root.toggleHistory()
      else root.toggleSettings()
      break
    case "primary":
      root.primaryAction(root.selectedIndex)
      break
    case "quick":
      var row = root.quickSettingRows[root.selectedIndex]
      if (row) root.cycleOption(row.key, 1)
      break
    case "recents":
      if (root.selectedIndex === 0) root.openFolder()
      else {
        var take = root.visibleRecents[root.selectedIndex - 1]
        if (take) root.openVideo(take)
      }
      break
    }
  }

  function onTextKey(text) {
    var key = String(text || "").toLowerCase()
    if (root.takeMenuOpen) return
    if (root.page === "settings" && root.typingPrefix) return
    if (key === "q") {
      root.close()
      return
    }
    if (key === "s") {
      root.toggleSettings()
      return
    }
    if (root.page === "settings") return
    if (key === "r") root.toggleRecording()
    else if (key === "p") root.togglePause()
    else if (key === "d") root.discard()
    else if (key === "o") root.openFolder()
    else if (key === "m") root.slowmoSelected()
    else if (key === "u") root.shareSelected()
    else if (key === "a") {
      if (keyboardPanel.loadedContent
          && typeof keyboardPanel.loadedContent.openTakeMenuForSelected === "function")
        keyboardPanel.loadedContent.openTakeMenuForSelected()
    }
    else if (key === "c") {
      var row = root.selectedTake()
      if (row) root.copyPath(row.path)
    }
  }

  function back() {
    if (root.page === "settings") {
      root.closeSettings()
      return
    }
    if (root.page === "history") {
      root.closeHistory()
      return
    }
    // Escape cancels a selection or countdown, but never an active take. The
    // panel itself may close while recording without stopping it.
    if (root.starting) {
      root.stop()
      return
    }
    root.close()
  }

  // ---------- settings page cursor ---------------------------------------

  function openHistory() {
    if (root.page === "history") return
    if (!omacutProbe.running) omacutProbe.running = true
    var area = root.scrollArea()
    var flick = area ? area.contentItem : null
    if (flick && flick.contentY !== undefined) flick.contentY = 0
    root.page = "history"
    root.focusSection = "recents"
    root.selectedIndex = 0
    root.cursorActive = true
    Qt.callLater(root.focusPanelKeys)
  }

  function closeHistory() {
    root.page = "home"
    root.focusSection = "primary"
    root.selectedIndex = 0
    root.cursorActive = true
    Qt.callLater(root.focusPanelKeys)
  }

  function toggleHistory() {
    if (root.page === "history") root.closeHistory()
    else root.openHistory()
  }

  function openSettings() {
    root.page = "settings"
    root.typingPrefix = false
    root.settingsIndex = 0
    root.cursorActive = true
    Qt.callLater(root.focusPanelKeys)
  }

  function closeSettings() {
    root.page = "home"
    root.typingPrefix = false
    root.focusSection = "header"
    root.selectedIndex = 0
    Qt.callLater(root.focusPanelKeys)
  }

  function focusPanelKeys() {
    if (keyboardPanel.loadedContent) keyboardPanel.loadedContent.forceActiveFocus()
  }

  function toggleSettings() {
    if (root.page === "settings") root.closeSettings()
    else root.openSettings()
  }

  function settingsCount() {
    return root.advancedSettingRows.length
  }

  function moveSettings(dy, dx) {
    if (!root.cursorActive) {
      root.cursorActive = true
      return
    }
    if (dy !== 0) {
      root.settingsIndex = Model.clamp(root.settingsIndex + dy, 0, root.settingsCount() - 1)
      return
    }
    if (dx !== 0 && root.settingsIndex < root.advancedSettingRows.length) {
      var row = root.advancedSettingRows[root.settingsIndex]
      root.cycleOption(row.key, dx)
    }
  }

  function activateSettings() {
    var row = root.advancedSettingRows[root.settingsIndex]
    if (row) root.cycleOption(row.key, 1)
  }

  // ---------- scroll ------------------------------------------------------

  function scrollArea() {
    if (!keyboardPanel.loadedContent) return null
    return keyboardPanel.loadedContent.scrollArea || null
  }

  function resetScroll() {
    var area = root.scrollArea()
    var flick = area ? area.contentItem : null
    if (flick && flick.contentY !== undefined) flick.contentY = 0
  }

  function ensureCursorVisible(item) {
    var area = root.scrollArea()
    var flick = area ? area.contentItem : null
    if (!item || !flick || flick.contentY === undefined) return
    var margin = 6
    var maxY = Math.max(0, (flick.contentHeight || 0) - flick.height)
    if (maxY <= Style.space(24)) {
      flick.contentY = 0
      return
    }
    var point = item.mapToItem(flick.contentItem || flick, 0, 0)
    var top = point.y
    var bottom = top + (item.height || 0)
    var viewTop = flick.contentY
    var viewBottom = viewTop + flick.height
    if (top < viewTop + margin) flick.contentY = Math.max(0, Math.min(maxY, top - margin))
    else if (bottom > viewBottom - margin) flick.contentY = Math.max(0, Math.min(maxY, bottom + margin - flick.height))
  }

  // ---------- lifecycle ---------------------------------------------------

  onPageChanged: {
    if (root.takeMenuOpen) root.closeTakeMenu()
  }

  onRecentsChanged: {
    if (root.page === "history" && root.focusSection === "recents")
      root.selectedIndex = Math.max(0, Math.min(root.selectedIndex, root.sectionCount("recents") - 1))
  }

  onOpenedChanged: {
    if (opened) {
      root.page = "home"
      root.focusSection = "primary"
      root.selectedIndex = 0
      root.cursorActive = false
      root.settingsIndex = 0
      root.showShortcuts = false
      root.notice = ""
      root.discardArmed = false
      root.trashArmedPath = ""
      root.takeMenuOpen = false
      root.takeMenuRow = null
      root.takeMenuIndex = 0
      root.syncConfig()
      if (root.service) root.service.refresh()
      Qt.callLater(root.resetScroll)
    } else {
      root.discardArmed = false
      root.trashArmedPath = ""
      root.takeMenuOpen = false
      root.takeMenuRow = null
      root.takeMenuIndex = 0
    }
  }

  implicitWidth: button.implicitWidth
  implicitHeight: button.implicitHeight

  FontMetrics {
    id: barLabelMetrics
    font.family: root.fontFamily
    font.pixelSize: Style.bar.iconFont
  }

  // The idle mark is drawn rather than borrowed from the icon font: a small
  // screen outline with a record dot. It keeps its shape at every bar size and
  // picks up the bar foreground plus the theme accent. While a take is
  // running the mark gives way to the clock, and pause keeps the pause glyph,
  // so the bar always answers "is it recording?" without opening the panel.
  WidgetButton {
    id: button
    anchors.fill: parent
    bar: root.bar
    text: ""
    hasVisualContent: true
    fixedWidth: root.barTimer
      ? barLabelMetrics.advanceWidth(root.barLabel())
        + (root.status === "recording" || root.capturing
          ? Style.space(30) : Style.space(20))
      : Style.bar.iconSlot
    active: root.active || root.starting || root.finalizing
    tooltipText: root.barTooltip()
    onPressed: function(button) {
      if (button === Qt.RightButton) {
        if (root.active) root.togglePause()
        else if (!root.starting && !root.finalizing) root.start()
        else root.toggle()
      } else if (button === Qt.MiddleButton) {
        root.openFolder()
      } else {
        root.toggle()
      }
    }

    Item {
      visible: !root.busy
      anchors.centerIn: parent
      width: Style.bar.iconCanvas
      height: Style.bar.iconCanvas
      readonly property real unit: width / 24

      Rectangle {
        id: screenOutline
        x: 2.5 * parent.unit
        y: 4 * parent.unit
        width: 19 * parent.unit
        height: 15 * parent.unit
        radius: 4.5 * parent.unit
        color: "transparent"
        border.width: Math.max(1, Math.round(1.6 * parent.unit))
        border.color: root.foreground
        opacity: 0.9
      }

      Rectangle {
        anchors.centerIn: screenOutline
        width: 6.4 * parent.unit
        height: 6.4 * parent.unit
        radius: 3.2 * parent.unit
        color: root.foreground
      }

      Rectangle {
        x: 9.5 * parent.unit
        y: 21 * parent.unit
        width: 5 * parent.unit
        height: Math.max(1, 1.6 * parent.unit)
        radius: Math.max(0.5, 0.8 * parent.unit)
        color: root.foreground
        opacity: 0.55
      }
    }

    Text {
      visible: root.paused
      anchors.centerIn: parent
      textFormat: Text.PlainText
      text: root.barGlyph()
      color: root.foreground
      font.family: root.fontFamily
      font.pixelSize: Style.bar.iconFont
    }

    Row {
      visible: root.barTimer
      anchors.centerIn: parent
      spacing: Style.space(6)

      // The capture warmup is a real take, so the recording dot appears as
      // soon as the clock starts rather than a second or two later.
      Rectangle {
        visible: root.status === "recording" || root.capturing
        width: Style.space(6)
        height: width
        radius: width / 2
        anchors.verticalCenter: parent.verticalCenter
        color: root.foreground

        SequentialAnimation on opacity {
          running: root.status === "recording" || root.capturing
          loops: Animation.Infinite
          NumberAnimation { to: 0.3; duration: 700; easing.type: Easing.InOutQuad }
          NumberAnimation { to: 1.0; duration: 700; easing.type: Easing.InOutQuad }
        }
      }

      Text {
        textFormat: Text.PlainText
        text: root.barLabel()
        color: root.status === "recording" || root.capturing
          ? root.urgent : root.finalizing ? root.dim : root.foreground
        font.family: root.fontFamily
        font.pixelSize: Style.bar.iconFont
        font.bold: root.status === "recording" || root.capturing
        anchors.verticalCenter: parent.verticalCenter
      }
    }
  }

  KeyboardPanel {
    id: keyboardPanel
    anchorItem: button
    owner: root
    bar: root.bar
    open: root.opened
    focusTarget: keyboardPanel.loadedContent
    contentWidth: keyboardPanel.fittedContentWidth(Style.space(510))
    contentHeight: keyboardPanel.fittedContentHeight(
      keyboardPanel.loadedContent ? keyboardPanel.loadedContent.implicitHeight : Style.space(480),
      Style.space(720))

    content: ReelKeyCatcher {
      id: keyCatcher
      property alias scrollArea: scrollArea
      implicitHeight: contentColumn.implicitHeight
      anchors.fill: parent
      blocked: root.page === "settings" && root.typingPrefix

      onMoveRequested: function(dx, dy) {
        if (root.takeMenuOpen) root.moveTakeMenu(dy)
        else root.moveCursor(dx, dy)
      }
      onActivateRequested: {
        if (root.takeMenuOpen) root.activateTakeMenu()
        else root.activateCursor()
      }
      onCloseRequested: {
        if (root.takeMenuOpen) root.closeTakeMenu()
        else root.back()
      }
      onDeleteRequested: {
        if (root.takeMenuOpen) root.closeTakeMenu()
        if (root.page === "history") root.trashSelected()
      }
      onTabRequested: function(direction) {
        if (!root.takeMenuOpen) root.moveSection(direction)
      }
      onTextKey: function(text) { root.onTextKey(text) }
      onHistoryRequested: {
        if (root.takeMenuOpen) root.closeTakeMenu()
        else root.toggleHistory()
      }
      ScrollView {
        id: scrollArea
        anchors.fill: parent
        clip: true
        ScrollBar.horizontal.policy: ScrollBar.AlwaysOff
        ScrollBar.vertical.policy: ScrollBar.AlwaysOff
        Binding {
          target: scrollArea.contentItem
          property: "interactive"
          value: contentColumn.implicitHeight > scrollArea.height
        }

        Column {
          id: contentColumn
          width: scrollArea.availableWidth
          spacing: Style.space(12)

          // ---------- hero ----------

          PanelHero {
            iconComponent: Component {
              // The same drawn mark the bar uses, at display size. Recording
              // pulses the dot, pause splits it into two bars, and the save
              // pass dims it, so the header matches the bar state.
              Item {
                id: heroMark
                width: Style.font.display
                height: Style.font.display
                readonly property real unit: width / 24
                readonly property color markColor: root.finalizing
                  ? root.dim : root.foreground

                Rectangle {
                  id: heroScreen
                  x: 2.5 * heroMark.unit
                  y: 4 * heroMark.unit
                  width: 19 * heroMark.unit
                  height: 15 * heroMark.unit
                  radius: 4.5 * heroMark.unit
                  color: "transparent"
                  border.width: Math.max(1, Math.round(1.5 * heroMark.unit))
                  border.color: root.foreground
                  opacity: 0.9
                }

                // centerIn rounds against the fractional screen geometry and
                // leaves the mark a pixel off, so center from the dimensions.
                Row {
                  visible: root.paused
                  x: heroScreen.x + (heroScreen.width - width) / 2
                  y: heroScreen.y + (heroScreen.height - height) / 2
                  spacing: 2.2 * heroMark.unit

                  Rectangle {
                    width: 2.6 * heroMark.unit
                    height: 7 * heroMark.unit
                    radius: 1 * heroMark.unit
                    color: root.foreground
                  }

                  Rectangle {
                    width: 2.6 * heroMark.unit
                    height: 7 * heroMark.unit
                    radius: 1 * heroMark.unit
                    color: root.foreground
                  }
                }

                Rectangle {
                  visible: !root.paused
                  x: heroScreen.x + (heroScreen.width - width) / 2
                  y: heroScreen.y + (heroScreen.height - height) / 2
                  width: 6.4 * heroMark.unit
                  height: width
                  radius: width / 2
                  color: heroMark.markColor

                  SequentialAnimation on opacity {
                    running: root.status === "recording"
                    loops: Animation.Infinite
                    NumberAnimation { to: 0.3; duration: 700; easing.type: Easing.InOutQuad }
                    NumberAnimation { to: 1.0; duration: 700; easing.type: Easing.InOutQuad }
                  }
                }

                Rectangle {
                  x: 9.5 * heroMark.unit
                  y: 21 * heroMark.unit
                  width: 5 * heroMark.unit
                  height: Math.max(1, 1.6 * heroMark.unit)
                  radius: Math.max(0.5, 0.8 * heroMark.unit)
                  color: root.foreground
                  opacity: 0.55
                }
              }
            }
            title: root.page === "settings" ? "Reel settings"
              : root.page === "history" ? "Reel history" : "Reel"
            meta: root.stateMeta()
            foreground: root.foreground
            fontFamily: root.fontFamily
            trailingControl: Component {
              Row {
                spacing: Style.space(2)

                PanelActionButton {
                  iconText: "󰋚"
                  tooltipText: root.page === "history" ? "Back from history (Shift+H)" : "History (Shift+H)"
                  foreground: root.page === "history" ? root.accent : root.foreground
                  fontFamily: root.fontFamily
                  hasCursor: root.cursorActive && root.focusSection === "header"
                    && root.selectedIndex === 0
                  onHovered: function(on) {
                    if (!on) return
                    root.cursorActive = true
                    root.focusSection = "header"
                    root.selectedIndex = 0
                  }
                  onClicked: root.toggleHistory()
                }

                PanelActionButton {
                  iconText: "󰒓"
                  tooltipText: root.page === "settings" ? "Back to takes (s)" : "Settings (s)"
                  foreground: root.page === "settings" ? root.accent : root.foreground
                  fontFamily: root.fontFamily
                  hasCursor: root.cursorActive && root.focusSection === "header"
                    && root.selectedIndex === 1
                  onHovered: function(on) {
                    if (!on) return
                    root.cursorActive = true
                    root.focusSection = "header"
                    root.selectedIndex = 1
                  }
                  onClicked: root.toggleSettings()
                }
              }
            }
          }

          // ---------- live timer ----------

          Item {
            width: parent.width
            implicitHeight: timerRow.implicitHeight
            visible: root.page === "home" && (root.active || root.finalizing)

            Row {
              id: timerRow
              width: parent.width
              spacing: Style.space(12)

              Text {
                textFormat: Text.PlainText
                text: root.active ? Model.clock(root.elapsed) : "saving"
                color: root.paused ? root.dim : root.foreground
                font.family: root.fontFamily
                font.pixelSize: Math.round(Style.font.display * 1.55)
                font.bold: true
                anchors.verticalCenter: parent.verticalCenter
              }

              Column {
                anchors.verticalCenter: parent.verticalCenter
                spacing: Style.space(2)

                Text {
                  textFormat: Text.PlainText
                  text: root.paused ? "PAUSED" : (root.finalizing ? "SAVING TAKE" : "RECORDING")
                  color: root.paused ? root.dim : root.urgent
                  font.family: root.fontFamily
                  font.pixelSize: Style.font.caption
                  font.bold: true
                  font.letterSpacing: 1.4
                }

                Text {
                  textFormat: Text.PlainText
                  text: root.targetLabel(root.state.target)
                  color: root.dim
                  font.family: root.fontFamily
                  font.pixelSize: Style.font.caption
                  elide: Text.ElideRight
                  width: Math.min(implicitWidth, Style.space(250))
                }
              }
            }
          }

          // ---------- notice ----------

          Text {
            textFormat: Text.PlainText
            visible: root.page === "home" && root.notice !== ""
            text: root.notice
            color: root.noticeUrgent ? root.urgent : root.dim
            font.family: root.fontFamily
            font.pixelSize: Style.font.caption
            wrapMode: Text.WordWrap
            width: parent.width
          }

          // ---------- primary actions ----------

          Row {
            width: parent.width
            visible: root.page === "home"
            spacing: Style.space(8)

            ActionButton {
              width: (parent.width - parent.spacing * 2) / 3
              rowIndex: 0
              label: root.primaryLabel()
              glyph: root.primaryGlyph()
              tone: root.active || root.starting ? "urgent" : "accent"
              primary: true
              enabled: !root.finalizing
            }

            ActionButton {
              width: (parent.width - parent.spacing * 2) / 3
              rowIndex: 1
              label: root.paused ? "Resume" : "Pause"
              glyph: root.paused ? "󰐊" : "󰏤"
              tone: "neutral"
              enabled: root.active
            }

            ActionButton {
              width: (parent.width - parent.spacing * 2) / 3
              rowIndex: 2
              label: root.discardArmed ? "Discard?" : "Discard"
              glyph: "󰆴"
              tone: "neutral"
              enabled: root.active
            }
          }

          PanelSeparator {
            visible: root.page === "home"
            foreground: root.foreground
          }

          // ---------- preview ----------

          Column {
            width: parent.width
            visible: root.page === "home"
            spacing: Style.space(8)

            PanelSectionHeader {
              text: "Next take"
              foreground: root.foreground
              fontFamily: root.fontFamily
            }

            Loader {
              id: previewLoader
              width: parent.width
              // Build the capture view only while the panel is open. A fresh
              // view starts its live stream reliably; keeping it loaded would
              // keep the compositor capturing behind a closed panel.
              active: root.opened
              sourceComponent: liveScreen
            }
          }

          Component {
            id: liveScreen

            Item {
              id: previewRoot
              width: previewLoader.width
              height: implicitHeight
              implicitHeight: previewColumn.implicitHeight

              // The target's aspect ratio decides the frame; the screen image
              // behind it is scaled uniformly and clipped to the same rectangle.
              readonly property var plan: root.previewPlan()
              readonly property var layout: Model.previewLayout(
                plan, width, Math.round(width * 9 / 16))
              readonly property var dimensions: Model.captureSize(plan, screenView.sourceSize)

              Column {
                id: previewColumn
                width: parent.width
                spacing: Style.space(6)

                Item {
                  id: previewFrameHost
                  width: parent.width
                  height: previewCard.height

                  Rectangle {
                    id: previewCard
                    width: (previewRoot.layout ? previewRoot.layout.frameWidth : 0) + 2
                    height: (previewRoot.layout ? previewRoot.layout.frameHeight : 0) + 2
                    anchors.horizontalCenter: parent.horizontalCenter
                    radius: Style.cornerRadius
                    color: Util.alpha(root.foreground, 0.07)
                    border.width: 1
                    border.color: Util.alpha(root.foreground, 0.10)
                  }

                  Item {
                    id: previewStage
                    anchors.centerIn: previewCard
                    width: previewRoot.layout ? previewRoot.layout.frameWidth : 0
                    height: previewRoot.layout ? previewRoot.layout.frameHeight : 0
                    clip: true

                    readonly property string placeholder: {
                      var plan = previewRoot.plan
                      if (!plan) return "Preview unavailable."
                      if (plan.unavailable) return String(plan.reason || "Preview unavailable.")
                      if (screenView.sourceStopped) return "Preview stopped. Close and reopen the panel to retry."
                      if (screenView.sourceTimedOut) {
                        var source = plan.screenRect ? plan.screenRect.name : "the source"
                        return "Preview unavailable. No frames arrived from " + source + "."
                      }
                      if (!screenView.hasContent) return "Starting preview…"
                      return ""
                    }

                    ScreencopyView {
                      id: screenView
                      x: previewRoot.layout ? previewRoot.layout.sourceX : 0
                      y: previewRoot.layout ? previewRoot.layout.sourceY : 0
                      width: previewRoot.layout ? previewRoot.layout.sourceWidth : 0
                      height: previewRoot.layout ? previewRoot.layout.sourceHeight : 0
                      visible: hasContent && !sourceStopped
                      captureSource: previewRoot.plan && previewRoot.plan.source
                        ? previewRoot.plan.source : null
                      live: true
                      paintCursor: false

                      property bool sourceStopped: false
                      property bool sourceTimedOut: false
                      onStopped: sourceStopped = true
                      onCaptureSourceChanged: {
                        sourceStopped = false
                        sourceTimedOut = false
                      }
                      onHasContentChanged: {
                        if (hasContent) sourceTimedOut = false
                      }
                    }

                    Timer {
                      id: frameTimeout
                      interval: 4000
                      repeat: false
                      running: !!previewRoot.plan && !!previewRoot.plan.source
                        && !screenView.hasContent && !screenView.sourceStopped
                        && !screenView.sourceTimedOut
                      onTriggered: screenView.sourceTimedOut = true
                    }

                    Rectangle {
                      anchors.fill: parent
                      visible: previewStage.placeholder !== ""
                      color: Util.alpha(root.foreground, 0.04)
                    }

                    Text {
                      anchors.centerIn: parent
                      visible: previewStage.placeholder !== ""
                      width: Math.max(0, parent.width - Style.space(20))
                      textFormat: Text.PlainText
                      text: previewStage.placeholder
                      color: root.dim
                      font.family: root.fontFamily
                      font.pixelSize: Style.font.caption
                      horizontalAlignment: Text.AlignHCenter
                      wrapMode: Text.WordWrap
                    }
                  }

                  MouseArea {
                    anchors.fill: previewCard
                    hoverEnabled: true
                    cursorShape: Qt.PointingHandCursor
                    onClicked: root.start()
                  }
                }

                Column {
                  width: parent.width
                  spacing: Style.space(2)

                  Text {
                    width: parent.width
                    textFormat: Text.PlainText
                    text: root.previewTitle(previewRoot.plan)
                    color: root.foreground
                    font.family: root.fontFamily
                    font.pixelSize: Style.font.caption
                    font.bold: true
                    elide: Text.ElideMiddle
                  }

                  Text {
                    width: parent.width
                    textFormat: Text.PlainText
                    visible: text !== ""
                    text: {
                      var plan = previewRoot.plan
                      if (!plan) return ""
                      if (plan.unavailable) return String(plan.reason || "Preview unavailable.")
                      var parts = []
                      if (screenView.sourceStopped) {
                        return "Preview stopped · close and reopen the panel to retry"
                      }
                      if (screenView.sourceTimedOut) {
                        return "Preview unavailable · no frames from "
                          + (plan.screenRect ? plan.screenRect.name : "the source")
                      }
                      if (plan.mode === "idle") {
                        if (plan.screenRect) parts.push(plan.screenRect.name)
                        parts.push("choose a target when recording")
                        return parts.join(" · ")
                      }
                      if (!screenView.hasContent) parts.push("Starting preview…")
                      var title = root.previewTitle(plan)
                      if (screenView.hasContent && previewRoot.dimensions
                          && !/[0-9]+\s*×\s*[0-9]+/.test(title)) {
                        parts.push(previewRoot.dimensions.width + " × "
                          + previewRoot.dimensions.height)
                      }
                      return parts.join(" · ")
                    }
                    color: root.dim
                    font.family: root.fontFamily
                    font.pixelSize: Style.font.caption
                    elide: Text.ElideRight
                  }
                }
              }
            }
          }

          // ---------- quick settings ----------

          Column {
            width: parent.width
            visible: root.page === "home"
            spacing: Style.space(4)

            PanelSectionHeader {
              text: "Quick settings"
              foreground: root.foreground
              fontFamily: root.fontFamily
            }

            Repeater {
              model: root.quickSettingRows

              SettingRow {
                required property var modelData
                required property int index
                width: parent.width
                settingKey: modelData.key
                label: modelData.label
                glyph: modelData.glyph
                rowIndex: index
                quick: true
              }
            }
          }

          // ---------- shortcuts ----------

          PanelSeparator {
            width: parent.width
            foreground: root.foreground
            strength: 0.1
            visible: root.page === "home"
          }

          Item {
            width: parent.width
            height: Style.space(30)
            visible: root.page === "home"

            RowLayout {
              anchors.fill: parent
              spacing: Style.space(8)

              Rectangle {
                Layout.preferredWidth: Style.space(20)
                Layout.preferredHeight: Style.space(20)
                Layout.alignment: Qt.AlignVCenter
                radius: Style.cornerRadius
                color: Util.alpha(root.dim, 0.2)

                Text {
                  anchors.centerIn: parent
                  textFormat: Text.PlainText
                  text: "?"
                  color: root.dim
                  font.family: root.fontFamily
                  font.pixelSize: Style.font.bodySmall
                  font.bold: true
                }
              }

              Text {
                Layout.alignment: Qt.AlignVCenter
                textFormat: Text.PlainText
                text: "Keyboard shortcuts"
                color: root.foreground
                font.family: root.fontFamily
                font.pixelSize: Style.font.body
              }

              Item { Layout.fillWidth: true }

              Text {
                Layout.alignment: Qt.AlignVCenter
                textFormat: Text.PlainText
                text: "›"
                rotation: root.showShortcuts ? 90 : 0
                color: root.dim
                font.family: root.fontFamily
                font.pixelSize: Style.font.title
                Behavior on rotation {
                  NumberAnimation { duration: 120; easing.type: Easing.OutCubic }
                }
              }
            }

            MouseArea {
              anchors.fill: parent
              hoverEnabled: true
              cursorShape: Qt.PointingHandCursor
              onClicked: root.showShortcuts = !root.showShortcuts
            }
          }

          Column {
            width: parent.width
            spacing: Style.space(4)
            visible: root.page === "home" && root.showShortcuts

            Repeater {
              model: [
                { keys: "↑ ↓ / j k", what: "move between rows" },
                { keys: "← →", what: "move across the action row" },
                { keys: "h / l", what: "move horizontally or cycle the focused option" },
                { keys: "enter / space", what: "start, stop, save, or open" },
                { keys: "tab", what: "move between sections" },
                { keys: "r", what: "start or stop a take" },
                { keys: "p", what: "pause or resume" },
                { keys: "d", what: "discard the current take" },
                { keys: "Shift+H", what: "open or close history" },
                { keys: "s", what: "settings" },
                { keys: "a", what: "open the take actions menu" },
                { keys: "x / delete", what: "trash the selected take" },
                { keys: "m", what: "make a slow-motion copy" },
                { keys: "u", what: "share the selected take" },
                { keys: "c", what: "copy the selected take's path" },
                { keys: "o", what: "open the recordings folder" },
                { keys: "q", what: "close" },
                { keys: "esc", what: "back, then close" }
              ]

              RowLayout {
                required property var modelData
                width: parent.width
                height: Style.space(20)
                spacing: Style.space(8)

                Rectangle {
                  Layout.preferredWidth: Math.max(Style.space(64), keyText.implicitWidth + Style.space(8))
                  Layout.preferredHeight: Style.space(20)
                  Layout.alignment: Qt.AlignVCenter
                  radius: Style.cornerRadius
                  color: Util.alpha(root.dim, 0.14)

                  Text {
                    id: keyText
                    anchors.centerIn: parent
                    textFormat: Text.PlainText
                    text: modelData.keys
                    color: root.dim
                    font.family: root.fontFamily
                    font.pixelSize: Style.font.caption
                  }
                }

                Text {
                  Layout.fillWidth: true
                  Layout.minimumWidth: 0
                  Layout.alignment: Qt.AlignVCenter
                  textFormat: Text.PlainText
                  text: modelData.what
                  color: root.dim
                  font.family: root.fontFamily
                  font.pixelSize: Style.font.caption
                  elide: Text.ElideRight
                }
              }
            }
          }

          // ---------- history page ----------

          Column {
            width: parent.width
            visible: root.page === "history"
            spacing: Style.space(8)

            Item {
              width: parent.width
              implicitHeight: Math.max(takesHeader.implicitHeight, takesHint.implicitHeight)

              PanelSectionHeader {
                id: takesHeader
                text: "Recent takes"
                foreground: root.foreground
                fontFamily: root.fontFamily
                anchors.left: parent.left
                anchors.verticalCenter: parent.verticalCenter
              }

              Text {
                id: takesHint
                textFormat: Text.PlainText
                text: root.visibleRecents.length > 0
                  ? "enter opens · a actions · x trashes"
                  : "takes appear here after the first recording"
                color: root.dim
                font.family: root.fontFamily
                font.pixelSize: Style.font.caption
                anchors.right: parent.right
                anchors.verticalCenter: parent.verticalCenter
              }
            }

            OpenFolderRow {
              width: parent.width
              rowIndex: 0
            }

            Repeater {
              id: takesRepeater
              model: root.visibleRecents

              TakeRow {
                required property var modelData
                required property int index
                width: parent.width
                row: modelData
                rowIndex: index + 1
              }
            }

            Text {
              textFormat: Text.PlainText
              visible: root.notice !== ""
              text: root.notice
              color: root.noticeUrgent ? root.urgent : root.dim
              font.family: root.fontFamily
              font.pixelSize: Style.font.caption
              wrapMode: Text.WordWrap
              width: parent.width
            }
          }

          // ---------- settings page ----------

          Column {
            visible: root.page === "settings"
            width: parent.width
            spacing: Style.space(10)

            Repeater {
              model: root.advancedSettingRows

              SettingRow {
                required property var modelData
                required property int index
                width: parent.width
                settingKey: modelData.key
                label: modelData.label
                glyph: modelData.glyph
                rowIndex: index
              }
            }

            Column {
              width: parent.width
              spacing: Style.space(4)

              PanelSectionHeader {
                text: "Filename prefix"
                foreground: root.foreground
                fontFamily: root.fontFamily
              }

              Rectangle {
                width: parent.width
                implicitHeight: Style.space(34)
                radius: Style.cornerRadius
                color: Util.alpha(root.foreground, 0.06)
                border.width: prefixInput.activeFocus ? 1 : 0
                border.color: root.accent

                TextInput {
                  id: prefixInput
                  anchors.fill: parent
                  anchors.leftMargin: Style.space(9)
                  anchors.rightMargin: Style.space(9)
                  verticalAlignment: TextInput.AlignVCenter
                  text: root.currentValue("filenamePrefix")
                  color: root.foreground
                  selectionColor: root.accent
                  selectedTextColor: root.inkOn(root.accent)
                  font.family: root.fontFamily
                  font.pixelSize: Style.font.body
                  selectByMouse: true
                  clip: true
                  onActiveFocusChanged: root.typingPrefix = activeFocus
                  onEditingFinished: root.writeSetting("filenamePrefix", text)
                }
              }

              Text {
                textFormat: Text.PlainText
                text: root.currentValue("filenamePrefix") === ""
                  ? "Empty uses reel-<date>. The date is added to every take."
                  : "Takes are named " + root.currentValue("filenamePrefix") + "<date>."
                color: root.dim
                font.family: root.fontFamily
                font.pixelSize: Style.font.caption
                wrapMode: Text.WordWrap
                width: parent.width
              }
            }

            Text {
              textFormat: Text.PlainText
              text: "Recordings: " + Model.shortPath(root.outputDir, root.home)
              color: root.dim
              font.family: root.fontFamily
              font.pixelSize: Style.font.caption
              elide: Text.ElideMiddle
              width: parent.width
            }

            Text {
              textFormat: Text.PlainText
              text: "Folder, webcam device and panel placement live in the Omarchy bar settings."
              color: root.dim
              font.family: root.fontFamily
              font.pixelSize: Style.font.caption
              wrapMode: Text.WordWrap
              width: parent.width
            }
          }
        }
      }

      // ---------- take actions menu ----------

      Item {
        id: takeMenuOverlay
        anchors.fill: parent
        visible: root.takeMenuOpen
        z: 1000

        MouseArea {
          anchors.fill: parent
          acceptedButtons: Qt.AllButtons
          cursorShape: Qt.ArrowCursor
          onClicked: root.closeTakeMenu()
        }

        BorderSurface {
          id: takeMenuCard
          x: root.takeMenuX
          y: root.takeMenuY
          width: root.takeMenuWidth
          height: root.takeMenuHeight
          color: Commons.Color.background
          borderSpec: root.takeMenuBorderSpec
          radius: Style.cornerRadius
          padding: root.takeMenuPadding

          // Swallow clicks on the card's padding so only the overlay behind it
          // dismisses the menu.
          MouseArea {
            anchors.fill: parent
            acceptedButtons: Qt.AllButtons
          }

          Column {
            anchors.fill: parent
            anchors.leftMargin: takeMenuCard.contentLeftInset
            anchors.rightMargin: takeMenuCard.contentRightInset
            anchors.topMargin: takeMenuCard.contentTopInset
            anchors.bottomMargin: takeMenuCard.contentBottomInset

            Repeater {
              model: root.takeMenuItems

              TakeMenuRow {
                required property var modelData
                required property int index
                width: parent.width
                itemIndex: index
                glyph: String(modelData.glyph || "")
                label: String(modelData.label || "")
                urgent: !!modelData.urgent
                selected: root.takeMenuIndex === index
              }
            }
          }
        }
      }
    }
  }

  // ---------- helpers -----------------------------------------------------

  // Reel owns the key map because h/l are vim-style movement while Shift+H
  // opens history. PanelKeyCatcher reserves h for vim-left, which swallowed it.
  component ReelKeyCatcher: Item {
    id: catcher

    property bool blocked: false

    signal moveRequested(int dx, int dy)
    signal activateRequested()
    signal closeRequested()
    signal deleteRequested()
    signal tabRequested(int direction)
    signal textKey(string text, int modifiers)
    signal historyRequested()
    focus: true
    Keys.priority: Keys.BeforeItem

    function openTakeMenuForSelected() {
      var row = root.selectedTake()
      if (!row) return
      var item = takesRepeater.itemAt(root.selectedIndex - 1)
      if (item && typeof item.openMenu === "function") item.openMenu()
    }

    Keys.onPressed: function(event) {
      if (catcher.blocked) return

      if (event.key === Qt.Key_Escape) {
        catcher.closeRequested()
        event.accepted = true
        return
      }
      if (event.key === Qt.Key_Tab || event.key === Qt.Key_Backtab) {
        catcher.tabRequested(
          (event.modifiers & Qt.ShiftModifier) || event.key === Qt.Key_Backtab ? -1 : 1)
        event.accepted = true
        return
      }
      var keyText = String(event.text || "").toLowerCase()
      var shifted = (event.modifiers & Qt.ShiftModifier) !== 0
      // Shift+H is the only history shortcut. Plain h/l are horizontal
      // movement; the focused section decides whether that moves the cursor
      // or cycles an option.
      if (shifted && (event.key === Qt.Key_H || keyText === "h")) {
        catcher.historyRequested()
        event.accepted = true
        return
      }
      if (event.text === "h") {
        catcher.moveRequested(-1, 0)
        event.accepted = true
        return
      }
      if (event.text === "l") {
        catcher.moveRequested(1, 0)
        event.accepted = true
        return
      }
      if (event.key === Qt.Key_Down || event.text === "j") {
        catcher.moveRequested(0, 1)
        event.accepted = true
        return
      }
      if (event.key === Qt.Key_Up || event.text === "k") {
        catcher.moveRequested(0, -1)
        event.accepted = true
        return
      }
      if (event.key === Qt.Key_Right) {
        catcher.moveRequested(1, 0)
        event.accepted = true
        return
      }
      if (event.key === Qt.Key_Left) {
        catcher.moveRequested(-1, 0)
        event.accepted = true
        return
      }
      if (event.key === Qt.Key_Return || event.key === Qt.Key_Enter) {
        catcher.activateRequested()
        event.accepted = true
        return
      }
      if (event.key === Qt.Key_Space) {
        catcher.activateRequested()
        event.accepted = true
        return
      }
      if (event.key === Qt.Key_Delete || event.text === "x" || event.text === "X") {
        catcher.deleteRequested()
        event.accepted = true
        return
      }
      // Share is handled here as well as in the root key map so it works while
      // the actions menu is open and survives any other text-key handling.
      if (event.text === "u" || event.text === "U") {
        if (root.takeMenuOpen) root.closeTakeMenu()
        root.shareSelected()
        event.accepted = true
        return
      }
      if (event.text && event.text.length === 1) {
        catcher.textKey(event.text, event.modifiers)
      }
    }
  }

  // A horizontal pair of chevrons keeps the cycle rows readable at a glance.
  component CycleChevrons: Row {
    property color foreground: root.foreground
    spacing: Style.space(4)

    Text {
      textFormat: Text.PlainText
      text: "‹"
      color: parent.foreground
      font.family: root.fontFamily
      font.pixelSize: Style.font.body
      opacity: 0.6
    }
    Text {
      textFormat: Text.PlainText
      text: "›"
      color: parent.foreground
      font.family: root.fontFamily
      font.pixelSize: Style.font.body
      opacity: 0.6
    }
  }

  component ActionButton: CursorSurface {
    id: action
    property int rowIndex: 0
    property string label: ""
    property string glyph: ""
    property string tone: "neutral"
    property bool primary: false

    readonly property color toneColor: tone === "urgent" ? root.urgent : tone === "accent" ? root.accent : root.foreground
    readonly property color toneInk: tone === "neutral" ? root.foreground : root.inkOn(action.toneColor)

    hasCursor: root.cursorActive && root.page === "home"
      && root.focusSection === "primary" && root.selectedIndex === rowIndex
    onHasCursorChanged: if (hasCursor) root.ensureCursorVisible(action)
    foreground: root.foreground
    accent: root.accent
    fill: root.bar ? Style.hoverFillFor(root.foreground, root.accent) : root.accentSoft
    currentFill: root.accentSoft
    color: !enabled ? "transparent"
      : primary ? action.toneColor
      : hasCursor ? action.fill
      : "transparent"
    borderSpec: !enabled
      ? Border.controlSpec("normal", root.dim, root.accent)
      : primary ? Border.none()
      : Border.controlSpec(hasCursor ? "hover-cursor" : "normal", root.foreground, root.accent)
    implicitHeight: Style.space(40)
    opacity: enabled ? 1.0 : 0.45

    Row {
      anchors.centerIn: parent
      spacing: Style.space(7)

      Text {
        textFormat: Text.PlainText
        text: action.glyph
        color: action.primary ? action.toneInk : action.toneColor
        font.family: root.fontFamily
        font.pixelSize: Style.font.body
        anchors.verticalCenter: parent.verticalCenter
      }

      Text {
        textFormat: Text.PlainText
        text: action.label
        color: action.primary ? action.toneInk : root.foreground
        font.family: root.fontFamily
        font.pixelSize: Style.font.body
        font.bold: true
        anchors.verticalCenter: parent.verticalCenter
      }
    }

    MouseArea {
      anchors.fill: parent
      hoverEnabled: true
      enabled: action.enabled
      cursorShape: Qt.PointingHandCursor
      onContainsMouseChanged: if (containsMouse) {
        root.cursorActive = true
        root.focusSection = "primary"
        root.selectedIndex = action.rowIndex
      }
      onClicked: root.primaryAction(action.rowIndex)
    }
  }

  component TakeMenuRow: Rectangle {
    id: menuItem
    property int itemIndex: 0
    property string glyph: ""
    property string label: ""
    property bool selected: false
    property bool urgent: false

    height: root.takeMenuRowHeight
    radius: Style.cornerRadius
    color: menuItem.selected
      ? (menuItem.urgent ? root.urgentSoft : root.accentSoft)
      : "transparent"

    Row {
      anchors.fill: parent
      anchors.leftMargin: Style.space(8)
      anchors.rightMargin: Style.space(8)
      spacing: Style.space(8)

      Text {
        textFormat: Text.PlainText
        text: menuItem.glyph
        color: menuItem.urgent ? root.urgent : root.dim
        font.family: root.fontFamily
        font.pixelSize: Style.font.body
        anchors.verticalCenter: parent.verticalCenter
        width: Style.space(20)
        horizontalAlignment: Text.AlignHCenter
      }

      Text {
        textFormat: Text.PlainText
        text: menuItem.label
        color: menuItem.urgent ? root.urgent : root.foreground
        font.family: root.fontFamily
        font.pixelSize: Style.font.body
        anchors.verticalCenter: parent.verticalCenter
        elide: Text.ElideRight
        width: parent.width - Style.space(28)
      }
    }

    MouseArea {
      anchors.fill: parent
      hoverEnabled: true
      cursorShape: Qt.PointingHandCursor
      onContainsMouseChanged: if (containsMouse) root.takeMenuIndex = menuItem.itemIndex
      onClicked: {
        root.takeMenuIndex = menuItem.itemIndex
        root.activateTakeMenu()
      }
    }
  }

  component TakeRow: CursorSurface {
    id: take
    property var row: null
    property int rowIndex: 0

    readonly property bool armed: root.trashArmedPath !== "" && root.trashArmedPath === String(row.path || "")
    readonly property string preview: String(row.preview || "")
    readonly property bool menuOpen: root.takeMenuOpen && root.takeMenuRow !== null
      && String(root.takeMenuRow.path || "") === String(row.path || "")

    function openMenu() {
      root.openTakeMenu(take.row, take.rowIndex, menuButton)
    }

    hasCursor: root.cursorActive && root.page === "history"
      && root.focusSection === "recents" && root.selectedIndex === rowIndex
    onHasCursorChanged: if (hasCursor) root.ensureCursorVisible(take)
    foreground: root.foreground
    accent: root.accent
    fill: root.bar ? Style.hoverFillFor(root.foreground, root.accent) : root.accentSoft
    implicitHeight: Style.space(56)
    radius: Style.cornerRadius

    Row {
      anchors.fill: parent
      anchors.leftMargin: Style.space(6)
      anchors.rightMargin: Style.space(6)
      spacing: Style.space(9)

      Item {
        width: Style.space(72)
        height: Style.space(42)
        anchors.verticalCenter: parent.verticalCenter
        clip: true

        // Plain Image keeps the panel working on Omarchy 4.0.x, where
        // qs.Ui.SizedImage does not exist yet.
        Image {
          anchors.fill: parent
          visible: take.preview !== ""
          source: take.preview !== "" ? Util.fileUrl(take.preview) : ""
          fillMode: Image.PreserveAspectCrop
          asynchronous: true
          sourceSize.width: Math.ceil(width * Screen.devicePixelRatio)
          sourceSize.height: Math.ceil(height * Screen.devicePixelRatio)
        }

        Rectangle {
          anchors.fill: parent
          visible: take.preview === ""
          color: Util.alpha(root.foreground, 0.08)
          border.width: 0

          Text {
            anchors.centerIn: parent
            textFormat: Text.PlainText
            text: "󰕧"
            color: root.dim
            font.family: root.fontFamily
            font.pixelSize: Style.font.title
          }
        }
      }

      Column {
        width: parent.width - Style.space(72) - actions.width - parent.spacing * 2
        anchors.verticalCenter: parent.verticalCenter
        spacing: Style.space(2)

        Text {
          textFormat: Text.PlainText
          text: Model.takeTitle(take.row)
          color: root.foreground
          font.family: root.fontFamily
          font.pixelSize: Style.font.body
          font.bold: take.hasCursor
          elide: Text.ElideMiddle
          width: parent.width
        }

        Text {
          textFormat: Text.PlainText
          text: Model.takeMeta(take.row)
          color: take.armed ? root.urgent : root.dim
          font.family: root.fontFamily
          font.pixelSize: Style.font.caption
          elide: Text.ElideRight
          width: parent.width
        }
      }

      Row {
        id: actions
        spacing: Style.space(2)
        anchors.verticalCenter: parent.verticalCenter

        PanelActionButton {
          id: menuButton
          iconText: "󰇘"
          tooltipText: "Take actions"
          foreground: take.menuOpen ? root.accent : root.dim
          hoverColor: root.accent
          hasCursor: take.menuOpen
          fontFamily: root.fontFamily
          onClicked: take.menuOpen ? root.closeTakeMenu() : take.openMenu()
        }
      }
    }

    MouseArea {
      anchors.fill: parent
      z: -1
      hoverEnabled: true
      acceptedButtons: Qt.LeftButton | Qt.RightButton
      cursorShape: Qt.PointingHandCursor
      onContainsMouseChanged: if (containsMouse) {
        root.cursorActive = true
        root.focusSection = "recents"
        root.selectedIndex = take.rowIndex
      }
      onClicked: function(mouse) {
        if (mouse.button === Qt.RightButton) take.openMenu()
        else root.openVideo(take.row)
      }
    }
  }

  component OpenFolderRow: CursorSurface {
    id: folder
    property int rowIndex: 0

    hasCursor: root.cursorActive && root.page === "history"
      && root.focusSection === "recents" && root.selectedIndex === rowIndex
    onHasCursorChanged: if (hasCursor) root.ensureCursorVisible(folder)
    foreground: root.foreground
    accent: root.accent
    fill: root.bar ? Style.hoverFillFor(root.foreground, root.accent) : root.accentSoft
    implicitHeight: Style.space(34)
    radius: Style.cornerRadius

    Row {
      anchors.fill: parent
      anchors.leftMargin: Style.space(6)
      anchors.rightMargin: Style.space(8)
      spacing: Style.space(8)

      Text {
        textFormat: Text.PlainText
        text: "󰉋"
        color: root.dim
        font.family: root.fontFamily
        font.pixelSize: Style.font.body
        anchors.verticalCenter: parent.verticalCenter
        width: Style.space(20)
        horizontalAlignment: Text.AlignHCenter
      }

      Text {
        textFormat: Text.PlainText
        text: root.visibleRecents.length > 0 ? "Open the recordings folder" : "No takes yet · open the recordings folder"
        color: root.foreground
        font.family: root.fontFamily
        font.pixelSize: Style.font.body
        anchors.verticalCenter: parent.verticalCenter
        elide: Text.ElideRight
        width: parent.width - Style.space(24)
      }
    }

    MouseArea {
      anchors.fill: parent
      hoverEnabled: true
      cursorShape: Qt.PointingHandCursor
      onContainsMouseChanged: if (containsMouse) {
        root.cursorActive = true
        root.focusSection = "recents"
        root.selectedIndex = folder.rowIndex
      }
      onClicked: root.openFolder()
    }
  }

  component SettingRow: CursorSurface {
    id: setting
    property string settingKey: ""
    property string label: ""
    property string glyph: ""
    property int rowIndex: 0
    property bool quick: false

    readonly property bool selected: root.cursorActive && (quick
      ? root.page === "home" && root.focusSection === "quick" && root.selectedIndex === rowIndex
      : root.page === "settings" && root.settingsIndex === rowIndex)

    hasCursor: selected
    onHasCursorChanged: if (hasCursor) root.ensureCursorVisible(setting)
    foreground: root.foreground
    accent: root.accent
    fill: root.bar ? Style.hoverFillFor(root.foreground, root.accent) : root.accentSoft
    implicitHeight: Style.space(34)
    radius: Style.cornerRadius

    Row {
      anchors.fill: parent
      anchors.leftMargin: Style.space(6)
      anchors.rightMargin: Style.space(8)
      spacing: Style.space(8)

      Text {
        textFormat: Text.PlainText
        text: setting.glyph
        color: root.dim
        font.family: root.fontFamily
        font.pixelSize: Style.font.body
        anchors.verticalCenter: parent.verticalCenter
        width: Style.space(20)
        horizontalAlignment: Text.AlignHCenter
      }

      Text {
        textFormat: Text.PlainText
        text: setting.label
        color: root.foreground
        font.family: root.fontFamily
        font.pixelSize: Style.font.body
        anchors.verticalCenter: parent.verticalCenter
        width: parent.width - Style.space(20) - valueText.width - Style.space(28) - parent.spacing * 2
        elide: Text.ElideRight
      }

      Text {
        id: valueText
        textFormat: Text.PlainText
        text: root.valueLabel(setting.settingKey, root.currentValue(setting.settingKey))
        color: setting.selected ? root.foreground : root.dim
        font.family: root.fontFamily
        font.pixelSize: Style.font.caption
        font.bold: true
        anchors.verticalCenter: parent.verticalCenter
      }

      CycleChevrons {
        foreground: setting.selected ? root.foreground : root.dim
        anchors.verticalCenter: parent.verticalCenter
      }
    }

    MouseArea {
      anchors.fill: parent
      hoverEnabled: true
      cursorShape: Qt.PointingHandCursor
      acceptedButtons: Qt.LeftButton | Qt.RightButton
      onContainsMouseChanged: if (containsMouse) {
        root.cursorActive = true
        if (setting.quick) {
          root.focusSection = "quick"
          root.selectedIndex = setting.rowIndex
        } else {
          root.settingsIndex = setting.rowIndex
        }
      }
      onClicked: function(mouse) {
        root.cursorActive = true
        if (setting.quick) {
          root.focusSection = "quick"
          root.selectedIndex = setting.rowIndex
        } else {
          root.settingsIndex = setting.rowIndex
        }
        root.cycleOption(setting.settingKey, mouse.button === Qt.RightButton ? -1 : 1)
      }
    }
  }

  // ---------- processes ---------------------------------------------------

  Timer {
    id: openAfterAction
    interval: 350
    repeat: false
    onTriggered: {
      if (root.service) root.service.refresh()
      root.open()
      if (root.pendingNotice !== "") {
        root.setNotice(root.pendingNotice, true)
        root.pendingNotice = ""
      }
    }
  }

  Process {
    id: omacutProbe
    running: false
    command: ["sh", "-c", "command -v omacut >/dev/null 2>&1"]
    onExited: function(code) { root.hasOmacut = code === 0 }
  }

  Process {
    id: controlProc
    running: false
    stdout: StdioCollector { waitForEnd: true }
    stderr: StdioCollector { id: controlErr; waitForEnd: true }
    onExited: function(code) {
      var message = String(controlErr.text || "").trim()
      var action = root.controlAction
      root.controlAction = ""
      if (code !== 0 && !root.cancelling && message !== "") {
        var failure = message.replace(/^reel:\s*/, "") || "That action failed."
        if (action === "start") root.pendingNotice = failure
        else root.setNotice(failure, true)
      }
      root.cancelling = false
      if (root.service) root.service.refresh()
      if (action === "start" && code !== 0) openAfterAction.restart()
    }
  }

  Process {
    id: stopProc
    running: false
    stdout: StdioCollector { waitForEnd: true }
    stderr: StdioCollector { id: stopErr; waitForEnd: true }
    onExited: function(code) {
      var message = String(stopErr.text || "").trim()
      if (code !== 0 && !root.cancelling) {
        root.setNotice(message.replace(/^reel:\s*/, "") || "The recording could not be stopped.", true)
      }
      root.cancelling = false
      if (root.service) root.service.refresh()
    }
  }

  Process {
    id: slowmoProc
    running: false
    stdout: StdioCollector { waitForEnd: true }
    stderr: StdioCollector { id: slowmoErr; waitForEnd: true }
    onExited: function(code) {
      var message = String(slowmoErr.text || "").trim()
      if (code === 0) root.setNotice("Slow-motion copy saved.")
      else root.setNotice(message.replace(/^reel:\s*/, "") || "The slow-motion copy failed.", true)
      if (root.service) root.service.refresh()
    }
  }

  Process {
    id: shareProc
    running: false
    stdout: StdioCollector { waitForEnd: true }
    stderr: StdioCollector { id: shareErr; waitForEnd: true }
    onExited: function(code) {
      var message = String(shareErr.text || "").trim()
      if (code === 0) root.setNotice("Sharing with LocalSend.")
      else root.setNotice(message.replace(/^omarchy:\s*/, "")
        || "Sharing failed. Is LocalSend installed?", true)
    }
  }

  Process {
    id: trashProc
    running: false
    stdout: StdioCollector { waitForEnd: true }
    stderr: StdioCollector { id: trashErr; waitForEnd: true }
    onExited: function(code) {
      var message = String(trashErr.text || "").trim()
      if (code !== 0) root.setNotice(message.replace(/^reel:\s*/, "") || "Could not move that take to the trash.", true)
      else root.setNotice("Take moved to the trash.")
      if (root.service) root.service.refresh()
    }
  }

  Process {
    id: configProc
    running: false
    stderr: StdioCollector { id: configErr; waitForEnd: true }
    onExited: function(code) {
      if (code !== 0) {
        var message = String(configErr.text || "").trim()
        root.setNotice(message.replace(/^reel:\s*/, "") || "Settings saved in the bar, but reel.json did not update.", true)
      }
      root.flushConfig()
    }
  }

  // ---------- IPC ---------------------------------------------------------

  IpcHandler {
    target: root.ipcTarget

    function open(): void { root.open() }
    function close(): void { root.close() }
    function show(): void { root.open() }
    function hide(): void { root.close() }
    function toggle(): void { root.toggle() }
    function toggleRecording(): void { root.toggleRecording() }
    function capture(): void { root.startImmediate() }
    function stop(): void { root.stop() }
    function stopDiscard(): void { root.discardNow() }
    function pause(): void { if (root.status === "recording") root.togglePause() }
    function resume(): void { if (root.paused) root.togglePause() }
    function togglePause(): void { root.togglePause() }
    function settings(): void {
      root.open()
      root.openSettings()
    }
    function refresh(): void { if (root.service) root.service.refresh() }
  }

  Component.onCompleted: {
    if (root.service) root.service.refresh()
    if (!omacutProbe.running) omacutProbe.running = true
  }
}
