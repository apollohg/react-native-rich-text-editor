import CoreText
import XCTest

enum TableInputTestSchema {
    static let tableConfig = #"{"schema":{"nodes":[{"name":"doc","content":"block+","role":"doc"},{"name":"paragraph","content":"inline*","group":"block","role":"textBlock"},{"name":"text","content":"","group":"inline","role":"text"},{"name":"table","content":"table_row+","group":"block","role":"block","tableRole":"table","attrs":{"class":{"default":null}}},{"name":"table_row","content":"(table_cell | table_header)*","role":"block","tableRole":"row"},{"name":"table_cell","content":"block+","role":"block","tableRole":"cell","attrs":{"class":{"default":null},"colspan":{"type":"number","default":1,"min":1},"rowspan":{"type":"number","default":1,"min":1},"colwidth":{"default":null}}},{"name":"table_header","content":"block+","role":"block","tableRole":"header_cell","attrs":{"class":{"default":null},"colspan":{"type":"number","default":1,"min":1},"rowspan":{"type":"number","default":1,"min":1},"colwidth":{"default":null}}}],"marks":[]},"initialization":{"type":"localEmpty"}}"#
    static let strongMarkTableConfig = tableConfig.replacingOccurrences(
        of: #""marks":[]"#, with: #""marks":[{"name":"\#(TableToolbarTestItems.strongMark)"}]"#
    )
    static let listTableConfig = #"{"schema":{"nodes":[{"name":"doc","content":"block+","role":"doc"},{"name":"paragraph","content":"inline*","group":"block","role":"textBlock"},{"name":"text","content":"","group":"inline","role":"text"},{"name":"bulletList","content":"listItem+","group":"block","role":"list"},{"name":"listItem","content":"block+","role":"listItem"},{"name":"table","content":"table_row+","group":"block","role":"block","tableRole":"table","attrs":{"class":{"default":null}}},{"name":"table_row","content":"(table_cell | table_header)*","role":"block","tableRole":"row"},{"name":"table_cell","content":"block+","role":"block","tableRole":"cell","attrs":{"class":{"default":null},"colspan":{"type":"number","default":1,"min":1},"rowspan":{"type":"number","default":1,"min":1},"colwidth":{"default":null}}},{"name":"table_header","content":"block+","role":"block","tableRole":"header_cell","attrs":{"class":{"default":null},"colspan":{"type":"number","default":1,"min":1},"rowspan":{"type":"number","default":1,"min":1},"colwidth":{"default":null}}}],"marks":[]},"initialization":{"type":"localEmpty"}}"#
    static let twoCellDocument = #"{"type":"doc","content":[{"type":"table","content":[{"type":"table_row","content":[{"type":"table_cell","content":[{"type":"paragraph","content":[{"type":"text","text":"one"}]}]},{"type":"table_cell","content":[{"type":"paragraph","content":[{"type":"text","text":"two"}]}]}]}]}]}"#
}

private final class ReentrantTableTabDelegate: NSObject, EditorTextViewDelegate {
    var onUpdate: (() -> Void)?

    func editorTextView(_ textView: EditorTextView, selectionDidChange anchor: UInt32, head: UInt32) {}

    func editorTextView(_ textView: EditorTextView, didReceiveUpdate updateJSON: String) {
        let callback = onUpdate
        onUpdate = nil
        callback?()
    }
}

final class EditorTableNavigationTests: XCTestCase {
    let tableConfig = TableInputTestSchema.tableConfig
    let listTableConfig = TableInputTestSchema.listTableConfig

