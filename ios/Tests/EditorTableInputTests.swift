import CoreText
import XCTest

final class EditorTableInputTests: XCTestCase {
    final class UpdateSpy: EditorTextViewDelegate {
        var updates: [String] = []
        var selections: [[UInt32]] = []
        var onUpdate: (() -> Void)?

        func editorTextView(_ textView: EditorTextView, selectionDidChange anchor: UInt32, head: UInt32) {
            selections.append([anchor, head])
        }
        func editorTextView(_ textView: EditorTextView, didReceiveUpdate updateJSON: String) {
            updates.append(updateJSON)
            onUpdate?()
        }
    }

    private let tableConfig = TableInputTestSchema.tableConfig
    private static let selectRowsKey = "selectRows"
    private let listTableConfig = TableInputTestSchema.listTableConfig
    private let wideTwoCellDocument = #"{"type":"doc","content":[{"type":"paragraph","content":[{"type":"text","text":"before"}]},{"type":"table","content":[{"type":"table_row","content":[{"type":"table_cell","attrs":{"colwidth":[500]},"content":[{"type":"paragraph","content":[{"type":"text","text":"one"}]}]},{"type":"table_cell","attrs":{"colwidth":[500]},"content":[{"type":"paragraph","content":[{"type":"text","text":"two"}]}]}]}]}]}"#
    private let fourCellDocument = #"{"type":"doc","content":[{"type":"table","content":[{"type":"table_row","content":[{"type":"table_cell","content":[{"type":"paragraph","content":[{"type":"text","text":"one"}]}]},{"type":"table_cell","content":[{"type":"paragraph","content":[{"type":"text","text":"two"}]}]}]},{"type":"table_row","content":[{"type":"table_cell","content":[{"type":"paragraph","content":[{"type":"text","text":"three"}]}]},{"type":"table_cell","content":[{"type":"paragraph","content":[{"type":"text","text":"four"}]}]}]}]}]}"#
    private let proseBeforeTableDocument = #"{"type":"doc","content":[{"type":"paragraph","content":[{"type":"text","text":"intro"}]},{"type":"table","content":[{"type":"table_row","content":[{"type":"table_cell","content":[{"type":"paragraph","content":[{"type":"text","text":"one"}]}]},{"type":"table_cell","content":[{"type":"paragraph","content":[{"type":"text","text":"two"}]}]}]},{"type":"table_row","content":[{"type":"table_cell","content":[{"type":"paragraph","content":[{"type":"text","text":"three"}]}]},{"type":"table_cell","content":[{"type":"paragraph","content":[{"type":"text","text":"four"}]}]}]}]}]}"#
    private let fixedWidthFourCellDocument = #"{"type":"doc","content":[{"type":"table","content":[{"type":"table_row","content":[{"type":"table_cell","attrs":{"colwidth":[120]},"content":[{"type":"paragraph","content":[{"type":"text","text":"first"}]}]},{"type":"table_cell","attrs":{"colwidth":[120]},"content":[{"type":"paragraph","content":[{"type":"text","text":"second"}]}]}]},{"type":"table_row","content":[{"type":"table_cell","attrs":{"colwidth":[120]},"content":[{"type":"paragraph","content":[{"type":"text","text":"third"}]}]},{"type":"table_cell","attrs":{"colwidth":[120]},"content":[{"type":"paragraph","content":[{"type":"text","text":"fourth"}]}]}]}]}]}"#
    private let proseThenFixedWidthTableDocument = #"{"type":"doc","content":[{"type":"paragraph","content":[{"type":"text","text":"before"}]},{"type":"table","content":[{"type":"table_row","content":[{"type":"table_cell","attrs":{"colwidth":[120]},"content":[{"type":"paragraph","content":[{"type":"text","text":"first"}]}]},{"type":"table_cell","attrs":{"colwidth":[120]},"content":[{"type":"paragraph","content":[{"type":"text","text":"second"}]}]}]}]}]}"#

    private func tallTableDocument(rowCount: Int) throws -> String {
        let rows: [[String: Any]] = (0..<rowCount).map { index in
            ["type": "table_row", "content": [[
                "type": "table_cell", "content": [[
                    "type": "paragraph", "content": [["type": "text", "text": "row \(index)"]]
                ]]
            ]]]
        }
        let data = try JSONSerialization.data(withJSONObject: [
            "type": "doc", "content": [["type": "table", "content": rows]]
        ])
        return try XCTUnwrap(String(data: data, encoding: .utf8))
    }

    struct MountedTableFixture {
        let view: RichTextEditorView
        let adapter: EditorV2Adapter
        let tableID: String
        let positions: [UInt32]
        let surface: EditorTableSurface
        let drawing: PreparedProseDrawingView
        let updates: UpdateSpy

        func hostPoint(for role: TableSelectionHandleRole) throws -> CGPoint {
            let handle = try XCTUnwrap(drawing.selectionHandles().first { $0.role == role })
            return drawing.convert(handle.center, to: view)
        }

        func hostPoint(inCell index: Int) throws -> CGPoint {
            let cell = try XCTUnwrap(drawing.mountedTablePresentation()?.cells.first {
                $0.surface.identity == tableID && $0.sourcePosition == Int(positions[index])
            })
            let visible = cell.bounds.intersection(cell.clip).intersection(drawing.bounds)
            XCTAssertFalse(visible.isEmpty)
            return drawing.convert(CGPoint(x: visible.midX, y: visible.midY), to: view)
        }

        func selection() throws -> (UInt32, UInt32) {
            let raw = try XCTUnwrap(adapter.cachedAtomicRenderJSON)
            let object = try XCTUnwrap(JSONSerialization.jsonObject(with: Data(raw.utf8)) as? [String: Any])
            return try XCTUnwrap(EditorCellSelection.endpointPositions(object["selection"] as Any))
        }

        func engineSelection() throws -> (UInt32, UInt32) {
            let result = editorV2RenderUpdate(editorId: adapter.editorId,
                                              mirrorScalarAnchor: nil, mirrorScalarHead: nil)
            XCTAssertNil(result.error)
            let raw = try XCTUnwrap(result.value)
            let object = try XCTUnwrap(JSONSerialization.jsonObject(with: Data(raw.utf8)) as? [String: Any])
            return try XCTUnwrap(EditorCellSelection.endpointPositions(object["selection"] as Any))
        }

        func publishedSelection() throws -> (UInt32, UInt32) {
            let raw = try XCTUnwrap(updates.updates.last)
            let object = try XCTUnwrap(JSONSerialization.jsonObject(with: Data(raw.utf8)) as? [String: Any])
            return try XCTUnwrap(EditorCellSelection.endpointPositions(object["selection"] as Any))
        }

        func publishedSelectionType() throws -> String {
            let raw = try XCTUnwrap(updates.updates.last)
            let object = try XCTUnwrap(JSONSerialization.jsonObject(with: Data(raw.utf8)) as? [String: Any])
            return try XCTUnwrap((object["selection"] as? [String: Any])?["type"] as? String)
        }

        func textSelectionScalars() throws -> (UInt32, UInt32) {
            let selection = try XCTUnwrap(adapter.cachedAtomicRenderSelection())
            XCTAssertEqual(selection["type"] as? String, "text")
            return (try XCTUnwrap(EditorV2Adapter.uint32Field(selection, "anchorScalar")),
                    try XCTUnwrap(EditorV2Adapter.uint32Field(selection, "headScalar")))
        }

        func presentedCell(_ index: Int) throws -> ViewerTablePresentedCell {
            try XCTUnwrap(drawing.mountedTablePresentation()?.cells.first {
                $0.surface.identity == tableID && $0.sourcePosition == Int(positions[index])
            }, "cell \(index) is not mounted")
        }

        func trailingEdgeHostPoint(cellIndex index: Int) throws -> CGPoint {
            let cell = try presentedCell(index)
            let x = cell.surface.direction == .rightToLeft ? cell.bounds.minX : cell.bounds.maxX
            return drawing.convert(CGPoint(x: x, y: cell.bounds.midY), to: view)
        }

        func documentObject() throws -> NSDictionary {
            let json = try XCTUnwrap(adapter.documentJson())
            return try XCTUnwrap(JSONSerialization.jsonObject(with: Data(json.utf8)) as? NSDictionary)
        }

        func columnWidths(row: Int) throws -> [[Int]?] {
            let json = try XCTUnwrap(adapter.documentJson())
            let root = try XCTUnwrap(JSONSerialization.jsonObject(with: Data(json.utf8)) as? [String: Any])
            let table = try XCTUnwrap((root["content"] as? [[String: Any]])?.first { $0["type"] as? String == "table" })
            let rows = try XCTUnwrap(table["content"] as? [[String: Any]])
            let cells = try XCTUnwrap(rows[row]["content"] as? [[String: Any]])
            return cells.map { ($0["attrs"] as? [String: Any])?["colwidth"] as? [Int] }
        }
    }

    private func withMountedHandles(
        document: String, configJSON: String? = nil, theme: EditorTheme? = nil,
        size: CGSize = CGSize(width: 360, height: 240), anchorIndex: Int, headIndex: Int,
        roomAwareness: ((String, String) -> FfiJsonResult)? = nil,
        _ body: (MountedTableFixture) throws -> Void
    ) throws {
        try withMountedTable(document: document, configJSON: configJSON, theme: theme, size: size,
                             cellSelection: (anchorIndex, headIndex), roomAwareness: roomAwareness, body)
    }

    func withMountedTable(
        document: String, configJSON: String? = nil, theme: EditorTheme? = nil,
        size: CGSize = CGSize(width: 360, height: 240), cellSelection: (anchor: Int, head: Int)?,
        roomAwareness: ((String, String) -> FfiJsonResult)? = nil,
        _ body: (MountedTableFixture) throws -> Void
    ) throws {
        let editorId = makeV2Editor(configJson: configJSON ?? tableConfig, roomAwareness: roomAwareness)
        defer { destroyV2Editor(id: editorId) }
        let adapter = try XCTUnwrap(EditorV2Registry.adapter(forLegacyId: editorId))
        let window = UIWindow(frame: CGRect(origin: .zero, size: size))
        let view = RichTextEditorView(frame: window.bounds)
        window.addSubview(view)
        window.makeKeyAndVisible()
        defer { window.isHidden = true }
        view.bindEditor(id: editorId, initialUpdateJSON: try XCTUnwrap(adapter.initialUpdateJSON()))
        if let theme { XCTAssertTrue(view.applyTheme(theme)) }
        XCTAssertTrue(view.textView.applyUpdateJSON(try XCTUnwrap(adapter.setContentJson(document))))
        let tableID = try adapter.editableTableID()
        let positions = try adapter.tableCellPositions()
        if let cellSelection {
            try view.textView.selectTableCells(adapter: adapter,
                                               anchor: positions[cellSelection.anchor], head: positions[cellSelection.head])
        }
        view.layoutIfNeeded()
        let surface = try XCTUnwrap(view.subviews.compactMap { $0 as? EditorTableSurface }.first)
        let drawing = try XCTUnwrap(surface.subviews.compactMap { $0 as? PreparedProseDrawingView }.first)
        let updates = UpdateSpy()
        view.textView.editorDelegate = updates
        try body(MountedTableFixture(view: view, adapter: adapter, tableID: tableID,
                                      positions: positions, surface: surface, drawing: drawing,
                                      updates: updates))
    }

    func testMountedHeadDragPublishesExactCellSelectionWithoutDocumentMutation() throws {
        try withMountedHandles(document: fourCellDocument, anchorIndex: 0, headIndex: 0) { fixture in
            let beforeDocument = try XCTUnwrap(fixture.adapter.documentJson())
            let beforeRevision = fixture.adapter.baseDocumentRevision
            let beforeHistory = try XCTUnwrap(fixture.adapter.cachedHistoryState)
            XCTAssertEqual(fixture.drawing.selectionHandles().count, 2)
            XCTAssertTrue(fixture.surface.beginHandleDrag(at: try fixture.hostPoint(for: .head)))
            fixture.surface.updateHandleDrag(at: try fixture.hostPoint(inCell: 3))
            fixture.surface.updateHandleDrag(at: try fixture.hostPoint(inCell: 1))
            XCTAssertEqual(try fixture.selection().0, fixture.positions[0])
            XCTAssertEqual(try fixture.selection().1, fixture.positions[1])
            XCTAssertEqual(try fixture.engineSelection().1, fixture.positions[1])
            XCTAssertEqual(try fixture.publishedSelection().1, fixture.positions[1])
            XCTAssertEqual(fixture.drawing.selectedTableCellEndpoints?.head, fixture.positions[1])
            XCTAssertEqual(try XCTUnwrap(fixture.adapter.documentJson()), beforeDocument)
            XCTAssertEqual(fixture.adapter.baseDocumentRevision, beforeRevision)
            XCTAssertEqual(fixture.adapter.cachedHistoryState?.canUndo, beforeHistory.canUndo)
            XCTAssertEqual(fixture.adapter.cachedHistoryState?.canRedo, beforeHistory.canRedo)
            fixture.surface.cancelHandleDrag()
        }
    }

    private final class PresenceLog {
        var selections: [[String: Any]] = []

        func record(_ json: String) -> FfiJsonResult {
            let object = try? JSONSerialization.jsonObject(with: Data(json.utf8))
            selections.append(object as? [String: Any] ?? [:])
            return FfiJsonResult(value: #"{"outboundChanged":false}"#, error: nil)
        }

        func lastIsCells(_ anchor: UInt32, _ head: UInt32) -> Bool {
            let last = selections.last
            return last?["type"] as? String == "cell"
                && (last?["anchorCell"] as? NSNumber)?.uint32Value == anchor
                && (last?["headCell"] as? NSNumber)?.uint32Value == head
        }
    }

    func testRoomHeadDragPublishesTheDraggedCellsAsPresence() throws {
        let log = PresenceLog()
        try withMountedHandles(document: fourCellDocument, anchorIndex: 0, headIndex: 0,
                               roomAwareness: { _, json in log.record(json) }) { fixture in
            log.selections.removeAll()
            XCTAssertTrue(fixture.surface.beginHandleDrag(at: try fixture.hostPoint(for: .head)))
            fixture.surface.updateHandleDrag(at: try fixture.hostPoint(inCell: 3))
            fixture.surface.cancelHandleDrag()

            XCTAssertTrue(log.lastIsCells(fixture.positions[0], fixture.positions[3]), "\(log.selections)")
        }
    }

    func testRoomNativeSelectionOnlyTableCommandPublishesCellPresence() throws {
        let log = PresenceLog()
        try withMountedTable(document: fourCellDocument, cellSelection: nil,
                             roomAwareness: { _, json in log.record(json) }) { fixture in
            XCTAssertNotNil(fixture.adapter.nativeOwnerId, "the mounted view owns native intents")
            let caret = try XCTUnwrap(fixture.adapter.scalarPosition(forDoc: fixture.positions[0] + 2))
            let revision = fixture.adapter.baseDocumentRevision
            log.selections.removeAll()

            XCTAssertNotNil(fixture.adapter.commandAtSelection(["type": "selectTableRows"], anchor: caret, head: caret))

            XCTAssertEqual(fixture.adapter.baseDocumentRevision, revision)
            XCTAssertTrue(log.lastIsCells(fixture.positions[0], fixture.positions[1]), "\(log.selections)")
        }
    }

    func testHandleDragReportsTheCellRectangleThroughTheSelectionDelegate() throws {
        try withMountedHandles(document: fourCellDocument, anchorIndex: 0, headIndex: 0) { fixture in
            XCTAssertTrue(fixture.surface.beginHandleDrag(at: try fixture.hostPoint(for: .head)))
            fixture.surface.updateHandleDrag(at: try fixture.hostPoint(inCell: 3))
            fixture.surface.cancelHandleDrag()

            XCTAssertEqual(fixture.updates.selections, [[fixture.positions[0], fixture.positions[3]]],
                           "the delegate receives the new cell endpoints in document positions")
            let state = try XCTUnwrap(fixture.adapter.currentStateJSON())
            let object = try XCTUnwrap(JSONSerialization.jsonObject(with: Data(state.utf8)) as? [String: Any])
            let selection = try XCTUnwrap(object["selection"] as? [String: Any])
            XCTAssertEqual(selection["type"] as? String, "cell", "the state published beside the event keeps the rectangle")
            XCTAssertEqual(try XCTUnwrap(EditorCellSelection.endpointPositions(selection)).0, fixture.positions[0])
            XCTAssertEqual(try XCTUnwrap(EditorCellSelection.endpointPositions(selection)).1, fixture.positions[3])
        }
    }

    func testHostTableDirectionMirrorsUndeclaredTablesAndYieldsToDeclaredDirection() throws {
        try withMountedTable(document: fixedWidthFourCellDocument, cellSelection: nil) { fixture in
            XCTAssertEqual(try fixture.presentedCell(0).surface.direction, .leftToRight)
            XCTAssertLessThanOrEqual(try fixture.presentedCell(0).bounds.maxX, try fixture.presentedCell(1).bounds.minX + 0.5)

            fixture.view.tableDirection = .rightToLeft
            fixture.view.layoutIfNeeded()

            XCTAssertEqual(try fixture.presentedCell(0).surface.direction, .rightToLeft)
            XCTAssertLessThanOrEqual(try fixture.presentedCell(1).bounds.maxX, try fixture.presentedCell(0).bounds.minX + 0.5,
                                     "the host direction lays logical column 0 out on the right")
            let first = try fixture.presentedCell(0)
            XCTAssertEqual(fixture.drawing.hitResizeEdge(at: CGPoint(x: first.bounds.minX, y: first.bounds.midY))?.edge.column, 0)

            fixture.view.tableDirection = nil
            fixture.view.layoutIfNeeded()

            XCTAssertLessThanOrEqual(try fixture.presentedCell(0).bounds.maxX, try fixture.presentedCell(1).bounds.minX + 0.5,
                                     "clearing the host direction restores the platform direction")
        }
        let config = tableConfig.replacingOccurrences(
            of: #""tableRole":"table","attrs":{"class":{"default":null}}"#,
            with: #""tableRole":"table","attrs":{"class":{"default":null},"dir":{"default":null}}"#
        )
        let declaredLTR = fixedWidthFourCellDocument.replacingOccurrences(
            of: #"{"type":"table","content""#, with: #"{"type":"table","attrs":{"dir":"ltr"},"content""#
        )
        try withMountedTable(document: declaredLTR, configJSON: config, cellSelection: nil) { fixture in
            fixture.view.tableDirection = .rightToLeft
            fixture.view.layoutIfNeeded()

            XCTAssertEqual(try fixture.presentedCell(0).surface.direction, .leftToRight,
                           "a declared table direction outranks the host direction")
        }
    }

    func testTableDirectionPropReachesTheEditorAndIgnoresUnknownValues() {
        let view = NativeEditorExpoView(appContext: nil)
        XCTAssertNil(view.richTextView.tableDirection)
        view.setTableDirection("rtl")
        XCTAssertEqual(view.richTextView.tableDirection, .rightToLeft)
        view.setTableDirection("ltr")
        XCTAssertEqual(view.richTextView.tableDirection, .leftToRight)
        view.setTableDirection("auto")
        XCTAssertNil(view.richTextView.tableDirection)
        view.setTableDirection("rtl")
        view.setTableDirection(nil)
        XCTAssertNil(view.richTextView.tableDirection)
    }

    func testMountedAnchorDragCrossesHeadAndKeepsSourceRoles() throws {
        try withMountedHandles(document: fourCellDocument, anchorIndex: 0, headIndex: 2) { fixture in
            XCTAssertTrue(fixture.surface.beginHandleDrag(at: try fixture.hostPoint(for: .anchor)))
            fixture.surface.updateHandleDrag(at: try fixture.hostPoint(inCell: 3))
            let selection = try fixture.selection()
            XCTAssertEqual(selection.0, fixture.positions[3])
            XCTAssertEqual(selection.1, fixture.positions[2])
            XCTAssertEqual(fixture.drawing.selectionHandles().first { $0.role == .anchor }?.sourcePosition,
                           fixture.positions[3])
            fixture.surface.cancelHandleDrag()
        }
    }

    func testCancelledOrForeignOwnedDragCannotRetargetSelection() throws {
        try withMountedHandles(document: fourCellDocument, anchorIndex: 0, headIndex: 0) { fixture in
            XCTAssertTrue(fixture.surface.beginHandleDrag(at: try fixture.hostPoint(for: .head)))
            fixture.surface.cancelHandleDrag()
            fixture.surface.updateHandleDrag(at: try fixture.hostPoint(inCell: 3))
            XCTAssertEqual(try fixture.selection().1, fixture.positions[0])
            XCTAssertTrue(fixture.surface.beginHandleDrag(at: try fixture.hostPoint(for: .head)))
            fixture.adapter.releaseNativeBindingOwner(token: try XCTUnwrap(fixture.adapter.nativeOwnerToken))
            fixture.surface.updateHandleDrag(at: try fixture.hostPoint(inCell: 3))
            XCTAssertEqual(try fixture.selection().1, fixture.positions[0])
        }
    }

    func testForeignSelectionAndDocumentResetCancelHeldHandleWithoutRestore() throws {
        try withMountedHandles(document: fourCellDocument, anchorIndex: 0, headIndex: 0) { fixture in
            XCTAssertTrue(fixture.surface.beginHandleDrag(at: try fixture.hostPoint(for: .head)))
            let request = fixture.adapter.callWithEnvelope([
                "selection": ["type": "cell",
                              "anchorCell": ["kind": "document", "offset": Int(fixture.positions[1])],
                              "headCell": ["kind": "document", "offset": Int(fixture.positions[1])]]
            ]) { editorV2SetSelection(editorId: fixture.adapter.editorId, requestJson: $0) }
            XCTAssertNil(request.error)
            XCTAssertTrue(fixture.view.textView.applyUpdateJSON(
                try XCTUnwrap(fixture.adapter.refreshFromRustState(mirrorSelection: nil))
            ))
            fixture.surface.updateHandleDrag(at: try fixture.hostPoint(inCell: 3))
            XCTAssertEqual(try fixture.selection().0, fixture.positions[1])
            XCTAssertEqual(try fixture.selection().1, fixture.positions[1])
            XCTAssertTrue(fixture.surface.beginHandleDrag(at: try fixture.hostPoint(for: .head)))
            let stalePoint = try fixture.hostPoint(inCell: 3)
            XCTAssertTrue(fixture.view.textView.applyUpdateJSON(
                try XCTUnwrap(fixture.adapter.setContentJson(fourCellDocument))
            ))
            fixture.surface.updateHandleDrag(at: stalePoint)
            XCTAssertEqual(fixture.adapter.cachedAtomicRenderDocumentRevision,
                           fixture.adapter.baseDocumentRevision)
            XCTAssertNil(fixture.drawing.selectedTableCellEndpoints)
        }
    }

    func testDetachedViewAndReboundEditorReleaseHeldHandle() throws {
        try withMountedHandles(document: fourCellDocument, anchorIndex: 0, headIndex: 0) { fixture in
            XCTAssertTrue(fixture.surface.beginHandleDrag(at: try fixture.hostPoint(for: .head)))
            let before = try fixture.selection()
            fixture.view.removeFromSuperview()
            fixture.surface.updateHandleDrag(at: CGPoint(x: 300, y: 200))
            XCTAssertEqual(try fixture.selection().0, before.0)
            XCTAssertEqual(try fixture.selection().1, before.1)
        }
    }

    func testRebindingAnAttachedEditorCancelsHeldHandleWithoutTouchingOldEngine() throws {
        try withMountedHandles(document: fourCellDocument, anchorIndex: 0, headIndex: 0) { fixture in
            XCTAssertTrue(fixture.surface.beginHandleDrag(at: try fixture.hostPoint(for: .head)))
            let oldSelection = try fixture.engineSelection()
            let reboundID = makeV2Editor(configJson: tableConfig)
            defer { destroyV2Editor(id: reboundID) }
            let reboundAdapter = try XCTUnwrap(EditorV2Registry.adapter(forLegacyId: reboundID))
            fixture.view.bindEditor(id: reboundID,
                                    initialUpdateJSON: try XCTUnwrap(reboundAdapter.initialUpdateJSON()))
            fixture.surface.updateHandleDrag(at: CGPoint(x: 300, y: 200))
            XCTAssertEqual(try fixture.engineSelection().0, oldSelection.0)
            XCTAssertEqual(try fixture.engineSelection().1, oldSelection.1)
            XCTAssertTrue(fixture.updates.updates.isEmpty)
        }
    }

    func testMountedWideTableOnlyExposesHandlesAtRealVisiblePositions() throws {
        try withMountedHandles(document: wideTwoCellDocument, anchorIndex: 0, headIndex: 1) { fixture in
            XCTAssertEqual(fixture.drawing.selectionHandles().map(\.role), [.anchor])
            let offscreenHead = try XCTUnwrap(fixture.drawing.mountedTablePresentation()?.cells.first {
                $0.surface.identity == fixture.tableID && $0.sourcePosition == Int(fixture.positions[1])
            })
            XCTAssertNil(fixture.drawing.hitSelectionHandle(at: CGPoint(x: offscreenHead.bounds.maxX - 8,
                                                                          y: offscreenHead.bounds.maxY - 8)))
            fixture.drawing.setTableLogicalOffset(350, sourceIdentity: fixture.tableID)
            XCTAssertTrue(fixture.drawing.selectionHandles().isEmpty)
            let table = try XCTUnwrap(fixture.drawing.mountedTablePresentation()?.tables.first {
                $0.surface.identity == fixture.tableID
            })
            let maximum = table.surface.bounds.width - table.surface.hostViewportWidth
            fixture.drawing.setTableLogicalOffset(maximum, sourceIdentity: fixture.tableID)
            XCTAssertEqual(fixture.drawing.selectionHandles().map(\.role), [.head])
        }
    }

    func testHandleHaloCanBeHitOutsideTableButNeverOutsideHostViewport() throws {
        try withMountedHandles(document: wideTwoCellDocument, anchorIndex: 0, headIndex: 0) { fixture in
            let anchor = try XCTUnwrap(fixture.drawing.selectionHandles().first { $0.role == .anchor })
            let halo = CGPoint(x: anchor.center.x, y: anchor.clip.minY - 4)
            let viewport = try XCTUnwrap(fixture.drawing.tableSelectionViewport())
            XCTAssertTrue(viewport.contains(halo))
            XCTAssertFalse(anchor.clip.contains(halo))
            XCTAssertEqual(fixture.drawing.hitSelectionHandle(at: halo)?.role, .anchor)
            XCTAssertNil(fixture.drawing.selectedTableCell(at: halo, tableID: fixture.tableID))
            let outsideHost = CGPoint(x: viewport.minX - 1, y: anchor.center.y)
            XCTAssertNil(fixture.drawing.hitSelectionHandle(at: outsideHost))
        }
    }

    func testOverlappingMinimumHitTargetsChooseNearestThenAnchorOnExactTie() throws {
        let document = #"{"type":"doc","content":[{"type":"table","content":[{"type":"table_row","content":[{"type":"table_cell","content":[{"type":"paragraph"}]}]}]}]}"#
        let theme = EditorTheme(dictionary: ["table": [
            "minColumnWidth": 24, "cellPadding": 0, "borderWidth": 1
        ]])
        try withMountedHandles(document: document, theme: theme,
                               size: CGSize(width: 40, height: 100), anchorIndex: 0, headIndex: 0) { fixture in
            let anchor = try XCTUnwrap(fixture.drawing.selectionHandles().first { $0.role == .anchor })
            let head = try XCTUnwrap(fixture.drawing.selectionHandles().first { $0.role == .head })
            let midpoint = CGPoint(x: (anchor.center.x + head.center.x) / 2,
                                   y: (anchor.center.y + head.center.y) / 2)
            XCTAssertLessThan(hypot(head.center.x - anchor.center.x,
                                    head.center.y - anchor.center.y), 44)
            XCTAssertEqual(fixture.drawing.hitSelectionHandle(at: midpoint)?.role, .anchor)
            let towardHead = CGPoint(x: (midpoint.x + head.center.x) / 2,
                                     y: (midpoint.y + head.center.y) / 2)
            XCTAssertEqual(fixture.drawing.hitSelectionHandle(at: towardHead)?.role, .head)
        }
    }

    func testOffscreenRealRowsDoNotCreateViewportEdgeHandles() throws {
        try withMountedHandles(document: tallTableDocument(rowCount: 25),
                               anchorIndex: 0, headIndex: 24) { fixture in
            XCTAssertEqual(fixture.drawing.selectionHandles().map(\.role), [.anchor])
            let last = try XCTUnwrap(fixture.drawing.mountedTablePresentation()?.cells.first {
                $0.surface.identity == fixture.tableID
                    && $0.sourcePosition == Int(fixture.positions[24])
            })
            XCTAssertTrue(fixture.surface.beginHandleDrag(at: try fixture.hostPoint(for: .anchor)))
            fixture.surface.updateHandleDrag(at: fixture.drawing.convert(
                CGPoint(x: last.bounds.midX, y: last.bounds.midY), to: fixture.view
            ))
            XCTAssertEqual(try fixture.engineSelection().0, fixture.positions[0],
                           "offscreen real row must not be a drag target")
            fixture.surface.cancelHandleDrag()
            fixture.view.textView.setContentOffset(
                CGPoint(x: 0, y: fixture.view.textView.contentSize.height - fixture.view.textView.bounds.height),
                animated: false
            )
            fixture.surface.updateGeometry(from: fixture.view.textView)
            XCTAssertEqual(fixture.drawing.selectionHandles().map(\.role), [.head])
            XCTAssertEqual(try fixture.engineSelection().0, fixture.positions[0])
            XCTAssertEqual(try fixture.engineSelection().1, fixture.positions[24])
        }
    }

