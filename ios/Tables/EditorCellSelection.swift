import Foundation

enum EditorCellSelection: Equatable {
    case drawable(tableID: String, sourcePositions: Set<Int>)
    case unavailable(tableID: String)

    private struct Cell {
        let sourcePosition: UInt32
        let top: UInt32
        let left: UInt32
        let bottom: UInt32
        let right: UInt32

        func intersects(_ bounds: Bounds) -> Bool {
            top < bounds.bottom && bottom > bounds.top && left < bounds.right && right > bounds.left
        }
    }

    private struct Bounds: Equatable {
        var top: UInt32
        var left: UInt32
        var bottom: UInt32
        var right: UInt32

        mutating func include(_ cell: Cell) {
            top = min(top, cell.top)
            left = min(left, cell.left)
            bottom = max(bottom, cell.bottom)
            right = max(right, cell.right)
        }
    }

    static func resolve(_ value: Any, records: [String: [String: Any]]) -> EditorCellSelection? {
        guard let selection = value as? [String: Any],
              Set(selection.keys) == ["type", "anchorCell", "headCell"],
              selection["type"] as? String == "cell",
              let anchor = v2ExactUInt32(selection["anchorCell"] as? NSNumber),
              let head = v2ExactUInt32(selection["headCell"] as? NSNumber)
        else { return nil }

        var drawable: [EditorCellSelection] = []
        var unavailable: [(extent: UInt32, selection: EditorCellSelection)] = []
        for (tableID, record) in records {
            guard let tableStart = EditorV2Adapter.uint32Field(record, "tablePos"),
                  let tableEnd = EditorV2Adapter.uint32Field(record, "sourceEnd"),
                  tableStart < tableEnd
            else { continue }
            if !(record["failure"] is NSNull) {
                if tableStart <= anchor && anchor < tableEnd && tableStart <= head && head < tableEnd {
                    unavailable.append((tableEnd - tableStart, .unavailable(tableID: tableID)))
                }
                continue
            }
            guard let rows = EditorV2Adapter.uint32Field(record, "rows"),
                  let columns = EditorV2Adapter.uint32Field(record, "columns"),
                  let rawCells = record["cells"] as? [[String: Any]]
            else { continue }
            var cells: [Cell] = []
            for raw in rawCells {
                guard let source = EditorV2Adapter.uint32Field(raw, "sourcePos"),
                      let row = EditorV2Adapter.uint32Field(raw, "row"),
                      let column = EditorV2Adapter.uint32Field(raw, "column"),
                      let rowspan = EditorV2Adapter.uint32Field(raw, "rowspan"),
                      let colspan = EditorV2Adapter.uint32Field(raw, "colspan"),
                      rowspan > 0, colspan > 0,
                      row < rows, column < columns,
                      rowspan <= rows - row, colspan <= columns - column,
                      tableStart < source, source < tableEnd
                else { return nil }
                cells.append(Cell(sourcePosition: source, top: row, left: column,
                                  bottom: row + rowspan, right: column + colspan))
            }
            guard cells.filter({ $0.sourcePosition == anchor }).count == 1,
                  cells.filter({ $0.sourcePosition == head }).count == 1,
                  let first = cells.first(where: { $0.sourcePosition == anchor }),
                  let last = cells.first(where: { $0.sourcePosition == head })
            else { continue }
            var bounds = Bounds(top: min(first.top, last.top), left: min(first.left, last.left),
                                bottom: max(first.bottom, last.bottom), right: max(first.right, last.right))
            while true {
                let previous = bounds
                for cell in cells where cell.intersects(previous) { bounds.include(cell) }
                if bounds == previous { break }
            }
            drawable.append(.drawable(tableID: tableID, sourcePositions: Set(cells.filter { $0.intersects(bounds) }.map { Int($0.sourcePosition) })))
        }
        if drawable.count == 1 { return drawable[0] }
        if !drawable.isEmpty { return nil }
        guard let smallest = unavailable.map(\.extent).min(),
              unavailable.filter({ $0.extent == smallest }).count == 1
        else { return nil }
        return unavailable.first { $0.extent == smallest }?.selection
    }
}
