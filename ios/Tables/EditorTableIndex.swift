import Foundation

struct TableFrameChanges: Equatable {
    let fullReset: Bool
    let replacedTables: Set<String>
    let removedTables: Set<String>
    let changedCells: [String: IndexSet]
}

enum TableFrameRejection: Error, Equatable {
    case baseRevisionMismatch(expected: UInt64?, actual: UInt64)
    case unknownTable(String)
    case cellIndexOutOfRange(String, Int)
    case cellStructureChanged(String, Int)
    case docSizeMismatch(String, expected: UInt32, actual: UInt32)
    case scalarSizeMismatch(String, expected: UInt32, actual: UInt32)
    case inputBlockOutOfStride(String, Int)
    case missingAttribute(String)
    case duplicateTableKey(String)
    case hostMissing(String)
    case extentsIncomplete
}

final class EditorTableIndex {
    private static let nodeBoundarySize: UInt32 = 2

    private struct Entry {
        var record: FfiTableRecord
        var docPrefix: [UInt32]
        var scalarPrefix: [UInt32]
        var attributeCounts: [String: Int]
        var nestedCells: [String: Int]
        var scalarSize: UInt32 { scalarPrefix.last ?? 0 }
    }

    private struct Origin {
        let doc: UInt32
        let scalar: UInt32?
    }

    private var entries: [String: Entry] = [:]
    private var attributes: [String: String] = [:]
    private(set) var attributeObjects: [String: [String: Any]] = [:]
    private var extents: [String: FfiTableExtent] = [:]
    private var origins: [String: Origin] = [:]
    private var roots: [String] = []

    func adopt(_ frame: FfiTableFrame, installedRevision: UInt64?, frameRevision: UInt64) -> Result<TableFrameChanges, TableFrameRejection> {
        do {
            return .success(try stage(frame, installedRevision: installedRevision, frameRevision: frameRevision))
        } catch let rejection as TableFrameRejection {
            return .failure(rejection)
        } catch {
            preconditionFailure("Table frame validation threw an undeclared error: \(error)")
        }
    }

