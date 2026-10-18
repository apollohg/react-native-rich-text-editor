import Foundation

extension EditorV2Adapter {
    private static func viewUpdate(
        from snapshot: AtomicRenderSnapshot,
        renderBlocks: [[NSDictionary]],
        patchBlocks: [[NSDictionary]]?,
        strippingViewSelection: Bool
    ) -> EditorV2DerivedUpdate? {
        var update = snapshot.renderObject
        if !(update["renderBlocks"] is NSNull) {
            update["renderBlocks"] = renderBlocks
        } else if let patchBlocks, var patch = update["renderPatch"] as? [String: Any] {
            patch["renderBlocks"] = patchBlocks
            update["renderPatch"] = patch
        }
        guard strippingViewSelection else {
            return EditorV2DerivedUpdate(
                updateJSON: snapshot.viewUpdateJSON, update: update, scalarLength: snapshot.scalarLength
            )
        }
        update.removeValue(forKey: "selection")
        guard let strippedData = try? JSONSerialization.data(withJSONObject: update),
              let strippedJSON = String(data: strippedData, encoding: .utf8)
        else { return nil }
        return EditorV2DerivedUpdate(
            updateJSON: strippedJSON,
            update: update,
            scalarLength: snapshot.scalarLength
        )
    }

    private static func sharesStorage(_ lhs: String, _ rhs: String) -> Bool {
        lhs.utf8.withContiguousStorageIfAvailable { left in
            rhs.utf8.withContiguousStorageIfAvailable { right in
                left.count == right.count && left.baseAddress == right.baseAddress
            } ?? false
        } ?? false
    }

    private func adoptedViewUpdate(sharing updateJSON: String) -> [String: Any]? {
        runtimeLock.lock()
        defer { runtimeLock.unlock() }
        guard let cachedViewUpdate, Self.sharesStorage(cachedViewUpdate.updateJSON, updateJSON) else { return nil }
        return cachedViewUpdate.update
    }

    static func viewUpdateObject(_ updateJSON: String, adoptedBy adapter: EditorV2Adapter?) -> [String: Any]? {
        adapter?.adoptedViewUpdate(sharing: updateJSON)
            ?? updateJSON.data(using: .utf8).flatMap { try? JSONSerialization.jsonObject(with: $0) as? [String: Any] }
    }

    private func fetchNativeFrame(mirrorScalarSelection: (anchor: UInt32, head: UInt32)?) -> FfiNativeRenderFrame? {
        let stageStarted = PreparedProseInstrumentation.now()
        defer { PreparedProseInstrumentation.recordTableStage(.nativeFrameAndFFI, start: stageStarted) }
        renderUpdateCallCountForTesting += 1
        let result = editorV2RenderNativeFrame(
            editorId: editorId, ownerId: nativeOwnerId.map(String.init),
            mirrorScalarAnchor: mirrorScalarSelection?.anchor, mirrorScalarHead: mirrorScalarSelection?.head
        )
        switch (result.frame, result.error) {
        case let (.some(frame), .none): return transformNativeFrameForTesting?(frame) ?? frame
        case let (.none, .some(error)): emit(error)
        default: emit(Self.contractError("v2 result must carry exactly one of frame/error"))
        }
        return nil
    }

