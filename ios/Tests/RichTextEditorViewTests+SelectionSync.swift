import ExpoModulesCore
import UIKit
import XCTest

private final class PendingCaretTapRecognizer: CaretPlacementTapRecognizer {
    var point: CGPoint?
    var pointReadCount = 0
    var pointReadLimit = Int.max

    override var pendingCaretPoint: CGPoint? {
        pointReadCount += 1
        // Bound recursive callbacks so a regression fails without crashing XCTest.
        return pointReadCount <= pointReadLimit ? point : nil
    }
}

extension RichTextEditorViewTests {
    func testLiveStoppingEditorDecelerationPreservesCaretUntilTheNextTap() throws {
        try LiveGestureProbe.requireOptIn()
        let editorId = makeV2Editor()
        defer { destroyV2Editor(id: editorId) }
        let adapter = try XCTUnwrap(EditorV2Registry.adapter(forLegacyId: editorId))
        let window = TouchObservingTestWindow(windowScene: testWindowScene())
        window.frame = UIScreen.main.bounds
        window.backgroundColor = .systemBackground
        let editorFrame = CGRect(x: 0, y: 100, width: window.bounds.width, height: 300)
        let view = RichTextEditorView(frame: editorFrame)
        view.editorId = editorId
        let textView = view.textView
        let paragraphCount = 120
        textView.bindEditor(id: editorId, initialHTML: String(repeating: "<p>abcdefghijklmno uvwxyz</p>", count: paragraphCount))
        window.addSubview(view)
        let status = UILabel(frame: CGRect(x: 0, y: 60, width: window.bounds.width, height: 30))
        status.backgroundColor = .systemYellow
        status.textAlignment = .center
        window.addSubview(status)
        window.makeKeyAndVisible()
        defer {
            window.onTouchHitTest = nil
            _ = textView.resignFirstResponder()
            window.isHidden = true
        }
        XCTAssertTrue(textView.becomeFirstResponder())
        flushMainQueue()
        view.layoutIfNeeded()
        textView.selectedRange = NSRange(location: 0, length: 0)
        textView.textViewDidChangeSelection(textView)
        flushMainQueue()
        let originalRange = textView.selectedRange
        let originalSelection = currentSelection(in: editorId)
        let originalDocument = try XCTUnwrap(adapter.documentJson())
        let originalRevision = adapter.baseDocumentRevision
        let initialScrollOffset: CGFloat = 400
        textView.setContentOffset(CGPoint(x: 0, y: initialScrollOffset), animated: false)
        status.text = "EDITOR FLICK READY"
        let coasting = XCTNSPredicateExpectation(predicate: NSPredicate { _, _ in
            textView.isDecelerating && textView.contentOffset.y > initialScrollOffset
        }, object: nil)
        guard XCTWaiter.wait(for: [coasting], timeout: 30) == .completed else {
            return XCTFail("The real editor flick must decelerate before the stop touch")
        }
        XCTAssertEqual(textView.selectedRange, originalRange, "The flick must preserve the caret")
        var deceleratingAtStopTouch: Bool?
        window.onTouchHitTest = { [weak textView] hit in
            guard deceleratingAtStopTouch == nil, let textView,
                  hit === textView || hit.isDescendant(of: textView)
            else { return }
            deceleratingAtStopTouch = textView.isDecelerating
        }
        status.text = "EDITOR STOP TOUCH READY"
        let stopped = XCTNSPredicateExpectation(predicate: NSPredicate { _, _ in
            deceleratingAtStopTouch != nil && !textView.isDecelerating && !textView.isTracking
        }, object: nil)
        guard XCTWaiter.wait(for: [stopped], timeout: 5) == .completed else {
            return XCTFail("The finger must stop the real editor's deceleration")
        }
        XCTAssertEqual(deceleratingAtStopTouch, true, "The stop touch must arrive while the editor is coasting")
        window.onTouchHitTest = nil
        flushMainQueue()
        guard textView.selectedRange == originalRange else {
            return XCTFail("Stopping a flick moved the caret from \(originalRange) to \(textView.selectedRange)")
        }
        let stoppedSelection = currentSelection(in: editorId)
        for endpoint in ["anchor", "head"] {
            let original = try XCTUnwrap(originalSelection[endpoint] as? NSNumber, "The original core selection must contain \(endpoint)")
            let stopped = try XCTUnwrap(stoppedSelection[endpoint] as? NSNumber, "The stopped core selection must contain \(endpoint)")
            XCTAssertEqual(stopped.uint32Value, original.uint32Value, "Stopping a flick must preserve the core selection's \(endpoint)")
        }
        let stoppedOffset = textView.contentOffset.y
        XCTAssertGreaterThan(stoppedOffset, initialScrollOffset, "Stopping a flick must preserve the scrolled viewport")
        let settleInterval: TimeInterval = 0.2
        RunLoop.main.run(until: Date().addingTimeInterval(settleInterval))
        XCTAssertEqual(textView.contentOffset.y, stoppedOffset, accuracy: 1)
        XCTAssertEqual(textView.selectedRange, originalRange)

        status.text = "EDITOR CARET TAP READY"
        let placed = XCTNSPredicateExpectation(predicate: NSPredicate { _, _ in
            textView.selectedRange != originalRange
        }, object: nil)
        XCTAssertEqual(XCTWaiter.wait(for: [placed], timeout: 10), .completed, "The next deliberate tap must place the caret normally")
        flushMainQueue()
        let placedSelection = currentSelection(in: editorId)
        let originalHead = try XCTUnwrap(originalSelection["head"] as? NSNumber)
        let placedHead = try XCTUnwrap(placedSelection["head"] as? NSNumber)
        XCTAssertNotEqual(placedHead.uint32Value, originalHead.uint32Value)
        XCTAssertEqual(adapter.baseDocumentRevision, originalRevision)
        XCTAssertEqual(try XCTUnwrap(adapter.documentJson()), originalDocument)
    }

