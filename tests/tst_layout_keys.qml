import QtQuick
import QtTest
import "../Model.js" as Model

TestCase {
  name: "LayoutKeys"

  // xkb keycodes (evdev + 8) for the US QWERTY letter keys used below.
  readonly property int keyR: 27
  readonly property int keyH: 43
  readonly property int keyA: 38
  readonly property int keyU: 30
  readonly property int keyM: 58

  function test_asciiKeepsItsOwnText() {
    compare(Model.shortcutText("r", keyR), "r")
    // Dvorak types "p" on the QWERTY R key; the layout's letter wins.
    compare(Model.shortcutText("p", keyR), "p")
    compare(Model.shortcutText("1", 10), "1")
  }

  function test_cyrillicFollowsThePosition() {
    compare(Model.shortcutText("к", keyR), "r")   // Russian
    compare(Model.shortcutText("р", keyH), "h")
    compare(Model.shortcutText("ф", keyA), "a")
    compare(Model.shortcutText("г", keyU), "u")
  }

  function test_caseIsKept() {
    compare(Model.shortcutText("Р", keyH), "H")   // Uppercase mapping is kept
    compare(Model.shortcutText("Г", keyU), "U")   // Shift+U is the updater
  }

  function test_otherScriptsFollowThePosition() {
    compare(Model.shortcutText("ρ", keyR), "r")   // Greek
    compare(Model.shortcutText("ר", keyR), "r")   // Hebrew
    compare(Model.shortcutText("ة", keyM), "m")   // Arabic
  }

  function test_keysOutsideTheLetterRowsAreUntouched() {
    compare(Model.shortcutText("х", 34), "х")     // Russian on the [ key
    compare(Model.shortcutText("é", 11), "é")     // AZERTY on the 2 key
  }

  function test_emptyAndMissingInput() {
    compare(Model.shortcutText("", keyR), "")
    compare(Model.shortcutText(undefined, keyR), "")
    compare(Model.shortcutText("к", undefined), "к")
  }
}
