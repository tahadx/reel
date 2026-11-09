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

  function test_savingRowShowsThePublishedName() {
    compare(Model.savingTitle("/videos/reel-2026-11-06_17-00-00.capture.mp4"),
      "reel-2026-11-06_17-00-00.mp4")
    compare(Model.savingTitle("/videos/reel-2026-11-06_17-00-00.mp4"),
      "reel-2026-11-06_17-00-00.mp4")
    compare(Model.publishedPath("/videos/reel-2026-11-06_17-00-00.capture.mp4"),
      "/videos/reel-2026-11-06_17-00-00.mp4")
    compare(Model.publishedPath(""), "")
    compare(Model.savingTitle(""), "Saving take")
    compare(Model.savingTitle(null), "Saving take")
  }

  function test_takeStemDropsOnlyTheExtension() {
    compare(Model.takeStem("/videos/reel-2026-11-06_17-00-00.mp4"),
      "reel-2026-11-06_17-00-00")
    compare(Model.takeStem("/videos/demo.mp4"), "demo")
    compare(Model.takeStem("/videos/no-extension"), "no-extension")
    compare(Model.takeStem(".hidden"), ".hidden")
    compare(Model.takeStem(""), "")
    compare(Model.takeStem(null), "")
  }

  function test_renamedPathKeepsTheExtension() {
    compare(Model.renamedPath("/videos/old.mp4", "demo"), "/videos/demo.mp4")
    compare(Model.renamedPath("/videos/old.mp4", "demo.mp4"), "/videos/demo.mp4")
    compare(Model.renamedPath("/videos/old.mp4", "demo.MP4"), "/videos/demo.mp4")
    compare(Model.renamedPath("/videos/old.mkv", "demo"), "/videos/demo.mkv")
    compare(Model.renamedPath("/videos/old.mp4", "  demo  "), "/videos/demo.mp4")
    compare(Model.renamedPath("/videos/old.mp4", ""), "/videos/old.mp4")
    compare(Model.renamedPath("", "demo"), "")
  }

  function test_withPathOnlyChangesTheDisplayedPath() {
    var row = { path: "/videos/old.mp4", preview: "/previews/old.jpg", duration: 4 }
    var renamed = Model.withPath(row, "/videos/demo.mp4")
    compare(renamed.path, "/videos/demo.mp4")
    compare(renamed.preview, "/previews/old.jpg")
    compare(renamed.duration, 4)
    compare(row.path, "/videos/old.mp4")
  }

  function test_visibleRecentsAppliesRenamesAndHidesPendingTrash() {
    var rows = [
      { path: "/videos/a.mp4", preview: "/previews/a.jpg" },
      { path: "/videos/b.mp4" },
      { path: "/videos/c.mp4" }
    ]
    var out = Model.visibleRecents(rows,
      { "/videos/b.mp4": true },
      { "/videos/a.mp4": "/videos/demo.mp4" })
    compare(out.length, 2)
    compare(out[0].path, "/videos/demo.mp4")
    compare(out[0].preview, "/previews/a.jpg")
    compare(out[1].path, "/videos/c.mp4")
    compare(rows[0].path, "/videos/a.mp4")
  }
}
