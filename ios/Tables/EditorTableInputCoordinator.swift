import UIKit

enum TableInputPhase: Equatable {
    case inactive
    case bound(tableKey: String, cellIndex: UInt32, documentRevision: String, positionEpoch: String)
    case composing(tableKey: String, cellIndex: UInt32, documentRevision: String, positionEpoch: String)
}

final class EditorTableInputCoordinator {
    struct Target {
        let binding: TableCellPositionMap.Binding
        let isSynthetic: Bool
        let isNestedTarget: Bool
    }

    struct Projection {
        let target: Target
        let text: NSAttributedString
        let positionMap: TableCellPositionMap
    }

    let cellInput = TableCellInputTextView(frame: .zero, textContainer: nil)
    private(set) var phase: TableInputPhase = .inactive
    private(set) var positionMap: TableCellPositionMap?
    private(set) var activeTableID: String?
    private(set) var activeCellIndex: UInt32?
    var inputInstanceCountForTesting: Int { 1 }

    init() {
        cellInput.isScrollEnabled = false
    }

    static func canBind(_ target: Target) -> Bool {
        !target.isSynthetic && !target.isNestedTarget
    }

    static func projection(
        cellIndex: UInt32,
        tableKey: String,
        index: EditorTableIndex,
        documentRevision: UInt64,
        positionEpoch: UInt64,
        baseFont: UIFont,
        textColor: UIColor,
        theme: EditorTheme?,
        atomConfiguration: AtomRenderConfiguration?
    ) -> Projection? {
        guard let table = index.record(tableKey: tableKey), Int(cellIndex) < table.cells.count,
              let cellDocStart = index.docStart(tableKey: tableKey, cellIndex: Int(cellIndex)),
              let segments = index.inputSegments(tableKey: tableKey, cellIndex: Int(cellIndex))
        else { return nil }
        let inputCell = table.cells[Int(cellIndex)]
        let target = Target(
            binding: .init(tableKey: tableKey, cellIndex: cellIndex, documentRevision: documentRevision, positionEpoch: positionEpoch),
            isSynthetic: false, isNestedTarget: table.readOnlyDescendants
        )
        guard canBind(target), !inputCell.inputBlocks.isEmpty,
              let elements = RenderBridge.inputElements(inputCell.elements, voidElementIndices: inputCell.voidElementIndices, cellDocStart: cellDocStart) else { return nil }
        let tableIDs = Set(inputCell.nestedTables.map(\.tableKey))
        guard tableIDs.count == inputCell.nestedTables.count,
              inputCell.nestedTables.allSatisfy({ $0.scalarStart != nil && $0.scalarEnd != nil }) else { return nil }

        var blockRanges: [Int: NSRange] = [:]
        let renderedWithMarkers = RenderBridge.renderElements(
            fromArray: elements,
            baseFont: baseFont,
            textColor: textColor,
            theme: theme,
            atomConfiguration: atomConfiguration,
            rootTableIDs: tableIDs
        ) { elementIndex, range in
            blockRanges[elementIndex] = range
        }
        let rendered = renderedWithMarkers
        var observedTables = Set<String>()
        var markersValid = true
        rendered.enumerateAttribute(RenderBridgeAttributes.rootTableMarker,
                                    in: NSRange(location: 0, length: rendered.length)) { value, range, _ in
            guard let value else { return }
            guard let tableID = value as? String, range.length == 1,
                  observedTables.insert(tableID).inserted else {
                markersValid = false
                return
            }
        }
        guard markersValid, observedTables == tableIDs else { return nil }
        for (block, segment) in zip(inputCell.inputBlocks, segments) {
            guard let range = blockRanges[Int(block.elementIndex)] else { return nil }
            let localStart = PositionBridge.utf16OffsetToScalar(range.location, in: rendered)
            let localEnd = PositionBridge.utf16OffsetToScalar(NSMaxRange(range), in: rendered)
            let prefixLength = block.contentScalarStart - block.scalarStart
            guard localStart >= prefixLength,
                  segment.localScalarRange == (localStart - prefixLength)..<(localEnd + 1) else { return nil }
        }
        return Projection(target: target, text: rendered, positionMap: .init(binding: target.binding, segments: segments))
    }

