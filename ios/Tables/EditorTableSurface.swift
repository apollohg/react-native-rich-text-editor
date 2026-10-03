import UIKit

final class TableSelectionHandleGestureRecognizer: UIGestureRecognizer {
    private var primaryTouch: UITouch?

    override func touchesBegan(_ touches: Set<UITouch>, with event: UIEvent) {
        guard primaryTouch == nil, touches.count == 1,
              event.allTouches?.count == 1, let touch = touches.first
        else { state = .cancelled; return }
        primaryTouch = touch
        state = .began
    }

    override func touchesMoved(_ touches: Set<UITouch>, with event: UIEvent) {
        guard let primaryTouch, touches.contains(primaryTouch),
              event.allTouches?.count == 1
        else { state = .cancelled; return }
        state = .changed
    }

    override func touchesEnded(_ touches: Set<UITouch>, with event: UIEvent) {
        state = primaryTouch.map(touches.contains) == true ? .ended : .cancelled
    }

    override func touchesCancelled(_ touches: Set<UITouch>, with event: UIEvent) {
        state = .cancelled
    }

    override func reset() {
        super.reset()
        primaryTouch = nil
    }
}

struct TableSelectionObstructions: Equatable {
    let safeArea: CGRect
    let keyboard: CGRect?
}

struct TableSelectionGeometry: Equatable {
    static let coordinateSpace = "window"

    let editorId: UInt64
    let documentRevision: UInt64
    let layoutEpoch: UInt64
    let tablePos: UInt32
    let rects: [CGRect]
    let viewport: CGRect
    let obstructions: TableSelectionObstructions
    let editMenuVisible: Bool

    var eventPayload: [String: Any] {
        var payload: [String: Any] = [
            "documentRevision": String(documentRevision),
            "layoutEpoch": String(layoutEpoch),
            "tablePos": Int(tablePos),
            "coordinateSpace": Self.coordinateSpace,
            "rects": rects.map(Self.rectPayload),
            "viewport": Self.rectPayload(viewport),
            "safeArea": Self.rectPayload(obstructions.safeArea),
            "editMenuVisible": editMenuVisible
        ]
        if let keyboard = obstructions.keyboard {
            payload["keyboard"] = Self.rectPayload(keyboard)
        }
        return payload
    }

    private static func rectPayload(_ rect: CGRect) -> [String: Double] {
        ["x": Double(rect.minX), "y": Double(rect.minY), "width": Double(rect.width), "height": Double(rect.height)]
    }
}

struct TableResizePreview: Equatable {
    let edge: TableResizeEdge
    let width: CGFloat
}

final class EditorTableSurface: UIView, UIGestureRecognizerDelegate {
    enum ArrowDestination {
        case cell(UInt32)
        case surroundingProse
        case blocked
    }
    struct RootTableCellHit: Equatable {
        let tableID: String
        let cellIndex: UInt32
        let contentRect: CGRect
    }

    private struct Entry {
        let tableID: String
        let surface: ViewerTableSurface
        let localTableBounds: CGRect
        let occupiedHeight: CGFloat
        let themeDigest: String
    }

    private struct ReusableCellKey: Hashable {
        let contentKey: String
        let header: Bool
        let attributesKey: String
        let widthPixels: Int
        let displayScaleBits: UInt64
    }

    private struct ReusableCellContents {

        private var contents: [ReusableCellKey: [PreparedViewerTableCell]] = [:]

        init(_ entry: Entry?, themeDigest: String) {
            guard let entry, entry.themeDigest == themeDigest, let source = entry.surface.sourceTable else { return }
            for cell in entry.surface.cells.reversed() {
                let index = cell.sourceIndex
                guard source.cells.indices.contains(index), cell.isPositionFree else { continue }
                let sourceCell = source.cells[index]
                let key = ReusableCellKey(
                    contentKey: sourceCell.contentKey,
                    header: sourceCell.header,
                    attributesKey: sourceCell.attrsKey,
                    widthPixels: cell.contentKey.widthPixels,
                    displayScaleBits: cell.contentKey.displayScaleBits
                )
                contents[key, default: []].append(cell)
            }
        }

        mutating func take(_ cell: TableSurfaceCell, widthPixels: Int, displayScale: CGFloat) -> PreparedViewerTableCell? {
            let key = ReusableCellKey(
                contentKey: cell.contentKey,
                header: cell.header,
                attributesKey: cell.attrsKey,
                widthPixels: widthPixels,
                displayScaleBits: Double(displayScale).bitPattern
            )
            return contents[key]?.popLast()
        }
    }

