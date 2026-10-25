.pragma library

// Pure helpers for Reel. The panel and the background service both import
// this file, so nothing here touches QML objects or the filesystem.

function emptyState() {
  return { status: "idle", elapsed: 0, active: false }
}

function emptyRecent() {
  return { recordings: [] }
}

function object(value) {
  return value && typeof value === "object" && !Array.isArray(value) ? value : {}
}

function parseState(text) {
  var raw = object(JSON.parse(String(text || "{}") || "{}"))
  // Old builds stopped at a preflight state; treat it as idle now.
  if (!raw.status || raw.status === "ready") raw.status = "idle"
  return raw
}

function parseStateSafe(text) {
  try {
    return parseState(text)
  } catch (e) {
    return emptyState()
  }
}

function parseRecent(text) {
  try {
    var payload = object(JSON.parse(String(text || "{}") || "{}"))
    var list = payload.recordings
    if (!Array.isArray(list)) return []
    var clean = []
    for (var i = 0; i < list.length; i++) {
      var row = object(list[i])
      if (typeof row.path === "string" && row.path.length > 0) clean.push(row)
    }
    return clean
  } catch (e) {
    return []
  }
}

function number(value, fallback) {
  var n = Number(value)
  return isFinite(n) ? n : (fallback || 0)
}

function status(state) {
  return String(object(state).status || "idle")
}

function isActive(state) {
  var s = status(state)
  return s === "recording" || s === "paused"
}

function isBusy(state) {
  var s = status(state)
  return s === "starting" || s === "recording" || s === "paused" || s === "finalizing"
}

function canPause(state) {
  return isActive(state)
}

function elapsed(state, tick) {
  // `tick` is read only so QML re-evaluates this once a second while the
  // service's clock runs.
  tick = tick || 0
  var s = object(state)
  var now = Math.floor(Date.now() / 1000)
  var started = number(s.startedAt, now)
  var pausedTotal = number(s.pausedTotal, 0)
  var value
  if (status(s) === "paused") value = number(s.pausedAt, now) - started - pausedTotal
  else value = now - started - pausedTotal
  return Math.max(0, Math.floor(value))
}

function pad(value) {
  var n = Math.floor(number(value, 0))
  return n < 10 ? "0" + n : String(n)
}

function clock(seconds) {
  var total = Math.max(0, Math.floor(number(seconds, 0)))
  var hours = Math.floor(total / 3600)
  var minutes = Math.floor((total % 3600) / 60)
  var secs = total % 60
  return hours > 0 ? hours + ":" + pad(minutes) + ":" + pad(secs) : minutes + ":" + pad(secs)
}

function duration(seconds) {
  var total = Math.max(0, Math.floor(number(seconds, 0)))
  var hours = Math.floor(total / 3600)
  var minutes = Math.floor((total % 3600) / 60)
  var secs = total % 60
  return hours > 0 ? hours + ":" + pad(minutes) + ":" + pad(secs) : minutes + ":" + pad(secs)
}

function bytes(value) {
  var n = Math.max(0, number(value, 0))
  if (n < 1024) return Math.round(n) + " B"
  var units = ["KB", "MB", "GB", "TB"]
  var index = -1
  do {
    n /= 1024
    index++
  } while (n >= 1024 && index < units.length - 1)
  return (n >= 10 ? Math.round(n) : n.toFixed(1)) + " " + units[index]
}

function relative(epoch) {
  var then = number(epoch, 0)
  if (then <= 0) return ""
  var seconds = Math.max(0, Math.floor(Date.now() / 1000) - then)
  if (seconds < 60) return "just now"
  if (seconds < 3600) return Math.floor(seconds / 60) + "m ago"
  if (seconds < 86400) return Math.floor(seconds / 3600) + "h ago"
  if (seconds < 604800) return Math.floor(seconds / 86400) + "d ago"
  return Math.floor(seconds / 604800) + "w ago"
}

function baseName(path) {
  var value = String(path || "")
  var slash = value.lastIndexOf("/")
  return slash >= 0 ? value.slice(slash + 1) : value
}

