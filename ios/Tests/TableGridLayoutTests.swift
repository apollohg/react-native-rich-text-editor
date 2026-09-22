import XCTest

final class TableGridLayoutTests: XCTestCase {
    func testViewerSurfaceRetainsOnePreparedCellPerSourceAnchor() {
        let cells = [
            TableGridCell(sourcePosition: 10, row: 0, column: 0, contentKey: "a"),
            TableGridCell(sourcePosition: 20, row: 0, column: 1, contentKey: "b")
        ]
        let record = TableGridRecord(documentOwner: "viewer", columns: 2, rows: 1,
                                     columnWidths: [nil, nil], cells: cells)
        let surface = ViewerTableSurface(
            identity: "t1",
            record: record,
            viewportWidth: 160,
            style: TableStyle(),
            direction: .leftToRight
        ) { cell, width in
            return PreparedProseLayout.error(
                key: ProseLayoutKey(semanticKey: cell.contentKey, widthPixels: Int(width), themeDigest: "", nativeFontRevision: 0,
                                    fontEnvironmentRevision: 0, displayScale: 1, attachmentRevision: 0,
                                    generationIdentity: "test", semanticGenerationIdentity: "test"),
                width: width,
                error: .layout(message: "test")
            )
        }

        XCTAssertEqual(surface.cells.map(\.sourcePosition), [10, 20])
        XCTAssertEqual(surface.cells.count, 2)
        XCTAssertTrue(surface.bounds.height.isFinite)
        XCTAssertEqual(surface.visibleCells(in: surface.bounds).count, 2)
    }

    func testViewerSurfaceMeasuresEqualContentOncePerSourceAndKeepsSourceArtifacts() {
        let cells = [
            TableGridCell(sourcePosition: 10, row: 0, column: 0, contentKey: "same"),
            TableGridCell(sourcePosition: 20, row: 0, column: 1, contentKey: "same")
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
            preparedSources.append(cell.sourcePosition)
            preparedContentKeys.append(cell.contentKey)
            return PreparedProseLayout.error(
                key: ProseLayoutKey(semanticKey: "\(cell.sourcePosition)", widthPixels: Int(width), themeDigest: "", nativeFontRevision: 0,
                                    fontEnvironmentRevision: 0, displayScale: 1, attachmentRevision: 0,
                                    generationIdentity: "test", semanticGenerationIdentity: "test"),
                width: width,
                error: .layout(message: "test")
            )
        }

        XCTAssertEqual(surface.cells.map(\.sourcePosition), [10, 20])
        XCTAssertNotEqual(surface.cells[0].content.key.semanticKey, surface.cells[1].content.key.semanticKey)
        XCTAssertEqual(preparedSources, [10, 20])
        XCTAssertEqual(preparedContentKeys, ["same", "same"])
    }

    private func record(
        columns: Int = 2,
        rows: Int = 1,
        widths: [CGFloat?] = [nil, nil],
        cells: [TableGridCell] = []
    ) -> TableGridRecord {
        TableGridRecord(documentOwner: "test-document", columns: columns, rows: rows,
                        columnWidths: widths, cells: cells)
    }

    func testUnspecifiedColumnsShareSurplusAndOverflowKeepsMinimum() {
        let layout = TableGridLayout().layout(record: record(), viewportWidth: 240,
                                               style: TableStyle(), direction: .leftToRight) { _, _ in 10 }
        XCTAssertEqual(layout.columnWidths, [120, 120])

        let overflow = TableGridLayout().layout(record: record(), viewportWidth: 100,
                                                 style: TableStyle(), direction: .leftToRight) { _, _ in 10 }
        XCTAssertEqual(overflow.columnWidths, [80, 80])
        XCTAssertEqual(overflow.contentSize.width, 160)
    }

