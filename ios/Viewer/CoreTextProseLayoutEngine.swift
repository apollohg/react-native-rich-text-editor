import CoreText
import UIKit

let preparedAtomAttribute = NSAttributedString.Key("PREPPreparedAtom")
/// Core Text has no strikethrough attribute. This marks a shaped run so its
/// immutable strike rectangle can be prepared from Core Text's own metrics.
let preparedStrikeAttribute = NSAttributedString.Key("PREPPreparedStrike")

final class CoreTextProseLayoutEngine {
    var tableCellPreparationObserver: ((Int) -> Void)?
    var tableCellShapeBuildObserver: ((Int) -> Void)?
    var tableCellBindingObserver: ((Int) -> Void)?

    final class HighlightingScope {
        let configuration: NativeCodeHighlightConfiguration
        let generation: String
        private var identifiers: [String: Int] = [:]
        private(set) var blocks: [NativeCodeHighlightBlock] = []

        init(configuration: NativeCodeHighlightConfiguration, generation: String) {
            self.configuration = configuration
            self.generation = generation
        }

        func preassign(document: ViewerDocument) {
            for (index, block) in document.blocks.enumerated() {
                register(document: document, index: index, block: block)
                guard let table = block.table else { continue }
                for cell in table.cells.sorted(by: { $0.sourcePos < $1.sourcePos }) {
                    guard let child = try? document.cellDocument(for: cell) else { continue }
                    preassign(document: child)
                }
            }
        }

        func start(document: ViewerDocument, index: Int, block: ViewerBlock) -> Int? {
            guard EditorStyleSheet.element(block.nodeType) == "codeBlock" else { return nil }
            return identifiers[identifier(document: document, index: index)]
        }

        private func register(document: ViewerDocument, index: Int, block: ViewerBlock) {
            guard EditorStyleSheet.element(block.nodeType) == "codeBlock" else { return }
            let identifier = identifier(document: document, index: index)
            guard identifiers[identifier] == nil else { return }
            let text = block.inlines.compactMap { inline -> String? in
                if case let .text(value, _) = inline { return value }
                return nil
            }.joined()
            let start = blocks.count
            identifiers[identifier] = start
            blocks.append(NativeCodeHighlightBlock(start: start, text: text, language: block.language))
        }

        private func identifier(document: ViewerDocument, index: Int) -> String {
            "\(document.semanticKey):\(index)"
        }

        var request: PreparedViewerHighlightingRequest {
            PreparedViewerHighlightingRequest(configuration: configuration, generation: generation, blocks: blocks)
        }
    }
    /// UIFont and CTFont are toll-free bridged. Recreating a system font from
    /// UIFont.fontName is not equivalent on current iOS releases: private
    /// .SFUI names are not valid public Core Text PostScript names.
    static func coreTextFont(from font: UIFont) -> CTFont {
        font as CTFont
    }