    func testCaretPlacementTap_emptyDocumentPendingTapDoesNotRecurse() throws {
        try assertEmptyDocumentCaretTap(initiallyFocused: true, placeCaret: false)
    }

    func testCaretPlacementTap_emptyDocumentFocusDoesNotRecurse() throws {
        try assertEmptyDocumentCaretTap(initiallyFocused: false, placeCaret: true)
    }

    func testCaretPlacementTap_emptyDocumentRepeatedTapDoesNotRecurse() throws {
        try assertEmptyDocumentCaretTap(initiallyFocused: true, placeCaret: true)
    }

    private func assertEmptyDocumentCaretTap(initiallyFocused: Bool, placeCaret: Bool) throws {
        let editorId = makeV2Editor()
        defer { destroyV2Editor(id: editorId) }
        let view = RichTextEditorView(frame: CGRect(x: 0, y: 0, width: 320, height: 240))
        view.editorId = editorId
        let textView = view.textView
        let window = hostEditorView(view)
        defer { window.isHidden = true }
        if initiallyFocused {
            XCTAssertTrue(textView.becomeFirstResponder())
        }
        flushMainQueue()
        XCTAssertEqual(textView.text, "\u{200B}")
        let caret = textView.caretRect(for: textView.endOfDocument)
        let point = CGPoint(x: caret.maxX + 40, y: caret.midY)
        let position = try XCTUnwrap(textView.closestPosition(to: point))
        XCTAssertEqual(textView.offset(from: textView.beginningOfDocument, to: position), 1)
        let recognizer = PendingCaretTapRecognizer()
        recognizer.point = point
        recognizer.pointReadLimit = 16
        textView.caretPlacementTapRecognizer = recognizer
        var publishedSelections: [NSRange] = []
        textView.onSelectionOrContentMayChange = { [weak textView] in
            if let textView { publishedSelections.append(textView.selectedRange) }
        }

        for _ in 0..<3 {
            recognizer.pointReadCount = 0
            if placeCaret {
                XCTAssertTrue(textView.placeCaret(at: point))
                XCTAssertTrue(textView.isFirstResponder)
            } else {
                textView.delegate?.textViewDidChangeSelection?(textView)
            }
            XCTAssertLessThan(recognizer.pointReadCount, recognizer.pointReadLimit)
            XCTAssertEqual(textView.selectedRange, NSRange(location: 0, length: 0))
            flushMainQueue()
        }
        XCTAssertFalse(publishedSelections.isEmpty)
        XCTAssertTrue(publishedSelections.allSatisfy { $0 == NSRange(location: 0, length: 0) })
        let selection = currentSelection(in: editorId)
        XCTAssertEqual((selection["anchor"] as? NSNumber)?.uint32Value, 1)
        XCTAssertEqual((selection["head"] as? NSNumber)?.uint32Value, 1)
        recognizer.point = nil
        textView.insertText("A")
        XCTAssertEqual(textView.text, "A")
    }

