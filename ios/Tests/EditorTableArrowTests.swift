import XCTest

private final class TableArrowSelectionDelegate: NSObject, EditorTextViewDelegate {
    var selections: [(UInt32, UInt32)] = []
    var onSelection: (() -> Void)?

    func editorTextView(_ textView: EditorTextView, selectionDidChange anchor: UInt32, head: UInt32) {
        selections.append((anchor, head))
        let callback = onSelection
        onSelection = nil
        callback?()
    }

    func editorTextView(_ textView: EditorTextView, didReceiveUpdate updateJSON: String) {}
}

extension EditorTableNavigationTests {
    func testHardwareRightArrowAtCellEndSelectsVisualNeighborWithoutChangingDocument() throws {
        let fixture = try makeFixture(
            config: tableConfig,
            document: TableInputTestSchema.twoCellDocument,
            size: Self.editorSize,
            windowed: true
        )
        defer { fixture.close() }
        let adapter = fixture.adapter
        let view = fixture.view
        let tableID = try XCTUnwrap(adapter.tableMappingsForTesting?.tables.keys.first)
        XCTAssertTrue(view.bindTableCell(tableID: tableID, cellIndex: 0, contentRect: .zero))
        let input = view.activeTextInput
        select(NSRange(location: ("one" as NSString).length, length: 0), in: input)
        XCTAssertTrue(input.becomeFirstResponder())
        let before = try XCTUnwrap(adapter.documentJson())
        try pressArrow(.right, in: input)
        XCTAssertEqual(try XCTUnwrap(adapter.documentJson()), before)
        XCTAssertTrue(view.activeTextInput === input)
        XCTAssertTrue(input.isFirstResponder)
        XCTAssertEqual(
            input.tableCellPositionMap?.binding.documentPosition(in: adapter),
            adapter.tableMappingsForTesting?.tables[tableID]?.cells[1].sourcePos
        )
        view.activeTextInput.insertText("!")
        let edited = try XCTUnwrap(adapter.documentJson())
        XCTAssertTrue(edited.contains(#""text":"!two""#), edited)
    }

    func testHardwareLeftArrowAtCellStartSelectsPreviousCell() throws {
        let fixture = try makeFixture(
            config: tableConfig,
            document: TableInputTestSchema.twoCellDocument,
            size: Self.editorSize,
            windowed: true
        )
        defer { fixture.close() }
        let adapter = fixture.adapter
        let view = fixture.view
        let tableID = try XCTUnwrap(adapter.tableMappingsForTesting?.tables.keys.first)
        XCTAssertTrue(view.bindTableCell(tableID: tableID, cellIndex: 1, contentRect: .zero))
        let input = view.activeTextInput
        select(NSRange(location: 0, length: 0), in: input)
        XCTAssertTrue(input.becomeFirstResponder())
        let before = try XCTUnwrap(adapter.documentJson())
        try pressArrow(.left, in: input)
        XCTAssertEqual(try XCTUnwrap(adapter.documentJson()), before)
        XCTAssertEqual(
            input.tableCellPositionMap?.binding.documentPosition(in: adapter),
            adapter.tableMappingsForTesting?.tables[tableID]?.cells[0].sourcePos
        )
        XCTAssertTrue(input.isFirstResponder)
        input.insertText("!")
        let edited = try XCTUnwrap(adapter.documentJson())
        XCTAssertTrue(edited.contains(#""text":"one!""#), edited)
    }

    func testHardwareVerticalArrowsSelectSameColumnAndDoNotMutateDocument() throws {
        let fixture = try makeFixture(
            config: tableConfig,
            document: Self.twoByTwoDocument,
            size: Self.editorSize,
            windowed: true
        )
        defer { fixture.close() }
        let adapter = fixture.adapter
        let view = fixture.view
        let tableID = try XCTUnwrap(adapter.tableMappingsForTesting?.tables.keys.first)
        XCTAssertTrue(view.bindTableCell(tableID: tableID, cellIndex: 1, contentRect: .zero))
        let input = view.activeTextInput
        select(NSRange(location: 0, length: 0), in: input)
        let first = try XCTUnwrap(input.position(from: input.beginningOfDocument, offset: 0))
        let last = try XCTUnwrap(input.position(from: input.beginningOfDocument, offset: ("top-right" as NSString).length))
        XCTAssertEqual(input.caretRect(for: first).midY, input.caretRect(for: last).midY)
        let nativeDown = try XCTUnwrap(input.position(from: first, in: .down, offset: 1))
        XCTAssertNotEqual(input.offset(from: input.beginningOfDocument, to: nativeDown), 0)
        XCTAssertEqual(input.caretRect(for: first).midY, input.caretRect(for: nativeDown).midY)
        XCTAssertTrue(input.becomeFirstResponder())
        let before = try XCTUnwrap(adapter.documentJson())
        try pressArrow(.down, in: input)
        XCTAssertEqual(
            input.tableCellPositionMap?.binding.documentPosition(in: adapter),
            adapter.tableMappingsForTesting?.tables[tableID]?.cells[3].sourcePos
        )
        XCTAssertEqual(try XCTUnwrap(adapter.documentJson()), before)
        select(NSRange(location: 0, length: 0), in: input)
        try pressArrow(.up, in: input)
        XCTAssertEqual(
            input.tableCellPositionMap?.binding.documentPosition(in: adapter),
            adapter.tableMappingsForTesting?.tables[tableID]?.cells[1].sourcePos
        )
        XCTAssertEqual(try XCTUnwrap(adapter.documentJson()), before)
        XCTAssertTrue(input.isFirstResponder)
    }

    func testPlainArrowCommandsLeaveOrdinaryTextSelectionToUIKit() throws {
        let fixture = try makeFixture(
            config: tableConfig,
            document: TableInputTestSchema.twoCellDocument,
            size: Self.editorSize,
            windowed: true
        )
        defer { fixture.close() }
        let adapter = fixture.adapter
        let view = fixture.view
        let tableID = try XCTUnwrap(adapter.tableMappingsForTesting?.tables.keys.first)
        XCTAssertTrue(view.bindTableCell(tableID: tableID, cellIndex: 0, contentRect: .zero))
        let input = view.activeTextInput
        select(NSRange(location: 1, length: 0), in: input)
        XCTAssertNil(input.keyCommands?.first { $0.input == UIKeyCommand.inputRightArrow })
        XCTAssertNil(input.keyCommands?.first { $0.input == UIKeyCommand.inputLeftArrow })
        select(NSRange(location: 0, length: 3), in: input)
        XCTAssertNil(input.keyCommands?.first { $0.input == UIKeyCommand.inputUpArrow })
        XCTAssertNil(input.keyCommands?.first { $0.input == UIKeyCommand.inputDownArrow })
        XCTAssertEqual(input.keyCommands?.filter {
            $0.modifierFlags.contains(.shift)
                && [
                    UIKeyCommand.inputLeftArrow,
                    UIKeyCommand.inputRightArrow,
                    UIKeyCommand.inputUpArrow,
                    UIKeyCommand.inputDownArrow
                ].contains($0.input) }.count, 0)
    }

    func testHardwareArrowPublishesNativeSelectionEvent() throws {
        let fixture = try makeFixture(
            config: tableConfig,
            document: TableInputTestSchema.twoCellDocument,
            size: Self.editorSize,
            windowed: true
        )
        defer { fixture.close() }
        let adapter = fixture.adapter
        let view = fixture.view
        let tableID = try XCTUnwrap(adapter.tableMappingsForTesting?.tables.keys.first)
        XCTAssertTrue(view.bindTableCell(tableID: tableID, cellIndex: 0, contentRect: .zero))
        let input = view.activeTextInput
        select(NSRange(location: 3, length: 0), in: input)
        XCTAssertTrue(input.becomeFirstResponder())
        let delegate = TableArrowSelectionDelegate()
        input.editorDelegate = delegate
        try pressArrow(.right, in: input)
        let targetScalar = try XCTUnwrap(input.currentScalarSelection()?.head)
        XCTAssertEqual(delegate.selections.count, 1)
        XCTAssertEqual(delegate.selections.first?.0, adapter.documentPosition(forScalar: targetScalar))
        XCTAssertEqual(delegate.selections.first?.1, adapter.documentPosition(forScalar: targetScalar))
    }

    func testHorizontalArrowsWrapAcrossRowsWithoutAppending() throws {
        let fixture = try makeFixture(
            config: tableConfig,
            document: Self.twoByTwoDocument,
            size: Self.editorSize,
            windowed: true
        )
        defer { fixture.close() }
        let adapter = fixture.adapter
        let view = fixture.view
        let tableID = try XCTUnwrap(adapter.tableMappingsForTesting?.tables.keys.first)
        XCTAssertTrue(view.bindTableCell(tableID: tableID, cellIndex: 1, contentRect: .zero))
        let input = view.activeTextInput
        select(NSRange(location: ("top-right" as NSString).length, length: 0), in: input)
        let before = try XCTUnwrap(adapter.documentJson())
        try pressArrow(.right, in: input)
        XCTAssertEqual(
            input.tableCellPositionMap?.binding.documentPosition(in: adapter),
            adapter.tableMappingsForTesting?.tables[tableID]?.cells[2].sourcePos
        )
        XCTAssertEqual(try XCTUnwrap(adapter.documentJson()), before)
        select(NSRange(location: 0, length: 0), in: input)
        try pressArrow(.left, in: input)
        XCTAssertEqual(
            input.tableCellPositionMap?.binding.documentPosition(in: adapter),
            adapter.tableMappingsForTesting?.tables[tableID]?.cells[1].sourcePos
        )
        XCTAssertEqual(try XCTUnwrap(adapter.documentJson()), before)
    }

    func testRightArrowSkipsSyntheticSlotAtIrregularRowEnd() throws {
        try assertIrregularRowArrowWrap(rightToLeft: false)
    }

    func testRtlLeftArrowSkipsSyntheticSlotAtIrregularRowEnd() throws {
        try assertIrregularRowArrowWrap(rightToLeft: true)
    }

    func testRightArrowLeavesIrregularLastRealCellForProse() throws {
        let document = #"{"type":"doc","content":[{"type":"table","content":[{"type":"table_row","content":[{"type":"table_cell","content":[{"type":"paragraph","content":[{"type":"text","text":"A"}]}]},{"type":"table_cell","content":[{"type":"paragraph","content":[{"type":"text","text":"B"}]}]}]},{"type":"table_row","content":[{"type":"table_cell","content":[{"type":"paragraph","content":[{"type":"text","text":"C"}]}]}]}]},{"type":"paragraph","content":[{"type":"text","text":"after"}]}]}"#
        let fixture = try makeFixture(
            config: tableConfig,
            document: document,
            size: Self.editorSize,
            windowed: true
        )
        defer { fixture.close() }
        let adapter = fixture.adapter
        let view = fixture.view
        let tableID = try XCTUnwrap(adapter.tableMappingsForTesting?.tables.keys.first)
        let tableSurface = try XCTUnwrap(view.subviews.compactMap { $0 as? EditorTableSurface }.first)
        let drawing = try XCTUnwrap(tableSurface.subviews.compactMap { $0 as? PreparedProseDrawingView }.first)
        let prepared = try XCTUnwrap(drawing.layout?.blocks.first?.tableSurface)
        let finalCell = try XCTUnwrap(prepared.cells.first { $0.sourceIndex == 2 })
        XCTAssertLessThan(
            prepared.frame(ofCell: finalCell).maxX,
            prepared.bounds.maxX,
            "The final authored cell must stop before the synthetic trailing slot"
        )
        XCTAssertTrue(view.bindTableCell(tableID: tableID, cellIndex: 2, contentRect: .zero))
        let input = view.activeTextInput
        select(NSRange(location: 1, length: 0), in: input)
        let before = try XCTUnwrap(adapter.documentJson())
        try pressArrow(.right, in: input)
        XCTAssertTrue(view.activeTextInput === view.textView)
        XCTAssertEqual(try XCTUnwrap(adapter.documentJson()), before)
        view.activeTextInput.insertText("!")
        XCTAssertTrue(try XCTUnwrap(adapter.documentJson()).contains(#""text":"!after""#))
    }

    func testRtlArrowsFollowMirroredCellsAndWrapBackwardAcrossRows() throws {
        let config = tableConfig.replacingOccurrences(
            of: #""tableRole":"table","attrs":{"class":{"default":null}}"#,
            with: #""tableRole":"table","attrs":{"class":{"default":null},"dir":{"default":null}}"#
        )
        let document = Self.twoByTwoDocument.replacingOccurrences(
            of: #""type":"table","content""#,
            with: #""type":"table","attrs":{"dir":"rtl"},"content""#
        )
        let fixture = try makeFixture(
            config: config,
            document: document,
            size: Self.editorSize,
            windowed: true
        )
        defer { fixture.close() }
        let adapter = fixture.adapter
        let view = fixture.view
        let tableID = try XCTUnwrap(adapter.tableMappingsForTesting?.tables.keys.first)
        XCTAssertTrue(view.bindTableCell(tableID: tableID, cellIndex: 2, contentRect: .zero))
        let input = view.activeTextInput
        select(NSRange(location: ("bottom-left" as NSString).length, length: 0), in: input)
        let before = try XCTUnwrap(adapter.documentJson())
        try pressArrow(.right, in: input)
        XCTAssertEqual(
            input.tableCellPositionMap?.binding.documentPosition(in: adapter),
            adapter.tableMappingsForTesting?.tables[tableID]?.cells[1].sourcePos
        )
        XCTAssertEqual(try XCTUnwrap(adapter.documentJson()), before)
        select(NSRange(location: 0, length: 0), in: input)
        try pressArrow(.left, in: input)
        XCTAssertEqual(
            input.tableCellPositionMap?.binding.documentPosition(in: adapter),
            adapter.tableMappingsForTesting?.tables[tableID]?.cells[2].sourcePos
        )
    }

    func testDownArrowUsesMergedRowspanGeometry() throws {
        let document = #"{"type":"doc","content":[{"type":"table","content":[{"type":"table_row","content":[{"type":"table_cell","attrs":{"rowspan":2},"content":[{"type":"paragraph","content":[{"type":"text","text":"tall"}]}]},{"type":"table_cell","content":[{"type":"paragraph","content":[{"type":"text","text":"upper"}]}]}]},{"type":"table_row","content":[{"type":"table_cell","content":[{"type":"paragraph","content":[{"type":"text","text":"lower"}]}]}]}]}]}"#
        let fixture = try makeFixture(
            config: tableConfig,
            document: document,
            size: Self.editorSize,
            windowed: true
        )
        defer { fixture.close() }
        let adapter = fixture.adapter
        let view = fixture.view
        let tableID = try XCTUnwrap(adapter.tableMappingsForTesting?.tables.keys.first)
        XCTAssertTrue(view.bindTableCell(tableID: tableID, cellIndex: 1, contentRect: .zero))
        let input = view.activeTextInput
        select(NSRange(location: 1, length: 0), in: input)
        let before = try XCTUnwrap(adapter.documentJson())
        try pressArrow(.down, in: input)
        XCTAssertEqual(
            input.tableCellPositionMap?.binding.documentPosition(in: adapter),
            adapter.tableMappingsForTesting?.tables[tableID]?.cells[2].sourcePos
        )
        XCTAssertEqual(try XCTUnwrap(adapter.documentJson()), before)
    }

    func testArrowEntersNestedContainingOuterProseWithoutEditingDescendant() throws {
        let document = try nestedDocument(
            leadingText: "outer",
            nestedTableCount: 1,
            trailingText: "after",
            precedingCellText: "one"
        )
        let fixture = try makeFixture(
            config: tableConfig,
            document: document,
            size: Self.editorSize,
            windowed: true
        )
        defer { fixture.close() }
        let adapter = fixture.adapter
        let view = fixture.view
        let tableID = try XCTUnwrap(adapter.tableMappingsForTesting?.tables.keys.first { key in
            adapter.tableMappingsForTesting?.tables[key]?.cells.count == 2
        })
        XCTAssertTrue(view.bindTableCell(tableID: tableID, cellIndex: 0, contentRect: .zero))
        let input = view.activeTextInput
        select(NSRange(location: 3, length: 0), in: input)
        let before = try XCTUnwrap(adapter.documentJson())
        let nestedBefore = try nestedTableContent(in: before, outerCellIndex: 1)
        try pressArrow(.right, in: input)
        XCTAssertEqual(
            input.tableCellPositionMap?.binding.documentPosition(in: adapter),
            adapter.tableMappingsForTesting?.tables[tableID]?.cells[1].sourcePos
        )
        XCTAssertEqual(try XCTUnwrap(adapter.documentJson()), before)
        input.insertText("!")
        let edited = try XCTUnwrap(adapter.documentJson())
        XCTAssertTrue(edited.contains(#""text":"!outer""#), edited)
        XCTAssertEqual(try nestedTableContent(in: edited, outerCellIndex: 1), nestedBefore)
    }

    func testArrowAtTableEdgeEntersSurroundingProseAndBareEdgeDoesNothing() throws {
        let fixture = try makeFixture(
            config: tableConfig,
            document: Self.surroundedTableDocument,
            size: Self.editorSize,
            windowed: true
        )
        defer { fixture.close() }
        let adapter = fixture.adapter
        let view = fixture.view
        let tableID = try XCTUnwrap(adapter.tableMappingsForTesting?.tables.keys.first)
        XCTAssertTrue(view.bindTableCell(tableID: tableID, cellIndex: 0, contentRect: .zero))
        let input = view.activeTextInput
        select(NSRange(location: 0, length: 0), in: input)
        let before = try XCTUnwrap(adapter.documentJson())
        try pressArrow(.left, in: input)
        XCTAssertTrue(view.activeTextInput === view.textView)
        XCTAssertEqual(try XCTUnwrap(adapter.documentJson()), before)
        view.activeTextInput.insertText("!")
        let edited = try XCTUnwrap(adapter.documentJson())
        XCTAssertTrue(edited.contains(#""text":"before!""#), edited)

        let bare = try makeFixture(
            config: tableConfig,
            document: TableInputTestSchema.twoCellDocument,
            size: Self.editorSize,
            windowed: true
        )
        defer { bare.close() }
        let bareID = try XCTUnwrap(bare.adapter.tableMappingsForTesting?.tables.keys.first)
        XCTAssertTrue(bare.view.bindTableCell(tableID: bareID, cellIndex: 0, contentRect: .zero))
        let bareInput = bare.view.activeTextInput
        select(NSRange(location: 0, length: 0), in: bareInput)
        let bareBefore = try XCTUnwrap(bare.adapter.documentJson())
        try pressArrow(.left, in: bareInput)
        XCTAssertTrue(bare.view.activeTextInput === bareInput)
        XCTAssertEqual(try XCTUnwrap(bare.adapter.documentJson()), bareBefore)
    }

    func testVerticalArrowsExitToSurroundingProse() throws {
        let fixture = try makeFixture(
            config: tableConfig,
            document: Self.surroundedTableDocument,
            size: Self.editorSize,
            windowed: true
        )
        defer { fixture.close() }
        let adapter = fixture.adapter
        let view = fixture.view
        let tableID = try XCTUnwrap(adapter.tableMappingsForTesting?.tables.keys.first)
        XCTAssertTrue(view.bindTableCell(tableID: tableID, cellIndex: 0, contentRect: .zero))
        let input = view.activeTextInput
        select(NSRange(location: 0, length: 0), in: input)
        let before = try XCTUnwrap(adapter.documentJson())
        try pressArrow(.up, in: input)
        XCTAssertTrue(view.activeTextInput === view.textView)
        XCTAssertEqual(try XCTUnwrap(adapter.documentJson()), before)
        XCTAssertTrue(view.bindTableCell(tableID: tableID, cellIndex: 1, contentRect: .zero))
        select(NSRange(location: 0, length: 0), in: input)
        try pressArrow(.down, in: input)
        XCTAssertTrue(view.activeTextInput === view.textView)
        XCTAssertEqual(try XCTUnwrap(adapter.documentJson()), before)
        view.activeTextInput.insertText("!")
        let edited = try XCTUnwrap(adapter.documentJson())
        XCTAssertTrue(edited.contains(#""text":"!after""#), edited)
    }

    func testArrowDoesNotExitIntoGapBetweenAdjacentTables() throws {
        let original = try XCTUnwrap(JSONSerialization.jsonObject(
            with: Data(TableInputTestSchema.twoCellDocument.utf8)
        ) as? [String: Any])
        let table = try XCTUnwrap((original["content"] as? [[String: Any]])?.first)
        let data = try JSONSerialization.data(withJSONObject: ["type": "doc", "content": [table, table]])
        let document = try XCTUnwrap(String(data: data, encoding: .utf8))
        let fixture = try makeFixture(
            config: tableConfig,
            document: document,
            size: Self.editorSize,
            windowed: true
        )
        defer { fixture.close() }
        let adapter = fixture.adapter
        let view = fixture.view
        let tableID = try XCTUnwrap(adapter.tableMappingsForTesting?.tables.min(by: {
            ($0.value.extent?.scalarStart ?? UInt32.max) < ($1.value.extent?.scalarStart ?? UInt32.max)
        })?.key)
        XCTAssertTrue(view.bindTableCell(tableID: tableID, cellIndex: 1, contentRect: .zero))
        let input = view.activeTextInput
        select(NSRange(location: 3, length: 0), in: input)
        let before = try XCTUnwrap(adapter.documentJson())
        try pressArrow(.right, in: input)
        XCTAssertTrue(view.activeTextInput === input)
        XCTAssertEqual(try XCTUnwrap(adapter.documentJson()), before)
    }

    func testCapturedArrowRefusesStaleEpochAndOldOwner() throws {
        let fixture = try makeFixture(
            config: tableConfig,
            document: TableInputTestSchema.twoCellDocument,
            size: Self.editorSize,
            windowed: true
        )
        defer { fixture.close() }
        let adapter = fixture.adapter
        let view = fixture.view
        let tableID = try XCTUnwrap(adapter.tableMappingsForTesting?.tables.keys.first)
        XCTAssertTrue(view.bindTableCell(tableID: tableID, cellIndex: 0, contentRect: .zero))
        let input = view.activeTextInput
        select(NSRange(location: 3, length: 0), in: input)
        let command = try XCTUnwrap(input.keyCommands?.first { $0.input == UIKeyCommand.inputRightArrow })
        let oldEpoch = try XCTUnwrap(input.tableCellPositionMap?.binding.positionEpoch)
        _ = adapter.selectionJSON()
        XCTAssertNotEqual(adapter.positionEpoch, oldEpoch)
        let before = try XCTUnwrap(adapter.documentJson())
        _ = input.perform(command.action)
        XCTAssertEqual(try XCTUnwrap(adapter.documentJson()), before)
        XCTAssertEqual(
            input.tableCellPositionMap?.binding.documentPosition(in: adapter),
            adapter.tableMappingsForTesting?.tables[tableID]?.cells[0].sourcePos
        )

        view.editorId = 0
        let second = RichTextEditorView(frame: view.frame)
        second.bindEditor(id: fixture.editorId, initialUpdateJSON: try XCTUnwrap(adapter.initialUpdateJSON()))
        XCTAssertTrue(second.textView.ownsNativeBinding(adapter))
        _ = input.perform(command.action)
        XCTAssertTrue(view.activeTextInput === view.textView)
        XCTAssertEqual(try XCTUnwrap(adapter.documentJson()), before)
    }

    func testCapturedArrowRefusesReadOnlyInputAndReplacedDocument() throws {
        let fixture = try makeFixture(
            config: tableConfig,
            document: TableInputTestSchema.twoCellDocument,
            size: Self.editorSize,
            windowed: true
        )
        defer { fixture.close() }
        let adapter = fixture.adapter
        let view = fixture.view
        let tableID = try XCTUnwrap(adapter.tableMappingsForTesting?.tables.keys.first)
        XCTAssertTrue(view.bindTableCell(tableID: tableID, cellIndex: 0, contentRect: .zero))
        let input = view.activeTextInput
        select(NSRange(location: 3, length: 0), in: input)
        let command = try XCTUnwrap(input.keyCommands?.first { $0.input == UIKeyCommand.inputRightArrow })
        input.isEditable = false
        let before = try XCTUnwrap(adapter.documentJson())
        _ = input.perform(command.action)
        XCTAssertEqual(try XCTUnwrap(adapter.documentJson()), before)
        XCTAssertEqual(
            input.tableCellPositionMap?.binding.documentPosition(in: adapter),
            adapter.tableMappingsForTesting?.tables[tableID]?.cells[0].sourcePos
        )
        input.isEditable = true
        let replacement = #"{"type":"doc","content":[{"type":"paragraph","content":[{"type":"text","text":"replacement"}]}]}"#
        XCTAssertTrue(view.textView.applyUpdateJSON(try XCTUnwrap(adapter.setContentJson(replacement))))
        let settled = try XCTUnwrap(adapter.documentJson())
        _ = input.perform(command.action)
        XCTAssertEqual(try XCTUnwrap(adapter.documentJson()), settled)
        XCTAssertTrue(view.activeTextInput === view.textView)
    }

    func testCapturedArrowDoesNotOverrideExternalCompositionOrReentrantSelection() throws {
        let fixture = try makeFixture(
            config: tableConfig,
            document: TableInputTestSchema.twoCellDocument,
            size: Self.editorSize,
            windowed: true
        )
        defer { fixture.close() }
        let adapter = fixture.adapter
        let view = fixture.view
        let tableID = try XCTUnwrap(adapter.tableMappingsForTesting?.tables.keys.first)
        XCTAssertTrue(view.bindTableCell(tableID: tableID, cellIndex: 0, contentRect: .zero))
        let input = view.activeTextInput
        select(NSRange(location: 3, length: 0), in: input)
        let command = try XCTUnwrap(input.keyCommands?.first { $0.input == UIKeyCommand.inputRightArrow })
        let began = input.beginExternalTextComposition(sessionId: "arrow-composition")
        XCTAssertTrue(began.contains(#""type":"active""#), began)
        let before = try XCTUnwrap(adapter.documentJson())
        _ = input.perform(command.action)
        XCTAssertEqual(try XCTUnwrap(adapter.documentJson()), before)
        XCTAssertTrue(view.activeTextInput === input)
        XCTAssertTrue(input.hasPendingCompositionForExternalRefresh)
        _ = input.cancelExternalTextComposition(sessionId: "arrow-composition", cause: "consumer")
        XCTAssertTrue(view.bindTableCell(tableID: tableID, cellIndex: 0, contentRect: .zero))
        select(NSRange(location: 3, length: 0), in: input)
        let next = try XCTUnwrap(adapter.tableMappingsForTesting?.tables[tableID]?.cells[1].blocks.first?.contentScalarStart)
        let delegate = TableArrowSelectionDelegate()
        input.editorDelegate = delegate
        delegate.onSelection = {
            _ = EditorV2Shadow.setSelectionScalar(
                id: fixture.editorId, scalarAnchor: next + 1, scalarHead: next + 1
            )
        }
        try pressArrow(.right, in: input)
        XCTAssertEqual(delegate.selections.count, 1)
        let selectionJSON = try XCTUnwrap(adapter.selectionJSON())
        let selection = try XCTUnwrap(JSONSerialization.jsonObject(with: Data(selectionJSON.utf8)) as? [String: Any])
        XCTAssertEqual(v2ExactUInt32(selection["headScalar"] as? NSNumber), next + 1)
    }

    func testMultilineMiddleMovementAndVisualLineEdgesRemainNative() throws {
        let document = #"{"type":"doc","content":[{"type":"table","content":[{"type":"table_row","content":[{"type":"table_cell","content":[{"type":"paragraph","content":[{"type":"text","text":"first"}]},{"type":"paragraph","content":[{"type":"text","text":"middle 🧭"}]},{"type":"paragraph","content":[{"type":"text","text":"last"}]}]},{"type":"table_cell","content":[{"type":"paragraph","content":[{"type":"text","text":"target"}]}]}]}]}]}"#
        let fixture = try makeFixture(
            config: tableConfig,
            document: document,
            size: Self.editorSize,
            windowed: true
        )
        defer { fixture.close() }
        let adapter = fixture.adapter
        let view = fixture.view
        let tableID = try XCTUnwrap(adapter.tableMappingsForTesting?.tables.keys.first)
        XCTAssertTrue(view.bindTableCell(tableID: tableID, cellIndex: 0, contentRect: .zero))
        let input = view.activeTextInput
        let text = input.textStorage.string as NSString
        let middle = text.range(of: "middle")
        let last = text.range(of: "last")
        XCTAssertNotEqual(middle.location, NSNotFound)
        XCTAssertNotEqual(last.location, NSNotFound)
        select(NSRange(location: middle.location + 2, length: 0), in: input)
        XCTAssertNil(input.keyCommands?.first { $0.input == UIKeyCommand.inputUpArrow })
        XCTAssertNil(input.keyCommands?.first { $0.input == UIKeyCommand.inputDownArrow })
        select(NSRange(location: NSMaxRange(text.range(of: "first")), length: 0), in: input)
        XCTAssertNil(input.keyCommands?.first { $0.input == UIKeyCommand.inputRightArrow })
        select(NSRange(location: last.location + 1, length: 0), in: input)
        XCTAssertNotNil(input.keyCommands?.first { $0.input == UIKeyCommand.inputDownArrow })
        select(NSRange(location: 1, length: 0), in: input)
        XCTAssertNotNil(input.keyCommands?.first { $0.input == UIKeyCommand.inputUpArrow })
    }

    func testSoftWrappedUpstreamCaretKeepsNativeDownMovement() throws {
        let text = "alpha beta gamma delta 🧭 אבג epsilon zeta eta theta iota kappa lambda"
        let document = #"{"type":"doc","content":[{"type":"table","content":[{"type":"table_row","content":[{"type":"table_cell","content":[{"type":"paragraph","content":[{"type":"text","text":"\#(text)"}]}]}]}]}]}"#
        let fixture = try makeFixture(
            config: tableConfig,
            document: document,
            size: CGSize(width: 170, height: 300),
            windowed: true
        )
        defer { fixture.close() }
        let adapter = fixture.adapter
        let view = fixture.view
        let tableID = try XCTUnwrap(adapter.tableMappingsForTesting?.tables.keys.first)
        XCTAssertTrue(view.bindTableCell(tableID: tableID, cellIndex: 0, contentRect: .zero))
        let input = view.activeTextInput
        input.layoutManager.ensureLayout(for: input.textContainer)
        var lines: [(used: CGRect, glyphs: NSRange)] = []
        input.layoutManager.enumerateLineFragments(
            forGlyphRange: NSRange(location: 0, length: input.layoutManager.numberOfGlyphs)
        ) { _, used, _, glyphs, _ in
            lines.append((used, glyphs))
        }
        XCTAssertGreaterThan(lines.count, 2, "Fixture must actually soft-wrap one paragraph")
        let first = lines[lines.count - 2]
        let next = try XCTUnwrap(lines.last)
        XCTAssertTrue(input.becomeFirstResponder())
        let upstream = try XCTUnwrap(input.closestPosition(
            to: CGPoint(
                x: input.bounds.maxX - input.textContainerInset.right,
                y: first.used.midY + input.textContainerInset.top
            )
        ))
        let nativeDown = try XCTUnwrap(input.position(from: upstream, in: .down, offset: 1))
        let upstreamOffset = input.offset(from: input.beginningOfDocument, to: upstream)
        let nextOffset = input.offset(from: input.beginningOfDocument, to: nativeDown)
        XCTAssertEqual(
            upstreamOffset,
            input.layoutManager.characterIndexForGlyph(at: next.glyphs.location),
            "The caret must have upstream affinity at the soft-wrap offset; first=\(first), next=\(next), bounds=\(input.bounds), inset=\(input.textContainerInset), upstreamCaret=\(input.caretRect(for: upstream))"
        )
        XCTAssertGreaterThan(
            input.caretRect(for: nativeDown).midY,
            input.caretRect(for: upstream).midY,
            "UIKit must have another visual line below this caret"
        )
        XCTAssertNotEqual(nextOffset, upstreamOffset)
        input.selectedTextRange = input.textRange(from: upstream, to: upstream)
        input.textViewDidChangeSelection(input)
        let selected = try XCTUnwrap(input.selectedTextRange?.start)
        XCTAssertEqual(input.caretRect(for: selected).midY, input.caretRect(for: upstream).midY)
        XCTAssertNil(
            input.keyCommands?.first { $0.input == UIKeyCommand.inputDownArrow },
            "Down must remain native at the upstream side of a soft wrap"
        )
    }

    func testEmptyCellAndMarkedTextDoNotLeakOrHijackInput() throws {
        let document = #"{"type":"doc","content":[{"type":"table","content":[{"type":"table_row","content":[{"type":"table_cell","content":[{"type":"paragraph"}]},{"type":"table_cell","content":[{"type":"paragraph"}]}]}]}]}"#
        let fixture = try makeFixture(
            config: tableConfig,
            document: document,
            size: Self.editorSize,
            windowed: true
        )
        defer { fixture.close() }
        let adapter = fixture.adapter
        let view = fixture.view
        let tableID = try XCTUnwrap(adapter.tableMappingsForTesting?.tables.keys.first)
        XCTAssertTrue(view.bindTableCell(tableID: tableID, cellIndex: 0, contentRect: .zero))
        let input = view.activeTextInput
        select(NSRange(location: 0, length: 0), in: input)
        let before = try XCTUnwrap(adapter.documentJson())
        let command = try XCTUnwrap(input.keyCommands?.first { $0.input == UIKeyCommand.inputRightArrow })
        _ = input.perform(command.action)
        XCTAssertEqual(
            input.tableCellPositionMap?.binding.documentPosition(in: adapter),
            adapter.tableMappingsForTesting?.tables[tableID]?.cells[1].sourcePos
        )
        XCTAssertEqual(try XCTUnwrap(adapter.documentJson()), before)
        XCTAssertTrue(view.bindTableCell(tableID: tableID, cellIndex: 0, contentRect: .zero))
        select(NSRange(location: 0, length: 0), in: input)
        input.setMarkedText("候", selectedRange: NSRange(location: 1, length: 0))
        _ = input.perform(command.action)
        XCTAssertTrue(view.activeTextInput === input)
        XCTAssertEqual(try XCTUnwrap(adapter.documentJson()), before)
        input.unmarkText()
        XCTAssertTrue(input.tableCellPositionMap != nil)
    }

    private func pressArrow(_ direction: TableCellArrowDirection, in input: EditorTextView) throws {
        let command = try XCTUnwrap(input.keyCommands?.first {
            $0.input == direction.keyInput && $0.modifierFlags.isEmpty
        }, "Expected \(direction) arrow at selected range \(input.selectedRange) in \(String(reflecting: input.textStorage.string))")
        XCTAssertTrue(command.wantsPriorityOverSystemBehavior)
        _ = input.perform(command.action)
    }

    private func assertIrregularRowArrowWrap(rightToLeft: Bool) throws {
        let config = rightToLeft ? tableConfig.replacingOccurrences(
            of: #""tableRole":"table","attrs":{"class":{"default":null}}"#,
            with: #""tableRole":"table","attrs":{"class":{"default":null},"dir":{"default":null}}"#
        ) : tableConfig
        let document = rightToLeft ? Self.irregularRowDocument.replacingOccurrences(
            of: #""type":"table","content"#,
            with: #""type":"table","attrs":{"dir":"rtl"},"content"#
        ) : Self.irregularRowDocument
        let fixture = try makeFixture(
            config: config,
            document: document,
            size: Self.editorSize,
            windowed: true
        )
        defer { fixture.close() }
        let adapter = fixture.adapter
        let view = fixture.view
        let tableID = try XCTUnwrap(adapter.tableMappingsForTesting?.tables.keys.first)
        let record = try XCTUnwrap(adapter.tableRecordsForTesting[tableID])
        XCTAssertEqual(record["irregular"] as? Bool, true)
        XCTAssertEqual(adapter.tableMappingsForTesting?.tables[tableID]?.cells.count, 5)
        let tableSurface = try XCTUnwrap(view.subviews.compactMap { $0 as? EditorTableSurface }.first)
        let drawing = try XCTUnwrap(tableSurface.subviews.compactMap { $0 as? PreparedProseDrawingView }.first)
        let prepared = try XCTUnwrap(drawing.layout?.blocks.first?.tableSurface)
        let source = try XCTUnwrap(prepared.cells.first { $0.sourceIndex == 2 })
        if rightToLeft {
            XCTAssertGreaterThan(prepared.frame(ofCell: source).minX, prepared.bounds.minX)
        } else {
            XCTAssertLessThan(prepared.frame(ofCell: source).maxX, prepared.bounds.maxX)
        }
        XCTAssertTrue(view.bindTableCell(tableID: tableID, cellIndex: 2, contentRect: .zero))
        let input = view.activeTextInput
        let direction: TableCellArrowDirection = rightToLeft ? .left : .right
        select(NSRange(location: rightToLeft ? 0 : 1, length: 0), in: input)
        let before = try XCTUnwrap(adapter.documentJson())
        try pressArrow(direction, in: input)
        XCTAssertEqual(
            input.tableCellPositionMap?.binding.documentPosition(in: adapter),
            adapter.tableMappingsForTesting?.tables[tableID]?.cells[3].sourcePos
        )
        XCTAssertTrue(view.activeTextInput === input)
        XCTAssertEqual(try XCTUnwrap(adapter.documentJson()), before)
    }

    private static let twoByTwoDocument = #"{"type":"doc","content":[{"type":"table","content":[{"type":"table_row","content":[{"type":"table_cell","content":[{"type":"paragraph","content":[{"type":"text","text":"top-left"}]}]},{"type":"table_cell","content":[{"type":"paragraph","content":[{"type":"text","text":"top-right"}]}]}]},{"type":"table_row","content":[{"type":"table_cell","content":[{"type":"paragraph","content":[{"type":"text","text":"bottom-left"}]}]},{"type":"table_cell","content":[{"type":"paragraph","content":[{"type":"text","text":"bottom-right"}]}]}]}]}]}"#
    private static let surroundedTableDocument = #"{"type":"doc","content":[{"type":"paragraph","content":[{"type":"text","text":"before"}]},{"type":"table","content":[{"type":"table_row","content":[{"type":"table_cell","content":[{"type":"paragraph","content":[{"type":"text","text":"one"}]}]},{"type":"table_cell","content":[{"type":"paragraph","content":[{"type":"text","text":"two"}]}]}]}]},{"type":"paragraph","content":[{"type":"text","text":"after"}]}]}"#
    private static let irregularRowDocument = #"{"type":"doc","content":[{"type":"table","content":[{"type":"table_row","content":[{"type":"table_cell","content":[{"type":"paragraph","content":[{"type":"text","text":"A"}]}]},{"type":"table_cell","content":[{"type":"paragraph","content":[{"type":"text","text":"B"}]}]}]},{"type":"table_row","content":[{"type":"table_cell","content":[{"type":"paragraph","content":[{"type":"text","text":"C"}]}]}]},{"type":"table_row","content":[{"type":"table_cell","content":[{"type":"paragraph","content":[{"type":"text","text":"D"}]}]},{"type":"table_cell","content":[{"type":"paragraph","content":[{"type":"text","text":"E"}]}]}]}]}]}"#
}
