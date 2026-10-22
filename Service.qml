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
  readonly property string runtimeDir: {
    var runtime = Quickshell.env("XDG_RUNTIME_DIR") || ""
    if (runtime) return runtime + "/reel"
    return (Quickshell.env("XDG_STATE_HOME") || root.home + "/.local/state") + "/omarchy/reel/run"
  }
  readonly property string statePath: root.runtimeDir + "/state.json"

  property string stateText: ""
  property var state: Model.emptyState()
  property var recents: []
  property string outputDir: ""
  property string lastError: ""
  property int tick: 0
  property bool recentPending: false
  property string stateSignature: ""
  // True once the first-run configure step (the ~/.local/bin link) has
  // finished. Commands must not run before that, or a fresh install flashes a
  // "not on PATH" error while bin/configure is still linking the wrapper.
  property bool configured: false
  // Configure's own error is the useful one on a fresh install; later command
  // failures must not replace it with a generic "not on PATH" message.
  property bool configureFailed: false

  readonly property bool active: Model.isActive(root.state)
  readonly property bool busy: Model.isBusy(root.state)
  readonly property bool starting: Model.status(root.state) === "starting"
  readonly property int elapsed: Model.elapsed(root.state, root.tick)

  function refresh() {
    if (!root.configured) return
    root.refreshRecent()
    root.refreshOutputDir()
    if (!statusProc.running) statusProc.running = true
  }

  function refreshRecent() {
    if (!root.configured) return
    if (recentProc.running) {
      root.recentPending = true
      return
    }
    root.recentPending = false
    recentProc.running = true
  }

  function refreshOutputDir() {
    if (!root.configured) return
    if (outputProc.running) return
    outputProc.running = true
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
  }

  // First-run bootstrap: the plugin id is stable, so the installed path is
  // deterministic. The configure step links the shipped engine's wrapper into
  // ~/.local/bin; nothing else runs until it exits.
  Component.onCompleted: {
    configureProc.command = [
      root.home + "/.config/omarchy/plugins/io.github.tahadx.reel/bin/configure"
    ]
    configureProc.running = true
  }

  Process {
    id: configureProc
    running: false
    stderr: StdioCollector {
      id: configureError
      waitForEnd: true
    }
    onExited: function(code) {
      var message = String(configureError.text || "").trim()
      if (code !== 0) root.configureFailed = true
      if (message !== "") {
        root.lastError = message
      } else if (code !== 0) {
        root.lastError = "Reel could not configure its command. Run the plugin's bin/configure for details."
      }
      root.configured = true
      root.refresh()
    }
  }

  FileView {
    id: stateFile
    path: root.statePath
    watchChanges: true
    printErrors: false
    onFileChanged: reload()
    onLoaded: root.applyState(stateFile.text())
    onLoadFailed: root.applyState("")
  }

  Process {
    id: statusProc
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
    onExited: function(code) {
      // Configure owns the first failure message. Only report a missing
      // command once configure has succeeded (or left no error of its own).
      if (code === 127 && root.configured && root.lastError === "")
        root.lastError = "The reel command is not on PATH."
    }
  }

  Process {
    id: recentProc
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
    onExited: function(code) {
      if (code === 127 && root.configured && root.lastError === "")
        root.lastError = "The reel command is not on PATH."
      if (root.recentPending) {
        root.recentPending = false
        root.refreshRecent()
      }
    }
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
  }

  // One tick a second is enough for the bar clock and costs almost nothing.
  Timer {
    id: tickTimer
    interval: 1000
    repeat: true
    running: root.active
    onTriggered: root.tick++
  }

  // The watcher normally sees every write. This poll closes the one gap that
  // matters: a recorder that died and left its state file behind.
  Timer {
    id: pollTimer
    interval: 3000
    repeat: true
    running: root.active || root.starting
    onTriggered: if (root.configured && !statusProc.running) statusProc.running = true
  }
}
