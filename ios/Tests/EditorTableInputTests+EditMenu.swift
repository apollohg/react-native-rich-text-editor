import XCTest

extension EditorTableInputTests {
    private enum CellMenu {
        static let gridDocument = #"{"type":"doc","content":[{"type":"table","content":[{"type":"table_row","content":[{"type":"table_cell","content":[{"type":"paragraph","content":[{"type":"text","text":"A"}]}]},{"type":"table_cell","content":[{"type":"paragraph","content":[{"type":"text","text":"B"}]}]}]},{"type":"table_row","content":[{"type":"table_cell","content":[{"type":"paragraph","content":[{"type":"text","text":"C"}]}]},{"type":"table_cell","content":[{"type":"paragraph","content":[{"type":"text","text":"D"}]}]}]}]},{"type":"paragraph","content":[{"type":"text","text":"after"}]}]}"#
        static let nestedDocument = #"{"type":"doc","content":[{"type":"paragraph","content":[{"type":"text","text":"before"}]},{"type":"table","content":[{"type":"table_row","content":[{"type":"table_cell","content":[{"type":"paragraph","content":[{"type":"text","text":"Alpha"}]}]},{"type":"table_cell","content":[{"type":"table","content":[{"type":"table_row","content":[{"type":"table_cell","content":[{"type":"paragraph","content":[{"type":"text","text":"Nested"}]}]}]}]}]}]}]},{"type":"paragraph","content":[{"type":"text","text":"after"}]}]}"#
        static let editorSize = CGSize(width: 480, height: 320)
        static let tallRowCount = 24
        static let firstCell = 0
        static let secondCell = 1
        static let lastCell = 3
        static let firstRowTSV = "A\tB"
        static let pastedGridTSV = "w\tx\ny\tz"
        static let copy = #selector(UIResponderStandardEditActions.copy(_:))
        static let cut = #selector(UIResponderStandardEditActions.cut(_:))
        static let paste = #selector(UIResponderStandardEditActions.paste(_:))
        static let allItems = [cut, copy, paste]
        static let copyOnly = [copy]
    }

    private final class SuggestedActionsProbe: NSObject, UIEditMenuInteractionDelegate {
        private(set) var suggested: [UIMenuElement]?

        func editMenuInteraction(_ interaction: UIEditMenuInteraction, menuFor configuration: UIEditMenuConfiguration,
                                 suggestedActions: [UIMenuElement]) -> UIMenu? {
            suggested = suggestedActions
            return nil
        }
    }

    private func menuCommands(for textView: EditorTextView) throws -> [UICommand] {
        let probe = SuggestedActionsProbe()
        let interaction = UIEditMenuInteraction(delegate: probe)
        textView.addInteraction(interaction)
        defer { textView.removeInteraction(interaction) }
        interaction.presentEditMenu(with: UIEditMenuConfiguration(identifier: nil, sourcePoint: .zero))
        let suggested = try XCTUnwrap(probe.suggested, "UIKit did not gather the responder's edit actions")
        return TableCellEditMenu.commands(in: suggested, performableBy: textView)
    }

    private func withCellMenuTable(
        _ document: String = CellMenu.gridDocument, size: CGSize = CellMenu.editorSize,
        anchor: Int = CellMenu.firstCell, head: Int = CellMenu.secondCell,
        _ body: (MountedTableFixture) throws -> Void
    ) throws {
        UIPasteboard.general.string = CellMenu.pastedGridTSV
        defer { UIPasteboard.general.items = [] }
        try withMountedTable(document: document, size: size, cellSelection: (anchor, head)) { fixture in
            XCTAssertTrue(fixture.view.textView.becomeFirstResponder())
            XCTAssertTrue(fixture.view.textView.authoritativeCellSelectionActive)
            fixture.updates.updates.removeAll()
            try body(fixture)
        }
    }

    private func surfacePoint(_ fixture: MountedTableFixture, inCell cell: Int) throws -> CGPoint {
        fixture.view.convert(try fixture.hostPoint(inCell: cell), to: fixture.surface)
    }