function parentPath(path) {
  var value = String(path || "")
  var slash = value.lastIndexOf("/")
  return slash > 0 ? value.slice(0, slash) : value
}

function shortPath(path, home) {
  var value = String(path || "")
  var base = String(home || "")
  if (base && value === base) return "~"
  if (base && value.indexOf(base + "/") === 0) return "~" + value.slice(base.length)
  return value
}

function stateTitle(state) {
  switch (status(state)) {
  case "starting":
    return String(object(state).stage || "") === "capture" ? "Recording" : "Choose what to record"
  case "recording": return "Recording"
  case "paused": return "Paused"
  case "finalizing": return "Saving the take"
  default: return "Ready to record"
  }
}

function stateMeta(state, elapsedSeconds) {
  var s = object(state)
  switch (status(s)) {
  case "starting":
    if (String(s.stage || "") === "capture") return clock(elapsedSeconds) + " · recording"
    return String(s.stage || "select") === "countdown"
      ? "starting in a moment" : "pick a window, area, or monitor"
  case "recording":
    return clock(elapsedSeconds) + " · " + (s.fps ? number(s.fps) + " fps" : "recording")
  case "paused":
    return "paused at " + clock(elapsedSeconds)
  case "finalizing":
    return "trimming and normalizing audio"
  default:
    return "ready · " + (s.fps ? number(s.fps) + " fps" : "60 fps")
  }
}

function takeTitle(row) {
  var name = baseName(object(row).path)
  return name || "Recording"
}

function takeMeta(row) {
  var r = object(row)
  var parts = []
  if (number(r.duration, 0) > 0) parts.push(duration(r.duration))
  if (number(r.size, 0) > 0) parts.push(bytes(r.size))
  var when = relative(r.mtime || r.finishedAt || 0)
  if (when) parts.push(when)
  return parts.join(" · ")
}

function truthy(value) {
  if (value === true || value === 1) return true
  var text = String(value === undefined || value === null ? "" : value).toLowerCase()
  return text === "true" || text === "on" || text === "yes" || text === "1" || text === "enabled"
}

function cycle(list, current, direction) {
  if (!Array.isArray(list) || list.length === 0) return current
  var index = list.indexOf(current)
  if (index < 0) index = 0
  var step = direction < 0 ? -1 : 1
  return list[(index + step + list.length) % list.length]
}

function indexOf(list, value) {
  return Array.isArray(list) ? list.indexOf(value) : -1
}

function clamp(value, low, high) {
  return Math.max(low, Math.min(high, value))
}

// ---------- capture preview geometry -------------------------------------

// The picker and the compositor both speak logical desktop coordinates.
// ScreencopyView reports the output's physical buffer separately, so keep the
// two spaces apart: geometry below is logical, sourceSize is physical.

function parseTarget(target) {
  var value = String(target || "")
  if (value === "portal") return { kind: "portal", target: value }
  if (value.indexOf("monitor:") === 0) {
    var name = value.slice(8)
    return name === "" ? null : { kind: "monitor", target: value, name: name }
  }
  var match = /^region:(\d+)x(\d+)\+(-?\d+)\+(-?\d+)$/.exec(value)
  if (!match) return null
  var width = Number(match[1])
  var height = Number(match[2])
  var x = Number(match[3])
  var y = Number(match[4])
  if (!(width > 0) || !(height > 0)) return null
  return { kind: "region", target: value, x: x, y: y, width: width, height: height }
}

function screenRect(screen) {
  if (!screen) return null
  var x = Number(screen.x)
  var y = Number(screen.y)
  var width = Number(screen.width)
  var height = Number(screen.height)
  if (!isFinite(x) || !isFinite(y) || !(width > 0) || !(height > 0)) return null
  return {
    name: String(screen.name || ""),
    x: x,
    y: y,
    width: width,
    height: height,
    object: screen
  }
}

