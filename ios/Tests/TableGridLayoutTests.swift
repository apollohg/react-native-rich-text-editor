import XCTest

final class TableGridLayoutTests: XCTestCase {
    func testLayoutRectanglesAreKeyedBySourceIndex() {
        let cells = [
            TableGridCell(sourceIndex: 1, row: 0, column: 1, contentKey: "second"),
            TableGridCell(sourceIndex: 0, row: 0, column: 0, contentKey: "first")
        ]
        let layout = TableGridLayout(displayScale: 1).layout(
            record: record(cells: cells), viewportWidth: 160,
            style: TableStyle(), direction: .leftToRight
        ) { _, _ in 10 }
        XCTAssertEqual(Set(layout.rectangles.keys), [0, 1])
        XCTAssertEqual(layout.sourceOrder, [0, 1])
        XCTAssertEqual(layout.columnOffsets, [0, 80, 160])
        XCTAssertEqual(layout.rectangles[0]?.minX, 0)
        XCTAssertEqual(layout.rectangles[1]?.minX, 80)
    }

    func testViewerSurfaceRetainsOnePreparedCellPerSourceAnchor() {
        let cells = [
            TableGridCell(sourceIndex: 10, row: 0, column: 0, contentKey: "a"),
            TableGridCell(sourceIndex: 20, row: 0, column: 1, contentKey: "b")
        ]
        let record = TableGridRecord(
            documentOwner: "viewer",
            columns: 2,
            rows: 1,
            columnWidths: [nil, nil],
            cells: cells
        )
        let surface = ViewerTableSurface(
            identity: "t1",
            record: record,
            viewportWidth: 160,
            style: TableStyle(),
            direction: .leftToRight
        ) { cell, width in
            return PreparedProseLayout.error(
                key: ProseLayoutKey(
                    semanticKey: cell.contentKey,
                    widthPixels: Int(width),
                    themeDigest: "",
                    nativeFontRevision: 0,
                    fontEnvironmentRevision: 0,
                    displayScale: 1,
                    attachmentRevision: 0,
                    generationIdentity: "test",
                    semanticGenerationIdentity: "test"
                ),
                width: width,
                error: .layout(message: "test")
            )
        }

        XCTAssertEqual(surface.cells.map(\.sourceIndex), [10, 20])
        XCTAssertEqual(surface.cells.count, 2)
        XCTAssertTrue(surface.bounds.height.isFinite)
        XCTAssertEqual(surface.visibleCells(in: surface.bounds).count, 2)
    }

    func testViewerSurfaceMeasuresEqualContentOncePerSourceAndKeepsSourceArtifacts() {
        let cells = [
            TableGridCell(sourceIndex: 10, row: 0, column: 0, contentKey: "same"),
            TableGridCell(sourceIndex: 20, row: 0, column: 1, contentKey: "same")
        ]
        var preparedSources: [Int] = []
        var preparedContentKeys: [String] = []
        let surface = ViewerTableSurface(
            identity: "t1",
            record: TableGridRecord(documentOwner: "viewer", columns: 2, rows: 1, columnWidths: [nil, nil], cells: cells),
            viewportWidth: 160,
            style: TableStyle(),
            direction: .leftToRight
        ) { cell, width in
            preparedSources.append(cell.sourceIndex)
            preparedContentKeys.append(cell.contentKey)
            return PreparedProseLayout.error(
                key: ProseLayoutKey(
                    semanticKey: "\(cell.sourceIndex)",
                    widthPixels: Int(width),
                    themeDigest: "",
                    nativeFontRevision: 0,
                    fontEnvironmentRevision: 0,
                    displayScale: 1,
                    attachmentRevision: 0,
                    generationIdentity: "test",
                    semanticGenerationIdentity: "test"
                ),
                width: width,
                error: .layout(message: "test")
            )
        }

        XCTAssertEqual(surface.cells.map(\.sourceIndex), [10, 20])
        XCTAssertNotEqual(surface.cells[0].content.key.semanticKey, surface.cells[1].content.key.semanticKey)
        XCTAssertEqual(preparedSources, [10, 20])
        XCTAssertEqual(preparedContentKeys, ["same", "same"])
    }

