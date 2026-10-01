import CoreText
import UIKit

enum TableSelectionHandleRole: Int {
    case anchor
    case head
}

struct TableSelectionHandle {
    let role: TableSelectionHandleRole
    let tableID: String
    let sourcePosition: UInt32
    let center: CGPoint
    let clip: CGRect
    let color: UIColor
}

struct TableSelectionEndpoints: Equatable {
    let tableID: String
    let anchor: UInt32
    let head: UInt32
}

struct RemoteTableCellSelection: Equatable {
    let tableID: String
    let sourceIndices: Set<Int>
    let color: UIColor
}

struct TableCellDropTarget: Equatable {
    let tableID: String
    let sourceIndex: Int
}

struct TableResizeEdge: Equatable {
    let tableID: String
    let column: Int
}

struct TableResizeEdgeHit: Equatable {
    let edge: TableResizeEdge
    let columnWidth: CGFloat
    let minimumColumnWidth: CGFloat
    let rightToLeft: Bool
}

/// Mapping/reference overhead only. Decoded image allocations are owned and
/// accounted for by the shared native image cache.
internal enum PreparedProseImagePixelMapAccounting {
    static let mapRetainedBytes = 48
    static let entryRetainedBytes = 48

    static func retainedBytes(entryCount: Int) -> Int {
        guard entryCount > 0 else { return 0 }
        return saturatingAdd(
            mapRetainedBytes,
            saturatingMultiply(entryCount, entryRetainedBytes)
        )
    }

    private static func saturatingAdd(_ left: Int, _ right: Int) -> Int {
        guard left <= Int.max - right else { return Int.max }
        return left + right
    }

    private static func saturatingMultiply(_ left: Int, _ right: Int) -> Int {
        guard left > 0, right > 0 else { return 0 }
        guard left <= Int.max / right else { return Int.max }
        return left * right
    }
}

@objc public protocol PreparedProseDrawingViewInteractionDelegate: AnyObject {
    func preparedProseDrawingView(_ view: PreparedProseDrawingView, didActivateLink href: String, text: String) -> Bool
    func preparedProseDrawingView(_ view: PreparedProseDrawingView, didActivateMention docPos: UInt32, label: String, attrsJSON: String) -> Bool
}

/// A rendering-only view: it consumes already prepared Core Text lines.
@objc(PREPPreparedProseDrawingView)
public final class PreparedProseDrawingView: UIView {
    struct TableLayerCell: Equatable {
        let tableID: String
        let sourceIndex: Int
    }

    private enum TableLayerName: String {
        case above, boundRow, boundCell, below
    }

    private struct BoundCellChrome: Equatable {
        let row: Int
        let column: Int
        let rowspan: Int
        let colspan: Int
        let isHeader: Bool
        let attributesKey: String?
        let clip: CGRect
        let drawingBounds: CGRect
        let displayScaleBits: UInt64
        let borderWidth: CGFloat
        let borderColor: UIColor
        let headerBackgroundColor: UIColor
    }

    private struct TableLayerState {
        let cell: TableLayerCell?
        let window: CGRect
        let frame: CGRect
        let row: CGRect
        let rowOffsets: [CGFloat]
        let revision: UInt64?
        let appearance: String
        let excludesContent: Bool
        let chrome: BoundCellChrome?
    }

    let aboveLayer = CALayer()
    let boundRowLayer = CALayer()
    let boundCellLayer = CALayer()
    let belowLayer = CALayer()
    private(set) var layerRedrawsForTesting: [String: Int] = [:]
    var usesEditAnchoredLayers = false {
        didSet {
            guard usesEditAnchoredLayers != oldValue else { return }
            clearTableLayers()
            setNeedsDisplay()
        }
    }
    var tableLayerCell: TableLayerCell? {
        didSet {
            if tableLayerCell != oldValue { invalidateTableLayers(); setNeedsDisplay() }
        }
    }
    var tableLayerRevision: UInt64?
    var tableLayerChanges: TableFrameChanges?
    var tableLayerAppearance = ""
    private var tableLayerState: TableLayerState?

    private var tableLayers: [CALayer] { [aboveLayer, boundRowLayer, boundCellLayer, belowLayer] }

    private var retainedTableLayerBytes: Int {
        let stateBytes = tableLayerState.map { state in
            Self.saturatingAdd(MemoryLayout<TableLayerState>.stride,
                Self.saturatingAdd(Self.saturatingMultiply(state.rowOffsets.count, MemoryLayout<CGFloat>.stride),
                    Self.saturatingAdd(state.appearance.utf8.count, state.chrome?.attributesKey?.utf8.count ?? 0)))
        } ?? 0
        return tableLayers.reduce(stateBytes) { total, layer in
            guard let contents = layer.contents else { return total }
            let image = contents as! CGImage
            return Self.saturatingAdd(total, image.bytesPerRow * image.height)
        }
    }

    private func clearTableLayers() {
        CATransaction.begin()
        CATransaction.setDisableActions(true)
        defer { CATransaction.commit() }
        tableLayerState = nil
        for layer in tableLayers {
            layer.contents = nil
            layer.isHidden = true
        }
    }

    private func invalidateTableLayers() {
        tableLayerState = nil
    }

    let codeHighlightingSession = NativeCodeHighlightingSession()
    var onCodeHighlightingResolved: ((String) -> Void)?
    var onCodeHighlightingFailure: ((Error) -> Void)?
    @objc public static let codeHighlightingDidResolve = Notification.Name("com.apollohg.editor.viewer.codeHighlightingDidResolve")
    @objc public static let codeHighlightingDidFail = Notification.Name("com.apollohg.editor.viewer.codeHighlightingDidFail")

    deinit { codeHighlightingSession.cancel() }

    var imagePixels: [String: UIImage] = [:] {
        didSet {
            invalidateTableLayers()
            PreparedProseInstrumentation.retained(.image, scope: "drawing-\(ObjectIdentifier(self))", bytes: retainedImagePixelsBytesForTesting)
            setNeedsDisplay()
        }
    }
    /// This map owns only mapping/reference overhead. The shared native image
    /// cache is the sole owner charged for a decoded CGImage allocation.
    internal var retainedImagePixelsBytesForTesting: Int {
        PreparedProseImagePixelMapAccounting.retainedBytes(entryCount: imagePixels.count)
    }
    internal var preparedSurfaceRetainedBytesForTesting: Int {
        Self.saturatingAdd(
            Self.saturatingAdd(
                Self.saturatingAdd(layout?.retainedBytes ?? 0, retainedTableLayerBytes),
                imageRevisions.retainedPublicationBytesForTesting
            ),
            Self.saturatingAdd(
                retainedImagePixelsBytesForTesting,
                tablePresentationOwner.retainedBytes
            )
        )
    }
    internal var tablePresentationRetainedBytesForTesting: Int {
        tablePresentationOwner.retainedBytes
    }
    internal var imageRevisionForTesting: UInt64 { imageRevisions.revision }
    internal var imageRevisionStateForTesting: ViewerAttachmentRevisionState { imageRevisions }
    private func updateSidecarInstrumentation() {
        PreparedProseInstrumentation.retained(
            .sidecars,
            scope: "drawing-\(ObjectIdentifier(self))",
            bytes: Self.saturatingAdd(
                imageRevisions.retainedPublicationBytesForTesting,
                tablePresentationOwner.retainedBytes
            )
        )
    }
    @objc public static let imageMetadataDidResolve = Notification.Name("com.apollohg.editor.viewer.imageMetadataDidResolve")
    @objc public static let imageResourceDidFail = Notification.Name("com.apollohg.editor.viewer.imageResourceDidFail")
    private let imagePipeline: ViewerImagePipeline
    private var imageRevisions = ViewerAttachmentRevisionState()
    private var imageGeneration = ""
    private var imageConfiguration: (enabled: Bool, policy: ImageLoadingPolicy) = (false, .default)
    private var scrollObservations: [NSKeyValueObservation] = []
    private var observedScrollViewIDs: [ObjectIdentifier] = []
    private var tableOwnerIdentityOverride: String?
    private var mountedTableOwnerIdentity: String?
    private var tableInteractionController: TableInteractionController?
    var layout: PreparedProseLayout? {
        didSet {
            guard oldValue !== layout else { return }
            if layout == nil { clearTableLayers() }
            else if tableLayerState?.revision == layout?.key.attachmentRevision { invalidateTableLayers() }
            tableInteractionController?.cancelMotion()
            let nextOwner = layout.map { tableOwnerIdentityOverride ?? $0.key.semanticKey }
                ?? tableOwnerIdentityOverride
            if nextOwner == nil || nextOwner != mountedTableOwnerIdentity {
                tablePresentationOwner = ViewerTablePresentationOwner()
            } else if let layout {
                tablePresentationOwner.retain(surfaces: ViewerTablePresentation.surfaces(in: layout))
            }
            mountedTableOwnerIdentity = nextOwner
            updateSidecarInstrumentation()
            invalidateAccessibilityNodes()
            setNeedsDisplay()
        }
    }
    private var tablePresentationOwner = ViewerTablePresentationOwner()
    private var drawnPresentationWindow: CGRect?
    fileprivate var accessibilityPresentationGeneration = 0
    @objc public var onTableGeometryChanged: (() -> Void)?
    var onMountedTableCellsDrawnForTesting: ((Int) -> Void)?
    var onTableChromeDrawnForTesting: ((Int) -> Void)?
    var onTableRichFragmentDrawnForTesting: (() -> Void)?
    weak var excludedTableCellContentLayout: PreparedProseLayout? {
        didSet {
            if oldValue !== excludedTableCellContentLayout { setNeedsDisplay() }
        }
    }
    var selectedTableCellSourceIndices: [String: Set<Int>] = [:] {
        didSet {
            if selectedTableCellSourceIndices != oldValue { invalidateTableLayers(); setNeedsDisplay() }
        }
    }
    var selectedTableCellEndpoints: TableSelectionEndpoints? {
        didSet {
            if selectedTableCellEndpoints != oldValue { invalidateTableLayers(); setNeedsDisplay() }
        }
    }
    var remoteTableCellSelections: [RemoteTableCellSelection] = [] {
        didSet {
            if remoteTableCellSelections != oldValue { invalidateTableLayers(); setNeedsDisplay() }
        }
    }
    var activeTableResizeEdge: TableResizeEdge? {
        didSet {
            if activeTableResizeEdge != oldValue { invalidateTableLayers(); setNeedsDisplay() }
        }
    }
    var tableCellDropTarget: TableCellDropTarget? {
        didSet {
            if tableCellDropTarget != oldValue { invalidateTableLayers(); setNeedsDisplay() }
        }
    }