function screenRects(screens) {
  var out = []
  // Quickshell.screens is an array-like model, not a JavaScript Array.
  var list = screens && typeof screens.length === "number" ? screens : []
  for (var i = 0; i < list.length; i++) {
    var rect = screenRect(list[i])
    if (rect) out.push(rect)
  }
  return out
}

function screenNamed(screens, name) {
  var list = screenRects(screens)
  for (var i = 0; i < list.length; i++) {
    if (list[i].name === String(name || "")) return list[i]
  }
  return null
}

function rectContains(outer, inner) {
  return !!outer && !!inner
    && inner.x >= outer.x
    && inner.y >= outer.y
    && inner.x + inner.width <= outer.x + outer.width
    && inner.y + inner.height <= outer.y + outer.height
}

function rectIntersection(a, b) {
  if (!a || !b) return null
  var left = Math.max(a.x, b.x)
  var top = Math.max(a.y, b.y)
  var right = Math.min(a.x + a.width, b.x + b.width)
  var bottom = Math.min(a.y + a.height, b.y + b.height)
  if (right <= left || bottom <= top) return null
  return { x: left, y: top, width: right - left, height: bottom - top }
}

function screensIntersecting(screens, area) {
  var out = []
  var list = screenRects(screens)
  for (var i = 0; i < list.length; i++) {
    if (rectIntersection(list[i], area)) out.push(list[i])
  }
  return out
}

function screenContaining(screens, area) {
  var list = screenRects(screens)
  for (var i = 0; i < list.length; i++) {
    if (rectContains(list[i], area)) return list[i]
  }
  return null
}

function unavailablePlan(target, kind, title, area, reason) {
  return {
    mode: "capture",
    kind: String(kind || ""),
    target: String(target || ""),
    title: title,
    area: area || null,
    screenRect: null,
    source: null,
    unavailable: true,
    reason: reason
  }
}

// Resolve the saved recording target against the screens that are connected
// now. A region that is not wholly inside one screen cannot be represented by
// a single source, so it is refused instead of being silently clamped.
function capturePlan(target, screens, kind) {
  var value = String(target || "")
  var parsed = parseTarget(value)
  if (!parsed) {
    return unavailablePlan(value, kind, "Capture target", null,
      "The saved target is malformed. Choose the target again.")
  }
  if (parsed.kind === "portal") {
    return unavailablePlan(value, kind || "portal", "Portal capture", null,
      "Portal capture is chosen in the system picker. An exact live preview is unavailable.")
  }
  if (parsed.kind === "monitor") {
    var monitor = screenNamed(screens, parsed.name)
    if (!monitor) {
      return unavailablePlan(value, kind || "monitor", parsed.name, null,
        parsed.name + " is not connected. Choose another monitor.")
    }
    return {
      mode: "capture",
      kind: kind || "monitor",
      target: value,
      title: monitor.name,
      area: { x: monitor.x, y: monitor.y, width: monitor.width, height: monitor.height },
      screenRect: monitor,
      source: monitor.object,
      unavailable: false,
      reason: ""
    }
  }

  var area = { x: parsed.x, y: parsed.y, width: parsed.width, height: parsed.height }
  var title = kind === "window" ? "Window area" : "Selected area"
  var containing = screenContaining(screens, area)
  if (containing) {
    return {
      mode: "capture",
      kind: kind || "region",
      target: value,
      title: title,
      area: area,
      screenRect: containing,
      source: containing.object,
      unavailable: false,
      reason: ""
    }
  }

  var hits = screensIntersecting(screens, area)
  if (hits.length > 1) {
    return unavailablePlan(value, kind, title, area,
      "Selection crosses monitor boundaries. An exact live preview is unavailable.")
  }
  if (hits.length === 1) {
    return unavailablePlan(value, kind, title, area,
      "Selection extends beyond " + hits[0].name + ". An exact live preview is unavailable.")
  }
  return unavailablePlan(value, kind, title, area,
    "Selected area is not on a connected monitor.")
}