    func testCellReplacementMatchesFreshGeometryAndPreservesPreviousSurface() throws {
        let gridCells = [
            TableGridCell(sourceIndex: 0, row: 0, column: 0, rowspan: 2, contentKey: "span-a"),
            TableGridCell(sourceIndex: 1, row: 0, column: 1, colspan: 2, contentKey: "wide"),
            TableGridCell(sourceIndex: 2, row: 1, column: 1, rowspan: 2, contentKey: "span-b"),
            TableGridCell(sourceIndex: 3, row: 1, column: 2, contentKey: "short"),
            TableGridCell(sourceIndex: 4, row: 2, column: 0, contentKey: "last-a"),
            TableGridCell(sourceIndex: 5, row: 2, column: 2, contentKey: "last-b")
        ]
        let record = record(columns: 3, rows: 3, widths: [81.2, 83.4, 85.6], cells: gridCells)
        let viewport: CGFloat = 251.3
        let style = TableStyle(cellPadding: 8.05)
        for direction in [TableLayoutDirection.leftToRight, .rightToLeft] {
            for scale in [CGFloat(1), 2, 3] {
                var heights: [Int: CGFloat] = [0: 120.3, 1: 24.1, 2: 140.7, 3: 18.2, 4: 22.4, 5: 19.1]
                var labels = Dictionary(uniqueKeysWithValues: gridCells.map { ($0.sourceIndex, $0.contentKey) })
                func content(_ cell: TableGridCell, _ width: CGFloat, revision: Int) -> PreparedProseLayout {
                    let key = ProseLayoutKey(
                        semanticKey: "cell-\(cell.sourceIndex)-\(revision)",
                        widthPixels: Int((width * scale).rounded()),
                        themeDigest: "test",
                        nativeFontRevision: 0,
                        fontEnvironmentRevision: 0,
                        displayScale: scale,
                        attachmentRevision: 0,
                        generationIdentity: "test",
                        semanticGenerationIdentity: "test"
                    )
                    let size = CGSize(width: width, height: heights[cell.sourceIndex]!)
                    let node = PreparedProseAccessibilityNode(
                        interactionIndex: nil,
                        role: .text,
                        label: labels[cell.sourceIndex]!,
                        bounds: CGRect(origin: .zero, size: size)
                    )
                    return PreparedProseLayout(
                        key: key,
                        size: size,
                        blocks: [],
                        accessibilityNodes: [node],
                        retainedBytes: node.estimatedRetainedBytes
                    )
                }
                var surface = ViewerTableSurface(
                    identity: "incremental",
                    record: record,
                    viewportWidth: viewport,
                    style: style,
                    direction: direction,
                    displayScale: scale,
                    prepareCell: { content($0, $1, revision: 0) }
                )
                // Same height, spanning growth/shrink, then a cell below its row's maximum.
                let edits: [(Int, CGFloat)] = [(1, 24.1), (0, 200.7), (2, 230.2), (0, 30.1), (2, 28.3), (3, 19.2)]
                for (revision, edit) in edits.enumerated() {
                    let (index, height) = edit
                    let old = surface
                    let oldRectangles = old.layout.rectangles
                    let oldSizes = old.cells.map(\.contentSize)
                    let cell = try XCTUnwrap(old.cell(sourceIndex: index))
                    heights[index] = height
                    labels[index, default: ""] += " edited"
                    let replacement = content(gridCells[index], cell.contentSize.width, revision: revision + 1)
                    surface = old.replacingCells([index: replacement], contentHeights: [index: height])
                    let fresh = ViewerTableSurface(
                        identity: "fresh",
                        record: record,
                        viewportWidth: viewport,
                        style: style,
                        direction: direction,
                        displayScale: scale,
                        prepareCell: { content($0, $1, revision: revision + 1) }
                    )
                    let context = "direction=\(direction) scale=\(scale) edit=\(revision) cell=\(index) height=\(height)"
                    XCTAssertEqual(surface.layout.rowOffsets, fresh.layout.rowOffsets, context)
                    XCTAssertEqual(surface.layout.columnOffsets, fresh.layout.columnOffsets, context)
                    XCTAssertEqual(surface.layout.rectangles, fresh.layout.rectangles, context)
                    XCTAssertEqual(surface.metadataRetainedBytes, fresh.metadataRetainedBytes, context)
                    XCTAssertGreaterThan(surface.metadataRetainedBytes, old.metadataRetainedBytes, context)
                    XCTAssertEqual(
                        surface.visibleCells(in: surface.bounds).map(\.sourceIndex),
                        fresh.visibleCells(in: fresh.bounds).map(\.sourceIndex),
                        context
                    )
                    XCTAssertEqual(old.layout.rectangles, oldRectangles, context)
                    XCTAssertEqual(old.cells.map(\.contentSize), oldSizes, context)
                    for sibling in old.cells where sibling.sourceIndex != index {
                        XCTAssertTrue(surface.cell(sourceIndex: sibling.sourceIndex) === sibling, context)
                    }
                }
            }
        }
    }