    @discardableResult
    private func adopt(
        _ frame: FfiNativeRenderFrame,
        strippingViewSelection: Bool
    ) -> EditorV2DerivedUpdate? {
        let stageStarted = PreparedProseInstrumentation.now()
        defer { PreparedProseInstrumentation.recordTableStage(.adapterAdoption, start: stageStarted) }
        guard let snapshot = Self.parseAtomicRenderSnapshot(frame.snapshotJson) else { return nil }
        let framePositionEpoch = frame.positionEpoch.flatMap { UInt64($0) }
        guard framePositionEpoch.map(String.init) == frame.positionEpoch else { return nil }
        let nextIndex = tableIndex.copy()
        guard case let .success(changes) = nextIndex.adopt(
            frame.tables, installedRevision: installedFrameRevision, frameRevision: snapshot.documentRevision
        ), nextIndex.rootExtents.values.allSatisfy({ $0.scalarEnd <= snapshot.scalarLength }) else { return nil }
        var canReuseRetainedSemanticAdmission = false
        var candidate = snapshot.renderObject["renderBlocks"] as? [[NSDictionary]]
        var patchBlocks: [[NSDictionary]]?
        if candidate == nil, let patch = snapshot.renderObject["renderPatch"] as? [String: Any] {
            if let retained = cachedSemanticRenderBlocks,
               let base = patch["baseDocumentVersion"] as? String, UInt64(base) == cachedAtomicRenderDocumentRevision,
               let start = Self.uint32Field(patch, "startIndex"), let delete = Self.uint32Field(patch, "deleteCount"),
               let inserted = patch["renderBlocks"] as? [[NSDictionary]],
               UInt64(start) + UInt64(delete) <= UInt64(retained.count) {
                candidate = retained
                patchBlocks = inserted
                candidate!.replaceSubrange(Int(start)..<(Int(start) + Int(delete)), with: inserted)
                canReuseRetainedSemanticAdmission = frame.tables.kind == .delta
                    && installedFrameRevision == cachedAtomicRenderDocumentRevision
                    && tableIndex.tableKeys.isEmpty
                    && tableIndex.rootExtents.isEmpty
                    && nextIndex.tableKeys.isEmpty
                    && nextIndex.rootExtents.isEmpty
            } else { return nil }
        }
        guard let candidate else { return nil }
        let rootKeys = Set(candidate.joined().compactMap(RenderBridge.rootTableID(of:)))
        guard (canReuseRetainedSemanticAdmission && rootKeys.isEmpty)
            || Self.validSemanticRenderElements(candidate.joined().map { $0 as Any }, tableIndex: nextIndex)
        else { return nil }
        guard rootKeys == Set(nextIndex.rootExtents.keys) else { return nil }
        if let selection = snapshot.renderObject["selection"] as? [String: Any], selection["type"] as? String == "cell",
           EditorCellSelection.resolve(selection, index: nextIndex) == nil { return nil }
        guard let derived = Self.viewUpdate(
            from: snapshot,
            renderBlocks: candidate,
            patchBlocks: patchBlocks,
            strippingViewSelection: strippingViewSelection
        ) else {
            return nil
        }
        let tablePresentation = EditorTablePresentationSnapshot(
            documentRevision: snapshot.documentRevision,
            baseDocumentRevision: changes.fullReset ? nil : installedFrameRevision,
            positionEpoch: framePositionEpoch,
            index: nextIndex, changes: changes
        )
        tableIndex = nextIndex
        installedFrameRevision = snapshot.documentRevision
        if changes.fullReset { fullFrameAdoptionCountForTesting += 1 } else { deltaFrameAdoptionCountForTesting += 1 }
        baseDocumentRevision = snapshot.documentRevision
        cachedAtomicRenderDocumentRevision = snapshot.documentRevision
        cachedSemanticRenderBlocks = candidate
        if let epoch = framePositionEpoch {
            positionEpoch = epoch
        }
        installRenderState(
            snapshot,
            derived: derived,
            atomicRenderJSON: frame.snapshotJson,
            tablePresentation: tablePresentation
        )
        return derived
    }

    private func installRenderState(
        _ snapshot: AtomicRenderSnapshot,
        derived: EditorV2DerivedUpdate,
        atomicRenderJSON: String,
        tablePresentation: EditorTablePresentationSnapshot
    ) {
        stateRevision = snapshot.stateRevision
        cachedScalarLength = snapshot.scalarLength
        cachedActiveState = snapshot.activeState
        cachedHistoryState = snapshot.historyState
        cachedViewUpdate = derived
        cachedAtomicRenderJSON = atomicRenderJSON
        cachedAtomicRenderSelectionObject = snapshot.renderObject["selection"] as? [String: Any]
        cachedTablePresentation = tablePresentation
        cachedAuthoritativeScalarSelection = snapshot.selection
    }

