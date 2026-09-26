import XCTest

extension EditorTableInputTests {
    private enum TableAccessibilityFixture {
        static let fourCellDocument = #"{"type":"doc","content":[{"type":"table","content":[{"type":"table_row","content":[{"type":"table_cell","content":[{"type":"paragraph","content":[{"type":"text","text":"one"}]}]},{"type":"table_cell","content":[{"type":"paragraph","content":[{"type":"text","text":"two"}]}]}]},{"type":"table_row","content":[{"type":"table_cell","content":[{"type":"paragraph","content":[{"type":"text","text":"three"}]}]},{"type":"table_cell","content":[{"type":"paragraph","content":[{"type":"text","text":"four"}]}]}]}]}]}"#
        static let frameBesideTableDocument = #"{"type":"doc","content":[{"type":"paragraph","content":[{"type":"text","text":"before"}]},{"type":"table"},{"type":"table","content":[{"type":"table_row","content":[{"type":"table_cell","content":[{"type":"paragraph","content":[{"type":"text","text":"keep"}]}]}]}]},{"type":"paragraph","content":[{"type":"text","text":"after"}]}]}"#
    }

    private func accessibleTable(_ fixture: MountedTableFixture, containing label: String) throws
        -> TableAccessibilityTableElement {
        try XCTUnwrap((0..<fixture.drawing.accessibilityElementCount()).lazy.compactMap {
            fixture.drawing.accessibilityElement(at: $0) as? TableAccessibilityTableElement
        }.first { $0.cellElements.contains { $0.accessibilityLabel == label } },
        "no data table element contains \(label)")
    }

    private func accessibleFrame(_ fixture: MountedTableFixture) throws -> TableAccessibilityFrameElement {
        try XCTUnwrap(fixture.surface.accessibilityElements?.compactMap { $0 as? TableAccessibilityFrameElement }.first,
                      "an empty frame the editor cannot draw must still be an accessibility element")
    }

    private func publishedActionLabels(_ fixture: MountedTableFixture) throws -> [String] {
        let commands = try XCTUnwrap(fixture.adapter.cachedActiveState?["commands"] as? [String: Any])
        return TableAccessibilityAction.all.filter { commands[$0.applicability] as? Bool == true }.map(\.label)
    }

    private func perform(_ label: String, on element: NSObject) throws -> Bool {
        let action = try XCTUnwrap(element.accessibilityCustomActions?.first { $0.name == label },
                                   "missing \(label) in \(element.accessibilityCustomActions?.map(\.name) ?? [])")
        return action.actionHandler?(action) ?? false
    }

    private func rowCount(_ fixture: MountedTableFixture, tableIndex: Int = 0) throws -> Int {
        let tables = try XCTUnwrap(try fixture.documentObject()["content"] as? [[String: Any]])
            .filter { $0["type"] as? String == "table" }
        return (tables[tableIndex]["content"] as? [Any])?.count ?? 0
    }

    func testTableSurfaceExposesOnlyTheDrawingViewAsItsAccessibilityContainer() throws {
        try withMountedTable(document: TableAccessibilityFixture.fourCellDocument, cellSelection: nil) { fixture in
            XCTAssertEqual(fixture.surface.accessibilityElements?.count, 1)
            XCTAssertTrue(fixture.surface.accessibilityElements?.first as? PreparedProseDrawingView === fixture.drawing,
                          "the active input must only be reachable through its table cell slot")
            let table = try accessibleTable(fixture, containing: "one")
            XCTAssertEqual(table.cellElements.compactMap(\.accessibilityLabel), ["one", "two", "three", "four"])
            XCTAssertEqual(table.accessibilityRowCount(), 2)
            XCTAssertEqual(table.accessibilityColumnCount(), 2)
        }
    }

    func testSelectedCellsOfferExactlyThePublishedTableActions() throws {
        try withMountedTable(document: TableAccessibilityFixture.fourCellDocument, cellSelection: (0, 1)) { fixture in
            let expected = try publishedActionLabels(fixture)
            let merge = try XCTUnwrap(TableAccessibilityAction.all.first { $0.key == "mergeCells" })
            let split = try XCTUnwrap(TableAccessibilityAction.all.first { $0.key == "splitCell" })
            XCTAssertTrue(expected.contains(merge.label),
                          "a two-cell selection must publish merge: \(expected)")
            XCTAssertFalse(expected.contains(split.label))
            let cells = try accessibleTable(fixture, containing: "one").cellElements
            XCTAssertEqual(cells[0].accessibilityCustomActions?.map(\.name), expected)
            XCTAssertEqual(cells[1].accessibilityCustomActions?.map(\.name), expected)
            XCTAssertEqual(cells[2].accessibilityCustomActions?.count, 0, "unselected cells carry no table actions")
            XCTAssertEqual(cells[3].accessibilityCustomActions?.count, 0)
        }
    }