function idlePlan(screens, focusedName) {
  var monitor = screenNamed(screens, focusedName)
  if (!monitor) {
    var list = screenRects(screens)
    monitor = list.length > 0 ? list[0] : null
  }
  if (!monitor) {
    return {
      mode: "idle",
      kind: "monitor",
      target: "",
      title: "Full monitor preview",
      area: null,
      screenRect: null,
      source: null,
      unavailable: true,
      reason: "No connected monitor is available for a preview."
    }
  }
  return {
    mode: "idle",
    kind: "monitor",
    target: "",
    title: "Full monitor preview",
    area: { x: monitor.x, y: monitor.y, width: monitor.width, height: monitor.height },
    screenRect: monitor,
    source: monitor.object,
    unavailable: false,
    reason: ""
  }
}

function fitPreview(width, height, maxWidth, maxHeight) {
  var w = Number(width)
  var h = Number(height)
  var maxW = Number(maxWidth)
  var maxH = Number(maxHeight)
  if (!(w > 0) || !(h > 0) || !(maxW > 0) || !(maxH > 0)) return null
  var scale = Math.min(maxW / w, maxH / h)
  if (!isFinite(scale) || !(scale > 0)) return null
  return { width: w * scale, height: h * scale, scale: scale }
}

// Fit the selected rectangle into the panel, then place the full source screen
// behind it. Both use the same scale, so the viewport is an exact crop rather
// than a stretched image.
function previewLayout(plan, maxWidth, maxHeight) {
  var p = object(plan)
  var area = p.area
  var hasArea = !!area && Number(area.width) > 0 && Number(area.height) > 0
  if (!hasArea) {
    var empty = fitPreview(16, 9, maxWidth, maxHeight)
    if (!empty) return null
    return {
      frameWidth: empty.width,
      frameHeight: empty.height,
      scale: empty.scale,
      sourceX: 0,
      sourceY: 0,
      sourceWidth: 0,
      sourceHeight: 0,
      hasSource: false
    }
  }

  var fit = fitPreview(area.width, area.height, maxWidth, maxHeight)
  if (!fit) return null
  var screen = p.screenRect
  var hasSource = !!screen && !!p.source
  var layout = {
    frameWidth: fit.width,
    frameHeight: fit.height,
    scale: fit.scale,
    sourceX: 0,
    sourceY: 0,
    sourceWidth: 0,
    sourceHeight: 0,
    hasSource: hasSource
  }
  if (hasSource) {
    layout.sourceX = (screen.x - area.x) * fit.scale
    layout.sourceY = (screen.y - area.y) * fit.scale
    layout.sourceWidth = screen.width * fit.scale
    layout.sourceHeight = screen.height * fit.scale
  }
  return layout
}

function span(start, end, scale) {
  return Math.max(0, Math.round(end * scale) - Math.round(start * scale))
}

// Dimensions the compositor will encode for the selected rectangle. The only
// reliable source is the real buffer size: Qt's devicePixelRatio can differ
// from the compositor's fractional scale, so never guess from it.
function captureSize(plan, sourceSize) {
  var p = object(plan)
  var area = p.area
  var screen = p.screenRect
  if (!area || !screen) return null

  var sourceWidth = Number(sourceSize && sourceSize.width)
  var sourceHeight = Number(sourceSize && sourceSize.height)
  if (!(sourceWidth > 0) || !(sourceHeight > 0)) return null
  var scaleX = sourceWidth / screen.width
  var scaleY = sourceHeight / screen.height

  var left = area.x - screen.x
  var top = area.y - screen.y
  return {
    width: span(left, left + area.width, scaleX),
    height: span(top, top + area.height, scaleY)
  }
}

// Omacut is the preferred trimmer. Keep mpv as a fallback so history stays
// useful when Omacut is missing or the preference is turned off.
function openVideoCommand(path, hasOmacut, openWithOmacut) {
  var value = String(path || "")
  if (value === "") return []
  return hasOmacut && openWithOmacut
    ? ["omacut", value]
    : ["mpv", "--force-window=yes", "--", value]
}