    @discardableResult
    func bind(
        _ target: Target,
        text: NSAttributedString,
        positionMap: TableCellPositionMap? = nil,
        editorId: UInt64 = 0,
        tableID: String? = nil,
        cellIndex: UInt32? = nil,
        inputAuthority: (() -> Bool)? = nil
    ) -> Bool {
        guard Self.canBind(target), let positionMap else { return false }
        let sameCell = self.positionMap?.binding.tableKey == target.binding.tableKey
            && self.positionMap?.binding.cellIndex == target.binding.cellIndex
            && cellInput.editorId == editorId
        if sameCell, cellInput.textStorage.isEqual(to: text),
           cellInput.lastAuthorizedAttributedTextStorage.isEqual(to: text),
           !cellInput.isComposing, cellInput.markedTextRange == nil {
            self.positionMap = positionMap
            cellInput.tableCellPositionMap = positionMap
            cellInput.tableCellInputAuthority = inputAuthority
            phase = .bound(tableKey: target.binding.tableKey, cellIndex: target.binding.cellIndex,
                           documentRevision: String(target.binding.documentRevision), positionEpoch: String(target.binding.positionEpoch))
            return true
        }
        _ = cellInput.discardTransientNativeInputForEditorRebind()
        cellInput.finishTransientMarkedTextMutation()
        self.positionMap = positionMap
        cellInput.setAuthoritativeCellSelectionActive(false)
        activeTableID = tableID
        activeCellIndex = cellIndex
        cellInput.editorId = editorId
        cellInput.tableCellPositionMap = positionMap
        cellInput.tableCellInputAuthority = inputAuthority
        _ = cellInput.applyAttributedRender(text, usedPatch: false, positionCacheUpdate: .invalidate)
        phase = .bound(tableKey: target.binding.tableKey, cellIndex: target.binding.cellIndex, documentRevision: String(target.binding.documentRevision), positionEpoch: String(target.binding.positionEpoch))
        return true
    }

    func copyInputTraits(from root: EditorTextView) {
        cellInput.baseFont = root.baseFont
        cellInput.baseTextColor = root.baseTextColor
        cellInput.baseBackgroundColor = root.baseBackgroundColor
        cellInput.baseTextContainerInset = root.baseTextContainerInset
        cellInput.baseLineFragmentPadding = root.baseLineFragmentPadding
        cellInput.theme = root.theme
        cellInput.atomRenderConfiguration = root.atomRenderConfiguration
        cellInput.allowImageResizing = root.allowImageResizing
        cellInput.isEditable = root.isEditable
    }

    func beginComposition() {
        guard case let .bound(tableKey, cellIndex, documentRevision, positionEpoch) = phase else { return }
        phase = .composing(tableKey: tableKey, cellIndex: cellIndex, documentRevision: documentRevision, positionEpoch: positionEpoch)
    }

    func refreshPositionMap(_ map: TableCellPositionMap) -> Bool {
        guard case .bound = phase,
              let previous = positionMap,
              previous.binding.tableKey == map.binding.tableKey,
              previous.binding.cellIndex == map.binding.cellIndex,
              previous.binding.documentRevision == map.binding.documentRevision,
              cellInput.tableCellPositionMap != nil
        else { return false }
        positionMap = map
        cellInput.tableCellPositionMap = map
        phase = .bound(tableKey: map.binding.tableKey, cellIndex: map.binding.cellIndex,
                       documentRevision: String(map.binding.documentRevision),
                       positionEpoch: String(map.binding.positionEpoch))
        return true
    }

    @discardableResult
    func invalidateBinding() -> String? {
        guard phase != .inactive
            || positionMap != nil
            || activeTableID != nil
            || activeCellIndex != nil
            || cellInput.editorId != 0
            || cellInput.tableCellPositionMap != nil
            || cellInput.onProjectedUpdate != nil
            || cellInput.tableCellInputAuthority != nil
            || cellInput.onTableCellArrow != nil
        else { return nil }
        let cancellation = cellInput.discardTransientNativeInputForEditorRebind()
        cellInput.finishTransientMarkedTextMutation()
        cellInput.restoreAuthorizedTextSnapshot()
        positionMap = nil
        cellInput.setAuthoritativeCellSelectionActive(false)
        activeTableID = nil
        activeCellIndex = nil
        cellInput.tableCellPositionMap = nil
        cellInput.tableCellInputAuthority = nil
        cellInput.onProjectedUpdate = nil
        cellInput.onTableCellTab = nil
        cellInput.onTableCellArrow = nil
        cellInput.onAuthoritativeTextSelectionSynced = nil
        cellInput.editorId = 0
        phase = .inactive
        _ = cellInput.resignFirstResponder()
        return cancellation
    }
}