    func adoptSelectionState(_ selectionState: [String: Any]) -> EditorV2DerivedUpdate? {
        let stageStarted = PreparedProseInstrumentation.now()
        defer { PreparedProseInstrumentation.recordTableStage(.adapterAdoption, start: stageStarted) }
        guard Set(selectionState.keys) == Self.renderStateKeys,
              let documentRevision = Self.uint64Field(selectionState, "documentVersion"),
              documentRevision == baseDocumentRevision,
              documentRevision == installedFrameRevision,
              documentRevision == cachedAtomicRenderDocumentRevision,
              let retained = cachedSemanticRenderBlocks
        else { return nil }
        var renderObject = selectionState
        renderObject["renderBlocks"] = NSNull()
        renderObject["renderPatch"] = [
            "baseDocumentVersion": String(documentRevision),
            "startIndex": retained.count,
            "deleteCount": 0,
            "renderBlocks": [Any]()
        ] as [String: Any]
        guard let snapshot = Self.atomicRenderSnapshot(validatingStateOf: renderObject),
              snapshot.scalarLength == cachedScalarLength
        else { return nil }
        if let selection = renderObject["selection"] as? [String: Any], selection["type"] as? String == "cell",
           EditorCellSelection.resolve(selection, index: tableIndex) == nil { return nil }
        guard let atomicData = try? JSONSerialization.data(withJSONObject: renderObject),
              let atomicRenderJSON = String(data: atomicData, encoding: .utf8),
              let derived = Self.viewUpdate(
                  from: snapshot,
                  renderBlocks: retained,
                  patchBlocks: [],
                  strippingViewSelection: false
              )
        else { return nil }
        installRenderState(
            snapshot,
            derived: derived,
            atomicRenderJSON: atomicRenderJSON,
            tablePresentation: EditorTablePresentationSnapshot(
                documentRevision: documentRevision,
                baseDocumentRevision: documentRevision,
                positionEpoch: positionEpoch,
                index: tableIndex,
                changes: TableFrameChanges(fullReset: false, replacedTables: [], removedTables: [], changedCells: [:])
            )
        )
        return derived
    }

    func atomicRenderJSON(matchingDocumentRevision documentRevision: UInt64) -> String? {
        guard beginRuntimeOperation() else { return nil }
        defer { endRuntimeOperation() }
        guard cachedAtomicRenderDocumentRevision == documentRevision else { return nil }
        return cachedAtomicRenderJSON
    }

    private func parseExternalReset(_ resetJSON: String) -> (payload: [String: Any], revision: UInt64)? {
        guard let data = resetJSON.data(using: .utf8),
              let reset = try? JSONSerialization.jsonObject(with: data) as? [String: Any],
              reset["history"] as? String == "resetAndClear",
              Set(reset.keys) == ["history", "documentRevision", reset["setJson"] != nil ? "setJson" : "setHtml"],
              reset["setJson"] is [String: Any] || reset["setHtml"] is String,
              let revision = Self.uint64Field(reset, "documentRevision")
        else {
            rejectExternalRenderEnvelope("external reset intent is malformed")
            return nil
        }
        return (reset, revision)
    }

    func validateExternalReset(_ resetJSON: String) -> Bool {
        parseExternalReset(resetJSON) != nil
    }

