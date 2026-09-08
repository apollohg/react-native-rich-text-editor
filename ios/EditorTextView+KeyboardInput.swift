import os
import UIKit

enum EditorPasteMode: String {
    case rich
    case plainText
    case disabled
}

struct EditorClipboardPayload {
    static let fragmentType = "com.apollohg.native-editor.fragment"

    let fragment: String
    let html: String
    let text: String

    init(fragment: String, html: String, text: String) {
        self.fragment = fragment
        self.html = html
        self.text = text
    }

    init?(json: String) {
        guard let data = json.data(using: .utf8),
              let object = try? JSONSerialization.jsonObject(with: data) as? [String: Any],
              object["empty"] as? Bool != true,
              let fragment = object["fragment"] as? String,
              let html = object["html"] as? String,
              let text = object["text"] as? String
        else { return nil }
        self.init(fragment: fragment, html: html, text: text)
    }

    @discardableResult
    func write(to pasteboard: UIPasteboard) -> Bool {
        pasteboard.items = [[
            Self.fragmentType: Data(fragment.utf8),
            "public.html": Data(html.utf8),
            "public.utf8-plain-text": text
        ]]
        return pasteboard.data(forPasteboardType: Self.fragmentType) != nil
            && pasteboard.data(forPasteboardType: "public.html") != nil
            && pasteboard.string != nil
    }
}

enum EditorClipboardPaste {
    static let maximumRTFBytes = 64 * 1_024 * 1_024

    static let supportedTypes = [
        EditorClipboardPayload.fragmentType,
        "public.html",
        "public.rtf",
        "public.utf8-plain-text",
        "public.plain-text",
        "public.text"
    ]

    static func hasSupportedContent(in pasteboard: UIPasteboard) -> Bool {
        pasteboard.contains(pasteboardTypes: supportedTypes)
    }

    static func command(
        from pasteboard: UIPasteboard,
        mode: EditorPasteMode,
        maximumRTFBytes: Int = EditorClipboardPaste.maximumRTFBytes
    ) -> [String: Any]? {
        guard mode != .disabled else { return nil }
        var command: [String: Any] = ["type": "paste"]
        if let fragment = utf8String(pasteboard.data(forPasteboardType: EditorClipboardPayload.fragmentType)) {
            command["fragment"] = fragment
        }
        var html = utf8String(pasteboard.data(forPasteboardType: "public.html"))
        var text = pasteboard.string
        if html == nil,
           let rtf = pasteboard.data(forPasteboardType: "public.rtf"),
           rtf.count <= maximumRTFBytes,
           let attributed = try? NSAttributedString(
               data: rtf,
               options: [.documentType: NSAttributedString.DocumentType.rtf],
               documentAttributes: nil
           ) {
            let convertedHTML = semanticHTML(from: attributed)
            if !convertedHTML.isEmpty {
                html = convertedHTML
            }
            if text == nil {
                text = attributed.string
            }
        }
        let hasStructuredContent = command["fragment"] != nil || html != nil
        guard hasStructuredContent || !(text?.isEmpty ?? true) else { return nil }
        if let html {
            command["html"] = html
        }
        if let text {
            command["text"] = text
        }
        if mode == .plainText {
            command["plainText"] = true
        }
        return command.count > 1 ? command : nil
    }

    private static func utf8String(_ data: Data?) -> String? {
        guard let data else { return nil }
        return String(data: data, encoding: .utf8)
    }

