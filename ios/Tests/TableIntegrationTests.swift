import XCTest

final class TableIntegrationTests: XCTestCase {
    private enum Integration {
        static let gridDocument = #"{"type":"doc","content":[{"type":"table","content":[{"type":"table_row","content":[{"type":"table_cell","content":[{"type":"paragraph","content":[{"type":"text","text":"A"}]}]},{"type":"table_cell","content":[{"type":"paragraph","content":[{"type":"text","text":"B"}]}]}]},{"type":"table_row","content":[{"type":"table_cell","content":[{"type":"paragraph","content":[{"type":"text","text":"C"}]}]},{"type":"table_cell","content":[{"type":"paragraph","content":[{"type":"text","text":"D"}]}]}]}]},{"type":"paragraph","content":[{"type":"text","text":"after"}]}]}"#
        static let shiftedGridDocument = #"{"type":"doc","content":[{"type":"paragraph","content":[{"type":"text","text":"shifted"}]},{"type":"table","content":[{"type":"table_row","content":[{"type":"table_cell","content":[{"type":"paragraph","content":[{"type":"text","text":"A"}]}]},{"type":"table_cell","content":[{"type":"paragraph","content":[{"type":"text","text":"B"}]}]}]},{"type":"table_row","content":[{"type":"table_cell","content":[{"type":"paragraph","content":[{"type":"text","text":"C"}]}]},{"type":"table_cell","content":[{"type":"paragraph","content":[{"type":"text","text":"D"}]}]}]}]},{"type":"paragraph","content":[{"type":"text","text":"after"}]}]}"#
        static let irregularDocument = #"{"type":"doc","content":[{"type":"paragraph","content":[{"type":"text","text":"before"}]},{"type":"table","content":[{"type":"table_row","content":[{"type":"table_cell","attrs":{"rowspan":2},"content":[{"type":"paragraph","content":[{"type":"text","text":"tall"}]}]},{"type":"table_cell","attrs":{"colspan":2},"content":[{"type":"paragraph","content":[{"type":"text","text":"wide"}]}]}]},{"type":"table_row","content":[{"type":"table_cell","content":[{"type":"paragraph","content":[{"type":"text","text":"later"}]}]}]}]},{"type":"paragraph","content":[{"type":"text","text":"after"}]}]}"#
        static let gridLastText = "D"
        static let proseBeforeTableText = "before"
        static let tallText = "tall"
        static let wideText = "wide"
        static let laterText = "later"
        static let wideDocument = #"{"type":"doc","content":[{"type":"table","content":[{"type":"table_row","content":[{"type":"table_cell","attrs":{"colwidth":[500]},"content":[{"type":"paragraph","content":[{"type":"text","text":"one"}]}]},{"type":"table_cell","attrs":{"colwidth":[500]},"content":[{"type":"paragraph","content":[{"type":"text","text":"two"}]}]}]}]},{"type":"paragraph","content":[{"type":"text","text":"after"}]}]}"#
        static let editorSize = CGSize(width: 480, height: 320)
        static let narrowEditorSize = CGSize(width: 360, height: 240)
        static let firstPeer = "7"
        static let secondPeer = "9"
        static let firstPeerColor = "#FF0000"
        static let firstPeerFill = UIColor(red: 1, green: 0, blue: 0, alpha: 1)
        static let secondPeerColor = "#0000FF"
        static let secondPeerFill = UIColor(red: 0, green: 0, blue: 1, alpha: 1)
        static let peerName = "Remote"
        static let remoteRequestIdBase: UInt64 = 22_000_000
        static let horizontalScroll: CGFloat = -300
        static let pixelInsetFromCellBottom: CGFloat = 3
        static let geometryAccuracy: CGFloat = 0.5
        static let compositionText = "Z"
        static let staleCompositionText = "Q"
        static let cellStartCaret = NSRange(location: 0, length: 0)
        static let tallTextHead = NSRange(location: 0, length: 2)
        static let remoteProseText = "R"
        static let remoteProseScalar: UInt32 = 0
        static let remoteSelectionAffinity = "before"
        static let irregularRectangleTSV = "Zwide\t\nlater\t"
        static let tallCell = 0
        static let wideCell = 1
        static let laterCell = 2
        static let gridFirst = 0
        static let gridSecond = 1
        static let gridLast = 3
        static let mergedColumnCount = 2
        static let deleteTable = "deleteTable"
        static let deleteTableRows = "deleteTableRows"
        static let insertText = "insertText"
        static let toggleTableHeader = "toggleTableHeader"
        static let headerTargetCell = "cell"
        static let headerCellNode = #""type":"table_header""#
        static let mergeTableCells = "mergeTableCells"
        static let copy = #selector(UIResponderStandardEditActions.copy(_:))
        static let cut = #selector(UIResponderStandardEditActions.cut(_:))
        static let paste = #selector(UIResponderStandardEditActions.paste(_:))
        static let cellMenuItems = [cut, copy, paste]
    }

    private struct Peer {
        let clientId: String
        let color: String
        let anchor: UInt32
        let head: UInt32
        let cellRectangle: (anchor: UInt32, head: UInt32)?
    }

