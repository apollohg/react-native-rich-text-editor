import XCTest

final class EditorCommandSequenceTests: XCTestCase {
    private static let editorSize = CGSize(width: 600, height: 500)
    private static let rootContext = "root"
    private static let boundaryBackspaceAllowance = 2
    private static let selectedDeletePresses = 1
    private static let placeholder = "\u{200B}"
    private static let objectReplacement = "\u{FFFC}"
    private static let quoteNode = "blockquote"
    private static let codeNode = "codeBlock"
    private static let headingNode = "h2"
    private static let atomicNodes = ["horizontalRule", "image", "mention", "hardBreak"]
    private static let listNodes = ["bulletList", "orderedList", "taskList"]
    private static let boldMark = "bold"
    private static let italicMark = "italic"
    private static let nodeContentOffset = 1
    private static let nodeBoundarySize = 2
    private static let cellContentDepth = 3
    private static let partialSuffixLength = 2
    private static let defaultListStart = 1
    private static let emptyContinuationUnicodeName = "surrogate"
    private static let multilineReplacement = "one\ntwo\nthree"

    func testRootNodePairsWithSelectionDeletion() throws { try runPairs(context: Self.rootContext, mode: "selection") }
    func testRootNodePairsWithRepeatedBackspace() throws { try runPairs(context: Self.rootContext, mode: "backspace") }
    func testBodyCellNodePairsWithSelectionDeletion() throws { try runPairs(context: "table_cell", mode: "selection") }
    func testBodyCellNodePairsWithRepeatedBackspace() throws { try runPairs(context: "table_cell", mode: "backspace") }
    func testHeaderCellNodePairsWithSelectionDeletion() throws { try runPairs(context: "table_header", mode: "selection") }
    func testHeaderCellNodePairsWithRepeatedBackspace() throws { try runPairs(context: "table_header", mode: "backspace") }
    func testRootUnicodeSequences() throws { try runUnicode(context: Self.rootContext) }
    func testBodyCellUnicodeSequences() throws { try runUnicode(context: "table_cell") }
    func testHeaderCellUnicodeSequences() throws { try runUnicode(context: "table_header") }
    func testRootReverseSelectionContinuation() throws { try runReverseSelection(context: Self.rootContext) }
    func testBodyCellReverseSelectionContinuation() throws { try runReverseSelection(context: "table_cell") }
    func testHeaderCellReverseSelectionContinuation() throws { try runReverseSelection(context: "table_header") }
    func testRootReturnJoinContinuation() throws { try runReturnJoin(context: Self.rootContext) }
    func testBodyCellReturnJoinContinuation() throws { try runReturnJoin(context: "table_cell") }
    func testHeaderCellReturnJoinContinuation() throws { try runReturnJoin(context: "table_header") }
    func testRootForwardBoundaryDeletionContinuation() throws { try runForwardBoundaryDeletion(context: Self.rootContext) }
    func testBodyCellForwardBoundaryDeletionContinuation() throws { try runForwardBoundaryDeletion(context: "table_cell") }
    func testHeaderCellForwardBoundaryDeletionContinuation() throws { try runForwardBoundaryDeletion(context: "table_header") }
    func testRootStoredMarkHistoryContinuation() throws { try runStoredMarks(context: Self.rootContext) }
    func testBodyCellStoredMarkHistoryContinuation() throws { try runStoredMarks(context: "table_cell") }
    func testHeaderCellStoredMarkHistoryContinuation() throws { try runStoredMarks(context: "table_header") }
    func testRootRichNodeClearHistoryContinuation() throws { try runRichHistory(context: Self.rootContext) }
    func testBodyCellRichNodeClearHistoryContinuation() throws { try runRichHistory(context: "table_cell") }
    func testHeaderCellRichNodeClearHistoryContinuation() throws { try runRichHistory(context: "table_header") }

    func testGraphemeBackspaceHistoryRestoresCaretAndSelectedDeletionRestoresRange() throws {
        let corpus = try corpus()
        let prefix = try XCTUnwrap(corpus["prefix"] as? String)
        let suffix = try XCTUnwrap(corpus["suffix"] as? String)
        let typed = try XCTUnwrap(corpus["probe"] as? String)
        let samples = try XCTUnwrap(corpus["unicode"] as? [[String: Any]]).filter {
            guard let text = $0["text"] as? String else { return false }
            return text.unicodeScalars.count > text.count
        }
        XCTAssertFalse(samples.isEmpty, "the history matrix requires a multi-scalar grapheme")
        for context in try XCTUnwrap(corpus["contexts"] as? [String]) {
            for sample in samples {
                let name = try XCTUnwrap(sample["name"] as? String)
                let text = try XCTUnwrap(sample["text"] as? String)
                let retained = try XCTUnwrap((sample["backspaces"] as? [String])?.first)
                let original = prefix + text + suffix
                let retainedPrefix = prefix + retained
                let caretOffset = prefix.utf16.count + text.utf16.count
                for selectedDeletion in [false, true] {
                    let mode = selectedDeletion ? "selected-range" : "collapsed-caret"
                    let label = context + "/grapheme-history/" + name + "/" + mode
                    try withSequenceEditor(corpus, context: context, inline: true, blocks: [paragraph(original)]) { fixture, neighbour in
                        let input = fixture.view.activeTextInput
                        let originalDocument = try json(document(fixture))
                        let originalSelection = NSRange(
                            location: selectedDeletion ? retainedPrefix.utf16.count : caretOffset,
                            length: selectedDeletion ? caretOffset - retainedPrefix.utf16.count : 0
                        )
                        try checkBlocks(fixture, context: context, blocks: [paragraph(original)], offset: 0,
                            neighbour: neighbour, label: label + "/initial")
                        input.selectedRange = originalSelection
                        input.deleteBackward()
                        try checkBlocks(fixture, context: context, blocks: [paragraph(retainedPrefix + suffix)],
                            offset: retainedPrefix.utf16.count, neighbour: neighbour, label: label + "/delete")
                        let deletedDocument = try json(document(fixture))
                        for stage in ["undo", "redo-undo"] {
                            if stage == "redo-undo" {
                                input.performToolbarRedo()
                                XCTAssertEqual(try json(document(fixture)), deletedDocument, label + "/redo exact document")
                                try checkBlocks(fixture, context: context, blocks: [paragraph(retainedPrefix + suffix)],
                                    offset: retainedPrefix.utf16.count, neighbour: neighbour, label: label + "/redo")
                            }
                            input.performToolbarUndo()
                            XCTAssertEqual(try json(document(fixture)), originalDocument, label + "/\(stage) exact document")
                            try check(fixture, context: context, expected: original, neighbour: neighbour, label: label + "/" + stage)
                            XCTAssertEqual(input.selectedRange, originalSelection, label + "/\(stage) original native selection")
                            let selection = try XCTUnwrap(fixture.adapter.cachedAtomicRenderSelection())
                            let textStart = (context == Self.rootContext ? 0 : Self.cellContentDepth) + Self.nodeContentOffset
                            for (endpoint, offset) in [("anchor", originalSelection.location), ("head", NSMaxRange(originalSelection))] {
                                let scalarOffset = (original as NSString).substring(to: offset).unicodeScalars.count
                                XCTAssertEqual(EditorV2Adapter.uint32Field(selection, endpoint), UInt32(textStart + scalarOffset),
                                    label + "/\(stage) original document \(endpoint)")
                            }
                        }
                        let continuedPrefix = selectedDeletion ? retainedPrefix : prefix + text
                        input.insertText(typed)
                        var remaining = typed
                        repeat {
                            try checkBlocks(fixture, context: context, blocks: [paragraph(continuedPrefix + remaining + suffix)],
                                offset: continuedPrefix.utf16.count + remaining.utf16.count, neighbour: neighbour,
                                label: label + "/continue/remaining=\(remaining.count)")
                            if remaining.isEmpty { break }
                            remaining.removeLast()
                            input.deleteBackward()
                        } while true
                    }
                }
            }
        }
    }

