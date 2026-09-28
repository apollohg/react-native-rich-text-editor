import UIKit

struct PreparedViewerTableCell {
    let sourceIndex: Int
    let row: Int
    let column: Int
    let rowspan: Int
    let colspan: Int
    let contentOrigin: CGPoint
    let content: PreparedProseLayout
    let isHeader: Bool
    let attributesKey: String?

    var retainedBytes: Int { 96 + content.retainedBytes }
}

final class ViewerTableSurface {
    let identity: String
    let scrollIdentity: String
    let hostViewportWidth: CGFloat
    let style: TableStyle
    let direction: TableLayoutDirection
    let layout: TableLayoutResult
    let cells: [PreparedViewerTableCell]
    let sourceTable: TableSurfaceSource?
    let sourceAttributes: [String: [String: Any]]
    let syntheticRegions: [TableRenderSyntheticRegion]
    let columnEdgeHandleRows: [Int: Int]
    let preparationError: ProseViewerError?
    private let cellIndex: ViewerTableCellIndex

    var bounds: CGRect { CGRect(origin: .zero, size: layout.contentSize) }
    var retainedBytes: Int {
        256 + cells.reduce(0) { $0 + $1.retainedBytes }
            + (sourceTable?.cells.count ?? 0) * 16 + syntheticRegions.count * 64 + columnEdgeHandleRows.count * 16
            + (layout.columnWidths.count + layout.columnOffsets.count + layout.rowOffsets.count) * 16
            + layout.rectangles.count * 96 + layout.sourceOrder.count * 16
    }

    init(
        identity: String,
        scrollIdentity: String? = nil,
        record: TableGridRecord,
        viewportWidth: CGFloat,
        style: TableStyle,
        direction: TableLayoutDirection,
        displayScale: CGFloat = UIScreen.main.scale,
        themeDigest: String = "",
        fontEnvironmentRevision: Int = 0,
        textScale: CGFloat = 1,
        sourceTable: TableSurfaceSource? = nil,
        sourceAttributes: [String: [String: Any]] = [:],
        prepareCell: (TableGridCell, CGFloat) -> PreparedProseLayout
    ) {
        self.identity = identity
        self.scrollIdentity = scrollIdentity ?? identity
        self.hostViewportWidth = viewportWidth
        self.style = style
        self.direction = direction
        self.sourceTable = sourceTable
        self.sourceAttributes = sourceAttributes
        self.syntheticRegions = sourceTable?.syntheticRegions ?? []
        self.columnEdgeHandleRows = Dictionary(
            (sourceTable?.cells ?? []).map { (Int($0.column + $0.colspan) - 1, Int($0.row)) },
            uniquingKeysWith: min
        )
        let sourceCells = Dictionary(uniqueKeysWithValues: record.cells.map { ($0.sourceIndex, $0) })
        let measurementRecord = TableGridRecord(
            documentOwner: record.documentOwner,
            columns: record.columns,
            rows: record.rows,
            columnWidths: record.columnWidths,
            cells: record.cells.map {
                TableGridCell(sourceIndex: $0.sourceIndex, row: $0.row, column: $0.column,
                              rowspan: $0.rowspan, colspan: $0.colspan,
                              contentKey: "\($0.contentKey):\($0.sourceIndex)",
                              attachmentRevision: $0.attachmentRevision)
            },
            failure: record.failure,
            compatibilityDiagnostic: record.compatibilityDiagnostic
        )
        var prepared: [Int: PreparedProseLayout] = [:]
        var firstPreparationError: ProseViewerError?
        let canonicalScale = displayScale.isFinite && displayScale > 0 ? displayScale : 1
        let grid = TableGridLayout(displayScale: canonicalScale)
        let resolvedLayout = grid.layout(
            record: measurementRecord,
            viewportWidth: viewportWidth,
            style: style,
            direction: direction,
            themeDigest: themeDigest,
            fontEnvironmentRevision: fontEnvironmentRevision,
            textScale: textScale
        ) { measuredCell, width in
            guard let cell = sourceCells[measuredCell.sourceIndex] else { return nil }
            let content = prepareCell(cell, width)
            prepared[cell.sourceIndex] = content
            if let error = content.error, firstPreparationError == nil { firstPreparationError = error }
            return content.size.height
        }
        for cell in record.cells where prepared[cell.sourceIndex] == nil {
            guard let frame = resolvedLayout.rectangles[cell.sourceIndex] else { continue }
            let inner = max(0, frame.width - 2 * (style.cellPadding + style.borderWidth))
            let pixels = (inner * canonicalScale).rounded()
            guard pixels.isFinite, pixels >= 0, let widthPixels = Int(exactly: pixels) else { continue }
            let content = prepareCell(cell, CGFloat(widthPixels) / canonicalScale)
            if let error = content.error, firstPreparationError == nil { firstPreparationError = error }
            prepared[cell.sourceIndex] = content
        }
        layout = resolvedLayout
        preparationError = firstPreparationError
        let preparedCells = resolvedLayout.sourceOrder.compactMap { sourceIndex -> PreparedViewerTableCell? in
            guard let gridCell = sourceCells[sourceIndex], let content = prepared[sourceIndex] else {
                return nil
            }
            let sourceCell = sourceTable?.cells[sourceIndex]
            let inset = style.cellPadding + style.borderWidth
            return PreparedViewerTableCell(sourceIndex: sourceIndex,
                                           row: gridCell.row, column: gridCell.column,
                                           rowspan: gridCell.rowspan, colspan: gridCell.colspan,
                                           contentOrigin: CGPoint(x: inset, y: inset), content: content,
                                           isHeader: sourceCell?.header ?? false,
                                           attributesKey: sourceCell?.attrsKey)
        }
        cells = preparedCells
        cellIndex = ViewerTableCellIndex(cells: preparedCells, direction: direction)
    }