    private enum TableHandleMetrics {
        static let radius: CGFloat = 8
        static let hitDiameter: CGFloat = 44
    }

    private enum TableResizeMetrics {
        static let indicatorWidth: CGFloat = 2
    }

    private static let redrawHysteresisViewports: CGFloat = 0.5

    @objc public func install(layout: PreparedProseLayout?) {
        guard self.layout !== layout else { return }
        self.layout = layout
        scheduleCodeHighlighting()
    }

    func setTableOwnerIdentity(_ identity: String?) {
        guard tableOwnerIdentityOverride != identity else { return }
        tableOwnerIdentityOverride = identity
        clearTableLayers()
        tableInteractionController?.cancelMotion()
        tablePresentationOwner = ViewerTablePresentationOwner()
        mountedTableOwnerIdentity = nil
        updateSidecarInstrumentation()
        updateConfiguredImagesForVisibleWindow()
        invalidateAccessibilityNodes()
        onTableGeometryChanged?()
        setNeedsDisplay()
    }

    @objc(configureImagesWithGeneration:imagesEnabled:policyJSON:)
    public func configureImages(generation: String, imagesEnabled: Bool, policyJSON: String?) {
        imageGeneration = generation
        imageConfiguration = (imagesEnabled, ImageLoadingPolicy.from(json: policyJSON))
        imagePipeline.onPixels = { [weak self] attachment, image in self?.imagePixels[attachment.id] = image }
        imagePipeline.onIntrinsicMetadata = { [weak self] attachment, size in
            guard let self,
                  self.imagePipeline.acceptsCompletion(generation: generation),
                  self.imageRevisions.recordIntrinsicSize(size, for: attachment.id, ordinal: attachment.ordinal, declaredSize: attachment.declaredSize)
            else { return }
            NotificationCenter.default.post(
                name: Self.imageMetadataDidResolve,
                object: self,
                userInfo: ["generation": generation, "revision": self.imageRevisions.revision]
            )
        }
        imagePipeline.onResourceFailure = { [weak self] attachment in
            guard let self,
                  self.imagePipeline.acceptsCompletion(generation: generation),
                  self.imageRevisions.recordResourceFailure(for: attachment.ordinal)
            else { return }
            NotificationCenter.default.post(
                name: Self.imageResourceDidFail,
                object: self,
                userInfo: ["generation": generation, "attachment": attachment.id]
            )
        }
        imagePipeline.begin(
            generation: imageGeneration,
            imagesEnabled: imageConfiguration.enabled,
            policy: imageConfiguration.policy
        )
        imageRevisions.admit(attachmentCount: layout?.imageAttachments.count ?? 0)
    }

    /// Phase one of Fabric setup: semantic props/state have been accepted but
    /// no mounted artifact exists yet. This clears active intrinsic fallback
    /// before measurement/preparation and phase two only binds ordinals.
    @objc(beginSemanticImageGeneration:)
    public func beginSemanticImageGeneration(_ generation: String) {
        guard imageRevisions.beginSemanticGeneration(generation) else { return }
        imageGeneration = ""
        imagePipeline.cancel()
        imagePixels = [:]
    }

    /// Fabric has already reset this sidecar during Yoga preparation. Mount
    /// only transfers the stable surface owner; it must not reopen metadata or
    /// error publication by resetting a second time.
    @objc(bindFabricAttachmentStateSurfaceId:componentTag:leaseHandle:)
    public func bindFabricAttachmentState(surfaceId: Int64, componentTag: Int64, leaseHandle: UInt64) {
        guard let state = FabricAttachmentSidecars.state(
            for: .init(surfaceId: surfaceId, componentTag: componentTag),
            leaseHandle: leaseHandle
        ) else { return }
        imageRevisions = state
    }

    @objc public func updateConfiguredImagesForVisibleWindow() {
        refreshScrollObservations()
        guard let layout, let visible = configuredVisibleRect() else {
            imagePipeline.leaveViewport()
            if !imagePixels.isEmpty { imagePixels = [:] }
            onVisibleRectChange?(nil)
            return
        }
        let attachments: [ViewerImageAttachment] = ViewerTablePresentation.project(
            layout: layout,
            owner: tablePresentationOwner,
            viewport: .known(visible)
        ).images.compactMap { image in
            let bounds = image.bounds.intersection(image.clip)
            guard !bounds.isNull, !bounds.isEmpty else { return nil }
            return ViewerImageAttachment(
                ordinal: image.attachment.ordinal,
                id: image.attachment.id,
                source: image.attachment.source,
                bounds: bounds,
                declaredSize: image.attachment.declaredSize
            )
        }
        let retainedIDs = imagePipeline.updateVisibleRect(visible, attachments: attachments)
        onVisibleRectChange?(visible)
        guard imagePixels.keys.contains(where: { !retainedIDs.contains($0) }) else { return }
        imagePixels = imagePixels.filter { retainedIDs.contains($0.key) }
    }

    @objc public func cancelConfiguredImages() {
        imageGeneration = ""
        imagePipeline.cancel()
        imagePixels = [:]
    }

    func mountedTablePresentation() -> ViewerTablePresentationSnapshot? {
        presentationSnapshot()
    }

    func presentedTableCell(tableID: String, sourceIndex: Int) -> ViewerTablePresentedCell? {
        guard let table = presentationSnapshot()?.tables.first(where: { $0.surface.identity == tableID }),
              let cell = table.surface.cell(sourceIndex: sourceIndex)
        else { return nil }
        return ViewerTablePresentation.present(cell, in: table, owner: tablePresentationOwner)
    }

    func tableSelectionViewport() -> CGRect? {
        configuredVisibleRect()
    }

    func selectedTableCellRects(tableID: String) -> [CGRect]? {
        tableCellRects(tableID: tableID, sourceIndices: selectedTableCellSourceIndices[tableID] ?? [])
    }

    func tableCellRects(tableID: String, sourceIndices: Set<Int>) -> [CGRect]? {
        guard let snapshot = presentationSnapshot(),
              snapshot.tables.contains(where: { $0.surface.identity == tableID })
        else { return nil }
        return snapshot.cells.filter { $0.surface.identity == tableID && isRealTableCell($0, in: sourceIndices) }
            .map { $0.bounds.intersection($0.clip) }
            .filter { !$0.isNull && !$0.isEmpty }
    }

    private func isSelectedTableCell(_ cell: ViewerTablePresentedCell) -> Bool {
        isRealTableCell(cell, in: selectedTableCellSourceIndices[cell.surface.identity] ?? [])
    }

    private func isRealTableCell(_ cell: ViewerTablePresentedCell, in sourceIndices: Set<Int>) -> Bool {
        cell.surface.sourceTable != nil && sourceIndices.contains(cell.sourceIndex)
    }

    func selectionHandles(visibleIn requestedViewport: CGRect? = nil) -> [TableSelectionHandle] {
        guard let endpoints = selectedTableCellEndpoints,
              let selectedPositions = selectedTableCellSourceIndices[endpoints.tableID],
              let visible = configuredVisibleRect()?.intersection(requestedViewport ?? .infinite),
              !visible.isNull, !visible.isEmpty,
              let snapshot = presentationSnapshot(),
              let table = snapshot.tables.first(where: { $0.surface.identity == endpoints.tableID })
        else { return [] }
        let cells = snapshot.cells.filter {
            $0.surface === table.surface && $0.surface.sourceTable != nil
                && selectedPositions.contains($0.sourceIndex)
        }
        let selected = selectedPositions.compactMap { table.surface.cell(sourceIndex: $0) }
            .filter { _ in table.surface.sourceTable != nil }
        guard selected.contains(where: { tableCellDocumentPosition?(endpoints.tableID, $0.sourceIndex) == endpoints.anchor }),
              selected.contains(where: { tableCellDocumentPosition?(endpoints.tableID, $0.sourceIndex) == endpoints.head }),
              let firstCell = selected.min(by: { lhs, rhs in
                  let lhsFrame = table.surface.frame(ofCell: lhs)
                  let rhsFrame = table.surface.frame(ofCell: rhs)
                  if lhsFrame.minY != rhsFrame.minY { return lhsFrame.minY < rhsFrame.minY }
                  let left = table.surface.direction == .rightToLeft ? -lhsFrame.maxX : lhsFrame.minX
                  let right = table.surface.direction == .rightToLeft ? -rhsFrame.maxX : rhsFrame.minX
                  return left == right ? lhs.sourceIndex < rhs.sourceIndex : left < right
              }),
              let lastCell = selected.max(by: { lhs, rhs in
                  let lhsFrame = table.surface.frame(ofCell: lhs)
                  let rhsFrame = table.surface.frame(ofCell: rhs)
                  if lhsFrame.maxY != rhsFrame.maxY { return lhsFrame.maxY < rhsFrame.maxY }
                  let left = table.surface.direction == .rightToLeft ? -lhsFrame.minX : lhsFrame.maxX
                  let right = table.surface.direction == .rightToLeft ? -rhsFrame.minX : rhsFrame.maxX
                  return left == right ? lhs.sourceIndex < rhs.sourceIndex : left < right
              })
        else { return [] }
        let first = ViewerTablePresentation.present(firstCell, in: table, owner: tablePresentationOwner)
        let last = ViewerTablePresentation.present(lastCell, in: table, owner: tablePresentationOwner)
        let inset = TableHandleMetrics.radius
        let rtl = table.surface.direction == .rightToLeft
        let firstCenter = CGPoint(x: rtl ? first.bounds.maxX - inset : first.bounds.minX + inset,
                                  y: first.bounds.minY + inset)
        let lastCenter = CGPoint(x: rtl ? last.bounds.minX + inset : last.bounds.maxX - inset,
                                 y: last.bounds.maxY - inset)
        let forward = endpoints.anchor <= endpoints.head
        let handles = [
            TableSelectionHandle(role: forward ? .anchor : .head, tableID: endpoints.tableID,
                                 sourcePosition: forward ? endpoints.anchor : endpoints.head,
                                 center: firstCenter, clip: table.clip,
                                 color: table.surface.style.selectionColor.withAlphaComponent(1)),
            TableSelectionHandle(role: forward ? .head : .anchor, tableID: endpoints.tableID,
                                 sourcePosition: forward ? endpoints.head : endpoints.anchor,
                                 center: lastCenter, clip: table.clip,
                                 color: table.surface.style.selectionColor.withAlphaComponent(1))
        ]
        return handles.filter { handle in
            visible.contains(handle.center) && table.clip.contains(handle.center)
                && cells.contains(where: { $0.bounds.contains(handle.center) })
        }
    }

