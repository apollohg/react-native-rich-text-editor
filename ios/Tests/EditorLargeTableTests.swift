import XCTest

final class EditorLargeTableTests: XCTestCase {
    private enum PlainTable {
        static func cellText(row: Int, column: Int) -> String {
            String(format: "R%04dC%04dXY", row, column)
        }
        static let headerRow = 0
        static let document = "doc"
        static let table = "table"
        static let row = "table_row"
        static let cell = "table_cell"
        static let headerCell = "table_header"
        static let paragraph = "paragraph"
        static let text = "text"
    }

    private func plainTableDocument(rows: Int, columns: Int, repeatedText: String? = nil) throws -> String {
        let tableRows: [[String: Any]] = (0..<rows).map { row in
            let type = row == PlainTable.headerRow ? PlainTable.headerCell : PlainTable.cell
            return ["type": PlainTable.row, "content": (0..<columns).map { column in [
                "type": type,
                "content": [["type": PlainTable.paragraph, "content": [["type": PlainTable.text, "text": repeatedText ?? PlainTable.cellText(row: row, column: column)]]]]
            ] }]
        }
        let data = try JSONSerialization.data(withJSONObject: [
            "type": PlainTable.document, "content": [["type": PlainTable.table, "content": tableRows]]
        ])
        return try XCTUnwrap(String(data: data, encoding: .utf8))
    }

    private enum Window {
        static let viewport = CGSize(width: 390, height: 844)
        static let overscanViewports: CGFloat = 1
        static let straddlingCells = 1
    }

    private enum AccessibilityWalk {
        static let rows = [1, 150, 400, 999]
    }

    private enum EditedTable {
        static let rows = 50
        static let columns = 4
        static let typed = "x"
    }

    private static let twentyThousandSlotTables = [(rows: 1000, columns: 20), (rows: 100, columns: 200)]

    private var maximumRetainedPresentations: Int {
        let style = TableStyle()
        let span = 1 + 2 * Window.overscanViewports
        let minimumRowHeight = 2 * (style.cellPadding + style.borderWidth)
        let columns = Int((span * Window.viewport.width / style.minColumnWidth).rounded(.up)) + Window.straddlingCells
        let rows = Int((span * Window.viewport.height / minimumRowHeight).rounded(.up)) + Window.straddlingCells
        return columns * rows
    }

    private func withMountedTable(
        rows: Int,
        columns: Int,
        repeatedText: String? = nil,
        _ body: (RichTextEditorView, EditorTableSurface, PreparedProseDrawingView) throws -> Void
    ) throws {
        let label = "\(rows)x\(columns)"
        let editorId = makeV2Editor(configJson: TableInputTestSchema.tableConfig)
        defer { destroyV2Editor(id: editorId) }
        let adapter = try XCTUnwrap(EditorV2Registry.adapter(forLegacyId: editorId))
        let view = RichTextEditorView(frame: CGRect(origin: .zero, size: Window.viewport))
        let window = hostEditorView(view, size: Window.viewport)
        defer { window.isHidden = true }
        view.bindEditor(id: editorId, initialUpdateJSON: try XCTUnwrap(adapter.initialUpdateJSON()))
        let update = try XCTUnwrap(adapter.setContentJson(try plainTableDocument(rows: rows, columns: columns, repeatedText: repeatedText)),
                                   "\(label): the fixture renders instead of failing: \(adapter.debugNotes)")
        XCTAssertTrue(view.textView.applyUpdateJSON(update), "\(label): the render applies")
        view.layoutIfNeeded()
        let surface = try XCTUnwrap(view.subviews.compactMap { $0 as? EditorTableSurface }.first)
        let drawing = try XCTUnwrap(surface.subviews.compactMap { $0 as? PreparedProseDrawingView }.first)
        try body(view, surface, drawing)
    }

    func testColdLayoutRetainsOnlyWindowLayouts() throws {
        try withMountedTable(rows: 1000, columns: 20) { _, _, drawing in
            let table = try XCTUnwrap(drawing.mountedTablePresentation()?.tables.first?.surface)
            XCTAssertEqual(table.cells.count, 20_000)
            XCTAssertLessThanOrEqual(table.cells.filter { $0.cachedContent != nil }.count, maximumRetainedPresentations)
            XCTAssertLessThanOrEqual(table.layoutStore.unmountedRetainedBytes,
                                     PreparedProseLayoutCache.preparedLayoutUnmountedByteBudget)
            XCTAssertEqual(table.cells.map(\.contentSize.height).count, 20_000)
        }
    }

