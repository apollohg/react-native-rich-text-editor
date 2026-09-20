import XCTest

final class TableGridLayoutTests: XCTestCase {
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
}