    func hitSelectionHandle(at point: CGPoint, visibleIn viewport: CGRect? = nil) -> TableSelectionHandle? {
        guard let visible = configuredVisibleRect()?.intersection(viewport ?? .infinite),
              visible.contains(point)
        else { return nil }
        let radiusSquared = pow(TableHandleMetrics.hitDiameter / 2, 2)
        return selectionHandles(visibleIn: viewport).compactMap { handle -> (TableSelectionHandle, CGFloat)? in
            let distance = pow(point.x - handle.center.x, 2) + pow(point.y - handle.center.y, 2)
            return distance <= radiusSquared ? (handle, distance) : nil
        }.min { lhs, rhs in
            lhs.1 == rhs.1 ? lhs.0.role.rawValue < rhs.0.role.rawValue : lhs.1 < rhs.1
        }?.0
    }

    func selectedTableCell(at point: CGPoint, tableID: String, visibleIn viewport: CGRect? = nil) -> UInt32? {
        guard let visible = configuredVisibleRect()?.intersection(viewport ?? .infinite),
              visible.contains(point),
              let snapshot = presentationSnapshot(),
              let table = snapshot.tables.first(where: { $0.surface.identity == tableID })
        else { return nil }
        return snapshot.cells.first(where: {
            $0.surface === table.surface && $0.surface.sourceTable != nil
                && $0.bounds.contains(point) && $0.clip.contains(point)
        }).flatMap { tableCellDocumentPosition?(tableID, $0.sourceIndex) }
    }

    func tableLogicalOffset(for identity: String) -> CGFloat {
        guard let surface = presentationSnapshot()?.tables.first(where: { $0.surface.identity == identity })?.surface
        else { return 0 }
        return tablePresentationOwner.logicalOffset(for: surface)
    }

    private func rootTable(_ identity: String, in snapshot: ViewerTablePresentationSnapshot) -> ViewerTablePresentedTable? {
        snapshot.tables.first { $0.parentScrollIdentity == nil && $0.surface.identity == identity }
    }

    private func tableContentOriginX(_ table: ViewerTablePresentedTable) -> CGFloat {
        table.bounds.minX - tablePresentationOwner.physicalOffset(for: table.surface)
    }

    func columnTrailingEdgeX(for edge: TableResizeEdge) -> CGFloat? {
        guard let snapshot = presentationSnapshot(),
              let table = rootTable(edge.tableID, in: snapshot),
              edge.column >= 0, edge.column < table.surface.layout.columnWidths.count
        else { return nil }
        let logical = table.surface.layout.columnWidths.prefix(edge.column + 1).reduce(CGFloat.zero, +)
        let origin = tableContentOriginX(table)
        return table.surface.direction == .rightToLeft
            ? origin + table.surface.bounds.width - logical
            : origin + logical
    }

    func hitResizeEdge(at point: CGPoint, visibleIn viewport: CGRect? = nil) -> TableResizeEdgeHit? {
        guard let visible = configuredVisibleRect()?.intersection(viewport ?? .infinite),
              visible.contains(point),
              let snapshot = presentationSnapshot()
        else { return nil }
        let reach = TableHandleMetrics.hitDiameter / 2
        var best: (hit: TableResizeEdgeHit, distance: CGFloat)?
        for table in snapshot.tables where table.parentScrollIdentity == nil {
            guard table.clip.minY <= point.y, point.y < table.clip.maxY,
                  let sourceTable = table.surface.sourceTable
            else { continue }
            let sourceCells = sourceTable.cells
            let widths = table.surface.layout.columnWidths
            let rowOffsets = table.surface.layout.rowOffsets
            let selectedColumns = selectedWholeColumns(tableID: table.surface.identity, in: sourceTable)
            let rightToLeft = table.surface.direction == .rightToLeft
            for cell in snapshot.cells where cell.surface === table.surface {
                let index = cell.cell.sourceIndex
                guard index < sourceCells.count,
                      cell.bounds.minY <= point.y, point.y < cell.bounds.maxY
                else { continue }
                let source = sourceCells[index]
                let column = Int(source.column + source.colspan) - 1
                let row = Int(source.row)
                let handleRowHeight = row + 1 < rowOffsets.count ? rowOffsets[row + 1] - rowOffsets[row] : 0
                let inHandleRow = row == table.surface.columnEdgeHandleRows[column]
                    && point.y < cell.bounds.minY + handleRowHeight
                guard inHandleRow || selectedColumns.contains(column) else { continue }
                let x = rightToLeft ? cell.bounds.minX : cell.bounds.maxX
                let distance = abs(point.x - x)
                guard distance <= reach,
                      x >= table.clip.minX, x <= table.clip.maxX,
                      x >= visible.minX, x <= visible.maxX
                else { continue }
                guard column >= 0, column < widths.count else { continue }
                if let current = best,
                   current.distance < distance || (current.distance == distance && current.hit.edge.column <= column) {
                    continue
                }
                best = (TableResizeEdgeHit(
                    edge: TableResizeEdge(tableID: table.surface.identity, column: column),
                    columnWidth: widths[column],
                    minimumColumnWidth: table.surface.style.minColumnWidth,
                    rightToLeft: rightToLeft
                ), distance)
            }
        }
        return best?.hit
    }

    private func selectedWholeColumns(tableID: String, in table: TableSurfaceSource) -> Range<Int> {
        guard let endpoints = selectedTableCellEndpoints, endpoints.tableID == tableID,
              let positions = selectedTableCellSourceIndices[tableID],
              let anchor = table.cells.first(where: { tableCellDocumentPosition?(tableID, $0.sourceIndex) == endpoints.anchor }),
              let head = table.cells.first(where: { tableCellDocumentPosition?(tableID, $0.sourceIndex) == endpoints.head }),
              min(anchor.row, head.row) == 0,
              max(anchor.row + anchor.rowspan, head.row + head.rowspan) == table.rows
        else { return 0..<0 }
        let selected = table.cells.filter { positions.contains($0.sourceIndex) }
        guard let left = selected.map(\.column).min(),
              let right = selected.map({ $0.column + $0.colspan }).max()
        else { return 0..<0 }
        return Int(left)..<Int(right)
    }

    func tableChain(at point: CGPoint) -> [String] {
        guard let snapshot = presentationSnapshot(),
              let deepest = snapshot.tables.last(where: { $0.clip.contains(point) && $0.bounds.contains(point) })
        else { return [] }
        var chain: [String] = []
        var table: ViewerTablePresentedTable? = deepest
        while let current = table {
            chain.append(current.surface.scrollIdentity)
            table = current.parentScrollIdentity.flatMap { parent in
                snapshot.tables.last { $0.surface.scrollIdentity == parent }
            }
        }
        return chain
    }

    func canScrollTables(in chain: [String], by physicalDelta: CGFloat) -> Bool {
        guard let snapshot = presentationSnapshot() else { return false }
        return chain.contains { identity in
            guard let surface = snapshot.tables.first(where: { $0.surface.scrollIdentity == identity })?.surface
            else { return false }
            let logicalDelta = physicalDelta * (surface.direction == .rightToLeft ? 1 : -1)
            let offset = tablePresentationOwner.logicalOffset(for: surface)
            let maximum = max(0, surface.bounds.width - surface.hostViewportWidth)
            return logicalDelta > 0 ? offset < maximum : offset > 0
        }
    }

    @discardableResult
    func scrollTables(in chain: [String], by physicalDelta: CGFloat) -> CGFloat {
        guard physicalDelta.isFinite, physicalDelta != 0,
              let snapshot = presentationSnapshot()
        else { return physicalDelta }
        var remaining = physicalDelta
        var changed = false
        for identity in chain where remaining != 0 {
            guard let surface = snapshot.tables.first(where: { $0.surface.scrollIdentity == identity })?.surface
            else { return remaining }
            let old = tablePresentationOwner.logicalOffset(for: surface)
            let direction: CGFloat = surface.direction == .rightToLeft ? 1 : -1
            tablePresentationOwner.setLogicalOffset(old + remaining * direction, for: surface)
            let consumed = (tablePresentationOwner.logicalOffset(for: surface) - old) / direction
            remaining -= consumed
            changed = changed || consumed != 0
        }
        if changed {
            invalidateTableLayers()
            updateSidecarInstrumentation()
            updateConfiguredImagesForVisibleWindow()
            onTableGeometryChanged?()
            setNeedsDisplay()
        }
        return remaining
    }