    func testEmptyScopeNoOpThenUnicodeReturnAndTypingContinuation() throws {
        let corpus = try corpus()
        let sample = try XCTUnwrap((corpus["unicode"] as? [[String: Any]])?.first {
            $0["name"] as? String == Self.emptyContinuationUnicodeName
        })
        let unicode = try XCTUnwrap(sample["text"] as? String)
        let typed = try XCTUnwrap(corpus["probe"] as? String)
        let recovery = try XCTUnwrap(corpus["recovery"] as? String)
        for context in try XCTUnwrap(corpus["contexts"] as? [String]) {
            let label = context + "/empty-no-op"
            try withSequenceEditor(corpus, context: context, inline: true, blocks: [paragraph("")]) { fixture, neighbour in
                let input = fixture.view.activeTextInput
                let emptyDocument = try json(document(fixture))
                try checkBlocks(fixture, context: context, blocks: [paragraph("")], offset: 0, neighbour: neighbour, label: label + "/initial")
                input.insertText("")
                try checkBlocks(fixture, context: context, blocks: [paragraph("")], offset: 0, neighbour: neighbour, label: label + "/empty-insert")
                XCTAssertEqual(try json(document(fixture)), emptyDocument, label + "/empty-insert exact document")
                input.deleteBackward()
                try checkBlocks(fixture, context: context, blocks: [paragraph("")], offset: 0, neighbour: neighbour, label: label + "/beginning-backspace")
                XCTAssertEqual(try json(document(fixture)), emptyDocument, label + "/beginning-backspace exact document")
                input.insertText(unicode)
                try checkBlocks(fixture, context: context, blocks: [paragraph(unicode)], offset: unicode.utf16.count,
                    neighbour: neighbour, label: label + "/unicode")
                input.insertText("\n")
                try checkBlocks(fixture, context: context, blocks: [paragraph(unicode), paragraph("")], paragraphIndex: Self.nodeContentOffset,
                    offset: 0, neighbour: neighbour, label: label + "/return")
                input.insertText(typed)
                var remaining = typed
                repeat {
                    try checkBlocks(fixture, context: context, blocks: [paragraph(unicode), paragraph(remaining)], paragraphIndex: Self.nodeContentOffset,
                        offset: remaining.utf16.count, neighbour: neighbour, label: label + "/continued-type/remaining=\(remaining.count)")
                    if remaining.isEmpty { break }
                    remaining.removeLast()
                    input.deleteBackward()
                } while true
                input.deleteBackward()
                try checkBlocks(fixture, context: context, blocks: [paragraph(unicode)], offset: unicode.utf16.count,
                    neighbour: neighbour, label: label + "/join")
                for remaining in try XCTUnwrap(sample["backspaces"] as? [String]) {
                    input.deleteBackward()
                    try checkBlocks(fixture, context: context, blocks: [paragraph(remaining)], offset: remaining.utf16.count,
                        neighbour: neighbour, label: label + "/unicode-backspace/remaining=\(String(reflecting: remaining))")
                }
                XCTAssertEqual(try json(document(fixture)), emptyDocument, label + "/unicode-backspace exact document")
                input.insertText(recovery)
                try checkBlocks(fixture, context: context, blocks: [paragraph(recovery)], offset: recovery.utf16.count,
                    neighbour: neighbour, label: label + "/recover")
            }
        }
    }

    func testSelectedHardBreakMultilineReplacementKeepsContinuationAndHistoryExact() throws {
        let corpus = try corpus()
        let prefix = try XCTUnwrap(corpus["prefix"] as? String)
        let suffix = try XCTUnwrap(corpus["suffix"] as? String)
        let typed = try XCTUnwrap(corpus["probe"] as? String)
        let recovery = try XCTUnwrap(corpus["recovery"] as? String)
        let lines = Self.multilineReplacement.components(separatedBy: "\n")
        for fullMarkedSelection in [false, true] {
            let selectionStart = fullMarkedSelection ? 0 : prefix.utf16.count - Self.partialSuffixLength
            let selectionEnd = prefix.utf16.count + "\n".utf16.count
                + (fullMarkedSelection ? suffix.utf16.count : Self.partialSuffixLength)
            let left = fullMarkedSelection ? "" : (prefix as NSString).substring(to: selectionStart)
            let right = fullMarkedSelection ? "" : (suffix as NSString).substring(from: Self.partialSuffixLength)
            let replacementMarks = fullMarkedSelection ? [Self.boldMark] : []
            let originalBlocks = [paragraph([textNode(prefix, marks: replacementMarks), ["type": "hardBreak"], textNode(suffix)])]
            let paragraphIndex = lines.count - Self.nodeContentOffset
            func expectedParagraph(_ text: String) -> [String: Any] {
                paragraph([textNode(text, marks: replacementMarks)])
            }
            func expectedBlocks(_ lastLine: String) -> [[String: Any]] {
                [expectedParagraph(left + lines[0])] + lines.dropFirst().dropLast().map(expectedParagraph)
                    + [expectedParagraph(lastLine + right)]
            }
            for context in try XCTUnwrap(corpus["contexts"] as? [String]) {
                let mode = fullMarkedSelection ? "full-marked" : "partial-plain"
                let label = context + "/selected-hard-break/" + mode + "/multiline"
                try withSequenceEditor(corpus, context: context, inline: true, blocks: originalBlocks) { fixture, neighbour in
                    let input = fixture.view.activeTextInput
                    let originalDocument = try json(document(fixture))
                    try check(fixture, context: context, expected: prefix + suffix, neighbour: neighbour, label: label + "/initial")
                    _ = try checkNativeTextMarks(input, text: prefix, marks: Set(replacementMarks),
                        offset: 0, label: label + "/initial-left")
                    _ = try checkNativeTextMarks(input, text: suffix, marks: [],
                        offset: prefix.utf16.count + "\n".utf16.count, label: label + "/initial-right")
                    let originalSelection = NSRange(location: selectionStart, length: selectionEnd - selectionStart)
                    input.selectedRange = originalSelection
                    input.insertText(Self.multilineReplacement)
                    let completeLastLine = try XCTUnwrap(lines.last)
                    try checkBlocks(fixture, context: context, blocks: expectedBlocks(completeLastLine), paragraphIndex: paragraphIndex,
                        offset: completeLastLine.utf16.count, neighbour: neighbour, label: label + "/replace")
                    let replacedDocument = try json(document(fixture))
                    var shortenedLastLine = completeLastLine
                    shortenedLastLine.removeLast()
                    input.deleteBackward()
                    try checkBlocks(fixture, context: context, blocks: expectedBlocks(shortenedLastLine), paragraphIndex: paragraphIndex,
                        offset: shortenedLastLine.utf16.count, neighbour: neighbour, label: label + "/immediate-backspace")
                    input.insertText(typed)
                    let continuedLastLine = shortenedLastLine + typed
                    try checkBlocks(fixture, context: context, blocks: expectedBlocks(continuedLastLine), paragraphIndex: paragraphIndex,
                        offset: continuedLastLine.utf16.count, neighbour: neighbour, label: label + "/continue")
                    let continuedDocument = try json(document(fixture))
                    input.performToolbarUndo()
                    try checkBlocks(fixture, context: context, blocks: expectedBlocks(shortenedLastLine), paragraphIndex: paragraphIndex,
                        offset: shortenedLastLine.utf16.count, neighbour: neighbour, label: label + "/undo-typing")
                    input.performToolbarRedo()
                    XCTAssertEqual(try json(document(fixture)), continuedDocument, label + "/redo-typing exact document")
                    try checkBlocks(fixture, context: context, blocks: expectedBlocks(continuedLastLine), paragraphIndex: paragraphIndex,
                        offset: continuedLastLine.utf16.count, neighbour: neighbour, label: label + "/redo-typing")
                    input.performToolbarUndo()
                    try checkBlocks(fixture, context: context, blocks: expectedBlocks(shortenedLastLine), paragraphIndex: paragraphIndex,
                        offset: shortenedLastLine.utf16.count, neighbour: neighbour, label: label + "/undo-typing-again")
                    input.performToolbarUndo()
                    XCTAssertEqual(try json(document(fixture)), replacedDocument, label + "/undo-backspace exact document")
                    try checkBlocks(fixture, context: context, blocks: expectedBlocks(completeLastLine), paragraphIndex: paragraphIndex,
                        offset: completeLastLine.utf16.count, neighbour: neighbour, label: label + "/undo-backspace")
                    input.performToolbarUndo()
                    XCTAssertEqual(try json(document(fixture)), originalDocument, label + "/undo-replacement exact rich document")
                    XCTAssertEqual(input.selectedRange, originalSelection, label + "/undo-replacement selected range")
                    try check(fixture, context: context, expected: prefix + suffix, neighbour: neighbour, label: label + "/undo-replacement")
                    _ = try checkNativeTextMarks(input, text: prefix, marks: Set(replacementMarks),
                        offset: 0, label: label + "/undo-replacement-left")
                    _ = try checkNativeTextMarks(input, text: suffix, marks: [],
                        offset: prefix.utf16.count + "\n".utf16.count, label: label + "/undo-replacement-right")
                    let restoredSelection = try XCTUnwrap(fixture.adapter.cachedAtomicRenderSelection())
                    let textStart = (context == Self.rootContext ? 0 : Self.cellContentDepth) + Self.nodeContentOffset
                    XCTAssertEqual(EditorV2Adapter.uint32Field(restoredSelection, "anchor"), UInt32(textStart + selectionStart), label + "/undo-replacement anchor")
                    XCTAssertEqual(EditorV2Adapter.uint32Field(restoredSelection, "head"), UInt32(textStart + selectionEnd), label + "/undo-replacement head")
                    for (stage, line) in [("replacement", completeLastLine), ("backspace", shortenedLastLine), ("typing", continuedLastLine)] {
                        input.performToolbarRedo()
                        try checkBlocks(fixture, context: context, blocks: expectedBlocks(line), paragraphIndex: paragraphIndex,
                            offset: line.utf16.count, neighbour: neighbour, label: label + "/redo-\(stage)")
                    }
                    XCTAssertEqual(try json(document(fixture)), continuedDocument, label + "/redo-all exact document")
                    input.insertText(recovery)
                    try checkBlocks(fixture, context: context, blocks: expectedBlocks(continuedLastLine + recovery), paragraphIndex: paragraphIndex,
                        offset: continuedLastLine.utf16.count + recovery.utf16.count, neighbour: neighbour, label: label + "/recover")
                }
            }
        }
    }

