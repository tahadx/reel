.pragma library

// Pure helpers for the shortcut overlay. Matching happens in the Hyprland Lua
// hook; this module only turns `hyprctl binds -j` and `xkbcli compile-keymap`
// into the small TSV the hook reads, and parses the one-line event it writes.

function clean(value, limit) {
  var text = String(value === undefined || value === null ? "" : value)
  text = text.replace(/[\t\r\n]+/g, " ").replace(/ +/g, " ").trim()
  if (limit > 0 && text.length > limit) text = text.slice(0, limit)
  return text
}

function comboKeys(mask, key) {
  var bits = Number(mask)
  if (!isFinite(bits) || bits < 0) bits = 0
  var mods = []
  // Omarchy lists modifiers in this order: SUPER SHIFT CTRL + KEY.
  if (bits & 64) mods.push("SUPER")
  if (bits & 1) mods.push("SHIFT")
  if (bits & 4) mods.push("CTRL")
  if (bits & 8) mods.push("ALT")
  var name = clean(key, 40).toUpperCase()
  if (name === "") return ""
  return (mods.length > 0 ? mods.join(" ") + " + " : "") + name
}

function parseBinds(text) {
  var data
  try {
    data = JSON.parse(String(text || ""))
  } catch (e) {
    return []
  }
  if (!Array.isArray(data)) return []

  var out = []
  for (var i = 0; i < data.length; i++) {
    var bind = data[i] || {}
    if (bind.mouse === true) continue
    if (String(bind.submap || "") !== "") continue
    var key = clean(bind.key, 40).toUpperCase()
    if (key === "") continue
    var label = clean(bind.description, 100)
    if (label === "") continue
    var keys = comboKeys(bind.modmask, key)
    if (keys === "") continue
    var mask = Number(bind.modmask)
    out.push({
      mask: isFinite(mask) && mask > 0 ? Math.floor(mask) : 0,
      key: key,
      keys: keys,
      label: label
    })
  }
  return out
}

function parseKeymap(text) {
  var codeByName = {}
  var symbolByCode = {}
  var lines = String(text || "").split("\n")
  var section = ""
  var openKey = ""

  for (var i = 0; i < lines.length; i++) {
    var line = lines[i]
    if (/xkb_keycodes/.test(line)) {
      section = "codes"
      continue
    }
    if (/xkb_symbols/.test(line)) {
      section = "syms"
      continue
    }
    if (section === "codes") {
      var codeMatch = line.match(/<([A-Za-z0-9_]+)>\s*=\s*(\d+)\s*;/)
      if (codeMatch) codeByName[codeMatch[1]] = parseInt(codeMatch[2], 10)
    } else if (section === "syms") {
      var keyMatch = line.match(/key\s*<([A-Za-z0-9_]+)>\s*\{\s*\[\s*([A-Za-z0-9_]+)/)
      if (keyMatch) {
        openKey = ""
        var code = codeByName[keyMatch[1]]
        if (code) symbolByCode[code] = clean(keyMatch[2], 40).toUpperCase()
      } else if (/key\s*<[A-Za-z0-9_]+>/.test(line)) {
        // Some layouts put the first symbol on the following line.
        var pending = line.match(/key\s*<([A-Za-z0-9_]+)>/)
        openKey = pending ? pending[1] : ""
      } else if (openKey !== "") {
        var symMatch = line.match(/symbols\[[^\]]*\]\s*=\s*\[\s*([A-Za-z0-9_]+)/)
        if (symMatch) {
          var pendingCode = codeByName[openKey]
          if (pendingCode) symbolByCode[pendingCode] = clean(symMatch[1], 40).toUpperCase()
          openKey = ""
        }
      }
    }
  }
  return { symbolByCode: symbolByCode }
}

// Hyprland reports XKB keycodes, the same space xkbcli compiles. Publishing
// an evdev-shifted duplicate would collide with a different real key, so the
// map uses only the XKB spelling.
function buildMap(bindsText, keymapText) {
  var binds = parseBinds(bindsText)
  var keymap = parseKeymap(keymapText)
  var symbols = keymap.symbolByCode
  var lines = []

  // XKB modifier keycodes, with their Hyprland modmask bits.
  var modifiers = [
    [50, 1], [62, 1], [66, 2], [37, 4], [105, 4],
    [64, 8], [108, 8], [85, 16], [133, 64], [134, 64]
  ]
  for (var m = 0; m < modifiers.length; m++) {
    var code = modifiers[m][0]
    var bit = modifiers[m][1]
    lines.push("M\t" + code + "\t" + bit)
  }

  for (var key in symbols) {
    if (!Object.prototype.hasOwnProperty.call(symbols, key)) continue
    var value = symbols[key]
    lines.push("S\t" + key + "\t" + value)
  }

  var seen = {}
  for (var i = 0; i < binds.length; i++) {
    var bind = binds[i]
    var combo = bind.mask + "\t" + bind.key
    if (seen[combo]) continue
    seen[combo] = true
    lines.push("B\t" + bind.mask + "\t" + bind.key + "\t"
      + clean(bind.keys, 80) + "\t" + clean(bind.label, 120))
  }
  return lines.join("\n") + "\n"
}

function parseEvent(text) {
  var fields = String(text || "").split("\t")
  return {
    seq: clean(fields[0], 20),
    keys: clean(fields[1], 80),
    label: clean(fields[2], 120)
  }
}
