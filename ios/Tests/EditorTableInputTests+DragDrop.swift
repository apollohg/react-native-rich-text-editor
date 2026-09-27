import XCTest

@MainActor
extension EditorTableInputTests {
    private enum CellDrag {
        static let config = #"{"schema":{"nodes":[{"name":"doc","content":"block+","role":"doc"},{"name":"paragraph","content":"inline*","group":"block","role":"textBlock","htmlTag":"p"},{"name":"text","content":"","group":"inline","role":"text"},{"name":"table","content":"table_row+","group":"block","role":"block","tableRole":"table","htmlTag":"table"},{"name":"table_row","content":"(table_cell | table_header)*","role":"block","tableRole":"row","htmlTag":"tr"},{"name":"table_cell","content":"block+","role":"block","tableRole":"cell","htmlTag":"td","attrs":{"colspan":{"type":"number","default":1,"min":1},"rowspan":{"type":"number","default":1,"min":1},"colwidth":{"default":null}}},{"name":"table_header","content":"block+","role":"block","tableRole":"header_cell","htmlTag":"th","attrs":{"colspan":{"type":"number","default":1,"min":1},"rowspan":{"type":"number","default":1,"min":1},"colwidth":{"default":null}}}],"marks":[]},"initialization":{"type":"localEmpty"}}"#
        static let gridDocument = #"{"type":"doc","content":[{"type":"table","content":[{"type":"table_row","content":[{"type":"table_cell","content":[{"type":"paragraph","content":[{"type":"text","text":"A"}]}]},{"type":"table_cell","content":[{"type":"paragraph","content":[{"type":"text","text":"B"}]}]}]},{"type":"table_row","content":[{"type":"table_cell","content":[{"type":"paragraph","content":[{"type":"text","text":"C"}]}]},{"type":"table_cell","content":[{"type":"paragraph","content":[{"type":"text","text":"D"}]}]}]}]},{"type":"paragraph","content":[{"type":"text","text":"after"}]}]}"#
        static let targetDocument = #"{"type":"doc","content":[{"type":"table","content":[{"type":"table_row","content":[{"type":"table_cell","content":[{"type":"paragraph","content":[{"type":"text","text":"w"}]}]},{"type":"table_cell","content":[{"type":"paragraph","content":[{"type":"text","text":"x"}]}]}]},{"type":"table_row","content":[{"type":"table_cell","content":[{"type":"paragraph","content":[{"type":"text","text":"y"}]}]},{"type":"table_cell","content":[{"type":"paragraph","content":[{"type":"text","text":"z"}]}]}]}]},{"type":"paragraph","content":[{"type":"text","text":"after"}]}]}"#
        static let irregularDocument = #"{"type":"doc","content":[{"type":"table","content":[{"type":"table_row","content":[{"type":"table_cell","attrs":{"rowspan":2,"colwidth":[100]},"content":[{"type":"paragraph","content":[{"type":"text","text":"tall"}]}]},{"type":"table_cell","attrs":{"colspan":2,"colwidth":[100,100]},"content":[{"type":"paragraph","content":[{"type":"text","text":"wide"}]}]}]},{"type":"table_row","content":[{"type":"table_cell","attrs":{"colwidth":[100]},"content":[{"type":"paragraph","content":[{"type":"text","text":"later"}]}]}]}]}]}"#
        static let irregularWideCell = 1
        static let irregularLaterCell = 2
        static let cellTextOffset: UInt32 = 2
        static let staleEdit = "!"
        static let editorSize = CGSize(width: 480, height: 320)
        static let firstCell = 0
        static let secondCell = 1
        static let thirdCell = 2
        static let lastCell = 3
        static let firstRowTSV = "A\tB"
        static let externalTSV = "e1\te2"
        static let loadTimeout: TimeInterval = 5
        static let lateUpdateWindow: TimeInterval = 1
    }

    private struct StartedCellDrag {
        let session: TestTextDragSession
        let items: [UIDragItem]
        let interaction: UIDragInteraction
    }

