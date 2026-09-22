import Foundation
import UIKit

/// A source-neutral cell shape is retained only through a live parent layout.
struct PreparedCellShapeKey: Hashable {
    let contentKey: String
    let widthPixels: Int
    let scaleBits: UInt64
    let styleDigest: String
    let atomGeometryDigest: String
    let imageGeometryDigest: String
    let format = 1
}

final class PreparedCellShape {
    let key: PreparedCellShapeKey
    let localLayout: PreparedProseLayout

    init(key: PreparedCellShapeKey, localLayout: PreparedProseLayout) {
        self.key = key
        self.localLayout = localLayout
    }

    var retainedBytes: Int { localLayout.retainedBytes }

    /// The neutral layout owns a second wrapper/table/cell graph. Charging the
    /// full local-layout estimate is intentionally conservative because Core
    /// Text leaves are shared while the wrappers are not.
    var catalogRetainedBytes: Int {
        localLayout.retainedBytes + 256 + key.contentKey.utf8.count
            + key.styleDigest.utf8.count + key.atomGeometryDigest.utf8.count
            + key.imageGeometryDigest.utf8.count
    }
}

/// Per-parent-build access to the parent cache's shape catalog.
final class PreparedCellShapeBuildContext {
    private let catalog: PreparedCellShapeCatalog
    private var resolved: [PreparedCellShapeKey: PreparedCellShape] = [:]
    private var pins: [ObjectIdentifier: PreparedCellShape] = [:]
    private var closed = false

    fileprivate init(catalog: PreparedCellShapeCatalog) {
        self.catalog = catalog
    }

    func resolve(
        _ key: PreparedCellShapeKey,
        build: () throws -> PreparedProseLayout,
        bind: (PreparedCellShape) -> PreparedProseLayout?
    ) throws -> PreparedProseLayout {
        if let shape = resolved[key] {
            if let bound = bind(shape) { return bound.withCellShape(shape) }
            return try build()
        }
        if let shape = catalog.acquireForBuild(key) {
            rememberPinned(shape)
            resolved[key] = shape
            if let bound = bind(shape) {
                return bound.withCellShape(shape)
            }
            return try build()
        }
        let fresh = try build()
        let candidate = PreparedCellShape(key: key, localLayout: fresh.sourceNeutralized())
        let shape = catalog.stageForBuild(candidate)
        rememberPinned(shape)
        resolved[key] = shape
        if shape !== candidate, let bound = bind(shape) {
            return bound.withCellShape(shape)
        }
        return fresh.withCellShape(shape)
    }

    func close() {
        guard !closed else { return }
        closed = true
        catalog.releaseBuildPins(Array(pins.values))
        pins.removeAll()
        resolved.removeAll()
    }

    private func rememberPinned(_ shape: PreparedCellShape) {
        let identifier = ObjectIdentifier(shape)
        guard pins[identifier] == nil else { return }
        pins[identifier] = shape
    }
}

/// Private index owned by `PreparedProseLayoutCache`; it never publishes a layout.
final class PreparedCellShapeCatalog {
    private let lock = NSLock()
    private var entries: [PreparedCellShapeKey: PreparedCellShape] = [:]
    private var buildPins: [ObjectIdentifier: Int] = [:]
    private var owners: [ObjectIdentifier: [PreparedCellShapeKey: PreparedCellShape]] = [:]
    private var ownerCounts: [ObjectIdentifier: Int] = [:]

    func newBuildContext() -> PreparedCellShapeBuildContext {
        PreparedCellShapeBuildContext(catalog: self)
    }

    func acquireForBuild(_ key: PreparedCellShapeKey) -> PreparedCellShape? {
        lock.lock()
        defer { lock.unlock() }
        guard let shape = entries[key] else { return nil }
        buildPins[ObjectIdentifier(shape), default: 0] += 1
        return shape
    }

    func stageForBuild(_ shape: PreparedCellShape) -> PreparedCellShape {
        lock.lock()
        let retained = entries[shape.key] ?? shape
        if entries[shape.key] == nil { entries[shape.key] = shape }
        buildPins[ObjectIdentifier(retained), default: 0] += 1
        lock.unlock()
        return retained
    }

