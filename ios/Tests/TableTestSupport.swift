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

    func applySelection(_ selection: [String: Any]) throws {
        try apply(["selection": selection]) { editorV2SetSelection(editorId: $0, requestJson: $1) }
    }
}

private let cellSelectionType = "cell"
private let documentPositionKind = "document"
private let roomInitializationType = "room"

enum TableToolbarTestItems {
    static let strongMark = "strong"
    static let strongLabel = "Bold"
    static let strongJson =
        #"[{"type":"mark","mark":"\#(strongMark)","label":"\#(strongLabel)","icon":{"type":"default","id":"bold"}}]"#
    static let undoLabel = "Undo"
    static let redoLabel = "Redo"
    static let historyJson =
        #"[{"type":"command","command":"undo","label":"\#(undoLabel)","icon":{"type":"default","id":"undo"}},"#
            + #"{"type":"command","command":"redo","label":"\#(redoLabel)","icon":{"type":"default","id":"redo"}}]"#
}

extension EditorV2Adapter {
    func editableTableID() throws -> String {
        try XCTUnwrap(cachedTableRecords.first { $0.value["readOnlyDescendants"] as? Bool == false }?.key)
    }

    func tableCellPositions() throws -> [UInt32] {
        let cells = try XCTUnwrap(cachedTableRecords[try editableTableID()]?["cells"] as? [[String: Any]])
        return try cells.map { try XCTUnwrap(EditorV2Adapter.uint32Field($0, "sourcePos")) }
    }

    func tableCellTexts() throws -> [[String]] {
        func text(_ node: [String: Any]) -> String {
            if node["type"] as? String == "text" { return node["text"] as? String ?? "" }
            return (node["content"] as? [[String: Any]] ?? []).map(text).joined()
        }
        let json = try XCTUnwrap(documentJson())
        let root = try XCTUnwrap(JSONSerialization.jsonObject(with: Data(json.utf8)) as? [String: Any])
        let content = try XCTUnwrap(root["content"] as? [[String: Any]])
        let table = try XCTUnwrap(content.first { $0["type"] as? String == "table" })
        let rows = try XCTUnwrap(table["content"] as? [[String: Any]])
        return rows.map { row in (row["content"] as? [[String: Any]] ?? []).map(text) }
    }
}

func documentCellSelection(anchor: UInt32, head: UInt32) -> [String: Any] {
    [
        "type": cellSelectionType,
        "anchorCell": ["kind": documentPositionKind, "offset": Int(anchor)],
        "headCell": ["kind": documentPositionKind, "offset": Int(head)]
    ]
}

final class TableCollaborationRelay {
    private static let nowMillis = "0"
    private static let maximumRounds = 64
    private let generations: [String: String]

    init(editorIds: [String]) throws {
        var generations: [String: String] = [:]
        for editorId in editorIds {
            let driven = try Self.object(editorV2CollaborationDrive(editorId: editorId, nowMillis: Self.nowMillis))
            let generation = try XCTUnwrap(driven["generationToOpen"] as? String, "\(editorId) issued no generation: \(driven)")
            _ = try Self.object(editorV2CollaborationSocketOpen(editorId: editorId, generation: generation,
                                                                nowMillis: Self.nowMillis))
            generations[editorId] = generation
        }
        self.generations = generations
    }

    @discardableResult
    func exchangeUntilIdle() throws -> Set<String> {
        var committed: Set<String> = []
        for _ in 0..<Self.maximumRounds {
            var delivered = false
            for (from, fromGeneration) in generations {
                let lease = editorV2CollaborationLeaseOutbound(editorId: from, generation: fromGeneration)
                XCTAssertNil(lease.error, "\(from) could not lease: \(String(describing: lease.error))")
                guard let outbound = lease.value else { continue }
                for (to, toGeneration) in generations where to != from {
                    let received = try Self.object(editorV2CollaborationReceive(
                        editorId: to, generation: toGeneration, message: outbound.frame, nowMillis: Self.nowMillis
                    ))
                    if received["remoteCommitApplied"] as? Bool == true {
                        committed.insert(to)
                    }
                }
                _ = try Self.object(editorV2CollaborationAckOutbound(editorId: from, generation: fromGeneration,
                                                                     leaseId: outbound.leaseId))
                delivered = true
            }
            if !delivered {
                return committed
            }
        }
        XCTFail("the peers never went quiet")
        return committed
    }