    init(
        identity: String,
        scrollIdentity: String? = nil,
        hostViewportWidth: CGFloat,
        style: TableStyle,
        direction: TableLayoutDirection,
        layout: TableLayoutResult,
        cells: [PreparedViewerTableCell],
        preparationError: ProseViewerError?
    ) {
        self.identity = identity
        self.scrollIdentity = scrollIdentity ?? identity
        self.hostViewportWidth = hostViewportWidth
        self.style = style
        self.direction = direction
        self.layout = layout
        self.cells = cells
        self.cellIndex = ViewerTableCellIndex(cells: cells, direction: direction)
        self.sourceTable = nil
        self.sourceAttributes = [:]
        self.syntheticRegions = []
        self.columnEdgeHandleRows = [:]
        self.preparationError = preparationError
    }

    func parentImageAttachments(offset: Int, tableOrigin: CGPoint) -> [ViewerImageAttachment] {
        var nextOrdinal = offset
        var attachments: [ViewerImageAttachment] = []
        func append(_ layout: PreparedProseLayout, at origin: CGPoint) {
            var attachmentsByID: [String: Int] = [:]
            for (index, attachment) in layout.imageAttachments.enumerated() {
                attachmentsByID[attachment.id] = index
            }
            var appended = Set<Int>()
            func appendAttachment(_ index: Int) {
                guard appended.insert(index).inserted else { return }
                let attachment = layout.imageAttachments[index]
                attachments.append(ViewerImageAttachment(
                    ordinal: nextOrdinal,
                    id: attachment.id,
                    source: attachment.source,
                    bounds: attachment.bounds.offsetBy(dx: origin.x, dy: origin.y),
                    declaredSize: attachment.declaredSize
                ))
                nextOrdinal += 1
            }
            for block in layout.blocks {
                if let attachment = block.imageAttachment, let index = attachmentsByID.removeValue(forKey: attachment.id) {
                    appendAttachment(index)
                }
                guard let nested = block.tableSurface else { continue }
                let tableBounds = block.tableBounds ?? block.bounds
                append(nested, at: CGPoint(x: origin.x + tableBounds.minX, y: origin.y + tableBounds.minY))
            }
            for index in layout.imageAttachments.indices { appendAttachment(index) }
        }
        func append(_ surface: ViewerTableSurface, at origin: CGPoint) {
            for cell in surface.cells {
                let frame = surface.frame(ofCell: cell)
                append(cell.content, at: CGPoint(x: origin.x + frame.minX + cell.contentOrigin.x,
                                                  y: origin.y + frame.minY + cell.contentOrigin.y))
            }
        }
        append(self, at: tableOrigin)
        return attachments
    }