    func testStructuralRowInsertionReusesEvictedCellGeometry() throws {
        let shape = Self.twentyThousandSlotTables[0]
        try withMountedTable(rows: shape.rows, columns: shape.columns) { view, surface, drawing in
            let adapter = try XCTUnwrap(EditorV2Registry.adapter(forLegacyId: view.editorId))
            let tableID = try adapter.editableTableID()
            XCTAssertTrue(view.bindTableCell(tableID: tableID, cellIndex: 0, contentRect: .zero))
            view.layoutIfNeeded()
            let before = try XCTUnwrap(drawing.mountedTablePresentation()?.tables.first?.surface)
            let source = try XCTUnwrap(before.sourceTable)
            let oldKeys = Set(source.cells.map(\.contentKey))
            let middleIndex = before.cells.count / 2
            let middle = before.cells[middleIndex]
            XCTAssertNil(middle.cachedContent, "The regression requires an evicted unchanged cell")
            var preparedKeys: [String] = []
            surface.onTableCellPreparedForTesting = { _, key in preparedKeys.append(key) }
            let command = try XCTUnwrap(TableAccessibilityAction.all.first { $0.key == "addRowAfter" }).command
            let update = try XCTUnwrap(adapter.commandAtSelection(command, anchor: 0, head: 0))
            XCTAssertTrue(view.textView.applyUpdateJSON(update))
            view.layoutIfNeeded()
            let after = try XCTUnwrap(drawing.mountedTablePresentation()?.tables.first?.surface)
            XCTAssertEqual(after.cells.count, before.cells.count + shape.columns)
            XCTAssertEqual(preparedKeys.filter { oldKeys.contains($0) }.count, 0,
                "Structural geometry must reuse unchanged cell metadata even after drawing eviction")
            let moved = after.cells[middleIndex + shape.columns]
            XCTAssertEqual(moved.contentSize, middle.contentSize)
            XCTAssertGreaterThan(after.frame(ofCell: moved).minY, before.frame(ofCell: middle).minY)
            XCTAssertNil(moved.cachedContent, "Structural updates must not populate offscreen drawing objects")
            let rebuilt = moved.content
            XCTAssertEqual(rebuilt.size, middle.contentSize,
                "The moved cell must still rebuild after geometry reuse")
            XCTAssertEqual(TableAccessibility.contentNodes(of: rebuilt), middle.accessibilityNodes,
                "Reconstruction must preserve the moved cell's text and local accessibility geometry")
            XCTAssertLessThanOrEqual(after.layoutStore.unmountedRetainedBytes,
                PreparedProseLayoutCache.preparedLayoutUnmountedByteBudget)
        }
    }

    func testScrollingPreparesOnlyEnteringCells() throws {
        try withMountedTable(rows: 1000, columns: 20) { view, surface, drawing in
            var preparations: [Int] = []
            surface.onTableCellPreparedForTesting = { index, _ in preparations.append(index) }
            view.textView.contentOffset.y = view.textView.contentSize.height / 2
            view.layoutIfNeeded()
            let first = try XCTUnwrap(drawing.mountedTablePresentation())
            XCTAssertFalse(first.cells.isEmpty)
            XCTAssertGreaterThan(preparations.count, 0, "Entering an uncached region must prepare cells")
            preparations.removeAll()
            _ = drawing.mountedTablePresentation()
            XCTAssertTrue(preparations.isEmpty, "Repeated presentation must reuse entering-cell layouts")
            XCTAssertLessThanOrEqual(first.cells.count, maximumRetainedPresentations)
        }
    }

    func testAccessibilityMetadataCoversTheWholeTable() throws {
        try withMountedTable(rows: 1000, columns: 20) { _, _, drawing in
            let table = try XCTUnwrap(drawing.mountedTablePresentation()?.tables.first?.surface)
            let before = table.layoutStore.count
            XCTAssertEqual(table.cells.count, 20_000)
            XCTAssertTrue(table.cells.allSatisfy { !$0.accessibilityNodes.isEmpty })
            XCTAssertFalse(try XCTUnwrap(table.cell(sourceIndex: 19_999)).accessibilityNodes.map(\.label).joined().isEmpty)
            XCTAssertEqual(table.layoutStore.count, before, "Offscreen metadata must not prepare drawing layouts")
        }
    }