    private static func semanticHTML(from attributed: NSAttributedString) -> String {
        let range = NSRange(location: 0, length: attributed.length)
        var hasList = false
        attributed.enumerateAttribute(.paragraphStyle, in: range) { value, _, stop in
            if let style = value as? NSParagraphStyle, !style.textLists.isEmpty {
                hasList = true
                stop.pointee = true
            }
        }
        guard attributed.string.contains("\n") || attributed.string.contains("\r") || hasList else {
            return semanticInlineHTML(from: attributed, range: range)
        }

        let string = attributed.string as NSString
        var paragraphs: [(html: String, listTags: [String])] = []
        var location = 0
        while location < string.length {
            let paragraphRange = string.paragraphRange(
                for: NSRange(location: location, length: 0)
            )
            var contentLength = paragraphRange.length
            while contentLength > 0 {
                let scalar = string.character(at: paragraphRange.location + contentLength - 1)
                guard scalar == 0x000A || scalar == 0x000D else { break }
                contentLength -= 1
            }
            let contentRange = NSRange(location: paragraphRange.location, length: contentLength)
            let styleLocation = min(paragraphRange.location, max(attributed.length - 1, 0))
            let style = attributed.length > 0
                ? attributed.attribute(.paragraphStyle, at: styleLocation, effectiveRange: nil)
                    as? NSParagraphStyle
                : nil
            paragraphs.append((
                html: semanticInlineHTML(from: attributed, range: contentRange),
                listTags: style?.textLists.map(listTag) ?? []
            ))
            location = NSMaxRange(paragraphRange)
        }
        if let finalScalar = attributed.string.utf16.last,
           finalScalar == 0x000A || finalScalar == 0x000D {
            let style = attributed.attribute(
                .paragraphStyle,
                at: attributed.length - 1,
                effectiveRange: nil
            ) as? NSParagraphStyle
            paragraphs.append((html: "", listTags: style?.textLists.map(listTag) ?? []))
        }

        var html = ""
        var openListTags: [String] = []
        for paragraph in paragraphs {
            guard !paragraph.listTags.isEmpty else {
                closeLists(&openListTags, into: &html)
                html += "<p>\(paragraph.html)</p>"
                continue
            }

            let sharedDepth = zip(openListTags, paragraph.listTags)
                .prefix { $0.0 == $0.1 }
                .count
            while openListTags.count > sharedDepth {
                let tag = openListTags.removeLast()
                html += "</li></\(tag)>"
            }
            if !openListTags.isEmpty, openListTags.count == paragraph.listTags.count {
                html += "</li><li>"
            }
            for tag in paragraph.listTags.dropFirst(sharedDepth) {
                html += "<\(tag)><li>"
                openListTags.append(tag)
            }
            html += paragraph.html
        }
        closeLists(&openListTags, into: &html)
        return html
    }

    private static func semanticInlineHTML(
        from attributed: NSAttributedString,
        range: NSRange
    ) -> String {
        var html = ""
        attributed.enumerateAttributes(in: range) { attributes, range, _ in
            var run = escapeHTMLText(attributed.attributedSubstring(from: range).string)
            if let font = attributes[.font] as? UIFont {
                let traits = font.fontDescriptor.symbolicTraits
                if traits.contains(.traitBold) {
                    run = "<strong>\(run)</strong>"
                }
                if traits.contains(.traitItalic) {
                    run = "<em>\(run)</em>"
                }
            }
            if styleIsSet(attributes[.underlineStyle]) {
                run = "<u>\(run)</u>"
            }
            if styleIsSet(attributes[.strikethroughStyle]) {
                run = "<s>\(run)</s>"
            }
            if let href = linkString(attributes[.link]) {
                run = "<a href=\"\(escapeHTMLAttribute(href))\">\(run)</a>"
            }
            html += run
        }
        guard !html.isEmpty else { return "" }
        return "<span style=\"white-space:pre-wrap\">\(html)</span>"
    }

    private static func listTag(_ list: NSTextList) -> String {
        let marker = String(describing: list.markerFormat).lowercased()
        let orderedMarkers = ["decimal", "roman", "alpha", "latin"]
        return orderedMarkers.contains { marker.contains($0) } ? "ol" : "ul"
    }

    private static func closeLists(_ tags: inout [String], into html: inout String) {
        while let tag = tags.popLast() {
            html += "</li></\(tag)>"
        }
    }

    private static func escapeHTMLText(_ value: String) -> String {
        value
            .replacingOccurrences(of: "&", with: "&amp;")
            .replacingOccurrences(of: "<", with: "&lt;")
            .replacingOccurrences(of: ">", with: "&gt;")
            .replacingOccurrences(of: "\r\n", with: "\n")
            .replacingOccurrences(of: "\r", with: "\n")
            .replacingOccurrences(of: "\n", with: "<br>")
    }

    private static func escapeHTMLAttribute(_ value: String) -> String {
        value
            .replacingOccurrences(of: "&", with: "&amp;")
            .replacingOccurrences(of: "<", with: "&lt;")
            .replacingOccurrences(of: ">", with: "&gt;")
            .replacingOccurrences(of: "\"", with: "&quot;")
            .replacingOccurrences(of: "'", with: "&#39;")
    }

    private static func styleIsSet(_ value: Any?) -> Bool {
        guard let number = value as? NSNumber else { return false }
        return number.intValue != 0
    }

    private static func linkString(_ value: Any?) -> String? {
        if let url = value as? URL {
            return url.absoluteString
        }
        return value as? String
    }
}