    func testCaretPlacementTap_correctsWordSnapBeforePublishingSelection() throws {
        try assertPendingCaretTapSelection(
            nativeSelection: NSRange(location: 15, length: 0),
            expectedSelection: NSRange(location: 6, length: 0)
        )
    }

    func testCaretPlacementTap_preservesNativeWordSelection() throws {
        try assertPendingCaretTapSelection(
            nativeSelection: NSRange(location: 0, length: 15),
            expectedSelection: NSRange(location: 0, length: 15)
        )
    }

    private func assertPendingCaretTapSelection(
        nativeSelection: NSRange,
        expectedSelection: NSRange
    ) throws {
        let editorId = makeV2Editor()
        defer { destroyV2Editor(id: editorId) }
        let view = RichTextEditorView(frame: CGRect(x: 0, y: 0, width: 320, height: 240))
        view.editorId = editorId
        let textView = view.textView
        textView.bindEditor(id: editorId, initialHTML: "<p>abcdefghijklmno</p>")
        let window = hostEditorView(view)
        defer { window.isHidden = true }
        XCTAssertTrue(textView.becomeFirstResponder())
        flushMainQueue()
        let position = try XCTUnwrap(textView.position(from: textView.beginningOfDocument, offset: 6))
        let caret = textView.caretRect(for: position)
        let recognizer = PendingCaretTapRecognizer()
        recognizer.point = CGPoint(x: caret.midX, y: caret.midY)
        textView.caretPlacementTapRecognizer = recognizer
        var publishedSelections: [NSRange] = []
        textView.onSelectionOrContentMayChange = { [weak textView] in
            if let textView { publishedSelections.append(textView.selectedRange) }
        }
        textView.selectedRange = nativeSelection
        textView.textViewDidChangeSelection(textView)
        flushMainQueue()
        XCTAssertEqual(textView.selectedRange, expectedSelection)
        XCTAssertFalse(publishedSelections.isEmpty)
        XCTAssertTrue(publishedSelections.allSatisfy { $0 == expectedSelection }, "\(publishedSelections)")
    }

    func testCaretPlacementTap_placesInteriorCaretAndSyncsToRust() throws {
        try assertInteriorCaretPlacement(initiallyFocused: true)
    }

    func testCaretPlacementTap_focusesAtTappedCharacter() throws {
        try assertInteriorCaretPlacement(initiallyFocused: false)
    }

    func testCaretPlacementTap_usesScrolledCoordinates() throws {
        try assertInteriorCaretPlacement(initiallyFocused: true, afterScrolling: true)
    }

    func testCaretPlacementTap_preservesComposedCharacterBoundaries() throws {
        try assertInteriorCaretPlacement(initiallyFocused: true, text: "abc👩🏽‍💻def", prefix: "abc👩🏽‍💻")
    }