    func testTwentyThousandSlotTablesRenderAndPresentOnlyTheirViewportWindow() throws {
        for (rows, columns) in Self.twentyThousandSlotTables {
            let label = "\(rows)x\(columns)"
            try withMountedTable(rows: rows, columns: columns) { view, surface, drawing in
                let table = try XCTUnwrap(drawing.mountedTablePresentation()?.tables.first, "\(label): the table is mounted")
                XCTAssertEqual(table.surface.cells.count, rows * columns, "\(label): every cell is prepared")
                var drawnCells: [Int] = []
                drawing.onMountedTableCellsDrawnForTesting = { drawnCells.append($0) }
                let renderer = UIGraphicsImageRenderer(bounds: drawing.bounds)
                let textView = view.textView
                let chain = [table.surface.scrollIdentity]
                let bottom = max(0, textView.contentSize.height - textView.bounds.height)
                let horizontalRoom = table.surface.bounds.width - table.surface.hostViewportWidth
                let stops: [(offsetY: CGFloat, physicalDelta: CGFloat)] = [
                    (0, 0), (bottom / 2, -horizontalRoom / 2), (bottom, -horizontalRoom / 2)
                ]
                for (offsetY, physicalDelta) in stops {
                    textView.contentOffset.y = offsetY
                    drawing.scrollTables(in: chain, by: physicalDelta)
                    view.layoutIfNeeded()
                    _ = renderer.image { _ in drawing.drawInstalledLayersForTesting() }
                    let presented = try XCTUnwrap(drawing.mountedTablePresentation())
                    print("\(label) at y \(offsetY), table offset \(drawing.tableLogicalOffset(for: table.surface.identity)): \(presented.cells.count) presented, \(drawnCells.last ?? -1) drawn, bound \(maximumRetainedPresentations)")
                    XCTAssertLessThanOrEqual(presented.cells.count, maximumRetainedPresentations,
                                             "\(label): the presentation is bounded by the viewport window, not the table")
                    XCTAssertLessThanOrEqual(presented.layouts.count, maximumRetainedPresentations + 1,
                                             "\(label): only window cells reach the recursive traversal")
                    XCTAssertLessThanOrEqual(try XCTUnwrap(drawnCells.last), maximumRetainedPresentations,
                                             "\(label): drawing mounts only the window")
                    let centre = CGPoint(x: surface.bounds.midX, y: surface.bounds.midY)
                    XCTAssertNotNil(surface.cellHit(at: centre), "\(label): the cell under the viewport centre is presented")
                }
            }
        }
    }

    func testLargeTableAccessibilityFocusWalksPastTheWindowWithHeadersAndStableElements() throws {
        let size = Self.twentyThousandSlotTables[0]
        try withMountedTable(rows: size.rows, columns: size.columns) { view, _, drawing in
            let screen = try XCTUnwrap(view.window).bounds
            func tableElement() throws -> TableAccessibilityTableElement {
                try XCTUnwrap((0..<drawing.accessibilityElementCount()).lazy.compactMap {
                    drawing.accessibilityElement(at: $0) as? TableAccessibilityTableElement
                }.first)
            }
            let table = try tableElement()
            XCTAssertEqual(table.accessibilityRowCount(), size.rows)
            XCTAssertEqual(table.accessibilityColumnCount(), size.columns)
            let firstBody = try XCTUnwrap(table.accessibilityDataTableCellElement(forRow: 1, column: 0) as? NSObject)
            for row in AccessibilityWalk.rows {
                let column = row % size.columns
                let element = try XCTUnwrap(
                    table.accessibilityDataTableCellElement(forRow: row, column: column) as? TableAccessibilityCellElement,
                    "row \(row) has an element before it is scrolled into view"
                )
                element.accessibilityElementDidBecomeFocused()
                view.layoutIfNeeded()
                let frame = element.accessibilityFrame
                let presented = try XCTUnwrap(drawing.presentedAccessibilityCell(element.cell))
                let expectedFrame = drawing.convert(presented.bounds.intersection(presented.clip),
                    to: try XCTUnwrap(view.window).screen.coordinateSpace)
                XCTAssertEqual(frame.minX, expectedFrame.minX, accuracy: 1 / drawing.contentScaleFactor)
                XCTAssertEqual(frame.minY, expectedFrame.minY, accuracy: 1 / drawing.contentScaleFactor)
                let headers = table.accessibilityHeaderElements(forColumn: column) ?? []
                print("row \(row) column \(column): frame \(frame), headers \(headers.compactMap { ($0 as? NSObject)?.accessibilityLabel }), offset \(view.textView.contentOffset)")
                XCTAssertTrue(screen.contains(CGPoint(x: frame.midX, y: frame.midY)), "row \(row) is revealed on screen: \(frame)")
                XCTAssertEqual(element.accessibilityRowRange(), NSRange(location: row, length: 1))
                XCTAssertEqual(element.accessibilityLabel, PlainTable.cellText(row: row, column: column))
                XCTAssertEqual(headers.count, 1, "row \(row) announces its column header")
                XCTAssertEqual((headers.first as? NSObject)?.accessibilityLabel, PlainTable.cellText(row: PlainTable.headerRow, column: column))
                XCTAssertTrue(try tableElement() === table, "row \(row) keeps the same table element")
                XCTAssertTrue(table.accessibilityDataTableCellElement(forRow: row, column: column) === element,
                              "row \(row) keeps focus on the same element after its reveal")
                XCTAssertTrue(drawing.isLiveAccessibilityElement(table), "row \(row) keeps the table element live")
            }
            XCTAssertTrue(table.accessibilityDataTableCellElement(forRow: 1, column: 0) === firstBody,
                          "cell elements do not change identity while scrolling")
        }
    }

