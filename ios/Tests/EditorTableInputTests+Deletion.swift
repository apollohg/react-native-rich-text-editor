import XCTest

extension EditorTableInputTests {
    private enum TableDeletion {
        static let emptyFrameDocument = #"{"type":"doc","content":[{"type":"paragraph","content":[{"type":"text","text":"before"}]},{"type":"table"},{"type":"paragraph","content":[{"type":"text","text":"after"}]}]}"#
        static let nestedDocument = #"{"type":"doc","content":[{"type":"paragraph","content":[{"type":"text","text":"before"}]},{"type":"table","content":[{"type":"table_row","content":[{"type":"table_cell","content":[{"type":"paragraph","content":[{"type":"text","text":"Alpha"}]}]},{"type":"table_cell","content":[{"type":"table","content":[{"type":"table_row","content":[{"type":"table_cell","content":[{"type":"paragraph","content":[{"type":"text","text":"Nested"}]}]}]}]}]}]}]},{"type":"paragraph","content":[{"type":"text","text":"after"}]}]}"#
        static let remainingProse: NSArray = [
            ["type": "paragraph", "content": [["type": "text", "text": "before"]]],
            ["type": "paragraph", "content": [["type": "text", "text": "after"]]]
        ]
    }

    private func blocks(_ fixture: MountedTableFixture) throws -> NSArray {
        try XCTUnwrap(try fixture.documentObject()["content"] as? NSArray)
    }

    func testDeleteTableRemovesAnEmptyFrameByItsPositionInOneUndoableMutation() throws {
        try withMountedTable(document: TableDeletion.emptyFrameDocument, cellSelection: nil) { fixture in
            let record = try XCTUnwrap(fixture.adapter.cachedTableRecords[fixture.tableID])
            XCTAssertEqual(record["rows"] as? Int, 0, "the fixture must be an empty frame: \(record)")
            XCTAssertTrue(fixture.positions.isEmpty, "an empty frame has no cell to anchor a delete")
            let before = try fixture.documentObject()
            let admission = try XCTUnwrap(fixture.adapter.tableMutationAdmission(tableID: fixture.tableID))

            let update = try XCTUnwrap(fixture.adapter.deleteTable(admission: admission),
                                       "an admitted explicit delete must reach the engine")
            XCTAssertTrue(fixture.view.textView.applyUpdateJSON(update))

            XCTAssertEqual(try blocks(fixture), TableDeletion.remainingProse)
            XCTAssertTrue(fixture.adapter.cachedTableRecords.isEmpty, "the frame's record is gone")
            XCTAssertEqual(fixture.adapter.historyFlags()?.canUndo, true)
            XCTAssertTrue(fixture.view.textView.applyUpdateJSON(try XCTUnwrap(fixture.adapter.undo())))
            XCTAssertEqual(try fixture.documentObject(), before, "one undo restores the frame")
            XCTAssertEqual(fixture.adapter.historyFlags()?.canUndo, false, "the delete was one history entry")
        }
    }

    func testDeleteTableRefusesStaleOwnerStaleRevisionAndNestedAdmissions() throws {
        try withMountedTable(document: TableDeletion.nestedDocument, cellSelection: nil) { fixture in
            let before = try fixture.documentObject()
            let nestedID = try XCTUnwrap(fixture.adapter.cachedTableRecords.first {
                $0.value["readOnlyDescendants"] as? Bool == true
            }?.key)
            let nested = try XCTUnwrap(fixture.adapter.tableMutationAdmission(tableID: nestedID))
            XCTAssertNil(fixture.adapter.deleteTable(admission: nested), "a nested table is read-only")
            XCTAssertEqual(try fixture.documentObject(), before)

            let stale = try XCTUnwrap(fixture.adapter.tableMutationAdmission(tableID: fixture.tableID))
            XCTAssertTrue(fixture.view.textView.applyUpdateJSON(
                try XCTUnwrap(fixture.adapter.setContentJson(TableDeletion.nestedDocument))))
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