    func testMountedMergedRTLHandlesMirrorPhysicalCornersButKeepEndpointRoles() throws {
        let config = tableConfig.replacingOccurrences(
            of: #""tableRole":"table","attrs":{"class":{"default":null}}"#,
            with: #""tableRole":"table","attrs":{"class":{"default":null},"dir":{"default":null}}"#
        )
        let document = #"{"type":"doc","content":[{"type":"table","attrs":{"dir":"rtl"},"content":[{"type":"table_row","content":[{"type":"table_cell","attrs":{"colspan":2},"content":[{"type":"paragraph","content":[{"type":"text","text":"wide"}]}]},{"type":"table_cell","content":[{"type":"paragraph","content":[{"type":"text","text":"next"}]}]}]}]}]}"#
        try withMountedHandles(document: document, configJSON: config, anchorIndex: 0, headIndex: 1) { fixture in
            let anchor = try XCTUnwrap(fixture.drawing.selectionHandles().first { $0.role == .anchor })
            let head = try XCTUnwrap(fixture.drawing.selectionHandles().first { $0.role == .head })
            XCTAssertEqual(anchor.sourcePosition, fixture.positions[0])
            XCTAssertEqual(head.sourcePosition, fixture.positions[1])
            XCTAssertGreaterThan(anchor.center.x, head.center.x)
            XCTAssertTrue(fixture.surface.beginHandleDrag(at: try fixture.hostPoint(for: .head)))
            fixture.surface.updateHandleDrag(at: try fixture.hostPoint(inCell: 0))
            XCTAssertEqual(try fixture.selection().1, fixture.positions[0])
            fixture.surface.cancelHandleDrag()
        }
    }

    func testIrregularMergedTableUsesOccupiedRealCellCornersAndRejectsSyntheticGap() throws {
        let document = #"{"type":"doc","content":[{"type":"table","content":[{"type":"table_row","content":[{"type":"table_cell","attrs":{"rowspan":2},"content":[{"type":"paragraph","content":[{"type":"text","text":"tall"}]}]},{"type":"table_cell","attrs":{"colspan":2},"content":[{"type":"paragraph","content":[{"type":"text","text":"wide"}]}]}]},{"type":"table_row","content":[{"type":"table_cell","content":[{"type":"paragraph","content":[{"type":"text","text":"later"}]}]}]}]}]}"#
        try withMountedHandles(document: document, anchorIndex: 0, headIndex: 2) { fixture in
            let cells = try XCTUnwrap(fixture.drawing.mountedTablePresentation()?.cells.filter {
                $0.surface.identity == fixture.tableID && $0.cell.sourceCellIndex != nil
            })
            XCTAssertEqual(cells.count, 3)
            let first = try XCTUnwrap(cells.first { $0.sourcePosition == Int(fixture.positions[0]) })
            let last = try XCTUnwrap(cells.first { $0.sourcePosition == Int(fixture.positions[2]) })
            let anchor = try XCTUnwrap(fixture.drawing.selectionHandles().first { $0.role == .anchor })
            let head = try XCTUnwrap(fixture.drawing.selectionHandles().first { $0.role == .head })
            XCTAssertEqual(anchor.center.x, first.bounds.minX + 8, accuracy: 1)
            XCTAssertEqual(anchor.center.y, first.bounds.minY + 8, accuracy: 1)
            XCTAssertEqual(head.center.x, last.bounds.maxX - 8, accuracy: 1)
            XCTAssertEqual(head.center.y, last.bounds.maxY - 8, accuracy: 1)
            let gap = CGPoint(x: head.clip.maxX - 8, y: last.bounds.midY)
            XCTAssertNil(fixture.drawing.selectedTableCell(at: gap, tableID: fixture.tableID))
            XCTAssertTrue(fixture.surface.beginHandleDrag(at: try fixture.hostPoint(for: .head)))
            fixture.surface.updateHandleDrag(at: fixture.drawing.convert(gap, to: fixture.view))
            XCTAssertEqual(try fixture.engineSelection().1, fixture.positions[2])
            fixture.surface.cancelHandleDrag()
        }
    }

    func testNestedOnlyOuterCellIsTheDragTarget() throws {
        let document = #"{"type":"doc","content":[{"type":"table","content":[{"type":"table_row","content":[{"type":"table_cell","content":[{"type":"table","content":[{"type":"table_row","content":[{"type":"table_cell","content":[{"type":"paragraph","content":[{"type":"text","text":"nested"}]}]}]}]}]},{"type":"table_cell","content":[{"type":"paragraph","content":[{"type":"text","text":"outer"}]}]}]}]}]}"#
        try withMountedHandles(document: document, anchorIndex: 1, headIndex: 1) { fixture in
            let nested = try XCTUnwrap(fixture.drawing.mountedTablePresentation()?.cells.first {
                $0.surface.identity != fixture.tableID && $0.cell.sourceCellIndex != nil
            })
            let point = CGPoint(x: nested.bounds.midX, y: nested.bounds.midY)
            XCTAssertEqual(fixture.drawing.selectedTableCell(at: point, tableID: fixture.tableID),
                           fixture.positions[0])
            XCTAssertTrue(fixture.surface.beginHandleDrag(at: try fixture.hostPoint(for: .head)))
            fixture.surface.updateHandleDrag(at: fixture.drawing.convert(point, to: fixture.view))
            XCTAssertEqual(try fixture.selection().1, fixture.positions[0])
            fixture.surface.cancelHandleDrag()
        }
    }

    func testDisabledAndComposingRootCannotAcquireOrContinueHandleDrag() throws {
        try withMountedHandles(document: fourCellDocument, anchorIndex: 0, headIndex: 0) { fixture in
            let head = try fixture.hostPoint(for: .head)
            fixture.view.textView.isEditable = false
            XCTAssertFalse(fixture.surface.beginHandleDrag(at: head))
            fixture.view.textView.isEditable = true
            XCTAssertTrue(fixture.surface.beginHandleDrag(at: head))
            fixture.view.textView.isComposing = true
            fixture.surface.updateHandleDrag(at: try fixture.hostPoint(inCell: 3))
            XCTAssertEqual(try fixture.selection().1, fixture.positions[0])
            fixture.view.textView.isComposing = false
        }
    }

    func testHeldPointerRetargetsAfterHorizontalTableScrollWithRoomRemaining() throws {
        try withMountedHandles(document: wideTwoCellDocument, anchorIndex: 0, headIndex: 1) { fixture in
            let table = try XCTUnwrap(fixture.drawing.mountedTablePresentation()?.tables.first {
                $0.surface.identity == fixture.tableID
            })
            let initialDocument = try XCTUnwrap(fixture.adapter.documentJson())
            let anchor = try XCTUnwrap(fixture.drawing.selectionHandles().first { $0.role == .anchor })
            XCTAssertTrue(fixture.surface.beginHandleDrag(at: fixture.drawing.convert(anchor.center, to: fixture.view)))
            let edgePoint = CGPoint(x: table.clip.maxX - 6, y: anchor.center.y)
            fixture.surface.updateHandleDrag(at: fixture.drawing.convert(edgePoint, to: fixture.view))
            XCTAssertEqual(try fixture.selection().0, fixture.positions[0])
            RunLoop.main.run(until: Date().addingTimeInterval(0.7))
            let offset = fixture.drawing.tableLogicalOffset(for: fixture.tableID)
            XCTAssertGreaterThan(offset, 150)
            XCTAssertLessThan(offset, table.surface.bounds.width - table.surface.hostViewportWidth)
            XCTAssertEqual(try fixture.selection().0, fixture.positions[1],
                           "held pointer must re-hit after each frame's table scroll")
            XCTAssertEqual(try XCTUnwrap(fixture.adapter.documentJson()), initialDocument)
            fixture.surface.cancelHandleDrag()
        }
    }

    func testHeldPointerRetargetsAfterVerticalDocumentScrollAndStopsOnCancel() throws {
        let rowCount = 25
        let document = try tallTableDocument(rowCount: rowCount)
        try withMountedHandles(document: document, anchorIndex: 0, headIndex: 0) { fixture in
            let initialDocument = try XCTUnwrap(fixture.adapter.documentJson())
            let head = try fixture.hostPoint(for: .head)
            XCTAssertTrue(fixture.surface.beginHandleDrag(at: head))
            let viewport = try XCTUnwrap(fixture.drawing.tableSelectionViewport())
            let edgePoint = CGPoint(x: fixture.drawing.convert(head, from: fixture.view).x,
                                    y: viewport.maxY - 6)
            let window = try XCTUnwrap(fixture.view.window)
            let edgeInWindow = fixture.drawing.convert(edgePoint, to: window)
            fixture.surface.updateHandleDrag(at: fixture.drawing.convert(edgePoint, to: fixture.view))
            let beforeFrame = try fixture.selection().1
            RunLoop.main.run(until: Date().addingTimeInterval(0.3))
            let afterFrame = try fixture.selection().1
            let scroll = fixture.view.textView
            let maximum = scroll.contentSize.height - scroll.bounds.height + scroll.adjustedContentInset.bottom
            XCTAssertGreaterThan(scroll.contentOffset.y, 0)
            XCTAssertLessThan(scroll.contentOffset.y, maximum)
            XCTAssertNotEqual(afterFrame, beforeFrame,
                              "stationary pointer must select a newly reached real row")
            let pointedCell = fixture.drawing.selectedTableCell(
                at: fixture.drawing.convert(edgeInWindow, from: window), tableID: fixture.tableID
            )
            XCTAssertEqual(afterFrame, pointedCell)
            XCTAssertEqual(try fixture.engineSelection().0, fixture.positions[0])
            XCTAssertEqual(try fixture.engineSelection().1, afterFrame)
            XCTAssertEqual(try fixture.publishedSelection().1, afterFrame)
            fixture.surface.cancelHandleDrag()
            let stoppedOffset = scroll.contentOffset.y
            RunLoop.main.run(until: Date().addingTimeInterval(0.1))
            XCTAssertEqual(scroll.contentOffset.y, stoppedOffset, accuracy: 1)
            XCTAssertEqual(try XCTUnwrap(fixture.adapter.documentJson()), initialDocument)
        }
    }

    func testAutoGrowHostRetargetsStationaryWindowPointerThroughScrollAncestor() throws {
        try withMountedHandles(document: tallTableDocument(rowCount: 25),
                               anchorIndex: 0, headIndex: 0) { fixture in
            let window = try XCTUnwrap(fixture.view.window)
            let scroll = UIScrollView(frame: window.bounds)
            let contentHeight: CGFloat = 1_400
            fixture.view.removeFromSuperview()
            fixture.view.frame = CGRect(x: 0, y: 0, width: window.bounds.width, height: contentHeight)
            fixture.view.heightBehavior = .autoGrow
            scroll.contentSize = CGSize(width: window.bounds.width, height: contentHeight)
            scroll.addSubview(fixture.view)
            window.addSubview(scroll)
            fixture.view.layoutIfNeeded()
            let head = try fixture.hostPoint(for: .head)
            XCTAssertFalse(fixture.view.textView.isScrollEnabled)
            XCTAssertTrue(fixture.surface.beginHandleDrag(at: head))
            let visible = try XCTUnwrap(fixture.drawing.tableSelectionViewport())
            let edgeInDrawing = CGPoint(x: fixture.drawing.convert(head, from: fixture.view).x,
                                        y: visible.maxY - 6)
            let edgeInWindow = fixture.drawing.convert(edgeInDrawing, to: window)
            fixture.surface.updateHandleDrag(at: fixture.view.convert(edgeInWindow, from: window))
            let before = try fixture.engineSelection().1
            RunLoop.main.run(until: Date().addingTimeInterval(0.3))
            let after = try fixture.engineSelection().1
            XCTAssertGreaterThan(scroll.contentOffset.y, 0)
            XCTAssertLessThan(scroll.contentOffset.y,
                              scroll.contentSize.height - scroll.bounds.height)
            XCTAssertNotEqual(after, before)
            XCTAssertEqual(after, fixture.drawing.selectedTableCell(
                at: fixture.drawing.convert(edgeInWindow, from: window), tableID: fixture.tableID
            ))
            XCTAssertEqual(try fixture.engineSelection().0, fixture.positions[0])
            fixture.surface.cancelHandleDrag()
        }
    }

    func testResizeDragPreviewsLocallyThenCommitsOneUndoableColumnWidth() throws {
        try withMountedHandles(document: fixedWidthFourCellDocument, anchorIndex: 3, headIndex: 3) { fixture in
            let beforeDocument = try XCTUnwrap(fixture.adapter.documentJson())
            let beforeObject = try fixture.documentObject()
            let beforeRevision = fixture.adapter.baseDocumentRevision
            XCTAssertEqual(fixture.adapter.cachedHistoryState?.canUndo, false)
            let edge = try fixture.trailingEdgeHostPoint(cellIndex: 0)
            XCTAssertTrue(fixture.surface.beginResizeDrag(at: edge))
            XCTAssertEqual(fixture.drawing.activeTableResizeEdge, TableResizeEdge(tableID: fixture.tableID, column: 0))
            let dragged = CGPoint(x: edge.x + 60, y: edge.y)
            fixture.surface.updateResizeDrag(at: dragged)
            XCTAssertEqual(fixture.surface.resizePreview,
                           TableResizePreview(edge: TableResizeEdge(tableID: fixture.tableID, column: 0), width: 180))
            XCTAssertEqual(try fixture.presentedCell(0).bounds.width, 180, accuracy: 0.5, "preview must remeasure the dragged column")
            XCTAssertEqual(try fixture.presentedCell(2).bounds.width, 180, accuracy: 0.5, "every cell covering the column follows the preview")
            XCTAssertEqual(try fixture.presentedCell(1).bounds.width, 120, accuracy: 0.5)
            XCTAssertEqual(try fixture.presentedCell(1).bounds.minX, try fixture.presentedCell(0).bounds.maxX, accuracy: 0.5)
            XCTAssertEqual(try XCTUnwrap(fixture.adapter.documentJson()), beforeDocument, "moves must not mutate the document")
            XCTAssertEqual(fixture.adapter.baseDocumentRevision, beforeRevision)
            XCTAssertEqual(fixture.adapter.cachedHistoryState?.canUndo, false)
            XCTAssertTrue(fixture.updates.updates.isEmpty, "moves must not publish updates")

            fixture.surface.endResizeDrag(at: dragged)
            XCTAssertNil(fixture.surface.resizePreview)
            XCTAssertNil(fixture.drawing.activeTableResizeEdge)
            XCTAssertEqual(try fixture.columnWidths(row: 0), [[180], [120]])
            XCTAssertEqual(try fixture.columnWidths(row: 1), [[180], [120]])
            XCTAssertGreaterThan(fixture.adapter.baseDocumentRevision, beforeRevision)
            XCTAssertEqual(try fixture.presentedCell(0).bounds.width, 180, accuracy: 0.5, "authoritative geometry replaces the preview")
            XCTAssertEqual(try fixture.selection().0, fixture.positions[3], "explicit column resize keeps the cell selection")
            XCTAssertEqual(try fixture.selection().1, fixture.positions[3])
            XCTAssertEqual(try fixture.publishedSelection().1, fixture.positions[3])
            XCTAssertEqual(fixture.adapter.cachedHistoryState?.canUndo, true)

            XCTAssertTrue(fixture.view.textView.applyUpdateJSON(try XCTUnwrap(fixture.adapter.undo())))
            XCTAssertEqual(try fixture.documentObject(), beforeObject, "one undo restores the pre-resize document")
            XCTAssertEqual(fixture.adapter.cachedHistoryState?.canUndo, false)
            XCTAssertEqual(fixture.adapter.cachedHistoryState?.canRedo, true)
            XCTAssertEqual(try fixture.presentedCell(0).bounds.width, 120, accuracy: 0.5)
        }
    }

    func testResizeFromProseCaretKeepsCaretAndAddsOneHistoryEntry() throws {
        try withMountedTable(document: proseThenFixedWidthTableDocument, cellSelection: nil) { fixture in
            fixture.view.textView.selectedRange = NSRange(location: 2, length: 0)
            fixture.view.textView.syncSelectionImmediately()
            XCTAssertEqual(try fixture.textSelectionScalars().0, 2)
            let beforeObject = try fixture.documentObject()
            let edge = try fixture.trailingEdgeHostPoint(cellIndex: 0)
            XCTAssertTrue(fixture.surface.beginResizeDrag(at: edge))
            let dragged = CGPoint(x: edge.x + 40, y: edge.y)
            fixture.surface.updateResizeDrag(at: dragged)
            XCTAssertEqual(try fixture.textSelectionScalars().0, 2, "preview must not move the engine selection")
            fixture.surface.endResizeDrag(at: dragged)
            XCTAssertEqual(try fixture.columnWidths(row: 0), [[160], [120]])
            XCTAssertEqual(try fixture.textSelectionScalars().0, 2, "an explicit table target leaves the prose caret alone")
            XCTAssertEqual(try fixture.textSelectionScalars().1, 2)
            XCTAssertEqual(fixture.view.textView.selectedRange, NSRange(location: 2, length: 0))
            XCTAssertTrue(fixture.view.activeTextInput === fixture.view.textView)
            XCTAssertEqual(try fixture.publishedSelectionType(), "text")
            XCTAssertEqual(fixture.adapter.cachedHistoryState?.canUndo, true)
            XCTAssertTrue(fixture.view.textView.applyUpdateJSON(try XCTUnwrap(fixture.adapter.undo())))
            XCTAssertEqual(try fixture.documentObject(), beforeObject)
            XCTAssertEqual(fixture.adapter.cachedHistoryState?.canUndo, false, "one resize is one history entry")
            XCTAssertEqual(try fixture.textSelectionScalars().0, 2)
        }
    }

    func testRTLResizeEdgeIsTheLogicalTrailingEdgeAndInvertsDelta() throws {
        let config = tableConfig.replacingOccurrences(
            of: #""tableRole":"table","attrs":{"class":{"default":null}}"#,
            with: #""tableRole":"table","attrs":{"class":{"default":null},"dir":{"default":null}}"#
        )
        let document = #"{"type":"doc","content":[{"type":"table","attrs":{"dir":"rtl"},"content":[{"type":"table_row","content":[{"type":"table_cell","attrs":{"colwidth":[120]},"content":[{"type":"paragraph","content":[{"type":"text","text":"first"}]}]},{"type":"table_cell","attrs":{"colwidth":[120]},"content":[{"type":"paragraph","content":[{"type":"text","text":"second"}]}]}]}]}]}"#
        try withMountedTable(document: document, configJSON: config, cellSelection: nil) { fixture in
            let first = try fixture.presentedCell(0)
            let second = try fixture.presentedCell(1)
            XCTAssertLessThanOrEqual(second.bounds.maxX, first.bounds.minX, "logical column 0 renders at the right in RTL")
            XCTAssertEqual(fixture.drawing.hitResizeEdge(at: CGPoint(x: first.bounds.minX, y: first.bounds.midY))?.edge.column, 0)
            XCTAssertNil(fixture.drawing.hitResizeEdge(at: CGPoint(x: first.bounds.maxX - 1, y: first.bounds.midY)),
                         "the table's physical right border is not a trailing edge in RTL")
            let edge = try fixture.trailingEdgeHostPoint(cellIndex: 0)
            XCTAssertTrue(fixture.surface.beginResizeDrag(at: edge))
            let dragged = CGPoint(x: edge.x - 40, y: edge.y)
            fixture.surface.updateResizeDrag(at: dragged)
            XCTAssertEqual(fixture.surface.resizePreview?.width, 160, "dragging toward the physical left widens the RTL column")
            XCTAssertEqual(try fixture.presentedCell(0).bounds.width, 160, accuracy: 0.5)
            XCTAssertEqual(try fixture.presentedCell(1).bounds.width, 120, accuracy: 0.5)
            XCTAssertLessThanOrEqual(try fixture.presentedCell(1).bounds.maxX, try fixture.presentedCell(0).bounds.minX)
            fixture.surface.endResizeDrag(at: dragged)
            XCTAssertEqual(try fixture.columnWidths(row: 0), [[160], [120]])
        }
    }

    func testMergedCellTrailingEdgeResizesLastCoveredColumn() throws {
        let document = #"{"type":"doc","content":[{"type":"table","content":[{"type":"table_row","content":[{"type":"table_cell","attrs":{"colspan":2,"colwidth":[100,100]},"content":[{"type":"paragraph","content":[{"type":"text","text":"wide"}]}]}]},{"type":"table_row","content":[{"type":"table_cell","attrs":{"colwidth":[100]},"content":[{"type":"paragraph","content":[{"type":"text","text":"left"}]}]},{"type":"table_cell","attrs":{"colwidth":[100]},"content":[{"type":"paragraph","content":[{"type":"text","text":"right"}]}]}]}]}]}"#
        try withMountedHandles(document: document, anchorIndex: 1, headIndex: 1) { fixture in
            let merged = try fixture.presentedCell(0)
            let interior = CGPoint(x: merged.bounds.minX + 100, y: merged.bounds.midY)
            XCTAssertNil(fixture.drawing.hitResizeEdge(at: interior), "a merged cell has no edge where column 0 ends")
            XCTAssertEqual(fixture.drawing.hitResizeEdge(at: CGPoint(x: merged.bounds.maxX, y: merged.bounds.midY))?.edge.column, 1)
            let edge = try fixture.trailingEdgeHostPoint(cellIndex: 0)
            XCTAssertTrue(fixture.surface.beginResizeDrag(at: edge))
            let dragged = CGPoint(x: edge.x + 30, y: edge.y)
            fixture.surface.updateResizeDrag(at: dragged)
            XCTAssertEqual(try fixture.presentedCell(0).bounds.width, 230, accuracy: 0.5)
            XCTAssertEqual(try fixture.presentedCell(1).bounds.width, 100, accuracy: 0.5)
            XCTAssertEqual(try fixture.presentedCell(2).bounds.width, 130, accuracy: 0.5)
            fixture.surface.endResizeDrag(at: dragged)
            XCTAssertEqual(try fixture.columnWidths(row: 0), [[100, 130]])
            XCTAssertEqual(try fixture.columnWidths(row: 1), [[100], [130]])
        }
    }

    func testResizeDragCancelsOnDocumentResetAndOwnerLossWithoutMutating() throws {
        try withMountedTable(document: fixedWidthFourCellDocument, cellSelection: nil) { fixture in
            let edge = try fixture.trailingEdgeHostPoint(cellIndex: 0)
            XCTAssertTrue(fixture.surface.beginResizeDrag(at: edge))
            let dragged = CGPoint(x: edge.x + 60, y: edge.y)
            fixture.surface.updateResizeDrag(at: dragged)
            XCTAssertEqual(fixture.surface.resizePreview?.width, 180)
            XCTAssertTrue(fixture.view.textView.applyUpdateJSON(
                try XCTUnwrap(fixture.adapter.setContentJson(fixedWidthFourCellDocument))
            ))
            XCTAssertNil(fixture.surface.resizePreview, "a document reset discards the preview")
            XCTAssertNil(fixture.drawing.activeTableResizeEdge)
            XCTAssertEqual(try fixture.presentedCell(0).bounds.width, 120, accuracy: 0.5)
            let resetRevision = fixture.adapter.baseDocumentRevision
            fixture.surface.endResizeDrag(at: dragged)
            XCTAssertEqual(try fixture.columnWidths(row: 0), [[120], [120]])
            XCTAssertEqual(fixture.adapter.baseDocumentRevision, resetRevision)

            let secondEdge = try fixture.trailingEdgeHostPoint(cellIndex: 0)
            XCTAssertTrue(fixture.surface.beginResizeDrag(at: secondEdge))
            fixture.surface.updateResizeDrag(at: CGPoint(x: secondEdge.x + 60, y: secondEdge.y))
            XCTAssertEqual(fixture.surface.resizePreview?.width, 180)
            let beforeDocument = try XCTUnwrap(fixture.adapter.documentJson())
            fixture.adapter.releaseNativeBindingOwner(token: try XCTUnwrap(fixture.adapter.nativeOwnerToken))
            fixture.surface.endResizeDrag(at: CGPoint(x: secondEdge.x + 60, y: secondEdge.y))
            XCTAssertNil(fixture.surface.resizePreview)
            XCTAssertEqual(try XCTUnwrap(fixture.adapter.documentJson()), beforeDocument, "a lost owner must not commit")
        }
    }

    func testReadOnlyComposingAndActiveInputGovernResizeAdmission() throws {
        try withMountedTable(document: fixedWidthFourCellDocument, cellSelection: nil) { fixture in
            let edge = try fixture.trailingEdgeHostPoint(cellIndex: 0)
            fixture.view.textView.isEditable = false
            XCTAssertFalse(fixture.surface.beginResizeDrag(at: edge))
            fixture.view.textView.isEditable = true
            XCTAssertTrue(fixture.surface.beginResizeDrag(at: edge))
            fixture.view.textView.isComposing = true
            fixture.surface.updateResizeDrag(at: CGPoint(x: edge.x + 60, y: edge.y))
            XCTAssertNil(fixture.surface.resizePreview, "composition cancels a held resize")
            fixture.view.textView.isComposing = false
            fixture.surface.endResizeDrag(at: CGPoint(x: edge.x + 60, y: edge.y))
            XCTAssertEqual(try fixture.columnWidths(row: 0), [[120], [120]])

            let content = try XCTUnwrap(fixture.surface.cellFrame(tableID: fixture.tableID, cellIndex: 0))
            XCTAssertTrue(fixture.view.bindTableCell(tableID: fixture.tableID, cellIndex: 0, contentRect: content))
            let input = fixture.view.activeTextInput
            XCTAssertFalse(input === fixture.view.textView)
            let inputFrame = fixture.surface.convert(input.bounds, from: input)
            let insideInput = CGPoint(x: inputFrame.maxX - 2, y: inputFrame.midY)
            XCTAssertFalse(fixture.surface.beginResizeDrag(at: fixture.surface.convert(insideInput, to: fixture.view)),
                           "touches inside the active cell input stay text gestures")
            let border = try fixture.trailingEdgeHostPoint(cellIndex: 0)
            XCTAssertTrue(fixture.surface.beginResizeDrag(at: border))
            let dragged = CGPoint(x: border.x + 50, y: border.y)
            fixture.surface.updateResizeDrag(at: dragged)
            XCTAssertEqual(fixture.surface.resizePreview?.width, 170)
            let previewInput = fixture.surface.convert(input.bounds, from: input)
            XCTAssertEqual(previewInput.width, inputFrame.width + 50, accuracy: 1, "the active input follows the previewed cell")
            fixture.surface.endResizeDrag(at: dragged)
            XCTAssertEqual(try fixture.columnWidths(row: 0), [[170], [120]])
            XCTAssertTrue(fixture.view.activeTextInput === input, "committing keeps the bound cell input")
            XCTAssertEqual(fixture.surface.convert(input.bounds, from: input).width, inputFrame.width + 50, accuracy: 1)
        }
    }

    func testSelectionHandleTakesPrecedenceOverSharedTrailingEdge() throws {
        try withMountedHandles(document: tallTableDocument(rowCount: 6), anchorIndex: 0, headIndex: 0) { fixture in
            let head = try XCTUnwrap(fixture.drawing.selectionHandles().first { $0.role == .head })
            let cornerEdge = CGPoint(x: try fixture.presentedCell(0).bounds.maxX, y: head.center.y)
            XCTAssertNotNil(fixture.drawing.hitResizeEdge(at: cornerEdge))
            XCTAssertFalse(fixture.surface.beginResizeDrag(at: fixture.drawing.convert(cornerEdge, to: fixture.view)))
            XCTAssertTrue(fixture.surface.beginHandleDrag(at: fixture.drawing.convert(cornerEdge, to: fixture.view)))
            fixture.surface.cancelHandleDrag()
            XCTAssertTrue(fixture.surface.beginResizeDrag(at: try fixture.trailingEdgeHostPoint(cellIndex: 4)))
            XCTAssertFalse(fixture.surface.beginHandleDrag(at: try fixture.trailingEdgeHostPoint(cellIndex: 4)),
                           "a held resize owns the touch")
            fixture.surface.cancelResizeDrag()
        }
    }

    func testWideTableEdgeIsOnlyActionableWhenVisibleAndKeepsScrollAnchor() throws {
        try withMountedTable(document: wideTwoCellDocument, cellSelection: nil) { fixture in
            let offscreen = try fixture.trailingEdgeHostPoint(cellIndex: 0)
            XCTAssertFalse(fixture.surface.beginResizeDrag(at: offscreen), "an edge past the host viewport is not grabbable")
            fixture.drawing.setTableLogicalOffset(300, sourceIdentity: fixture.tableID)
            let edge = try fixture.trailingEdgeHostPoint(cellIndex: 0)
            XCTAssertTrue(fixture.surface.beginResizeDrag(at: edge))
            let dragged = CGPoint(x: edge.x + 50, y: edge.y)
            fixture.surface.updateResizeDrag(at: dragged)
            XCTAssertEqual(fixture.surface.resizePreview?.width, 550)
            XCTAssertEqual(try fixture.presentedCell(0).bounds.width, 550, accuracy: 0.5)
            XCTAssertEqual(fixture.drawing.tableLogicalOffset(for: fixture.tableID), 300, accuracy: 1)
            fixture.surface.endResizeDrag(at: dragged)
            XCTAssertEqual(try fixture.columnWidths(row: 0), [[550], [500]])
            XCTAssertEqual(fixture.drawing.tableLogicalOffset(for: fixture.tableID), 300, accuracy: 1,
                           "the logical scroll anchor survives the width change")
        }
    }