    func testFramePositionsMatchEngineForTwentyThousandCellsAndMultipleParagraphs() throws {
        let size = Self.twentyThousandSlotTables[0]
        let multiParagraph = TableInputTestSchema.twoCellDocument.replacingOccurrences(
            of: #""text":"one"}]}"#,
            with: #""text":"one"}]},{"type":"paragraph"},{"type":"paragraph","content":[{"type":"text","text":"😀two"}]}"#
        )
        for document in [try plainTableDocument(rows: size.rows, columns: size.columns), multiParagraph] {
            let editorId = makeV2Editor(configJson: TableInputTestSchema.tableConfig)
            defer { destroyV2Editor(id: editorId) }
            let adapter = try XCTUnwrap(EditorV2Registry.adapter(forLegacyId: editorId))
            XCTAssertNotNil(adapter.setContentJson(document))
            let revision = adapter.baseDocumentRevision
            for key in adapter.tableIndex.tableKeys {
                let record = try XCTUnwrap(adapter.tableIndex.record(tableKey: key))
                for (cellIndex, cell) in record.cells.enumerated() {
                    let docStart = try XCTUnwrap(adapter.tableIndex.docStart(tableKey: key, cellIndex: cellIndex))
                    let scalarStart = try XCTUnwrap(adapter.tableIndex.scalarStart(tableKey: key, cellIndex: cellIndex))
                    for block in cell.inputBlocks {
                        let doc = docStart + block.docStart
                        let scalar = scalarStart + block.contentScalarStart
                        let canonicalScalar = scalarStart + (block.docStart == block.docEnd ? block.scalarEnd : block.contentScalarStart)
                        XCTAssertEqual(adapter.scalarPosition(forDoc: doc), canonicalScalar, "cell \(cellIndex) doc \(doc)")
                        let result = editorV2ScalarToDoc(editorId: adapter.editorId, scalar: scalar)
                        XCTAssertNil(result.error)
                        let json = try XCTUnwrap(result.value)
                        let object = try XCTUnwrap(JSONSerialization.jsonObject(with: Data(json.utf8)) as? [String: Any])
                        XCTAssertEqual(try XCTUnwrap(EditorV2Adapter.uint32Field(object, "doc")), doc, "cell \(cellIndex) scalar \(scalar)")
                    }
                }
            }
            XCTAssertEqual(adapter.installedFrameRevision, revision)
        }
    }

    func testOneCellDeltaPreparesOneCellAndRemeasuresNothingElse() throws {
        let editorId = makeV2Editor(configJson: TableInputTestSchema.tableConfig)
        defer { destroyV2Editor(id: editorId) }
        let adapter = try XCTUnwrap(EditorV2Registry.adapter(forLegacyId: editorId))
        let window = makeTestWindow(frame: CGRect(origin: .zero, size: Window.viewport))
        let view = RichTextEditorView(frame: window.bounds)
        window.addSubview(view)
        window.makeKeyAndVisible()
        defer { window.isHidden = true }
        view.bindEditor(id: editorId, initialUpdateJSON: try XCTUnwrap(adapter.initialUpdateJSON()))
        let document = try plainTableDocument(rows: EditedTable.rows, columns: EditedTable.columns)
        XCTAssertTrue(view.textView.applyUpdateJSON(try XCTUnwrap(adapter.setContentJson(document))))
        view.layoutIfNeeded()
        let surface = try XCTUnwrap(view.subviews.compactMap { $0 as? EditorTableSurface }.first)
        let point = CGPoint(x: surface.bounds.midX, y: surface.bounds.midY)
        let edited = try XCTUnwrap(surface.cellHit(at: point))
        XCTAssertTrue(view.activateTableCell(at: point))
        let input = view.activeTextInput
        XCTAssertTrue(input.becomeFirstResponder())
        var prepared: [Int] = []
        surface.onTableCellPreparedForTesting = { index, _ in prepared.append(index) }

        let replacementsBefore = surface.incrementalRelayoutsForTesting
        let fullBefore = adapter.fullFrameAdoptionCountForTesting
        let deltaBefore = adapter.deltaFrameAdoptionCountForTesting
        input.insertText(EditedTable.typed)
        view.layoutIfNeeded()

        let editedText = try adapter.tableCellTexts().joined().filter { $0.contains(EditedTable.typed) }
        print("typing into cell \(edited.cellIndex) prepared cells \(prepared) of \(EditedTable.rows * EditedTable.columns)")
        XCTAssertEqual(adapter.fullFrameAdoptionCountForTesting, fullBefore)
        XCTAssertEqual(adapter.deltaFrameAdoptionCountForTesting, deltaBefore + 1)
        XCTAssertEqual(adapter.cachedTablePresentation?.changes.changedCells[edited.tableID], IndexSet(integer: Int(edited.cellIndex)))
        XCTAssertEqual(editedText.count, 1, "the keystroke lands in exactly one cell")
        XCTAssertEqual(prepared.count, 1, "only the edited cell is measured again: \(prepared)")
        XCTAssertEqual(surface.incrementalRelayoutsForTesting, replacementsBefore + 1)
    }
    func testWrappingEditMovesLaterRowsWithoutPreparingThem() throws {
        try withMountedTable(rows: 4, columns: 2) { view, surface, drawing in
            let adapter = try XCTUnwrap(EditorV2Registry.adapter(forLegacyId: view.editorId))
            let before = try XCTUnwrap(drawing.mountedTablePresentation()?.tables.first?.surface)
            let tableID = try adapter.editableTableID()
            let later = try XCTUnwrap(before.cell(sourceIndex: 2))
            let previousY = before.frame(ofCell: later).minY
            XCTAssertTrue(view.bindTableCell(tableID: tableID, cellIndex: 0, contentRect: .zero))
            let input = view.activeTextInput
            input.selectedRange = NSRange(location: input.textStorage.length, length: 0)
            var prepared: [Int] = []
            surface.onTableCellPreparedForTesting = { index, _ in prepared.append(index) }
            input.insertText(String(repeating: " wrapping text", count: 12))
            view.layoutIfNeeded()
            let after = try XCTUnwrap(drawing.mountedTablePresentation()?.tables.first?.surface)
            let shifted = try XCTUnwrap(after.cell(sourceIndex: 2))
            XCTAssertGreaterThan(after.frame(ofCell: shifted).minY, previousY)
            XCTAssertTrue(shifted.content === later.content, "unchanged rows retain their prepared layout")
            XCTAssertEqual(prepared, [0])
        }
    }