    private func showMenuByTappingSelection(_ fixture: MountedTableFixture) throws {
        fixture.view.tapTableCell(at: try surfacePoint(fixture, inCell: CellMenu.firstCell),
                                  touchedAt: ProcessInfo.processInfo.systemUptime)
        XCTAssertTrue(fixture.surface.isCellEditMenuVisible, "a tap inside the selection shows the cell menu")
        XCTAssertTrue(fixture.view.textView.authoritativeCellSelectionActive, "the tap keeps the cell selection")
        fixture.updates.updates.removeAll()
    }

    private func tableCellTapRecognizer(_ fixture: MountedTableFixture, taps: Int) throws -> UITapGestureRecognizer {
        try XCTUnwrap(fixture.view.textView.gestureRecognizers?.compactMap { $0 as? UITapGestureRecognizer }
            .first { $0.delegate === fixture.view && $0.numberOfTapsRequired == taps },
            "no \(taps)-tap table cell recognizer")
    }

    private func perform(_ command: UICommand, fixture: MountedTableFixture) {
        XCTAssertTrue(UIApplication.shared.sendAction(command.action, to: nil, from: command, for: nil),
                      "\(command.action) found no responder")
    }

    func testHandleDragEndLeavesTheMenuClosedAndATapInsideOffersCopyCutAndPaste() throws {
        try withCellMenuTable(head: CellMenu.firstCell) { fixture in
            XCTAssertTrue(fixture.surface.beginHandleDrag(at: try fixture.hostPoint(for: .head)))
            fixture.surface.updateHandleDrag(at: try fixture.hostPoint(inCell: CellMenu.lastCell))
            fixture.surface.cancelHandleDrag()
            XCTAssertEqual(try fixture.selection().1, fixture.positions[CellMenu.lastCell])
            XCTAssertFalse(fixture.surface.isCellEditMenuVisible, "a handle drag leaves the toolbar in charge")
            try showMenuByTappingSelection(fixture)
            let commands = try menuCommands(for: fixture.view.textView)
            XCTAssertEqual(commands.map(\.action), CellMenu.allItems)
            XCTAssertTrue(commands.allSatisfy { !$0.title.isEmpty }, "system titles: \(commands.map(\.title))")
        }
    }

    func testCopyItemCopiesTheCellsWithoutAMutation() throws {
        try withCellMenuTable { fixture in
            try showMenuByTappingSelection(fixture)
            let before = try fixture.documentObject()
            let copy = try XCTUnwrap(try menuCommands(for: fixture.view.textView).first { $0.action == CellMenu.copy })

            perform(copy, fixture: fixture)

            XCTAssertEqual(UIPasteboard.general.string, CellMenu.firstRowTSV)
            XCTAssertTrue(fixture.updates.updates.isEmpty, "copy must not mutate: \(fixture.updates.updates)")
            XCTAssertEqual(try fixture.documentObject(), before)
            XCTAssertEqual(fixture.adapter.historyFlags()?.canUndo, false)
        }
    }

    func testCutItemClearsTheCellsInOneMutation() throws {
        try withCellMenuTable { fixture in
            try showMenuByTappingSelection(fixture)
            let before = try fixture.documentObject()
            let cut = try XCTUnwrap(try menuCommands(for: fixture.view.textView).first { $0.action == CellMenu.cut })

            perform(cut, fixture: fixture)

            XCTAssertEqual(UIPasteboard.general.string, CellMenu.firstRowTSV)
            XCTAssertEqual(fixture.updates.updates.count, 1, "cut is exactly one mutation")
            XCTAssertNotEqual(try fixture.documentObject(), before)
            XCTAssertTrue(fixture.view.textView.applyUpdateJSON(try XCTUnwrap(fixture.adapter.undo())))
            XCTAssertEqual(try fixture.documentObject(), before, "one undo restores the cut cells")
            XCTAssertEqual(fixture.adapter.historyFlags()?.canUndo, false)
        }
    }