    func testStationaryPointerAtViewportEdgeAutoscrollsAndGrowsColumn() throws {
        try withMountedTable(document: wideTwoCellDocument, cellSelection: nil) { fixture in
            fixture.drawing.setTableLogicalOffset(300, sourceIdentity: fixture.tableID)
            let table = try XCTUnwrap(fixture.drawing.mountedTablePresentation()?.tables.first {
                $0.surface.identity == fixture.tableID
            })
            let edge = try fixture.trailingEdgeHostPoint(cellIndex: 0)
            let edgeInDrawing = fixture.drawing.convert(edge, from: fixture.view)
            XCTAssertTrue(fixture.surface.beginResizeDrag(at: edge))
            let held = CGPoint(x: table.clip.maxX - 6, y: edgeInDrawing.y)
            let fingerDelta = held.x - edgeInDrawing.x
            fixture.surface.updateResizeDrag(at: fixture.drawing.convert(held, to: fixture.view))
            XCTAssertEqual(fixture.surface.resizePreview?.width, (500 + fingerDelta).rounded())
            RunLoop.main.run(until: Date().addingTimeInterval(0.5))
            let scrolled = fixture.drawing.tableLogicalOffset(for: fixture.tableID) - 300
            XCTAssertGreaterThan(scrolled, 20, "a held pointer at the viewport edge scrolls the table")
            XCTAssertEqual(fixture.surface.resizePreview?.width, (500 + fingerDelta + scrolled).rounded(),
                           "scrolled distance keeps growing the column under a stationary finger")
            fixture.surface.cancelResizeDrag()
            XCTAssertNil(fixture.surface.resizePreview)
            XCTAssertEqual(try fixture.presentedCell(0).bounds.width, 500, accuracy: 0.5, "cancel restores authoritative geometry")
            let stopped = fixture.drawing.tableLogicalOffset(for: fixture.tableID)
            RunLoop.main.run(until: Date().addingTimeInterval(0.1))
            XCTAssertEqual(fixture.drawing.tableLogicalOffset(for: fixture.tableID), stopped, accuracy: 0.5)
        }
    }

    func testNoNetMovementCommitsNothingAndShrinkClampsToMinimumWidth() throws {
        try withMountedTable(document: fixedWidthFourCellDocument, cellSelection: nil) { fixture in
            let beforeDocument = try XCTUnwrap(fixture.adapter.documentJson())
            let beforeRevision = fixture.adapter.baseDocumentRevision
            let edge = try fixture.trailingEdgeHostPoint(cellIndex: 0)
            XCTAssertTrue(fixture.surface.beginResizeDrag(at: edge))
            fixture.surface.updateResizeDrag(at: CGPoint(x: edge.x + 60, y: edge.y))
            XCTAssertEqual(try fixture.presentedCell(0).bounds.width, 180, accuracy: 0.5)
            fixture.surface.endResizeDrag(at: edge)
            XCTAssertEqual(try XCTUnwrap(fixture.adapter.documentJson()), beforeDocument)
            XCTAssertEqual(fixture.adapter.baseDocumentRevision, beforeRevision)
            XCTAssertEqual(fixture.adapter.cachedHistoryState?.canUndo, false)
            XCTAssertEqual(try fixture.presentedCell(0).bounds.width, 120, accuracy: 0.5)

            let minimum = try fixture.presentedCell(0).surface.style.minColumnWidth
            XCTAssertTrue(fixture.surface.beginResizeDrag(at: edge))
            let dragged = CGPoint(x: edge.x - 200, y: edge.y)
            fixture.surface.updateResizeDrag(at: dragged)
            XCTAssertEqual(fixture.surface.resizePreview?.width, minimum)
            fixture.surface.endResizeDrag(at: dragged)
            XCTAssertEqual(try fixture.columnWidths(row: 0), [[Int(minimum)], [120]])
        }
    }

    func testFrameStepCancelRestoresAuthoritativeGeometryAfterOwnerLoss() throws {
        try withMountedTable(document: wideTwoCellDocument, cellSelection: nil) { fixture in
            fixture.drawing.setTableLogicalOffset(300, sourceIdentity: fixture.tableID)
            let table = try XCTUnwrap(fixture.drawing.mountedTablePresentation()?.tables.first {
                $0.surface.identity == fixture.tableID
            })
            let edge = try fixture.trailingEdgeHostPoint(cellIndex: 0)
            XCTAssertTrue(fixture.surface.beginResizeDrag(at: edge))
            let held = CGPoint(x: table.clip.maxX - 6, y: fixture.drawing.convert(edge, from: fixture.view).y)
            fixture.surface.updateResizeDrag(at: fixture.drawing.convert(held, to: fixture.view))
            XCTAssertGreaterThan(try fixture.presentedCell(0).bounds.width, 500)
            fixture.adapter.releaseNativeBindingOwner(token: try XCTUnwrap(fixture.adapter.nativeOwnerToken))
            RunLoop.main.run(until: Date().addingTimeInterval(0.2))
            XCTAssertNil(fixture.surface.resizePreview, "the frame step must drop a drag whose owner is gone")
            XCTAssertNil(fixture.drawing.activeTableResizeEdge)
            XCTAssertEqual(try fixture.presentedCell(0).bounds.width, 500, accuracy: 0.5,
                           "a frame-step cancel must not leave previewed widths on screen")
            let stopped = fixture.drawing.tableLogicalOffset(for: fixture.tableID)
            RunLoop.main.run(until: Date().addingTimeInterval(0.1))
            XCTAssertEqual(fixture.drawing.tableLogicalOffset(for: fixture.tableID), stopped, accuracy: 0.5)
        }
    }

    func testNestedTableEdgesAndSyntheticGapsAreNeverResizeTargets() throws {
        let nestedDocument = #"{"type":"doc","content":[{"type":"table","content":[{"type":"table_row","content":[{"type":"table_cell","attrs":{"colwidth":[200]},"content":[{"type":"table","content":[{"type":"table_row","content":[{"type":"table_cell","content":[{"type":"paragraph","content":[{"type":"text","text":"in"}]}]},{"type":"table_cell","content":[{"type":"paragraph","content":[{"type":"text","text":"ner"}]}]}]}]}]},{"type":"table_cell","attrs":{"colwidth":[120]},"content":[{"type":"paragraph","content":[{"type":"text","text":"outer"}]}]}]}]}]}"#
        try withMountedTable(document: nestedDocument, cellSelection: nil) { fixture in
            let nestedCells = try XCTUnwrap(fixture.drawing.mountedTablePresentation()?.cells.filter {
                $0.surface.identity != fixture.tableID && $0.cell.sourceCellIndex != nil
            })
            XCTAssertEqual(nestedCells.count, 2)
            let innerEdge = try XCTUnwrap(nestedCells.min { $0.bounds.minX < $1.bounds.minX })
            let point = CGPoint(x: innerEdge.bounds.maxX, y: innerEdge.bounds.midY)
            XCTAssertNil(fixture.drawing.hitResizeEdge(at: point), "a nested table border is not a resize edge")
            XCTAssertFalse(fixture.surface.beginResizeDrag(at: fixture.drawing.convert(point, to: fixture.view)))
            let outer = try fixture.presentedCell(0)
            let hit = try XCTUnwrap(fixture.drawing.hitResizeEdge(at: CGPoint(x: outer.bounds.maxX, y: outer.bounds.midY)))
            XCTAssertEqual(hit.edge, TableResizeEdge(tableID: fixture.tableID, column: 0))
        }
        let irregularDocument = #"{"type":"doc","content":[{"type":"table","content":[{"type":"table_row","content":[{"type":"table_cell","attrs":{"rowspan":2,"colwidth":[100]},"content":[{"type":"paragraph","content":[{"type":"text","text":"tall"}]}]},{"type":"table_cell","attrs":{"colspan":2,"colwidth":[100,100]},"content":[{"type":"paragraph","content":[{"type":"text","text":"wide"}]}]}]},{"type":"table_row","content":[{"type":"table_cell","attrs":{"colwidth":[100]},"content":[{"type":"paragraph","content":[{"type":"text","text":"later"}]}]}]}]}]}"#
        try withMountedTable(document: irregularDocument, cellSelection: nil) { fixture in
            let wide = try fixture.presentedCell(1)
            let later = try fixture.presentedCell(2)
            XCTAssertFalse(wide.surface.syntheticRegions.isEmpty, "the irregular fixture must project a synthetic gap")
            XCTAssertNil(fixture.drawing.hitResizeEdge(at: CGPoint(x: wide.bounds.maxX, y: later.bounds.midY)),
                         "the gap's outer border is not a resize edge")
            XCTAssertGreaterThan(wide.bounds.maxX, later.bounds.maxX, "the gap sits after the last real cell of row 1")
            XCTAssertNil(fixture.drawing.hitResizeEdge(at: CGPoint(x: (later.bounds.maxX + wide.bounds.maxX) / 2,
                                                                   y: later.bounds.midY)),
                         "the gap interior is not a resize edge")
            XCTAssertEqual(fixture.drawing.hitResizeEdge(at: CGPoint(x: wide.bounds.maxX, y: wide.bounds.midY))?.edge.column, 2)
            XCTAssertEqual(fixture.drawing.hitResizeEdge(at: CGPoint(x: later.bounds.maxX, y: later.bounds.midY))?.edge.column, 1)
        }
    }

    func testLiveCellHandleGestureChangesEngineSelection() throws {
        guard ProcessInfo.processInfo.environment["NATIVE_TABLE_GESTURE_PROBE"] == "1" else {
            throw XCTSkip("Live simulator gesture probe is opt in")
        }
        let editorId = makeV2Editor(configJson: tableConfig)
        defer { destroyV2Editor(id: editorId) }
        let adapter = try XCTUnwrap(EditorV2Registry.adapter(forLegacyId: editorId))
        let window = UIWindow(frame: UIScreen.main.bounds)
        window.backgroundColor = .systemBackground
        let view = RichTextEditorView(frame: CGRect(x: 20, y: 160, width: 360, height: 240))
        window.addSubview(view)
        let status = UILabel(frame: CGRect(x: 0, y: 80, width: window.bounds.width, height: 40))
        status.textAlignment = .center
        status.backgroundColor = .systemYellow
        window.addSubview(status)
        window.makeKeyAndVisible()
        defer { window.isHidden = true }
        view.bindEditor(id: editorId, initialUpdateJSON: try XCTUnwrap(adapter.initialUpdateJSON()))
        XCTAssertTrue(view.textView.applyUpdateJSON(try XCTUnwrap(adapter.setContentJson(fourCellDocument))))
        let tableID = try XCTUnwrap(adapter.cachedTableRecords.keys.first)
        let rawCells = try XCTUnwrap(adapter.cachedTableRecords[tableID]?["cells"] as? [[String: Any]])
        let positions = try rawCells.map { try XCTUnwrap(EditorV2Adapter.uint32Field($0, "sourcePos")) }
        let request = adapter.callWithEnvelope([
            "selection": ["type": "cell",
                          "anchorCell": ["kind": "document", "offset": Int(positions[0])],
                          "headCell": ["kind": "document", "offset": Int(positions[0])]]
        ]) { editorV2SetSelection(editorId: adapter.editorId, requestJson: $0) }
        XCTAssertNil(request.error)
        XCTAssertTrue(view.textView.applyUpdateJSON(try XCTUnwrap(adapter.refreshFromRustState(mirrorSelection: nil))))
        view.layoutIfNeeded()
        let surface = try XCTUnwrap(view.subviews.compactMap { $0 as? EditorTableSurface }.first)
        let drawing = try XCTUnwrap(surface.subviews.compactMap { $0 as? PreparedProseDrawingView }.first)
        let head = try XCTUnwrap(drawing.selectionHandles().first { $0.role == .head })
        let target = try XCTUnwrap(drawing.mountedTablePresentation()?.cells.first {
            $0.surface.identity == tableID && $0.sourcePosition == Int(positions[3])
        })
        let start = drawing.convert(head.center, to: window)
        let end = drawing.convert(CGPoint(x: target.bounds.midX, y: target.bounds.midY), to: window)
        let beforeDocument = try XCTUnwrap(adapter.documentJson())
        let beforeRevision = adapter.baseDocumentRevision
        status.text = "HANDLE READY \(Int(start.x)),\(Int(start.y)) TO \(Int(end.x)),\(Int(end.y))"
        print("CELL_HANDLE_GESTURE_START \(start.x) \(start.y) END \(end.x) \(end.y)")
        let before = XCTAttachment(image: UIGraphicsImageRenderer(bounds: window.bounds).image {
            window.layer.render(in: $0.cgContext)
        })
        before.name = "Cell handle before real gesture"
        before.lifetime = .keepAlways
        add(before)
        let changed = XCTNSPredicateExpectation(predicate: NSPredicate { _, _ in
            guard let raw = adapter.cachedAtomicRenderJSON,
                  let object = try? JSONSerialization.jsonObject(with: Data(raw.utf8)) as? [String: Any],
                  let endpoints = EditorCellSelection.endpointPositions(object["selection"] as Any)
            else { return false }
            return endpoints.anchor == positions[0] && endpoints.head == positions[3]
        }, object: nil)
        XCTAssertEqual(XCTWaiter.wait(for: [changed], timeout: 90), .completed,
                       "real handle gesture did not change the authoritative cell endpoint")
        XCTAssertEqual(adapter.baseDocumentRevision, beforeRevision)
        XCTAssertEqual(try XCTUnwrap(adapter.documentJson()), beforeDocument)
        XCTAssertEqual(drawing.selectedTableCellEndpoints?.head, positions[3])
        status.text = "HANDLE DRAG COMPLETE"
        let after = XCTAttachment(image: UIGraphicsImageRenderer(bounds: window.bounds).image {
            window.layer.render(in: $0.cgContext)
        })
        after.name = "Cell handle after real gesture"
        after.lifetime = .keepAlways
        add(after)
    }