    private final class Fixture {
        let expo: NativeEditorExpoView
        let adapter: EditorV2Adapter
        let editorId: UInt64
        let tableID: String
        let surface: EditorTableSurface
        let drawing: PreparedProseDrawingView
        private let remote: RemoteTablePeer

        init(expo: NativeEditorExpoView, adapter: EditorV2Adapter, editorId: UInt64, tableID: String,
             surface: EditorTableSurface, drawing: PreparedProseDrawingView) {
            self.expo = expo
            self.adapter = adapter
            self.editorId = editorId
            self.tableID = tableID
            self.surface = surface
            self.drawing = drawing
            remote = RemoteTablePeer(adapter: adapter, requestIdBase: Integration.remoteRequestIdBase)
        }

        var view: RichTextEditorView { expo.richTextView }

        func positions() throws -> [UInt32] {
            try adapter.tableCellPositions(tableID: tableID)
        }

        func documentObject() throws -> NSDictionary {
            let json = try XCTUnwrap(adapter.documentJson())
            return try XCTUnwrap(JSONSerialization.jsonObject(with: Data(json.utf8)) as? NSDictionary)
        }

        func tablePos() throws -> UInt32 {
            try XCTUnwrap(adapter.tableRecordsForTesting[tableID].flatMap { EditorV2Adapter.uint32Field($0, "tablePos") })
        }

        func presentedCell(_ position: UInt32) throws -> ViewerTablePresentedCell {
            try drawing.presentedRealCell(tableID: tableID, position: position)
        }

        func remoteRects(_ selection: RemoteTableCellSelection) throws -> [CGRect] {
            try XCTUnwrap(drawing.tableCellRects(tableID: selection.tableID, sourceIndices: selection.sourceIndices))
        }

        func activeCellPosition() -> UInt32? {
            view.activeTableCellPosition
        }

        func setPeers(_ peers: [Peer]) throws {
            let items = peers.map { peer -> [String: Any] in
                var item: [String: Any] = [
                    "clientId": peer.clientId, "anchor": Int(peer.anchor), "head": Int(peer.head),
                    "color": peer.color, "name": Integration.peerName, "isFocused": true
                ]
                if let rectangle = peer.cellRectangle {
                    item["cellRectangle"] = ["anchorCell": Int(rectangle.anchor), "headCell": Int(rectangle.head)]
                }
                return item
            }
            let data = try JSONSerialization.data(withJSONObject: items)
            expo.setRemoteSelectionsJson(try XCTUnwrap(String(data: data, encoding: .utf8)))
        }

        func applyRemoteCommand(_ command: [String: Any]) throws {
            try remote.applyCommand(command)
        }

        func applyRemoteTextSelection(at scalar: UInt32) throws {
            try remote.applySelection(EditorV2PositionBridge.textSelectionEnvelope(
                anchor: scalar, head: scalar, affinity: Integration.remoteSelectionAffinity
            ))
        }

        func applyRemoteCellSelection(anchor: UInt32, head: UInt32) throws {
            try remote.applySelection(documentCellSelection(anchor: anchor, head: head))
        }

        func deliverRemoteCommit() {
            expo.deliverRemoteCommit(editorId: editorId)
        }

        func renderDrawing() throws -> CGImage {
            let format = UIGraphicsImageRendererFormat()
            format.scale = 1
            let image = UIGraphicsImageRenderer(size: drawing.bounds.size, format: format).image { _ in
                drawing.draw(drawing.bounds)
            }
            return try XCTUnwrap(image.cgImage)
        }
    }

    func testRemoteRectangleResolvesByIndex() throws {
        try withTable(Integration.gridDocument) { fixture in
            let positions = try fixture.positions()
            let first = positions[Integration.gridFirst]
            let second = positions[Integration.gridSecond]

            try fixture.setPeers([Peer(clientId: Integration.firstPeer, color: Integration.firstPeerColor,
                                       anchor: first, head: second, cellRectangle: (first, second))])

            let remote = try XCTUnwrap(fixture.drawing.remoteTableCellSelections.first)
            XCTAssertEqual(fixture.drawing.remoteTableCellSelections.count, 1)
            XCTAssertEqual(remote.tableID, fixture.tableID)
            XCTAssertEqual(remote.sourceIndices, [Integration.gridFirst, Integration.gridSecond])
            XCTAssertEqual(rgba(remote.color), expectedPeerFill(Integration.firstPeerFill))
            let expected = try [first, second].map { position -> CGRect in
                let cell = try fixture.presentedCell(position)
                return cell.bounds.intersection(cell.clip)
            }
            XCTAssertEqual(Set(try fixture.remoteRects(remote).map(NSValue.init(cgRect:))),
                           Set(expected.map(NSValue.init(cgRect:))),
                           "the rectangle must use the presented cell frames")
            XCTAssertTrue(fixture.view.remoteSelectionOverlaySubviewsForTesting().isEmpty,
                          "a drawn rectangle replaces the peer's cursor fallback")

            try fixture.setPeers([Peer(clientId: Integration.firstPeer, color: Integration.firstPeerColor,
                                       anchor: first, head: second, cellRectangle: nil)])

            XCTAssertTrue(fixture.drawing.remoteTableCellSelections.isEmpty)
            XCTAssertFalse(fixture.view.remoteSelectionOverlaySubviewsForTesting().isEmpty,
                           "a peer without a rectangle keeps its ordinary cursor")
        }
    }