    func releaseBuildPins(_ shapes: [PreparedCellShape]) {
        lock.lock()
        for shape in shapes {
            let identifier = ObjectIdentifier(shape)
            let remaining = max(0, (buildPins[identifier] ?? 1) - 1)
            if remaining == 0 { buildPins.removeValue(forKey: identifier) } else { buildPins[identifier] = remaining }
        }
        pruneLocked()
        lock.unlock()
    }

    func retainParent(_ layout: PreparedProseLayout) {
        lock.lock()
        let identifier = ObjectIdentifier(layout)
        ownerCounts[identifier, default: 0] += 1
        if owners[identifier] == nil {
            var shapes: [PreparedCellShapeKey: PreparedCellShape] = [:]
            collectShapes(in: layout, into: &shapes)
            owners[identifier] = shapes
            for (key, shape) in shapes where entries[key] == nil { entries[key] = shape }
        }
        lock.unlock()
    }

    func releaseParent(_ layout: PreparedProseLayout) {
        lock.lock()
        let identifier = ObjectIdentifier(layout)
        let remaining = max(0, (ownerCounts[identifier] ?? 1) - 1)
        if remaining == 0 {
            ownerCounts.removeValue(forKey: identifier)
            owners.removeValue(forKey: identifier)
        } else {
            ownerCounts[identifier] = remaining
        }
        pruneLocked()
        lock.unlock()
    }

    var countForTesting: Int {
        lock.lock(); defer { lock.unlock() }
        return entries.count
    }

    var retainedBytesForTesting: Int {
        lock.lock(); defer { lock.unlock() }
        return entries.values.reduce(0) { $0 + $1.catalogRetainedBytes }
    }

    private func pruneLocked() {
        let owned = Set(owners.values.flatMap { $0.keys })
        entries = entries.filter { key, shape in
            owned.contains(key) || buildPins[ObjectIdentifier(shape), default: 0] > 0
        }
    }

    private func collectShapes(
        in layout: PreparedProseLayout,
        into destination: inout [PreparedCellShapeKey: PreparedCellShape]
    ) {
        if let shape = layout.cellShape { destination[shape.key] = shape }
        for block in layout.blocks {
            guard let table = block.tableSurface else { continue }
            for cell in table.cells { collectShapes(in: cell.content, into: &destination) }
        }
    }
}

private extension PreparedProseLayout {
    func sourceNeutralized() -> PreparedProseLayout {
        let neutralBlocks = blocks.map { block -> PreparedProseBlock in
            let neutralTable = block.tableSurface.map { $0.sourceNeutralized() }
            let neutralAtom = block.atomSlot.map {
                PreparedProseAtomSlot(nodeType: $0.nodeType, docPos: 0, attrsJSON: "", bounds: $0.bounds)
            }
            let neutralImage = block.imageAttachment.map {
                ViewerImageAttachment(ordinal: $0.ordinal, id: "", source: "", bounds: $0.bounds, declaredSize: $0.declaredSize)
            }
            return PreparedProseBlock(
                fragments: block.fragments,
                bounds: block.bounds,
                atomSlot: neutralAtom,
                imageAttachment: neutralImage,
                tableSurface: neutralTable,
                tableBounds: block.tableBounds
            )
        }
        return PreparedProseLayout(
            key: ProseLayoutKey(
                semanticKey: "cell-shape",
                widthPixels: key.widthPixels,
                themeDigest: key.themeDigest,
                nativeFontRevision: key.nativeFontRevision,
                fontEnvironmentRevision: key.fontEnvironmentRevision,
                displayScale: CGFloat(Double(bitPattern: key.displayScaleBits)),
                attachmentRevision: 0,
                generationIdentity: "cell-shape",
                semanticGenerationIdentity: "cell-shape"
            ),
            size: size,
            blocks: neutralBlocks,
            interactions: interactions.map {
                PreparedProseInteraction(kind: $0.kind, rects: $0.rects, href: $0.href, visibleText: $0.visibleText, docPos: nil, label: $0.label, attrsJSON: nil, sourceBlockIndex: $0.sourceBlockIndex)
            },
            accessibilityNodes: accessibilityNodes,
            imageAttachments: imageAttachments.map {
                ViewerImageAttachment(ordinal: $0.ordinal, id: "", source: "", bounds: $0.bounds, declaredSize: $0.declaredSize)
            },
            retainedBytes: retainedBytes,
            decorations: decorations,
            highlightingRequest: nil,
            highlightingResolved: false
        )
    }
}