    private func assertInteriorCaretPlacement(
        initiallyFocused: Bool,
        afterScrolling: Bool = false,
        text: String = "abcdefghijklmno",
        prefix: String = "abcdef"
    ) throws {
        let editorId = makeV2Editor()
        defer { destroyV2Editor(id: editorId) }
        let view = RichTextEditorView(frame: CGRect(x: 0, y: 0, width: 320, height: 240))
        view.editorId = editorId
        let textView = view.textView
        let before = afterScrolling ? String(repeating: "<p>Before</p>", count: 12) : ""
        textView.bindEditor(id: editorId, initialHTML: before + "<p>\(text)</p>")
        let window = hostEditorView(view)
        defer { window.isHidden = true }
        if initiallyFocused {
            XCTAssertTrue(textView.becomeFirstResponder())
        }
        textView.selectedRange = NSRange(location: 0, length: 0)
        flushMainQueue()
        textView.layoutIfNeeded()
        let wordRange = (textView.textStorage.string as NSString).range(of: text)
        XCTAssertNotEqual(wordRange.location, NSNotFound)
        let target = wordRange.location + prefix.utf16.count
        let position = try XCTUnwrap(textView.position(from: textView.beginningOfDocument, offset: target))
        let caret = textView.caretRect(for: position)
        if afterScrolling {
            textView.setContentOffset(CGPoint(x: 0, y: caret.midY - 100), animated: false)
            XCTAssertGreaterThan(textView.contentOffset.y, 0)
        }
        let point = CGPoint(x: caret.midX, y: caret.midY)
        XCTAssertTrue(textView.bounds.contains(point))
        XCTAssertTrue(textView.placeCaret(at: point))
        XCTAssertTrue(textView.isFirstResponder)
        XCTAssertEqual(textView.selectedRange, NSRange(location: target, length: 0))
        flushMainQueue()
        XCTAssertEqual(textView.selectedRange, NSRange(location: target, length: 0))
        let scalar = PositionBridge.utf16OffsetToScalar(target, in: textView)
        let doc = EditorV2Shadow.scalarToDoc(id: editorId, scalar: scalar)
        let selection = currentSelection(in: editorId)
        XCTAssertEqual((selection["anchor"] as? NSNumber)?.uint32Value, doc)
        XCTAssertEqual((selection["head"] as? NSNumber)?.uint32Value, doc)
    }

    func testCaretPlacementTap_doesNotFocusReadOnlyEditor() {
        let editorId = makeV2Editor()
        defer { destroyV2Editor(id: editorId) }
        let view = RichTextEditorView(frame: CGRect(x: 0, y: 0, width: 320, height: 240))
        view.editorId = editorId
        view.textView.bindEditor(id: editorId, initialHTML: "<p>abcdefghijklmno</p>")
        let window = hostEditorView(view)
        defer { window.isHidden = true }
        view.textView.isEditable = false
        let selection = view.textView.selectedRange
        XCTAssertFalse(view.textView.placeCaret(at: CGPoint(x: 55, y: 18)))
        XCTAssertFalse(view.textView.isFirstResponder)
        XCTAssertEqual(view.textView.selectedRange, selection)
    }

    func testResolveMentionQueryStateTriggersAfterSentencePunctuation() {
        let state = resolveMentionQueryState(
            in: "Testing.@",
            cursorScalar: 9,
            trigger: "@",
            isCaretInsideMention: false
        )

        XCTAssertEqual(
            state,
            MentionQueryState(query: "", trigger: "@", anchor: 8, head: 9)
        )
    }

    func testResolveMentionQueryStateSupportsHyphenatedQueries() {
        let state = resolveMentionQueryState(
            in: "@apollo-team",
            cursorScalar: 12,
            trigger: "@",
            isCaretInsideMention: false
        )

        XCTAssertEqual(
            state,
            MentionQueryState(query: "apollo-team", trigger: "@", anchor: 0, head: 12)
        )
    }