    let inputCoordinator: EditorTableInputCoordinator
    private let drawingView = PreparedProseDrawingView(frame: .zero)
    private let activeCellClipView = UIView(frame: .zero)
    private var entries: [String: Entry] = [:]
    private(set) var incrementalRelayoutsForTesting = 0
    private(set) var seededShapeLayoutsForTesting = 0
    private var latestPresentation: EditorV2Adapter.EditorTablePresentationSnapshot?
    private var pinnedInputCell: PreparedViewerTableCell?
    var presentedDocumentRevision: UInt64? { presentationRevision }
    private var presentationRevision: UInt64?
    private var preparedWidth: CGFloat = 0
    private var appearanceRevision: UInt64 = 0
    var hostTableDirection: TableLayoutDirection? {
        didSet {
            guard oldValue != hostTableDirection else { return }
            invalidateAppearance()
        }
    }
    private var preparedAppearanceRevision: UInt64?
    private var mountedTableFrames: [String: CGRect] = [:]
    private var mountedSurfaces: [String: ViewerTableSurface] = [:]
    private var mountedCanvasSize = CGSize.zero
    private var drawingOffset = CGPoint.zero
    private var activeCell: (tableID: String, cellIndex: UInt32)?
    private var activeInputRevealScheduled = false
    private(set) weak var interactionHost: RichTextEditorView?
    private lazy var selectionGesture: TableSelectionHandleGestureRecognizer = {
        let recognizer = TableSelectionHandleGestureRecognizer(target: self, action: #selector(handleSelectionGesture(_:)))
        recognizer.delegate = self
        recognizer.cancelsTouchesInView = true
        return recognizer
    }()
    private lazy var textSelectionGesture: TableTextSelectionGestureRecognizer = {
        let recognizer = TableTextSelectionGestureRecognizer(target: self, action: #selector(handleTextSelectionGesture(_:)))
        recognizer.delegate = self
        recognizer.crossesCellBoundary = { [weak self] in self?.textSelectionDragTarget(at: $0) != nil }
        recognizer.trackingDidReset = { [weak self] in self?.textSelectionOrigin = nil }
        return recognizer
    }()
    private struct TextSelectionOrigin {
        let adapter: EditorV2Adapter
        let admission: EditorV2Adapter.TableMutationAdmission
        let cellIndex: UInt32
        let role: TableSelectionHandleRole
    }
    private var textSelectionOrigin: TextSelectionOrigin?

    private lazy var cellDragInteraction: UIDragInteraction = {
        let interaction = UIDragInteraction(delegate: self)
        interaction.isEnabled = true
        return interaction
    }()
    private lazy var resizeGesture: TableHorizontalPanGestureRecognizer = {
        let recognizer = TableHorizontalPanGestureRecognizer(target: self, action: #selector(handleResizeGesture(_:)))
        recognizer.maximumNumberOfTouches = 1
        recognizer.delegate = self
        recognizer.cancelsTouchesInView = true
        return recognizer
    }()
    private var resizeTouchPoint = CGPoint.zero
    private class DragSession {
        let adapter: EditorV2Adapter
        let tableID: String
        var windowPoint: CGPoint

        init(adapter: EditorV2Adapter, tableID: String, windowPoint: CGPoint) {
            self.adapter = adapter
            self.tableID = tableID
            self.windowPoint = windowPoint
        }
    }
    private final class HandleDrag: DragSession {
        var admission: EditorV2Adapter.TableCellSelectionAdmission
        let role: TableSelectionHandleRole
        let touchOffset: CGPoint

        init(
            adapter: EditorV2Adapter,
            admission: EditorV2Adapter.TableCellSelectionAdmission,
            role: TableSelectionHandleRole,
            touchOffset: CGPoint,
            windowPoint: CGPoint
        ) {
            self.admission = admission
            self.role = role
            self.touchOffset = touchOffset
            super.init(adapter: adapter, tableID: admission.tableID, windowPoint: windowPoint)
        }
    }
    private final class ResizeDrag: DragSession {
        let admission: EditorV2Adapter.TableMutationAdmission
        let edge: TableResizeEdge
        let startWidth: CGFloat
        let startX: CGFloat
        let minimumWidth: CGFloat
        let directionSign: CGFloat
        var scrolledLogical: CGFloat = 0
        var previewWidth: CGFloat

        init(
            adapter: EditorV2Adapter,
            admission: EditorV2Adapter.TableMutationAdmission,
            hit: TableResizeEdgeHit,
            startX: CGFloat,
            windowPoint: CGPoint
        ) {
            self.admission = admission
            self.edge = hit.edge
            self.startWidth = hit.columnWidth
            self.startX = startX
            self.minimumWidth = hit.minimumColumnWidth
            self.directionSign = hit.rightToLeft ? -1 : 1
            self.previewWidth = hit.columnWidth
            super.init(adapter: adapter, tableID: hit.edge.tableID, windowPoint: windowPoint)
        }

        func clampedWidth(_ requested: CGFloat) -> CGFloat {
            min(ResizeMetrics.maximumColumnWidth, max(minimumWidth.rounded(.up), requested.rounded()))
        }
    }
    private var activeDrag: DragSession?
    private var handleDrag: HandleDrag? { activeDrag as? HandleDrag }
    private var resizeDrag: ResizeDrag? { activeDrag as? ResizeDrag }
    private var dragFrameLink: CADisplayLink?
    private var runningDragFrame = false
    private var submittingHandleSelection = false
    private(set) var resizePreview: TableResizePreview?
    private var preparedResizePreview: TableResizePreview?
    var onSelectionGeometryMayChange: (() -> Void)?
    var onTableCellPreparedForTesting: ((Int, String) -> Void)?
    private lazy var cellEditMenu = TableCellEditMenu(
        anchor: { [weak self] in self?.cellEditMenuAnchor() },
        visibilityChanged: { [weak self] in self?.onSelectionGeometryMayChange?() },
        tableActions: { [weak self] in self?.cellEditMenuActions() ?? [] }
    )
    private var cellEditMenuEndpoints: TableSelectionEndpoints?
    private var cellEditMenuDocumentRevision: UInt64?
    private var accessibilityDocumentRevision: UInt64?
    private var accessibilityUnanchoredTables: Set<String> = []
    var isCellEditMenuVisible: Bool { cellEditMenu.isVisible }

    private enum HandleScrollMetrics {
        static let edgeBand: CGFloat = 36
        static let stepPerFrame: CGFloat = 6
    }

    private enum ResizeMetrics {
        static let maximumColumnWidth: CGFloat = 10_000
    }

    init(inputCoordinator: EditorTableInputCoordinator) {
        self.inputCoordinator = inputCoordinator
        super.init(frame: .zero)
        clipsToBounds = true
        drawingView.usesEditAnchoredLayers = true
        drawingView.preparesTableCellsOnViewportChange = true
        drawingView.isOpaque = false
        drawingView.backgroundColor = .clear
        drawingView.isUserInteractionEnabled = false
        addSubview(drawingView)
        activeCellClipView.clipsToBounds = true
        addSubview(activeCellClipView)
        inputCoordinator.cellInput.isHidden = true
        inputCoordinator.cellInput.onSelectionOrContentMayChange = { [weak self] in
            self?.scheduleActiveInputReveal()
        }
        for name in [UIResponder.keyboardWillChangeFrameNotification, UIResponder.keyboardWillHideNotification] {
            NotificationCenter.default.addObserver(self, selector: #selector(scheduleActiveInputReveal), name: name, object: nil)
        }
        activeCellClipView.addSubview(inputCoordinator.cellInput)
        drawingView.onTableGeometryChanged = { [weak self] in
            self?.refreshActiveInputFrame()
            self?.selectionGeometryMayChange()
        }
        drawingView.tableAccessibilityEditing = self
        drawingView.tableDocumentPosition = { [weak self] tableID in
            self?.latestPresentation?.index.tableDocStart(tableKey: tableID)
        }
        drawingView.tableCellDocumentPosition = { [weak self] tableID, sourceIndex in
            self?.cellDocumentPosition(tableID: tableID, sourceIndex: sourceIndex)
        }
    }

    func cellDocumentPosition(tableID: String, sourceIndex: Int) -> UInt32? {
        latestPresentation?.index.docStart(tableKey: tableID, cellIndex: sourceIndex)
    }

    override var accessibilityElements: [Any]? {
        get { [drawingView] }
        set { }
    }

    required init?(coder: NSCoder) {
        fatalError("init(coder:) has not been implemented")
    }

    deinit {
        NotificationCenter.default.removeObserver(self)
        if let pinnedInputCell { pinnedInputCell.layoutStore.unpin(pinnedInputCell.contentKey) }
        discardActiveDrag()
        selectionGesture.view?.removeGestureRecognizer(selectionGesture)
        textSelectionGesture.view?.removeGestureRecognizer(textSelectionGesture)
        resizeGesture.view?.removeGestureRecognizer(resizeGesture)
        cellEditMenu.interaction.view?.removeInteraction(cellEditMenu.interaction)
        cellDragInteraction.view?.removeInteraction(cellDragInteraction)
    }

    override func didMoveToWindow() {
        super.didMoveToWindow()
        guard window == nil else { return }
        discardActiveDrag()
        dismissCellEditMenu()
    }

    func installTableInteraction(on host: UIView) {
        interactionHost = host as? RichTextEditorView
        host.addGestureRecognizer(selectionGesture)
        host.addGestureRecognizer(textSelectionGesture)
        host.addGestureRecognizer(resizeGesture)
        interactionHost?.textView.addInteraction(cellEditMenu.interaction)
        interactionHost?.textView.addInteraction(cellDragInteraction)
        drawingView.installTableInteraction(on: host)
    }

    func present(
        _ presentation: EditorV2Adapter.EditorTablePresentationSnapshot,
        selection: EditorCellSelection?,
        endpoints: (anchor: UInt32, head: UInt32)?,
        ownerIdentity: String,
        from textView: EditorTextView
    ) {
        defer { selectionGeometryMayChange() }
        drawingView.setTableOwnerIdentity(ownerIdentity)
        latestPresentation = presentation
        let unanchored = unanchoredTableIDs()
        if accessibilityDocumentRevision != presentation.documentRevision || accessibilityUnanchoredTables != unanchored {
            accessibilityDocumentRevision = presentation.documentRevision
            accessibilityUnanchoredTables = unanchored
            drawingView.invalidateTableAccessibility()
        }
        if let drag = resizeDrag, !validResizeDrag(drag) { discardActiveDrag() }
        if case let .drawable(tableID, sourceIndices) = selection {
            drawingView.selectedTableCellSourceIndices = [tableID: sourceIndices]
            drawingView.selectedTableCellEndpoints = endpoints.map {
                TableSelectionEndpoints(tableID: tableID, anchor: $0.anchor, head: $0.head)
            }
        } else {
            drawingView.selectedTableCellSourceIndices = [:]
            drawingView.selectedTableCellEndpoints = nil
        }
        reprepareIfNeeded(from: textView)
        updateGeometry(from: textView)
        discardInvalidDrag()
    }

    func invalidateAppearance() {
        appearanceRevision &+= 1
    }

    func clearPresentation() {
        defer { selectionGeometryMayChange() }
        discardActiveDrag()
        accessibilityDocumentRevision = nil
        accessibilityUnanchoredTables = []
        drawingView.setTableOwnerIdentity(nil)
        entries.removeAll()
        latestPresentation = nil
        if let pinnedInputCell { pinnedInputCell.layoutStore.unpin(pinnedInputCell.contentKey) }
        pinnedInputCell = nil
        presentationRevision = nil
        preparedWidth = 0
        preparedAppearanceRevision = nil
        preparedResizePreview = nil
        mountedTableFrames.removeAll()
        mountedSurfaces.removeAll()
        mountedCanvasSize = .zero
        drawingOffset = .zero
        drawingView.bounds.origin = .zero
        drawingView.excludedTableCellContentLayout = nil
        drawingView.selectedTableCellSourceIndices = [:]
        drawingView.selectedTableCellEndpoints = nil
        drawingView.install(layout: nil)
    }

    func presentRemoteCellSelections(_ selections: [RemoteTableCellSelection]) {
        drawingView.remoteTableCellSelections = selections
    }

    func clearCellSelection() {
        defer { selectionGeometryMayChange() }
        cancelHandleDrag()
        drawingView.selectedTableCellSourceIndices = [:]
        drawingView.selectedTableCellEndpoints = nil
    }

    func updateGeometry(from textView: EditorTextView) {
        drawingView.performTablePresentationUpdate { installGeometry(from: textView) }
        selectionGeometryMayChange()
    }

    private func installGeometry(from textView: EditorTextView) {
        discardInvalidDrag()
        reprepareIfNeeded(from: textView)
        drawingView.tableLayerRevision = latestPresentation?.documentRevision
        drawingView.tableLayerChanges = latestPresentation?.changes
        drawingView.tableLayerAppearance = "\(appearanceRevision)-\(textView.renderAppearanceRevision)"
        guard !entries.isEmpty else {
            mountedTableFrames.removeAll()
            mountedSurfaces.removeAll()
            mountedCanvasSize = .zero
            drawingView.excludedTableCellContentLayout = nil
            drawingView.install(layout: nil)
            return
        }
        let anchors = anchorFrames(in: textView)
        let tableFrames = anchors.filter { entries[$0.key] != nil }
        let blocks = entries.values.compactMap { entry -> PreparedProseBlock? in
            guard let frame = tableFrames[entry.tableID] else { return nil }
            return PreparedProseBlock(
                fragments: [],
                bounds: frame,
                tableSurface: entry.surface,
                tableBounds: frame
            )
        }.sorted { $0.bounds.minY < $1.bounds.minY }
        guard !blocks.isEmpty else {
            mountedTableFrames.removeAll()
            mountedSurfaces.removeAll()
            mountedCanvasSize = .zero
            drawingView.excludedTableCellContentLayout = nil
            drawingView.install(layout: nil)
            return
        }
        let currentSurfaces = entries.mapValues(\.surface)
        let canvasSize = canvasSize(for: tableFrames, textView: textView)
        if drawingView.layout == nil
            || mountedTableFrames != tableFrames
            || !sameSurfaces(currentSurfaces, mountedSurfaces)
            || mountedCanvasSize != canvasSize {
            let scale = displayScale(for: textView)
            let key = ProseLayoutKey(
                semanticKey: "editor-table-presentation",
                widthPixels: max(1, Int((canvasSize.width * scale).rounded())),
                themeDigest: "editor-table",
                nativeFontRevision: 0,
                fontEnvironmentRevision: 0,
                displayScale: scale,
                attachmentRevision: presentationRevision ?? 0,
                generationIdentity: "editor-table-presentation",
                semanticGenerationIdentity: "editor-table-presentation"
            )
            let tableRetainedBytes = blocks.reduce(0) { $0 + ($1.tableSurface?.retainedBytes ?? 0) }
            let retainedBytes = blocks.reduce(tableRetainedBytes) { $0 + $1.nonTableRetainedBytes }
            drawingView.install(layout: PreparedProseLayout(
                key: key,
                size: canvasSize,
                blocks: blocks,
                retainedBytes: retainedBytes,
                tableRetainedBytesAtPreparation: tableRetainedBytes
            ))
            mountedTableFrames = tableFrames
            mountedSurfaces = currentSurfaces
            mountedCanvasSize = canvasSize
        }
        let previousBounds = drawingView.bounds
        if drawingView.frame != bounds {
            drawingView.frame = bounds
        }
        drawingOffset = textView.contentOffset
        drawingView.accessibilityRevealScrollView = textView
        let visibleBounds = CGRect(
            origin: drawingOffset,
            size: bounds.size
        )
        if drawingView.bounds != visibleBounds {
            drawingView.bounds = visibleBounds
        }
        if previousBounds != visibleBounds {
            drawingView.setNeedsDisplay()
        }
        let presented = activeCell.flatMap { presentedCell(tableID: $0.tableID, cellIndex: $0.cellIndex) }
        updateExcludedCellContent(presented)
        if let presented { placeInput(in: presented, fallback: .zero) }
    }

    func selectionGeometry(obstructions: TableSelectionObstructions) -> TableSelectionGeometry? {
        guard let host = interactionHost, host.editorId != 0,
              let presentation = latestPresentation,
              let layoutEpoch = presentation.positionEpoch,
              let anchor = toolbarAnchorCells(),
              let tablePos = presentation.index.tableDocStart(tableKey: anchor.tableID),
              let visible = drawingView.tableSelectionViewport(),
              let rects = clipped(
                  drawingView.tableCellRects(
                      tableID: anchor.tableID,
                      sourceIndices: anchor.sourceIndices
                  ),
                  to: visible
              )
        else { return nil }
        return TableSelectionGeometry(
            editorId: host.editorId,
            documentRevision: presentation.documentRevision,
            layoutEpoch: layoutEpoch,
            tablePos: tablePos,
            rects: rects.map { drawingView.convert($0, to: nil) },
            viewport: drawingView.convert(visible, to: nil),
            obstructions: obstructions,
            editMenuVisible: cellEditMenu.isVisible
        )
    }

    private func toolbarAnchorCells() -> (tableID: String, sourceIndices: Set<Int>)? {
        if let selected = drawingView.selectedTableCellSourceIndices.first {
            return (selected.key, selected.value)
        }
        guard let activeCell,
              let presented = presentedCell(tableID: activeCell.tableID, cellIndex: activeCell.cellIndex)
        else { return nil }
        return (activeCell.tableID, [presented.sourceIndex])
    }

    private func clipped(_ rects: [CGRect]?, to visible: CGRect) -> [CGRect]? {
        rects?.map { $0.intersection(visible) }
            .filter { !$0.isNull && !$0.isEmpty }
    }

    private func selectionGeometryMayChange() {
        refreshCellEditMenu()
        onSelectionGeometryMayChange?()
    }

    private func cellEditMenuTextView() -> EditorTextView? {
        guard let host = interactionHost, host.window != nil, host.editorId != 0,
              EditorV2Registry.adapter(forLegacyId: host.editorId) != nil,
              host.activeTextInput === host.textView,
              host.textView.authoritativeCellSelectionActive,
              host.textView.isFirstResponder,
              drawingView.selectedTableCellEndpoints != nil
        else { return nil }
        return host.textView
    }

    func cellDragSource(at point: CGPoint) -> TableCellDragSource? {
        guard activeDrag == nil, cellSelectionContains(point), !hasSelectionHandle(at: point),
              actionableResizeEdge(at: convert(point, to: drawingView)) == nil,
              interactionHost?.hasPendingCompositionForExternalRefresh == false,
              let endpoints = drawingView.selectedTableCellEndpoints,
              let sourceIndices = drawingView.selectedTableCellSourceIndices[endpoints.tableID]
        else { return nil }
        return TableCellDragSource(
            tableID: endpoints.tableID,
            anchor: endpoints.anchor,
            head: endpoints.head,
            sourceIndices: sourceIndices
        )
    }

    func cellDragPreview() -> UITargetedDragPreview? {
        guard drawingView.window != nil, let rects = visibleSelectedCellRects() else { return nil }
        let visiblePath = UIBezierPath()
        rects.forEach { visiblePath.append(UIBezierPath(rect: $0)) }
        let parameters = UIDragPreviewParameters()
        parameters.visiblePath = visiblePath
        return UITargetedDragPreview(view: drawingView, parameters: parameters)
    }

    func showTableCellDropTarget(_ target: TableCellDropTarget?) {
        drawingView.tableCellDropTarget = target
    }

    func rootTableContains(_ point: CGPoint) -> Bool {
        let drawingPoint = convert(point, to: drawingView)
        return drawingView.mountedTablePresentation()?.tables.contains {
            entries[$0.surface.identity] != nil && $0.bounds.contains(drawingPoint) && $0.clip.contains(drawingPoint)
        } == true
    }

    func cellSelectionIncludes(_ target: TableCellDropTarget) -> Bool {
        drawingView.selectedTableCellSourceIndices[target.tableID]?.contains(target.sourceIndex) == true
    }

    private func visibleSelectedCellRects() -> [CGRect]? {
        guard let tableID = drawingView.selectedTableCellEndpoints?.tableID,
              let viewport = interactionViewport(),
              let rects = clipped(drawingView.selectedTableCellRects(tableID: tableID), to: viewport),
              !rects.isEmpty
        else { return nil }
        return rects
    }

    private func cellEditMenuAnchor() -> CGRect? {
        guard let textView = cellEditMenuTextView(),
              let rects = visibleSelectedCellRects()
        else { return nil }
        let union = rects.dropFirst().reduce(rects[0]) { $0.union($1) }
        return drawingView.convert(union, to: textView)
    }

    private func refreshCellEditMenu() {
        guard cellEditMenu.isVisible else { return }
        guard cellEditMenuTextView() != nil,
              cellEditMenuEndpoints == drawingView.selectedTableCellEndpoints,
              cellEditMenuDocumentRevision == latestPresentation?.documentRevision
        else {
            dismissCellEditMenu()
            return
        }
        cellEditMenu.reanchor()
    }

    func presentCellEditMenu() {
        guard activeDrag == nil, cellEditMenuTextView() != nil else { return }
        cellEditMenuEndpoints = drawingView.selectedTableCellEndpoints
        cellEditMenuDocumentRevision = latestPresentation?.documentRevision
        cellEditMenu.present()
    }

    func presentCellEditMenu(at point: CGPoint) {
        guard let host = interactionHost, host.tableEditMenuEnabled,
              !host.hasPendingCompositionForExternalRefresh,
              !hasSelectionHandle(at: point), let hit = cellHit(at: point),
              let context = tableMutationContext(tableID: hit.tableID) else { return }
        drawingView.cancelTableMotion()
        if !cellSelectionContains(point) {
            guard let update = context.adapter.selectTableCell(cellIndex: Int(hit.cellIndex), admission: context.admission),
                  let current = tableMutationContext(tableID: hit.tableID),
                  current.adapter === context.adapter, current.admission == context.admission,
                  host.activeTextInput.applyUpdateJSON(update) else { return }
        }
        guard host.textView.becomeFirstResponder() else { return }
        presentCellEditMenu()
    }

    private func cellEditMenuActions() -> [UIMenuElement] {
        guard interactionHost?.tableEditMenuEnabled == true, cellEditMenuTextView() != nil,
              let endpoints = drawingView.selectedTableCellEndpoints,
              let index = drawingView.selectedTableCellSourceIndices[endpoints.tableID]?.min(),
              let cell = drawingView.tableAccessibilityCell(tableID: endpoints.tableID, sourceIndex: index),
              let context = tableMutationContext(tableID: endpoints.tableID),
              let admission = context.adapter.tableCellSelectionAdmission(tableID: endpoints.tableID),
              admission.anchor == endpoints.anchor, admission.head == endpoints.head else { return [] }
        let actions = tableAccessibilityActions(for: cell, tableID: endpoints.tableID)
        return TableCellEditMenu.groupedActions(actions) { [weak self] action in
            guard let self, self.interactionHost?.tableEditMenuEnabled == true,
                  self.cellEditMenuTextView() != nil,
                  self.drawingView.selectedTableCellEndpoints == endpoints,
                  let current = self.tableMutationContext(tableID: endpoints.tableID),
                  current.adapter.admitsTableCellSelection(admission) else { return }
            self.dismissCellEditMenu()
            _ = self.performTableAccessibilityAction(action, for: cell, tableID: endpoints.tableID)
        }
    }

    func dismissCellEditMenu() {
        cellEditMenu.dismiss()
    }

    func cellSelectionContains(_ point: CGPoint) -> Bool {
        let drawingPoint = convert(point, to: drawingView)
        return cellEditMenuTextView() != nil
            && visibleSelectedCellRects()?.contains(where: { $0.contains(drawingPoint) }) == true
    }

    func toggleCellEditMenu(at point: CGPoint, touchedAt timestamp: TimeInterval) -> Bool {
        guard cellSelectionContains(point) else { return false }
        if cellEditMenu.wasVisible(since: timestamp) {
            dismissCellEditMenu()
        } else {
            presentCellEditMenu()
        }
        return true
    }

    func placeActiveInput(tableID: String, cellIndex: UInt32, fallback contentRect: CGRect) {
        let changedCell = activeCell?.tableID != tableID || activeCell?.cellIndex != cellIndex
        activeCell = (tableID, cellIndex)
        let sourceIndex = Int(cellIndex)
        inputCoordinator.cellInput.tableAccessibilityCell = TableAccessibilityActiveCell(
            cell: { [weak self] in
                self?.drawingView.tableAccessibilityCell(tableID: tableID, sourceIndex: sourceIndex)
            },
            actions: { [weak self] in
                guard let self,
                      let cell = self.drawingView.tableAccessibilityCell(tableID: tableID, sourceIndex: sourceIndex)
                else { return [] }
                return TableAccessibility.customActions(for: cell, tableID: tableID, editing: self)
            }
        )
        let presented = presentedCell(tableID: tableID, cellIndex: cellIndex)
        updateExcludedCellContent(presented)
        placeInput(in: presented, fallback: contentRect)
        inputCoordinator.cellInput.isHidden = false
        if changedCell { scheduleActiveInputReveal() }
        selectionGeometryMayChange()
    }

    @objc func scheduleActiveInputReveal() {
        guard !activeInputRevealScheduled else { return }
        activeInputRevealScheduled = true
        DispatchQueue.main.async { [weak self] in
            guard let self else { return }
            self.activeInputRevealScheduled = false
            self.revealActiveInput()
        }
    }

    private func revealActiveInput() {
        let input = inputCoordinator.cellInput
        guard activeCell != nil, !input.isHidden, input.isFirstResponder,
              !input.authoritativeCellSelectionActive,
              let host = interactionHost, host.window != nil,
              let scroll = verticalScrollTarget(for: host) else { return }
        host.layoutIfNeeded()
        input.layoutIfNeeded()
        updateGeometry(from: host.textView)
        guard let selection = input.selectedTextRange,
              let viewport = interactionViewport() else { return }
        let logical = input.isComposing ? nil : input.currentLogicalScalarSelection()
        let backward = logical.map { $0.head < $0.anchor } ?? false
        let caret = scroll.convert(input.caretRect(for: backward ? selection.start : selection.end), from: input)
        if scroll.scrollVerticallyToReveal(caret, within: scroll.convert(viewport, from: drawingView)) {
            updateGeometry(from: host.textView)
        }
    }

    func hideActiveInput() {
        defer { selectionGeometryMayChange() }
        activeCell = nil
        updateExcludedCellContent(nil)
        inputCoordinator.cellInput.tableAccessibilityCell = nil
        inputCoordinator.cellInput.isHidden = true
        activeCellClipView.frame = .zero
    }

    func cellFrame(tableID: String, cellIndex: UInt32) -> CGRect? {
        guard let presented = drawingView.tableCellGeometry(tableID: tableID, sourceIndex: Int(cellIndex)) else { return nil }
        let inset = presented.surface.style.cellPadding + presented.surface.style.borderWidth
        return presented.bounds.offsetBy(dx: -drawingOffset.x, dy: -drawingOffset.y)
            .insetBy(dx: inset, dy: inset)
    }

    func isRightToLeft(tableID: String) -> Bool? {
        guard let surface = drawingView.mountedTablePresentation()?.tables.first(where: {
            $0.surface.identity == tableID
        })?.surface else { return nil }
        return surface.direction == .rightToLeft
    }

    func arrowDestination(
        tableID: String,
        cellIndex: UInt32,
        direction: TableCellArrowDirection,
        caret: CGPoint
    ) -> ArrowDestination? {
        guard let presented = drawingView.tableCellGeometry(tableID: tableID, sourceIndex: Int(cellIndex)),
              let source = presented.surface.cells.first(where: { $0.sourceIndex == Int(cellIndex) })
        else { return nil }
        func frame(_ cell: PreparedViewerTableCell) -> CGRect { presented.surface.frame(ofCell: cell) }
        let origin = CGPoint(
            x: presented.bounds.minX - frame(source).minX - drawingOffset.x,
            y: presented.bounds.minY - frame(source).minY - drawingOffset.y
        )
        let cells = presented.surface.cells.filter { $0.sourceIndex != Int(cellIndex) }
        let x = min(max(caret.x - origin.x, frame(source).minX), frame(source).maxX.nextDown)
        let y = min(max(caret.y - origin.y, frame(source).minY), frame(source).maxY.nextDown)
        let candidates: [PreparedViewerTableCell]
        switch direction {
        case .left:
            candidates = cells.filter {
                frame($0).minY <= y && y < frame($0).maxY
                    && frame($0).minX < frame(source).minX && frame($0).maxX <= frame(source).minX
            }.sorted { frame($0).maxX > frame($1).maxX }
        case .right:
            candidates = cells.filter {
                frame($0).minY <= y && y < frame($0).maxY
                    && frame($0).minX >= frame(source).maxX && frame($0).maxX > frame(source).maxX
            }.sorted { frame($0).minX < frame($1).minX }
        case .up:
            candidates = cells.filter {
                frame($0).minX <= x && x < frame($0).maxX
                    && frame($0).minY < frame(source).minY && frame($0).maxY <= frame(source).minY
            }.sorted { frame($0).maxY > frame($1).maxY }
        case .down:
            candidates = cells.filter {
                frame($0).minX <= x && x < frame($0).maxX
                    && frame($0).minY >= frame(source).maxY && frame($0).maxY > frame(source).maxY
            }.sorted { frame($0).minY < frame($1).minY }
        }
        if let index = candidates.first?.sourceIndex {
            return .cell(UInt32(index))
        }
        if direction == .left || direction == .right {
            let forward = (direction == .right) != (presented.surface.direction == .rightToLeft)
            let nextIndex = Int(cellIndex) + (forward ? 1 : -1)
            if presented.surface.cells.contains(where: { $0.sourceIndex == nextIndex }) {
                return .cell(UInt32(nextIndex))
            }
            return .surroundingProse
        }
        let outerEdge = direction == .up
            ? frame(source).minY == presented.surface.bounds.minY
            : frame(source).maxY == presented.surface.bounds.maxY
        return outerEdge ? .surroundingProse : .blocked
    }

    func nestedTableHeights(tableID: String, cellIndex: UInt32, input: EditorTextView? = nil) -> [String: CGFloat]? {
        guard let cell = entries[tableID]?.surface.cell(sourceIndex: Int(cellIndex)) else { return nil }
        guard cell.hasNestedTables else { return [:] }
        guard let presented = presentedCell(tableID: tableID, cellIndex: cellIndex) else { return nil }
        return nestedTableHeights(content: presented.content, input: input)
    }

    private func nestedTableHeights(content: PreparedProseLayout, input: EditorTextView?) -> [String: CGFloat]? {
        var precedingSpacing: [String: CGFloat] = [:]
        if let input, input.textStorage.length > 0 {
            input.textStorage.enumerateAttribute(
                RenderBridgeAttributes.rootTableMarker,
                in: NSRange(location: 0, length: input.textStorage.length)
            ) { value, range, _ in
                guard let identity = value as? String,
                      range.location > 0
                else { return }
                let paragraph = input.textStorage.attribute(
                    .paragraphStyle, at: range.location - 1, effectiveRange: nil
                ) as? NSParagraphStyle
                precedingSpacing[identity] = paragraph?.paragraphSpacing ?? 0
            }
        }
        var heights: [String: CGFloat] = [:]
        let blocks = content.blocks
        for index in blocks.indices {
            let block = blocks[index]
            guard let nested = block.tableSurface else { continue }
            let previous = index > blocks.startIndex ? blocks[index - 1] : nil
            let nextStart = index + 1 < blocks.endIndex
                ? blocks[index + 1].bounds.minY : content.size.height
            let leading = previous?.tableSurface == nil
                ? block.bounds.minY - (previous?.bounds.maxY ?? 0) : 0
            let trailing = nextStart - block.bounds.maxY
            let height = block.bounds.height + leading + trailing
                - (precedingSpacing[nested.identity] ?? 0)
            guard height.isFinite, height > 0 else { return nil }
            heights[nested.identity] = height
        }
        return heights
    }

    func cellHit(at point: CGPoint) -> RootTableCellHit? {
        let contentPoint = CGPoint(x: point.x + drawingOffset.x, y: point.y + drawingOffset.y)
        guard let presented = drawingView.mountedTablePresentation()?.cells.last(where: {
            $0.surface.sourceTable != nil && $0.clip.contains(contentPoint) && $0.bounds.contains(contentPoint)
        }) else { return nil }
        let inset = presented.surface.style.cellPadding + presented.surface.style.borderWidth
        return RootTableCellHit(
            tableID: presented.surface.identity,
            cellIndex: UInt32(presented.sourceIndex),
            contentRect: presented.bounds.offsetBy(dx: -drawingOffset.x, dy: -drawingOffset.y)
                .insetBy(dx: inset, dy: inset)
        )
    }

    private func interactionViewport() -> CGRect? {
        guard let host = interactionHost,
              let visible = drawingView.tableSelectionViewport()
        else { return nil }
        let insets = host.textView.adjustedContentInset
        let padded = CGRect(
            x: drawingView.bounds.minX + insets.left,
            y: drawingView.bounds.minY + insets.top,
            width: drawingView.bounds.width - insets.left - insets.right,
            height: drawingView.bounds.height - insets.top - insets.bottom
        )
        var viewport = visible.intersection(padded)
        guard !viewport.isNull, !viewport.isEmpty else { return nil }
        if let window = host.window,
           let keyboardFrame = host.textView.keyboardFrameInScreen {
            let windowFrame = window.convert(keyboardFrame, from: window.screen.coordinateSpace)
            let occlusion = drawingView.convert(windowFrame, from: window)
            if viewport.intersects(occlusion) {
                viewport.size.height = max(0, min(viewport.maxY, occlusion.minY) - viewport.minY)
            }
        }
        return viewport.isEmpty ? nil : viewport
    }

    func hasSelectionHandle(at point: CGPoint) -> Bool {
        guard actionableHandle(at: convert(point, to: drawingView)) != nil else { return false }
        return true
    }

    private func hostAllowsTableInteraction(
        _ adapter: EditorV2Adapter? = nil
    ) -> (host: RichTextEditorView, adapter: EditorV2Adapter)? {
        guard let host = interactionHost,
              host.window != nil,
              host.editorId != 0,
              let current = EditorV2Registry.adapter(forLegacyId: host.editorId),
              adapter == nil || adapter === current,
              host.hasTableCellBindingAuthority(current),
              host.isUserInteractionEnabled, host.textView.isUserInteractionEnabled,
              host.textView.isEditable,
              !host.hasPendingCompositionForExternalRefresh,
              host.activeTextInput.selectedTextRange?.isEmpty != false
        else { return nil }
        return (host, current)
    }

    private struct ActionableHandle {
        let handle: TableSelectionHandle
        let adapter: EditorV2Adapter
        let admission: EditorV2Adapter.TableCellSelectionAdmission
    }

    private func actionableHandle(at point: CGPoint) -> ActionableHandle? {
        guard let (host, adapter) = hostAllowsTableInteraction(),
              activeCell == nil, host.activeTextInput === host.textView,
              let viewport = interactionViewport(),
              let handle = drawingView.hitSelectionHandle(at: point, visibleIn: viewport),
              let endpoints = drawingView.selectedTableCellEndpoints,
              endpoints.tableID == handle.tableID,
              let admission = adapter.tableCellSelectionAdmission(tableID: endpoints.tableID),
              admission.anchor == endpoints.anchor, admission.head == endpoints.head
        else { return nil }
        return ActionableHandle(handle: handle, adapter: adapter, admission: admission)
    }

    private func activeInputContains(_ point: CGPoint) -> Bool {
        let input = inputCoordinator.cellInput
        guard !input.isHidden, input.window != nil else { return false }
        return drawingView.convert(input.bounds, from: input).contains(point)
    }

    private struct ActionableResizeEdge {
        let hit: TableResizeEdgeHit
        let adapter: EditorV2Adapter
        let admission: EditorV2Adapter.TableMutationAdmission
    }

    private func actionableResizeEdge(at point: CGPoint) -> ActionableResizeEdge? {
        guard activeDrag == nil,
              let (_, adapter) = hostAllowsTableInteraction(),
              let viewport = interactionViewport(),
              drawingView.hitSelectionHandle(at: point, visibleIn: viewport) == nil,
              !activeInputContains(point),
              let hit = drawingView.hitResizeEdge(at: point, visibleIn: viewport),
              entries[hit.edge.tableID] != nil,
              let admission = adapter.tableMutationAdmission(tableID: hit.edge.tableID),
              adapter.admitsTableMutation(admission)
        else { return nil }
        return ActionableResizeEdge(hit: hit, adapter: adapter, admission: admission)
    }

    func gestureRecognizer(_ gestureRecognizer: UIGestureRecognizer, shouldReceive touch: UITouch) -> Bool {
        if gestureRecognizer === selectionGesture, handleDrag != nil { return true }
        if gestureRecognizer === resizeGesture, resizeDrag != nil || resizeGesture.state != .possible { return true }
        guard touch.tapCount == 1,
              touch.view?.window === interactionHost?.window
        else { return false }
        if gestureRecognizer === textSelectionGesture, let host = interactionHost {
            return trackTextSelectionDrag(at: touch.location(in: host))
        }
        let point = touch.location(in: drawingView)
        if gestureRecognizer === selectionGesture {
            return actionableHandle(at: point) != nil
        }
        if gestureRecognizer === resizeGesture {
            guard actionableResizeEdge(at: point) != nil else { return false }
            resizeTouchPoint = point
            return true
        }
        return false
    }

    override func gestureRecognizerShouldBegin(_ gestureRecognizer: UIGestureRecognizer) -> Bool {
        if gestureRecognizer === textSelectionGesture, let host = interactionHost {
            return textSelectionDragTarget(at: gestureRecognizer.location(in: host)) != nil
        }
        if gestureRecognizer === selectionGesture {
            return actionableHandle(at: gestureRecognizer.location(in: drawingView)) != nil
        }
        guard gestureRecognizer === resizeGesture else { return false }
        return actionableResizeEdge(at: resizeTouchPoint) != nil
    }

    func gestureRecognizer(
        _ gestureRecognizer: UIGestureRecognizer,
        shouldBeRequiredToFailBy otherGestureRecognizer: UIGestureRecognizer
    ) -> Bool {
        guard gestureRecognizer === resizeGesture, otherGestureRecognizer !== resizeGesture,
              !(otherGestureRecognizer is UIScreenEdgePanGestureRecognizer)
        else { return false }
        return otherGestureRecognizer is UIPanGestureRecognizer || isTextInputGesture(otherGestureRecognizer)
    }

    private func isTextInputGesture(_ recognizer: UIGestureRecognizer) -> Bool {
        guard let view = recognizer.view, let host = interactionHost else { return false }
        return host.textInputs.contains { view.isDescendant(of: $0) }
    }

    func gestureRecognizer(
        _ gestureRecognizer: UIGestureRecognizer,
        shouldRequireFailureOf otherGestureRecognizer: UIGestureRecognizer
    ) -> Bool {
        gestureRecognizer === resizeGesture && otherGestureRecognizer is UIScreenEdgePanGestureRecognizer
    }

    @objc private func handleSelectionGesture(_ recognizer: TableSelectionHandleGestureRecognizer) {
        guard recognizer === selectionGesture, let host = interactionHost else { return }
        let point = recognizer.location(in: host)
        switch recognizer.state {
        case .began:
            _ = beginHandleDrag(at: point)
        case .changed:
            updateHandleDrag(at: point)
        case .ended:
            endHandleDrag(at: point)
        case .cancelled, .failed:
            cancelHandleDrag()
        default:
            break
        }
    }

    @objc private func handleTextSelectionGesture(_ recognizer: TableTextSelectionGestureRecognizer) {
        guard let host = interactionHost else { return }
        let point = recognizer.location(in: host)
        switch recognizer.state {
        case .began:
            _ = beginTextSelectionDrag(at: point)
        case .changed:
            updateHandleDrag(at: point)
        case .ended:
            endHandleDrag(at: point)
        case .cancelled, .failed:
            cancelHandleDrag()
        default:
            break
        }
    }

    @discardableResult
    func trackTextSelectionDrag(at hostPoint: CGPoint) -> Bool {
        textSelectionOrigin = nil
        guard activeDrag == nil, let host = interactionHost,
              let cell = activeCell, let context = tableMutationContext(tableID: cell.tableID),
              host.activeTextInput === inputCoordinator.cellInput,
              let map = inputCoordinator.cellInput.tableCellPositionMap,
              map.binding.tableKey == cell.tableID, map.binding.cellIndex == cell.cellIndex,
              map.binding.documentRevision == context.admission.documentRevision,
              inputCoordinator.cellInput.isFirstResponder,
              let range = inputCoordinator.cellInput.selectedTextRange, !range.isEmpty else { return false }
        let input = inputCoordinator.cellInput
        let point = host.convert(hostPoint, to: input)
        guard let role = input.selectionHandleRole(at: point) else { return false }
        textSelectionOrigin = TextSelectionOrigin(
            adapter: context.adapter, admission: context.admission, cellIndex: cell.cellIndex, role: role
        )
        return true
    }

    private func textSelectionDragTarget(at hostPoint: CGPoint) -> RootTableCellHit? {
        guard let origin = textSelectionOrigin, let host = interactionHost,
              host.activeTextInput === inputCoordinator.cellInput,
              activeCell?.tableID == origin.admission.tableID, activeCell?.cellIndex == origin.cellIndex,
              let context = tableMutationContext(tableID: origin.admission.tableID),
              context.adapter === origin.adapter, context.admission == origin.admission,
              let target = cellHit(at: host.convert(hostPoint, to: self)),
              target.tableID == origin.admission.tableID, target.cellIndex != origin.cellIndex else { return nil }
        return target
    }

    @discardableResult
    func beginTextSelectionDrag(at hostPoint: CGPoint) -> Bool {
        guard textSelectionDragTarget(at: hostPoint) != nil,
              let origin = textSelectionOrigin, let host = interactionHost, let window = host.window,
              let update = origin.adapter.selectTableCell(cellIndex: Int(origin.cellIndex), admission: origin.admission),
              let context = tableMutationContext(tableID: origin.admission.tableID),
              context.adapter === origin.adapter, context.admission == origin.admission,
              host.textView.applyUpdateJSON(update), host.textView.becomeFirstResponder(),
              let admission = origin.adapter.tableCellSelectionAdmission(tableID: origin.admission.tableID)
        else { return false }
        dismissCellEditMenu()
        drawingView.cancelTableMotion()
        activeDrag = HandleDrag(adapter: origin.adapter, admission: admission, role: origin.role,
            touchOffset: .zero, windowPoint: host.convert(hostPoint, to: window))
        updateHandleDrag(at: hostPoint)
        return handleDrag != nil
    }

    @objc private func handleResizeGesture(_ recognizer: TableHorizontalPanGestureRecognizer) {
        guard recognizer === resizeGesture, let host = interactionHost else { return }
        let point = recognizer.location(in: host)
        switch recognizer.state {
        case .began:
            guard beginResizeDrag(at: drawingView.convert(resizeTouchPoint, to: host)) else { return }
            updateResizeDrag(at: point)
        case .changed:
            updateResizeDrag(at: point)
        case .ended:
            endResizeDrag(at: point)
        case .cancelled, .failed:
            cancelResizeDrag()
        default:
            break
        }
    }

    @discardableResult
    func beginHandleDrag(at hostPoint: CGPoint) -> Bool {
        guard let host = interactionHost else { return false }
        cancelActiveDrag()
        guard let actionable = actionableHandle(at: drawingView.convert(hostPoint, from: host)) else { return false }
        dismissCellEditMenu()
        drawingView.cancelTableMotion()
        let point = drawingView.convert(hostPoint, from: host)
        let offset = CGPoint(
            x: point.x - actionable.handle.center.x,
            y: point.y - actionable.handle.center.y
        )
        guard let window = host.window else { return false }
        activeDrag = HandleDrag(
            adapter: actionable.adapter,
            admission: actionable.admission,
            role: actionable.handle.role,
            touchOffset: offset,
            windowPoint: host.convert(hostPoint, to: window)
        )
        scheduleDragFrame()
        return true
    }

    func updateHandleDrag(at hostPoint: CGPoint) {
        guard let drag = handleDrag else { return }
        guard let host = interactionHost, let window = host.window else { cancelHandleDrag(); return }
        drag.windowPoint = host.convert(hostPoint, to: window)
        retargetHandleDrag(drag)
        if activeDrag === drag { scheduleDragFrame() }
    }

    @discardableResult
    func beginResizeDrag(at hostPoint: CGPoint) -> Bool {
        guard let host = interactionHost, let window = host.window else { return false }
        let point = drawingView.convert(hostPoint, from: host)
        guard let actionable = actionableResizeEdge(at: point) else { return false }
        drawingView.cancelTableMotion()
        activeDrag = ResizeDrag(
            adapter: actionable.adapter,
            admission: actionable.admission,
            hit: actionable.hit,
            startX: point.x,
            windowPoint: host.convert(hostPoint, to: window)
        )
        drawingView.activeTableResizeEdge = actionable.hit.edge
        return true
    }

    func updateResizeDrag(at hostPoint: CGPoint) {
        guard let drag = resizeDrag else { return }
        guard let host = interactionHost, let window = host.window else { cancelResizeDrag(); return }
        drag.windowPoint = host.convert(hostPoint, to: window)
        retargetResizeDrag(drag)
        if activeDrag === drag { scheduleDragFrame() }
    }

    func endResizeDrag(at hostPoint: CGPoint) {
        updateResizeDrag(at: hostPoint)
        guard let drag = resizeDrag else { return }
        let valid = validResizeDrag(drag)
        discardActiveDrag()
        guard let host = interactionHost else { return }
        guard valid, drag.previewWidth != drag.clampedWidth(drag.startWidth),
              let update = drag.adapter.resizeTableColumn(
                  column: drag.edge.column, width: Int(drag.previewWidth), admission: drag.admission
              ),
              host.activeTextInput.applyUpdateJSON(update)
        else {
            updateGeometry(from: host.textView)
            return
        }
    }

    func cancelResizeDrag() {
        guard resizeDrag != nil else { return }
        discardActiveDrag()
        if let host = interactionHost { updateGeometry(from: host.textView) }
    }

    private func validDrag(_ drag: DragSession) -> Bool {
        switch drag {
        case let handle as HandleDrag:
            return validHandleDrag(handle)
        case let resize as ResizeDrag:
            return validResizeDrag(resize)
        default:
            return false
        }
    }

    private func validHandleDrag(_ drag: HandleDrag) -> Bool {
        guard activeDrag === drag,
              let (host, _) = hostAllowsTableInteraction(drag.adapter),
              activeCell == nil, host.activeTextInput === host.textView,
              drawingView.selectedTableCellEndpoints == TableSelectionEndpoints(
                  tableID: drag.admission.tableID,
                  anchor: drag.admission.anchor,
                  head: drag.admission.head
              )
        else { return false }
        return drag.adapter.admitsTableCellSelection(drag.admission)
    }

    private func validResizeDrag(_ drag: ResizeDrag) -> Bool {
        guard activeDrag === drag,
              hostAllowsTableInteraction(drag.adapter) != nil,
              let surface = entries[drag.tableID]?.surface,
              drag.edge.column < surface.layout.columnWidths.count
        else { return false }
        return drag.adapter.admitsTableMutation(drag.admission)
    }

    private func discardInvalidDrag() {
        if let drag = activeDrag, !validDrag(drag) { discardActiveDrag() }
    }

    private func retargetHandleDrag(_ drag: HandleDrag) {
        guard !submittingHandleSelection else { return }
        guard validHandleDrag(drag), let host = interactionHost, let window = host.window else {
            cancelHandleDrag()
            return
        }
        let point = drawingView.convert(drag.windowPoint, from: window)
        let targetPoint = CGPoint(
            x: point.x - drag.touchOffset.x,
            y: point.y - drag.touchOffset.y
        )
        guard let viewport = interactionViewport(),
              let target = drawingView.selectedTableCell(
                  at: targetPoint, tableID: drag.admission.tableID, visibleIn: viewport
              )
        else { return }
        let anchor = drag.role == .anchor ? target : drag.admission.anchor
        let head = drag.role == .head ? target : drag.admission.head
        guard anchor != drag.admission.anchor || head != drag.admission.head else { return }
        submittingHandleSelection = true
        defer { submittingHandleSelection = false }
        guard let update = drag.adapter.selectExactTableCells(
            anchor: anchor, head: head, admission: drag.admission
        ), let nextEpoch = drag.adapter.positionEpoch else {
            cancelHandleDrag()
            return
        }
        drag.admission.anchor = anchor
        drag.admission.head = head
        drag.admission.positionEpoch = nextEpoch
        guard host.textView.applyUpdateJSON(update),
              activeDrag === drag, validHandleDrag(drag)
        else { cancelHandleDrag(); return }
        host.textView.editorDelegate?.editorTextView(host.textView, selectionDidChange: anchor, head: head)
    }

    private func retargetResizeDrag(_ drag: ResizeDrag) {
        guard validResizeDrag(drag), let host = interactionHost, let window = host.window else {
            cancelResizeDrag()
            return
        }
        let point = drawingView.convert(drag.windowPoint, from: window)
        let requested = drag.startWidth + drag.directionSign * (point.x - drag.startX) + drag.scrolledLogical
        let width = drag.clampedWidth(requested)
        guard width.isFinite, width != drag.previewWidth else { return }
        drag.previewWidth = width
        resizePreview = TableResizePreview(edge: drag.edge, width: width)
        updateGeometry(from: host.textView)
    }

    private func scheduleDragFrame() {
        guard activeDrag != nil, dragFrameLink == nil else { return }
        let link = CADisplayLink(target: self, selector: #selector(stepDragFrame(_:)))
        dragFrameLink = link
        link.add(to: .main, forMode: .common)
    }

    @objc private func stepDragFrame(_ link: CADisplayLink) {
        guard link === dragFrameLink, !runningDragFrame,
              let drag = activeDrag, validDrag(drag),
              let host = interactionHost, let window = host.window, let viewport = interactionViewport()
        else {
            cancelActiveDrag()
            return
        }
        runningDragFrame = true
        defer { runningDragFrame = false }
        let point = drawingView.convert(drag.windowPoint, from: window)
        var scrolled = false
        if let table = drawingView.mountedTablePresentation()?.tables.first(where: {
            $0.surface.identity == drag.tableID
        }) {
            let tableViewport = viewport.intersection(table.clip)
            if !tableViewport.isNull, !tableViewport.isEmpty {
                let horizontal: CGFloat = point.x < tableViewport.minX + HandleScrollMetrics.edgeBand
                    ? HandleScrollMetrics.stepPerFrame
                    : point.x > tableViewport.maxX - HandleScrollMetrics.edgeBand
                    ? -HandleScrollMetrics.stepPerFrame : 0
                if horizontal != 0 {
                    let before = drawingView.tableLogicalOffset(for: drag.tableID)
                    scrolled = drawingView.scrollTables(
                        in: [table.surface.scrollIdentity], by: horizontal
                    ) != horizontal
                    if scrolled, let resize = drag as? ResizeDrag {
                        resize.scrolledLogical += drawingView.tableLogicalOffset(for: drag.tableID) - before
                    }
                }
            }
        }
        guard activeDrag === drag, validDrag(drag) else { cancelActiveDrag(); return }
        if drag is HandleDrag {
            let vertical: CGFloat = point.y < viewport.minY + HandleScrollMetrics.edgeBand
                ? -HandleScrollMetrics.stepPerFrame
                : point.y > viewport.maxY - HandleScrollMetrics.edgeBand
                ? HandleScrollMetrics.stepPerFrame : 0
            if vertical != 0, let scroll = verticalScrollTarget(for: host) {
                let minimum = -scroll.adjustedContentInset.top
                let maximum = max(
                    minimum,
                    scroll.contentSize.height - scroll.bounds.height
                        + scroll.adjustedContentInset.bottom
                )
                let next = min(maximum, max(minimum, scroll.contentOffset.y + vertical))
                if next != scroll.contentOffset.y {
                    scroll.setContentOffset(CGPoint(x: scroll.contentOffset.x, y: next), animated: false)
                    scrolled = true
                }
            }
            guard activeDrag === drag, validDrag(drag) else { cancelActiveDrag(); return }
        }
        if scrolled {
            switch drag {
            case let handle as HandleDrag:
                retargetHandleDrag(handle)
            case let resize as ResizeDrag:
                retargetResizeDrag(resize)
            default:
                break
            }
        } else {
            dragFrameLink?.invalidate()
            dragFrameLink = nil
        }
    }

    private func verticalScrollTarget(for host: RichTextEditorView) -> UIScrollView? {
        if host.textView.isScrollEnabled { return host.textView }
        var ancestor = host.superview
        while let view = ancestor {
            if let scroll = view as? UIScrollView, scroll.isScrollEnabled {
                return scroll
            }
            ancestor = view.superview
        }
        return nil
    }

    func endHandleDrag(at hostPoint: CGPoint) {
        updateHandleDrag(at: hostPoint)
        guard let drag = handleDrag else { return }
        let valid = validHandleDrag(drag)
        discardActiveDrag()
        if valid, interactionHost?.tableEditMenuEnabled == true { presentCellEditMenu() }
    }

    func cancelHandleDrag() {
        guard handleDrag != nil else { return }
        discardActiveDrag()
    }

    private func cancelActiveDrag() {
        if resizeDrag != nil {
            cancelResizeDrag()
        } else {
            discardActiveDrag()
        }
    }

    private func discardActiveDrag() {
        activeDrag = nil
        dragFrameLink?.invalidate()
        dragFrameLink = nil
        resizePreview = nil
        drawingView.activeTableResizeEdge = nil
    }

    override func hitTest(_ point: CGPoint, with event: UIEvent?) -> UIView? {
        let input = inputCoordinator.cellInput
        if !input.isHidden, activeCellClipView.frame.contains(point) {
            if input.frame.contains(convert(point, to: activeCellClipView)) {
                return super.hitTest(point, with: event)
            }
            return input.selectionHandleHitTest(convert(point, to: input), with: event)
        }
        return nil
    }

    private func prepareEntries(
        _ presentation: EditorV2Adapter.EditorTablePresentationSnapshot,
        tableIDs: Set<String>,
        width: CGFloat,
        displayScale: CGFloat,
        textView: EditorTextView
    ) -> [String: Entry] {
        let layoutStarted = PreparedProseInstrumentation.now()
        defer { PreparedProseInstrumentation.laidOut(layoutStarted, generation: "editor-table-\(presentation.documentRevision)") }
        let appearanceDigest = "editor-table-\(appearanceRevision)-\(textView.renderAppearanceRevision)"
        var theme = PreparedProseTheme.resolve(
            editorTheme: textView.theme,
            baseFont: textView.baseFont,
            textColor: textView.baseTextColor,
            semanticGeneration: "editor-table",
            styleIdentity: appearanceDigest
        )
        theme.contentInsets = .zero
        theme.tableDirection = hostTableDirection
        let shapeCatalog = PreparedCellShapeCatalog()
        let shapes = shapeCatalog.newBuildContext()
        var seededShapes: PreparedCellShapeBuildContext?
        defer {
            shapes.close()
            seededShapes?.close()
        }
        return tableIDs.reduce(into: [:]) { entries, tableID in
            guard var table = presentation.index.record(tableKey: tableID) else { return }
            var themeDigest = appearanceDigest
            if let preview = resizePreview, preview.edge.tableID == tableID,
               preview.edge.column >= 0, preview.edge.column < table.columnWidths.count,
               let width = UInt32(exactly: preview.width) {
                table.columnWidths[preview.edge.column] = width
                themeDigest += "-resize-\(preview.edge.column)-\(width)"
            }
            let document = ViewerDocument(
                semanticKey: "editor-table-\(tableID)-\(presentation.documentRevision)",
                blocks: [ViewerBlock(
                    nodeType: "table",
                    depth: 0,
                    inBlockquote: false,
                    listContext: nil,
                    listItemBoundary: nil,
                    inlines: [],
                    frameTable: table
                )],
                isEmpty: false,
                retainedBytes: 256,
                preparedTheme: theme,
                tableAttributes: presentation.index.attributeObjects,
                frameIndex: presentation.index
            )
            guard let widthPixels = ProseLayoutMetrics.widthPixels(widthPoints: width, scale: displayScale) else { return }
            let previousEntry = self.entries[tableID]
            var reusable: ReusableCellContents?
            let engine = CoreTextProseLayoutEngine()
            engine.allowsTransientCellMeasurement = drawingView.preparesTableCellsOnViewportChange
            engine.tableCellShapeContextProvider = { incremental in
                if incremental { return shapes }
                if let seededShapes { return seededShapes }
                let previousLayouts = tableIDs.flatMap { self.entries[$0]?.surface.layoutStore.residentLayouts ?? [] }
                self.seededShapeLayoutsForTesting += previousLayouts.count
                let context = shapeCatalog.newBuildContext(reusing: previousLayouts)
                seededShapes = context
                return context
            }
            engine.tableCellPreparationObserver = { [weak self] index, contentKey in
                self?.onTableCellPreparedForTesting?(index, contentKey)
            }
            defer {
                engine.incrementalTableSurface = nil
                engine.tableCellShapeContextProvider = nil
                engine.reusableTableCell = nil
                engine.reusableTableCellStore = nil
                engine.tableIncrementalRelayoutObserver = nil
            }
            if let previous = self.entries[tableID], previous.themeDigest == themeDigest,
               let presentationRevision, presentationRevision == presentation.baseDocumentRevision,
               !presentation.changes.fullReset, !presentation.changes.replacedTables.contains(tableID),
               previous.surface.scrollIdentity == (document.tableSourceIDs[tableID] ?? tableID),
               previous.surface.cells.allSatisfy(\.isPositionFree) {
                engine.incrementalTableSurface = { key in
                    guard key == tableID else { return nil }
                    return (previous.surface, presentation.changes.changedCells[key] ?? [])
                }
                engine.tableIncrementalRelayoutObserver = { self.incrementalRelayoutsForTesting += 1 }
            }
            if let previousEntry {
                engine.reusableTableCellStore = previousEntry.surface.layoutStore
                engine.reusableTableCell = { cell, widthPixels in
                    if reusable == nil { reusable = ReusableCellContents(previousEntry, themeDigest: themeDigest) }
                    return reusable?.take(cell, widthPixels: widthPixels, displayScale: displayScale)
                }
            }
            let key = ProseLayoutKey(
                semanticKey: document.semanticKey,
                widthPixels: widthPixels,
                themeDigest: themeDigest,
                nativeFontRevision: 0,
                fontEnvironmentRevision: 0,
                displayScale: displayScale,
                attachmentRevision: presentation.documentRevision,
                generationIdentity: document.semanticKey,
                semanticGenerationIdentity: document.semanticKey
            )
            guard let prepared = try? engine.prepare(
                document: document,
                key: key,
                widthPoints: width,
                displayScale: displayScale,
                cellShapeContext: shapes
            ), let tableBlock = prepared.blocks.first(where: { $0.tableSurface != nil }),
            let surface = tableBlock.tableSurface,
            let localTableBounds = tableBlock.tableBounds
            else { return }
            entries[tableID] = Entry(
                tableID: tableID,
                surface: surface,
                localTableBounds: localTableBounds,
                occupiedHeight: prepared.size.height,
                themeDigest: themeDigest
            )
        }
    }

    private func reprepareIfNeeded(from textView: EditorTextView) {
        guard let presentation = latestPresentation else { return }
        let width = availableWidth(in: textView)
        guard width > 0 else {
            entries.removeAll()
            presentationRevision = nil
            preparedWidth = 0
            preparedResizePreview = nil
            drawingView.install(layout: nil)
            return
        }
        guard presentationRevision != presentation.documentRevision
            || abs(preparedWidth - width) > 0.5
            || preparedAppearanceRevision != appearanceRevision
            || preparedResizePreview != resizePreview
        else {
            textView.reserveRootTableHeights(entries.mapValues(\.occupiedHeight))
            return
        }
        textView.traitCollection.performAsCurrent {
            entries = prepareEntries(
                presentation,
                tableIDs: anchorTableIDs(in: textView),
                width: width,
                displayScale: displayScale(for: textView),
                textView: textView
            )
        }
        presentationRevision = presentation.documentRevision
        preparedWidth = width
        preparedAppearanceRevision = appearanceRevision
        preparedResizePreview = resizePreview
        textView.reserveRootTableHeights(entries.mapValues(\.occupiedHeight))
    }

    private func availableWidth(in textView: EditorTextView) -> CGFloat {
        let width = textView.textContainer.size.width - 2 * textView.textContainer.lineFragmentPadding
        return width.isFinite ? max(0, width) : 0
    }

    private func displayScale(for textView: EditorTextView) -> CGFloat {
        let scale = textView.window?.screen.scale ?? UIScreen.main.scale
        return scale.isFinite && scale > 0 ? scale : 1
    }

    private func canvasSize(for tableFrames: [String: CGRect], textView: EditorTextView) -> CGSize {
        let tableBounds = tableFrames.reduce(CGRect.null) { bounds, item in
            guard let surface = entries[item.key]?.surface else { return bounds }
            return bounds.union(CGRect(origin: item.value.origin, size: surface.bounds.size))
        }
        let width = max(bounds.width, textView.contentSize.width, tableBounds.maxX)
        let height = max(bounds.height, textView.contentSize.height, tableBounds.maxY)
        return CGSize(
            width: width.isFinite ? max(1, width) : max(1, bounds.width),
            height: height.isFinite ? max(1, height) : max(1, bounds.height)
        )
    }

    private func anchorFrames(in textView: EditorTextView) -> [String: CGRect] {
        guard textView.textStorage.length > 0 else { return [:] }
        var frames: [String: CGRect] = [:]
        let fullRange = NSRange(location: 0, length: textView.textStorage.length)
        textView.textStorage.enumerateAttribute(
            RenderBridgeAttributes.rootTableMarker,
            in: fullRange,
            options: []
        ) { value, range, _ in
            guard let tableID = value as? String,
                  self.entries[tableID] != nil
            else { return }
            textView.layoutManager.ensureLayout(forCharacterRange: range)
            let glyphRange = textView.layoutManager.glyphRange(forCharacterRange: range, actualCharacterRange: nil)
            guard glyphRange.location != NSNotFound else { return }
            let line = textView.layoutManager.lineFragmentRect(forGlyphAt: glyphRange.location, effectiveRange: nil)
            guard line.origin.x.isFinite, line.origin.y.isFinite,
                  line.width.isFinite, line.height.isFinite,
                  line.height > 0
            else { return }
            guard let entry = self.entries[tableID] else { return }
            frames[tableID] = entry.localTableBounds.offsetBy(
                dx: textView.textContainerInset.left + line.minX,
                dy: textView.textContainerInset.top + line.minY
            )
        }
        return frames
    }

    private func anchorTableIDs(in textView: EditorTextView) -> Set<String> {
        guard textView.textStorage.length > 0 else { return [] }
        var tableIDs = Set<String>()
        textView.textStorage.enumerateAttribute(
            RenderBridgeAttributes.rootTableMarker,
            in: NSRange(location: 0, length: textView.textStorage.length),
            options: []
        ) { value, _, _ in
            if let tableID = value as? String {
                tableIDs.insert(tableID)
            }
        }
        return tableIDs
    }

    private func presentedCell(tableID: String, cellIndex: UInt32) -> ViewerTablePresentedCell? {
        drawingView.presentedTableCell(tableID: tableID, sourceIndex: Int(cellIndex))
    }

    private func placeInput(in presented: ViewerTablePresentedCell?, fallback: CGRect) {
        guard let presented else {
            activeCellClipView.frame = bounds
            inputCoordinator.cellInput.frame = fallback
            return
        }
        let clip = presented.clip.offsetBy(dx: -drawingOffset.x, dy: -drawingOffset.y)
            .intersection(bounds)
        activeCellClipView.frame = clip.isNull ? .zero : clip.integral
        let inset = presented.surface.style.cellPadding + presented.surface.style.borderWidth
        let content = presented.bounds.offsetBy(dx: -drawingOffset.x, dy: -drawingOffset.y)
            .insetBy(dx: inset, dy: inset)
        inputCoordinator.cellInput.frame = content.offsetBy(
            dx: -activeCellClipView.frame.minX, dy: -activeCellClipView.frame.minY
        )
    }

    private func refreshActiveInputFrame() {
        guard let activeCell,
              let presented = presentedCell(tableID: activeCell.tableID, cellIndex: activeCell.cellIndex)
        else { return }
        placeInput(in: presented, fallback: .zero)
    }

    private func updateExcludedCellContent(_ presented: ViewerTablePresentedCell?) {
        if let presented {
            presented.cell.layoutStore.pin(presented.cell.contentKey)
            presented.cell.layoutStore.insert(presented.content, for: presented.cell.contentKey)
        }
        if let pinnedInputCell { pinnedInputCell.layoutStore.unpin(pinnedInputCell.contentKey) }
        pinnedInputCell = presented?.cell
        drawingView.tableLayerCell = activeCell.map {
            PreparedProseDrawingView.TableLayerCell(tableID: $0.tableID, sourceIndex: Int($0.cellIndex))
        }
        guard let presented else {
            drawingView.excludedTableCellContentLayout = nil
            return
        }
        drawingView.excludedTableCellContentLayout = presented.content
        if let heights = nestedTableHeights(content: presented.content, input: inputCoordinator.cellInput) {
            inputCoordinator.cellInput.reserveRootTableHeights(heights)
        }
    }

    private func sameSurfaces(
        _ left: [String: ViewerTableSurface],
        _ right: [String: ViewerTableSurface]
    ) -> Bool {
        guard left.count == right.count else { return false }
        return left.allSatisfy { tableID, surface in right[tableID] === surface }
    }
}

extension EditorTableSurface: TableAccessibilityEditing {
    struct TableMutationContext {
        let host: RichTextEditorView
        let adapter: EditorV2Adapter
        let admission: EditorV2Adapter.TableMutationAdmission
    }

    func tableMutationContext(tableID: String) -> TableMutationContext? {
        guard let host = interactionHost, host.window != nil, host.editorId != 0,
              let adapter = EditorV2Registry.adapter(forLegacyId: host.editorId),
              host.hasTableCellBindingAuthority(adapter),
              host.textView.isEditable,
              !host.hasPendingCompositionForExternalRefresh,
              let admission = adapter.tableMutationAdmission(tableID: tableID),
              adapter.admitsTableMutation(admission)
        else { return nil }
        return TableMutationContext(host: host, adapter: adapter, admission: admission)
    }

    private func ownsAccessibilitySelection(_ cell: TableAccessibilityCell, tableID: String) -> Bool {
        if let activeCell {
            return activeCell.tableID == tableID && Int(activeCell.cellIndex) == cell.sourceIndex
        }
        return drawingView.selectedTableCellSourceIndices[tableID]?.contains(cell.sourceIndex) == true
    }

    func tableAccessibilityActions(for cell: TableAccessibilityCell, tableID: String) -> [TableAccessibilityAction] {
        guard ownsAccessibilitySelection(cell, tableID: tableID),
              let context = tableMutationContext(tableID: tableID),
              let commands = context.adapter.cachedActiveState?["commands"] as? [String: Any]
        else { return [] }
        return TableAccessibilityAction.all.filter { commands[$0.applicability] as? Bool == true }
    }

    func performTableAccessibilityAction(
        _ action: TableAccessibilityAction,
        for cell: TableAccessibilityCell,
        tableID: String
    ) -> Bool {
        guard tableAccessibilityActions(for: cell, tableID: tableID).contains(action),
              let context = tableMutationContext(tableID: tableID),
              let update = context.adapter.applyTableCommandAtSelection(action.command, admission: context.admission)
        else { return false }
        return context.host.activeTextInput.applyUpdateJSON(update)
    }

    func activateTableAccessibilityCell(_ cell: TableAccessibilityCell, tableID: String) -> Bool {
        guard let host = interactionHost,
              let index = UInt32(exactly: cell.sourceIndex),
              let presented = drawingView.tableCellGeometry(tableID: tableID, sourceIndex: Int(index))
        else { return false }
        let visible = presented.bounds.intersection(presented.clip)
        guard !visible.isNull, !visible.isEmpty,
              host.activateTableCell(at: drawingView.convert(CGPoint(x: visible.midX, y: visible.midY), to: self))
        else { return false }
        UIAccessibility.post(notification: .layoutChanged, argument: inputCoordinator.cellInput)
        return true
    }

    func activeTableAccessibilityElement(for cell: TableAccessibilityCell, tableID: String) -> TableCellInputTextView? {
        let input = inputCoordinator.cellInput
        guard let activeCell, activeCell.tableID == tableID,
              Int(activeCell.cellIndex) == cell.sourceIndex, !input.isHidden
        else { return nil }
        return input
    }

    private func unanchoredTableIDs() -> Set<String> {
        guard let presentation = latestPresentation else { return [] }
        return Set(presentation.index.rootExtents.filter { $0.value.scalarStart == $0.value.scalarEnd }.keys)
    }

    func detachedTableAccessibilityFrames() -> [TableAccessibilityDetachedFrame] {
        guard let presentation = latestPresentation else { return [] }
        return unanchoredTableIDs().compactMap { tableID in
            guard let record = presentation.index.record(tableKey: tableID),
                  let tablePos = presentation.index.tableDocStart(tableKey: tableID) else { return nil }
            let unfilled = record.failure == nil && (record.rows == 0 || record.columns == 0)
            return TableAccessibilityDetachedFrame(
                tableID: tableID, tablePos: tablePos, frame: unfilled ? .empty : .failed,
                screenFrame: { [weak self] in self?.detachedFrameScreenRect(tablePos: tablePos) ?? .zero }
            )
        }
    }

    private func detachedFrameScreenRect(tablePos: UInt32) -> CGRect {
        guard let host = interactionHost, host.editorId != 0,
              let adapter = EditorV2Registry.adapter(forLegacyId: host.editorId),
              let scalar = adapter.scalarPosition(forDoc: tablePos)
        else { return .zero }
        let textView = host.textView
        let caret = textView.caretRect(for: PositionBridge.scalarToTextView(scalar, in: textView))
        guard !caret.isNull, caret.minX.isFinite, caret.minY.isFinite, caret.height.isFinite else { return .zero }
        let insets = textView.textContainerInset
        let line = CGRect(
            x: insets.left,
            y: caret.minY,
            width: max(caret.width, textView.bounds.width - insets.left - insets.right),
            height: caret.height
        )
        return accessibilityScreenRect(line, in: textView)
    }

    func canDeleteTableAccessibilityFrame(tableID: String) -> Bool {
        tableMutationContext(tableID: tableID) != nil
    }

    func deleteTableAccessibilityFrame(tableID: String) -> Bool {
        guard let context = tableMutationContext(tableID: tableID),
              let update = context.adapter.deleteTable(admission: context.admission)
        else { return false }
        return context.host.activeTextInput.applyUpdateJSON(update)
    }
}
