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
    }

    let inputCoordinator: EditorTableInputCoordinator
    private let drawingView = PreparedProseDrawingView(frame: .zero)
    private let activeCellClipView = UIView(frame: .zero)
    private var entries: [String: Entry] = [:]
    private var latestPresentation: EditorV2Adapter.EditorTablePresentationSnapshot?
    private var presentationRevision: UInt64?
    private var preparedWidth: CGFloat = 0
    private var appearanceRevision: UInt64 = 0
    private var preparedAppearanceRevision: UInt64?
    private var mountedTableFrames: [String: CGRect] = [:]
    private var mountedSurfaces: [String: ViewerTableSurface] = [:]
    private var mountedCanvasSize = CGSize.zero
    private var drawingOffset = CGPoint.zero
    private var activeCell: (tableID: String, cellIndex: UInt32)?
    private weak var interactionHost: RichTextEditorView?
    private lazy var selectionGesture: TableSelectionHandleGestureRecognizer = {
        let recognizer = TableSelectionHandleGestureRecognizer(target: self, action: #selector(handleSelectionGesture(_:)))
        recognizer.delegate = self
        recognizer.cancelsTouchesInView = true
        return recognizer
    }()
    private final class HandleDrag {
        let adapter: EditorV2Adapter
        var admission: EditorV2Adapter.TableCellSelectionAdmission
        let role: TableSelectionHandleRole
        let touchOffset: CGPoint
        var windowPoint: CGPoint

        init(adapter: EditorV2Adapter, admission: EditorV2Adapter.TableCellSelectionAdmission,
             role: TableSelectionHandleRole, touchOffset: CGPoint, windowPoint: CGPoint) {
            self.adapter = adapter
            self.admission = admission
            self.role = role
            self.touchOffset = touchOffset
            self.windowPoint = windowPoint
        }
    }
    private var handleDrag: HandleDrag?
    private var handleFrameLink: CADisplayLink?
    private var runningHandleFrame = false
    private var submittingHandleSelection = false

    private enum HandleScrollMetrics {
        static let edgeBand: CGFloat = 36
        static let stepPerFrame: CGFloat = 6
    }

    init(inputCoordinator: EditorTableInputCoordinator) {
        self.inputCoordinator = inputCoordinator
        super.init(frame: .zero)
        clipsToBounds = true
        drawingView.isOpaque = false
        drawingView.backgroundColor = .clear
        drawingView.isUserInteractionEnabled = false
        addSubview(drawingView)
        activeCellClipView.clipsToBounds = true
        addSubview(activeCellClipView)
        inputCoordinator.cellInput.isHidden = true
        activeCellClipView.addSubview(inputCoordinator.cellInput)
        drawingView.onTableGeometryChanged = { [weak self] in
            self?.refreshActiveInputFrame()
        }
    }

    required init?(coder: NSCoder) {
        fatalError("init(coder:) has not been implemented")
    }

    deinit {
        cancelHandleDrag()
        selectionGesture.view?.removeGestureRecognizer(selectionGesture)
    }

    override func didMoveToWindow() {
        super.didMoveToWindow()
        if window == nil { cancelHandleDrag() }
    }

    func installTableInteraction(on host: UIView) {
        interactionHost = host as? RichTextEditorView
        host.addGestureRecognizer(selectionGesture)
        drawingView.installTableInteraction(on: host)
    }

    func present(_ presentation: EditorV2Adapter.EditorTablePresentationSnapshot,
                 selection: EditorCellSelection?, endpoints: (anchor: UInt32, head: UInt32)?, ownerIdentity: String,
                 from textView: EditorTextView) {
        drawingView.setTableOwnerIdentity(ownerIdentity)
        latestPresentation = presentation
        if case let .drawable(tableID, sourcePositions) = selection {
            drawingView.selectedTableCellSourcePositions = [tableID: sourcePositions]
            drawingView.selectedTableCellEndpoints = endpoints.map {
                TableSelectionEndpoints(tableID: tableID, anchor: $0.anchor, head: $0.head)
            }
        } else {
            drawingView.selectedTableCellSourcePositions = [:]
            drawingView.selectedTableCellEndpoints = nil
        }
        reprepareIfNeeded(from: textView)
        updateGeometry(from: textView)
        if let drag = handleDrag, !validHandleDrag(drag) { cancelHandleDrag() }
    }

    func invalidateAppearance() {
        appearanceRevision &+= 1
    }

    func clearPresentation() {
        cancelHandleDrag()
        drawingView.setTableOwnerIdentity(nil)
        entries.removeAll()
        latestPresentation = nil
        presentationRevision = nil
        preparedWidth = 0
        preparedAppearanceRevision = nil
        mountedTableFrames.removeAll()
        mountedSurfaces.removeAll()
        mountedCanvasSize = .zero
        drawingOffset = .zero
        drawingView.bounds.origin = .zero
        drawingView.excludedTableCellContentLayout = nil
        drawingView.selectedTableCellSourcePositions = [:]
        drawingView.selectedTableCellEndpoints = nil
        drawingView.install(layout: nil)
    }

    func clearCellSelection() {
        cancelHandleDrag()
        drawingView.selectedTableCellSourcePositions = [:]
        drawingView.selectedTableCellEndpoints = nil
    }

    func updateGeometry(from textView: EditorTextView) {
        if let drag = handleDrag, !validHandleDrag(drag) { cancelHandleDrag() }
        reprepareIfNeeded(from: textView)
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
            let retainedBytes = blocks.reduce(0) { $0 + $1.estimatedRetainedBytes }
            drawingView.install(layout: PreparedProseLayout(
                key: key,
                size: canvasSize,
                blocks: blocks,
                retainedBytes: retainedBytes
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
        let visibleBounds = CGRect(
            origin: drawingOffset,
            size: bounds.size
        )
        if drawingView.bounds != visibleBounds {
            drawingView.bounds = visibleBounds
        }
        if previousBounds != visibleBounds {
            drawingView.setNeedsDisplay()
            drawingView.updateConfiguredImagesForVisibleWindow()
        }
        updateExcludedCellContent()
        refreshActiveInputFrame()
    }

    func placeActiveInput(tableID: String, cellIndex: UInt32, fallback contentRect: CGRect) {
        activeCell = (tableID, cellIndex)
        updateExcludedCellContent()
        placeInput(in: presentedCell(tableID: tableID, cellIndex: cellIndex), fallback: contentRect)
        inputCoordinator.cellInput.isHidden = false
    }

    func hideActiveInput() {
        activeCell = nil
        drawingView.excludedTableCellContentLayout = nil
        inputCoordinator.cellInput.isHidden = true
        activeCellClipView.frame = .zero
    }

    func cellFrame(tableID: String, cellIndex: UInt32) -> CGRect? {
        guard let presented = presentedCell(tableID: tableID, cellIndex: cellIndex) else { return nil }
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

    func arrowDestination(tableID: String, cellIndex: UInt32,
                          direction: TableCellArrowDirection, caret: CGPoint) -> ArrowDestination? {
        guard let presented = presentedCell(tableID: tableID, cellIndex: cellIndex),
              let source = presented.surface.cells.first(where: { $0.sourceCellIndex == Int(cellIndex) })
        else { return nil }
        let origin = CGPoint(x: presented.bounds.minX - source.frame.minX - drawingOffset.x,
                             y: presented.bounds.minY - source.frame.minY - drawingOffset.y)
        let cells = presented.surface.cells.filter { $0.sourceCellIndex != nil && $0.sourceCellIndex != Int(cellIndex) }
        let x = min(max(caret.x - origin.x, source.frame.minX), source.frame.maxX.nextDown)
        let y = min(max(caret.y - origin.y, source.frame.minY), source.frame.maxY.nextDown)
        let candidates: [PreparedViewerTableCell]
        switch direction {
        case .left:
            candidates = cells.filter {
                $0.frame.minY <= y && y < $0.frame.maxY
                    && $0.frame.minX < source.frame.minX && $0.frame.maxX <= source.frame.minX
            }.sorted { $0.frame.maxX > $1.frame.maxX }
        case .right:
            candidates = cells.filter {
                $0.frame.minY <= y && y < $0.frame.maxY
                    && $0.frame.minX >= source.frame.maxX && $0.frame.maxX > source.frame.maxX
            }.sorted { $0.frame.minX < $1.frame.minX }
        case .up:
            candidates = cells.filter {
                $0.frame.minX <= x && x < $0.frame.maxX
                    && $0.frame.minY < source.frame.minY && $0.frame.maxY <= source.frame.minY
            }.sorted { $0.frame.maxY > $1.frame.maxY }
        case .down:
            candidates = cells.filter {
                $0.frame.minX <= x && x < $0.frame.maxX
                    && $0.frame.minY >= source.frame.maxY && $0.frame.maxY > source.frame.maxY
            }.sorted { $0.frame.minY < $1.frame.minY }
        }
        if let index = candidates.first?.sourceCellIndex {
            return .cell(UInt32(index))
        }
        if direction == .left || direction == .right {
            let forward = (direction == .right) != (presented.surface.direction == .rightToLeft)
            let nextIndex = Int(cellIndex) + (forward ? 1 : -1)
            if presented.surface.cells.contains(where: { $0.sourceCellIndex == nextIndex }) {
                return .cell(UInt32(nextIndex))
            }
            return .surroundingProse
        }
        let outerEdge = direction == .up
            ? source.frame.minY == presented.surface.bounds.minY
            : source.frame.maxY == presented.surface.bounds.maxY
        return outerEdge ? .surroundingProse : .blocked
    }

    func nestedTableHeights(tableID: String, cellIndex: UInt32, input: EditorTextView? = nil) -> [String: CGFloat]? {
        guard let cell = presentedCell(tableID: tableID, cellIndex: cellIndex)?.cell else { return nil }
        var precedingSpacing: [String: CGFloat] = [:]
        if let input, input.textStorage.length > 0 {
            input.textStorage.enumerateAttribute(
                RenderBridgeAttributes.rootTableScalarExtent,
                in: NSRange(location: 0, length: input.textStorage.length)
            ) { value, range, _ in
                guard let marker = value as? RenderBridge.RootTableScalarExtent,
                      let identity = marker.tableID,
                      range.location > 0
                else { return }
                let paragraph = input.textStorage.attribute(
                    .paragraphStyle, at: range.location - 1, effectiveRange: nil
                ) as? NSParagraphStyle
                precedingSpacing[identity] = paragraph?.paragraphSpacing ?? 0
            }
        }
        var heights: [String: CGFloat] = [:]
        let blocks = cell.content.blocks
        for index in blocks.indices {
            let block = blocks[index]
            guard let nested = block.tableSurface else { continue }
            let previous = index > blocks.startIndex ? blocks[index - 1] : nil
            let nextStart = index + 1 < blocks.endIndex
                ? blocks[index + 1].bounds.minY : cell.content.size.height
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
            $0.cell.sourceCellIndex != nil && $0.clip.contains(contentPoint) && $0.bounds.contains(contentPoint)
        }), let sourceCellIndex = presented.cell.sourceCellIndex else { return nil }
        let inset = presented.surface.style.cellPadding + presented.surface.style.borderWidth
        return RootTableCellHit(
            tableID: presented.surface.identity,
            cellIndex: UInt32(sourceCellIndex),
            contentRect: presented.bounds.offsetBy(dx: -drawingOffset.x, dy: -drawingOffset.y)
                .insetBy(dx: inset, dy: inset)
        )
    }

    private func handleViewport() -> CGRect? {
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

    private func actionableHandle(at point: CGPoint) -> (
        handle: TableSelectionHandle, adapter: EditorV2Adapter,
        admission: EditorV2Adapter.TableCellSelectionAdmission
    )? {
        guard let host = interactionHost,
              host.window != nil,
              host.editorId != 0,
              let adapter = EditorV2Registry.adapter(forLegacyId: host.editorId),
              host.hasTableCellBindingAuthority(adapter),
              host.isUserInteractionEnabled, host.textView.isUserInteractionEnabled,
              host.textView.isEditable,
              !host.hasPendingCompositionForExternalRefresh,
              host.textView.selectedTextRange?.isEmpty != false,
              activeCell == nil, host.activeTextInput === host.textView,
              let ownerID = adapter.nativeOwnerId,
              let ownerToken = adapter.nativeOwnerToken,
              let epoch = adapter.positionEpoch,
              let viewport = handleViewport(),
              let handle = drawingView.hitSelectionHandle(at: point, visibleIn: viewport),
              let endpoints = drawingView.selectedTableCellEndpoints,
              endpoints.tableID == handle.tableID
        else { return nil }
        let admission = EditorV2Adapter.TableCellSelectionAdmission(
            tableID: endpoints.tableID,
            documentRevision: adapter.baseDocumentRevision,
            positionEpoch: epoch,
            presentationGeneration: adapter.tableResetGeneration,
            ownerID: ownerID,
            ownerToken: ownerToken,
            anchor: endpoints.anchor,
            head: endpoints.head
        )
        guard adapter.admitsTableCellSelection(admission) else { return nil }
        return (handle, adapter, admission)
    }

    func gestureRecognizer(_ gestureRecognizer: UIGestureRecognizer, shouldReceive touch: UITouch) -> Bool {
        guard gestureRecognizer === selectionGesture else { return false }
        if handleDrag != nil { return true }
        guard touch.tapCount == 1,
              touch.view?.window === interactionHost?.window
        else { return false }
        return actionableHandle(at: touch.location(in: drawingView)) != nil
    }

    override func gestureRecognizerShouldBegin(_ gestureRecognizer: UIGestureRecognizer) -> Bool {
        gestureRecognizer === selectionGesture
            && actionableHandle(at: gestureRecognizer.location(in: drawingView)) != nil
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
            updateHandleDrag(at: point)
            cancelHandleDrag()
        case .cancelled, .failed:
            cancelHandleDrag()
        default:
            break
        }
    }

    @discardableResult
    func beginHandleDrag(at hostPoint: CGPoint) -> Bool {
        guard let host = interactionHost,
              let actionable = actionableHandle(at: drawingView.convert(hostPoint, from: host))
        else { return false }
        cancelHandleDrag()
        drawingView.cancelTableMotion()
        let point = drawingView.convert(hostPoint, from: host)
        let offset = CGPoint(x: point.x - actionable.handle.center.x,
                             y: point.y - actionable.handle.center.y)
        guard let window = host.window else { return false }
        handleDrag = HandleDrag(adapter: actionable.adapter, admission: actionable.admission,
                                role: actionable.handle.role, touchOffset: offset,
                                windowPoint: host.convert(hostPoint, to: window))
        scheduleHandleFrame()
        return true
    }

    func updateHandleDrag(at hostPoint: CGPoint) {
        guard let drag = handleDrag else { return }
        guard let host = interactionHost, let window = host.window else { cancelHandleDrag(); return }
        drag.windowPoint = host.convert(hostPoint, to: window)
        retargetHandleDrag(drag)
        if handleDrag === drag { scheduleHandleFrame() }
    }

    private func validHandleDrag(_ drag: HandleDrag) -> Bool {
        guard let host = interactionHost,
              host.window != nil,
              host.editorId != 0,
              EditorV2Registry.adapter(forLegacyId: host.editorId) === drag.adapter,
              host.hasTableCellBindingAuthority(drag.adapter),
              host.isUserInteractionEnabled, host.textView.isUserInteractionEnabled,
              host.textView.isEditable,
              !host.hasPendingCompositionForExternalRefresh,
              host.textView.selectedTextRange?.isEmpty != false,
              activeCell == nil, host.activeTextInput === host.textView,
              drawingView.selectedTableCellEndpoints == TableSelectionEndpoints(
                tableID: drag.admission.tableID,
                anchor: drag.admission.anchor,
                head: drag.admission.head
              )
        else { return false }
        return drag.adapter.admitsTableCellSelection(drag.admission)
    }

    private func retargetHandleDrag(_ drag: HandleDrag) {
        guard !submittingHandleSelection else { return }
        guard validHandleDrag(drag), let host = interactionHost, let window = host.window else {
            cancelHandleDrag()
            return
        }
        let point = drawingView.convert(drag.windowPoint, from: window)
        let targetPoint = CGPoint(x: point.x - drag.touchOffset.x,
                                  y: point.y - drag.touchOffset.y)
        guard let viewport = handleViewport(),
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
              handleDrag === drag, validHandleDrag(drag)
        else { cancelHandleDrag(); return }
    }

    private func scheduleHandleFrame() {
        guard handleDrag != nil, handleFrameLink == nil else { return }
        let link = CADisplayLink(target: self, selector: #selector(stepHandleFrame(_:)))
        handleFrameLink = link
        link.add(to: .main, forMode: .common)
    }

    @objc private func stepHandleFrame(_ link: CADisplayLink) {
        guard link === handleFrameLink, !runningHandleFrame,
              let drag = handleDrag, validHandleDrag(drag),
              let host = interactionHost, let window = host.window, let viewport = handleViewport()
        else {
            cancelHandleDrag()
            return
        }
        runningHandleFrame = true
        defer { runningHandleFrame = false }
        let point = drawingView.convert(drag.windowPoint, from: window)
        var scrolled = false
        if let table = drawingView.mountedTablePresentation()?.tables.first(where: {
            $0.surface.identity == drag.admission.tableID
        }) {
            let tableViewport = viewport.intersection(table.clip)
            if !tableViewport.isNull, !tableViewport.isEmpty {
                let horizontal: CGFloat = point.x < tableViewport.minX + HandleScrollMetrics.edgeBand
                    ? HandleScrollMetrics.stepPerFrame
                    : point.x > tableViewport.maxX - HandleScrollMetrics.edgeBand
                        ? -HandleScrollMetrics.stepPerFrame : 0
                if horizontal != 0 {
                    scrolled = drawingView.scrollTables(
                        in: [table.surface.scrollIdentity], by: horizontal
                    ) != horizontal
                }
            }
        }
        guard handleDrag === drag, validHandleDrag(drag) else { cancelHandleDrag(); return }
        let vertical: CGFloat = point.y < viewport.minY + HandleScrollMetrics.edgeBand
            ? -HandleScrollMetrics.stepPerFrame
            : point.y > viewport.maxY - HandleScrollMetrics.edgeBand
                ? HandleScrollMetrics.stepPerFrame : 0
        if vertical != 0, let scroll = verticalScrollTarget(for: host) {
            let minimum = -scroll.adjustedContentInset.top
            let maximum = max(minimum, scroll.contentSize.height - scroll.bounds.height
                              + scroll.adjustedContentInset.bottom)
            let next = min(maximum, max(minimum, scroll.contentOffset.y + vertical))
            if next != scroll.contentOffset.y {
                scroll.setContentOffset(CGPoint(x: scroll.contentOffset.x, y: next), animated: false)
                scrolled = true
            }
        }
        guard handleDrag === drag, validHandleDrag(drag) else { cancelHandleDrag(); return }
        if scrolled { retargetHandleDrag(drag) }
        if !scrolled {
            handleFrameLink?.invalidate()
            handleFrameLink = nil
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

    func cancelHandleDrag() {
        handleDrag = nil
        handleFrameLink?.invalidate()
        handleFrameLink = nil
    }

    override func hitTest(_ point: CGPoint, with event: UIEvent?) -> UIView? {
        if !inputCoordinator.cellInput.isHidden,
           activeCellClipView.frame.contains(point),
           inputCoordinator.cellInput.frame.contains(convert(point, to: activeCellClipView)) {
            return super.hitTest(point, with: event)
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
        var theme = PreparedProseTheme.resolve(
            editorTheme: textView.theme,
            baseFont: textView.baseFont,
            textColor: textView.baseTextColor,
            semanticGeneration: "editor-table"
        )
        theme.contentInsets = .zero
        let themeDigest = "editor-table-\(appearanceRevision)-\(textView.renderAppearanceRevision)"
        return tableIDs.reduce(into: [:]) { entries, tableID in
            guard let table = presentation.tableRecords[tableID] else { return }
            let document = ViewerDocument(
                semanticKey: "editor-table-\(tableID)-\(presentation.documentRevision)",
                blocks: [ViewerBlock(
                    nodeType: "table",
                    depth: 0,
                    inBlockquote: false,
                    listContext: nil,
                    listItemBoundary: nil,
                    inlines: [],
                    table: table
                )],
                isEmpty: false,
                retainedBytes: 256,
                preparedTheme: theme,
                tableAttributes: presentation.tableAttributes,
                tableRecords: presentation.tableRecords,
                tableSourceIDs: presentation.tableSourceIDs
            )
            guard let widthPixels = ProseLayoutMetrics.widthPixels(widthPoints: width, scale: displayScale) else { return }
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
            guard let prepared = try? CoreTextProseLayoutEngine().prepare(
                document: document,
                key: key,
                widthPoints: width,
                displayScale: displayScale
            ), let tableBlock = prepared.blocks.first(where: { $0.tableSurface != nil }),
               let surface = tableBlock.tableSurface,
               let localTableBounds = tableBlock.tableBounds
            else { return }
            entries[tableID] = Entry(
                tableID: tableID,
                surface: surface,
                localTableBounds: localTableBounds,
                occupiedHeight: prepared.size.height
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
            drawingView.install(layout: nil)
            return
        }
        guard presentationRevision != presentation.documentRevision
                || abs(preparedWidth - width) > 0.5
                || preparedAppearanceRevision != appearanceRevision
        else {
            textView.reserveRootTableHeights(entries.mapValues(\.occupiedHeight))
            return
        }
        entries = prepareEntries(
            presentation,
            tableIDs: anchorTableIDs(in: textView),
            width: width,
            displayScale: displayScale(for: textView),
            textView: textView
        )
        presentationRevision = presentation.documentRevision
        preparedWidth = width
        preparedAppearanceRevision = appearanceRevision
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
        return CGSize(width: width.isFinite ? max(1, width) : max(1, bounds.width),
                      height: height.isFinite ? max(1, height) : max(1, bounds.height))
    }

    private func anchorFrames(in textView: EditorTextView) -> [String: CGRect] {
        guard textView.textStorage.length > 0 else { return [:] }
        var frames: [String: CGRect] = [:]
        let fullRange = NSRange(location: 0, length: textView.textStorage.length)
        textView.textStorage.enumerateAttribute(
            RenderBridgeAttributes.rootTableScalarExtent,
            in: fullRange,
            options: []
        ) { value, range, _ in
            guard let extent = value as? RenderBridge.RootTableScalarExtent,
                  let tableID = extent.tableID,
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
            RenderBridgeAttributes.rootTableScalarExtent,
            in: NSRange(location: 0, length: textView.textStorage.length),
            options: []
        ) { value, _, _ in
            if let tableID = (value as? RenderBridge.RootTableScalarExtent)?.tableID {
                tableIDs.insert(tableID)
            }
        }
        return tableIDs
    }

    private func presentedCell(tableID: String, cellIndex: UInt32) -> ViewerTablePresentedCell? {
        drawingView.mountedTablePresentation()?.cells.first {
            $0.surface.identity == tableID && $0.cell.sourceCellIndex == Int(cellIndex)
        }
    }

    private func placeInput(in presented: ViewerTablePresentedCell?, fallback: CGRect) {
        guard let presented else {
            activeCellClipView.frame = bounds
            inputCoordinator.cellInput.frame = fallback.integral
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
        ).integral
    }

    private func refreshActiveInputFrame() {
        guard let activeCell,
              let presented = presentedCell(tableID: activeCell.tableID, cellIndex: activeCell.cellIndex)
        else { return }
        placeInput(in: presented, fallback: .zero)
    }

    private func updateExcludedCellContent() {
        guard let activeCell,
              let presented = presentedCell(tableID: activeCell.tableID, cellIndex: activeCell.cellIndex) else {
            drawingView.excludedTableCellContentLayout = nil
            return
        }
        drawingView.excludedTableCellContentLayout = presented.content
        if let heights = nestedTableHeights(
            tableID: activeCell.tableID,
            cellIndex: activeCell.cellIndex,
            input: inputCoordinator.cellInput
        ) {
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
