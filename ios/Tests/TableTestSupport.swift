import XCTest

final class RemoteTablePeer {
    private let adapter: EditorV2Adapter
    private var nextRequestId: UInt64

    init(adapter: EditorV2Adapter, requestIdBase: UInt64) {
        self.adapter = adapter
        nextRequestId = requestIdBase
    }

    func apply(_ payload: [String: Any], call: (String, String) -> FfiJsonResult) throws {
        nextRequestId += 1
        var envelope = payload
        envelope["version"] = 1
        envelope["requestId"] = String(nextRequestId)
        envelope["baseDocumentRevision"] = String(adapter.baseDocumentRevision)
        let data = try JSONSerialization.data(withJSONObject: envelope)
        let result = call(adapter.editorId, try XCTUnwrap(String(data: data, encoding: .utf8)))
        XCTAssertNil(result.error, "the remote peer's change was refused: \(String(describing: result.error))")
    }

    func applyCommand(_ command: [String: Any]) throws {
        try apply(["command": command]) { editorV2ApplyCommand(editorId: $0, requestJson: $1) }
    }
}

extension EditorV2Adapter {
    func tableCellPositions(tableID: String) throws -> [UInt32] {
        let cells = try XCTUnwrap(cachedTableRecords[tableID]?["cells"] as? [[String: Any]])
        return try cells.map { try XCTUnwrap(EditorV2Adapter.uint32Field($0, "sourcePos")) }
    }
}

extension PreparedProseDrawingView {
    func presentedRealCell(tableID: String, position: UInt32) throws -> ViewerTablePresentedCell {
        try XCTUnwrap(mountedTablePresentation()?.cells.first {
            $0.surface.identity == tableID && $0.sourcePosition == Int(position) && $0.cell.sourceCellIndex != nil
        }, "cell \(position) is not presented")
    }
}

extension RichTextEditorView {
    var activeTableCellPosition: UInt32? {
        activeTextInput.tableCellPositionMap?.binding.cellSourcePosition
    }
}

extension NativeEditorExpoView {
    func deliverRemoteCommit(editorId: UInt64) {
        NativeEditorViewRegistry.shared.applyRemoteCommitRefresh(editorId: editorId)
        layoutIfNeeded()
    }
}
