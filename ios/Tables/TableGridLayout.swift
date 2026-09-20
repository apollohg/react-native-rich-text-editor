import UIKit

enum TableLayoutDirection { case leftToRight, rightToLeft }

func tablePhysicalX(logicalX: CGFloat, width: CGFloat, totalWidth: CGFloat, rtl: Bool) -> CGFloat {
    rtl ? totalWidth - logicalX - width : logicalX
}

struct TableGridCell: Hashable {
    let sourcePosition: Int
    let row: Int
    let column: Int
    let rowspan: Int
    let colspan: Int
    let contentKey: String
    let attachmentRevision: Int

    init(sourcePosition: Int, row: Int, column: Int, rowspan: Int = 1, colspan: Int = 1,
         contentKey: String, attachmentRevision: Int = 0) {
        self.sourcePosition = sourcePosition; self.row = row; self.column = column
        self.rowspan = rowspan; self.colspan = colspan; self.contentKey = contentKey
        self.attachmentRevision = attachmentRevision
    }
}

struct TableGridRecord {
    let documentOwner: String
    let columns: Int
    let rows: Int
    let columnWidths: [CGFloat?]
    let cells: [TableGridCell]
    let failure: TableRenderFailure?
    let compatibilityDiagnostic: TableCompatibilityDiagnostic?

    init(documentOwner: String, columns: Int, rows: Int, columnWidths: [CGFloat?], cells: [TableGridCell],
         failure: TableRenderFailure? = nil, compatibilityDiagnostic: TableCompatibilityDiagnostic? = nil) {
        self.documentOwner = documentOwner; self.columns = columns; self.rows = rows
        self.columnWidths = columnWidths; self.cells = cells; self.failure = failure
        self.compatibilityDiagnostic = compatibilityDiagnostic
    }

    init(table: FfiViewerTable, documentOwner: String) {
        self.init(documentOwner: documentOwner, columns: Int(table.columns), rows: Int(table.rows),
                  columnWidths: table.columnWidths.map { $0.map(CGFloat.init) },
                  cells: table.cells.map { TableGridCell(sourcePosition: Int($0.sourcePos), row: Int($0.row), column: Int($0.column), rowspan: Int($0.rowspan), colspan: Int($0.colspan), contentKey: $0.contentKey) },
                  failure: table.failure, compatibilityDiagnostic: table.compatibilityDiagnostic)
    }
}

struct TableLayoutResult {
    let columnWidths: [CGFloat]
    let rowOffsets: [CGFloat]
    let rectangles: [Int: CGRect]
    let sourceOrder: [Int]
    let contentSize: CGSize
    let failure: TableRenderFailure?
    let compatibilityDiagnostic: TableCompatibilityDiagnostic?
}

final class TableGridLayout {
    private let displayScale: CGFloat
    private let cache: TableCellMeasurementCache

    init(displayScale: CGFloat = UIScreen.main.scale, cache: TableCellMeasurementCache = TableCellMeasurementCache()) {
        self.displayScale = displayScale.isFinite && displayScale > 0 ? displayScale : 1
        self.cache = cache
    }