    private static func object(_ result: FfiJsonResult) throws -> [String: Any] {
        XCTAssertNil(result.error, "collaboration call failed: \(String(describing: result.error))")
        let json = try XCTUnwrap(result.value)
        return try XCTUnwrap(JSONSerialization.jsonObject(with: Data(json.utf8)) as? [String: Any])
    }
}

struct TableRoomSeed {
    let configJson: String
    let encodedState: Data

    init(localConfigJson: String, documentJson: String) throws {
        let builder = makeV2Editor(configJson: localConfigJson)
        defer { destroyV2Editor(id: builder) }
        let adapter = try XCTUnwrap(EditorV2Registry.adapter(forLegacyId: builder))
        XCTAssertNotNil(adapter.setContentJson(documentJson))
        let exported = editorV2SnapshotExport(editorId: adapter.editorId)
        XCTAssertNil(exported.error, "snapshot export failed: \(String(describing: exported.error))")
        let snapshot = try XCTUnwrap(exported.value)
        let metadata = try XCTUnwrap(JSONSerialization.jsonObject(with: Data(snapshot.metadataJson.utf8)) as? [String: Any])
        var config = try XCTUnwrap(JSONSerialization.jsonObject(with: Data(localConfigJson.utf8)) as? [String: Any])
        config["initialization"] = [
            "type": roomInitializationType,
            "documentId": try XCTUnwrap(metadata["documentId"]),
            "lineageId": try XCTUnwrap(metadata["lineageId"]),
            "snapshot": metadata
        ]
        configJson = try XCTUnwrap(String(data: JSONSerialization.data(withJSONObject: config), encoding: .utf8))
        encodedState = snapshot.encodedState
    }

    func makeEditor() -> UInt64 {
        makeV2Editor(configJson: configJson, snapshotState: encodedState) {
            editorV2CollaborationSetAwarenessSelection(editorId: $0, selectionJson: $1)
        }
    }
}

extension EditorV2Adapter {
    func applyLocalSelection(_ selection: [String: Any]) -> FfiJsonResult {
        callWithEnvelope(["selection": selection]) { editorV2SetSelection(editorId: self.editorId, requestJson: $0) }
    }

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

extension EditorTextView {
    func applyLocalSelection(adapter: EditorV2Adapter, selection: [String: Any]) throws {
        let request = adapter.applyLocalSelection(selection)
        XCTAssertNil(request.error, "engine rejected the selection \(selection): \(String(describing: request.error))")
        XCTAssertTrue(applyUpdateJSON(try XCTUnwrap(adapter.refreshFromRustState(mirrorSelection: nil))))
    }

    func selectTableCells(adapter: EditorV2Adapter, anchor: UInt32, head: UInt32) throws {
        try applyLocalSelection(adapter: adapter, selection: documentCellSelection(anchor: anchor, head: head))
    }

    func pressAccessoryToolbarButton(labeled label: String) throws {
        let toolbar = try XCTUnwrap(inputAccessoryView, "the input shows no keyboard toolbar")
        var pending: [UIView] = [toolbar]
        var button: UIButton?
        while button == nil, !pending.isEmpty {
            let view = pending.removeFirst()
            button = (view as? UIButton).flatMap { $0.accessibilityLabel == label ? $0 : nil }
            pending.append(contentsOf: view.subviews)
        }
        let target = try XCTUnwrap(button, "the keyboard toolbar has no \(label) button")
        XCTAssertTrue(target.isEnabled, "the \(label) button is disabled")
        target.sendActions(for: .touchUpInside)
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