    private func withCellDragTable(
        _ document: String = CellDrag.gridDocument,
        anchor: Int = CellDrag.firstCell, head: Int = CellDrag.secondCell,
        _ body: (MountedTableFixture) throws -> Void
    ) throws {
        try withMountedTable(document: document, configJSON: CellDrag.config, size: CellDrag.editorSize,
                             cellSelection: (anchor, head)) { fixture in
            XCTAssertTrue(fixture.view.textView.becomeFirstResponder())
            XCTAssertTrue(fixture.view.textView.authoritativeCellSelectionActive, "root did not adopt the cell selection")
            XCTAssertEqual(fixture.adapter.historyFlags()?.canUndo, false, "fixture must start without history")
            fixture.updates.updates.removeAll()
            try body(fixture)
        }
    }

    private func windowPoint(_ fixture: MountedTableFixture, inCell index: Int) throws -> CGPoint {
        fixture.view.convert(try fixture.hostPoint(inCell: index), to: nil)
    }

    private func cellDragInteraction(_ fixture: MountedTableFixture) throws -> UIDragInteraction {
        try XCTUnwrap(fixture.view.textView.interactions.compactMap { $0 as? UIDragInteraction }
            .first { $0.delegate === fixture.surface }, "the table surface installs no drag interaction on the root")
    }

    private func startCellDrag(_ fixture: MountedTableFixture, at windowPoint: CGPoint) throws -> StartedCellDrag {
        let interaction = try cellDragInteraction(fixture)
        let session = TestTextDragSession(items: [], windowLocation: windowPoint)
        let items = fixture.surface.dragInteraction(interaction, itemsForBeginning: session)
        return StartedCellDrag(session: session, items: items, interaction: interaction)
    }

    private func dropRequest(_ fixture: MountedTableFixture, session: UIDropSession) -> TestTextDropRequest {
        TestTextDropRequest(dropPosition: fixture.view.textView.beginningOfDocument, isSameView: true,
                            dropSession: session)
    }

    private func proposal(_ fixture: MountedTableFixture, for request: TestTextDropRequest) -> UITextDropProposal {
        fixture.view.textView.textDroppableView(fixture.view.textView, proposalForDrop: request)
    }

    private func perform(_ fixture: MountedTableFixture, _ request: TestTextDropRequest) {
        fixture.view.textView.textDroppableView(fixture.view.textView, willPerformDrop: request)
    }

    private func dragCellTexts(_ fixture: MountedTableFixture) throws -> [[String]] {
        func text(_ node: [String: Any]) -> String {
            if node["type"] as? String == "text" { return node["text"] as? String ?? "" }
            return (node["content"] as? [[String: Any]] ?? []).map(text).joined()
        }
        let root = try XCTUnwrap(fixture.documentObject() as? [String: Any])
        let content = try XCTUnwrap(root["content"] as? [[String: Any]])
        let table = try XCTUnwrap(content.first { $0["type"] as? String == "table" })
        let rows = try XCTUnwrap(table["content"] as? [[String: Any]])
        return rows.map { row in (row["content"] as? [[String: Any]] ?? []).map(text) }
    }

    private func loadedData(_ provider: NSItemProvider, type: String) throws -> Data {
        let loaded = expectation(description: "load \(type)")
        var result: Data?
        _ = provider.loadDataRepresentation(forTypeIdentifier: type) { data, error in
            XCTAssertNil(error, "\(type) failed to load: \(String(describing: error))")
            result = data
            loaded.fulfill()
        }
        wait(for: [loaded], timeout: CellDrag.loadTimeout)
        return try XCTUnwrap(result, "\(type) loaded no data")
    }

    private func dropTarget(_ fixture: MountedTableFixture, cell index: Int) -> TableCellDropTarget {
        TableCellDropTarget(tableID: fixture.tableID, sourcePosition: Int(fixture.positions[index]))
    }

    private func assertSingleUndoRestores(_ fixture: MountedTableFixture, _ before: NSDictionary,
                                          file: StaticString = #filePath, line: UInt = #line) throws {
        XCTAssertEqual(fixture.updates.updates.count, 1, "the drop publishes exactly one update", file: file, line: line)
        XCTAssertTrue(fixture.view.textView.applyUpdateJSON(try XCTUnwrap(fixture.adapter.undo())), file: file, line: line)
        XCTAssertEqual(try fixture.documentObject(), before, "one undo restores source and target", file: file, line: line)
        XCTAssertEqual(fixture.adapter.historyFlags()?.canUndo, false,
                       "the drop must be a single history entry", file: file, line: line)
    }

