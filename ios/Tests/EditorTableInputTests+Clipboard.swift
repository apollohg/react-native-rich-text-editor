import XCTest

extension EditorTableInputTests {
    private enum TableClipboard {
        static let config = #"{"schema":{"nodes":[{"name":"doc","content":"block+","role":"doc"},{"name":"paragraph","content":"inline*","group":"block","role":"textBlock","htmlTag":"p"},{"name":"text","content":"","group":"inline","role":"text"},{"name":"mention","role":"inline","group":"inline","isVoid":true,"attrs":{"id":{},"label":{"default":""}},"allowUndeclaredAttrs":true},{"name":"table","content":"table_row+","group":"block","role":"block","tableRole":"table","htmlTag":"table"},{"name":"table_row","content":"(table_cell | table_header)*","role":"block","tableRole":"row","htmlTag":"tr"},{"name":"table_cell","content":"block+","role":"block","tableRole":"cell","htmlTag":"td","attrs":{"colspan":{"type":"number","default":1,"min":1},"rowspan":{"type":"number","default":1,"min":1},"colwidth":{"default":null}}},{"name":"table_header","content":"block+","role":"block","tableRole":"header_cell","htmlTag":"th","attrs":{"colspan":{"type":"number","default":1,"min":1},"rowspan":{"type":"number","default":1,"min":1},"colwidth":{"default":null}}}],"marks":[]},"initialization":{"type":"localEmpty"}}"#
        static let gridDocument = #"{"type":"doc","content":[{"type":"table","content":[{"type":"table_row","content":[{"type":"table_cell","content":[{"type":"paragraph","content":[{"type":"text","text":"A"}]}]},{"type":"table_cell","content":[{"type":"paragraph","content":[{"type":"text","text":"B"}]}]}]},{"type":"table_row","content":[{"type":"table_cell","content":[{"type":"paragraph","content":[{"type":"text","text":"C"}]}]},{"type":"table_cell","content":[{"type":"paragraph","content":[{"type":"text","text":"D"}]}]}]}]},{"type":"paragraph","content":[{"type":"text","text":"after"}]}]}"#
        static let mergedDocument = #"{"type":"doc","content":[{"type":"table","content":[{"type":"table_row","content":[{"type":"table_cell","attrs":{"colspan":2},"content":[{"type":"paragraph","content":[{"type":"text","text":"wide"}]}]},{"type":"table_cell","content":[{"type":"paragraph","content":[{"type":"text","text":"right"}]}]}]},{"type":"table_row","content":[{"type":"table_cell","content":[{"type":"paragraph","content":[{"type":"text","text":"c0"}]}]},{"type":"table_cell","content":[{"type":"paragraph","content":[{"type":"text","text":"c1"}]}]},{"type":"table_cell","content":[{"type":"paragraph","content":[{"type":"text","text":"c2"}]}]}]}]}]}"#
        static let nestedDocument = #"{"type":"doc","content":[{"type":"paragraph","content":[{"type":"text","text":"before"}]},{"type":"table","content":[{"type":"table_row","content":[{"type":"table_cell","content":[{"type":"paragraph","content":[{"type":"text","text":"Alpha"}]}]},{"type":"table_cell","content":[{"type":"table","content":[{"type":"table_row","content":[{"type":"table_cell","content":[{"type":"paragraph","content":[{"type":"text","text":"Nested"}]}]}]}]}]}]}]},{"type":"paragraph","content":[{"type":"text","text":"after"}]}]}"#
        static let atomDocument = #"{"type":"doc","content":[{"type":"table","content":[{"type":"table_row","content":[{"type":"table_cell","content":[{"type":"paragraph","content":[{"type":"text","text":"a"},{"type":"mention","attrs":{"id":"m1","label":"Sam","metadata":{"kind":"person"}}}]}]},{"type":"table_cell","content":[{"type":"paragraph","content":[{"type":"text","text":"b"}]}]}]},{"type":"table_row","content":[{"type":"table_cell","content":[{"type":"paragraph","content":[{"type":"text","text":"c"}]}]},{"type":"table_cell","content":[{"type":"paragraph","content":[{"type":"text","text":"d"}]}]}]}]}]}"#
        static let mergedRectangleTSV = "wide\t\nc0\tc1"
        static let mergedRectangleHTML = #"<table><tbody><tr><td colspan="2" rowspan="1"><p>wide</p></td></tr><tr><td colspan="1" rowspan="1"><p>c0</p></td><td colspan="1" rowspan="1"><p>c1</p></td></tr></tbody></table>"#
        static let firstRowTSV = "A\tB"
        static let pastedGridTSV = "w\tx\ny\tz"
        static let plainAlternativeTSV = "p1\tp2"
        static let htmlTable = "<table><tr><td>h1</td><td>h2</td></tr></table>"
        static let nestedCellText = "Nested"
        static let atomMetadataKind = "person"
        static let staleText = "stale clipboard"
        static let htmlType = "public.html"
        static let plainTextType = "public.utf8-plain-text"
        static let cellSelectionType = "cell"
        static let mergedWideCell = 0
        static let mergedSecondRowMiddleCell = 3
        static let mergedColspan = 2
        static let firstCell = 0
        static let secondCell = 1
        static let thirdCell = 2
        static let lastCell = 3
        static let editorSize = CGSize(width: 480, height: 320)
        static let emittedErrorNote = "emit "
    }

