import QtQuick
import QtTest
import "../Model.js" as Model

TestCase {
  name: "HistoryActions"

  function test_opensInOmacutWhenAvailable() {
    compare(Model.openVideoCommand("/videos/reel.mp4", true), [
      "omacut", "/videos/reel.mp4"
    ])
  }

  function test_fallsBackToMpvWhenOmacutIsMissing() {
    compare(Model.openVideoCommand("/videos/reel.mp4", false), [
      "mpv", "--force-window=yes", "--", "/videos/reel.mp4"
    ])
  }

  function test_ignoresRowsWithoutAPath() {
    compare(Model.openVideoCommand("", true), [])
    compare(Model.openVideoCommand(null, false), [])
  }
}