    func testPartialSelectionAcrossHardBreakAndParagraphKeepsEditingAndHistoryExact() throws {
        let corpus = try corpus()
        let leftText = "abc"
        let middleText = "def"
        let rightText = "ghi"
        let replacement = "X"
        let typed = try XCTUnwrap(corpus["probe"] as? String)
        let originalBlocks = [paragraph([textNode(leftText), ["type": "hardBreak"], textNode(middleText)]), paragraph(rightText)]
        let retainedCharacterCount = 1
        let nativeSeparatorLength = "\n".utf16.count
        let selectionStart = retainedCharacterCount
        let selectionEnd = leftText.utf16.count + nativeSeparatorLength + middleText.utf16.count
            + nativeSeparatorLength + rightText.utf16.count - retainedCharacterCount
        let retainedLeft = (leftText as NSString).substring(to: selectionStart)
        let retainedRight = (rightText as NSString).substring(from: rightText.utf16.count - retainedCharacterCount)
        let originalSelection = NSRange(location: selectionStart, length: selectionEnd - selectionStart)
        for context in try XCTUnwrap(corpus["contexts"] as? [String]) {
            for inserted in ["", replacement] {
                let mode = inserted.isEmpty ? "delete" : "replace"
                let label = context + "/partial-hard-break-and-paragraph/" + mode
                try withSequenceEditor(corpus, context: context, inline: true, blocks: originalBlocks) { fixture, neighbour in
                    let input = fixture.view.activeTextInput
                    let originalDocument = try json(document(fixture))
                    try check(fixture, context: context, expected: leftText + middleText + rightText,
                        neighbour: neighbour, label: label + "/initial")
                    input.selectedRange = originalSelection
                    if inserted.isEmpty { input.deleteBackward() }
                    else { input.insertText(inserted) }
                    let prefix = retainedLeft + inserted
                    let resultBlocks = [paragraph(prefix + retainedRight)]
                    try checkBlocks(fixture, context: context, blocks: resultBlocks, offset: prefix.utf16.count,
                        neighbour: neighbour, label: label + "/mutate")
                    let resultDocument = try json(document(fixture))
                    input.performToolbarUndo()
                    XCTAssertEqual(try json(document(fixture)), originalDocument, label + "/undo exact rich document")
                    XCTAssertEqual(input.selectedRange, originalSelection, label + "/undo selected range")
                    try check(fixture, context: context, expected: leftText + middleText + rightText,
                        neighbour: neighbour, label: label + "/undo")
                    let selection = try XCTUnwrap(fixture.adapter.cachedAtomicRenderSelection())
                    let contentStart = context == Self.rootContext ? 0 : Self.cellContentDepth
                    XCTAssertEqual(EditorV2Adapter.uint32Field(selection, "anchor"),
                        UInt32(contentStart + Self.nodeContentOffset + selectionStart), label + "/undo document anchor")
                    XCTAssertEqual(EditorV2Adapter.uint32Field(selection, "head"),
                        UInt32(contentStart + nodeSize(originalBlocks[0]) + Self.nodeContentOffset
                            + rightText.unicodeScalars.count - retainedCharacterCount), label + "/undo document head")
                    input.performToolbarRedo()
                    XCTAssertEqual(try json(document(fixture)), resultDocument, label + "/redo exact document")
                    try checkBlocks(fixture, context: context, blocks: resultBlocks, offset: prefix.utf16.count,
                        neighbour: neighbour, label: label + "/redo")
                    input.insertText(typed)
                    var remaining = typed
                    repeat {
                        try checkBlocks(fixture, context: context, blocks: [paragraph(prefix + remaining + retainedRight)],
                            offset: prefix.utf16.count + remaining.utf16.count, neighbour: neighbour,
                            label: label + "/continued-type/remaining=\(remaining.count)")
                        if remaining.isEmpty { break }
                        remaining.removeLast()
                        input.deleteBackward()
                    } while true
                    XCTAssertEqual(try json(document(fixture)), resultDocument, label + "/probe restored exact document")
                }
            }
        }
    }

