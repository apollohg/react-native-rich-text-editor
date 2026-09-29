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
        localLayout.currentRetainedBytes + 256 + key.contentKey.utf8.count
            + key.styleDigest.utf8.count + key.atomGeometryDigest.utf8.count
            + key.imageGeometryDigest.utf8.count
    }
}

private final class PreparedCellShapeReference {
    weak var shape: PreparedCellShape?
    init(_ shape: PreparedCellShape) { self.shape = shape }
}

final class PreparedCellShapeBuildContext {
    private let catalog: PreparedCellShapeCatalog
    private var resolved: [PreparedCellShapeKey: PreparedCellShapeReference] = [:]
    private var pins: [ObjectIdentifier: PreparedCellShapeReference] = [:]
    private var closed = false

    fileprivate init(catalog: PreparedCellShapeCatalog) {
        self.catalog = catalog
    }

    func fork() -> PreparedCellShapeBuildContext { catalog.newBuildContext() }

    func resolve(
        _ key: PreparedCellShapeKey,
        build: () throws -> PreparedProseLayout,
        bind: (PreparedCellShape) -> PreparedProseLayout?
    ) throws -> PreparedProseLayout {
        if closed { return try build() }
        if let shape = resolved[key]?.shape {
            if let bound = bind(shape) { return bound.withCellShape(shape) }
            return try build()
        }
        if let shape = catalog.acquireForBuild(key) {
            rememberPinned(shape)
            if let bound = bind(shape) {
                return bound.withCellShape(shape)
            }
            return try build()
        }
        let fresh = try build()
        let candidate = PreparedCellShape(key: key, localLayout: fresh.sourceNeutralized(semanticKey: UUID().uuidString))
        let shape = catalog.stageForBuild(candidate)
        rememberPinned(shape)
        if shape !== candidate, let bound = bind(shape) {
            return bound.withCellShape(shape)
        }
        return fresh.withCellShape(shape)
    }

    func close() {
        guard !closed else { return }
        closed = true
        catalog.releaseBuildPins(pins.values.compactMap(\.shape))
        pins.removeAll()
        resolved.removeAll()
    }

    fileprivate func rememberPinned(_ shape: PreparedCellShape) {
        if let previous = resolved.removeValue(forKey: shape.key)?.shape { release(previous) }
        if resolved.count >= TableCellLayoutStore.maximumResidentLayouts {
            for (key, reference) in resolved where reference.shape == nil { resolved.removeValue(forKey: key) }
            if resolved.count >= TableCellLayoutStore.maximumResidentLayouts, let key = resolved.keys.first {
                if let previous = resolved.removeValue(forKey: key)?.shape { release(previous) }
            }
            pins = pins.filter { $0.value.shape != nil }
        }
        let reference = PreparedCellShapeReference(shape)
        resolved[shape.key] = reference
        pins[ObjectIdentifier(shape)] = reference
    }

    private func release(_ shape: PreparedCellShape) {
        if pins.removeValue(forKey: ObjectIdentifier(shape)) != nil { catalog.releaseBuildPins([shape]) }
    }

}

/// Private index owned by `PreparedProseLayoutCache`; it never publishes a layout.
final class PreparedCellShapeCatalog {
    private let lock = NSLock()
    private var entries: [PreparedCellShapeKey: PreparedCellShapeReference] = [:]
    private var buildPins: [ObjectIdentifier: (reference: PreparedCellShapeReference, count: Int)] = [:]
    private var owners: [ObjectIdentifier: Set<PreparedCellShapeKey>] = [:]
    private var ownerCounts: [ObjectIdentifier: Int] = [:]
    private var stagedSincePrune = 0
    private(set) var prunePassesForTesting = 0

    func newBuildContext(reusing layouts: [PreparedProseLayout] = []) -> PreparedCellShapeBuildContext {
        let context = PreparedCellShapeBuildContext(catalog: self)
        var shapes: [PreparedCellShapeKey: PreparedCellShape] = [:]
        let capacity = TableCellLayoutStore.maximumResidentLayouts
        for layout in layouts {
            guard shapes.count < capacity else { break }
            collectShapes(in: layout, into: &shapes, maximumCount: capacity)
        }
        shapes.values.forEach { context.rememberPinned(stageForBuild($0)) }
        return context
    }

    func acquireForBuild(_ key: PreparedCellShapeKey) -> PreparedCellShape? {
        lock.lock()
        defer { lock.unlock() }
        guard let shape = entries[key]?.shape else { return nil }
        pinLocked(shape)
        return shape
    }