    func prepare(
        document: ViewerDocument,
        key: ProseLayoutKey,
        widthPoints: CGFloat,
        displayScale: CGFloat,
        semanticGenerationIdentity: String? = nil,
        cellMode: Bool = false,
        highlightingScope: HighlightingScope? = nil,
        cellShapeContext: PreparedCellShapeBuildContext? = nil
    ) throws -> PreparedProseLayout {
        // This context is deliberately passed separately from the layout key's
        // revision-sensitive generation identity. A replacement layout for an
        // attachment/font/width revision must not reopen a missing-font warning.
        let warningSemanticGeneration = semanticGenerationIdentity ?? key.semanticGenerationIdentity
        guard let widthPixels = ProseLayoutMetrics.widthPixels(widthPoints: widthPoints, scale: displayScale) else {
            return .error(key: key, width: 0, error: .hostContract(message: "A finite positive width is required for prose measurement."))
        }
        let canonicalWidth = ProseLayoutMetrics.canonicalWidth(widthPixels: widthPixels, scale: displayScale)
        if document.isEmpty {
            return PreparedProseLayout(key: key, size: CGSize(width: canonicalWidth, height: 0), blocks: [], retainedBytes: document.retainedBytes)
        }

        let theme = document.preparedTheme ?? PreparedProseTheme.resolve(themeJSON: nil)
        let ownsHighlightingScope = highlightingScope == nil
        let scope = highlightingScope ?? theme.codeHighlighting.map { HighlightingScope(configuration: $0, generation: key.semanticGenerationIdentity) }
        if ownsHighlightingScope {
            scope?.preassign(document: document)
        }
        let highlighting = scope.flatMap { PreparedViewerHighlightingStore.result(for: $0.generation) }
        var cursorY = theme.contentInsets.top
        var blocks: [PreparedProseBlock] = []
        var containerBounds: [Int: CGRect] = [:]
        var containerStyles: [Int: (EditorStyleBox, Int)] = [:]
        var interactions: [PreparedProseInteraction] = []
        var accessibilityNodes: [PreparedProseAccessibilityNode] = []
        var imageAttachments: [ViewerImageAttachment] = []
        var retainedBytes = document.retainedBytes
        var listMarkersByIdentity: [Int: PreparedListMarker] = [:]
        for block in document.blocks {
            guard let boundary = block.listItemBoundary,
                  let context = block.listContext,
                  listMarkersByIdentity[boundary.identity] == nil
            else { continue }
            let markerNestingDepth = block.listItemAncestors.firstIndex { ancestor in
                ancestor.identity == boundary.identity
            } ?? 0
            listMarkersByIdentity[boundary.identity] = makeListMarker(
                context,
                nestingDepth: markerNestingDepth,
                paint: theme.paint(for: block),
                theme: theme,
                ancestors: block.markerStyleAncestors
            )
        }
        for (index, block) in document.blocks.enumerated() {
            let listMarker = block.listItemBoundary.flatMap { listMarkersByIdentity[$0.identity] }
            let nextAncestorIdentities = Set(
                document.blocks.indices.contains(index + 1)
                    ? listItemAncestors(document.blocks[index + 1]).map(\.identity)
                    : []
            )
            let disappearingListItemIdentities = Set(
                listItemAncestors(block)
                    .filter { !nextAncestorIdentities.contains($0.identity) }
                    .map(\.identity)
            )
            let priorIds = Set(index > 0 ? document.blocks[index - 1].styleAncestors.map(\.identity) : [])
            let followingIds = Set(document.blocks.indices.contains(index + 1) ? document.blocks[index + 1].styleAncestors.map(\.identity) : [])
            let opening = block.styleAncestors.filter { !priorIds.contains($0.identity) }
            let closing = block.styleAncestors.filter { !followingIds.contains($0.identity) }
            if let sheet = theme.styleSheet, index > 0 {
                let previous = document.blocks[index - 1]
                let shared = zip(previous.styleAncestors, block.styleAncestors).prefix { $0 == $1 }.count
                let previousSibling = previous.styleAncestors.dropFirst(shared).first?.nodeType ?? previous.nodeType
                let nextSibling = block.styleAncestors.dropFirst(shared).first?.nodeType ?? block.nodeType
                let previousMargin = sheet.box(previousSibling, ancestors: previous.styleAncestors.prefix(shared).map(\.nodeType)).margin.bottom
                let nextMargin = sheet.box(nextSibling, ancestors: block.styleAncestors.prefix(shared).map(\.nodeType)).margin.top
                cursorY -= previousMargin + nextMargin - EditorStyleSheet.collapsedMargin(previousMargin, nextMargin)
            }
            let omitBottomMargin = block.nodeType == "paragraph"
                && block.styleAncestors.last.map { $0.nodeType == "blockquote" && closing.contains($0) } == true
            let top = opening.reduce(CGFloat.zero) { $0 + (theme.styleSheet?.box($1.nodeType, ancestors: block.ancestors(before: $1)).outerInsets.top ?? 0) }
            let bottom = closing.reduce(CGFloat.zero) { $0 + (theme.styleSheet?.box($1.nodeType, ancestors: block.ancestors(before: $1)).outerInsets.bottom ?? 0) }
            cursorY += top
            if let table = block.table {
                var cellTheme = theme
                cellTheme.contentInsets = .zero
                let tableBox = theme.styleSheet?.box(block.nodeType, ancestors: block.styleAncestors.map(\.nodeType)) ?? EditorStyleBox()
                let placement = listPlacement(block: block, listMarker: listMarker, theme: theme, sheet: theme.styleSheet, paint: theme.paint(for: block), box: tableBox, omitBottomMargin: omitBottomMargin, disappearingListItemIdentities: disappearingListItemIdentities)
                let tableAncestors = block.styleAncestors.reduce(UIEdgeInsets.zero) {
                    $0.adding(theme.styleSheet?.box($1.nodeType, ancestors: block.ancestors(before: $1)).outerInsets ?? .zero)
                }
                let tableX = theme.contentInsets.left + tableAncestors.left + tableBox.margin.left + placement.listInset + placement.quoteInset + tableBox.inset.left
                let tableWidth = max(1, canonicalWidth - theme.contentInsets.left - theme.contentInsets.right
                    - tableAncestors.left - tableAncestors.right - tableBox.margin.left - tableBox.margin.right - tableBox.inset.left - tableBox.inset.right - placement.listInset - placement.quoteInset)
                let record = TableGridRecord(table: table, documentOwner: document.semanticKey)
                let cellsByPosition = Dictionary(uniqueKeysWithValues: table.cells.map { (Int($0.sourcePos), $0) })
                let surface = ViewerTableSurface(
                    identity: "t\(table.tablePos)",
                    record: record,
                    viewportWidth: tableWidth,
                    style: theme.tableStyle,
                    direction: table.direction == "rtl" ? .rightToLeft : (table.direction == "ltr" ? .leftToRight : UIView.userInterfaceLayoutDirection(for: .unspecified) == .rightToLeft ? .rightToLeft : .leftToRight),
                    displayScale: displayScale,
                    themeDigest: key.themeDigest,
                    fontEnvironmentRevision: Int(key.fontEnvironmentRevision),
                    textScale: theme.fontScale,
                    sourceTable: table,
                    sourceAttributes: document.tableAttributes
                ) { cell, cellWidth in
                    guard let source = cellsByPosition[cell.sourcePosition],
                          let child = try? document.cellDocument(for: source).withPreparedTheme(cellTheme)
                    else {
                        return .error(key: key, width: cellWidth, error: .layout(message: "Invalid table cell."))
                    }
                    do {
                        guard let cellWidthPixels = ProseLayoutMetrics.widthPixels(widthPoints: cellWidth, scale: displayScale) else {
                            return .error(key: key, width: cellWidth, error: .hostContract(message: "A finite positive table cell width is required."))
                        }
                        let cellKey = ProseLayoutKey(
                            semanticKey: child.semanticKey,
                            widthPixels: cellWidthPixels,
                            themeDigest: key.themeDigest,
                            nativeFontRevision: key.nativeFontRevision,
                            fontEnvironmentRevision: key.fontEnvironmentRevision,
                            displayScale: displayScale,
                            attachmentRevision: key.attachmentRevision,
                            generationIdentity: key.generationIdentity,
                            semanticGenerationIdentity: key.semanticGenerationIdentity
                        )
                        let build = {
                            self.tableCellPreparationObserver?(cell.sourcePosition)
                            self.tableCellShapeBuildObserver?(cell.sourcePosition)
                            let prepared = try self.prepare(
                                document: child,
                                key: cellKey,
                                widthPoints: cellWidth,
                                displayScale: displayScale,
                                semanticGenerationIdentity: warningSemanticGeneration,
                                cellMode: true,
                                highlightingScope: scope,
                                cellShapeContext: cellShapeContext
                            )
                            self.tableCellBindingObserver?(cell.sourcePosition)
                            return prepared
                        }
                        guard let cellShapeContext, theme.codeHighlighting == nil else { return try build() }
                        let shapeKey = preparedCellShapeKey(
                            contentKey: cell.contentKey,
                            document: child,
                            widthPixels: cellWidthPixels,
                            theme: cellTheme,
                            key: cellKey
                        )
                        return try cellShapeContext.resolve(shapeKey, build: build) { shape in
                            let bound = self.bindCellShape(
                                shape,
                                document: child,
                                key: cellKey,
                                widthPoints: cellWidth,
                                displayScale: displayScale,
                                theme: cellTheme,
                                warningSemanticGeneration: warningSemanticGeneration,
                                context: cellShapeContext
                            )
                            if bound != nil { self.tableCellBindingObserver?(cell.sourcePosition) }
                            return bound
                        }
                    } catch let error as ProseViewerError {
                        return .error(key: key, width: cellWidth, error: error)
                    } catch {
                        return .error(key: key, width: cellWidth, error: .layout(message: "Table cell preparation failed."))
                    }
                }
                let bounds = CGRect(x: tableX, y: cursorY + tableBox.margin.top, width: surface.bounds.width, height: surface.bounds.height)
                if let error = surface.preparationError {
                    return .error(key: key, width: canonicalWidth, error: error)
                }
                var fragments: [PreparedProseFragment] = []
                if let marker = placement.marker {
                    let markerHeight = marker.ascent + marker.descent
                    let markerTop = bounds.midY - markerHeight / 2
                    let markerX = tableX - placement.markerGutter
                    fragments.append(.init(kind: .marker, line: marker.line, origin: CGPoint(x: markerX, y: markerTop + marker.ascent), bounds: CGRect(x: markerX, y: markerTop, width: marker.width, height: markerHeight), color: placement.markerColor.cgColor, label: marker.label, checked: marker.checked, styleBox: placement.checkbox))
                }
                let blockBounds = fragments.reduce(bounds) { $0.union($1.bounds) }
                let tableBlock = PreparedProseBlock(fragments: fragments, bounds: blockBounds, tableSurface: surface, tableBounds: bounds)
                blocks.append(tableBlock)
                if !cellMode {
                    let childAttachments = surface.parentImageAttachments(offset: imageAttachments.count, tableOrigin: bounds.origin)
                    guard imageAttachments.count + childAttachments.count <= ViewerImageAttachment.maximumAdmittedAttachments else {
                        return .error(key: key, width: canonicalWidth, error: .layout(message: "The table exceeds the maximum admitted image attachment count."))
                    }
                    imageAttachments.append(contentsOf: childAttachments)
                    retainedBytes += childAttachments.count * 128
                }
                if let sheet = theme.styleSheet {
                    for (depth, ancestor) in block.styleAncestors.enumerated() {
                        let ancestorBox = sheet.box(ancestor.nodeType, ancestors: block.ancestors(before: ancestor))
                        containerBounds[ancestor.identity] = containerBounds[ancestor.identity].map { $0.union(bounds) } ?? bounds
                        containerStyles[ancestor.identity] = (ancestorBox, depth)
                    }
                }
                cursorY = bounds.maxY + bottom + placement.itemSpacing
                retainedBytes += tableBlock.estimatedRetainedBytes
                continue
            }
            let prepared = prepareBlock(
                block,
                highlighting: scope.flatMap { $0.start(document: document, index: index, block: block) }.flatMap { highlighting?.ranges[$0] } ?? [],
                attachmentOrdinal: imageAttachments.count,
                listMarker: listMarker,
                theme: theme,
                width: canonicalWidth,
                cursorY: cursorY,
                omitBottomMargin: omitBottomMargin,
                disappearingListItemIdentities: disappearingListItemIdentities,
                displayScale: displayScale,
                warningSemanticGeneration: warningSemanticGeneration
            )
            let preparedBlockIndex = blocks.count
            blocks.append(prepared.block)
            let interactionIndexOffset = interactions.count
            accessibilityNodes.append(contentsOf: prepared.accessibilityNodes.map { node in
                PreparedProseAccessibilityNode(
                    interactionIndex: node.interactionIndex.map { interactionIndexOffset + $0 },
                    role: node.role,
                    label: node.label,
                    rects: node.rects,
                    sourceBlockIndex: preparedBlockIndex
                )
            })
            interactions.append(contentsOf: prepared.interactions.map {
                PreparedProseInteraction(
                    kind: $0.kind,
                    rects: $0.rects,
                    href: $0.href,
                    visibleText: $0.visibleText,
                    docPos: $0.docPos,
                    label: $0.label,
                    attrsJSON: $0.attrsJSON,
                    sourceBlockIndex: preparedBlockIndex
                )
            })
            if let attachment = prepared.attachment { imageAttachments.append(attachment) }
            if let sheet = theme.styleSheet {
                var outerLeft: CGFloat = 0
                var outerRight: CGFloat = 0
                for (depth, ancestor) in block.styleAncestors.enumerated() {
                    let box = sheet.box(ancestor.nodeType, ancestors: block.ancestors(before: ancestor))
                    let remaining = block.styleAncestors.dropFirst(depth + 1)
                    let innerTop = remaining.reduce(CGFloat.zero) { $0 + (opening.contains($1) ? sheet.box($1.nodeType, ancestors: block.ancestors(before: $1)).outerInsets.top : 0) }
                    let innerBottom = remaining.reduce(CGFloat.zero) { $0 + (closing.contains($1) ? sheet.box($1.nodeType, ancestors: block.ancestors(before: $1)).outerInsets.bottom : 0) }
                    let y = cursorY - innerTop - (opening.contains(ancestor) ? box.inset.top : 0)
                    let end = prepared.nextY + innerBottom + (closing.contains(ancestor) ? box.inset.bottom : 0)
                    let rect = CGRect(
                        x: theme.contentInsets.left + outerLeft + box.margin.left,
                        y: y,
                        width: max(1, canonicalWidth - theme.contentInsets.left - theme.contentInsets.right - outerLeft - outerRight - box.margin.left - box.margin.right),
                        height: max(0, end - y)
                    )
                    containerBounds[ancestor.identity] = containerBounds[ancestor.identity].map { $0.union(rect) } ?? rect
                    containerStyles[ancestor.identity] = (box, depth)
                    outerLeft += box.outerInsets.left
                    outerRight += box.outerInsets.right
                }
            }
            cursorY = prepared.nextY + bottom
            retainedBytes += prepared.retainedBytes
        }
        let renderedBottom = blocks.map(\.bounds.maxY).max() ?? cursorY
        if !cellMode, imageAttachments.count > ViewerImageAttachment.maximumAdmittedAttachments {
            return .error(key: key, width: canonicalWidth, error: .layout(message: "The document exceeds the maximum admitted image attachment count."))
        }
        cursorY = (theme.styleSheet == nil ? renderedBottom : max(cursorY, renderedBottom)) + theme.contentInsets.bottom
        var decorations = containerBounds.keys.sorted { (containerStyles[$0]?.1 ?? 0) < (containerStyles[$1]?.1 ?? 0) }.compactMap { identity -> PreparedProseFragment? in
            guard let bounds = containerBounds[identity], let style = containerStyles[identity]?.0 else { return nil }
            return PreparedProseFragment(kind: .background, bounds: bounds, styleBox: style)
        }
        if let sheet = theme.styleSheet, !cellMode {
            decorations.insert(PreparedProseFragment(kind: .background, bounds: CGRect(x: 0, y: 0, width: canonicalWidth, height: cursorY), styleBox: sheet.box("content")), at: 0)
        }
        let highlightingRequest = cellMode ? nil : scope?.request
        retainedBytes += decorations.count * 512 + (highlightingRequest?.retainedBytes ?? 0) + (cellMode ? 0 : highlighting?.retainedBytes ?? 0)
        let pixelHeight = ceil(cursorY * displayScale)
        retainedBytes += interactions.reduce(0) { $0 + $1.estimatedRetainedBytes }
            + accessibilityNodes.reduce(0) { $0 + $1.estimatedRetainedBytes }
        // Mounted image-publication sidecars are runtime surface ownership,
        // not immutable artifact/cache ownership; account them at the host.
        return PreparedProseLayout(
            key: key,
            size: CGSize(width: canonicalWidth, height: pixelHeight / displayScale),
            blocks: blocks,
            interactions: interactions,
            accessibilityNodes: accessibilityNodes,
            imageAttachments: imageAttachments,
            retainedBytes: retainedBytes,
            decorations: decorations,
            highlightingRequest: highlightingRequest,
            highlightingResolved: highlighting != nil
        )
    }