    func testClosedSequenceFixtureReleasesNativeViewsAfterFocusMoves() throws {
        let corpus = try corpus()
        weak var retainedEditor: RichTextEditorView?
        weak var retainedRootInput: EditorTextView?
        weak var retainedCellInput: EditorTextView?
        weak var retainedWindow: UIWindow?
        let editorId = try autoreleasepool { () throws -> UInt64 in
            let fixture = try makeMountedEditorFixture(config: json(XCTUnwrap(corpus["config"])),
                document: initialDocument(corpus, context: "table_cell", inline: true), size: Self.editorSize, windowed: true)
            XCTAssertTrue(fixture.view.bindTableCell(tableID: try fixture.adapter.editableTableID(), cellIndex: 0, contentRect: .zero))
            XCTAssertTrue(fixture.view.activeTextInput.becomeFirstResponder())
            fixture.view.activeTextInput.insertText(try XCTUnwrap(corpus["probe"] as? String))
            retainedEditor = fixture.view
            retainedRootInput = fixture.view.textView
            retainedCellInput = fixture.view.activeTextInput
            retainedWindow = fixture.window
            fixture.close()
            return fixture.editorId
        }
        XCTAssertEqual(retainedCellInput?.editorId, 0, "closed fixture left its cell input bound")
        XCTAssertNil(retainedCellInput?.tableCellPositionMap, "closed fixture left a cell projection active")
        XCTAssertFalse(retainedCellInput?.isFirstResponder == true, "closed fixture left its cell input focused")
        let next = try makeMountedEditorFixture(config: json(XCTUnwrap(corpus["config"])),
            document: #"{"type":"doc","content":[{"type":"paragraph"}]}"#, size: Self.editorSize, windowed: true)
        defer { next.close() }
        XCTAssertTrue(next.view.activeTextInput.becomeFirstResponder())
        let released = XCTNSPredicateExpectation(predicate: NSPredicate { _, _ in
            retainedEditor == nil && retainedRootInput == nil && retainedCellInput == nil && retainedWindow == nil
        }, object: nil)
        wait(for: [released], timeout: MountedEditorTestFixture.releaseTimeout)
        XCTAssertNil(EditorV2Registry.adapter(forLegacyId: editorId), "closed fixture left its Rust adapter registered")
        XCTAssertNil(retainedEditor, "closed fixture retained its editor view")
        XCTAssertNil(retainedRootInput, "closed fixture retained its root input")
        XCTAssertNil(retainedCellInput, "closed fixture retained its cell input")
        XCTAssertNil(retainedWindow, "closed fixture retained its window")
    }

    func testInsertedBlockquoteReplacesItsTrailingPlaceholderWhenTyping() throws {
        let corpus = try corpus()
        let fragment = try XCTUnwrap((corpus["fragments"] as? [[String: Any]])?.first { $0["name"] as? String == Self.quoteNode })
        let middle = try XCTUnwrap(fragment["text"] as? String)
        let typed = try XCTUnwrap(corpus["probe"] as? String)
        let fixture = try makeMountedEditorFixture(config: json(XCTUnwrap(corpus["config"])),
            document: #"{"type":"doc","content":[{"type":"paragraph"}]}"#, size: Self.editorSize, windowed: true)
        defer { fixture.close() }
        let input = fixture.view.activeTextInput
        XCTAssertTrue(input.becomeFirstResponder())
        let selection = try XCTUnwrap(input.currentLogicalScalarSelection())
        XCTAssertTrue(input.applyUpdateJSON(try XCTUnwrap(fixture.adapter.insertContentJson(
            json(["type": "doc", "content": [try XCTUnwrap(fragment["node"])]]),
            anchor: selection.anchor, head: selection.head))))
        try check(fixture, context: Self.rootContext, expected: middle, neighbour: nil, label: "quote/insert")
        let emptyDocument = try json(document(fixture))
        XCTAssertEqual(input.textStorage.string, middle + "\n" + Self.placeholder)
        XCTAssertEqual(input.currentTopLevelChildMetadata?.map(\.startOffset), [0, middle.utf16.count],
            "the trailing paragraph's replacement range must include its leading separator and placeholder")
        input.insertText(typed)
        try check(fixture, context: Self.rootContext, expected: middle + typed, neighbour: nil, label: "quote/type")
        XCTAssertEqual(input.selectedRange, NSRange(location: middle.utf16.count + 1 + typed.utf16.count, length: 0),
            "quote/type rendered=\(String(reflecting: input.textStorage.string)) metadata=\(String(describing: input.currentTopLevelChildMetadata))")
        var remaining = typed
        while !remaining.isEmpty {
            remaining.removeLast()
            input.deleteBackward()
            try check(fixture, context: Self.rootContext, expected: middle + remaining, neighbour: nil,
                label: "quote/backspace/remaining=\(remaining.count)")
        }
        XCTAssertEqual(try json(document(fixture)), emptyDocument, "backspace must restore the exact quoted document")
    }

    private func corpus() throws -> [String: Any] {
        let url = try XCTUnwrap(Bundle(for: Self.self).url(forResource: "editor-command-sequences", withExtension: "json"))
        return try XCTUnwrap(JSONSerialization.jsonObject(with: Data(contentsOf: url)) as? [String: Any])
    }

    private func json(_ value: Any) throws -> String {
        try XCTUnwrap(String(data: JSONSerialization.data(withJSONObject: value, options: .sortedKeys), encoding: .utf8))
    }

    private func descendants(_ node: [String: Any]) -> [[String: Any]] {
        [node] + (node["content"] as? [[String: Any]] ?? []).flatMap(descendants)
    }

    private func document(_ fixture: MountedEditorTestFixture) throws -> [String: Any] {
        let raw = try XCTUnwrap(fixture.adapter.documentJson())
        return try XCTUnwrap(JSONSerialization.jsonObject(with: Data(raw.utf8)) as? [String: Any])
    }

    private func scope(_ document: [String: Any], context: String) throws -> [String: Any] {
        if context == Self.rootContext { return document }
        let tables = try XCTUnwrap(document["content"] as? [[String: Any]])
        XCTAssertEqual(tables.count, 1)
        let rows = try XCTUnwrap(tables.first?["content"] as? [[String: Any]])
        XCTAssertEqual(rows.count, 1)
        let cells = try XCTUnwrap(rows.first?["content"] as? [[String: Any]])
        XCTAssertEqual(cells.count, 2)
        return try XCTUnwrap(cells.first)
    }

    private func initialDocument(_ corpus: [String: Any], context: String, inline: Bool) throws -> String {
        let prefix = try XCTUnwrap(corpus["prefix"] as? String)
        let suffix = try XCTUnwrap(corpus["suffix"] as? String)
        let paragraphs = inline ? [paragraph(prefix + suffix)] : [paragraph(prefix), paragraph(suffix)]
        return try scopedDocument(corpus, context: context, blocks: paragraphs)
    }

    private func textNode(_ text: String, marks: [String] = []) -> [String: Any] {
        var node: [String: Any] = ["type": "text", "text": text]
        if !marks.isEmpty { node["marks"] = marks.map { ["type": $0] } }
        return node
    }

    private func paragraph(_ text: String) -> [String: Any] {
        paragraph(text.isEmpty ? [] : [textNode(text)])
    }

    private func paragraph(_ content: [[String: Any]]) -> [String: Any] {
        var node: [String: Any] = ["type": "paragraph"]
        if !content.isEmpty { node["content"] = content }
        return node
    }

    private func scopedDocument(_ corpus: [String: Any], context: String, blocks: [[String: Any]]) throws -> String {
        let documentBlocks: [[String: Any]] = context == Self.rootContext ? blocks : [[
            "type": "table", "content": [["type": "table_row", "content": [
                ["type": context, "content": blocks],
                ["type": "table_cell", "content": [paragraph(try XCTUnwrap(corpus["neighbour"] as? String))]]
            ]]]
        ]]
        return try json(["type": "doc", "content": documentBlocks])
    }

    private func check(
        _ fixture: MountedEditorTestFixture, context: String, expected: String, neighbour: String?, label: String
    ) throws {
        fixture.view.layoutIfNeeded()
        let input = fixture.view.activeTextInput
        input.layoutIfNeeded()
        let object = try document(fixture)
        let currentScope = try scope(object, context: context)
        let actual = descendants(currentScope).compactMap { $0["text"] as? String }.joined()
        XCTAssertEqual(actual, expected, "\(label): document=\(object) input=\(input.textStorage.string) selection=\(input.selectedRange)")
        try checkNativeRendering(input, node: currentScope, context: context, label: label)
        if context != Self.rootContext {
            XCTAssertEqual(try scope(object, context: context)["type"] as? String, context, "\(label): target cell type changed")
            XCTAssertFalse(input === fixture.view.textView, "\(label): cell input retired")
            XCTAssertEqual(input.tableCellPositionMap?.binding.cellIndex, 0, "\(label): wrong active cell")
            let table = try XCTUnwrap((object["content"] as? [[String: Any]])?.first)
            let row = try XCTUnwrap((table["content"] as? [[String: Any]])?.first)
            let cells = try XCTUnwrap(row["content"] as? [[String: Any]])
            XCTAssertEqual(try json(cells[1]), neighbour, "\(label): neighbour changed")
        }
        XCTAssertTrue(input.isAuthorizedForTableCellInput(), "\(label): stale input authority")
        XCTAssertTrue(NSMaxRange(input.selectedRange) <= input.textStorage.length, "\(label): caret beyond native text")
        let logical = try XCTUnwrap(input.currentLogicalScalarSelection(), "\(label): unmappable native caret")
        let selection = try XCTUnwrap(fixture.adapter.cachedAtomicRenderSelection())
        XCTAssertEqual(EditorV2Adapter.uint32Field(selection, "anchor"), fixture.adapter.documentPosition(forScalar: logical.anchor), "\(label): anchor disagrees with engine")
        XCTAssertEqual(EditorV2Adapter.uint32Field(selection, "head"), fixture.adapter.documentPosition(forScalar: logical.head), "\(label): head disagrees with engine")
        if input.selectedRange.length == 0 {
            let position = try XCTUnwrap(input.selectedTextRange?.end)
            let caret = input.caretRect(for: position)
            XCTAssertTrue([caret.minX, caret.minY, caret.width, caret.height].allSatisfy(\.isFinite), "\(label): non-finite caret \(caret)")
            XCTAssertGreaterThan(caret.height, 0, "\(label): collapsed caret")
            XCTAssertTrue(input.bounds.insetBy(dx: -caret.width, dy: -caret.height).intersects(caret), "\(label): invisible caret \(caret) bounds=\(input.bounds)")
        }
        for token in expected.isEmpty ? [] : [expected] {
            if token.rangeOfCharacter(from: .newlines) == nil {
                // List markers and paragraph separators may split the visible string.
                for part in ["left", "middle", "typed", "right", "again"] where token.contains(part) {
                    XCTAssertTrue(input.textStorage.string.contains(part), "\(label): visible text missing \(part)")
                }
            }
        }
    }

    private func probe(
        _ fixture: MountedEditorTestFixture, corpus: [String: Any], context: String,
        expected: String, neighbour: String?, label: String
    ) throws {
        let input = fixture.view.activeTextInput
        let before = try json(document(fixture))
        let beforeSelection = try XCTUnwrap(fixture.adapter.cachedAtomicRenderSelection())
        let caret = input.caretRect(for: try XCTUnwrap(input.selectedTextRange?.end))
        let text = try XCTUnwrap(corpus["probe"] as? String)
        input.insertText(text)
        try check(fixture, context: context, expected: expected, neighbour: neighbour, label: label + "/type")
        let visible = (input.textStorage.string as NSString).range(of: text)
        XCTAssertNotEqual(visible.location, NSNotFound, "\(label): probe invisible")
        XCTAssertEqual(input.selectedRange, NSRange(location: NSMaxRange(visible), length: 0), "\(label): typing left caret behind rendered=\(String(reflecting: input.textStorage.string)) selection=\(String(describing: fixture.adapter.cachedAtomicRenderSelection())) logical=\(String(describing: input.currentLogicalScalarSelection()))")
        XCTAssertNotEqual(input.caretRect(for: try XCTUnwrap(input.selectedTextRange?.end)), caret, "\(label): visual caret stuck")
        let probeRange = try XCTUnwrap(expected.range(of: text, options: .backwards), "\(label): expected probe missing")
        let left = String(expected[..<probeRange.lowerBound])
        let right = String(expected[probeRange.upperBound...])
        var remaining = text
        while !remaining.isEmpty {
            remaining.removeLast()
            input.deleteBackward()
            try check(fixture, context: context, expected: left + remaining + right, neighbour: neighbour,
                label: label + "/backspace/remaining=\(remaining.count)")
        }
        XCTAssertEqual(try json(document(fixture)), before, "\(label): typing/backspace changed structure or neighbours")
        let afterSelection = try XCTUnwrap(fixture.adapter.cachedAtomicRenderSelection())
        for endpoint in ["anchor", "head"] {
            XCTAssertEqual(EditorV2Adapter.uint32Field(afterSelection, endpoint), EditorV2Adapter.uint32Field(beforeSelection, endpoint), "\(label): typing/backspace did not restore \(endpoint)")
        }
    }

    private func clear(_ fixture: MountedEditorTestFixture, context: String, mode: String, name: String, preservesFormatting: Bool, label: String) throws {
        let input = fixture.view.activeTextInput
        input.selectedRange = NSRange(location: mode == "selection" ? 0 : input.textStorage.length,
                                      length: mode == "selection" ? input.textStorage.length : 0)
        let budget = mode == "selection" ? Self.selectedDeletePresses : input.textStorage.length +
            descendants(try scope(document(fixture), context: context)).count * Self.boundaryBackspaceAllowance
        for press in 0..<budget {
            input.deleteBackward()
            let currentScope = try scope(document(fixture), context: context)
            try checkNativeRendering(input, node: currentScope, context: context, label: label + "/clear/press=\(press)")
            let nodes = descendants(currentScope)
            if !nodes.contains(where: { $0["type"] as? String == "text" || $0["type"] as? String == name }) { return }
            if mode == "selection", preservesFormatting,
               !nodes.contains(where: { $0["type"] as? String == "text" }) { return }
        }
        XCTFail("\(label): deletion exhausted \(budget) presses: \(try document(fixture))")
    }

    private func inlineText(_ node: [String: Any]) -> String {
        switch node["type"] as? String {
        case "text": return node["text"] as? String ?? ""
        case "hardBreak": return "\n"
        case "mention": return (node["attrs"] as? [String: Any])?["label"] as? String ?? ""
        default: return (node["content"] as? [[String: Any]] ?? []).map(inlineText).joined()
        }
    }

    private func nativeBlocks(_ node: [String: Any]) -> [String] {
        switch node["type"] as? String {
        case "horizontalRule", "image": return [Self.objectReplacement]
        case "paragraph", Self.headingNode, Self.codeNode:
            let text = inlineText(node)
            let children = node["content"] as? [[String: Any]] ?? []
            let endsInBreak = children.last?["type"] as? String == "hardBreak"
                || node["type"] as? String == Self.codeNode && text.hasSuffix("\n")
            return [text.isEmpty ? Self.placeholder : text + (endsInBreak ? Self.placeholder : "")]
        default: return (node["content"] as? [[String: Any]] ?? []).flatMap(nativeBlocks)
        }
    }

    private func atomicDocumentPositions(_ node: [String: Any], start: Int) -> [String: Set<UInt32>] {
        let type = node["type"] as? String ?? ""
        if Self.atomicNodes.contains(type) { return [type: [UInt32(start)]] }
        var positions: [String: Set<UInt32>] = [:]
        var childStart = start + (type == "doc" ? 0 : Self.nodeContentOffset)
        for child in node["content"] as? [[String: Any]] ?? [] {
            for (type, values) in atomicDocumentPositions(child, start: childStart) {
                positions[type, default: []].formUnion(values)
            }
            childStart += nodeSize(child)
        }
        return positions
    }

    private func expectedListMarkers(_ node: [String: Any]) throws -> Set<String> {
        var markers: Set<String> = []
        for list in descendants(node) where Self.listNodes.contains(list["type"] as? String ?? "") {
            let type = try XCTUnwrap(list["type"] as? String)
            let ordered = type == "orderedList"
            let start = ((list["attrs"] as? [String: Any])?["start"] as? NSNumber)?.intValue ?? Self.defaultListStart
            let items = list["content"] as? [[String: Any]] ?? []
            for (index, item) in items.enumerated() {
                var marker: [String: Any] = ["ordered": ordered, "index": ordered ? start + index : index + Self.defaultListStart,
                    "start": start, "total": items.count, "isFirst": index == 0, "isLast": index == items.count - Self.nodeContentOffset]
                if type == "taskList" {
                    marker["kind"] = "task"
                    marker["checked"] = ((item["attrs"] as? [String: Any])?["checked"] as? NSNumber)?.boolValue == true
                }
                markers.insert(try json(marker))
            }
        }
        return markers
    }

    private func checkNativeRendering(_ input: EditorTextView, node: [String: Any], context: String, label: String) throws {
        let nodes = descendants(node)
        let types = Set(nodes.compactMap { $0["type"] as? String })
        XCTAssertEqual(input.textStorage.string, nativeBlocks(node).joined(separator: "\n"),
            "\(label): native text differs from the document: \(String(reflecting: input.textStorage.string))")
        var atomicPositions: [String: Set<UInt32>] = [:]
        var renderedLists: Set<String> = []
        var renderedMarkerContexts: [[String: Any]] = []
        var decorationRanges: [String: [NSRange]] = [:]
        var imagePositions: Set<UInt32> = []
        var rulePositions: Set<UInt32> = []
        input.textStorage.enumerateAttributes(in: NSRange(location: 0, length: input.textStorage.length)) { attrs, range, _ in
            let position = (attrs[RenderBridgeAttributes.docPos] as? NSNumber)?.uint32Value
            if let type = attrs[RenderBridgeAttributes.voidNodeType] as? String, let position {
                atomicPositions[type, default: []].insert(position)
            }
            if let position, attrs[.attachment] is BlockImageAttachment { imagePositions.insert(position) }
            if let position, attrs[.attachment] is HorizontalRuleAttachment { rulePositions.insert(position) }
            if let list = attrs[RenderBridgeAttributes.listMarkerContext] as? [String: Any] {
                let type = list["kind"] as? String == "task" ? "taskList"
                    : ((list["ordered"] as? NSNumber)?.boolValue == true ? "orderedList" : "bulletList")
                renderedLists.insert(type)
                var marker = list.filter { ["ordered", "index", "start", "total", "isFirst", "isLast"].contains($0.key) }
                if let kind = list["kind"] as? String { marker["kind"] = kind }
                if let checked = list["checked"] as? NSNumber { marker["checked"] = checked.boolValue }
                renderedMarkerContexts.append(marker)
                decorationRanges[type, default: []].append(range)
            }
            for (type, key) in [(Self.quoteNode, RenderBridgeAttributes.blockquoteBorderWidth),
                (Self.codeNode, RenderBridgeAttributes.codeBlockBackgroundColor)] where attrs[key] != nil {
                decorationRanges[type, default: []].append(range)
            }
            if attrs[RenderBridgeAttributes.blockNodeType] as? String == Self.headingNode {
                decorationRanges[Self.headingNode, default: []].append(range)
            }
        }
        let expectedPositions = atomicDocumentPositions(node, start: context == Self.rootContext ? 0 : Self.cellContentDepth - Self.nodeContentOffset)
        for type in Self.atomicNodes {
            XCTAssertEqual(atomicPositions[type] ?? [], expectedPositions[type] ?? [], "\(label): stale native \(type) positions")
        }
        XCTAssertEqual(imagePositions.count, nodes.filter { $0["type"] as? String == "image" }.count,
            "\(label): stale native image attachments")
        XCTAssertEqual(rulePositions.count, nodes.filter { $0["type"] as? String == "horizontalRule" }.count,
            "\(label): stale native rule attachments")
        XCTAssertEqual(renderedLists, types.intersection(Self.listNodes), "\(label): stale native list markers")
        XCTAssertEqual(try Set(renderedMarkerContexts.map(json)), try expectedListMarkers(node),
            "\(label): stale native list numbering, marker kind, or checkbox state")
        input.layoutManager.ensureLayout(for: input.textContainer)
        for type in Self.listNodes + [Self.quoteNode, Self.codeNode, Self.headingNode] {
            let ranges = decorationRanges[type] ?? []
            XCTAssertEqual(!ranges.isEmpty, types.contains(type), "\(label): stale native \(type) decoration")
            for range in ranges {
                let glyphs = input.layoutManager.glyphRange(forCharacterRange: range, actualCharacterRange: nil)
                let bounds = input.layoutManager.boundingRect(forGlyphRange: glyphs, in: input.textContainer)
                XCTAssertGreaterThan(bounds.height, 0, "\(label): collapsed native \(type)")
            }
        }
    }

    private func withSequenceEditor(
        _ corpus: [String: Any], context: String, inline: Bool, blocks: [[String: Any]]? = nil,
        _ body: (MountedEditorTestFixture, String?) throws -> Void
    ) throws {
        try autoreleasepool {
            let seed = try blocks.map { try scopedDocument(corpus, context: context, blocks: $0) }
                ?? initialDocument(corpus, context: context, inline: inline)
            let fixture = try makeMountedEditorFixture(config: json(XCTUnwrap(corpus["config"])),
                                                       document: seed, size: Self.editorSize, windowed: true)
            defer { fixture.close() }
            var neighbour: String?
            if context != Self.rootContext {
                XCTAssertTrue(fixture.view.bindTableCell(tableID: try fixture.adapter.editableTableID(), cellIndex: 0, contentRect: .zero))
                let table = try XCTUnwrap((document(fixture)["content"] as? [[String: Any]])?.first)
                let row = try XCTUnwrap((table["content"] as? [[String: Any]])?.first)
                neighbour = try json(XCTUnwrap((row["content"] as? [[String: Any]])?[1]))
            }
            XCTAssertTrue(fixture.view.activeTextInput.becomeFirstResponder())
            try body(fixture, neighbour)
        }
    }

    private func nodeSize(_ node: [String: Any]) -> Int {
        if node["type"] as? String == "text" { return (node["text"] as? String ?? "").unicodeScalars.count }
        if Self.atomicNodes.contains(node["type"] as? String ?? "") { return Self.nodeContentOffset }
        return Self.nodeBoundarySize + (node["content"] as? [[String: Any]] ?? []).reduce(0) { $0 + nodeSize($1) }
    }

    private func checkNativeTextMarks(
        _ input: EditorTextView, text: String, marks: Set<String>, offset: Int, label: String
    ) throws -> Bool {
        guard offset + text.utf16.count <= input.textStorage.length else {
            XCTFail("\(label): expected font run extends beyond the stale native text at \(offset)")
            return false
        }
        for position in offset..<(offset + text.utf16.count) {
            let font = try XCTUnwrap(input.textStorage.attribute(.font, at: position, effectiveRange: nil) as? UIFont)
            XCTAssertEqual(font.fontDescriptor.symbolicTraits.contains(.traitBold), marks.contains(Self.boldMark),
                "\(label): stale bold font at native offset \(position)")
            XCTAssertEqual(font.fontDescriptor.symbolicTraits.contains(.traitItalic), marks.contains(Self.italicMark),
                "\(label): stale italic font at native offset \(position)")
        }
        return true
    }

    private func checkBlocks(
        _ fixture: MountedEditorTestFixture, context: String, blocks: [[String: Any]],
        paragraphIndex: Int = 0, offset: Int, neighbour: String?, label: String
    ) throws {
        let expected = blocks.flatMap(descendants).compactMap { $0["text"] as? String }.joined()
        try check(fixture, context: context, expected: expected, neighbour: neighbour, label: label)
        XCTAssertEqual(try json(XCTUnwrap(try scope(document(fixture), context: context)["content"])), try json(blocks),
            "\(label): exact scoped nodes or marks changed")
        let contentStart = context == Self.rootContext ? 0 : Self.cellContentDepth
        let scalarOffset = (inlineText(blocks[paragraphIndex]) as NSString).substring(to: offset).unicodeScalars.count
        let docCaret = contentStart + blocks.prefix(paragraphIndex).reduce(0) { $0 + nodeSize($1) }
            + Self.nodeContentOffset + scalarOffset
        let selection = try XCTUnwrap(fixture.adapter.cachedAtomicRenderSelection())
        for endpoint in ["anchor", "head"] {
            XCTAssertEqual(EditorV2Adapter.uint32Field(selection, endpoint), UInt32(docCaret),
                "\(label): concrete document \(endpoint) is wrong")
        }
        let nativeCaret = blocks.prefix(paragraphIndex).reduce(0) {
            $0 + nativeBlocks($1).joined(separator: "\n").utf16.count + Self.nodeContentOffset
        } + offset + (blocks.count > Self.nodeContentOffset &&
            (blocks[paragraphIndex]["content"] as? [[String: Any]] ?? []).isEmpty ? Self.placeholder.utf16.count : 0)
        let input = fixture.view.activeTextInput
        XCTAssertEqual(input.selectedRange, NSRange(location: nativeCaret, length: 0), "\(label): concrete native caret is wrong")
        var nativeOffset = 0
        for block in blocks {
            for run in block["content"] as? [[String: Any]] ?? [] {
                let text = try XCTUnwrap(run["text"] as? String, "\(label): expected a text run")
                let marks = Set((run["marks"] as? [[String: Any]] ?? []).compactMap { $0["type"] as? String })
                guard try checkNativeTextMarks(input, text: text, marks: marks, offset: nativeOffset, label: label) else { return }
                nativeOffset += text.utf16.count
            }
            if (block["content"] as? [[String: Any]] ?? []).isEmpty { nativeOffset += Self.placeholder.utf16.count }
            nativeOffset += Self.nodeContentOffset
        }
    }

    private func runReverseSelection(context: String) throws {
        let corpus = try corpus()
        let prefix = try XCTUnwrap(corpus["prefix"] as? String)
        let original = prefix + (try XCTUnwrap(corpus["suffix"] as? String))
        let recovery = try XCTUnwrap(corpus["recovery"] as? String)
        let typed = try XCTUnwrap(corpus["probe"] as? String)
        let start = prefix.utf16.count - Self.nodeContentOffset
        let end = prefix.utf16.count + Self.partialSuffixLength
        let left = (original as NSString).substring(to: start)
        let right = (original as NSString).substring(from: end)
        try withSequenceEditor(corpus, context: context, inline: true) { fixture, neighbour in
            let input = fixture.view.activeTextInput
            let anchor = try XCTUnwrap(input.inputScalar(atLocalScalar: UInt32(end)))
            let head = try XCTUnwrap(input.inputScalar(atLocalScalar: UInt32(start)))
            XCTAssertNotNil(fixture.adapter.syncSelection(anchor: anchor, head: head), "\(context)/reverse: selection refused")
            XCTAssertTrue(input.applyUpdateJSON(try XCTUnwrap(fixture.adapter.cachedAtomicRenderJSON)))
            let backward = try XCTUnwrap(input.currentLogicalScalarSelection())
            XCTAssertEqual(backward.anchor, anchor, "\(context)/reverse: anchor direction lost")
            XCTAssertEqual(backward.head, head, "\(context)/reverse: head direction lost")
            XCTAssertEqual(input.selectedRange, NSRange(location: start, length: end - start))
            input.insertText(recovery)
            try checkBlocks(fixture, context: context, blocks: [paragraph(left + recovery + right)],
                offset: start + recovery.utf16.count, neighbour: neighbour, label: context + "/reverse/replace")
            input.insertText(typed)
            var remaining = typed
            repeat {
                try checkBlocks(fixture, context: context, blocks: [paragraph(left + recovery + remaining + right)],
                    offset: start + recovery.utf16.count + remaining.utf16.count, neighbour: neighbour,
                    label: context + "/reverse/continue/remaining=\(remaining.count)")
                if remaining.isEmpty { break }
                remaining.removeLast()
                input.deleteBackward()
            } while true
        }
    }

    private func runReturnJoin(context: String) throws {
        let corpus = try corpus()
        let prefix = try XCTUnwrap(corpus["prefix"] as? String)
        let original = prefix + (try XCTUnwrap(corpus["suffix"] as? String))
        let typed = try XCTUnwrap(corpus["probe"] as? String)
        for offset in [0, prefix.utf16.count, original.utf16.count] {
            let label = "\(context)/return-join/offset=\(offset)"
            try withSequenceEditor(corpus, context: context, inline: true) { fixture, neighbour in
                let input = fixture.view.activeTextInput
                let left = (original as NSString).substring(to: offset)
                let right = (original as NSString).substring(from: offset)
                input.selectedRange = NSRange(location: offset, length: 0)
                input.insertText("\n")
                try checkBlocks(fixture, context: context, blocks: [paragraph(left), paragraph(right)],
                    paragraphIndex: Self.nodeContentOffset, offset: 0, neighbour: neighbour, label: label + "/return")
                input.deleteBackward()
                try checkBlocks(fixture, context: context, blocks: [paragraph(original)], offset: offset,
                    neighbour: neighbour, label: label + "/immediate-backspace")
                input.insertText(typed)
                var remaining = typed
                repeat {
                    try checkBlocks(fixture, context: context, blocks: [paragraph(left + remaining + right)],
                        offset: offset + remaining.utf16.count, neighbour: neighbour,
                        label: label + "/continue/remaining=\(remaining.count)")
                    if remaining.isEmpty { break }
                    remaining.removeLast()
                    input.deleteBackward()
                } while true
            }
        }
    }

    private func runForwardBoundaryDeletion(context: String) throws {
        let corpus = try corpus()
        let left = try XCTUnwrap(corpus["prefix"] as? String)
        let right = try XCTUnwrap(corpus["suffix"] as? String)
        let typed = try XCTUnwrap(corpus["probe"] as? String)
        let blocks = [paragraph([textNode(left, marks: [Self.boldMark])]), paragraph(right)]
        try withSequenceEditor(corpus, context: context, inline: false, blocks: blocks) { fixture, neighbour in
            let input = fixture.view.activeTextInput
            input.selectedRange = NSRange(location: left.utf16.count, length: 0)
            let start = try XCTUnwrap(input.position(from: input.beginningOfDocument, offset: left.utf16.count))
            let end = try XCTUnwrap(input.position(from: start, offset: Self.nodeContentOffset))
            let range = try XCTUnwrap(input.textRange(from: start, to: end))
            input.replace(range, withText: "")
            try checkBlocks(fixture, context: context,
                blocks: [paragraph([textNode(left, marks: [Self.boldMark]), textNode(right)])],
                offset: left.utf16.count, neighbour: neighbour, label: context + "/forward-boundary/delete")
            input.insertText(typed)
            var remaining = typed
            repeat {
                try checkBlocks(fixture, context: context,
                    blocks: [paragraph([textNode(left + remaining, marks: [Self.boldMark]), textNode(right)])],
                    offset: left.utf16.count + remaining.utf16.count, neighbour: neighbour,
                    label: context + "/forward-boundary/continue/remaining=\(remaining.count)")
                if remaining.isEmpty { break }
                remaining.removeLast()
                input.deleteBackward()
            } while true
        }
    }

    private func paragraph(runs: [(String, [String])]) -> [String: Any] {
        var combined: [(String, [String])] = []
        for (text, marks) in runs where !text.isEmpty {
            if combined.last?.1 == marks { combined[combined.count - Self.nodeContentOffset].0 += text }
            else { combined.append((text, marks)) }
        }
        return paragraph(combined.map { textNode($0.0, marks: $0.1) })
    }

    private func runStoredMarks(context: String) throws {
        let corpus = try corpus()
        let prefix = try XCTUnwrap(corpus["prefix"] as? String)
        let original = prefix + (try XCTUnwrap(corpus["suffix"] as? String))
        let typed = try XCTUnwrap(corpus["probe"] as? String)
        let recovery = try XCTUnwrap(corpus["recovery"] as? String)
        let activeMarks = [[Self.boldMark], [Self.boldMark, Self.italicMark], [Self.italicMark], []]
        let toggles = [Self.boldMark, Self.italicMark, Self.boldMark, Self.italicMark]
        for offset in [0, prefix.utf16.count, original.utf16.count] {
            let label = "\(context)/stored-marks/offset=\(offset)"
            try withSequenceEditor(corpus, context: context, inline: true) { fixture, neighbour in
                let input = fixture.view.activeTextInput
                input.selectedRange = NSRange(location: offset, length: 0)
                let left = (original as NSString).substring(to: offset)
                let right = (original as NSString).substring(from: offset)
                var runs: [(String, [String])] = []
                for (phase, toggle) in toggles.enumerated() {
                    input.performToolbarToggleMark(toggle)
                    let active = try XCTUnwrap(fixture.adapter.cachedActiveState?["marks"] as? [String: Any])
                    for mark in [Self.boldMark, Self.italicMark] {
                        XCTAssertEqual((active[mark] as? NSNumber)?.boolValue == true, activeMarks[phase].contains(mark),
                            "\(label)/toggle/\(phase): engine stored \(mark) state is wrong")
                    }
                    try checkBlocks(fixture, context: context, blocks: [paragraph(runs: [(left, [])] + runs + [(right, [])])],
                        offset: offset + runs.reduce(0) { $0 + $1.0.utf16.count }, neighbour: neighbour,
                        label: label + "/toggle/\(phase)")
                    input.insertText(typed)
                    runs.append((typed, activeMarks[phase]))
                    try checkBlocks(fixture, context: context, blocks: [paragraph(runs: [(left, [])] + runs + [(right, [])])],
                        offset: offset + runs.reduce(0) { $0 + $1.0.utf16.count }, neighbour: neighbour,
                        label: label + "/type/\(phase)")
                }
                let complete = try json(document(fixture))
                let completeSelection = try XCTUnwrap(fixture.adapter.cachedAtomicRenderSelection())
                input.performToolbarUndo()
                try checkBlocks(fixture, context: context, blocks: [paragraph(runs: [(left, [])] + runs.dropLast() + [(right, [])])],
                    offset: offset + runs.dropLast().reduce(0) { $0 + $1.0.utf16.count }, neighbour: neighbour, label: label + "/undo")
                input.performToolbarRedo()
                XCTAssertEqual(try json(document(fixture)), complete, label + "/redo exact document")
                let redoneSelection = try XCTUnwrap(fixture.adapter.cachedAtomicRenderSelection())
                for endpoint in ["anchor", "head"] {
                    XCTAssertEqual(EditorV2Adapter.uint32Field(redoneSelection, endpoint),
                        EditorV2Adapter.uint32Field(completeSelection, endpoint), label + "/redo \(endpoint)")
                }
                try checkBlocks(fixture, context: context, blocks: [paragraph(runs: [(left, [])] + runs + [(right, [])])],
                    offset: offset + runs.reduce(0) { $0 + $1.0.utf16.count }, neighbour: neighbour, label: label + "/redo")
                input.insertText(recovery)
                try checkBlocks(fixture, context: context,
                    blocks: [paragraph(runs: [(left, [])] + runs + [(recovery + right, [])])],
                    offset: offset + runs.reduce(0) { $0 + $1.0.utf16.count } + recovery.utf16.count,
                    neighbour: neighbour, label: label + "/continue")
            }
        }
    }

    private func runRichHistory(context: String) throws {
        let corpus = try corpus()
        let prefix = try XCTUnwrap(corpus["prefix"] as? String)
        let suffix = try XCTUnwrap(corpus["suffix"] as? String)
        let recovery = try XCTUnwrap(corpus["recovery"] as? String)
        let typed = try XCTUnwrap(corpus["probe"] as? String)
        for fragment in try XCTUnwrap(corpus["fragments"] as? [[String: Any]]) {
            let name = try XCTUnwrap(fragment["name"] as? String)
            let label = context + "/rich-history/" + name
            try withSequenceEditor(corpus, context: context, inline: fragment["inline"] as? Bool == true) { fixture, neighbour in
                let input = fixture.view.activeTextInput
                input.selectedRange = NSRange(location: prefix.utf16.count, length: 0)
                let selection = try XCTUnwrap(input.currentLogicalScalarSelection())
                XCTAssertTrue(input.applyUpdateJSON(try XCTUnwrap(fixture.adapter.insertContentJson(
                    json(["type": "doc", "content": [try XCTUnwrap(fragment["node"])]]),
                    anchor: selection.anchor, head: selection.head))))
                let richText = prefix + (try XCTUnwrap(fragment["text"] as? String)) + suffix
                try check(fixture, context: context, expected: richText, neighbour: neighbour, label: label + "/insert")
                input.selectedRange = NSRange(location: 0, length: input.textStorage.length)
                let richDocument = try json(document(fixture))
                let selectedRange = input.selectedRange
                input.deleteBackward()
                try check(fixture, context: context, expected: "", neighbour: neighbour, label: label + "/clear")
                let clearedDocument = try json(document(fixture))
                let clearedCaret = input.selectedRange
                input.performToolbarUndo()
                XCTAssertEqual(try json(document(fixture)), richDocument, label + "/undo exact document")
                XCTAssertEqual(input.selectedRange, selectedRange, label + "/undo selected range")
                try check(fixture, context: context, expected: richText, neighbour: neighbour, label: label + "/undo")
                input.performToolbarRedo()
                XCTAssertEqual(try json(document(fixture)), clearedDocument, label + "/redo exact document")
                XCTAssertEqual(input.selectedRange, clearedCaret, label + "/redo caret")
                try check(fixture, context: context, expected: "", neighbour: neighbour, label: label + "/redo")
                try probe(fixture, corpus: corpus, context: context, expected: typed, neighbour: neighbour, label: label + "/probe")
                input.insertText(recovery)
                try check(fixture, context: context, expected: recovery, neighbour: neighbour, label: label + "/continue")
            }
        }
    }

    private func runUnicode(context: String) throws {
        let corpus = try corpus()
        let prefix = try XCTUnwrap(corpus["prefix"] as? String)
        let suffix = try XCTUnwrap(corpus["suffix"] as? String)
        let original = prefix + suffix
        for offset in [0, prefix.utf16.count, original.utf16.count] {
            for sample in try XCTUnwrap(corpus["unicode"] as? [[String: Any]]) {
                let label = "\(context)/unicode \(try XCTUnwrap(sample["name"]))/offset \(offset)"
                try XCTContext.runActivity(named: label) { _ in
                    try withSequenceEditor(corpus, context: context, inline: true) { fixture, neighbour in
                        let input = fixture.view.activeTextInput
                        let left = (original as NSString).substring(to: offset)
                        let right = (original as NSString).substring(from: offset)
                        input.selectedRange = NSRange(location: offset, length: 0)
                        let text = try XCTUnwrap(sample["text"] as? String)
                        input.insertText(text)
                        try check(fixture, context: context, expected: left + text + right, neighbour: neighbour, label: label + "/insert")
                        XCTAssertEqual(input.selectedRange, NSRange(location: offset + text.utf16.count, length: 0), label)
                        for (press, remaining) in try XCTUnwrap(sample["backspaces"] as? [String]).enumerated() {
                            input.deleteBackward()
                            let stage = label + "/backspace \(press)"
                            try check(fixture, context: context, expected: left + remaining + right, neighbour: neighbour, label: stage)
                            XCTAssertEqual(input.textStorage.string, left + remaining + right, "\(stage): Unicode rendering stale")
                            XCTAssertEqual(input.selectedRange, NSRange(location: offset + remaining.utf16.count, length: 0), "\(stage): wrong grapheme caret")
                        }
                        let recovery = try XCTUnwrap(corpus["recovery"] as? String)
                        input.insertText(recovery)
                        try check(fixture, context: context, expected: left + recovery + right, neighbour: neighbour, label: label + "/recover")
                    }
                }
            }
        }
    }

    private func runPairs(context: String, mode: String) throws {
        let corpus = try corpus()
        let fragments = try XCTUnwrap(corpus["fragments"] as? [[String: Any]])
        let prefix = try XCTUnwrap(corpus["prefix"] as? String)
        let suffix = try XCTUnwrap(corpus["suffix"] as? String)
        let probeText = try XCTUnwrap(corpus["probe"] as? String)
        for first in fragments {
            for second in fragments {
                let label = "\(context)/\(mode)/\(try XCTUnwrap(first["name"])) -> \(try XCTUnwrap(second["name"]))"
                try XCTContext.runActivity(named: label) { _ in
                    try withSequenceEditor(corpus, context: context, inline: first["inline"] as? Bool == true) { fixture, neighbour in
                        let input = fixture.view.activeTextInput
                        input.selectedRange = NSRange(location: prefix.utf16.count, length: 0)
                        for (phase, fragment) in [first, second].enumerated() {
                            let stage = label + "/phase \(phase)"
                            let selection = try XCTUnwrap(input.currentLogicalScalarSelection())
                            let update = try XCTUnwrap(fixture.adapter.insertContentJson(
                                                        json(["type": "doc", "content": [try XCTUnwrap(fragment["node"])] ]),
                                                        anchor: selection.anchor, head: selection.head), stage)
                            XCTAssertTrue(input.applyUpdateJSON(update), stage)
                            let middle = try XCTUnwrap(fragment["text"] as? String)
                            let left = phase == 0 ? prefix : ""
                            let right = phase == 0 ? suffix : ""
                            try check(fixture, context: context, expected: left + middle + right, neighbour: neighbour, label: stage)
                            XCTAssertTrue(descendants(try scope(document(fixture), context: context)).contains {
                                $0["type"] as? String == fragment["name"] as? String
                            }, "\(stage): inserted node missing")
                            try probe(fixture, corpus: corpus, context: context, expected: left + middle + probeText + right,
                                      neighbour: neighbour, label: stage)
                            try clear(fixture, context: context, mode: mode, name: XCTUnwrap(fragment["name"] as? String),
                                      preservesFormatting: fragment["preservesFormatting"] as? Bool == true, label: stage)
                            try check(fixture, context: context, expected: "", neighbour: neighbour, label: stage + "/clear")
                        }
                        let recovery = try XCTUnwrap(corpus["recovery"] as? String)
                        input.insertText(recovery)
                        try check(fixture, context: context, expected: recovery, neighbour: neighbour, label: label + "/recover")
                    }
                }
            }
        }
    }
}