    private func record(
        columns: Int = 2,
        rows: Int = 1,
        widths: [CGFloat?] = [nil, nil],
        cells: [TableGridCell] = []
    ) -> TableGridRecord {
        TableGridRecord(
            documentOwner: "test-document",
            columns: columns,
            rows: rows,
            columnWidths: widths,
            cells: cells
        )
    }

    func testUnspecifiedColumnsShareSurplusAndOverflowKeepsMinimum() {
        let layout = TableGridLayout().layout(
            record: record(),
            viewportWidth: 240,
            style: TableStyle(),
            direction: .leftToRight
        ) { _, _ in 10 }
        XCTAssertEqual(layout.columnWidths, [120, 120])

        let overflow = TableGridLayout().layout(
            record: record(),
            viewportWidth: 100,
            style: TableStyle(),
            direction: .leftToRight
        ) { _, _ in 10 }
        XCTAssertEqual(overflow.columnWidths, [80, 80])
        XCTAssertEqual(overflow.contentSize.width, 160)
    }

    func testSpansMeasureInnerWidthAndRowspanAddsDeficitToLastCoveredRow() {
        let cells = [
            TableGridCell(sourceIndex: 10, row: 0, column: 0, rowspan: 2, colspan: 1, contentKey: "a"),
            TableGridCell(sourceIndex: 20, row: 0, column: 1, contentKey: "b"),
            TableGridCell(sourceIndex: 30, row: 1, column: 1, contentKey: "c")
        ]
        var measuredWidths: [CGFloat] = []
        let result = TableGridLayout().layout(
            record: record(rows: 2, cells: cells),
            viewportWidth: 160,
            style: TableStyle(),
            direction: .leftToRight
        ) { cell, width in
            measuredWidths.append(width)
            return cell.sourceIndex == 10 ? 80 : 20
        }
        XCTAssertEqual(measuredWidths.first, 62)
        XCTAssertEqual(result.rowOffsets, [0, 38, 98])
        XCTAssertEqual(result.rectangles[10]?.height, 98)
        let cached = TableGridLayout().relayout(
            record: record(rows: 2, cells: cells),
            viewportWidth: 160,
            style: TableStyle(),
            direction: .leftToRight,
            cachedContentHeights: [10: 80, 20: 20, 30: 20]
        )
        XCTAssertEqual(cached.rowOffsets, [0, 38, 98])
        XCTAssertEqual(cached.rectangles, result.rectangles)
        let invalid = TableGridLayout().relayout(
            record: record(rows: 2, cells: cells),
            viewportWidth: 160,
            style: TableStyle(),
            direction: .leftToRight,
            cachedContentHeights: [10: .nan, 20: 20, 30: 20]
        )
        XCTAssertEqual(invalid.failure, .invalidAttributes)
    }

    func testRtlMirrorsPhysicalXWithoutChangingSourceOrder() {
        let cells = [
            TableGridCell(sourceIndex: 10, row: 0, column: 0, contentKey: "a"),
            TableGridCell(sourceIndex: 20, row: 0, column: 1, contentKey: "b")
        ]
        let result = TableGridLayout().layout(
            record: record(cells: cells),
            viewportWidth: 160,
            style: TableStyle(),
            direction: .rightToLeft
        ) { _, _ in 10 }
        XCTAssertEqual(result.rectangles[10]?.minX, 80)
        XCTAssertEqual(result.rectangles[20]?.minX, 0)
        XCTAssertEqual(result.sourceOrder, [10, 20])
    }

    func testFailureAndEmptyTablesHaveFiniteMinimumFrameAndProvenance() {
        let failure = TableGridRecord(
            documentOwner: "test-document",
            columns: 0,
            rows: 0,
            columnWidths: [],
            cells: [],
            failure: .gridLimit
        )
        let result = TableGridLayout().layout(
            record: failure,
            viewportWidth: 200,
            style: TableStyle(),
            direction: .leftToRight
        ) { _, _ in 0 }
        XCTAssertEqual(result.failure, .gridLimit)
        XCTAssertGreaterThan(result.contentSize.height, 0)
        XCTAssertEqual(result.rowOffsets.count, 2)
    }