    /// Reuses local Core Text geometry while rebuilding source-qualified cell metadata.
    private func bindCellShape(
        _ shape: PreparedCellShape,
        document: ViewerDocument,
        key: ProseLayoutKey,
        widthPoints: CGFloat,
        displayScale: CGFloat,
        theme: PreparedProseTheme,
        warningSemanticGeneration: String,
        context: PreparedCellShapeBuildContext
    ) -> PreparedProseLayout? {
        let local = shape.localLayout
        guard local.error == nil, local.blocks.count == document.blocks.count else { return nil }
        replayInlineFontWarnings(
            in: document,
            theme: theme,
            semanticGeneration: warningSemanticGeneration
        )
        var blocks: [PreparedProseBlock] = []
        var interactions: [PreparedProseInteraction] = []
        var accessibility: [PreparedProseAccessibilityNode] = []
        var interactionIndexes: [Int: Int] = [:]

        for (index, current) in document.blocks.enumerated() {
            let localBlock = local.blocks[index]
            var atomSlot = localBlock.atomSlot
            if let slot = localBlock.atomSlot {
                guard current.isBlockAtom,
                      case let .some(.atom(nodeType, docPos, attrsJSON, _)) = current.inlines.first,
                      nodeType == slot.nodeType
                else { return nil }
                atomSlot = PreparedProseAtomSlot(nodeType: nodeType, docPos: docPos, attrsJSON: attrsJSON, bounds: slot.bounds)
            } else if current.isBlockAtom,
                      case let .some(.atom(nodeType, _, _, _)) = current.inlines.first,
                      theme.viewerAtoms?.nodeTypes.contains(nodeType) == true {
                return nil
            }

            var tableSurface: ViewerTableSurface?
            if let table = current.table {
                guard let cachedSurface = localBlock.tableSurface,
                      let tableBounds = localBlock.tableBounds,
                      let bound = bindTableSurface(
                        table,
                        document: document,
                        cachedSurface: cachedSurface,
                        tableBounds: tableBounds,
                        key: key,
                        displayScale: displayScale,
                        theme: theme,
                        warningSemanticGeneration: warningSemanticGeneration,
                        context: context
                      )
                else { return nil }
                tableSurface = bound
            } else if localBlock.tableSurface != nil {
                return nil
            }

            var imageAttachment: ViewerImageAttachment?
            if let cachedImage = localBlock.imageAttachment {
                guard let currentImage = ViewerImageAttachment.sourceAndDeclaredSize(in: current) else { return nil }
                imageAttachment = ViewerImageAttachment(
                    ordinal: cachedImage.ordinal,
                    id: currentImage.id,
                    source: currentImage.source,
                    bounds: cachedImage.bounds,
                    declaredSize: currentImage.declaredSize
                )
            } else if current.nodeType == "image" {
                return nil
            }

            let currentInteractions = semanticBindings(for: current)
            let localInteractionIndexes = local.interactions.indices.filter { local.interactions[$0].sourceBlockIndex == index }
            let localInteractions = localInteractionIndexes.map { local.interactions[$0] }
            guard currentInteractions.count == localInteractions.count else { return nil }
            for (localIndex, pair) in zip(localInteractionIndexes, zip(localInteractions, currentInteractions)) {
                let (prepared, currentInteraction) = pair
                guard prepared.kind == currentInteraction.kind,
                      prepared.href == currentInteraction.href,
                      prepared.visibleText == currentInteraction.visibleText
                else { return nil }
                interactionIndexes[localIndex] = interactions.count
                interactions.append(PreparedProseInteraction(
                    kind: prepared.kind,
                    rects: prepared.rects,
                    href: prepared.href,
                    visibleText: prepared.visibleText,
                    docPos: currentInteraction.docPos,
                    label: currentInteraction.label,
                    attrsJSON: currentInteraction.attrsJSON,
                    sourceBlockIndex: index
                ))
            }
            for node in local.accessibilityNodes where node.sourceBlockIndex == index {
                accessibility.append(PreparedProseAccessibilityNode(
                    interactionIndex: node.interactionIndex.flatMap { interactionIndexes[$0] },
                    role: node.role,
                    label: node.label,
                    rects: node.rects,
                    sourceBlockIndex: index
                ))
            }
            blocks.append(PreparedProseBlock(
                fragments: localBlock.fragments,
                bounds: localBlock.bounds,
                atomSlot: atomSlot,
                imageAttachment: imageAttachment,
                tableSurface: tableSurface,
                tableBounds: localBlock.tableBounds
            ))
        }

        guard interactions.count == local.interactions.count else { return nil }
        return PreparedProseLayout(
            key: key,
            size: local.size,
            blocks: blocks,
            interactions: interactions,
            accessibilityNodes: accessibility,
            imageAttachments: blocks.compactMap(\.imageAttachment),
            retainedBytes: local.retainedBytes,
            decorations: local.decorations,
            highlightingRequest: nil,
            highlightingResolved: local.highlightingResolved
        )
    }

