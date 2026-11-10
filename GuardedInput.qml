import QtQuick

// A TextInput with Qt's native clipboard paths removed.
//
// Keyboard paste is intercepted for every standard binding, including the
// dedicated Paste key, and routed to the bounded helper. The mouse is handled
// here so Qt never sees a middle click (synchronous PRIMARY paste) or a
// context paste. Drag and double-click selection are reimplemented on top of
// `positionAt`.
TextInput {
  id: guardInput

  signal boundedPaste()
  property int pasteLimit: 200

  selectByMouse: false
  maximumLength: pasteLimit

  function isPasteShortcut(event) {
    // StandardKey covers Ctrl+V, Shift+Insert, and the dedicated Paste key on
    // platforms that expose it.
    if (event.matches(StandardKey.Paste)) return true
    return event.key === Qt.Key_Paste
      || (event.modifiers & Qt.ControlModifier && event.key === Qt.Key_V)
      || (event.modifiers & Qt.ShiftModifier && event.key === Qt.Key_Insert)
  }

  Keys.onPressed: function(event) {
    if (guardInput.isPasteShortcut(event)) {
      event.accepted = true
      guardInput.boundedPaste()
    }
  }

  MouseArea {
    anchors.fill: parent
    acceptedButtons: Qt.LeftButton | Qt.MiddleButton | Qt.RightButton
    cursorShape: Qt.IBeamCursor
    property int pressPosition: 0
    property bool dragging: false
    onPressed: function(mouse) {
      if (mouse.button !== Qt.LeftButton) {
        mouse.accepted = true
        return
      }
      guardInput.forceActiveFocus()
      guardInput.cursorPosition = guardInput.positionAt(mouse.x, mouse.y)
      pressPosition = guardInput.cursorPosition
      dragging = true
    }
    onPositionChanged: function(mouse) {
      if (!dragging) return
      var position = guardInput.positionAt(mouse.x, mouse.y)
      guardInput.select(Math.min(pressPosition, position), Math.max(pressPosition, position))
    }
    onReleased: function(mouse) {
      dragging = false
      mouse.accepted = true
    }
    onDoubleClicked: function(mouse) {
      guardInput.cursorPosition = guardInput.positionAt(mouse.x, mouse.y)
      guardInput.selectWord()
    }
  }
}