    func testCellDragExportsTheCopyFlavoursAndLiftsTheSelectedCellUnion() throws {
        try withCellDragTable { fixture in
            let copied = UIPasteboard.withUniqueName()
            defer { UIPasteboard.remove(withName: copied.name) }
            XCTAssertTrue(fixture.view.textView.exportSelectionToPasteboard(copied))

            let drag = try startCellDrag(fixture, at: try windowPoint(fixture, inCell: CellDrag.firstCell))

            let item = try XCTUnwrap(drag.items.first, "a long press inside the selection lifts the cells")
            XCTAssertEqual(drag.items.count, 1)
            XCTAssertEqual(item.itemProvider.registeredTypeIdentifiers, EditorClipboardPayload.exportedTypes)
            XCTAssertEqual(try loadedData(item.itemProvider, type: EditorClipboardPayload.fragmentType),
                           copied.data(forPasteboardType: EditorClipboardPayload.fragmentType))
            XCTAssertEqual(try loadedData(item.itemProvider, type: EditorClipboardPayload.htmlType),
                           copied.data(forPasteboardType: EditorClipboardPayload.htmlType))
            XCTAssertEqual(String(data: try loadedData(item.itemProvider, type: EditorClipboardPayload.plainTextType),
                                  encoding: .utf8), CellDrag.firstRowTSV)
            XCTAssertEqual(copied.string, CellDrag.firstRowTSV)
            let context = try XCTUnwrap(drag.session.localContext as? TableCellDragContext)
            XCTAssertTrue(context.movable)
            XCTAssertEqual(context.source.anchor, fixture.positions[CellDrag.firstCell])
            XCTAssertEqual(context.source.head, fixture.positions[CellDrag.secondCell])
            XCTAssertTrue(fixture.updates.updates.isEmpty, "lifting the cells must not publish an update")

            let preview = try XCTUnwrap(fixture.surface.dragInteraction(
                drag.interaction, previewForLifting: item, session: drag.session
            ))
            let selected = try XCTUnwrap(fixture.drawing.selectedTableCellRects(tableID: fixture.tableID))
            let union = selected.dropFirst().reduce(selected[0]) { $0.union($1) }
            XCTAssertTrue(preview.view === fixture.drawing)
            XCTAssertEqual(preview.parameters.visiblePath?.bounds, union,
                           "the lift preview is exactly the selected cells")
        }
    }

    func testCellDragDoesNotBeginOnAHandleOrOutsideTheSelection() throws {
        try withCellDragTable { fixture in
            let handle = fixture.view.convert(try fixture.hostPoint(for: .head), to: nil)
            XCTAssertTrue(try startCellDrag(fixture, at: handle).items.isEmpty,
                          "the selection handle keeps precedence over the drag")
            XCTAssertTrue(try startCellDrag(fixture, at: try windowPoint(fixture, inCell: CellDrag.thirdCell))
                .items.isEmpty, "a long press outside the selection lifts nothing")
        }
    }

    func testSameEditorDropMovesTheCellsInOneUndoableUpdate() throws {
        try withCellDragTable { fixture in
            let before = try fixture.documentObject()
            let drag = try startCellDrag(fixture, at: try windowPoint(fixture, inCell: CellDrag.firstCell))
            let request = dropRequest(fixture, session: TestTextDropSession(
                dragSession: drag.session, windowLocation: try windowPoint(fixture, inCell: CellDrag.thirdCell)
            ))

            let offered = proposal(fixture, for: request)
            XCTAssertEqual(offered.operation, .move)
            XCTAssertEqual(offered.dropPerformer, .delegate)
            XCTAssertEqual(fixture.drawing.tableCellDropTarget, dropTarget(fixture, cell: CellDrag.thirdCell),
                           "hovering highlights the real drop cell")
            perform(fixture, request)

            XCTAssertEqual(try dragCellTexts(fixture), [["", ""], ["A", "B"]])
            XCTAssertNil(fixture.drawing.tableCellDropTarget, "the highlight ends with the drop")
            try assertSingleUndoRestores(fixture, before)
        }
    }