    func visibleCells(in viewport: CGRect, horizontalOffset: CGFloat = 0) -> [PreparedViewerTableCell] {
        cells(intersecting: viewport.offsetBy(dx: horizontalOffset, dy: 0))
    }

    func cells(intersecting rect: CGRect) -> [PreparedViewerTableCell] {
        cellIndex.indexes(intersecting: rect, in: self).map { cells[$0] }
    }

    var hasAtoms: Bool { !cellIndex.atomCells.isEmpty }

    func presentationCells(intersecting window: CGRect) -> [PreparedViewerTableCell] {
        Set(cellIndex.indexes(intersecting: window, in: self)).union(cellIndex.atomCells).sorted().map { cells[$0] }
    }

    var nestedTableCells: [PreparedViewerTableCell] {
        cellIndex.nestedTableCells.map { cells[$0] }
    }

    func cell(sourceIndex: Int) -> PreparedViewerTableCell? {
        cellIndex.bySourceIndex[sourceIndex].map { cells[$0] }
    }

    func frame(ofCell cell: PreparedViewerTableCell) -> CGRect {
        let x = layout.columnOffsets[cell.column]
        let width = layout.columnOffsets[cell.column + cell.colspan] - x
        let y = layout.rowOffsets[cell.row]
        return CGRect(x: tablePhysicalX(logicalX: x, width: width, totalWidth: layout.contentSize.width,
                                        rtl: direction == .rightToLeft),
                      y: y, width: width, height: layout.rowOffsets[cell.row + cell.rowspan] - y)
    }

}

private struct ViewerTableCellIndex {
    private struct Band {
        let row: Int
        let cells: [Int]
    }

    private let bands: [Band]
    private let spanning: [Int]
    let nestedTableCells: [Int]
    let atomCells: [Int]
    let bySourceIndex: [Int: Int]

    init(cells: [PreparedViewerTableCell], direction: TableLayoutDirection) {
        spanning = cells.indices.filter { cells[$0].rowspan > 1 || cells[$0].colspan > 1 }
        let slotted = cells.indices.filter { cells[$0].rowspan == 1 && cells[$0].colspan == 1 }
        let rows = Dictionary(grouping: slotted, by: { cells[$0].row })
        bands = rows.keys.sorted().map { row in
            Band(row: row, cells: rows[row, default: []].sorted {
                direction == .rightToLeft ? cells[$0].column > cells[$1].column : cells[$0].column < cells[$1].column
            })
        }
        nestedTableCells = cells.indices.filter { index in
            cells[index].content.blocks.contains { $0.tableSurface != nil }
        }
        atomCells = cells.indices.filter { index in
            cells[index].content.blocks.contains { $0.atomSlot != nil || $0.tableSurface?.hasAtoms == true }
        }
        bySourceIndex = Dictionary(cells.indices.map { (cells[$0].sourceIndex, $0) }, uniquingKeysWith: min)
    }

    func indexes(intersecting rect: CGRect, in surface: ViewerTableSurface) -> [Int] {
        guard !rect.isNull, !rect.isEmpty else { return [] }
        func frame(_ index: Int) -> CGRect { surface.frame(ofCell: surface.cells[index]) }
        var result = spanning.filter { frame($0).intersects(rect) }
        let firstBand = Self.partition(bands.count) { surface.layout.rowOffsets[bands[$0].row + 1] > rect.minY }
        for band in bands[firstBand...] {
            guard surface.layout.rowOffsets[band.row] < rect.maxY else { break }
            let firstCell = Self.partition(band.cells.count) { frame(band.cells[$0]).maxX > rect.minX }
            for index in band.cells[firstCell...] {
                let cellFrame = frame(index)
                guard cellFrame.minX < rect.maxX else { break }
                if cellFrame.intersects(rect) { result.append(index) }
            }
        }
        return result.sorted()
    }

