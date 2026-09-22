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

    let cellInput = EditorTextView(frame: .zero, textContainer: nil)
    private(set) var phase: TableInputPhase = .inactive
    private(set) var positionMap: TableCellPositionMap?
    var inputInstanceCountForTesting: Int { 1 }

    static func canBind(_ target: Target) -> Bool {
        !target.isSynthetic && !target.isNestedTarget
    }

    @discardableResult
    func bind(
        _ target: Target,
        text: NSAttributedString,
        positionMap: TableCellPositionMap? = nil,
        editorId: UInt64 = 0
    ) -> Bool {
        guard Self.canBind(target) else { return false }
        _ = cellInput.discardTransientNativeInputForEditorRebind()
        self.positionMap = positionMap
        cellInput.editorId = editorId
        cellInput.tableCellPositionMap = positionMap
        _ = cellInput.applyAttributedRender(text, usedPatch: false, positionCacheUpdate: .invalidate)
        phase = .bound(
            cellSourcePos: target.binding.cellSourcePosition,
            documentRevision: String(target.binding.documentRevision),
            positionEpoch: String(target.binding.positionEpoch)
        )
        return true
    }

    func beginComposition() {
        guard case let .bound(cellSourcePos, documentRevision, positionEpoch) = phase else { return }
        phase = .composing(cellSourcePos: cellSourcePos, documentRevision: documentRevision, positionEpoch: positionEpoch)
    }

    func invalidateBinding() {
        _ = cellInput.discardTransientNativeInputForEditorRebind()
        positionMap = nil
        cellInput.tableCellPositionMap = nil
        cellInput.editorId = 0
        phase = .inactive
    }
}