    private func bindTableSurface(
        _ table: FfiViewerTable,
        document: ViewerDocument,
        cachedSurface: ViewerTableSurface,
        tableBounds: CGRect,
        key: ProseLayoutKey,
        displayScale: CGFloat,
        theme: PreparedProseTheme,
        warningSemanticGeneration: String,
        context: PreparedCellShapeBuildContext
    ) -> ViewerTableSurface? {
        let cellsByPosition = Dictionary(uniqueKeysWithValues: table.cells.map { (Int($0.sourcePos), $0) })
        var childTheme = theme
        childTheme.contentInsets = .zero
        let surface = ViewerTableSurface(
            identity: "t\(table.tablePos)",
            record: TableGridRecord(table: table, documentOwner: document.semanticKey),
            viewportWidth: cachedSurface.hostViewportWidth,
            style: cachedSurface.style,
            direction: cachedSurface.direction,
            displayScale: displayScale,
            themeDigest: key.themeDigest,
            fontEnvironmentRevision: Int(key.fontEnvironmentRevision),
            textScale: theme.fontScale,
            sourceTable: table,
            sourceAttributes: document.tableAttributes
        ) { cell, cellWidth in
            guard let source = cellsByPosition[cell.sourcePosition],
                  let child = try? document.cellDocument(for: source).withPreparedTheme(childTheme),
                  let widthPixels = ProseLayoutMetrics.widthPixels(widthPoints: cellWidth, scale: displayScale)
            else { return .error(key: key, width: cellWidth, error: .layout(message: "Invalid table cell.")) }
            let childKey = ProseLayoutKey(
                semanticKey: child.semanticKey,
                widthPixels: widthPixels,
                themeDigest: key.themeDigest,
                nativeFontRevision: key.nativeFontRevision,
                fontEnvironmentRevision: key.fontEnvironmentRevision,
                displayScale: displayScale,
                attachmentRevision: key.attachmentRevision,
                generationIdentity: key.generationIdentity,
                semanticGenerationIdentity: key.semanticGenerationIdentity
            )
            let shapeKey = preparedCellShapeKey(contentKey: cell.contentKey, document: child, widthPixels: widthPixels, theme: childTheme, key: childKey)
            do {
                return try context.resolve(shapeKey, build: {
                    self.tableCellPreparationObserver?(cell.sourcePosition)
                    self.tableCellShapeBuildObserver?(cell.sourcePosition)
                    let prepared = try self.prepare(document: child, key: childKey, widthPoints: cellWidth, displayScale: displayScale, semanticGenerationIdentity: warningSemanticGeneration, cellMode: true, highlightingScope: nil, cellShapeContext: context)
                    self.tableCellBindingObserver?(cell.sourcePosition)
                    return prepared
                }) { nestedShape in
                    let bound = self.bindCellShape(nestedShape, document: child, key: childKey, widthPoints: cellWidth, displayScale: displayScale, theme: childTheme, warningSemanticGeneration: warningSemanticGeneration, context: context)
                    if bound != nil { self.tableCellBindingObserver?(cell.sourcePosition) }
                    return bound
                }
            } catch {
                return .error(key: childKey, width: cellWidth, error: .layout(message: "Table cell preparation failed."))
            }
        }
        guard abs(surface.bounds.width - tableBounds.width) <= 1,
              abs(surface.bounds.height - tableBounds.height) <= 1,
              surface.preparationError == nil
        else { return nil }
        return surface
    }

