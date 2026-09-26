import ExpoModulesCore
import UIKit

final class TableSelectionGeometryPublisher: NSObject {
    private let resolve: () -> TableSelectionGeometry?
    private let emit: ([String: Any]) -> Void
    private var published: TableSelectionGeometry?
    private var frameLink: CADisplayLink?

    init(resolve: @escaping () -> TableSelectionGeometry?, emit: @escaping ([String: Any]) -> Void) {
        self.resolve = resolve
        self.emit = emit
    }

    var hasScheduledFlushForTesting: Bool { frameLink != nil }

    func scheduleFlush() {
        guard frameLink == nil else { return }
        let link = CADisplayLink(target: self, selector: #selector(flushScheduledFrame(_:)))
        frameLink = link
        link.add(to: .main, forMode: .common)
    }

    func flush() {
        cancelScheduledFlush()
        let current = resolve()
        if let previous = published, current?.editorId != previous.editorId || current == nil {
            published = nil
            if let cleared = NativeEditorExpoView.editorScopedEventPayload(
                [:], originatingEditorId: previous.editorId
            ) {
                emit(cleared)
            }
        }
        guard let current, current != published,
              let payload = NativeEditorExpoView.editorScopedEventPayload(
                current.eventPayload, originatingEditorId: current.editorId
              )
        else { return }
        published = current
        emit(payload)
    }

    func cancelScheduledFlush() {
        frameLink?.invalidate()
        frameLink = nil
    }

    @objc private func flushScheduledFrame(_ link: CADisplayLink) {
        guard link === frameLink else { return }
        flush()
    }
}

extension NativeEditorExpoView {
    func currentTableSelectionGeometry() -> TableSelectionGeometry? {
        guard let window, richTextView.activeTextInput.isFirstResponder,
              let geometry = richTextView.tableSelectionGeometry(
                obstructions: tableSelectionObstructions(in: window)
              ),
              geometry.editorId == richTextView.editorId
        else { return nil }
        return geometry
    }

    func tableSelectionObstructions(in window: UIWindow) -> TableSelectionObstructions {
        let keyboard = (richTextView.activeTextInput.keyboardFrameInScreen ?? richTextView.textView.keyboardFrameInScreen)
            .map { window.convert($0, from: window.screen.coordinateSpace).intersection(window.bounds) }
        return TableSelectionObstructions(
            safeArea: window.bounds.inset(by: window.safeAreaInsets),
            keyboard: keyboard.flatMap { $0.isNull || $0.isEmpty ? nil : $0 }
        )
    }

    @objc func keyboardFrameMayChange(_ notification: Notification) {
        tableSelectionGeometryPublisher.scheduleFlush()
    }

    func dispatchTableSelectionGeometry(_ payload: [String: Any]) {
        if let onTableSelectionGeometryForTesting {
            onTableSelectionGeometryForTesting(payload)
        } else {
            onTableSelectionGeometry(payload)
        }
    }

    func editorTextView(
        _ textView: EditorTextView,
        didEndExternalTextComposition resultJSON: String
    ) {
        schedulePendingAtomsWakeIfNeeded()
        dispatchExternalTextCompositionEnd(resultJSON)
    }

    func dispatchExternalTextCompositionEnd(_ resultJSON: String) {
        let payload: [String: Any] = [
            "editorId": String(richTextView.editorId),
            "resultJson": resultJSON
        ]
        if let onExternalTextCompositionEndForTesting {
            onExternalTextCompositionEndForTesting(payload)
        } else {
            onExternalTextCompositionEnd(payload)
        }
    }