    func layout(record: TableGridRecord, viewportWidth: CGFloat, style: TableStyle, direction: TableLayoutDirection,
                themeDigest: String = "", fontEnvironmentRevision: Int = 0, textScale: CGFloat = 1,
                measureCell: (TableGridCell, CGFloat) -> CGFloat?) -> TableLayoutResult {
        let minimumRow = style.cellPadding * 2 + style.borderWidth * 2
        let fallbackHeight = minimumRow.isFinite && minimumRow >= 1 ? minimumRow : 1
        let fallbackWidth: CGFloat
        if viewportWidth.isFinite, viewportWidth >= 0 {
            fallbackWidth = max(fallbackHeight, min(viewportWidth, style.minColumnWidth.isFinite ? style.minColumnWidth : fallbackHeight))
        } else {
            fallbackWidth = fallbackHeight
        }
        let invalidInput = !style.isValid || !viewportWidth.isFinite || viewportWidth < 0 || record.columns <= 0 || record.rows <= 0 || record.columnWidths.contains { ($0?.isFinite == false) || ($0 ?? 0) < 0 }
        guard !invalidInput, record.failure == nil else {
            return fallback(record.failure ?? .invalidAttributes, record, width: fallbackWidth, height: fallbackHeight)
        }
        var widths = (0..<record.columns).map { index -> CGFloat in
            let requested = index < record.columnWidths.count ? record.columnWidths[index] : nil
            return max(style.minColumnWidth, requested ?? 0)
        }
        let specified = widths.enumerated().filter { $0.offset < record.columnWidths.count && record.columnWidths[$0.offset] != nil }.map(\.offset)
        let unspecified = Set(0..<record.columns).subtracting(specified)
        let minimumWidth = widths.reduce(0, +)
        guard minimumWidth.isFinite else { return fallback(.invalidAttributes, record, width: fallbackWidth, height: fallbackHeight) }
        let surplus = viewportWidth - minimumWidth
        if surplus > 0, !unspecified.isEmpty {
            let share = surplus / CGFloat(unspecified.count)
            for index in unspecified { widths[index] += share }
        }
        widths = widths.map { snapOutward($0) }
        guard widths.allSatisfy(\.isFinite) else { return fallback(.invalidAttributes, record, width: fallbackWidth, height: fallbackHeight) }
        var xOffsets: [CGFloat] = [0]
        for width in widths {
            let offset = xOffsets.last! + width
            guard offset.isFinite else { return fallback(.invalidAttributes, record, width: fallbackWidth, height: fallbackHeight) }
            xOffsets.append(offset)
        }
        var heights = Array(repeating: fallbackHeight, count: record.rows)
        let ordered = record.cells.sorted { $0.sourcePosition < $1.sourcePosition }
        guard ordered.allSatisfy({ valid($0, in: record) }) else {
            return fallback(.invalidStructure, record, width: fallbackWidth, height: fallbackHeight)
        }
        func contentHeight(_ cell: TableGridCell, _ inner: CGFloat) -> CGFloat? {
            let pixels = inner * displayScale
            let roundedPixels = pixels.rounded()
            guard roundedPixels.isFinite, roundedPixels >= 0,
                  let innerWidthPixels = Int(exactly: roundedPixels) else { return nil }
            let measuredWidth = CGFloat(innerWidthPixels) / displayScale
            let key = TableCellMeasurementKey(documentOwner: record.documentOwner, contentKey: cell.contentKey,
                                              innerWidthPixels: innerWidthPixels, themeDigest: themeDigest,
                                              fontEnvironmentRevision: fontEnvironmentRevision, textScale: textScale,
                                              attachmentRevision: cell.attachmentRevision)
            if let cached = cache.value(for: key) { return cached }
            guard let measured = measureCell(cell, measuredWidth), measured.isFinite, measured >= 0 else { return nil }
            let content = measured
            cache.insert(content, for: key)
            return content
        }
        for cell in ordered where cell.rowspan == 1 {
            let start = xOffsets[cell.column], end = xOffsets[cell.column + cell.colspan]
            let inner = max(0, end - start - 2 * (style.cellPadding + style.borderWidth))
            guard let content = contentHeight(cell, inner) else { return fallback(.invalidAttributes, record, width: fallbackWidth, height: fallbackHeight) }
            let wanted = max(fallbackHeight, content + 2 * (style.cellPadding + style.borderWidth))
            guard wanted.isFinite else { return fallback(.invalidAttributes, record, width: fallbackWidth, height: fallbackHeight) }
            heights[cell.row] = max(heights[cell.row], wanted)
        }
        for cell in ordered where cell.rowspan > 1 {
            let inner = max(0, xOffsets[cell.column + cell.colspan] - xOffsets[cell.column] - 2 * (style.cellPadding + style.borderWidth))
            guard let content = contentHeight(cell, inner) else { return fallback(.invalidAttributes, record, width: fallbackWidth, height: fallbackHeight) }
            let wanted = max(fallbackHeight, content + 2 * (style.cellPadding + style.borderWidth))
            guard wanted.isFinite else { return fallback(.invalidAttributes, record, width: fallbackWidth, height: fallbackHeight) }
            let covered = cell.row..<(cell.row + cell.rowspan)
            let current = covered.reduce(CGFloat(0)) { $0 + heights[$1] }
            guard current.isFinite else { return fallback(.invalidAttributes, record, width: fallbackWidth, height: fallbackHeight) }
            if wanted > current {
                let height = heights[covered.upperBound - 1] + wanted - current
                guard height.isFinite else { return fallback(.invalidAttributes, record, width: fallbackWidth, height: fallbackHeight) }
                heights[covered.upperBound - 1] = height
            }
        }
        var rows: [CGFloat] = [0]
        for height in heights {
            let offset = rows.last! + snapOutward(height)
            guard offset.isFinite else { return fallback(.invalidAttributes, record, width: fallbackWidth, height: fallbackHeight) }
            rows.append(offset)
        }
        let total = xOffsets.last!
        var rectangles: [Int: CGRect] = [:]
        for cell in ordered where valid(cell, in: record) {
            let logical = xOffsets[cell.column], width = xOffsets[cell.column + cell.colspan] - logical
            rectangles[cell.sourcePosition] = CGRect(x: tablePhysicalX(logicalX: logical, width: width, totalWidth: total, rtl: direction == .rightToLeft), y: rows[cell.row], width: width, height: rows[cell.row + cell.rowspan] - rows[cell.row])
        }
        return TableLayoutResult(columnWidths: widths, rowOffsets: rows, rectangles: rectangles, sourceOrder: ordered.map(\.sourcePosition), contentSize: CGSize(width: total, height: rows.last!), failure: nil, compatibilityDiagnostic: record.compatibilityDiagnostic)
    }

    private func valid(_ cell: TableGridCell, in record: TableGridRecord) -> Bool {
        cell.row >= 0 && cell.column >= 0 && cell.rowspan > 0 && cell.colspan > 0 &&
        cell.rowspan <= record.rows - cell.row && cell.colspan <= record.columns - cell.column
    }

    private func fallback(_ failure: TableRenderFailure, _ record: TableGridRecord, width: CGFloat, height: CGFloat) -> TableLayoutResult {
        TableLayoutResult(columnWidths: [], rowOffsets: [0, height], rectangles: [:], sourceOrder: [], contentSize: CGSize(width: width, height: height), failure: failure, compatibilityDiagnostic: record.compatibilityDiagnostic)
    }

    private func snapOutward(_ value: CGFloat) -> CGFloat { ceil(value * displayScale) / displayScale }
}
