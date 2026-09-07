import ExpoModulesCore
import XCTest

private final class PendingCaretTapRecognizer: CaretPlacementTapRecognizer {
    var point: CGPoint?

    override var pendingCaretPoint: CGPoint? { point }
}

extension RichTextEditorViewTests {
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