    func testCacheIncludesOwnerAttachmentAndPixelWidthAndIsBounded() {
        let cache = TableCellMeasurementCache(capacity: 1)
        let originalKey = TableCellMeasurementKey(
            documentOwner: "a",
            contentKey: "cell",
            innerWidthPixels: 120,
            themeDigest: "theme",
            fontEnvironmentRevision: 1,
            textScale: 1,
            attachmentRevision: 1
        )
        let changedAttachment = TableCellMeasurementKey(
            documentOwner: "a",
            contentKey: "cell",
            innerWidthPixels: 120,
            themeDigest: "theme",
            fontEnvironmentRevision: 1,
            textScale: 1,
            attachmentRevision: 2
        )
        cache.insert(20, for: originalKey)
        XCTAssertEqual(cache.value(for: originalKey), 20)
        XCTAssertNil(cache.value(for: changedAttachment))
        cache.insert(30, for: changedAttachment)
        XCTAssertNil(cache.value(for: originalKey))
    }

    func testFractionalScaleSnapsOutwardWithoutUndershootingMinimum() {
        let style = TableStyle(minColumnWidth: 80.2)
        let result = TableGridLayout(displayScale: 2).layout(
            record: record(),
            viewportWidth: 100,
            style: style,
            direction: .leftToRight
        ) { _, _ in 10 }
        XCTAssertGreaterThanOrEqual(result.columnWidths[0], 80.2)
        XCTAssertEqual(result.columnWidths[0] * 2, (result.columnWidths[0] * 2).rounded())
    }

    func testCacheMeasuresAtItsPixelKeyWidthAcrossFractionalChromeChanges() {
        let cache = TableCellMeasurementCache()
        let grid = TableGridLayout(displayScale: 2, cache: cache)
        let cell = TableGridCell(sourceIndex: 1, row: 0, column: 0, contentKey: "cell")
        let input = record(columns: 1, widths: [100], cells: [cell])
        var widths: [CGFloat] = []
        _ = grid.layout(record: input, viewportWidth: 100, style: TableStyle(cellPadding: 8.05), direction: .leftToRight) { _, width in
            widths.append(width)
            return width
        }
        _ = grid.layout(record: input, viewportWidth: 100, style: TableStyle(cellPadding: 8.1), direction: .leftToRight) { _, width in
            widths.append(width)
            return width
        }
        XCTAssertEqual(widths, [82])
    }

    func testExtremeFiniteInputsReturnFiniteFailureFrame() {
        let result = TableGridLayout().layout(
            record: record(columns: 2, widths: [CGFloat.greatestFiniteMagnitude, CGFloat.greatestFiniteMagnitude]),
            viewportWidth: CGFloat.greatestFiniteMagnitude,
            style: TableStyle(), direction: .leftToRight
        ) { _, _ in 10 }
        XCTAssertEqual(result.failure, TableRenderFailure.invalidAttributes)
        XCTAssertTrue(result.contentSize.width.isFinite)
        XCTAssertTrue(result.contentSize.height.isFinite)
    }

    func testNonFiniteMeasurementReturnsFailureInsteadOfZeroHeightSuccess() {
        let result = TableGridLayout().layout(
            record: record(cells: [TableGridCell(sourceIndex: 1, row: 0, column: 0, contentKey: "cell")]),
            viewportWidth: 160,
            style: TableStyle(),
            direction: .leftToRight
        ) { _, _ in .infinity }
        XCTAssertEqual(result.failure, TableRenderFailure.invalidAttributes)
        XCTAssertEqual(result.contentSize.height, 18)
    }

    func testHorizontalSpanKeepsSyntheticGapAnchorFreeAndProcessesLaterSingleRowMinimum() {
        let cells = [
            TableGridCell(sourceIndex: 10, row: 0, column: 0, rowspan: 2, contentKey: "span"),
            TableGridCell(sourceIndex: 20, row: 0, column: 1, colspan: 2, contentKey: "wide"),
            TableGridCell(sourceIndex: 30, row: 1, column: 2, contentKey: "later")
        ]
        let result = TableGridLayout().layout(
            record: record(columns: 3, rows: 2, widths: [80, 80, 80], cells: cells),
            viewportWidth: 240,
            style: TableStyle(),
            direction: .leftToRight
        ) { cell, _ in
            switch cell.sourceIndex {
            case 10: return 102
            case 20: return 20
            default: return 60
            }
        }

        XCTAssertEqual(result.rectangles[20]?.width, 160)
        XCTAssertEqual(result.rowOffsets, [0, 38, 120])
        XCTAssertEqual(result.rectangles.count, cells.count)
        XCTAssertEqual(Set(result.rectangles.keys), Set(cells.map(\.sourceIndex)))
        XCTAssertEqual(result.sourceOrder, [10, 20, 30])
    }