    func testMatchingInputTextSkipsInputRerender() throws {
        try withMountedTable(rows: 2, columns: 2) { view, _, _ in
            let adapter = try XCTUnwrap(EditorV2Registry.adapter(forLegacyId: view.editorId))
            let key = try adapter.editableTableID()
            XCTAssertTrue(view.bindTableCell(tableID: key, cellIndex: 0, contentRect: .zero))
            let input = view.activeTextInput
            let before = input.inputRerendersForTesting
            let epoch = input.tableCellPositionMap?.binding.positionEpoch
            XCTAssertNotNil(adapter.currentStateJSON())
            XCTAssertTrue(view.bindTableCell(tableID: key, cellIndex: 0, contentRect: .zero))
            XCTAssertEqual(input.inputRerendersForTesting, before)
            XCTAssertNotEqual(input.tableCellPositionMap?.binding.positionEpoch, epoch)
            view.textView.baseTextColor = .red
            XCTAssertTrue(view.bindTableCell(tableID: key, cellIndex: 0, contentRect: .zero))
            XCTAssertEqual(input.inputRerendersForTesting, before + 1, "format changes still render")
        }
    }

    func testStructuralReplacementRemeasuresNoUnchangedContent() throws {
        try withMountedTable(rows: 3, columns: 2) { view, surface, drawing in
            let adapter = try XCTUnwrap(EditorV2Registry.adapter(forLegacyId: view.editorId))
            let before = try XCTUnwrap(drawing.mountedTablePresentation()?.tables.first?.surface)
            let retained = Set(before.cells.map { ObjectIdentifier($0.content) })
            var prepared: [Int] = []
            surface.onTableCellPreparedForTesting = { index, _ in prepared.append(index) }
            let key = try adapter.editableTableID()
            let scalar = try XCTUnwrap(adapter.tableIndex.scalarStart(tableKey: key, cellIndex: 4))
            XCTAssertNotNil(adapter.syncSelection(anchor: scalar, head: scalar))
            XCTAssertTrue(view.textView.applyUpdateJSON(try XCTUnwrap(adapter.applyTableCommandAtSelection(
                try XCTUnwrap(TableAccessibilityAction.all.first { $0.key == "addRowAfter" }).command, admission: try XCTUnwrap(adapter.tableMutationAdmission(tableID: key))))))
            view.layoutIfNeeded()
            let after = try XCTUnwrap(drawing.mountedTablePresentation()?.tables.first?.surface)
            XCTAssertEqual(after.cells.filter { retained.contains(ObjectIdentifier($0.content)) }.count, before.cells.count)
            XCTAssertEqual(prepared.count, 1, "the new row shares one empty-cell shape")
        }
    }

    func testStructuralReuseKeepsIdenticalCellsIndependentlyDrawable() throws {
        try withMountedTable(rows: 4, columns: 2, repeatedText: "repeated cell") { view, surface, drawing in
            let adapter = try XCTUnwrap(EditorV2Registry.adapter(forLegacyId: view.editorId))
            let tableID = try adapter.editableTableID()
            XCTAssertTrue(view.bindTableCell(tableID: tableID, cellIndex: 2, contentRect: .zero))
            let command = try XCTUnwrap(TableAccessibilityAction.all.first { $0.key == "addRowAfter" }).command
            for _ in 0..<2 {
                let previous = try XCTUnwrap(drawing.mountedTablePresentation()?.tables.first?.surface.sourceTable)
                let oldKeys = Set(previous.cells.map(\.contentKey))
                var preparedKeys: [String] = []
                surface.onTableCellPreparedForTesting = { _, key in preparedKeys.append(key) }
                let update = try XCTUnwrap(adapter.commandAtSelection(command, anchor: 0, head: 0))
                XCTAssertTrue(view.textView.applyUpdateJSON(update))
                view.layoutIfNeeded()
                let table = try XCTUnwrap(drawing.mountedTablePresentation()?.tables.first?.surface)
                let identities = Set(table.cells.map { ObjectIdentifier($0.content) })
                XCTAssertEqual(identities.count, table.cells.count,
                    "Equal text must preserve separate drawing identities and active-cell exclusion")
                guard identities.count == table.cells.count else { return }
                drawing.layer.displayIfNeeded()
                try assertLayeredMatchesSinglePass(drawing)
                XCTAssertEqual(preparedKeys.filter { oldKeys.contains($0) }.count, 0,
                    "New bindings with existing content should share its shape without reshaping")
            }
        }
    }