    private static func partition(_ count: Int, isAfter: (Int) -> Bool) -> Int {
        var low = 0
        var high = count
        while low < high {
            let middle = (low + high) / 2
            if isAfter(middle) { high = middle } else { low = middle + 1 }
        }
        return low
    }
}

final class ViewerTablePresentationOwner {
    private static let mapRetainedBytes = 48
    private static let entryRetainedBytes = 80
    private struct Position {
        let column: Int
        let withinColumn: CGFloat
    }
    private var positions: [String: Position] = [:]

    var retainedBytes: Int {
        guard !positions.isEmpty else { return 0 }
        return positions.reduce(Self.mapRetainedBytes) { total, entry in
            let keyBytes = entry.key.utf16.count.multipliedReportingOverflow(by: 2)
            let entryBytes = keyBytes.overflow ? Int.max : Self.saturatingAdd(keyBytes.partialValue, Self.entryRetainedBytes)
            return Self.saturatingAdd(total, entryBytes)
        }
    }

    func logicalOffset(for surface: ViewerTableSurface) -> CGFloat {
        guard let position = positions[surface.scrollIdentity] else { return 0 }
        let leading = surface.layout.columnWidths.prefix(position.column).reduce(CGFloat.zero, +)
        return clamp(leading + position.withinColumn, for: surface)
    }

    func setLogicalOffset(_ offset: CGFloat, for surface: ViewerTableSurface) {
        var remaining = clamp(offset, for: surface)
        for (column, width) in surface.layout.columnWidths.enumerated() {
            if remaining < width || column == surface.layout.columnWidths.count - 1 {
                positions[surface.scrollIdentity] = Position(column: column, withinColumn: remaining)
                return
            }
            remaining -= width
        }
    }

    func retain(surfaces: [ViewerTableSurface]) {
        let live = Set(surfaces.map(\.scrollIdentity))
        positions = positions.filter { live.contains($0.key) }
    }

    func physicalOffset(for surface: ViewerTableSurface) -> CGFloat {
        let logical = logicalOffset(for: surface)
        let maximum = maximumOffset(for: surface)
        return surface.direction == .rightToLeft ? maximum - logical : logical
    }

    private func clamp(_ offset: CGFloat, for surface: ViewerTableSurface) -> CGFloat {
        guard offset.isFinite else { return 0 }
        return min(max(0, offset), maximumOffset(for: surface))
    }

    private func maximumOffset(for surface: ViewerTableSurface) -> CGFloat {
        let value = surface.bounds.width - surface.hostViewportWidth
        return value.isFinite ? max(0, value) : 0
    }

    private static func saturatingAdd(_ left: Int, _ right: Int) -> Int {
        guard left <= Int.max - right else { return Int.max }
        return left + right
    }
}

enum ViewerTablePresentationViewport {
    case unknown
    case known(CGRect)

    var window: CGRect? {
        guard case let .known(rect) = self, rect.width > 0, rect.height > 0,
              rect.width.isFinite, rect.height.isFinite
        else { return nil }
        return rect.insetBy(dx: -rect.width, dy: -rect.height)
    }
}

struct ViewerTablePresentedBlock {
    let layout: PreparedProseLayout
    let block: PreparedProseBlock
    let origin: CGPoint
    let clip: CGRect
}

struct ViewerTablePresentedLayout {
    let layout: PreparedProseLayout
    let origin: CGPoint
    let clip: CGRect
}

struct ViewerTablePresentedCell {
    let surface: ViewerTableSurface
    let cell: PreparedViewerTableCell
    let sourceIndex: Int
    let content: PreparedProseLayout
    let bounds: CGRect
    let contentBounds: CGRect
    let clip: CGRect
}

