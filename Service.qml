import QtQuick
import Quickshell
import Quickshell.Io
import "Bridge.js" as Bridge
import "Model.js" as Model

// Reel's background service. It owns the run-time state the bar reads, so the
// widget keeps an honest timer and a take that ends without a stop is noticed
// even while the panel is closed. The CLI still owns the recording itself.
Item {
  id: root
  visible: false

  // Injected by the plugin host.
  property var shell: null
  // The bridge api the widget reads when its bar cannot give it this object.
  property var barBridge: null

  readonly property string home: Quickshell.env("HOME") || ""
  readonly property string runtimeDir: {
    var runtime = Quickshell.env("XDG_RUNTIME_DIR") || ""
    if (runtime) return runtime + "/reel"
    return (Quickshell.env("XDG_STATE_HOME") || root.home + "/.local/state") + "/omarchy/reel/run"
  }
  readonly property string statePath: root.runtimeDir + "/state.json"

  property string stateText: ""
  property var state: Model.emptyState()
  property string fileStateText: ""
  property var recents: []
  property int recentVersion: 0
  property string outputDir: ""
  property string lastError: ""
  property int tick: 0
  property bool recentPending: false
  // A state write can land while `reel status` is still answering; remember
  // it so the fast path does not lose the transition to the next slow poll.
  property bool statusPending: false
  property string stateSignature: ""
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
  readonly property var saves: Model.saves(root.state)
  readonly property bool saving: root.saves.length > 0
  readonly property bool busy: Model.isBusy(root.state)
  readonly property bool starting: Model.status(root.state) === "starting"
  readonly property string stage: String(root.state.stage || "")
  // The encoder is launched and warming up; the take is running even though
  // its state has not flipped to "recording" until the first frame lands.
  readonly property bool capturing: root.starting
    && (root.stage === "warmup" || root.stage === "capture")
  readonly property int elapsed: Model.elapsed(root.state, root.tick)

  function refresh() {
    if (!root.configured) return
    root.refreshRecent()
    root.refreshOutputDir()
    root.startStatus()
  }

  function startStatus() {
    if (!root.configured) return
    if (statusProc.running) {
      root.statusPending = true
      return
    }
    root.statusPending = false
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

  function applyState(text) {
    var previous = Model.status(root.state)
    var previousSaves = Model.saves(root.state)
    var parsed = Model.parseStateSafe(text)
    var signature = JSON.stringify(parsed)
    root.stateText = String(text || "")
    if (signature === root.stateSignature) return
    root.stateSignature = signature
    root.state = parsed
    var next = Model.status(parsed)
    var nextSaves = Model.saves(parsed)
    if (previous !== next || previousSaves.length !== nextSaves.length) {
      root.tick = 0
      // A save that just landed is not in the history yet. Refresh on the
      // transition so the placeholder row is replaced in the same beat.
      if (next === "idle" || next === "finalizing"
        || nextSaves.length < previousSaves.length) {
        root.refreshRecent()
        root.refreshOutputDir()
      }
    }
  }

  function applyStatus(text) {
    root.applyState(text)
  }

  // `reel status` enriches state.json with the save jobs and computed fields,
  // but it is a process on PATH. When that call cannot run or misses its
  // deadline, the watched file still knows the recorder's real status, so use
  // it instead of leaving the panel on an old idle state. Keep the save jobs
  // the last successful status carried.
  function applyFileState(text) {
    var parsed = Model.parseStateSafe(text)
    var jobs = Model.saves(root.state)
    if (jobs.length > 0 && !Array.isArray(parsed.saves)) parsed.saves = jobs
    root.applyState(JSON.stringify(parsed))
  }

  function applyRecent(text) {
    root.recents = Model.parseRecent(text)
    root.recentVersion++
  }

  // A replacement bar cannot reach this object through its service-less
  // facade. Read the live properties here instead; the panel polls the reader
  // and copies only what changed. The objects are read-only: this service
  // replaces them rather than mutating them in place.
  function bridgeSnapshot() {
    return {
      state: root.state,
      recents: root.recents,
      elapsed: root.elapsed,
      recentVersion: root.recentVersion,
      outputDir: root.outputDir,
      lastError: root.lastError,
      configureFailed: root.configureFailed
    }
  }

  // The panel may ask for work through the bridge. Keep this list closed:
  // Bridge.js rejects every name not present here.
  function bridgeActions() {
    return {
      refresh: function() { root.refresh() },
      retryConfigure: function() { root.retryConfigure() }
    }
  }

  // First-run bootstrap: the plugin id is stable, so the installed path is
  // deterministic. The configure step links the shipped engine's wrapper into
  // ~/.local/bin; nothing else runs until it exits. A start failure or a hung
  // configure must not leave every command gated forever, so it becomes a
  // visible, retryable error instead.
  Component.onCompleted: {
    root.barBridge = Bridge.publish(function() { return root.bridgeSnapshot() },
      root.bridgeActions())
    root.startConfigure()
  }

  Component.onDestruction: Bridge.clear(root.barBridge)

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
    if (label === "recent") root.recentPending = false
    if (label === "status" && root.fileStateText !== "")
      root.applyFileState(root.fileStateText)
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
  // nothing runs, and once a second while a take or save job is live so the
  // bar clock and saving rows stay honest. The FileView below turns the
  // engine's writes into an immediate poll, so a new take does not wait for
  // the slow idle interval.
  Timer {
    id: statePoll
    interval: root.active || root.starting || root.finalizing || root.saving ? 1000 : 5000
    repeat: true
    running: root.configured
    onTriggered: root.startStatus()
  }

  // The engine writes state.json on every transition (select, warmup,
  // countdown, recording, pause). Watching it lets the bar and panel react in
  // milliseconds instead of waiting up to five seconds for the next poll.
  // The content still comes from `reel status`, whose Rust reader is bounded.
  FileView {
    id: stateFile
    path: root.statePath
    watchChanges: true
    printErrors: false
    // Atomic state writes replace the file, so reload to watch the new one.
    onFileChanged: reload()
    onLoaded: {
      root.fileStateText = stateFile.text()
      // The poll is the richer answer; keep it running. The watched text is
      // the fallback if that process cannot start or misses its deadline.
      if (root.configured) root.startStatus()
    }
    onLoadFailed: {
      root.fileStateText = ""
      // The poll timer retries; the runtime file may not exist until the
      // first take.
    }
  }

  // FileView cannot watch a path that does not exist yet, and the first take
  // creates it. Keep reloading until it loads, like the countdown overlay.
  Timer {
    interval: 1000
    repeat: true
    running: root.configured && !stateFile.loaded
    onTriggered: stateFile.reload()
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
      if (!statusProc.hasStarted && root.fileStateText !== "")
        root.applyFileState(root.fileStateText)
      statusProc.hasStarted = false
      // A state write landed while this answer was in flight. Ask again now
      // instead of waiting for the next poll, which can be five seconds away.
      if (root.statusPending) root.startStatus()
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