    func testHardwareTabMovesFromSelectedTextToNextCellAndTypingEditsTarget() throws {
        let document = TableInputTestSchema.twoCellDocument
        let fixture = try makeFixture(config: tableConfig, document: document, size: Self.editorSize, windowed: true)
        defer { fixture.close() }
        let adapter = fixture.adapter
        let view = fixture.view
        let tableID = try XCTUnwrap(adapter.cachedTableInputMappings?.tables.keys.first)
        XCTAssertTrue(view.bindTableCell(tableID: tableID, cellIndex: 0, contentRect: .zero))
        let input = view.activeTextInput
        select(NSRange(location: 0, length: ("one" as NSString).length), in: input)
        XCTAssertNotEqual(input.currentScalarSelection()?.anchor, input.currentScalarSelection()?.head)
        XCTAssertTrue(input.becomeFirstResponder())
        let before = try XCTUnwrap(adapter.documentJson())
        let command = try XCTUnwrap(input.keyCommands?.first {
            $0.input == "\t" && $0.modifierFlags.isEmpty
        })
        _ = input.perform(command.action)
        XCTAssertEqual(try XCTUnwrap(adapter.documentJson()), before)
        XCTAssertEqual(view.activeTextInput.tableCellPositionMap?.binding.cellSourcePosition,
                       adapter.cachedTableInputMappings?.tables[tableID]?.cells[1].sourcePos)
        XCTAssertTrue(view.activeTextInput.isFirstResponder)
        view.activeTextInput.insertText("!")
        let edited = try XCTUnwrap(adapter.documentJson())
        XCTAssertTrue(edited.contains(#""text":"!two""#), edited)
        XCTAssertTrue(edited.contains(#""text":"one""#), edited)
    }

    func testHardwareShiftTabMovesBackwardAndStopsAtFirstCell() throws {
        let document = TableInputTestSchema.twoCellDocument
        let fixture = try makeFixture(config: tableConfig, document: document, size: Self.editorSize, windowed: true)
        defer { fixture.close() }
        let adapter = fixture.adapter
        let view = fixture.view
        let tableID = try XCTUnwrap(adapter.cachedTableInputMappings?.tables.keys.first)
        XCTAssertTrue(view.bindTableCell(tableID: tableID, cellIndex: 1, contentRect: .zero))
        let input = view.activeTextInput
        select(NSRange(location: 0, length: 0), in: input)
        XCTAssertTrue(input.becomeFirstResponder())
        let before = try XCTUnwrap(adapter.documentJson())
        let command = try XCTUnwrap(input.keyCommands?.first {
            $0.input == "\t" && $0.modifierFlags == [.shift]
        })
        _ = input.perform(command.action)
        XCTAssertEqual(try XCTUnwrap(adapter.documentJson()), before)
        XCTAssertEqual(view.activeTextInput.tableCellPositionMap?.binding.cellSourcePosition,
                       adapter.cachedTableInputMappings?.tables[tableID]?.cells[0].sourcePos)
        XCTAssertTrue(view.activeTextInput.isFirstResponder)
        _ = view.activeTextInput.perform(command.action)
        XCTAssertEqual(try XCTUnwrap(adapter.documentJson()), before)
        XCTAssertEqual(view.activeTextInput.tableCellPositionMap?.binding.cellSourcePosition,
                       adapter.cachedTableInputMappings?.tables[tableID]?.cells[0].sourcePos)
    }

    func testHardwareTabAtFinalCellAppendsRowAndUndoRestoresDocument() throws {
        let document = TableInputTestSchema.twoCellDocument
        let fixture = try makeFixture(config: tableConfig, document: document, size: Self.editorSize, windowed: true)
        defer { fixture.close() }
        let editorId = fixture.editorId
        let adapter = fixture.adapter
        let view = fixture.view
        let tableID = try XCTUnwrap(adapter.cachedTableInputMappings?.tables.keys.first)
        XCTAssertTrue(view.bindTableCell(tableID: tableID, cellIndex: 1, contentRect: .zero))
        let input = view.activeTextInput
        select(NSRange(location: 0, length: 0), in: input)
        XCTAssertTrue(input.becomeFirstResponder())
        let before = try XCTUnwrap(adapter.documentJson())
        let command = try XCTUnwrap(input.keyCommands?.first {
            $0.input == "\t" && $0.modifierFlags.isEmpty
        })
        _ = input.perform(command.action)
        let appended = try XCTUnwrap(adapter.documentJson())
        let object = try XCTUnwrap(JSONSerialization.jsonObject(with: Data(appended.utf8)) as? [String: Any])
        let contents = try XCTUnwrap(object["content"] as? [[String: Any]])
        let table = try XCTUnwrap(contents[0]["content"] as? [[String: Any]])
        XCTAssertEqual(table.count, 2)
        XCTAssertNotEqual(appended, before)
        XCTAssertEqual(view.activeTextInput.tableCellPositionMap?.binding.cellSourcePosition,
                       adapter.cachedTableInputMappings?.tables[tableID]?.cells[2].sourcePos)
        XCTAssertTrue(view.activeTextInput.isFirstResponder)
        XCTAssertTrue(view.textView.applyUpdateJSON(EditorV2Shadow.undo(id: editorId)))
        XCTAssertEqual(try XCTUnwrap(adapter.documentJson()), before)
        XCTAssertTrue(view.bindTableCell(tableID: tableID, cellIndex: 1, contentRect: .zero))
        let rebound = view.activeTextInput
        select(NSRange(location: 0, length: 0), in: rebound)
        _ = rebound.perform(command.action)
        view.activeTextInput.insertText("!")
        let typed = try XCTUnwrap(adapter.documentJson())
        let typedObject = try XCTUnwrap(JSONSerialization.jsonObject(with: Data(typed.utf8)) as? [String: Any])
        let typedTable = try XCTUnwrap((typedObject["content"] as? [[String: Any]])?.first?["content"] as? [[String: Any]])
        let newRow = try XCTUnwrap(typedTable[1]["content"] as? [[String: Any]])
        let newCell = try XCTUnwrap(newRow[0]["content"] as? [[String: Any]])
        XCTAssertTrue(String(describing: newCell).contains("!"), typed)
    }

    func testHardwareTabUsesLogicalOrderAcrossMergedRtlCells() throws {
        let config = tableConfig.replacingOccurrences(
            of: #""tableRole":"table","attrs":{"class":{"default":null}}"#,
            with: #""tableRole":"table","attrs":{"class":{"default":null},"dir":{"default":null}}"#
        )
        let document = #"{"type":"doc","content":[{"type":"table","attrs":{"dir":"rtl"},"content":[{"type":"table_row","content":[{"type":"table_cell","attrs":{"colspan":2},"content":[{"type":"paragraph","content":[{"type":"text","text":"wide"}]}]},{"type":"table_cell","content":[{"type":"paragraph","content":[{"type":"text","text":"next"}]}]}]}]}]}"#
        let fixture = try makeFixture(config: config, document: document, size: Self.editorSize, windowed: true)
        defer { fixture.close() }
        let adapter = fixture.adapter
        let view = fixture.view
        let tableID = try XCTUnwrap(adapter.cachedTableInputMappings?.tables.keys.first)
        let record = try XCTUnwrap(adapter.cachedTableRecords[tableID])
        XCTAssertEqual(record["direction"] as? String, "rtl")
        let cells = try XCTUnwrap(record["cells"] as? [[String: Any]])
        XCTAssertEqual(cells.count, 2)
        XCTAssertEqual(cells[0]["colspan"] as? Int, 2)
        XCTAssertTrue(view.bindTableCell(tableID: tableID, cellIndex: 0, contentRect: .zero))
        let input = view.activeTextInput
        select(NSRange(location: 0, length: 0), in: input)
        XCTAssertTrue(input.becomeFirstResponder())
        let before = try XCTUnwrap(adapter.documentJson())
        let command = try XCTUnwrap(input.keyCommands?.first {
            $0.input == "\t" && $0.modifierFlags.isEmpty
        })
        _ = input.perform(command.action)
        XCTAssertEqual(try XCTUnwrap(adapter.documentJson()), before)
        XCTAssertEqual(view.activeTextInput.tableCellPositionMap?.binding.cellSourcePosition,
                       adapter.cachedTableInputMappings?.tables[tableID]?.cells[1].sourcePos)
        view.activeTextInput.insertText("!")
        let edited = try XCTUnwrap(adapter.documentJson())
        XCTAssertTrue(edited.contains(#""text":"!next""#), edited)
        XCTAssertTrue(edited.contains(#""text":"wide""#), edited)
    }

    func testHardwareTabDoesNotRestoreStaleTargetAfterSameOwnerReentrantUpdate() throws {
        let document = TableInputTestSchema.twoCellDocument
        let replacement = #"{"type":"doc","content":[{"type":"paragraph","content":[{"type":"text","text":"replacement"}]}]}"#
        let fixture = try makeFixture(config: tableConfig, document: document, size: Self.editorSize)
        defer { fixture.close() }
        let adapter = fixture.adapter
        let view = fixture.view
        let tableID = try XCTUnwrap(adapter.cachedTableInputMappings?.tables.keys.first)
        XCTAssertTrue(view.bindTableCell(tableID: tableID, cellIndex: 0, contentRect: .zero))
        let input = view.activeTextInput
        select(NSRange(location: 0, length: 0), in: input)
        let delegate = ReentrantTableTabDelegate()
        view.textView.editorDelegate = delegate
        delegate.onUpdate = {
            guard let update = adapter.setContentJson(replacement) else { return }
            _ = view.textView.applyUpdateJSON(update, notifyDelegate: false)
        }
        let command = try XCTUnwrap(input.keyCommands?.first {
            $0.input == "\t" && $0.modifierFlags.isEmpty
        })
        _ = input.perform(command.action)
        XCTAssertNil(delegate.onUpdate)
        XCTAssertTrue(view.activeTextInput === view.textView)
        let settled = try XCTUnwrap(adapter.documentJson())
        let object = try XCTUnwrap(JSONSerialization.jsonObject(with: Data(settled.utf8)) as? [String: Any])
        let blocks = try XCTUnwrap(object["content"] as? [[String: Any]])
        XCTAssertEqual(blocks.count, 1)
        let text = try XCTUnwrap((blocks[0]["content"] as? [[String: Any]])?.first?["text"] as? String)
        XCTAssertEqual(text, "replacement")
        input.insertText("unsafe")
        XCTAssertEqual(try XCTUnwrap(adapter.documentJson()), settled)
    }

    func testHardwareTabEntersNestedContainingOuterCellAndEditsProseOnBothSides() throws {
        let document = try nestedDocument(leadingText: "outer", nestedTableCount: 1,
                                          trailingText: "after", precedingCellText: "one")
        let fixture = try makeFixture(config: tableConfig, document: document, size: Self.editorSize)
        defer { fixture.close() }
        let adapter = fixture.adapter
        let view = fixture.view
        let tableID = try XCTUnwrap(adapter.cachedTableInputMappings?.tables.keys.first { key in
            adapter.cachedTableInputMappings?.tables[key]?.cells.count == 2
        })
        let target = try XCTUnwrap(adapter.cachedTableInputMappings?.tables[tableID]?.cells[1])
        XCTAssertEqual(target.excluded.count, 1)
        XCTAssertEqual(target.blocks.count, 2)
        XCTAssertTrue(view.bindTableCell(tableID: tableID, cellIndex: 0, contentRect: .zero))
        let input = view.activeTextInput
        select(NSRange(location: 0, length: 0), in: input)
        let before = try XCTUnwrap(adapter.documentJson())
        let nestedBefore = try nestedTableContent(in: before, outerCellIndex: 1)
        let command = try XCTUnwrap(input.keyCommands?.first {
            $0.input == "\t" && $0.modifierFlags.isEmpty
        })
        _ = input.perform(command.action)
        XCTAssertEqual(try XCTUnwrap(adapter.documentJson()), before)
        XCTAssertTrue(view.activeTextInput === input)
        XCTAssertEqual(input.tableCellPositionMap?.binding.cellSourcePosition, target.sourcePos)
        var map = try XCTUnwrap(input.tableCellPositionMap)
        XCTAssertEqual(map.segments.count, 2)
        let first = try XCTUnwrap(map.segments.first)
        let firstScalar = try XCTUnwrap(map.globalScalar(forLocalScalar: first.localScalarRange.lowerBound))
        XCTAssertEqual(input.currentScalarSelection()?.head, firstScalar)
        input.insertText("A")
        map = try XCTUnwrap(input.tableCellPositionMap)
        let last = try XCTUnwrap(map.segments.last)
        let lastScalar = try XCTUnwrap(map.globalScalar(forLocalScalar: last.localScalarRange.lowerBound))
        input.selectedRange = NSRange(
            location: PositionBridge.scalarToUtf16Offset(last.localScalarRange.lowerBound, in: input),
            length: 0
        )
        input.textViewDidChangeSelection(input)
        XCTAssertEqual(input.currentScalarSelection()?.head, lastScalar)
        input.insertText("B")
        let edited = try XCTUnwrap(adapter.documentJson())
        XCTAssertTrue(edited.contains(#""text":"Aouter""#), edited)
        XCTAssertTrue(edited.contains(#""text":"Bafter""#), edited)
        XCTAssertTrue(edited.contains(#""text":"nested-0""#), edited)
        XCTAssertTrue(edited.contains(#""text":"one""#), edited)
        XCTAssertEqual(try nestedTableContent(in: edited, outerCellIndex: 1), nestedBefore)
        let reverse = try XCTUnwrap(input.keyCommands?.first {
            $0.input == "\t" && $0.modifierFlags == [.shift]
        })
        _ = input.perform(reverse.action)
        XCTAssertEqual(input.tableCellPositionMap?.binding.cellSourcePosition,
                       adapter.cachedTableInputMappings?.tables[tableID]?.cells[0].sourcePos)
        _ = input.perform(command.action)
        XCTAssertEqual(input.tableCellPositionMap?.binding.cellSourcePosition,
                       adapter.cachedTableInputMappings?.tables[tableID]?.cells[1].sourcePos)
        _ = input.perform(command.action)
        XCTAssertEqual(input.tableCellPositionMap?.binding.cellSourcePosition,
                       adapter.cachedTableInputMappings?.tables[tableID]?.cells[2].sourcePos)
        _ = input.perform(reverse.action)
        XCTAssertEqual(input.tableCellPositionMap?.binding.cellSourcePosition,
                       adapter.cachedTableInputMappings?.tables[tableID]?.cells[1].sourcePos)
    }

    func testNestedMarkerCannotReceiveCellInputOrCrossBlockEdits() throws {
        let document = try nestedDocument(leadingText: "before", nestedTableCount: 1,
                                          trailingText: "after")
        let fixture = try makeFixture(config: tableConfig, document: document, size: Self.editorSize)
        defer { fixture.close() }
        let adapter = fixture.adapter
        let view = fixture.view
        let tableID = try XCTUnwrap(adapter.cachedTableInputMappings?.tables.keys.first { key in
            adapter.cachedTableInputMappings?.tables[key]?.cells.contains { !$0.excluded.isEmpty } == true
        })
        let cell = try XCTUnwrap(adapter.cachedTableInputMappings?.tables[tableID]?.cells.first)
        let nested = try XCTUnwrap(cell.excluded.first)
        XCTAssertFalse(view.bindTableCell(tableID: nested.tableID, cellIndex: 0, contentRect: .zero))
        XCTAssertTrue(view.bindTableCell(tableID: tableID, cellIndex: 0, contentRect: .zero))
        let input = view.activeTextInput
        let map = try XCTUnwrap(input.tableCellPositionMap)
        XCTAssertEqual(map.segments.count, 2)
        var marker: RenderBridge.RootTableScalarExtent?
        var markerRange: NSRange?
        input.textStorage.enumerateAttribute(
            RenderBridgeAttributes.rootTableScalarExtent,
            in: NSRange(location: 0, length: input.textStorage.length)
        ) { value, range, _ in
            if let extent = value as? RenderBridge.RootTableScalarExtent {
                marker = extent
                markerRange = range
            }
        }
        let extent = try XCTUnwrap(marker)
        let utf16Range = try XCTUnwrap(markerRange)
        XCTAssertEqual(extent.tableID, nested.tableID)
        XCTAssertGreaterThan(extent.scalarEnd - extent.scalarStart, 1)
        let interior = extent.scalarStart + 1
        XCTAssertNil(input.inputScalar(atLocalScalar: interior))
        XCTAssertNil(input.inputScalarRange(fromLocal: map.segments[0].localScalarRange.lowerBound,
                                            toLocal: map.segments[1].localScalarRange.lowerBound))
        let before = try XCTUnwrap(adapter.documentJson())
        input.selectedRange = utf16Range
        input.textViewDidChangeSelection(input)
        input.insertText("unsafe")
        input.deleteBackward()
        XCTAssertEqual(try XCTUnwrap(adapter.documentJson()), before)
        XCTAssertTrue(try XCTUnwrap(adapter.documentJson()).contains(#""text":"nested-0""#))
    }

    func testNestedTableReservationAlignsFollowingOuterProseWithPreparedLayout() throws {
        let cases: [(leading: String?, nestedCount: Int)] = [
            ("before", 1), (nil, 1), ("before", 2)
        ]
        for sample in cases {
            let document = try nestedDocument(leadingText: sample.leading,
                                              nestedTableCount: sample.nestedCount,
                                              trailingText: "after")
            let fixture = try makeFixture(config: tableConfig, document: document, size: Self.geometrySize)
            defer { fixture.close() }
            let adapter = fixture.adapter
            let view = fixture.view
            XCTAssertTrue(view.applyTheme(EditorTheme(dictionary: [
                "version": 1,
                "styles": ["table": ["marginTop": 17, "marginBottom": 23]]
            ])))
            view.layoutIfNeeded()
            let tableID = try XCTUnwrap(adapter.cachedTableInputMappings?.tables.keys.first { key in
                adapter.cachedTableInputMappings?.tables[key]?.cells.contains { !$0.excluded.isEmpty } == true
            })
            let tableSurface = try XCTUnwrap(view.subviews.compactMap { $0 as? EditorTableSurface }.first)
            let drawing = try XCTUnwrap(tableSurface.subviews.compactMap { $0 as? PreparedProseDrawingView }.first)
            XCTAssertTrue(view.bindTableCell(tableID: tableID, cellIndex: 0, contentRect: .zero))
            let input = view.activeTextInput
            func assertAligned(leadingText: String?) throws {
                view.layoutIfNeeded()
                let outer = try XCTUnwrap(drawing.layout?.blocks.first?.tableSurface)
                let prepared = try XCTUnwrap(outer.cells.first?.content)
                XCTAssertEqual(prepared.blocks.count, sample.nestedCount + (leadingText == nil ? 1 : 2))
                input.layoutManager.ensureLayout(for: input.textContainer)
                let after = (input.textStorage.string as NSString).range(of: "after")
                let lastGlyphs = input.layoutManager.glyphRange(forCharacterRange: after, actualCharacterRange: nil)
                let lastY = input.layoutManager.boundingRect(forGlyphRange: lastGlyphs,
                                                             in: input.textContainer).minY
                var currentMarkerHeight: CGFloat?
                input.textStorage.enumerateAttribute(
                    RenderBridgeAttributes.rootTableScalarExtent,
                    in: NSRange(location: 0, length: input.textStorage.length)
                ) { value, range, _ in
                    guard value is RenderBridge.RootTableScalarExtent else { return }
                    currentMarkerHeight = (input.textStorage.attribute(
                        .paragraphStyle, at: range.location, effectiveRange: nil
                    ) as? NSParagraphStyle)?.minimumLineHeight
                }
                XCTAssertEqual(lastY, try XCTUnwrap(prepared.blocks.last).bounds.minY, accuracy: 3,
                               "leading=\(String(describing: leadingText)) nested=\(sample.nestedCount) reserved=\(tableSurface.nestedTableHeights(tableID: tableID, cellIndex: 0, input: input) ?? [:]) current=\(String(describing: currentMarkerHeight))")
                if let leadingText {
                    let before = (input.textStorage.string as NSString).range(of: leadingText)
                    let firstGlyphs = input.layoutManager.glyphRange(forCharacterRange: before,
                                                                     actualCharacterRange: nil)
                    let firstY = input.layoutManager.boundingRect(forGlyphRange: firstGlyphs,
                                                                  in: input.textContainer).minY
                    XCTAssertEqual(firstY, prepared.blocks[0].bounds.minY, accuracy: 3)
                }
            }
            try assertAligned(leadingText: sample.leading)
            if sample.leading != nil && sample.nestedCount == 1 {
                view.frame.size.width = 280
                try assertAligned(leadingText: "before")
                XCTAssertTrue(view.activeTextInput === input)
                XCTAssertTrue(view.applyTheme(EditorTheme(dictionary: [
                    "version": 1,
                    "styles": ["table": ["marginTop": 11, "marginBottom": 13]]
                ])))
                if input.tableCellPositionMap == nil {
                    let settled = try XCTUnwrap(adapter.documentJson())
                    input.insertText("unsafe")
                    XCTAssertEqual(try XCTUnwrap(adapter.documentJson()), settled)
                    XCTAssertTrue(view.bindTableCell(tableID: tableID, cellIndex: 0, contentRect: .zero))
                }
                try assertAligned(leadingText: "before")
                XCTAssertTrue(view.activeTextInput === input)
                select(NSRange(location: 0, length: 0), in: input)
                input.insertText("X")
                try assertAligned(leadingText: "Xbefore")
                XCTAssertTrue(view.activeTextInput === input)

                let replacement = try nestedDocument(leadingText: "remote", nestedTableCount: 1,
                                                     trailingText: "after")
                XCTAssertTrue(view.textView.applyUpdateJSON(try XCTUnwrap(adapter.setContentJson(replacement))))
                if input.tableCellPositionMap == nil {
                    let settled = try XCTUnwrap(adapter.documentJson())
                    input.insertText("unsafe")
                    XCTAssertEqual(try XCTUnwrap(adapter.documentJson()), settled)
                    XCTAssertTrue(view.bindTableCell(tableID: tableID, cellIndex: 0, contentRect: .zero))
                }
                try assertAligned(leadingText: "remote")
            }
        }
    }

    func testActiveNestedCellOpaqueHostScreenshots() throws {
        let samples: [(name: String, size: CGSize, style: UIUserInterfaceStyle, ground: UIColor)] = [
            ("phone-light", Self.editorSize, .light, .white),
            ("phone-dark", Self.editorSize, .dark, .black),
            ("tablet-width-light", CGSize(width: 768, height: 480), .light, .white),
            ("tablet-width-dark", CGSize(width: 768, height: 480), .dark, .black)
        ]
        let document = try nestedDocument(leadingText: "outer", nestedTableCount: 1,
                                          trailingText: "after", precedingCellText: "one")
        for sample in samples {
            let fixture = try makeFixture(config: tableConfig, document: document,
                                          size: sample.size, windowed: true)
            defer { fixture.close() }
            let view = fixture.view
            fixture.window?.overrideUserInterfaceStyle = sample.style
            fixture.window?.backgroundColor = sample.ground
            view.backgroundColor = sample.ground
            view.textView.backgroundColor = sample.ground
            view.layoutIfNeeded()
            let tableID = try XCTUnwrap(fixture.adapter.cachedTableInputMappings?.tables.keys.first { key in
                fixture.adapter.cachedTableInputMappings?.tables[key]?.cells.contains {
                    !$0.excluded.isEmpty
                } == true
            })
            XCTAssertTrue(view.bindTableCell(tableID: tableID, cellIndex: 1, contentRect: .zero))
            XCTAssertNotNil(view.activeTextInput.tableCellPositionMap)
            view.layoutIfNeeded()
            let format = UIGraphicsImageRendererFormat()
            format.scale = 1
            format.opaque = true
            let image = UIGraphicsImageRenderer(size: sample.size, format: format).image { context in
                sample.ground.setFill()
                context.fill(CGRect(origin: .zero, size: sample.size))
                view.layer.render(in: context.cgContext)
            }
            let attachment = XCTAttachment(image: image)
            attachment.name = "active-nested-\(sample.name)"
            attachment.lifetime = .keepAlways
            add(attachment)
        }
    }

    func testHardwareTabRejectsReadOnlyAndStaleRevision() throws {
        let document = TableInputTestSchema.twoCellDocument
        let replacement = #"{"type":"doc","content":[{"type":"paragraph","content":[{"type":"text","text":"replacement"}]}]}"#
        let fixture = try makeFixture(config: tableConfig, document: document, size: Self.editorSize)
        defer { fixture.close() }
        let adapter = fixture.adapter
        let view = fixture.view
        let tableID = try XCTUnwrap(adapter.cachedTableInputMappings?.tables.keys.first)
        XCTAssertTrue(view.bindTableCell(tableID: tableID, cellIndex: 0, contentRect: .zero))
        let input = view.activeTextInput
        select(NSRange(location: 0, length: 0), in: input)
        let command = try XCTUnwrap(input.keyCommands?.first {
            $0.input == "\t" && $0.modifierFlags.isEmpty
        })
        let before = try XCTUnwrap(adapter.documentJson())
        input.isEditable = false
        _ = input.perform(command.action)
        XCTAssertEqual(try XCTUnwrap(adapter.documentJson()), before)
        XCTAssertTrue(view.activeTextInput === input)
        input.isEditable = true
        _ = try XCTUnwrap(adapter.setContentJson(replacement))
        let settled = try XCTUnwrap(adapter.documentJson())
        _ = input.perform(command.action)
        XCTAssertEqual(try XCTUnwrap(adapter.documentJson()), settled)
        input.insertText("unsafe")
        XCTAssertEqual(try XCTUnwrap(adapter.documentJson()), settled)
    }

    func testHardwareTabCommitsMarkedCompositionBeforeNextNavigation() throws {
        let document = TableInputTestSchema.twoCellDocument
        let fixture = try makeFixture(config: tableConfig, document: document, size: Self.editorSize)
        defer { fixture.close() }
        let adapter = fixture.adapter
        let view = fixture.view
        let tableID = try XCTUnwrap(adapter.cachedTableInputMappings?.tables.keys.first)
        XCTAssertTrue(view.bindTableCell(tableID: tableID, cellIndex: 0, contentRect: .zero))
        let input = view.activeTextInput
        select(NSRange(location: 0, length: 0), in: input)
        input.setMarkedText("Z", selectedRange: NSRange(location: 1, length: 0))
        XCTAssertNotNil(input.markedTextRange)
        let command = try XCTUnwrap(input.keyCommands?.first {
            $0.input == "\t" && $0.modifierFlags.isEmpty
        })
        _ = input.perform(command.action)
        let edited = try XCTUnwrap(adapter.documentJson())
        XCTAssertTrue(edited.contains(#""text":"Zone""#), edited)
        XCTAssertTrue(edited.contains(#""text":"two""#), edited)
        XCTAssertNil(view.activeTextInput.markedTextRange)
        XCTAssertEqual(view.activeTextInput.tableCellPositionMap?.binding.cellSourcePosition,
                       adapter.cachedTableInputMappings?.tables[tableID]?.cells[0].sourcePos)
        _ = view.activeTextInput.perform(command.action)
        XCTAssertEqual(view.activeTextInput.tableCellPositionMap?.binding.cellSourcePosition,
                       adapter.cachedTableInputMappings?.tables[tableID]?.cells[1].sourcePos)
        view.activeTextInput.insertText("!")
        let typed = try XCTUnwrap(adapter.documentJson())
        XCTAssertTrue(typed.contains(#""text":"!two""#), typed)
    }

    func testHardwareTabKeepsRootListIndentBehavior() throws {
        let document = #"{"type":"doc","content":[{"type":"bulletList","content":[{"type":"listItem","content":[{"type":"paragraph","content":[{"type":"text","text":"one"}]}]},{"type":"listItem","content":[{"type":"paragraph","content":[{"type":"text","text":"two"}]}]}]}]}"#
        let fixture = try makeFixture(config: listTableConfig, document: document, size: Self.editorSize)
        defer { fixture.close() }
        let adapter = fixture.adapter
        let view = fixture.view
        let root = view.textView
        let range = (root.text as NSString).range(of: "two")
        XCTAssertNotEqual(range.location, NSNotFound)
        root.selectedRange = NSRange(location: range.location, length: 0)
        root.textViewDidChangeSelection(root)
        let command = try XCTUnwrap(root.keyCommands?.first {
            $0.input == "\t" && $0.modifierFlags.isEmpty
        })
        _ = root.perform(command.action)
        let edited = try XCTUnwrap(adapter.documentJson())
        let object = try XCTUnwrap(JSONSerialization.jsonObject(with: Data(edited.utf8)) as? [String: Any])
        let top = try XCTUnwrap(object["content"] as? [[String: Any]])
        let items = try XCTUnwrap(top[0]["content"] as? [[String: Any]])
        let firstItem = try XCTUnwrap(items.first?["content"] as? [[String: Any]])
        XCTAssertTrue(firstItem.contains { $0["type"] as? String == "bulletList" }, edited)
    }

    func testHardwareTabCommitsExternalCompositionBeforeNextNavigation() throws {
        let document = TableInputTestSchema.twoCellDocument
        let fixture = try makeFixture(config: tableConfig, document: document, size: Self.editorSize)
        defer { fixture.close() }
        let adapter = fixture.adapter
        let view = fixture.view
        let tableID = try XCTUnwrap(adapter.cachedTableInputMappings?.tables.keys.first)
        XCTAssertTrue(view.bindTableCell(tableID: tableID, cellIndex: 0, contentRect: .zero))
        let input = view.activeTextInput
        select(NSRange(location: 0, length: 0), in: input)
        let began = input.beginExternalTextComposition(sessionId: "table-tab-speech")
        let beganObject = try XCTUnwrap(JSONSerialization.jsonObject(with: Data(began.utf8)) as? [String: Any])
        XCTAssertEqual(beganObject["type"] as? String, "active")
        XCTAssertEqual(beganObject["sessionId"] as? String, "table-tab-speech")
        let updated = input.updateExternalTextComposition(sessionId: "table-tab-speech", text: "Z")
        let updatedObject = try XCTUnwrap(JSONSerialization.jsonObject(with: Data(updated.utf8)) as? [String: Any])
        XCTAssertEqual(updatedObject["type"] as? String, "active")
        let command = try XCTUnwrap(input.keyCommands?.first {
            $0.input == "\t" && $0.modifierFlags.isEmpty
        })
        _ = input.perform(command.action)
        let committed = try XCTUnwrap(adapter.documentJson())
        XCTAssertTrue(committed.contains(#""text":"Zone""#), committed)
        XCTAssertTrue(committed.contains(#""text":"two""#), committed)
        XCTAssertTrue(view.activeTextInput === input)
        XCTAssertFalse(input.hasPendingCompositionForExternalRefresh)
        XCTAssertEqual(view.activeTextInput.tableCellPositionMap?.binding.cellSourcePosition,
                       adapter.cachedTableInputMappings?.tables[tableID]?.cells[0].sourcePos)
        _ = input.perform(command.action)
        XCTAssertEqual(view.activeTextInput.tableCellPositionMap?.binding.cellSourcePosition,
                       adapter.cachedTableInputMappings?.tables[tableID]?.cells[1].sourcePos)
        view.activeTextInput.insertText("!")
        let typed = try XCTUnwrap(adapter.documentJson())
        XCTAssertTrue(typed.contains(#""text":"!two""#), typed)
    }

    func testExternalCompositionRefusesExplicitlyStaleTableCellEpoch() throws {
        let fixture = try makeFixture(config: tableConfig,
                                      document: TableInputTestSchema.twoCellDocument,
                                      size: Self.editorSize)
        defer { fixture.close() }
        let adapter = fixture.adapter
        let view = fixture.view
        let tableID = try XCTUnwrap(adapter.cachedTableInputMappings?.tables.keys.first)
        XCTAssertTrue(view.bindTableCell(tableID: tableID, cellIndex: 0, contentRect: .zero))
        let input = view.activeTextInput
        select(NSRange(location: 0, length: 0), in: input)
        let binding = try XCTUnwrap(input.tableCellPositionMap?.binding)
        let before = try XCTUnwrap(adapter.documentJson())
        _ = adapter.selectionJSON()
        XCTAssertNotEqual(adapter.positionEpoch, binding.positionEpoch)

        let result = input.beginExternalTextComposition(sessionId: "stale-table-speech")
        let object = try XCTUnwrap(JSONSerialization.jsonObject(with: Data(result.utf8)) as? [String: Any])
        XCTAssertEqual(object["type"] as? String, "error")
        let error = try XCTUnwrap(object["error"] as? [String: Any])
        XCTAssertEqual(error["code"] as? String, "EXTERNAL_COMPOSITION_SELECTION_INCOMPATIBLE")
        XCTAssertFalse(input.hasPendingCompositionForExternalRefresh)
        let command = try XCTUnwrap(input.keyCommands?.first {
            $0.input == "\t" && $0.modifierFlags.isEmpty
        })
        _ = input.perform(command.action)
        input.insertText("unsafe")
        XCTAssertEqual(try XCTUnwrap(adapter.documentJson()), before)
    }

    func testHardwareTabTakesPrecedenceOverListIndentInsideCell() throws {
        let document = #"{"type":"doc","content":[{"type":"table","content":[{"type":"table_row","content":[{"type":"table_cell","content":[{"type":"bulletList","content":[{"type":"listItem","content":[{"type":"paragraph","content":[{"type":"text","text":"one"}]}]},{"type":"listItem","content":[{"type":"paragraph","content":[{"type":"text","text":"two"}]}]}]}]},{"type":"table_cell","content":[{"type":"paragraph","content":[{"type":"text","text":"target"}]}]}]}]}]}"#
        let fixture = try makeFixture(config: listTableConfig, document: document, size: Self.editorSize)
        defer { fixture.close() }
        let adapter = fixture.adapter
        let view = fixture.view
        let tableID = try XCTUnwrap(adapter.cachedTableInputMappings?.tables.keys.first)
        XCTAssertTrue(view.bindTableCell(tableID: tableID, cellIndex: 0, contentRect: .zero))
        let input = view.activeTextInput
        select(NSRange(location: 0, length: 0), in: input)
        let before = try XCTUnwrap(adapter.documentJson())
        let command = try XCTUnwrap(input.keyCommands?.first {
            $0.input == "\t" && $0.modifierFlags.isEmpty
        })
        _ = input.perform(command.action)
        XCTAssertEqual(try XCTUnwrap(adapter.documentJson()), before)
        XCTAssertEqual(view.activeTextInput.tableCellPositionMap?.binding.cellSourcePosition,
                       adapter.cachedTableInputMappings?.tables[tableID]?.cells[1].sourcePos)
        view.activeTextInput.insertText("!")
        let edited = try XCTUnwrap(adapter.documentJson())
        XCTAssertTrue(edited.contains(#""text":"!target""#), edited)
    }

    func testHardwareTabFromOldNativeOwnerCannotMutateOrRebind() throws {
        let fixture = try makeFixture(config: tableConfig, document: TableInputTestSchema.twoCellDocument,
                                      size: Self.editorSize)
        defer { fixture.close() }
        let editorId = fixture.editorId
        let adapter = fixture.adapter
        let first = fixture.view
        let tableID = try XCTUnwrap(adapter.cachedTableInputMappings?.tables.keys.first)
        XCTAssertTrue(first.bindTableCell(tableID: tableID, cellIndex: 0, contentRect: .zero))
        let oldInput = first.activeTextInput
        select(NSRange(location: 0, length: 0), in: oldInput)
        let command = try XCTUnwrap(oldInput.keyCommands?.first {
            $0.input == "\t" && $0.modifierFlags.isEmpty
        })
        first.editorId = 0
        let second = RichTextEditorView(frame: first.frame)
        second.bindEditor(id: editorId, initialUpdateJSON: try XCTUnwrap(adapter.initialUpdateJSON()))
        XCTAssertTrue(second.textView.ownsNativeBinding(adapter))
        XCTAssertFalse(first.textView.ownsNativeBinding(adapter))
        let before = try XCTUnwrap(adapter.documentJson())
        _ = oldInput.perform(command.action)
        XCTAssertEqual(try XCTUnwrap(adapter.documentJson()), before)
        XCTAssertTrue(first.activeTextInput === first.textView)
        oldInput.insertText("unsafe")
        XCTAssertEqual(try XCTUnwrap(adapter.documentJson()), before)
    }
    static let editorSize = CGSize(width: 360, height: 220)
    static let geometrySize = CGSize(width: 360, height: 360)

    struct Fixture {
        let editorId: UInt64
        let adapter: EditorV2Adapter
        let view: RichTextEditorView
        let window: UIWindow?

        func close() {
            window?.isHidden = true
            destroyV2Editor(id: editorId)
        }
    }

    func makeFixture(
        config: String,
        document: String,
        size: CGSize,
        windowed: Bool = false
    ) throws -> Fixture {
        let editorId = makeV2Editor(configJson: config)
        var finished = false
        defer { if !finished { destroyV2Editor(id: editorId) } }
        let adapter = try XCTUnwrap(EditorV2Registry.adapter(forLegacyId: editorId))
        let frame = CGRect(origin: .zero, size: size)
        let window = windowed ? UIWindow(frame: frame) : nil
        let view = RichTextEditorView(frame: frame)
        window?.addSubview(view)
        window?.makeKeyAndVisible()
        view.bindEditor(id: editorId, initialUpdateJSON: try XCTUnwrap(adapter.initialUpdateJSON()))
        XCTAssertTrue(view.textView.applyUpdateJSON(try XCTUnwrap(adapter.setContentJson(document))))
        finished = true
        return Fixture(editorId: editorId, adapter: adapter, view: view, window: window)
    }

    func nestedDocument(
        leadingText: String?,
        nestedTableCount: Int,
        trailingText: String,
        precedingCellText: String? = nil
    ) throws -> String {
        func paragraph(_ text: String) -> [String: Any] {
            ["type": "paragraph", "content": [["type": "text", "text": text]]]
        }
        func cell(_ content: [[String: Any]]) -> [String: Any] {
            ["type": "table_cell", "content": content]
        }
        var outerContent: [[String: Any]] = []
        if let leadingText { outerContent.append(paragraph(leadingText)) }
        for index in 0..<nestedTableCount {
            outerContent.append([
                "type": "table",
                "content": [["type": "table_row", "content": [cell([paragraph("nested-\(index)")])]]]
            ])
        }
        outerContent.append(paragraph(trailingText))
        var cells: [[String: Any]] = []
        if let precedingCellText { cells.append(cell([paragraph(precedingCellText)])) }
        cells.append(cell(outerContent))
        let document: [String: Any] = [
            "type": "doc",
            "content": [["type": "table", "content": [["type": "table_row", "content": cells]]]]
        ]
        let data = try JSONSerialization.data(withJSONObject: document)
        return try XCTUnwrap(String(data: data, encoding: .utf8))
    }

    func nestedTableContent(in json: String, outerCellIndex: Int) throws -> Data {
        let object = try XCTUnwrap(JSONSerialization.jsonObject(with: Data(json.utf8)) as? [String: Any])
        let table = try XCTUnwrap((object["content"] as? [[String: Any]])?.first)
        let row = try XCTUnwrap((table["content"] as? [[String: Any]])?.first)
        let cells = try XCTUnwrap(row["content"] as? [[String: Any]])
        let content = try XCTUnwrap(cells[outerCellIndex]["content"] as? [[String: Any]])
        let nested = try XCTUnwrap(content.first { $0["type"] as? String == "table" })
        return try JSONSerialization.data(withJSONObject: nested, options: [.sortedKeys])
    }

    func select(_ range: NSRange, in input: EditorTextView) {
        input.selectedRange = range
        input.textViewDidChangeSelection(input)
        XCTAssertNotNil(input.currentScalarSelection())
    }
}