extension EditorTextView {
    @discardableResult
    func exportSelectionToPasteboard(_ pasteboard: UIPasteboard = .general) -> Bool {
        ensureInternalTextViewDelegate()
        guard editorId != 0 else { return false }
        guard finishExternalTextCompositionBeforeInteractionIfNeeded() else { return false }
        guard prepareForExternalEditorUpdate() else { return false }
        guard syncClipboardSelectionToRust(requiresRange: true) != nil else { return false }
        guard let payload = EditorV2Shadow.clipboardPayload(id: editorId) else { return false }
        return payload.write(to: pasteboard)
    }

    func syncClipboardSelectionToRust(
        requiresRange: Bool = false
    ) -> (anchor: UInt32, head: UInt32)? {
        guard let selection = currentScalarSelection() else { return nil }
        guard !requiresRange
            || selection.anchor != selection.head
            || selectedImageSelectionState() != nil
        else { return nil }
        let sync: EditorV2SelectionSync?
        if let image = selectedImageSelectionState() {
            sync = EditorV2Shadow.setNodeSelection(id: editorId, docPos: image.docPos)
        } else {
            sync = EditorV2Shadow.setSelectionScalar(
                id: editorId,
                scalarAnchor: selection.anchor,
                scalarHead: selection.head
            )
        }
        guard let sync else { return nil }
        if let refreshed = sync.refreshedUpdateJSON {
            applyUpdateJSON(refreshed, notifyDelegate: false)
        }
        return selection
    }

    func applyClipboardCommand(_ command: [String: Any]) {
        let updateJSON = EditorV2Shadow.applyClipboardCommand(id: editorId, command: command)
        applyUpdateJSON(updateJSON)
    }

    @objc func handleIndentKeyCommand() {
        handleListDepthKeyCommand(outdent: false)
    }

    @objc func handleHardBreakKeyCommand() {
        guard !isApplyingRustState, editorId != 0, isEditable else { return }
        guard finishExternalTextCompositionBeforeInteractionIfNeeded() else { return }
        guard flushPendingNativeTextMutationCommitIfNeeded() else { return }
        guard !isCollapsedAtomBoundary(selectedUtf16Range()) else { return }
        performInterceptedInput {
            insertNodeInRust(preferredHardBreakNodeType())
        }
    }

    @objc func handleOutdentKeyCommand() {
        handleListDepthKeyCommand(outdent: true)
    }

    func adjacentVoidBlockDeleteRangeForBackwardDelete(
        cursorUtf16Offset: Int,
        cursorScalar: UInt32
    ) -> (from: UInt32, to: UInt32)? {
        guard cursorUtf16Offset >= 0, cursorUtf16Offset < textStorage.length else {
            return nil
        }
        let attrs = textStorage.attributes(at: cursorUtf16Offset, effectiveRange: nil)
        guard attrs[.attachment] is NSTextAttachment,
              attrs[RenderBridgeAttributes.voidNodeType] is String,
              cursorScalar < UInt32.max
        else {
            return nil
        }
        return (from: cursorScalar, to: cursorScalar + 1)
    }

    func trailingVoidBlockDeleteRangeForBackwardDelete(
        cursorUtf16Offset: Int
    ) -> (from: UInt32, to: UInt32)? {
        let text = textStorage.string as NSString
        guard text.length > 0 else { return nil }

        let clampedCursor = min(max(cursorUtf16Offset, 0), text.length)
        let paragraphProbe = min(max(clampedCursor - 1, 0), text.length - 1)
        let paragraphRange = text.paragraphRange(for: NSRange(location: paragraphProbe, length: 0))

        let placeholderRange = NSRange(location: paragraphRange.location, length: 1)
        guard placeholderRange.location + placeholderRange.length <= text.length else {
            return nil
        }

        let paragraphText = text.substring(with: placeholderRange)
        guard paragraphText == "\u{200B}" else { return nil }
        guard paragraphRange.location >= 2 else { return nil }
        guard text.character(at: paragraphRange.location - 1) == 0x000A else { return nil }

        let attachmentIndex = paragraphRange.location - 2
        guard
            let deleteRange = scalarDeleteRangeForVoidAttachment(at: attachmentIndex)
        else {
            return nil
        }

        return deleteRange
    }

    private func scalarDeleteRangeForVoidAttachment(
        at utf16Offset: Int
    ) -> (from: UInt32, to: UInt32)? {
        guard utf16Offset >= 0, utf16Offset < textStorage.length else {
            return nil
        }
        let attrs = textStorage.attributes(at: utf16Offset, effectiveRange: nil)
        guard let attachment = attrs[.attachment] as? NSTextAttachment,
              !(attachment is AtomBlockAttachment),
              attrs[RenderBridgeAttributes.voidNodeType] is String
        else {
            return nil
        }

        let attachmentEndScalar = PositionBridge.utf16OffsetToScalar(
            utf16Offset + 1,
            in: self
        )
        guard attachmentEndScalar > 0 else { return nil }
        return (from: attachmentEndScalar - 1, to: attachmentEndScalar)
    }