    func testUnresolvableRemoteRectangleIsDroppedAndOnlyTheCursorFallbackRemains() throws {
        try withTable(Integration.gridDocument) { fixture in
            let first = try fixture.positions()[Integration.gridFirst]
            let insideFirstCellText = first + 2

            try fixture.setPeers([Peer(clientId: Integration.firstPeer, color: Integration.firstPeerColor,
                                       anchor: first, head: first, cellRectangle: (insideFirstCellText, first))])

            XCTAssertTrue(fixture.drawing.remoteTableCellSelections.isEmpty,
                          "an anchor that is not a real cell opening must not draw")
            XCTAssertFalse(fixture.view.remoteSelectionOverlaySubviewsForTesting().isEmpty)
        }
    }

    func testRemoteMergeReresolvesTheRectangleToTheMergedCell() throws {
        try withTable(Integration.gridDocument) { fixture in
            let before = try fixture.positions()
            let first = before[Integration.gridFirst]
            let firstWidth = try fixture.presentedCell(first).bounds.width
            let secondWidth = try fixture.presentedCell(before[Integration.gridSecond]).bounds.width
            try fixture.setPeers([Peer(clientId: Integration.firstPeer, color: Integration.firstPeerColor,
                                       anchor: first, head: first, cellRectangle: (first, first))])
            let unmerged = try fixture.remoteRects(try XCTUnwrap(fixture.drawing.remoteTableCellSelections.first))
            XCTAssertEqual(unmerged.count, 1)
            XCTAssertEqual(try XCTUnwrap(unmerged.first).width, firstWidth, accuracy: Integration.geometryAccuracy)

            try fixture.applyRemoteCellSelection(anchor: first, head: before[Integration.gridSecond])
            try fixture.applyRemoteCommand(["type": Integration.mergeTableCells])
            fixture.deliverRemoteCommit()

            let mergedRecord = try XCTUnwrap((fixture.adapter.tableRecordsForTesting[fixture.tableID]?["cells"] as? [[String: Any]])?
                .first { EditorV2Adapter.uint32Field($0, "sourcePos") == first })
            XCTAssertEqual(EditorV2Adapter.uint32Field(mergedRecord, "colspan"), UInt32(Integration.mergedColumnCount),
                           "the remote merge must land")
            let merged = try fixture.presentedCell(first)
            let remote = try XCTUnwrap(fixture.drawing.remoteTableCellSelections.first)
            XCTAssertEqual(remote.sourceIndices, [Integration.gridFirst])
            let rects = try fixture.remoteRects(remote)
            XCTAssertEqual(rects.count, 1)
            XCTAssertEqual(try XCTUnwrap(rects.first), merged.bounds.intersection(merged.clip))
            XCTAssertEqual(try XCTUnwrap(rects.first).width, firstWidth + secondWidth,
                           accuracy: Integration.geometryAccuracy, "the rectangle follows the merged cell")
        }
    }

    func testExpiredPeersTakeTheirRectanglesWithThem() throws {
        try withTable(Integration.gridDocument) { fixture in
            let positions = try fixture.positions()
            let first = positions[Integration.gridFirst]
            let last = positions[Integration.gridLast]
            let firstPeer = Peer(clientId: Integration.firstPeer, color: Integration.firstPeerColor,
                                 anchor: first, head: first, cellRectangle: (first, first))
            let secondPeer = Peer(clientId: Integration.secondPeer, color: Integration.secondPeerColor,
                                  anchor: last, head: last, cellRectangle: (last, last))
            try fixture.setPeers([firstPeer, secondPeer])
            XCTAssertEqual(fixture.drawing.remoteTableCellSelections.map(\.sourceIndices),
                           [[Integration.gridFirst], [Integration.gridLast]])

            try fixture.setPeers([secondPeer])

            XCTAssertEqual(fixture.drawing.remoteTableCellSelections.map(\.sourceIndices), [[Integration.gridLast]])
            XCTAssertEqual(rgba(try XCTUnwrap(fixture.drawing.remoteTableCellSelections.first).color),
                           expectedPeerFill(Integration.secondPeerFill))

            try fixture.setPeers([])

            XCTAssertTrue(fixture.drawing.remoteTableCellSelections.isEmpty)
            XCTAssertTrue(fixture.view.remoteSelectionOverlaySubviewsForTesting().isEmpty)
        }
    }

