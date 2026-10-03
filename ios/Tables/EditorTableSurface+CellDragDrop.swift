import os
import UIKit

struct TableCellDragSource: Equatable {
    let tableID: String
    let anchor: UInt32
    let head: UInt32
    let sourceIndices: Set<Int>
}

final class TableCellDragContext {
    let editorId: UInt64
    let documentRevision: UInt64
    let source: TableCellDragSource
    let payload: EditorClipboardPayload
    let movable: Bool

    init(
        editorId: UInt64,
        documentRevision: UInt64,
        source: TableCellDragSource,
        payload: EditorClipboardPayload,
        movable: Bool
    ) {
        self.editorId = editorId
        self.documentRevision = documentRevision
        self.source = source
        self.payload = payload
        self.movable = movable
    }
}

private enum TableCellDropCommand {
    static let cellDropKey = "cellDrop"
    static let targetCellKey = "targetCell"
    static let movedCellsKey = "movedCells"
    static let anchorCellKey = "anchorCell"
    static let headCellKey = "headCell"
}

enum TableCellDropLoadError: Error {
    case unreadableRepresentation(type: String, underlying: Error)
}

private enum TableCellDropKind {
    case move(TableCellDragContext)
    case copy(TableCellDragContext)
    case external(NSItemProvider)
}

private struct TableCellDrop {
    let target: TableCellDropTarget
    let revision: UInt64
    let kind: TableCellDropKind
    let pastesIntoSelection: Bool
}

private enum TableCellDropResolution {
    case selfDrop
    case refused
    case accepted(TableCellDrop)
}

extension EditorTableSurface: UIDragInteractionDelegate {
    func dragInteraction(
        _ interaction: UIDragInteraction,
        itemsForBeginning session: UIDragSession
    ) -> [UIDragItem] {
        guard let host = interactionHost, host.editorId != 0,
              let adapter = EditorV2Registry.adapter(forLegacyId: host.editorId),
              let source = cellDragSource(at: session.location(in: self)),
              let payload = EditorV2Shadow.clipboardPayload(id: host.editorId)
        else { return [] }
        let context = TableCellDragContext(
            editorId: host.editorId, documentRevision: adapter.baseDocumentRevision, source: source,
            payload: payload, movable: host.textView.canMutateSelectedTableCells()
        )
        session.localContext = context
        dismissCellEditMenu()
        let item = UIDragItem(itemProvider: payload.itemProvider())
        item.localObject = context
        return [item]
    }

    func dragInteraction(
        _ interaction: UIDragInteraction,
        previewForLifting item: UIDragItem,
        session: UIDragSession
    ) -> UITargetedDragPreview? {
        cellDragPreview()
    }
}

extension EditorTableSurface: TableCellDropHandling {
    func tableCellDropOperation(for session: UIDropSession) -> UIDropOperation? {
        guard let resolution = resolveTableCellDrop(session) else {
            showTableCellDropTarget(nil)
            return nil
        }
        switch resolution {
        case .selfDrop:
            showTableCellDropTarget(nil)
            return .cancel
        case .refused:
            showTableCellDropTarget(nil)
            return .forbidden
        case let .accepted(drop):
            showTableCellDropTarget(drop.target)
            switch drop.kind {
            case .move: return .move
            case .copy, .external: return .copy
            }
        }
    }

    func performTableCellDrop(_ session: UIDropSession) -> Bool {
        defer { endTableCellDropHover() }
        guard let resolution = resolveTableCellDrop(session) else { return false }
        guard case let .accepted(drop) = resolution else { return true }
        switch drop.kind {
        case let .move(context):
            applyTableCellDrop(drop, payload: context.payload.representations, moved: context.source)
        case let .copy(context):
            applyTableCellDrop(drop, payload: context.payload.representations, moved: nil)
        case let .external(provider):
            loadDroppedRepresentations(provider) { [weak self] result in
                switch result {
                case let .success(representations):
                    self?.applyTableCellDrop(drop, payload: representations, moved: nil)
                case let .failure(.unreadableRepresentation(type, underlying)):
                    EditorTextView.inputLog.error(
                        "[drop] refused an external cell drop: \(type, privacy: .public) failed to load: \(String(describing: underlying), privacy: .public)"
                    )
                }
            }
        }
        return true
    }