    func testPasteItemFillsTheSelectionInOneMutation() throws {
        try withCellMenuTable(head: CellMenu.lastCell) { fixture in
            try showMenuByTappingSelection(fixture)
            let before = try fixture.documentObject()
            let paste = try XCTUnwrap(try menuCommands(for: fixture.view.textView).first { $0.action == CellMenu.paste })

            perform(paste, fixture: fixture)

            XCTAssertEqual(fixture.updates.updates.count, 1, "paste is exactly one mutation")
            XCTAssertEqual(try fixture.selection().0, fixture.positions[CellMenu.firstCell])
            XCTAssertEqual(try fixture.selection().1, fixture.positions[CellMenu.lastCell])
            XCTAssertTrue(fixture.view.textView.applyUpdateJSON(try XCTUnwrap(fixture.adapter.undo())))
            XCTAssertEqual(try fixture.documentObject(), before, "one undo restores the pasted grid")
            XCTAssertEqual(fixture.adapter.historyFlags()?.canUndo, false)
        }
    }

    func testPasteItemIsAbsentWithAnEmptyClipboard() throws {
        try withCellMenuTable { fixture in
            UIPasteboard.general.items = []
            try showMenuByTappingSelection(fixture)
            XCTAssertEqual(try menuCommands(for: fixture.view.textView).map(\.action), [CellMenu.cut, CellMenu.copy])
        }
    }

    func testReadOnlyDescendantSelectionOffersOnlyCopy() throws {
        UIPasteboard.general.string = CellMenu.pastedGridTSV
        defer { UIPasteboard.general.items = [] }
        try withMountedTable(document: CellMenu.nestedDocument, size: CellMenu.editorSize, cellSelection: nil) { fixture in
            let nested = try XCTUnwrap(fixture.adapter.cachedTableRecords.values.first {
                $0["readOnlyDescendants"] as? Bool == true
            })
            let opening = try XCTUnwrap(EditorV2Adapter.uint32Field(
                try XCTUnwrap((nested["cells"] as? [[String: Any]])?.first), "sourcePos"
            ))
            try Self.selectCells(anchor: opening, head: opening, adapter: fixture.adapter, view: fixture.view)
            XCTAssertTrue(fixture.view.textView.becomeFirstResponder())
            XCTAssertEqual(try menuCommands(for: fixture.view.textView).map(\.action), CellMenu.copyOnly)
        }
    }

    func testAViewThatDoesNotOwnTheTableOffersOnlyCopy() throws {
        try withCellMenuTable(head: CellMenu.lastCell) { fixture in
            let stale = RichTextEditorView(frame: CGRect(origin: .zero, size: CellMenu.editorSize))
            fixture.view.window?.addSubview(stale)
            defer { stale.removeFromSuperview() }
            stale.bindEditor(id: fixture.view.editorId,
                             initialUpdateJSON: try XCTUnwrap(fixture.adapter.initialUpdateJSON()))
            XCTAssertTrue(stale.textView.applyUpdateJSON(
                try XCTUnwrap(fixture.adapter.refreshFromRustState(mirrorSelection: nil))
            ))
            XCTAssertFalse(stale.textView.ownsNativeBinding(fixture.adapter))
            XCTAssertTrue(stale.textView.authoritativeCellSelectionActive)
            XCTAssertTrue(stale.textView.becomeFirstResponder())
            XCTAssertEqual(try menuCommands(for: stale.textView).map(\.action), CellMenu.copyOnly)
        }
    }

    func testTapInsideTheSelectionTogglesTheMenuAndATapOutsideEditsThatCell() throws {
        try withCellMenuTable { fixture in
            let inside = try surfacePoint(fixture, inCell: CellMenu.firstCell)
            fixture.view.tapTableCell(at: inside, touchedAt: ProcessInfo.processInfo.systemUptime)
            XCTAssertTrue(fixture.surface.isCellEditMenuVisible, "a tap inside the selection shows the menu")
            XCTAssertTrue(fixture.view.textView.authoritativeCellSelectionActive, "the tap keeps the cell selection")

            let secondTouch = ProcessInfo.processInfo.systemUptime
            fixture.surface.dismissCellEditMenu()
            fixture.view.tapTableCell(at: inside, touchedAt: secondTouch)
            XCTAssertFalse(fixture.surface.isCellEditMenuVisible,
                           "a tap whose touch-down closed the menu must not reopen it")

            fixture.view.tapTableCell(at: inside, touchedAt: ProcessInfo.processInfo.systemUptime)
            XCTAssertTrue(fixture.surface.isCellEditMenuVisible)
            fixture.view.tapTableCell(at: try surfacePoint(fixture, inCell: CellMenu.lastCell),
                                      touchedAt: ProcessInfo.processInfo.systemUptime)
            XCTAssertFalse(fixture.surface.isCellEditMenuVisible, "leaving the cell selection closes the menu")
            XCTAssertTrue(fixture.view.activeTextInput !== fixture.view.textView, "the outside tap edits that cell")
        }
    }