    private func withClipboardTable(
        _ document: String, anchorIndex: Int? = nil, headIndex: Int? = nil,
        _ body: (MountedTableFixture) throws -> Void
    ) throws {
        UIPasteboard.general.items = []
        defer { UIPasteboard.general.items = [] }
        try withMountedTable(document: document, configJSON: TableClipboard.config,
                             size: TableClipboard.editorSize, cellSelection: nil) { fixture in
            XCTAssertEqual(fixture.adapter.historyFlags()?.canUndo, false, "fixture must start without history")
            if let anchorIndex, let headIndex {
                try select(fixture, anchor: fixture.positions[anchorIndex], head: fixture.positions[headIndex])
            }
            try body(fixture)
        }
    }

    private func select(_ fixture: MountedTableFixture, anchor: UInt32, head: UInt32) throws {
        try fixture.view.textView.selectTableCells(adapter: fixture.adapter, anchor: anchor, head: head)
        XCTAssertTrue(fixture.view.textView.authoritativeCellSelectionActive, "root did not adopt the cell selection")
        fixture.updates.updates.removeAll()
    }

    private func tableRows(_ fixture: MountedTableFixture) throws -> [[String: Any]] {
        let root = try XCTUnwrap(fixture.documentObject() as? [String: Any])
        let content = try XCTUnwrap(root["content"] as? [[String: Any]])
        let table = try XCTUnwrap(content.first { $0["type"] as? String == "table" })
        return try XCTUnwrap(table["content"] as? [[String: Any]])
    }

    private func cellTexts(_ fixture: MountedTableFixture) throws -> [[String]] {
        func text(_ node: [String: Any]) -> String {
            if node["type"] as? String == "text" { return node["text"] as? String ?? "" }
            return (node["content"] as? [[String: Any]] ?? []).map(text).joined()
        }
        return try tableRows(fixture).map { row in
            (row["content"] as? [[String: Any]] ?? []).map(text)
        }
    }

    private func assertOneUndoableUpdate(_ fixture: MountedTableFixture, restoring before: NSDictionary,
                                         file: StaticString = #filePath, line: UInt = #line) throws {
        XCTAssertEqual(fixture.updates.updates.count, 1, "exactly one published update", file: file, line: line)
        XCTAssertEqual(fixture.adapter.historyFlags()?.canUndo, true, file: file, line: line)
        XCTAssertTrue(fixture.view.textView.applyUpdateJSON(try XCTUnwrap(fixture.adapter.undo())),
                      file: file, line: line)
        XCTAssertEqual(try fixture.documentObject(), before, "one undo must restore the table", file: file, line: line)
        XCTAssertEqual(fixture.adapter.historyFlags()?.canUndo, false,
                       "the action must be a single history entry", file: file, line: line)
    }

