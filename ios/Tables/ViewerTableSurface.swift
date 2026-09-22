import UIKit

/// Immutable table preparation retained by one parent prose artifact. Scrolling
/// state intentionally belongs to the mounted drawing view, never this value.
struct PreparedViewerTableCell {
    let sourcePosition: Int
    let frame: CGRect
    let contentOrigin: CGPoint
    let content: PreparedProseLayout
    let sourceCellIndex: Int?
    let isHeader: Bool
    let attributesKey: String?

    var retainedBytes: Int { 96 + content.retainedBytes }
}

/// A rendering-only table. It owns grid geometry and rich cell artifacts, but
/// has no cache, lease, editor session, or mutable viewport state.
final class ViewerTableSurface {
    let identity: String
    let hostViewportWidth: CGFloat
    let style: TableStyle
    let direction: TableLayoutDirection
    let layout: TableLayoutResult
    let cells: [PreparedViewerTableCell]
    let sourceTable: FfiViewerTable?
    let sourceAttributes: [String: [String: Any]]
    let syntheticRegions: [TableRenderSyntheticRegion]
    let preparationError: ProseViewerError?

    var bounds: CGRect { CGRect(origin: .zero, size: layout.contentSize) }
    var retainedBytes: Int {
        256 + cells.reduce(0) { $0 + $1.retainedBytes }
            + (sourceTable?.cells.count ?? 0) * 16 + syntheticRegions.count * 64
            + layout.columnWidths.count * 16 + layout.rowOffsets.count * 16
            + layout.rectangles.count * 96 + layout.sourceOrder.count * 16
    }

    init(
        identity: String,
        record: TableGridRecord,
        viewportWidth: CGFloat,
        style: TableStyle,
        direction: TableLayoutDirection,
        displayScale: CGFloat = UIScreen.main.scale,
        themeDigest: String = "",
        fontEnvironmentRevision: Int = 0,
        textScale: CGFloat = 1,
        sourceTable: FfiViewerTable? = nil,
        sourceAttributes: [String: [String: Any]] = [:],
        prepareCell: (TableGridCell, CGFloat) -> PreparedProseLayout
    ) {
        self.identity = identity
        self.hostViewportWidth = viewportWidth
        self.style = style
        self.direction = direction
        self.sourceTable = sourceTable
        self.sourceAttributes = sourceAttributes
        self.syntheticRegions = sourceTable?.syntheticRegions ?? []
        let sourceCellIndexes = Dictionary(uniqueKeysWithValues: (sourceTable?.cells ?? []).enumerated().map { (Int($0.element.sourcePos), $0.offset) })
        let sourceCells = Dictionary(uniqueKeysWithValues: record.cells.map { ($0.sourcePosition, $0) })
        let measurementRecord = TableGridRecord(
            documentOwner: record.documentOwner,
            columns: record.columns,
            rows: record.rows,
            columnWidths: record.columnWidths,
            cells: record.cells.map {
                TableGridCell(sourcePosition: $0.sourcePosition, row: $0.row, column: $0.column,
                              rowspan: $0.rowspan, colspan: $0.colspan,
                              contentKey: "\($0.contentKey):\($0.sourcePosition)",
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
            guard let cell = sourceCells[measuredCell.sourcePosition] else { return nil }
            let content = prepareCell(cell, width)
            prepared[cell.sourcePosition] = content
            if let error = content.error, firstPreparationError == nil { firstPreparationError = error }
            return content.size.height
        }
        for cell in record.cells where prepared[cell.sourcePosition] == nil {
            guard let frame = resolvedLayout.rectangles[cell.sourcePosition] else { continue }
            let inner = max(0, frame.width - 2 * (style.cellPadding + style.borderWidth))
            let pixels = (inner * canonicalScale).rounded()
            guard pixels.isFinite, pixels >= 0, let widthPixels = Int(exactly: pixels) else { continue }
            let content = prepareCell(cell, CGFloat(widthPixels) / canonicalScale)
            if let error = content.error, firstPreparationError == nil { firstPreparationError = error }
            prepared[cell.sourcePosition] = content
        }
        layout = resolvedLayout
        preparationError = firstPreparationError
        cells = resolvedLayout.sourceOrder.compactMap { sourcePosition in
            guard let frame = resolvedLayout.rectangles[sourcePosition], let content = prepared[sourcePosition] else {
                return nil
            }
            let sourceCellIndex = sourceCellIndexes[sourcePosition]
            let sourceCell = sourceCellIndex.flatMap { sourceTable?.cells[$0] }
            let inset = style.cellPadding + style.borderWidth
            return PreparedViewerTableCell(sourcePosition: sourcePosition, frame: frame,
                                           contentOrigin: CGPoint(x: inset, y: inset), content: content,
                                           sourceCellIndex: sourceCellIndex, isHeader: sourceCell?.header ?? false,
                                           attributesKey: sourceCell?.attrsKey)
        }
    }

    init(
        identity: String,
        hostViewportWidth: CGFloat,
        style: TableStyle,
        direction: TableLayoutDirection,
        layout: TableLayoutResult,
        cells: [PreparedViewerTableCell],
        preparationError: ProseViewerError?
    ) {
        self.identity = identity
        self.hostViewportWidth = hostViewportWidth
        self.style = style
        self.direction = direction
        self.layout = layout
        self.cells = cells
        self.sourceTable = nil
        self.sourceAttributes = [:]
        self.syntheticRegions = []
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
                append(cell.content, at: CGPoint(x: origin.x + cell.frame.minX + cell.contentOrigin.x,
                                                  y: origin.y + cell.frame.minY + cell.contentOrigin.y))
            }
        }
        append(self, at: tableOrigin)
        return attachments
    }

