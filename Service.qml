import QtQuick
import Quickshell
import Quickshell.Io
import "Model.js" as Model

// Reel's background service. It owns the run-time state the bar reads, so the
// widget keeps an honest timer and a take that ends without a stop is noticed
// even while the panel is closed. The CLI still owns the recording itself.
Item {
  id: root
  visible: false

  // Injected by the plugin host.
  property var shell: null

  readonly property string home: Quickshell.env("HOME") || ""

  property string stateText: ""
  property var state: Model.emptyState()
  property var recents: []
  property int recentVersion: 0
  property string outputDir: ""
  property string version: ""
  property string lastError: ""
  property int tick: 0
  property bool recentPending: false
  property string stateSignature: ""
  // The update check is background work: a cached or remote answer the panel
  // and bar read without ever waiting on it.
  property var updateInfo: Model.emptyUpdate()
  property bool updateChecking: false
  property bool updatePending: false
  // True once the first-run configure step (the ~/.local/bin link) has
  // finished. Commands must not run before that, or a fresh install flashes a
  // "not on PATH" error while bin/configure is still linking the wrapper.
  property bool configured: false
  // Configure's own error is the useful one on a fresh install; later command
  // failures must not replace it with a generic "not on PATH" message.
  property bool configureFailed: false
  property bool configuring: false
  property bool configureStarted: false
  property int configureAttempts: 0

  readonly property bool active: Model.isActive(root.state)
  readonly property bool finalizing: Model.status(root.state) === "finalizing"
  readonly property bool busy: Model.isBusy(root.state)
  readonly property bool starting: Model.status(root.state) === "starting"
  // The encoder is launched and warming up; the take is running even though
  // its state has not flipped to "recording" yet.
  readonly property bool capturing: root.starting
    && String(root.state.stage || "") === "capture"
  readonly property int elapsed: Model.elapsed(root.state, root.tick)

  function refresh() {
    if (!root.configured) return
    root.refreshRecent()
    root.refreshOutputDir()
    if (root.version === "" && !versionProc.running) {
      versionDeadline.restart()
      versionProc.running = true
    }
    root.startStatus()
  }

  function startStatus() {
    if (!root.configured || statusProc.running) return
    // Start the deadline before the launch so a process that never starts is
    // still recovered, not just one that hangs after starting.
    statusDeadline.restart()
    statusProc.running = true
  }

  function refreshRecent() {
    if (!root.configured) return
    if (recentProc.running) {
      root.recentPending = true
      return
    }
    root.recentPending = false
    recentDeadline.restart()
    recentProc.running = true
  }

  function refreshOutputDir() {
    if (!root.configured) return
    if (outputProc.running) return
    outputDeadline.restart()
    outputProc.running = true
  }

  // The engine caches a successful check for an hour, so an ordinary poll is
  // cheap. `force` is the user asking again; it always contacts the remote.
  function checkForUpdates(force) {
    if (!root.configured) return
    if (updateProc.running) {
      root.updatePending = root.updatePending || !!force
      return
    }
    var command = ["reel", "update", "--check", "--json"]
    if (force) command.push("--force")
    root.updateChecking = true
    updateProc.hasStarted = false
    updateProc.command = command
    updateDeadline.restart()
    updateProc.running = true
  }

  function applyUpdate(text) {
    root.updateInfo = Model.parseUpdate(String(text || ""))
    // The check reports the running engine version even when the remote is
    // unreachable, so it is a second source for the pill until --version
    // answers.
    if (root.updateInfo.current !== "") root.version = root.updateInfo.current
  }

  function applyVersion(text) {
    var value = Model.parseVersion(String(text || ""))
    if (value !== "") root.version = value
  }

  function applyState(text) {
    var previous = Model.status(root.state)
    var parsed = Model.parseStateSafe(text)
    var signature = JSON.stringify(parsed)
    root.stateText = String(text || "")
    if (signature === root.stateSignature) return
    root.stateSignature = signature
    root.state = parsed
    var next = Model.status(parsed)
    if (previous !== next) {
      root.tick = 0
      if (next === "idle" || next === "finalizing") {
        root.refreshRecent()
        root.refreshOutputDir()
      }
    }
  }

  function applyStatus(text) {
    root.applyState(text)
  }

  function applyRecent(text) {
    root.recents = Model.parseRecent(text)
    root.recentVersion++
  }

  // First-run bootstrap: the plugin id is stable, so the installed path is
  // deterministic. The configure step links the shipped engine's wrapper into
  // ~/.local/bin; nothing else runs until it exits. A start failure or a hung
  // configure must not leave every command gated forever, so it becomes a
  // visible, retryable error instead.
  Component.onCompleted: root.startConfigure()

  function startConfigure() {
    if (configureProc.running) return
    root.configuring = true
    root.configureStarted = false
    root.configured = false
    configureProc.command = [
      root.home + "/.config/omarchy/plugins/io.github.tahadx.reel/bin/configure"
    ]
    configureProc.running = true
    configureDeadline.restart()
  }

  function configureFailedWith(message) {
    if (!root.configuring) return
    configureDeadline.stop()
    root.configuring = false
    root.configureFailed = true
    // Unblock the panel so it can show the error and offer a retry; a failed
    // setup must not leave every command disabled with no explanation.
    root.configured = true
    root.lastError = message
    if (root.configureAttempts < 3) {
      root.configureAttempts++
      configureRetry.interval = 2000 * Math.pow(2, root.configureAttempts - 1)
      configureRetry.restart()
    }
    root.refresh()
  }

  function configureSucceeded() {
    var wasFailed = root.configureFailed
    configureDeadline.stop()
    root.configuring = false
    root.configureFailed = false
    root.configureAttempts = 0
    root.configured = true
    if (wasFailed) root.lastError = ""
    root.refresh()
  }

  // The panel calls this when it opens on a failed setup, so recovery does not
  // require restarting the shell.
  function retryConfigure() {
    configureRetry.stop()
    root.configureAttempts = 0
    root.lastError = ""
    root.startConfigure()
  }

  // A finite engine call that outlives its deadline is killed rather than left
  // holding the service. The next poll can retry normally.
  function noteTimeout(label) {
    if (label === "setup") {
      root.configureFailedWith("Reel's setup step did not finish. Open the plugin's bin/configure or reinstall Reel.")
      return
    }
    if (!root.configured) return
    if (label === "update") {
      root.updateChecking = false
      root.updateInfo = {
        available: false,
        current: root.version,
        latest: "",
        behind: 0,
        checkedAt: 0,
        error: "The update check took too long."
      }
      return
    }
    if (label === "recent") root.recentPending = false
    root.lastError = "Reel did not answer the " + label + " step in time."
  }

  component ProcessDeadline: Timer {
    id: deadline
    property var watched: null
    property string label: ""
    property int milliseconds: 5000
    interval: milliseconds
    repeat: false
    onTriggered: {
      if (!watched || !watched.running) return
      watched.signal(9)
      root.noteTimeout(label)
    }
  }

  Process {
    id: configureProc
    running: false
    stderr: StdioCollector {
      id: configureError
      waitForEnd: true
    }
    onStarted: {
      root.configureStarted = true
      configureDeadline.restart()
    }
    onRunningChanged: {
      if (configureProc.running) return
      configureDeadline.stop()
      if (!root.configureStarted && root.configuring)
        root.configureFailedWith("Reel could not start its setup step. Open the plugin's bin/configure or reinstall Reel.")
      root.configureStarted = false
    }
    onExited: function(code) {
      root.configureStarted = false
      var message = String(configureError.text || "").trim()
      if (code !== 0) {
        root.configureFailedWith(message !== ""
          ? message
          : "Reel could not configure its command. Run the plugin's bin/configure for details.")
        return
      }
      if (message !== "") root.lastError = message
      root.configureSucceeded()
    }
  }

  ProcessDeadline {
    id: configureDeadline
    watched: configureProc
    label: "setup"
    milliseconds: 15000
  }

  Timer {
    id: configureRetry
    repeat: false
    onTriggered: if (!root.configured || root.configureFailed) root.startConfigure()
  }

  // State is read through `reel status --json`, whose Rust reader is bounded,
  // instead of pulling state.json straight into the shell. Poll slowly while
  // idle and once a second while a take is live so the bar clock stays honest.
  Timer {
    id: statePoll
    interval: root.active || root.starting || root.finalizing ? 1000 : 5000
    repeat: true
    running: root.configured
    onTriggered: root.startStatus()
  }

  Process {
    id: statusProc
    property bool hasStarted: false
    command: ["reel", "status", "--json"]
    stdout: StdioCollector {
      waitForEnd: true
      onStreamFinished: root.applyStatus(String(text || ""))
    }
    stderr: StdioCollector {
      waitForEnd: true
      onStreamFinished: {
        var message = String(text || "").trim()
        if (message !== "" && !root.configureFailed) root.lastError = message
      }
    }
    onStarted: {
      statusProc.hasStarted = true
      statusDeadline.restart()
    }
    onRunningChanged: {
      if (statusProc.running) return
      statusDeadline.stop()
      if (!statusProc.hasStarted && root.configured && !root.configureFailed)
        root.lastError = "Reel's status command could not start."
      statusProc.hasStarted = false
    }
    onExited: function(code) {
      statusDeadline.stop()
      // Configure owns the first failure message. Only report a missing
      // command once configure has succeeded (or left no error of its own).
      if (code === 127 && root.configured && root.lastError === "")
        root.lastError = "The reel command is not on PATH."
    }
  }

  ProcessDeadline {
    id: statusDeadline
    watched: statusProc
    label: "status"
    milliseconds: 5000
  }

  Process {
    id: recentProc
    property bool hasStarted: false
    command: ["reel", "recent", "--json", "--limit", "20"]
    stdout: StdioCollector {
      waitForEnd: true
      onStreamFinished: root.applyRecent(String(text || ""))
    }
    stderr: StdioCollector {
      waitForEnd: true
      onStreamFinished: {
        var message = String(text || "").trim()
        if (message !== "" && !root.configureFailed) root.lastError = message
      }
    }
    onStarted: {
      recentProc.hasStarted = true
      recentDeadline.restart()
    }
    onRunningChanged: {
      if (recentProc.running) return
      recentDeadline.stop()
      if (!recentProc.hasStarted) {
        root.recentPending = false
        if (root.configured && !root.configureFailed)
          root.lastError = "Reel's history command could not start."
      }
      recentProc.hasStarted = false
    }
    onExited: function(code) {
      recentDeadline.stop()
      if (code === 127 && root.configured && root.lastError === "")
        root.lastError = "The reel command is not on PATH."
      if (root.recentPending) {
        root.recentPending = false
        root.refreshRecent()
      }
    }
  }

  ProcessDeadline {
    id: recentDeadline
    watched: recentProc
    label: "history"
    milliseconds: 8000
  }

  Process {
    id: outputProc
    command: ["reel", "output"]
    stdout: StdioCollector {
      waitForEnd: true
      onStreamFinished: {
        var value = String(text || "").trim()
        if (value !== "") root.outputDir = value
      }
    }
    onStarted: outputDeadline.restart()
    onExited: outputDeadline.stop()
  }

  ProcessDeadline {
    id: outputDeadline
    watched: outputProc
    label: "output folder"
    milliseconds: 5000
  }

  Process {
    id: versionProc
    running: false
    command: ["reel", "--version"]
    stdout: StdioCollector {
      waitForEnd: true
      onStreamFinished: root.applyVersion(String(text || ""))
    }
    onStarted: versionDeadline.restart()
    onExited: versionDeadline.stop()
  }

  ProcessDeadline {
    id: versionDeadline
    watched: versionProc
    label: "version"
    milliseconds: 5000
  }

  Process {
    id: updateProc
    property bool hasStarted: false
    running: false
    stdout: StdioCollector {
      waitForEnd: true
      onStreamFinished: root.applyUpdate(String(text || ""))
    }
    stderr: StdioCollector {
      waitForEnd: true
      onStreamFinished: {
        var message = String(text || "").trim()
        if (message !== "" && !root.configureFailed) root.lastError = message
      }
    }
    onStarted: {
      updateProc.hasStarted = true
      updateDeadline.restart()
    }
    onRunningChanged: {
      if (updateProc.running) return
      updateDeadline.stop()
      if (!updateProc.hasStarted) {
        root.updateChecking = false
        root.updateInfo = {
          available: false,
          current: root.version,
          latest: "",
          behind: 0,
          checkedAt: 0,
          error: "The update check could not start."
        }
      }
      updateProc.hasStarted = false
    }
    onExited: function(code) {
      updateDeadline.stop()
      root.updateChecking = false
      // A check that was asked for while this one ran runs once more, so a
      // manual refresh always ends with a fresh answer.
      if (root.updatePending) {
        root.updatePending = false
        root.checkForUpdates(true)
      }
    }
  }

  ProcessDeadline {
    id: updateDeadline
    watched: updateProc
    label: "update"
    milliseconds: 35000
  }

  // Check once when the service is ready and then once an hour. The engine
  // turns repeated calls inside the hour into a cached read.
  Timer {
    id: updateTimer
    interval: 3600000
    repeat: true
    triggeredOnStart: true
    running: root.configured
    onTriggered: root.checkForUpdates(false)
  }

  // One tick a second is enough for the bar clock and costs almost nothing.
  // During the capture warmup the clock already runs, so keep ticking there
  // too instead of freezing at 0:00 until "recording" arrives.
  Timer {
    id: tickTimer
    interval: 1000
    repeat: true
    running: root.active || root.capturing
    onTriggered: root.tick++
  }
}