    func stageForBuild(_ shape: PreparedCellShape) -> PreparedCellShape {
        lock.lock()
        stagedSincePrune += 1
        if stagedSincePrune >= TableCellLayoutStore.maximumResidentLayouts {
            pruneLocked()
            stagedSincePrune = 0
        }
        let retained = entries[shape.key]?.shape ?? shape
        if entries[shape.key]?.shape == nil { entries[shape.key] = PreparedCellShapeReference(shape) }
        pinLocked(retained)
        lock.unlock()
        return retained
    }

    private func pinLocked(_ shape: PreparedCellShape) {
        let identifier = ObjectIdentifier(shape)
        let previous = buildPins[identifier]
        let count = previous?.reference.shape === shape ? (previous?.count ?? 0) : 0
        buildPins[identifier] = (PreparedCellShapeReference(shape), count + 1)
    }

    func releaseBuildPins(_ shapes: [PreparedCellShape]) {
        lock.lock()
        for shape in shapes {
            let identifier = ObjectIdentifier(shape)
            guard let pin = buildPins[identifier], pin.reference.shape === shape else { continue }
            if pin.count == 1 { buildPins.removeValue(forKey: identifier) }
            else { buildPins[identifier] = (pin.reference, pin.count - 1) }
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
            owners[identifier] = Set(shapes.keys)
            for (key, shape) in shapes where entries[key]?.shape == nil { entries[key] = PreparedCellShapeReference(shape) }
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
        pruneLocked()
        return entries.count
    }

    var retainedBytesForTesting: Int {
        lock.lock(); defer { lock.unlock() }
        pruneLocked()
        return entries.values.reduce(0) { $0 + ($1.shape?.catalogRetainedBytes ?? 0) }
    }

    private func pruneLocked() {
        prunePassesForTesting += 1
        buildPins = buildPins.filter { $0.value.reference.shape != nil }
        let owned = Set(owners.values.flatMap { $0 })
        entries = entries.filter { key, reference in
            guard let shape = reference.shape else { return false }
            return owned.contains(key) || (buildPins[ObjectIdentifier(shape)]?.count ?? 0) > 0
        }
    }

    private func collectShapes(
        in layout: PreparedProseLayout,
        into destination: inout [PreparedCellShapeKey: PreparedCellShape],
        maximumCount: Int = Int.max
    ) {
        layout.forEachRetainedLayout { retained in
            if destination.count < maximumCount, let shape = retained.cellShape { destination[shape.key] = shape }
        }
    }
}

private extension PreparedProseLayout {
    func sourceNeutralized(semanticKey: String) -> PreparedProseLayout {
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
                semanticKey: semanticKey,
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
        let store = TableCellLayoutStore()
        return ViewerTableSurface(identity: "cell-table", hostViewportWidth: hostViewportWidth,
            style: style, direction: direction, layout: layout, cells: cells.map { cell in
                let content = cell.content
                let rebuild = content.cellPreparation ?? { content }
                let semanticKey = "cell-shape:\(cell.sourceIndex)"
                func neutralize(_ prepared: PreparedProseLayout) -> PreparedProseLayout {
                    prepared.cellShape?.localLayout ?? prepared.sourceNeutralized(semanticKey: semanticKey)
                }
                let prepare = { neutralize(rebuild()) }
                return PreparedViewerTableCell(sourceIndex: cell.sourceIndex, row: cell.row, column: cell.column,
                    rowspan: cell.rowspan, colspan: cell.colspan, contentOrigin: cell.contentOrigin,
                    content: neutralize(content), isHeader: cell.isHeader, attributesKey: nil,
                    layoutStore: store, prepareContent: prepare)
            }, preparationError: preparationError, displayScale: displayScale)
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
            guard let table = block.tableSurfaceSource, let tableKey = block.tableKey else { continue }
            for cell in table.cells {
                if let child = try? current.cellDocument(for: cell, in: tableKey) { append(child) }
            }
        }
    }
    append(document)
    return PreparedCellShapeKey(
        contentKey: contentKey,
        widthPixels: widthPixels,
        scaleBits: key.displayScaleBits,
        styleDigest: "\(theme.cellShapeStyleDigest):\(key.nativeFontRevision):\(key.fontEnvironmentRevision):\(Double(theme.fontScale).bitPattern):\(theme.tableDirection?.rawValue ?? "")",
        atomGeometryDigest: atoms.joined(separator: "|"),
        imageGeometryDigest: images.joined(separator: "|")
    )
}
