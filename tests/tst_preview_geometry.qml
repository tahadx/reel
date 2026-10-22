import QtQuick
import QtTest
import "../Model.js" as Model

TestCase {
  name: "PreviewGeometry"

  readonly property var screens: [
    { name: "DP-1", x: 0, y: 0, width: 2560, height: 1440, devicePixelRatio: 1 },
    { name: "DP-2", x: 2560, y: -200, width: 1920, height: 1080, devicePixelRatio: 1.5 },
    { name: "eDP-1", x: -1728, y: 0, width: 1728, height: 972, devicePixelRatio: 1.6666666 }
  ]

  function test_parseTarget() {
    compare(Model.parseTarget("monitor:DP-1"), {
      kind: "monitor", target: "monitor:DP-1", name: "DP-1"
    })
    compare(Model.parseTarget("region:1280x720+-100+-40"), {
      kind: "region",
      target: "region:1280x720+-100+-40",
      x: -100,
      y: -40,
      width: 1280,
      height: 720
    })
    compare(Model.parseTarget("portal"), { kind: "portal", target: "portal" })
    compare(Model.parseTarget("region:0x720+0+0"), null)
    compare(Model.parseTarget("region:1280x720+10"), null)
    compare(Model.parseTarget(""), null)
  }

  function test_monitorPlanUsesSelectedScreen() {
    var plan = Model.capturePlan("monitor:DP-1", screens, "monitor")
    verify(!plan.unavailable)
    compare(plan.source, screens[0])
    compare(plan.area, { x: 0, y: 0, width: 2560, height: 1440 })
    compare(plan.screenRect.name, "DP-1")

    var missing = Model.capturePlan("monitor:HDMI-9", screens, "monitor")
    verify(missing.unavailable)
    compare(missing.source, null)
    verify(missing.reason.indexOf("not connected") >= 0)
  }

  function test_regionOnSecondaryWithNegativeOrigin() {
    var plan = Model.capturePlan("region:800x600+2700+-150", screens, "window")
    verify(!plan.unavailable)
    compare(plan.source, screens[1])
    compare(plan.kind, "window")
    compare(plan.area, { x: 2700, y: -150, width: 800, height: 600 })
  }

  function test_acceptsArrayLikeScreenModel() {
    var model = { length: 2, 0: screens[0], 1: screens[1] }
    var plan = Model.capturePlan("region:800x600+2700+-150", model, "window")
    verify(!plan.unavailable)
    compare(plan.source, screens[1])
  }

  function test_regionMustBeContainedByOneScreen() {
    var crossing = Model.capturePlan("region:800x600+2400+100", screens, "region")
    verify(crossing.unavailable)
    verify(crossing.reason.indexOf("crosses monitor") >= 0)

    var outside = Model.capturePlan("region:800x600+9000+100", screens, "region")
    verify(outside.unavailable)
    verify(outside.reason.indexOf("not on a connected monitor") >= 0)

    var pastEdge = Model.capturePlan("region:200x600+4400+100", screens, "region")
    verify(pastEdge.unavailable)
    verify(pastEdge.reason.indexOf("extends beyond") >= 0)
  }

  function test_portalNeverBorrowsAScreen() {
    var plan = Model.capturePlan("portal", screens, "portal")
    verify(plan.unavailable)
    compare(plan.source, null)
    compare(plan.area, null)
    verify(plan.reason.indexOf("system picker") >= 0)
  }

  function test_idlePlanFollowsFocusAndFallsBack() {
    var focused = Model.idlePlan(screens, "DP-2")
    compare(focused.source, screens[1])
    compare(focused.title, "Full monitor preview")

    var fallback = Model.idlePlan(screens, "missing")
    compare(fallback.source, screens[0])

    var none = Model.idlePlan([], "DP-1")
    verify(none.unavailable)
    compare(none.source, null)
  }

  function test_fitPreviewPreservesAspect() {
    var wide = Model.fitPreview(1920, 1080, 510, 287)
    fuzzyCompare(wide.width, 510, 0.0001)
    fuzzyCompare(wide.height, 286.875, 0.0001)
    fuzzyCompare(wide.width / wide.height, 1920 / 1080, 0.0001)

    var ultrawide = Model.fitPreview(3440, 1440, 510, 287)
    fuzzyCompare(ultrawide.width, 510, 0.0001)
    fuzzyCompare(ultrawide.height, 510 * 1440 / 3440, 0.0001)

    var portrait = Model.fitPreview(1080, 1920, 510, 287)
    fuzzyCompare(portrait.height, 287, 0.0001)
    fuzzyCompare(portrait.width, 287 * 1080 / 1920, 0.0001)
    verify(portrait.width < 510)

    var square = Model.fitPreview(1000, 1000, 510, 287)
    fuzzyCompare(square.width, 287, 0.0001)
    fuzzyCompare(square.height, 287, 0.0001)

    compare(Model.fitPreview(0, 100, 510, 287), null)
    compare(Model.fitPreview(100, 100, 0, 287), null)
  }

  function test_layoutCropsExactLogicalRectangle() {
    var plan = Model.capturePlan("region:1280x720+300+200", screens, "region")
    var layout = Model.previewLayout(plan, 510, 287)
    verify(layout.hasSource)
    fuzzyCompare(layout.frameWidth, 510, 0.0001)
    fuzzyCompare(layout.frameHeight, 286.875, 0.0001)
    fuzzyCompare(layout.scale, 510 / 1280, 0.0001)
    fuzzyCompare(layout.sourceX, -300 * layout.scale, 0.0001)
    fuzzyCompare(layout.sourceY, -200 * layout.scale, 0.0001)
    fuzzyCompare(layout.sourceWidth, 2560 * layout.scale, 0.0001)
    fuzzyCompare(layout.sourceHeight, 1440 * layout.scale, 0.0001)
    verify(layout.sourceX <= 0)
    verify(layout.sourceY <= 0)
    verify(layout.sourceX + layout.sourceWidth >= layout.frameWidth)
    verify(layout.sourceY + layout.sourceHeight >= layout.frameHeight)
  }

  function test_layoutHandlesNegativeScreenOrigin() {
    var plan = Model.capturePlan("region:1000x700+-1600+100", screens, "window")
    verify(!plan.unavailable)
    var layout = Model.previewLayout(plan, 510, 287)
    fuzzyCompare(layout.scale, 287 / 700, 0.0001)
    fuzzyCompare(layout.sourceX, -128 * layout.scale, 0.0001)
    fuzzyCompare(layout.sourceY, -100 * layout.scale, 0.0001)
    verify(layout.sourceX + layout.sourceWidth >= layout.frameWidth)
  }

  function test_monitorLayoutFillsFrame() {
    var plan = Model.capturePlan("monitor:DP-2", screens, "monitor")
    var layout = Model.previewLayout(plan, 510, 287)
    fuzzyCompare(layout.frameWidth / layout.frameHeight, 1920 / 1080, 0.0001)
    fuzzyCompare(layout.sourceX, 0, 0.0001)
    fuzzyCompare(layout.sourceY, 0, 0.0001)
    fuzzyCompare(layout.sourceWidth, layout.frameWidth, 0.0001)
    fuzzyCompare(layout.sourceHeight, layout.frameHeight, 0.0001)
  }

  function test_captureSizeUsesSourcePixels() {
    var plan = Model.capturePlan("region:1280x720+100+50", screens, "region")
    var size = Model.captureSize(plan, { width: 2560, height: 1440 })
    compare(size, { width: 1280, height: 720 })

    var monitor = Model.capturePlan("monitor:eDP-1", screens, "monitor")
    var physical = Model.captureSize(monitor, { width: 2880, height: 1620 })
    compare(physical, { width: 2880, height: 1620 })

    compare(Model.captureSize(plan, { width: 0, height: 0 }), null)
    compare(Model.captureSize(plan, null), null)
  }

  function test_captureSizeRoundsPhysicalEdges() {
    // Qt reports dpr 2 here, but the buffer is 2880x1620 at a 1.6667 scale.
    // The source buffer, not dpr, decides the encoded width.
    var plan = Model.capturePlan("region:100x100+-1727+1", screens, "region")
    verify(!plan.unavailable)
    var size = Model.captureSize(plan, { width: 2880, height: 1620 })
    compare(size, { width: 166, height: 166 })
  }

  function test_rotatedMonitorUsesPortraitFrame() {
    var portrait = [{ name: "HDMI-A-1", x: 0, y: 0, width: 1080, height: 1920 }]
    var plan = Model.capturePlan("monitor:HDMI-A-1", portrait, "monitor")
    var layout = Model.previewLayout(plan, 510, 287)
    fuzzyCompare(layout.frameHeight, 287, 0.0001)
    fuzzyCompare(layout.frameWidth, 287 * 1080 / 1920, 0.0001)
    fuzzyCompare(layout.sourceWidth, layout.frameWidth, 0.0001)
  }

  function test_layoutWithoutAreaUsesStablePlaceholderRatio() {
    var plan = Model.capturePlan("portal", screens, "portal")
    var layout = Model.previewLayout(plan, 510, 287)
    verify(!layout.hasSource)
    fuzzyCompare(layout.frameWidth, 510, 0.0001)
    fuzzyCompare(layout.frameHeight, 286.875, 0.0001)
  }
}