    func testManualSelectionInMiddleOfWordSyncsInteriorCaretPositionToRust() {
        let editorId = makeV2Editor()
        defer { destroyV2Editor(id: editorId) }

        let textView = EditorTextView(frame: CGRect(x: 0, y: 0, width: 320, height: 120))
        textView.bindEditor(id: editorId, initialHTML: "<p>Hello</p>")

        guard
            let start = textView.position(from: textView.beginningOfDocument, offset: 2),
            let range = textView.textRange(from: start, to: start)
        else {
            XCTFail("expected interior caret position")
            return
        }

        textView.selectedTextRange = range
        flushMainQueue()

        let selection = currentSelection(in: editorId)
        let expectedDoc = EditorV2Shadow.scalarToDoc(id: editorId, scalar: 2)

        XCTAssertEqual(selection["type"] as? String, "text")
        XCTAssertEqual((selection["anchor"] as? NSNumber)?.uint32Value, expectedDoc)
        XCTAssertEqual((selection["head"] as? NSNumber)?.uint32Value, expectedDoc)
    }

    func testManualSelectionIntoListItemRefreshesSelectionDependentActiveState() {
        let editorId = makeV2Editor()
        defer { destroyV2Editor(id: editorId) }

        let textView = EditorTextView(frame: CGRect(x: 0, y: 0, width: 320, height: 200))
        textView.bindEditor(
            id: editorId,
            initialHTML: "<p>Alpha</p><ul><li><p>Beta</p></li></ul>"
        )

        let plainOffset = (textView.attributedText.string as NSString).range(of: "Alpha").location
        let listOffset = (textView.attributedText.string as NSString).range(of: "Beta").location
        XCTAssertNotEqual(plainOffset, NSNotFound)
        XCTAssertNotEqual(listOffset, NSNotFound)

        setCollapsedSelection(in: textView, utf16Offset: plainOffset + 2)
        flushMainQueue()
        XCTAssertTrue(
            activeState(in: editorId).insertableNodes.contains("horizontal_rule"),
            "horizontal rule should be insertable in a normal paragraph"
        )

        setCollapsedSelection(in: textView, utf16Offset: listOffset + 2)
        flushMainQueue()
        XCTAssertFalse(
            activeState(in: editorId).insertableNodes.contains("horizontal_rule"),
            "horizontal rule should be disabled in list items after a manual caret move"
        )
    }

    func testManualSelectionInMiddleOfWordPersistsAfterDeferredSelectionSync() {
        let editorId = makeV2Editor()
        defer { destroyV2Editor(id: editorId) }

        let textView = EditorTextView(frame: CGRect(x: 0, y: 0, width: 320, height: 120))
        textView.bindEditor(id: editorId, initialHTML: "<p>Hello world</p>")

        setCollapsedSelection(in: textView, utf16Offset: 3)
        flushMainQueue()

        let actualOffset = textView.offset(
            from: textView.beginningOfDocument,
            to: textView.selectedTextRange?.start ?? textView.endOfDocument
        )
        XCTAssertEqual(
            actualOffset,
            3,
            "deferred selection sync should not snap the caret to a word boundary"
        )
    }

    func testManualSelectionAfterBlockquoteSyncsInteriorCaretPositionToRust() {
        let editorId = makeV2Editor()
        defer { destroyV2Editor(id: editorId) }

        let textView = EditorTextView(frame: CGRect(x: 0, y: 0, width: 320, height: 160))
        textView.bindEditor(
            id: editorId,
            initialHTML: "<blockquote><p>Hello</p></blockquote><p>World</p>"
        )

        let secondParagraphOffset = (textView.attributedText.string as NSString).range(of: "World").location
        XCTAssertNotEqual(secondParagraphOffset, NSNotFound)

        setCollapsedSelection(in: textView, utf16Offset: secondParagraphOffset + 3)
        flushMainQueue()

        let selection = currentSelection(in: editorId)
        let expectedDoc = EditorV2Shadow.scalarToDoc(id: editorId, scalar: UInt32(secondParagraphOffset + 3))

        XCTAssertEqual(selection["type"] as? String, "text")
        XCTAssertEqual((selection["anchor"] as? NSNumber)?.uint32Value, expectedDoc)
        XCTAssertEqual((selection["head"] as? NSNumber)?.uint32Value, expectedDoc)
    }

}

