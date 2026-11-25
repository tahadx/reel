import QtQuick
import QtTest
import "../Bridge.js" as Bridge

TestCase {
  name: "ServiceBridge"

  function cleanup() {
    Bridge.clear(Bridge.current())
  }

  function test_noPublisherHasNoSnapshot() {
    compare(Bridge.current(), null)
  }

  function test_publishesALiveReaderAndClosedActions() {
    var calls = []
    var value = { status: "idle" }
    var api = Bridge.publish(function() { return { state: value } }, {
      refresh: function() { calls.push("refresh") },
      retryConfigure: function() { calls.push("configure") }
    })

    compare(Bridge.current(), api)
    compare(api.snapshot().state, value)

    value = { status: "recording" }
    compare(api.snapshot().state.status, "recording")

    api.call("refresh")
    api.call("retryConfigure")
    compare(calls, ["refresh", "configure"])
  }

  function test_unknownOrInheritedActionsAreRejected() {
    var calls = 0
    var api = Bridge.publish(function() { return {} }, {
      refresh: function() { calls++ }
    })

    verify(api.call("missing") === undefined)
    verify(api.call("toString") === undefined)
    verify(api.call("refresh", "not-an-array") === undefined)
    compare(calls, 1)
  }

  function test_aNewPublisherRetiresTheOldOne() {
    var first = Bridge.publish(function() { return { state: "first" } }, {})
    var second = Bridge.publish(function() { return { state: "second" } }, {})

    compare(Bridge.current(), second)
    compare(first.snapshot(), null)
    compare(second.snapshot().state, "second")

    // A retiring old service must not clear the newer publish.
    Bridge.clear(first)
    compare(Bridge.current(), second)
    Bridge.clear(second)
    compare(Bridge.current(), null)
  }

  function test_aRetiredPublisherCannotCallActions() {
    var calls = 0
    var first = Bridge.publish(function() { return {} }, {
      refresh: function() { calls++ }
    })
    Bridge.publish(function() { return {} }, {})

    first.call("refresh")
    compare(calls, 0)
  }

  function test_snapshotValueNormalizesMissingFields() {
    var value = Bridge.snapshotValue({
      elapsed: "12",
      recentVersion: "3",
      lastError: null
    })

    compare(value.state, null)
    compare(value.recents, [])
    compare(value.elapsed, 12)
    compare(value.recentVersion, 3)
    compare(value.outputDir, "")
    compare(value.lastError, "")
    compare(value.configureFailed, false)
  }
}
