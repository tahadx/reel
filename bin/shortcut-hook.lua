-- Reel shortcut overlay hook.
--
-- Registered at runtime with `hyprctl eval 'dofile(".../bin/shortcut-hook.lua")'`
-- while a take is recording. It reads the TSV built by Shortcuts.js, watches
-- raw key events, and writes only recognised Omarchy shortcuts to a one-line
-- event file. No keystrokes or text are logged.

local runtime = os.getenv("XDG_RUNTIME_DIR") or ""
if runtime == "" then return end
local base = runtime .. "/reel"
local mapPath = base .. "/shortcut-map.tsv"
local eventPath = base .. "/shortcut-event"

local modifierBit = {}
local symbol = {}
local bind = {}
local mapFile = io.open(mapPath, "r")
if mapFile then
  for line in mapFile:lines() do
    local kind, first, second, rest = line:match("^([MSB])\t([^\t]+)\t([^\t]+)\t?(.*)$")
    if kind == "M" then
      modifierBit[tonumber(first)] = tonumber(second)
    elseif kind == "S" then
      symbol[tonumber(first)] = second
    elseif kind == "B" then
      local keys, label = rest:match("^([^\t]+)\t(.+)$")
      if keys then
        bind[first .. "\t" .. second] = keys .. "\t" .. label
      end
    end
  end
  mapFile:close()
end

local held = {}
local down = {}
local sequence = 0

-- Sum unique bits: both shifts can be down at once and must count once.
local function currentMask()
  local total, seen = 0, {}
  for _code, bit in pairs(held) do
    if bit and not seen[bit] then
      seen[bit] = true
      total = total + bit
    end
  end
  return total
end

if _G.reel_shortcuts then
  _G.reel_shortcuts:remove()
  _G.reel_shortcuts = nil
end

local function onKey(keycode, _time, state)
  local code = tonumber(keycode) or 0
  local pressed = tonumber(state) == 1

  if modifierBit[code] then
    if pressed then held[code] = modifierBit[code] else held[code] = nil end
    return
  end

  if not pressed then
    down[code] = nil
    return
  end
  if down[code] then return end -- key repeat
  down[code] = true

  local key = symbol[code]
  if not key then return end
  local combo = bind[tostring(currentMask()) .. "\t" .. key]
  if not combo then return end

  sequence = sequence + 1
  local event = io.open(eventPath, "w")
  if event then
    event:write(tostring(sequence) .. "\t" .. combo .. "\n")
    event:close()
  end
end

-- Exposed for offline checks of the hook without sending real key events.
_G.reel_shortcut_handler = onKey
_G.reel_shortcuts = hl.on("input.keyboard.key", onKey)

return nil