    func testNonWrappingKeystrokeRedrawsOnlyTheBoundCellLayer() throws {
        try withMountedTable(rows: 12, columns: 2) { view, _, drawing in
            let adapter = try XCTUnwrap(EditorV2Registry.adapter(forLegacyId: view.editorId))
            XCTAssertTrue(view.bindTableCell(tableID: try adapter.editableTableID(), cellIndex: 4, contentRect: .zero))
            let input = view.activeTextInput
            input.selectedRange = NSRange(location: input.textStorage.length, length: 0)
            drawing.layer.displayIfNeeded()
            let before = drawing.layerRedrawsForTesting
            input.insertText("x")
            view.layoutIfNeeded()
            drawing.layer.displayIfNeeded()
            for name in ["above", "boundRow", "below"] {
                XCTAssertEqual(drawing.layerRedrawsForTesting[name], before[name], name)
            }
            XCTAssertEqual(drawing.layerRedrawsForTesting["boundCell", default: 0], before["boundCell", default: 0] + 1)
        }
    }

    func testWrappingKeystrokeRedrawsTheBoundRowAndTranslatesRowsBelow() throws {
        try withMountedTable(rows: 12, columns: 2) { view, _, drawing in
            let adapter = try XCTUnwrap(EditorV2Registry.adapter(forLegacyId: view.editorId))
            XCTAssertTrue(view.bindTableCell(tableID: try adapter.editableTableID(), cellIndex: 4, contentRect: .zero))
            let input = view.activeTextInput
            input.selectedRange = NSRange(location: input.textStorage.length, length: 0)
            drawing.layer.displayIfNeeded()
            let before = drawing.layerRedrawsForTesting
            let below = drawing.belowLayer.position
            input.insertText(String(repeating: " wrapping text", count: 8))
            view.layoutIfNeeded()
            drawing.layer.displayIfNeeded()
            for name in ["above", "below"] {
                XCTAssertEqual(drawing.layerRedrawsForTesting[name], before[name], name)
            }
            XCTAssertEqual(drawing.layerRedrawsForTesting["boundRow", default: 0], before["boundRow", default: 0] + 1)
            XCTAssertGreaterThan(drawing.belowLayer.position.y, below.y)
            try assertLayeredMatchesSinglePass(drawing)
        }
    }

    func testRemoteWrappingAboveViewportKeepsCachedRowsCovered() throws {
        try withMountedTable(rows: 60, columns: 2) { view, _, drawing in
            let adapter = try XCTUnwrap(EditorV2Registry.adapter(forLegacyId: view.editorId))
            let boundCell = 4
            XCTAssertTrue(view.bindTableCell(tableID: try adapter.editableTableID(), cellIndex: UInt32(boundCell), contentRect: .zero))
            let input = view.activeTextInput
            let text = input.textStorage.string
            let scalar = try XCTUnwrap(input.tableCellPositionMap?.globalScalar(forLocalUTF16: text.utf16.count, in: text))
            view.textView.contentOffset.y = Window.viewport.height
            view.layoutIfNeeded()
            drawing.layer.displayIfNeeded()
            let window = drawing.bounds
            let before = try XCTUnwrap(drawing.mountedTablePresentation()?.tables.first?.surface)
            let oldHeight = before.layout.rowOffsets[3] - before.layout.rowOffsets[2]
            let oldOffset = view.textView.contentOffset
            let remote = RemoteTablePeer(adapter: adapter, requestIdBase: 90_000)
            try remote.applySelection(EditorV2PositionBridge.textSelectionEnvelope(anchor: scalar, head: scalar, affinity: "before"))
            try remote.applyCommand(["type": "insertText", "text": String(repeating: " wrapping text", count: 8)])
            let preflight = input.prepareForExternalEditorUpdateResult()
            XCTAssertTrue(preflight.ready)
            XCTAssertTrue(view.textView.applyUpdateJSON(try XCTUnwrap(preflight.adoptedUpdateJSON ?? adapter.refreshFromRustState(mirrorSelection: nil))))
            view.layoutIfNeeded()
            drawing.layer.displayIfNeeded()
            let after = try XCTUnwrap(drawing.mountedTablePresentation()?.tables.first?.surface)
            XCTAssertGreaterThan(after.layout.rowOffsets[3] - after.layout.rowOffsets[2], oldHeight)
            XCTAssertEqual(view.textView.contentOffset, oldOffset, "remote growth must leave the scrolled viewport fixed")
            XCTAssertEqual(drawing.bounds, window)
            XCTAssertLessThan(after.layout.rowOffsets[3], drawing.bounds.minY, "the changed row must remain above the viewport")
            try assertLayeredMatchesSinglePass(drawing)
        }
    }