    @objc(setTableLogicalOffset:sourceIdentity:)
    public func setTableLogicalOffset(_ offset: CGFloat, sourceIdentity: String) {
        tableInteractionController?.cancelMotion()
        guard let layout,
              let surface = ViewerTablePresentation.surfaces(in: layout).first(where: { $0.identity == sourceIdentity })
        else { return }
        tablePresentationOwner.setLogicalOffset(offset, for: surface)
        invalidateTableLayers()
        updateSidecarInstrumentation()
        updateConfiguredImagesForVisibleWindow()
        onTableGeometryChanged?()
        setNeedsDisplay()
    }

    private func presentationSnapshot() -> ViewerTablePresentationSnapshot? {
        layout.map { ViewerTablePresentation.project(layout: $0, owner: tablePresentationOwner, viewport: presentationViewport()) }
    }

    private func presentationViewport() -> ViewerTablePresentationViewport {
        guard window != nil else { return .unknown }
        guard !isHidden, alpha > 0 else { return .known(.zero) }
        guard let visible = configuredVisibleRect() else { return .known(.zero) }
        return .known(visible)
    }

    /// A semantic prop replacement starts a new source-qualified publication
    /// generation. Attachment-revision replacements deliberately do not call
    /// this: they are the one reflow being deduplicated.
    @objc public func resetIntrinsicImagePublication() {
        imageRevisions.reset()
    }

    @objc public var errorDomain: String? { layout?.error?.domain }
    @objc public var errorCode: String? { layout?.error?.code }
    @objc public var errorMessage: String? { layout?.error?.message }

    @objc public func atomLayoutsJSON(origin: CGPoint) -> String {
        guard let layout else { return "[]" }
        let snapshot = ViewerTablePresentation.project(
            layout: layout,
            owner: tablePresentationOwner,
            viewport: presentationViewport()
        )
        let mountedLayouts = Set(snapshot.mountedCells.map { ObjectIdentifier($0.content) })
        let atoms: [[String: Any]] = snapshot.atoms.map { presented in
            let atom = presented.atom
            var value: [String: Any] = [
                "nodeType": atom.nodeType,
                "docPos": atom.docPos,
                "attrsJson": atom.attrsJSON,
                "x": presented.bounds.minX + origin.x,
                "y": presented.bounds.minY + origin.y,
                "width": atom.bounds.width,
                "height": atom.bounds.height
            ]
            guard presented.layout !== layout else { return value }
            let rawClip = presented.clip
            let clip = [rawClip.minX, rawClip.minY, rawClip.width, rawClip.height]
                .allSatisfy(\.isFinite)
                ? rawClip
                : CGRect(x: presented.bounds.minX, y: presented.bounds.minY, width: 0, height: 0)
            let translatedClip = clip.offsetBy(dx: origin.x, dy: origin.y)
            value["presentation"] = [
                "clip": [
                    "x": translatedClip.minX,
                    "y": translatedClip.minY,
                    "width": max(0, translatedClip.width),
                    "height": max(0, translatedClip.height)
                ],
                "candidate": mountedLayouts.contains(ObjectIdentifier(presented.layout))
            ]
            return value
        }
        guard let data = try? JSONSerialization.data(withJSONObject: atoms, options: [.sortedKeys]) else { return "[]" }
        return String(data: data, encoding: .utf8) ?? "[]"
    }

    /// The owner chooses its delivery channel (UIKit delegate or Fabric event).
    var onActivateInteraction: ((PreparedProseInteraction) -> Bool)?
    var onVisibleRectChange: ((CGRect?) -> Void)?
    @objc public weak var interactionDelegate: PreparedProseDrawingViewInteractionDelegate?
    @objc public var linkInteractionsEnabled = true {
        didSet {
            guard oldValue != linkInteractionsEnabled else { return }
            invalidateAccessibilityNodes()
        }
    }
    private var accessibilityElementsByIndex: [Int: NSObject] = [:]
    private var accessibilityItemsCache: (generation: Int, items: [TableAccessibilityItem])?
    private enum AccessibilityAnnouncement {
        case structure
        case content(NSObject)
    }
    private var materializedAccessibilityStructure: [TableAccessibilityStructure] = []
    private var pendingAccessibilityAnnouncement: AccessibilityAnnouncement?
    private var accessibilityAnnouncementScheduled = false
    var accessibilityFocusProbe: (NSObject) -> Bool = { $0.accessibilityElementIsFocused() }
    var onAccessibilityLayoutChangedForTesting: ((Any?) -> Void)?
    var tableDocumentPosition: ((String) -> UInt32?)?
    var tableCellDocumentPosition: ((String, Int) -> UInt32?)?
    weak var tableAccessibilityEditing: TableAccessibilityEditing?
    weak var accessibilityRevealScrollView: UIScrollView?
    internal var materializedAccessibilityElementCountForTesting: Int { accessibilityElementsByIndex.count }