    func adoptExternalReset(resetJSON: String) -> String? {
        guard beginRuntimeOperation() else { return nil }
        defer { endRuntimeOperation() }
        guard let intent = parseExternalReset(resetJSON) else { return nil }
        var reset = intent.payload
        let resetRevision = intent.revision
        guard !destroyed else {
            rejectExternalRenderEnvelope("external editor update notice is malformed")
            return nil
        }
        guard let current = refreshInternal(mirrorSelection: nil) else { return nil }
        if baseDocumentRevision == resetRevision {
            tableResetGeneration &+= 1
            return current.updateJSON
        }
        if latestJSDrivenDocumentRevision > resetRevision { return current.updateJSON }
        switch Self.normalizeJsonResult(editorV2GetState(editorId: editorId)) {
        case .failure(let error):
            emit(error)
            return nil
        case .success(let json):
            guard let data = json.data(using: .utf8),
                  let state = try? JSONSerialization.jsonObject(with: data) as? [String: Any],
                  let origin = state["documentOrigin"] as? String
            else {
                emit(contractError("v2 reset state violates the frozen shape"))
                return nil
            }
            if origin != "nativeView" {
                tableResetGeneration &+= 1
                return current.updateJSON
            }
        }
        reset.removeValue(forKey: "documentRevision")
        let result = callWithEnvelope(reset, includeBaseRevision: false) { requestJSON in
            editorV2ReplaceDocument(editorId: self.editorId, requestJson: requestJSON)
        }
        switch Self.normalizeJsonResult(result) {
        case .failure(let error):
            emit(error)
            return nil
        case .success(let json):
            guard let data = json.data(using: .utf8),
                  let commit = try? JSONSerialization.jsonObject(with: data) as? [String: Any],
                  let changed = commit["changed"] as? Bool,
                  Self.uint64Field(commit, "documentRevision") != nil
            else {
                emit(contractError("v2 reset result violates the frozen shape"))
                return nil
            }
            guard let update = refreshInternal(mirrorSelection: nil, strippingViewSelection: false) else {
                return nil
            }
            tableResetGeneration &+= 1
            if changed {
                publishCachedCollaborationSelection()
                notifyCollaborationMutation()
            }
            return update.updateJSON
        }
    }

    struct ExternalRenderNotice {
        let documentRevision: UInt64
        let stateRevision: UInt64?
    }

    func adoptExternalRender() -> String? {
        guard beginRuntimeOperation() else { return nil }
        defer { endRuntimeOperation() }
        guard !destroyed else {
            rejectExternalRenderEnvelope("external editor update adapter is destroyed")
            return nil
        }
        guard let update = refreshInternal(mirrorSelection: nil)?.updateJSON else { return nil }
        publishCollaborationCellsIfChanged()
        return update
    }

    func externalRenderNotice(_ renderJSON: String) -> ExternalRenderNotice? {
        guard beginRuntimeOperation() else { return nil }
        defer { endRuntimeOperation() }
        guard !destroyed, let data = renderJSON.data(using: .utf8),
              let object = try? JSONSerialization.jsonObject(with: data) as? [String: Any],
              let documentRevision = Self.uint64Field(object, "documentVersion") else {
            rejectExternalRenderEnvelope("external editor update notice is malformed")
            return nil
        }
        return ExternalRenderNotice(documentRevision: documentRevision, stateRevision: Self.uint64Field(object, "stateRevision"))
    }

    func pinCurrentPositionEpoch(_ documentRevision: UInt64) -> Bool {
        guard let nativeOwnerId else { return true }
        switch Self.normalizeJsonResult(
            editorV2PinPositionEpoch(
                editorId: editorId,
                ownerId: String(nativeOwnerId),
                documentRevision: String(documentRevision)
            )
        ) {
        case .failure(let error):
            emit(error)
            return false
        case .success(let json):
            guard let data = json.data(using: .utf8),
                  let object = try? JSONSerialization.jsonObject(with: data) as? [String: Any],
                  Set(object.keys) == ["positionEpoch"],
                  let value = object["positionEpoch"] as? String,
                  let epoch = UInt64(value), String(epoch) == value
            else {
                emit(Self.contractError("v2 position epoch result violates the frozen shape"))
                return false
            }
            positionEpoch = epoch
            return true
        }
    }