    func testHorizontalTableScrollMovesTheRectangleWithTheCellAndClipsItToTheTable() throws {
        try withTable(Integration.wideDocument, size: Integration.narrowEditorSize) { fixture in
            let second = try fixture.positions()[Integration.gridSecond]
            try fixture.setPeers([Peer(clientId: Integration.firstPeer, color: Integration.firstPeerColor,
                                       anchor: second, head: second, cellRectangle: (second, second))])
            let remote = try XCTUnwrap(fixture.drawing.remoteTableCellSelections.first)
            let before = try fixture.presentedCell(second)
            XCTAssertTrue(try fixture.remoteRects(remote).isEmpty,
                          "the second wide cell starts outside the table viewport: \(before.bounds) \(before.clip)")
            let table = try XCTUnwrap(fixture.drawing.mountedTablePresentation()?.tables.first {
                $0.surface.identity == fixture.tableID
            })

            fixture.drawing.scrollTables(in: [table.surface.scrollIdentity], by: Integration.horizontalScroll)

            let after = try fixture.presentedCell(second)
            XCTAssertEqual(after.bounds.minX, before.bounds.minX + Integration.horizontalScroll,
                           accuracy: Integration.geometryAccuracy)
            let rect = try XCTUnwrap(try fixture.remoteRects(remote).first)
            XCTAssertEqual(rect.minX, after.bounds.minX, accuracy: Integration.geometryAccuracy,
                           "the rectangle moves with the scrolled cell")
            XCTAssertEqual(rect.maxX, after.clip.maxX, accuracy: Integration.geometryAccuracy,
                           "the rectangle is clipped to the table viewport")
            XCTAssertLessThan(rect.maxX, after.bounds.maxX)
        }
    }

    func testRightToLeftTableMirrorsTheRemoteRectangle() throws {
        try withTable(Integration.gridDocument) { fixture in
            fixture.view.tableDirection = .rightToLeft
            fixture.expo.layoutIfNeeded()
            let positions = try fixture.positions()
            let first = positions[Integration.gridFirst]
            try fixture.setPeers([Peer(clientId: Integration.firstPeer, color: Integration.firstPeerColor,
                                       anchor: first, head: first, cellRectangle: (first, first))])

            let firstCell = try fixture.presentedCell(first)
            let secondCell = try fixture.presentedCell(positions[Integration.gridSecond])
            XCTAssertEqual(firstCell.surface.direction, .rightToLeft)
            XCTAssertGreaterThan(firstCell.bounds.minX, secondCell.bounds.minX, "the first column sits on the right")
            let rect = try XCTUnwrap(try fixture.remoteRects(
                try XCTUnwrap(fixture.drawing.remoteTableCellSelections.first)
            ).first)
            XCTAssertEqual(rect, firstCell.bounds.intersection(firstCell.clip))
        }
    }

    func testOwnerRebindingReresolvesTheRectangleAgainstTheNewOwner() throws {
        try withTable(Integration.gridDocument) { fixture in
            let first = try fixture.positions()[Integration.gridFirst]
            try fixture.setPeers([Peer(clientId: Integration.firstPeer, color: Integration.firstPeerColor,
                                       anchor: first, head: first, cellRectangle: (first, first))])
            XCTAssertEqual(fixture.drawing.remoteTableCellSelections.count, 1)

            fixture.expo.setEditorId(0)
            XCTAssertTrue(fixture.drawing.remoteTableCellSelections.isEmpty, "an unbound view draws no presence")

            fixture.expo.setEditorId(fixture.editorId)
            fixture.expo.layoutIfNeeded()
            let restored = try XCTUnwrap(fixture.drawing.remoteTableCellSelections.first)
            XCTAssertEqual(restored.sourceIndices, [Integration.gridFirst])
            XCTAssertEqual(try fixture.remoteRects(restored).count, 1)

            let otherEditorId = makeV2Editor(configJson: TableInputTestSchema.tableConfig)
            defer { destroyV2Editor(id: otherEditorId) }
            let other = try XCTUnwrap(EditorV2Registry.adapter(forLegacyId: otherEditorId))
            _ = try XCTUnwrap(other.setContentJson(Integration.shiftedGridDocument))
            fixture.expo.setEditorId(otherEditorId)
            XCTAssertTrue(fixture.view.textView.applyUpdateJSON(try XCTUnwrap(other.refreshFromRustState(mirrorSelection: nil))))
            fixture.expo.layoutIfNeeded()
            let otherOpenings = try XCTUnwrap(other.tableRecordsForTesting.values.first?["cells"] as? [[String: Any]])
                .compactMap { EditorV2Adapter.uint32Field($0, "sourcePos") }
            XCTAssertFalse(otherOpenings.contains(first), "the fixture must move the openings")
            XCTAssertTrue(fixture.drawing.remoteTableCellSelections.isEmpty,
                          "the old opening addresses no cell of the new owner")
            XCTAssertFalse(fixture.view.remoteSelectionOverlaySubviewsForTesting().isEmpty)
            fixture.expo.setEditorId(fixture.editorId)
        }
    }