    func visibleCells(in viewport: CGRect, horizontalOffset: CGFloat = 0) -> [PreparedViewerTableCell] {
        let visible = viewport.offsetBy(dx: horizontalOffset, dy: 0)
        return cells.filter { $0.frame.intersects(visible) }
    }
}

/// Mutable mounted state deliberately kept outside the immutable prepared surface.
final class ViewerTablePresentationOwner {
    private static let mapRetainedBytes = 48
    private static let entryRetainedBytes = 64
    private var logicalOffsets: [String: CGFloat] = [:]

    var retainedBytes: Int {
        guard !logicalOffsets.isEmpty else { return 0 }
        return logicalOffsets.reduce(Self.mapRetainedBytes) { total, entry in
            let keyBytes = entry.key.utf16.count.multipliedReportingOverflow(by: 2)
            let entryBytes = keyBytes.overflow ? Int.max : Self.saturatingAdd(keyBytes.partialValue, Self.entryRetainedBytes)
            return Self.saturatingAdd(total, entryBytes)
        }
    }

    func logicalOffset(for surface: ViewerTableSurface) -> CGFloat {
        logicalOffsets[surface.identity] ?? 0
    }

    func setLogicalOffset(_ offset: CGFloat, for surface: ViewerTableSurface) {
        logicalOffsets[surface.identity] = clamp(offset, for: surface)
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
}

struct ViewerTablePresentedBlock {
    let layout: PreparedProseLayout
    let block: PreparedProseBlock
    let origin: CGPoint
    let clip: CGRect
}

/// One entry per immutable layout reached by the mounted traversal. Keeping
/// decorations here prevents consumers from rebuilding table transforms.
struct ViewerTablePresentedLayout {
    let layout: PreparedProseLayout
    let origin: CGPoint
    let clip: CGRect
}

struct ViewerTablePresentedCell {
    let surface: ViewerTableSurface
    let cell: PreparedViewerTableCell
    let sourcePosition: Int
    let content: PreparedProseLayout
    let bounds: CGRect
    let contentBounds: CGRect
    let clip: CGRect
}

struct ViewerTablePresentedImage {
    /// The root attachment remains the publication owner; this is only a mounted geometry projection.
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
    let cells: [ViewerTablePresentedCell]
    let mountedCells: [ViewerTablePresentedCell]
    let images: [ViewerTablePresentedImage]
    let atoms: [ViewerTablePresentedAtom]
    let interactions: [ViewerTablePresentedInteraction]
    let accessibilityNodes: [ViewerTablePresentedAccessibilityNode]
}

/// One recursive coordinate seam for drawing, rich hits, media, atoms, and accessibility.
enum ViewerTablePresentation {
    static func project(
        layout root: PreparedProseLayout,
        owner: ViewerTablePresentationOwner,
        viewport: ViewerTablePresentationViewport
    ) -> ViewerTablePresentationSnapshot {
        var layouts: [ViewerTablePresentedLayout] = []
        var blocks: [ViewerTablePresentedBlock] = []
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

        func appendLayout(_ layout: PreparedProseLayout, origin: CGPoint, clip: CGRect) {
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
                let hostOrigin = CGPoint(x: origin.x + tableBounds.minX, y: origin.y + tableBounds.minY)
                let hostWidth = min(surface.hostViewportWidth, surface.bounds.width)
                let hostClip = clip.intersection(CGRect(origin: hostOrigin, size: CGSize(width: hostWidth, height: surface.bounds.height)))
                let contentOrigin = CGPoint(x: hostOrigin.x - owner.physicalOffset(for: surface), y: hostOrigin.y)
                for cell in surface.cells {
                    let cellBounds = transformed(cell.frame, by: contentOrigin)
                    let childOrigin = CGPoint(x: cellBounds.minX + cell.contentOrigin.x, y: cellBounds.minY + cell.contentOrigin.y)
                    let contentBounds = CGRect(origin: childOrigin, size: cell.content.size)
                    let presented = ViewerTablePresentedCell(
                        surface: surface,
                        cell: cell,
                        sourcePosition: cell.sourcePosition,
                        content: cell.content,
                        bounds: cellBounds,
                        contentBounds: contentBounds,
                        clip: hostClip
                    )
                    cells.append(presented)
                    appendLayout(cell.content, origin: childOrigin, clip: hostClip.intersection(contentBounds))
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

        appendLayout(root, origin: .zero, clip: .infinite)
        let mountedCells: [ViewerTablePresentedCell]
        switch viewport {
        case .unknown:
            mountedCells = cells
        case let .known(rect) where rect.width > 0 && rect.height > 0 && rect.width.isFinite && rect.height.isFinite:
            let candidate = rect.insetBy(dx: -rect.width, dy: -rect.height)
            mountedCells = cells.filter { $0.bounds.intersects(candidate) }
        case .known:
            mountedCells = []
        }
        return ViewerTablePresentationSnapshot(
            layouts: layouts,
            blocks: blocks,
            cells: cells,
            mountedCells: mountedCells,
            images: images,
            atoms: atoms,
            interactions: interactions,
            accessibilityNodes: accessibilityNodes
        )
    }
}