    func testSpansMeasureInnerWidthAndRowspanAddsDeficitToLastCoveredRow() {
        let cells = [
            TableGridCell(sourcePosition: 10, row: 0, column: 0, rowspan: 2, colspan: 1, contentKey: "a"),
            TableGridCell(sourcePosition: 20, row: 0, column: 1, contentKey: "b"),
            TableGridCell(sourcePosition: 30, row: 1, column: 1, contentKey: "c")
        ]
        var measuredWidths: [CGFloat] = []
        let result = TableGridLayout().layout(record: record(rows: 2, cells: cells), viewportWidth: 160,
                                              style: TableStyle(), direction: .leftToRight) { cell, width in
            measuredWidths.append(width)
            return cell.sourcePosition == 10 ? 80 : 20
        }
        XCTAssertEqual(measuredWidths.first, 62)
        XCTAssertEqual(result.rowOffsets, [0, 38, 98])
        XCTAssertEqual(result.rectangles[10]?.height, 98)
    }

    func testRtlMirrorsPhysicalXWithoutChangingSourceOrder() {
        let cells = [
            TableGridCell(sourcePosition: 10, row: 0, column: 0, contentKey: "a"),
            TableGridCell(sourcePosition: 20, row: 0, column: 1, contentKey: "b")
        ]
        let result = TableGridLayout().layout(record: record(cells: cells), viewportWidth: 160,
                                              style: TableStyle(), direction: .rightToLeft) { _, _ in 10 }
        XCTAssertEqual(result.rectangles[10]?.minX, 80)
        XCTAssertEqual(result.rectangles[20]?.minX, 0)
        XCTAssertEqual(result.sourceOrder, [10, 20])
    }

    func testFailureAndEmptyTablesHaveFiniteMinimumFrameAndProvenance() {
        let failure = TableGridRecord(documentOwner: "test-document", columns: 0, rows: 0,
                                      columnWidths: [], cells: [], failure: .gridLimit)
        let result = TableGridLayout().layout(record: failure, viewportWidth: 200,
                                              style: TableStyle(), direction: .leftToRight) { _, _ in 0 }
        XCTAssertEqual(result.failure, .gridLimit)
        XCTAssertGreaterThan(result.contentSize.height, 0)
        XCTAssertEqual(result.rowOffsets.count, 2)
    }

    func testCacheIncludesOwnerAttachmentAndPixelWidthAndIsBounded() {
        let cache = TableCellMeasurementCache(capacity: 1)
        let a = TableCellMeasurementKey(documentOwner: "a", contentKey: "cell", innerWidthPixels: 120,
                                        themeDigest: "theme", fontEnvironmentRevision: 1, textScale: 1,
                                        attachmentRevision: 1)
        let changedAttachment = TableCellMeasurementKey(documentOwner: "a", contentKey: "cell", innerWidthPixels: 120,
                                                        themeDigest: "theme", fontEnvironmentRevision: 1, textScale: 1,
                                                        attachmentRevision: 2)
        cache.insert(20, for: a)
        XCTAssertEqual(cache.value(for: a), 20)
        XCTAssertNil(cache.value(for: changedAttachment))
        cache.insert(30, for: changedAttachment)
        XCTAssertNil(cache.value(for: a))
    }

    func testFractionalScaleSnapsOutwardWithoutUndershootingMinimum() {
        let style = TableStyle(minColumnWidth: 80.2)
        let result = TableGridLayout(displayScale: 2).layout(record: record(), viewportWidth: 100,
                                                              style: style, direction: .leftToRight) { _, _ in 10 }
        XCTAssertGreaterThanOrEqual(result.columnWidths[0], 80.2)
        XCTAssertEqual(result.columnWidths[0] * 2, (result.columnWidths[0] * 2).rounded())
    }

    func testCacheMeasuresAtItsPixelKeyWidthAcrossFractionalChromeChanges() {
        let cache = TableCellMeasurementCache()
        let grid = TableGridLayout(displayScale: 2, cache: cache)
        let cell = TableGridCell(sourcePosition: 1, row: 0, column: 0, contentKey: "cell")
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
        let result = TableGridLayout().layout(record: record(cells: [TableGridCell(sourcePosition: 1, row: 0, column: 0, contentKey: "cell")]), viewportWidth: 160,
                                              style: TableStyle(), direction: .leftToRight) { _, _ in .infinity }
        XCTAssertEqual(result.failure, TableRenderFailure.invalidAttributes)
        XCTAssertEqual(result.contentSize.height, 18)
    }