struct ViewerTablePresentedTable {
    let surface: ViewerTableSurface
    let bounds: CGRect
    let clip: CGRect
    let parentScrollIdentity: String?
}

struct ViewerTablePresentedImage {
    let attachment: ViewerImageAttachment
    let sourceIdentity: String
    let bounds: CGRect
    let clip: CGRect
    let layout: PreparedProseLayout
    let block: PreparedProseBlock?
}

struct ViewerTablePresentedAtom {
    let atom: PreparedProseAtomSlot
    let sourceIdentity: String
    let bounds: CGRect
    let clip: CGRect
    let layout: PreparedProseLayout
    let block: PreparedProseBlock
}

struct ViewerTablePresentedInteraction {
    let interaction: PreparedProseInteraction
    let sourceIdentity: String
    let rects: [CGRect]
    let clip: CGRect
    let layout: PreparedProseLayout
}

struct ViewerTablePresentedAccessibilityNode {
    let node: PreparedProseAccessibilityNode
    let sourceIdentity: String
    let interactionSourceIdentity: String?
    let rects: [CGRect]
    let clip: CGRect
    let layout: PreparedProseLayout
}

struct ViewerTablePresentationSnapshot {
    let layouts: [ViewerTablePresentedLayout]
    let blocks: [ViewerTablePresentedBlock]
    let tables: [ViewerTablePresentedTable]
    let cells: [ViewerTablePresentedCell]
    let mountedCells: [ViewerTablePresentedCell]
    let images: [ViewerTablePresentedImage]
    let atoms: [ViewerTablePresentedAtom]
    let interactions: [ViewerTablePresentedInteraction]
    let accessibilityNodes: [ViewerTablePresentedAccessibilityNode]
}

enum ViewerTablePresentation {
    static func present(
        _ cell: PreparedViewerTableCell,
        in table: ViewerTablePresentedTable,
        owner: ViewerTablePresentationOwner
    ) -> ViewerTablePresentedCell {
        let contentOrigin = CGPoint(x: table.bounds.minX - owner.physicalOffset(for: table.surface), y: table.bounds.minY)
        return present(cell, of: table.surface, contentOrigin: contentOrigin, clip: table.clip)
    }

    private static func present(
        _ cell: PreparedViewerTableCell,
        of surface: ViewerTableSurface,
        contentOrigin: CGPoint,
        clip: CGRect
    ) -> ViewerTablePresentedCell {
        let cellBounds = surface.frame(ofCell: cell).offsetBy(dx: contentOrigin.x, dy: contentOrigin.y)
        let childOrigin = CGPoint(x: cellBounds.minX + cell.contentOrigin.x, y: cellBounds.minY + cell.contentOrigin.y)
        return ViewerTablePresentedCell(
            surface: surface,
            cell: cell,
            sourceIndex: cell.sourceIndex,
            content: cell.content,
            bounds: cellBounds,
            contentBounds: CGRect(origin: childOrigin, size: cell.content.size),
            clip: clip
        )
    }

    static func surfaces(in root: PreparedProseLayout) -> [ViewerTableSurface] {
        var surfaces: [ViewerTableSurface] = []
        func visit(_ layout: PreparedProseLayout) {
            for surface in layout.blocks.compactMap(\.tableSurface) {
                surfaces.append(surface)
                surface.nestedTableCells.forEach { visit($0.content) }
            }
        }
        visit(root)
        return surfaces
    }

    private static func presentTable(
        _ surface: ViewerTableSurface,
        tableBounds: CGRect,
        origin: CGPoint,
        clip: CGRect,
        parentScrollIdentity: String?
    ) -> ViewerTablePresentedTable {
        let hostOrigin = CGPoint(x: origin.x + tableBounds.minX, y: origin.y + tableBounds.minY)
        let hostWidth = min(surface.hostViewportWidth, surface.bounds.width)
        let hostBounds = CGRect(origin: hostOrigin, size: CGSize(width: hostWidth, height: surface.bounds.height))
        return ViewerTablePresentedTable(surface: surface, bounds: hostBounds, clip: clip.intersection(hostBounds),
                                         parentScrollIdentity: parentScrollIdentity)
    }

