import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import QtQuick.Shapes
import Quickshell
import Quickshell.Io
import qs.Commons
import qs.Commons as Commons
import qs.Ui
import "Bridge.js" as Bridge
import "Model.js" as Model

// Reel: a bar widget, recording history, and inline settings tabs. The recording
// itself belongs to bin/reel; everything here drives it and reads its state.
Panel {
  id: root
  moduleName: "io.github.tahadx.reel"
  ipcTarget: "io.github.tahadx.reel"
  manageIpc: false

  readonly property string home: Quickshell.env("HOME") || ""
  readonly property var directService: root.bar && root.bar.shell
    && typeof root.bar.shell.serviceFor === "function"
    ? root.bar.shell.serviceFor(root.moduleName) : null
  // A replacement bar gives this widget a service-less facade, so serviceFor()
  // returns null. The service publishes a live reader through Bridge.js; this
  // stable surface mirrors it so bindings and Connections keep working. The
  // stock bar keeps the direct object and never polls the bridge.
  QtObject {
    id: bridgeService

    property var state: Model.emptyState()
    property var recents: []
    property int elapsed: 0
    property int recentVersion: 0
    property string outputDir: ""
    property string version: ""
    property var updateInfo: Model.emptyUpdate()
    property bool updateChecking: false
    property string lastError: ""
    property bool configureFailed: false

    function refresh() { root.callBridge("refresh") }
    function checkForUpdates(force) { root.callBridge("checkForUpdates", [force === true]) }
    function retryConfigure() { root.callBridge("retryConfigure") }
  }
  readonly property var service: root.directService || bridgeService
  property bool bridgeHasPublisher: false

  function callBridge(name, args) {
    var api = Bridge.current()
    return api && typeof api.call === "function" ? api.call(name, args) : undefined
  }

  function syncBridge() {
    if (root.directService) return
    var api = Bridge.current()
    var next = api && typeof api.snapshot === "function" ? api.snapshot() : null
    if (!next) {
      if (!root.bridgeHasPublisher) return
      root.bridgeHasPublisher = false
      root.updateNoticePending = false
      bridgeService.state = Model.emptyState()
      bridgeService.recents = []
      bridgeService.elapsed = 0
      bridgeService.recentVersion = 0
      bridgeService.outputDir = ""
      bridgeService.version = ""
      bridgeService.updateInfo = Model.emptyUpdate()
      bridgeService.updateChecking = false
      bridgeService.lastError = ""
      bridgeService.configureFailed = false
      return
    }
    root.bridgeHasPublisher = true
    var value = Bridge.snapshotValue(next)
    if (value.state && bridgeService.state !== value.state) bridgeService.state = value.state
    if (bridgeService.recents !== value.recents) bridgeService.recents = value.recents
    if (bridgeService.elapsed !== value.elapsed) bridgeService.elapsed = value.elapsed
    if (bridgeService.recentVersion !== value.recentVersion)
      bridgeService.recentVersion = value.recentVersion
    if (bridgeService.outputDir !== value.outputDir) bridgeService.outputDir = value.outputDir
    if (bridgeService.version !== value.version) bridgeService.version = value.version
    if (value.updateInfo && bridgeService.updateInfo !== value.updateInfo)
      bridgeService.updateInfo = value.updateInfo
    if (bridgeService.updateChecking !== value.updateChecking)
      bridgeService.updateChecking = value.updateChecking
    if (bridgeService.lastError !== value.lastError) bridgeService.lastError = value.lastError
    if (bridgeService.configureFailed !== value.configureFailed)
      bridgeService.configureFailed = value.configureFailed
  }

  // Read immediately when the panel opens; otherwise the bar icon refreshes
  // half a second behind the service.
  Timer {
    interval: root.opened ? 150 : 500
    repeat: true
    running: !root.directService
    onTriggered: root.syncBridge()
  }

  readonly property var state: root.service && root.service.state
    ? root.service.state : Model.emptyState()
  readonly property var recents: root.service ? root.service.recents : []
  readonly property var visibleRecents: Model.visibleRecents(
    root.recents, root.pendingTrash, root.pendingRenames)
  readonly property int elapsed: root.service ? root.service.elapsed : 0
  readonly property string status: Model.status(root.state)
  readonly property bool active: Model.isActive(root.state)
  readonly property bool starting: root.status === "starting"
  readonly property string stage: String(root.state.stage || "")
  // The encoder is launched and warming up: the take runs, but the state only
  // becomes "recording" once its first frame exists.
  readonly property bool capturing: root.starting
    && (root.stage === "warmup" || root.stage === "capture")
  readonly property bool paused: root.status === "paused"
  // The CPU fallback records one continuous stream, so pause and resume are
  // unavailable for that take. Older state files have no flag and are GPU
  // takes.
  readonly property bool canPause: root.state.canPause !== false
  readonly property bool finalizing: root.status === "finalizing"
  // `reel start` supervises the encoder for the whole take when pointer
  // telemetry runs (the default), so a live start process is evidence of a
  // take even when the last state poll is late. The engine state still wins;
  // this keeps Stop and Pause usable instead of calling a recording take a
  // pending command.
  readonly property bool liveTake: root.active || root.starting
    || (!root.finalizing && controlProc.running && root.controlAction === "start")
  // Pause and discard act on a take that already has frames. While the take is
  // being armed the primary control is Cancel, so they stay off.
  readonly property bool controlsActive: root.active
    || (!root.starting && !root.finalizing
      && controlProc.running && root.controlAction === "start")
  // A take in the save pass already belongs to the user, so history lists one
  // placeholder row per live job before it lands. A row has no pointer, no
  // cursor, and no actions. The last row is held for the beat between the job
  // ending and the refreshed history carrying the take, so it swaps in place
  // instead of blinking out and dropping back in a moment later.
  property bool savingHold: false
  property bool savingWasFinalizing: false
  property int savingHoldVersion: 0
  property int savingHoldCount: 0
  property string savingHoldPath: ""
  property string savingTitleCache: ""

  readonly property var savingJobs: Model.saves(root.state)
  property var previousSavingJobs: []
  // Every save is its own row. `savingHold` remains the short bridge from the
  // old one-at-a-time flow, and `finalizing` covers the beat where the encoder
  // is still stopping before its job is published. A ListModel keeps the rows
  // stable across status ticks so their pulse does not restart every second.
  ListModel {
    id: savingRowsModel
  }
  readonly property bool savingTake: savingRowsModel.count > 0
  readonly property int savingCount: savingRowsModel.count
  readonly property string savingTakePath: String(root.state.file || "")
  readonly property int recentsVersion: root.service
    ? root.service.recentVersion : 0

  function syncSavingRows() {
    var labels = []
    for (var i = 0; i < root.savingJobs.length; i++) {
      var job = root.savingJobs[i]
      labels.push(Model.savingTitle(String(job.output || job.file || "")))
    }
    if (labels.length === 0 && root.savingHold)
      labels.push(root.savingTitleCache !== ""
        ? root.savingTitleCache : Model.savingTitle(root.savingHoldPath))
    if (labels.length === 0 && root.finalizing)
      labels.push(Model.savingTitle(root.savingTakePath))

    var same = savingRowsModel.count === labels.length
    if (same) {
      for (var j = 0; j < labels.length; j++) {
        if (String(savingRowsModel.get(j).label || "") !== labels[j]) {
          same = false
          break
        }
      }
    }
    if (same) return
    savingRowsModel.clear()
    for (var k = 0; k < labels.length; k++) savingRowsModel.append({ label: labels[k] })
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
    root.syncSavingRows()
  }

  onSavingHoldChanged: root.syncSavingRows()
  onSavingTitleCacheChanged: root.syncSavingRows()

  onSavingJobsChanged: {
    var previous = root.previousSavingJobs || []
    root.previousSavingJobs = root.savingJobs
    if (root.savingJobs.length > 0) {
      if (root.savingHold) root.releaseSavingHold()
    } else if (previous.length > 0) {
      // The last save landed. Hold its row until the refreshed history carries
      // the take, so the placeholder swaps instead of blinking out.
      var last = previous[previous.length - 1]
      var path = String(last.output || last.file || "")
      if (path !== "") {
        root.savingHoldPath = Model.publishedPath(path)
        if (!root.historyContains(root.savingHoldPath)) {
          root.savingTitleCache = Model.savingTitle(path)
          root.savingHoldCount = root.visibleRecents.length
          root.savingHoldVersion = root.recentsVersion
          root.savingHold = true
          savingHoldTimer.restart()
        }
      }
    }
    root.syncSavingRows()
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
  readonly property color cardFill: Util.alpha(root.foreground, 0.03)
  readonly property color cardBorder: Util.alpha(root.foreground, 0.07)
  readonly property color rowDivider: Util.alpha(root.foreground, 0.06)
  readonly property color cursorFill: Util.alpha(root.accent, 0.09)
  readonly property color selectedFill: Util.alpha(root.foreground, 0.14)
  readonly property color hairline: Util.alpha(root.foreground, 0.12)
  readonly property color strongBorder: Util.alpha(root.foreground, 0.24)
  readonly property color secondary: Util.alpha(root.foreground, 0.72)
  readonly property string fontFamily: root.bar ? root.bar.fontFamily : Style.font.family
  readonly property color onAccent: root.inkOn(root.accent)
  readonly property bool isRounded: Style.cornerRadius > 0
  readonly property int panelRadius: Style.cornerRadius

  function chipRadius(maxRadius) {
    return Model.scaledRadius(Style.cornerRadius, maxRadius === undefined ? Style.space(4) : maxRadius)
  }

  function badgeRadius(height) {
    return Model.pillRadius(Style.cornerRadius, height)
  }

  function settingSubtitle(key) {
    if (key === "desktopAudio")
      return root.audioDeviceLabel(root.valueString("desktopDevice", ""), "output")
    if (key === "microphoneAudio")
      return root.audioDeviceLabel(root.valueString("microphoneDevice", ""), "input")
    if (key === "webcam" && root.currentValue("webcam") !== "off")
      return Model.cameraLabel(root.cameras, root.valueString("webcamDevice", ""))
    return ""
  }

  function disabledSettingsMap() {
    return {
      "noiseCancellation": root.currentValue("microphoneAudio") !== "on"
    }
  }

  function settingDisabled(key) {
    return root.disabledSettingsMap()[key] === true
  }

  function settingDependencyHint(key) {
    return root.settingDisabled(key) ? "Mic off" : ""
  }

  readonly property var defaults: ({
    "desktopAudio": "on",
    "microphoneAudio": "off",
    "noiseCancellation": "off",
    "desktopDevice": "",
    "microphoneDevice": "",
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
    "clickEffects": "off",
    "outputDir": "",
    "webcamDevice": ""
  })

  property var audioDevices: []

  readonly property var desktopDeviceOptions: {
    var options = [""]
    var list = root.audioDevices || []
    for (var i = 0; i < list.length; i++) {
      var device = list[i] || ({})
      if (device.id === "default_output") continue
      var kind = String(device.kind || "")
      if (kind === "output" || kind === "unknown") options.push(String(device.id || ""))
    }
    return options
  }

  readonly property var microphoneDeviceOptions: {
    var options = [""]
    var list = root.audioDevices || []
    for (var i = 0; i < list.length; i++) {
      var device = list[i] || ({})
      if (device.id === "default_input") continue
      var kind = String(device.kind || "")
      if (kind === "input" || kind === "unknown") options.push(String(device.id || ""))
    }
    return options
  }

  function audioDeviceLabel(id, fallbackKind) {
    var value = String(id || "")
    if (value === "") return fallbackKind === "output" ? "Default output" : "Default input"
    var list = root.audioDevices || []
    for (var i = 0; i < list.length; i++) {
      var device = list[i] || ({})
      if (String(device.id || "") === value) {
        var label = String(device.label || value)
        return label !== "" ? label : value
      }
    }
    return value
  }

  function refreshAudioDevices() {
    if (audioDevicesProc.running) return
    audioDevicesProc.running = true
  }

  // Device choices live on their own page so Recording stays short. Each
  // audio group starts with its system-default row, then the connected
  // devices; a stored device that is no longer plugged in stays visible with
  // an explicit label instead of silently snapping to the default.
  function appendAudioDeviceRows(rows, key, kind, section, glyph, current) {
    var first = true
    var push = function(value, label) {
      rows.push({
        key: "device",
        settingKey: key,
        value: value,
        label: label,
        glyph: glyph,
        section: first ? section : undefined,
        selected: current === value
      })
      first = false
    }
    push("", kind === "output" ? "Default output" : "Default input")
    var list = root.audioDevices || []
    var known = current === ""
    for (var i = 0; i < list.length; i++) {
      var device = list[i] || ({})
      var id = String(device.id || "")
      if (id === "" || id === "default_output" || id === "default_input") continue
      var deviceKind = String(device.kind || "")
      if (deviceKind !== kind && deviceKind !== "unknown") continue
      if (id === current) known = true
      push(id, String(device.label || id))
    }
    if (!known) push(current, root.audioDeviceLabel(current, kind) + " (unplugged)")
  }

  function deviceRows() {
    var rows = []
    var desktop = root.valueString("desktopDevice", "")
    var mic = root.valueString("microphoneDevice", "")
    var camera = root.valueString("webcamDevice", "")
    root.appendAudioDeviceRows(rows, "desktopDevice", "output", "Desktop audio", "󰓃", desktop)
    root.appendAudioDeviceRows(rows, "microphoneDevice", "input", "Microphone", "󰍰", mic)

    rows.push({
      key: "device",
      settingKey: "webcamDevice",
      value: "",
      label: "First available camera",
      glyph: "󰄀",
      section: "Camera",
      selected: camera === ""
    })
    var known = camera === ""
    for (var i = 0; i < root.cameras.length; i++) {
      var cam = root.cameras[i] || ({})
      var id = String(cam.id || "")
      if (id === "") continue
      var path = String(cam.path || "")
      if (id === camera || (path !== "" && path === camera)) known = true
      rows.push({
        key: "device",
        settingKey: "webcamDevice",
        value: id,
        label: String(cam.label || id),
        glyph: "󰄀",
        selected: id === camera || (path !== "" && path === camera)
      })
    }
    if (!known) {
      rows.push({
        key: "device",
        settingKey: "webcamDevice",
        value: camera,
        label: Model.cameraLabel(root.cameras, camera),
        glyph: "󰄀",
        selected: true
      })
    }
    return rows
  }

  function chooseDevice(row) {
    if (!row) return
    var key = String(row.settingKey || "")
    if (key === "") return
    root.writeSetting(key, row.value)
  }

  function inputDevicesRowIndex() {
    var rows = root.settingsRows()
    for (var i = 0; i < rows.length; i++)
      if (rows[i].key === "inputDevices") return i
    return 0
  }

  readonly property var recordingSettingRows: [
    { key: "desktopAudio", label: "Desktop audio", glyph: "󰕾", section: "Audio" },
    { key: "microphoneAudio", label: "Microphone", glyph: "󰍬" },
    { key: "noiseCancellation", label: "Noise reduction", glyph: "󱑽", dependent: true },
    { key: "fps", label: "Frame rate", glyph: "󰈰", section: "Video" },
    { key: "quality", label: "Quality", glyph: "󰓅" },
    { key: "codec", label: "Video codec", glyph: "󰅴" },
    { key: "resolution", label: "Resolution cap", glyph: "󰍹" },
    { key: "container", label: "Container", glyph: "󰏗" },
    { key: "countdown", label: "Countdown", glyph: "󰔛", section: "Capture" },
    { key: "webcam", label: "Webcam corner", glyph: "󰄀" },
    { key: "inputDevices", label: "Input devices", glyph: "󰍰", section: "Devices", kind: "page" },
    { key: "notifications", label: "Saved notification", glyph: "󰂚", section: "Saving" }
  ]
  readonly property var shortcutRows: [
    { keys: "Shift+U", what: "update Reel" },
    { keys: "↑ ↓ / j k", what: "move between rows" },
    { keys: "← →", what: "move horizontally or step an option" },
    { keys: "h / l", what: "move horizontally or cycle an option" },
    { keys: "enter / space", what: "activate, toggle, or open" },
    { keys: "tab", what: "cycle pages (Shift+Tab reverses)" },
    { keys: "r", what: "start or stop a take" },
    { keys: "p", what: "pause or resume" },
    { keys: "d", what: "discard the current take" },
    { keys: "Ctrl+Scroll", what: "zoom during a take" },
    { keys: "s", what: "focus the settings rows" },
    { keys: "t", what: "cut the selected take in Omacut" },
    { keys: "a", what: "open the take actions menu" },
    { keys: "x / delete", what: "trash the selected take" },
    { keys: "u", what: "share the selected take" },
    { keys: "n", what: "rename the selected take" },
    { keys: "c", what: "copy the selected take's path" },
    { keys: "o", what: "open the recordings folder and close" },
    { keys: "q", what: "close the panel" },
    { keys: "esc", what: "go back, cancel countdown, or close" }
  ]
  readonly property var settingsTabs: [
    { id: "recording", label: "Recording", rows: root.recordingSettingRows },
    { id: "visuals", label: "Visuals", rows: [
      { key: "manualZoom", label: "Manual zoom", glyph: "󰍉", section: "Zoom" },
      { key: "autoZoom", label: "Auto zoom", glyph: "󰁨" },
      { key: "clickEffects", label: "Click effects", glyph: "󰝁" },
      { key: "cursor", label: "Record the pointer", glyph: "󰍽", section: "Pointer" }
    ] },
    { id: "shortcuts", label: "Shortcuts", rows: root.shortcutRows }
  ]
  // The strip puts Recording, Visuals, History, and Shortcuts as four equal
  // pages cycled left to right with Tab.
  readonly property var navDestinations: [
    { id: "recording", label: "Recording" },
    { id: "visuals", label: "Visuals" },
    { id: "history", label: "History" },
    { id: "shortcuts", label: "Shortcuts" }
  ]

  function isNavActive(id) {
    if (id === "history") return root.page === "history"
    if (root.page === "devices") return root.settingsTab === id
    return root.page === "home" && root.settingsTab === id
  }

  function selectNav(id) {
    root.typingPrefix = false
    root.cursorActive = false
    if (id === "history") {
      root.openHistory()
      return
    }
    root.clearHover()
    if (root.page !== "home") root.page = "home"
    root.settingsTab = id
    root.settingsIndex = 0
    root.focusSection = "settings"
    root.beginKeyboardScroll()
    Qt.callLater(root.resetScroll)
  }

  // ---------- panel state -------------------------------------------------

  property string page: "home"
  property int lastRecentsCount: 0
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
  // Explicit paste for the two short name fields. Qt's native paste reads the
  // whole clipboard synchronously inside the shell process; this asks the
  // bounded helper instead, caps the result, and refuses control characters.
  readonly property string pluginBin: root.home + "/.config/omarchy/plugins/io.github.tahadx.reel/bin"
  property var pasteTarget: null
  property var pastePending: null
  property int pasteSeq: 0
  property string pasteBuffer: ""
  property string pasteError: ""
  property string pasteErrorKind: ""
  readonly property int pasteLimit: 200

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

  // A paste is read by bin/clipboard, which caps stdout at 4096 bytes and has
  // a hard deadline. Nothing here hands an unbounded clipboard to Qt.
  //
  // Requests are serialized: a paste that is still running is killed, and its
  // exit starts the queued request. Every request carries a sequence number so
  // a late completion can never consume a newer request's target.
  function startPaste(kind, input, limit) {
    root.pasteSeq++
    var request = { kind: kind, input: input, limit: limit, seq: root.pasteSeq }
    root.pasteError = ""
    root.pasteErrorKind = ""
    root.pastePending = request
    if (pasteProc.running) {
      pasteProc.signal(9)
      return
    }
    root.launchPaste()
  }

  function launchPaste() {
    var request = root.pastePending
    if (!request) return
    root.pastePending = null
    root.pasteTarget = request
    root.pasteBuffer = ""
    pasteProc.command = [root.pluginBin + "/clipboard"]
    pasteProc.running = true
    pasteDeadline.restart()
  }

  function cancelPaste(kind) {
    if (root.pastePending && (kind === "" || root.pastePending.kind === kind)) {
      root.pastePending = null
      root.pasteSeq++
    }
    if (root.pasteTarget && (kind === "" || root.pasteTarget.kind === kind)) {
      root.pasteTarget = null
      root.pasteSeq++
      if (pasteProc.running) pasteProc.signal(9)
      pasteDeadline.stop()
    }
    if (kind === "" || root.pasteErrorKind === kind) root.clearPasteError(kind)
  }

  function clearPasteError(kind) {
    if (kind === "" || root.pasteErrorKind === kind) {
      root.pasteError = ""
      root.pasteErrorKind = ""
    }
  }

  function finishPaste(code) {
    pasteDeadline.stop()
    var target = root.pasteTarget
    root.pasteTarget = null
    // A newer request queued while this one was shutting down: drop the old
    // completion and start the queued paste.
    if (root.pastePending) {
      root.launchPaste()
      return
    }
    if (!target || target.seq !== root.pasteSeq) return
    if (code !== 0) {
      root.pasteErrorKind = target.kind
      root.pasteError = code === 141
        ? "Paste is longer than this field allows."
        : (code === 124 || code === 137)
          ? "The clipboard did not answer in time."
          : "Could not read the clipboard."
      return
    }
    var result = Model.pasteText(root.pasteBuffer, target.limit)
    if (!result.ok) {
      root.pasteErrorKind = target.kind
      root.pasteError = result.error
      return
    }
    if (result.text === "" || !target.input) return
    var input = target.input
    var start = input.selectionStart
    var end = input.selectionEnd
    if (start < 0 || end < 0) {
      start = input.cursorPosition
      end = start
    }
    if (start > end) {
      var swap = start
      start = end
      end = swap
    }
    if (start !== end) input.remove(start, end)
    input.insert(start, result.text)
    root.clearPasteError(target.kind)
  }

  Process {
    id: pasteProc
    running: false
    stdout: StdioCollector {
      waitForEnd: true
      onStreamFinished: root.pasteBuffer = String(text || "")
    }
    onExited: function(code) { root.finishPaste(code) }
  }

  Timer {
    id: pasteDeadline
    interval: 1200
    repeat: false
    onTriggered: {
      var kind = root.pasteTarget ? root.pasteTarget.kind : ""
      root.pasteSeq++
      root.pastePending = null
      root.pasteTarget = null
      if (pasteProc.running) pasteProc.signal(9)
      if (kind !== "") {
        root.pasteErrorKind = kind
        root.pasteError = "The clipboard did not answer in time."
      }
    }
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
    case "desktopDevice": return root.desktopDeviceOptions
    case "microphoneDevice": return root.microphoneDeviceOptions
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
    case "clickEffects": return ["off", "ripple", "halo"]
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
    if ((key === "desktopAudio" || key === "microphoneAudio"
        || key === "noiseCancellation" || key === "cursor"
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
    case "desktopDevice":
      return root.audioDeviceLabel(value, "output")
    case "microphoneDevice":
      return root.audioDeviceLabel(value, "input")
    case "webcam":
      return value === "off" ? "Off" : value.charAt(0).toUpperCase() + value.slice(1)
    case "clickEffects":
      return value === "off" ? "Off" : value.charAt(0).toUpperCase() + value.slice(1)
    case "webcamDevice":
      return Model.cameraLabel(root.cameras, value)
    case "inputDevices":
      return "Manage"
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
    if (root.finalizing) text = "Reel · finishing the take"
    else if (root.paused) text = "Reel · paused at " + Model.clock(root.elapsed) + " · right-click to resume"
    else if (root.capturing) text = "Reel · recording " + Model.clock(root.elapsed)
    else if (root.starting && root.stage !== "select") text = "Reel · starting the take"
    else if (root.starting) text = "Reel · waiting for a selection"
    else if (root.active && !root.canPause) text = "Reel · recording " + Model.clock(root.elapsed) + " · CPU fallback, no pause"
    else if (root.active) text = "Reel · recording " + Model.clock(root.elapsed) + " · right-click to pause"
    else if (root.savingJobs.length > 0) text = "Reel · saving " + root.savingJobs.length
      + (root.savingJobs.length === 1 ? " take" : " takes") + " · right-click to record"
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
    var current = root.currentVersion()
    return current !== "" ? "Check for updates · v" + current : "Check for updates"
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
    if (root.liveTake) return "Stop"
    return "Record"
  }

  function primaryGlyph() {
    if (root.starting) return "󰜺"
    if (root.liveTake) return "󰓛"
    return "󰻂"
  }

  // ---------- actions -----------------------------------------------------

  function startArgs() {
    return [
      "reel", "start",
      "--desktop-audio", root.currentValue("desktopAudio"),
      "--microphone-audio", root.currentValue("microphoneAudio"),
      "--noise-reduction", root.currentValue("noiseCancellation"),
      "--desktop-device", root.valueString("desktopDevice", ""),
      "--microphone-device", root.valueString("microphoneDevice", ""),
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
    // The primary action should already have routed a live take to stop; keep
    // this honest for IPC and keyboard callers.
    if (root.liveTake) {
      root.stop()
      return
    }
    if (root.finalizing) {
      root.setNotice("Reel is still saving the last take. Try again in a moment.", true)
      return
    }
    if (controlProc.running) {
      // A start process with no live take is between stop and exit, not a
      // pending take. Asking again would be lost while that process finishes.
      root.setNotice("Reel is still finishing the last take. Try again in a moment.", true)
      return
    }
    root.startImmediate()
  }

  // The IPC path goes through the same picker without a panel press. The
  // engine still owns the real busy check and queues a press while the encoder
  // is shutting down; this only avoids starting a second local supervisor.
  function startImmediate() {
    if (root.liveTake || root.finalizing) return
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
    if (!root.controlsActive) return
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
    if (!root.controlsActive || root.starting) return
    if (!root.canPause) {
      root.setNotice("This take is using the CPU fallback, which cannot pause.", true)
      return
    }
    // A pause/resume already in flight is the current answer. Ignore a second
    // press instead of claiming a recording take is finishing a command.
    if (pauseProc.running) return
    root.cancelling = false
    pauseProc.command = ["reel", "toggle-pause"]
    pauseProc.running = true
    pauseDeadline.restart()
  }

  function toggleRecording() {
    if (root.liveTake) root.stop()
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
    if (!root.cursorActive || root.page !== "history" || root.focusSection !== "recents") return null
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
      if (root.liveTake) root.stop()
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
    if (!root.cursorActive) return
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
    if (root.page === "devices") {
      root.closeDevices()
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

  function openDevices() {
    root.clearHover()
    root.settingsTab = "recording"
    root.page = "devices"
    root.focusSection = "settings"
    root.settingsIndex = 0
    root.cursorActive = false
    root.typingPrefix = false
    root.refreshAudioDevices()
    root.refreshCameras()
    root.resetScroll()
    Qt.callLater(root.focusPanelKeys)
  }

  function closeDevices() {
    root.clearHover()
    root.page = "home"
    root.settingsTab = "recording"
    root.focusSection = "settings"
    root.settingsIndex = root.inputDevicesRowIndex()
    root.cursorActive = false
    root.beginKeyboardScroll()
    root.resetScroll()
    Qt.callLater(root.focusPanelKeys)
  }

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
    root.selectedIndex = root.visibleRecents.length > 0 ? 1 : 0
    root.cursorActive = false
    root.historyAnchorPath = ""
    root.suppressHistoryHover = false
    Qt.callLater(root.focusPanelKeys)
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
    if (root.page !== "home") root.page = "home"
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
    root.page = "home"
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
    if (root.page === "devices") root.closeDevices()
    else if (root.focusSection === "settings") root.closeSettings()
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

  function activeRows() {
    if (root.page === "devices") return root.deviceRows()
    if (root.page === "home" && root.settingsTab === "shortcuts") return root.shortcutRows
    return root.settingsRows()
  }

  function switchSettingsTab(delta) {
    root.clearHover()
    root.beginKeyboardScroll()
    root.page = "home"
    var index = 0
    for (var i = 0; i < root.settingsTabs.length; i++)
      if (root.settingsTabs[i].id === root.settingsTab) index = i
    index = (index + delta + root.settingsTabs.length) % root.settingsTabs.length
    root.settingsTab = root.settingsTabs[index].id
    root.settingsIndex = 0
    root.cursorActive = true
  }

  function cycleNav(delta) {
    var ids = []
    for (var i = 0; i < root.navDestinations.length; i++)
      ids.push(root.navDestinations[i].id)
    if (ids.length === 0) return
    var currentId = root.page === "history" ? "history" : root.settingsTab
    var index = ids.indexOf(currentId)
    if (index < 0) index = 0
    root.selectNav(ids[(index + delta + ids.length) % ids.length])
  }

  function settingsCount() {
    return root.activeRows().length
  }

  function moveSettings(dy, dx) {
    if (!root.cursorActive) {
      root.cursorActive = true
      return
    }
    var onShortcuts = root.page === "home" && root.settingsTab === "shortcuts"
    if (dy !== 0) {
      var rows = root.activeRows()
      var disabled = (root.page === "devices" || onShortcuts)
        ? ({}) : root.disabledSettingsMap()
      var next = Model.nextEnabledIndex(rows, root.settingsIndex, dy, disabled)
      if (next < 0) {
        if (dy < 0 && root.page !== "devices" && !onShortcuts) {
          root.focusSection = "primary"
          root.selectedIndex = 0
        }
        return
      }
      root.settingsIndex = next
      return
    }
    if (dx !== 0) {
      if (root.page === "devices" || onShortcuts) return
      var row = root.settingsRows()[root.settingsIndex]
      if (row && !root.settingDisabled(row.key) && row.kind !== "info" && row.kind !== "page")
        root.cycleOption(row.key, dx)
    }
  }

  function footerHints() {
    var recordLabel = root.starting ? "Cancel" : (root.liveTake ? "Stop" : "Record")
    if (root.page === "home" && root.settingsTab === "shortcuts") {
      return [
        { key: "↑/↓", label: "Move" },
        { key: "r", label: recordLabel },
        { key: "Tab", label: "Pages" },
        { key: "Esc", label: "Hide" }
      ]
    }
    if (root.page === "devices") {
      if (!root.cursorActive) {
        return [
          { key: "↑/↓", label: "Move" },
          { key: "Esc", label: "Back" }
        ]
      }
      return [
        { key: "↑/↓", label: "Move" },
        { key: "Enter", label: "Select" },
        { key: "Esc", label: "Back" }
      ]
    }
    if (root.page === "history") {
      if (root.visibleRecents.length === 0 && !root.savingTake) {
        return [
          { key: "r", label: recordLabel },
          { key: "o", label: "Folder" },
          { key: "Tab", label: "Pages" },
          { key: "Esc", label: "Hide" }
        ]
      }
      if (!root.cursorActive) {
        return [
          { key: "↑/↓", label: "Move" },
          { key: "r", label: recordLabel },
          { key: "o", label: "Folder" },
          { key: "Tab", label: "Pages" },
          { key: "Esc", label: "Hide" }
        ]
      }
      if (root.focusSection === "recents" && root.selectedIndex === 0) {
        return [
          { key: "↑/↓", label: "Move" },
          { key: "Enter", label: "Open" },
          { key: "o", label: "Folder" },
          { key: "Tab", label: "Pages" },
          { key: "Esc", label: "Hide" }
        ]
      }
      return [
        { key: "↑/↓", label: "Move" },
        { key: "Enter", label: "Open" },
        { key: "a", label: "Actions" },
        { key: "x", label: "Trash" },
        { key: "o", label: "Folder" },
        { key: "Esc", label: "Hide" }
      ]
    }
    if (!root.cursorActive) {
      return [
        { key: "↑/↓", label: "Move" },
        { key: "r", label: recordLabel },
        { key: "Tab", label: "Pages" },
        { key: "Esc", label: "Hide" }
      ]
    }
    var actionKey = "Space"
    var actionLabel = "Toggle"
    if (root.focusSection === "settings") {
      var activeRow = root.settingsRows()[root.settingsIndex]
      if (activeRow && !root.settingDisabled(activeRow.key)) {
        if (activeRow.kind === "page") {
          actionKey = "Enter"
          actionLabel = "Open"
        } else if (!root.isSwitch(activeRow.key) && activeRow.kind !== "info") {
          actionKey = "←/→"
          actionLabel = "Step"
        }
      }
    } else if (root.focusSection === "primary") {
      actionKey = "Enter"
      actionLabel = "Activate"
    }
    return [
      { key: "↑/↓", label: "Move" },
      { key: actionKey, label: actionLabel },
      { key: "r", label: recordLabel },
      { key: "Tab", label: "Pages" },
      { key: "Esc", label: "Hide" }
    ]
  }

  function activateSettings() {
    if (root.page === "devices") {
      root.chooseDevice(root.deviceRows()[root.settingsIndex])
      return
    }
    if (root.page === "home" && root.settingsTab === "shortcuts") return
    var row = root.settingsRows()[root.settingsIndex]
    if (!row || root.settingDisabled(row.key)) return
    if (row.kind === "page") {
      root.openDevices()
      return
    }
    if (row.kind !== "info") root.cycleOption(row.key, 1)
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
    var prevCount = root.lastRecentsCount
    root.lastRecentsCount = root.visibleRecents.length
    root.prunePendingTrash()
    root.prunePendingRenames()
    root.settleHistorySelection()
    if (prevCount === 0 && root.visibleRecents.length > 0
        && root.opened && root.page === "history"
        && root.focusSection === "recents" && root.selectedIndex === 0) {
      root.selectedIndex = 1
    }
  }

  onFinalizingChanged: {
    if (root.finalizing) {
      // Freeze the clock for the closing pill: the take is over, so its time
      // must stop rather than keep counting while the encoder shuts down.
      root.savedElapsed = root.elapsed
      root.savingHold = false
      root.savingWasFinalizing = true
      root.savingTitleCache = Model.savingTitle(root.savingTakePath)
      root.syncSavingRows()
      return
    }
    if (!root.savingWasFinalizing) {
      root.syncSavingRows()
      return
    }
    root.savingWasFinalizing = false
    // Save jobs own their rows now; the old hold only bridges a state that has
    // no job record.
    if (root.savingJobs.length > 0 || root.savingTakePath === "") {
      root.syncSavingRows()
      return
    }
    // A failed save publishes no take, so there is nothing to hand off to.
    if (root.state.last && String(root.state.last.error || "") !== "") {
      root.syncSavingRows()
      return
    }
    // Hold the row until the history refreshed for this take has landed: the
    // saved path is in the list, or the list gained the take. The service bumps
    // recentVersion on every refresh, so the placeholder and the real row then
    // change within one frame instead of leaving a gap.
    root.savingHoldPath = Model.publishedPath(root.savingTakePath)
    root.savingHoldCount = root.visibleRecents.length
    root.savingHoldVersion = root.recentsVersion
    root.savingHold = true
    savingHoldTimer.restart()
    root.syncSavingRows()
  }

  onPendingTrashChanged: {
    root.settleHistorySelection()
  }

  onOpenedChanged: {
    if (opened) {
      root.pinPanelAnchor()
      root.syncBridge()
      root.panelUp = true
      root.clearHover()
      // A new window placement makes the last scene point meaningless.
      root.lastPointerScene = Qt.point(-100000, -100000)
      root.page = "home"
      root.focusSection = "settings"
      root.selectedIndex = 0
      root.cursorActive = false
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
      root.refreshAudioDevices()
      root.refreshCameras()
      if (root.service) {
        if (root.service.configureFailed) root.service.retryConfigure()
        root.service.refresh()
      }
      Qt.callLater(root.resetScroll)
    } else {
      root.panelUp = false
      root.clearHover()
      root.cancelPaste("")
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

  // A bar on the left or right edge is vertical, and the live timer turns with
  // it: the same dot + clock laid along the bar instead of across it.
  readonly property bool barVertical: root.bar ? root.bar.vertical === true : false

  // Bare bar timer while a take runs: pulsing dot + elapsed clock in bar ink,
  // with no outer pill border or fill.
  readonly property real barTakeHeight: Style.space(20)
  readonly property real barTakePad: Style.space(6)
  readonly property real barTakeDot: Style.space(6)
  readonly property real barTakeGap: Style.space(4)
  readonly property real barTakeWidth: root.barTakePad * 2 + root.barTakeDot
    + root.barTakeGap + barLabelMetrics.advanceWidth(root.barClockText())

  // The room the bar gives this widget: the take's clock while frames run, the
  // resting mark's icon slot otherwise. The bar lays its neighbours out around
  // this width, so a take landing widens the slot and moves everything beside
  // it. The card is not hung from this slot (see panelAnchor below): it must
  // not ride that reflow.
  readonly property real barSlotWidth: root.busy ? root.barTakeWidth
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
  // hands the widgets a foreground that contrasts with that strip. Both the
  // resting mark and live timer use that ink, never the theme's raw
  // foreground.
  readonly property color barInk: root.bar && root.bar.barForeground
    ? root.bar.barForeground : root.foreground

  // The take's bare dot + clock while a take is live, and the resting mark
  // when nothing runs. Both are sized from the bar's own tokens, so the widget
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
        if (root.controlsActive) root.togglePause()
        else if (!root.starting) root.start()
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
      // The drawn screen is flat-topped while icon-font neighbours carry
      // ascender ink (the agents mark's antenna, the chevron's tip), so an
      // ink-centred box reads a couple of pixels high. Drop it one point to
      // line the mark's body up with the glyph mass beside it.
      anchors.verticalCenterOffset: 1
      width: Math.min(Style.bar.iconCanvas, button.barSize - Style.space(4))
      height: width
      scale: button.pressScale

      // The outline and the dot are two paths of one Shape, so they share one
      // coordinate system and land on the same device pixels. Vector paths stay
      // sharp at every bar size, display scale, and press animation; a Canvas
      // rasterises the mark at bar size and the texture softens once the
      // compositor scales it, which is what made it look low quality.
      Shape {
        id: idleShape
        width: 24
        height: 24
        preferredRendererType: Shape.CurveRenderer
        antialiasing: true

        transform: Scale {
          xScale: idleMark.width / idleShape.width
          yScale: idleMark.height / idleShape.height
        }

        // The 19 × 15 screen with its 1.6-unit stroke kept inside the box: the
        // centreline is inset by half the stroke, and the 3.7 corner radius
        // leaves the 4.5 outer radius.
        ShapePath {
          strokeColor: Util.alpha(root.barInk, 0.9)
          strokeWidth: 1.6
          fillColor: "transparent"
          joinStyle: ShapePath.RoundJoin
          PathSvg {
            path: "M 7 5.3 H 17 A 3.7 3.7 0 0 1 20.7 9 V 15 A 3.7 3.7 0 0 1 17 18.7 H 7 A 3.7 3.7 0 0 1 3.3 15 V 9 A 3.7 3.7 0 0 1 7 5.3 Z"
          }
        }

        // The 6.4 record dot on the same centre point.
        ShapePath {
          fillColor: root.barInk
          strokeWidth: -1
          PathSvg {
            path: "M 12 8.8 A 3.2 3.2 0 1 0 12 15.2 A 3.2 3.2 0 1 0 12 8.8 Z"
          }
        }
      }
    }

    // Bare timer for every state a live take can be in: armed at 0:00, counting
    // while frames run, frozen when paused, steady while the encoder finishes
    // and the save job takes over.
    Item {
      id: takeClock
      visible: root.busy
      anchors.centerIn: parent
      width: root.barVertical ? root.barTakeHeight : root.barTakeWidth
      height: root.barVertical ? root.barTakeWidth : root.barTakeHeight
      scale: button.pressScale

      // Laid along the bar's long axis and rotated on vertical bars so the dot
      // leads from the top edge and the clock runs cleanly along the strip.
      Item {
        id: takeClockBody
        anchors.centerIn: parent
        width: root.barTakeWidth
        height: root.barTakeHeight
        rotation: root.barVertical ? 90 : 0

        Row {
          anchors.centerIn: parent
          spacing: root.barTakeGap

          // The dot is bar ink at rest, exactly like the hour label; accent
          // belongs to frames running.
          Rectangle {
            id: takeDot
            width: root.barTakeDot
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
    contentHeight: Style.space(436)

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
        if (!root.takeMenuOpen) root.cycleNav(direction)
      }
      onTextKey: function(text) { root.onTextKey(text) }
      ColumnLayout {
        anchors.fill: parent
        spacing: Style.space(10)

        // ---------- hero ----------

        PanelHero {
          Layout.fillWidth: true
          iconComponent: Component {
            Item {
              id: statusPillRoot
              width: Math.max(Style.space(64),
                statusRow.implicitWidth + Style.space(16))
              height: Style.space(22)

              readonly property bool recordingLive: (root.status === "recording" || root.capturing) && !root.paused
              readonly property bool idleReady: !root.active && !root.capturing

              Rectangle {
                anchors.fill: parent
                radius: root.badgeRadius(height)
                color: statusPillRoot.recordingLive ? Util.alpha(root.urgent, 0.16)
                  : statusPillRoot.idleReady ? Util.alpha(root.accent, 0.13)
                  : root.restFill
                border.width: 1
                border.color: statusPillRoot.recordingLive ? Util.alpha(root.urgent, 0.40)
                  : statusPillRoot.idleReady ? Util.alpha(root.accent, 0.30)
                  : root.hairline

                Row {
                  id: statusRow
                  anchors.centerIn: parent
                  spacing: Style.space(5)

                  Rectangle {
                    width: Style.space(6)
                    height: width
                    radius: width / 2
                    color: statusPillRoot.recordingLive ? root.urgent
                      : root.paused ? root.secondary
                      : root.accent
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
                    color: root.paused ? root.secondary : root.foreground
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

              // Quiet manual update trigger at rest ("󰑐 Update") and calm
              // neutral badge ("↑ Update") when an update is waiting so it
              // never competes with the status pill or primary CTA.
              Item {
                id: updateCue
                anchors.verticalCenter: parent.verticalCenter
                width: Math.max(Style.space(64), updateCueRow.implicitWidth + Style.space(14))
                height: Style.space(22)

                readonly property bool hot: (root.pointerActive && root.hoverSection === "header" && root.hoverIndex === 0)
                  || (root.cursorActive && root.focusSection === "header" && root.selectedIndex === 0
                    && !root.hoverPainted)
                readonly property bool failed: !root.updateChecking && !root.updateAvailable
                  && String(root.updateInfo.error || "") !== ""

                Rectangle {
                  id: updateBadge
                  anchors.fill: parent
                  radius: root.badgeRadius(height)
                  clip: true
                  color: updateCue.failed ? (updateCue.hot ? Util.alpha(root.urgent, 0.22) : root.urgentSoft)
                    : updateCue.hot ? Style.hoverFillFor(root.foreground, root.accent)
                    : root.updateAvailable && !root.updateChecking ? root.cardFill
                    : "transparent"
                  border.width: 1
                  border.color: updateCue.failed
                    ? (updateCue.hot ? root.urgent : Util.alpha(root.urgent, 0.45))
                    : updateCue.hot ? root.accent
                    : root.updateAvailable && !root.updateChecking ? root.cardBorder
                    : Util.alpha(root.foreground, 0.06)

                  Behavior on color {
                    ColorAnimation { duration: Style.duration(120) }
                  }
                  Behavior on border.color {
                    ColorAnimation { duration: Style.duration(120) }
                  }

                  Row {
                    id: updateCueRow
                    anchors.centerIn: parent
                    spacing: Style.space(5)

                    Text {
                      visible: !root.updateChecking
                      anchors.verticalCenter: parent.verticalCenter
                      textFormat: Text.PlainText
                      text: updateCue.failed ? "󰑐"
                        : root.updateAvailable ? "↑"
                        : "󰑐"
                      color: updateCue.failed ? root.urgent
                        : root.updateAvailable ? root.accent
                        : (updateCue.hot ? root.foreground : Util.alpha(root.foreground, 0.34))
                      font.family: root.fontFamily
                      font.pixelSize: Style.font.caption
                      font.bold: root.updateAvailable || updateCue.failed
                      Behavior on color {
                        ColorAnimation { duration: Style.duration(120) }
                      }
                    }

                    Text {
                      anchors.verticalCenter: parent.verticalCenter
                      textFormat: Text.PlainText
                      text: root.updateCueLabel()
                      color: updateCue.failed ? root.urgent
                        : updateCue.hot ? root.foreground
                        : root.updateChecking ? root.secondary
                        : root.updateAvailable ? root.secondary
                        : Util.alpha(root.foreground, 0.36)
                      font.family: root.fontFamily
                      font.pixelSize: Style.font.caption
                      font.bold: updateCue.failed || (root.updateAvailable && updateCue.hot)
                      Behavior on color {
                        ColorAnimation { duration: Style.duration(120) }
                      }
                    }
                  }

                  Item {
                    visible: root.updateChecking
                    anchors.left: parent.left
                    anchors.right: parent.right
                    anchors.bottom: parent.bottom
                    height: Math.max(1, Style.space(2))
                    clip: true

                    Rectangle {
                      anchors.fill: parent
                      color: root.hairline
                    }

                    Rectangle {
                      id: updateSweep
                      width: Math.max(4, Math.round(parent.width * 0.34))
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
          id: primaryActionRow
          Layout.fillWidth: true
          spacing: Style.space(8)
          readonly property real buttonWidth: Math.floor((width - spacing * 2) / 3)

          ActionButton {
            id: recordActionBtn
            width: primaryActionRow.buttonWidth
            rowIndex: 0
            label: root.primaryLabel()
            glyph: root.primaryGlyph()
            tone: "accent"
            primary: true
            hint: "r"
          }

          ActionButton {
            id: pauseActionBtn
            width: primaryActionRow.buttonWidth
            rowIndex: 1
            label: root.canPause ? (root.paused ? "Resume" : "Pause") : "No pause"
            glyph: root.paused ? "󰐊" : "󰏤"
            tone: "neutral"
            enabled: root.controlsActive && root.canPause
            hint: "p"
          }

          ActionButton {
            width: Math.max(0, primaryActionRow.width - primaryActionRow.spacing * 2
              - recordActionBtn.width - pauseActionBtn.width)
            rowIndex: 2
            label: root.discardArmed ? "Discard?" : "Discard"
            glyph: "󰆴"
            tone: "neutral"
            enabled: root.controlsActive
            hint: "d"
          }
        }

        // The strip is the panel's navigation: four equal pages (Recording,
        // Visuals, History, Shortcuts) cycled left to right with Tab.
        Item {
          Layout.fillWidth: true
          Layout.preferredHeight: Style.space(30)
          Layout.topMargin: Style.space(4)
          Layout.bottomMargin: 0

          Row {
            id: settingsTabRow
            anchors.fill: parent
            spacing: 0
            readonly property real tabWidth: Math.floor(width / root.navDestinations.length)

            Repeater {
              model: root.navDestinations

              delegate: Item {
                id: settingsTabItem
                required property var modelData
                required property int index
                width: index === root.navDestinations.length - 1
                  ? Math.max(0, settingsTabRow.width - settingsTabRow.tabWidth * (root.navDestinations.length - 1))
                  : settingsTabRow.tabWidth
                height: settingsTabRow.height

                  readonly property bool active: root.isNavActive(modelData.id)
                  readonly property bool hot: root.pointerActive && settingsTabMouse.containsMouse

                  Row {
                    id: tabLabelRow
                    anchors.centerIn: parent
                    spacing: Style.space(5)

                    Text {
                      textFormat: Text.PlainText
                      text: modelData.label
                      color: settingsTabItem.active ? root.foreground
                        : settingsTabItem.hot ? root.foreground
                        : Util.alpha(root.foreground, 0.58)
                      font.family: root.fontFamily
                      font.pixelSize: Style.font.bodySmall
                      font.letterSpacing: 0.2
                      font.bold: settingsTabItem.active
                      anchors.verticalCenter: parent.verticalCenter
                      Behavior on color {
                        ColorAnimation { duration: 150 }
                      }
                    }
                  }

                  Rectangle {
                    anchors.bottom: parent.bottom
                    anchors.bottomMargin: 1
                    anchors.horizontalCenter: parent.horizontalCenter
                    width: Math.max(0, parent.width - Style.space(14))
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
                      ? Math.max(0, parent.width - Style.space(14)) : 0
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
                    onClicked: root.selectNav(modelData.id)
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
                  model: root.settingsTab === "shortcuts" ? [] : Model.groupBySection(root.settingsRows())

                  Column {
                    id: sectionGroup
                    required property var modelData
                    width: parent.width
                    spacing: Style.space(5)

                    SectionMicroHeader {
                      visible: sectionGroup.modelData.section !== ""
                      text: sectionGroup.modelData.section
                    }

                    Rectangle {
                      width: parent.width
                      implicitHeight: sectionCardCol.implicitHeight
                      radius: root.panelRadius
                      color: root.cardFill
                      border.width: 1
                      border.color: root.cardBorder
                      clip: true

                      Column {
                        id: sectionCardCol
                        width: parent.width
                        spacing: 0

                        Repeater {
                          model: sectionGroup.modelData.rows

                          Column {
                            required property var modelData
                            required property int index
                            width: parent.width
                            spacing: 0

                            Rectangle {
                              visible: index > 0
                              width: parent.width
                              height: 1
                              color: root.rowDivider
                            }

                            SettingRow {
                              width: parent.width
                              settingKey: modelData.key
                              label: modelData.label
                              glyph: modelData.glyph
                              kind: modelData.kind === undefined ? "cycle" : String(modelData.kind)
                              infoValue: modelData.value === undefined ? "" : String(modelData.value)
                              rowIndex: modelData.flatIndex
                              dependent: modelData.dependent === true
                              subtitle: root.settingSubtitle(modelData.key)
                              disabled: root.settingDisabled(modelData.key)
                              dependencyHint: root.settingDependencyHint(modelData.key)
                            }
                          }
                        }

                        Column {
                          visible: root.settingsTab === "recording"
                            && sectionGroup.modelData.section === "Saving"
                          width: parent.width
                          spacing: 0

                          Rectangle {
                            width: parent.width
                            height: 1
                            color: root.rowDivider
                          }

                          Column {
                            width: parent.width
                            padding: Style.space(8)
                            spacing: Style.space(6)

                            Text {
                              textFormat: Text.PlainText
                              text: "Filename prefix"
                              color: Util.alpha(root.foreground, 0.85)
                              font.family: root.fontFamily
                              font.pixelSize: Style.font.bodySmall
                            }

                            Rectangle {
                              width: parent.width - Style.space(16)
                              implicitHeight: Style.space(28)
                              radius: root.panelRadius
                              color: root.restFill
                              border.width: 1
                              border.color: prefixInput.activeFocus ? root.accent : root.hairline
                              Behavior on border.color {
                                ColorAnimation { duration: 120 }
                              }

                              GuardedInput {
                                id: prefixInput
                                pasteLimit: root.pasteLimit
                                anchors.fill: parent
                                anchors.leftMargin: Style.space(8)
                                anchors.rightMargin: Style.space(8)
                                verticalAlignment: TextInput.AlignVCenter
                                text: root.currentValue("filenamePrefix")
                                color: root.foreground
                                selectionColor: root.accent
                                selectedTextColor: root.inkOn(root.accent)
                                font.family: root.fontFamily
                                font.pixelSize: Style.font.body
                                clip: true
                                onActiveFocusChanged: {
                                  root.typingPrefix = activeFocus
                                  if (!activeFocus) root.cancelPaste("prefix")
                                }
                                onTextEdited: root.clearPasteError("prefix")
                                onEditingFinished: root.writeSetting("filenamePrefix", text)
                                onBoundedPaste: root.startPaste("prefix", prefixInput, root.pasteLimit)
                              }
                            }

                            Text {
                              textFormat: Text.PlainText
                              text: root.pasteErrorKind === "prefix" ? root.pasteError
                                : root.currentValue("filenamePrefix") === ""
                                  ? "Empty uses reel-<date>. Recordings: " + Model.shortPath(root.outputDir, root.home)
                                  : "Named " + root.currentValue("filenamePrefix") + "<date>. Recordings: " + Model.shortPath(root.outputDir, root.home)
                              color: root.pasteErrorKind === "prefix" ? root.urgent : Util.alpha(root.foreground, 0.50)
                              font.family: root.fontFamily
                              font.pixelSize: Style.font.caption
                              wrapMode: Text.WordWrap
                              width: parent.width - Style.space(16)
                            }
                          }
                        }
                      }
                    }
                  }
                }
              }
            }

            // ---------- shortcuts page ----------

            Rectangle {
              visible: root.page === "home" && root.settingsTab === "shortcuts"
              width: parent.width
              implicitHeight: shortcutsCol.implicitHeight
              radius: root.panelRadius
              color: root.cardFill
              border.width: 1
              border.color: root.cardBorder
              clip: true

              Column {
                id: shortcutsCol
                width: parent.width
                spacing: 0

                Repeater {
                  model: root.page === "home" && root.settingsTab === "shortcuts" ? root.shortcutRows : []

                  Column {
                    required property var modelData
                    required property int index
                    width: parent.width
                    spacing: 0

                    Rectangle {
                      visible: index > 0
                      width: parent.width
                      height: 1
                      color: root.rowDivider
                    }

                    ShortcutRow {
                      width: parent.width
                      row: modelData
                      rowIndex: index
                    }
                  }
                }
              }
            }

            // ---------- devices page ----------

            Column {
              width: parent.width
              visible: root.page === "devices"
              spacing: Style.space(10)

              Item {
                width: parent.width
                implicitHeight: Math.max(devicesHeader.implicitHeight, devicesHint.implicitHeight)

                SectionMicroHeader {
                  id: devicesHeader
                  text: "Input devices"
                  anchors.left: parent.left
                  anchors.verticalCenter: parent.verticalCenter
                }

                Text {
                  id: devicesHint
                  textFormat: Text.PlainText
                  text: "esc back"
                  color: Util.alpha(root.foreground, 0.50)
                  font.family: root.fontFamily
                  font.pixelSize: Style.font.caption
                  anchors.right: parent.right
                  anchors.verticalCenter: parent.verticalCenter
                }
              }

              Repeater {
                model: root.page === "devices" ? Model.groupBySection(root.deviceRows()) : []

                Column {
                  id: deviceSectionGroup
                  required property var modelData
                  width: parent.width
                  spacing: Style.space(5)

                  SectionMicroHeader {
                    visible: deviceSectionGroup.modelData.section !== ""
                    text: deviceSectionGroup.modelData.section
                  }

                  Rectangle {
                    width: parent.width
                    implicitHeight: deviceCardCol.implicitHeight
                    radius: root.panelRadius
                    color: root.cardFill
                    border.width: 1
                    border.color: root.cardBorder
                    clip: true

                    Column {
                      id: deviceCardCol
                      width: parent.width
                      spacing: 0

                      Repeater {
                        model: deviceSectionGroup.modelData.rows

                        Column {
                          required property var modelData
                          required property int index
                          width: parent.width
                          spacing: 0

                          Rectangle {
                            visible: index > 0
                            width: parent.width
                            height: 1
                            color: root.rowDivider
                          }

                          DeviceRow {
                            width: parent.width
                            row: modelData
                            rowIndex: modelData.flatIndex
                          }
                        }
                      }
                    }
                  }
                }
              }
            }

            // ---------- history page ----------

            Column {
              width: parent.width
              visible: root.page === "history"
                && (root.visibleRecents.length > 0 || root.savingTake)
              spacing: Style.space(8)

              OpenFolderRow {
                width: parent.width
                rowIndex: 0
              }

              Repeater {
                model: savingRowsModel

                delegate: SavingRow {
                  required property string label
                  width: parent.width
                  title: label
                }
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

        // ---------- bottom keyhint footer ----------

        Item {
          id: footerStrip
          readonly property int outerPad: Math.min(keyboardPanel.padding, Style.space(8))
          readonly property int bottomOverlap: keyboardPanel.padding - outerPad
          Layout.fillWidth: true
          Layout.preferredHeight: 1 + outerPad + Style.space(16) - bottomOverlap

          Rectangle {
            id: footerHairline
            anchors.top: parent.top
            width: parent.width
            height: 1
            color: root.hairline
          }

          // Match outerPad below the hairline and down to the card border so
          // the keyhints sit compact and vertically centered in the footer band.
          Item {
            anchors.top: footerHairline.bottom
            anchors.topMargin: footerStrip.outerPad
            anchors.left: parent.left
            anchors.right: parent.right
            anchors.bottom: parent.bottom
            anchors.bottomMargin: -footerStrip.bottomOverlap

            Row {
              anchors.left: parent.left
              anchors.verticalCenter: parent.verticalCenter
              spacing: Style.space(8)

              Repeater {
                model: root.footerHints()

                Row {
                  required property var modelData
                  spacing: Style.space(4)

                  Rectangle {
                    width: footerKeyText.implicitWidth + Style.space(8)
                    height: Style.space(16)
                    radius: root.chipRadius(Style.space(3))
                    color: Util.alpha(root.foreground, 0.06)
                    border.width: 1
                    border.color: Util.alpha(root.foreground, 0.14)
                    anchors.verticalCenter: parent.verticalCenter

                    Text {
                      id: footerKeyText
                      anchors.centerIn: parent
                      textFormat: Text.PlainText
                      text: modelData.key
                      color: Util.alpha(root.foreground, 0.78)
                      font.family: root.fontFamily
                      font.pixelSize: Style.font.caption
                      font.bold: true
                    }
                  }

                  Text {
                    textFormat: Text.PlainText
                    text: modelData.label
                    color: Util.alpha(root.foreground, 0.50)
                    font.family: root.fontFamily
                    font.pixelSize: Style.font.caption
                    anchors.verticalCenter: parent.verticalCenter
                  }
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

              GuardedInput {
                id: renameInput
                pasteLimit: root.pasteLimit
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
                clip: true
                enabled: !root.renameBusy
                focus: root.takeMenuRenaming
                onTextEdited: {
                  root.renameText = text
                  root.clearPasteError("rename")
                }
                onActiveFocusChanged: {
                  if (activeFocus) {
                    selectAll()
                  } else {
                    root.cancelPaste("rename")
                  }
                }
                onAccepted: root.commitRename()
                onBoundedPaste: root.startPaste("rename", renameInput, root.pasteLimit)
                Keys.onEscapePressed: function(event) {
                  root.cancelRename()
                  event.accepted = true
                }
              }
            }

            Text {
              textFormat: Text.PlainText
              text: root.pasteErrorKind === "rename" ? root.pasteError
                : root.renameBusy ? "Renaming…" : "Enter to rename · Esc to cancel"
              color: root.pasteErrorKind === "rename" ? root.urgent : root.dim
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

  // Reel owns the key map because h/l are vim-style movement. PanelKeyCatcher
  // reserves h for vim-left and Tab for its own focus ring.
  component ReelKeyCatcher: Item {
    id: catcher

    property bool blocked: false

    signal moveRequested(int dx, int dy)
    signal activateRequested()
    signal closeRequested()
    signal deleteRequested()
    signal tabRequested(int direction)
    signal textKey(string text, int modifiers)
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
      // Letter shortcuts follow the key's position on non-Latin layouts.
      var text = Model.shortcutText(event.text, event.nativeScanCode)
      if (text === "h") {
        catcher.moveRequested(-1, 0)
        event.accepted = true
        return
      }
      if (text === "l") {
        catcher.moveRequested(1, 0)
        event.accepted = true
        return
      }
      if (event.key === Qt.Key_Down || text === "j") {
        catcher.moveRequested(0, 1)
        event.accepted = true
        return
      }
      if (event.key === Qt.Key_Up || text === "k") {
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
      if (event.key === Qt.Key_Delete || text === "x" || text === "X") {
        catcher.deleteRequested()
        event.accepted = true
        return
      }
      // Share is handled here as well as in the root key map so it works while
      // the actions menu is open and survives any other text-key handling.
      if (text === "u") {
        if (root.takeMenuOpen) root.closeTakeMenu()
        root.shareSelected()
        event.accepted = true
        return
      }
      if (text.length === 1) {
        catcher.textKey(text, event.modifiers)
      }
    }
  }

  component SectionMicroHeader: Text {
    textFormat: Text.PlainText
    text: ""
    font.family: root.fontFamily
    font.pixelSize: Style.font.caption
    font.bold: true
    font.letterSpacing: 1.0
    font.capitalization: Font.AllUppercase
    color: Util.alpha(root.foreground, 0.48)
    leftPadding: Style.space(2)
    topPadding: Math.ceil(font.pixelSize * 0.15)
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

    readonly property bool pointerHovered: root.pointerActive && root.hoverSection === "primary" && root.hoverIndex === rowIndex
    readonly property bool keyboardCursor: root.cursorActive
      && root.focusSection === "primary" && root.selectedIndex === rowIndex

    hasCursor: pointerHovered || (keyboardCursor && !root.hoverPainted)
    onHasCursorChanged: if (hasCursor && root.keyboardScroll) root.ensureCursorVisible(action)
    foreground: root.foreground
    accent: root.accent
    fill: Style.hoverFillFor(root.foreground, root.accent)
    currentFill: root.selectedFill
    color: !enabled ? root.cardFill
      : primary ? (actionMouse.pressed ? Qt.darker(action.toneColor, 0.92) : action.toneColor)
      : actionMouse.pressed ? root.selectedFill
      : hasCursor ? action.fill
      : root.restFill
    borderSpec: !enabled
      ? Border.flat(Util.alpha(root.foreground, 0.06), 1)
      : primary ? Border.flat(keyboardCursor ? Qt.lighter(action.toneColor, 1.25) : "transparent",
          keyboardCursor ? 1 : 0)
      : Border.flat(hasCursor ? root.accent : Util.alpha(root.foreground, 0.10), 1)
    implicitHeight: Style.space(38)
    opacity: enabled ? 1.0 : 0.38
    scale: actionMouse.pressed ? 0.985 : 1.0
    Behavior on scale {
      NumberAnimation { duration: 90; easing.type: Easing.OutCubic }
    }

    Rectangle {
      visible: action.primary && action.enabled
      anchors.fill: parent
      radius: root.panelRadius
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
      anchors.leftMargin: Math.max(1, Math.round(root.panelRadius * 0.5))
      anchors.rightMargin: Math.max(1, Math.round(root.panelRadius * 0.5))
      height: 1
      color: Util.alpha(root.foreground, 0.22)
      opacity: 0.75
    }

    Rectangle {
      anchors.fill: parent
      radius: root.panelRadius
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
      anchors.rightMargin: Style.space(6)
      anchors.verticalCenter: parent.verticalCenter
      spacing: Style.space(7)

      Text {
        id: actionGlyphText
        textFormat: Text.PlainText
        text: action.glyph
        color: action.primary ? action.toneInk : root.foreground
        font.family: root.fontFamily
        font.pixelSize: Style.font.icon
        anchors.verticalCenter: parent.verticalCenter
      }

      Text {
        width: Math.max(0, parent.width - actionGlyphText.implicitWidth - parent.spacing)
        textFormat: Text.PlainText
        text: action.label
        color: action.primary ? action.toneInk : root.foreground
        font.family: root.fontFamily
        font.pixelSize: Style.font.body
        font.letterSpacing: 0.2
        font.bold: action.primary
        elide: Text.ElideRight
        anchors.verticalCenter: parent.verticalCenter
      }
    }

    KeyHintChip {
      id: hintSlot
      visible: action.hint !== ""
      anchors.right: parent.right
      anchors.rightMargin: Style.space(8)
      anchors.verticalCenter: parent.verticalCenter
      horizontalPadding: Style.space(8)
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
    radius: root.panelRadius
    color: root.cardFill
    border.width: 1
    border.color: root.cardBorder
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
        radius: root.chipRadius(Style.space(4))
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
    fill: root.cursorFill
    color: hasCursor ? root.cursorFill : root.cardFill
    borderSpec: Border.flat(hasCursor ? root.accent : root.cardBorder, 1)
    implicitHeight: removing ? 0 : Style.space(56)
    radius: root.panelRadius
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
      anchors.leftMargin: Style.space(8)
      anchors.rightMargin: Style.space(8)
      spacing: Style.space(9)

      Rectangle {
        width: Style.space(84)
        height: Style.space(48)
        radius: root.chipRadius(Style.space(4))
        anchors.verticalCenter: parent.verticalCenter
        color: Util.alpha(root.foreground, 0.08)
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

        Text {
          visible: take.preview === "" || previewImage.status !== Image.Ready
          anchors.centerIn: parent
          textFormat: Text.PlainText
          text: "󰕧"
          color: root.dim
          font.family: root.fontFamily
          font.pixelSize: Style.font.title
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
        spacing: Style.space(4)
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
      width: Math.max(2, Style.space(3))
      height: parent.height
      anchors.left: parent.left
      anchors.verticalCenter: parent.verticalCenter
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
    fill: root.cursorFill
    color: hasCursor ? root.cursorFill : root.cardFill
    borderSpec: Border.flat(hasCursor ? root.accent : root.cardBorder, 1)
    implicitHeight: Style.space(34)
    radius: root.panelRadius
    clip: true

    Row {
      anchors.left: parent.left
      anchors.leftMargin: Style.space(10)
      anchors.right: folderHint.visible ? folderHint.left : parent.right
      anchors.rightMargin: Style.space(8)
      anchors.verticalCenter: parent.verticalCenter
      spacing: Style.space(10)

      Text {
        id: folderGlyph
        textFormat: Text.PlainText
        text: "󰉋"
        color: folder.hasCursor ? root.foreground : Util.alpha(root.foreground, 0.72)
        font.family: root.fontFamily
        font.pixelSize: Style.font.icon
        anchors.verticalCenter: parent.verticalCenter
      }

      Text {
        textFormat: Text.PlainText
        text: root.visibleRecents.length > 0 ? "Open the recordings folder" : "No takes yet · open the recordings folder"
        color: root.foreground
        font.family: root.fontFamily
        font.pixelSize: Style.font.body
        anchors.verticalCenter: parent.verticalCenter
        elide: Text.ElideRight
        width: Math.max(0, parent.width - folderGlyph.implicitWidth - parent.spacing)
      }
    }

    KeyHintChip {
      id: folderHint
      visible: folder.hasCursor
      anchors.right: parent.right
      anchors.rightMargin: Style.space(10)
      anchors.verticalCenter: parent.verticalCenter
      horizontalPadding: Style.space(8)
      label: "o"
      active: true
    }

    Rectangle {
      visible: folder.hasCursor
      width: Math.max(2, Style.space(3))
      height: parent.height
      anchors.left: parent.left
      anchors.verticalCenter: parent.verticalCenter
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
      radius: chip.radius > 0 ? chip.radius + Style.space(1) : 0
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
      radius: root.chipRadius(Style.space(4))
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
    readonly property bool selected: root.cursorActive
      && root.page === "home" && root.settingsTab === "shortcuts"
      && root.focusSection === "settings" && root.settingsIndex === rowIndex

    hasCursor: pointerHovered || (selected && !root.hoverPainted)
    onHasCursorChanged: if (hasCursor && root.keyboardScroll) root.ensureCursorVisible(shortcut)
    foreground: root.foreground
    accent: root.accent
    fill: root.cursorFill
    borderSpec: Border.none()
    implicitHeight: Style.space(32)
    radius: 0

    Row {
      anchors.fill: parent
      anchors.leftMargin: Style.space(10)
      anchors.rightMargin: Style.space(10)
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
      width: Math.max(2, Style.space(3))
      height: parent.height
      anchors.left: parent.left
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

  component DeviceRow: CursorSurface {
    id: device
    property var row: null
    property int rowIndex: 0

    readonly property bool pointerHovered: root.pointerActive
      && root.hoverSection === "devices" && root.hoverIndex === rowIndex
    readonly property bool selected: root.cursorActive
      && root.page === "devices"
      && root.focusSection === "settings"
      && root.settingsIndex === rowIndex
    readonly property bool chosen: row !== null && row.selected === true

    hasCursor: pointerHovered || (selected && !root.hoverPainted)
    onHasCursorChanged: if (hasCursor && root.keyboardScroll) root.ensureCursorVisible(device)
    foreground: root.foreground
    accent: root.accent
    fill: root.cursorFill
    borderSpec: Border.none()
    implicitHeight: Style.space(34)
    radius: 0

    Row {
      anchors.left: parent.left
      anchors.leftMargin: Style.space(12)
      anchors.right: deviceTrailing.left
      anchors.rightMargin: Style.space(8)
      anchors.verticalCenter: parent.verticalCenter
      spacing: Style.space(10)

      Text {
        id: deviceGlyph
        textFormat: Text.PlainText
        text: device.row === null ? "" : String(device.row.glyph || "")
        color: device.chosen ? root.accent
          : device.hasCursor ? root.foreground
          : Util.alpha(root.foreground, 0.68)
        font.family: root.fontFamily
        font.pixelSize: Style.font.icon
        anchors.verticalCenter: parent.verticalCenter
      }

      Text {
        id: deviceLabel
        textFormat: Text.PlainText
        text: device.row === null ? "" : String(device.row.label || "")
        color: device.hasCursor || device.chosen ? root.foreground : root.secondary
        font.family: root.fontFamily
        font.pixelSize: Style.font.body
        font.bold: device.chosen
        anchors.verticalCenter: parent.verticalCenter
        // A middle ellipsis keeps the distinguishing tail of long ALSA and
        // camera names visible. The full name stays available in a tooltip.
        elide: Text.ElideMiddle
        width: Math.max(0, parent.width - deviceGlyph.implicitWidth - parent.spacing)
      }
    }

    Row {
      id: deviceTrailing
      anchors.right: parent.right
      anchors.rightMargin: Style.space(10)
      anchors.verticalCenter: parent.verticalCenter
      spacing: Style.space(8)

      KeyHintChip {
        visible: device.hasCursor && !device.chosen
        anchors.verticalCenter: parent.verticalCenter
        horizontalPadding: Style.space(8)
        label: "Enter"
        active: true
      }

      Text {
        id: deviceCheck
        textFormat: Text.PlainText
        text: device.chosen ? "󰄬" : ""
        color: root.accent
        font.family: root.fontFamily
        font.pixelSize: Style.font.body
        anchors.verticalCenter: parent.verticalCenter
        width: Style.space(16)
        horizontalAlignment: Text.AlignHCenter
      }
    }

    Rectangle {
      visible: device.hasCursor
      width: Math.max(2, Style.space(3))
      height: parent.height
      anchors.left: parent.left
      anchors.verticalCenter: parent.verticalCenter
      color: root.accent
    }

    MouseArea {
      id: deviceMouse
      anchors.fill: parent
      hoverEnabled: true
      cursorShape: Qt.PointingHandCursor
      onContainsMouseChanged: root.setRowHover("devices", device.rowIndex, containsMouse)
      onClicked: {
        root.cursorActive = true
        root.focusSection = "settings"
        root.settingsIndex = device.rowIndex
        root.chooseDevice(device.row)
      }
    }

    PanelToolTip {
      visible: deviceMouse.containsMouse && root.pointerActive && deviceLabel.truncated
      text: device.row === null ? "" : String(device.row.label || "")
      fontFamily: root.fontFamily
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
    property bool dependent: false
    property string subtitle: ""
    property bool disabled: false
    property string dependencyHint: ""

    readonly property int stepperMaxWidth: Math.max(Style.space(90), width * 0.42)
    readonly property bool pointerHovered: !setting.disabled
      && root.pointerActive && root.hoverSection === "settings" && root.hoverIndex === rowIndex
    readonly property bool selected: !setting.disabled && root.cursorActive
      && root.page === "home"
      && root.focusSection === "settings"
      && root.settingsIndex === rowIndex
    readonly property bool switchRow: setting.kind !== "info" && root.isSwitch(setting.settingKey)
    readonly property bool switchOn: root.currentValue(setting.settingKey) === "on"
    readonly property string inlineHint: setting.kind === "info" || setting.disabled ? ""
      : setting.switchRow ? "Space"
      : setting.kind === "page" ? "Enter"
      : "←/→"

    hasCursor: !setting.disabled && (pointerHovered || (selected && !root.hoverPainted))
    onHasCursorChanged: if (hasCursor && root.keyboardScroll) root.ensureCursorVisible(setting)
    foreground: root.foreground
    accent: root.accent
    fill: root.cursorFill
    color: setting.hasCursor ? root.cursorFill : "transparent"
    borderSpec: Border.none()
    implicitHeight: setting.subtitle !== "" ? Style.space(42) : Style.space(34)
    radius: 0

    Row {
      anchors.left: parent.left
      anchors.leftMargin: Style.space(12)
      anchors.right: trailingControls.left
      anchors.rightMargin: Style.space(8)
      anchors.verticalCenter: parent.verticalCenter
      spacing: Style.space(10)
      opacity: setting.disabled ? 0.36 : 1.0

      Text {
        id: settingGlyph
        textFormat: Text.PlainText
        text: setting.glyph
        color: setting.hasCursor ? root.foreground : Util.alpha(root.foreground, 0.72)
        font.family: root.fontFamily
        font.pixelSize: Style.font.icon
        anchors.verticalCenter: parent.verticalCenter
      }

      Column {
        width: Math.max(0, parent.width - settingGlyph.implicitWidth - parent.spacing)
        anchors.verticalCenter: parent.verticalCenter
        spacing: Style.space(1)

        Text {
          width: parent.width
          textFormat: Text.PlainText
          text: setting.label
          color: setting.hasCursor ? root.foreground : Util.alpha(root.foreground, 0.90)
          font.family: root.fontFamily
          font.pixelSize: Style.font.body
          elide: Text.ElideRight
        }

        Text {
          visible: setting.subtitle !== ""
          width: parent.width
          textFormat: Text.PlainText
          text: setting.subtitle
          color: Util.alpha(root.foreground, 0.48)
          font.family: root.fontFamily
          font.pixelSize: Style.font.caption
          elide: Text.ElideRight
        }
      }
    }

    Row {
      id: trailingControls
      anchors.right: parent.right
      anchors.rightMargin: Style.space(10)
      anchors.verticalCenter: parent.verticalCenter
      spacing: Style.space(8)
      opacity: setting.disabled ? 0.40 : 1.0

      Text {
        visible: setting.disabled && setting.dependencyHint !== ""
        textFormat: Text.PlainText
        text: setting.dependencyHint
        color: Util.alpha(root.foreground, 0.65)
        font.family: root.fontFamily
        font.pixelSize: Style.font.caption
        anchors.verticalCenter: parent.verticalCenter
      }

      KeyHintChip {
        visible: setting.hasCursor && !setting.disabled && setting.inlineHint !== ""
        anchors.verticalCenter: parent.verticalCenter
        horizontalPadding: Style.space(8)
        label: setting.inlineHint
        active: true
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

      // Page navigation value (e.g. Input devices -> Manage ›)
      Row {
        visible: setting.kind === "page"
        anchors.verticalCenter: parent.verticalCenter
        spacing: Style.space(4)

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

      // Inline stepper (< 60 fps >) for cycle options so keycaps remain the
      // only bordered boxes while allowing direct one-click stepping.
      Row {
        id: stepperRow
        visible: !setting.switchRow && setting.kind !== "info" && setting.kind !== "page"
        anchors.verticalCenter: parent.verticalCenter
        spacing: Style.space(2)

        Item {
          width: Style.space(14)
          height: Style.space(22)
          anchors.verticalCenter: parent.verticalCenter

          Text {
            anchors.centerIn: parent
            textFormat: Text.PlainText
            text: "‹"
            color: prevStepMouse.containsMouse || setting.hasCursor ? root.foreground : root.dim
            font.family: root.fontFamily
            font.pixelSize: Style.font.bodySmall
          }

          MouseArea {
            id: prevStepMouse
            anchors.fill: parent
            enabled: !setting.disabled
            hoverEnabled: true
            cursorShape: setting.disabled ? Qt.ArrowCursor : Qt.PointingHandCursor
            onClicked: {
              if (setting.disabled) return
              root.cursorActive = true
              root.focusSection = "settings"
              root.settingsIndex = setting.rowIndex
              root.cycleOption(setting.settingKey, -1)
            }
          }
        }

        Text {
          textFormat: Text.PlainText
          text: root.valueLabel(setting.settingKey, root.currentValue(setting.settingKey))
          color: setting.hasCursor ? root.foreground : Util.alpha(root.foreground, 0.86)
          font.family: root.fontFamily
          font.pixelSize: Style.font.bodySmall
          font.bold: true
          anchors.verticalCenter: parent.verticalCenter
          width: Math.min(implicitWidth, setting.stepperMaxWidth)
          horizontalAlignment: Text.AlignHCenter
          elide: Text.ElideRight
        }

        Item {
          width: Style.space(14)
          height: Style.space(22)
          anchors.verticalCenter: parent.verticalCenter

          Text {
            anchors.centerIn: parent
            textFormat: Text.PlainText
            text: "›"
            color: nextStepMouse.containsMouse || setting.hasCursor ? root.foreground : root.dim
            font.family: root.fontFamily
            font.pixelSize: Style.font.bodySmall
          }

          MouseArea {
            id: nextStepMouse
            anchors.fill: parent
            enabled: !setting.disabled
            hoverEnabled: true
            cursorShape: setting.disabled ? Qt.ArrowCursor : Qt.PointingHandCursor
            onClicked: {
              if (setting.disabled) return
              root.cursorActive = true
              root.focusSection = "settings"
              root.settingsIndex = setting.rowIndex
              root.cycleOption(setting.settingKey, 1)
            }
          }
        }
      }

      // Toggle switch: follows Omarchy's cornerRadius (sharp 0px when sharp,
      // rounded pill when rounded).
      Rectangle {
        visible: setting.switchRow
        anchors.verticalCenter: parent.verticalCenter
        width: Style.space(32)
        height: Style.space(16)
        radius: root.badgeRadius(height)
        color: setting.switchOn && !setting.disabled ? root.accent : Util.alpha(root.foreground, 0.14)
        border.width: 1
        border.color: setting.switchOn && !setting.disabled ? root.accent : Util.alpha(root.foreground, 0.18)
        Behavior on color {
          ColorAnimation { duration: Style.duration(120) }
        }
        Behavior on border.color {
          ColorAnimation { duration: Style.duration(120) }
        }

        Rectangle {
          width: parent.height - Style.space(4)
          height: width
          radius: root.badgeRadius(width)
          y: (parent.height - height) / 2
          x: setting.switchOn ? parent.width - width - Style.space(2) : Style.space(2)
          color: setting.switchOn && !setting.disabled
            ? root.inkOn(root.accent) : Util.alpha(root.foreground, 0.65)

          Behavior on x {
            NumberAnimation { duration: Style.duration(120); easing.type: Easing.OutCubic }
          }
          Behavior on color {
            ColorAnimation { duration: Style.duration(120) }
          }
        }
      }
    }

    Rectangle {
      visible: setting.hasCursor
      width: Math.max(2, Style.space(3))
      height: parent.height
      anchors.left: parent.left
      anchors.verticalCenter: parent.verticalCenter
      color: root.accent
    }

    MouseArea {
      anchors.fill: parent
      z: -1
      enabled: !setting.disabled
      hoverEnabled: !setting.disabled
      cursorShape: setting.disabled || setting.kind === "info" ? Qt.ArrowCursor : Qt.PointingHandCursor
      acceptedButtons: Qt.LeftButton | Qt.RightButton
      onContainsMouseChanged: {
        if (!setting.disabled) root.setRowHover("settings", setting.rowIndex, containsMouse)
      }
      onPositionChanged: function(mouse) {
        if (!setting.disabled) root.pointerMoved("settings", setting.rowIndex, setting, mouse)
      }
      onClicked: function(mouse) {
        if (setting.disabled || setting.kind === "info") return
        root.cursorActive = true
        root.focusSection = "settings"
        root.settingsIndex = setting.rowIndex
        if (setting.kind === "page") root.openDevices()
        else root.cycleOption(setting.settingKey, mouse.button === Qt.RightButton ? -1 : 1)
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
    id: audioDevicesProc
    running: false
    command: ["reel", "audio-devices", "--json"]
    stdout: StdioCollector {
      id: audioDevicesOut
      waitForEnd: true
      onStreamFinished: {
        try {
          var payload = JSON.parse(String(text || "{}") || "{}")
          var list = payload.devices
          root.audioDevices = Array.isArray(list) ? list : []
        } catch (e) {
          root.audioDevices = []
        }
      }
    }
    stderr: StdioCollector { waitForEnd: true }
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
  // uses its own process instead of waiting for controlProc to return. A pause
  // that never answers must not disable the button for the rest of the take.
  Timer {
    id: pauseDeadline
    interval: 6000
    repeat: false
    onTriggered: {
      if (!pauseProc.running) return
      pauseProc.signal(9)
      root.setNotice("Reel did not answer the pause command.", true)
    }
  }

  Process {
    id: pauseProc
    running: false
    stdout: StdioCollector { waitForEnd: true }
    stderr: StdioCollector { id: pauseErr; waitForEnd: true }
    onExited: function(code) {
      pauseDeadline.stop()
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
    function onLastErrorChanged() {
      if (root.service && root.service.lastError !== "")
        root.setNotice(root.service.lastError, true)
    }
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
        var currentVer = root.currentVersion()
        root.setNotice(currentVer !== ""
          ? "Reel v" + currentVer + " is up to date."
          : "Reel is up to date.")
      }
    }
  }

  Component.onCompleted: {
    root.syncBridge()
    root.syncSavingRows()
    if (root.service) root.service.refresh()
    root.refreshAudioDevices()
    if (!omacutProbe.running) omacutProbe.running = true
    // A shell restart in the middle of a save must still catch the handoff to
    // the finished take, so arm the hold as if finalizing had just started.
    if (root.finalizing) {
      root.savingWasFinalizing = true
      root.savingTitleCache = Model.savingTitle(root.savingTakePath)
    }
  }
}