    private lazy var tapRecognizer: UITapGestureRecognizer = {
        let recognizer = UITapGestureRecognizer(target: self, action: #selector(handleTap(_:)))
        recognizer.cancelsTouchesInView = false
        return recognizer
    }()

    public override init(frame: CGRect) {
        imagePipeline = ViewerImagePipeline(policy: .default)
        super.init(frame: frame)
        configureDrawingView()
    }

    init(frame: CGRect, imagePipeline: ViewerImagePipeline) {
        self.imagePipeline = imagePipeline
        super.init(frame: frame)
        configureDrawingView()
    }

    private func configureDrawingView() {
        isAccessibilityElement = false
        addGestureRecognizer(tapRecognizer)
    }

    public override func didMoveToSuperview() {
        super.didMoveToSuperview()
        if isUserInteractionEnabled {
            installTableInteraction(on: superview)
        }
        updateConfiguredImagesForVisibleWindow()
    }

    public override func didMoveToWindow() {
        super.didMoveToWindow()
        if window == nil { tableInteractionController?.cancelMotion(); clearTableLayers() }
        if window == nil { codeHighlightingSession.cancel() } else { scheduleCodeHighlighting() }
        updateConfiguredImagesForVisibleWindow()
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) { fatalError("PreparedProseDrawingView does not support NSCoder") }

    func installTableInteraction(on host: UIView?) {
        tableInteractionController?.detach()
        tableInteractionController = host.map { TableInteractionController(host: $0, drawing: self) }
    }

    func cancelTableMotion() {
        tableInteractionController?.cancelMotion()
    }

    internal static func pixelAllocationBytes(for image: UIImage) -> Int {
        if let cgImage = image.cgImage {
            return saturatingMultiply(cgImage.bytesPerRow, cgImage.height)
        }
        let width = image.size.width * image.scale
        let height = image.size.height * image.scale
        guard width.isFinite, height.isFinite, width > 0, height > 0 else { return 0 }
        let pixels = min(Double(Int.max), width.rounded(.up) * height.rounded(.up))
        return saturatingMultiply(Int(pixels), 4)
    }

    internal static func saturatingAdd(_ left: Int, _ right: Int) -> Int {
        guard left <= Int.max - right else { return Int.max }
        return left + right
    }

    internal static func saturatingMultiply(_ left: Int, _ right: Int) -> Int {
        guard left > 0, right > 0 else { return 0 }
        guard left <= Int.max / right else { return Int.max }
        return left * right
    }

    private func configuredVisibleRect() -> CGRect? {
        editorVisibleRectInWindow
    }

    private var ancestorScrollViews: [UIScrollView] {
        var scrollViews: [UIScrollView] = []
        var ancestor = superview
        while let view = ancestor {
            if let scrollView = view as? UIScrollView { scrollViews.append(scrollView) }
            ancestor = view.superview
        }
        return scrollViews
    }

    private func refreshScrollObservations() {
        let activeScrollViews = window == nil ? [] : ancestorScrollViews
        let nextIDs = activeScrollViews.map(ObjectIdentifier.init)
        guard nextIDs != observedScrollViewIDs else { return }
        scrollObservations.removeAll()
        observedScrollViewIDs = nextIDs
        scrollObservations = activeScrollViews.map { scrollView in
            scrollView.observe(\.contentOffset, options: [.new]) { [weak self] _, _ in
                self?.redrawIfVisibleRectLeftDrawnWindow()
                self?.updateConfiguredImagesForVisibleWindow()
                self?.onTableGeometryChanged?()
            }
        }
    }

    private func redrawIfVisibleRectLeftDrawnWindow() {
        guard let visible = configuredVisibleRect(),
              drawnPresentationWindow?.contains(visible.insetBy(
                  dx: -visible.width * Self.redrawHysteresisViewports,
                  dy: -visible.height * Self.redrawHysteresisViewports
              )) != true
        else { return }
        setNeedsDisplay()
    }

    func interaction(at point: CGPoint) -> PreparedProseInteraction? {
        presentationSnapshot()?.interactions.first { interaction in
            (linkInteractionsEnabled || interaction.interaction.kind != .link) &&
                interaction.rects.contains { $0.contains(point) } && interaction.clip.contains(point)
        }?.interaction
    }

    @objc private func handleTap(_ recognizer: UITapGestureRecognizer) {
        guard recognizer.state == .ended,
              let interaction = interaction(at: recognizer.location(in: self))
        else { return }
        _ = activate(interaction)
    }

    @discardableResult
    private func activate(_ interaction: PreparedProseInteraction) -> Bool {
        if let onActivateInteraction { return onActivateInteraction(interaction) }
        switch interaction.kind {
        case .link:
            guard linkInteractionsEnabled, let href = interaction.href else { return false }
            return interactionDelegate?.preparedProseDrawingView(self, didActivateLink: href, text: interaction.visibleText) ?? false
        case .mention:
            guard let docPos = interaction.docPos, let attrsJSON = interaction.attrsJSON else { return false }
            return interactionDelegate?.preparedProseDrawingView(
                self,
                didActivateMention: docPos,
                label: interaction.label,
                attrsJSON: attrsJSON
            ) ?? false
        }
    }

    public override func accessibilityElementCount() -> Int { accessibilityItems.count }

    public override func accessibilityElement(at index: Int) -> Any? {
        let items = accessibilityItems
        guard items.indices.contains(index), layout != nil else { return nil }
        if let existing = accessibilityElementsByIndex[index] { return existing }
        let element: NSObject
        switch items[index] {
        case let .node(presented):
            element = PreparedProseDrawingAccessibilityElement(container: self, presented: presented)
        case let .table(table) where table.frame != nil:
            element = TableAccessibilityFrameElement(drawingView: self, source: .drawn(table))
        case let .table(table):
            element = TableAccessibilityTableElement(drawingView: self, table: table)
        case let .detachedFrame(frame):
            element = TableAccessibilityFrameElement(drawingView: self, source: .detached(frame))
        }
        accessibilityElementsByIndex[index] = element
        return element
    }

    public override func index(ofAccessibilityElement element: Any) -> Int {
        reconcileAccessibilityElementsIfNeeded()
        guard let element = element as? NSObject,
              let index = accessibilityElementsByIndex.first(where: { $0.value === element })?.key
        else { return NSNotFound }
        return index
    }

    func isLiveAccessibilityElement(_ element: NSObject) -> Bool {
        reconcileAccessibilityElementsIfNeeded()
        return layout != nil && accessibilityElementsByIndex.values.contains { $0 === element }
    }

    func reconcileAccessibilityElementsIfNeeded() {
        _ = accessibilityItems
    }

    private func refreshAccessibilityElement(_ element: NSObject, with item: TableAccessibilityItem) {
        switch (element, item) {
        case let (node as PreparedProseDrawingAccessibilityElement, .node(presented)):
            node.presented = presented
        case let (frame as TableAccessibilityFrameElement, .table(table)):
            frame.refresh(.drawn(table))
        case let (table as TableAccessibilityTableElement, .table(presented)):
            table.refresh(presented)
        case let (frame as TableAccessibilityFrameElement, .detachedFrame(detached)):
            frame.refresh(.detached(detached))
        default:
            break
        }
    }

    private func focusableAccessibilityElements() -> [NSObject] {
        accessibilityElementsByIndex.values.flatMap { element -> [NSObject] in
            (element as? TableAccessibilityTableElement)?.materializedElements ?? [element]
        }
    }

    private func reconcileAccessibilityElements(with items: [TableAccessibilityItem]) {
        let structure = TableAccessibility.structure(of: items)
        defer { materializedAccessibilityStructure = structure }
        guard !accessibilityElementsByIndex.isEmpty else { return }
        guard structure == materializedAccessibilityStructure else {
            accessibilityElementsByIndex.removeAll(keepingCapacity: true)
            if accessibilityAnnouncementScheduled { pendingAccessibilityAnnouncement = .structure }
            return
        }
        let before = focusableAccessibilityElements().map { ($0, $0.accessibilityLabel, $0.accessibilityValue) }
        for (index, element) in accessibilityElementsByIndex {
            refreshAccessibilityElement(element, with: items[index])
        }
        guard accessibilityAnnouncementScheduled, pendingAccessibilityAnnouncement == nil,
              let changed = before.first(where: { element, label, value in
                  accessibilityFocusProbe(element)
                      && (element.accessibilityLabel != label || element.accessibilityValue != value)
              })?.0
        else { return }
        pendingAccessibilityAnnouncement = .content(changed)
    }

    public override var accessibilityCustomRotors: [UIAccessibilityCustomRotor]? {
        get { [linkRotor] }
        set { }
    }

    private lazy var linkRotor = UIAccessibilityCustomRotor(systemType: .link) { [weak self] predicate in
        self?.linkRotorResult(predicate)
    }

    private func linkRotorResult(_ predicate: UIAccessibilityCustomRotorSearchPredicate) -> UIAccessibilityCustomRotorItemResult? {
        let targets: [NSObject] = (0..<accessibilityElementCount()).flatMap { index -> [NSObject] in
            switch accessibilityElement(at: index) {
            case let node as PreparedProseDrawingAccessibilityElement where node.presented.node.role == .link:
                return [node]
            case let table as TableAccessibilityTableElement:
                return table.table.cells.indices.filter { table.table.cells[$0].containsLink }
                    .compactMap(table.cellElement(at:))
            default:
                return []
            }
        }
        let current = predicate.currentItem.targetElement.flatMap { target in
            targets.firstIndex { $0 === (target as AnyObject) }
        }
        let next: Int
        switch (predicate.searchDirection, current) {
        case (.next, nil): next = 0
        case (.previous, nil): next = targets.count - 1
        case let (.next, index?): next = index + 1
        case let (.previous, index?): next = index - 1
        @unknown default: return nil
        }
        guard targets.indices.contains(next) else { return nil }
        return UIAccessibilityCustomRotorItemResult(targetElement: targets[next], targetRange: nil)
    }

    private var accessibilityItems: [TableAccessibilityItem] {
        if let cached = accessibilityItemsCache, cached.generation == accessibilityPresentationGeneration {
            return cached.items
        }
        var items: [TableAccessibilityItem] = []
        if let layout, let snapshot = presentationSnapshot() {
            items = TableAccessibility.items(
                root: layout, rootNodes: readableAccessibilityNodes(snapshot.accessibilityNodes).filter { $0.layout === layout },
                linksEnabled: linkInteractionsEnabled,
                detachedFrames: tableAccessibilityEditing?.detachedTableAccessibilityFrames() ?? [],
                tableDocumentPosition: tableDocumentPosition
            )
        }
        accessibilityItemsCache = (accessibilityPresentationGeneration, items)
        reconcileAccessibilityElements(with: items)
        return items
    }

    func invalidateTableAccessibility() {
        invalidateAccessibilityNodes()
    }

    func tableAccessibilityCell(tableID: String, sourceIndex: Int) -> TableAccessibilityCell? {
        accessibilityItems.lazy.compactMap { item -> TableAccessibilityCell? in
            guard case let .table(table) = item, table.identity == tableID,
                  let cell = table.surface.cell(sourceIndex: sourceIndex)
            else { return nil }
            return table.cellIndex(sourceIndex: cell.sourceIndex).map { table.cells[$0] }
        }.first
    }

    func accessibilityScreenFrame(_ rect: CGRect, clip: CGRect) -> CGRect {
        guard layout != nil else { return .zero }
        return accessibilityScreenRect(rect.intersection(clip), in: self)
    }

    func presentedRootTable(_ surface: ViewerTableSurface) -> ViewerTablePresentedTable? {
        layout.flatMap { ViewerTablePresentation.rootTables(in: $0).first { $0.surface === surface } }
    }

    func presentedAccessibilityCell(_ cell: TableAccessibilityCell) -> ViewerTablePresentedCell? {
        presentedRootTable(cell.surface).map {
            ViewerTablePresentation.present(cell.cell, in: $0, owner: tablePresentationOwner)
        }
    }

    func tableAccessibilityInteractions(for cell: TableAccessibilityCell) -> [ViewerTablePresentedAccessibilityNode] {
        guard let presented = presentedAccessibilityCell(cell) else { return [] }
        return readableAccessibilityNodes(
            ViewerTablePresentation.contentAccessibilityNodes(of: presented, owner: tablePresentationOwner)
        ).filter { $0.node.interactionIndex != nil }
    }

    func revealTableAccessibilityCell(_ cell: TableAccessibilityCell) {
        guard let presented = presentedAccessibilityCell(cell) else { return }
        let visible = presented.bounds.intersection(presented.clip)
        if !visible.isNull, visible.width >= min(presented.bounds.width, presented.clip.width) {
            revealInEnclosingScrollView(presented.bounds)
            return
        }
        let surface = cell.surface
        let logical = surface.layout.columnWidths.prefix(cell.columns.location).reduce(CGFloat.zero, +)
        if tableLogicalOffset(for: surface.identity) != logical {
            setTableLogicalOffset(logical, sourceIdentity: surface.identity)
        }
        guard let revealed = presentedAccessibilityCell(cell) else { return }
        revealInEnclosingScrollView(revealed.bounds)
        let element = (0..<accessibilityElementCount()).lazy.compactMap { index in
            (self.accessibilityElement(at: index) as? TableAccessibilityTableElement)
                .flatMap { $0.table.identity == surface.identity ? $0.cellElement(sourceIndex: cell.sourceIndex) : nil }
        }.first
        UIAccessibility.post(notification: .layoutChanged, argument: element)
    }

    private func revealInEnclosingScrollView(_ rect: CGRect) {
        guard let scrollView = accessibilityRevealScrollView ?? ancestorScrollViews.first else { return }
        scrollView.scrollRectToVisible(convert(rect, to: scrollView), animated: false)
    }

    private func readableAccessibilityNodes(
        _ nodes: [ViewerTablePresentedAccessibilityNode]
    ) -> [ViewerTablePresentedAccessibilityNode] {
        nodes.map { presented in
            guard !linkInteractionsEnabled, presented.node.role == .link else { return presented }
            return ViewerTablePresentedAccessibilityNode(
                node: PreparedProseAccessibilityNode(
                    interactionIndex: nil,
                    role: .text,
                    label: presented.node.label,
                    rects: presented.rects,
                    sourceBlockIndex: presented.node.sourceBlockIndex
                ),
                sourceIdentity: presented.sourceIdentity,
                interactionSourceIdentity: nil,
                rects: presented.rects,
                clip: presented.clip,
                layout: presented.layout
            )
        }
    }

    fileprivate func accessibilityFrame(
        for node: ViewerTablePresentedAccessibilityNode
    ) -> CGRect {
        guard self.layout != nil else { return .zero }
        let rect = clippedAccessibilityRects(for: node).reduce(CGRect.null) { $0.union($1) }
        guard !rect.isNull, !rect.isEmpty else { return .zero }
        return accessibilityScreenFrame(rect, clip: .infinite)
    }

    fileprivate func accessibilityPath(
        for node: ViewerTablePresentedAccessibilityNode
    ) -> UIBezierPath? {
        let rects = clippedAccessibilityRects(for: node)
        guard self.layout != nil, let screen = window?.screen.coordinateSpace, !rects.isEmpty else { return nil }
        let path = UIBezierPath()
        for rect in rects {
            path.move(to: convert(CGPoint(x: rect.minX, y: rect.minY), to: screen))
            path.addLine(to: convert(CGPoint(x: rect.maxX, y: rect.minY), to: screen))
            path.addLine(to: convert(CGPoint(x: rect.maxX, y: rect.maxY), to: screen))
            path.addLine(to: convert(CGPoint(x: rect.minX, y: rect.maxY), to: screen))
            path.close()
        }
        return path
    }

    func activateAccessibilityNode(
        _ node: ViewerTablePresentedAccessibilityNode
    ) -> Bool {
        guard self.layout != nil,
              !clippedAccessibilityRects(for: node).isEmpty,
              let interactionIndex = node.node.interactionIndex,
              let interaction = node.layout.interactions[safe: interactionIndex]
        else { return false }
        return activate(interaction)
    }

    private func clippedAccessibilityRects(
        for node: ViewerTablePresentedAccessibilityNode
    ) -> [CGRect] {
        node.rects.compactMap { rect in
            let clipped = rect.intersection(node.clip)
            guard clipped.origin.x.isFinite, clipped.origin.y.isFinite,
                  clipped.width.isFinite, clipped.height.isFinite,
                  !clipped.isNull, !clipped.isEmpty
            else { return nil }
            return clipped
        }
    }

    private func invalidateAccessibilityNodes() {
        accessibilityPresentationGeneration &+= 1
        accessibilityItemsCache = nil
        guard window != nil else {
            accessibilityElementsByIndex.removeAll()
            return
        }
        guard !accessibilityElementsByIndex.isEmpty, !accessibilityAnnouncementScheduled else { return }
        accessibilityAnnouncementScheduled = true
        DispatchQueue.main.async { [weak self] in self?.announceAccessibilityChange() }
    }

    private func announceAccessibilityChange() {
        if window != nil { reconcileAccessibilityElementsIfNeeded() }
        let pending = pendingAccessibilityAnnouncement
        pendingAccessibilityAnnouncement = nil
        accessibilityAnnouncementScheduled = false
        guard window != nil, let announcement = pending else { return }
        let argument: Any?
        switch announcement {
        case .structure: argument = nil
        case let .content(element): argument = element
        }
        onAccessibilityLayoutChangedForTesting?(argument)
        UIAccessibility.post(notification: .layoutChanged, argument: argument)
    }

    /// Converts an artifact-top baseline to the flipped Core Graphics coordinate system.
    /// The view bounds, not the intrinsic artifact height, defines the flip origin.
    static func textPosition(
        baselineFromArtifactTop: CGFloat,
        in bounds: CGRect,
        artifactHeight _: CGFloat
    ) -> CGPoint {
        CGPoint(x: 0, y: bounds.height - baselineFromArtifactTop)
    }

    public override func draw(_ rect: CGRect) {
        let drawStarted = PreparedProseInstrumentation.now()
        guard let layout, let context = UIGraphicsGetCurrentContext(), !layout.blocks.isEmpty else { return }
        let viewport = presentationViewport()
        drawnPresentationWindow = viewport.window
        let snapshot = ViewerTablePresentation.project(layout: layout, owner: tablePresentationOwner, viewport: viewport)
        onMountedTableCellsDrawnForTesting?(snapshot.mountedCells.count)

        if usesEditAnchoredLayers {
            updateTableLayers(snapshot: snapshot)
        } else {
            paint(snapshot: snapshot, layout: layout, rect: rect, context: context)
        }
        PreparedProseInstrumentation.drew(drawStarted, visibleBlocks: snapshot.blocks.count)
    }

    private func paint(snapshot: ViewerTablePresentationSnapshot, layout: PreparedProseLayout,
                       rect: CGRect, context: CGContext, excludedRect: CGRect? = nil) {
        context.saveGState()
        defer { context.restoreGState() }
        if let content = layout.decorations.first, let box = content.styleBox {
            context.addPath(box.path(in: content.bounds).cgPath)
            context.clip()
        }
        let visibleCells = usesEditAnchoredLayers ? snapshot.mountedCells.filter { cell in
            let visible = cell.bounds.intersection(rect)
            return !visible.isNull && !visible.isEmpty && excludedRect?.contains(visible) != true
        } : snapshot.mountedCells
        let mountedLayoutIDs = Set(visibleCells.map { ObjectIdentifier($0.content) })
        let excludedLayoutID = excludedTableCellContentLayout.map(ObjectIdentifier.init)
        drawHierarchicalBackgrounds(snapshot, visibleCells: visibleCells, mountedLayoutIDs: mountedLayoutIDs, excludedLayoutID: excludedLayoutID, dirtyRect: rect, context: context)
        for remote in remoteTableCellSelections {
            for cell in visibleCells
            where cell.surface.identity == remote.tableID && isRealTableCell(cell, in: remote.sourceIndices) {
                fillTableCell(cell, color: remote.color, context: context)
            }
        }
        for cell in visibleCells where isSelectedTableCell(cell) {
            fillTableCell(cell, color: cell.surface.style.selectionColor, context: context)
        }
        if let target = tableCellDropTarget {
            for cell in visibleCells
            where cell.surface.identity == target.tableID && isRealTableCell(cell, in: [target.sourceIndex]) {
                fillTableCell(cell, color: cell.surface.style.selectionColor, context: context)
            }
        }
        context.saveGState()
        context.translateBy(x: 0, y: bounds.height)
        context.scaleBy(x: 1, y: -1)
        let scale = CGFloat(Double(bitPattern: layout.key.displayScaleBits))
        let visibleBlocks = snapshot.blocks.filter { presented in
            (presented.layout === layout || mountedLayoutIDs.contains(ObjectIdentifier(presented.layout))) &&
                ObjectIdentifier(presented.layout) != excludedLayoutID &&
                presented.block.bounds.offsetBy(dx: presented.origin.x, dy: presented.origin.y).intersects(rect)
        }
        // Keep paint phases global across the visible range: a nested code
        // background must never cover a quote border from an adjacent block.
        for presented in visibleBlocks {
            drawPresented(presented, context: context) { fragment in drawBackground(fragment, in: context) }
        }
        for cell in visibleCells {
            drawTableChromeBorder(cell, context: context)
        }
        for presented in visibleBlocks {
            drawPresented(presented, context: context) { fragment in drawBorderOrRule(fragment, in: context, scale: scale) }
        }
        for presented in visibleBlocks where presented.block.tableSurface?.layout.failure != nil {
            drawTableFailure(presented, context: context)
        }
        let attachmentsByBlock = Dictionary(
            uniqueKeysWithValues: snapshot.images.compactMap { image in
                image.block.map { (ObjectIdentifier($0), image.attachment) }
            }
        )
        for presented in visibleBlocks {
            let attachment = attachmentsByBlock[ObjectIdentifier(presented.block)]
            drawPresented(presented, context: context) { fragment in
                if presented.layout !== layout, fragment.kind == .text {
                    onTableRichFragmentDrawnForTesting?()
                }
                drawForeground(fragment, in: context, attachment: attachment)
            }
        }
        context.restoreGState()
        if let visible = configuredVisibleRect() {
            context.saveGState()
            context.clip(to: visible)
            for handle in selectionHandles(visibleIn: visible) {
                context.saveGState()
                context.clip(to: handle.clip)
                let radius = TableHandleMetrics.radius
                let circle = CGRect(x: handle.center.x - radius, y: handle.center.y - radius,
                                    width: radius * 2, height: radius * 2)
                context.setFillColor(handle.color.cgColor)
                context.fillEllipse(in: circle)
                context.restoreGState()
            }
            if let edge = activeTableResizeEdge,
               let table = rootTable(edge.tableID, in: snapshot),
               let x = columnTrailingEdgeX(for: edge) {
                context.saveGState()
                context.clip(to: table.clip)
                context.setFillColor(table.surface.style.resizeHandleColor.cgColor)
                context.fill(CGRect(x: x - TableResizeMetrics.indicatorWidth / 2, y: table.bounds.minY,
                                    width: TableResizeMetrics.indicatorWidth, height: table.bounds.height))
                context.restoreGState()
            }
            context.restoreGState()
        }
    }

    private func alignedTableLayerRect(_ rect: CGRect, scale: CGFloat) -> CGRect {
        guard !rect.isNull, !rect.isEmpty else { return rect }
        let minX = ((rect.minX - bounds.minX) * scale).rounded()
        let maxX = ((rect.maxX - bounds.minX) * scale).rounded()
        let minY = ((rect.minY - bounds.minY) * scale).rounded()
        let maxY = ((rect.maxY - bounds.minY) * scale).rounded()
        return CGRect(x: bounds.minX + minX / scale, y: bounds.minY + minY / scale,
                      width: (maxX - minX) / scale, height: (maxY - minY) / scale)
    }

    private func recordTableLayer(_ target: CALayer, name: TableLayerName, rect proposedRect: CGRect,
                                  excluding excluded: CGRect? = nil,
                                  snapshot: ViewerTablePresentationSnapshot, layout: PreparedProseLayout) {
        let scale = CGFloat(Double(bitPattern: layout.key.displayScaleBits))
        let rect = alignedTableLayerRect(proposedRect, scale: scale)
        let excluded = excluded.map { alignedTableLayerRect($0, scale: scale) }
        guard !rect.isNull, !rect.isEmpty else {
            target.isHidden = true
            target.contents = nil
            return
        }
        let format = UIGraphicsImageRendererFormat()
        format.scale = scale
        format.opaque = false
        let image = UIGraphicsImageRenderer(bounds: rect, format: format).image { renderer in
            let context = renderer.cgContext
            context.clip(to: rect)
            if let excluded, rect.intersects(excluded) {
                context.addRect(rect)
                context.addRect(excluded.intersection(rect))
                context.clip(using: .evenOdd)
            }
            paint(snapshot: snapshot, layout: layout, rect: rect, context: context, excludedRect: excluded)
        }
        target.contentsScale = scale
        target.contents = image.cgImage
        target.frame = rect
        target.isHidden = false
        layerRedrawsForTesting[name.rawValue, default: 0] += 1
    }

    private func boundCellChrome(_ presented: ViewerTablePresentedCell, layout: PreparedProseLayout) -> BoundCellChrome? {
        guard excludedTableCellContentLayout === presented.content, presented.cell.isPositionFree,
              presented.surface.layout.failure == nil, layout.decorations.isEmpty,
              layout.blocks.count == 1, let block = layout.blocks.first,
              block.tableSurface === presented.surface, block.fragments.isEmpty,
              block.atomSlot == nil, block.imageAttachment == nil,
              selectedTableCellSourceIndices.isEmpty, selectedTableCellEndpoints == nil,
              remoteTableCellSelections.isEmpty, tableCellDropTarget == nil, activeTableResizeEdge == nil
        else { return nil }
        let cell = presented.cell
        let style = presented.surface.style
        return BoundCellChrome(row: cell.row, column: cell.column, rowspan: cell.rowspan, colspan: cell.colspan,
            isHeader: cell.isHeader, attributesKey: cell.attributesKey, clip: presented.clip,
            drawingBounds: bounds, displayScaleBits: layout.key.displayScaleBits,
            borderWidth: style.borderWidth, borderColor: style.borderColor.resolvedColor(with: traitCollection),
            headerBackgroundColor: style.headerBackgroundColor.resolvedColor(with: traitCollection))
    }

    private func updateTableLayers(snapshot: ViewerTablePresentationSnapshot) {
        guard let layout else { clearTableLayers(); return }
        let window = configuredVisibleRect() ?? bounds
        guard !window.isEmpty, !window.isNull else { clearTableLayers(); return }
        CATransaction.begin()
        CATransaction.setDisableActions(true)
        defer { CATransaction.commit() }
        for child in tableLayers where child.superlayer !== layer { layer.addSublayer(child) }
        layer.masksToBounds = true
        let presented = tableLayerCell.flatMap { binding in
            snapshot.tables.first { $0.surface.identity == binding.tableID }.flatMap { table in
                table.surface.cell(sourceIndex: binding.sourceIndex).map {
                    ViewerTablePresentation.present($0, in: table, owner: tablePresentationOwner)
                }
            }
        }
        guard let binding = tableLayerCell, let presented else {
            if tableLayerState?.cell != nil || tableLayerState?.window != window ||
                tableLayerState?.revision != tableLayerRevision || tableLayerState?.appearance != tableLayerAppearance {
                clearTableLayers()
                recordTableLayer(aboveLayer, name: .above, rect: window, snapshot: snapshot, layout: layout)
            }
            tableLayerState = TableLayerState(cell: nil, window: window, frame: .zero, row: .zero, rowOffsets: [],
                                             revision: tableLayerRevision, appearance: tableLayerAppearance,
                                             excludesContent: false, chrome: nil)
            return
        }
        let frame = presented.bounds
        var firstRow = presented.cell.row
        var lastRow = firstRow + presented.cell.rowspan
        let cells = snapshot.mountedCells.filter { $0.surface.identity == binding.tableID }
        while true {
            let previous = firstRow..<lastRow
            for cell in cells where cell.cell.row < lastRow && cell.cell.row + cell.cell.rowspan > firstRow {
                firstRow = min(firstRow, cell.cell.row)
                lastRow = max(lastRow, cell.cell.row + cell.cell.rowspan)
            }
            if previous == firstRow..<lastRow { break }
        }
        let offsets = presented.surface.layout.rowOffsets
        let originY = frame.minY - offsets[presented.cell.row]
        let row = CGRect(x: window.minX, y: originY + offsets[firstRow], width: window.width,
                         height: offsets[lastRow] - offsets[firstRow])
        let rowOffsets = offsets[firstRow...lastRow].map { $0 - offsets[firstRow] }
        let old = tableLayerState
        let excludesContent = excludedTableCellContentLayout === presented.content
        let chrome = boundCellChrome(presented, layout: layout)
        let unchangedChrome = chrome != nil && old?.chrome == chrome && old?.row == row && old?.rowOffsets == rowOffsets
        let changedRevision = old?.revision != tableLayerRevision
        let changesAreBoundCellOnly = tableLayerChanges.map { changes in
            !changes.fullReset && changes.replacedTables.isEmpty && changes.removedTables.isEmpty &&
                changes.changedCells.allSatisfy { key, cells in
                    key == binding.tableID && cells.isSubset(of: IndexSet(integer: binding.sourceIndex))
                }
        } ?? false
        let reusable = old?.cell == binding && old?.window == window &&
            old?.appearance == tableLayerAppearance && old?.row.minY == row.minY &&
            old?.frame.minX == frame.minX && old?.frame.width == frame.width &&
            (!changedRevision || changesAreBoundCellOnly)
        let belowRect = CGRect(x: window.minX, y: max(row.maxY, window.minY), width: window.width,
                               height: max(0, window.maxY - max(row.maxY, window.minY)))
        if !reusable {
            clearTableLayers()
            recordTableLayer(aboveLayer, name: .above,
                rect: CGRect(x: window.minX, y: window.minY, width: window.width,
                             height: max(0, min(row.minY, window.maxY) - window.minY)), snapshot: snapshot, layout: layout)
            recordTableLayer(belowLayer, name: .below, rect: belowRect, snapshot: snapshot, layout: layout)
        } else if let old, old.row.height != row.height {
            let delta = row.height - old.row.height
            let scale = CGFloat(Double(bitPattern: layout.key.displayScaleBits))
            let rasterDelta = (delta * scale).rounded() / scale
            let fractionalTranslation = abs(delta - rasterDelta) > max(row.maxY.ulp, old.row.maxY.ulp)
            belowLayer.position.y += rasterDelta
            let required = alignedTableLayerRect(belowRect, scale: scale)
            let coverage = alignedTableLayerRect(belowLayer.frame, scale: scale)
            if fractionalTranslation || coverage.minY > required.minY || coverage.maxY < required.maxY {
                recordTableLayer(belowLayer, name: .below, rect: belowRect, snapshot: snapshot, layout: layout)
            }
        }
        if !reusable || old?.rowOffsets != rowOffsets {
            recordTableLayer(boundRowLayer, name: .boundRow, rect: row.intersection(window),
                             excluding: frame, snapshot: snapshot, layout: layout)
        }
        if !reusable || old?.frame != frame || old?.excludesContent != excludesContent || old?.chrome != chrome ||
            old?.row != row || old?.rowOffsets != rowOffsets || (changedRevision && !unchangedChrome) {
            recordTableLayer(boundCellLayer, name: .boundCell, rect: frame.intersection(window),
                             snapshot: snapshot, layout: layout)
        }
        tableLayerState = TableLayerState(cell: binding, window: window, frame: frame, row: row, rowOffsets: rowOffsets,
                                         revision: tableLayerRevision, appearance: tableLayerAppearance,
                                         excludesContent: excludesContent, chrome: chrome)
    }

    private func fillTableCell(_ cell: ViewerTablePresentedCell, color: UIColor, context: CGContext) {
        guard !cell.clip.isNull, !cell.clip.isEmpty else { return }
        context.saveGState()
        context.clip(to: cell.clip)
        context.setFillColor(color.cgColor)
        context.fill(cell.bounds)
        context.restoreGState()
    }

    private func drawTableFailure(_ presented: ViewerTablePresentedBlock, context: CGContext) {
        guard !presented.clip.isNull, !presented.clip.isEmpty,
              let tableBounds = presented.block.tableBounds else { return }
        context.saveGState()
        context.clip(to: flipped(presented.clip))
        context.translateBy(x: presented.origin.x, y: -presented.origin.y)
        let rect = CGRect(x: tableBounds.minX, y: bounds.height - tableBounds.maxY, width: tableBounds.width, height: tableBounds.height)
        context.setFillColor(UIColor.systemRed.withAlphaComponent(0.18).cgColor)
        context.fill(rect)
        context.setStrokeColor(UIColor.systemRed.cgColor)
        context.setLineWidth(1)
        context.stroke(rect)
        context.restoreGState()
    }

    private func drawHierarchicalBackgrounds(
        _ snapshot: ViewerTablePresentationSnapshot,
        visibleCells: [ViewerTablePresentedCell],
        mountedLayoutIDs: Set<ObjectIdentifier>,
        excludedLayoutID: ObjectIdentifier?,
        dirtyRect: CGRect,
        context: CGContext
    ) {
        let layouts = Dictionary(uniqueKeysWithValues: snapshot.layouts.map { (ObjectIdentifier($0.layout), $0) })
        let blocks = Dictionary(grouping: snapshot.blocks, by: { ObjectIdentifier($0.layout) })
        let cells = Dictionary(grouping: visibleCells, by: { ObjectIdentifier($0.surface) })

        func drawLayout(_ presented: ViewerTablePresentedLayout) {
            guard !presented.clip.isNull, !presented.clip.isEmpty else { return }
            if ObjectIdentifier(presented.layout) != excludedLayoutID {
                context.saveGState()
                context.clip(to: presented.clip)
                context.translateBy(x: presented.origin.x, y: presented.origin.y)
                for fragment in presented.layout.decorations {
                    guard fragment.bounds.offsetBy(dx: presented.origin.x, dy: presented.origin.y).intersects(dirtyRect) else { continue }
                    fragment.styleBox?.draw(in: fragment.bounds, context: context)
                }
                context.restoreGState()
            }

            for block in blocks[ObjectIdentifier(presented.layout)] ?? [] {
                guard let surface = block.block.tableSurface else { continue }
                let surfaceCells = cells[ObjectIdentifier(surface)] ?? []
                context.saveGState()
                context.setFillColor(surface.style.headerBackgroundColor.resolvedColor(with: traitCollection).cgColor)
                for cell in surfaceCells where cell.cell.isHeader {
                    let rect = cell.bounds.intersection(cell.clip)
                    guard !rect.isNull, !rect.isEmpty else { continue }
                    context.addRect(rect)
                }
                // One fill keeps shared fractional edges opaque during scrolling.
                context.fillPath()
                context.restoreGState()
                for cell in surfaceCells {
                    guard let child = layouts[ObjectIdentifier(cell.content)],
                          mountedLayoutIDs.contains(ObjectIdentifier(cell.content))
                    else { continue }
                    drawLayout(child)
                }
            }
        }

        guard let root = snapshot.layouts.first else { return }
        drawLayout(root)
    }

    private func drawTableChromeBorder(_ cell: ViewerTablePresentedCell, context: CGContext) {
        guard !cell.clip.isNull, !cell.clip.isEmpty else { return }
        onTableChromeDrawnForTesting?(cell.sourceIndex)
        context.saveGState()
        context.clip(to: flipped(cell.clip))
        let rect = flipped(cell.bounds).insetBy(dx: cell.surface.style.borderWidth / 2, dy: cell.surface.style.borderWidth / 2)
        context.setStrokeColor(cell.surface.style.borderColor.cgColor)
        context.setLineWidth(cell.surface.style.borderWidth)
        context.stroke(rect)
        context.restoreGState()
    }

    private func flipped(_ rect: CGRect) -> CGRect {
        CGRect(x: rect.minX, y: bounds.height - rect.maxY, width: rect.width, height: rect.height)
    }

    private func drawPresented(
        _ presented: ViewerTablePresentedBlock,
        context: CGContext,
        draw: (PreparedProseFragment) -> Void
    ) {
        guard !presented.clip.isNull, !presented.clip.isEmpty else { return }
        context.saveGState()
        context.clip(to: flipped(presented.clip))
        context.translateBy(x: presented.origin.x, y: -presented.origin.y)
        presented.block.fragments.forEach(draw)
        context.restoreGState()
    }

    private func drawingRect(for fragment: PreparedProseFragment) -> CGRect {
        CGRect(
            x: fragment.bounds.minX,
            y: bounds.height - fragment.bounds.maxY,
            width: fragment.bounds.width,
            height: fragment.bounds.height
        )
    }

    private func drawBackground(_ fragment: PreparedProseFragment, in context: CGContext) {
        guard fragment.kind == .background || fragment.kind == .atom || fragment.kind == .image else { return }
        let rect = drawingRect(for: fragment)
        if let box = fragment.styleBox {
            context.saveGState()
            context.translateBy(x: 0, y: bounds.height)
            context.scaleBy(x: 1, y: -1)
            box.draw(in: fragment.bounds, context: context)
            context.restoreGState()
            return
        }
        context.setFillColor(fragment.color ?? UIColor.clear.cgColor)
        context.addPath(UIBezierPath(roundedRect: rect, cornerRadius: fragment.cornerRadius).cgPath)
        context.drawPath(using: .fill)
    }

    private func drawBorderOrRule(_ fragment: PreparedProseFragment, in context: CGContext, scale: CGFloat) {
        let rect = drawingRect(for: fragment)
        switch fragment.kind {
        case .border:
            context.setFillColor(fragment.color ?? UIColor.clear.cgColor)
            context.fill(rect)
        case .rule:
            let unit = scale.isFinite && scale > 0 ? 1 / scale : 1
            let alignedY = (rect.minY / unit).rounded() * unit
            context.setFillColor(fragment.color ?? UIColor.clear.cgColor)
            context.fill(CGRect(x: rect.minX, y: alignedY, width: rect.width, height: max(unit, rect.height)))
        case .atom where fragment.strokeWidth > 0:
            context.setStrokeColor(fragment.borderColor ?? fragment.color ?? UIColor.clear.cgColor)
            context.setLineWidth(fragment.strokeWidth)
            let inset = fragment.strokeWidth / 2
            context.addPath(
                UIBezierPath(
                    roundedRect: rect.insetBy(dx: inset, dy: inset),
                    cornerRadius: max(0, fragment.cornerRadius - inset)
                ).cgPath
            )
            context.drawPath(using: .stroke)
        default:
            break
        }
    }

    private func drawForeground(_ fragment: PreparedProseFragment, in context: CGContext, attachment presentedAttachment: ViewerImageAttachment? = nil) {
        let rect = CGRect(
            x: fragment.bounds.minX,
            y: bounds.height - fragment.bounds.maxY,
            width: fragment.bounds.width,
            height: fragment.bounds.height
        )
        switch fragment.kind {
        case .text:
            guard let line = fragment.line else { return }
            context.textPosition = CGPoint(x: fragment.origin.x, y: bounds.height - fragment.origin.y)
            CTLineDraw(line, context)
        case .atom:
            guard let line = fragment.line else { return }
            context.textPosition = CGPoint(x: fragment.origin.x, y: bounds.height - fragment.origin.y)
            CTLineDraw(line, context)
        case .image:
            guard let attachment = presentedAttachment ?? layout?.imageAttachments.first(where: { $0.bounds == fragment.bounds }),
                  let image = imagePixels[attachment.id] else { return }
            context.saveGState()
            context.translateBy(x: rect.minX, y: rect.maxY)
            context.scaleBy(x: 1, y: -1)
            let localBounds = CGRect(origin: .zero, size: rect.size)
            if let box = fragment.styleBox {
                context.addPath(box.path(in: localBounds, inner: true).cgPath)
                context.clip()
                context.clip(to: localBounds.inset(by: box.inset))
                image.draw(in: box.imageRect(image.size, in: localBounds))
            } else { image.draw(in: localBounds) }
            context.restoreGState()
        case .marker:
            if fragment.label == "•", fragment.line == nil {
                context.setFillColor(fragment.color ?? UIColor.label.cgColor)
                context.fillEllipse(in: rect)
            } else if let line = fragment.line {
                context.textPosition = CGPoint(x: fragment.origin.x, y: bounds.height - fragment.origin.y)
                CTLineDraw(line, context)
            } else if let box = fragment.styleBox {
                context.saveGState()
                context.translateBy(x: 0, y: bounds.height)
                context.scaleBy(x: 1, y: -1)
                EditorStyleSheet.drawCheckbox(box, in: fragment.bounds, checked: fragment.checked, context: context)
                context.restoreGState()
            } else {
                drawTaskMarker(in: rect, checked: fragment.checked, color: UIColor(cgColor: fragment.color ?? UIColor.label.cgColor))
            }
        case .strike:
            context.setFillColor(fragment.color ?? UIColor.clear.cgColor)
            context.addPath(UIBezierPath(roundedRect: rect, cornerRadius: fragment.cornerRadius).cgPath)
            context.fillPath()
        case .background, .border, .rule:
            break
        }
    }

    private func drawTaskMarker(in rect: CGRect, checked: Bool, color: UIColor) {
        let inset = max(1, rect.height * 0.2)
        let box = CGRect(x: rect.minX + inset, y: rect.minY + inset, width: rect.height - inset * 2, height: rect.height - inset * 2)
        let path = UIBezierPath(roundedRect: box, cornerRadius: box.width * 0.2)
        color.setStroke()
        path.lineWidth = max(1, box.width * 0.1)
        path.stroke()
        guard checked else { return }
        let check = UIBezierPath()
        check.move(to: CGPoint(x: box.minX + box.width * 0.2, y: box.midY))
        check.addLine(to: CGPoint(x: box.minX + box.width * 0.43, y: box.maxY - box.height * 0.2))
        check.addLine(to: CGPoint(x: box.maxX - box.width * 0.16, y: box.minY + box.height * 0.2))
        check.lineCapStyle = .round
        check.lineJoinStyle = .round
        check.lineWidth = max(1.4, box.width * 0.12)
        color.setStroke()
        check.stroke()
    }
}

private final class PreparedProseDrawingAccessibilityElement: UIAccessibilityElement {
    weak var drawingView: PreparedProseDrawingView?
    var presented: ViewerTablePresentedAccessibilityNode