private enum SelectionEventActiveStateFixture {
    static let initialHTML = "<p>al<strong>pha b</strong>eta gamma</p>"
    static let replacedHTML = "<p>replaced paragraph text</p>"
    static let boldWord = "pha"
    static let plainWord = "gamma"
    static let replacedWord = "paragraph"
    static let wordInteriorOffset = 1
    static let nextCaretDistance = 2
}

private enum EmptyDocumentToolbarFixture {
    static let insertableNodesKey = "insertableNodes"
    static let imageNode = "image"
}

private final class SelectionEventStateRecorder {
    private(set) var states: [[String: Any]] = []

    init(_ host: NativeEditorExpoView) {
        host.onSelectionChangeForTesting = { [unowned self] event in
            let stateJSON = event["stateJson"] as? String ?? ""
            let state = try? JSONSerialization.jsonObject(with: Data(stateJSON.utf8)) as? [String: Any]
            states.append(state ?? [:])
        }
    }
}

extension RichTextEditorViewTests {
    func testSelectionEventsOmitAnUnchangedActiveStateUntilItChanges() throws {
        try withSelectionEventHost { host, recorder in
            let plain = try caretOffset(in: host, word: SelectionEventActiveStateFixture.plainWord)
            let bold = try caretOffset(in: host, word: SelectionEventActiveStateFixture.boldWord)
            let nextPlain = plain + SelectionEventActiveStateFixture.nextCaretDistance

            let first = try moveCaret(host, recorder, to: plain)
            let unchanged = try moveCaret(host, recorder, to: nextPlain)
            let boldState = try moveCaret(host, recorder, to: bold)
            let plainAgain = try moveCaret(host, recorder, to: plain)
            let repeated = try moveCaret(host, recorder, to: nextPlain)

            XCTAssertNotNil(first[EditorSelectionState.activeStateKey], "the first event carries the active state: \(first)")
            XCTAssertNil(unchanged[EditorSelectionState.activeStateKey], "an unchanged active state is omitted: \(unchanged)")
            XCTAssertEqual(
                unchanged[EditorSelectionState.activeStateRevisionKey] as? String,
                first["stateRevision"] as? String,
                "the omission names the revision that carried the active state"
            )
            let boldMarks = (boldState[EditorSelectionState.activeStateKey] as? [String: Any])?["marks"] as? [String: Any]
            XCTAssertEqual(boldMarks?["bold"] as? Bool, true, "a changed active state is sent again: \(boldState)")
            XCTAssertNotNil(plainAgain[EditorSelectionState.activeStateKey], "leaving bold sends the plain state: \(plainAgain)")
            XCTAssertNil(repeated[EditorSelectionState.activeStateKey], "\(repeated)")
            XCTAssertEqual(
                repeated[EditorSelectionState.activeStateRevisionKey] as? String,
                plainAgain["stateRevision"] as? String
            )
            XCTAssertNotEqual(host.toolbarState.marks["bold"], true, "the toolbar follows the final plain caret")
        }
    }