    func testDroppingTheCellsOntoThemselvesIsANoOp() throws {
        try withCellDragTable { fixture in
            let before = try fixture.documentObject()
            let drag = try startCellDrag(fixture, at: try windowPoint(fixture, inCell: CellDrag.firstCell))
            let request = dropRequest(fixture, session: TestTextDropSession(
                dragSession: drag.session, windowLocation: try windowPoint(fixture, inCell: CellDrag.secondCell)
            ))

            XCTAssertEqual(proposal(fixture, for: request).operation, .cancel)
            XCTAssertNil(fixture.drawing.tableCellDropTarget)
            perform(fixture, request)

            XCTAssertTrue(fixture.updates.updates.isEmpty)
            XCTAssertEqual(try fixture.documentObject(), before)
            XCTAssertEqual(fixture.adapter.historyFlags()?.canUndo, false)
        }
    }

    func testCrossEditorDropCopiesAndLeavesTheSourceIntact() throws {
        try withCellDragTable { source in
            let sourceBefore = try source.documentObject()
            let drag = try startCellDrag(source, at: try windowPoint(source, inCell: CellDrag.firstCell))
            XCTAssertFalse(drag.items.isEmpty)

            try withMountedTable(document: CellDrag.targetDocument, configJSON: CellDrag.config,
                                 size: CellDrag.editorSize, cellSelection: nil) { target in
                let request = dropRequest(target, session: TestTextDropSession(
                    dragSession: drag.session, windowLocation: try windowPoint(target, inCell: CellDrag.thirdCell)
                ))

                XCTAssertEqual(proposal(target, for: request).operation, .copy)
                XCTAssertEqual(target.drawing.tableCellDropTarget, dropTarget(target, cell: CellDrag.thirdCell))
                perform(target, request)

                XCTAssertEqual(try dragCellTexts(target), [["w", "x"], ["A", "B"]])
                XCTAssertEqual(target.updates.updates.count, 1)
            }
            XCTAssertEqual(try source.documentObject(), sourceBefore, "a copy never clears the source")
            XCTAssertTrue(source.updates.updates.isEmpty)
            XCTAssertEqual(source.adapter.historyFlags()?.canUndo, false)
        }
    }

    func testExternalDropLoadsItsItemsAndPastesTheMatrixAtTheDropCell() throws {
        try withMountedTable(document: CellDrag.targetDocument, configJSON: CellDrag.config,
                             size: CellDrag.editorSize, cellSelection: nil) { fixture in
            let before = try fixture.documentObject()
            let item = UIDragItem(itemProvider: NSItemProvider(object: CellDrag.externalTSV as NSString))
            let request = dropRequest(fixture, session: TestTextDropSession(
                externalItems: [item], windowLocation: try windowPoint(fixture, inCell: CellDrag.lastCell)
            ))

            XCTAssertEqual(proposal(fixture, for: request).operation, .copy)
            let pasted = expectation(description: "the loaded items are pasted")
            pasted.assertForOverFulfill = false
            fixture.updates.onUpdate = { pasted.fulfill() }
            perform(fixture, request)
            wait(for: [pasted], timeout: CellDrag.loadTimeout)
            RunLoop.main.run(until: Date().addingTimeInterval(CellDrag.lateUpdateWindow))
            XCTAssertEqual(fixture.updates.updates.count, 1, "updates: \(fixture.updates.updates)")

            XCTAssertEqual(try dragCellTexts(fixture), [["w", "x", ""], ["y", "e1", "e2"]],
                           "the matrix grows the table from the real drop cell")
            try assertSingleUndoRestores(fixture, before)
        }
    }

    func testDropOutsideTheTableKeepsTheTextDropProposal() throws {
        try withCellDragTable { fixture in
            let drag = try startCellDrag(fixture, at: try windowPoint(fixture, inCell: CellDrag.firstCell))
            let textView = fixture.view.textView
            let caret = textView.caretRect(for: textView.endOfDocument)
            let request = dropRequest(fixture, session: TestTextDropSession(
                dragSession: drag.session,
                windowLocation: textView.convert(CGPoint(x: caret.midX, y: caret.midY), to: nil)
            ))

            XCTAssertEqual(proposal(fixture, for: request).operation, request.suggestedProposal.operation)
            XCTAssertNil(fixture.drawing.tableCellDropTarget)
        }
    }

