import QtQuick
import QtTest
import "../Model.js" as Model

TestCase {
  name: "PanelModel"

  function test_scaledRadiusRespectsSharpMode() {
    compare(Model.scaledRadius(0, 4), 0)
    compare(Model.scaledRadius(-1, 4), 0)
    compare(Model.scaledRadius(0, 8), 0)
    compare(Model.scaledRadius(4, 4), 2)
    compare(Model.scaledRadius(8, 4), 4)
    compare(Model.scaledRadius(14, 4), 4)
  }

  function test_pillRadiusFollowsCornerRadius() {
    compare(Model.pillRadius(0, 16), 0)
    compare(Model.pillRadius(0, 22), 0)
    compare(Model.pillRadius(4, 16), 4)
    compare(Model.pillRadius(6, 16), 8)
    compare(Model.pillRadius(10, 16), 8)
    compare(Model.pillRadius(10, 22), 11)
    compare(Model.pillRadius(8, 12), 6)
  }

  function test_groupBySectionPreservesFlatIndices() {
    var rows = [
      { key: "desktopAudio", section: "Audio" },
      { key: "microphoneAudio" },
      { key: "noiseCancellation", dependent: true },
      { key: "fps", section: "Video" },
      { key: "quality" }
    ]
    var groups = Model.groupBySection(rows)
    compare(groups.length, 2)
    compare(groups[0].section, "Audio")
    compare(groups[0].rows.length, 3)
    compare(groups[0].rows[0].flatIndex, 0)
    compare(groups[0].rows[1].flatIndex, 1)
    compare(groups[0].rows[2].flatIndex, 2)
    compare(groups[0].rows[2].dependent, true)
    compare(groups[1].section, "Video")
    compare(groups[1].rows.length, 2)
    compare(groups[1].rows[0].flatIndex, 3)
    compare(groups[1].rows[1].flatIndex, 4)
  }

  function test_groupBySectionHandlesEmpty() {
    compare(Model.groupBySection([]), [])
    compare(Model.groupBySection(null), [])
  }

  function test_nextEnabledIndexSkipsDisabledDependentRows() {
    var rows = [
      { key: "desktopAudio" },
      { key: "microphoneAudio" },
      { key: "noiseCancellation" },
      { key: "fps" }
    ]
    var disabled = { noiseCancellation: true }
    compare(Model.nextEnabledIndex(rows, 1, 1, disabled), 3)
    compare(Model.nextEnabledIndex(rows, 3, -1, disabled), 1)
    compare(Model.nextEnabledIndex(rows, 0, -1, disabled), -1)
    compare(Model.nextEnabledIndex(rows, 3, 1, disabled), -1)
    compare(Model.nextEnabledIndex(rows, 1, 1, {}), 2)
  }
}
