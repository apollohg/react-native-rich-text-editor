import Foundation

extension EditorV2Adapter {
    private static func viewUpdate(
        from snapshot: AtomicRenderSnapshot,
        strippingViewSelection: Bool
    ) -> String? {
        guard strippingViewSelection,
              let data = snapshot.viewUpdateJSON.data(using: .utf8),
              var update = try? JSONSerialization.jsonObject(with: data) as? [String: Any]
        else {
            return strippingViewSelection ? nil : snapshot.viewUpdateJSON
        }
        update.removeValue(forKey: "selection")
        guard let strippedData = try? JSONSerialization.data(withJSONObject: update) else { return nil }
        return String(data: strippedData, encoding: .utf8)
    }

    private func fetchNativeFrame(mirrorScalarSelection: (anchor: UInt32, head: UInt32)?) -> FfiNativeRenderFrame? {
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
        guard let snapshot = Self.parseAtomicRenderSnapshot(frame.snapshotJson) else { return nil }
        let nextIndex = tableIndex.copy()
        guard case let .success(changes) = nextIndex.adopt(
            frame.tables, installedRevision: installedFrameRevision, frameRevision: snapshot.documentRevision
        ), nextIndex.rootExtents.values.allSatisfy({ $0.scalarEnd <= snapshot.scalarLength }) else { return nil }
        var candidate = snapshot.renderObject["renderBlocks"] as? [[[String: Any]]]
        if candidate == nil, let patch = snapshot.renderObject["renderPatch"] as? [String: Any] {
            if let retained = cachedSemanticRenderBlocks,
               let base = patch["baseDocumentVersion"] as? String, UInt64(base) == cachedAtomicRenderDocumentRevision,
               let start = Self.uint32Field(patch, "startIndex"), let delete = Self.uint32Field(patch, "deleteCount"),
               let inserted = patch["renderBlocks"] as? [[[String: Any]]],
               UInt64(start) + UInt64(delete) <= UInt64(retained.count) {
                candidate = retained
                candidate!.replaceSubrange(Int(start)..<(Int(start) + Int(delete)), with: inserted)
            } else { return nil }
        }
        guard let candidate, Self.validSemanticRenderElements(candidate.joined().map { $0 as Any }, tableIndex: nextIndex) else { return nil }
        let rootKeys = Set(candidate.joined().compactMap { $0["type"] as? String == "table" ? $0["tableId"] as? String : nil })
        guard rootKeys == Set(nextIndex.rootExtents.keys) else { return nil }
        if let selection = snapshot.renderObject["selection"] as? [String: Any], selection["type"] as? String == "cell",
           EditorCellSelection.resolve(selection, index: nextIndex) == nil { return nil }
        guard let updateJSON = Self.viewUpdate(
            from: snapshot,
            strippingViewSelection: strippingViewSelection
        ) else {
            return nil
        }
        let tablePresentation = EditorTablePresentationSnapshot(
            documentRevision: snapshot.documentRevision, positionEpoch: snapshot.positionEpoch,
            index: nextIndex, changes: changes
        )
        tableIndex = nextIndex
        installedFrameRevision = snapshot.documentRevision
        if changes.fullReset { fullFrameAdoptionCountForTesting += 1 }
        else { deltaFrameAdoptionCountForTesting += 1 }
        baseDocumentRevision = snapshot.documentRevision
        stateRevision = snapshot.stateRevision
        cachedScalarLength = snapshot.scalarLength
        cachedActiveState = snapshot.activeState
        cachedHistoryState = snapshot.historyState
        cachedViewUpdateJSON = updateJSON
        cachedAtomicRenderJSON = frame.snapshotJson
        cachedAtomicRenderSelectionObject = snapshot.renderObject["selection"] as? [String: Any]
        cachedAtomicRenderDocumentRevision = snapshot.documentRevision
        cachedSemanticRenderBlocks = candidate
        cachedTablePresentation = tablePresentation
        if let epoch = snapshot.positionEpoch {
            positionEpoch = epoch
        }
        cachedAuthoritativeScalarSelection = snapshot.selection
        return EditorV2DerivedUpdate(updateJSON: updateJSON, scalarLength: snapshot.scalarLength)
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

    func adoptExternalReset(_ renderJSON: String, resetJSON: String) -> String? {
        guard beginRuntimeOperation() else { return nil }
        defer { endRuntimeOperation() }
        guard let intent = parseExternalReset(resetJSON) else { return nil }
        var reset = intent.payload
        let resetRevision = intent.revision
        guard validateExternalRender(renderJSON), let current = refreshInternal(mirrorSelection: nil) else { return nil }
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

    func adoptExternalRender(_ renderJSON: String) -> String? {
        guard beginRuntimeOperation() else { return nil }
        defer { endRuntimeOperation() }
        guard !destroyed else {
            rejectExternalRenderEnvelope("external editor update adapter is destroyed")
            return nil
        }
        guard validateExternalRender(renderJSON) else { return nil }
        guard let update = refreshInternal(mirrorSelection: nil)?.updateJSON else { return nil }
        publishCollaborationCellsIfChanged()
        return update
    }

    func validateExternalRender(_ renderJSON: String) -> Bool {
        guard beginRuntimeOperation() else { return false }
        defer { endRuntimeOperation() }
        guard !destroyed, let data = renderJSON.data(using: .utf8),
              let object = try? JSONSerialization.jsonObject(with: data) as? [String: Any],
              Self.uint64Field(object, "documentVersion") != nil else {
            rejectExternalRenderEnvelope("external editor update notice is malformed")
            return false
        }
        return true
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

    func currentSelectionStateJSON() -> String? {
        guard hasCurrentSelectionState || currentStateJSON() != nil,
              hasCurrentSelectionState,
              let documentRevision = cachedAtomicRenderDocumentRevision,
              let selection = cachedAtomicRenderSelectionObject,
              let activeState = cachedActiveState,
              let history = cachedHistoryState,
              let data = try? JSONSerialization.data(withJSONObject: [
                  "documentVersion": String(documentRevision),
                  "selection": selection,
                  "activeState": activeState,
                  "historyState": ["canUndo": history.canUndo, "canRedo": history.canRedo]
              ])
        else { return nil }
        return String(data: data, encoding: .utf8)
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
        guard let update = refreshInternal(mirrorSelection: nil)?.updateJSON,
              let blocks = cachedSemanticRenderBlocks,
              let data = update.data(using: .utf8),
              var object = try? JSONSerialization.jsonObject(with: data) as? [String: Any] else { return nil }
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
        guard let derived = refreshInternal(mirrorSelection: lastSyncedScalarSelection),
              let updateData = derived.updateJSON.data(using: .utf8),
              let update = try? JSONSerialization.jsonObject(with: updateData) as? [String: Any],
              let selection = update["selection"],
              let data = try? JSONSerialization.data(withJSONObject: selection)
        else {
            return nil
        }
        return String(data: data, encoding: .utf8)
    }

}