    func testHorizontalSpanKeepsSyntheticGapAnchorFreeAndProcessesLaterSingleRowMinimum() {
        let cells = [
            TableGridCell(sourcePosition: 10, row: 0, column: 0, rowspan: 2, contentKey: "span"),
            TableGridCell(sourcePosition: 20, row: 0, column: 1, colspan: 2, contentKey: "wide"),
            TableGridCell(sourcePosition: 30, row: 1, column: 2, contentKey: "later")
        ]
        let result = TableGridLayout().layout(record: record(columns: 3, rows: 2, widths: [80, 80, 80], cells: cells), viewportWidth: 240,
                                              style: TableStyle(), direction: .leftToRight) { cell, _ in
            switch cell.sourcePosition {
            case 10: return 102
            case 20: return 20
            default: return 60
            }
        }

        XCTAssertEqual(result.rectangles[20]?.width, 160)
        XCTAssertEqual(result.rowOffsets, [0, 38, 120])
        XCTAssertEqual(result.rectangles.count, cells.count)
        XCTAssertEqual(Set(result.rectangles.keys), Set(cells.map(\.sourcePosition)))
        XCTAssertEqual(result.sourceOrder, [10, 20, 30])
    }

    func testWarmLayoutsReuseMeasurementsAndDependencyChangesInvalidateOnlyTheirKeys() {
        let cache = TableCellMeasurementCache()
        let grid = TableGridLayout(cache: cache)
        var cells = [
            TableGridCell(sourcePosition: 10, row: 0, column: 0, contentKey: "a"),
            TableGridCell(sourcePosition: 20, row: 1, column: 0, contentKey: "b")
        ]
        var calls = 0
        var measuredPositions: [Int] = []
        func layout(width: CGFloat = 100, theme: String = "theme", fontRevision: Int = 1) -> TableLayoutResult {
            grid.layout(record: record(columns: 1, rows: 2, widths: [width], cells: cells), viewportWidth: width,
                        style: TableStyle(), direction: .leftToRight, themeDigest: theme,
                        fontEnvironmentRevision: fontRevision) { cell, _ in
                calls += 1
                measuredPositions.append(cell.sourcePosition)
                return 20
            }
        }

        let first = layout()
        let warm = layout()
        XCTAssertEqual(first.rectangles, warm.rectangles)
        XCTAssertEqual(calls, 2)

        cells[0] = TableGridCell(sourcePosition: 10, row: 0, column: 0, contentKey: "a", attachmentRevision: 1)
        _ = layout()
        XCTAssertEqual(calls, 3)
        XCTAssertEqual(measuredPositions.last, 10)

        cells[0] = TableGridCell(sourcePosition: 10, row: 0, column: 0, contentKey: "updated", attachmentRevision: 1)
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
        let hot = TableCellMeasurementKey(documentOwner: "owner", contentKey: "hot", innerWidthPixels: 80,
                                          themeDigest: "theme", fontEnvironmentRevision: 1, textScale: 1,
                                          attachmentRevision: 0)
        cache.insert(20, for: hot)
        for index in 0..<1_000 {
            XCTAssertEqual(cache.value(for: hot), 20)
            cache.insert(20, for: TableCellMeasurementKey(documentOwner: "owner", contentKey: "cold-\(index)", innerWidthPixels: 80,
                                                           themeDigest: "theme", fontEnvironmentRevision: 1, textScale: 1,
                                                           attachmentRevision: 0))
        }
        XCTAssertEqual(cache.value(for: hot), 20)
        XCTAssertLessThanOrEqual(cache.metadataCount, 2)
    }

    func testPixelWidthAtIntMaximumFallsBackWithoutTrapping() {
        let result = TableGridLayout(displayScale: 1).layout(record: record(columns: 1, widths: [CGFloat(Int.max)],
                                                              cells: [TableGridCell(sourcePosition: 1, row: 0, column: 0, contentKey: "cell")]),
                                              viewportWidth: CGFloat(Int.max), style: TableStyle(cellPadding: 0, borderWidth: 0),
                                              direction: .leftToRight) { _, _ in 10 }
        XCTAssertEqual(result.failure, .invalidAttributes)
        XCTAssertTrue(result.contentSize.width.isFinite)
        XCTAssertTrue(result.contentSize.height.isFinite)
    }
}