    func testLiveColumnResizeChangesFirstColumnWhileSecondCellIsSelected() throws {
        guard ProcessInfo.processInfo.environment["NATIVE_TABLE_GESTURE_PROBE"] == "1" else {
            throw XCTSkip("Live simulator gesture probe is opt in")
        }
        try withMountedHandles(document: fixedWidthFourCellDocument, size: CGSize(width: 360, height: 400),
                               anchorIndex: 3, headIndex: 3) { fixture in
            fixture.view.frame = CGRect(x: 0, y: 100, width: 360, height: 240)
            fixture.view.layoutIfNeeded()
            fixture.surface.updateGeometry(from: fixture.view.textView)
            let table = try XCTUnwrap(fixture.drawing.mountedTablePresentation()?.tables.first {
                $0.surface.identity == fixture.tableID
            })
            let first = try XCTUnwrap(fixture.drawing.mountedTablePresentation()?.cells.first {
                $0.surface.identity == fixture.tableID && $0.sourcePosition == Int(fixture.positions[0])
            })
            let start = fixture.drawing.convert(
                CGPoint(x: first.bounds.maxX, y: first.bounds.midY), to: fixture.view.window
            )
            let end = CGPoint(x: start.x + 60, y: start.y)
            let before = try XCTUnwrap(fixture.adapter.documentJson())
            print("COLUMN_RESIZE_GESTURE_START \(start.x) \(start.y) END \(end.x) \(end.y)")
            XCTAssertEqual(table.surface.layout.columnWidths[0], 120, accuracy: 1)
            let resized = XCTNSPredicateExpectation(predicate: NSPredicate { _, _ in
                guard let json = fixture.adapter.documentJson() else { return false }
                return json != before && json.contains(#""colwidth":[180]"#)
            }, object: nil)
            XCTAssertEqual(XCTWaiter.wait(for: [resized], timeout: 90), .completed,
                           "real drag on first logical column edge did not resize that column")
            let json = try XCTUnwrap(fixture.adapter.documentJson())
            let root = try XCTUnwrap(JSONSerialization.jsonObject(with: Data(json.utf8)) as? [String: Any])
            let tableNode = try XCTUnwrap((root["content"] as? [[String: Any]])?.first)
            let row = try XCTUnwrap((tableNode["content"] as? [[String: Any]])?.first)
            let cells = try XCTUnwrap(row["content"] as? [[String: Any]])
            XCTAssertEqual((cells[0]["attrs"] as? [String: Any])?["colwidth"] as? [Int], [180])
            XCTAssertEqual((cells[1]["attrs"] as? [String: Any])?["colwidth"] as? [Int], [120])
            XCTAssertEqual(try fixture.selection().0, fixture.positions[3])
            XCTAssertEqual(try fixture.selection().1, fixture.positions[3])
        }
    }

    func testMountedOffsetFollowsSourceIdentityAcrossProseTypingAndClearsOnReset() throws {
        let editorId = makeV2Editor(configJson: tableConfig)
        defer { destroyV2Editor(id: editorId) }
        let adapter = try XCTUnwrap(EditorV2Registry.adapter(forLegacyId: editorId))
        let document = wideTwoCellDocument
        let view = RichTextEditorView(frame: CGRect(x: 0, y: 0, width: 320, height: 200))
        view.bindEditor(id: editorId, initialUpdateJSON: try XCTUnwrap(adapter.initialUpdateJSON()))
        XCTAssertTrue(view.textView.applyUpdateJSON(try XCTUnwrap(adapter.setContentJson(document))))
        view.layoutIfNeeded()
        let tableSurface = try XCTUnwrap(view.subviews.compactMap { $0 as? EditorTableSurface }.first)
        let drawing = try XCTUnwrap(tableSurface.subviews.compactMap { $0 as? PreparedProseDrawingView }.first)
        let initialTableID = try XCTUnwrap(adapter.cachedTableRecords.keys.first)
        let sourceID = try XCTUnwrap(adapter.cachedTableRecords[initialTableID]?["sourceId"] as? String)
        drawing.setTableLogicalOffset(100, sourceIdentity: initialTableID)
        XCTAssertEqual(drawing.tableLogicalOffset(for: initialTableID), 100, accuracy: 1)

        view.textView.selectedRange = NSRange(location: 0, length: 0)
        view.textView.insertText("X")
        view.layoutIfNeeded()
        let shiftedTableID = try XCTUnwrap(adapter.cachedTableRecords.first {
            $0.value["sourceId"] as? String == sourceID
        }?.key)
        XCTAssertNotEqual(shiftedTableID, initialTableID)
        XCTAssertEqual(drawing.tableLogicalOffset(for: shiftedTableID), 100, accuracy: 1)

        XCTAssertTrue(view.textView.applyUpdateJSON(try XCTUnwrap(adapter.setContentJson(document))))
        view.layoutIfNeeded()
        let resetTableID = try XCTUnwrap(adapter.cachedTableRecords.keys.first)
        XCTAssertEqual(drawing.tableLogicalOffset(for: resetTableID), 0, accuracy: 1)
    }

    func testActiveInputClipPassesNeighborHitToRootAfterScroll() throws {
        let editorId = makeV2Editor(configJson: tableConfig)
        defer { destroyV2Editor(id: editorId) }
        let adapter = try XCTUnwrap(EditorV2Registry.adapter(forLegacyId: editorId))
        let view = RichTextEditorView(frame: CGRect(x: 0, y: 0, width: 320, height: 200))
        view.bindEditor(id: editorId, initialUpdateJSON: try XCTUnwrap(adapter.initialUpdateJSON()))
        XCTAssertTrue(view.textView.applyUpdateJSON(try XCTUnwrap(adapter.setContentJson(wideTwoCellDocument))))
        view.layoutIfNeeded()
        let tableID = try XCTUnwrap(adapter.cachedTableRecords.keys.first)
        let surface = try XCTUnwrap(view.subviews.compactMap { $0 as? EditorTableSurface }.first)
        let drawing = try XCTUnwrap(surface.subviews.compactMap { $0 as? PreparedProseDrawingView }.first)
        let first = try XCTUnwrap(surface.cellFrame(tableID: tableID, cellIndex: 0))
        XCTAssertTrue(view.bindTableCell(tableID: tableID, cellIndex: 0, contentRect: first))

        drawing.setTableLogicalOffset(350, sourceIdentity: tableID)
        let scrolledFirst = try XCTUnwrap(surface.cellFrame(tableID: tableID, cellIndex: 0))
        XCTAssertEqual(scrolledFirst.minX, first.minX - 350, accuracy: 1)
        XCTAssertEqual(surface.convert(view.activeTextInput.bounds, from: view.activeTextInput).minX,
                       scrolledFirst.minX, accuracy: 1)
        guard case .cell(1) = surface.arrowDestination(
            tableID: tableID, cellIndex: 0, direction: .right,
            caret: CGPoint(x: scrolledFirst.maxX, y: scrolledFirst.midY)
        ) else {
            return XCTFail("projected first-cell caret did not navigate to visible neighbor")
        }
        let second = try XCTUnwrap(surface.cellFrame(tableID: tableID, cellIndex: 1))
        let visibleSecond = second.intersection(surface.bounds)
        XCTAssertFalse(visibleSecond.isEmpty)
        let point = CGPoint(x: visibleSecond.midX, y: visibleSecond.midY)
        let hit = try XCTUnwrap(surface.cellHit(at: point))
        XCTAssertEqual(hit.cellIndex, 1)
        XCTAssertNil(surface.hitTest(point, with: nil), "clipped active input must not intercept a visible neighboring cell")
        let rootPoint = surface.convert(point, to: view)
        XCTAssertFalse(view.hitTest(rootPoint, with: nil) === view.activeTextInput)
    }

    func testLiveMountedEditorPansInactiveAndActiveCellWithoutDocumentMutation() throws {
        guard ProcessInfo.processInfo.environment["NATIVE_TABLE_GESTURE_PROBE"] == "1" else {
            throw XCTSkip("Live simulator gesture probe is opt in")
        }
        let editorId = makeV2Editor(configJson: tableConfig)
        defer { destroyV2Editor(id: editorId) }
        let adapter = try XCTUnwrap(EditorV2Registry.adapter(forLegacyId: editorId))
        let document = wideTwoCellDocument
        let window = UIWindow(frame: UIScreen.main.bounds)
        window.backgroundColor = .systemBackground
        let view = RichTextEditorView(frame: CGRect(x: 0, y: 100, width: window.bounds.width, height: 240))
        window.addSubview(view)
        view.bindEditor(id: editorId, initialUpdateJSON: try XCTUnwrap(adapter.initialUpdateJSON()))
        XCTAssertTrue(view.textView.applyUpdateJSON(try XCTUnwrap(adapter.setContentJson(document))))
        view.layoutIfNeeded()
        let tableID = try XCTUnwrap(adapter.cachedTableRecords.keys.first)
        let tableSurface = try XCTUnwrap(view.subviews.compactMap { $0 as? EditorTableSurface }.first)
        let drawing = try XCTUnwrap(tableSurface.subviews.compactMap { $0 as? PreparedProseDrawingView }.first)
        let firstCell = try XCTUnwrap(tableSurface.cellFrame(tableID: tableID, cellIndex: 0))
        let initialJSON = try XCTUnwrap(adapter.documentJson())
        let initialRevision = adapter.baseDocumentRevision
        let status = UILabel(frame: CGRect(x: 0, y: 350, width: window.bounds.width, height: 30))
        status.backgroundColor = .systemYellow
        status.textAlignment = .center
        status.text = "EDITOR INACTIVE READY"
        window.addSubview(status)
        window.makeKeyAndVisible()
        defer { window.isHidden = true }

        let inactive = XCTNSPredicateExpectation(predicate: NSPredicate { _, _ in
            drawing.tableLogicalOffset(for: tableID) > 40
        }, object: nil)
        guard XCTWaiter.wait(for: [inactive], timeout: 180) == .completed else {
            return XCTFail("Real pan over inactive editor cell did not scroll the table")
        }
        XCTAssertEqual(adapter.baseDocumentRevision, initialRevision)
        XCTAssertEqual(try XCTUnwrap(adapter.documentJson()), initialJSON)
        drawing.cancelTableMotion()
        drawing.setTableLogicalOffset(0, sourceIdentity: tableID)
        XCTAssertEqual(drawing.tableLogicalOffset(for: tableID), 0, accuracy: 1)
        XCTAssertTrue(view.bindTableCell(tableID: tableID, cellIndex: 0, contentRect: firstCell))
        let activeInput = view.activeTextInput
        XCTAssertFalse(activeInput === view.textView)
        XCTAssertTrue(activeInput.becomeFirstResponder())
        XCTAssertTrue(activeInput.isFirstResponder)
        activeInput.selectedRange = NSRange(location: 0, length: 0)
        activeInput.setMarkedText("Z", selectedRange: NSRange(location: 1, length: 0))
        XCTAssertNotNil(activeInput.markedTextRange)
        status.text = "EDITOR ACTIVE READY"
        let active = XCTNSPredicateExpectation(predicate: NSPredicate { _, _ in
            drawing.tableLogicalOffset(for: tableID) > 40
        }, object: nil)
        guard XCTWaiter.wait(for: [active], timeout: 180) == .completed else {
            return XCTFail("Real pan over the one active cell input did not scroll the table")
        }
        XCTAssertEqual(adapter.baseDocumentRevision, initialRevision)
        XCTAssertEqual(try XCTUnwrap(adapter.documentJson()), initialJSON)
        let scrolled = try XCTUnwrap(tableSurface.cellFrame(tableID: tableID, cellIndex: 0))
        XCTAssertLessThan(scrolled.minX, firstCell.minX)
        XCTAssertTrue(view.activeTextInput === activeInput)
        XCTAssertFalse(activeInput.isHidden)
        XCTAssertTrue(activeInput.isFirstResponder)
        XCTAssertNotNil(activeInput.markedTextRange, "horizontal pan must preserve an in-progress IME composition")
        let offsetAfterPan = drawing.tableLogicalOffset(for: tableID)
        activeInput.unmarkText()
        let composed = try XCTUnwrap(adapter.documentJson())
        XCTAssertTrue(composed.contains(#""text":"Zone""#), composed)
        XCTAssertNil(activeInput.markedTextRange)
        XCTAssertEqual(drawing.tableLogicalOffset(for: tableID), offsetAfterPan, accuracy: 1)
        XCTAssertTrue(view.textView.applyUpdateJSON(try XCTUnwrap(adapter.undo())))
        let undone = try XCTUnwrap(adapter.documentJson())
        let initialObject = try XCTUnwrap(JSONSerialization.jsonObject(with: Data(initialJSON.utf8)) as? NSDictionary)
        let undoneObject = try XCTUnwrap(JSONSerialization.jsonObject(with: Data(undone.utf8)) as? NSDictionary)
        XCTAssertEqual(undoneObject, initialObject)
        XCTAssertEqual(drawing.tableLogicalOffset(for: tableID), offsetAfterPan, accuracy: 1)
        XCTAssertTrue(view.textView.applyUpdateJSON(try XCTUnwrap(adapter.redo())))
        let redone = try XCTUnwrap(adapter.documentJson())
        let composedObject = try XCTUnwrap(JSONSerialization.jsonObject(with: Data(composed.utf8)) as? NSDictionary)
        let redoneObject = try XCTUnwrap(JSONSerialization.jsonObject(with: Data(redone.utf8)) as? NSDictionary)
        XCTAssertEqual(redoneObject, composedObject)
        XCTAssertEqual(drawing.tableLogicalOffset(for: tableID), offsetAfterPan, accuracy: 1)
    }

    func testLiveNativeSelectionHandlePrecedesTablePan() throws {
        guard ProcessInfo.processInfo.environment["NATIVE_TABLE_GESTURE_PROBE"] == "1" else {
            throw XCTSkip("Live simulator gesture probe is opt in")
        }
        let editorId = makeV2Editor(configJson: tableConfig)
        defer { destroyV2Editor(id: editorId) }
        let adapter = try XCTUnwrap(EditorV2Registry.adapter(forLegacyId: editorId))
        let document = wideTwoCellDocument.replacingOccurrences(
            of: #""text":"one""#, with: #""text":"selection words for native drag""#
        )
        let window = UIWindow(frame: UIScreen.main.bounds)
        window.backgroundColor = .systemBackground
        let view = RichTextEditorView(frame: CGRect(x: 0, y: 100, width: window.bounds.width, height: 240))
        window.addSubview(view)
        window.makeKeyAndVisible()
        defer { window.isHidden = true }
        view.bindEditor(id: editorId, initialUpdateJSON: try XCTUnwrap(adapter.initialUpdateJSON()))
        XCTAssertTrue(view.textView.applyUpdateJSON(try XCTUnwrap(adapter.setContentJson(document))))
        view.layoutIfNeeded()
        let tableID = try XCTUnwrap(adapter.cachedTableRecords.keys.first)
        let surface = try XCTUnwrap(view.subviews.compactMap { $0 as? EditorTableSurface }.first)
        let drawing = try XCTUnwrap(surface.subviews.compactMap { $0 as? PreparedProseDrawingView }.first)
        let first = try XCTUnwrap(surface.cellFrame(tableID: tableID, cellIndex: 0))
        XCTAssertTrue(view.bindTableCell(tableID: tableID, cellIndex: 0, contentRect: first))
        let input = view.activeTextInput
        XCTAssertTrue(input.becomeFirstResponder())
        input.selectedRange = NSRange(location: 0, length: 0)
        let originalDocument = try XCTUnwrap(adapter.documentJson())
        let status = UILabel(frame: CGRect(x: 0, y: 350, width: window.bounds.width, height: 30))
        status.backgroundColor = .systemYellow
        status.textAlignment = .center
        status.text = "WORD SELECTION READY"
        window.addSubview(status)

        let selected = XCTNSPredicateExpectation(predicate: NSPredicate { _, _ in
            input.selectedRange.length > 0
        }, object: nil)
        guard XCTWaiter.wait(for: [selected], timeout: 180) == .completed else {
            return XCTFail("real word-selection gesture did not create a native text selection")
        }
        let initialSelection = input.selectedRange
        XCTAssertEqual(drawing.tableLogicalOffset(for: tableID), 0, accuracy: 1)
        status.text = "SELECTION HANDLE READY"
        let dragged = XCTNSPredicateExpectation(predicate: NSPredicate { _, _ in
            input.selectedRange != initialSelection
        }, object: nil)
        guard XCTWaiter.wait(for: [dragged], timeout: 180) == .completed else {
            return XCTFail("real selection handle drag did not alter the native selection")
        }
        XCTAssertEqual(drawing.tableLogicalOffset(for: tableID), 0, accuracy: 1)
        XCTAssertEqual(try XCTUnwrap(adapter.documentJson()), originalDocument)
        XCTAssertTrue(input.isFirstResponder)
    }

    func testEngineCellSelectionAdmitsAuthoritativeSnapshot() throws {
        let editorId = makeV2Editor(configJson: tableConfig)
        defer { destroyV2Editor(id: editorId) }
        let adapter = try XCTUnwrap(EditorV2Registry.adapter(forLegacyId: editorId))
        let document = #"{"type":"doc","content":[{"type":"table","content":[{"type":"table_row","content":[{"type":"table_cell","content":[{"type":"paragraph","content":[{"type":"text","text":"one"}]}]},{"type":"table_cell","content":[{"type":"paragraph","content":[{"type":"text","text":"two"}]}]},{"type":"table_cell","content":[{"type":"paragraph","content":[{"type":"text","text":"three"}]}]}]}]}]}"#
        let view = RichTextEditorView(frame: CGRect(x: 0, y: 0, width: 360, height: 180))
        view.bindEditor(id: editorId, initialUpdateJSON: try XCTUnwrap(adapter.initialUpdateJSON()))
        XCTAssertTrue(view.textView.applyUpdateJSON(try XCTUnwrap(adapter.setContentJson(document))))
        let record = try XCTUnwrap(adapter.cachedTableRecords.values.first)
        let cells = try XCTUnwrap(record["cells"] as? [[String: Any]])
        let first = try XCTUnwrap(cells[0]["sourcePos"] as? Int)
        let second = try XCTUnwrap(cells[1]["sourcePos"] as? Int)
        let firstScalar = EditorV2Shadow.docToScalar(id: editorId, docPos: UInt32(first + 2))
        let secondScalar = EditorV2Shadow.docToScalar(id: editorId, docPos: UInt32(second + 2))
        let result = editorV2SetSelection(
            editorId: adapter.editorId,
            requestJson: #"{"version":1,"requestId":"991102","baseDocumentRevision":"\#(adapter.baseDocumentRevision)","selection":{"type":"cell","anchorCell":{"offset":\#(firstScalar),"kind":"scalar"},"headCell":{"offset":\#(secondScalar),"kind":"scalar"}}}"#
        )
        XCTAssertNil(result.error)
        let raw = try XCTUnwrap(editorV2RenderUpdate(editorId: adapter.editorId, mirrorScalarAnchor: nil, mirrorScalarHead: nil).value)
        let object = try XCTUnwrap(JSONSerialization.jsonObject(with: Data(raw.utf8)) as? [String: Any])
        let selection = try XCTUnwrap(object["selection"] as? [String: Any])
        XCTAssertEqual(selection["type"] as? String, "cell")
        XCTAssertEqual(selection["anchorCell"] as? Int, first)
        XCTAssertEqual(selection["headCell"] as? Int, second)
        XCTAssertFalse(raw.isEmpty)
        XCTAssertTrue(view.textView.applyUpdateJSON(EditorV2Shadow.getCurrentState(id: editorId)))
        let snapshot = try XCTUnwrap(adapter.cachedAtomicRenderJSON)
        XCTAssertTrue(snapshot.contains(#""type":"cell""#))
        XCTAssertTrue(view.applyTheme(EditorTheme(dictionary: ["table": ["selectionColor": "#FF000080"]])))
        view.layoutIfNeeded()
        let surface = try XCTUnwrap(view.subviews.compactMap { $0 as? EditorTableSurface }.first)
        let drawing = try XCTUnwrap(surface.subviews.compactMap { $0 as? PreparedProseDrawingView }.first)
        let block = try XCTUnwrap(drawing.layout?.blocks.first)
        let table = try XCTUnwrap(block.tableSurface)
        let origin = try XCTUnwrap(block.tableBounds).origin
        let format = UIGraphicsImageRendererFormat()
        format.scale = 1
        let image = UIGraphicsImageRenderer(size: drawing.bounds.size, format: format).image { _ in
            drawing.draw(drawing.bounds)
        }
        let cgImage = try XCTUnwrap(image.cgImage)
        var pixels = [UInt8](repeating: 0, count: cgImage.width * cgImage.height * 4)
        let bitmap = try XCTUnwrap(CGContext(data: &pixels, width: cgImage.width, height: cgImage.height,
                                              bitsPerComponent: 8, bytesPerRow: cgImage.width * 4,
                                              space: CGColorSpaceCreateDeviceRGB(),
                                              bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue | CGBitmapInfo.byteOrder32Big.rawValue))
        bitmap.draw(cgImage, in: CGRect(x: 0, y: 0, width: cgImage.width, height: cgImage.height))
        let selectedPositions = drawing.selectedTableCellSourcePositions
        drawing.selectedTableCellSourcePositions = [:]
        let baselineImage = UIGraphicsImageRenderer(size: drawing.bounds.size, format: format).image { _ in
            drawing.draw(drawing.bounds)
        }
        drawing.selectedTableCellSourcePositions = selectedPositions
        let baselineCG = try XCTUnwrap(baselineImage.cgImage)
        var baselinePixels = [UInt8](repeating: 0, count: baselineCG.width * baselineCG.height * 4)
        let baselineBitmap = try XCTUnwrap(CGContext(data: &baselinePixels, width: baselineCG.width, height: baselineCG.height,
                                                      bitsPerComponent: 8, bytesPerRow: baselineCG.width * 4,
                                                      space: CGColorSpaceCreateDeviceRGB(),
                                                      bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue | CGBitmapInfo.byteOrder32Big.rawValue))
        baselineBitmap.draw(baselineCG, in: CGRect(x: 0, y: 0, width: baselineCG.width, height: baselineCG.height))
        let offset = { (cell: PreparedViewerTableCell) -> Int in
            let x = Int(origin.x + cell.frame.maxX - 6)
            let y = Int(origin.y + cell.frame.maxY - 6)
            return (y * cgImage.width + x) * 4
        }
        let alpha = { (cell: PreparedViewerTableCell) -> UInt8 in
            pixels[offset(cell) + 3]
        }
        XCTAssertGreaterThan(alpha(table.cells[0]), alpha(table.cells[2]))
        XCTAssertGreaterThan(alpha(table.cells[1]), alpha(table.cells[2]))
        for cell in table.cells.prefix(2) {
            let index = offset(cell)
            XCTAssertGreaterThan(pixels[index], pixels[index + 1])
            XCTAssertGreaterThan(pixels[index], pixels[index + 2])
            XCTAssertGreaterThan(pixels[index + 3], baselinePixels[index + 3])
        }
        let third = offset(table.cells[2])
        XCTAssertEqual(Array(pixels[third..<(third + 4)]), Array(baselinePixels[third..<(third + 4)]))
        let attachment = XCTAttachment(image: image)
        attachment.name = "Native-rendered iPhone 17 light cell selection"
        attachment.lifetime = .keepAlways
        add(attachment)
    }

    func testMountedEngineCellSelectionDrawsOpaqueEndpointHandles() throws {
        let editorId = makeV2Editor(configJson: tableConfig)
        defer { destroyV2Editor(id: editorId) }
        let adapter = try XCTUnwrap(EditorV2Registry.adapter(forLegacyId: editorId))
        let document = #"{"type":"doc","content":[{"type":"table","content":[{"type":"table_row","content":[{"type":"table_cell","content":[{"type":"paragraph","content":[{"type":"text","text":"one"}]}]},{"type":"table_cell","content":[{"type":"paragraph","content":[{"type":"text","text":"two"}]}]}]}]}]}"#
        let window = UIWindow(frame: CGRect(x: 0, y: 0, width: 360, height: 220))
        let view = RichTextEditorView(frame: window.bounds)
        window.addSubview(view)
        window.makeKeyAndVisible()
        defer { window.isHidden = true }
        view.bindEditor(id: editorId, initialUpdateJSON: try XCTUnwrap(adapter.initialUpdateJSON()))
        XCTAssertTrue(view.textView.applyUpdateJSON(try XCTUnwrap(adapter.setContentJson(document))))
        XCTAssertTrue(view.applyTheme(EditorTheme(dictionary: ["table": ["selectionColor": "#FF000080"]])))
        let tableID = try XCTUnwrap(adapter.cachedTableRecords.keys.first)
        let cells = try XCTUnwrap(adapter.cachedTableRecords[tableID]?["cells"] as? [[String: Any]])
        let anchor = try XCTUnwrap(cells[0]["sourcePos"] as? Int)
        let head = try XCTUnwrap(cells[1]["sourcePos"] as? Int)
        let request = #"{"version":1,"requestId":"991107","baseDocumentRevision":"\#(adapter.baseDocumentRevision)","selection":{"type":"cell","anchorCell":{"offset":\#(anchor),"kind":"document"},"headCell":{"offset":\#(head),"kind":"document"}}}"#
        XCTAssertNil(editorV2SetSelection(editorId: adapter.editorId, requestJson: request).error)
        XCTAssertTrue(view.textView.applyUpdateJSON(try XCTUnwrap(adapter.refreshFromRustState(mirrorSelection: nil))))
        view.layoutIfNeeded()
        let surface = try XCTUnwrap(view.subviews.compactMap { $0 as? EditorTableSurface }.first)
        let drawing = try XCTUnwrap(surface.subviews.compactMap { $0 as? PreparedProseDrawingView }.first)
        let presentation = try XCTUnwrap(drawing.mountedTablePresentation())
        let selected = presentation.cells.filter {
            $0.surface.identity == tableID && drawing.selectedTableCellSourcePositions[tableID]?.contains($0.sourcePosition) == true
        }
        XCTAssertEqual(selected.count, 2)
        let first = try XCTUnwrap(selected.min { $0.bounds.minX < $1.bounds.minX })
        let last = try XCTUnwrap(selected.max { $0.bounds.maxX < $1.bounds.maxX })
        let handleInset: CGFloat = 8
        let points = [CGPoint(x: first.bounds.minX + handleInset, y: first.bounds.minY + handleInset),
                      CGPoint(x: last.bounds.maxX - handleInset, y: last.bounds.maxY - handleInset)]
        let renderer = UIGraphicsImageRenderer(size: drawing.bounds.size)
        let image = renderer.image { _ in drawing.draw(drawing.bounds) }
        let cgImage = try XCTUnwrap(image.cgImage)
        var pixels = [UInt8](repeating: 0, count: cgImage.width * cgImage.height * 4)
        let bitmap = try XCTUnwrap(CGContext(data: &pixels, width: cgImage.width, height: cgImage.height,
                                              bitsPerComponent: 8, bytesPerRow: cgImage.width * 4,
                                              space: CGColorSpaceCreateDeviceRGB(),
                                              bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue | CGBitmapInfo.byteOrder32Big.rawValue))
        bitmap.draw(cgImage, in: CGRect(x: 0, y: 0, width: cgImage.width, height: cgImage.height))
        for point in points {
            let x = Int(point.x * image.scale)
            let y = Int(point.y * image.scale)
            XCTAssertGreaterThanOrEqual(x, 0)
            XCTAssertGreaterThanOrEqual(y, 0)
            XCTAssertLessThan(x, cgImage.width)
            XCTAssertLessThan(y, cgImage.height)
            let offset = (y * cgImage.width + x) * 4
            XCTAssertLessThan(pixels[offset + 1], 32, "opaque selection handle green at \(point)")
            XCTAssertGreaterThan(pixels[offset + 3], 240, "opaque selection handle alpha at \(point)")
        }
    }

    func testCellSelectionPreservesFocusedCellInputAndRejectsStaleTyping() throws {
        let editorId = makeV2Editor(configJson: tableConfig)
        defer { destroyV2Editor(id: editorId) }
        let adapter = try XCTUnwrap(EditorV2Registry.adapter(forLegacyId: editorId))
        let document = #"{"type":"doc","content":[{"type":"table","content":[{"type":"table_row","content":[{"type":"table_cell","content":[{"type":"paragraph","content":[{"type":"text","text":"one"}]}]},{"type":"table_cell","content":[{"type":"paragraph","content":[{"type":"text","text":"two"}]}]}]}]},{"type":"paragraph","content":[{"type":"text","text":"after"}]}]}"#
        let window = UIWindow(frame: CGRect(x: 0, y: 0, width: 360, height: 220))
        let view = RichTextEditorView(frame: window.bounds)
        window.addSubview(view)
        window.makeKeyAndVisible()
        defer { window.isHidden = true }
        view.bindEditor(id: editorId, initialUpdateJSON: try XCTUnwrap(adapter.initialUpdateJSON()))
        XCTAssertTrue(view.textView.applyUpdateJSON(try XCTUnwrap(adapter.setContentJson(document))))
        let tableID = try XCTUnwrap(adapter.cachedTableInputMappings?.tables.keys.first)
        let cells = try XCTUnwrap(adapter.cachedTableRecords[tableID]?["cells"] as? [[String: Any]])
        let first = try XCTUnwrap(cells[0]["sourcePos"] as? Int)
        let second = try XCTUnwrap(cells[1]["sourcePos"] as? Int)
        XCTAssertTrue(view.bindTableCell(tableID: tableID, cellIndex: 0, contentRect: CGRect(x: 0, y: 0, width: 100, height: 50)))
        let input = view.activeTextInput
        XCTAssertTrue(input.becomeFirstResponder())
        let request = #"{"version":1,"requestId":"991103","baseDocumentRevision":"\#(adapter.baseDocumentRevision)","selection":{"type":"cell","anchorCell":{"offset":\#(EditorV2Shadow.docToScalar(id: editorId, docPos: UInt32(first + 2))),"kind":"scalar"},"headCell":{"offset":\#(EditorV2Shadow.docToScalar(id: editorId, docPos: UInt32(second + 2))),"kind":"scalar"}}}"#
        XCTAssertNil(editorV2SetSelection(editorId: adapter.editorId, requestJson: request).error)
        XCTAssertTrue(view.textView.applyUpdateJSON(EditorV2Shadow.getCurrentState(id: editorId)))
        XCTAssertFalse(input.isFirstResponder)
        XCTAssertTrue(view.textView.isFirstResponder)
        XCTAssertTrue(view.activeTextInput === view.textView)
        let before = try XCTUnwrap(adapter.documentJson())
        let tab = try XCTUnwrap(input.keyCommands?.first {
            $0.input == "\t" && $0.modifierFlags.isEmpty
        })
        _ = input.perform(tab.action)
        XCTAssertEqual(try XCTUnwrap(adapter.documentJson()), before)
        input.insertText("unsafe")
        view.textView.insertText("unsafe")
        XCTAssertEqual(try XCTUnwrap(adapter.documentJson()), before)
        XCTAssertTrue(view.textView.rootTableSelectionInputBlocked)
        let root = view.textView
        let afterRange = (root.text as NSString).range(of: "after")
        XCTAssertNotEqual(afterRange.location, NSNotFound)
        root.selectedRange = NSRange(location: afterRange.location, length: 0)
        root.textViewDidChangeSelection(root)
        XCTAssertTrue(root.authoritativeCellSelectionActive)
        XCTAssertTrue(root.rootTableSelectionInputBlocked)
        root.insertText("unsafe")
        XCTAssertEqual(try XCTUnwrap(adapter.documentJson()), before)

        root.layoutManager.ensureLayout(forCharacterRange: afterRange)
        let glyphs = root.layoutManager.glyphRange(forCharacterRange: afterRange, actualCharacterRange: nil)
        let rect = root.layoutManager.boundingRect(forGlyphRange: glyphs, in: root.textContainer)
        let point = CGPoint(x: rect.minX + root.textContainerInset.left + 2,
                            y: rect.midY + root.textContainerInset.top)
        XCTAssertTrue(root.placeCaret(at: point))
        flushMainQueue()
        XCTAssertFalse(root.authoritativeCellSelectionActive)
        XCTAssertFalse(root.rootTableSelectionInputBlocked)
        let surface = try XCTUnwrap(view.subviews.compactMap { $0 as? EditorTableSurface }.first)
        let drawing = try XCTUnwrap(surface.subviews.compactMap { $0 as? PreparedProseDrawingView }.first)
        XCTAssertTrue(drawing.selectedTableCellSourcePositions.isEmpty)
        let proseSnapshot = try XCTUnwrap(adapter.cachedAtomicRenderJSON)
        let proseObject = try XCTUnwrap(JSONSerialization.jsonObject(with: Data(proseSnapshot.utf8)) as? [String: Any])
        XCTAssertEqual((proseObject["selection"] as? [String: Any])?["type"] as? String, "text")
        root.insertText("!")
        XCTAssertTrue(try XCTUnwrap(adapter.documentJson()).contains(#""text":"!after""#))

        let nextRequest = #"{"version":1,"requestId":"991104","baseDocumentRevision":"\#(adapter.baseDocumentRevision)","selection":{"type":"cell","anchorCell":{"offset":\#(EditorV2Shadow.docToScalar(id: editorId, docPos: UInt32(first + 2))),"kind":"scalar"},"headCell":{"offset":\#(EditorV2Shadow.docToScalar(id: editorId, docPos: UInt32(second + 2))),"kind":"scalar"}}}"#
        XCTAssertNil(editorV2SetSelection(editorId: adapter.editorId, requestJson: nextRequest).error)
        XCTAssertTrue(root.applyUpdateJSON(EditorV2Shadow.getCurrentState(id: editorId)))
        let cellFrame = try XCTUnwrap(surface.cellFrame(tableID: tableID, cellIndex: 0))
        XCTAssertTrue(view.activateTableCell(at: CGPoint(x: cellFrame.midX, y: cellFrame.midY)))
        let cellInput = view.activeTextInput
        flushMainQueue()
        XCTAssertTrue(drawing.selectedTableCellSourcePositions.isEmpty)
        XCTAssertFalse(root.authoritativeCellSelectionActive)
        XCTAssertEqual(cellInput.tableCellPositionMap?.binding.positionEpoch, adapter.positionEpoch)
        let cellSnapshot = try XCTUnwrap(adapter.cachedAtomicRenderJSON)
        let cellObject = try XCTUnwrap(JSONSerialization.jsonObject(with: Data(cellSnapshot.utf8)) as? [String: Any])
        XCTAssertEqual((cellObject["selection"] as? [String: Any])?["type"] as? String, "text")
        let beforeCellTap = try XCTUnwrap(adapter.documentJson())
        cellInput.insertText("?")
        let editedJSON = try XCTUnwrap(adapter.documentJson())
        XCTAssertNotEqual(editedJSON, beforeCellTap)
        let edited = try XCTUnwrap(JSONSerialization.jsonObject(with: Data(editedJSON.utf8)) as? [String: Any])
        let topLevel = try XCTUnwrap(edited["content"] as? [[String: Any]])
        let rows = try XCTUnwrap(topLevel[0]["content"] as? [[String: Any]])
        let editedCells = try XCTUnwrap(rows[0]["content"] as? [[String: Any]])
        let firstCellJSON = try XCTUnwrap(String(data: JSONSerialization.data(withJSONObject: editedCells[0]), encoding: .utf8))
        XCTAssertTrue(firstCellJSON.contains("?"))
        XCTAssertFalse(try XCTUnwrap(String(data: JSONSerialization.data(withJSONObject: editedCells[1]), encoding: .utf8)).contains("?"))
        XCTAssertFalse(try XCTUnwrap(String(data: JSONSerialization.data(withJSONObject: topLevel[1]), encoding: .utf8)).contains("?"))
    }

    func testCellSelectionResolverUsesRealSourceCellsAndClosesMergedSpans() {
        func cell(_ pos: Int, _ row: Int, _ column: Int, colspan: Int = 1) -> [String: Any] {
            ["sourcePos": pos, "row": row, "column": column, "rowspan": 1, "colspan": colspan]
        }
        let records: [String: [String: Any]] = [
            "t1": ["tablePos": 1, "sourceEnd": 40, "rows": 2, "columns": 3,
                   "direction": "rtl", "failure": NSNull(),
                   "cells": [cell(3, 0, 0), cell(7, 0, 1, colspan: 2),
                             cell(13, 1, 0), cell(17, 1, 1), cell(21, 1, 2)],
                   "syntheticRegions": [["row": 0, "column": 2]]],
            "t40": ["tablePos": 40, "sourceEnd": 70, "rows": 1, "columns": 1,
                    "failure": NSNull(), "cells": [cell(42, 0, 0)]]
        ]
        let selection: [String: Any] = ["type": "cell", "anchorCell": 3, "headCell": 17]
        XCTAssertEqual(EditorCellSelection.resolve(selection, records: records),
                       .drawable(tableID: "t1", sourcePositions: Set([3, 7, 13, 17, 21])))
        XCTAssertNil(EditorCellSelection.resolve(selection.merging(["anchorScalar": 0]) { _, new in new }, records: records))
        XCTAssertNil(EditorCellSelection.resolve(["type": "cell", "anchorCell": 3.5, "headCell": 17], records: records))
        XCTAssertNil(EditorCellSelection.resolve(["type": "cell", "anchorCell": 3, "headCell": 42], records: records))
        let failed: [String: [String: Any]] = ["t1": ["tablePos": 1, "sourceEnd": 40,
                                                   "failure": "gridLimit", "cells": []]]
        XCTAssertEqual(EditorCellSelection.resolve(selection, records: failed), .unavailable(tableID: "t1"))
        let chained: [String: [String: Any]] = ["t1": ["tablePos": 1, "sourceEnd": 40,
            "rows": 3, "columns": 3, "failure": NSNull(), "direction": "rtl",
            "cells": [
                ["sourcePos": 3, "row": 0, "column": 0, "rowspan": 2, "colspan": 1],
                ["sourcePos": 7, "row": 2, "column": 0, "rowspan": 1, "colspan": 2],
                ["sourcePos": 11, "row": 1, "column": 1, "rowspan": 1, "colspan": 1],
                ["sourcePos": 15, "row": 1, "column": 2, "rowspan": 2, "colspan": 1]
            ]]]
        let forward: [String: Any] = ["type": "cell", "anchorCell": 11, "headCell": 15]
        let backward: [String: Any] = ["type": "cell", "anchorCell": 15, "headCell": 11]
        let closed = EditorCellSelection.drawable(tableID: "t1", sourcePositions: Set([3, 7, 11, 15]))
        XCTAssertEqual(EditorCellSelection.resolve(forward, records: chained), closed)
        XCTAssertEqual(EditorCellSelection.resolve(backward, records: chained), closed)
    }

    func testExpoOwnerPublishesAuthoritativeCellSelectionShape() throws {
        let editorId = makeV2Editor(configJson: tableConfig)
        defer { destroyV2Editor(id: editorId) }
        let adapter = try XCTUnwrap(EditorV2Registry.adapter(forLegacyId: editorId))
        let expoHost = NativeEditorExpoView()
        defer { expoHost.setEditorId(0) }
        expoHost.setEditorId(editorId)
        let document = #"{"type":"doc","content":[{"type":"table","content":[{"type":"table_row","content":[{"type":"table_cell","content":[{"type":"paragraph","content":[{"type":"text","text":"one"}]}]},{"type":"table_cell","content":[{"type":"paragraph","content":[{"type":"text","text":"two"}]}]}]}]}]}"#
        XCTAssertTrue(expoHost.richTextView.textView.applyUpdateJSON(try XCTUnwrap(adapter.setContentJson(document))))
        let tableID = try XCTUnwrap(adapter.cachedTableInputMappings?.tables.keys.first)
        let cells = try XCTUnwrap(adapter.cachedTableRecords[tableID]?["cells"] as? [[String: Any]])
        let first = try XCTUnwrap(cells[0]["sourcePos"] as? Int)
        let second = try XCTUnwrap(cells[1]["sourcePos"] as? Int)
        XCTAssertTrue(expoHost.richTextView.bindTableCell(tableID: tableID, cellIndex: 0, contentRect: .zero))
        let staleCell = expoHost.richTextView.activeTextInput
        let request = #"{"version":1,"requestId":"991105","baseDocumentRevision":"\#(adapter.baseDocumentRevision)","selection":{"type":"cell","anchorCell":{"offset":\#(EditorV2Shadow.docToScalar(id: editorId, docPos: UInt32(first + 2))),"kind":"scalar"},"headCell":{"offset":\#(EditorV2Shadow.docToScalar(id: editorId, docPos: UInt32(second + 2))),"kind":"scalar"}}}"#
        XCTAssertNil(editorV2SetSelection(editorId: adapter.editorId, requestJson: request).error)
        let update = EditorV2Shadow.getCurrentState(id: editorId)
        XCTAssertTrue(expoHost.richTextView.textView.applyUpdateJSON(update))
        XCTAssertTrue(expoHost.ownsNativeBinding(editorId: editorId))
        XCTAssertTrue(expoHost.richTextView.activeTextInput === expoHost.richTextView.textView)
        XCTAssertEqual(staleCell.editorId, 0)
        let event = try XCTUnwrap(NativeEditorExpoView.nativeCommitEventPayload(
            originatingEditorId: adapter.editorId, updateJSON: update
        ))
        XCTAssertEqual(Set(event.keys), ["editorId", "documentRevision", "updateJson"])
        XCTAssertEqual(event["editorId"] as? String, adapter.editorId)
        let atomic = try XCTUnwrap(event["updateJson"] as? String)
        let payload = try XCTUnwrap(JSONSerialization.jsonObject(with: Data(atomic.utf8)) as? [String: Any])
        let selection = try XCTUnwrap(payload["selection"] as? [String: Any])
        XCTAssertEqual(Set(selection.keys), ["type", "anchorCell", "headCell"])
        XCTAssertEqual(selection["anchorCell"] as? Int, first)
        XCTAssertEqual(selection["headCell"] as? Int, second)
    }

    private struct ExpoGeometryFixture {
        let host: NativeEditorExpoView
        let adapter: EditorV2Adapter
        let tableID: String
        let positions: [UInt32]
        let drawing: PreparedProseDrawingView
        let recorder: GeometryRecorder

        func selectText(anchor: UInt32, head: UInt32) throws {
            try host.richTextView.textView.applyLocalSelection(
                adapter: adapter, selection: EditorV2PositionBridge.textSelectionEnvelope(anchor: anchor, head: head)
            )
        }

        func selectCells(anchor: Int, head: Int) throws {
            try host.richTextView.textView.selectTableCells(adapter: adapter, anchor: positions[anchor],
                                                            head: positions[head])
        }

        func activateCell(_ index: Int) throws -> EditorTextView {
            let surface = try XCTUnwrap(drawing.superview as? EditorTableSurface)
            let frame = try XCTUnwrap(surface.cellFrame(tableID: tableID, cellIndex: UInt32(index)))
            XCTAssertTrue(host.richTextView.activateTableCell(at: CGPoint(x: frame.midX, y: frame.midY)),
                          "cell \(index) refused activation")
            let input = host.richTextView.activeTextInput
            XCTAssertFalse(input === host.richTextView.textView, "cell \(index) must own the cell input")
            XCTAssertTrue(input.isFirstResponder, "activating cell \(index) focuses its input")
            return input
        }

        func expectedWindowRects(sourcePositions: Set<Int>? = nil) throws -> [CGRect] {
            let visible = try XCTUnwrap(drawing.tableSelectionViewport())
            let selected = try sourcePositions ?? XCTUnwrap(drawing.selectedTableCellSourcePositions[tableID])
            return try XCTUnwrap(drawing.mountedTablePresentation()).cells.filter {
                $0.surface.identity == tableID && $0.cell.sourceCellIndex != nil
                    && selected.contains($0.sourcePosition)
            }.map { $0.bounds.intersection($0.clip).intersection(visible) }
                .filter { !$0.isNull && !$0.isEmpty }
                .map { windowRect(drawing.convert($0, to: host)) }
        }

        func windowRect(_ hostRect: CGRect) -> CGRect {
            hostRect.offsetBy(dx: host.frame.minX, dy: host.frame.minY)
        }
    }

    private final class GeometryRecorder {
        var payloads: [[String: Any]] = []

        func rects(at index: Int) throws -> [CGRect] {
            let rects = try XCTUnwrap(payloads[index]["rects"] as? [[String: Double]])
            return try rects.map { try Self.rect($0) }
        }

        static func rect(_ raw: [String: Double]) throws -> CGRect {
            XCTAssertEqual(Set(raw.keys), ["x", "y", "width", "height"])
            return CGRect(x: try XCTUnwrap(raw["x"]), y: try XCTUnwrap(raw["y"]),
                          width: try XCTUnwrap(raw["width"]), height: try XCTUnwrap(raw["height"]))
        }
    }

    private func assertRects(_ actual: [CGRect], _ expected: [CGRect], _ message: String = "",
                             file: StaticString = #filePath, line: UInt = #line) {
        let accuracy: CGFloat = 0.001
        let matches = actual.count == expected.count && zip(actual, expected).allSatisfy { lhs, rhs in
            abs(lhs.minX - rhs.minX) <= accuracy && abs(lhs.minY - rhs.minY) <= accuracy
                && abs(lhs.width - rhs.width) <= accuracy && abs(lhs.height - rhs.height) <= accuracy
        }
        XCTAssertTrue(matches, "\(message) actual \(actual) expected \(expected)", file: file, line: line)
    }

    private func waitForGeometryFrame() {
        RunLoop.main.run(until: Date().addingTimeInterval(0.1))
    }

    private func withExpoTableGeometry(
        configJson: String = TableInputTestSchema.tableConfig, document: String, clippingAncestor: CGRect? = nil,
        _ body: (ExpoGeometryFixture) throws -> Void
    ) throws {
        let editorId = makeV2Editor(configJson: configJson)
        defer { destroyV2Editor(id: editorId) }
        let adapter = try XCTUnwrap(EditorV2Registry.adapter(forLegacyId: editorId))
        let window = UIWindow(frame: CGRect(x: 0, y: 0, width: 400, height: 500))
        let host = NativeEditorExpoView()
        let hostFrame = CGRect(x: 24, y: 72, width: 340, height: 260)
        if let clippingAncestor {
            let ancestor = UIView(frame: clippingAncestor)
            ancestor.clipsToBounds = true
            window.addSubview(ancestor)
            host.frame = hostFrame.offsetBy(dx: -clippingAncestor.minX, dy: -clippingAncestor.minY)
            ancestor.addSubview(host)
        } else {
            host.frame = hostFrame
            window.addSubview(host)
        }
        window.makeKeyAndVisible()
        defer {
            host.setEditorId(0)
            window.isHidden = true
        }
        host.setEditorId(editorId)
        let recorder = GeometryRecorder()
        host.onTableSelectionGeometryForTesting = { recorder.payloads.append($0) }
        XCTAssertTrue(host.richTextView.textView.applyUpdateJSON(try XCTUnwrap(adapter.setContentJson(document))))
        host.layoutIfNeeded()
        XCTAssertTrue(host.richTextView.textView.becomeFirstResponder())
        let tableID = try adapter.editableTableID()
        let positions = try adapter.tableCellPositions()
        let surface = try XCTUnwrap(host.richTextView.subviews.compactMap { $0 as? EditorTableSurface }.first)
        let drawing = try XCTUnwrap(surface.subviews.compactMap { $0 as? PreparedProseDrawingView }.first)
        waitForGeometryFrame()
        try body(ExpoGeometryFixture(host: host, adapter: adapter, tableID: tableID, positions: positions,
                                     drawing: drawing, recorder: recorder))
    }

    func testCellSelectionPublishesWindowSpaceGeometryOnTheNextFrame() throws {
        try withExpoTableGeometry(document: fourCellDocument) { fixture in
            XCTAssertTrue(fixture.recorder.payloads.isEmpty, "a caret selection has no geometry: \(fixture.recorder.payloads)")
            try fixture.selectCells(anchor: 0, head: 3)
            XCTAssertTrue(fixture.recorder.payloads.isEmpty, "emission waits for the next display frame")
            waitForGeometryFrame()

            XCTAssertEqual(fixture.recorder.payloads.count, 1, "\(fixture.recorder.payloads)")
            let payload = try XCTUnwrap(fixture.recorder.payloads.first)
            XCTAssertEqual(Set(payload.keys).subtracting(["keyboard"]),
                           ["editorId", "documentRevision", "layoutEpoch", "tablePos",
                            "coordinateSpace", "rects", "viewport", "safeArea", "editMenuVisible"])
            XCTAssertEqual(payload["editMenuVisible"] as? Bool, false, "no native edit menu is showing")
            let window = try XCTUnwrap(fixture.host.window)
            XCTAssertEqual(try GeometryRecorder.rect(try XCTUnwrap(payload["safeArea"] as? [String: Double])),
                           window.bounds.inset(by: window.safeAreaInsets),
                           "the safe area is the window minus its safe-area insets")
            XCTAssertEqual(payload["editorId"] as? String, fixture.adapter.editorId)
            XCTAssertEqual(payload["documentRevision"] as? String, String(fixture.adapter.baseDocumentRevision))
            XCTAssertEqual(payload["layoutEpoch"] as? String, try XCTUnwrap(fixture.adapter.positionEpoch).description)
            let record = try XCTUnwrap(fixture.adapter.cachedTableRecords[fixture.tableID])
            XCTAssertEqual(payload["tablePos"] as? Int,
                           Int(try XCTUnwrap(EditorV2Adapter.uint32Field(record, "tablePos"))))
            XCTAssertEqual(payload["coordinateSpace"] as? String, "window")
            let rects = try fixture.recorder.rects(at: 0)
            XCTAssertEqual(rects.count, 4)
            assertRects(rects, try fixture.expectedWindowRects())
            let viewport = try GeometryRecorder.rect(try XCTUnwrap(payload["viewport"] as? [String: Double]))
            XCTAssertEqual(viewport, fixture.host.frame, "viewport is the visible editor in window space")
            for rect in rects {
                XCTAssertTrue(viewport.contains(rect), "\(rect) lies inside the window-space editor \(viewport)")
            }
        }
    }

    func testNativeCellEditMenuVisibilityIsRepublishedForTheToolbarToYield() throws {
        try withExpoTableGeometry(document: fourCellDocument) { fixture in
            try fixture.selectCells(anchor: 0, head: 3)
            waitForGeometryFrame()
            let surface = try XCTUnwrap(fixture.host.richTextView.subviews.compactMap { $0 as? EditorTableSurface }.first)
            XCTAssertEqual(fixture.recorder.payloads.last?["editMenuVisible"] as? Bool, false)

            surface.presentCellEditMenu()
            XCTAssertTrue(surface.isCellEditMenuVisible, "the menu presents over the focused cell selection")
            waitForGeometryFrame()
            XCTAssertEqual(fixture.recorder.payloads.count, 2, "showing the menu republishes once: \(fixture.recorder.payloads)")
            XCTAssertEqual(fixture.recorder.payloads.last?["editMenuVisible"] as? Bool, true)
            assertRects(try fixture.recorder.rects(at: 1), try fixture.recorder.rects(at: 0),
                        "the menu does not move the selection geometry")

            surface.dismissCellEditMenu()
            waitForGeometryFrame()
            XCTAssertEqual(fixture.recorder.payloads.count, 3, "\(fixture.recorder.payloads)")
            XCTAssertEqual(fixture.recorder.payloads.last?["editMenuVisible"] as? Bool, false,
                           "the toolbar returns once the menu closes")
        }
    }

    func testHorizontalTableScrollMovesPublishedRectsAtMostOncePerFrame() throws {
        try withExpoTableGeometry(document: wideTwoCellDocument) { fixture in
            try fixture.selectCells(anchor: 0, head: 1)
            waitForGeometryFrame()
            XCTAssertEqual(fixture.recorder.payloads.count, 1, "\(fixture.recorder.payloads)")
            let before = try fixture.recorder.rects(at: 0)
            let table = try XCTUnwrap(fixture.drawing.mountedTablePresentation()?.tables.first {
                $0.surface.identity == fixture.tableID
            })
            let step: CGFloat = -100
            for _ in 0..<3 {
                XCTAssertEqual(fixture.drawing.scrollTables(in: [table.surface.scrollIdentity], by: step), 0)
            }
            XCTAssertEqual(fixture.drawing.tableLogicalOffset(for: fixture.tableID), 300)
            XCTAssertEqual(fixture.recorder.payloads.count, 1, "scroll steps inside one frame stay queued")
            waitForGeometryFrame()

            XCTAssertEqual(fixture.recorder.payloads.count, 2, "three scroll steps coalesce into one event")
            let after = try fixture.recorder.rects(at: 1)
            assertRects(after, try fixture.expectedWindowRects())
            XCTAssertNotEqual(after, before, "rects follow the table scroll offset")
            let firstCell = try XCTUnwrap(fixture.drawing.mountedTablePresentation()?.cells.first {
                $0.surface.identity == fixture.tableID && $0.sourcePosition == Int(fixture.positions[0])
            })
            let scrolledEdge = fixture.windowRect(fixture.drawing.convert(firstCell.bounds, to: fixture.host)).maxX
            XCTAssertEqual(after.first?.maxX ?? .nan, scrolledEdge, accuracy: 0.001,
                           "the first cell's trailing edge is reported where the scrolled table draws it")
            XCTAssertEqual(fixture.recorder.payloads[1]["tablePos"] as? Int, fixture.recorder.payloads[0]["tablePos"] as? Int)
        }
    }

    func testParentMovingTheHostRepublishesShiftedRectsOnce() throws {
        try withExpoTableGeometry(document: fourCellDocument) { fixture in
            try fixture.selectCells(anchor: 0, head: 3)
            waitForGeometryFrame()
            XCTAssertEqual(fixture.recorder.payloads.count, 1)
            let before = try fixture.recorder.rects(at: 0)
            let shift: CGFloat = 40

            fixture.host.frame = fixture.host.frame.offsetBy(dx: 0, dy: shift)
            XCTAssertTrue(fixture.host.tableSelectionGeometryPublisher.hasScheduledFlushForTesting,
                          "moving the host schedules a geometry frame")
            XCTAssertEqual(fixture.recorder.payloads.count, 1, "the move is published on the next frame")
            waitForGeometryFrame()

            XCTAssertEqual(fixture.recorder.payloads.count, 2, "\(fixture.recorder.payloads)")
            let after = try fixture.recorder.rects(at: 1)
            assertRects(after, before.map { $0.offsetBy(dx: 0, dy: shift) }, "rects follow the moved host")
            assertRects(after, try fixture.expectedWindowRects())
            let viewport = try GeometryRecorder.rect(try XCTUnwrap(fixture.recorder.payloads[1]["viewport"] as? [String: Double]))
            XCTAssertEqual(viewport, fixture.host.frame)
        }
    }

    func testUnchangedGeometryIsNeverPublishedTwice() throws {
        try withExpoTableGeometry(document: fourCellDocument) { fixture in
            try fixture.selectCells(anchor: 0, head: 1)
            waitForGeometryFrame()
            XCTAssertEqual(fixture.recorder.payloads.count, 1)

            fixture.host.tableSelectionGeometryPublisher.flush()
            fixture.host.richTextView.setNeedsLayout()
            fixture.host.richTextView.layoutIfNeeded()
            fixture.host.tableSelectionGeometryPublisher.scheduleFlush()
            waitForGeometryFrame()

            XCTAssertEqual(fixture.recorder.payloads.count, 1, "\(fixture.recorder.payloads)")
        }
    }

    private final class StubKeyboardGuide {
        let guide = UILayoutGuide()
        private let origin: CGPoint
        private let top: NSLayoutConstraint
        private let leading: NSLayoutConstraint
        private let width: NSLayoutConstraint
        private let height: NSLayoutConstraint

        init(in host: UIView, window: UIWindow, frame: CGRect) {
            host.addLayoutGuide(guide)
            origin = host.convert(CGPoint.zero, to: window)
            top = guide.topAnchor.constraint(equalTo: window.topAnchor, constant: origin.y + frame.minY)
            leading = guide.leadingAnchor.constraint(equalTo: window.leadingAnchor, constant: origin.x + frame.minX)
            width = guide.widthAnchor.constraint(equalToConstant: frame.width)
            height = guide.heightAnchor.constraint(equalToConstant: frame.height)
            NSLayoutConstraint.activate([top, leading, width, height])
        }

        func move(to frame: CGRect) {
            top.constant = origin.y + frame.minY
            leading.constant = origin.x + frame.minX
            width.constant = frame.width
            height.constant = frame.height
        }
    }

    func testKeyboardOcclusionFollowsTheUndockedKeyboardLayoutGuide() throws {
        try withExpoTableGeometry(document: fourCellDocument) { fixture in
            XCTAssertTrue(fixture.host.keyboardLayoutGuide.followsUndockedKeyboard,
                          "the guide must follow undocked and floating keyboards")
            XCTAssertEqual(fixture.host.keyboardOcclusionConstraints.count, 4)
            XCTAssertTrue(fixture.host.keyboardOcclusionConstraints.allSatisfy {
                $0.secondItem === fixture.host.keyboardLayoutGuide
            }, "occlusion is tracked from the host keyboard layout guide")
        }
    }

    func testAKeyboardGuideRestingBelowTheEditorPublishesNoKeyboard() throws {
        try withExpoTableGeometry(document: fourCellDocument) { fixture in
            let restingBand = CGRect(x: 0, y: 260, width: 340, height: 168)
            let stub = StubKeyboardGuide(in: fixture.host, window: try XCTUnwrap(fixture.host.window), frame: restingBand)
            fixture.host.trackKeyboardOcclusion(of: stub.guide)
            try fixture.selectCells(anchor: 0, head: 3)
            waitForGeometryFrame()

            let payload = try XCTUnwrap(fixture.recorder.payloads.last)
            XCTAssertNil(payload["keyboard"], "a guide resting at the editor's bottom edge is no keyboard: \(payload)")
        }
    }

    func testADockedKeyboardPublishesOnlyTheEditorBandItCovers() throws {
        try withExpoTableGeometry(document: fourCellDocument) { fixture in
            let docked = CGRect(x: 0, y: 180, width: 340, height: 200)
            let stub = StubKeyboardGuide(in: fixture.host, window: try XCTUnwrap(fixture.host.window), frame: docked)
            fixture.host.trackKeyboardOcclusion(of: stub.guide)
            try fixture.selectCells(anchor: 0, head: 3)
            waitForGeometryFrame()

            let payload = try XCTUnwrap(fixture.recorder.payloads.last)
            XCTAssertEqual(try GeometryRecorder.rect(try XCTUnwrap(payload["keyboard"] as? [String: Double],
                                                                   "no keyboard in \(payload)")),
                           CGRect(x: 24, y: 252, width: 340, height: 80))
        }
    }

    func testMovingTheKeyboardGuideRepublishesTheOcclusionWithoutAKeyboardNotification() throws {
        try withExpoTableGeometry(document: fourCellDocument) { fixture in
            let docked = CGRect(x: 0, y: 180, width: 340, height: 200)
            let stub = StubKeyboardGuide(in: fixture.host, window: try XCTUnwrap(fixture.host.window), frame: docked)
            fixture.host.trackKeyboardOcclusion(of: stub.guide)
            try fixture.selectCells(anchor: 0, head: 3)
            waitForGeometryFrame()
            let published = fixture.recorder.payloads.count

            stub.move(to: CGRect(x: 60, y: 40, width: 120, height: 100))
            waitForGeometryFrame()

            XCTAssertEqual(fixture.recorder.payloads.count, published + 1, "\(fixture.recorder.payloads)")
            let payload = try XCTUnwrap(fixture.recorder.payloads.last)
            XCTAssertEqual(try GeometryRecorder.rect(try XCTUnwrap(payload["keyboard"] as? [String: Double],
                                                                   "no keyboard in \(payload)")),
                           CGRect(x: 84, y: 112, width: 120, height: 100),
                           "a floating keyboard is published where the guide moved it")
        }
    }

    func testViewportIsClippedByAClippingAncestor() throws {
        let clip = CGRect(x: 0, y: 120, width: 400, height: 150)
        try withExpoTableGeometry(document: fourCellDocument, clippingAncestor: clip) { fixture in
            try fixture.selectCells(anchor: 0, head: 3)
            waitForGeometryFrame()
            let payload = try XCTUnwrap(fixture.recorder.payloads.last)
            let viewport = try GeometryRecorder.rect(try XCTUnwrap(payload["viewport"] as? [String: Double]))
            let hostInWindow = try XCTUnwrap(fixture.host.superview).convert(fixture.host.frame, to: nil)

            XCTAssertEqual(viewport, hostInWindow.intersection(clip),
                           "the viewport excludes what a clipping ancestor hides")
            for rect in try fixture.recorder.rects(at: fixture.recorder.payloads.count - 1) {
                XCTAssertTrue(viewport.contains(rect), "\(rect) lies inside the clipped viewport \(viewport)")
            }
        }
    }

    func testBlurClearsGeometryAndSuppressesItUntilRefocus() throws {
        try withExpoTableGeometry(document: wideTwoCellDocument) { fixture in
            try fixture.selectCells(anchor: 0, head: 1)
            waitForGeometryFrame()
            XCTAssertEqual(fixture.recorder.payloads.count, 1)

            XCTAssertTrue(fixture.host.richTextView.textView.resignFirstResponder())
            XCTAssertEqual(fixture.recorder.payloads.count, 2, "blur clears synchronously")
            XCTAssertEqual(fixture.recorder.payloads[1] as? [String: String], ["editorId": fixture.adapter.editorId])
            let table = try XCTUnwrap(fixture.drawing.mountedTablePresentation()?.tables.first {
                $0.surface.identity == fixture.tableID
            })
            fixture.drawing.scrollTables(in: [table.surface.scrollIdentity], by: -120)
            waitForGeometryFrame()
            XCTAssertEqual(fixture.recorder.payloads.count, 2, "a blurred editor publishes no geometry")

            XCTAssertTrue(fixture.host.richTextView.textView.becomeFirstResponder())
            waitForGeometryFrame()
            XCTAssertEqual(fixture.recorder.payloads.count, 3)
            assertRects(try fixture.recorder.rects(at: 2), try fixture.expectedWindowRects())
        }
    }

    func testBindingChangeClearsGeometryUnderThePreviousEditor() throws {
        try withExpoTableGeometry(document: fourCellDocument) { fixture in
            try fixture.selectCells(anchor: 0, head: 3)
            waitForGeometryFrame()
            XCTAssertEqual(fixture.recorder.payloads.count, 1)

            fixture.host.setEditorId(0)

            XCTAssertEqual(fixture.recorder.payloads.count, 2)
            XCTAssertEqual(fixture.recorder.payloads[1] as? [String: String], ["editorId": fixture.adapter.editorId])
            XCTAssertFalse(fixture.host.tableSelectionGeometryPublisher.hasScheduledFlushForTesting)
        }
    }

    func testEditorDestructionClearsGeometry() throws {
        try withExpoTableGeometry(document: fourCellDocument) { fixture in
            try fixture.selectCells(anchor: 1, head: 2)
            waitForGeometryFrame()
            XCTAssertEqual(fixture.recorder.payloads.count, 1)

            fixture.host.handleEditorDestroyed(fixture.host.richTextView.editorId)

            XCTAssertEqual(fixture.recorder.payloads.count, 2)
            XCTAssertEqual(fixture.recorder.payloads[1] as? [String: String], ["editorId": fixture.adapter.editorId])
            waitForGeometryFrame()
            XCTAssertEqual(fixture.recorder.payloads.count, 2)
        }
    }

    func testTextSelectionsPublishNoGeometryAndEndACellSelectionsGeometry() throws {
        try withExpoTableGeometry(document: proseThenFixedWidthTableDocument) { fixture in
            try fixture.selectText(anchor: 1, head: 4)
            waitForGeometryFrame()
            XCTAssertTrue(fixture.recorder.payloads.isEmpty, "\(fixture.recorder.payloads)")

            try fixture.selectCells(anchor: 0, head: 1)
            waitForGeometryFrame()
            XCTAssertEqual(fixture.recorder.payloads.count, 1)

            try fixture.selectText(anchor: 2, head: 2)
            waitForGeometryFrame()
            XCTAssertEqual(fixture.recorder.payloads.count, 2, "\(fixture.recorder.payloads)")
            XCTAssertEqual(fixture.recorder.payloads[1] as? [String: String], ["editorId": fixture.adapter.editorId])
        }
    }

    func testBoundCellInputCarriesTheKeyboardToolbar() throws {
        try withExpoTableGeometry(document: fourCellDocument) { fixture in
            XCTAssertTrue(fixture.host.richTextView.textView.inputAccessoryView === fixture.host.accessoryToolbar,
                          "the focused root shows the keyboard toolbar")
            let input = try fixture.activateCell(1)
            XCTAssertTrue(input.inputAccessoryView === fixture.host.accessoryToolbar,
                          "the focused cell input shows the keyboard toolbar, got \(String(describing: input.inputAccessoryView))")
            XCTAssertTrue(fixture.host.isUsingAccessoryToolbarForTesting())
        }
    }

    func testCaretInABoundCellPublishesTheActiveCellGeometry() throws {
        try withExpoTableGeometry(document: fourCellDocument) { fixture in
            XCTAssertTrue(fixture.recorder.payloads.isEmpty, "a prose caret has no geometry: \(fixture.recorder.payloads)")
            let input = try fixture.activateCell(1)
            XCTAssertEqual(input.selectedRange.length, 0, "activation leaves a caret")
            XCTAssertTrue(fixture.drawing.selectedTableCellSourcePositions.isEmpty, "a caret draws no cell rectangle")
            waitForGeometryFrame()

            XCTAssertEqual(fixture.recorder.payloads.count, 1, "\(fixture.recorder.payloads)")
            let payload = try XCTUnwrap(fixture.recorder.payloads.first)
            XCTAssertEqual(payload["editorId"] as? String, fixture.adapter.editorId)
            let record = try XCTUnwrap(fixture.adapter.cachedTableRecords[fixture.tableID])
            XCTAssertEqual(payload["tablePos"] as? Int,
                           Int(try XCTUnwrap(EditorV2Adapter.uint32Field(record, "tablePos"))))
            assertRects(try fixture.recorder.rects(at: 0),
                        try fixture.expectedWindowRects(sourcePositions: [Int(fixture.positions[1])]),
                        "the active cell anchors the table toolbar")

            fixture.host.richTextView.invalidateTableCellBinding()
            waitForGeometryFrame()
            XCTAssertEqual(fixture.recorder.payloads.count, 2, "\(fixture.recorder.payloads)")
            XCTAssertEqual(fixture.recorder.payloads[1] as? [String: String], ["editorId": fixture.adapter.editorId],
                           "releasing the cell clears its geometry")
        }
    }

    func testKeyboardOverAFocusedCellInsetsTheRootAndRevealsTheCellCaret() throws {
        let keyboardHeight: CGFloat = 140
        try withExpoTableGeometry(document: try tallTableDocument(rowCount: 12)) { fixture in
            let root = fixture.host.richTextView.textView
            let surface = try XCTUnwrap(fixture.drawing.superview as? EditorTableSurface)
            let keyboardTopInHost = fixture.host.bounds.height - keyboardHeight
            let covered = try XCTUnwrap(fixture.positions.indices.first { index in
                guard let frame = surface.cellFrame(tableID: fixture.tableID, cellIndex: UInt32(index)) else {
                    return false
                }
                let hostFrame = surface.convert(frame, to: fixture.host)
                return hostFrame.minY > keyboardTopInHost && hostFrame.maxY < fixture.host.bounds.height
            }, "no visible cell lies under the keyboard")
            let input = try fixture.activateCell(covered)
            XCTAssertFalse(root.isFirstResponder)
            let window = try XCTUnwrap(fixture.host.window)
            let keyboardTop = fixture.host.convert(CGPoint(x: 0, y: keyboardTopInHost), to: window).y
            let keyboardFrame = window.convert(
                CGRect(x: 0, y: keyboardTop, width: window.bounds.width, height: window.bounds.maxY - keyboardTop),
                to: window.screen.coordinateSpace
            )
            NotificationCenter.default.post(
                name: UIResponder.keyboardWillChangeFrameNotification,
                object: nil,
                userInfo: [
                    UIResponder.keyboardFrameEndUserInfoKey: NSValue(cgRect: keyboardFrame),
                    UIResponder.keyboardAnimationDurationUserInfoKey: 0
                ]
            )
            fixture.host.layoutIfNeeded()

            XCTAssertGreaterThan(root.contentInset.bottom, 0, "the root reserves room for the keyboard")
            let caret = input.caretRect(for: try XCTUnwrap(input.selectedTextRange).end)
            let caretInWindow = input.convert(caret, to: window)
            XCTAssertLessThanOrEqual(caretInWindow.maxY, keyboardTop,
                                     "the focused cell's caret \(caretInWindow) scrolls above the keyboard at \(keyboardTop)")
            NotificationCenter.default.post(name: UIResponder.keyboardWillHideNotification, object: nil)
            XCTAssertEqual(root.contentInset.bottom, 0, accuracy: 0.5)
        }
    }

    func testFocusSurvivesTheRootToCellHandoffAndClearsOnRealBlur() throws {
        try withExpoTableGeometry(document: fourCellDocument) { fixture in
            var focusEvents: [[String: Any]] = []
            fixture.host.onFocusChangeForTesting = { focusEvents.append($0) }
            let input = try fixture.activateCell(1)
            RunLoop.main.run(until: Date())
            XCTAssertTrue(input.isFirstResponder)
            XCTAssertEqual(focusEvents.compactMap { $0["isFocused"] as? Bool }, [],
                           "handing focus from the root to the cell input is not a blur: \(focusEvents)")

            fixture.host.blur()
            RunLoop.main.run(until: Date())
            XCTAssertFalse(input.isFirstResponder, "blur resigns the focused cell input")
            XCTAssertEqual(focusEvents.compactMap { $0["isFocused"] as? Bool }, [false], "\(focusEvents)")
            XCTAssertEqual(focusEvents.last?["editorId"] as? String, fixture.adapter.editorId)
        }
    }

    func testRowSelectedFromAFocusedCellKeepsFocusAndPublishesTheRectangle() throws {
        try withExpoTableGeometry(document: fourCellDocument) { fixture in
            let input = try fixture.activateCell(0)
            waitForGeometryFrame()
            var focusEvents: [[String: Any]] = []
            fixture.host.onFocusChangeForTesting = { focusEvents.append($0) }
            let before = fixture.recorder.payloads.count
            let selectRow = try XCTUnwrap(input.accessibilityCustomActions?.first {
                $0.name == TableAccessibilityAction.all.first { $0.key == Self.selectRowsKey }?.label
            }, "the focused cell offers no select-row action")
            XCTAssertTrue(selectRow.actionHandler?(selectRow) ?? false, "select row was refused")
            RunLoop.main.run(until: Date())

            XCTAssertEqual(focusEvents.compactMap { $0["isFocused"] as? Bool }, [],
                           "leaving the cell for a rectangle is not a blur: \(focusEvents)")
            XCTAssertTrue(fixture.host.richTextView.textView.isFirstResponder, "the root takes focus for the rectangle")
            XCTAssertEqual(fixture.drawing.selectedTableCellSourcePositions[fixture.tableID],
                           Set([Int(fixture.positions[0]), Int(fixture.positions[1])]))
            waitForGeometryFrame()
            XCTAssertGreaterThan(fixture.recorder.payloads.count, before, "\(fixture.recorder.payloads)")
            assertRects(try fixture.recorder.rects(at: fixture.recorder.payloads.count - 1),
                        try fixture.expectedWindowRects(), "the row rectangle anchors the table toolbar")
        }
    }

    func testTappingACellInAnUnfocusedEditorEmitsFocus() throws {
        try withExpoTableGeometry(document: fourCellDocument) { fixture in
            fixture.host.blur()
            RunLoop.main.run(until: Date())
            var focusEvents: [[String: Any]] = []
            fixture.host.onFocusChangeForTesting = { focusEvents.append($0) }
            _ = try fixture.activateCell(1)
            RunLoop.main.run(until: Date())
            XCTAssertEqual(focusEvents.compactMap { $0["isFocused"] as? Bool }, [true], "\(focusEvents)")
            XCTAssertEqual(focusEvents.last?["editorId"] as? String, fixture.adapter.editorId)
        }
    }

    func testBlurClearsTheActiveCellGeometry() throws {
        try withExpoTableGeometry(document: fourCellDocument) { fixture in
            let input = try fixture.activateCell(1)
            waitForGeometryFrame()
            XCTAssertEqual(fixture.recorder.payloads.count, 1, "\(fixture.recorder.payloads)")
            fixture.host.blur()
            XCTAssertFalse(input.isFirstResponder)
            XCTAssertEqual(fixture.recorder.payloads.count, 2, "blur clears synchronously: \(fixture.recorder.payloads)")
            XCTAssertEqual(fixture.recorder.payloads[1] as? [String: String], ["editorId": fixture.adapter.editorId])
            XCTAssertTrue(fixture.host.richTextView.activeTextInput === input, "blur keeps the cell bound")
            waitForGeometryFrame()
            XCTAssertEqual(fixture.recorder.payloads.count, 2, "a blurred cell publishes no geometry")
        }
    }

    func testNativeCommandPreflightIsBlockedWhileACellIsActive() throws {
        try withExpoTableGeometry(document: fourCellDocument) { fixture in
            _ = try fixture.activateCell(1)
            let preparation = try XCTUnwrap(JSONSerialization.jsonObject(
                with: Data(fixture.host.prepareForEditorCommandJSON().utf8)
            ) as? [String: Any])
            XCTAssertEqual(preparation["ready"] as? Bool, false, "\(preparation)")
            XCTAssertEqual(preparation["blockedReason"] as? String, "composition", "\(preparation)")
        }
    }

    func testKeyboardToolbarMarkPressTogglesTheMarkInTheBoundCell() throws {
        try withExpoTableGeometry(configJson: TableInputTestSchema.strongMarkTableConfig,
                                  document: fourCellDocument) { fixture in
            fixture.host.setToolbarButtonsJson(TableToolbarTestItems.strongJson)
            let input = try fixture.activateCell(1)
            input.selectedRange = NSRange(location: 0, length: input.textStorage.length)
            input.textViewDidChangeSelection(input)
            try input.pressAccessoryToolbarButton(labeled: TableToolbarTestItems.strongLabel)

            let document = try XCTUnwrap(JSONSerialization.jsonObject(
                with: Data(try XCTUnwrap(fixture.adapter.documentJson()).utf8)
            ) as? [String: Any])
            let table = try XCTUnwrap((document["content"] as? [[String: Any]])?.first)
            let row = try XCTUnwrap((table["content"] as? [[String: Any]])?.first)
            let cells = try XCTUnwrap(row["content"] as? [[String: Any]])
            func runs(_ cell: [String: Any]) -> [[String: Any]] {
                ((cell["content"] as? [[String: Any]])?.first?["content"] as? [[String: Any]]) ?? []
            }
            XCTAssertEqual(runs(cells[1]).map { $0["text"] as? String }, ["two"], "\(cells[1])")
            XCTAssertEqual(runs(cells[1]).first?["marks"] as? [[String: String]], [["type": TableToolbarTestItems.strongMark]],
                           "the toolbar marks the focused cell's text: \(cells[1])")
            XCTAssertNil(runs(cells[0]).first?["marks"], "the root's former caret cell is untouched: \(cells[0])")
        }
    }

    private func authoritativeSelection(_ adapter: EditorV2Adapter) throws -> [String: Any] {
        let atomic = try XCTUnwrap(adapter.cachedAtomicRenderJSON)
        let snapshot = try XCTUnwrap(JSONSerialization.jsonObject(with: Data(atomic.utf8)) as? [String: Any])
        return try XCTUnwrap(snapshot["selection"] as? [String: Any])
    }

    func testKeyboardToolbarUndoAndRedoApplyUnderACellRectangle() throws {
        try withExpoTableGeometry(document: fourCellDocument) { fixture in
            fixture.host.setToolbarButtonsJson(TableToolbarTestItems.historyJson)
            let root = fixture.host.richTextView.textView
            let original = try fixture.adapter.tableCellTexts()
            let originalSelection = try authoritativeSelection(fixture.adapter) as NSDictionary
            try fixture.activateCell(1).insertText("X")
            let edited = try fixture.adapter.tableCellTexts()
            let editedSelection = try authoritativeSelection(fixture.adapter) as NSDictionary
            XCTAssertEqual(edited, [["one", "twoX"], ["three", "four"]], "typing edits the cell")
            try fixture.selectCells(anchor: 0, head: 1)
            XCTAssertEqual(try authoritativeSelection(fixture.adapter)["type"] as? String, "cell")
            XCTAssertTrue(root.authoritativeCellSelectionActive, "the rectangle is authoritative on the root")
            XCTAssertTrue(fixture.host.richTextView.activeTextInput === root, "the rectangle retires the cell input")

            let view = fixture.host.richTextView
            let cellInput = try XCTUnwrap(view.textInputs.last { $0 !== root })
            let focus = recordFocus(fixture.host)

            try root.pressAccessoryToolbarButton(labeled: TableToolbarTestItems.undoLabel)
            XCTAssertEqual(try fixture.adapter.tableCellTexts(), original, "undo under a rectangle restores the document")
            XCTAssertEqual(try authoritativeSelection(fixture.adapter) as NSDictionary, originalSelection,
                           "undo resolves to the selection before the edit")
            XCTAssertFalse(root.authoritativeCellSelectionActive, "undo leaves no stale rectangle on the root")
            XCTAssertEqual(fixture.drawing.selectedTableCellSourcePositions, [:], "undo clears the drawn rectangle")
            XCTAssertTrue(view.activeTextInput === cellInput, "the cell holding the restored caret takes the input")
            XCTAssertEqual(view.activeTableCellPosition, fixture.positions[0])
            XCTAssertTrue(cellInput.isFirstResponder, "the bound cell takes focus")
            XCTAssertFalse(root.isFirstResponder)
            XCTAssertEqual(cellInput.textStorage.string, "one")
            XCTAssertEqual(cellInput.selectedRange, NSRange(location: 0, length: 0), "the caret is restored in the cell")

            try cellInput.pressAccessoryToolbarButton(labeled: TableToolbarTestItems.redoLabel)
            XCTAssertEqual(try fixture.adapter.tableCellTexts(), edited, "redo reapplies the edit")
            XCTAssertEqual(try authoritativeSelection(fixture.adapter) as NSDictionary, editedSelection,
                           "redo resolves to the selection after the edit")
            XCTAssertEqual(view.activeTableCellPosition, fixture.positions[1])
            XCTAssertTrue(cellInput.isFirstResponder)
            XCTAssertEqual(cellInput.selectedRange, NSRange(location: 4, length: 0))
            RunLoop.main.run(until: Date().addingTimeInterval(0.05))
            XCTAssertFalse(focus.events.contains(false), "the rectangle-to-cell handoff never blurs: \(focus.events)")

            fixture.host.setEditable(false)
            root.performToolbarUndo()
            XCTAssertEqual(try fixture.adapter.tableCellTexts(), edited, "a read-only editor refuses undo")
        }
    }

    func testKeyboardToolbarUndoUnderACellRectangleKeepsARestoredProseCaretOnTheRoot() throws {
        try withExpoTableGeometry(document: proseBeforeTableDocument) { fixture in
            fixture.host.setToolbarButtonsJson(TableToolbarTestItems.historyJson)
            let view = fixture.host.richTextView
            let root = view.textView
            let originalSelection = try authoritativeSelection(fixture.adapter) as NSDictionary
            try fixture.activateCell(1).insertText("X")
            try fixture.selectCells(anchor: 0, head: 1)
            XCTAssertTrue(view.activeTextInput === root)
            XCTAssertTrue(root.isFirstResponder)
            let focus = recordFocus(fixture.host)

            try root.pressAccessoryToolbarButton(labeled: TableToolbarTestItems.undoLabel)
            XCTAssertEqual(try fixture.adapter.tableCellTexts(), [["one", "two"], ["three", "four"]])
            XCTAssertEqual(try authoritativeSelection(fixture.adapter) as NSDictionary, originalSelection)
            XCTAssertTrue(view.activeTextInput === root, "a restored prose caret stays on the root")
            XCTAssertNil(view.activeTableCellPosition)
            XCTAssertTrue(root.isFirstResponder)
            XCTAssertEqual(root.selectedRange, NSRange(location: 0, length: 0))
            XCTAssertFalse(root.rootTableSelectionInputBlocked)
            RunLoop.main.run(until: Date().addingTimeInterval(0.05))
            XCTAssertFalse(focus.events.contains(false), "\(focus.events)")
        }
    }

    private final class FocusRecorder {
        var events: [Bool] = []
    }

    private func recordFocus(_ host: NativeEditorExpoView) -> FocusRecorder {
        let recorder = FocusRecorder()
        host.onFocusChangeForTesting = { event in
            if let focused = event["isFocused"] as? Bool { recorder.events.append(focused) }
        }
        return recorder
    }

    func testKeyboardToolbarUndoAndRedoRebindTheCellHoldingTheRestoredCaret() throws {
        try withExpoTableGeometry(document: fourCellDocument) { fixture in
            fixture.host.setToolbarButtonsJson(TableToolbarTestItems.historyJson)
            let view = fixture.host.richTextView
            let original = try fixture.adapter.tableCellTexts()
            let input = try fixture.activateCell(0)
            input.selectedRange = NSRange(location: input.textStorage.length, length: 0)
            input.textViewDidChangeSelection(input)
            let originalSelection = try authoritativeSelection(fixture.adapter) as NSDictionary
            input.insertText("X")
            let edited = try fixture.adapter.tableCellTexts()
            let editedSelection = try authoritativeSelection(fixture.adapter) as NSDictionary
            XCTAssertEqual(edited, [["oneX", "two"], ["three", "four"]], "typing edits the first cell")
            XCTAssertTrue(try fixture.activateCell(1) === input)
            let editedPositions = try fixture.adapter.tableCellPositions()
            XCTAssertEqual(view.activeTableCellPosition, editedPositions[1], "the second cell is bound")
            let focus = recordFocus(fixture.host)

            let authority = input.tableCellInputAuthority
            input.tableCellInputAuthority = { false }
            input.performToolbarUndo()
            XCTAssertEqual(try fixture.adapter.tableCellTexts(), edited, "a cell without binding authority refuses undo")
            input.tableCellInputAuthority = authority

            try input.pressAccessoryToolbarButton(labeled: TableToolbarTestItems.undoLabel)
            XCTAssertEqual(try fixture.adapter.tableCellTexts(), original, "undo through the bound cell restores the document")
            XCTAssertEqual(try authoritativeSelection(fixture.adapter) as NSDictionary, originalSelection,
                           "undo resolves to the selection before the edit")
            XCTAssertTrue(view.activeTextInput === input, "the restored caret's cell keeps the cell input")
            XCTAssertEqual(view.activeTableCellPosition, fixture.positions[0], "the earlier cell holding the caret is bound")
            XCTAssertTrue(input.isFirstResponder, "the rebound cell keeps focus")
            XCTAssertEqual(input.textStorage.string, "one")
            let localCaret = try XCTUnwrap(input.currentScalarSelection())
            let restoredCaret = try XCTUnwrap(input.inputScalarRange(fromLocal: localCaret.anchor, toLocal: localCaret.head))
            XCTAssertEqual(Int(restoredCaret.from), originalSelection["anchorScalar"] as? Int, "the caret is restored in the cell")
            XCTAssertEqual(Int(restoredCaret.to), originalSelection["headScalar"] as? Int)

            try input.pressAccessoryToolbarButton(labeled: TableToolbarTestItems.redoLabel)
            XCTAssertEqual(try fixture.adapter.tableCellTexts(), edited, "redo reapplies the edit")
            XCTAssertEqual(try authoritativeSelection(fixture.adapter) as NSDictionary, editedSelection,
                           "redo resolves to the selection after the edit")
            XCTAssertEqual(view.activeTableCellPosition, editedPositions[0], "redo keeps the edited cell bound")
            XCTAssertTrue(input.isFirstResponder, "the rebound cell keeps focus")
            XCTAssertEqual(input.textStorage.string, "oneX")
            XCTAssertEqual(input.selectedRange, NSRange(location: 4, length: 0), "the caret follows the reapplied edit")
            RunLoop.main.run(until: Date().addingTimeInterval(0.05))
            XCTAssertFalse(focus.events.contains(false), "moving between cells never blurs: \(focus.events)")
        }
    }

    func testKeyboardToolbarUndoRestoringAProseCaretHandsFocusToTheRoot() throws {
        try withExpoTableGeometry(document: proseBeforeTableDocument) { fixture in
            fixture.host.setToolbarButtonsJson(TableToolbarTestItems.historyJson)
            let view = fixture.host.richTextView
            let root = view.textView
            let originalSelection = try authoritativeSelection(fixture.adapter) as NSDictionary
            XCTAssertEqual(originalSelection["anchorScalar"] as? Int, 0, "the caret starts in the prose")
            let input = try fixture.activateCell(1)
            input.insertText("X")
            XCTAssertEqual(try fixture.adapter.tableCellTexts(), [["one", "twoX"], ["three", "four"]])
            let focus = recordFocus(fixture.host)

            try input.pressAccessoryToolbarButton(labeled: TableToolbarTestItems.undoLabel)
            XCTAssertEqual(try fixture.adapter.tableCellTexts(), [["one", "two"], ["three", "four"]])
            XCTAssertEqual(try authoritativeSelection(fixture.adapter) as NSDictionary, originalSelection)
            XCTAssertTrue(view.activeTextInput === root, "a prose caret retires the cell input")
            XCTAssertNil(view.activeTableCellPosition)
            XCTAssertTrue(root.isFirstResponder, "the root takes over focus")
            XCTAssertFalse(input.isFirstResponder)
            XCTAssertEqual(root.selectedRange, NSRange(location: 0, length: 0), "the root shows the restored caret")
            XCTAssertFalse(root.rootTableSelectionInputBlocked, "the restored prose caret accepts input")
            RunLoop.main.run(until: Date().addingTimeInterval(0.05))
            XCTAssertFalse(focus.events.contains(false), "the cell-to-root handoff never blurs: \(focus.events)")
        }
    }

    func testKeyboardToolbarUndoIsRefusedWhileTheCellCannotDrain() throws {
        try withExpoTableGeometry(document: fourCellDocument) { fixture in
            fixture.host.setToolbarButtonsJson(TableToolbarTestItems.historyJson)
            let input = try fixture.activateCell(1)
            input.insertText("X")
            let edited = try fixture.adapter.tableCellTexts()
            input.blockExternalEditorUpdatePreparationForTesting = true
            try input.pressAccessoryToolbarButton(labeled: TableToolbarTestItems.undoLabel)
            XCTAssertEqual(try fixture.adapter.tableCellTexts(), edited, "an undrained cell refuses undo")
            XCTAssertEqual(fixture.host.richTextView.activeTableCellPosition, fixture.positions[1])
            input.blockExternalEditorUpdatePreparationForTesting = false
            try input.pressAccessoryToolbarButton(labeled: TableToolbarTestItems.undoLabel)
            XCTAssertEqual(try fixture.adapter.tableCellTexts(), [["one", "two"], ["three", "four"]],
                           "undo applies once the cell drains")
        }
    }

    func testKeyboardToolbarUndoSettlesTheBoundCellCompositionFirst() throws {
        try withExpoTableGeometry(document: fourCellDocument) { fixture in
            fixture.host.setToolbarButtonsJson(TableToolbarTestItems.historyJson)
            let input = try fixture.activateCell(1)
            input.insertText("X")
            input.setMarkedText("zz", selectedRange: NSRange(location: 2, length: 0))
            XCTAssertNotNil(input.markedTextRange, "the cell is composing")

            try input.pressAccessoryToolbarButton(labeled: TableToolbarTestItems.undoLabel)
            XCTAssertNil(input.markedTextRange, "undo commits the cell composition first")
            XCTAssertEqual(try fixture.adapter.tableCellTexts(), [["one", "twoX"], ["three", "four"]],
                           "undo reverts the committed composition as its own step")

            try fixture.host.richTextView.activeTextInput.pressAccessoryToolbarButton(labeled: TableToolbarTestItems.redoLabel)
            XCTAssertEqual(try fixture.adapter.tableCellTexts(), [["one", "twoXzz"], ["three", "four"]],
                           "redo restores the committed composition exactly once")
        }
    }

    func testSyntheticPreservedFailureFrameAdmitsCellSelectionWithoutGeometry() throws {
        let editorId = makeV2Editor(configJson: tableConfig)
        defer { destroyV2Editor(id: editorId) }
        let adapter = try XCTUnwrap(EditorV2Registry.adapter(forLegacyId: editorId))
        let document = #"{"type":"doc","content":[{"type":"table","content":[{"type":"table_row","content":[{"type":"table_cell","content":[{"type":"paragraph","content":[{"type":"text","text":"one"}]}]},{"type":"table_cell","content":[{"type":"paragraph","content":[{"type":"text","text":"two"}]}]}]}]}]}"#
        _ = try XCTUnwrap(adapter.setContentJson(document))
        let tableID = try XCTUnwrap(adapter.cachedTableRecords.keys.first)
        let cells = try XCTUnwrap(adapter.cachedTableRecords[tableID]?["cells"] as? [[String: Any]])
        let first = try XCTUnwrap(cells[0]["sourcePos"] as? Int)
        let second = try XCTUnwrap(cells[1]["sourcePos"] as? Int)
        let request = #"{"version":1,"requestId":"991106","baseDocumentRevision":"\#(adapter.baseDocumentRevision)","selection":{"type":"cell","anchorCell":{"offset":\#(EditorV2Shadow.docToScalar(id: editorId, docPos: UInt32(first + 2))),"kind":"scalar"},"headCell":{"offset":\#(EditorV2Shadow.docToScalar(id: editorId, docPos: UInt32(second + 2))),"kind":"scalar"}}}"#
        XCTAssertNil(editorV2SetSelection(editorId: adapter.editorId, requestJson: request).error)
        let raw = try XCTUnwrap(editorV2RenderUpdate(editorId: adapter.editorId, mirrorScalarAnchor: nil, mirrorScalarHead: nil).value)
        var snapshot = try XCTUnwrap(JSONSerialization.jsonObject(with: Data(raw.utf8)) as? [String: Any])
        var records = try XCTUnwrap(snapshot["tableRecords"] as? [String: [String: Any]])
        var record = try XCTUnwrap(records[tableID])
        record["rows"] = 0
        record["columns"] = 0
        record["columnWidths"] = []
        record["sourceRows"] = []
        record["cells"] = []
        record["syntheticRegions"] = []
        record["failure"] = "gridLimit"
        record["compatibilityDiagnostic"] = NSNull()
        records[tableID] = record
        snapshot["tableRecords"] = records
        let attributes = try XCTUnwrap(snapshot["tableAttributes"] as? [String: String])
        let tableAttrsKey = try XCTUnwrap(record["attrsKey"] as? String)
        snapshot["tableAttributes"] = attributes.filter { $0.key == tableAttrsKey }
        snapshot.removeValue(forKey: "tableInputMappings")
        let selection = try XCTUnwrap(snapshot["selection"] as? [String: Any])
        XCTAssertEqual(EditorCellSelection.resolve(selection, records: records), .unavailable(tableID: tableID))
        let failureJSON = try XCTUnwrap(String(data: JSONSerialization.data(withJSONObject: snapshot), encoding: .utf8))
        XCTAssertNotNil(EditorV2Adapter.parseAtomicRenderSnapshot(failureJSON))

        var malformed = snapshot
        malformed["selection"] = selection.merging(["anchorScalar": 0]) { _, new in new }
        let malformedJSON = try XCTUnwrap(String(data: JSONSerialization.data(withJSONObject: malformed), encoding: .utf8))
        XCTAssertNil(EditorV2Adapter.parseAtomicRenderSnapshot(malformedJSON))
    }

    func testHostRoutesCellEditThroughRootAdapterUsingGeneratedSnapshotMapping() throws {
        let editorId = makeV2Editor(configJson: tableConfig)
        defer { destroyV2Editor(id: editorId) }
        let adapter = try XCTUnwrap(EditorV2Registry.adapter(forLegacyId: editorId))
        let document = #"{"type":"doc","content":[{"type":"paragraph","content":[{"type":"text","text":"before"}]},{"type":"table","content":[{"type":"table_row","content":[{"type":"table_cell","content":[{"type":"paragraph","content":[{"type":"text","text":"first"}]}]},{"type":"table_cell","content":[{"type":"paragraph","content":[{"type":"text","text":"second"}]}]}]}]},{"type":"paragraph","content":[{"type":"text","text":"after"}]}]}"#
        let view = RichTextEditorView(frame: CGRect(x: 0, y: 0, width: 320, height: 200))
        view.bindEditor(id: editorId, initialUpdateJSON: try XCTUnwrap(adapter.initialUpdateJSON()))
        let update = try XCTUnwrap(adapter.setContentJson(document))
        XCTAssertTrue(view.textView.applyUpdateJSON(update))

        XCTAssertNotNil(adapter.cachedTableInputMappings, "rebuilt engine must publish the snapshot sidecar")
        XCTAssertTrue(view.bindTableCell(tableID: "t8", cellIndex: 1, contentRect: CGRect(x: 8, y: 8, width: 140, height: 40)))
        XCTAssertEqual(view.activeTextInput.currentLogicalScalarSelection()?.head, 13)
        view.activeTextInput.insertText("!")

        let documentJSON = try XCTUnwrap(adapter.documentJson())
        XCTAssertTrue(documentJSON.contains(#""text":"!second""#), documentJSON)
        XCTAssertTrue(documentJSON.contains(#""text":"first""#), documentJSON)
        XCTAssertTrue(view.textView.ownsNativeBinding(adapter))
        XCTAssertFalse(view.activeTextInput.ownsNativeBinding(adapter))
        XCTAssertEqual(view.activeTextInput.currentLogicalScalarSelection()?.head, 14)
        view.activeTextInput.insertText("😀")

        let secondDocumentJSON = try XCTUnwrap(adapter.documentJson())
        XCTAssertTrue(secondDocumentJSON.contains(#""text":"!😀second""#), secondDocumentJSON)
        XCTAssertEqual(view.activeTextInput.currentLogicalScalarSelection()?.head, 15)
    }

    func testRootTableUsesPreparedViewerPresentation() throws {
        let editorId = makeV2Editor(configJson: tableConfig)
        defer { destroyV2Editor(id: editorId) }
        let adapter = try XCTUnwrap(EditorV2Registry.adapter(forLegacyId: editorId))
        let document = #"{"type":"doc","content":[{"type":"paragraph","content":[{"type":"text","text":"before"}]},{"type":"table","content":[{"type":"table_row","content":[{"type":"table_cell","content":[{"type":"paragraph","content":[{"type":"text","text":"cell"}]}]}]}]},{"type":"paragraph","content":[{"type":"text","text":"after"}]}]}"#
        let view = RichTextEditorView(frame: CGRect(x: 0, y: 0, width: 320, height: 240))
        view.bindEditor(id: editorId, initialUpdateJSON: try XCTUnwrap(adapter.initialUpdateJSON()))
        XCTAssertTrue(view.textView.applyUpdateJSON(try XCTUnwrap(adapter.setContentJson(document))))
        view.layoutIfNeeded()

        let tableSurface = try XCTUnwrap(view.subviews.compactMap { $0 as? EditorTableSurface }.first)
        let drawing = try XCTUnwrap(tableSurface.subviews.compactMap { $0 as? PreparedProseDrawingView }.first)
        let layout = try XCTUnwrap(drawing.layout)
        XCTAssertFalse(drawing.isOpaque)
        XCTAssertEqual(layout.blocks.compactMap(\.tableSurface).count, 1)
        XCTAssertEqual(layout.blocks.compactMap(\.tableSurface).first?.cells.count, 1)

        var paintedCells = 0
        var paintedText = 0
        drawing.onTableChromeDrawnForTesting = { _ in paintedCells += 1 }
        drawing.onTableRichFragmentDrawnForTesting = { paintedText += 1 }
        let format = UIGraphicsImageRendererFormat()
        format.scale = 1
        _ = UIGraphicsImageRenderer(size: drawing.bounds.size, format: format).image { _ in
            drawing.draw(drawing.bounds)
        }
        XCTAssertGreaterThan(paintedCells, 0)
        XCTAssertGreaterThan(paintedText, 0)
    }

    func testRootTableKeepsScalarMarkerAndTracksItsRealCellAfterScroll() throws {
        let editorId = makeV2Editor(configJson: tableConfig)
        defer { destroyV2Editor(id: editorId) }
        let adapter = try XCTUnwrap(EditorV2Registry.adapter(forLegacyId: editorId))
        let document = #"{"type":"doc","content":[{"type":"paragraph","content":[{"type":"text","text":"before"}]},{"type":"table","content":[{"type":"table_row","content":[{"type":"table_cell","content":[{"type":"paragraph","content":[{"type":"text","text":"cell"}]}]}]}]},{"type":"paragraph","content":[{"type":"text","text":"after"}]}]}"#
        let view = RichTextEditorView(frame: CGRect(x: 0, y: 0, width: 320, height: 100))
        view.bindEditor(id: editorId, initialUpdateJSON: try XCTUnwrap(adapter.initialUpdateJSON()))
        XCTAssertTrue(view.textView.applyUpdateJSON(try XCTUnwrap(adapter.setContentJson(document))))
        view.layoutIfNeeded()

        let tableID = try XCTUnwrap(adapter.cachedTableInputMappings?.tables.keys.first)
        let extent = try XCTUnwrap(adapter.cachedTableInputMappings?.tables[tableID]?.extent)
        let marker = (view.textView.text as NSString).range(of: "\u{200B}")
        XCTAssertNotEqual(marker.location, NSNotFound)
        XCTAssertEqual(PositionBridge.utf16OffsetToScalar(marker.location, in: view.textView), extent.scalarStart)
        XCTAssertEqual(PositionBridge.utf16OffsetToScalar(NSMaxRange(marker), in: view.textView), extent.scalarEnd)

        let tableSurface = try XCTUnwrap(view.subviews.compactMap { $0 as? EditorTableSurface }.first)
        let drawing = try XCTUnwrap(tableSurface.subviews.compactMap { $0 as? PreparedProseDrawingView }.first)
        let contentLayout = try XCTUnwrap(drawing.layout)
        let tableBounds = try XCTUnwrap(drawing.layout?.blocks.first?.tableBounds)
        let after = (view.textView.text as NSString).range(of: "after")
        XCTAssertNotEqual(after.location, NSNotFound)
        view.textView.layoutManager.ensureLayout(forCharacterRange: after)
        let afterGlyphs = view.textView.layoutManager.glyphRange(forCharacterRange: after, actualCharacterRange: nil)
        let afterLine = view.textView.layoutManager.lineFragmentRect(forGlyphAt: afterGlyphs.location, effectiveRange: nil)
        XCTAssertGreaterThanOrEqual(
            view.textView.textContainerInset.top + afterLine.minY,
            tableBounds.maxY - 0.5
        )
        let firstFrame = try XCTUnwrap(tableSurface.cellFrame(tableID: tableID, cellIndex: 0))
        let firstCenter = CGPoint(x: firstFrame.midX, y: firstFrame.midY)
        XCTAssertEqual(tableSurface.cellHit(at: firstCenter)?.tableID, tableID)
        XCTAssertEqual(tableSurface.cellHit(at: firstCenter)?.cellIndex, 0)

        let renderCalls = adapter.renderUpdateCallCountForTesting
        view.textView.contentOffset.y += 12
        tableSurface.updateGeometry(from: view.textView)
        XCTAssertEqual(adapter.renderUpdateCallCountForTesting, renderCalls)
        XCTAssertTrue(drawing.layout === contentLayout)
        let scrolledFrame = try XCTUnwrap(tableSurface.cellFrame(tableID: tableID, cellIndex: 0))
        XCTAssertEqual(scrolledFrame.minY, firstFrame.minY - 12, accuracy: 0.5)
        XCTAssertNil(tableSurface.cellHit(at: CGPoint(x: tableBounds.maxX + 1, y: scrolledFrame.midY)))
    }

    func testRootTableRepreparesForAppearanceAndHostWidthWithoutDocumentUpdate() throws {
        let editorId = makeV2Editor(configJson: tableConfig)
        defer { destroyV2Editor(id: editorId) }
        let adapter = try XCTUnwrap(EditorV2Registry.adapter(forLegacyId: editorId))
        let document = #"{"type":"doc","content":[{"type":"table","content":[{"type":"table_row","content":[{"type":"table_cell","content":[{"type":"paragraph","content":[{"type":"text","text":"cell"}]}]}]}]}]}"#
        let view = RichTextEditorView(frame: CGRect(x: 0, y: 0, width: 320, height: 160))
        view.bindEditor(id: editorId, initialUpdateJSON: try XCTUnwrap(adapter.initialUpdateJSON()))
        XCTAssertTrue(view.textView.applyUpdateJSON(try XCTUnwrap(adapter.setContentJson(document))))
        view.layoutIfNeeded()

        let tableSurface = try XCTUnwrap(view.subviews.compactMap { $0 as? EditorTableSurface }.first)
        let drawing = try XCTUnwrap(tableSurface.subviews.compactMap { $0 as? PreparedProseDrawingView }.first)
        let initialLayout = try XCTUnwrap(drawing.layout)
        let initialWidth = try XCTUnwrap(initialLayout.blocks.first?.tableSurface?.hostViewportWidth)
        let renderCalls = adapter.renderUpdateCallCountForTesting

        view.configure(font: .systemFont(ofSize: 22))
        let appearanceLayout = try XCTUnwrap(drawing.layout)
        XCTAssertFalse(initialLayout === appearanceLayout)
        XCTAssertEqual(adapter.renderUpdateCallCountForTesting, renderCalls)

        view.frame.size.width = 220
        view.layoutIfNeeded()

        let resizedWidth = try XCTUnwrap(drawing.layout?.blocks.first?.tableSurface?.hostViewportWidth)
        XCTAssertLessThan(resizedWidth, initialWidth)
        XCTAssertEqual(adapter.renderUpdateCallCountForTesting, renderCalls)
    }

    func testRootTableProjectsEditorThemeIntoInactiveCells() throws {
        let editorId = makeV2Editor(configJson: tableConfig)
        defer { destroyV2Editor(id: editorId) }
        let adapter = try XCTUnwrap(EditorV2Registry.adapter(forLegacyId: editorId))
        let document = #"{"type":"doc","content":[{"type":"table","content":[{"type":"table_row","content":[{"type":"table_cell","content":[{"type":"paragraph","content":[{"type":"text","text":"cell"}]}]}]}]}]}"#
        let view = RichTextEditorView(frame: CGRect(x: 0, y: 0, width: 320, height: 160))
        view.bindEditor(id: editorId, initialUpdateJSON: try XCTUnwrap(adapter.initialUpdateJSON()))
        XCTAssertTrue(view.textView.applyUpdateJSON(try XCTUnwrap(adapter.setContentJson(document))))
        XCTAssertTrue(view.applyTheme(EditorTheme(dictionary: [
            "text": ["fontSize": 22, "color": "#FF0000"],
            "table": ["cellPadding": 14]
        ])))

        let tableSurface = try XCTUnwrap(view.subviews.compactMap { $0 as? EditorTableSurface }.first)
        let drawing = try XCTUnwrap(tableSurface.subviews.compactMap { $0 as? PreparedProseDrawingView }.first)
        let cell = try XCTUnwrap(drawing.layout?.blocks.first?.tableSurface?.cells.first)
        let line = try XCTUnwrap(cell.content.blocks.flatMap(\.fragments).first(where: { $0.kind == .text })?.line)
        let run = try XCTUnwrap((CTLineGetGlyphRuns(line) as? [CTRun])?.first)
        let attributes = CTRunGetAttributes(run) as NSDictionary
        let color = try unwrapCoreTextAttribute(attributes[kCTForegroundColorAttributeName], as: CGColor.self)

        XCTAssertEqual(UIColor(cgColor: color), UIColor.red)
        XCTAssertEqual(drawing.layout?.blocks.first?.tableSurface?.style.cellPadding, 14)
    }

    func testRootTableUsesHostInsetWidthOnce() throws {
        let editorId = makeV2Editor(configJson: tableConfig)
        defer { destroyV2Editor(id: editorId) }
        let adapter = try XCTUnwrap(EditorV2Registry.adapter(forLegacyId: editorId))
        let document = #"{"type":"doc","content":[{"type":"table","content":[{"type":"table_row","content":[{"type":"table_cell","content":[{"type":"paragraph","content":[{"type":"text","text":"cell"}]}]}]}]}]}"#
        let view = RichTextEditorView(frame: CGRect(x: 0, y: 0, width: 320, height: 160))
        view.bindEditor(id: editorId, initialUpdateJSON: try XCTUnwrap(adapter.initialUpdateJSON()))
        XCTAssertTrue(view.textView.applyUpdateJSON(try XCTUnwrap(adapter.setContentJson(document))))
        XCTAssertTrue(view.applyTheme(EditorTheme(dictionary: [
            "version": 1,
            "styles": ["content": ["paddingLeft": 20, "paddingRight": 13]]
        ])))
        view.layoutIfNeeded()

        let tableSurface = try XCTUnwrap(view.subviews.compactMap { $0 as? EditorTableSurface }.first)
        let drawing = try XCTUnwrap(tableSurface.subviews.compactMap { $0 as? PreparedProseDrawingView }.first)
        let prepared = try XCTUnwrap(drawing.layout?.blocks.first?.tableSurface)
        XCTAssertEqual(view.textView.textContainerInset.left, 20)
        XCTAssertEqual(view.textView.textContainerInset.right, 13)
        XCTAssertEqual(prepared.hostViewportWidth, 287, accuracy: 0.5)
    }

    func testActiveHeaderCellSuppressesOnlyItsPreparedContent() throws {
        let editorId = makeV2Editor(configJson: tableConfig)
        defer { destroyV2Editor(id: editorId) }
        let adapter = try XCTUnwrap(EditorV2Registry.adapter(forLegacyId: editorId))
        let document = #"{"type":"doc","content":[{"type":"table","content":[{"type":"table_row","content":[{"type":"table_header","content":[{"type":"paragraph","content":[{"type":"text","text":"active"}]}]},{"type":"table_header","content":[{"type":"paragraph","content":[{"type":"text","text":"inactive"}]}]}]}]}]}"#
        let view = RichTextEditorView(frame: CGRect(x: 0, y: 0, width: 320, height: 180))
        view.bindEditor(id: editorId, initialUpdateJSON: try XCTUnwrap(adapter.initialUpdateJSON()))
        XCTAssertTrue(view.textView.applyUpdateJSON(try XCTUnwrap(adapter.setContentJson(document))))
        view.layoutIfNeeded()

        let tableID = try XCTUnwrap(adapter.cachedTableInputMappings?.tables.keys.first)
        let tableSurface = try XCTUnwrap(view.subviews.compactMap { $0 as? EditorTableSurface }.first)
        let drawing = try XCTUnwrap(tableSurface.subviews.compactMap { $0 as? PreparedProseDrawingView }.first)
        let block = try XCTUnwrap(drawing.layout?.blocks.first)
        let surface = try XCTUnwrap(block.tableSurface)
        let tableBounds = try XCTUnwrap(block.tableBounds)
        let activeHeader = try XCTUnwrap(surface.cells.first { $0.sourceCellIndex == 0 })
        let format = UIGraphicsImageRendererFormat()
        format.scale = 1
        func paint() -> (rich: Int, chrome: Int, image: CGImage?) {
            var rich = 0
            var chrome = 0
            drawing.onTableRichFragmentDrawnForTesting = { rich += 1 }
            drawing.onTableChromeDrawnForTesting = { _ in chrome += 1 }
            let image = UIGraphicsImageRenderer(size: drawing.bounds.size, format: format).image { _ in
                drawing.draw(drawing.bounds)
            }.cgImage
            return (rich, chrome, image)
        }

        let unbound = paint()
        XCTAssertGreaterThanOrEqual(unbound.rich, 2)
        XCTAssertEqual(unbound.chrome, 2)
        XCTAssertTrue(view.bindTableCell(tableID: tableID, cellIndex: 0, contentRect: .zero))
        let bound = paint()
        XCTAssertGreaterThan(bound.rich, 0)
        XCTAssertLessThan(bound.rich, unbound.rich)
        XCTAssertEqual(bound.chrome, 2)

        let image = try XCTUnwrap(bound.image)
        let point = CGPoint(x: tableBounds.minX + activeHeader.frame.minX + 3, y: tableBounds.minY + activeHeader.frame.minY + 3)
        var pixels = [UInt8](repeating: 0, count: image.width * image.height * 4)
        let context = try XCTUnwrap(CGContext(data: &pixels, width: image.width, height: image.height, bitsPerComponent: 8, bytesPerRow: image.width * 4, space: CGColorSpaceCreateDeviceRGB(), bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue | CGBitmapInfo.byteOrder32Big.rawValue))
        context.draw(image, in: CGRect(x: 0, y: 0, width: image.width, height: image.height))
        let pixelIndex = Int(point.y) * image.width * 4 + Int(point.x) * 4
        var red: CGFloat = 0
        var green: CGFloat = 0
        var blue: CGFloat = 0
        var alpha: CGFloat = 0
        XCTAssertTrue(surface.style.headerBackgroundColor.getRed(&red, green: &green, blue: &blue, alpha: &alpha))
        XCTAssertEqual(Array(pixels[pixelIndex..<(pixelIndex + 4)]), [red, green, blue, alpha].map { UInt8(($0 * 255).rounded()) })

        tableSurface.invalidateAppearance()
        tableSurface.updateGeometry(from: view.textView)
        let refreshedCell = try XCTUnwrap(drawing.layout?.blocks.first?.tableSurface?.cells.first { $0.sourceCellIndex == 0 })
        XCTAssertFalse(refreshedCell.content === activeHeader.content)
        XCTAssertEqual(paint().rich, bound.rich)

        view.invalidateTableCellBinding()
        let restored = paint()
        XCTAssertEqual(restored.rich, unbound.rich)
        XCTAssertEqual(restored.chrome, 2)
    }

    func testActiveCellInputStaysTransparentAcrossRootBackgrounds() throws {
        let editorId = makeV2Editor(configJson: tableConfig)
        defer { destroyV2Editor(id: editorId) }
        let adapter = try XCTUnwrap(EditorV2Registry.adapter(forLegacyId: editorId))
        let document = #"{"type":"doc","content":[{"type":"table","content":[{"type":"table_row","content":[{"type":"table_cell","content":[{"type":"paragraph","content":[{"type":"text","text":"cell"}]}]}]}]}]}"#
        let view = RichTextEditorView(frame: CGRect(x: 0, y: 0, width: 320, height: 180))
        view.bindEditor(id: editorId, initialUpdateJSON: try XCTUnwrap(adapter.initialUpdateJSON()))
        XCTAssertTrue(view.textView.applyUpdateJSON(try XCTUnwrap(adapter.setContentJson(document))))
        let tableID = try XCTUnwrap(adapter.cachedTableInputMappings?.tables.keys.first)

        for color in [UIColor.red, UIColor.clear] {
            view.textView.baseBackgroundColor = color
            view.textView.backgroundColor = color
            XCTAssertTrue(view.bindTableCell(tableID: tableID, cellIndex: 0, contentRect: .zero))
            XCTAssertEqual(view.activeTextInput.backgroundColor, .clear)
            XCTAssertFalse(view.activeTextInput.isOpaque)
        }
    }

    func testRootTableMarginsOffsetPaintAndReserveFollowingProse() throws {
        let editorId = makeV2Editor(configJson: tableConfig)
        defer { destroyV2Editor(id: editorId) }
        let adapter = try XCTUnwrap(EditorV2Registry.adapter(forLegacyId: editorId))
        let document = #"{"type":"doc","content":[{"type":"table","content":[{"type":"table_row","content":[{"type":"table_cell","content":[{"type":"paragraph","content":[{"type":"text","text":"cell"}]}]}]}]},{"type":"paragraph","content":[{"type":"text","text":"after"}]}]}"#
        let view = RichTextEditorView(frame: CGRect(x: 0, y: 0, width: 320, height: 180))
        view.bindEditor(id: editorId, initialUpdateJSON: try XCTUnwrap(adapter.initialUpdateJSON()))
        XCTAssertTrue(view.textView.applyUpdateJSON(try XCTUnwrap(adapter.setContentJson(document))))
        XCTAssertTrue(view.applyTheme(EditorTheme(dictionary: [
            "version": 1,
            "styles": ["table": ["marginTop": 17, "marginBottom": 23, "marginLeft": 11, "marginRight": 7]]
        ])))
        view.layoutIfNeeded()

        let tableSurface = try XCTUnwrap(view.subviews.compactMap { $0 as? EditorTableSurface }.first)
        let drawing = try XCTUnwrap(tableSurface.subviews.compactMap { $0 as? PreparedProseDrawingView }.first)
        let tableBounds = try XCTUnwrap(drawing.layout?.blocks.first?.tableBounds)
        let marker = (view.textView.text as NSString).range(of: "\u{200B}")
        XCTAssertNotEqual(marker.location, NSNotFound)
        let markerGlyph = view.textView.layoutManager.glyphRange(forCharacterRange: marker, actualCharacterRange: nil)
        let markerLine = view.textView.layoutManager.lineFragmentRect(forGlyphAt: markerGlyph.location, effectiveRange: nil)
        let anchorX = view.textView.textContainerInset.left + markerLine.minX
        let anchorY = view.textView.textContainerInset.top + markerLine.minY
        XCTAssertEqual(tableBounds.minX, anchorX + 11, accuracy: 0.5)
        XCTAssertEqual(tableBounds.minY, anchorY + 17, accuracy: 0.5)
        let markerStyle = try XCTUnwrap(view.textView.textStorage.attribute(.paragraphStyle, at: marker.location, effectiveRange: nil) as? NSParagraphStyle)
        XCTAssertGreaterThanOrEqual(markerStyle.minimumLineHeight, tableBounds.maxY + 22 - anchorY)
        let after = (view.textView.text as NSString).range(of: "after")
        XCTAssertNotEqual(after.location, NSNotFound)
        view.textView.layoutManager.ensureLayout(forCharacterRange: after)
        let afterGlyph = view.textView.layoutManager.glyphRange(forCharacterRange: after, actualCharacterRange: nil)
        let afterLine = view.textView.layoutManager.lineFragmentRect(forGlyphAt: afterGlyph.location, effectiveRange: nil)
        let newlineStyle = try XCTUnwrap(view.textView.textStorage.attribute(.paragraphStyle, at: marker.location + 1, effectiveRange: nil) as? NSParagraphStyle)
        XCTAssertEqual(newlineStyle.minimumLineHeight, markerStyle.minimumLineHeight, accuracy: 0.5)
        XCTAssertGreaterThanOrEqual(
            view.textView.textContainerInset.top + afterLine.minY,
            tableBounds.maxY + 22
        )
    }

    func testRootTableDeepScrollKeepsDrawingViewportMounted() throws {
        let editorId = makeV2Editor(configJson: tableConfig)
        defer { destroyV2Editor(id: editorId) }
        let adapter = try XCTUnwrap(EditorV2Registry.adapter(forLegacyId: editorId))
        let before: [[String: Any]] = (0..<20).map { index in
            ["type": "paragraph", "content": [["type": "text", "text": "before \(index)"]]]
        }
        let document = try XCTUnwrap(String(
            data: JSONSerialization.data(withJSONObject: [
                "type": "doc",
                "content": before + [[
                    "type": "table",
                    "content": [[
                        "type": "table_row",
                        "content": [[
                            "type": "table_cell",
                            "content": [[
                                "type": "paragraph",
                                "content": [["type": "text", "text": "cell"]]
                            ]]
                        ]]
                    ]]
                ]]
            ]),
            encoding: .utf8
        ))
        let view = RichTextEditorView(frame: CGRect(x: 0, y: 0, width: 320, height: 80))
        view.bindEditor(id: editorId, initialUpdateJSON: try XCTUnwrap(adapter.initialUpdateJSON()))
        XCTAssertTrue(view.textView.applyUpdateJSON(try XCTUnwrap(adapter.setContentJson(document))))
        view.layoutIfNeeded()

        let tableID = try XCTUnwrap(adapter.cachedTableInputMappings?.tables.keys.first)
        let tableSurface = try XCTUnwrap(view.subviews.compactMap { $0 as? EditorTableSurface }.first)
        let drawing = try XCTUnwrap(tableSurface.subviews.compactMap { $0 as? PreparedProseDrawingView }.first)
        let initialFrame = try XCTUnwrap(tableSurface.cellFrame(tableID: tableID, cellIndex: 0))
        XCTAssertGreaterThan(initialFrame.minY, view.bounds.height)
        let revision = adapter.baseDocumentRevision
        let history = adapter.cachedHistoryState
        let documentBeforeScroll = try XCTUnwrap(adapter.documentJson())

        let window = UIWindow(frame: view.bounds)
        window.addSubview(view)
        window.isHidden = false
        defer { window.isHidden = true }

        view.textView.contentOffset.y = initialFrame.minY - 10
        tableSurface.updateGeometry(from: view.textView)

        let visibleFrame = try XCTUnwrap(tableSurface.cellFrame(tableID: tableID, cellIndex: 0))
        XCTAssertEqual(visibleFrame.minY, 10, accuracy: 0.5)
        XCTAssertTrue(drawing.frame.intersects(tableSurface.bounds))
        XCTAssertLessThanOrEqual(drawing.bounds.height, view.bounds.height)
        var paintedCells = 0
        var paintedText = 0
        drawing.onTableChromeDrawnForTesting = { _ in paintedCells += 1 }
        drawing.onTableRichFragmentDrawnForTesting = { paintedText += 1 }
        let format = UIGraphicsImageRendererFormat()
        format.scale = 1
        _ = UIGraphicsImageRenderer(size: view.bounds.size, format: format).image { context in
            context.cgContext.translateBy(x: -drawing.bounds.minX, y: -drawing.bounds.minY)
            drawing.draw(drawing.bounds)
        }
        XCTAssertGreaterThan(paintedCells, 0)
        XCTAssertGreaterThan(paintedText, 0)
        XCTAssertEqual(adapter.baseDocumentRevision, revision)
        XCTAssertEqual(adapter.cachedHistoryState?.canUndo, history?.canUndo)
        XCTAssertEqual(adapter.cachedHistoryState?.canRedo, history?.canRedo)
        XCTAssertEqual(try XCTUnwrap(adapter.documentJson()), documentBeforeScroll)
    }

    func testInactiveCellTouchBelongsToRootScrollView() throws {
        let editorId = makeV2Editor(configJson: tableConfig)
        defer { destroyV2Editor(id: editorId) }
        let adapter = try XCTUnwrap(EditorV2Registry.adapter(forLegacyId: editorId))
        let document = #"{"type":"doc","content":[{"type":"table","content":[{"type":"table_row","content":[{"type":"table_cell","content":[{"type":"paragraph","content":[{"type":"text","text":"cell"}]}]}]}]}]}"#
        let view = RichTextEditorView(frame: CGRect(x: 0, y: 0, width: 320, height: 160))
        view.bindEditor(id: editorId, initialUpdateJSON: try XCTUnwrap(adapter.initialUpdateJSON()))
        XCTAssertTrue(view.textView.applyUpdateJSON(try XCTUnwrap(adapter.setContentJson(document))))
        view.layoutIfNeeded()

        let tableID = try XCTUnwrap(adapter.cachedTableInputMappings?.tables.keys.first)
        let tableSurface = try XCTUnwrap(view.subviews.compactMap { $0 as? EditorTableSurface }.first)
        let frame = try XCTUnwrap(tableSurface.cellFrame(tableID: tableID, cellIndex: 0))
        XCTAssertTrue(view.hitTest(CGPoint(x: frame.midX, y: frame.midY), with: nil) === view.textView)
        XCTAssertTrue(view.bindTableCell(tableID: tableID, cellIndex: 0, contentRect: .zero))
        XCTAssertTrue(view.hitTest(CGPoint(x: frame.midX, y: frame.midY), with: nil) === view.activeTextInput)
    }

    func testActiveCellInputUsesPreparedContentInsets() throws {
        let editorId = makeV2Editor(configJson: tableConfig)
        defer { destroyV2Editor(id: editorId) }
        let adapter = try XCTUnwrap(EditorV2Registry.adapter(forLegacyId: editorId))
        let document = #"{"type":"doc","content":[{"type":"table","content":[{"type":"table_row","content":[{"type":"table_cell","content":[{"type":"paragraph","content":[{"type":"text","text":"cell"}]}]}]}]}]}"#
        let view = RichTextEditorView(frame: CGRect(x: 0, y: 0, width: 320, height: 160))
        view.bindEditor(id: editorId, initialUpdateJSON: try XCTUnwrap(adapter.initialUpdateJSON()))
        XCTAssertTrue(view.textView.applyUpdateJSON(try XCTUnwrap(adapter.setContentJson(document))))
        XCTAssertTrue(view.applyTheme(EditorTheme(dictionary: ["contentInsets": ["top": 12, "left": 20]])))
        view.layoutIfNeeded()

        let tableID = try XCTUnwrap(adapter.cachedTableInputMappings?.tables.keys.first)
        let tableSurface = try XCTUnwrap(view.subviews.compactMap { $0 as? EditorTableSurface }.first)
        let drawing = try XCTUnwrap(tableSurface.subviews.compactMap { $0 as? PreparedProseDrawingView }.first)
        let cell = try XCTUnwrap(drawing.layout?.blocks.first?.tableSurface?.cells.first)
        XCTAssertTrue(view.bindTableCell(tableID: tableID, cellIndex: 0, contentRect: .zero))
        let input = view.activeTextInput
        let origin = try XCTUnwrap(drawing.layout?.blocks.first?.tableBounds?.origin)
        XCTAssertEqual(input.convert(.zero, to: tableSurface).x,
                       (origin.x + cell.frame.minX + cell.contentOrigin.x).rounded(), accuracy: 1)
        XCTAssertEqual(input.textContainerInset, .zero)
        XCTAssertEqual(input.textContainer.lineFragmentPadding, 0)
    }

    func testRootTableRebindWithSameRevisionReplacesPreparedContent() throws {
        let firstID = makeV2Editor(configJson: tableConfig)
        let secondID = makeV2Editor(configJson: tableConfig)
        defer { destroyV2Editor(id: firstID); destroyV2Editor(id: secondID) }
        let first = try XCTUnwrap(EditorV2Registry.adapter(forLegacyId: firstID))
        let second = try XCTUnwrap(EditorV2Registry.adapter(forLegacyId: secondID))
        let firstDocument = #"{"type":"doc","content":[{"type":"table","content":[{"type":"table_row","content":[{"type":"table_cell","content":[{"type":"paragraph","content":[{"type":"text","text":"first"}]}]}]}]}]}"#
        let secondDocument = #"{"type":"doc","content":[{"type":"table","content":[{"type":"table_row","content":[{"type":"table_cell","content":[{"type":"paragraph","content":[{"type":"text","text":"second"}]}]}]}]}]}"#
        let view = RichTextEditorView(frame: CGRect(x: 0, y: 0, width: 320, height: 160))
        view.bindEditor(id: firstID, initialUpdateJSON: try XCTUnwrap(first.initialUpdateJSON()))
        XCTAssertTrue(view.textView.applyUpdateJSON(try XCTUnwrap(first.setContentJson(firstDocument))))
        view.layoutIfNeeded()
        let tableSurface = try XCTUnwrap(view.subviews.compactMap { $0 as? EditorTableSurface }.first)
        let drawing = try XCTUnwrap(tableSurface.subviews.compactMap { $0 as? PreparedProseDrawingView }.first)
        let firstSurface = try XCTUnwrap(drawing.layout?.blocks.first?.tableSurface)

        _ = try XCTUnwrap(second.initialUpdateJSON())
        _ = try XCTUnwrap(second.setContentJson(secondDocument))
        XCTAssertEqual(first.baseDocumentRevision, second.baseDocumentRevision)
        view.bindEditor(id: secondID, initialUpdateJSON: try XCTUnwrap(second.initialUpdateJSON()))
        view.layoutIfNeeded()

        XCTAssertEqual(first.baseDocumentRevision, second.baseDocumentRevision)
        let secondSurface = try XCTUnwrap(drawing.layout?.blocks.first?.tableSurface)
        XCTAssertFalse(firstSurface === secondSurface)
        XCTAssertTrue(String(describing: secondSurface.sourceTable).contains("second"))
        let secondTableID = try XCTUnwrap(second.cachedTableInputMappings?.tables.keys.first)
        XCTAssertNotNil(second.positionEpoch)
        XCTAssertTrue(view.bindTableCell(tableID: secondTableID, cellIndex: 0, contentRect: .zero))
        XCTAssertTrue(view.textView.ownsNativeBinding(second))
    }

    func testPrepopulatedInitialBindRetainsTablePresentationAndCellAuthority() throws {
        let editorId = makeV2Editor(configJson: tableConfig)
        defer { destroyV2Editor(id: editorId) }
        let adapter = try XCTUnwrap(EditorV2Registry.adapter(forLegacyId: editorId))
        let document = #"{"type":"doc","content":[{"type":"table","content":[{"type":"table_row","content":[{"type":"table_cell","content":[{"type":"paragraph","content":[{"type":"text","text":"preloaded"}]}]}]}]}]}"#
        _ = try XCTUnwrap(adapter.setContentJson(document))
        let revision = adapter.baseDocumentRevision
        let history = adapter.cachedHistoryState
        let view = RichTextEditorView(frame: CGRect(x: 0, y: 0, width: 320, height: 160))
        view.bindEditor(id: editorId, initialUpdateJSON: try XCTUnwrap(adapter.initialUpdateJSON()))
        view.layoutIfNeeded()

        let tableSurface = try XCTUnwrap(view.subviews.compactMap { $0 as? EditorTableSurface }.first)
        let drawing = try XCTUnwrap(tableSurface.subviews.compactMap { $0 as? PreparedProseDrawingView }.first)
        XCTAssertTrue(String(describing: drawing.layout?.blocks.first?.tableSurface?.sourceTable).contains("preloaded"))
        let tableID = try XCTUnwrap(adapter.cachedTableInputMappings?.tables.keys.first)
        XCTAssertNotNil(adapter.positionEpoch)
        XCTAssertTrue(view.bindTableCell(tableID: tableID, cellIndex: 0, contentRect: .zero))
        XCTAssertTrue(view.textView.ownsNativeBinding(adapter))
        XCTAssertEqual(adapter.baseDocumentRevision, revision)
        XCTAssertEqual(adapter.cachedHistoryState?.canUndo, history?.canUndo)
        XCTAssertEqual(adapter.cachedHistoryState?.canRedo, history?.canRedo)
    }

    func testRootTableRecoversAfterZeroWidthLayout() throws {
        let editorId = makeV2Editor(configJson: tableConfig)
        defer { destroyV2Editor(id: editorId) }
        let adapter = try XCTUnwrap(EditorV2Registry.adapter(forLegacyId: editorId))
        let document = #"{"type":"doc","content":[{"type":"table","content":[{"type":"table_row","content":[{"type":"table_cell","content":[{"type":"paragraph","content":[{"type":"text","text":"cell"}]}]}]}]}]}"#
        let view = RichTextEditorView(frame: CGRect(x: 0, y: 0, width: 320, height: 160))
        view.bindEditor(id: editorId, initialUpdateJSON: try XCTUnwrap(adapter.initialUpdateJSON()))
        XCTAssertTrue(view.textView.applyUpdateJSON(try XCTUnwrap(adapter.setContentJson(document))))
        view.layoutIfNeeded()
        let tableSurface = try XCTUnwrap(view.subviews.compactMap { $0 as? EditorTableSurface }.first)
        let drawing = try XCTUnwrap(tableSurface.subviews.compactMap { $0 as? PreparedProseDrawingView }.first)
        XCTAssertNotNil(drawing.layout)

        view.frame.size.width = 0
        view.layoutIfNeeded()
        XCTAssertNil(drawing.layout)
        view.frame.size.width = 320
        view.layoutIfNeeded()
        XCTAssertNotNil(drawing.layout)
    }

    func testHostRejectsTableCellBindingWhenAnotherHostOwnsTheSession() throws {
        let editorId = makeV2Editor(configJson: tableConfig)
        defer { destroyV2Editor(id: editorId) }
        let adapter = try XCTUnwrap(EditorV2Registry.adapter(forLegacyId: editorId))
        let document = #"{"type":"doc","content":[{"type":"table","content":[{"type":"table_row","content":[{"type":"table_cell","content":[{"type":"paragraph","content":[{"type":"text","text":"cell"}]}]}]}]}]}"#
        let owner = RichTextEditorView(frame: .zero)
        let stale = RichTextEditorView(frame: .zero)
        owner.bindEditor(id: editorId, initialUpdateJSON: try XCTUnwrap(adapter.initialUpdateJSON()))
        XCTAssertTrue(owner.textView.applyUpdateJSON(try XCTUnwrap(adapter.setContentJson(document))))

        stale.bindEditor(id: editorId, initialUpdateJSON: try XCTUnwrap(adapter.initialUpdateJSON()))
        let tableID = try XCTUnwrap(adapter.cachedTableInputMappings?.tables.keys.first)

        XCTAssertTrue(owner.textView.ownsNativeBinding(adapter))
        XCTAssertFalse(stale.textView.ownsNativeBinding(adapter))
        XCTAssertFalse(stale.bindTableCell(tableID: tableID, cellIndex: 0, contentRect: .zero))
    }

    func testReturnedSelectionOutsideCellInvalidatesInput() throws {
        let editorId = makeV2Editor(configJson: tableConfig)
        defer { destroyV2Editor(id: editorId) }
        let adapter = try XCTUnwrap(EditorV2Registry.adapter(forLegacyId: editorId))
        let document = #"{"type":"doc","content":[{"type":"table","content":[{"type":"table_row","content":[{"type":"table_cell","content":[{"type":"paragraph","content":[{"type":"text","text":"cell"}]}]}]}]},{"type":"paragraph","content":[{"type":"text","text":"outside"}]}]}"#
        let view = RichTextEditorView(frame: .zero)
        view.bindEditor(id: editorId, initialUpdateJSON: try XCTUnwrap(adapter.initialUpdateJSON()))
        XCTAssertTrue(view.textView.applyUpdateJSON(try XCTUnwrap(adapter.setContentJson(document))))
        let tableID = try XCTUnwrap(adapter.cachedTableInputMappings?.tables.keys.first)
        XCTAssertTrue(view.bindTableCell(tableID: tableID, cellIndex: 0, contentRect: .zero))
        let cell = view.activeTextInput
        EditorV2Shadow.setSelectionScalar(id: editorId, scalarAnchor: 6, scalarHead: 6)
        XCTAssertTrue(cell.applyUpdateJSON(EditorV2Shadow.getCurrentState(id: editorId)))
        XCTAssertTrue(view.activeTextInput === view.textView)
        XCTAssertEqual(cell.editorId, 0)
        let before = try XCTUnwrap(adapter.documentJson())
        cell.insertText("!")
        XCTAssertEqual(try XCTUnwrap(adapter.documentJson()), before)
    }

    func testProjectedUpdateAfterCellSourceMovesRebindsTheCellHoldingTheCaret() throws {
        let editorId = makeV2Editor(configJson: tableConfig)
        defer { destroyV2Editor(id: editorId) }
        let adapter = try XCTUnwrap(EditorV2Registry.adapter(forLegacyId: editorId))
        let initial = #"{"type":"doc","content":[{"type":"table","content":[{"type":"table_row","content":[{"type":"table_cell","content":[{"type":"paragraph","content":[{"type":"text","text":"a"}]}]},{"type":"table_cell","content":[{"type":"paragraph","content":[{"type":"text","text":"b"}]}]}]}]}]}"#
        let replacement = #"{"type":"doc","content":[{"type":"table","content":[{"type":"table_row","content":[{"type":"table_cell","content":[{"type":"paragraph","content":[{"type":"text","text":"longer"}]}]},{"type":"table_cell","content":[{"type":"paragraph","content":[{"type":"text","text":"replacement"}]}]}]}]}]}"#
        let view = RichTextEditorView(frame: .zero)
        view.bindEditor(id: editorId, initialUpdateJSON: try XCTUnwrap(adapter.initialUpdateJSON()))
        XCTAssertTrue(view.textView.applyUpdateJSON(try XCTUnwrap(adapter.setContentJson(initial))))
        let tableID = try XCTUnwrap(adapter.cachedTableInputMappings?.tables.keys.first)
        XCTAssertTrue(view.bindTableCell(tableID: tableID, cellIndex: 1, contentRect: .zero))
        let oldInput = view.activeTextInput
        let oldSource = try XCTUnwrap(adapter.cachedTableInputMappings?.tables[tableID]?.cells[1].sourcePos)

        _ = try XCTUnwrap(adapter.setContentJson(replacement))
        let movedCell = try XCTUnwrap(adapter.cachedTableInputMappings?.tables[tableID]?.cells[1])
        XCTAssertNotEqual(movedCell.sourcePos, oldSource)
        let newSelection = try XCTUnwrap(movedCell.blocks.first?.contentScalarStart)
        EditorV2Shadow.setSelectionScalar(id: editorId, scalarAnchor: newSelection, scalarHead: newSelection)
        XCTAssertTrue(oldInput.applyUpdateJSON(EditorV2Shadow.getCurrentState(id: editorId)))

        XCTAssertTrue(view.activeTextInput === oldInput, "the input follows the authoritative caret")
        XCTAssertEqual(view.activeTableCellPosition, movedCell.sourcePos, "the moved cell is bound from a fresh projection")
        XCTAssertEqual(oldInput.textStorage.string, "replacement")
        oldInput.insertText("!")
        XCTAssertEqual(try adapter.tableCellTexts(), [["longer", "!replacement"]],
                       "typing lands at the authoritative caret in the moved cell")
    }

    func testRootInputBlocksStaleCaretWhenAuthoritativeSelectionMovesInsideTable() throws {
        let editorId = makeV2Editor(configJson: tableConfig)
        defer { destroyV2Editor(id: editorId) }
        let adapter = try XCTUnwrap(EditorV2Registry.adapter(forLegacyId: editorId))
        let document = #"{"type":"doc","content":[{"type":"paragraph","content":[{"type":"text","text":"before"}]},{"type":"table","content":[{"type":"table_row","content":[{"type":"table_cell","content":[{"type":"paragraph","content":[{"type":"text","text":"cell"}]}]}]}]},{"type":"paragraph","content":[{"type":"text","text":"after"}]}]}"#
        let view = RichTextEditorView(frame: .zero)
        view.bindEditor(id: editorId, initialUpdateJSON: try XCTUnwrap(adapter.initialUpdateJSON()))
        XCTAssertTrue(view.textView.applyUpdateJSON(try XCTUnwrap(adapter.setContentJson(document))))
        let tableID = try XCTUnwrap(adapter.cachedTableInputMappings?.tables.keys.first)
        let table = try XCTUnwrap(adapter.cachedTableInputMappings?.tables[tableID])
        let interior = try XCTUnwrap(table.cells.first?.blocks.first?.contentScalarStart) + 1
        let extent = try XCTUnwrap(table.extent)
        XCTAssertTrue(extent.scalarStart < interior && interior < extent.scalarEnd)

        EditorV2Shadow.setSelectionScalar(id: editorId, scalarAnchor: interior, scalarHead: interior)
        XCTAssertTrue(view.textView.applyUpdateJSON(EditorV2Shadow.getCurrentState(id: editorId)))
        XCTAssertTrue(view.textView.rootTableSelectionInputBlocked)
        let before = try XCTUnwrap(adapter.documentJson())
        view.textView.insertText("!")
        view.textView.deleteBackward()
        XCTAssertFalse(view.textView.pasteHTML("<strong>unsafe</strong>", detectContentChange: true))
        XCTAssertEqual(try XCTUnwrap(adapter.documentJson()), before)

        let afterStart = extent.scalarEnd + 1
        EditorV2Shadow.setSelectionScalar(id: editorId, scalarAnchor: afterStart, scalarHead: afterStart)
        XCTAssertTrue(view.textView.applyUpdateJSON(EditorV2Shadow.getCurrentState(id: editorId)))
        XCTAssertFalse(view.textView.rootTableSelectionInputBlocked)
        view.textView.insertText("!")
        XCTAssertTrue(try XCTUnwrap(adapter.documentJson()).contains(#""text":"!after""#))

        EditorV2Shadow.setSelectionScalar(id: editorId, scalarAnchor: interior, scalarHead: interior)
        XCTAssertTrue(view.textView.applyUpdateJSON(EditorV2Shadow.getCurrentState(id: editorId)))
        XCTAssertTrue(view.textView.rootTableSelectionInputBlocked)
        let afterOffset = (view.textView.text as NSString).range(of: "!after").location
        XCTAssertNotEqual(afterOffset, NSNotFound)
        view.textView.selectedRange = NSRange(location: afterOffset, length: 0)
        view.textView.textViewDidChangeSelection(view.textView)
        XCTAssertFalse(view.textView.rootTableSelectionInputBlocked)
        XCTAssertEqual(view.textView.currentLogicalScalarSelection()?.head, afterStart)

        EditorV2Shadow.setSelectionScalar(id: editorId, scalarAnchor: interior, scalarHead: interior)
        XCTAssertTrue(view.textView.applyUpdateJSON(EditorV2Shadow.getCurrentState(id: editorId)))
        XCTAssertTrue(view.textView.rootTableSelectionInputBlocked)
        view.bindEditor(id: 0, initialUpdateJSON: nil)
        XCTAssertFalse(view.textView.rootTableSelectionInputBlocked)
    }

    func testRootTableAnchorEndpointsCannotEditACellWithoutCellBinding() throws {
        let editorId = makeV2Editor(configJson: tableConfig)
        defer { destroyV2Editor(id: editorId) }
        let adapter = try XCTUnwrap(EditorV2Registry.adapter(forLegacyId: editorId))
        let document = #"{"type":"doc","content":[{"type":"table","content":[{"type":"table_row","content":[{"type":"table_cell","content":[{"type":"paragraph","content":[{"type":"text","text":"cell"}]}]}]}]}]}"#
        let view = RichTextEditorView(frame: .zero)
        view.bindEditor(id: editorId, initialUpdateJSON: try XCTUnwrap(adapter.initialUpdateJSON()))
        XCTAssertTrue(view.textView.applyUpdateJSON(try XCTUnwrap(adapter.setContentJson(document))))
        let extent = try XCTUnwrap(adapter.cachedTableInputMappings?.tables.values.first?.extent)
        let before = try XCTUnwrap(adapter.documentJson())

        for scalar in [extent.scalarStart, extent.scalarEnd] {
            EditorV2Shadow.setSelectionScalar(id: editorId, scalarAnchor: scalar, scalarHead: scalar)
            XCTAssertTrue(view.textView.applyUpdateJSON(EditorV2Shadow.getCurrentState(id: editorId)))
            view.textView.insertText("!")
            XCTAssertEqual(try XCTUnwrap(adapter.documentJson()), before)
        }
    }

    func testRootTextDragRejectsTableSourceAndDestination() throws {
        try MainActor.assumeIsolated {
            let editorId = makeV2Editor(configJson: tableConfig)
            defer { destroyV2Editor(id: editorId) }
            let adapter = try XCTUnwrap(EditorV2Registry.adapter(forLegacyId: editorId))
            let document = #"{"type":"doc","content":[{"type":"paragraph","content":[{"type":"text","text":"before"}]},{"type":"table","content":[{"type":"table_row","content":[{"type":"table_cell","content":[{"type":"paragraph","content":[{"type":"text","text":"cell"}]}]}]}]},{"type":"paragraph","content":[{"type":"text","text":"after"}]}]}"#
            let view = RichTextEditorView(frame: .zero)
            view.bindEditor(id: editorId, initialUpdateJSON: try XCTUnwrap(adapter.initialUpdateJSON()))
            XCTAssertTrue(view.textView.applyUpdateJSON(try XCTUnwrap(adapter.setContentJson(document))))
            let textView = view.textView
            let before = try XCTUnwrap(adapter.documentJson())
            let tableOffset = (textView.text as NSString).range(of: "\u{200B}").location
            XCTAssertNotEqual(tableOffset, NSNotFound)
            let item = UIDragItem(itemProvider: NSItemProvider(object: "be" as NSString))

            @MainActor func position(_ offset: Int) throws -> UITextPosition {
                try XCTUnwrap(textView.position(from: textView.beginningOfDocument, offset: offset))
            }
            @MainActor func drag(_ start: Int, _ end: Int) throws -> TestTextDragSession {
                let session = TestTextDragSession(items: [item])
                let range = try XCTUnwrap(textView.textRange(from: position(start), to: position(end)))
                _ = textView.textDraggableView(textView, itemsForDrag: TestTextDragRequest(
                    dragRange: range,
                    suggestedItems: [item],
                    isSelected: true,
                    dragSession: session
                ))
                return session
            }
            @MainActor func proposal(_ destination: Int, session: TestTextDragSession) throws -> UITextDropProposal {
                let request = TestTextDropRequest(
                    dropPosition: try position(destination),
                    isSameView: true,
                    dropSession: TestTextDropSession(dragSession: session)
                )
                let result = textView.textDroppableView(textView, proposalForDrop: request)
                textView.textDroppableView(textView, willPerformDrop: request)
                return result
            }

            XCTAssertEqual(try proposal(tableOffset, session: drag(0, 2)).operation, .forbidden)
            XCTAssertEqual(try proposal(tableOffset + 1, session: drag(0, 2)).operation, .forbidden)
            XCTAssertEqual(try proposal(0, session: drag(tableOffset, tableOffset + 1)).operation, .forbidden)
            XCTAssertEqual(try XCTUnwrap(adapter.documentJson()), before)
        }
    }

    func testInvalidationClearsUIKitCompositionAndResignsCellInput() throws {
        let editorId = makeV2Editor(configJson: tableConfig)
        defer { destroyV2Editor(id: editorId) }
        let adapter = try XCTUnwrap(EditorV2Registry.adapter(forLegacyId: editorId))
        let document = #"{"type":"doc","content":[{"type":"table","content":[{"type":"table_row","content":[{"type":"table_cell","content":[{"type":"paragraph","content":[{"type":"text","text":"cell"}]}]}]}]}]}"#
        let view = RichTextEditorView(frame: CGRect(x: 0, y: 0, width: 320, height: 200))
        let window = UIWindow(frame: CGRect(x: 0, y: 0, width: 320, height: 480))
        window.rootViewController = UIViewController()
        window.makeKeyAndVisible()
        window.rootViewController?.view.addSubview(view)
        defer { window.isHidden = true }
        view.layoutIfNeeded()
        view.bindEditor(id: editorId, initialUpdateJSON: try XCTUnwrap(adapter.initialUpdateJSON()))
        XCTAssertTrue(view.textView.applyUpdateJSON(try XCTUnwrap(adapter.setContentJson(document))))
        let tableID = try XCTUnwrap(adapter.cachedTableInputMappings?.tables.keys.first)
        XCTAssertTrue(view.bindTableCell(tableID: tableID, cellIndex: 0, contentRect: CGRect(x: 0, y: 0, width: 140, height: 50)))
        let cell = view.activeTextInput
        XCTAssertTrue(cell.becomeFirstResponder())
        cell.setMarkedText("draft", selectedRange: NSRange(location: 5, length: 0))
        XCTAssertNotNil(cell.markedTextRange)
        let before = try XCTUnwrap(adapter.documentJson())

        view.invalidateTableCellBinding()

        XCTAssertNil(cell.markedTextRange)
        XCTAssertFalse(cell.isFirstResponder)
        XCTAssertFalse(cell.isComposing)
        XCTAssertEqual(try XCTUnwrap(adapter.documentJson()), before)
        XCTAssertTrue(view.bindTableCell(tableID: tableID, cellIndex: 0, contentRect: .zero))
        XCTAssertTrue(view.activeTextInput === cell)
        XCTAssertNil(cell.markedTextRange)
    }

    func testHostRebindInvalidatesRetainedTableCellInput() throws {
        let firstEditorId = makeV2Editor(configJson: tableConfig)
        let secondEditorId = makeV2Editor(configJson: tableConfig)
        defer {
            destroyV2Editor(id: firstEditorId)
            destroyV2Editor(id: secondEditorId)
        }
        let firstAdapter = try XCTUnwrap(EditorV2Registry.adapter(forLegacyId: firstEditorId))
        let document = #"{"type":"doc","content":[{"type":"table","content":[{"type":"table_row","content":[{"type":"table_cell","content":[{"type":"paragraph","content":[{"type":"text","text":"old"}]}]}]}]}]}"#
        let view = RichTextEditorView(frame: .zero)
        view.bindEditor(id: firstEditorId, initialUpdateJSON: try XCTUnwrap(firstAdapter.initialUpdateJSON()))
        XCTAssertTrue(view.textView.applyUpdateJSON(try XCTUnwrap(firstAdapter.setContentJson(document))))
        let tableID = try XCTUnwrap(firstAdapter.cachedTableInputMappings?.tables.keys.first)
        XCTAssertTrue(view.bindTableCell(tableID: tableID, cellIndex: 0, contentRect: .zero))
        let staleCellInput = view.activeTextInput
        XCTAssertNotNil(staleCellInput.tableCellPositionMap)
        XCTAssertNotNil(staleCellInput.onProjectedUpdate)

        view.bindEditor(id: 0, initialUpdateJSON: nil)
        XCTAssertTrue(view.activeTextInput === view.textView)
        XCTAssertEqual(staleCellInput.editorId, 0)
        XCTAssertNil(staleCellInput.tableCellPositionMap)
        XCTAssertNil(staleCellInput.onProjectedUpdate)

        view.bindEditor(id: secondEditorId, initialUpdateJSON: try XCTUnwrap(
            EditorV2Registry.adapter(forLegacyId: secondEditorId)?.initialUpdateJSON()
        ))
        staleCellInput.insertText("!")

        XCTAssertFalse(try XCTUnwrap(firstAdapter.documentJson()).contains("!old"))
        XCTAssertFalse(try XCTUnwrap(
            EditorV2Registry.adapter(forLegacyId: secondEditorId)?.documentJson()
        ).contains("!"))
    }

    func testRetainedComposingCellCannotCommitAfterExpoTakesNativeOwnership() throws {
        let editorId = makeV2Editor(configJson: tableConfig)
        defer { destroyV2Editor(id: editorId) }
        let adapter = try XCTUnwrap(EditorV2Registry.adapter(forLegacyId: editorId))
        let document = #"{"type":"doc","content":[{"type":"table","content":[{"type":"table_row","content":[{"type":"table_cell","content":[{"type":"paragraph","content":[{"type":"text","text":"cell"}]}]}]}]}]}"#
        let originalHost = RichTextEditorView(frame: .zero)
        originalHost.bindEditor(id: editorId, initialUpdateJSON: try XCTUnwrap(adapter.initialUpdateJSON()))
        XCTAssertTrue(originalHost.textView.applyUpdateJSON(try XCTUnwrap(adapter.setContentJson(document))))
        let tableID = try XCTUnwrap(adapter.cachedTableInputMappings?.tables.keys.first)
        XCTAssertTrue(originalHost.bindTableCell(tableID: tableID, cellIndex: 0, contentRect: .zero))
        let staleCellInput = originalHost.activeTextInput
        staleCellInput.setMarkedText("!", selectedRange: NSRange(location: 1, length: 0))
        XCTAssertTrue(staleCellInput.isComposing)
        XCTAssertNotNil(staleCellInput.markedTextReplacementScalarRange)
        let documentJSONBeforeTakeover = try XCTUnwrap(adapter.documentJson())

        let expoHost = NativeEditorExpoView()
        defer { expoHost.setEditorId(0) }
        expoHost.setEditorId(editorId)

        XCTAssertFalse(originalHost.textView.ownsNativeBinding(adapter))
        XCTAssertTrue(expoHost.ownsNativeBinding(editorId: editorId))
        XCTAssertTrue(expoHost.richTextView.bindTableCell(
            tableID: tableID,
            cellIndex: 0,
            contentRect: .zero
        ))
        staleCellInput.unmarkText()

        XCTAssertEqual(try XCTUnwrap(adapter.documentJson()), documentJSONBeforeTakeover)
    }

    func testExpoDestroyInvalidatesRetainedTableCellInput() throws {
        let editorId = makeV2Editor(configJson: tableConfig)
        var destroyed = false
        defer {
            if !destroyed {
                destroyV2Editor(id: editorId)
            }
        }
        let adapter = try XCTUnwrap(EditorV2Registry.adapter(forLegacyId: editorId))
        let document = #"{"type":"doc","content":[{"type":"table","content":[{"type":"table_row","content":[{"type":"table_cell","content":[{"type":"paragraph","content":[{"type":"text","text":"cell"}]}]}]}]}]}"#
        let expoHost = NativeEditorExpoView()
        expoHost.setEditorId(editorId)
        XCTAssertTrue(expoHost.richTextView.textView.applyUpdateJSON(
            try XCTUnwrap(adapter.setContentJson(document))
        ))
        let tableID = try XCTUnwrap(adapter.cachedTableInputMappings?.tables.keys.first)
        XCTAssertTrue(expoHost.richTextView.bindTableCell(
            tableID: tableID,
            cellIndex: 0,
            contentRect: .zero
        ))
        let staleCellInput = expoHost.richTextView.activeTextInput

        NativeEditorViewRegistry.shared.invalidateDestroyedEditor(editorId: editorId)
        destroyV2Editor(id: editorId)
        destroyed = true

        XCTAssertEqual(expoHost.richTextView.editorId, 0)
        XCTAssertEqual(staleCellInput.editorId, 0)
        XCTAssertNil(staleCellInput.tableCellPositionMap)
        XCTAssertNil(staleCellInput.onProjectedUpdate)
    }

    func testStaleComposingCellCannotCommitStoredRangeAfterNativeRevisionChanges() throws {
        let editorId = makeV2Editor(configJson: tableConfig)
        defer { destroyV2Editor(id: editorId) }
        let adapter = try XCTUnwrap(EditorV2Registry.adapter(forLegacyId: editorId))
        let initialDocument = #"{"type":"doc","content":[{"type":"table","content":[{"type":"table_row","content":[{"type":"table_cell","content":[{"type":"paragraph","content":[{"type":"text","text":"cell"}]}]}]}]}]}"#
        let replacementDocument = #"{"type":"doc","content":[{"type":"table","content":[{"type":"table_row","content":[{"type":"table_cell","content":[{"type":"paragraph","content":[{"type":"text","text":"replacement"}]}]}]}]}]}"#
        let view = RichTextEditorView(frame: .zero)
        view.bindEditor(id: editorId, initialUpdateJSON: try XCTUnwrap(adapter.initialUpdateJSON()))
        XCTAssertTrue(view.textView.applyUpdateJSON(try XCTUnwrap(adapter.setContentJson(initialDocument))))
        let tableID = try XCTUnwrap(adapter.cachedTableInputMappings?.tables.keys.first)
        XCTAssertTrue(view.bindTableCell(tableID: tableID, cellIndex: 0, contentRect: .zero))
        let staleCellInput = view.activeTextInput
        staleCellInput.setMarkedText("!", selectedRange: NSRange(location: 1, length: 0))
        XCTAssertTrue(staleCellInput.isComposing)
        XCTAssertNotNil(staleCellInput.markedTextReplacementScalarRange)

        _ = try XCTUnwrap(adapter.setContentJson(replacementDocument))
        let documentJSONAfterNativeRevision = try XCTUnwrap(adapter.documentJson())
        staleCellInput.unmarkText()

        XCTAssertEqual(try XCTUnwrap(adapter.documentJson()), documentJSONAfterNativeRevision)
    }

    func testFullContextProjectionMapsTwoParagraphsAndAnEmptyParagraph() throws {
        let editorId = makeV2Editor(configJson: tableConfig)
        defer { destroyV2Editor(id: editorId) }
        let adapter = try XCTUnwrap(EditorV2Registry.adapter(forLegacyId: editorId))
        let view = RichTextEditorView(frame: CGRect(x: 0, y: 0, width: 320, height: 200))
        view.bindEditor(id: editorId, initialUpdateJSON: try XCTUnwrap(adapter.initialUpdateJSON()))
        let document = #"{"type":"doc","content":[{"type":"table","content":[{"type":"table_row","content":[{"type":"table_cell","content":[{"type":"paragraph","content":[{"type":"text","text":"one"}]},{"type":"paragraph"},{"type":"paragraph","content":[{"type":"text","text":"two"}]}]}]}]}]}"#
        XCTAssertTrue(view.textView.applyUpdateJSON(try XCTUnwrap(adapter.setContentJson(document))))
        let tableID = try XCTUnwrap(adapter.cachedTableInputMappings?.tables.keys.first)
        let mapping = try XCTUnwrap(adapter.cachedTableInputMappings?.tables[tableID])
        let table = try XCTUnwrap(adapter.cachedTableRecords[tableID])
        let projection = try XCTUnwrap(EditorTableInputCoordinator.projection(
            cellIndex: 0,
            table: table,
            mapping: mapping,
            documentRevision: adapter.baseDocumentRevision,
            positionEpoch: try XCTUnwrap(adapter.positionEpoch),
            baseFont: view.textView.baseFont,
            textColor: view.textView.baseTextColor,
            theme: view.textView.theme,
            atomConfiguration: view.textView.atomRenderConfiguration
        ))

        XCTAssertEqual(projection.text.string.replacingOccurrences(of: "\u{200B}", with: ""), "one\n\ntwo")
        XCTAssertEqual(projection.positionMap.segments.count, 3)
        XCTAssertTrue(view.bindTableCell(tableID: tableID, cellIndex: 0, contentRect: .zero))
        XCTAssertEqual(view.activeTextInput.inputScalarRange(fromLocal: 0, toLocal: 9)?.from, 0)
        XCTAssertEqual(view.activeTextInput.inputScalarRange(fromLocal: 0, toLocal: 9)?.to, 9)
    }

    func testCellAppliesAndPreservesBackwardGlobalSelection() throws {
        let editorId = makeV2Editor(configJson: tableConfig)
        defer { destroyV2Editor(id: editorId) }
        let adapter = try XCTUnwrap(EditorV2Registry.adapter(forLegacyId: editorId))
        let view = RichTextEditorView(frame: CGRect(x: 0, y: 0, width: 320, height: 200))
        view.bindEditor(id: editorId, initialUpdateJSON: try XCTUnwrap(adapter.initialUpdateJSON()))
        let document = #"{"type":"doc","content":[{"type":"table","content":[{"type":"table_row","content":[{"type":"table_cell","content":[{"type":"paragraph","content":[{"type":"text","text":"alpha"}]}]}]}]}]}"#
        XCTAssertTrue(view.textView.applyUpdateJSON(try XCTUnwrap(adapter.setContentJson(document))))
        let tableID = try XCTUnwrap(adapter.cachedTableInputMappings?.tables.keys.first)
        XCTAssertTrue(view.bindTableCell(tableID: tableID, cellIndex: 0, contentRect: .zero))
        let map = try XCTUnwrap(view.activeTextInput.tableCellPositionMap)
        let cellStart = try XCTUnwrap(map.globalScalar(forLocalScalar: 0))
        let cellEnd = try XCTUnwrap(map.globalScalar(forLocalScalar: 5))

        _ = view.activeTextInput.applySelectionFromJSON([
            "type": "text",
            "anchor": NSNumber(value: cellEnd),
            "head": NSNumber(value: cellStart),
            "anchorScalar": NSNumber(value: cellEnd),
            "headScalar": NSNumber(value: cellStart)
        ])

        XCTAssertEqual(view.activeTextInput.currentLogicalScalarSelection()?.anchor, cellEnd)
        XCTAssertEqual(view.activeTextInput.currentLogicalScalarSelection()?.head, cellStart)
    }

    func testListCellMapsTextEndpointsAcrossTwoItems() throws {
        let editorId = makeV2Editor(configJson: listTableConfig)
        defer { destroyV2Editor(id: editorId) }
        let adapter = try XCTUnwrap(EditorV2Registry.adapter(forLegacyId: editorId))
        let view = RichTextEditorView(frame: CGRect(x: 0, y: 0, width: 320, height: 200))
        view.bindEditor(id: editorId, initialUpdateJSON: try XCTUnwrap(adapter.initialUpdateJSON()))
        let document = #"{"type":"doc","content":[{"type":"table","content":[{"type":"table_row","content":[{"type":"table_cell","content":[{"type":"bulletList","content":[{"type":"listItem","content":[{"type":"paragraph","content":[{"type":"text","text":"one"}]}]},{"type":"listItem","content":[{"type":"paragraph","content":[{"type":"text","text":"two"}]}]}]}]}]}]}]}"#
        XCTAssertTrue(view.textView.applyUpdateJSON(try XCTUnwrap(adapter.setContentJson(document))))
        let tableID = try XCTUnwrap(adapter.cachedTableInputMappings?.tables.keys.first)
        let mapping = try XCTUnwrap(adapter.cachedTableInputMappings?.tables[tableID])
        let table = try XCTUnwrap(adapter.cachedTableRecords[tableID])
        let projection = try XCTUnwrap(EditorTableInputCoordinator.projection(
            cellIndex: 0,
            table: table,
            mapping: mapping,
            documentRevision: adapter.baseDocumentRevision,
            positionEpoch: try XCTUnwrap(adapter.positionEpoch),
            baseFont: view.textView.baseFont,
            textColor: view.textView.baseTextColor,
            theme: view.textView.theme,
            atomConfiguration: view.textView.atomRenderConfiguration
        ))
        XCTAssertTrue(view.bindTableCell(tableID: tableID, cellIndex: 0, contentRect: .zero))
        let start = PositionBridge.utf16OffsetToScalar(0, in: view.activeTextInput)
        let end = PositionBridge.utf16OffsetToScalar(view.activeTextInput.attributedText.length, in: view.activeTextInput)

        XCTAssertNotNil(
            view.activeTextInput.inputScalarRange(fromLocal: start, toLocal: end),
            "local=\(start)...\(end) segments=\(projection.positionMap.segments) text=\(projection.text.string.debugDescription)"
        )
    }

    func testPositionMapConvertsEmojiUtf16IntoCurrentGlobalScalarRange() throws {
        let map = TableCellPositionMap(
            binding: .init(cellSourcePosition: 10, documentRevision: 4, positionEpoch: 9),
            segments: [.init(localScalarRange: 0..<5, globalScalarStart: 40)]
        )

        XCTAssertEqual(map.globalScalar(forLocalUTF16: 3, in: "a😀bc"), 42)
        let range = try XCTUnwrap(
            map.globalScalarRange(forLocalUTF16: NSRange(location: 1, length: 2), in: "a😀bc")
        )
        XCTAssertEqual(range.0, 41)
        XCTAssertEqual(range.1, 42)
    }

    func testPositionMapRejectsStaleAndNestedOrSyntheticTargets() {
        let binding = TableCellPositionMap.Binding(
            cellSourcePosition: 10,
            documentRevision: 4,
            positionEpoch: 9
        )
        let map = TableCellPositionMap(
            binding: binding,
            segments: [.init(localScalarRange: 0..<2, globalScalarStart: 40)]
        )

        XCTAssertNil(map.globalScalar(forLocalScalar: 1, currentRevision: 5, currentEpoch: 9))
        XCTAssertFalse(EditorTableInputCoordinator.canBind(
            .init(binding: binding, isSynthetic: true, isNestedTarget: false)
        ))
        XCTAssertFalse(EditorTableInputCoordinator.canBind(
            .init(binding: binding, isSynthetic: false, isNestedTarget: true)
        ))
    }

    func testCoordinatorReusesOneInputAcrossThreeCellBindings() {
        let coordinator = EditorTableInputCoordinator()
        let input = coordinator.cellInput
        let target = { (position: UInt32) in
            EditorTableInputCoordinator.Target(
                binding: .init(cellSourcePosition: position, documentRevision: 4, positionEpoch: 9),
                isSynthetic: false,
                isNestedTarget: false
            )
        }

        XCTAssertTrue(coordinator.bind(target(10), text: NSAttributedString(string: "one"), positionMap: .init(binding: target(10).binding, segments: [.init(localScalarRange: 0..<4, globalScalarStart: 10)])))
        XCTAssertTrue(coordinator.bind(target(20), text: NSAttributedString(string: "two"), positionMap: .init(binding: target(20).binding, segments: [.init(localScalarRange: 0..<4, globalScalarStart: 20)])))
        XCTAssertTrue(coordinator.bind(target(30), text: NSAttributedString(string: "three"), positionMap: .init(binding: target(30).binding, segments: [.init(localScalarRange: 0..<6, globalScalarStart: 30)])))

        XCTAssertTrue(coordinator.cellInput === input)
        XCTAssertEqual(coordinator.inputInstanceCountForTesting, 1)
        XCTAssertEqual(coordinator.phase, .bound(cellSourcePos: 30, documentRevision: "4", positionEpoch: "9"))
    }
}
