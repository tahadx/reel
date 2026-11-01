import QtQuick
import QtTest
import "../Model.js" as Model

TestCase {
  name: "HistoryActions"

  function test_playsInMpvWhenPresent() {
    compare(Model.playCommand("/videos/reel.mp4"), [
      "mpv", "--force-window=yes", "--", "/videos/reel.mp4"
    ])
  }

  function test_playIgnoresRowsWithoutAPath() {
    compare(Model.playCommand(""), [])
    compare(Model.playCommand(null), [])
  }

  function test_cutAlwaysUsesOmacut() {
    compare(Model.cutCommand("/videos/reel.mp4", true), [
      "omacut", "/videos/reel.mp4"
    ])
  }

  function test_cutNeedsOmacut() {
    compare(Model.cutCommand("/videos/reel.mp4", false), [])
    compare(Model.cutCommand("", true), [])
    compare(Model.cutCommand(null, true), [])
  }
}
