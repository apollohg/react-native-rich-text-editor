import ExpoModulesCore
import UIKit

extension NativeEditorExpoView {
    func focus() {
        _ = richTextView.activeTextInput.becomeFirstResponder()
    }

    func blur() {
        clearRecentToolbarTouch()
        _ = richTextView.activeTextInput.resignFirstResponder()
    }

    func getCaretRectJson() -> String? {
        layoutIfNeeded()
        richTextView.layoutIfNeeded()

        guard let caretRect = richTextView.currentCaretRect() else {
            return nil
        }
        let editorRect = richTextView.convert(caretRect, to: self)
        let payload: [String: Any] = [
            "x": editorRect.minX,
            "y": editorRect.minY,
            "width": editorRect.width,
            "height": editorRect.height,
            "editorWidth": bounds.width,
            "editorHeight": bounds.height
        ]
        guard let data = try? JSONSerialization.data(withJSONObject: payload),
              let json = String(data: data, encoding: .utf8)
        else {
            return nil
        }
        return json
    }

    @objc func textViewDidBeginEditing(_ notification: Notification) {
        guard notification.object as? EditorTextView === richTextView.activeTextInput else { return }
        installOutsideTapRecognizerIfNeeded()
        richTextView.textView.refreshSelectionVisualState()
        refreshMentionQuery()
        tableSelectionGeometryPublisher.scheduleFlush()
        emitFocusChange(true)
    }

    @objc func textViewDidEndEditing(_ notification: Notification) {
        guard notification.object as? EditorTextView === richTextView.activeTextInput else {
            let editorId = richTextView.editorId
            DispatchQueue.main.async { [weak self] in
                guard let self, self.richTextView.editorId == editorId,
                      !self.richTextView.activeTextInput.isFirstResponder
                else { return }
                self.activeTextInputDidEndEditing()
            }
            return
        }
        activeTextInputDidEndEditing()
    }

    private func activeTextInputDidEndEditing() {
        if consumeToolbarFocusPreservationForBlur() {
            DispatchQueue.main.async { [weak self] in
                _ = self?.richTextView.activeTextInput.becomeFirstResponder()
            }
            return
        }

        uninstallOutsideTapRecognizer()
        richTextView.textView.refreshSelectionVisualState()
        clearMentionQueryStateAndHidePopover()
        tableSelectionGeometryPublisher.flush()
        emitFocusChange(false)
    }

    private func emitFocusChange(_ isFocused: Bool) {
        let originatingEditorId = richTextView.editorId
        if lastEmittedFocus?.editorId == originatingEditorId && lastEmittedFocus?.isFocused == isFocused { return }
        guard let event = Self.editorScopedEventPayload(
            ["isFocused": isFocused],
            originatingEditorId: originatingEditorId
        ) else { return }
        lastEmittedFocus = (originatingEditorId, isFocused)
        if let onFocusChangeForTesting {
            onFocusChangeForTesting(event)
        } else {
            onFocusChange(event)
        }
    }

    @objc func handleOutsideTap(_ recognizer: UITapGestureRecognizer) {
        guard recognizer.state == .ended else { return }
        guard richTextView.activeTextInput.isFirstResponder else { return }
        guard let tapWindow = gestureWindow ?? window else { return }
        let locationInWindow = recognizer.location(in: tapWindow)
        guard shouldHandleOutsideTap(locationInWindow: locationInWindow, touchedView: nil) else {
            return
        }
        clearRecentToolbarTouch()
        blur()
    }

    func installOutsideTapRecognizerIfNeeded() {
        guard let window else { return }
        if gestureWindow === window, window.gestureRecognizers?.contains(outsideTapGestureRecognizer) == true {
            return
        }
        uninstallOutsideTapRecognizer()
        window.addGestureRecognizer(outsideTapGestureRecognizer)
        gestureWindow = window
    }

    func uninstallOutsideTapRecognizer() {
        if let window = gestureWindow {
            window.removeGestureRecognizer(outsideTapGestureRecognizer)
        }
        gestureWindow = nil
    }

    func gestureRecognizer(_ gestureRecognizer: UIGestureRecognizer, shouldReceive touch: UITouch) -> Bool {
        guard gestureRecognizer === outsideTapGestureRecognizer else { return true }
        guard let tapWindow = gestureWindow ?? window else { return true }
        let locationInWindow = touch.location(in: tapWindow)
        return prepareOutsideTapForFocusHandling(
            locationInWindow: locationInWindow,
            touchedView: touch.view
        )
    }

    func prepareOutsideTapForFocusHandling(
        locationInWindow: CGPoint,
        touchedView: UIView?
    ) -> Bool {
        if isLocationInStandaloneToolbarFrame(locationInWindow) {
            markRecentToolbarTouch()
        }
        let result = shouldHandleOutsideTap(
            locationInWindow: locationInWindow,
            touchedView: touchedView
        )
        if result {
            clearRecentToolbarTouch()
        }
        return result
    }

    func markRecentToolbarTouch() {
        lastToolbarTouchUptime = ProcessInfo.processInfo.systemUptime
    }

    private func clearRecentToolbarTouch() {
        lastToolbarTouchUptime = -Double.infinity
    }

    func shouldPreserveFocusAfterToolbarTouch() -> Bool {
        ProcessInfo.processInfo.systemUptime - lastToolbarTouchUptime <= 0.75
    }

    func consumeToolbarFocusPreservationForBlur() -> Bool {
        guard shouldPreserveFocusAfterToolbarTouch() else { return false }
        clearRecentToolbarTouch()
        return true
    }

    private func isLocationInStandaloneToolbarFrame(_ locationInWindow: CGPoint) -> Bool {
        toolbarFramesInWindow.contains(where: { $0.contains(locationInWindow) })
    }

    private func shouldHandleOutsideTap(
        locationInWindow: CGPoint,
        touchedView: UIView?
    ) -> Bool {
        if let touchedView, touchedView.isDescendant(of: self) {
            return false
        }
        if let tapWindow = gestureWindow ?? window {
            let editorFrameInWindow = convert(bounds, to: tapWindow)
            if editorFrameInWindow.contains(locationInWindow) {
                return false
            }
        }
        if let touchedView, touchedView.isDescendant(of: accessoryToolbar) {
            return false
        }
        if isLocationInStandaloneToolbarFrame(locationInWindow) {
            return false
        }
        return true
    }

}