    func testDocumentReplacementFromJSAndRebindingResendTheActiveState() throws {
        try withSelectionEventHost { host, recorder in
            let editorId = host.richTextView.editorId
            let adapter = try XCTUnwrap(EditorV2Registry.adapter(forLegacyId: editorId))
            let plain = try caretOffset(in: host, word: SelectionEventActiveStateFixture.plainWord)
            _ = try moveCaret(host, recorder, to: plain)
            let omitted = try moveCaret(host, recorder, to: plain + SelectionEventActiveStateFixture.nextCaretDistance)
            XCTAssertNil(omitted[EditorSelectionState.activeStateKey], "\(omitted)")

            let replaced = adapter.callWithEnvelope(
                ["setHtml": SelectionEventActiveStateFixture.replacedHTML, "history": "undoableBoundary"],
                includeBaseRevision: false
            ) {
                editorV2ReplaceDocument(editorId: adapter.editorId, requestJson: $0)
            }
            let revision = try XCTUnwrap(parseJSONObject(replaced.value)["documentRevision"] as? String, "\(replaced)")
            XCTAssertTrue(host.applyEditorUpdate(#"{"documentVersion":"\#(revision)"}"#))
            flushMainQueue()
            let replacedCaret = try caretOffset(in: host, word: SelectionEventActiveStateFixture.replacedWord)
            let afterReplace = try moveCaret(host, recorder, to: replacedCaret)
            XCTAssertNotNil(afterReplace[EditorSelectionState.activeStateKey], "a JS replacement resets the channel: \(afterReplace)")

            host.setEditorId(0)
            host.setEditorId(editorId)
            flushMainQueue()
            let afterRebind = try moveCaret(
                host,
                recorder,
                to: replacedCaret + SelectionEventActiveStateFixture.nextCaretDistance
            )
            XCTAssertNotNil(afterRebind[EditorSelectionState.activeStateKey], "rebinding resets the channel: \(afterRebind)")
        }
    }

    func testFocusingALocalEmptyEditorKeepsAnActiveStateThatCanInsertAnImage() throws {
        let editorId = makeV2Editor()
        defer { destroyV2Editor(id: editorId) }
        let adapter = try XCTUnwrap(EditorV2Registry.adapter(forLegacyId: editorId))
        let window = makeTestWindow(frame: UIScreen.main.bounds)
        let host = NativeEditorExpoView()
        host.frame = window.bounds
        let recorder = SelectionEventStateRecorder(host)
        window.addSubview(host)
        window.makeKeyAndVisible()
        defer {
            host.blur()
            host.setEditorId(0)
            window.isHidden = true
        }
        host.setEditorId(editorId)
        flushMainQueue()

        host.focus()
        flushMainQueue()

        XCTAssertTrue(host.richTextView.activeTextInput.isFirstResponder, "focus reaches the editor")
        let renderState = parseJSONObject(
            editorV2RenderState(editorId: adapter.editorId, mirrorScalarAnchor: nil, mirrorScalarHead: nil).value
        )
        let activeState = try XCTUnwrap(
            renderState[EditorSelectionState.activeStateKey] as? [String: Any],
            "the render state JS seeds from: \(renderState)"
        )
        XCTAssertTrue(
            (activeState[EmptyDocumentToolbarFixture.insertableNodesKey] as? [String] ?? [])
                .contains(EmptyDocumentToolbarFixture.imageNode),
            "JS must be able to seed an image-capable state; focus published \(recorder.states)"
        )
        XCTAssertTrue(
            host.toolbarState.insertableNodes.contains(EmptyDocumentToolbarFixture.imageNode),
            "native toolbar state after focus: \(host.toolbarState.insertableNodes)"
        )
    }

    private func withSelectionEventHost(
        _ body: (NativeEditorExpoView, SelectionEventStateRecorder) throws -> Void
    ) throws {
        let editorId = makeV2Editor()
        defer { destroyV2Editor(id: editorId) }
        _ = EditorV2Shadow.setHtml(id: editorId, html: SelectionEventActiveStateFixture.initialHTML)
        let host = NativeEditorExpoView()
        defer { host.setEditorId(0) }
        host.setEditorId(editorId)
        try body(host, SelectionEventStateRecorder(host))
    }

    private func caretOffset(in host: NativeEditorExpoView, word: String) throws -> Int {
        let location = (host.richTextView.textView.attributedText.string as NSString).range(of: word).location
        XCTAssertNotEqual(location, NSNotFound, "\(word) is rendered")
        return location + SelectionEventActiveStateFixture.wordInteriorOffset
    }

    private func moveCaret(
        _ host: NativeEditorExpoView,
        _ recorder: SelectionEventStateRecorder,
        to utf16Offset: Int
    ) throws -> [String: Any] {
        let before = recorder.states.count
        setCollapsedSelection(in: host.richTextView.textView, utf16Offset: utf16Offset)
        flushMainQueue()
        XCTAssertGreaterThan(recorder.states.count, before, "caret \(utf16Offset) publishes a selection event")
        return try XCTUnwrap(recorder.states.last)
    }
}
