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

    private func plainTableDocument(rows: Int, columns: Int) throws -> String {
        let tableRows: [[String: Any]] = (0..<rows).map { row in
            let type = row == PlainTable.headerRow ? PlainTable.headerCell : PlainTable.cell
            return ["type": PlainTable.row, "content": (0..<columns).map { column in [
                "type": type,
                "content": [["type": PlainTable.paragraph, "content": [["type": PlainTable.text, "text": PlainTable.cellText(row: row, column: column)]]]]
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
        _ body: (RichTextEditorView, EditorTableSurface, PreparedProseDrawingView) throws -> Void
    ) throws {
        let label = "\(rows)x\(columns)"
        let editorId = makeV2Editor(configJson: TableInputTestSchema.tableConfig)
        defer { destroyV2Editor(id: editorId) }
        let adapter = try XCTUnwrap(EditorV2Registry.adapter(forLegacyId: editorId))
        let window = UIWindow(frame: CGRect(origin: .zero, size: Window.viewport))
        let view = RichTextEditorView(frame: window.bounds)
        window.addSubview(view)
        window.makeKeyAndVisible()
        defer { window.isHidden = true }
        view.bindEditor(id: editorId, initialUpdateJSON: try XCTUnwrap(adapter.initialUpdateJSON()))
        let update = try XCTUnwrap(adapter.setContentJson(try plainTableDocument(rows: rows, columns: columns)),
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

    func testScrollingPreparesOnlyEnteringCells() throws {
        try withMountedTable(rows: 1000, columns: 20) { view, surface, drawing in
            var preparations: [Int] = []
            surface.onTableCellPreparedForTesting = { preparations.append($0) }
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
        let window = UIWindow(frame: CGRect(origin: .zero, size: Window.viewport))
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
        surface.onTableCellPreparedForTesting = { prepared.append($0) }

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
            surface.onTableCellPreparedForTesting = { prepared.append($0) }
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
            surface.onTableCellPreparedForTesting = { prepared.append($0) }
            let key = try adapter.editableTableID()
            let scalar = try XCTUnwrap(adapter.tableIndex.scalarStart(tableKey: key, cellIndex: 4))
            XCTAssertNotNil(adapter.syncSelection(anchor: scalar, head: scalar))
            XCTAssertTrue(view.textView.applyUpdateJSON(try XCTUnwrap(adapter.applyTableCommandAtSelection(
                try XCTUnwrap(TableAccessibilityAction.all.first { $0.key == "addRowAfter" }).command, admission: try XCTUnwrap(adapter.tableMutationAdmission(tableID: key))))))
            view.layoutIfNeeded()
            let after = try XCTUnwrap(drawing.mountedTablePresentation()?.tables.first?.surface)
            XCTAssertEqual(after.cells.filter { retained.contains(ObjectIdentifier($0.content)) }.count, before.cells.count)
            XCTAssertEqual(prepared.count, 2, "only the new row is prepared")
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

    private func assertLayeredMatchesSinglePass(_ drawing: PreparedProseDrawingView,
                                               file: StaticString = #filePath, line: UInt = #line) throws {
        let format = UIGraphicsImageRendererFormat()
        format.scale = drawing.contentScaleFactor
        func pixels() throws -> Data {
            let image = UIGraphicsImageRenderer(size: drawing.bounds.size, format: format).image { _ in
                drawing.drawInstalledLayersForTesting()
            }
            return try XCTUnwrap(image.cgImage?.dataProvider?.data) as Data
        }
        let layered = try pixels()
        drawing.usesEditAnchoredLayers = false
        defer { drawing.usesEditAnchoredLayers = true }
        let singlePass = try pixels()
        XCTAssertTrue(layered == singlePass, "translated layers must match a complete repaint pixel for pixel", file: file, line: line)
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