    func testDoubleTapInsideTheSelectionEditsTheTappedCellWithoutFlashingTheMenu() throws {
        try withCellMenuTable(head: CellMenu.lastCell) { fixture in
            let single = try tableCellTapRecognizer(fixture, taps: 1)
            let double = try tableCellTapRecognizer(fixture, taps: 2)
            XCTAssertTrue(fixture.view.gestureRecognizer(single, shouldRequireFailureOf: double),
                          "the menu tap must wait for a double tap to fail")
            XCTAssertFalse(fixture.view.gestureRecognizer(double, shouldRequireFailureOf: single))

            fixture.view.doubleTapTableCell(at: try surfacePoint(fixture, inCell: CellMenu.lastCell))

            XCTAssertFalse(fixture.surface.isCellEditMenuVisible)
            XCTAssertTrue(fixture.view.activeTextInput !== fixture.view.textView, "the double tap edits a cell")
            XCTAssertTrue(fixture.view.activeTextInput.isFirstResponder)
            let map = try XCTUnwrap(fixture.view.activeTextInput.tableCellPositionMap)
            XCTAssertEqual(map.binding.cellSourcePosition, fixture.positions[CellMenu.lastCell],
                           "the caret lands in the double-tapped cell")
        }
    }

    func testSelectionChangeClosesTheMenu() throws {
        try withCellMenuTable { fixture in
            try showMenuByTappingSelection(fixture)
            try Self.selectCells(anchor: fixture.positions[CellMenu.firstCell],
                                 head: fixture.positions[CellMenu.lastCell],
                                 adapter: fixture.adapter, view: fixture.view)
            XCTAssertFalse(fixture.surface.isCellEditMenuVisible, "a different rectangle closes the menu")
        }
    }

    func testBlurClosesTheMenu() throws {
        try withCellMenuTable { fixture in
            try showMenuByTappingSelection(fixture)
            XCTAssertTrue(fixture.view.textView.resignFirstResponder())
            XCTAssertFalse(fixture.surface.isCellEditMenuVisible)
        }
    }

    func testEditorDestroyClosesTheMenu() throws {
        try withCellMenuTable { fixture in
            try showMenuByTappingSelection(fixture)
            fixture.view.editorId = 0
            XCTAssertFalse(fixture.surface.isCellEditMenuVisible)
        }
    }

    func testScrollReanchorsTheMenuUntilTheSelectionLeavesTheViewport() throws {
        let rows: [[String: Any]] = (0..<CellMenu.tallRowCount).map { index in
            ["type": "table_row", "content": [
                ["type": "table_cell", "content": [["type": "paragraph", "content": [["type": "text", "text": "r\(index)"]]]]],
                ["type": "table_cell", "content": [["type": "paragraph", "content": [["type": "text", "text": "s\(index)"]]]]]
            ]]
        }
        let data = try JSONSerialization.data(withJSONObject: ["type": "doc", "content": [["type": "table", "content": rows]]])
        let document = try XCTUnwrap(String(data: data, encoding: .utf8))
        try withCellMenuTable(document) { fixture in
            try showMenuByTappingSelection(fixture)
            let textView = fixture.view.textView
            let rowHeight = try fixture.presentedCell(CellMenu.firstCell).bounds.height
            textView.setContentOffset(CGPoint(x: 0, y: textView.contentOffset.y + rowHeight / 2), animated: false)
            fixture.view.layoutIfNeeded()
            XCTAssertTrue(fixture.surface.isCellEditMenuVisible, "a partly visible selection keeps its menu")

            textView.setContentOffset(CGPoint(x: 0, y: textView.contentSize.height - textView.bounds.height),
                                      animated: false)
            fixture.view.layoutIfNeeded()
            XCTAssertFalse(fixture.surface.isCellEditMenuVisible, "an offscreen selection closes the menu")
        }
    }
}
