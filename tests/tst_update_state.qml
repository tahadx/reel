import QtQuick
import QtTest
import "../Model.js" as Model

TestCase {
  name: "UpdateState"

  function test_emptyUpdateLooksCurrent() {
    var state = Model.emptyUpdate()
    compare(state.available, false)
    compare(state.current, "")
    compare(state.behind, 0)
    compare(state.error, "")
  }

  function test_parsesAnAvailableUpdate() {
    var state = Model.parseUpdate('{"current":"abc1234","latest":"def5678","available":true,"behind":2,"checkedAt":1700,"error":""}')
    compare(state.current, "abc1234")
    compare(state.latest, "def5678")
    compare(state.available, true)
    compare(state.behind, 2)
    compare(state.checkedAt, 1700)
    compare(state.error, "")
  }

  function test_a_malformed_result_reads_as_current() {
    compare(Model.parseUpdate("not json").available, false)
    compare(Model.parseUpdate("").available, false)
  }

  function test_an_error_stays_visible() {
    var state = Model.parseUpdate('{"available":false,"error":"The remote could not be reached."}')
    compare(state.error, "The remote could not be reached.")
    compare(state.available, false)
  }

  function test_updateCueShowsCheckingUpdateAndRetry() {
    compare(Model.updateCueLabel({ available: false }, false), "Update")
    compare(Model.updateCueLabel({}, true), "Checking…")
    compare(Model.updateCueLabel({ current: "abc1234", latest: "def5678", available: true }, false), "Update")
    compare(Model.updateCueLabel({ available: false, error: "offline" }, false), "Retry")
  }

  function test_updateBehindLabelShowsCommits() {
    compare(Model.updateBehindLabel({ behind: 1 }), "1 new commit")
    compare(Model.updateBehindLabel({ behind: 3 }), "3 new commits")
    compare(Model.updateBehindLabel({ behind: 0 }), "an update")
    compare(Model.updateBehindLabel({}), "an update")
  }
}
