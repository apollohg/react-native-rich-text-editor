import UIKit
import XCTest

extension EditorTableInputTests {
    private enum TableDeletion {
        static let emptyFrameDocument = #"{"type":"doc","content":[{"type":"paragraph","content":[{"type":"text","text":"before"}]},{"type":"table"},{"type":"paragraph","content":[{"type":"text","text":"after"}]}]}"#
        static let nestedDocument = #"{"type":"doc","content":[{"type":"paragraph","content":[{"type":"text","text":"before"}]},{"type":"table","content":[{"type":"table_row","content":[{"type":"table_cell","content":[{"type":"paragraph","content":[{"type":"text","text":"Alpha"}]}]},{"type":"table_cell","content":[{"type":"table","content":[{"type":"table_row","content":[{"type":"table_cell","content":[{"type":"paragraph","content":[{"type":"text","text":"Nested"}]}]}]}]}]}]}]},{"type":"paragraph","content":[{"type":"text","text":"after"}]}]}"#
        static let remainingProse: NSArray = [
            ["type": "paragraph", "content": [["type": "text", "text": "before"]]],
            ["type": "paragraph", "content": [["type": "text", "text": "after"]]]
        ]
        static let remainingText = "before\nafter"
        // The shared table schema has no paragraph DOM specification.
        static let remainingHTML = "beforeafter"
        static let leadingProse = "before"
        static let peerID = "47"
    }