    func testUnboundWindowUsesOneLayer() throws {
        try withMountedTable(rows: 12, columns: 2) { _, _, drawing in
            drawing.layer.displayIfNeeded()
            let layers = [drawing.aboveLayer, drawing.boundRowLayer, drawing.boundCellLayer, drawing.belowLayer]
            XCTAssertEqual(layers.filter { !$0.isHidden }.count, 1)
            XCTAssertEqual(drawing.layerRedrawsForTesting.values.reduce(0, +), 1)
            drawing.install(layout: nil)
            XCTAssertTrue(layers.allSatisfy { $0.contents == nil && $0.isHidden }, "releasing the surface frees every cached bitmap")
        }
    }

    func testHorizontalScrollingMatchesAFullRepaintAtPartialCellEdges() throws {
        try withMountedTable(rows: 12, columns: 20) { view, _, drawing in
            let table = try XCTUnwrap(drawing.mountedTablePresentation()?.tables.first?.surface)
            let columnWidth = try XCTUnwrap(table.layout.columnOffsets.dropFirst().first)
            let maximum = table.bounds.width - table.hostViewportWidth
            let pixel = 1 / drawing.contentScaleFactor
            let offsets: [CGFloat] = [0, pixel / 2, columnWidth - pixel / 2,
                columnWidth, columnWidth + pixel / 2, maximum / 2, maximum - pixel / 2,
                maximum, maximum / 2, 0]
            for offset in offsets {
                try XCTContext.runActivity(named: "horizontal offset \(offset)") { _ in
                    let current = drawing.tableLogicalOffset(for: table.identity)
                    drawing.scrollTables(in: [table.scrollIdentity], by: current - offset)
                    view.layoutIfNeeded()
                    try assertLayeredMatchesSinglePass(drawing)
                }
            }
        }
    }

    func testFractionalRootScrollingKeepsBoundTableLayersAligned() throws {
        try withMountedTable(rows: 12, columns: 20) { view, _, drawing in
            let adapter = try XCTUnwrap(EditorV2Registry.adapter(forLegacyId: view.editorId))
            XCTAssertTrue(view.bindTableCell(tableID: try adapter.editableTableID(), cellIndex: 2, contentRect: .zero))
            view.layoutIfNeeded()
            let pixel = 1 / drawing.contentScaleFactor
            view.textView.contentOffset.y += pixel / 2
            view.layoutIfNeeded()
            let table = try XCTUnwrap(drawing.mountedTablePresentation()?.tables.first?.surface)
            let columnWidth = try XCTUnwrap(table.layout.columnOffsets.dropFirst().first)
            let current = drawing.tableLogicalOffset(for: table.identity)
            drawing.scrollTables(in: [table.scrollIdentity], by: current - columnWidth - pixel / 2)
            view.layoutIfNeeded()
            try assertLayeredMatchesSinglePass(drawing)
        }
    }

    func testHorizontalScrollingKeepsEnteringHeaderBackgroundOpaque() throws {
        let frameCount = 120
        let channels = 4
        let opaqueAlpha: UInt8 = 255
        for bindsHeader in [false, true] {
            try withMountedTable(rows: 1_000, columns: 20) { view, _, drawing in
                let table = try XCTUnwrap(drawing.mountedTablePresentation()?.tables.first?.surface)
                if bindsHeader {
                    let adapter = try XCTUnwrap(EditorV2Registry.adapter(forLegacyId: view.editorId))
                    XCTAssertTrue(view.bindTableCell(tableID: try adapter.editableTableID(), cellIndex: 2, contentRect: .zero))
                    view.layoutIfNeeded()
                }
                let scale = drawing.contentScaleFactor
                let maximum = table.bounds.width - table.hostViewportWidth
                for frame in 0...frameCount {
                    let fraction = CGFloat((1 - cos(Double(frame) / Double(frameCount) * 2 * .pi)) / 2)
                    let current = drawing.tableLogicalOffset(for: table.identity)
                    drawing.scrollTables(in: [table.scrollIdentity], by: current - fraction * maximum)
                    view.layoutIfNeeded()
                    let geometry = try XCTUnwrap(drawing.mountedTablePresentation()?.tables.first)
                    let clip = geometry.clip.intersection(drawing.bounds)
                    let start = ceil((clip.minX + table.style.borderWidth) * scale) / scale
                    let end = floor((clip.maxX - table.style.borderWidth) * scale) / scale
                    let width = Int(((end - start) * scale).rounded())
                    XCTAssertGreaterThan(width, 0)
                    let y = floor(geometry.bounds.minY + table.style.borderWidth + table.style.cellPadding / 2)
                    var pixels = [UInt8](repeating: 0, count: width * channels)
                    let context = try XCTUnwrap(CGContext(data: &pixels, width: width, height: 1,
                        bitsPerComponent: 8, bytesPerRow: width * channels,
                        space: CGColorSpaceCreateDeviceRGB(),
                        bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue | CGBitmapInfo.byteOrder32Big.rawValue))
                    context.scaleBy(x: scale, y: scale)
                    context.translateBy(x: -start, y: -y)
                    drawing.layer.displayIfNeeded()
                    for layer in [drawing.aboveLayer, drawing.boundRowLayer, drawing.boundCellLayer, drawing.belowLayer]
                    where !layer.isHidden {
                        let image = try XCTUnwrap(layer.contents) as! CGImage
                        XCTAssertEqual(image.width, Int((layer.bounds.width * scale).rounded()),
                            "frame \(frame): bitmap width must match its physical-pixel frame \(layer.frame)")
                        XCTAssertEqual(image.height, Int((layer.bounds.height * scale).rounded()),
                            "frame \(frame): bitmap height must match its physical-pixel frame \(layer.frame)")
                    }
                    drawing.layer.render(in: context)
                    let transparent = (0..<width).filter { pixels[$0 * channels + channels - 1] != opaqueAlpha }
                    XCTAssertTrue(transparent.isEmpty,
                        "header scanline (bound: \(bindsHeader), scale: \(scale)) lost opacity at frame \(frame), offset \(fraction * maximum), pixels \(transparent)")
                }
            }
        }
    }

