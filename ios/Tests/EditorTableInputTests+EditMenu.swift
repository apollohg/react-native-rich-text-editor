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
        static let gestureTimeout: TimeInterval = 90
    }

    private final class SuggestedActionsProbe: NSObject, UIEditMenuInteractionDelegate {
        private(set) var suggested: [UIMenuElement]?

        func editMenuInteraction(
            _ interaction: UIEditMenuInteraction,
            menuFor configuration: UIEditMenuConfiguration,
            suggestedActions: [UIMenuElement]
        ) -> UIMenu? {
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

    private func tableMenu(_ fixture: MountedTableFixture) throws -> UIMenu {
        let interaction = try XCTUnwrap(fixture.view.textView.interactions.compactMap { $0 as? UIEditMenuInteraction }
            .first { $0.delegate is TableCellEditMenu })
        return try XCTUnwrap(interaction.delegate?.editMenuInteraction?(
            interaction, menuFor: UIEditMenuConfiguration(identifier: nil, sourcePoint: .zero), suggestedActions: []
        ))
    }

    func testNativeTableMenuOffersGroupedApplicableCommands() throws {
        try withCellMenuTable { fixture in
            let menu = try tableMenu(fixture)
            XCTAssertEqual(menu.children.compactMap { ($0 as? UIMenu)?.title }, ["Row", "Column", "Header", "More"])
            func actions(_ menu: UIMenu) -> [UIAction] {
                menu.children.flatMap { element -> [UIAction] in
                    if let submenu = element as? UIMenu { return actions(submenu) }
                    return (element as? UIAction).map { [$0] } ?? []
                }
            }
            let applicable = try XCTUnwrap(fixture.adapter.cachedActiveState?["commands"] as? [String: Any])
            XCTAssertEqual(Set(actions(menu).map(\.title)), Set(TableAccessibilityAction.all
                .filter { applicable[$0.applicability] as? Bool == true }.map(\.label)))
            let insert = try XCTUnwrap(actions(menu).first { $0.title == "Insert row below" })
            let before = try fixture.documentObject()
            let button = UIButton(primaryAction: insert)
            button.sendActions(for: .primaryActionTriggered)
            XCTAssertNotEqual(try fixture.documentObject(), before)
            XCTAssertEqual(fixture.updates.updates.count, 1, "one action produces one document mutation")
            XCTAssertTrue(fixture.view.textView.applyUpdateJSON(try XCTUnwrap(fixture.adapter.undo())))
            XCTAssertEqual(try fixture.documentObject(), before)
        }
    }

    func testTableMenuHasALongPressRecognizer() throws {
        try withCellMenuTable { fixture in
            let recognizer = try XCTUnwrap(fixture.view.gestureRecognizers?.first {
                $0 is UILongPressGestureRecognizer && $0.delegate === fixture.view
            })
            let textGestures = fixture.view.textView.interactions.compactMap { $0 as? UITextInteraction }
                .flatMap(\.gesturesForFailureRequirements)
            XCTAssertFalse(textGestures.isEmpty)
            for gesture in textGestures {
                XCTAssertTrue(fixture.view.gestureRecognizer(recognizer, shouldBeRequiredToFailBy: gesture))
            }
            XCTAssertFalse(fixture.view.gestureRecognizer(recognizer,
                shouldBeRequiredToFailBy: fixture.view.textView.panGestureRecognizer))
        }
    }

    private func prepareLiveCellMenuHost(_ fixture: MountedTableFixture) throws -> UIWindow {
        let window = try XCTUnwrap(fixture.view.window)
        let controller = UIViewController()
        window.rootViewController = controller
        controller.view.addSubview(fixture.view)
        fixture.view.frame = CGRect(x: 0, y: 120, width: 390, height: 500)
        fixture.view.layoutIfNeeded()
        fixture.surface.updateGeometry(from: fixture.view.textView)
        return window
    }

    func testLiveTextSelectionHandleWithinCellKeepsNativeSelection() throws {
        try LiveGestureProbe.requireOptIn()
        let document = CellMenu.gridDocument.replacingOccurrences(of: "\"text\":\"A\"", with: "\"text\":\"Alpha Beta\"")
        try withMountedTable(document: document, size: CGSize(width: 390, height: 844), cellSelection: nil) { fixture in
            let window = try prepareLiveCellMenuHost(fixture)
            let handle = try selectFirstCellText(fixture)
            let input = fixture.view.activeTextInput
            let fullLength = input.selectedRange.length
            let start = fixture.view.convert(handle, to: window)
            let before = try fixture.documentObject()
            print("TABLE_TEXT_SELECTION_WITHIN \(start.x) \(start.y)")
            let changed = XCTNSPredicateExpectation(predicate: NSPredicate { _, _ in
                input.selectedRange.length > 0 && input.selectedRange.length < fullLength
            }, object: nil)
            XCTAssertEqual(XCTWaiter.wait(for: [changed], timeout: CellMenu.gestureTimeout), .completed)
            XCTAssertTrue(fixture.view.activeTextInput === input)
            XCTAssertFalse(fixture.view.textView.authoritativeCellSelectionActive)
            XCTAssertFalse(fixture.surface.isCellEditMenuVisible)
            XCTAssertEqual(try fixture.documentObject(), before)
        }
    }

    func testLiveTextSelectionHandleCrossesCellsAndReopensMenuAfterAdjustment() throws {
        try LiveGestureProbe.requireOptIn()
        let document = CellMenu.gridDocument.replacingOccurrences(of: "\"text\":\"A\"", with: "\"text\":\"Alpha Beta\"")
        try withMountedTable(document: document, size: CGSize(width: 390, height: 844), cellSelection: nil) { fixture in
            let window = try prepareLiveCellMenuHost(fixture)
            let handle = try selectFirstCellText(fixture)
            let before = try fixture.documentObject()
            let start = fixture.view.convert(handle, to: window)
            let target = fixture.view.convert(try fixture.hostPoint(inCell: CellMenu.lastCell), to: window)
            print("TABLE_TEXT_SELECTION_DRAG \(start.x) \(start.y) \(target.x) \(target.y)")
            let presented = XCTNSPredicateExpectation(predicate: NSPredicate { _, _ in
                fixture.surface.isCellEditMenuVisible
            }, object: nil)
            guard XCTWaiter.wait(for: [presented], timeout: CellMenu.gestureTimeout) == .completed else {
                XCTFail("dragging a native text handle across cells must present the menu on release")
                return
            }
            XCTAssertEqual(try fixture.selection().0, fixture.positions[CellMenu.firstCell])
            XCTAssertEqual(try fixture.selection().1, fixture.positions[CellMenu.lastCell])
            XCTAssertEqual(try fixture.documentObject(), before)
            fixture.surface.dismissCellEditMenu()
            let cellHandle = fixture.view.convert(try fixture.hostPoint(for: .head), to: window)
            let next = fixture.view.convert(try fixture.hostPoint(inCell: CellMenu.secondCell), to: window)
            print("TABLE_CELL_SELECTION_DRAG \(cellHandle.x) \(cellHandle.y) \(next.x) \(next.y)")
            let adjusted = XCTNSPredicateExpectation(predicate: NSPredicate { _, _ in
                fixture.surface.isCellEditMenuVisible
                    && (try? fixture.selection().1) == fixture.positions[CellMenu.secondCell]
            }, object: nil)
            XCTAssertEqual(XCTWaiter.wait(for: [adjusted], timeout: CellMenu.gestureTimeout), .completed)
            XCTAssertEqual(try fixture.documentObject(), before)
        }
    }

    func testLiveLongPressPresentsNativeTableActions() throws {
        try LiveGestureProbe.requireOptIn()
        try withMountedTable(document: CellMenu.gridDocument, size: CGSize(width: 390, height: 844), cellSelection: nil) { fixture in
            let window = try prepareLiveCellMenuHost(fixture)
            let point = fixture.view.convert(try fixture.hostPoint(inCell: CellMenu.firstCell), to: fixture.view.window)
            print("TABLE_NATIVE_MENU_LONG_PRESS \(point.x) \(point.y)")
            let presented = XCTNSPredicateExpectation(predicate: NSPredicate { _, _ in
                fixture.surface.isCellEditMenuVisible
            }, object: nil)
            guard XCTWaiter.wait(for: [presented], timeout: CellMenu.gestureTimeout) == .completed else {
                XCTFail("the real long press did not open the table menu")
                return
            }
            let attachment = XCTAttachment(image: UIGraphicsImageRenderer(bounds: window.bounds).image {
                window.layer.render(in: $0.cgContext)
            })
            attachment.name = "Native table menu after a real long press"
            attachment.lifetime = .keepAlways
            add(attachment)
            let before = try fixture.documentObject()
            print("TABLE_NATIVE_MENU_ACTION_READY")
            let mutated = XCTNSPredicateExpectation(predicate: NSPredicate { _, _ in
                (try? fixture.documentObject()) != before
            }, object: nil)
            XCTAssertEqual(XCTWaiter.wait(for: [mutated], timeout: CellMenu.gestureTimeout), .completed)
            XCTAssertFalse(fixture.surface.isCellEditMenuVisible)
        }
    }

    func testDocumentMutationClosesMenuWithoutRetargetingItsActions() throws {
        try withCellMenuTable { fixture in
            try showMenuByLongPressingSelection(fixture)
            let context = try XCTUnwrap(fixture.surface.tableMutationContext(tableID: fixture.tableID))
            let before = try fixture.selection()
            let update = try XCTUnwrap(fixture.adapter.resizeTableColumn(column: 0, width: 120, admission: context.admission))
            XCTAssertTrue(fixture.view.textView.applyUpdateJSON(update))
            XCTAssertEqual(try fixture.selection().0, before.0)
            XCTAssertEqual(try fixture.selection().1, before.1)
            XCTAssertFalse(fixture.surface.isCellEditMenuVisible, "a changed document invalidates the displayed menu")
        }
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

    private func showMenuByLongPressingSelection(_ fixture: MountedTableFixture) throws {
        fixture.surface.presentCellEditMenu(at: try surfacePoint(fixture, inCell: CellMenu.firstCell))
        XCTAssertTrue(fixture.surface.isCellEditMenuVisible, "a long press inside the selection shows the cell menu")
        XCTAssertTrue(fixture.view.textView.authoritativeCellSelectionActive, "the long press keeps the cell selection")
        fixture.updates.updates.removeAll()
    }

    private func tableCellTapRecognizer(_ fixture: MountedTableFixture, taps: Int) throws -> UITapGestureRecognizer {
        try XCTUnwrap(
            fixture.view.textView.gestureRecognizers?.compactMap { $0 as? UITapGestureRecognizer }
                .first { $0.delegate === fixture.view && $0.numberOfTapsRequired == taps },
            "no \(taps)-tap table cell recognizer"
        )
    }

    private func perform(_ command: UICommand, fixture: MountedTableFixture) {
        XCTAssertTrue(
            UIApplication.shared.sendAction(command.action, to: nil, from: command, for: nil),
            "\(command.action) found no responder"
        )
    }

    func testHandleDragEndOpensTheMenuWithCopyCutAndPaste() throws {
        try withCellMenuTable(head: CellMenu.firstCell) { fixture in
            XCTAssertTrue(fixture.surface.beginHandleDrag(at: try fixture.hostPoint(for: .head)))
            fixture.surface.updateHandleDrag(at: try fixture.hostPoint(inCell: CellMenu.lastCell))
            fixture.surface.endHandleDrag(at: try fixture.hostPoint(inCell: CellMenu.lastCell))
            XCTAssertEqual(try fixture.selection().1, fixture.positions[CellMenu.lastCell])
            XCTAssertTrue(fixture.surface.isCellEditMenuVisible, "releasing a cell handle opens the menu")
            let commands = try menuCommands(for: fixture.view.textView)
            XCTAssertEqual(commands.map(\.action), CellMenu.allItems)
            XCTAssertTrue(commands.allSatisfy { !$0.title.isEmpty }, "system titles: \(commands.map(\.title))")
        }
    }

    func testCancellingACellHandleDragDoesNotOpenTheMenu() throws {
        try withCellMenuTable(head: CellMenu.firstCell) { fixture in
            XCTAssertTrue(fixture.surface.beginHandleDrag(at: try fixture.hostPoint(for: .head)))
            fixture.surface.updateHandleDrag(at: try fixture.hostPoint(inCell: CellMenu.lastCell))
            fixture.surface.cancelHandleDrag()
            XCTAssertFalse(fixture.surface.isCellEditMenuVisible)
        }
    }

    private func selectFirstCellText(_ fixture: MountedTableFixture) throws -> CGPoint {
        XCTAssertTrue(fixture.view.activateTableCell(at: try surfacePoint(fixture, inCell: CellMenu.firstCell)))
        let input = fixture.view.activeTextInput
        input.selectedRange = NSRange(location: 0, length: input.textStorage.length)
        input.syncSelectionImmediately()
        let range = try XCTUnwrap(input.selectedTextRange)
        let caret = input.caretRect(for: range.end)
        return input.convert(CGPoint(x: caret.midX, y: caret.maxY), to: fixture.view)
    }

    func testTextSelectionCrossesIntoACellRectangleAndOpensMenuOnRelease() throws {
        try withMountedTable(document: CellMenu.gridDocument, size: CellMenu.editorSize, cellSelection: nil) { fixture in
            let handle = try selectFirstCellText(fixture)
            let before = try fixture.documentObject()
            XCTAssertTrue(fixture.surface.trackTextSelectionDrag(at: handle))
            XCTAssertFalse(fixture.surface.beginTextSelectionDrag(at: try fixture.hostPoint(inCell: CellMenu.firstCell)))
            XCTAssertTrue(fixture.view.activeTextInput !== fixture.view.textView)
            XCTAssertEqual(fixture.view.activeTextInput.selectedRange, NSRange(location: 0, length: 1))
            let target = try fixture.hostPoint(inCell: CellMenu.lastCell)
            XCTAssertTrue(fixture.surface.beginTextSelectionDrag(at: target))
            XCTAssertEqual(try fixture.selection().0, fixture.positions[CellMenu.firstCell])
            XCTAssertEqual(try fixture.selection().1, fixture.positions[CellMenu.lastCell])
            XCTAssertFalse(fixture.surface.isCellEditMenuVisible, "the menu stays hidden during the drag")
            fixture.surface.endHandleDrag(at: target)
            XCTAssertTrue(fixture.surface.isCellEditMenuVisible)
            XCTAssertEqual(try fixture.documentObject(), before, "selection must not mutate the document")
            fixture.view.textView.copy(nil)
            XCTAssertEqual(UIPasteboard.general.string, "A\tB\nC\tD")
            UIPasteboard.general.items = []
        }
    }

    func testTextSelectionHandoffRejectsCompositionAndCollapsedCarets() throws {
        try withMountedTable(document: CellMenu.gridDocument, size: CellMenu.editorSize, cellSelection: nil) { fixture in
            let handle = try selectFirstCellText(fixture)
            let input = fixture.view.activeTextInput
            input.isComposing = true
            XCTAssertFalse(fixture.surface.trackTextSelectionDrag(at: handle))
            input.isComposing = false
            input.selectedRange = NSRange(location: 0, length: 0)
            input.syncSelectionImmediately()
            XCTAssertFalse(fixture.surface.trackTextSelectionDrag(at: handle))
            XCTAssertFalse(fixture.surface.beginTextSelectionDrag(at: try fixture.hostPoint(inCell: CellMenu.lastCell)))
            XCTAssertTrue(fixture.view.activeTextInput === input)
        }
    }

    func testTextSelectionAnchorCrossesCellsWithNativeMenuDisabled() throws {
        try withMountedTable(document: CellMenu.gridDocument, size: CellMenu.editorSize, cellSelection: nil) { fixture in
            fixture.view.tableEditMenuEnabled = false
            _ = try selectFirstCellText(fixture)
            let input = fixture.view.activeTextInput
            let range = try XCTUnwrap(input.selectedTextRange)
            let caret = input.caretRect(for: range.start)
            let handle = input.convert(CGPoint(x: caret.midX, y: caret.minY), to: fixture.view)
            XCTAssertTrue(fixture.surface.trackTextSelectionDrag(at: handle))
            let target = try fixture.hostPoint(inCell: CellMenu.lastCell)
            XCTAssertTrue(fixture.surface.beginTextSelectionDrag(at: target))
            fixture.surface.endHandleDrag(at: target)
            XCTAssertEqual(try fixture.selection().0, fixture.positions[CellMenu.lastCell])
            XCTAssertEqual(try fixture.selection().1, fixture.positions[CellMenu.firstCell])
            XCTAssertFalse(fixture.surface.isCellEditMenuVisible)
        }
    }

    func testNativeTextGrabberOutsideTheInputFrameReceivesTouches() throws {
        try withMountedTable(document: CellMenu.gridDocument, size: CellMenu.editorSize, cellSelection: nil) { fixture in
            var point = try selectFirstCellText(fixture)
            point.y += PreparedProseDrawingView.TableHandleMetrics.radius
            let input = fixture.view.activeTextInput
            XCTAssertFalse(input.bounds.contains(fixture.view.convert(point, to: input)), "probe the visible grabber outside the tight text frame")
            let hit = fixture.view.hitTest(point, with: nil)
            XCTAssertTrue(hit === input || hit?.isDescendant(of: input) == true,
                "native text selection must receive touches on its visible grabber: \(String(describing: hit))")
        }
    }

    func testTextSelectionHandoffRejectsAChangedDocument() throws {
        try withMountedTable(document: CellMenu.gridDocument, size: CellMenu.editorSize, cellSelection: nil) { fixture in
            XCTAssertTrue(fixture.surface.trackTextSelectionDrag(at: try selectFirstCellText(fixture)))
            let target = try fixture.hostPoint(inCell: CellMenu.lastCell)
            XCTAssertTrue(fixture.view.activeTextInput.applyUpdateJSON(try XCTUnwrap(
                fixture.adapter.setContentJson(CellMenu.gridDocument)
            )))
            XCTAssertFalse(fixture.surface.beginTextSelectionDrag(at: target))
            fixture.surface.endHandleDrag(at: target)
            XCTAssertFalse(fixture.surface.isCellEditMenuVisible)
        }
    }

    func testCopyItemCopiesTheCellsWithoutAMutation() throws {
        try withCellMenuTable { fixture in
            try showMenuByLongPressingSelection(fixture)
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
            try showMenuByLongPressingSelection(fixture)
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
            try showMenuByLongPressingSelection(fixture)
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
            try showMenuByLongPressingSelection(fixture)
            XCTAssertEqual(try menuCommands(for: fixture.view.textView).map(\.action), [CellMenu.cut, CellMenu.copy])
        }
    }

    func testReadOnlyDescendantSelectionOffersOnlyCopy() throws {
        UIPasteboard.general.string = CellMenu.pastedGridTSV
        defer { UIPasteboard.general.items = [] }
        try withMountedTable(document: CellMenu.nestedDocument, size: CellMenu.editorSize, cellSelection: nil) { fixture in
            let nested = try XCTUnwrap(fixture.adapter.tableRecordsForTesting.values.first {
                $0["readOnlyDescendants"] as? Bool == true
            })
            let opening = try XCTUnwrap(EditorV2Adapter.uint32Field(
                try XCTUnwrap((nested["cells"] as? [[String: Any]])?.first), "sourcePos"
            ))
            try fixture.view.textView.selectTableCells(adapter: fixture.adapter, anchor: opening, head: opening)
            XCTAssertTrue(fixture.view.textView.becomeFirstResponder())
            XCTAssertEqual(try menuCommands(for: fixture.view.textView).map(\.action), CellMenu.copyOnly)
        }
    }

    func testAViewThatDoesNotOwnTheTableOffersOnlyCopy() throws {
        try withCellMenuTable(head: CellMenu.lastCell) { fixture in
            let stale = RichTextEditorView(frame: CGRect(origin: .zero, size: CellMenu.editorSize))
            fixture.view.window?.addSubview(stale)
            defer { stale.removeFromSuperview() }
            stale.bindEditor(
                id: fixture.view.editorId,
                initialUpdateJSON: try XCTUnwrap(fixture.adapter.initialUpdateJSON())
            )
            XCTAssertTrue(stale.textView.applyUpdateJSON(
                try XCTUnwrap(fixture.adapter.refreshFromRustState(mirrorSelection: nil))
            ))
            XCTAssertFalse(stale.textView.ownsNativeBinding(fixture.adapter))
            XCTAssertTrue(stale.textView.authoritativeCellSelectionActive)
            XCTAssertTrue(stale.textView.becomeFirstResponder())
            XCTAssertEqual(try menuCommands(for: stale.textView).map(\.action), CellMenu.copyOnly)
        }
    }

    func testLongPressPreservesTheRectangleAndTapReturnsToEditing() throws {
        try withCellMenuTable { fixture in
            let inside = try surfacePoint(fixture, inCell: CellMenu.firstCell)
            let selection = try fixture.selection()
            fixture.surface.presentCellEditMenu(at: inside)
            XCTAssertTrue(fixture.surface.isCellEditMenuVisible)
            XCTAssertEqual(try fixture.selection().0, selection.0)
            XCTAssertEqual(try fixture.selection().1, selection.1)
            fixture.view.tapTableCell(at: inside, touchedAt: ProcessInfo.processInfo.systemUptime)
            XCTAssertFalse(fixture.surface.isCellEditMenuVisible)
            XCTAssertTrue(fixture.view.activeTextInput !== fixture.view.textView)
            XCTAssertTrue(fixture.view.activeTextInput.isFirstResponder)
        }
    }

    func testLongPressSelectsAnInactiveCellAndLeavesActiveTextGesturesAlone() throws {
        try withMountedTable(document: CellMenu.gridDocument, size: CellMenu.editorSize, cellSelection: nil) { fixture in
            let point = try surfacePoint(fixture, inCell: CellMenu.lastCell)
            XCTAssertFalse(fixture.surface.isCellEditMenuVisible)
            XCTAssertTrue(fixture.view.canPresentTableMenu(at: point, touchedView: fixture.view.textView))
            fixture.surface.presentCellEditMenu(at: point)
            XCTAssertTrue(fixture.surface.isCellEditMenuVisible)
            XCTAssertEqual(try fixture.selection().0, fixture.positions[CellMenu.lastCell])
            XCTAssertEqual(try fixture.selection().1, fixture.positions[CellMenu.lastCell])
            fixture.view.tapTableCell(at: point, touchedAt: ProcessInfo.processInfo.systemUptime)
            let input = fixture.view.activeTextInput
            XCTAssertFalse(input === fixture.view.textView)
            XCTAssertFalse(fixture.view.canPresentTableMenu(at: point, touchedView: input))
            XCTAssertFalse(fixture.view.canPresentTableMenu(at: point, touchedView: UIButton()))
        }
    }

    func testNativeMenuActionsCannotRetargetAfterSelectionOrRevisionChanges() throws {
        for changeDocument in [false, true] {
            try withCellMenuTable { fixture in
                try showMenuByLongPressingSelection(fixture)
                let menu = try tableMenu(fixture)
                let row = try XCTUnwrap(menu.children.compactMap { $0 as? UIMenu }.first { $0.title == "Row" })
                let action = try XCTUnwrap(row.children.compactMap { $0 as? UIAction }.first)
                if changeDocument {
                    XCTAssertTrue(fixture.view.textView.applyUpdateJSON(try XCTUnwrap(fixture.adapter.setContentJson(
                        CellMenu.gridDocument.replacingOccurrences(of: "A", with: "remote")
                    ))))
                } else {
                    try fixture.view.textView.selectTableCells(adapter: fixture.adapter,
                        anchor: fixture.positions[CellMenu.lastCell], head: fixture.positions[CellMenu.lastCell])
                }
                let before = try fixture.documentObject()
                XCTAssertFalse(fixture.surface.isCellEditMenuVisible)
                UIButton(primaryAction: action).sendActions(for: .primaryActionTriggered)
                XCTAssertEqual(try fixture.documentObject(), before, "an old menu must never target a new selection")
            }
        }
    }

    func testRetainedMenuActionRejectsUnrenderedSelectionChange() throws {
        try withCellMenuTable { fixture in
            let menu = try tableMenu(fixture)
            let row = try XCTUnwrap(menu.children.compactMap { $0 as? UIMenu }.first { $0.title == "Row" })
            let action = try XCTUnwrap(row.children.compactMap { $0 as? UIAction }.first)
            let context = try XCTUnwrap(fixture.surface.tableMutationContext(tableID: fixture.tableID))
            XCTAssertNotNil(fixture.adapter.selectTableCell(cellIndex: CellMenu.firstCell, admission: context.admission))
            XCTAssertEqual(fixture.drawing.selectedTableCellEndpoints?.head, fixture.positions[CellMenu.secondCell])
            let before = try fixture.documentObject()
            UIButton(primaryAction: action).sendActions(for: .primaryActionTriggered)
            XCTAssertEqual(try fixture.documentObject(), before, "a retained menu cannot act on an unrendered selection")
        }
    }

    func testLongPressSelectionRejectsReentrantPublicationReset() throws {
        var onPublish: (() -> Void)?
        try withMountedTable(document: CellMenu.gridDocument, size: CellMenu.editorSize, cellSelection: nil,
            roomAwareness: { _, _ in
                let callback = onPublish
                onPublish = nil
                callback?()
                return FfiJsonResult(value: #"{"outboundChanged":false}"#, error: nil)
            }, { fixture in
            let context = try XCTUnwrap(fixture.surface.tableMutationContext(tableID: fixture.tableID))
            var didReset = false
            onPublish = {
                didReset = true
                XCTAssertNotNil(fixture.adapter.setContentJson(CellMenu.gridDocument))
            }
            XCTAssertNil(fixture.adapter.selectTableCell(cellIndex: CellMenu.lastCell, admission: context.admission),
                "a reset during presence publication invalidates the selection update")
            XCTAssertTrue(didReset)
        })
    }

    func testDisabledTableMenuPreservesClipboardAndCannotOpenByLongPress() throws {
        try withCellMenuTable { fixture in
            try showMenuByLongPressingSelection(fixture)
            fixture.view.tableEditMenuEnabled = false
            XCTAssertFalse(fixture.surface.isCellEditMenuVisible)
            let point = try surfacePoint(fixture, inCell: CellMenu.firstCell)
            fixture.surface.presentCellEditMenu(at: point)
            XCTAssertFalse(fixture.surface.isCellEditMenuVisible)
            XCTAssertTrue(try tableMenu(fixture).children.isEmpty)
            XCTAssertEqual(try menuCommands(for: fixture.view.textView).map(\.action), CellMenu.allItems)
        }
    }

    func testDoubleTapInsideTheSelectionEditsTheTappedCellWithoutFlashingTheMenu() throws {
        try withCellMenuTable(head: CellMenu.lastCell) { fixture in
            let single = try tableCellTapRecognizer(fixture, taps: 1)
            let double = try tableCellTapRecognizer(fixture, taps: 2)
            XCTAssertTrue(
                fixture.view.gestureRecognizer(single, shouldRequireFailureOf: double),
                "the menu tap must wait for a double tap to fail"
            )
            XCTAssertFalse(fixture.view.gestureRecognizer(double, shouldRequireFailureOf: single))

            fixture.view.doubleTapTableCell(at: try surfacePoint(fixture, inCell: CellMenu.lastCell))

            XCTAssertFalse(fixture.surface.isCellEditMenuVisible)
            XCTAssertTrue(fixture.view.activeTextInput !== fixture.view.textView, "the double tap edits a cell")
            XCTAssertTrue(fixture.view.activeTextInput.isFirstResponder)
            let map = try XCTUnwrap(fixture.view.activeTextInput.tableCellPositionMap)
            XCTAssertEqual(
                map.binding.documentPosition(in: fixture.adapter),
                fixture.positions[CellMenu.lastCell],
                "the caret lands in the double-tapped cell"
            )
        }
    }

    func testSelectionChangeClosesTheMenu() throws {
        try withCellMenuTable { fixture in
            try showMenuByLongPressingSelection(fixture)
            try fixture.view.textView.selectTableCells(
                adapter: fixture.adapter,
                anchor: fixture.positions[CellMenu.firstCell],
                head: fixture.positions[CellMenu.lastCell]
            )
            XCTAssertFalse(fixture.surface.isCellEditMenuVisible, "a different rectangle closes the menu")
        }
    }

    func testBlurClosesTheMenu() throws {
        try withCellMenuTable { fixture in
            try showMenuByLongPressingSelection(fixture)
            XCTAssertTrue(fixture.view.textView.resignFirstResponder())
            XCTAssertFalse(fixture.surface.isCellEditMenuVisible)
        }
    }

    func testEditorDestroyClosesTheMenu() throws {
        try withCellMenuTable { fixture in
            try showMenuByLongPressingSelection(fixture)
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
            try showMenuByLongPressingSelection(fixture)
            let textView = fixture.view.textView
            let rowHeight = try fixture.presentedCell(CellMenu.firstCell).bounds.height
            textView.setContentOffset(CGPoint(x: 0, y: textView.contentOffset.y + rowHeight / 2), animated: false)
            fixture.view.layoutIfNeeded()
            XCTAssertTrue(fixture.surface.isCellEditMenuVisible, "a partly visible selection keeps its menu")

            textView.setContentOffset(
                CGPoint(x: 0, y: textView.contentSize.height - textView.bounds.height),
                animated: false
            )
            fixture.view.layoutIfNeeded()
            XCTAssertFalse(fixture.surface.isCellEditMenuVisible, "an offscreen selection closes the menu")
        }
    }
}