    private func semanticBindings(for block: ViewerBlock) -> [PreparedProseInteraction] {
        var result: [PreparedProseInteraction] = []
        for inline in block.inlines {
            switch inline {
            case let .text(text, marks):
                guard let href = href(in: marks), !text.isEmpty else { continue }
                if let previous = result.last, previous.kind == .link, previous.href == href {
                    result[result.count - 1] = PreparedProseInteraction(kind: .link, rects: [], href: href, visibleText: previous.visibleText + text, docPos: nil, label: previous.label + text, attrsJSON: nil)
                } else {
                    result.append(PreparedProseInteraction(kind: .link, rects: [], href: href, visibleText: text, docPos: nil, label: text, attrsJSON: nil))
                }
            case let .atom(nodeType, docPos, attrsJSON, label):
                guard nodeType == "mention" else { continue }
                result.append(PreparedProseInteraction(kind: .mention, rects: [], href: nil, visibleText: label, docPos: docPos, label: label, attrsJSON: attrsJSON))
            }
        }
        return result
    }

    private func replayInlineFontWarnings(
        in document: ViewerDocument,
        theme: PreparedProseTheme,
        semanticGeneration: String
    ) {
        for block in document.blocks {
            let paint = theme.paint(for: block)
            let ancestors = block.styleAncestors.map(\.nodeType) + [block.nodeType]
            for inline in block.inlines {
                guard case let .text(_, marks) = inline else { continue }
                _ = attributes(
                    for: marks,
                    paint: paint,
                    theme: theme,
                    warningSemanticGeneration: semanticGeneration,
                    ancestors: ancestors
                )
            }
        }
    }

    private func listItemAncestors(_ block: ViewerBlock) -> [ViewerListItemAncestor] {
        if !block.listItemAncestors.isEmpty {
            return block.listItemAncestors
        }
        guard let boundary = block.listItemBoundary,
              let context = block.listContext
        else { return [] }
        return [ViewerListItemAncestor(identity: boundary.identity, context: context)]
    }

    private struct BlockPreparation {
        let block: PreparedProseBlock
        let interactions: [PreparedProseInteraction]
        let accessibilityNodes: [PreparedProseAccessibilityNode]
        let attachment: ViewerImageAttachment?
        let nextY: CGFloat
        let retainedBytes: Int
    }

    private struct ListPlacement {
        let marker: PreparedListMarker?
        let markerGutter: CGFloat
        let listInset: CGFloat
        let quoteInset: CGFloat
        let markerColor: UIColor
        let checkbox: EditorStyleBox?
        let itemSpacing: CGFloat
    }

    private func listPlacement(
        block: ViewerBlock,
        listMarker: PreparedListMarker?,
        theme: PreparedProseTheme,
        sheet: EditorStyleSheet?,
        paint: PreparedTextPaint,
        box: EditorStyleBox,
        omitBottomMargin: Bool,
        disappearingListItemIdentities: Set<Int>
    ) -> ListPlacement {
        let listDepth = block.listContext == nil ? 0 : (block.listItemBoundary.map { Int($0.nestingDepth) } ?? max(0, Int(block.depth) - 1))
        let fallbackMarkerNestingDepth = max(0, block.listItemAncestors.count - 1)
        let measured = listMarker ?? block.listContext.map { makeListMarker($0, nestingDepth: fallbackMarkerNestingDepth, paint: paint, theme: theme, ancestors: block.markerStyleAncestors) }
        let marker = block.listItemBoundary.map { $0.isFirstRenderableLeaf ? measured : nil } ?? measured
        let listName = block.listContext.map { $0.kind == "task" ? "taskList" : $0.ordered ? "orderedList" : "bulletList" } ?? "bulletList"
        let values = sheet?[listName] ?? [:]
        let indent = EditorTheme.cgFloat(values["indent"]) ?? theme.listIndent
        let multiplier = EditorTheme.cgFloat(values["baseIndentMultiplier"]) ?? theme.listBaseIndentMultiplier
        let base = block.listContext == nil ? 0 : max(0, indent * multiplier)
        let nested = block.listContext == nil ? 0 : max(0, indent * CGFloat(listDepth))
        var contextual: CGFloat = 0
        if let sheet, block.listContext != nil {
            let lists = block.styleAncestors.filter { ["bulletList", "orderedList", "taskList"].contains(EditorStyleSheet.element($0.nodeType)) }
            for (depth, ancestor) in lists.enumerated() {
                let baseValues = sheet[ancestor.nodeType]
                let resolved = sheet.resolvedValues(ancestor.nodeType, ancestors: block.ancestors(before: ancestor))
                let baseIndent = EditorTheme.cgFloat(baseValues["indent"]) ?? theme.listIndent
                let resolvedIndent = EditorTheme.cgFloat(resolved["indent"]) ?? theme.listIndent
                let baseMultiplier = depth == 0 ? EditorTheme.cgFloat(baseValues["baseIndentMultiplier"]) ?? theme.listBaseIndentMultiplier : 1
                let resolvedMultiplier = depth == 0 ? EditorTheme.cgFloat(resolved["baseIndentMultiplier"]) ?? theme.listBaseIndentMultiplier : 1
                contextual += max(0, resolvedIndent * resolvedMultiplier) - max(0, baseIndent * baseMultiplier)
            }
        }
        let markerValues = sheet?.resolvedValues("listMarker", ancestors: block.markerStyleAncestors) ?? [:]
        let color = EditorTheme.color(from: markerValues["color"]) ?? theme.listMarkerColor
        let checkbox = block.listContext.flatMap { $0.kind == "task" ? sheet?.checkbox(checked: $0.checked, ancestors: block.markerStyleAncestors) : nil }
        let gap = checkbox?.number("gap", fallback: 8) ?? EditorTheme.cgFloat(markerValues["gap"]) ?? theme.listMarkerGap
        let gutter = measured.map { max(gap, $0.width + gap) } ?? 0
        let spacing: CGFloat
        if sheet != nil { spacing = omitBottomMargin ? 0 : box.margin.bottom } else if block.listContext == nil { spacing = paint.spacingAfter } else {
            spacing = listItemAncestors(block).reduce(CGFloat.zero) { result, ancestor in
                if disappearingListItemIdentities.contains(ancestor.identity) { return result + (ancestor.context.isLast ? theme.listSpacingAfter : theme.listItemSpacing) }
                if ancestor.identity == block.listItemBoundary?.identity, block.listItemBoundary?.isFinalRenderableLeaf == true { return result + theme.listItemSpacing }
                return result
            }
        }
        return ListPlacement(marker: marker, markerGutter: gutter, listInset: base + nested + contextual + gutter, quoteInset: block.inBlockquote ? theme.quoteBorderWidth + theme.quoteMarkerGap + theme.quoteIndent : 0, markerColor: color, checkbox: checkbox, itemSpacing: spacing)
    }