    func testRemoteRectangleIsPaintedBehindLocalHandlesAndTheActiveCellInput() throws {
        try withTable(Integration.gridDocument) { fixture in
            let positions = try fixture.positions()
            let first = positions[Integration.gridFirst]
            try fixture.view.textView.selectTableCells(adapter: fixture.adapter, anchor: first, head: first)
            fixture.expo.layoutIfNeeded()
            let handles = fixture.drawing.selectionHandles()
            XCTAssertEqual(handles.count, 2)
            let cell = try fixture.presentedCell(first)
            let interior = CGPoint(x: cell.bounds.midX, y: cell.bounds.maxY - Integration.pixelInsetFromCellBottom)
            let localOnly = try fixture.renderDrawing()

            try fixture.setPeers([Peer(clientId: Integration.firstPeer, color: Integration.firstPeerColor,
                                       anchor: first, head: first, cellRectangle: (first, first))])
            XCTAssertEqual(fixture.drawing.remoteTableCellSelections.count, 1)
            let withRemote = try fixture.renderDrawing()

            XCTAssertNotEqual(try pixel(withRemote, at: interior, in: fixture.drawing),
                              try pixel(localOnly, at: interior, in: fixture.drawing),
                              "the remote fill must reach the cell")
            for handle in handles {
                XCTAssertEqual(try pixel(withRemote, at: handle.center, in: fixture.drawing),
                               try pixel(localOnly, at: handle.center, in: fixture.drawing),
                               "the \(handle.role) handle must cover the remote fill")
            }

            XCTAssertTrue(fixture.view.bindTableCell(tableID: fixture.tableID, cellIndex: UInt32(Integration.gridFirst),
                                                     contentRect: .zero))
            let inputClip = try XCTUnwrap(fixture.view.activeTextInput.superview)
            let drawingIndex = try XCTUnwrap(fixture.surface.subviews.firstIndex(of: fixture.drawing))
            let inputIndex = try XCTUnwrap(fixture.surface.subviews.firstIndex(of: inputClip))
            XCTAssertGreaterThan(inputIndex, drawingIndex, "the active cell input sits above the painted rectangle")
            XCTAssertFalse(fixture.drawing.remoteTableCellSelections.isEmpty)
        }
    }

    func testKeyboardCompositionClipboardAndMenuEachMutateOnceAndNeverSelectSyntheticSlots() throws {
        UIPasteboard.general.items = []
        defer { UIPasteboard.general.items = [] }
        try withTable(Integration.irregularDocument) { fixture in
            var positions = try fixture.positions()
            let startRevision = fixture.adapter.baseDocumentRevision

            XCTAssertTrue(fixture.view.bindTableCell(tableID: fixture.tableID, cellIndex: UInt32(Integration.tallCell),
                                                     contentRect: .zero))
            XCTAssertTrue(fixture.view.activeTextInput.becomeFirstResponder())
            for expected in [Integration.wideCell, Integration.laterCell] {
                try pressTab(fixture)
                XCTAssertEqual(fixture.activeCellPosition(), positions[expected], "Tab must reach cell \(expected)")
                XCTAssertTrue(fixture.view.activeTextInput.isFirstResponder)
            }
            try pressTab(fixture, modifiers: [.shift])
            XCTAssertEqual(fixture.activeCellPosition(), positions[Integration.wideCell])
            XCTAssertEqual(fixture.adapter.baseDocumentRevision, startRevision, "focus moves never mutate")

            placeCaretAtCellStart(fixture)
            let composingInput = fixture.view.activeTextInput
            composingInput.setMarkedText(Integration.compositionText, selectedRange: NSRange(location: 1, length: 0))
            XCTAssertTrue(composingInput.isComposing)
            XCTAssertEqual(fixture.adapter.baseDocumentRevision, startRevision, "marked text is not a mutation")
            composingInput.unmarkText()
            let composedRevision = fixture.adapter.baseDocumentRevision
            XCTAssertEqual(composedRevision, startRevision + 1, "a committed composition is exactly one mutation")
            let composed = try XCTUnwrap(fixture.adapter.documentJson())
            XCTAssertTrue(composed.contains(textNode(Integration.compositionText + Integration.wideText)), composed)

            positions = try fixture.positions()
            let wide = positions[Integration.wideCell]
            let later = positions[Integration.laterCell]
            try fixture.view.textView.selectTableCells(adapter: fixture.adapter, anchor: wide, head: later)
            let root = fixture.view.textView
            XCTAssertTrue(root.becomeFirstResponder())
            fixture.expo.layoutIfNeeded()
            let wideCell = try fixture.presentedCell(wide)
            XCTAssertFalse(wideCell.surface.syntheticRegions.isEmpty, "the irregular fixture must project a synthetic gap")
            let laterCell = try fixture.presentedCell(later)
            let gap = CGRect(x: laterCell.bounds.maxX, y: laterCell.bounds.minY,
                             width: wideCell.bounds.maxX - laterCell.bounds.maxX, height: laterCell.bounds.height)
                .insetBy(dx: 1, dy: 1)
            XCTAssertFalse(gap.isEmpty, "the gap sits after the last real cell of the second row")
            let rectangle: Set<Int> = [Integration.wideCell, Integration.laterCell]
            XCTAssertEqual(fixture.drawing.selectedTableCellSourceIndices[fixture.tableID], rectangle)
            let selectedRects = try XCTUnwrap(fixture.drawing.selectedTableCellRects(tableID: fixture.tableID))
            XCTAssertEqual(selectedRects.count, rectangle.count)
            XCTAssertFalse(selectedRects.contains { $0.intersects(gap) }, "the gap slot is never selected")

            try fixture.setPeers([Peer(clientId: Integration.firstPeer, color: Integration.firstPeerColor,
                                       anchor: wide, head: later, cellRectangle: (wide, later))])
            let remote = try XCTUnwrap(fixture.drawing.remoteTableCellSelections.first)
            XCTAssertEqual(remote.sourceIndices, rectangle, "presence shares the local effective rectangle")
            XCTAssertFalse(try fixture.remoteRects(remote).contains { $0.intersects(gap) })

            root.copy(nil)
            XCTAssertEqual(UIPasteboard.general.string, Integration.irregularRectangleTSV)
            XCTAssertEqual(fixture.adapter.baseDocumentRevision, composedRevision, "copy never mutates")

            let tapPoint = fixture.drawing.convert(CGPoint(x: laterCell.bounds.midX, y: laterCell.bounds.midY),
                                                   to: fixture.surface)
            fixture.view.tapTableCell(at: tapPoint, touchedAt: ProcessInfo.processInfo.systemUptime)
            XCTAssertTrue(fixture.surface.isCellEditMenuVisible, "a tap inside the selection opens the cell menu")
            XCTAssertEqual(TableCellEditMenu.commands(in: standardMenuCommands(), performableBy: root).map(\.action),
                           Integration.cellMenuItems)
            let beforeCut = try fixture.documentObject()

            XCTAssertTrue(UIApplication.shared.sendAction(Integration.cut, to: nil, from: nil, for: nil))

            XCTAssertEqual(fixture.adapter.baseDocumentRevision, composedRevision + 1, "cut is exactly one mutation")
            let cutJSON = try XCTUnwrap(fixture.adapter.documentJson())
            XCTAssertFalse(cutJSON.contains(textNode(Integration.compositionText + Integration.wideText))
                           || cutJSON.contains(textNode(Integration.laterText)), cutJSON)
            XCTAssertTrue(cutJSON.contains(textNode(Integration.tallText)), cutJSON)
            XCTAssertEqual(UIPasteboard.general.string, Integration.irregularRectangleTSV)
            XCTAssertTrue(root.authoritativeCellSelectionActive, "cut keeps the cell selection")
            let cut = try fixture.documentObject()

            XCTAssertTrue(UIApplication.shared.sendAction(Integration.paste, to: nil, from: nil, for: nil))

            XCTAssertEqual(fixture.adapter.baseDocumentRevision, composedRevision + 1,
                           "a paste the planner refuses over the gap is no mutation at all")
            XCTAssertEqual(try fixture.documentObject(), cut)
            XCTAssertTrue(root.applyUpdateJSON(try XCTUnwrap(fixture.adapter.undo())))
            XCTAssertEqual(try fixture.documentObject(), beforeCut, "one undo restores the whole cut")
            XCTAssertEqual(fixture.adapter.historyFlags()?.canUndo, true, "the composition entry remains")
            XCTAssertEqual(fixture.drawing.selectedTableCellSourceIndices[fixture.tableID], rectangle)
        }
    }

