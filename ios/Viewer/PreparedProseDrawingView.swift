import CoreText
import UIKit

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
    let codeHighlightingSession = NativeCodeHighlightingSession()
    var onCodeHighlightingResolved: ((String) -> Void)?
    var onCodeHighlightingFailure: ((Error) -> Void)?
    @objc public static let codeHighlightingDidResolve = Notification.Name("com.apollohg.editor.viewer.codeHighlightingDidResolve")
    @objc public static let codeHighlightingDidFail = Notification.Name("com.apollohg.editor.viewer.codeHighlightingDidFail")

    deinit { codeHighlightingSession.cancel() }

    var imagePixels: [String: UIImage] = [:] {
        didSet {
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
                layout?.retainedBytes ?? 0,
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
    var layout: PreparedProseLayout? {
        didSet {
            guard oldValue !== layout else { return }
            tablePresentationOwner = ViewerTablePresentationOwner()
            updateSidecarInstrumentation()
            invalidateAccessibilityNodes()
            setNeedsDisplay()
        }
    }
    private var tablePresentationOwner = ViewerTablePresentationOwner()
    fileprivate var accessibilityPresentationGeneration = 0
    /// Geometry-only hook reserved for the following atom/event transport phase.
    @objc public var onTableGeometryChanged: (() -> Void)?
    var onMountedTableCellsDrawnForTesting: ((Int) -> Void)?
    var onTableChromeDrawnForTesting: ((Int) -> Void)?
    var onTableRichFragmentDrawnForTesting: (() -> Void)?
    weak var excludedTableCellContentLayout: PreparedProseLayout? {
        didSet {
            if oldValue !== excludedTableCellContentLayout { setNeedsDisplay() }
        }
    }

    @objc public func install(layout: PreparedProseLayout?) {
        guard self.layout !== layout else { return }
        self.layout = layout
        scheduleCodeHighlighting()
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

    /// Mounted-only offset seam. Host direction and gestures are deliberately
    /// not inferred here.
    @objc(setTableLogicalOffset:sourceIdentity:)
    public func setTableLogicalOffset(_ offset: CGFloat, sourceIdentity: String) {
        guard let layout,
              let surface = ViewerTablePresentation.project(
                layout: layout,
                owner: tablePresentationOwner,
                viewport: .unknown
              ).cells.first(where: { $0.surface.identity == sourceIdentity })?.surface
        else { return }
        tablePresentationOwner.setLogicalOffset(offset, for: surface)
        updateSidecarInstrumentation()
        updateConfiguredImagesForVisibleWindow()
        invalidateAccessibilityNodes()
        onTableGeometryChanged?()
        setNeedsDisplay()
    }

    private func presentationSnapshot(viewport: ViewerTablePresentationViewport = .unknown) -> ViewerTablePresentationSnapshot? {
        layout.map { ViewerTablePresentation.project(layout: $0, owner: tablePresentationOwner, viewport: viewport) }
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
    private var accessibilityElementsByIndex: [Int: PreparedProseDrawingAccessibilityElement] = [:]
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
        updateConfiguredImagesForVisibleWindow()
    }

    public override func didMoveToWindow() {
        super.didMoveToWindow()
        if window == nil { codeHighlightingSession.cancel() } else { scheduleCodeHighlighting() }
        updateConfiguredImagesForVisibleWindow()
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) { fatalError("PreparedProseDrawingView does not support NSCoder") }

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
        guard let window, !isHidden, alpha > 0 else { return nil }
        var visible = convert(window.bounds, from: window).intersection(bounds)
        var ancestor = superview
        while let view = ancestor, view !== window {
            guard !view.isHidden, view.alpha > 0 else { return nil }
            if view.clipsToBounds {
                visible = visible.intersection(convert(view.bounds, from: view))
            }
            ancestor = view.superview
        }
        guard visible.origin.x.isFinite, visible.origin.y.isFinite,
              visible.size.width.isFinite, visible.size.height.isFinite,
              !visible.isNull, !visible.isEmpty else { return nil }
        return visible
    }

    private func refreshScrollObservations() {
        var scrollViews: [UIScrollView] = []
        var ancestor = superview
        while let view = ancestor {
            if let scrollView = view as? UIScrollView { scrollViews.append(scrollView) }
            ancestor = view.superview
        }
        let activeScrollViews = window == nil ? [] : scrollViews
        let nextIDs = activeScrollViews.map(ObjectIdentifier.init)
        guard nextIDs != observedScrollViewIDs else { return }
        scrollObservations.removeAll()
        observedScrollViewIDs = nextIDs
        scrollObservations = activeScrollViews.map { scrollView in
            scrollView.observe(\.contentOffset, options: [.new]) { [weak self] _, _ in
                self?.updateConfiguredImagesForVisibleWindow()
                self?.onTableGeometryChanged?()
            }
        }
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

    public override func accessibilityElementCount() -> Int { accessibilityNodes.count }

    public override func accessibilityElement(at index: Int) -> Any? {
        let nodes = accessibilityNodes
        guard nodes.indices.contains(index), layout != nil else { return nil }
        if let existing = accessibilityElementsByIndex[index] { return existing }
        let element = PreparedProseDrawingAccessibilityElement(
            container: self,
            index: index,
            rootLayout: layout,
            generation: accessibilityPresentationGeneration,
            presented: nodes[index]
        )
        accessibilityElementsByIndex[index] = element
        return element
    }

    public override func index(ofAccessibilityElement element: Any) -> Int {
        guard let element = element as? PreparedProseDrawingAccessibilityElement,
              element.drawingView === self,
              element.belongs(to: layout, generation: accessibilityPresentationGeneration)
        else { return NSNotFound }
        return element.index
    }

    private var accessibilityNodes: [ViewerTablePresentedAccessibilityNode] {
        presentationSnapshot()?.accessibilityNodes.map { presented in
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
        } ?? []
    }

    fileprivate func accessibilityFrame(
        for node: ViewerTablePresentedAccessibilityNode
    ) -> CGRect {
        guard self.layout != nil else { return .zero }
        let rect = clippedAccessibilityRects(for: node).reduce(CGRect.null) { $0.union($1) }
        guard !rect.isNull, !rect.isEmpty else { return .zero }
        return UIAccessibility.convertToScreenCoordinates(rect, in: self)
    }

    fileprivate func accessibilityPath(
        for node: ViewerTablePresentedAccessibilityNode
    ) -> UIBezierPath? {
        let rects = clippedAccessibilityRects(for: node)
        guard self.layout != nil, !rects.isEmpty else { return nil }
        let path = UIBezierPath()
        for rect in rects {
            path.append(UIBezierPath(rect: rect))
        }
        return UIAccessibility.convertToScreenCoordinates(path, in: self)
    }

    fileprivate func activateAccessibilityNode(
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
        accessibilityElementsByIndex.removeAll(keepingCapacity: true)
        UIAccessibility.post(notification: .layoutChanged, argument: nil)
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
        let snapshot = ViewerTablePresentation.project(layout: layout, owner: tablePresentationOwner, viewport: presentationViewport())
        onMountedTableCellsDrawnForTesting?(snapshot.mountedCells.count)

        context.saveGState()
        defer { context.restoreGState() }
        if let content = layout.decorations.first, let box = content.styleBox {
            context.addPath(box.path(in: content.bounds).cgPath)
            context.clip()
        }
        let mountedLayoutIDs = Set(snapshot.mountedCells.map { ObjectIdentifier($0.content) })
        let excludedLayoutID = excludedTableCellContentLayout.map(ObjectIdentifier.init)
        drawHierarchicalBackgrounds(snapshot, mountedLayoutIDs: mountedLayoutIDs, excludedLayoutID: excludedLayoutID, dirtyRect: rect, context: context)
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
        for cell in snapshot.mountedCells {
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
        PreparedProseInstrumentation.drew(drawStarted, visibleBlocks: visibleBlocks.count)
    }

    private func drawTableFailure(_ presented: ViewerTablePresentedBlock, context: CGContext) {
        guard let tableBounds = presented.block.tableBounds else { return }
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
        mountedLayoutIDs: Set<ObjectIdentifier>,
        excludedLayoutID: ObjectIdentifier?,
        dirtyRect: CGRect,
        context: CGContext
    ) {
        let layouts = Dictionary(uniqueKeysWithValues: snapshot.layouts.map { (ObjectIdentifier($0.layout), $0) })
        let blocks = Dictionary(grouping: snapshot.blocks, by: { ObjectIdentifier($0.layout) })
        let cells = Dictionary(grouping: snapshot.mountedCells, by: { ObjectIdentifier($0.surface) })

        func drawLayout(_ presented: ViewerTablePresentedLayout) {
            context.saveGState()
            context.clip(to: presented.clip)
            context.translateBy(x: presented.origin.x, y: presented.origin.y)
            for fragment in presented.layout.decorations {
                guard fragment.bounds.offsetBy(dx: presented.origin.x, dy: presented.origin.y).intersects(dirtyRect) else { continue }
                fragment.styleBox?.draw(in: fragment.bounds, context: context)
            }
            context.restoreGState()

            for block in blocks[ObjectIdentifier(presented.layout)] ?? [] {
                guard let surface = block.block.tableSurface else { continue }
                let surfaceCells = cells[ObjectIdentifier(surface)] ?? []
                for cell in surfaceCells where cell.cell.isHeader {
                    context.saveGState()
                    context.clip(to: cell.clip)
                    context.setFillColor(cell.surface.style.headerBackgroundColor.cgColor)
                    context.fill(cell.bounds)
                    context.restoreGState()
                }
                for cell in surfaceCells {
                    guard let child = layouts[ObjectIdentifier(cell.content)],
                          mountedLayoutIDs.contains(ObjectIdentifier(cell.content)),
                          ObjectIdentifier(cell.content) != excludedLayoutID
                    else { continue }
                    drawLayout(child)
                }
            }
        }

        guard let root = snapshot.layouts.first else { return }
        drawLayout(root)
    }

    private func drawTableChromeBorder(_ cell: ViewerTablePresentedCell, context: CGContext) {
        onTableChromeDrawnForTesting?(cell.sourcePosition)
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
    weak var rootLayout: PreparedProseLayout?
    weak var layout: PreparedProseLayout?
    let index: Int
    let generation: Int
    let presented: ViewerTablePresentedAccessibilityNode

    init(
        container: PreparedProseDrawingView,
        index: Int,
        rootLayout: PreparedProseLayout?,
        generation: Int,
        presented: ViewerTablePresentedAccessibilityNode
    ) {
        drawingView = container
        self.index = index
        self.rootLayout = rootLayout
        self.generation = generation
        self.presented = presented
        self.layout = presented.layout
        super.init(accessibilityContainer: container)
    }

    func belongs(to layout: PreparedProseLayout?, generation: Int) -> Bool {
        rootLayout === layout && self.generation == generation
    }

    override var accessibilityLabel: String? {
        get { presented.node.label }
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
            guard let drawingView, belongs(to: drawingView.layout, generation: drawingView.accessibilityPresentationGeneration) else { return .zero }
            return drawingView.accessibilityFrame(for: presented)
        }
        set { }
    }
    override var accessibilityPath: UIBezierPath? {
        get {
            guard let drawingView, belongs(to: drawingView.layout, generation: drawingView.accessibilityPresentationGeneration) else { return nil }
            return drawingView.accessibilityPath(for: presented)
        }
        set { }
    }
    override func accessibilityActivate() -> Bool {
        guard let drawingView, belongs(to: drawingView.layout, generation: drawingView.accessibilityPresentationGeneration) else { return false }
        return drawingView.activateAccessibilityNode(presented)
    }
}

private extension Array {
    subscript(safe index: Int) -> Element? { indices.contains(index) ? self[index] : nil }
}