    private func stage(_ frame: FfiTableFrame, installedRevision: UInt64?, frameRevision: UInt64) throws -> TableFrameChanges {
        let full = frame.kind == .full
        if !full {
            let base = frame.baseDocumentRevision.flatMap(UInt64.init)
            guard let base, String(base) == frame.baseDocumentRevision, base == installedRevision else {
                throw TableFrameRejection.baseRevisionMismatch(expected: installedRevision, actual: base ?? frameRevision)
            }
        }
        var next = full ? [:] : entries
        var pool = full ? [:] : attributes
        var objects = full ? [:] : attributeObjects
        var nextExtents = full ? [:] : extents
        var removed = Set<String>()
        var replaced = Set<String>()
        var changed: [String: IndexSet] = [:]
        for key in frame.removedAttributeKeys {
            pool.removeValue(forKey: key)
            objects.removeValue(forKey: key)
        }
        for attribute in frame.attributes {
            guard let data = attribute.json.data(using: .utf8),
                  let object = try? JSONSerialization.jsonObject(with: data) as? [String: Any] else {
                throw TableFrameRejection.missingAttribute(attribute.key)
            }
            pool[attribute.key] = attribute.json
            objects[attribute.key] = object
        }
        for key in frame.removedTableKeys {
            guard removed.insert(key).inserted else { throw TableFrameRejection.duplicateTableKey(key) }
            guard next.removeValue(forKey: key) != nil else { throw TableFrameRejection.unknownTable(key) }
        }
        for table in frame.tables {
            let key = table.tableKey
            guard replaced.insert(key).inserted, !removed.contains(key) else {
                throw TableFrameRejection.duplicateTableKey(key)
            }
            next[key] = try Self.entry(table, pool: pool)
        }
        for (key, updates) in Dictionary(grouping: frame.cellUpdates, by: \.tableKey) {
            guard var entry = next[key] else { throw TableFrameRejection.unknownTable(key) }
            var indexes = IndexSet()
            for update in updates {
                let index = Int(update.cellIndex)
                guard entry.record.cells.indices.contains(index) else { throw TableFrameRejection.cellIndexOutOfRange(key, index) }
                guard !replaced.contains(key), !indexes.contains(index),
                      Self.sameStructure(entry.record.cells[index], update.cell) else {
                    throw TableFrameRejection.cellStructureChanged(key, index)
                }
                try Self.validate(update.cell, tableKey: key, index: index, pool: pool)
                indexes.insert(index)
            }
            for index in indexes {
                for nested in entry.record.cells[index].nestedTables {
                    entry.nestedCells.removeValue(forKey: nested.tableKey)
                }
            }
            for update in updates {
                let index = Int(update.cellIndex)
                let old = entry.record.cells[index]
                let size = UInt64(entry.record.docSize) - UInt64(old.docSize) + UInt64(update.cell.docSize)
                guard let docSize = UInt32(exactly: size) else {
                    throw TableFrameRejection.docSizeMismatch(key, expected: entry.record.docSize, actual: update.cell.docSize)
                }
                for nested in update.cell.nestedTables {
                    guard entry.nestedCells.updateValue(index, forKey: nested.tableKey) == nil else {
                        throw TableFrameRejection.duplicateTableKey(nested.tableKey)
                    }
                }
                entry.record.docSize = docSize
                entry.record.cells[index] = update.cell
                entry.attributeCounts[old.attrsKey, default: 0] -= 1
                if entry.attributeCounts[old.attrsKey] == 0 { entry.attributeCounts.removeValue(forKey: old.attrsKey) }
                entry.attributeCounts[update.cell.attrsKey, default: 0] += 1
            }
            if let first = indexes.first { try Self.rebuildPrefixes(&entry, from: first) }
            next[key] = entry
            changed[key] = indexes
        }
        for key in frame.removedAttributeKeys where pool[key] == nil {
            if next.values.contains(where: { $0.attributeCounts[key] != nil }) {
                throw TableFrameRejection.missingAttribute(key)
            }
        }
        let sameRevision = !full && installedRevision == frameRevision
        if !sameRevision || !frame.extents.isEmpty {
            nextExtents = [:]
            for extent in frame.extents {
                guard nextExtents.updateValue(extent, forKey: extent.tableKey) == nil else {
                    throw TableFrameRejection.duplicateTableKey(extent.tableKey)
                }
            }
        }
        for (key, entry) in next {
            if let host = entry.record.host {
                guard let parent = next[host.tableKey], parent.record.cells.indices.contains(Int(host.cellIndex)),
                      let nested = parent.record.cells[Int(host.cellIndex)].nestedTables.first(where: { $0.tableKey == key }) else {
                    throw TableFrameRejection.hostMissing(key)
                }
                try Self.validateNested(nested, child: entry, parent: parent.record.cells[Int(host.cellIndex)])
            } else {
                guard let extent = nextExtents[key] else { throw TableFrameRejection.extentsIncomplete }
                guard extent.docSize == entry.record.docSize else {
                    throw TableFrameRejection.docSizeMismatch(key, expected: entry.record.docSize, actual: extent.docSize)
                }
                guard extent.scalarEnd >= extent.scalarStart else {
                    throw TableFrameRejection.scalarSizeMismatch(key, expected: entry.scalarSize, actual: 0)
                }
                let width = extent.scalarEnd - extent.scalarStart
                guard entry.record.failure != nil || width == entry.scalarSize else {
                    throw TableFrameRejection.scalarSizeMismatch(key, expected: entry.scalarSize, actual: width)
                }
                guard UInt64(extent.docStart) + UInt64(extent.docSize) <= UInt64(UInt32.max) else {
                    throw TableFrameRejection.extentsIncomplete
                }
            }
        }
        for (key, entry) in next {
            for (childKey, index) in entry.nestedCells {
                guard let child = next[childKey] else { throw TableFrameRejection.unknownTable(childKey) }
                guard child.record.host == FfiTableHost(tableKey: key, cellIndex: UInt32(index)) else {
                    throw TableFrameRejection.hostMissing(childKey)
                }
            }
        }
        let rootKeys = Set(next.filter { $0.value.record.host == nil }.keys)
        guard rootKeys == Set(nextExtents.keys) else { throw TableFrameRejection.extentsIncomplete }
        let orderedRoots = rootKeys.sorted { nextExtents[$0]!.docStart < nextExtents[$1]!.docStart }
        var previousDocEnd: UInt64 = 0
        var previousScalarEnd: UInt32 = 0
        for key in orderedRoots {
            let extent = nextExtents[key]!
            guard UInt64(extent.docStart) >= previousDocEnd, extent.scalarStart >= previousScalarEnd else {
                throw TableFrameRejection.extentsIncomplete
            }
            previousDocEnd = UInt64(extent.docStart) + UInt64(extent.docSize)
            previousScalarEnd = extent.scalarEnd
        }
        var nextOrigins: [String: Origin] = [:]
        for key in next.keys {
            var path: [String] = []
            var visited = Set<String>()
            var current = key
            while nextOrigins[current] == nil {
                guard visited.insert(current).inserted, let entry = next[current] else { throw TableFrameRejection.hostMissing(current) }
                path.append(current)
                guard let host = entry.record.host else {
                    let extent = nextExtents[current]!
                    nextOrigins[current] = Origin(doc: extent.docStart, scalar: extent.scalarStart)
                    break
                }
                current = host.tableKey
            }
            for childKey in path.reversed() where nextOrigins[childKey] == nil {
                let host = next[childKey]!.record.host!
                let parent = next[host.tableKey]!
                let origin = nextOrigins[host.tableKey]!
                let index = Int(host.cellIndex)
                let cell = parent.record.cells[index]
                let nested = cell.nestedTables.first { $0.tableKey == childKey }!
                let doc = UInt64(origin.doc) + Self.relativeDocStart(parent, index) + UInt64(nested.docOffset)
                guard let docStart = UInt32(exactly: doc) else { throw TableFrameRejection.hostMissing(childKey) }
                let scalar = origin.scalar.flatMap { start in
                    nested.scalarStart.flatMap { UInt32(exactly: UInt64(start) + UInt64(parent.scalarPrefix[index]) + UInt64($0)) }
                }
                nextOrigins[childKey] = Origin(doc: docStart, scalar: scalar)
            }
        }
        entries = next
        attributes = pool
        attributeObjects = objects
        extents = nextExtents
        origins = nextOrigins
        roots = orderedRoots
        return TableFrameChanges(fullReset: full, replacedTables: replaced, removedTables: removed, changedCells: changed)
    }