    init(container: PreparedProseDrawingView, presented: ViewerTablePresentedAccessibilityNode) {
        drawingView = container
        self.presented = presented
        super.init(accessibilityContainer: container)
    }

    private var isCurrent: Bool { drawingView?.isLiveAccessibilityElement(self) == true }

    override var accessibilityLabel: String? {
        get {
            drawingView?.reconcileAccessibilityElementsIfNeeded()
            return presented.node.label
        }
        set { }
    }
    override var accessibilityTraits: UIAccessibilityTraits {
        get {
            switch presented.node.role {
            case .text, .separator:
                return .staticText
            case .heading:
                return [.staticText, .header]
            case .link:
                return .link
            case .mention:
                return .button
            case .image:
                return .image
            }
        }
        set { }
    }
    override var accessibilityFrame: CGRect {
        get {
            guard isCurrent, let drawingView else { return .zero }
            return drawingView.accessibilityFrame(for: presented)
        }
        set { }
    }
    override var accessibilityPath: UIBezierPath? {
        get {
            guard isCurrent, let drawingView else { return nil }
            return drawingView.accessibilityPath(for: presented)
        }
        set { }
    }
    override func accessibilityActivate() -> Bool {
        guard isCurrent, let drawingView else { return false }
        return drawingView.activateAccessibilityNode(presented)
    }
}

private extension Array {
    subscript(safe index: Int) -> Element? { indices.contains(index) ? self[index] : nil }
}