    static func rootTables(in root: PreparedProseLayout) -> [ViewerTablePresentedTable] {
        root.blocks.compactMap { block in
            guard let surface = block.tableSurface, let tableBounds = block.tableBounds else { return nil }
            return presentTable(surface, tableBounds: tableBounds, origin: .zero, clip: .infinite,
                                parentScrollIdentity: nil)
        }
    }

    static func contentAccessibilityNodes(
        of cell: ViewerTablePresentedCell,
        owner: ViewerTablePresentationOwner
    ) -> [ViewerTablePresentedAccessibilityNode] {
        project(layout: cell.content, owner: owner, window: nil, origin: cell.contentBounds.origin,
                clip: cell.clip.intersection(cell.contentBounds),
                parentScrollIdentity: cell.surface.scrollIdentity).accessibilityNodes
    }

    static func project(
        layout root: PreparedProseLayout,
        owner: ViewerTablePresentationOwner,
        viewport: ViewerTablePresentationViewport
    ) -> ViewerTablePresentationSnapshot {
        let window: CGRect?
        switch viewport {
        case .unknown: window = nil
        case .known: window = viewport.window ?? .null
        }
        return project(layout: root, owner: owner, window: window, origin: .zero, clip: .infinite,
                       parentScrollIdentity: nil)
    }

