import CoreText
import XCTest

final class EditorTableInputTests: XCTestCase {
    private final class UpdateSpy: EditorTextViewDelegate {
        var updates: [String] = []

        func editorTextView(_ textView: EditorTextView, selectionDidChange anchor: UInt32, head: UInt32) {}
        func editorTextView(_ textView: EditorTextView, didReceiveUpdate updateJSON: String) {
            updates.append(updateJSON)
        }
    }

    private let tableConfig = TableInputTestSchema.tableConfig
    private let listTableConfig = TableInputTestSchema.listTableConfig
    private let wideTwoCellDocument = #"{"type":"doc","content":[{"type":"paragraph","content":[{"type":"text","text":"before"}]},{"type":"table","content":[{"type":"table_row","content":[{"type":"table_cell","attrs":{"colwidth":[500]},"content":[{"type":"paragraph","content":[{"type":"text","text":"one"}]}]},{"type":"table_cell","attrs":{"colwidth":[500]},"content":[{"type":"paragraph","content":[{"type":"text","text":"two"}]}]}]}]}]}"#
    private let fourCellDocument = #"{"type":"doc","content":[{"type":"table","content":[{"type":"table_row","content":[{"type":"table_cell","content":[{"type":"paragraph","content":[{"type":"text","text":"one"}]}]},{"type":"table_cell","content":[{"type":"paragraph","content":[{"type":"text","text":"two"}]}]}]},{"type":"table_row","content":[{"type":"table_cell","content":[{"type":"paragraph","content":[{"type":"text","text":"three"}]}]},{"type":"table_cell","content":[{"type":"paragraph","content":[{"type":"text","text":"four"}]}]}]}]}]}"#

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

    private struct MountedHandleFixture {
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
    }

    private func withMountedHandles(
        document: String, configJSON: String? = nil, theme: EditorTheme? = nil,
        size: CGSize = CGSize(width: 360, height: 240), anchorIndex: Int, headIndex: Int,
        _ body: (MountedHandleFixture) throws -> Void
    ) throws {
        let editorId = makeV2Editor(configJson: configJSON ?? tableConfig)
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
        let tableID = try XCTUnwrap(adapter.cachedTableRecords.first {
            $0.value["readOnlyDescendants"] as? Bool == false
        }?.key)
        let rawCells = try XCTUnwrap(adapter.cachedTableRecords[tableID]?["cells"] as? [[String: Any]])
        let positions = try rawCells.map { try XCTUnwrap(EditorV2Adapter.uint32Field($0, "sourcePos")) }
        let request = adapter.callWithEnvelope([
            "selection": [
                "type": "cell",
                "anchorCell": ["kind": "document", "offset": Int(positions[anchorIndex])],
                "headCell": ["kind": "document", "offset": Int(positions[headIndex])]
            ]
        ]) { editorV2SetSelection(editorId: adapter.editorId, requestJson: $0) }
        XCTAssertNil(request.error)
        XCTAssertTrue(view.textView.applyUpdateJSON(try XCTUnwrap(adapter.refreshFromRustState(mirrorSelection: nil))))
        view.layoutIfNeeded()
        let surface = try XCTUnwrap(view.subviews.compactMap { $0 as? EditorTableSurface }.first)
        let drawing = try XCTUnwrap(surface.subviews.compactMap { $0 as? PreparedProseDrawingView }.first)
        let updates = UpdateSpy()
        view.textView.editorDelegate = updates
        try body(MountedHandleFixture(view: view, adapter: adapter, tableID: tableID,
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

    func testProjectedUpdateCannotRetargetInputAfterCellSourceMoves() throws {
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

        XCTAssertTrue(view.activeTextInput === view.textView)
        XCTAssertEqual(oldInput.editorId, 0)
        let settled = try XCTUnwrap(adapter.documentJson())
        oldInput.insertText("!")
        XCTAssertEqual(try XCTUnwrap(adapter.documentJson()), settled)
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