    private func handleListDepthKeyCommand(outdent: Bool) {
        guard !isApplyingRustState else { return }
        guard editorId != 0 else { return }
        guard isEditable else { return }
        guard finishExternalTextCompositionBeforeInteractionIfNeeded() else { return }
        guard flushPendingNativeTextMutationCommitIfNeeded() else { return }
        guard isCaretInsideList() else { return }
        guard let selection = currentScalarSelection() else { return }

        performInterceptedInput {
            let updateJSON = outdent
                ? EditorV2Shadow.outdentListItemAtSelectionScalar(
                    id: editorId,
                    scalarAnchor: selection.anchor,
                    scalarHead: selection.head
                )
                : EditorV2Shadow.indentListItemAtSelectionScalar(
                    id: editorId,
                    scalarAnchor: selection.anchor,
                    scalarHead: selection.head
                )
            applyUpdateJSON(updateJSON)
        }
    }

    private func isCaretInsideList() -> Bool {
        guard editorId != 0 else { return false }
        guard
            let data = EditorV2Shadow.getCurrentState(id: editorId).data(using: .utf8),
            let object = try? JSONSerialization.jsonObject(with: data) as? [String: Any],
            let activeState = object["activeState"] as? [String: Any],
            let nodes = activeState["nodes"] as? [String: Any]
        else {
            return false
        }

        return nodes.contains { nodeType, value in
            EditorNodeTypes.isListContainer(nodeType) && value as? Bool == true
        }
    }

    private func preferredHardBreakNodeType() -> String {
        guard
            let data = EditorV2Shadow.getCurrentState(id: editorId).data(using: .utf8),
            let object = try? JSONSerialization.jsonObject(with: data) as? [String: Any],
            let activeState = object["activeState"] as? [String: Any],
            let insertableNodes = activeState["insertableNodes"] as? [String]
        else {
            return "hardBreak"
        }

        return EditorNodeTypes.preferredHardBreak(in: Set(insertableNodes))
    }

    func ensureInternalTextViewDelegate() {
        // Some keyboard integrations replace UITextView's private delegate ivar
        // directly. The editor must own delegate callbacks so external observers
        // cannot inspect transient TextKit state during Rust-driven edits.
        // The delegate is a dedicated object rather than the text view itself;
        // see EditorTextViewInternalDelegate for why (APOLLO-REACT-56).
        guard (delegate as AnyObject?) !== internalTextViewDelegate else { return }
        delegate = internalTextViewDelegate
    }

    func performInterceptedInput(
        flushPendingNativeTextMutation: Bool = true,
        _ action: () -> Void
    ) {
        if flushPendingNativeTextMutation, interceptedInputDepth == 0 {
            guard flushPendingNativeTextMutationCommitIfNeeded() else { return }
        }
        interceptedInputDepth += 1
        Self.inputLog.debug(
            "[intercept.begin] depth=\(self.interceptedInputDepth) selection=\(self.selectionSummary(), privacy: .public) textState=\(self.textSnapshotSummary(), privacy: .public)"
        )
        action()
        DispatchQueue.main.async { [weak self] in
            guard let self else { return }
            self.interceptedInputDepth = max(0, self.interceptedInputDepth - 1)
            Self.inputLog.debug(
                "[intercept.end] depth=\(self.interceptedInputDepth) selection=\(self.selectionSummary(), privacy: .public) textState=\(self.textSnapshotSummary(), privacy: .public)"
            )
            if self.interceptedInputDepth == 0 {
                _ = self.drainPendingNativeTextMutation(
                    allowAfterBlur: false,
                    allowWhileIntercepting: false
                )
                self.drainDeferredInsertTextIfReady()
            }
        }
    }

    func enqueueDeferredInsertText(_ text: String) {
        deferredInsertTexts.append(text)
        scheduleDeferredInsertDrain()
    }

    private func scheduleDeferredInsertDrain() {
        guard !deferredInsertDrainScheduled else { return }
        deferredInsertDrainScheduled = true
        DispatchQueue.main.async { [weak self] in
            self?.drainDeferredInsertTextIfReady()
        }
    }