    private func prepareBlock(
        _ block: ViewerBlock,
        highlighting: [NativeCodeHighlightRange],
        attachmentOrdinal: Int,
        listMarker: PreparedListMarker?,
        theme: PreparedProseTheme,
        width: CGFloat,
        cursorY: CGFloat,
        omitBottomMargin: Bool,
        disappearingListItemIdentities: Set<Int>,
        displayScale: CGFloat,
        warningSemanticGeneration: String
    ) -> BlockPreparation {
        let sheet = theme.styleSheet
        let box = sheet?.box(block.nodeType, ancestors: block.styleAncestors.map(\.nodeType)) ?? EditorStyleBox()
        let ancestors = block.styleAncestors.reduce(UIEdgeInsets.zero) { $0.adding(sheet?.box($1.nodeType, ancestors: block.ancestors(before: $1)).outerInsets ?? .zero) }
        let cursorY = cursorY + box.margin.top
        let contentX = theme.contentInsets.left + ancestors.left + box.margin.left
        let contentWidth = max(1, width - theme.contentInsets.left - theme.contentInsets.right - ancestors.left - ancestors.right - box.margin.left - box.margin.right)
        let paint = theme.paint(for: block)
        let placement = listPlacement(block: block, listMarker: listMarker, theme: theme, sheet: sheet, paint: paint, box: box, omitBottomMargin: omitBottomMargin, disappearingListItemIdentities: disappearingListItemIdentities)
        let marker = placement.marker
        let markerGutter = placement.markerGutter
        let listInset = placement.listInset
        let quoteInset = placement.quoteInset
        let markerColor = placement.markerColor
        let checkbox = placement.checkbox
        let codeInset = block.nodeType == "codeBlock" ? theme.codePaddingHorizontal : 0
        let textX = contentX + listInset + quoteInset + codeInset + box.inset.left
        let itemSpacing = placement.itemSpacing
        if block.isBlockAtom, let atoms = theme.viewerAtoms,
           atoms.nodeTypes.contains(block.nodeType),
           case let .atom(nodeType, docPos, attrsJSON, _)? = block.inlines.first {
            let slotWidth = max(1, contentWidth - listInset - quoteInset)
            let height = atoms.height(nodeType: nodeType, docPos: docPos, width: slotWidth)
            let bounds = CGRect(x: textX, y: cursorY, width: slotWidth, height: height)
            var fragments: [PreparedProseFragment] = []
            if block.inBlockquote {
                fragments.append(.init(
                    kind: .border,
                    bounds: CGRect(
                        x: contentX,
                        y: cursorY,
                        width: theme.quoteBorderWidth,
                        height: height
                    ),
                    color: theme.quoteBorderColor.cgColor,
                    strokeWidth: theme.quoteBorderWidth
                ))
            }
            if let marker {
                let markerX = textX - markerGutter
                let markerTop = cursorY + max(0, (height - marker.ascent - marker.descent) / 2)
                let markerBounds = CGRect(
                    x: markerX,
                    y: markerTop,
                    width: marker.width,
                    height: marker.ascent + marker.descent
                )
                fragments.append(.init(
                    kind: .marker,
                    line: marker.line,
                    origin: CGPoint(x: markerX, y: markerTop + marker.ascent),
                    bounds: markerBounds,
                    color: markerColor.cgColor,
                    label: marker.label,
                    checked: marker.checked,
                    styleBox: checkbox
                ))
            }
            let blockBounds = fragments.reduce(bounds) { $0.union($1.bounds) }
            let prepared = PreparedProseBlock(
                fragments: fragments,
                bounds: blockBounds,
                atomSlot: PreparedProseAtomSlot(nodeType: nodeType, docPos: docPos, attrsJSON: attrsJSON, bounds: bounds)
            )
            return BlockPreparation(block: prepared, interactions: [], accessibilityNodes: [], attachment: nil, nextY: blockBounds.maxY + itemSpacing, retainedBytes: prepared.estimatedRetainedBytes)
        }
        if block.nodeType == "image", let image = ViewerImageAttachment.sourceAndDeclaredSize(in: block) {
            let availableImageWidth = max(1, contentWidth - listInset - quoteInset - box.inset.left - box.inset.right)
            let imageWidth = sheet == nil ? availableImageWidth : min(availableImageWidth, image.declaredSize?.width ?? availableImageWidth)
            let provisionalHeight = max(44, min(240, imageWidth * 0.56))
            let declared = image.declaredSize
            let resolvedSize = declared ?? ViewerImageIntrinsicStore.shared.size(for: image.id, source: image.source)
            let height = resolvedSize.map { imageWidth * $0.height / max(1, $0.width) } ?? provisionalHeight
            let bounds = CGRect(x: textX - box.inset.left, y: cursorY, width: imageWidth + box.inset.left + box.inset.right, height: height + box.inset.top + box.inset.bottom)
            let attachment = ViewerImageAttachment(ordinal: attachmentOrdinal, id: image.id, source: image.source, bounds: bounds, declaredSize: declared)
            let fragments = [PreparedProseFragment(kind: .image, bounds: bounds, color: UIColor.systemGray5.cgColor, styleBox: sheet == nil ? nil : box)]
            let prepared = PreparedProseBlock(fragments: fragments, bounds: bounds, imageAttachment: attachment)
            let imageLabel = block.inlines.compactMap { inline -> String? in
                guard case let .atom("image", _, attrsJSON, _) = inline else { return nil }
                let alt = jsonDictionary(attrsJSON)["alt"] as? String
                return alt?.trimmingCharacters(in: .whitespacesAndNewlines)
            }.first
            let accessibleImageLabel = imageLabel.flatMap { $0.isEmpty ? nil : $0 } ?? "Image"
            let node = PreparedProseAccessibilityNode(
                interactionIndex: nil,
                role: .image,
                label: accessibleImageLabel,
                bounds: bounds
            )
            return BlockPreparation(block: prepared, interactions: [], accessibilityNodes: [node], attachment: attachment, nextY: bounds.maxY + itemSpacing, retainedBytes: prepared.estimatedRetainedBytes + 192)
        }
        if block.nodeType == "horizontalRule" || block.nodeType == "horizontal_rule" {
            let thickness = sheet == nil ? theme.ruleThickness : box.number("height", fallback: theme.ruleThickness)
            let ruleX = contentX + listInset + quoteInset + box.inset.left
            let ruleWidth = max(1, contentWidth - listInset - quoteInset - box.inset.left - box.inset.right)
            let y = cursorY + theme.ruleMargin + box.inset.top
            let rule = CGRect(x: ruleX, y: y, width: ruleWidth, height: thickness)
            var fragments: [PreparedProseFragment] = [.init(kind: .rule, bounds: rule, color: theme.ruleColor.cgColor, strokeWidth: thickness)]
            let totalEnd = y + thickness + theme.ruleMargin + box.inset.bottom
            if sheet != nil {
                fragments = [.init(kind: .background, bounds: CGRect(x: contentX, y: cursorY, width: contentWidth, height: totalEnd - cursorY), styleBox: box)]
            }
            if block.inBlockquote {
                fragments.append(.init(kind: .border, bounds: CGRect(x: contentX, y: cursorY, width: theme.quoteBorderWidth, height: totalEnd - cursorY), color: theme.quoteBorderColor.cgColor, strokeWidth: theme.quoteBorderWidth))
            }
            if let marker {
                let markerX = textX - markerGutter
                let markerHeight = marker.ascent + marker.descent
                let markerTop = cursorY + (totalEnd - cursorY - markerHeight) / 2
                let markerBaseline = markerTop + marker.ascent
                let markerBounds = CGRect(x: markerX, y: markerTop, width: marker.width, height: markerHeight)
                fragments.append(.init(kind: .marker, line: marker.line, origin: CGPoint(x: markerX, y: markerBaseline), bounds: markerBounds, color: markerColor.cgColor, label: marker.label, checked: marker.checked, styleBox: checkbox))
            }
            let seedBounds = CGRect(x: contentX, y: cursorY, width: contentWidth, height: totalEnd - cursorY)
            let bounds = fragments.reduce(seedBounds) { $0.union($1.bounds) }
            let prepared = PreparedProseBlock(
                fragments: fragments,
                bounds: bounds
            )
            return BlockPreparation(
                block: prepared,
                interactions: [],
                accessibilityNodes: [PreparedProseAccessibilityNode(
                    interactionIndex: nil,
                    role: .separator,
                    label: "Separator",
                    bounds: bounds
                )],
                attachment: nil,
                nextY: totalEnd + itemSpacing,
                retainedBytes: prepared.estimatedRetainedBytes
            )
        }

        let availableWidth = max(1, contentWidth - listInset - quoteInset - codeInset * 2 - box.inset.left - box.inset.right)
        let attributed = makeAttributedString(block.inlines, paint: paint, theme: theme, warningSemanticGeneration: warningSemanticGeneration, ancestors: block.styleAncestors.map(\.nodeType) + [block.nodeType])
        let highlighted = NSMutableAttributedString(attributedString: attributed.string)
        NativeCodeHighlightPresentation.apply(highlighting, to: highlighted)
        let typesetter = CTTypesetterCreateWithAttributedString(highlighted)
        var location = 0
        var fragments: [PreparedProseFragment] = []
        var interactionRects: [[CGRect]] = Array(repeating: [], count: attributed.semanticRanges.count)
        var accessibilityRects: [[CGRect]] = Array(repeating: [], count: attributed.accessibilityRanges.count)
        let semanticGeometryRanges = attributed.semanticRanges.enumerated().map {
            (index: $0.offset, range: $0.element.range)
        }
        let accessibilityGeometryRanges = attributed.accessibilityRanges.enumerated().compactMap { index, range -> (index: Int, range: NSRange)? in
            guard range.role == .text else { return nil }
            return (index, range.range)
        }
        var semanticGeometryCursor = 0
        var accessibilityGeometryCursor = 0
        let codeTopInset = (block.nodeType == "codeBlock" ? theme.codePaddingVertical : 0) + box.inset.top
        let firstLineHeight = max(paint.font.lineHeight, paint.lineHeight ?? 0)
        let markerTopProtection = marker.map {
            max(0, ($0.ascent + $0.descent - firstLineHeight) / 2 - codeTopInset)
        } ?? 0
        var textTop = cursorY + codeTopInset + (cursorY == theme.contentInsets.top ? markerTopProtection : 0)
        var firstLineBounds: CGRect?
        while location < attributed.string.length {
            let suggested = CTTypesetterSuggestLineBreak(typesetter, location, availableWidth)
            let count = max(1, suggested)
            let shapedLine = CTTypesetterCreateLine(typesetter, CFRange(location: location, length: count))
            let shouldJustify = paint.textValues["textAlign"] as? String == "justify" && location + count < attributed.string.length && (attributed.string.string as NSString).character(at: location + count - 1) != 10
            let line = shouldJustify ? CTLineCreateJustifiedLine(shapedLine, 1, availableWidth) ?? shapedLine : shapedLine
            var ascent: CGFloat = 0
            var descent: CGFloat = 0
            var leading: CGFloat = 0
            let lineWidth = CGFloat(CTLineGetTypographicBounds(line, &ascent, &descent, &leading))
            let naturalHeight = ascent + descent + leading
            var requestedHeight = paint.lineHeight ?? 0
            attributed.string.enumerateAttribute(editorInlineLineHeightAttribute, in: NSRange(location: location, length: count)) { value, _, _ in
                requestedHeight = max(requestedHeight, EditorTheme.cgFloat(value) ?? 0)
            }
            let lineHeight = max(naturalHeight, requestedHeight)
            let baseline = textTop + (lineHeight - naturalHeight) / 2 + ascent
            let alignment = paint.textValues["textAlign"] as? String
            let lineTextX = textX + (alignment == "center" ? max(0, availableWidth - lineWidth) / 2 : alignment == "right" ? max(0, availableWidth - lineWidth) : 0)
            let lineBounds = CGRect(x: lineTextX, y: textTop, width: min(availableWidth, max(0, lineWidth)), height: lineHeight)
            fragments.append(contentsOf: inlineBackgroundFragments(for: line, bounds: lineBounds))
            let lineRange = NSRange(location: location, length: count)
            fragments.append(.init(kind: .text, line: line, origin: CGPoint(x: lineTextX, y: baseline), bounds: lineBounds))
            fragments.append(contentsOf: strikeFragments(
                for: line,
                lineOrigin: CGPoint(x: lineTextX, y: baseline),
                displayScale: displayScale
            ))
            appendShapedRects(
                ranges: semanticGeometryRanges,
                line: line,
                lineRange: lineRange,
                lineBounds: lineBounds,
                textX: lineTextX,
                displayScale: displayScale,
                rangeCursor: &semanticGeometryCursor,
                to: &interactionRects
            )
            appendShapedRects(
                ranges: accessibilityGeometryRanges,
                line: line,
                lineRange: lineRange,
                lineBounds: lineBounds,
                textX: lineTextX,
                displayScale: displayScale,
                rangeCursor: &accessibilityGeometryCursor,
                to: &accessibilityRects
            )
            if firstLineBounds == nil { firstLineBounds = lineBounds }
            for atom in attributed.atoms where NSIntersectionRange(atom.range, lineRange).length > 0 {
                let offset = CGFloat(CTLineGetOffsetForStringIndex(line, atom.range.location, nil))
                let atomBounds = CGRect(
                    x: lineTextX + offset,
                    y: baseline - atom.metrics.ascent,
                    width: atom.metrics.width,
                    height: atom.metrics.ascent + atom.metrics.descent
                )
                fragments.append(contentsOf: strikeFragments(for: atom.line, lineOrigin: CGPoint(x: atomBounds.minX + atom.appearance.padding.left, y: baseline), displayScale: displayScale))
                fragments.append(
                    .init(
                        kind: .atom,
                        line: atom.line,
                        origin: CGPoint(x: atomBounds.minX + atom.appearance.padding.left, y: baseline),
                        bounds: atomBounds,
                        color: atom.appearance.background.cgColor,
                        borderColor: atom.appearance.borderColor?.cgColor,
                        cornerRadius: atom.appearance.radius,
                        strokeWidth: atom.appearance.borderWidth,
                        padding: atom.appearance.padding,
                        label: atom.label,
                        styleBox: atom.appearance.styleBox
                    )
                )
            }
            location += count
            textTop += lineHeight
        }
        if fragments.isEmpty {
            let fallbackHeight = paint.lineHeight ?? paint.font.lineHeight
            let line = CTLineCreateWithAttributedString(NSAttributedString(string: "\u{200B}", attributes: baseAttributes(paint)))
            let alignment = paint.textValues["textAlign"] as? String
            let lineTextX = textX + (alignment == "center" ? max(0, availableWidth) / 2 : alignment == "right" ? max(0, availableWidth) : 0)
            let lineBounds = CGRect(x: lineTextX, y: textTop, width: 0, height: fallbackHeight)
            fragments.append(.init(kind: .text, line: line, origin: CGPoint(x: textX, y: textTop + paint.font.ascender), bounds: lineBounds))
            firstLineBounds = lineBounds
            textTop += fallbackHeight
        }
        let textEnd = textTop
        let totalEnd = textEnd + (block.nodeType == "codeBlock" ? theme.codePaddingVertical : 0) + box.inset.bottom
        let blockRect = CGRect(x: contentX, y: cursorY, width: contentWidth, height: max(0, totalEnd - cursorY))
        if sheet != nil {
            fragments.insert(.init(kind: .background, bounds: blockRect, styleBox: box), at: 0)
        } else if block.nodeType == "codeBlock" {
            fragments.insert(.init(kind: .background, bounds: blockRect, color: theme.codeBackground.cgColor, cornerRadius: theme.codeRadius), at: 0)
        }
        if block.inBlockquote {
            let border = CGRect(x: contentX, y: cursorY, width: theme.quoteBorderWidth, height: max(0, totalEnd - cursorY))
            fragments.append(.init(kind: .border, bounds: border, color: theme.quoteBorderColor.cgColor, strokeWidth: theme.quoteBorderWidth))
        }
        if let marker, let firstLineBounds {
            let markerX = textX - markerGutter
            let markerHeight = marker.ascent + marker.descent
            let markerTop = firstLineBounds.midY - markerHeight / 2
            let markerBaseline = markerTop + marker.ascent
            let markerBounds = CGRect(
                x: markerX,
                y: markerTop,
                width: marker.width,
                height: markerHeight
            )
            fragments.append(.init(kind: .marker, line: marker.line, origin: CGPoint(x: markerX, y: markerBaseline), bounds: markerBounds, color: markerColor.cgColor, label: marker.label, checked: marker.checked, styleBox: checkbox))
        }
        let seedBounds = CGRect(x: contentX, y: cursorY, width: contentWidth, height: max(0, totalEnd - cursorY))
        let bounds = fragments.reduce(seedBounds) { $0.union($1.bounds) }
        let prepared = PreparedProseBlock(fragments: fragments, bounds: bounds)
        var interactions: [PreparedProseInteraction] = []
        var interactionIndexBySemanticIndex: [Int: Int] = [:]
        for (semanticIndex, semantic) in attributed.semanticRanges.enumerated() {
            let rects = interactionRects[semanticIndex]
            guard !rects.isEmpty else { continue }
            let interaction: PreparedProseInteraction
            switch semantic {
            case let .link(_, href, text):
                interaction = PreparedProseInteraction(kind: .link, rects: rects, href: href, visibleText: text, docPos: nil, label: text, attrsJSON: nil)
            case let .mention(_, docPos, label, attrsJSON):
                interaction = PreparedProseInteraction(kind: .mention, rects: rects, href: nil, visibleText: label, docPos: docPos, label: label, attrsJSON: attrsJSON)
            }
            interactionIndexBySemanticIndex[semanticIndex] = interactions.count
            interactions.append(interaction)
        }
        var markerPending = block.listItemBoundary?.isFirstRenderableLeaf == true
            ? block.listContext.map { context in
                context.kind == "task" ? (context.checked ? "Checked" : "Unchecked") : "Item"
            }
            : nil
        let accessibilityNodes = attributed.accessibilityRanges.enumerated().compactMap { index, range -> PreparedProseAccessibilityNode? in
            let label = range.label.trimmingCharacters(in: .whitespacesAndNewlines)
            guard !label.isEmpty else { return nil }
            let accessibleLabel: String
            if let marker = markerPending {
                markerPending = nil
                accessibleLabel = "\(marker), \(label)"
            } else {
                accessibleLabel = label
            }
            let interactionIndex: Int?
            let role: PreparedProseAccessibilityNode.Role
            let rects: [CGRect]
            switch range.role {
            case .text:
                interactionIndex = nil
                role = block.nodeType == "heading" || theme.headings[block.nodeType] != nil ? .heading : .text
                rects = accessibilityRects[index]
            case let .link(semanticIndex):
                interactionIndex = interactionIndexBySemanticIndex[semanticIndex]
                role = interactionIndex == nil ? .text : .link
                rects = interactionRects[semanticIndex]
            case let .mention(semanticIndex):
                interactionIndex = interactionIndexBySemanticIndex[semanticIndex]
                role = interactionIndex == nil ? .text : .mention
                rects = interactionRects[semanticIndex]
            }
            guard !rects.isEmpty else { return nil }
            return PreparedProseAccessibilityNode(
                interactionIndex: interactionIndex,
                role: role,
                label: accessibleLabel,
                rects: rects
            )
        }
        return BlockPreparation(
            block: prepared,
            interactions: interactions,
            accessibilityNodes: accessibilityNodes,
            attachment: nil,
            nextY: totalEnd + itemSpacing,
            retainedBytes: 256 + attributed.retainedBytes + prepared.estimatedRetainedBytes
        )
    }

