import Foundation

enum EditorCellSelection: Equatable {
    case drawable(tableID: String, sourceIndices: Set<Int>)
    case unavailable(tableID: String)

    private struct Cell {
        let sourceIndex: Int
        let documentPosition: UInt32
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

    static func resolve(_ value: Any, index: EditorTableIndex) -> EditorCellSelection? {
        guard let endpoints = endpointPositions(value)
        else { return nil }
        return resolve(anchor: endpoints.anchor, head: endpoints.head, index: index)
    }

    static func resolve(anchor: UInt32, head: UInt32, index: EditorTableIndex) -> EditorCellSelection? {
        var drawable: [EditorCellSelection] = []
        var unavailable: [(extent: UInt32, selection: EditorCellSelection)] = []
        for tableID in index.tableKeys {
            guard let record = index.record(tableKey: tableID), let tableStart = index.tableDocStart(tableKey: tableID) else { continue }
            let tableEnd = tableStart + record.docSize
            if record.failure != nil {
                if tableStart <= anchor && anchor < tableEnd && tableStart <= head && head < tableEnd {
                    unavailable.append((record.docSize, .unavailable(tableID: tableID)))
                }
                continue
            }
            let cells = record.cells.enumerated().compactMap { cellIndex, cell -> Cell? in
                guard let source = index.docStart(tableKey: tableID, cellIndex: cellIndex) else { return nil }
                return Cell(
                    sourceIndex: cellIndex,
                    documentPosition: source,
                    top: cell.row,
                    left: cell.column,
                    bottom: cell.row + cell.rowspan,
                    right: cell.column + cell.colspan
                )
            }
            guard cells.filter({ $0.documentPosition == anchor }).count == 1,
                  cells.filter({ $0.documentPosition == head }).count == 1,
                  let first = cells.first(where: { $0.documentPosition == anchor }),
                  let last = cells.first(where: { $0.documentPosition == head })
            else { continue }
            var bounds = Bounds(
                top: min(first.top, last.top),
                left: min(first.left, last.left),
                bottom: max(first.bottom, last.bottom),
                right: max(first.right, last.right)
            )
            while true {
                let previous = bounds
                for cell in cells where cell.intersects(previous) { bounds.include(cell) }
                if bounds == previous { break }
            }
            drawable.append(.drawable(tableID: tableID, sourceIndices: Set(cells.filter { $0.intersects(bounds) }.map { $0.sourceIndex })))
        }
        if drawable.count == 1 { return drawable[0] }
        if !drawable.isEmpty { return nil }
        guard let smallest = unavailable.map(\.extent).min(),
              unavailable.filter({ $0.extent == smallest }).count == 1
        else { return nil }
        return unavailable.first { $0.extent == smallest }?.selection
    }

    static func endpointPositions(_ value: Any) -> (anchor: UInt32, head: UInt32)? {
        guard let selection = value as? [String: Any],
              Set(selection.keys) == ["type", "anchorCell", "headCell"],
              selection["type"] as? String == "cell",
              let anchor = v2ExactUInt32(selection["anchorCell"] as? NSNumber),
              let head = v2ExactUInt32(selection["headCell"] as? NSNumber)
        else { return nil }
        return (anchor, head)
    }
}
