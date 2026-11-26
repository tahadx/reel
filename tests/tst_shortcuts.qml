import QtQuick
import QtTest
import "../Shortcuts.js" as Shortcuts

TestCase {
  name: "Shortcuts"

  readonly property string binds: JSON.stringify([
    { mouse: false, submap: "", modmask: 64, key: "K", description: "Keybindings" },
    { mouse: false, submap: "", modmask: 65, key: "RETURN", description: "Browser" },
    { mouse: false, submap: "", modmask: 72, key: "F", description: "Full width" },
    { mouse: false, submap: "", modmask: 0, key: "PRINT", description: "Screenshot" },
    { mouse: false, submap: "", modmask: 64, key: "X", description: "" },
    { mouse: true, submap: "", modmask: 64, key: "Y", description: "Mouse bind" },
    { mouse: false, submap: "resize", modmask: 64, key: "Z", description: "Submap bind" },
    { mouse: false, submap: "", modmask: 0, key: "Escape", non_consuming: true, description: "Superwhisper: cancel" },
    { mouse: false, submap: "", modmask: 4, key: "SPACE", non_consuming: true, description: "Direct input" },
    { mouse: false, submap: "", modmask: 8, key: "Alt_R", release: true, non_consuming: true, description: "Stop dictation" }
  ])

  readonly property string keymap: "xkb_keymap {\nxkb_keycodes \"evdev\" {\n"
    + "  <AC09> = 45;\n  <RTRN> = 36;\n  <LFSH> = 50;\n};\n"
    + "xkb_symbols \"pc\" {\n"
    + "  key <AC09> { [ l, L ] };\n"
    + "  key <RTRN> { [ Return ] };\n"
    + "  key <LFSH> { [ Shift_L ] };\n};\n};\n"

  function test_comboKeysOrdersModifiersLikeOmarchy() {
    compare(Shortcuts.comboKeys(64, "k"), "SUPER + K")
    compare(Shortcuts.comboKeys(69, "SPACE"), "SUPER SHIFT CTRL + SPACE")
    compare(Shortcuts.comboKeys(72, "f"), "SUPER ALT + F")
    compare(Shortcuts.comboKeys(0, "PRINT"), "PRINT")
    compare(Shortcuts.comboKeys(64, ""), "")
  }

  function test_parseBindsDropsUnusableRows() {
    var list = Shortcuts.parseBinds(binds)
    compare(list.length, 5)
    compare(list[0].keys, "SUPER + K")
    compare(list[0].label, "Keybindings")
    compare(list[1].keys, "SUPER SHIFT + RETURN")
    compare(list[3].keys, "PRINT")
    // Passive bare-key hooks and release halves are app events, not shortcuts.
    compare(list.filter(function(b) { return b.key === "ESCAPE" }).length, 0)
    compare(list.filter(function(b) { return b.key === "ALT_R" }).length, 0)
    // A modified non-consuming bind is still a desktop shortcut.
    compare(list[4].keys, "CTRL + SPACE")
    compare(list[4].label, "Direct input")
  }

  function test_parseBindsSurvivesGarbage() {
    compare(Shortcuts.parseBinds(""), [])
    compare(Shortcuts.parseBinds("{"), [])
    compare(Shortcuts.parseBinds("null"), [])
  }

  function test_parseKeymapReadsBothSections() {
    var map = Shortcuts.parseKeymap(keymap).symbolByCode
    compare(map[45], "L")
    compare(map[36], "RETURN")
    compare(map[50], "SHIFT_L")
  }

  function test_buildMapUsesOnlyXkbKeycodes() {
    var lines = Shortcuts.buildMap(binds, keymap).trim().split("\n")
    verify(lines.indexOf("M\t50\t1") >= 0)
    verify(lines.indexOf("S\t45\tL") >= 0)
    verify(lines.indexOf("B\t64\tK\tSUPER + K\tKeybindings") >= 0)
    verify(lines.indexOf("B\t65\tRETURN\tSUPER SHIFT + RETURN\tBrowser") >= 0)
    // Shifted duplicates would collide with a different real keycode.
    verify(lines.indexOf("S\t37\tL") < 0)
    verify(lines.indexOf("M\t42\t1") < 0)
    // Unlabelled, mouse and submap binds never reach the hook.
    compare(lines.filter(function(line) { return line.indexOf("\tX\t") >= 0 }).length, 0)
    compare(lines.filter(function(line) { return line.indexOf("\tY\t") >= 0 }).length, 0)
    compare(lines.filter(function(line) { return line.indexOf("\tZ\t") >= 0 }).length, 0)
  }

  function test_parseEventSplitsKeysAndAction() {
    var event = Shortcuts.parseEvent("7\tSUPER + K\tKeybindings")
    compare(event.seq, "7")
    compare(event.keys, "SUPER + K")
    compare(event.label, "Keybindings")
    compare(Shortcuts.parseEvent("").keys, "")
  }

  function test_cleanCollapsesSeparators() {
    compare(Shortcuts.clean("  a\t\tb\n c ", 10), "a b c")
    compare(Shortcuts.clean("abcdef", 3), "abc")
  }
}
