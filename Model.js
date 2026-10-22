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
  case "starting": return "Choose what to record"
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