    func endTableCellDropHover() {
        showTableCellDropTarget(nil)
    }

    private func resolveTableCellDrop(_ session: UIDropSession) -> TableCellDropResolution? {
        guard let host = interactionHost else { return nil }
        let point = session.location(in: self)
        guard let hit = cellHit(at: point) else {
            return rootTableContains(point) ? .refused : nil
        }
        let target = TableCellDropTarget(tableID: hit.tableID, sourceIndex: Int(hit.cellIndex))
        let local = session.localDragSession?.localContext as? TableCellDragContext
        let sameEditorDrag = local.flatMap { $0.editorId == host.editorId ? $0 : nil }
        if let sameEditorDrag, sameEditorDrag.source.tableID == target.tableID,
           sameEditorDrag.source.sourceIndices.contains(target.sourceIndex) {
            return .selfDrop
        }
        guard host.textView.pasteMode != .disabled,
              let mutation = tableMutationContext(tableID: target.tableID)
        else { return .refused }
        let revision = mutation.adapter.baseDocumentRevision
        let kind: TableCellDropKind
        if let sameEditorDrag, sameEditorDrag.movable {
            guard sameEditorDrag.documentRevision == revision,
                  tableMutationContext(tableID: sameEditorDrag.source.tableID) != nil
            else { return .refused }
            kind = .move(sameEditorDrag)
        } else if let local {
            kind = .copy(local)
        } else if let provider = session.items.lazy.map(\.itemProvider).first(where: { provider in
            EditorClipboardPaste.supportedTypes.contains(where: provider.hasItemConformingToTypeIdentifier)
        }) {
            kind = .external(provider)
        } else {
            return .refused
        }
        let pastesIntoSelection = sameEditorDrag == nil && cellSelectionIncludes(target)
            && host.textView.canMutateSelectedTableCells()
        return .accepted(TableCellDrop(
            target: target,
            revision: revision,
            kind: kind,
            pastesIntoSelection: pastesIntoSelection
        ))
    }

    private func applyTableCellDrop(_ drop: TableCellDrop, payload: [String: Data], moved: TableCellDragSource?) {
        guard let mutation = tableMutationContext(tableID: drop.target.tableID),
              mutation.adapter.baseDocumentRevision == drop.revision,
              var command = EditorClipboardPaste.command(from: payload, mode: mutation.host.textView.pasteMode),
              mutation.host.activeTextInput.prepareForExternalEditorUpdate(),
              mutation.adapter.baseDocumentRevision == drop.revision
        else { return }
        if !drop.pastesIntoSelection {
            guard let position = cellDocumentPosition(tableID: drop.target.tableID, sourceIndex: drop.target.sourceIndex) else { return }
            var cellDrop: [String: Any] = [TableCellDropCommand.targetCellKey: position]
            if let moved {
                cellDrop[TableCellDropCommand.movedCellsKey] = [
                    TableCellDropCommand.anchorCellKey: Int(moved.anchor),
                    TableCellDropCommand.headCellKey: Int(moved.head)
                ]
            }
            command[TableCellDropCommand.cellDropKey] = cellDrop
        }
        guard let update = mutation.adapter.applyClipboardCommand(command) else { return }
        mutation.host.activeTextInput.applyUpdateJSON(update)
    }

    private func loadDroppedRepresentations(
        _ provider: NSItemProvider,
        completion: @escaping (Result<[String: Data], TableCellDropLoadError>) -> Void
    ) {
        let group = DispatchGroup()
        let lock = NSLock()
        var representations: [String: Data] = [:]
        var failure: TableCellDropLoadError?
        for type in EditorClipboardPaste.supportedTypes where provider.hasItemConformingToTypeIdentifier(type) {
            group.enter()
            _ = provider.loadDataRepresentation(forTypeIdentifier: type) { data, error in
                lock.lock()
                if let data {
                    representations[type] = data
                } else if let error, failure == nil {
                    failure = .unreadableRepresentation(type: type, underlying: error)
                }
                lock.unlock()
                group.leave()
            }
        }
        group.notify(queue: .main) {
            completion(failure.map(Result.failure) ?? .success(representations))
        }
    }
}