    func testPublicProseSkipsEmptyTablePreparationAndUndoRestoresCurrentGeometry() throws {
        let fixtures = try EditorPublicThemeFixtures.load()
        let theme = try XCTUnwrap(EditorTheme.from(json: fixtures.publicThemeJSON))
        XCTAssertNotNil(theme.styleSheet)
        var document = try XCTUnwrap(JSONSerialization.jsonObject(
            with: Data(TableDeletion.emptyFrameDocument.utf8)) as? [String: Any])
        var children = try XCTUnwrap(document["content"] as? [[String: Any]])
        let tableDocument = try XCTUnwrap(JSONSerialization.jsonObject(
            with: Data(TableInputTestSchema.twoCellDocument.utf8)) as? [String: Any])
        let table = try XCTUnwrap((tableDocument["content"] as? [[String: Any]])?.first)
        let tableIndex = try XCTUnwrap(children.firstIndex { $0["type"] as? String == "table" })
        children[tableIndex] = table
        document["content"] = children
        let source = try XCTUnwrap(String(data: JSONSerialization.data(withJSONObject: document), encoding: .utf8))
        var preparationSpans: [(UInt64, UInt64)] = []
        let previousObserver = PreparedProseInstrumentation.tableStageObserverForTesting
        PreparedProseInstrumentation.tableStageObserverForTesting = { stage, start, end in
            if stage == .tablePreparationAndGeometry { preparationSpans.append((start, end)) }
            previousObserver?(stage, start, end)
        }
        defer { PreparedProseInstrumentation.tableStageObserverForTesting = previousObserver }

        try withMountedTable(document: source, theme: theme, cellSelection: (0, 1)) { fixture in
            let input = fixture.view.textView
            defer {
                fixture.view.setRemoteSelections([])
                _ = input.resignFirstResponder()
            }
            let before = try fixture.documentObject()
            let initialCell = try fixture.presentedCell(0)
            XCTAssertGreaterThan(initialCell.bounds.width, 0)
            XCTAssertGreaterThan(initialCell.bounds.height, 0)
            XCTAssertGreaterThan(preparationSpans.count, 0,
                "The observer must see genuine preparation of the mounted two-cell table")
            XCTAssertTrue(preparationSpans.allSatisfy { $0.1 > $0.0 })
            fixture.view.setRemoteSelections([RemoteSelectionDecoration(
                clientId: TableDeletion.peerID, anchor: fixture.positions[0], head: fixture.positions[1],
                color: .systemBlue, name: nil, isFocused: true,
                cellRectangle: RemoteCellRectangle(anchorCell: fixture.positions[0], headCell: fixture.positions[1]),
                resolvedAt: try XCTUnwrap(RemoteSelectionFrame.installed(fixture.adapter)))])
            XCTAssertEqual(fixture.drawing.remoteTableCellSelections.count, 1,
                "The cleanup control must start with an actual remote cell rectangle")
            XCTAssertNotNil(fixture.drawing.selectedTableCellEndpoints)
            var acceptedMutations = 0
            let admission = try XCTUnwrap(fixture.adapter.tableMutationAdmission(tableID: fixture.tableID))
            var countBefore = preparationSpans.count
            XCTAssertTrue(input.applyUpdateJSON(try XCTUnwrap(fixture.adapter.deleteTable(admission: admission))))
            acceptedMutations += 1
            fixture.view.layoutIfNeeded()
            try assertNoTablePresentation(fixture, text: TableDeletion.remainingText, context: "delete")
            XCTAssertEqual(try blocks(fixture), TableDeletion.remainingProse)
            XCTAssertEqual(preparationSpans.count, countBefore,
                "Deleting the last table must not resolve a table theme or prepare empty entries")

            XCTAssertTrue(input.becomeFirstResponder())
            let caret = TableDeletion.leadingProse.utf16.count
            input.selectedRange = NSRange(location: caret, length: 0)
            input.syncSelectionImmediately()
            let inserted = "!"
            countBefore = preparationSpans.count
            let updatesBefore = fixture.updates.updates.count
            input.insertText(inserted)
            acceptedMutations += 1
            XCTAssertEqual(EditorV2Shadow.getHtml(id: fixture.view.editorId),
                TableDeletion.remainingHTML.replacingOccurrences(of: TableDeletion.leadingProse,
                    with: TableDeletion.leadingProse + inserted))
            XCTAssertEqual(input.selectedRange, NSRange(location: caret + inserted.utf16.count, length: 0))
            XCTAssertGreaterThan(fixture.updates.updates.count, updatesBefore,
                "Real public input must still publish its accepted commit")
            try assertNoTablePresentation(fixture,
                text: TableDeletion.remainingText.replacingOccurrences(of: TableDeletion.leadingProse,
                    with: TableDeletion.leadingProse + inserted), context: "insert")
            XCTAssertEqual(preparationSpans.count, countBefore, "Prose insertion must not prepare an empty table surface")
            countBefore = preparationSpans.count
            input.deleteBackward()
            acceptedMutations += 1
            XCTAssertEqual(EditorV2Shadow.getHtml(id: fixture.view.editorId), TableDeletion.remainingHTML)
            XCTAssertEqual(input.selectedRange, NSRange(location: caret, length: 0))
            try assertNoTablePresentation(fixture, text: TableDeletion.remainingText, context: "backspace")
            XCTAssertEqual(preparationSpans.count, countBefore, "Prose deletion must not prepare an empty table surface")

            let widthReduction: CGFloat = 80
            countBefore = preparationSpans.count
            fixture.view.frame.size.width -= widthReduction
            fixture.view.setNeedsLayout()
            fixture.view.layoutIfNeeded()
            let resizedSize = fixture.view.bounds.size
            try assertNoTablePresentation(fixture, text: TableDeletion.remainingText, context: "resize")
            XCTAssertEqual(preparationSpans.count, countBefore,
                "An empty surface must update its width bookkeeping without preparing table entries")

            var undoCount = 0
            countBefore = preparationSpans.count
            while fixture.adapter.tableRecordsForTesting.isEmpty, undoCount < acceptedMutations {
                XCTAssertTrue(input.applyUpdateJSON(try XCTUnwrap(fixture.adapter.undo())))
                fixture.view.layoutIfNeeded()
                undoCount += 1
            }
            XCTAssertFalse(fixture.adapter.tableRecordsForTesting.isEmpty,
                "Real history undo must restore the deleted table within the accepted mutation count")
            XCTAssertEqual(try fixture.documentObject(), before, "Undo must restore the actual original document")
            XCTAssertGreaterThan(preparationSpans.count, countBefore,
                "Restoring a real table must resume genuine preparation")
            let restoredTableID = try fixture.adapter.editableTableID()
            let positions = try fixture.adapter.tableCellPositions(tableID: restoredTableID)
            XCTAssertEqual(positions.count, fixture.positions.count)
            let restoredFixture = MountedTableFixture(view: fixture.view, adapter: fixture.adapter,
                tableID: restoredTableID, positions: positions, surface: fixture.surface,
                drawing: fixture.drawing, updates: fixture.updates)
            try input.selectTableCells(adapter: fixture.adapter, anchor: positions[0], head: positions[1])
            fixture.view.layoutIfNeeded()
            XCTAssertEqual(fixture.drawing.selectedTableCellEndpoints,
                TableSelectionEndpoints(tableID: restoredTableID, anchor: positions[0], head: positions[1]))
            XCTAssertEqual(fixture.drawing.selectedTableCellSourceIndices[restoredTableID], [0, 1])
            XCTAssertEqual(fixture.surface.presentedDocumentRevision, fixture.adapter.baseDocumentRevision)
            XCTAssertTrue(fixture.drawing.remoteTableCellSelections.isEmpty,
                "Restoring a table must not revive a remote rectangle from its obsolete frame")
            XCTAssertNotEqual(try restoredFixture.presentedCell(0).bounds.width, initialCell.bounds.width,
                "Restored cells must use the new viewport width")
            try withMountedTable(document: source, theme: theme, size: resizedSize, cellSelection: (0, 1)) { fresh in
                fresh.view.textView.setContentOffset(input.contentOffset, animated: false)
                fresh.view.layoutIfNeeded()
                for index in positions.indices {
                    let restored = try restoredFixture.presentedCell(index)
                    let expected = try fresh.presentedCell(index)
                    XCTAssertEqual(restored.bounds, expected.bounds, "Cell \(index) must match fresh current-width geometry")
                    XCTAssertEqual(restored.cell.contentSize, expected.cell.contentSize, "Cell \(index) must retain its prepared content size")
                    XCTAssertEqual(fixture.surface.cellFrame(tableID: restoredTableID, cellIndex: UInt32(index)),
                        fresh.surface.cellFrame(tableID: fresh.tableID, cellIndex: UInt32(index)))
                }
            }
        }
    }