    func testCellSelectionCopyWritesExactTableFlavoursWithoutAnUpdate() throws {
        try withClipboardTable(TableClipboard.mergedDocument, anchorIndex: TableClipboard.mergedWideCell,
                               headIndex: TableClipboard.mergedSecondRowMiddleCell) { fixture in
            let root = fixture.view.textView
            root.selectedRange = NSRange(location: 0, length: 0)
            let before = try fixture.documentObject()
            let revision = fixture.adapter.baseDocumentRevision
            UIPasteboard.general.string = TableClipboard.staleText

            XCTAssertTrue(root.canPerformAction(#selector(UIResponderStandardEditActions.copy(_:)), withSender: nil))
            root.copy(nil)

            XCTAssertEqual(UIPasteboard.general.string, TableClipboard.mergedRectangleTSV)
            let html = try XCTUnwrap(UIPasteboard.general.data(forPasteboardType: TableClipboard.htmlType))
            XCTAssertEqual(String(data: html, encoding: .utf8), TableClipboard.mergedRectangleHTML)
            let fragmentData = try XCTUnwrap(
                UIPasteboard.general.data(forPasteboardType: EditorClipboardPayload.fragmentType)
            )
            let fragment = try XCTUnwrap(JSONSerialization.jsonObject(with: fragmentData) as? [String: Any])
            XCTAssertEqual(fragment["text"] as? String, TableClipboard.mergedRectangleTSV)
            let copiedTable = try XCTUnwrap(((fragment["document"] as? [String: Any])?["content"] as? [[String: Any]])?.first)
            let copiedFirstCell = try XCTUnwrap(((copiedTable["content"] as? [[String: Any]])?.first?["content"] as? [[String: Any]])?.first)
            XCTAssertEqual((copiedFirstCell["attrs"] as? [String: Any])?["colspan"] as? Int, TableClipboard.mergedColspan)
            XCTAssertTrue(fixture.updates.updates.isEmpty, "copy must not publish an update")
            XCTAssertEqual(try fixture.documentObject(), before)
            XCTAssertEqual(fixture.adapter.baseDocumentRevision, revision)
            XCTAssertEqual(fixture.adapter.historyFlags()?.canUndo, false)
            let selection = try fixture.engineSelection()
            XCTAssertEqual(selection.0, fixture.positions[TableClipboard.mergedWideCell])
            XCTAssertEqual(selection.1, fixture.positions[TableClipboard.mergedSecondRowMiddleCell])
        }
    }

    func testCellSelectionCutClearsTheCellsInOneUndoableUpdate() throws {
        try withClipboardTable(TableClipboard.gridDocument, anchorIndex: TableClipboard.firstCell,
                               headIndex: TableClipboard.secondCell) { fixture in
            let root = fixture.view.textView
            let before = try fixture.documentObject()

            XCTAssertTrue(root.canPerformAction(#selector(UIResponderStandardEditActions.cut(_:)), withSender: nil))
            root.cut(nil)

            XCTAssertEqual(UIPasteboard.general.string, TableClipboard.firstRowTSV)
            XCTAssertEqual(try cellTexts(fixture), [["", ""], ["C", "D"]])
            XCTAssertEqual(try fixture.publishedSelectionType(), TableClipboard.cellSelectionType)
            try assertOneUndoableUpdate(fixture, restoring: before)
        }
    }

    func testCellSelectionPasteFillsTheGridWithoutReplacingTheSelection() throws {
        try withClipboardTable(TableClipboard.gridDocument, anchorIndex: TableClipboard.firstCell,
                               headIndex: TableClipboard.lastCell) { fixture in
            let before = try fixture.documentObject()
            UIPasteboard.general.string = TableClipboard.pastedGridTSV

            fixture.view.textView.paste(nil)

            XCTAssertEqual(try cellTexts(fixture), [["w", "x"], ["y", "z"]])
            let selection = try fixture.engineSelection()
            XCTAssertEqual(selection.0, fixture.positions[TableClipboard.firstCell])
            XCTAssertEqual(selection.1, fixture.positions[TableClipboard.lastCell])
            try assertOneUndoableUpdate(fixture, restoring: before)
        }
    }

    func testCellSelectionRichPastePrefersHTMLAndPlainPasteUsesText() throws {
        try withClipboardTable(TableClipboard.gridDocument, anchorIndex: TableClipboard.firstCell,
                               headIndex: TableClipboard.secondCell) { fixture in
            let before = try fixture.documentObject()
            UIPasteboard.general.items = [[
                TableClipboard.htmlType: Data(TableClipboard.htmlTable.utf8),
                TableClipboard.plainTextType: TableClipboard.plainAlternativeTSV
            ]]

            fixture.view.textView.paste(nil)
            XCTAssertEqual(try cellTexts(fixture), [["h1", "h2"], ["C", "D"]])
            try assertOneUndoableUpdate(fixture, restoring: before)

            try select(fixture, anchor: fixture.positions[TableClipboard.firstCell],
                       head: fixture.positions[TableClipboard.secondCell])
            fixture.view.textView.pasteAndMatchStyle(nil)
            XCTAssertEqual(try cellTexts(fixture), [["p1", "p2"], ["C", "D"]])
            try assertOneUndoableUpdate(fixture, restoring: before)
        }
    }

    func testNestedReadOnlyCellsCopyButNeverReachThePlannerForCutOrPaste() throws {
        try withClipboardTable(TableClipboard.nestedDocument) { fixture in
            let nested = try XCTUnwrap(fixture.adapter.cachedTableRecords.values.first {
                $0["readOnlyDescendants"] as? Bool == true
            })
            let cell = try XCTUnwrap((nested["cells"] as? [[String: Any]])?.first)
            let opening = try XCTUnwrap(EditorV2Adapter.uint32Field(cell, "sourcePos"))
            try select(fixture, anchor: opening, head: opening)
            let root = fixture.view.textView
            let before = try fixture.documentObject()
            let revision = fixture.adapter.baseDocumentRevision
            let notesBefore = fixture.adapter.debugNotes.count

            root.copy(nil)
            XCTAssertEqual(UIPasteboard.general.string, TableClipboard.nestedCellText)
            UIPasteboard.general.string = TableClipboard.pastedGridTSV
            XCTAssertFalse(root.canPerformAction(#selector(UIResponderStandardEditActions.cut(_:)), withSender: nil))
            XCTAssertFalse(root.canPerformAction(#selector(UIResponderStandardEditActions.paste(_:)), withSender: nil))
            root.cut(nil)
            XCTAssertEqual(UIPasteboard.general.string, TableClipboard.pastedGridTSV, "a refused cut must keep the clipboard")
            root.paste(nil)

            XCTAssertTrue(fixture.updates.updates.isEmpty, "a refused edit must not publish an update")
            XCTAssertEqual(fixture.adapter.debugNotes.dropFirst(notesBefore).filter {
                $0.hasPrefix(TableClipboard.emittedErrorNote)
            }, [], "a refused edit must not reach the engine or emit an error")
            XCTAssertEqual(fixture.adapter.baseDocumentRevision, revision)
            XCTAssertEqual(try fixture.documentObject(), before)
            XCTAssertEqual(fixture.adapter.historyFlags()?.canUndo, false)
        }
    }

    func testAViewThatDoesNotOwnTheTableCannotCutOrPasteItsCellSelection() throws {
        try withClipboardTable(TableClipboard.gridDocument, anchorIndex: TableClipboard.firstCell,
                               headIndex: TableClipboard.lastCell) { fixture in
            let stale = RichTextEditorView(frame: CGRect(origin: .zero, size: TableClipboard.editorSize))
            fixture.view.window?.addSubview(stale)
            defer { stale.removeFromSuperview() }
            stale.bindEditor(id: fixture.view.editorId,
                             initialUpdateJSON: try XCTUnwrap(fixture.adapter.initialUpdateJSON()))
            XCTAssertTrue(stale.textView.applyUpdateJSON(
                try XCTUnwrap(fixture.adapter.refreshFromRustState(mirrorSelection: nil))
            ))
            XCTAssertTrue(fixture.view.textView.ownsNativeBinding(fixture.adapter))
            XCTAssertFalse(stale.textView.ownsNativeBinding(fixture.adapter))
            XCTAssertTrue(stale.textView.authoritativeCellSelectionActive, "the stale view adopted the cell selection")
            let staleUpdates = UpdateSpy()
            stale.textView.editorDelegate = staleUpdates
            let before = try fixture.documentObject()
            let revision = fixture.adapter.baseDocumentRevision
            UIPasteboard.general.string = TableClipboard.pastedGridTSV

            XCTAssertFalse(stale.textView.canPerformAction(#selector(UIResponderStandardEditActions.cut(_:)), withSender: nil))
            XCTAssertFalse(stale.textView.canPerformAction(#selector(UIResponderStandardEditActions.paste(_:)), withSender: nil))
            stale.textView.cut(nil)
            stale.textView.paste(nil)

            XCTAssertTrue(staleUpdates.updates.isEmpty)
            XCTAssertEqual(UIPasteboard.general.string, TableClipboard.pastedGridTSV)
            XCTAssertEqual(fixture.adapter.baseDocumentRevision, revision)
            XCTAssertEqual(try fixture.documentObject(), before)
            XCTAssertEqual(fixture.adapter.historyFlags()?.canUndo, false)

            fixture.view.textView.paste(nil)
            XCTAssertEqual(try cellTexts(fixture), [["w", "x"], ["y", "z"]])
        }
    }

    func testAtomCellsRoundTripThroughTheCellClipboard() throws {
        try withClipboardTable(TableClipboard.atomDocument, anchorIndex: TableClipboard.firstCell,
                               headIndex: TableClipboard.secondCell) { fixture in
            fixture.view.textView.copy(nil)
            let fragment = try XCTUnwrap(
                UIPasteboard.general.data(forPasteboardType: EditorClipboardPayload.fragmentType)
            )
            XCTAssertTrue(try XCTUnwrap(String(data: fragment, encoding: .utf8)).contains(TableClipboard.atomMetadataKind))

            try select(fixture, anchor: fixture.positions[TableClipboard.thirdCell],
                       head: fixture.positions[TableClipboard.lastCell])
            let before = try fixture.documentObject()
            fixture.view.textView.paste(nil)

            let rows = try tableRows(fixture)
            XCTAssertEqual(rows[1]["content"] as? NSArray, rows[0]["content"] as? NSArray,
                           "the pasted row must reproduce the copied atom payload")
            try assertOneUndoableUpdate(fixture, restoring: before)
        }
    }
}