    func testRemoteTableDeletionUnderAnOpenCellMenuDropsTheSelectionAndRectangleButKeepsTheCursor() throws {
        try withTable(Integration.irregularDocument) { fixture in
            let positions = try fixture.positions()
            try fixture.view.textView.selectTableCells(adapter: fixture.adapter,
                                                       anchor: positions[Integration.tallCell], head: positions[Integration.wideCell])
            XCTAssertTrue(fixture.view.textView.becomeFirstResponder())
            fixture.expo.layoutIfNeeded()
            fixture.surface.presentCellEditMenu()
            XCTAssertTrue(fixture.surface.isCellEditMenuVisible)
            let staleRectangle = (positions[Integration.wideCell], positions[Integration.laterCell])
            try fixture.setPeers([Peer(clientId: Integration.firstPeer, color: Integration.firstPeerColor,
                                       anchor: staleRectangle.0, head: staleRectangle.1, cellRectangle: staleRectangle)])
            XCTAssertEqual(fixture.drawing.remoteTableCellSelections.count, 1)
            let tablePos = try fixture.tablePos()
            let revision = fixture.adapter.baseDocumentRevision

            try fixture.applyRemoteCommand(["type": Integration.deleteTable, "tablePos": Int(tablePos)])
            fixture.deliverRemoteCommit()
            try fixture.setPeers([Peer(clientId: Integration.firstPeer, color: Integration.firstPeerColor,
                                       anchor: tablePos, head: tablePos, cellRectangle: staleRectangle)])

            XCTAssertTrue(fixture.adapter.tableRecordsForTesting.isEmpty, "the remote peer deleted the table")
            XCTAssertEqual(fixture.adapter.baseDocumentRevision, revision + 1, "only the remote change was applied")
            XCTAssertFalse(fixture.view.textView.authoritativeCellSelectionActive)
            XCTAssertTrue(fixture.drawing.selectedTableCellSourceIndices.isEmpty)
            XCTAssertFalse(fixture.surface.isCellEditMenuVisible, "the menu closes with its selection")
            XCTAssertTrue(fixture.drawing.remoteTableCellSelections.isEmpty, "the dead rectangle is removed")
            XCTAssertFalse(fixture.view.remoteSelectionOverlaySubviewsForTesting().isEmpty,
                           "the peer keeps its ordinary cursor")
        }
    }

