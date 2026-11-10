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
  readonly property var visibleRecents: Model.visibleRecents(
    root.recents, root.pendingTrash, root.pendingRenames)
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
  // A take in the save pass already belongs to the user, so history lists it
  // before it lands. The row is a placeholder: no pointer, no cursor, no
  // actions. It is held for the beat between "saving" ending and the refreshed
  // history carrying the take, so the row swaps in place instead of blinking
  // out and dropping back in a moment later.
  property bool savingHold: false
  property bool savingWasFinalizing: false
  property int savingHoldVersion: 0
  property int savingHoldCount: 0
  property string savingHoldPath: ""
  property string savingTitleCache: ""

  readonly property bool savingTake: root.finalizing || root.savingHold
  readonly property string savingTakePath: String(root.state.file || "")
  readonly property int recentsVersion: root.service
    ? root.service.recentVersion : 0
  readonly property string savingTakeTitle: {
    if (!root.savingTake) return ""
    if (root.finalizing) return Model.savingTitle(root.savingTakePath)
    return root.savingTitleCache !== ""
      ? root.savingTitleCache : Model.savingTitle(root.savingTakePath)
  }

  function historyContains(path) {
    var rows = root.visibleRecents || []
    for (var i = 0; i < rows.length; i++) {
      if (String(rows[i].path || "") === path) return true
    }
    return false
  }

  function releaseSavingHold() {
    root.savingHold = false
    root.savingHoldPath = ""
    root.savingTitleCache = ""
  }

  onSavingTakePathChanged: {
    if (root.finalizing) root.savingTitleCache = Model.savingTitle(root.savingTakePath)
  }

  onRecentsVersionChanged: {
    if (!root.savingHold || root.recentsVersion === root.savingHoldVersion) return
    if ((root.savingHoldPath !== "" && root.historyContains(root.savingHoldPath))
      || root.visibleRecents.length !== root.savingHoldCount)
      root.releaseSavingHold()
  }

  Timer {
    id: savingHoldTimer
    interval: 2500
    onTriggered: root.releaseSavingHold()
  }
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
  readonly property color urgentSoft: Util.alpha(root.urgent, 0.16)
  readonly property color restFill: Util.alpha(root.foreground, 0.04)
  readonly property color selectedFill: Util.alpha(root.foreground, 0.14)
  readonly property color hairline: Util.alpha(root.foreground, 0.12)
  readonly property color strongBorder: Util.alpha(root.foreground, 0.24)
  readonly property color secondary: Util.alpha(root.foreground, 0.72)
  readonly property string fontFamily: root.bar ? root.bar.fontFamily : Style.font.family
  readonly property color onAccent: root.inkOn(root.accent)

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
    "autoZoom": "off",
    "outputDir": "",
    "webcamDevice": ""
  })

  readonly property var recordingSettingRows: [
    { key: "desktopAudio", label: "Desktop audio", glyph: "󰕾", section: "Audio" },
    { key: "microphoneAudio", label: "Microphone", glyph: "󰍬" },
    { key: "noiseCancellation", label: "Noise reduction", glyph: "󱑽" },
    { key: "fps", label: "Frame rate", glyph: "󰈰", section: "Video" },
    { key: "quality", label: "Quality", glyph: "󰓅" },
    { key: "codec", label: "Video codec", glyph: "󰅴" },
    { key: "resolution", label: "Resolution cap", glyph: "󰍹" },
    { key: "container", label: "Container", glyph: "󰏗" },
    { key: "countdown", label: "Countdown", glyph: "󰔛", section: "Capture" },
    { key: "webcam", label: "Webcam corner", glyph: "󰄀" },
    { key: "webcamDevice", label: "Webcam device", glyph: "󰄈" },
    { key: "notifications", label: "Saved notification", glyph: "󰂚", section: "Saving" }
  ]
  readonly property var shortcutRows: [
    { keys: "Shift+U", what: "update Reel" },
    { keys: "↑ ↓ / j k", what: "move between rows" },
    { keys: "← →", what: "move across an action row" },
    { keys: "h / l", what: "move horizontally or cycle an option" },
    { keys: "enter / space", what: "start, stop, save, or open" },
    { keys: "tab", what: "switch sections or settings tabs" },
    { keys: "1–3", what: "switch settings tabs" },
    { keys: "r", what: "start or stop a take" },
    { keys: "p", what: "pause or resume" },
    { keys: "d", what: "discard the current take" },
    { keys: "Shift+H", what: "open or close history" },
    { keys: "s", what: "focus the settings tabs" },
    { keys: "a", what: "open the take actions menu" },
    { keys: "x / delete", what: "trash the selected take" },
    { keys: "u", what: "share the selected take" },
    { keys: "n", what: "rename the selected take" },
    { keys: "c", what: "copy the selected take's path" },
    { keys: "o", what: "open the recordings folder and close" },
    { keys: "q", what: "close the panel" },
    { keys: "esc", what: "go back, then close" }
  ]
  readonly property var settingsTabs: [
    { id: "recording", label: "Recording", rows: root.recordingSettingRows },
    { id: "visuals", label: "Visuals", rows: [
      { key: "manualZoom", label: "Manual zoom", glyph: "󰍉" },
      { key: "autoZoom", label: "Auto zoom", glyph: "󰁨" },
      { key: "cursor", label: "Record the pointer", glyph: "󰍽" }
    ] },
    { id: "shortcuts", label: "Shortcuts", rows: root.shortcutRows }
  ]
  // The strip is navigation, not settings: the three settings tabs plus History,
  // which is a page beside them. Keeping the same strip on both pages is what
  // makes History a destination rather than a mode with its own frame.
  readonly property var navDestinations: {
    var list = []
    for (var i = 0; i < root.settingsTabs.length; i++) {
      var tab = root.settingsTabs[i]
      list.push({ id: tab.id, label: tab.label, key: String(i + 1) })
    }
    list.push({ id: "history", label: "History", key: "H", modifier: "⇧", separated: true })
    return list
  }

  function isNavActive(id) {
    if (id === "history") return root.page === "history"
    return root.page === "home" && root.settingsTab === id
  }

  function selectNav(id) {
    root.typingPrefix = false
    root.cursorActive = true
    if (id === "history") {
      root.openHistory()
      return
    }
    root.clearHover()
    if (root.page === "history") root.closeHistory()
    root.settingsTab = id
    root.settingsIndex = 0
    root.focusSection = "settings"
    root.beginKeyboardScroll()
    Qt.callLater(root.resetScroll)
  }

  // ---------- panel state -------------------------------------------------

  property string page: "home"
  // Relative take times only move as the clock does, so the history list needs
  // a dependency that advances. takeMeta() reads this counter and re-runs its
  // binding; the value itself is never used.
  property int historyTick: 0
  property bool typingPrefix: false
  property string focusSection: "primary"
  property int selectedIndex: 0
  property string hoverSection: ""
  property int hoverIndex: -1
  // Hyprland hides the pointer on a key press (hide_on_key_press) and, when
  // configured, after its inactive timeout. Qt never reports that the pointer
  // is gone, and an item under a hidden pointer keeps reporting containsMouse.
  // Hover state is still tracked, but only painted while the pointer is known
  // to be active: any key stops it, real pointer movement restores it.
  property bool pointerActive: true
  readonly property bool hoverPainted: root.pointerActive && root.hoverSection !== ""
  // Scene point of the last real pointer movement. A MouseArea also emits
  // positionChanged when its row scrolls under a still pointer, so movement
  // only counts when this point actually changed.
  property point lastPointerScene: Qt.point(-100000, -100000)
  property bool cursorActive: false
  property bool keyboardScroll: false
  property bool carryingFile: false
  property int settingsIndex: 0
  property string settingsTab: "recording"
  // A tab is a different page, so it starts at its first row. Setting this on
  // the tab itself keeps every caller honest (tab click, number keys, Tab).
  onSettingsTabChanged: {
    root.settingsIndex = 0
    Qt.callLater(root.resetScroll)
  }
  property bool hasOmacut: false
  // Cameras for the Webcam device row, from `reel webcams --json`.
  property var cameras: []

  property string notice: ""
  property bool noticeUrgent: false
  property int noticeSeq: 0
  property string trashArmedPath: ""
  property var pendingTrash: ({})
  property var pendingRenames: ({})
  property string removingPath: ""
  property string trashInFlightPath: ""
  property string renameInFlightPath: ""
  property string historyAnchorPath: ""
  property int historyAnchorIndex: 0
  property bool suppressHistoryHover: false
  property bool discardArmed: false
  property bool cancelling: false
  property string pendingConfig: ""
  property var lastActionArgs: []
  property string controlAction: ""
  property string pendingNotice: ""
  property bool updateNoticePending: false

  // The take actions menu lives at panel level rather than inside each row so
  // the keyboard cursor keeps working while it is open. It anchors to the
  // row's menu button and floats over the history list.
  property bool takeMenuOpen: false
  property var takeMenuRow: null
  property var takeMenuAnchor: null
  property int takeMenuIndex: 0
  property real takeMenuX: 0
  property real takeMenuY: 0
  // Rename swaps the action rows for one text field in the same card, so it
  // never becomes a second modal.
  property bool takeMenuRenaming: false
  property string renamePath: ""
  property string renameText: ""
  property bool renameBusy: false
  readonly property real takeMenuWidth: Style.space(220)
  readonly property real takeMenuRowHeight: Style.space(30)
  readonly property real takeMenuPadding: Style.space(5)
  readonly property var takeMenuBorderSpec: Border.flat(root.hairline, 1)
  readonly property var takeMenuItems: [
    { key: "open", label: "Open", glyph: "󰏌", shortcut: "Enter" },
    { key: "cut", label: "Cut in Omacut", glyph: "󰆐", shortcut: "t" },
    { key: "share", label: "Share", glyph: "󰒗", shortcut: "u" },
    { key: "rename", label: "Rename", glyph: "󰏫", shortcut: "n" },
    {
      key: "trash",
      label: root.trashArmedPath !== "" && root.takeMenuRow
        && root.trashArmedPath === String(root.takeMenuRow.path || "")
        ? "Confirm delete" : "Delete",
      glyph: "󰩹",
      shortcut: "x",
      urgent: root.trashArmedPath !== "" && root.takeMenuRow
        && root.trashArmedPath === String(root.takeMenuRow.path || "")
    }
  ]
  readonly property real takeMenuHeight: (root.takeMenuRenaming
    ? Style.space(52)
    : root.takeMenuRowHeight * root.takeMenuItems.length)
    + root.takeMenuPadding * 2
    + Border.top(root.takeMenuBorderSpec) + Border.bottom(root.takeMenuBorderSpec)

  // Rename shrinks the card; keep it anchored to the row's menu button instead
  // of leaving it where the taller action list was placed.
  onTakeMenuRenamingChanged: root.placeTakeMenu()

  // A take can be armed for a second press, but not for long.
  Timer {
    id: armTimer
    interval: 3500
    repeat: false
    onTriggered: {
      root.discardArmed = false
      root.trashArmedPath = ""
    }
  }

  // Keep the history's "just now / 5m ago" labels honest while the page is
  // visible. triggeredOnStart refreshes them the moment History opens.
  Timer {
    id: historyClock
    interval: 30000
    repeat: true
    triggeredOnStart: true
    running: root.opened && root.page === "history"
    onTriggered: root.historyTick++
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
      || key === "notifications" || key === "manualZoom" || key === "autoZoom"
      || key === "noiseCancellation"
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
    case "autoZoom": return ["off", "on"]
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
        || key === "notifications" || key === "manualZoom" || key === "autoZoom")
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
    case "autoZoom":
      return value === "on" ? "On" : "Off"
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

    if (root.bar && root.bar.shell && typeof root.bar.shell.updateEntryInline === "function")
      root.bar.shell.updateEntryInline(root.moduleName, entry)

    // Only send the key the user changed. Sending the whole settings object
    // let a stale or default value for an unrelated key overwrite the engine
    // config, which is how auto zoom kept turning itself off.
    var patch = ({})
    // Save-pass zoom has its own engine key. The retired live-zoom build used
    // `autoZoom`, and that stale value is still in old config files.
    patch[key === "autoZoom" ? "zoomOnClicks" : key] = value
    root.queueConfig(patch)
  }

  function queueConfig(patch) {
    var merged = ({})
    if (root.pendingConfig !== "") {
      try {
        merged = JSON.parse(root.pendingConfig)
      } catch (e) {
        merged = ({})
      }
    }
    for (var k in patch) if (k !== "id") merged[k] = patch[k]
    root.pendingConfig = JSON.stringify(merged)
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

  // ---------- labels ------------------------------------------------------

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

  // The header readout is both the manual refresh and the update cue: the
  // running version at rest, and the newer version plus an accent arrow when
  // one is waiting.
  function currentVersion() {
    var value = root.service && root.service.version ? String(root.service.version) : ""
    if (value === "") value = String(root.updateInfo.current || "")
    return value
  }

  function updateCueLabel() {
    return Model.updateCueLabel(root.updateInfo, root.currentVersion(), root.updateChecking)
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
    if (root.starting) return "󰜺"
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
      "--filename-prefix", root.valueString("filenamePrefix", ""),
      "--manual-zoom", root.currentValue("manualZoom"),
      "--auto-zoom", root.currentValue("autoZoom")
    ]
  }

  // Open the picker, then count down and record. The panel closes first so the
  // picker owns the screen; it stays closed while the take runs.
  function start() {
    if (root.finalizing) {
      root.setNotice("Saving the last take. Recording opens when it lands.")
      return
    }
    if (controlProc.running) {
      root.setNotice("Reel is still finishing the last command.")
      return
    }
    root.startImmediate()
  }

  // The IPC path goes through the same picker without a panel press.
  function startImmediate() {
    if (root.finalizing || controlProc.running) return
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
    if (root.renameInFlightPath !== "") return
    var command = Model.playCommand(row && row.path)
    if (command.length === 0) return
    Quickshell.execDetached(command)
    root.close()
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
    root.close()
  }

  function openFolder() {
    Quickshell.execDetached(["xdg-open", root.outputDir])
    root.close()
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

  function placeTakeMenu() {
    if (!root.takeMenuOpen || !root.takeMenuAnchor) return
    var surface = keyCatcher
    if (!surface) return
    var anchor = root.takeMenuAnchor
    var point = anchor.mapToItem(surface, 0, 0)
    var margin = Style.space(8)
    var gap = Style.space(4)
    var height = (root.takeMenuRenaming
      ? Style.space(52)
      : root.takeMenuRowHeight * root.takeMenuItems.length)
      + root.takeMenuPadding * 2
      + Border.top(root.takeMenuBorderSpec) + Border.bottom(root.takeMenuBorderSpec)
    var x = point.x + anchor.width - root.takeMenuWidth
    x = Math.max(margin, Math.min(x, surface.width - root.takeMenuWidth - margin))
    var y = point.y + anchor.height + gap
    if (y + height > surface.height - margin)
      y = point.y - height - gap
    y = Math.max(margin, Math.min(y, surface.height - height - margin))

    root.takeMenuX = x
    root.takeMenuY = y
  }

  function openTakeMenu(row, rowIndex, anchor) {
    if (!row || !anchor || root.renameInFlightPath !== "") return
    var surface = keyCatcher
    if (!surface) return
    root.cursorActive = true
    root.focusSection = "recents"
    root.selectedIndex = rowIndex

    root.takeMenuRow = row
    root.takeMenuAnchor = anchor
    root.takeMenuIndex = 0
    root.takeMenuRenaming = false
    root.renamePath = ""
    root.renameText = ""
    root.renameBusy = false
    root.takeMenuOpen = true
    root.placeTakeMenu()
  }

  function closeTakeMenu() {
    if (!root.takeMenuOpen) return
    root.takeMenuOpen = false
    root.takeMenuRow = null
    root.takeMenuAnchor = null
    root.takeMenuIndex = 0
    root.takeMenuRenaming = false
    root.renamePath = ""
    root.renameText = ""
    root.renameBusy = false
    if (root.opened) Qt.callLater(root.focusPanelKeys)
  }

  function beginRename(row) {
    if (!row || !row.path || root.renameInFlightPath !== "") return
    root.renamePath = String(row.path)
    root.renameText = Model.takeStem(row.path)
    root.renameBusy = false
    root.takeMenuRenaming = true
    Qt.callLater(function() {
      if (root.takeMenuRenaming) renameInput.forceActiveFocus()
    })
  }

  function cancelRename() {
    if (!root.takeMenuRenaming || root.renameBusy) return
    root.takeMenuRenaming = false
    root.renamePath = ""
    root.renameText = ""
    Qt.callLater(root.focusPanelKeys)
  }

  function commitRename() {
    if (root.renameBusy || root.renameInFlightPath !== "" || renameProc.running) return
    var path = root.renamePath
    var name = String(root.renameText || "").trim()
    if (path === "") return
    if (name === "") {
      root.setNotice("Enter a name for the take.", true)
      return
    }
    var renamedPath = Model.renamedPath(path, name)
    if (renamedPath !== path) root.addPendingRename(path, renamedPath)
    root.renameBusy = true
    root.renameInFlightPath = path
    renameProc.command = ["reel", "rename", path, name]
    root.closeTakeMenu()
    renameProc.running = true
  }

  function renameSelected() {
    var row = root.selectedTake()
    if (!row || root.renameInFlightPath !== "") return
    var item = takesRepeater.itemAt(root.selectedIndex - 1)
    if (item && typeof item.openRename === "function") item.openRename()
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
    } else if (key === "rename") {
      root.beginRename(row)
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

  function addPendingRename(path, renamedPath) {
    var next = {}
    for (var key in root.pendingRenames) next[key] = root.pendingRenames[key]
    next[String(path)] = String(renamedPath)
    root.pendingRenames = next
  }

  function dropPendingRename(path) {
    var next = {}
    for (var key in root.pendingRenames) {
      if (key !== String(path)) next[key] = root.pendingRenames[key]
    }
    root.pendingRenames = next
  }

  // Drop an optimistic name once the refreshed history carries the new path,
  // or once the old path is gone. Until then the row keeps showing it.
  function prunePendingRenames() {
    var next = {}
    var rows = root.recents || []
    for (var key in root.pendingRenames) {
      var newPath = root.pendingRenames[key]
      var hasOld = false
      var hasNew = false
      for (var i = 0; i < rows.length; i++) {
        var path = String(rows[i].path || "")
        if (path === key) hasOld = true
        if (path === newPath) hasNew = true
      }
      if (hasOld && !hasNew) next[key] = newPath
    }
    root.pendingRenames = next
  }

  function beginOptimisticTrash(row) {
    if (!row || !row.path || root.renameInFlightPath !== "") return
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
    if (!row || !row.path || root.renameInFlightPath !== "") return
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
    ? ["header", "primary", "recents"]
    : ["header", "primary", "settings"]

  function sectionCount(section) {
    switch (section) {
    case "header": return 1
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
    // Settings keep a second cursor index. Re-entering the section from another
    // one has to start at the top of the page, never at the row it was left on.
    if (root.focusSection === "settings") root.settingsIndex = 0
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

  function notePointerMotion(item, mouse) {
    if (!item || !mouse) return false
    var scene = item.mapToItem(null, mouse.x, mouse.y)
    var last = root.lastPointerScene
    if (Math.abs(scene.x - last.x) < 0.5 && Math.abs(scene.y - last.y) < 0.5) return false
    root.lastPointerScene = Qt.point(scene.x, scene.y)
    root.pointerActive = true
    return true
  }

  // Re-assert the row under the pointer on real motion: switching a tab or page
  // clears hover, and a pointer that never left its row gets no new
  // containsMouse change, so without this it would stay dark until it crossed a
  // row boundary.
  function pointerMoved(section, index, item, mouse) {
    if (!root.notePointerMotion(item, mouse)) return
    if (root.hoverSection === section && root.hoverIndex === index) return
    root.hoverSection = section
    root.hoverIndex = index
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
      // A row that scrolls out from under a hidden pointer has not been left by
      // the user, so it must not drop the keyboard cursor.
      if (root.pointerActive) root.cursorActive = false
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
      root.handleUpdate()
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
      var menuKey = raw.toLowerCase()
      if (menuKey === "a") root.closeTakeMenu()
      else if (menuKey === "n" && root.takeMenuRow) root.beginRename(root.takeMenuRow)
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
    if (key.length === 1 && key >= "1" && key <= "4") {
      var tab = Number(key) - 1
      if (tab >= 0 && tab < root.settingsTabs.length) {
        root.clearHover()
        if (root.page === "history") root.closeHistory()
        root.settingsTab = root.settingsTabs[tab].id
        root.settingsIndex = 0
        root.focusSection = "settings"
        root.cursorActive = true
        root.beginKeyboardScroll()
        Qt.callLater(root.resetScroll)
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
    else if (key === "n") root.renameSelected()
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
    if (!thumbProc.running) thumbProc.running = true
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
      if (root.settingsTab === "shortcuts") return
      var row = root.settingsRows()[root.settingsIndex]
      if (row && row.kind !== "info") root.cycleOption(row.key, dx)
    }
  }

  function activateSettings() {
    if (root.settingsTab === "shortcuts") return
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
    root.prunePendingRenames()
    root.settleHistorySelection()
  }

  onFinalizingChanged: {
    if (root.finalizing) {
      // Freeze the clock for the saving pill: the take is over, so its time
      // must stop rather than keep counting through the write-out.
      root.savedElapsed = root.elapsed
      root.setNotice("Saving the take…")
      root.savingHold = false
      root.savingWasFinalizing = true
      root.savingTitleCache = Model.savingTitle(root.savingTakePath)
      return
    }
    if (!root.savingWasFinalizing) return
    root.savingWasFinalizing = false
    // A failed save publishes no take, so there is nothing to hand off to.
    if (root.state.last && String(root.state.last.error || "") !== "") return
    // Hold the row until the history refreshed for this take has landed: the
    // saved path is in the list, or the list gained the take. The service bumps
    // recentVersion on every refresh, so the placeholder and the real row then
    // change within one frame instead of leaving a gap.
    root.savingHold = true
    root.savingHoldPath = Model.publishedPath(root.savingTakePath)
    root.savingHoldCount = root.visibleRecents.length
    root.savingHoldVersion = root.recentsVersion
    savingHoldTimer.restart()
  }

  onPendingTrashChanged: {
    root.settleHistorySelection()
  }

  onOpenedChanged: {
    if (opened) {
      root.pinPanelAnchor()
      root.panelUp = true
      root.clearHover()
      // A new window placement makes the last scene point meaningless.
      root.lastPointerScene = Qt.point(-100000, -100000)
      root.page = "home"
      root.focusSection = "primary"
      root.selectedIndex = 0
      root.cursorActive = true
      root.settingsIndex = 0
      root.settingsTab = "recording"
      root.carryingFile = false
      root.notice = ""
      root.discardArmed = false
      root.trashArmedPath = ""
      root.historyAnchorPath = ""
      root.suppressHistoryHover = false
      root.takeMenuOpen = false
      root.takeMenuRow = null
      root.takeMenuIndex = 0
      root.takeMenuRenaming = false
      root.renamePath = ""
      root.renameText = ""
      root.renameBusy = false
      root.refreshCameras()
      if (root.service) root.service.refresh()
      Qt.callLater(root.resetScroll)
    } else {
      root.panelUp = false
      root.clearHover()
      root.carryingFile = false
      root.discardArmed = false
      root.trashArmedPath = ""
      root.historyAnchorPath = ""
      root.suppressHistoryHover = false
      root.takeMenuOpen = false
      root.takeMenuRow = null
      root.takeMenuIndex = 0
      root.takeMenuRenaming = false
      root.renamePath = ""
      root.renameText = ""
      root.renameBusy = false
    }
  }

  implicitWidth: button.implicitWidth
  implicitHeight: button.implicitHeight

  // The take's clock: 0:00 while a take is being armed (the countdown is on
  // screen, so the bar does not echo it), the live time while frames run, and
  // the take's last time while it is saved.
  property int savedElapsed: 0

  function barClockText() {
    if (root.finalizing) return Model.clock(root.savedElapsed)
    if (!root.busy || root.starting) return Model.clock(0)
    return Model.clock(root.elapsed)
  }

  FontMetrics {
    id: barLabelMetrics
    font.family: root.fontFamily
    font.pixelSize: Style.bar.iconFont
  }

  // Centring a label on its line box keeps room for descenders, which lifts
  // the figures and makes the clock read a pixel high next to the take's dot.
  // Centre the digit height instead, the same way the bar's text buttons do.
  readonly property real barFigureOffset: (barLabelMetrics.descent
    - barLabelMetrics.ascent
    + barLabelMetrics.tightBoundingRect("0").height) / 2

  // A bar on the left or right edge is vertical, and the pill turns with it:
  // the same capsule laid along the bar instead of across it.
  readonly property bool barVertical: root.bar ? root.bar.vertical === true : false

  // The take's pill: the panel's capsule, carried into the bar, with the live
  // clock inside and the accent kept for its dot.
  readonly property real barPillHeight: Style.space(20)
  readonly property real barPillPad: Style.space(9)
  readonly property real barPillDot: Style.space(6)
  readonly property real barPillGap: Style.space(4)
  readonly property real barPillWidth: root.barPillPad * 2 + root.barPillDot
    + root.barPillGap + barLabelMetrics.advanceWidth(root.barClockText())

  // The room the bar gives this widget: the take's pill while frames run, the
  // resting mark's icon slot otherwise. The bar lays its neighbours out around
  // this width, so a take landing narrows the slot and moves everything beside
  // it. The card is not hung from this slot (see panelAnchor below): it must
  // not ride that reflow.
  readonly property real barSlotWidth: root.busy ? root.barPillWidth
    : Style.bar.iconSlot

  // Where the card hangs from: a point pinned to the widget's centre at the
  // moment the panel opens, parked in the bar's own surface so the bar's
  // layout cannot carry it away. The widget's slot is as wide as its drawing,
  // so a landing shrinks the slot by a third and slides everything in the
  // section sideways; a card anchored to the widget itself would be dragged
  // along in one frame. Pinned per open, so a card raised on the resting mark
  // stays under the mark, and one raised during a take stays under the pill.
  Item {
    id: panelAnchor
    parent: root.QsWindow.window ? root.QsWindow.window.contentItem : root
    width: root.barVertical ? 1 : Style.bar.iconSlot
    height: root.barVertical ? Style.bar.iconSlot : 1
    visible: false
  }

  // The card's own open flag, set after the pin so the first frame it draws
  // already hangs where it belongs.
  property bool panelUp: false

  function pinPanelAnchor() {
    var window = root.QsWindow.window
    if (!window || !window.contentItem) return
    panelAnchor.parent = window.contentItem
    var point = button.mapToItem(window.contentItem, 0, 0)
    panelAnchor.x = Math.round(point.x + button.width / 2
      - panelAnchor.width / 2)
    panelAnchor.y = Math.round(point.y + button.height / 2
      - panelAnchor.height / 2)
  }

  // The colour the bar itself writes in. A transparent bar has no surface of
  // its own: Omarchy samples the strip behind it (omarchy-bar-text-color) and
  // hands the widgets a foreground that contrasts with that strip. The pill
  // is built from that ink, never from the theme's raw foreground.
  readonly property color barInk: root.bar && root.bar.barForeground
    ? root.bar.barForeground : root.foreground
  readonly property real barInkLuminance: 0.2126 * root.barInk.r
    + 0.7152 * root.barInk.g + 0.0722 * root.barInk.b
  // Dark ink means a light strip, light ink means a dark one. Theme inks sit
  // at roughly 0.05–0.35 raw luminance for light strips and 0.6–0.95 for dark
  // ones, so the midpoint separates them cleanly.
  readonly property bool barOnLight: root.barInkLuminance < 0.5
  readonly property real barPillFillAlpha: root.barOnLight ? 0.10 : 0.05
  readonly property real barPillLineAlpha: root.barOnLight ? 0.30 : 0.14
  readonly property real barPillSavingFillAlpha: root.barOnLight ? 0.10 : 0.04
  readonly property real barPillSavingLineAlpha: root.barOnLight ? 0.30 : 0.12

  // The take's pill while a take is live, and the resting mark when nothing
  // runs. Both are drawings sized from the bar's own tokens, so the widget
  // follows the bar's thickness instead of assuming a fixed slot.
  WidgetButton {
    id: button
    anchors.fill: parent
    bar: root.bar
    text: ""
    hasVisualContent: true
    fixedWidth: root.barVertical ? -1 : root.barSlotWidth
    fixedHeight: root.barVertical ? root.barSlotWidth : -1
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

    // The resting mark: a small screen with a record dot, drawn rather than
    // borrowed from the icon font so it keeps its shape at every bar size and
    // on a vertical bar. It gives way to the take's pill while frames run, so
    // the bar still answers "is it recording?" without opening the panel.
    Item {
      id: idleMark
      visible: !root.busy
      anchors.centerIn: parent
      width: Math.min(Style.bar.iconCanvas, button.barSize - Style.space(4))
      height: width
      readonly property real unit: width / 24
      // The screen outline centres on its own box, which reads a pixel high
      // beside the bar's glyphs. Drop the mark to the bar's centre line where
      // the other icons sit.
      anchors.verticalCenterOffset: Style.space(1)
      scale: button.pressScale

      Rectangle {
        id: screenOutline
        x: 2.5 * parent.unit
        y: 4.5 * parent.unit
        width: 19 * parent.unit
        height: 15 * parent.unit
        radius: 4.5 * parent.unit
        color: "transparent"
        border.width: Math.max(1, Math.round(1.6 * parent.unit))
        border.color: root.barInk
        opacity: 0.9
      }

      Rectangle {
        anchors.centerIn: screenOutline
        width: 6.4 * parent.unit
        height: 6.4 * parent.unit
        radius: 3.2 * parent.unit
        color: root.barInk
      }
    }

    // The take's pill, for every state a live take can be in: armed at 0:00,
    // counting while frames run, frozen when paused, dimmed while the save
    // lands.
    Item {
      id: takePill
      visible: root.busy
      anchors.centerIn: parent
      width: root.barVertical ? root.barPillHeight : root.barPillWidth
      height: root.barVertical ? root.barPillWidth : root.barPillHeight

      property real savingPulse: 0

      // Saving is the same pill sinking into dim and breathing there until
      // the save is done. The floor stays high enough that the clock is still
      // readable — the dim is the message, not a fade out.
      opacity: root.finalizing
        ? (Style.reduceMotion ? 0.72 : 0.6 + 0.25 * savingPulse)
        : 1

      SequentialAnimation on savingPulse {
        running: root.finalizing && !Style.reduceMotion
        loops: Animation.Infinite
        NumberAnimation { to: 1; duration: 1000; easing.type: Easing.InOutQuad }
        NumberAnimation { to: 0; duration: 1000; easing.type: Easing.InOutQuad }
      }

      // The pill is drawn along its long axis and then turned with the bar, so
      // the dot leads from the top edge and the clock runs down the capsule.
      Item {
        id: pillBody
        anchors.centerIn: parent
        width: root.barPillWidth
        height: root.barPillHeight
        rotation: root.barVertical ? 90 : 0

        Rectangle {
          anchors.fill: parent
          radius: height / 2
          color: Util.alpha(root.barInk,
            root.finalizing ? root.barPillSavingFillAlpha : root.barPillFillAlpha)
          border.width: 1
          border.color: Util.alpha(root.barInk,
            root.finalizing ? root.barPillSavingLineAlpha : root.barPillLineAlpha)
        }

        Row {
          anchors.centerIn: parent
          spacing: root.barPillGap

          // The dot is bar ink at rest, exactly like the hour label; accent
          // belongs to frames running.
          Rectangle {
            id: takeDot
            width: root.barPillDot
            height: width
            radius: width / 2
            anchors.verticalCenter: parent.verticalCenter
            color: root.status === "recording" || root.capturing
              ? root.accent : root.barInk

            SequentialAnimation on opacity {
              running: (root.status === "recording" || root.capturing)
                && !Style.reduceMotion
              loops: Animation.Infinite
              NumberAnimation { to: 0.55; duration: 1000; easing.type: Easing.InOutQuad }
              NumberAnimation { to: 1.0; duration: 1000; easing.type: Easing.InOutQuad }
            }
          }

          Text {
            textFormat: Text.PlainText
            text: root.barClockText()
            color: root.barInk
            font.family: root.fontFamily
            font.pixelSize: Style.bar.iconFont
            font.bold: root.status === "recording" || root.capturing
            anchors.verticalCenter: parent.verticalCenter
            anchors.verticalCenterOffset: root.barFigureOffset
          }
        }
      }
    }
  }

  KeyboardPanel {
    id: keyboardPanel
    anchorItem: panelAnchor
    owner: root
    bar: root.bar
    open: root.panelUp
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
      blocked: root.typingPrefix || root.takeMenuRenaming

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
                    text: root.active || root.capturing ? Model.clock(root.elapsed)
                      : root.finalizing ? "Saving" : "Ready"
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
              spacing: Style.space(4)

              // The version is the update control. It stays a text slot, not a
              // badge: a hairline underline answers the pointer, and accent is
              // reserved for a waiting update or a failed check.
              Item {
                id: updateCue
                anchors.verticalCenter: parent.verticalCenter
                width: Math.max(Style.space(64), updateCueRow.width)
                height: Style.space(22)

                readonly property bool hot: (root.pointerActive && root.hoverSection === "header" && root.hoverIndex === 0)
                  || (root.cursorActive && root.focusSection === "header" && root.selectedIndex === 0
                    && !root.hoverPainted)
                readonly property bool failed: !root.updateChecking && !root.updateAvailable
                  && String(root.updateInfo.error || "") !== ""

                Row {
                  id: updateCueRow
                  anchors.centerIn: parent
                  spacing: Style.space(4)

                  Text {
                    anchors.verticalCenter: parent.verticalCenter
                    textFormat: Text.PlainText
                    text: root.updateCueLabel()
                    color: root.updateAvailable ? root.foreground
                      : updateCue.failed ? root.urgent
                      : root.updateChecking ? root.dim
                      : updateCue.hot ? root.foreground : root.secondary
                    font.family: root.fontFamily
                    font.pixelSize: Style.font.bodySmall
                    font.bold: root.updateAvailable
                    Behavior on color {
                      ColorAnimation { duration: 120 }
                    }
                  }

                  Text {
                    visible: root.updateAvailable && !root.updateChecking
                    anchors.verticalCenter: parent.verticalCenter
                    textFormat: Text.PlainText
                    text: "↑"
                    color: root.accent
                    font.family: root.fontFamily
                    font.pixelSize: Style.font.bodySmall
                    font.bold: true
                  }
                }

                // A quiet link: the underline appears on hover or the keyboard
                // cursor, and stays only for a failed check, where "Retry" is
                // the action. A waiting update is carried by the bold version
                // and the accent arrow, not by a line.
                Rectangle {
                  visible: !root.updateChecking
                  anchors.horizontalCenter: updateCueRow.horizontalCenter
                  anchors.bottom: parent.bottom
                  anchors.bottomMargin: 1
                  width: updateCueRow.width
                  height: 1
                  color: updateCue.hot ? root.accent
                    : updateCue.failed ? Util.alpha(root.urgent, 0.55)
                    : "transparent"
                  Behavior on color {
                    ColorAnimation { duration: 120 }
                  }
                }

                Item {
                  visible: root.updateChecking
                  anchors.horizontalCenter: updateCueRow.horizontalCenter
                  anchors.bottom: parent.bottom
                  anchors.bottomMargin: 1
                  width: updateCueRow.width
                  height: 1
                  clip: true

                  Rectangle {
                    anchors.fill: parent
                    color: root.hairline
                  }

                  Rectangle {
                    id: updateSweep
                    width: Math.max(2, Math.round(parent.width * 0.32))
                    height: parent.height
                    color: root.accent

                    SequentialAnimation on x {
                      running: root.updateChecking && !Style.reduceMotion
                      loops: Animation.Infinite
                      NumberAnimation {
                        from: 0
                        to: Math.max(0, updateSweep.parent.width - updateSweep.width)
                        duration: 850
                        easing.type: Easing.InOutQuad
                      }
                      NumberAnimation {
                        from: Math.max(0, updateSweep.parent.width - updateSweep.width)
                        to: 0
                        duration: 850
                        easing.type: Easing.InOutQuad
                      }
                    }
                  }
                }

                MouseArea {
                  id: updateCueMouse
                  anchors.fill: parent
                  hoverEnabled: true
                  cursorShape: root.updateChecking ? Qt.ArrowCursor : Qt.PointingHandCursor
                  enabled: !root.updateChecking
                  onContainsMouseChanged: root.setRowHover("header", 0, containsMouse)
                  onPositionChanged: function(mouse) { root.pointerMoved("header", 0, updateCue, mouse) }
                  onClicked: {
                    root.cursorActive = true
                    root.focusSection = "header"
                    root.selectedIndex = 0
                    root.handleUpdate()
                  }
                }

                PanelToolTip {
                  visible: updateCueMouse.containsMouse && root.pointerActive
                  text: root.updateTooltip()
                  fontFamily: root.fontFamily
                }
              }
            }
          }
        }

        // ---------- primary actions ----------

        Row {
          Layout.fillWidth: true
          spacing: Style.space(8)

          ActionButton {
            width: (parent.width - parent.spacing * 2) / 3
            rowIndex: 0
            label: root.primaryLabel()
            glyph: root.primaryGlyph()
            tone: root.active || root.starting ? "urgent" : "accent"
            primary: true
            // The last take owns the engine until its save lands: no second
            // take, no picker, no shortcut can slip in front of it. The button
            // stays legible and says why if pressed.
            subdued: root.finalizing
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

        // The strip is the panel's navigation: three settings tabs and History.
        // It stays fixed on both pages so History is reachable from the panel
        // itself, not only from a shortcut.
        Item {
          Layout.fillWidth: true
          Layout.preferredHeight: Style.space(30)
          Layout.topMargin: Style.space(8)
          Layout.bottomMargin: Style.space(2)

          Row {
            id: settingsTabRow
            anchors.fill: parent
            spacing: 0

            Repeater {
              model: root.navDestinations

              delegate: Item {
                id: settingsTabItem
                required property var modelData
                // The three settings tabs share the strip; History takes a wider
                // slot so its longer label and hint keep their air, and the
                // extra width itself separates it from the settings run.
                readonly property real historyShare: 0.24
                width: modelData.separated === true
                  ? settingsTabRow.width * historyShare
                  : settingsTabRow.width * (1 - historyShare) / (root.navDestinations.length - 1)
                height: settingsTabRow.height

                readonly property bool active: root.isNavActive(modelData.id)
                readonly property bool hot: root.pointerActive && settingsTabMouse.containsMouse

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
                    label: settingsTabItem.modelData.key
                    modifier: settingsTabItem.modelData.modifier || ""
                    horizontalPadding: settingsTabItem.modelData.separated === true
                      ? Style.space(15) : Style.space(10)
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
                  onPositionChanged: function(mouse) { root.notePointerMotion(settingsTabMouse, mouse) }
                  onClicked: root.selectNav(settingsTabItem.modelData.id)
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
                  model: root.settingsTab === "shortcuts" ? [] : root.settingsRows()

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
                  model: root.settingsTab === "shortcuts" ? root.shortcutRows : []

                  ShortcutRow {
                    required property var modelData
                    required property int index
                    width: parent.width
                    row: modelData
                    rowIndex: index
                  }
                }

              Column {
                visible: root.settingsTab === "recording"
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
                visible: root.settingsTab === "recording"
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
              visible: root.page === "history"
                && (root.visibleRecents.length > 0 || root.savingTake)
              spacing: Style.space(8)

              Item {
                width: parent.width
                implicitHeight: Math.max(takesHeader.implicitHeight, takesHint.implicitHeight)

                PanelSectionHeader {
                  id: takesHeader
                  text: root.savingTake && root.visibleRecents.length === 0
                    ? "Saving the take"
                    : root.visibleRecents.length === 0
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
                  text: root.savingTake && root.visibleRecents.length === 0
                    ? "it appears here when the save ends"
                    : root.visibleRecents.length > 0
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

              SavingRow {
                width: parent.width
                visible: root.savingTake
                title: root.savingTakeTitle
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
          && !root.savingTake
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
            visible: !root.takeMenuRenaming

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

          Column {
            anchors.fill: parent
            anchors.leftMargin: takeMenuCard.contentLeftInset
            anchors.rightMargin: takeMenuCard.contentRightInset
            anchors.topMargin: takeMenuCard.contentTopInset
            anchors.bottomMargin: takeMenuCard.contentBottomInset
            visible: root.takeMenuRenaming
            spacing: Style.space(4)

            Rectangle {
              width: parent.width
              height: root.takeMenuRowHeight
              radius: Style.cornerRadius
              color: root.restFill
              border.width: 1
              border.color: renameInput.activeFocus ? root.accent : root.hairline
              Behavior on border.color {
                ColorAnimation { duration: 120 }
              }

              TextInput {
                id: renameInput
                anchors.fill: parent
                anchors.leftMargin: Style.space(9)
                anchors.rightMargin: Style.space(9)
                verticalAlignment: TextInput.AlignVCenter
                text: root.renameText
                color: root.foreground
                selectionColor: root.accent
                selectedTextColor: root.inkOn(root.accent)
                font.family: root.fontFamily
                font.pixelSize: Style.font.body
                selectByMouse: true
                clip: true
                enabled: !root.renameBusy
                focus: root.takeMenuRenaming
                onTextEdited: root.renameText = text
                onActiveFocusChanged: if (activeFocus) selectAll()
                onAccepted: root.commitRename()
                Keys.onEscapePressed: function(event) {
                  root.cancelRename()
                  event.accepted = true
                }
              }
            }

            Text {
              textFormat: Text.PlainText
              text: root.renameBusy ? "Renaming…" : "Enter to rename · Esc to cancel"
              color: root.dim
              font.family: root.fontFamily
              font.pixelSize: Style.font.caption
              elide: Text.ElideRight
              width: parent.width
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
      // The compositor hides the pointer on a key press, so the hover under it
      // must stop painting now; the next pointer movement brings it back.
      root.pointerActive = false
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

  component ActionButton: CursorSurface {
    id: action
    property int rowIndex: 0
    property string label: ""
    property string glyph: ""
    property string tone: "neutral"
    property bool primary: false
    property string hint: ""
    // Blocked but not disabled: the button keeps its fill and label, and only
    // sits back. Used while the previous take is still being saved.
    property bool subdued: false

    readonly property color toneColor: tone === "urgent" ? root.urgent : tone === "accent" ? root.accent : root.foreground
    readonly property color toneInk: tone === "neutral" ? root.foreground : root.inkOn(action.toneColor)

    readonly property bool pointerHovered: root.pointerActive && root.hoverSection === "primary" && root.hoverIndex === rowIndex
    readonly property bool keyboardCursor: root.cursorActive
      && root.focusSection === "primary" && root.selectedIndex === rowIndex

    hasCursor: pointerHovered || (keyboardCursor && !root.hoverPainted)
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
    opacity: !enabled ? 0.35 : subdued ? 0.55 : 1.0
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
      onPositionChanged: function(mouse) { root.pointerMoved("primary", action.rowIndex, action, mouse) }
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
      onContainsMouseChanged: if (containsMouse && root.pointerActive) root.takeMenuIndex = menuItem.itemIndex
      onPositionChanged: function(mouse) { root.notePointerMotion(menuItem, mouse) }
      onClicked: {
        root.takeMenuIndex = menuItem.itemIndex
        root.activateTakeMenu()
      }
    }
  }

  // A take in the save pass. It reads like a history row so the save is
  // visible, but the row is a placeholder: no hover, no cursor, no menu, no
  // drag, and no key of its own.
  component SavingRow: Rectangle {
    id: saving
    property string title: ""

    implicitHeight: Style.space(56)
    radius: Style.cornerRadius
    color: "transparent"
    clip: true

    // One shared phase drives the three dots, so they pulse in sequence
    // without three separate animations drifting apart.
    property real phase: 0

    NumberAnimation on phase {
      running: saving.visible && !Style.reduceMotion
      from: 0
      to: 1
      duration: 1050
      loops: Animation.Infinite
    }

    Row {
      anchors.fill: parent
      anchors.leftMargin: Style.space(6)
      anchors.rightMargin: Style.space(6)
      spacing: Style.space(9)

      Rectangle {
        width: Style.space(84)
        height: Style.space(48)
        anchors.verticalCenter: parent.verticalCenter
        color: Util.alpha(root.foreground, 0.04)
        border.width: 1
        border.color: root.hairline

        Row {
          anchors.centerIn: parent
          spacing: Style.space(3)

          Repeater {
            model: 3

            Rectangle {
              required property int index
              width: Style.space(3)
              height: width
              radius: width / 2
              color: root.accent
              opacity: Style.reduceMotion ? 0.6
                : 0.22 + 0.78 * (1 - Math.abs(2 * (((saving.phase + index / 3) % 1)) - 1))
            }
          }
        }
      }

      Column {
        width: parent.width - Style.space(84) - parent.spacing
        anchors.verticalCenter: parent.verticalCenter
        spacing: Style.space(2)

        Text {
          width: parent.width
          textFormat: Text.PlainText
          text: saving.title !== "" ? saving.title : "Saving take"
          color: root.secondary
          font.family: root.fontFamily
          font.pixelSize: Style.font.body
          elide: Text.ElideMiddle
        }

        Text {
          width: parent.width
          textFormat: Text.PlainText
          text: "Saving · trimming and normalizing audio"
          color: root.dim
          font.family: root.fontFamily
          font.pixelSize: Style.font.caption
          elide: Text.ElideRight
        }
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

    function openRename() {
      if (!take.menuOpen) root.openTakeMenu(take.row, take.rowIndex, menuButton)
      root.beginRename(take.row)
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
      // The carry only exists to hand the take to another window, so the panel
      // must not stay on top of what it was dropped into. Close whenever the
      // platform ends the drag, including targets that never negotiate a drop
      // action and so report IgnoreAction for a drop that landed.
      Qt.callLater(root.close)
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

    readonly property bool pointerHovered: root.pointerActive && root.hoverSection === "recents" && root.hoverIndex === rowIndex
    readonly property bool keyboardCursor: root.cursorActive && root.page === "history"
      && root.focusSection === "recents" && root.selectedIndex === rowIndex

    hasCursor: pointerHovered || (keyboardCursor && !root.hoverPainted)
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
          id: previewImage
          anchors.fill: parent
          visible: take.preview !== ""
          source: take.preview !== "" ? Util.fileUrl(take.preview) : ""
          fillMode: Image.PreserveAspectCrop
          asynchronous: true
          sourceSize.width: Math.ceil(width * Screen.devicePixelRatio)
          sourceSize.height: Math.ceil(height * Screen.devicePixelRatio)
          // A fresh take has an uncached preview. Keep the placeholder under it
          // until the image is ready and fade it in, so the row never shows an
          // empty slot or pops.
          opacity: previewImage.status === Image.Ready ? 1 : 0
          Behavior on opacity {
            NumberAnimation { duration: 120 }
          }
        }

        Rectangle {
          anchors.fill: parent
          visible: take.preview === "" || previewImage.status !== Image.Ready
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
          text: Model.takeMeta(take.row, root.historyTick)
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
        root.pointerMoved("recents", take.rowIndex, take, mouse)
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

    readonly property bool pointerHovered: root.pointerActive && root.hoverSection === "recents" && root.hoverIndex === rowIndex
    readonly property bool keyboardCursor: root.cursorActive && root.page === "history"
      && root.focusSection === "recents" && root.selectedIndex === rowIndex

    hasCursor: pointerHovered || (keyboardCursor && !root.hoverPainted)
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
      onPositionChanged: function(mouse) { root.pointerMoved("recents", folder.rowIndex, folder, mouse) }
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
    // A modifier drawn before the label, subordinate to it: the shift glyph sits
    // lighter next to the letter instead of competing with it.
    property string modifier: ""
    property bool onAccent: false
    property color ink: root.foreground
    property bool active: false
    property real minimumWidth: 0
    // A two-character hint (⇧H) needs more air than a single digit.
    property real horizontalPadding: Style.space(10)

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
      width: Math.max(keyHint.minimumWidth, contentRow.implicitWidth + keyHint.horizontalPadding)
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

      Row {
        id: contentRow
        anchors.centerIn: parent
        spacing: Style.space(1)

        Text {
          visible: keyHint.modifier !== ""
          textFormat: Text.PlainText
          text: keyHint.modifier
          color: keyHint.onAccent ? keyHint.ink
            : keyHint.active ? root.foreground : Util.alpha(root.foreground, 0.62)
          font.family: root.fontFamily
          font.pixelSize: Style.font.caption
          anchors.verticalCenter: parent.verticalCenter
        }

        Text {
          id: labelText
          anchors.verticalCenter: parent.verticalCenter
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
  }

  component ShortcutRow: CursorSurface {
    id: shortcut
    property var row: null
    property int rowIndex: 0

    readonly property bool pointerHovered: root.pointerActive && root.hoverSection === "settings" && root.hoverIndex === rowIndex
    readonly property bool selected: root.cursorActive && root.page === "home"
      && root.focusSection === "settings" && root.settingsIndex === rowIndex

    hasCursor: pointerHovered || (selected && !root.hoverPainted)
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
      onPositionChanged: function(mouse) { root.pointerMoved("settings", shortcut.rowIndex, shortcut, mouse) }
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
    property string kind: "cycle"
    property string infoValue: ""

    readonly property bool pointerHovered: root.pointerActive && root.hoverSection === "settings" && root.hoverIndex === rowIndex
    readonly property bool selected: root.cursorActive
      && root.page === "home"
      && root.focusSection === "settings"
      && root.settingsIndex === rowIndex
    readonly property bool switchRow: setting.kind !== "info" && root.isSwitch(setting.settingKey)
    readonly property bool switchOn: root.currentValue(setting.settingKey) === "on"

    hasCursor: pointerHovered || (selected && !root.hoverPainted)
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
      onPositionChanged: function(mouse) { root.pointerMoved("settings", setting.rowIndex, setting, mouse) }
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

  // Previews live outside the cache now, but takes recorded before that change
  // may still have no thumbnail. Repair them in the background when history is
  // opened, then refresh the list so the images fill in.
  Process {
    id: thumbProc
    running: false
    command: ["reel", "thumbnails", "--limit", "20"]
    stdout: StdioCollector { waitForEnd: true }
    stderr: StdioCollector { waitForEnd: true }
    onExited: function(code) {
      if (code === 0 && root.service) root.service.refresh()
    }
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
    id: renameProc
    running: false
    stdout: StdioCollector { waitForEnd: true }
    stderr: StdioCollector { id: renameErr; waitForEnd: true }
    onExited: function(code) {
      var oldPath = root.renameInFlightPath
      root.renameInFlightPath = ""
      root.renameBusy = false
      if (code === 0) {
        root.setNotice("Take renamed.")
      } else {
        root.dropPendingRename(oldPath)
        var message = String(renameErr.text || "").trim()
        root.setNotice(message.replace(/^reel:\s*/, "") || "Could not rename that take.", true)
      }
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
    // A shell restart in the middle of a save must still catch the handoff to
    // the finished take, so arm the hold as if finalizing had just started.
    if (root.finalizing) {
      root.savingWasFinalizing = true
      root.savingTitleCache = Model.savingTitle(root.savingTakePath)
    }
  }
}