    private static func entry(_ record: FfiTableRecord, pool: [String: String]) throws -> Entry {
        let key = record.tableKey
        var counts: [String: Int] = [:]
        var nestedCells: [String: Int] = [:]
        for attribute in [record.attrsKey] + record.sourceRows.map(\.attrsKey) + record.syntheticRegions.map(\.attrsKey) {
            guard pool[attribute] != nil else { throw TableFrameRejection.missingAttribute(attribute) }
            counts[attribute, default: 0] += 1
        }
        var sourceRow = 0
        var rowCellCount: UInt32 = 0
        for (index, cell) in record.cells.enumerated() {
            while sourceRow < record.sourceRows.count && rowCellCount == record.sourceRows[sourceRow].cellCount {
                sourceRow += 1
                rowCellCount = 0
            }
            guard sourceRow < record.sourceRows.count, cell.sourceRow == UInt32(sourceRow),
                  cell.rowspan > 0, cell.colspan > 0, cell.row < record.rows, cell.column < record.columns,
                  cell.rowspan <= record.rows - cell.row, cell.colspan <= record.columns - cell.column else {
                throw TableFrameRejection.cellStructureChanged(key, index)
            }
            rowCellCount += 1
            try validate(cell, tableKey: key, index: index, pool: pool)
            counts[cell.attrsKey, default: 0] += 1
            for nested in cell.nestedTables {
                guard nestedCells.updateValue(index, forKey: nested.tableKey) == nil else {
                    throw TableFrameRejection.duplicateTableKey(nested.tableKey)
                }
            }
        }
        guard record.failure != nil || record.sourceRows.reduce(UInt64.zero, { $0 + UInt64($1.cellCount) }) == UInt64(record.cells.count) else {
            throw TableFrameRejection.cellIndexOutOfRange(key, record.cells.count)
        }
        var entry = Entry(
            record: record,
            docPrefix: Array(repeating: 0, count: record.cells.count + 1),
            scalarPrefix: Array(repeating: 0, count: record.cells.count + 1),
            attributeCounts: counts,
            nestedCells: nestedCells
        )
        try rebuildPrefixes(&entry, from: 0)
        let expected = UInt64(nodeBoundarySize) * UInt64(record.sourceRows.count + 1) + UInt64(entry.docPrefix.last ?? 0)
        guard record.failure != nil || expected == UInt64(record.docSize), record.failure == nil || record.cells.isEmpty else {
            throw TableFrameRejection.docSizeMismatch(key, expected: UInt32(clamping: expected), actual: record.docSize)
        }
        return entry
    }

