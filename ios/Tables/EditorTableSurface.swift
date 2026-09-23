import UIKit

final class EditorTableSurface: UIView {
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

    init(inputCoordinator: EditorTableInputCoordinator) {
        self.inputCoordinator = inputCoordinator
        super.init(frame: .zero)
        clipsToBounds = true
        drawingView.isOpaque = false
        drawingView.backgroundColor = .clear
        drawingView.isUserInteractionEnabled = false
        addSubview(drawingView)
        inputCoordinator.cellInput.isHidden = true
        addSubview(inputCoordinator.cellInput)
    }

    required init?(coder: NSCoder) {
        fatalError("init(coder:) has not been implemented")
    }

    func present(_ presentation: EditorV2Adapter.EditorTablePresentationSnapshot,
                 selection: EditorCellSelection?, from textView: EditorTextView) {
        latestPresentation = presentation
        if case let .drawable(tableID, sourcePositions) = selection {
            drawingView.selectedTableCellSourcePositions = [tableID: sourcePositions]
        } else {
            drawingView.selectedTableCellSourcePositions = [:]
        }
        reprepareIfNeeded(from: textView)
        updateGeometry(from: textView)
    }

    func invalidateAppearance() {
        appearanceRevision &+= 1
    }

    func clearPresentation() {
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
        drawingView.install(layout: nil)
    }

    func clearCellSelection() {
        drawingView.selectedTableCellSourcePositions = [:]
    }

    func updateGeometry(from textView: EditorTextView) {
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
        inputCoordinator.cellInput.frame = (cellFrame(tableID: tableID, cellIndex: cellIndex) ?? contentRect).integral
        inputCoordinator.cellInput.isHidden = false
    }

    func hideActiveInput() {
        activeCell = nil
        drawingView.excludedTableCellContentLayout = nil
        inputCoordinator.cellInput.isHidden = true
    }

    func cellFrame(tableID: String, cellIndex: UInt32) -> CGRect? {
        guard let entry = entries[tableID],
              let origin = tableOrigin(for: tableID),
              let cell = entry.surface.cells.first(where: { $0.sourceCellIndex == Int(cellIndex) })
        else { return nil }
        let inset = entry.surface.style.cellPadding + entry.surface.style.borderWidth
        return cell.frame.offsetBy(dx: origin.x, dy: origin.y).insetBy(dx: inset, dy: inset)
    }

    func cellHit(at point: CGPoint) -> RootTableCellHit? {
        for entry in entries.values {
            guard let origin = tableOrigin(for: entry.tableID) else { continue }
            for cell in entry.surface.cells {
                guard let sourceCellIndex = cell.sourceCellIndex,
                      cell.frame.offsetBy(dx: origin.x, dy: origin.y).contains(point),
                      !containsNestedTable(at: point, in: cell, tableOrigin: origin)
                else { continue }
                let inset = entry.surface.style.cellPadding + entry.surface.style.borderWidth
                return RootTableCellHit(
                    tableID: entry.tableID,
                    cellIndex: UInt32(sourceCellIndex),
                    contentRect: cell.frame.offsetBy(dx: origin.x, dy: origin.y).insetBy(dx: inset, dy: inset)
                )
            }
        }
        return nil
    }

    override func hitTest(_ point: CGPoint, with event: UIEvent?) -> UIView? {
        if !inputCoordinator.cellInput.isHidden,
           inputCoordinator.cellInput.frame.contains(point) {
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
                tableRecords: presentation.tableRecords
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

    private func tableOrigin(for tableID: String) -> CGPoint? {
        guard let origin = drawingView.layout?.blocks.first(where: { block in
            block.tableSurface === entries[tableID]?.surface
        })?.tableBounds?.origin else { return nil }
        return CGPoint(x: origin.x - drawingOffset.x, y: origin.y - drawingOffset.y)
    }

    private func containsNestedTable(
        at point: CGPoint,
        in cell: PreparedViewerTableCell,
        tableOrigin: CGPoint
    ) -> Bool {
        let contentOrigin = CGPoint(
            x: tableOrigin.x + cell.frame.minX + cell.contentOrigin.x,
            y: tableOrigin.y + cell.frame.minY + cell.contentOrigin.y
        )
        return cell.content.blocks.contains { block in
            guard let nestedBounds = block.tableBounds else { return false }
            return nestedBounds.offsetBy(dx: contentOrigin.x, dy: contentOrigin.y).contains(point)
        }
    }

    private func refreshActiveInputFrame() {
        guard let activeCell,
              let frame = cellFrame(tableID: activeCell.tableID, cellIndex: activeCell.cellIndex)
        else { return }
        inputCoordinator.cellInput.frame = frame.integral
    }

    private func updateExcludedCellContent() {
        guard let activeCell, let entry = entries[activeCell.tableID] else {
            drawingView.excludedTableCellContentLayout = nil
            return
        }
        drawingView.excludedTableCellContentLayout = entry.surface.cells.first {
            $0.sourceCellIndex == Int(activeCell.cellIndex)
        }?.content
    }

    private func sameSurfaces(
        _ left: [String: ViewerTableSurface],
        _ right: [String: ViewerTableSurface]
    ) -> Bool {
        guard left.count == right.count else { return false }
        return left.allSatisfy { tableID, surface in right[tableID] === surface }
    }
}