    private func drainDeferredInsertTextIfReady() {
        deferredInsertDrainScheduled = false
        guard !deferredInsertTexts.isEmpty else { return }
        guard editorId != 0 else {
            deferredInsertTexts.removeAll()
            return
        }
        guard !isApplyingRustState,
              interceptedInputDepth == 0,
              pendingNativeTextMutation == nil,
              !nativeTextMutationCommitScheduled
        else {
            scheduleDeferredInsertDrain()
            return
        }

        let text = deferredInsertTexts.removeFirst()
        isReplayingDeferredInsertText = true
        defer { isReplayingDeferredInsertText = false }
        insertText(text)
        if !deferredInsertTexts.isEmpty {
            scheduleDeferredInsertDrain()
        }
    }

    /// Handle return key press as a block split operation.
    private func handleReturnKey() {
        if let selectedRange = selectedTextRange, !selectedRange.isEmpty {
            let range = PositionBridge.textRangeToScalarRange(selectedRange, in: self)
            let updateJSON = EditorV2Shadow.deleteAndSplitScalar(
                id: editorId,
                scalarFrom: range.from,
                scalarTo: range.to
            )
            applyUpdateJSON(updateJSON)
        } else {
            let scalarPos = PositionBridge.cursorScalarOffset(in: self)
            splitBlockInRust(at: scalarPos)
        }
    }

    func interceptReturnInput(
        _ text: String,
        replacing replacementRange: UITextRange? = nil
    ) -> Bool {
        guard text == "\n" || text == "\r" else { return false }
        let scalarRange = replacementRange.map {
            PositionBridge.textRangeToScalarRange($0, in: self)
        }
        guard commitActiveMarkedTextBeforeReturn() else { return true }
        performInterceptedInput {
            if let scalarRange {
                if scalarRange.from == scalarRange.to {
                    splitBlockInRust(at: scalarRange.from)
                } else {
                    let updateJSON = EditorV2Shadow.deleteAndSplitScalar(
                        id: editorId,
                        scalarFrom: scalarRange.from,
                        scalarTo: scalarRange.to
                    )
                    applyUpdateJSON(updateJSON)
                }
            } else {
                handleReturnKey()
            }
        }
        return true
    }

    /// Split a block at a scalar position via the Rust editor.
    private func splitBlockInRust(at scalarPos: UInt32) {
        Self.inputLog.debug(
            "[rust.splitBlockScalar] scalarPos=\(scalarPos) selection=\(self.selectionSummary(), privacy: .public)"
        )
        let updateJSON = EditorV2Shadow.splitBlockScalar(id: editorId, scalarPos: scalarPos)
        applyUpdateJSON(updateJSON)
    }

    /// Paste HTML content through Rust.
    @discardableResult
    func pasteHTML(_ html: String, detectContentChange: Bool = false) -> Bool {
        let previousHTML = detectContentChange ? EditorV2Shadow.getHtml(id: editorId) : nil
        syncCurrentUIKitSelectionToRust()
        Self.inputLog.debug(
            "[rust.pasteHTML] html=\(self.preview(html), privacy: .public) selection=\(self.selectionSummary(), privacy: .public)"
        )
        let updateJSON = EditorV2Shadow.insertContentHtml(id: editorId, html: html)
        applyUpdateJSON(updateJSON)
        guard let previousHTML else { return true }
        return EditorV2Shadow.getHtml(id: editorId) != previousHTML
    }

    private func syncCurrentUIKitSelectionToRust() {
        guard editorId != 0, let range = selectedTextRange else { return }
        let anchor = PositionBridge.textViewToScalar(range.start, in: self)
        let head = PositionBridge.textViewToScalar(range.end, in: self)
        EditorV2Shadow.setSelectionScalar(id: editorId, scalarAnchor: anchor, scalarHead: head)
    }

    /// Paste plain text through Rust.
    func pastePlainText(_ text: String) {
        if let selectedRange = selectedTextRange, !selectedRange.isEmpty {
            let range = PositionBridge.textRangeToScalarRange(selectedRange, in: self)
            Self.inputLog.debug(
                "[rust.pastePlainText.replace] text=\(self.preview(text), privacy: .public) scalar=\(range.from)-\(range.to) selection=\(self.selectionSummary(), privacy: .public)"
            )
            let updateJSON = EditorV2Shadow.replaceTextScalar(
                id: editorId,
                scalarFrom: range.from,
                scalarTo: range.to,
                text: text
            )
            applyUpdateJSON(updateJSON)
        } else {
            Self.inputLog.debug(
                "[rust.pastePlainText.insert] text=\(self.preview(text), privacy: .public) selection=\(self.selectionSummary(), privacy: .public)"
            )
            insertTextInRust(text, at: PositionBridge.cursorScalarOffset(in: self))
        }
    }

}