    private static func rebuildPrefixes(_ entry: inout Entry, from first: Int) throws {
        for index in first..<entry.record.cells.count {
            let cell = entry.record.cells[index]
            guard let doc = UInt32(exactly: UInt64(entry.docPrefix[index]) + UInt64(cell.docSize)) else {
                throw TableFrameRejection.docSizeMismatch(entry.record.tableKey, expected: entry.record.docSize, actual: cell.docSize)
            }
            guard let scalar = UInt32(exactly: UInt64(entry.scalarPrefix[index]) + UInt64(cell.scalarStride)) else {
                throw TableFrameRejection.scalarSizeMismatch(entry.record.tableKey, expected: entry.scalarPrefix[index], actual: cell.scalarStride)
            }
            entry.docPrefix[index + 1] = doc
            entry.scalarPrefix[index + 1] = scalar
        }
    }

    private static func sameStructure(_ first: FfiTableCellRecord, _ second: FfiTableCellRecord) -> Bool {
        first.sourceRow == second.sourceRow && first.row == second.row && first.column == second.column
            && first.rowspan == second.rowspan && first.colspan == second.colspan && first.header == second.header
    }

    private static func validate(_ cell: FfiTableCellRecord, tableKey: String, index: Int, pool: [String: String]) throws {
        guard pool[cell.attrsKey] != nil else { throw TableFrameRejection.missingAttribute(cell.attrsKey) }
        var voidIndices = Set<UInt32>()
        for elementIndex in cell.voidElementIndices {
            guard Int(elementIndex) < cell.elements.count, voidIndices.insert(elementIndex).inserted else {
                throw TableFrameRejection.inputBlockOutOfStride(tableKey, index)
            }
            switch cell.elements[Int(elementIndex)] {
            case .inlineAtom, .blockAtom: break
            default: throw TableFrameRejection.inputBlockOutOfStride(tableKey, index)
            }
        }
        var previousDoc: UInt32 = 0
        var previousScalar: UInt32 = 0
        for block in cell.inputBlocks {
            guard Int(block.elementIndex) < cell.elements.count, previousDoc <= block.docStart,
                  block.docStart <= block.docEnd, block.docEnd <= cell.docSize,
                  previousScalar <= block.scalarStart, block.scalarStart <= block.contentScalarStart,
                  block.contentScalarStart <= block.scalarEnd, block.scalarEnd <= block.breakScalarEnd,
                  block.breakScalarEnd <= cell.scalarStride else {
                throw TableFrameRejection.inputBlockOutOfStride(tableKey, index)
            }
            previousDoc = block.docEnd
            previousScalar = block.breakScalarEnd
        }
    }