    private func appendShapedRects(
        ranges: [(index: Int, range: NSRange)],
        line: CTLine,
        lineRange: NSRange,
        lineBounds: CGRect,
        textX: CGFloat,
        displayScale: CGFloat,
        rangeCursor: inout Int,
        to rects: inout [[CGRect]]
    ) {
        guard ranges.indices.contains(rangeCursor) else { return }
        while ranges.indices.contains(rangeCursor), ranges[rangeCursor].range.upperBound <= lineRange.location {
            rangeCursor += 1
        }
        guard ranges.indices.contains(rangeCursor) else { return }
        guard ranges[rangeCursor].range.location < lineRange.upperBound else { return }
        let glyphRuns = CTLineGetGlyphRuns(line) as? [CTRun] ?? []
        var index = rangeCursor
        while ranges.indices.contains(index), ranges[index].range.location < lineRange.upperBound {
            let geometryRange = ranges[index]
            let range = geometryRange.range
            var visualPieces: [(rect: CGRect, rightToLeft: Bool)] = []
            for run in glyphRuns {
                let stringRange = CTRunGetStringRange(run)
                let runRange = NSRange(location: stringRange.location, length: stringRange.length)
                let overlap = NSIntersectionRange(NSIntersectionRange(range, lineRange), runRange)
                guard overlap.length > 0 else { continue }
                let start = CGFloat(CTLineGetOffsetForStringIndex(line, overlap.location, nil))
                let end = CGFloat(CTLineGetOffsetForStringIndex(line, overlap.location + overlap.length, nil))
                visualPieces.append((
                    CGRect(
                        x: textX + min(start, end),
                        y: lineBounds.minY,
                        width: max(1 / displayScale, abs(end - start)),
                        height: lineBounds.height
                    ),
                    CTRunGetStatus(run).contains(.rightToLeft)
                ))
            }
            var priorDirection: Bool?
            for piece in visualPieces.sorted(by: { PreparedProseInteractionGeometry.visualOrder($0.rect, $1.rect) }) {
                PreparedProseInteractionGeometry.appendSameLinePiece(
                    piece.rect,
                    to: &rects[geometryRange.index],
                    mayMergeWithPrior: priorDirection == piece.rightToLeft
                )
                priorDirection = piece.rightToLeft
            }
            index += 1
        }
    }

}