    func testReadOnlyEditorDragsACopyAndRefusesDrops() throws {
        try withCellDragTable { fixture in
            fixture.view.textView.isEditable = false
            XCTAssertTrue(fixture.view.textView.becomeFirstResponder(), "a read-only root can hold focus")
            XCTAssertTrue(fixture.view.textView.authoritativeCellSelectionActive)
            XCTAssertNotNil(fixture.drawing.selectedTableCellEndpoints)
            let before = try fixture.documentObject()

            let drag = try startCellDrag(fixture, at: try windowPoint(fixture, inCell: CellDrag.firstCell))
            let context = try XCTUnwrap(drag.session.localContext as? TableCellDragContext,
                                        "a read-only editor still offers its cells for copying")
            XCTAssertFalse(context.movable)
            let request = dropRequest(fixture, session: TestTextDropSession(
                dragSession: drag.session, windowLocation: try windowPoint(fixture, inCell: CellDrag.thirdCell)
            ))

            XCTAssertEqual(proposal(fixture, for: request).operation, .forbidden)
            perform(fixture, request)
            XCTAssertEqual(try fixture.documentObject(), before)
            XCTAssertTrue(fixture.updates.updates.isEmpty)
        }
    }

    func testAViewThatDoesNotOwnTheTableCannotMoveOrReceiveCellDrops() throws {
        try withCellDragTable { fixture in
            let stale = RichTextEditorView(frame: CGRect(origin: .zero, size: CellDrag.editorSize))
            fixture.view.window?.addSubview(stale)
            defer { stale.removeFromSuperview() }
            stale.bindEditor(id: fixture.view.editorId,
                             initialUpdateJSON: try XCTUnwrap(fixture.adapter.initialUpdateJSON()))
            XCTAssertTrue(stale.textView.applyUpdateJSON(
                try XCTUnwrap(fixture.adapter.refreshFromRustState(mirrorSelection: nil))
            ))
            stale.layoutIfNeeded()
            XCTAssertTrue(stale.textView.becomeFirstResponder())
            XCTAssertFalse(stale.textView.ownsNativeBinding(fixture.adapter))
            let staleSurface = try XCTUnwrap(stale.subviews.compactMap { $0 as? EditorTableSurface }.first)
            let staleInteraction = try XCTUnwrap(stale.textView.interactions.compactMap { $0 as? UIDragInteraction }
                .first { $0.delegate === staleSurface })
            let before = try fixture.documentObject()
            let session = TestTextDragSession(items: [], windowLocation: try windowPoint(fixture, inCell: CellDrag.firstCell))

            XCTAssertFalse(staleSurface.dragInteraction(staleInteraction, itemsForBeginning: session).isEmpty)
            XCTAssertEqual((session.localContext as? TableCellDragContext)?.movable, false)
            let request = TestTextDropRequest(
                dropPosition: stale.textView.beginningOfDocument, isSameView: true,
                dropSession: TestTextDropSession(
                    dragSession: session, windowLocation: try windowPoint(fixture, inCell: CellDrag.thirdCell)
                )
            )
            XCTAssertEqual(stale.textView.textDroppableView(stale.textView, proposalForDrop: request).operation,
                           .forbidden)
            stale.textView.textDroppableView(stale.textView, willPerformDrop: request)
            XCTAssertEqual(try fixture.documentObject(), before)
            XCTAssertEqual(fixture.adapter.historyFlags()?.canUndo, false)
        }
    }