    /// Re-read the authoritative v2 state and update the render caches.
    @discardableResult
    func refreshInternal(
        mirrorSelection: (anchor: UInt32, head: UInt32)?,
        strippingViewSelection: Bool = false
    ) -> EditorV2DerivedUpdate? {
        guard !destroyed else {
            emit(
                FfiError(
                    domain: "lifecycle",
                    code: "ENGINE_DESTROYED",
                    message: "editor session is destroyed",
                    requestId: nil,
                    operationIndex: nil,
                    limit: nil,
                    actual: nil,
                    detailsJson: nil
                )
            )
            return nil
        }
        guard let frame = fetchNativeFrame(mirrorScalarSelection: mirrorSelection) else { return nil }
        if let derived = adopt(frame, strippingViewSelection: strippingViewSelection) { return derived }
        if frame.tables.kind == .delta {
            if let ownerId = nativeOwnerId {
                _ = editorV2ReleaseNativeBinding(editorId: editorId, ownerId: String(ownerId))
            }
            guard let full = fetchNativeFrame(mirrorScalarSelection: mirrorSelection) else { return nil }
            guard full.tables.kind == .full,
                  let derived = adopt(full, strippingViewSelection: strippingViewSelection) else {
                emit(Self.contractError("native table frame violates the frozen shape"))
                return nil
            }
            return derived
        }
        emit(Self.contractError("native table frame violates the frozen shape"))
        return nil
    }

    /// Public recovery entry (stale-revision recovery, external refresh).
    func refreshFromRustState(mirrorSelection: (anchor: UInt32, head: UInt32)?) -> String? {
        guard beginRuntimeOperation() else { return nil }
        defer { endRuntimeOperation() }
        return refreshInternal(mirrorSelection: mirrorSelection)?.updateJSON
    }

    func recoverNativeRender() -> String? {
        guard beginRuntimeOperation() else { return nil }
        defer { endRuntimeOperation() }
        if let ownerId = nativeOwnerId {
            _ = editorV2ReleaseNativeBinding(editorId: editorId, ownerId: String(ownerId))
        }
        return refreshFromRustState(mirrorSelection: nil)
    }

    /// Synthesized current-state update (selection/activeState included,
    /// mirroring the legacy `editorGetCurrentState` contract).
    func currentStateJSON() -> String? {
        guard beginRuntimeOperation() else { return nil }
        defer { endRuntimeOperation() }
        return refreshInternal(mirrorSelection: lastSyncedScalarSelection)?.updateJSON
    }

    func currentSelectionState() -> EditorSelectionState? {
        guard hasCurrentSelectionState || currentStateJSON() != nil,
              hasCurrentSelectionState,
              let documentRevision = cachedAtomicRenderDocumentRevision,
              let selection = cachedAtomicRenderSelectionObject,
              let activeState = cachedActiveState,
              let history = cachedHistoryState
        else { return nil }
        return EditorSelectionState(
            documentVersion: String(documentRevision),
            stateRevision: String(stateRevision),
            selection: selection,
            activeState: activeState,
            historyState: ["canUndo": history.canUndo, "canRedo": history.canRedo]
        )
    }

    private var hasCurrentSelectionState: Bool {
        guard !destroyed, cachedAtomicRenderDocumentRevision == baseDocumentRevision,
              let rendered = cachedAuthoritativeScalarSelection
        else { return false }
        guard let synced = lastSyncedScalarSelection else { return true }
        return synced == rendered
    }

    /// The initial bind render. The host passes this exact snapshot directly
    /// to the text view and toolbar, so it must not be replayed by a later
    /// independent current-state read.
    func initialUpdateJSON() -> String? {
        guard beginRuntimeOperation() else { return nil }
        defer { endRuntimeOperation() }
        guard var object = refreshInternal(mirrorSelection: nil)?.update,
              let blocks = cachedSemanticRenderBlocks else { return nil }
        object["renderBlocks"] = blocks
        object["renderPatch"] = NSNull()
        guard let complete = try? JSONSerialization.data(withJSONObject: object) else { return nil }
        return String(data: complete, encoding: .utf8)
    }

