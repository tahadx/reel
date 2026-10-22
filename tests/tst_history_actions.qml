import QtQuick
import QtTest
import "../Model.js" as Model

TestCase {
  name: "HistoryActions"

  function test_opensInOmacutWhenAvailableAndEnabled() {
    compare(Model.openVideoCommand("/videos/reel.mp4", true, true), [
      "omacut", "/videos/reel.mp4"
    ])
  }

  function test_fallsBackToMpvWhenOmacutIsDisabled() {
    compare(Model.openVideoCommand("/videos/reel.mp4", true, false), [
      "mpv", "--force-window=yes", "--", "/videos/reel.mp4"
    ])
  }

  function test_fallsBackToMpvWhenOmacutIsMissing() {
    compare(Model.openVideoCommand("/videos/reel.mp4", false, true), [
      "mpv", "--force-window=yes", "--", "/videos/reel.mp4"
    ])
  }

  function test_ignoresRowsWithoutAPath() {
    compare(Model.openVideoCommand("", true, true), [])
    compare(Model.openVideoCommand(null, false, true), [])
    compare(Model.openVideoCommand(null, false, false), [])
  }
}
