import QtQuick
import QtTest
import "../Model.js" as Model

TestCase {
  name: "Paste"

  function test_plainTextFits() {
    var result = Model.pasteText("reel-demo", 200)
    compare(result.ok, true)
    compare(result.text, "reel-demo")
    compare(result.error, "")
  }

  function test_emptyPasteIsIgnored() {
    var result = Model.pasteText("", 200)
    compare(result.ok, true)
    compare(result.text, "")
  }

  function test_lineBreaksAndControlsAreRefused() {
    compare(Model.pasteText("one\ntwo", 200).ok, false)
    compare(Model.pasteText("one\ttwo", 200).ok, false)
    compare(Model.pasteText("one\u0000two", 200).ok, false)
    compare(Model.pasteText("one\u007ftwo", 200).ok, false)
  }

  function test_oversizedTextIsRefused() {
    var result = Model.pasteText("abcdef", 5)
    compare(result.ok, false)
    compare(result.text, "")
    compare(result.error.indexOf("5") >= 0, true)
    // Exactly at the limit is fine.
    compare(Model.pasteText("abcde", 5).ok, true)
  }
}