    func testRemoteTableDeletionDuringCellCompositionCancelsItWithoutAMutation() throws {
        try withTable(Integration.irregularDocument) { fixture in
            let input = try composeStaleText(in: Integration.tallCell, fixture)
            let revision = fixture.adapter.baseDocumentRevision

            try fixture.applyRemoteCommand(["type": Integration.deleteTable, "tablePos": Int(try fixture.tablePos())])
            let remoteDocument = try XCTUnwrap(fixture.adapter.documentJson())
            fixture.deliverRemoteCommit()
            input.unmarkText()

            assertCancelledComposition(input, landedOn: remoteDocument, revision: revision, fixture)
            XCTAssertTrue(fixture.adapter.tableRecordsForTesting.isEmpty)
        }
    }

    func testRemoteTableDeletionDuringARangeReplacingCellCompositionCancelsItWithoutAMutation() throws {
        try withTable(Integration.irregularDocument) { fixture in
            let input = try composeStaleText(in: Integration.tallCell, replacing: Integration.tallTextHead, fixture)
            let revision = fixture.adapter.baseDocumentRevision

            try fixture.applyRemoteCommand(["type": Integration.deleteTable, "tablePos": Int(try fixture.tablePos())])
            let remoteDocument = try XCTUnwrap(fixture.adapter.documentJson())
            fixture.deliverRemoteCommit()
            input.unmarkText()

            assertCancelledComposition(input, landedOn: remoteDocument, revision: revision, fixture)
            XCTAssertTrue(fixture.adapter.tableRecordsForTesting.isEmpty)
        }
    }

    func testRemoteRowDeletionDuringCellCompositionCancelsItWithoutAMutation() throws {
        try withTable(Integration.gridDocument) { fixture in
            let lastRowCell = try fixture.positions()[Integration.gridLast]
            let input = try composeStaleText(in: Integration.gridLast, fixture)
            let revision = fixture.adapter.baseDocumentRevision

            try fixture.applyRemoteCellSelection(anchor: lastRowCell, head: lastRowCell)
            try fixture.applyRemoteCommand(["type": Integration.deleteTableRows])
            let remoteDocument = try XCTUnwrap(fixture.adapter.documentJson())
            XCTAssertFalse(remoteDocument.contains(textNode(Integration.gridLastText)), "the remote peer removed the composing cell's row")
            fixture.deliverRemoteCommit()
            input.unmarkText()

            assertCancelledComposition(input, landedOn: remoteDocument, revision: revision, fixture)
            XCTAssertEqual(try fixture.positions().count, Integration.gridSecond + 1, "the first row survives")
        }
    }

    func testRemoteEditElsewhereDuringCellCompositionKeepsComposingInTheSameCell() throws {
        try withTable(Integration.irregularDocument) { fixture in
            let input = try composeStaleText(in: Integration.tallCell, fixture)
            let boundCell = fixture.activeCellPosition()
            let revision = fixture.adapter.baseDocumentRevision

            try fixture.applyRemoteTextSelection(at: Integration.remoteProseScalar)
            try fixture.applyRemoteCommand(["type": Integration.insertText, "text": Integration.remoteProseText])
            fixture.deliverRemoteCommit()

            XCTAssertTrue(input.isComposing, "a remote edit outside the cell must not end the composition")
            XCTAssertTrue(fixture.view.activeTextInput === input)
            XCTAssertEqual(fixture.activeCellPosition(), boundCell)
            input.unmarkText()

            let document = try XCTUnwrap(fixture.adapter.documentJson())
            XCTAssertTrue(document.contains(textNode(Integration.remoteProseText + Integration.proseBeforeTableText)), document)
            XCTAssertTrue(document.contains(textNode(Integration.staleCompositionText + Integration.tallText)),
                          "the composition lands in its moved cell: \(document)")
            XCTAssertEqual(fixture.adapter.baseDocumentRevision, revision + 2,
                           "one remote edit and one composition commit")
            XCTAssertFalse(input.isComposing)
        }
    }

    func testRemoteHeaderToggleOfTheComposingCellKeepsComposingInThatCell() throws {
        try withTable(Integration.irregularDocument) { fixture in
            let tall = try fixture.positions()[Integration.tallCell]
            let input = try composeStaleText(in: Integration.tallCell, fixture)
            let revision = fixture.adapter.baseDocumentRevision

            try fixture.applyRemoteCellSelection(anchor: tall, head: tall)
            try fixture.applyRemoteCommand(["type": Integration.toggleTableHeader,
                                            "target": Integration.headerTargetCell])
            let remoteDocument = try XCTUnwrap(fixture.adapter.documentJson())
            XCTAssertTrue(remoteDocument.contains(Integration.headerCellNode), "the remote peer retyped the cell")
            fixture.deliverRemoteCommit()
            XCTAssertTrue(input.isComposing)
            input.unmarkText()

            let document = try XCTUnwrap(fixture.adapter.documentJson())
            XCTAssertTrue(document.contains(textNode(Integration.staleCompositionText + Integration.tallText)),
                          "the composition lands in the retyped cell: \(document)")
            XCTAssertTrue(document.contains(Integration.headerCellNode), document)
            XCTAssertEqual(fixture.adapter.baseDocumentRevision, revision + 2,
                           "one remote retype and one composition commit")
            XCTAssertFalse(input.isComposing)
            XCTAssertTrue(fixture.view.activeTextInput === input, "the retyped cell keeps its input")
            XCTAssertEqual(fixture.activeCellPosition(), tall)
        }
    }