    func testAccessibilityRowActionPerformsExactlyOneCommand() throws {
        try withMountedTable(document: TableAccessibilityFixture.fourCellDocument, cellSelection: (0, 0)) { fixture in
            let before = try fixture.documentObject()
            let revision = fixture.adapter.baseDocumentRevision
            let insertBelow = try XCTUnwrap(TableAccessibilityAction.all.first { $0.key == "addRowAfter" })
            let cell = try accessibleTable(fixture, containing: "one").cellElements[0]

            XCTAssertTrue(try perform(insertBelow.label, on: cell))

            XCTAssertEqual(try rowCount(fixture), 3, "one row was inserted")
            XCTAssertEqual(fixture.updates.updates.count, 1, "exactly one update was applied")
            XCTAssertEqual(fixture.adapter.baseDocumentRevision, revision + 1, "exactly one engine mutation")
            XCTAssertTrue(fixture.view.textView.applyUpdateJSON(try XCTUnwrap(fixture.adapter.undo())))
            XCTAssertEqual(try fixture.documentObject(), before)
            XCTAssertEqual(fixture.adapter.historyFlags()?.canUndo, false, "the action was one history entry")
        }
    }

    func testEmptyFrameDeleteRemovesExactlyThatTableInOneMutation() throws {
        try withMountedTable(document: TableAccessibilityFixture.frameBesideTableDocument, cellSelection: nil) { fixture in
            let before = try fixture.documentObject()
            let revision = fixture.adapter.baseDocumentRevision
            let frame = try accessibleFrame(fixture)
            XCTAssertEqual(frame.accessibilityLabel, TableAccessibilityText.emptyTable.localized)
            XCTAssertFalse(frame.accessibilityFrame.isEmpty, "the frame is placed at its document position")
            XCTAssertEqual(frame.accessibilityCustomActions?.map(\.name), [TableAccessibilityAction.deleteTable.label])

            XCTAssertTrue(try perform(TableAccessibilityAction.deleteTable.label, on: frame))

            let blocks = try XCTUnwrap(try fixture.documentObject()["content"] as? [[String: Any]])
            XCTAssertEqual(blocks.map { $0["type"] as? String }, ["paragraph", "table", "paragraph"])
            XCTAssertEqual(try rowCount(fixture), 1, "the neighbouring table survives")
            XCTAssertEqual(fixture.adapter.cachedTableRecords.count, 1)
            XCTAssertEqual(fixture.updates.updates.count, 1)
            XCTAssertEqual(fixture.adapter.baseDocumentRevision, revision + 1)
            XCTAssertTrue(fixture.view.textView.applyUpdateJSON(try XCTUnwrap(fixture.adapter.undo())))
            XCTAssertEqual(try fixture.documentObject(), before)
            XCTAssertEqual(fixture.adapter.historyFlags()?.canUndo, false)
        }
    }

    func testReadOnlyEditorOffersNoTableActionsOrFrameDelete() throws {
        try withMountedTable(document: TableAccessibilityFixture.fourCellDocument, cellSelection: (0, 1)) { fixture in
            fixture.view.textView.isEditable = false
            let cells = try accessibleTable(fixture, containing: "one").cellElements
            XCTAssertEqual(cells[0].accessibilityCustomActions?.count, 0)
        }
        try withMountedTable(document: TableAccessibilityFixture.frameBesideTableDocument, cellSelection: nil) { fixture in
            fixture.view.textView.isEditable = false
            let before = try fixture.documentObject()
            XCTAssertEqual(try accessibleFrame(fixture).accessibilityCustomActions?.count, 0)
            XCTAssertEqual(try fixture.documentObject(), before)
        }
    }

    func testActivatedCellIsExposedAsTheRealInputInItsGridSlot() throws {
        try withMountedTable(document: TableAccessibilityFixture.fourCellDocument, cellSelection: nil) { fixture in
            let table = try accessibleTable(fixture, containing: "four")
            XCTAssertTrue(table.cellElements[3].accessibilityActivate())

            let input = fixture.surface.inputCoordinator.cellInput
            let fresh = try accessibleTable(fixture, containing: "one")
            let slots = try XCTUnwrap(fresh.accessibilityElements)
            XCTAssertEqual(slots.count, 4)
            XCTAssertTrue(slots[3] as? EditorTextView === input, "the active cell slot is the real text input")
            XCTAssertTrue(fresh.accessibilityDataTableCellElement(forRow: 1, column: 1) === input)
            XCTAssertEqual(input.accessibilityRowRange(), NSRange(location: 1, length: 1))
            XCTAssertEqual(input.accessibilityColumnRange(), NSRange(location: 1, length: 1))
            let expected = try publishedActionLabels(fixture)
            XCTAssertFalse(expected.isEmpty)
            XCTAssertEqual(input.accessibilityCustomActions?.map(\.name), expected)
            XCTAssertEqual(fresh.cellElements[0].accessibilityCustomActions?.count, 0)
        }
    }
}