    func editorTextView(_ textView: EditorTextView, selectionDidChange anchor: UInt32, head: UInt32) {
        let originatingEditorId = textView.editorId
        let stateJSON = refreshToolbarStateFromEditorSelection()
        refreshSystemAssistantToolbarIfNeeded()
        refreshMentionQuery()
        richTextView.refreshRemoteSelections()
        var event: [String: Any] = ["anchor": Int(anchor), "head": Int(head)]
        if let stateJSON {
            event["stateJson"] = stateJSON
        }
        guard let scopedEvent = Self.editorScopedEventPayload(
            event,
            originatingEditorId: originatingEditorId
        ) else { return }
        onSelectionChange(scopedEvent)
    }

    func editorTextView(_ textView: EditorTextView, didReceiveUpdate updateJSON: String) {
        schedulePendingAtomsWakeIfNeeded()
        if let revision = renderRevision(fromUpdateJSON: updateJSON) {
            renderedRevision = revision
        }
        // Capture both fields from the same committed atomic update before
        // any view work can cause a rebind. The event must never relabel A's
        // update as B merely because the host changes editorId afterwards.
        let nativeCommitEvent = Self.nativeCommitEventPayload(
            originatingEditorId: String(textView.editorId),
            updateJSON: updateJSON
        )
        if let state = NativeToolbarState(updateJSON: updateJSON) {
            toolbarState = state
            accessoryToolbar.apply(state: state)
            refreshSystemAssistantToolbarIfNeeded()
        }
        refreshMentionQuery()
        richTextView.refreshRemoteSelections()
        guard !isApplyingJSUpdate else { return }
        guard let nativeCommitEvent else { return }
        onEditorUpdate(nativeCommitEvent)
    }

    /// The canonical JS commit contract. `originatingEditorId` is captured
    /// synchronously from the text view which applied `updateJSON`; it is not
    /// read from the host view after asynchronous rebind work.
    static func nativeCommitEventPayload(
        originatingEditorId: String,
        updateJSON: String
    ) -> [String: Any]? {
        guard let editorId = v2CanonicalUInt64String(originatingEditorId),
              editorId != "0",
              let nativeEditorId = UInt64(editorId),
              let data = updateJSON.data(using: .utf8),
              let update = try? JSONSerialization.jsonObject(with: data) as? [String: Any],
              let rawRevision = update["documentVersion"] as? String,
              let documentRevision = v2CanonicalUInt64String(rawRevision),
              let revision = UInt64(documentRevision),
              let atomicUpdateJSON = EditorV2Registry.adapter(forLegacyId: nativeEditorId)?
              .atomicRenderJSON(matchingDocumentRevision: revision)
        else {
            return nil
        }
        return [
            "editorId": editorId,
            "documentRevision": documentRevision,
            "updateJson": atomicUpdateJSON
        ]
    }

    /// Every non-commit view event is labelled with the editor that produced
    /// it, captured before refresh/rebind work can change the view binding.
    static func editorScopedEventPayload(
        _ payload: [String: Any],
        originatingEditorId: UInt64
    ) -> [String: Any]? {
        guard originatingEditorId != 0,
              let editorId = v2CanonicalUInt64String(String(originatingEditorId))
        else {
            return nil
        }
        var scopedPayload = payload
        scopedPayload["editorId"] = editorId
        return scopedPayload
    }

    @discardableResult
    private func refreshToolbarStateFromEditorSelection() -> String? {
        guard richTextView.editorId != 0 else { return nil }
        let stateJSON = EditorV2Shadow.getSelectionState(id: richTextView.editorId)
        guard let state = NativeToolbarState(updateJSON: stateJSON) else { return nil }
        toolbarState = state
        accessoryToolbar.apply(state: state)
        return stateJSON
    }

    func configureAccessoryToolbar() {
        accessoryToolbar.onPressItem = { [weak self] item in
            self?.handleToolbarItemPress(item)
        }
        accessoryToolbar.onSelectMentionSuggestion = { [weak self] suggestion in
            self?.insertMentionSuggestion(suggestion)
        }
        accessoryToolbar.setItems(toolbarItems)
        accessoryToolbar.apply(state: toolbarState)
        updateAccessoryToolbarVisibility()
    }

}
