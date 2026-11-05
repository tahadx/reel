import QtQuick
import QtTest
import "../Model.js" as Model

TestCase {
  name: "Cameras"

  function test_readsIdAndLabel() {
    var cameras = Model.parseCameras(
      '{"cameras":[{"id":"/dev/video0","label":"Laptop Webcam"},'
      + '{"id":"/dev/video2","label":"Insta360 Link 2C Pro"}]}')
    compare(cameras.length, 2)
    compare(cameras[0].id, "/dev/video0")
    compare(cameras[0].label, "Laptop Webcam")
    compare(cameras[1].label, "Insta360 Link 2C Pro")
  }

  function test_aCameraWithoutALabelShowsItsPath() {
    var cameras = Model.parseCameras('{"cameras":[{"id":"/dev/video0","label":""}]}')
    compare(cameras[0].label, "/dev/video0")
  }

  function test_entriesWithoutAnIdAreSkipped() {
    var cameras = Model.parseCameras('{"cameras":[{"label":"Ghost"},{"id":"/dev/video1"}]}')
    compare(cameras.length, 1)
    compare(cameras[0].id, "/dev/video1")
  }

  function test_emptyOrBrokenOutputIsNoCameras() {
    compare(Model.parseCameras(""), [])
    compare(Model.parseCameras("not json"), [])
    compare(Model.parseCameras("{}"), [])
  }

  readonly property var plugged: [
    { id: "/dev/v4l/by-id/usb-Laptop_Webcam-video-index0", path: "/dev/video0", label: "Laptop Webcam" },
    { id: "/dev/v4l/by-id/usb-Insta360_Insta360_Link_2C_Pro-video-index0", path: "/dev/video2", label: "Insta360 Link 2C Pro" }
  ]

  function test_readsThePathNextToTheId() {
    var cameras = Model.parseCameras(
      '{"cameras":[{"id":"/dev/v4l/by-id/usb-Cam-video-index0","path":"/dev/video2","label":"Cam"}]}')
    compare(cameras[0].id, "/dev/v4l/by-id/usb-Cam-video-index0")
    compare(cameras[0].path, "/dev/video2")
  }

  function test_choicesAreAutoThenEveryCamera() {
    compare(Model.cameraChoices(plugged, ""), ["", plugged[0].id, plugged[1].id])
    compare(Model.cameraChoices([], ""), [""])
  }

  function test_anUnpluggedCameraStaysSelected() {
    var gone = "/dev/v4l/by-id/usb-Other_Cam-video-index0"
    compare(Model.cameraChoices(plugged, gone), ["", plugged[0].id, plugged[1].id, gone])
    compare(Model.cameraLabel(plugged, gone), "Other Cam (unplugged)")
    compare(Model.cameraChoices([], gone), ["", gone])
  }

  function test_anOldNodeSettingStandsInForItsCamera() {
    compare(Model.cameraChoices(plugged, "/dev/video2"), ["", plugged[0].id, "/dev/video2"])
    compare(Model.cameraLabel(plugged, "/dev/video2"), "Insta360 Link 2C Pro")
    compare(Model.cameraLabel(plugged, "/dev/video9"), "/dev/video9 (unplugged)")
  }

  function test_autoAndConnectedLabels() {
    compare(Model.cameraLabel(plugged, ""), "Auto")
    compare(Model.cameraLabel(plugged, plugged[1].id), "Insta360 Link 2C Pro")
  }
}