private extension ViewerTableSurface {
    func sourceNeutralized() -> ViewerTableSurface {
        let localSourcePositions = Dictionary(uniqueKeysWithValues: cells.enumerated().map { ($0.element.sourcePosition, $0.offset) })
        let localLayout = TableLayoutResult(
            columnWidths: layout.columnWidths,
            rowOffsets: layout.rowOffsets,
            rectangles: Dictionary(uniqueKeysWithValues: layout.rectangles.compactMap { sourcePosition, rect in
                localSourcePositions[sourcePosition].map { ($0, rect) }
            }),
            sourceOrder: layout.sourceOrder.compactMap { localSourcePositions[$0] },
            contentSize: layout.contentSize,
            failure: layout.failure,
            compatibilityDiagnostic: layout.compatibilityDiagnostic
        )
        return ViewerTableSurface(
            identity: "cell-table",
            hostViewportWidth: hostViewportWidth,
            style: style,
            direction: direction,
            layout: localLayout,
            cells: cells.enumerated().map { index, cell in
                PreparedViewerTableCell(
                    sourcePosition: index,
                    frame: cell.frame,
                    contentOrigin: cell.contentOrigin,
                    content: cell.content.cellShape?.localLayout ?? cell.content.sourceNeutralized(),
                    sourceCellIndex: nil,
                    isHeader: cell.isHeader,
                    attributesKey: nil
                )
            },
            preparationError: preparationError
        )
    }
}

func preparedCellShapeKey(
    contentKey: String,
    document: ViewerDocument,
    widthPixels: Int,
    theme: PreparedProseTheme,
    key: ProseLayoutKey
) -> PreparedCellShapeKey {
    var atoms: [String] = []
    var images: [String] = []
    func append(_ current: ViewerDocument) {
        for block in current.blocks {
            for inline in block.inlines {
                guard case let .atom(nodeType, docPos, attrsJSON, _) = inline else { continue }
                if theme.viewerAtoms?.nodeTypes.contains(nodeType) == true {
                    let measurement = theme.viewerAtoms?.measurements[String(docPos)]
                    let measuredWidth = measurement?["width"] ?? -1
                    let measuredHeight = measurement?["height"] ?? theme.viewerAtoms?.estimatedHeights[nodeType] ?? 0
                    atoms.append("\(nodeType):\(measuredWidth.bitPattern):\(measuredHeight.bitPattern)")
                }
                if let image = ViewerImageAttachment.sourceAndDeclaredSize(nodeType: nodeType, docPos: docPos, attrsJSON: attrsJSON) {
                    let resolved = image.declaredSize ?? ViewerImageIntrinsicStore.shared.size(for: image.id, source: image.source)
                    images.append("\(Double(resolved?.width ?? 0).bitPattern):\(Double(resolved?.height ?? 0).bitPattern)")
                }
            }
            guard let table = block.table else { continue }
            for cell in table.cells {
                if let child = try? current.cellDocument(for: cell) { append(child) }
            }
        }
    }
    append(document)
    return PreparedCellShapeKey(
        contentKey: contentKey,
        widthPixels: widthPixels,
        scaleBits: key.displayScaleBits,
        styleDigest: "\(theme.cellShapeStyleDigest):\(key.nativeFontRevision):\(key.fontEnvironmentRevision):\(Double(theme.fontScale).bitPattern)",
        atomGeometryDigest: atoms.joined(separator: "|"),
        imageGeometryDigest: images.joined(separator: "|")
    )
}