    func testWarmLayoutsReuseMeasurementsAndDependencyChangesInvalidateOnlyTheirKeys() {
        let cache = TableCellMeasurementCache()
        let grid = TableGridLayout(cache: cache)
        var cells = [
            TableGridCell(sourceIndex: 10, row: 0, column: 0, contentKey: "a"),
            TableGridCell(sourceIndex: 20, row: 1, column: 0, contentKey: "b")
        ]
        var calls = 0
        var measuredPositions: [Int] = []
        func layout(width: CGFloat = 100, theme: String = "theme", fontRevision: Int = 1) -> TableLayoutResult {
            grid.layout(
                record: record(columns: 1, rows: 2, widths: [width], cells: cells),
                viewportWidth: width,
                style: TableStyle(),
                direction: .leftToRight,
                themeDigest: theme,
                fontEnvironmentRevision: fontRevision
            ) { cell, _ in
                calls += 1
                measuredPositions.append(cell.sourceIndex)
                return 20
            }
        }

        let first = layout()
        let warm = layout()
        XCTAssertEqual(first.rectangles, warm.rectangles)
        XCTAssertEqual(calls, 2)

        cells[0] = TableGridCell(sourceIndex: 10, row: 0, column: 0, contentKey: "a", attachmentRevision: 1)
        _ = layout()
        XCTAssertEqual(calls, 3)
        XCTAssertEqual(measuredPositions.last, 10)

        cells[0] = TableGridCell(sourceIndex: 10, row: 0, column: 0, contentKey: "updated", attachmentRevision: 1)
        let contentChanged = layout()
        XCTAssertEqual(calls, 4)
        XCTAssertEqual(measuredPositions.last, 10)
        XCTAssertEqual(contentChanged.rectangles, layout().rectangles)
        XCTAssertEqual(calls, 4)
        _ = layout(theme: "new-theme")
        XCTAssertEqual(calls, 6)
        _ = layout(fontRevision: 2)
        XCTAssertEqual(calls, 8)
        _ = layout(width: 120)
        XCTAssertEqual(calls, 10)
    }

    func testCacheMetadataStaysBoundedAcrossRepeatedWarmHitsAndChurn() {
        let cache = TableCellMeasurementCache(capacity: 2)
        let hot = TableCellMeasurementKey(
            documentOwner: "owner",
            contentKey: "hot",
            innerWidthPixels: 80,
            themeDigest: "theme",
            fontEnvironmentRevision: 1,
            textScale: 1,
            attachmentRevision: 0
        )
        cache.insert(20, for: hot)
        for index in 0..<1_000 {
            XCTAssertEqual(cache.value(for: hot), 20)
            cache.insert(20, for: TableCellMeasurementKey(
                documentOwner: "owner",
                contentKey: "cold-\(index)",
                innerWidthPixels: 80,
                themeDigest: "theme",
                fontEnvironmentRevision: 1,
                textScale: 1,
                attachmentRevision: 0
            ))
        }
        XCTAssertEqual(cache.value(for: hot), 20)
        XCTAssertLessThanOrEqual(cache.metadataCount, 2)
    }

    func testPixelWidthAtIntMaximumFallsBackWithoutTrapping() {
        let result = TableGridLayout(displayScale: 1).layout(
            record: record(
                columns: 1,
                widths: [CGFloat(Int.max)],
                cells: [TableGridCell(sourceIndex: 1, row: 0, column: 0, contentKey: "cell")]
            ),
            viewportWidth: CGFloat(Int.max),
            style: TableStyle(cellPadding: 0, borderWidth: 0),
            direction: .leftToRight
        ) { _, _ in 10 }
        XCTAssertEqual(result.failure, .invalidAttributes)
        XCTAssertTrue(result.contentSize.width.isFinite)
        XCTAssertTrue(result.contentSize.height.isFinite)
    }
}