    func testAnActiveCompositionRefusesBothTheDragAndTheDrop() throws {
        try withCellDragTable { fixture in
            let movable = try startCellDrag(fixture, at: try windowPoint(fixture, inCell: CellDrag.firstCell))
            XCTAssertFalse(movable.items.isEmpty)
            let before = try fixture.documentObject()
            fixture.view.textView.isComposing = true
            defer { fixture.view.textView.isComposing = false }

            XCTAssertTrue(try startCellDrag(fixture, at: try windowPoint(fixture, inCell: CellDrag.firstCell))
                .items.isEmpty, "a composing editor lifts no cells")
            let request = dropRequest(fixture, session: TestTextDropSession(
                dragSession: movable.session, windowLocation: try windowPoint(fixture, inCell: CellDrag.thirdCell)
            ))
            XCTAssertEqual(proposal(fixture, for: request).operation, .forbidden)
            perform(fixture, request)
            XCTAssertEqual(try fixture.documentObject(), before)
            XCTAssertTrue(fixture.updates.updates.isEmpty)
        }
    }

    func testADropOntoASyntheticSlotIsRefusedWithoutMutation() throws {
        try withCellDragTable(CellDrag.irregularDocument, anchor: CellDrag.irregularLaterCell,
                              head: CellDrag.irregularLaterCell) { fixture in
            let wide = try fixture.presentedCell(CellDrag.irregularWideCell)
            let later = try fixture.presentedCell(CellDrag.irregularLaterCell)
            let gap = CGPoint(x: (later.bounds.maxX + wide.bounds.maxX) / 2, y: later.bounds.midY)
            XCTAssertFalse(wide.surface.syntheticRegions.isEmpty, "the fixture must project a synthetic gap")
            XCTAssertFalse(try XCTUnwrap(fixture.drawing.mountedTablePresentation()).cells.contains {
                $0.cell.sourceCellIndex != nil && $0.bounds.contains(gap)
            }, "the gap holds no real cell")
            let gapInWindow = fixture.drawing.convert(gap, to: nil)
            let drag = try startCellDrag(fixture, at: try windowPoint(fixture, inCell: CellDrag.irregularLaterCell))
            XCTAssertFalse(drag.items.isEmpty)
            let external = UIDragItem(itemProvider: NSItemProvider(object: CellDrag.externalTSV as NSString))
            let before = try XCTUnwrap(fixture.adapter.documentJson())

            for session in [TestTextDropSession(dragSession: drag.session, windowLocation: gapInWindow),
                            TestTextDropSession(externalItems: [external], windowLocation: gapInWindow)] {
                let request = dropRequest(fixture, session: session)
                XCTAssertEqual(proposal(fixture, for: request).operation, .forbidden,
                               "a synthetic slot never receives UIKit's text insertion")
                XCTAssertNil(fixture.drawing.tableCellDropTarget)
                perform(fixture, request)
            }
            RunLoop.main.run(until: Date().addingTimeInterval(CellDrag.lateUpdateWindow))

            XCTAssertTrue(fixture.updates.updates.isEmpty)
            XCTAssertEqual(try XCTUnwrap(fixture.adapter.documentJson()), before, "the document is byte-identical")
        }
    }

    func testAMoveIsRefusedWhenTheDocumentChangedAfterTheDragStarted() throws {
        try withCellDragTable { fixture in
            let drag = try startCellDrag(fixture, at: try windowPoint(fixture, inCell: CellDrag.firstCell))
            XCTAssertEqual((drag.session.localContext as? TableCellDragContext)?.movable, true)
            XCTAssertTrue(fixture.view.textView.applyUpdateJSON(EditorV2Shadow.insertText(
                id: fixture.view.editorId, pos: fixture.positions[CellDrag.lastCell] + CellDrag.cellTextOffset,
                text: CellDrag.staleEdit
            )))
            XCTAssertEqual(try dragCellTexts(fixture), [["A", "B"], ["C", CellDrag.staleEdit + "D"]])
            fixture.view.layoutIfNeeded()
            fixture.updates.updates.removeAll()
            let before = try fixture.documentObject()
            let request = dropRequest(fixture, session: TestTextDropSession(
                dragSession: drag.session, windowLocation: try windowPoint(fixture, inCell: CellDrag.thirdCell)
            ))

            XCTAssertEqual(proposal(fixture, for: request).operation, .forbidden)
            XCTAssertNil(fixture.drawing.tableCellDropTarget)
            perform(fixture, request)

            XCTAssertTrue(fixture.updates.updates.isEmpty)
            XCTAssertEqual(try fixture.documentObject(), before, "a stale move neither moves nor degrades to a copy")
        }
    }
}
