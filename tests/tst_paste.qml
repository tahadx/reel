import QtQuick
import QtTest
import ".." as Reel
import "../Model.js" as Model

TestCase {
  name: "Paste"
  when: windowShown
  width: 400
  height: 100

  Reel.GuardedInput {
    id: guarded
    x: 10
    y: 10
    width: 200
    height: 30
    font.pixelSize: 16
    focus: true
  }

  SignalSpy {
    id: pasteSpy
    target: guarded
    signalName: "boundedPaste"
  }

  function init() {
    guarded.text = ""
    guarded.forceActiveFocus()
    pasteSpy.clear()
  }

  function test_dedicatedPasteKeyIsIntercepted() {
    // Qt maps Qt::Key_Paste to QKeySequence::Paste for every platform. If the
    // handler does not run, the native TextInput would fetch the clipboard
    // synchronously.
    keyClick(Qt.Key_Paste)
    compare(pasteSpy.count, 1)
  }

  function test_ctrlVIsIntercepted() {
    keyClick(Qt.Key_V, Qt.ControlModifier)
    compare(pasteSpy.count, 1)
  }

  function test_shiftInsertIsIntercepted() {
    keyClick(Qt.Key_Insert, Qt.ShiftModifier)
    compare(pasteSpy.count, 1)
  }

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
