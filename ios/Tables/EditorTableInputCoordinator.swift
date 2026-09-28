import UIKit

enum TableInputPhase: Equatable {
    case inactive
    case bound(cellSourcePos: UInt32, documentRevision: String, positionEpoch: String)
    case composing(cellSourcePos: UInt32, documentRevision: String, positionEpoch: String)
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

    static func canBind(_ target: Target) -> Bool {
        !target.isSynthetic && !target.isNestedTarget
    }

    static func projection(
        cellIndex: UInt32,
        table: [String: Any],
        mapping: EditorV2Adapter.TableInputTable,
        documentRevision: UInt64,
        positionEpoch: UInt64,
        baseFont: UIFont,
        textColor: UIColor,
        theme: EditorTheme?,
        atomConfiguration: AtomRenderConfiguration?
    ) -> Projection? {
        guard Int(cellIndex) < mapping.cells.count,
              let cells = table["cells"] as? [[String: Any]],
              Int(cellIndex) < cells.count,
              let elements = cells[Int(cellIndex)]["elements"] as? [[String: Any]]
        else { return nil }
        let inputCell = mapping.cells[Int(cellIndex)]
        let target = Target(
            binding: .init(cellSourcePosition: inputCell.sourcePos, documentRevision: documentRevision, positionEpoch: positionEpoch),
            isSynthetic: false,
            isNestedTarget: table["readOnlyDescendants"] as? Bool ?? false
        )
        guard canBind(target), !inputCell.blocks.isEmpty else { return nil }

        let tableIDs = Set(inputCell.excluded.map(\.tableID))
        guard tableIDs.count == inputCell.excluded.count,
              inputCell.excluded.allSatisfy({ $0.extent != nil }) else { return nil }

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
        var segments: [TableCellPositionMap.Segment] = []
        for block in inputCell.blocks {
            guard let range = blockRanges[Int(block.elementIndex)] else { return nil }
            let localStart = PositionBridge.utf16OffsetToScalar(range.location, in: rendered)
            let localEnd = PositionBridge.utf16OffsetToScalar(NSMaxRange(range), in: rendered)
            let prefixLength = block.contentScalarStart - block.scalarStart
            guard localStart >= prefixLength,
                  localEnd >= localStart,
                  localEnd - (localStart - prefixLength) == block.scalarEnd - block.scalarStart,
                  localEnd < UInt32.max
            else { return nil }
            segments.append(.init(
                localScalarRange: (localStart - prefixLength)..<(localEnd + 1),
                globalScalarStart: block.scalarStart
            ))
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
        phase = .bound(cellSourcePos: target.binding.cellSourcePosition, documentRevision: String(target.binding.documentRevision), positionEpoch: String(target.binding.positionEpoch))
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
        guard case let .bound(cellSourcePos, documentRevision, positionEpoch) = phase else { return }
        phase = .composing(cellSourcePos: cellSourcePos, documentRevision: documentRevision, positionEpoch: positionEpoch)
    }

    func refreshPositionMap(_ map: TableCellPositionMap) -> Bool {
        guard case .bound = phase,
              let previous = positionMap,
              previous.binding.cellSourcePosition == map.binding.cellSourcePosition,
              previous.binding.documentRevision == map.binding.documentRevision,
              cellInput.tableCellPositionMap != nil
        else { return false }
        positionMap = map
        cellInput.tableCellPositionMap = map
        phase = .bound(cellSourcePos: map.binding.cellSourcePosition,
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