    private func assertNoTablePresentation(_ fixture: MountedTableFixture, text: String, context: String) throws {
        let input = fixture.view.textView
        XCTAssertEqual(input.textStorage.string, text, context)
        XCTAssertTrue(fixture.adapter.tableRecordsForTesting.isEmpty, context)
        XCTAssertTrue(fixture.adapter.tableIndex.tableKeys.isEmpty, context)
        XCTAssertEqual(fixture.surface.presentedDocumentRevision, fixture.adapter.baseDocumentRevision, context)
        XCTAssertNil(fixture.drawing.layout, context)
        XCTAssertNil(fixture.drawing.mountedTablePresentation(), context)
        XCTAssertNil(fixture.surface.cellFrame(tableID: fixture.tableID, cellIndex: 0), context)
        XCTAssertNil(fixture.drawing.excludedTableCellContentLayout, context)
        XCTAssertTrue(fixture.view.activeTextInput === input, context)
        XCTAssertTrue(fixture.drawing.selectedTableCellSourceIndices.isEmpty, context)
        XCTAssertNil(fixture.drawing.selectedTableCellEndpoints, context)
        XCTAssertTrue(fixture.drawing.selectionHandles().isEmpty, context)
        XCTAssertTrue(fixture.drawing.remoteTableCellSelections.isEmpty, context)
        XCTAssertTrue(fixture.view.remoteSelectionOverlaySubviewsForTesting().isEmpty, context)
        let fullRange = NSRange(location: 0, length: input.textStorage.length)
        input.textStorage.enumerateAttribute(RenderBridgeAttributes.rootTableMarker, in: fullRange) { value, _, _ in
            XCTAssertNil(value, "\(context): no stale table placeholder may remain mounted")
        }
        input.textStorage.enumerateAttribute(.paragraphStyle, in: fullRange) { value, _, _ in
            guard let style = value as? NSParagraphStyle else { return XCTFail("\(context): prose paragraph style missing") }
            XCTAssertEqual(style.minimumLineHeight, 0, "\(context): prose must not keep table height reservations")
            XCTAssertEqual(style.maximumLineHeight, 0, "\(context): prose must not keep table height reservations")
        }
    }