    private static func project(
        layout root: PreparedProseLayout,
        owner: ViewerTablePresentationOwner,
        window: CGRect?,
        origin rootOrigin: CGPoint,
        clip rootClip: CGRect,
        parentScrollIdentity rootParent: String?
    ) -> ViewerTablePresentationSnapshot {
        var layouts: [ViewerTablePresentedLayout] = []
        var blocks: [ViewerTablePresentedBlock] = []
        var tables: [ViewerTablePresentedTable] = []
        var cells: [ViewerTablePresentedCell] = []
        var images: [ViewerTablePresentedImage] = []
        var atoms: [ViewerTablePresentedAtom] = []
        var interactions: [ViewerTablePresentedInteraction] = []
        var accessibilityNodes: [ViewerTablePresentedAccessibilityNode] = []
        let canonicalAttachments = Dictionary(uniqueKeysWithValues: root.imageAttachments.map { ($0.id, $0) })
        var emittedImages = Set<String>()

        func transformed(_ rect: CGRect, by origin: CGPoint) -> CGRect {
            rect.offsetBy(dx: origin.x, dy: origin.y)
        }
        func appendLayout(_ layout: PreparedProseLayout, origin: CGPoint, clip: CGRect,
                          parentScrollIdentity: String?) {
            layouts.append(ViewerTablePresentedLayout(layout: layout, origin: origin, clip: clip))
            var emittedInteractions = Set<Int>()
            var emittedAccessibility = Set<Int>()
            let interactionsByBlock = Dictionary(grouping: layout.interactions.indices.compactMap { index in
                layout.interactions[index].sourceBlockIndex.map { ($0, index) }
            }, by: { $0.0 })
            let accessibilityByBlock = Dictionary(grouping: layout.accessibilityNodes.indices.compactMap { index in
                layout.accessibilityNodes[index].sourceBlockIndex.map { ($0, index) }
            }, by: { $0.0 })

            func appendInteraction(_ index: Int) {
                guard layout.interactions.indices.contains(index), emittedInteractions.insert(index).inserted else { return }
                let interaction = layout.interactions[index]
                interactions.append(ViewerTablePresentedInteraction(
                    interaction: interaction,
                    sourceIdentity: "\(layout.key.semanticKey):interaction:\(index)",
                    rects: interaction.rects.map { transformed($0, by: origin) },
                    clip: clip,
                    layout: layout
                ))
            }

            func appendAccessibility(_ index: Int) {
                guard layout.accessibilityNodes.indices.contains(index), emittedAccessibility.insert(index).inserted else { return }
                let node = layout.accessibilityNodes[index]
                let interactionIdentity = node.interactionIndex.map { "\(layout.key.semanticKey):interaction:\($0)" }
                accessibilityNodes.append(ViewerTablePresentedAccessibilityNode(
                    node: node,
                    sourceIdentity: "\(layout.key.semanticKey):accessibility:\(index)",
                    interactionSourceIdentity: interactionIdentity,
                    rects: node.rects.map { transformed($0, by: origin) },
                    clip: clip,
                    layout: layout
                ))
            }

            for (blockIndex, block) in layout.blocks.enumerated() {
                blocks.append(ViewerTablePresentedBlock(layout: layout, block: block, origin: origin, clip: clip))
                if let attachment = block.imageAttachment, emittedImages.insert(attachment.id).inserted {
                    images.append(ViewerTablePresentedImage(
                        attachment: canonicalAttachments[attachment.id] ?? attachment,
                        sourceIdentity: "\(layout.key.semanticKey):image:\(attachment.id)",
                        bounds: transformed(attachment.bounds, by: origin),
                        clip: clip,
                        layout: layout,
                        block: block
                    ))
                }
                if let atom = block.atomSlot {
                    atoms.append(ViewerTablePresentedAtom(
                        atom: atom,
                        sourceIdentity: "\(layout.key.semanticKey):atom:\(atom.nodeType):\(atom.docPos)",
                        bounds: transformed(atom.bounds, by: origin),
                        clip: clip,
                        layout: layout,
                        block: block
                    ))
                }
                for (_, index) in interactionsByBlock[blockIndex] ?? [] { appendInteraction(index) }
                for (_, index) in accessibilityByBlock[blockIndex] ?? [] { appendAccessibility(index) }
                guard let surface = block.tableSurface, let tableBounds = block.tableBounds else { continue }
                let table = presentTable(surface, tableBounds: tableBounds, origin: origin, clip: clip,
                                         parentScrollIdentity: parentScrollIdentity)
                tables.append(table)
                let hostClip = table.clip
                let contentOrigin = CGPoint(x: table.bounds.minX - owner.physicalOffset(for: surface), y: table.bounds.minY)
                let windowCells = window.map {
                    surface.presentationCells(intersecting: $0.offsetBy(dx: -contentOrigin.x, dy: -contentOrigin.y))
                } ?? surface.cells
                for cell in windowCells {
                    let presented = present(cell, of: surface, contentOrigin: contentOrigin, clip: hostClip)
                    cells.append(presented)
                    appendLayout(cell.content, origin: presented.contentBounds.origin,
                                 clip: hostClip.intersection(presented.contentBounds),
                                 parentScrollIdentity: surface.scrollIdentity)
                }
            }
            for attachment in layout.imageAttachments where emittedImages.insert(attachment.id).inserted {
                images.append(ViewerTablePresentedImage(
                    attachment: canonicalAttachments[attachment.id] ?? attachment,
                    sourceIdentity: "\(layout.key.semanticKey):image:\(attachment.id)",
                    bounds: transformed(attachment.bounds, by: origin),
                    clip: clip,
                    layout: layout,
                    block: nil
                ))
            }
            for index in layout.interactions.indices { appendInteraction(index) }
            for index in layout.accessibilityNodes.indices { appendAccessibility(index) }
        }

        appendLayout(root, origin: rootOrigin, clip: rootClip, parentScrollIdentity: rootParent)
        let mountedCells = window.map { window in cells.filter { $0.bounds.intersects(window) } } ?? cells
        return ViewerTablePresentationSnapshot(
            layouts: layouts,
            blocks: blocks,
            tables: tables,
            cells: cells,
            mountedCells: mountedCells,
            images: images,
            atoms: atoms,
            interactions: interactions,
            accessibilityNodes: accessibilityNodes
        )
    }
}