    func documentHtml() -> String? {
        guard beginRuntimeOperation() else { return nil }
        defer { endRuntimeOperation() }
        guard !destroyed else { return nil }
        switch Self.normalizeJsonResult(editorV2GetDocumentHtml(editorId: editorId)) {
        case .failure(let error):
            emit(error)
            return nil
        case .success(let json):
            guard let data = json.data(using: .utf8),
                  let object = try? JSONSerialization.jsonObject(with: data) as? [String: Any]
            else {
                emit(contractError("v2 getDocumentHtml value violates the frozen shape"))
                return nil
            }
            return object["html"] as? String
        }
    }

    /// The authoritative v2 document JSON.
    func documentJson() -> String? {
        guard beginRuntimeOperation() else { return nil }
        defer { endRuntimeOperation() }
        guard !destroyed else { return nil }
        return fetchDocumentJson()
    }

    /// The v2 content snapshot `{html, json}` (same frozen shape as legacy).
    func contentSnapshotJSON() -> String? {
        guard beginRuntimeOperation() else { return nil }
        defer { endRuntimeOperation() }
        guard !destroyed else { return nil }
        switch Self.normalizeJsonResult(editorV2GetContentSnapshot(editorId: editorId)) {
        case .failure(let error):
            emit(error)
            return nil
        case .success(let json):
            return json
        }
    }

    /// Engine-owned history flags (module `editorCanUndo/editorCanRedo`).
    func historyFlags() -> (canUndo: Bool, canRedo: Bool)? {
        guard beginRuntimeOperation() else { return nil }
        defer { endRuntimeOperation() }
        if let cachedHistoryState { return cachedHistoryState }
        guard refreshInternal(mirrorSelection: nil) != nil else { return nil }
        return cachedHistoryState
    }

    /// The resolved selection JSON (legacy `editorGetSelection` shape).
    func selectionJSON() -> String? {
        guard beginRuntimeOperation() else { return nil }
        defer { endRuntimeOperation() }
        guard let selection = refreshInternal(mirrorSelection: lastSyncedScalarSelection)?.update["selection"],
              let data = try? JSONSerialization.data(withJSONObject: selection)
        else {
            return nil
        }
        return String(data: data, encoding: .utf8)
    }

}

struct EditorSelectionState {
    static let activeStateKey = "activeState"
    static let activeStateRevisionKey = "activeStateRevision"

    let documentVersion: String
    let stateRevision: String
    let selection: [String: Any]
    let activeState: [String: Any]
    let historyState: [String: Any]

    var update: [String: Any] {
        update(activeStateKey: Self.activeStateKey, activeStateValue: activeState)
    }

    func update(activeStateKey: String, activeStateValue: Any) -> [String: Any] {
        [
            "documentVersion": documentVersion,
            "stateRevision": stateRevision,
            "selection": selection,
            activeStateKey: activeStateValue,
            "historyState": historyState
        ]
    }
}

final class SelectionEventActiveStateChannel {
    private var emittedActiveState: NSDictionary?
    private var emittedStateRevision: String?

    func update(for state: EditorSelectionState) -> [String: Any] {
        let activeState = state.activeState as NSDictionary
        if let emittedStateRevision, emittedActiveState == activeState {
            return state.update(
                activeStateKey: EditorSelectionState.activeStateRevisionKey,
                activeStateValue: emittedStateRevision
            )
        }
        emittedActiveState = activeState
        emittedStateRevision = state.stateRevision
        return state.update
    }

    func reset() {
        emittedActiveState = nil
        emittedStateRevision = nil
    }
}