    private static func validateNested(_ nested: FfiCellNestedTable, child: Entry, parent: FfiTableCellRecord) throws {
        let key = child.record.tableKey
        guard Int(nested.elementIndex) < parent.elements.count,
              UInt64(nested.docOffset) + UInt64(nested.docSize) <= UInt64(parent.docSize) else {
            throw TableFrameRejection.hostMissing(key)
        }
        guard nested.docSize == child.record.docSize else {
            throw TableFrameRejection.docSizeMismatch(key, expected: child.record.docSize, actual: nested.docSize)
        }
        let width: UInt32
        switch (nested.scalarStart, nested.scalarEnd) {
        case let (.some(start), .some(end)) where start <= end && end <= parent.scalarStride: width = end - start
        case (.none, .none): width = 0
        default: throw TableFrameRejection.scalarSizeMismatch(key, expected: child.scalarSize, actual: 0)
        }
        guard child.record.failure != nil || width == child.scalarSize else {
            throw TableFrameRejection.scalarSizeMismatch(key, expected: child.scalarSize, actual: width)
        }
    }

    private static func relativeDocStart(_ entry: Entry, _ index: Int) -> UInt64 {
        UInt64(nodeBoundarySize) * (UInt64(entry.record.cells[index].sourceRow) + 1) + UInt64(entry.docPrefix[index])
    }

    var tableKeys: Set<String> { Set(entries.keys) }
    var rootExtents: [String: FfiTableExtent] { extents }

    func copy() -> EditorTableIndex {
        let result = EditorTableIndex()
        result.entries = entries
        result.attributes = attributes
        result.attributeObjects = attributeObjects
        result.extents = extents
        result.origins = origins
        result.roots = roots
        return result
    }

    func subtree(tableKeys: Set<String>) -> EditorTableIndex {
        let result = EditorTableIndex()
        var pending = Array(tableKeys)
        while let key = pending.popLast() {
            guard result.entries[key] == nil, let entry = entries[key] else { continue }
            result.entries[key] = entry
            result.origins[key] = origins[key]
            pending.append(contentsOf: entry.nestedCells.keys)
            for attribute in entry.attributeCounts.keys {
                result.attributes[attribute] = attributes[attribute]
                result.attributeObjects[attribute] = attributeObjects[attribute]
            }
        }
        return result
    }

    func tableDocStart(tableKey: String) -> UInt32? { origins[tableKey]?.doc }

    func record(tableKey: String) -> FfiTableRecord? { entries[tableKey]?.record }

    func docStart(tableKey: String, cellIndex: Int) -> UInt32? {
        guard let entry = entries[tableKey], entry.record.cells.indices.contains(cellIndex), let origin = origins[tableKey] else { return nil }
        return UInt32(exactly: UInt64(origin.doc) + Self.relativeDocStart(entry, cellIndex))
    }

    func scalarStart(tableKey: String, cellIndex: Int) -> UInt32? {
        guard let entry = entries[tableKey], entry.record.cells.indices.contains(cellIndex), let start = origins[tableKey]?.scalar else { return nil }
        return UInt32(exactly: UInt64(start) + UInt64(entry.scalarPrefix[cellIndex]))
    }

    func cellIndex(tableKey: String, containingDoc position: UInt32) -> Int? {
        guard let entry = entries[tableKey], let origin = origins[tableKey], position >= origin.doc else { return nil }
        let relative = UInt64(position - origin.doc)
        let index = Self.precedingIndex(entry.record.cells.count) { Self.relativeDocStart(entry, $0) <= relative }
        guard let index, relative < Self.relativeDocStart(entry, index) + UInt64(entry.record.cells[index].docSize) else { return nil }
        return index
    }