    private func composeStaleText(in cellIndex: Int, replacing replaced: NSRange = Integration.cellStartCaret,
                                  _ fixture: Fixture) throws -> EditorTextView {
        XCTAssertTrue(fixture.view.bindTableCell(tableID: fixture.tableID, cellIndex: UInt32(cellIndex),
                                                 contentRect: .zero))
        let input = fixture.view.activeTextInput
        XCTAssertFalse(input === fixture.view.textView, "a cell input must own the composition")
        XCTAssertTrue(input.becomeFirstResponder())
        input.selectedRange = replaced
        input.textViewDidChangeSelection(input)
        input.setMarkedText(Integration.staleCompositionText, selectedRange: NSRange(location: 1, length: 0))
        XCTAssertTrue(input.isComposing)
        return input
    }

    private func assertCancelledComposition(_ input: EditorTextView, landedOn remoteDocument: String,
                                            revision: UInt64, _ fixture: Fixture) {
        XCTAssertEqual(fixture.adapter.documentJson(), remoteDocument, "the stale composition must not land anywhere")
        XCTAssertEqual(fixture.adapter.baseDocumentRevision, revision + 1, "only the remote change was applied")
        XCTAssertNil(input.markedTextRange)
        XCTAssertFalse(input.isComposing)
        XCTAssertFalse(input.textStorage.string.contains(Integration.staleCompositionText),
                       "the marked text is removed from the released input: \(input.textStorage.string)")
        XCTAssertTrue(fixture.view.activeTextInput === fixture.view.textView, "the dead cell input is released")
        XCTAssertFalse(fixture.view.textView.textStorage.string.contains(Integration.staleCompositionText),
                       fixture.view.textView.textStorage.string)
    }

    private func withTable(_ document: String, size: CGSize = Integration.editorSize,
                           _ body: (Fixture) throws -> Void) throws {
        let editorId = makeV2Editor(configJson: TableInputTestSchema.tableConfig)
        defer { destroyV2Editor(id: editorId) }
        let adapter = try XCTUnwrap(EditorV2Registry.adapter(forLegacyId: editorId))
        let window = UIWindow(frame: CGRect(origin: .zero, size: size))
        let expo = NativeEditorExpoView()
        expo.frame = window.bounds
        window.addSubview(expo)
        window.makeKeyAndVisible()
        defer {
            expo.setEditorId(0)
            window.isHidden = true
        }
        expo.setEditorId(editorId)
        XCTAssertTrue(expo.richTextView.textView.applyUpdateJSON(try XCTUnwrap(adapter.setContentJson(document))))
        expo.layoutIfNeeded()
        let tableID = try adapter.editableTableID()
        let surface = try XCTUnwrap(expo.richTextView.subviews.compactMap { $0 as? EditorTableSurface }.first)
        let drawing = try XCTUnwrap(surface.subviews.compactMap { $0 as? PreparedProseDrawingView }.first)
        try body(Fixture(expo: expo, adapter: adapter, editorId: editorId, tableID: tableID,
                         surface: surface, drawing: drawing))
    }

    private func rgba(_ color: UIColor) -> [CGFloat] {
        var red: CGFloat = 0
        var green: CGFloat = 0
        var blue: CGFloat = 0
        var alpha: CGFloat = 0
        XCTAssertTrue(color.getRed(&red, green: &green, blue: &blue, alpha: &alpha))
        return [red, green, blue, alpha]
    }

    private func expectedPeerFill(_ opaque: UIColor) -> [CGFloat] {
        rgba(opaque.withAlphaComponent(RemoteSelectionOverlayView.selectionAlpha))
    }

    private func pixel(_ image: CGImage, at point: CGPoint, in drawing: PreparedProseDrawingView) throws -> [UInt8] {
        var pixels = [UInt8](repeating: 0, count: image.width * image.height * 4)
        let bitmap = try XCTUnwrap(CGContext(
            data: &pixels, width: image.width, height: image.height, bitsPerComponent: 8,
            bytesPerRow: image.width * 4, space: CGColorSpaceCreateDeviceRGB(),
            bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue | CGBitmapInfo.byteOrder32Big.rawValue
        ))
        bitmap.draw(image, in: CGRect(x: 0, y: 0, width: image.width, height: image.height))
        let x = Int(point.x - drawing.bounds.minX)
        let y = Int(point.y - drawing.bounds.minY)
        let offset = (y * image.width + x) * 4
        return Array(pixels[offset..<(offset + 4)])
    }

    private func pressTab(_ fixture: Fixture, modifiers: UIKeyModifierFlags = []) throws {
        let input = fixture.view.activeTextInput
        let command = try XCTUnwrap(input.keyCommands?.first { $0.input == "\t" && $0.modifierFlags == modifiers })
        _ = input.perform(command.action)
    }

    private func placeCaretAtCellStart(_ fixture: Fixture) {
        let input = fixture.view.activeTextInput
        input.selectedRange = Integration.cellStartCaret
        input.textViewDidChangeSelection(input)
    }

    private func textNode(_ text: String) -> String {
        #""text":"\#(text)""#
    }

    private func standardMenuCommands() -> [UICommand] {
        Integration.cellMenuItems.map { UICommand(title: NSStringFromSelector($0), action: $0) }
    }
}
