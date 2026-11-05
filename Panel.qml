import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import Quickshell
import Quickshell.Io
import qs.Commons
import qs.Commons as Commons
import qs.Ui
import "Model.js" as Model

// Reel: a bar widget, recording history, and inline settings tabs. The recording
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
  readonly property var visibleRecents: {
    var rows = root.recents || []
    var pending = root.pendingTrash || ({})
    var out = []
    for (var i = 0; i < rows.length; i++) {
      if (!pending[String(rows[i].path || "")]) out.push(rows[i])
    }
    return out
  }
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
  readonly property string outputDir: root.service && root.service.outputDir !== ""
    ? root.service.outputDir
    : root.home + "/Videos/Reel"
  readonly property var updateInfo: root.service && root.service.updateInfo
    ? root.service.updateInfo : Model.emptyUpdate()
  readonly property bool updateAvailable: root.updateInfo.available === true
  readonly property bool updateChecking: root.service
    ? root.service.updateChecking === true : false

  readonly property color foreground: root.bar ? root.bar.foreground : Commons.Color.foreground
  readonly property color accent: Commons.Color.accent
  readonly property color urgent: Commons.Color.urgent
  readonly property color dim: Commons.Color.muted
  readonly property color accentSoft: Util.alpha(root.accent, 0.16)
  readonly property color urgentSoft: Util.alpha(root.urgent, 0.16)
  readonly property color restFill: Util.alpha(root.foreground, 0.04)
  readonly property color hoverFill: Util.alpha(root.foreground, 0.08)
  readonly property color selectedFill: Util.alpha(root.foreground, 0.14)
  readonly property color hairline: Util.alpha(root.foreground, 0.12)
  readonly property color strongBorder: Util.alpha(root.foreground, 0.24)
  readonly property color secondary: Util.alpha(root.foreground, 0.72)
  readonly property string fontFamily: root.bar ? root.bar.fontFamily : Style.font.family
  readonly property color onAccent: root.inkOn(root.accent)
  readonly property color onUrgent: root.inkOn(root.urgent)

  readonly property var defaults: ({
    "desktopAudio": "on",
    "microphoneAudio": "off",
    "noiseCancellation": "off",
    "cursor": "on",
    "webcam": "off",
    "countdown": 0,
    "fps": 60,
    "quality": "very_high",
    "codec": "auto",
    "resolution": "0x0",
    "container": "mp4",
    "notifications": "off",
    "manualZoom": "on",
    "zoomMax": "2.0",
    "panStep": "60",
    "autoZoomStyle": "off",
    "autoZoomDepth": "2.0",
    "outputDir": "",
    "webcamDevice": ""
  })

  readonly property var mediaSettingRows: [
    { key: "countdown", label: "Countdown", glyph: "󰔛", section: "Capture" },
    { key: "webcam", label: "Webcam corner", glyph: "󰄀" },
    { key: "webcamDevice", label: "Webcam device", glyph: "󰄀" },
    { key: "cursor", label: "Record the pointer", glyph: "󰍽" },
    { key: "desktopAudio", label: "Desktop audio", glyph: "󰕾", section: "Audio" },
    { key: "microphoneAudio", label: "Microphone", glyph: "󰍬" },
    { key: "noiseCancellation", label: "Noise reduction", glyph: "󰍬" },
    { key: "fps", label: "Frame rate", glyph: "󰈫", section: "Video" },
    { key: "quality", label: "Quality", glyph: "󰓅" },
    { key: "codec", label: "Video codec", glyph: "󰈙" },
    { key: "resolution", label: "Resolution cap", glyph: "󰍹" },
    { key: "container", label: "Container", glyph: "󰉋" }
  ]
  readonly property var shortcutRows: [
    { keys: "Shift+U", what: "update Reel" },
    { keys: "↑ ↓ / j k", what: "move between rows" },
    { keys: "← →", what: "move across an action row" },
    { keys: "h / l", what: "move horizontally or cycle an option" },
    { keys: "enter / space", what: "start, stop, save, or open" },
    { keys: "tab", what: "switch sections or settings tabs" },
    { keys: "1–4", what: "switch settings tabs" },
    { keys: "r", what: "start or stop a take" },
    { keys: "p", what: "pause or resume" },
    { keys: "d", what: "discard the current take" },
    { keys: "Shift+H", what: "open or close history" },
    { keys: "s", what: "focus the settings tabs" },
    { keys: "a", what: "open the take actions menu" },
    { keys: "x / delete", what: "trash the selected take" },
    { keys: "u", what: "share the selected take" },
    { keys: "c", what: "copy the selected take's path" },
    { keys: "o", what: "open the recordings folder" },
    { keys: "q", what: "close the panel" },
    { keys: "esc", what: "go back, then close" }
  ]
  readonly property var settingsTabs: [
    { id: "media", label: "Media", rows: root.mediaSettingRows },
    { id: "zoom", label: "Zoom", rows: [
      { key: "manualZoom", label: "Manual zoom", glyph: "󰍉" },
      { key: "zoomMax", label: "Zoom depth", glyph: "󰍉" },
      { key: "panStep", label: "Pan step", glyph: "󰆞" },
      { key: "autoZoomStyle", label: "Auto zoom", glyph: "󰍉", section: "Saved take" },
      { key: "autoZoomDepth", label: "Auto zoom depth", glyph: "󰍉" }
    ] },
    { id: "output", label: "Output", rows: [
      { key: "notifications", label: "Saved notification", glyph: "󰂚" }
    ] },
    { id: "keys", label: "Keys", rows: root.shortcutRows }
  ]

  // ---------- panel state -------------------------------------------------

  property string page: "home"
  property bool typingPrefix: false
  property int countdownTick: 0
  property string focusSection: "primary"
  property int selectedIndex: 0
  property string hoverSection: ""
  property int hoverIndex: -1
  property bool cursorActive: false
  property bool keyboardScroll: false
  property bool carryingFile: false
  property int settingsIndex: 0
  property string settingsTab: "media"
  onSettingsTabChanged: Qt.callLater(root.resetScroll)
  property bool hasOmacut: false
  // Cameras for the Webcam device row, from `reel webcams --json`.
  property var cameras: []

  property string notice: ""
  property bool noticeUrgent: false
  property int noticeSeq: 0
  property string discardArmedPath: ""
  property string trashArmedPath: ""
  property var pendingTrash: ({})
  property string removingPath: ""
  property string trashInFlightPath: ""
  property string historyAnchorPath: ""
  property int historyAnchorIndex: 0
  property bool suppressHistoryHover: false
  property bool discardArmed: false
  property bool cancelling: false
  property string pendingConfig: ""
  property string pushedSignature: ""
  property var lastActionArgs: []
  property string controlAction: ""
  property string pendingNotice: ""
  property bool updateNoticePending: false

  // The take actions menu lives at panel level rather than inside each row so
  // the keyboard cursor keeps working while it is open. It anchors to the
  // row's menu button and floats over the history list.
  property bool takeMenuOpen: false
  property var takeMenuRow: null
  property int takeMenuIndex: 0
  property real takeMenuX: 0
  property real takeMenuY: 0
  readonly property real takeMenuWidth: Style.space(220)
  readonly property real takeMenuRowHeight: Style.space(30)
  readonly property real takeMenuPadding: Style.space(5)
  readonly property var takeMenuBorderSpec: Border.flat(root.hairline, 1)
  readonly property var takeMenuItems: [
    { key: "open", label: "Open", glyph: "󰐊", shortcut: "Enter" },
    { key: "cut", label: "Cut in Omacut", glyph: "󰆐", shortcut: "t" },
    { key: "share", label: "Share", glyph: "󰒗", shortcut: "u" },
    {
      key: "trash",
      label: root.trashArmedPath !== "" && root.takeMenuRow
        && root.trashArmedPath === String(root.takeMenuRow.path || "")
        ? "Confirm delete" : "Delete",
      glyph: "󰆴",
      shortcut: "x",
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
    interval: 1900
    repeat: false
    onTriggered: root.notice = ""
  }

  function setNotice(text, urgent) {
    root.notice = String(text || "")
    root.noticeUrgent = !!urgent
    root.noticeSeq += 1
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

  function isSwitch(key) {
    return key === "desktopAudio" || key === "microphoneAudio" || key === "cursor"
      || key === "notifications" || key === "manualZoom" || key === "noiseCancellation"
  }

  function optionList(key) {
    switch (key) {
    case "desktopAudio": return ["on", "off"]
    case "microphoneAudio": return ["on", "off"]
    case "noiseCancellation": return ["off", "on"]
    case "cursor": return ["on", "off"]
    case "webcam": return ["off", "small", "medium", "large"]
    case "webcamDevice": return Model.cameraChoices(root.cameras, root.valueString("webcamDevice", ""))
    case "countdown": return ["0", "3", "5", "10"]
    case "fps": return ["30", "60", "120"]
    case "quality": return ["very_low", "low", "medium", "high", "very_high", "ultra"]
    case "codec": return ["auto", "h264", "hevc", "av1"]
    case "resolution": return ["0x0", "3840x2160", "2560x1440", "1920x1080", "1280x720"]
    case "container": return ["mp4", "mkv"]
    case "notifications": return ["on", "off"]
    case "manualZoom": return ["off", "on"]
    case "zoomMax": return ["1.5", "2.0", "2.5", "3.0"]
    case "panStep": return ["30", "60", "120"]
    case "autoZoomStyle": return ["off", "subtle", "cinematic", "close"]
    case "autoZoomDepth": return ["1.5", "2.0", "2.5"]
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
        || key === "notifications" || key === "manualZoom")
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
    case "noiseCancellation":
    case "cursor":
    case "notifications":
    case "manualZoom":
      return value === "on" ? "On" : "Off"
    case "zoomMax":
      return value + "x"
    case "autoZoomDepth":
      return value + "x"
    case "autoZoomStyle":
      return value === "off" ? "Off" : value.charAt(0).toUpperCase() + value.slice(1)
    case "panStep":
      return value + " px"
    case "webcam":
      return value === "off" ? "Off" : value.charAt(0).toUpperCase() + value.slice(1)
    case "webcamDevice":
      return Model.cameraLabel(root.cameras, value)
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
    case "outputDir":
      return Model.shortPath(root.outputDir, root.home)
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

  function countdownLeft() {
    var tick = root.countdownTick
    if (root.status !== "starting" || String(root.state.stage || "") !== "countdown") return 0
    var settings = root.state.settings || ({})
    var count = Number(settings.countdown || 0)
    var started = Number(root.state.startedAt || 0)
    if (count <= 0 || started <= 0) return 0
    return Math.max(0, Math.ceil(count - (Date.now() / 1000 - started)))
  }

  function barGlyph() {
    // The bar answers "what is happening?" with a mark rather than a word:
    // pause, save, target pick, or countdown. Recording speaks with the clock.
    if (root.paused) return "󰏤"
    if (root.finalizing) return "󰆓"
    if (root.starting) {
      var stage = String(root.state.stage || "select")
      if (stage === "countdown") {
        var left = root.countdownLeft()
        return left > 0 ? String(left) : "󰔟"
      }
      if (stage === "warmup") {
        var warmupSettings = root.state.settings || ({})
        var warmupCount = Math.max(0, Number(warmupSettings.countdown || 0))
        return warmupCount > 0 ? String(warmupCount) : "●"
      }
      if (stage === "select") return "󰒉"
    }
    return ""
  }

  function barLabel() {
    if (root.paused) return ""
    if (root.active || root.capturing) return Model.clock(root.elapsed)
    return ""
  }

  function barTooltip() {
    var text = "Reel · click to open, right-click to record"
    if (root.finalizing) text = "Reel · saving the take"
    else if (root.paused) text = "Reel · paused at " + Model.clock(root.elapsed) + " · right-click to resume"
    else if (root.capturing) text = "Reel · recording " + Model.clock(root.elapsed)
    else if (root.starting) text = "Reel · waiting for a selection"
    else if (root.active) text = "Reel · recording " + Model.clock(root.elapsed) + " · right-click to pause"
    if (root.updateAvailable) text += " · update available"
    return text
  }

  // The header button is both the manual refresh and the update cue: a quiet
  // dot when the install is current, the same dot in accent when a newer
  // commit is waiting.
  function currentVersion() {
    var value = root.service && root.service.version ? String(root.service.version) : ""
    if (value === "") value = String(root.updateInfo.current || "")
    return value
  }

  function updatePillLabel() {
    return Model.updatePillLabel(root.updateInfo, root.currentVersion(), root.updateChecking)
  }

  function updateVersionLabel() {
    var latest = String(root.updateInfo.latest || "")
    var current = root.currentVersion()
    return latest !== "" && latest !== current ? "v" + latest : ""
  }

  function updateTooltip() {
    if (root.updateChecking) return "Checking for updates"
    if (String(root.updateInfo.error || "") !== "")
      return "Could not check for updates. Click to retry."
    if (root.updateAvailable) {
      var version = root.updateVersionLabel()
      return version !== ""
        ? "Update available: " + version + " (Shift+U)"
        : "Update available (Shift+U)"
    }
    return "Check for updates"
  }

  function handleUpdate() {
    if (root.updateChecking) return
    if (root.updateAvailable) {
      root.updateReel()
      return
    }
    if (!root.service) return
    root.updateNoticePending = true
    root.service.checkForUpdates(true)
  }

  function primaryLabel() {
    if (root.starting) return "Cancel"
    if (root.active) return "Stop"
    return "Record"
  }

  function primaryGlyph() {
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
    if (pauseProc.running) {
      root.setNotice("Reel is still finishing the last command.")
      return
    }
    root.cancelling = false
    pauseProc.command = ["reel", "toggle-pause"]
    pauseProc.running = true
  }

  function toggleRecording() {
    if (root.active || root.starting) root.stop()
    else root.start()
  }

  function openVideo(row) {
    var command = Model.playCommand(row && row.path)
    if (command.length > 0) Quickshell.execDetached(command)
  }

  // Cut is the only history action that launches Omacut. Opening a take always
  // plays it, so Enter never jumps into the trimmer by surprise.
  function cutTake(row) {
    if (!row || !row.path) return
    var command = Model.cutCommand(row.path, root.hasOmacut)
    if (command.length === 0) {
      root.setNotice("Omacut is not installed, so this take cannot be trimmed.", true)
      return
    }
    Quickshell.execDetached(command)
  }

  function openFolder() {
    Quickshell.execDetached(["xdg-open", root.outputDir])
  }

  function copyPath(path) {
    if (!path) return
    Quickshell.execDetached(["wl-copy", String(path)])
    root.setNotice("Path copied.")
  }

  // The updater runs in a floating terminal rather than the panel process: the
  // plugin update reloads this widget, so an in-panel Process would be torn
  // down before its installer finished. The terminal also keeps the installer
  // prompt and its output visible.
  function updateReel() {
    root.closeTakeMenu()
    root.close()
    Quickshell.execDetached([
      "omarchy-launch-floating-terminal-with-presentation",
      "reel update"
    ])
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
    var surface = keyCatcher
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
    if (key === "open") {
      root.closeTakeMenu()
      root.openVideo(row)
    } else if (key === "cut") {
      root.closeTakeMenu()
      root.cutTake(row)
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

  function addPendingTrash(path) {
    var next = {}
    for (var key in root.pendingTrash) next[key] = true
    next[String(path)] = true
    root.pendingTrash = next
  }

  function dropPendingTrash(path) {
    var next = {}
    for (var key in root.pendingTrash) {
      if (key !== String(path)) next[key] = true
    }
    root.pendingTrash = next
  }

  function prunePendingTrash() {
    var next = {}
    var rows = root.recents || []
    for (var key in root.pendingTrash) {
      var found = false
      for (var i = 0; i < rows.length; i++) {
        if (String(rows[i].path || "") === key) {
          found = true
          break
        }
      }
      if (found) next[key] = true
    }
    root.pendingTrash = next
  }

  function beginOptimisticTrash(row) {
    if (!row || !row.path) return
    if (root.removingPath !== "" || trashProc.running) return
    var path = String(row.path)
    var index = -1
    for (var i = 0; i < root.visibleRecents.length; i++) {
      if (String(root.visibleRecents[i].path || "") === path) {
        index = i
        break
      }
    }
    root.historyAnchorPath = path
    root.historyAnchorIndex = index >= 0 ? index + 1 : root.selectedIndex
    root.suppressHistoryHover = true
    root.trashInFlightPath = path
    root.removingPath = path
    historyHoverTimer.interval = 1200
    historyHoverTimer.restart()
    trashTimer.restart()
  }

  function commitOptimisticTrash() {
    var path = root.removingPath
    if (path === "") return
    root.addPendingTrash(path)
    root.removingPath = ""
    if (trashProc.running) return
    trashProc.command = ["reel", "trash", path]
    trashProc.running = true
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
    root.beginOptimisticTrash(row)
  }

  // Keep the history cursor on the row the user was looking at after the list
  // shrinks. The removed item's slot is taken by the next take; if it was the
  // last row, clamp to the new end instead of jumping past it.
  function settleHistorySelection() {
    if (root.page !== "history") return
    var anchor = root.historyAnchorPath
    if (anchor !== "") {
      var index = -1
      for (var i = 0; i < root.visibleRecents.length; i++) {
        if (String(root.visibleRecents[i].path || "") === anchor) {
          index = i
          break
        }
      }
      var limit = Math.max(0, root.visibleRecents.length)
      root.selectedIndex = index >= 0
        ? index + 1
        : Math.min(Math.max(1, root.historyAnchorIndex), limit)
      root.historyAnchorPath = ""
    } else {
      root.selectedIndex = Math.min(root.selectedIndex,
        Math.max(0, root.visibleRecents.length))
    }
    historyHoverTimer.interval = 260
    historyHoverTimer.restart()
  }

  function trashSelected() {
    root.trashTake(root.selectedTake())
  }

  function cutSelected() {
    var row = root.selectedTake()
    if (row) root.cutTake(row)
  }

  function shareSelected() {
    var row = root.selectedTake()
    if (row) root.share(row)
  }

  function primaryAction(index) {
    if (index === 0) {
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
    : ["header", "primary", "settings"]

  function sectionCount(section) {
    switch (section) {
    case "header": return 2
    case "primary": return 3
    case "settings": return root.settingsCount()
    case "recents": return Math.max(1, root.visibleRecents.length + 1)
    default: return 0
    }
  }

  function moveSection(delta) {
    root.clearHover()
    root.beginKeyboardScroll()
    if (root.focusSection === "settings") {
      root.switchSettingsTab(delta)
      return
    }
    var index = root.sections.indexOf(root.focusSection)
    if (index < 0) index = 0
    index = (index + delta + root.sections.length) % root.sections.length
    root.focusSection = root.sections[index]
    root.selectedIndex = delta > 0 ? 0 : root.sectionCount(root.focusSection) - 1
  }

  function moveCursor(dx, dy) {
    root.clearHover()
    root.beginKeyboardScroll()
    if (root.focusSection === "settings") {
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
    if (root.focusSection === "recents") {
      var count = root.sectionCount(root.focusSection)
      var next = root.selectedIndex + dy
      if (next >= 0 && next < count) {
        root.selectedIndex = next
        return
      }
    }
    root.moveSection(dy)
  }

  function beginKeyboardScroll() {
    root.keyboardScroll = true
    Qt.callLater(root.endKeyboardScroll)
  }

  function endKeyboardScroll() {
    root.keyboardScroll = false
  }

  function clearHover() {
    root.hoverSection = ""
    root.hoverIndex = -1
  }

  function setRowHover(section, index, on) {
    if (on) {
      root.hoverSection = section
      root.hoverIndex = index
      return
    }
    if (root.hoverSection === section && root.hoverIndex === index) {
      root.hoverSection = ""
      root.hoverIndex = -1
      root.cursorActive = false
    }
  }

  function adjust(direction) {
    root.beginKeyboardScroll()
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
  }

  function activateCursor() {
    root.clearHover()
    switch (root.focusSection) {
    case "header":
      if (root.selectedIndex === 0) root.handleUpdate()
      else root.toggleHistory()
      break
    case "primary":
      root.primaryAction(root.selectedIndex)
      break
    case "settings":
      root.activateSettings()
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
    var raw = String(text || "")
    if (root.takeMenuOpen) {
      if (raw.toLowerCase() === "a") root.closeTakeMenu()
      return
    }
    if (root.typingPrefix) return
    // Shift+U is the updater. Check the unlowered text so it does not fall
    // through to u, which shares the selected take.
    if (raw === "U") {
      root.updateReel()
      return
    }
    var key = raw.toLowerCase()
    if (root.page === "home" && key.length === 1 && key >= "1" && key <= "4") {
      var tab = Number(key) - 1
      if (tab >= 0 && tab < root.settingsTabs.length) {
        root.clearHover()
        root.settingsTab = root.settingsTabs[tab].id
        root.settingsIndex = 0
        root.focusSection = "settings"
        root.cursorActive = true
        root.beginKeyboardScroll()
      }
      return
    }
    if (key === "q") {
      root.close()
      return
    }
    if (key === "s") {
      root.toggleSettings()
      return
    }
    if (key === "r") root.toggleRecording()
    else if (key === "p") root.togglePause()
    else if (key === "d") root.discard()
    else if (key === "o") root.openFolder()
    else if (key === "t") root.cutSelected()
    else if (key === "u") root.shareSelected()
    else if (key === "a") {
      if (keyCatcher
          && typeof keyCatcher.openTakeMenuForSelected === "function")
        keyCatcher.openTakeMenuForSelected()
    }
    else if (key === "c") {
      var row = root.selectedTake()
      if (row) root.copyPath(row.path)
    }
  }

  function back() {
    if (root.focusSection === "settings") {
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

  // ---------- settings cursor --------------------------------------------

  function openHistory() {
    if (root.page === "history") return
    root.clearHover()
    if (!omacutProbe.running) omacutProbe.running = true
    var area = root.scrollArea()
    var flick = area ? area.contentItem : null
    if (flick && flick.contentY !== undefined) flick.contentY = 0
    root.page = "history"
    root.focusSection = "recents"
    root.selectedIndex = 0
    root.cursorActive = true
    root.historyAnchorPath = ""
    root.suppressHistoryHover = false
    Qt.callLater(root.focusPanelKeys)
  }

  function closeHistory() {
    root.clearHover()
    root.page = "home"
    root.focusSection = "primary"
    root.selectedIndex = 0
    root.cursorActive = true
    root.historyAnchorPath = ""
    root.suppressHistoryHover = false
    Qt.callLater(root.focusPanelKeys)
  }

  function toggleHistory() {
    if (root.page === "history") root.closeHistory()
    else root.openHistory()
  }

  // The Webcam device row can only offer the cameras in root.cameras, so read
  // them before the row is shown: on panel open (the row is reachable straight
  // from the settings section) and again whenever settings open, which is the
  // point where a camera may have been plugged in since. A finished probe has
  // running === false again, so this also retries after a failure.
  function refreshCameras() {
    if (!cameraProbe.running) cameraProbe.running = true
  }

  function openSettings() {
    if (root.page === "history") root.page = "home"
    root.clearHover()
    root.beginKeyboardScroll()
    root.focusSection = "settings"
    root.settingsIndex = 0
    root.refreshCameras()
    root.typingPrefix = false
    root.cursorActive = true
    root.historyAnchorPath = ""
    root.suppressHistoryHover = false
    Qt.callLater(root.focusPanelKeys)
  }

  function closeSettings() {
    root.clearHover()
    root.focusSection = "primary"
    root.selectedIndex = 0
    root.typingPrefix = false
    root.cursorActive = true
    Qt.callLater(root.focusPanelKeys)
  }

  function focusPanelKeys() {
    if (keyCatcher) keyCatcher.forceActiveFocus()
  }

  function toggleSettings() {
    if (root.focusSection === "settings") root.closeSettings()
    else root.openSettings()
  }

  function settingsTabData() {
    for (var i = 0; i < root.settingsTabs.length; i++)
      if (root.settingsTabs[i].id === root.settingsTab) return root.settingsTabs[i]
    return root.settingsTabs[0]
  }

  function settingsRows() {
    return root.settingsTabData().rows
  }

  function switchSettingsTab(delta) {
    root.clearHover()
    root.beginKeyboardScroll()
    var index = 0
    for (var i = 0; i < root.settingsTabs.length; i++)
      if (root.settingsTabs[i].id === root.settingsTab) index = i
    index = (index + delta + root.settingsTabs.length) % root.settingsTabs.length
    root.settingsTab = root.settingsTabs[index].id
    root.settingsIndex = 0
    root.cursorActive = true
  }

  function settingsCount() {
    return root.settingsRows().length
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
    if (dx !== 0) {
      if (root.settingsTab === "keys") return
      var row = root.settingsRows()[root.settingsIndex]
      if (row && row.kind !== "info") root.cycleOption(row.key, dx)
    }
  }

  function activateSettings() {
    if (root.settingsTab === "keys") return
    var row = root.settingsRows()[root.settingsIndex]
    if (row && row.kind !== "info") root.cycleOption(row.key, 1)
  }

  // ---------- scroll ------------------------------------------------------

  function scrollArea() {
    if (!keyCatcher) return null
    return keyCatcher.scrollArea || null
  }

  function resetScroll() {
    var area = root.scrollArea()
    var flick = area ? area.contentItem : null
    if (flick && flick.contentY !== undefined) flick.contentY = 0
  }

  function ensureCursorVisible(item) {    var area = root.scrollArea()
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
    root.clearHover()
    if (root.takeMenuOpen) root.closeTakeMenu()
  }

  onRecentsChanged: {
    root.prunePendingTrash()
    root.settleHistorySelection()
  }

  onFinalizingChanged: {
    if (root.finalizing) root.setNotice("Saving the take…")
  }

  onPendingTrashChanged: {
    root.settleHistorySelection()
  }

  onOpenedChanged: {
    if (opened) {
      root.clearHover()
      root.page = "home"
      root.focusSection = "primary"
      root.selectedIndex = 0
      root.cursorActive = true
      root.settingsIndex = 0
      root.settingsTab = "media"
      root.carryingFile = false
      root.notice = ""
      root.discardArmed = false
      root.trashArmedPath = ""
      root.historyAnchorPath = ""
      root.suppressHistoryHover = false
      root.takeMenuOpen = false
      root.takeMenuRow = null
      root.takeMenuIndex = 0
      root.syncConfig()
      root.refreshCameras()
      if (root.service) root.service.refresh()
      Qt.callLater(root.resetScroll)
    } else {
      root.clearHover()
      root.carryingFile = false
      root.discardArmed = false
      root.trashArmedPath = ""
      root.historyAnchorPath = ""
      root.suppressHistoryHover = false
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
    fixedWidth: root.status === "recording" || root.capturing
      ? barLabelMetrics.advanceWidth(root.barLabel()) + Style.space(30)
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

      // An update waiting is actionable, so it earns a mark on the idle
      // widget. A take keeps the bar for its clock; the badge comes back
      // when the take is done.
      Rectangle {
        visible: root.updateAvailable
        x: 18.5 * parent.unit
        y: 0.5 * parent.unit
        width: 4.5 * parent.unit
        height: width
        radius: width / 2
        color: root.accent
      }
    }

    Row {
      visible: root.busy
      anchors.centerIn: parent
      spacing: Style.space(6)

      // Pause, save, target pick, and countdown are marks, not words. The
      // clock and the pulsing dot carry the recording state on their own.
      Text {
        visible: text !== ""
        anchors.verticalCenter: parent.verticalCenter
        textFormat: Text.PlainText
        text: root.barGlyph()
        color: root.paused || root.finalizing ? root.dim : root.foreground
        font.family: root.fontFamily
        font.pixelSize: Style.bar.iconFont
      }

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
          running: (root.status === "recording" || root.capturing) && !Style.reduceMotion
          loops: Animation.Infinite
          NumberAnimation { to: 0.4; duration: 1000; easing.type: Easing.InOutQuad }
          NumberAnimation { to: 1.0; duration: 1000; easing.type: Easing.InOutQuad }
        }
      }

      Text {
        visible: text !== ""
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
    focusTarget: keyCatcher
    contentWidth: keyboardPanel.fittedContentWidth(Style.space(440))
    contentHeight: Style.space(410)

    // A file drag needs the pointer to leave the panel and reach the window
    // underneath. With the normal full-screen mask the overlay would swallow
    // the drop, so the mask goes empty for the length of the carry.
    mask: Region {
      width: root.carryingFile ? 0 : keyboardPanel.screenW
      height: root.carryingFile ? 0 : keyboardPanel.screenH
    }

    ReelKeyCatcher {
      id: keyCatcher
      property alias scrollArea: scrollArea
      anchors.fill: parent
      blocked: root.typingPrefix

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
      ColumnLayout {
        anchors.fill: parent
        spacing: Style.space(10)

        // ---------- hero ----------

        PanelHero {
          Layout.fillWidth: true
          iconComponent: Component {
            // The old recording pill, now always present on the left: it reads
            // 0:00 at rest and the live elapsed time while a take is running.
            Item {
              width: Style.space(60)
              height: Style.space(22)

              Rectangle {
                anchors.fill: parent
                radius: height / 2
                color: root.restFill
                border.width: 1
                border.color: root.hairline

                Row {
                  anchors.centerIn: parent
                  spacing: Style.space(5)

                  Rectangle {
                    width: Style.space(6)
                    height: width
                    radius: width / 2
                    color: root.active
                      ? root.paused ? root.secondary : root.accent
                      : root.dim
                    anchors.verticalCenter: parent.verticalCenter

                    SequentialAnimation on opacity {
                      running: (root.status === "recording" || root.capturing)
                        && !root.paused && !Style.reduceMotion
                      loops: Animation.Infinite
                      NumberAnimation { to: 0.35; duration: 900; easing.type: Easing.InOutQuad }
                      NumberAnimation { to: 1.0; duration: 900; easing.type: Easing.InOutQuad }
                    }
                  }

                  Text {
                    textFormat: Text.PlainText
                    text: root.active || root.capturing ? Model.clock(root.elapsed) : "Ready"
                    color: root.active
                      ? root.paused ? root.secondary : root.foreground
                      : root.secondary
                    font.family: root.fontFamily
                    font.pixelSize: Style.font.bodySmall
                    font.bold: true
                    anchors.verticalCenter: parent.verticalCenter
                  }
                }
              }
            }
          }
          title: ""
          foreground: root.foreground
          fontFamily: root.fontFamily
          trailingControl: Component {
            Row {
              spacing: Style.space(2)

              // The version pill is the update control: low-contrast at rest,
              // accent when an update is waiting, and "Checking..." on a
              // manual check.
              Rectangle {
                id: versionPill
                anchors.verticalCenter: parent.verticalCenter
                implicitWidth: Math.max(Style.space(52),
                  versionLabel.implicitWidth + Style.space(16))
                width: implicitWidth
                height: Style.space(22)
                radius: height / 2
                color: root.updateAvailable
                  ? Util.alpha(root.accent, 0.16)
                  : versionPill.hot ? root.hoverFill : root.restFill
                border.width: 1
                border.color: root.updateAvailable ? root.accent
                  : versionPill.hot ? root.accent : root.hairline

                readonly property bool hot: (root.hoverSection === "header" && root.hoverIndex === 0)
                  || (root.cursorActive && root.focusSection === "header" && root.selectedIndex === 0
                    && root.hoverSection === "")

                Behavior on color {
                  ColorAnimation { duration: 120 }
                }
                Behavior on border.color {
                  ColorAnimation { duration: 120 }
                }

                Text {
                  id: versionLabel
                  anchors.centerIn: parent
                  textFormat: Text.PlainText
                  text: root.updatePillLabel()
                  color: root.updateAvailable ? root.accent
                    : versionPill.hot ? root.foreground : root.secondary
                  font.family: root.fontFamily
                  font.pixelSize: Style.font.bodySmall
                  font.bold: true
                }

                MouseArea {
                  id: versionMouse
                  anchors.fill: parent
                  hoverEnabled: true
                  cursorShape: root.updateChecking ? Qt.ArrowCursor : Qt.PointingHandCursor
                  enabled: !root.updateChecking
                  onContainsMouseChanged: root.setRowHover("header", 0, containsMouse)
                  onClicked: {
                    root.cursorActive = true
                    root.focusSection = "header"
                    root.selectedIndex = 0
                    root.handleUpdate()
                  }
                }

                PanelToolTip {
                  visible: versionMouse.containsMouse
                  text: root.updateTooltip()
                  fontFamily: root.fontFamily
                }
              }

              PanelActionButton {
                iconText: "󰋚"
                tooltipText: root.page === "history" ? "Back from history (Shift+H)" : "History (Shift+H)"
                foreground: root.page === "history" ? root.accent : root.foreground
                fontFamily: root.fontFamily
                hasCursor: (root.hoverSection === "header" && root.hoverIndex === 1)
                  || (root.cursorActive && root.focusSection === "header" && root.selectedIndex === 1
                    && root.hoverSection === "")
                onHovered: function(on) {
                  root.setRowHover("header", 1, on)
                }
                onClicked: {
                  root.cursorActive = true
                  root.focusSection = "header"
                  root.selectedIndex = 1
                  root.toggleHistory()
                }
              }
            }
          }
        }

        // ---------- primary actions ----------

        Row {
          Layout.fillWidth: true
          visible: root.page === "home"
          spacing: Style.space(8)

          ActionButton {
            width: (parent.width - parent.spacing * 2) / 3
            rowIndex: 0
            label: root.primaryLabel()
            glyph: root.primaryGlyph()
            tone: root.active || root.starting ? "urgent" : "accent"
            primary: true
            enabled: true
            hint: "r"
          }

          ActionButton {
            width: (parent.width - parent.spacing * 2) / 3
            rowIndex: 1
            label: root.paused ? "Resume" : "Pause"
            glyph: root.paused ? "󰐊" : "󰏤"
            tone: "neutral"
            enabled: root.active
            hint: "p"
          }

          ActionButton {
            width: (parent.width - parent.spacing * 2) / 3
            rowIndex: 2
            label: root.discardArmed ? "Discard?" : "Discard"
            glyph: "󰆴"
            tone: "neutral"
            enabled: root.active
            hint: "d"
          }
        }

        // The tab strip stays above the scroll area so it is always reachable
        // while the settings body scrolls underneath it.
        Item {
          Layout.fillWidth: true
          Layout.preferredHeight: Style.space(30)
          Layout.topMargin: Style.space(8)
          Layout.bottomMargin: Style.space(2)
          visible: root.page === "home"

          Row {
            id: settingsTabRow
            anchors.fill: parent
            spacing: 0

            Repeater {
              model: root.settingsTabs

              delegate: Item {
                id: settingsTabItem
                required property var modelData
                required property int index
                width: settingsTabRow.width / root.settingsTabs.length
                height: settingsTabRow.height

                readonly property bool active: root.settingsTab === modelData.id
                readonly property bool hot: settingsTabMouse.containsMouse

                Row {
                  anchors.centerIn: parent
                  spacing: Style.space(6)

                  Text {
                    textFormat: Text.PlainText
                    text: settingsTabItem.modelData.label
                    color: settingsTabItem.active ? root.accent
                      : settingsTabItem.hot ? root.foreground
                      : Util.alpha(root.foreground, 0.85)
                    font.family: root.fontFamily
                    font.pixelSize: Style.font.bodySmall
                    font.letterSpacing: 0.2
                    font.bold: settingsTabItem.active
                    anchors.verticalCenter: parent.verticalCenter
                    Behavior on color {
                      ColorAnimation { duration: 150 }
                    }
                  }

                  KeyHintChip {
                    label: String(settingsTabItem.index + 1)
                    active: settingsTabItem.active
                    anchors.verticalCenter: parent.verticalCenter
                  }
                }

                Rectangle {
                  anchors.bottom: parent.bottom
                  anchors.bottomMargin: 1
                  anchors.horizontalCenter: parent.horizontalCenter
                  width: Math.max(0, parent.width - Style.space(16))
                  height: 1
                  color: Util.alpha(root.foreground, 0.35)
                  opacity: settingsTabItem.hot && !settingsTabItem.active ? 1 : 0
                  Behavior on opacity {
                    NumberAnimation { duration: 120 }
                  }
                }

                Rectangle {
                  anchors.bottom: parent.bottom
                  anchors.bottomMargin: 1
                  anchors.horizontalCenter: parent.horizontalCenter
                  z: 1
                  width: settingsTabItem.active
                    ? Math.max(0, parent.width - Style.space(16)) : 0
                  height: Math.max(1, Style.space(2))
                  color: root.accent
                  Behavior on width {
                    NumberAnimation {
                      duration: 150
                      easing.type: Easing.OutCubic
                    }
                  }
                }

                MouseArea {
                  id: settingsTabMouse
                  anchors.fill: parent
                  hoverEnabled: true
                  cursorShape: Qt.PointingHandCursor
                  onClicked: {
                    root.settingsTab = settingsTabItem.modelData.id
                    root.settingsIndex = 0
                    root.cursorActive = true
                    root.typingPrefix = false
                  }
                }
              }
            }
          }

          Rectangle {
            anchors.bottom: parent.bottom
            width: parent.width
            height: 1
            color: root.hairline
          }
        }

        ScrollView {
          Layout.fillWidth: true
          Layout.fillHeight: true
          id: scrollArea
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
            spacing: Style.space(10)

            // ---------- settings ----------

            Column {
              visible: root.page === "home"
              width: parent.width
              spacing: Style.space(12)

              Column {
                id: settingsBody
                width: parent.width
                spacing: Style.space(12)

                Repeater {
                  model: root.settingsTab === "keys" ? [] : root.settingsRows()

                  Column {
                    required property var modelData
                    required property int index
                    width: parent.width
                    spacing: Style.space(4)

                    PanelSectionHeader {
                      visible: modelData.section !== undefined
                        && String(modelData.section) !== ""
                      text: modelData.section === undefined ? "" : String(modelData.section)
                      foreground: root.foreground
                      fontFamily: root.fontFamily
                    }

                    SettingRow {
                      width: parent.width
                      settingKey: modelData.key
                      label: modelData.label
                      glyph: modelData.glyph
                      kind: modelData.kind === undefined ? "cycle" : String(modelData.kind)
                      infoValue: modelData.value === undefined ? "" : String(modelData.value)
                      rowIndex: index
                    }
                  }
                }

                Repeater {
                  model: root.settingsTab === "keys" ? root.shortcutRows : []

                  ShortcutRow {
                    required property var modelData
                    required property int index
                    width: parent.width
                    row: modelData
                    rowIndex: index
                  }
                }

              Column {
                visible: root.settingsTab === "output"
                width: parent.width
                spacing: Style.space(4)

                PanelSectionHeader {
                  text: "Filename prefix"
                  foreground: root.foreground
                  fontFamily: root.fontFamily
                }

                Rectangle {
                  width: parent.width
                  implicitHeight: Style.space(30)
                  radius: Style.cornerRadius
                  color: root.restFill
                  border.width: 1
                  border.color: prefixInput.activeFocus ? root.accent : root.hairline
                  Behavior on border.color {
                    ColorAnimation { duration: 120 }
                  }

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
                visible: root.settingsTab === "output"
                textFormat: Text.PlainText
                text: "Recordings: " + Model.shortPath(root.outputDir, root.home)
                color: root.dim
                font.family: root.fontFamily
                font.pixelSize: Style.font.caption
                elide: Text.ElideMiddle
                width: parent.width
              }
              }
            }

            // ---------- history page ----------

            Column {
              width: parent.width
              visible: root.page === "history" && root.visibleRecents.length > 0
              spacing: Style.space(8)

              Item {
                width: parent.width
                implicitHeight: Math.max(takesHeader.implicitHeight, takesHint.implicitHeight)

                PanelSectionHeader {
                  id: takesHeader
                  text: root.visibleRecents.length === 0
                    ? "No takes yet"
                    : root.visibleRecents.length === 1
                      ? "1 take"
                      : root.visibleRecents.length + " takes"
                  foreground: root.foreground
                  fontFamily: root.fontFamily
                  anchors.left: parent.left
                  anchors.verticalCenter: parent.verticalCenter
                }

                Text {
                  id: takesHint
                  textFormat: Text.PlainText
                  text: root.visibleRecents.length > 0
                    ? "enter opens · t cuts · a actions · x trashes"
                    : "takes appear here after the first recording"
                  color: root.secondary
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
            }
          }
        }
      }

      Item {
        id: historyEmpty
        anchors.fill: parent
        anchors.topMargin: scrollArea.y
        visible: root.page === "history" && root.visibleRecents.length === 0
        z: 20

        Column {
          anchors.centerIn: parent
          width: Math.min(parent.width - Style.space(48), Style.space(320))
          spacing: Style.space(8)

          Rectangle {
            width: Style.space(46)
            height: width
            radius: width / 2
            anchors.horizontalCenter: parent.horizontalCenter
            color: "transparent"
            border.width: 1
            border.color: Util.alpha(root.accent, 0.45)

            Rectangle {
              width: Style.space(10)
              height: width
              radius: width / 2
              anchors.centerIn: parent
              color: root.accent
            }
          }

          Text {
            width: parent.width
            textFormat: Text.PlainText
            text: "No takes yet"
            color: root.foreground
            font.family: root.fontFamily
            font.pixelSize: Style.font.heading
            font.bold: true
            horizontalAlignment: Text.AlignHCenter
          }

          Text {
            width: parent.width
            textFormat: Text.PlainText
            text: "Record something and it will appear here."
            color: root.secondary
            font.family: root.fontFamily
            font.pixelSize: Style.font.bodySmall
            horizontalAlignment: Text.AlignHCenter
            wrapMode: Text.WordWrap
          }
        }
      }

      Item {
        id: noticeToast
        anchors.left: parent.left
        anchors.right: parent.right
        anchors.bottom: parent.bottom
        anchors.leftMargin: Style.space(10)
        anchors.rightMargin: Style.space(10)
        anchors.bottomMargin: Style.space(10)
        height: toastCard.height
        z: 900
        opacity: 0
        y: Style.space(10)
        scale: 0.97
        transformOrigin: Item.Center

        Connections {
          target: root
          function onNoticeSeqChanged() {
            if (root.notice === "") return
            noticeShow.stop()
            noticeHide.stop()
            noticeToast.opacity = 0
            noticeToast.y = Style.space(18)
            noticeToast.scale = 0.92
            toastProgress.width = toastCard.width
            toastProgressAnim.restart()
            noticeShow.restart()
          }
          function onNoticeChanged() {
            if (root.notice !== "") return
            noticeShow.stop()
            noticeHide.restart()
          }
        }

        ParallelAnimation {
          id: noticeShow
          NumberAnimation {
            target: noticeToast
            property: "opacity"
            to: 1
            duration: 160
            easing.type: Easing.OutCubic
          }
          NumberAnimation {
            target: noticeToast
            property: "y"
            to: 0
            duration: 200
            easing.type: Easing.OutBack
            easing.overshoot: 1.12
          }
          NumberAnimation {
            target: noticeToast
            property: "scale"
            to: 1
            duration: 200
            easing.type: Easing.OutBack
            easing.overshoot: 1.06
          }
        }

        ParallelAnimation {
          id: noticeHide
          NumberAnimation {
            target: noticeToast
            property: "opacity"
            to: 0
            duration: 120
            easing.type: Easing.InCubic
          }
          NumberAnimation {
            target: noticeToast
            property: "y"
            to: Style.space(12)
            duration: 150
            easing.type: Easing.InCubic
          }
          NumberAnimation {
            target: noticeToast
            property: "scale"
            to: 0.96
            duration: 150
            easing.type: Easing.InCubic
          }
        }

        Rectangle {
          x: toastCard.x - Style.space(2)
          y: toastCard.y + Style.space(2)
          width: toastCard.width + Style.space(4)
          height: toastCard.height + Style.space(4)
          radius: toastCard.radius + Style.space(2)
          color: Util.alpha(root.noticeUrgent ? root.urgent : root.accent, 0.10)
        }

        Rectangle {
          x: Style.space(1)
          y: Style.space(6)
          width: toastCard.width
          height: toastCard.height
          radius: toastCard.radius
          color: Util.alpha("#000000", 0.22)
        }

        Rectangle {
          x: Style.space(1)
          y: Style.space(3)
          width: toastCard.width
          height: toastCard.height
          radius: toastCard.radius
          color: Util.alpha("#000000", 0.20)
        }

        Rectangle {
          x: Style.space(1)
          y: Style.space(1)
          width: toastCard.width
          height: toastCard.height
          radius: toastCard.radius
          color: Util.alpha("#000000", 0.18)
        }

        Rectangle {
          id: toastCard
          width: Math.min(parent.width, Style.space(450))
          height: toastText.implicitHeight + Style.space(22)
          anchors.horizontalCenter: parent.horizontalCenter
          radius: Style.cornerRadius
          color: Commons.Color.background
          border.width: 1
          border.color: root.noticeUrgent
            ? Util.alpha(root.urgent, 0.55)
            : Util.alpha(root.accent, 0.38)
          clip: true

          Rectangle {
            anchors.fill: parent
            color: Util.alpha(root.noticeUrgent ? root.urgent : root.accent, 0.06)
          }

          Rectangle {
            width: root.notice !== "" ? Math.max(1, Style.space(3)) : 0
            height: parent.height
            anchors.left: parent.left
            color: root.noticeUrgent ? root.urgent : root.accent
            Behavior on width {
              NumberAnimation { duration: 180; easing.type: Easing.OutCubic }
            }
          }

          Rectangle {
            anchors.top: parent.top
            anchors.left: parent.left
            anchors.right: parent.right
            anchors.topMargin: 1
            anchors.leftMargin: 1
            anchors.rightMargin: 1
            height: 1
            color: Util.alpha(root.foreground, 0.10)
          }

          Rectangle {
            width: Style.space(22)
            height: width
            radius: width / 2
            anchors.left: parent.left
            anchors.leftMargin: Style.space(14)
            anchors.verticalCenter: parent.verticalCenter
            color: "transparent"
            border.width: 1
            border.color: Util.alpha(root.noticeUrgent ? root.urgent : root.accent, 0.45)

            Rectangle {
              width: Style.space(6)
              height: width
              radius: width / 2
              anchors.centerIn: parent
              color: root.noticeUrgent ? root.urgent : root.accent
            }
          }

          Text {
            id: toastText
            anchors.left: parent.left
            anchors.right: parent.right
            anchors.leftMargin: Style.space(46)
            anchors.rightMargin: Style.space(38)
            anchors.verticalCenter: parent.verticalCenter
            textFormat: Text.PlainText
            text: root.notice
            color: root.noticeUrgent ? root.urgent : root.foreground
            font.family: root.fontFamily
            font.pixelSize: Style.font.body
            wrapMode: Text.WordWrap
          }

          PanelActionButton {
            iconText: "󰅖"
            tooltipText: "Dismiss"
            foreground: root.secondary
            hoverColor: root.noticeUrgent ? root.urgent : root.accent
            fontFamily: root.fontFamily
            size: Style.space(22)
            anchors.top: parent.top
            anchors.right: parent.right
            anchors.topMargin: Style.space(5)
            anchors.rightMargin: Style.space(6)
            onClicked: {
              noticeTimer.stop()
              root.notice = ""
            }
          }

          Rectangle {
            id: toastProgress
            anchors.left: parent.left
            anchors.bottom: parent.bottom
            height: Math.max(1, Style.space(2))
            width: 0
            color: root.noticeUrgent ? root.urgent : root.accent
            opacity: 0.55

            NumberAnimation {
              id: toastProgressAnim
              target: toastProgress
              property: "width"
              from: toastCard.width
              to: 0
              duration: noticeTimer.interval
              easing.type: Easing.Linear
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

        Rectangle {
          x: root.takeMenuX + Style.space(2)
          y: root.takeMenuY + Style.space(5)
          width: root.takeMenuWidth
          height: root.takeMenuHeight
          radius: Style.cornerRadius
          color: Util.alpha("#000000", 0.28)
        }

        Rectangle {
          x: root.takeMenuX + Style.space(1)
          y: root.takeMenuY + Style.space(2)
          width: root.takeMenuWidth
          height: root.takeMenuHeight
          radius: Style.cornerRadius
          color: Util.alpha("#000000", 0.18)
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
                shortcut: String(modelData.shortcut || "")
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
      if (root.takeMenuOpen) {
        root.closeTakeMenu()
        return
      }
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
      if (event.text === "u") {
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
    property string hint: ""

    readonly property color toneColor: tone === "urgent" ? root.urgent : tone === "accent" ? root.accent : root.foreground
    readonly property color toneInk: tone === "neutral" ? root.foreground : root.inkOn(action.toneColor)

    readonly property bool pointerHovered: root.hoverSection === "primary" && root.hoverIndex === rowIndex
    readonly property bool keyboardCursor: root.cursorActive && root.page === "home"
      && root.focusSection === "primary" && root.selectedIndex === rowIndex

    hasCursor: pointerHovered || (keyboardCursor && root.hoverSection === "")
    onHasCursorChanged: if (hasCursor && root.keyboardScroll) root.ensureCursorVisible(action)
    foreground: root.foreground
    accent: root.accent
    fill: Style.hoverFillFor(root.foreground, root.accent)
    currentFill: root.selectedFill
    color: !enabled ? "transparent"
      : primary ? (actionMouse.pressed ? Qt.darker(action.toneColor, 0.92) : action.toneColor)
      : actionMouse.pressed ? root.selectedFill
      : hasCursor ? action.fill
      : root.restFill
    borderSpec: !enabled
      ? Border.controlSpec("normal", root.dim, root.accent)
      : primary ? Border.flat(keyboardCursor ? Qt.lighter(action.toneColor, 1.25) : "transparent",
          keyboardCursor ? 1 : 0)
      : Border.flat(hasCursor ? root.accent : root.hairline, 1)
    implicitHeight: Style.space(38)
    opacity: enabled ? 1.0 : 0.35
    scale: actionMouse.pressed ? 0.985 : 1.0
    Behavior on scale {
      NumberAnimation { duration: 90; easing.type: Easing.OutCubic }
    }

    Rectangle {
      visible: action.primary && action.enabled
      anchors.fill: parent
      radius: parent.radius
      gradient: Gradient {
        GradientStop {
          position: 0
          color: actionMouse.pressed ? Qt.darker(action.toneColor, 0.90)
            : Qt.lighter(action.toneColor, 1.05)
        }
        GradientStop {
          position: 1
          color: actionMouse.pressed ? Qt.darker(action.toneColor, 0.96)
            : action.toneColor
        }
      }
    }

    Rectangle {
      visible: action.primary && action.enabled
      anchors.top: parent.top
      anchors.left: parent.left
      anchors.right: parent.right
      anchors.topMargin: 1
      anchors.leftMargin: 1
      anchors.rightMargin: 1
      height: 1
      color: Util.alpha(root.foreground, 0.16)
      opacity: 0.6
    }

    Rectangle {
      anchors.fill: parent
      radius: parent.radius
      color: !action.primary || !action.enabled ? "transparent"
        : actionMouse.pressed ? Util.alpha(root.foreground, 0.18)
        : action.pointerHovered ? Util.alpha(root.foreground, 0.12)
        : "transparent"
      Behavior on color {
        ColorAnimation { duration: 120 }
      }
    }

    Row {
      anchors.left: parent.left
      anchors.leftMargin: Style.space(10)
      anchors.right: hintSlot.left
      anchors.rightMargin: Style.space(8)
      anchors.verticalCenter: parent.verticalCenter
      spacing: Style.space(10)

      Rectangle {
        width: Style.space(24)
        height: width
        anchors.verticalCenter: parent.verticalCenter
        color: action.primary ? "transparent" : root.restFill
        border.width: action.primary ? 0 : 1
        border.color: root.hairline

        Text {
          anchors.centerIn: parent
          textFormat: Text.PlainText
          text: action.glyph
          color: action.primary ? action.toneInk : root.foreground
          font.family: root.fontFamily
          font.pixelSize: Style.font.bodySmall
        }
      }

      Text {
        width: Math.max(0, parent.width - Style.space(24) - parent.spacing)
        textFormat: Text.PlainText
        text: action.label
        color: action.primary ? action.toneInk : root.foreground
        font.family: root.fontFamily
        font.pixelSize: Style.font.body
        font.letterSpacing: 0.3
        font.bold: true
        elide: Text.ElideRight
        anchors.verticalCenter: parent.verticalCenter
      }
    }

    KeyHintChip {
      id: hintSlot
      visible: action.hint !== "" && action.enabled
      anchors.right: parent.right
      anchors.rightMargin: Style.space(8)
      anchors.verticalCenter: parent.verticalCenter
      label: action.hint
      onAccent: action.primary
      ink: action.toneInk
    }

    HoverHandler {
      onHoveredChanged: root.setRowHover("primary", action.rowIndex, hovered)
    }

    MouseArea {
      id: actionMouse
      anchors.fill: parent
      hoverEnabled: true
      enabled: action.enabled
      cursorShape: Qt.PointingHandCursor
      onContainsMouseChanged: root.setRowHover("primary", action.rowIndex, containsMouse)
      onClicked: {
        root.cursorActive = true
        root.focusSection = "primary"
        root.selectedIndex = action.rowIndex
        root.primaryAction(action.rowIndex)
      }
    }
  }

  component TakeMenuRow: Rectangle {
    id: menuItem
    property int itemIndex: 0
    property string glyph: ""
    property string label: ""
    property string shortcut: ""
    property bool selected: false
    property bool urgent: false

    height: root.takeMenuRowHeight
    radius: Style.cornerRadius
    color: menuItem.selected
      ? (menuItem.urgent ? root.urgentSoft : root.selectedFill)
      : "transparent"
    Behavior on color {
      ColorAnimation { duration: 120 }
    }

    Rectangle {
      visible: menuItem.selected
      width: Math.max(1, Style.space(2))
      height: Math.max(0, parent.height - Style.space(8))
      anchors.left: parent.left
      anchors.leftMargin: Style.space(2)
      anchors.verticalCenter: parent.verticalCenter
      color: menuItem.urgent ? root.urgent : root.accent
    }

    Row {
      anchors.fill: parent
      anchors.leftMargin: Style.space(8)
      anchors.rightMargin: Style.space(8)
      spacing: Style.space(8)

      Text {
        textFormat: Text.PlainText
        text: menuItem.glyph
        color: menuItem.urgent ? root.urgent
          : menuItem.selected ? root.foreground : root.secondary
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
          - (shortcutText.visible ? shortcutText.implicitWidth + Style.space(8) : 0)
      }

      Text {
        id: shortcutText
        visible: menuItem.shortcut !== ""
        width: visible ? implicitWidth : 0
        textFormat: Text.PlainText
        text: menuItem.shortcut
        color: menuItem.urgent ? root.urgent : root.secondary
        opacity: 0.8
        font.family: root.fontFamily
        font.pixelSize: Style.font.caption
        anchors.verticalCenter: parent.verticalCenter
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
    readonly property bool removing: root.removingPath !== ""
      && root.removingPath === String(row.path || "")
    readonly property bool menuOpen: root.takeMenuOpen && root.takeMenuRow !== null
      && String(root.takeMenuRow.path || "") === String(row.path || "")

    function openMenu() {
      if (take.menuOpen) {
        root.closeTakeMenu()
        return
      }
      root.openTakeMenu(take.row, take.rowIndex, menuButton)
    }

    property bool pressing: false
    property bool dragWanted: false
    property bool dragging: false
    property bool dragStarted: false
    property point pressAt: Qt.point(0, 0)
    property var dragMime: ({})

    function beginDrag() {
      take.dragWanted = false
      if (!take.pressing || take.dragging) return
      var path = String(take.row ? take.row.path || "" : "")
      if (path === "") return
      take.dragMime = {
        "text/uri-list": Util.fileUrl(path) + "\r\n",
        "text/plain": path
      }
      take.dragging = true
      take.dragStarted = true
      root.carryingFile = true
      // This enters the platform drag loop; everything after it runs on drop.
      take.Drag.active = true
    }

    function finishDrag() {
      take.dragging = false
      take.dragWanted = false
      take.pressing = false
      take.Drag.active = false
      take.dragMime = ({})
      root.carryingFile = false
      dragClickGuard.restart()
    }

    Drag.dragType: Drag.Automatic
    Drag.supportedActions: Qt.CopyAction
    Drag.proposedAction: Qt.CopyAction
    Drag.mimeData: take.dragMime
    Drag.hotSpot: Qt.point(take.pressAt.x, take.pressAt.y)
    Drag.onDragFinished: take.finishDrag()

    Timer {
      id: dragClickGuard
      interval: 80
      onTriggered: take.dragStarted = false
    }

    readonly property bool pointerHovered: root.hoverSection === "recents" && root.hoverIndex === rowIndex
    readonly property bool keyboardCursor: root.cursorActive && root.page === "history"
      && root.focusSection === "recents" && root.selectedIndex === rowIndex

    hasCursor: pointerHovered || (keyboardCursor && root.hoverSection === "")
    onHasCursorChanged: if (hasCursor && root.keyboardScroll) root.ensureCursorVisible(take)
    foreground: root.foreground
    accent: root.accent
    fill: Style.hoverFillFor(root.foreground, root.accent)
    implicitHeight: removing ? 0 : Style.space(56)
    radius: Style.cornerRadius
    clip: true
    opacity: removing ? 0 : 1
    Behavior on opacity {
      NumberAnimation { duration: 120 }
    }
    Behavior on implicitHeight {
      NumberAnimation { duration: 160; easing.type: Easing.OutCubic }
    }

    Row {
      anchors.fill: parent
      anchors.leftMargin: Style.space(6)
      anchors.rightMargin: Style.space(6)
      spacing: Style.space(9)

      Item {
        width: Style.space(84)
        height: Style.space(48)
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
        width: parent.width - Style.space(84) - actions.width - parent.spacing * 2
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

    Rectangle {
      visible: take.hasCursor
      width: Math.max(1, Style.space(2))
      height: Math.max(0, parent.height - Style.space(12))
      anchors.left: parent.left
      anchors.leftMargin: Style.space(2)
      anchors.verticalCenter: parent.verticalCenter
      radius: width / 2
      color: root.accent
    }

    MouseArea {
      anchors.fill: parent
      z: -1
      hoverEnabled: true
      preventStealing: true
      acceptedButtons: Qt.LeftButton | Qt.RightButton
      cursorShape: Qt.PointingHandCursor
      onContainsMouseChanged: {
        if (containsMouse && root.suppressHistoryHover) return
        root.setRowHover("recents", take.rowIndex, containsMouse)
      }
      onPressed: function(mouse) {
        if (mouse.button !== Qt.LeftButton) return
        take.pressAt = Qt.point(mouse.x, mouse.y)
        take.pressing = true
      }
      onPositionChanged: function(mouse) {
        if (!take.pressing || take.dragging || take.dragWanted) return
        if (!(mouse.buttons & Qt.LeftButton)) return
        var dx = mouse.x - take.pressAt.x
        var dy = mouse.y - take.pressAt.y
        if (Math.abs(dx) + Math.abs(dy) >= Qt.styleHints.startDragDistance) {
          take.dragWanted = true
          Qt.callLater(take.beginDrag)
        }
      }
      onReleased: {
        take.pressing = false
        take.dragWanted = false
      }
      onCanceled: {
        take.pressing = false
        take.dragWanted = false
      }
      onClicked: function(mouse) {
        if (take.dragStarted) return
        root.cursorActive = true
        root.focusSection = "recents"
        root.selectedIndex = take.rowIndex
        if (mouse.button === Qt.RightButton) take.openMenu()
        else root.openVideo(take.row)
      }
    }
  }

  component OpenFolderRow: CursorSurface {
    id: folder
    property int rowIndex: 0

    readonly property bool pointerHovered: root.hoverSection === "recents" && root.hoverIndex === rowIndex
    readonly property bool keyboardCursor: root.cursorActive && root.page === "history"
      && root.focusSection === "recents" && root.selectedIndex === rowIndex

    hasCursor: pointerHovered || (keyboardCursor && root.hoverSection === "")
    onHasCursorChanged: if (hasCursor && root.keyboardScroll) root.ensureCursorVisible(folder)
    foreground: root.foreground
    accent: root.accent
    fill: Style.hoverFillFor(root.foreground, root.accent)
    implicitHeight: Style.space(30)
    radius: Style.cornerRadius

    Row {
      anchors.fill: parent
      anchors.leftMargin: Style.space(6)
      anchors.rightMargin: Style.space(8)
      spacing: Style.space(8)

      Rectangle {
        width: Style.space(24)
        height: width
        anchors.verticalCenter: parent.verticalCenter
        color: root.restFill
        border.width: 1
        border.color: root.hairline

        Text {
          anchors.centerIn: parent
          textFormat: Text.PlainText
          text: "󰉋"
          color: root.foreground
          font.family: root.fontFamily
          font.pixelSize: Style.font.body
        }
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

    Rectangle {
      visible: folder.hasCursor
      width: Math.max(1, Style.space(2))
      height: Math.max(0, parent.height - Style.space(10))
      anchors.left: parent.left
      anchors.leftMargin: Style.space(2)
      anchors.verticalCenter: parent.verticalCenter
      radius: width / 2
      color: root.accent
    }

    MouseArea {
      anchors.fill: parent
      hoverEnabled: true
      cursorShape: Qt.PointingHandCursor
      onContainsMouseChanged: {
        if (containsMouse && root.suppressHistoryHover) return
        root.setRowHover("recents", folder.rowIndex, containsMouse)
      }
      onClicked: {
        root.cursorActive = true
        root.focusSection = "recents"
        root.selectedIndex = folder.rowIndex
        root.openFolder()
      }
    }
  }

  component KeyHintChip: Item {
    id: keyHint
    property string label: ""
    property bool onAccent: false
    property color ink: root.foreground
    property bool active: false
    property real minimumWidth: 0

    width: implicitWidth
    height: implicitHeight
    implicitWidth: chip.width
    implicitHeight: chip.height

    Rectangle {
      visible: !keyHint.onAccent
      anchors.fill: chip
      anchors.topMargin: Style.space(2)
      anchors.leftMargin: Style.space(1)
      anchors.rightMargin: -Style.space(1)
      anchors.bottomMargin: -Style.space(2)
      radius: chip.radius + Style.space(1)
      color: Util.alpha("#000000", 0.22)
    }

    Rectangle {
      visible: !keyHint.onAccent
      anchors.fill: chip
      anchors.topMargin: Style.space(1)
      anchors.bottomMargin: -Style.space(1)
      radius: chip.radius
      color: Util.alpha("#000000", 0.18)
    }

    Rectangle {
      id: chip
      width: Math.max(keyHint.minimumWidth, labelText.implicitWidth + Style.space(10))
      height: Style.space(18)
      radius: Style.space(4)
      // Opaque base: the keycap shadow sits underneath, and a translucent
      // fill would let it darken the whole pill in light themes.
      color: keyHint.onAccent ? Util.alpha(keyHint.ink, 0.12) : Commons.Color.background
      border.width: 1
      border.color: keyHint.onAccent ? Util.alpha(keyHint.ink, 0.26)
        : keyHint.active ? Util.alpha(root.accent, 0.55)
        : root.strongBorder

      Rectangle {
        anchors.fill: parent
        radius: parent.radius
        visible: !keyHint.onAccent
        color: keyHint.active ? Util.alpha(root.accent, 0.14)
          : Util.alpha(root.foreground, 0.08)
      }

      Text {
        id: labelText
        anchors.centerIn: parent
        textFormat: Text.PlainText
        text: keyHint.label
        color: keyHint.onAccent ? keyHint.ink
          : keyHint.active ? root.foreground : Util.alpha(root.foreground, 0.82)
        font.family: root.fontFamily
        font.pixelSize: Style.font.caption
        font.bold: true
      }
    }
  }

  component ShortcutRow: CursorSurface {
    id: shortcut
    property var row: null
    property int rowIndex: 0

    readonly property bool pointerHovered: root.hoverSection === "settings" && root.hoverIndex === rowIndex
    readonly property bool selected: root.cursorActive && root.page === "home"
      && root.focusSection === "settings" && root.settingsIndex === rowIndex

    hasCursor: pointerHovered || (selected && root.hoverSection === "")
    onHasCursorChanged: if (hasCursor && root.keyboardScroll) root.ensureCursorVisible(shortcut)
    foreground: root.foreground
    accent: root.accent
    fill: Style.hoverFillFor(root.foreground, root.accent)
    implicitHeight: Style.space(30)
    radius: 0

    Row {
      anchors.fill: parent
      anchors.leftMargin: Style.space(6)
      anchors.rightMargin: Style.space(8)
      spacing: Style.space(10)

      KeyHintChip {
        id: keyChip
        anchors.verticalCenter: parent.verticalCenter
        label: shortcut.row ? String(shortcut.row.keys || "") : ""
        minimumWidth: Style.space(56)
        active: shortcut.hasCursor
      }

      Text {
        width: parent.width - keyChip.width - parent.spacing
        anchors.verticalCenter: parent.verticalCenter
        textFormat: Text.PlainText
        text: shortcut.row ? String(shortcut.row.what || "") : ""
        color: shortcut.hasCursor ? root.foreground : Util.alpha(root.foreground, 0.82)
        font.family: root.fontFamily
        font.pixelSize: Style.font.bodySmall
        elide: Text.ElideRight
      }
    }

    Rectangle {
      visible: shortcut.hasCursor
      width: Math.max(1, Style.space(2))
      height: Math.max(0, parent.height - Style.space(10))
      anchors.left: parent.left
      anchors.leftMargin: Style.space(2)
      anchors.verticalCenter: parent.verticalCenter
      color: root.accent
    }

    MouseArea {
      anchors.fill: parent
      hoverEnabled: true
      cursorShape: Qt.ArrowCursor
      onContainsMouseChanged: root.setRowHover("settings", shortcut.rowIndex, containsMouse)
      onClicked: {
        root.cursorActive = true
        root.focusSection = "settings"
        root.settingsIndex = shortcut.rowIndex
      }
    }
  }

  component SettingRow: CursorSurface {
    id: setting
    property string settingKey: ""
    property string label: ""
    property string glyph: ""
    property int rowIndex: 0
    property bool quick: false
    property string kind: "cycle"
    property string infoValue: ""

    readonly property bool pointerHovered: root.hoverSection === "settings" && root.hoverIndex === rowIndex
    readonly property bool selected: root.cursorActive
      && root.page === "home"
      && root.focusSection === "settings"
      && root.settingsIndex === rowIndex
    readonly property bool switchRow: setting.kind !== "info" && root.isSwitch(setting.settingKey)
    readonly property bool switchOn: root.currentValue(setting.settingKey) === "on"

    hasCursor: pointerHovered || (selected && root.hoverSection === "")
    onHasCursorChanged: if (hasCursor && root.keyboardScroll) root.ensureCursorVisible(setting)
    foreground: root.foreground
    accent: root.accent
    fill: Style.hoverFillFor(root.foreground, root.accent)
    implicitHeight: Style.space(30)
    radius: Style.cornerRadius

    Row {
      anchors.fill: parent
      anchors.leftMargin: Style.space(6)
      anchors.rightMargin: Style.space(8)
      spacing: Style.space(8)

      Rectangle {
        width: Style.space(24)
        height: width
        anchors.verticalCenter: parent.verticalCenter
        color: root.restFill
        border.width: 1
        border.color: root.hairline

        Text {
          anchors.centerIn: parent
          textFormat: Text.PlainText
          text: setting.glyph
          color: root.foreground
          font.family: root.fontFamily
          font.pixelSize: Style.font.body
        }
      }

      Text {
        textFormat: Text.PlainText
        text: setting.label
        color: root.foreground
        font.family: root.fontFamily
        font.pixelSize: Style.font.body
        anchors.verticalCenter: parent.verticalCenter
        width: parent.width - Style.space(24)
          - (setting.switchRow ? Style.space(32)
            : setting.kind === "info" ? valueText.width : valueChip.width)
          - parent.spacing * 2
        elide: Text.ElideRight
      }

      Text {
        id: valueText
        visible: setting.kind === "info"
        textFormat: Text.PlainText
        text: setting.infoValue !== ""
          ? setting.infoValue
          : root.valueLabel(setting.settingKey, root.currentValue(setting.settingKey))
        color: setting.hasCursor ? root.foreground : root.secondary
        font.family: root.fontFamily
        font.pixelSize: Style.font.bodySmall
        font.bold: true
        anchors.verticalCenter: parent.verticalCenter
        width: setting.kind === "info" ? Math.min(implicitWidth, Style.space(150)) : 0
        elide: Text.ElideMiddle
      }

      Rectangle {
        id: valueChip
        visible: !setting.switchRow && setting.kind !== "info"
        anchors.verticalCenter: parent.verticalCenter
        width: valueRow.implicitWidth + Style.space(16)
        height: Style.space(20)
        radius: Style.space(4)
        color: setting.hasCursor ? Util.alpha(root.foreground, 0.10) : root.restFill
        border.width: 1
        border.color: setting.hasCursor ? root.accent : root.hairline

        Row {
          id: valueRow
          anchors.centerIn: parent
          spacing: Style.space(6)

          Text {
            textFormat: Text.PlainText
            text: root.valueLabel(setting.settingKey, root.currentValue(setting.settingKey))
            color: setting.hasCursor ? root.foreground : root.secondary
            font.family: root.fontFamily
            font.pixelSize: Style.font.bodySmall
            font.bold: true
            anchors.verticalCenter: parent.verticalCenter
          }

          Text {
            textFormat: Text.PlainText
            text: "›"
            color: setting.hasCursor ? root.foreground : root.dim
            font.family: root.fontFamily
            font.pixelSize: Style.font.bodySmall
            anchors.verticalCenter: parent.verticalCenter
          }
        }

        Behavior on color {
          ColorAnimation { duration: 120 }
        }
        Behavior on border.color {
          ColorAnimation { duration: 120 }
        }
      }

      Rectangle {
        visible: setting.switchRow
        anchors.verticalCenter: parent.verticalCenter
        width: Style.space(32)
        height: Style.space(16)
        radius: height / 2
        color: setting.switchOn ? root.accent : Util.alpha(root.foreground, 0.18)
        border.width: 1
        border.color: setting.switchOn ? "transparent" : root.hairline

        Rectangle {
          width: parent.height - Style.space(4)
          height: width
          radius: width / 2
          y: (parent.height - height) / 2
          x: setting.switchOn ? parent.width - width - Style.space(2) : Style.space(2)
          color: setting.switchOn ? root.inkOn(root.accent) : root.secondary

          Behavior on x {
            NumberAnimation { duration: 120; easing.type: Easing.OutCubic }
          }
        }
      }
    }

    Rectangle {
      visible: setting.hasCursor
      width: Math.max(1, Style.space(2))
      height: Math.max(0, parent.height - Style.space(10))
      anchors.left: parent.left
      anchors.leftMargin: Style.space(2)
      anchors.verticalCenter: parent.verticalCenter
      radius: width / 2
      color: root.accent
    }

    MouseArea {
      anchors.fill: parent
      hoverEnabled: true
      cursorShape: setting.kind === "info" ? Qt.ArrowCursor : Qt.PointingHandCursor
      acceptedButtons: Qt.LeftButton | Qt.RightButton
      onContainsMouseChanged: root.setRowHover("settings", setting.rowIndex, containsMouse)
      onClicked: function(mouse) {
        if (setting.kind === "info") return
        root.cursorActive = true
        root.focusSection = "settings"
        root.settingsIndex = setting.rowIndex
        root.cycleOption(setting.settingKey, mouse.button === Qt.RightButton ? -1 : 1)
      }
    }
  }

  // ---------- processes ---------------------------------------------------

  Timer {
    id: trashTimer
    interval: 170
    repeat: false
    onTriggered: root.commitOptimisticTrash()
  }

  Timer {
    id: historyHoverTimer
    interval: 260
    repeat: false
    onTriggered: root.suppressHistoryHover = false
  }

  Timer {
    id: countdownTicker
    interval: 500
    repeat: true
    running: root.status === "starting"
      && String(root.state.stage || "") === "countdown"
    onTriggered: root.countdownTick++
  }

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

  // Cameras can be plugged in while the shell runs, so this is re-read on
  // panel open and every time settings open rather than only at startup.
  Process {
    id: cameraProbe
    running: false
    command: ["reel", "webcams", "--json"]
    stdout: StdioCollector { id: cameraOut; waitForEnd: true }
    // Only the list changes here: a stored camera that is unplugged keeps its
    // setting and shows as unplugged, and a failed probe keeps the last list.
    onExited: function(code) {
      if (code !== 0) return
      root.cameras = Model.parseCameras(String(cameraOut.text || ""))
    }
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

  // Pointer telemetry keeps `reel start` alive for the whole take, so pause
  // uses its own process instead of waiting for controlProc to return.
  Process {
    id: pauseProc
    running: false
    stdout: StdioCollector { waitForEnd: true }
    stderr: StdioCollector { id: pauseErr; waitForEnd: true }
    onExited: function(code) {
      var message = String(pauseErr.text || "").trim()
      if (code !== 0 && !root.cancelling) {
        root.setNotice(message.replace(/^reel:\s*/, "") || "That action failed.", true)
      }
      root.cancelling = false
      if (root.service) root.service.refresh()
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
      if (code !== 0) {
        root.dropPendingTrash(root.trashInFlightPath)
        root.setNotice(message.replace(/^reel:\s*/, "") || "Could not move that take to the trash.", true)
      } else {
        root.setNotice("Take moved to the trash.")
      }
      root.trashInFlightPath = ""
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

  Countdown { }

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
    function refresh(): void {
      if (root.service) {
        root.service.refresh()
        root.service.checkForUpdates(true)
      }
    }
  }

  Connections {
    target: root.service
    function onUpdateInfoChanged() {
      if (!root.updateNoticePending) return
      root.updateNoticePending = false
      var report = root.service && root.service.updateInfo
        ? root.service.updateInfo : Model.emptyUpdate()
      if (String(report.error || "") !== "") {
        root.setNotice("Could not check for updates.", true)
      } else if (report.available === true) {
        var latest = String(report.latest || "")
        var current = String(report.current || "")
        root.setNotice(latest !== "" && latest !== current
          ? "Update available: v" + latest + ". Press Shift+U to update."
          : "Update available. Press Shift+U to update.")
      } else {
        root.setNotice("Reel is up to date.")
      }
    }
  }

  Component.onCompleted: {
    if (root.service) root.service.refresh()
    if (!omacutProbe.running) omacutProbe.running = true
  }
}