    private func blocks(_ fixture: MountedTableFixture) throws -> NSArray {
        try XCTUnwrap(try fixture.documentObject()["content"] as? NSArray)
    }

    func testDeleteTableRemovesAnEmptyFrameByItsPositionInOneUndoableMutation() throws {
        try withMountedTable(document: TableDeletion.emptyFrameDocument, cellSelection: nil) { fixture in
            let record = try XCTUnwrap(fixture.adapter.tableRecordsForTesting[fixture.tableID])
            XCTAssertEqual(record["rows"] as? Int, 0, "the fixture must be an empty frame: \(record)")
            XCTAssertTrue(fixture.positions.isEmpty, "an empty frame has no cell to anchor a delete")
            let before = try fixture.documentObject()
            let admission = try XCTUnwrap(fixture.adapter.tableMutationAdmission(tableID: fixture.tableID))

            let update = try XCTUnwrap(
                fixture.adapter.deleteTable(admission: admission),
                "an admitted explicit delete must reach the engine"
            )
            XCTAssertTrue(fixture.view.textView.applyUpdateJSON(update))

            XCTAssertEqual(try blocks(fixture), TableDeletion.remainingProse)
            XCTAssertTrue(fixture.adapter.tableRecordsForTesting.isEmpty, "the frame's record is gone")
            XCTAssertEqual(fixture.adapter.historyFlags()?.canUndo, true)
            XCTAssertTrue(fixture.view.textView.applyUpdateJSON(try XCTUnwrap(fixture.adapter.undo())))
            XCTAssertEqual(try fixture.documentObject(), before, "one undo restores the frame")
            XCTAssertEqual(fixture.adapter.historyFlags()?.canUndo, false, "the delete was one history entry")
        }
    }

    func testDeleteTableRefusesStaleOwnerStaleRevisionAndNestedAdmissions() throws {
        try withMountedTable(document: TableDeletion.nestedDocument, cellSelection: nil) { fixture in
            let before = try fixture.documentObject()
            let nestedID = try XCTUnwrap(fixture.adapter.tableRecordsForTesting.first {
                $0.value["readOnlyDescendants"] as? Bool == true
            }?.key)
            let nested = try XCTUnwrap(fixture.adapter.tableMutationAdmission(tableID: nestedID))
            XCTAssertNil(fixture.adapter.deleteTable(admission: nested), "a nested table is read-only")
            XCTAssertEqual(try fixture.documentObject(), before)

            let stale = try XCTUnwrap(fixture.adapter.tableMutationAdmission(tableID: fixture.tableID))
            XCTAssertTrue(fixture.view.textView.applyUpdateJSON(
                try XCTUnwrap(fixture.adapter.setContentJson(TableDeletion.nestedDocument))
            ))
            XCTAssertNil(fixture.adapter.deleteTable(admission: stale), "a stale revision must not delete")
            XCTAssertEqual(try fixture.documentObject(), before)

            let owned = try XCTUnwrap(fixture.adapter.tableMutationAdmission(tableID: fixture.tableID))
            XCTAssertTrue(fixture.adapter.admitsTableMutation(owned))
            fixture.adapter.releaseNativeBindingOwner(token: try XCTUnwrap(fixture.adapter.nativeOwnerToken))
            XCTAssertNil(fixture.adapter.deleteTable(admission: owned), "a lost owner must not delete")
            XCTAssertEqual(try fixture.documentObject(), before)
            XCTAssertEqual(fixture.adapter.historyFlags()?.canUndo, false)
        }
    }
}