    private func assertLayeredMatchesSinglePass(_ drawing: PreparedProseDrawingView,
                                               file: StaticString = #filePath, line: UInt = #line) throws {
        let format = UIGraphicsImageRendererFormat()
        format.scale = drawing.contentScaleFactor
        let layered = UIGraphicsImageRenderer(bounds: drawing.bounds, format: format).image { _ in
            drawing.drawInstalledLayersForTesting()
        }
        drawing.usesEditAnchoredLayers = false
        defer { drawing.usesEditAnchoredLayers = true }
        let painted = UIGraphicsImageRenderer(bounds: drawing.bounds, format: format).image { _ in
            drawing.draw(drawing.bounds)
        }
        let reference = CALayer()
        reference.bounds = CGRect(origin: .zero, size: drawing.bounds.size)
        reference.contentsScale = format.scale
        reference.contents = painted.cgImage
        let singlePass = UIGraphicsImageRenderer(size: drawing.bounds.size, format: format).image { renderer in
            reference.render(in: renderer.cgContext)
        }
        let layeredPixels = try XCTUnwrap(layered.cgImage?.dataProvider?.data) as Data
        let singlePassPixels = try XCTUnwrap(singlePass.cgImage?.dataProvider?.data) as Data
        if layeredPixels != singlePassPixels {
            for (name, image) in [("cached layers", layered), ("full repaint", singlePass)] {
                let attachment = XCTAttachment(image: image)
                attachment.name = name
                attachment.lifetime = .keepAlways
                add(attachment)
            }
        }
        XCTAssertTrue(layeredPixels == singlePassPixels,
            "translated layers must match a complete repaint pixel for pixel", file: file, line: line)
    }

    func testWrappingInsideRowspanKeepsCrossingContentAndFollowingRowsAligned() throws {
        try withMountedTable(rows: 4, columns: 2) { view, _, drawing in
            let adapter = try XCTUnwrap(EditorV2Registry.adapter(forLegacyId: view.editorId))
            func cell(_ text: String, span: Int = 1) -> [String: Any] {
                ["type": PlainTable.cell, "attrs": ["rowspan": span], "content": [
                    ["type": PlainTable.paragraph, "content": [["type": PlainTable.text, "text": text]]]
                ]]
            }
            let rows = [
                [cell(String(repeating: "spanning content ", count: 20), span: 3), cell("first")],
                [cell("edited")], [cell("last")], [cell("below left"), cell("below right")]
            ].map { ["type": PlainTable.row, "content": $0] as [String: Any] }
            let data = try JSONSerialization.data(withJSONObject: ["type": PlainTable.document,
                "content": [["type": PlainTable.table, "content": rows]]])
            XCTAssertTrue(view.textView.applyUpdateJSON(try XCTUnwrap(adapter.setContentJson(String(decoding: data, as: UTF8.self)))))
            view.layoutIfNeeded()
            XCTAssertTrue(view.bindTableCell(tableID: try adapter.editableTableID(), cellIndex: 2, contentRect: .zero))
            let input = view.activeTextInput
            input.selectedRange = NSRange(location: input.textStorage.length, length: 0)
            drawing.layer.displayIfNeeded()
            input.insertText(String(repeating: " wrapping text", count: 8))
            view.layoutIfNeeded()
            drawing.layer.displayIfNeeded()
            try assertLayeredMatchesSinglePass(drawing)
        }
    }

}