    func cellIndex(tableKey: String, containingScalar position: UInt32) -> Int? {
        guard let entry = entries[tableKey], let start = origins[tableKey]?.scalar, position >= start else { return nil }
        let relative = position - start
        let index = Self.precedingIndex(entry.record.cells.count) { entry.scalarPrefix[$0] <= relative }
        guard let index, relative < entry.scalarPrefix[index + 1]
            || (index == entry.record.cells.count - 1 && relative == entry.scalarPrefix[index + 1]) else { return nil }
        return index
    }

    func tableKey(containingDoc position: UInt32) -> String? { containingTable(position, scalar: false) }
    func tableKey(containingScalar position: UInt32) -> String? { containingTable(position, scalar: true) }

    private func containingTable(_ position: UInt32, scalar: Bool) -> String? {
        guard let rootIndex = Self.precedingIndex(roots.count, before: {
            let extent = extents[roots[$0]]!
            return (scalar ? extent.scalarStart : extent.docStart) <= position
        }) else { return nil }
        var key = roots[rootIndex]
        let extent = extents[key]!
        guard scalar ? position <= extent.scalarEnd : UInt64(position) < UInt64(extent.docStart) + UInt64(extent.docSize) else { return nil }
        while let index = scalar ? cellIndex(tableKey: key, containingScalar: position) : cellIndex(tableKey: key, containingDoc: position) {
            let nested = entries[key]!.record.cells[index].nestedTables.first { nested in
                guard let origin = origins[nested.tableKey], let child = entries[nested.tableKey] else { return false }
                if scalar {
                    guard let start = origin.scalar else { return false }
                    return start <= position && UInt64(position) <= UInt64(start) + UInt64(child.scalarSize)
                }
                return origin.doc <= position && UInt64(position) < UInt64(origin.doc) + UInt64(child.record.docSize)
            }
            guard let nested else { break }
            key = nested.tableKey
        }
        return key
    }

    func absoluteDocPos(tableKey: String, cellIndex: Int, relative: UInt32) -> UInt32? {
        guard let entry = entries[tableKey], entry.record.cells.indices.contains(cellIndex),
              relative <= entry.record.cells[cellIndex].docSize, let start = docStart(tableKey: tableKey, cellIndex: cellIndex) else { return nil }
        return UInt32(exactly: UInt64(start) + UInt64(relative))
    }

    func inputSegments(tableKey: String, cellIndex: Int) -> [TableCellPositionMap.Segment]? {
        guard let entry = entries[tableKey], entry.record.cells.indices.contains(cellIndex),
              let start = scalarStart(tableKey: tableKey, cellIndex: cellIndex) else { return nil }
        var segments: [TableCellPositionMap.Segment] = []
        let cell = entry.record.cells[cellIndex]
        for block in cell.inputBlocks {
            var collapsedOffset: Int64 = 0
            for nested in cell.nestedTables where nested.elementIndex < block.elementIndex {
                if let lower = nested.scalarStart, let upper = nested.scalarEnd {
                    collapsedOffset += Int64(upper - lower) - 1
                }
            }
            guard let local = UInt32(exactly: Int64(block.scalarStart) - collapsedOffset),
                  let end = UInt32(exactly: Int64(block.scalarEnd) - collapsedOffset + 1),
                  let global = UInt32(exactly: UInt64(start) + UInt64(block.scalarStart)) else { return nil }
            segments.append(.init(localScalarRange: local..<end, globalScalarStart: global))
        }
        return segments
    }

    private static func precedingIndex(_ count: Int, before: (Int) -> Bool) -> Int? {
        var low = 0
        var high = count
        while low < high {
            let middle = low + (high - low) / 2
            if before(middle) { low = middle + 1 } else { high = middle }
        }
        return low == 0 ? nil : low - 1
    }
}
