import Foundation

extension EditorV2Adapter {
    struct TableMutationAdmission: Equatable {
        let tableID: String
        let documentRevision: UInt64
        let presentationGeneration: UInt64
        let ownerID: UInt64
        let ownerToken: UUID
    }

    struct TableCellSelectionAdmission {
        let tableID: String
        let documentRevision: UInt64
        var positionEpoch: UInt64
        let presentationGeneration: UInt64
        let ownerID: UInt64
        let ownerToken: UUID
        var anchor: UInt32
        var head: UInt32

        var mutation: TableMutationAdmission {
            TableMutationAdmission(
                tableID: tableID,
                documentRevision: documentRevision,
                presentationGeneration: presentationGeneration,
                ownerID: ownerID,
                ownerToken: ownerToken
            )
        }
    }

    func cachedAtomicRenderSelection() -> [String: Any]? {
        guard let render = cachedAtomicRenderJSON?.data(using: .utf8),
              let object = try? JSONSerialization.jsonObject(with: render) as? [String: Any]
        else { return nil }
        return object["selection"] as? [String: Any]
    }

    func admitsTableMutation(_ admission: TableMutationAdmission) -> Bool {
        !destroyed
            && baseDocumentRevision == admission.documentRevision
            && cachedAtomicRenderDocumentRevision == admission.documentRevision
            && cachedTablePresentation?.documentRevision == admission.documentRevision
            && tableResetGeneration == admission.presentationGeneration
            && nativeOwnerId == admission.ownerID
            && nativeOwnerToken == admission.ownerToken
            && cachedTableRecords[admission.tableID]?["readOnlyDescendants"] as? Bool == false
    }

    func selectedTableCellsMutationAdmission() -> TableMutationAdmission? {
        guard let ownerID = nativeOwnerId,
              let ownerToken = nativeOwnerToken,
              let selection = cachedAtomicRenderSelection(),
              case let .drawable(tableID, _) = EditorCellSelection.resolve(selection, records: cachedTableRecords)
        else { return nil }
        let admission = TableMutationAdmission(
            tableID: tableID,
            documentRevision: baseDocumentRevision,
            presentationGeneration: tableResetGeneration,
            ownerID: ownerID,
            ownerToken: ownerToken
        )
        return admitsTableMutation(admission) ? admission : nil
    }

    private func cachedSelectionIsExactCells(anchor: UInt32, head: UInt32, tableID: String) -> Bool {
        guard let selection = cachedAtomicRenderSelection(),
              let endpoints = EditorCellSelection.endpointPositions(selection),
              endpoints.anchor == anchor,
              endpoints.head == head,
              case let .drawable(selectedTableID, _) = EditorCellSelection.resolve(
                selection, records: cachedTableRecords
              )
        else { return false }
        return selectedTableID == tableID
    }

    func admitsTableCellSelection(_ admission: TableCellSelectionAdmission) -> Bool {
        admitsTableMutation(admission.mutation)
            && positionEpoch == admission.positionEpoch
            && cachedSelectionIsExactCells(anchor: admission.anchor, head: admission.head,
                                           tableID: admission.tableID)
    }

    private func applySelectionEnvelope(_ selection: [String: Any]) -> String? {
        performMutation(adoptEngineSelection: true, publishMutation: false) {
            self.callWithEnvelope(["selection": selection]) { requestJSON in
                editorV2SetSelection(editorId: self.editorId, requestJson: requestJSON)
            }
        }
    }

    private static func exactCellSelectionEnvelope(anchor: UInt32, head: UInt32) -> [String: Any] {
        [
            "type": "cell",
            "anchorCell": ["offset": Int(anchor), "kind": "document"],
            "headCell": ["offset": Int(head), "kind": "document"]
        ]
    }

    func selectExactTableCells(anchor: UInt32, head: UInt32,
                               admission: TableCellSelectionAdmission) -> String? {
        guard beginRuntimeOperation() else { return nil }
        defer { endRuntimeOperation() }
        guard admitsTableCellSelection(admission),
              let cells = cachedTableRecords[admission.tableID]?["cells"] as? [[String: Any]],
              cells.contains(where: { Self.uint32Field($0, "sourcePos") == anchor }),
              cells.contains(where: { Self.uint32Field($0, "sourcePos") == head })
        else { return nil }
        let update = applySelectionEnvelope(Self.exactCellSelectionEnvelope(anchor: anchor, head: head))
        if update != nil {
            publishCollaborationCellsIfChanged()
        }
        guard let update,
              admitsTableMutation(admission.mutation),
              positionEpoch != nil,
              cachedSelectionIsExactCells(anchor: anchor, head: head, tableID: admission.tableID)
        else { return nil }
        return update
    }

    func resizeTableColumn(column: Int, width: Int, admission: TableMutationAdmission) -> String? {
        guard beginRuntimeOperation() else { return nil }
        defer { endRuntimeOperation() }
        guard admitsTableMutation(admission),
              column >= 0,
              let record = cachedTableRecords[admission.tableID],
              let tablePos = Self.uint32Field(record, "tablePos")
        else { return nil }
        let command: [String: Any] = [
            "type": "setTableColumnWidth",
            "width": width,
            "column": column,
            "tablePos": Int(tablePos)
        ]
        return performMutation(adoptEngineSelection: true) {
            self.callWithEnvelope(["command": command]) { requestJson in
                editorV2ApplyCommand(editorId: self.editorId, requestJson: requestJson)
            }
        }
    }

    func syncNodeSelection(docPos: UInt32) -> EditorV2SelectionSync? {
        guard beginRuntimeOperation() else { return nil }
        defer { endRuntimeOperation() }
        guard let update = applySelectionEnvelope(["type": "atom", "docPos": Int(docPos), "edge": "node"]),
            let data = update.data(using: .utf8),
            let object = try? JSONSerialization.jsonObject(with: data) as? [String: Any],
            let selection = object["selection"] as? [String: Any]
        else { return nil }
        if selection["type"] as? String == "node",
           let pos = Self.uint32Field(selection, "pos") {
            publishCollaborationSelection(docAnchor: pos, docHead: pos + 1)
            return EditorV2SelectionSync(docAnchor: pos, docHead: pos, refreshedUpdateJSON: update)
        }
        guard let mapping = Self.textDocumentSelection(from: update) else { return nil }
        publishCollaborationSelection(docAnchor: mapping.docAnchor, docHead: mapping.docHead)
        return EditorV2SelectionSync(
            docAnchor: mapping.docAnchor,
            docHead: mapping.docHead,
            refreshedUpdateJSON: update
        )
    }

    enum SelectionSyncOutcome {
        case ok
        case refreshed(String)
        case failed
    }

    /// Clamp one scalar position into the cached document extent (the legacy
    /// engine clamped leniently; the v2 engine rejects out-of-range scalars
    /// with POSITION_INVALID — transient-IME cursors can overshoot).
    func clampScalar(_ scalar: UInt32) -> UInt32 {
        guard let extent = cachedScalarLength else { return scalar }
        return min(scalar, extent)
    }

    /// Cheap selection sync (no mapping harvest): one skip transaction.
    @discardableResult
    func ensureSelection(anchor: UInt32, head: UInt32) -> SelectionSyncOutcome {
        let clampedAnchor = clampScalar(anchor)
        let clampedHead = clampScalar(head)
        if let last = lastSyncedScalarSelection, last == (clampedAnchor, clampedHead) {
            return .ok
        }
        // Affinity policy mirrors the engine's own cursor resolution
        // (cursor_sticky_index_from_doc_pos): a collapsed caret prefers
        // After with a deterministic Before fallback at text-boundary
        // positions; a range uses Before. The fallback changes only the
        // stickiness of the SAME position — it is not a guessed-position
        // retry.
        let collapsed = clampedAnchor == clampedHead
        var result = callWithEnvelope(
            selectionEnvelope(
                anchor: clampedAnchor,
                head: clampedHead,
                affinity: collapsed ? "after" : "before"
            )
        ) { requestJson in
            editorV2SetSelection(editorId: self.editorId, requestJson: requestJson)
        }
        if collapsed, let error = result.error, error.code == "POSITION_INVALID" {
            result = callWithEnvelope(
                selectionEnvelope(anchor: clampedAnchor, head: clampedHead, affinity: "before")
            ) { requestJson in
                editorV2SetSelection(editorId: self.editorId, requestJson: requestJson)
            }
        }
        switch Self.normalizeJsonResult(result) {
        case .success(let value):
            if let outcome = parseMutationOutcome(value), case .transaction(_, let revision) = outcome.kind {
                baseDocumentRevision = revision
            }
            lastSyncedScalarSelection = (clampedAnchor, clampedHead)
            return .ok
        case .failure(let error):
            if error.code == "REVISION_MISMATCH" {
                if let update = refreshInternal(
                    mirrorSelection: nil,
                    strippingViewSelection: false
                )?.updateJSON {
                    return .refreshed(update)
                }
                return .failed
            }
            emit(error)
            return .failed
        }
    }

    /// Selection sync with the engine-authoritative doc-position mapping the
    /// delegate callback requires.
    @discardableResult
    func syncSelection(anchor: UInt32, head: UInt32) -> EditorV2SelectionSync? {
        guard beginRuntimeOperation() else { return nil }
        defer { endRuntimeOperation() }
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
        if nativeOwnerId != nil {
            let previousDocumentRevision = baseDocumentRevision
            guard let update = performNativeIntent(
                nativeIntent("setSelection", anchor: anchor, head: head)
            )?.updateJSON,
                let mapping = Self.textDocumentSelection(from: update)
            else {
                return nil
            }
            publishCollaborationSelection(docAnchor: mapping.docAnchor, docHead: mapping.docHead)
            return EditorV2SelectionSync(
                docAnchor: mapping.docAnchor,
                docHead: mapping.docHead,
                refreshedUpdateJSON: baseDocumentRevision == previousDocumentRevision ? nil : update
            )
        }
        var refreshedUpdateJSON: String?
        let mapping: (docAnchor: UInt32, docHead: UInt32)?
        switch ensureSelection(anchor: anchor, head: head) {
        case .ok:
            mapping = resolveSelectionMapping(scalarAnchor: anchor, scalarHead: head)
        case .refreshed(let updateJSON):
            refreshedUpdateJSON = updateJSON
            mapping = Self.textDocumentSelection(from: updateJSON)
        case .failed:
            return nil
        }
        guard let mapping else { return nil }
        publishCollaborationSelection(
            docAnchor: mapping.docAnchor,
            docHead: mapping.docHead
        )
        return EditorV2SelectionSync(
            docAnchor: mapping.docAnchor,
            docHead: mapping.docHead,
            refreshedUpdateJSON: refreshedUpdateJSON
        )
    }

    private static func textDocumentSelection(
        from updateJSON: String
    ) -> (docAnchor: UInt32, docHead: UInt32)? {
        guard let data = updateJSON.data(using: .utf8),
              let update = try? JSONSerialization.jsonObject(with: data) as? [String: Any],
              let selection = update["selection"] as? [String: Any],
              selection["type"] as? String == "text",
              let docAnchor = uint32Field(selection, "anchor"),
              let docHead = uint32Field(selection, "head")
        else {
            return nil
        }
        return (docAnchor, docHead)
    }

    /// Engine-authoritative scalar→doc selection mapping for one live v2
    /// session (the delegate callback's doc positions), resolved by the
    /// v2 accessor with the legacy lenient-mapping semantics.
    private func resolveSelectionMapping(
        scalarAnchor: UInt32,
        scalarHead: UInt32
    ) -> (docAnchor: UInt32, docHead: UInt32)? {
        let result = editorV2ResolveScalarSelection(
            editorId: editorId,
            scalarAnchor: scalarAnchor,
            scalarHead: scalarHead
        )
        switch Self.normalizeJsonResult(result) {
        case .failure(let error):
            debugNotes.append("resolveScalarSelection \(error.domain)/\(error.code)")
            return nil
        case .success(let json):
            guard let data = json.data(using: .utf8),
                  let selection = try? JSONSerialization.jsonObject(with: data) as? [String: Any],
                  let docAnchor = Self.uint32Field(selection, "anchor"),
                  let docHead = Self.uint32Field(selection, "head")
            else {
                debugNotes.append("resolveScalarSelection shape invalid")
                return nil
            }
            return (docAnchor, docHead)
        }
    }

    @discardableResult
    func syncSelectionQuiet(anchor: UInt32, head: UInt32) -> String? {
        guard beginRuntimeOperation() else { return nil }
        defer { endRuntimeOperation() }
        guard !destroyed else { return nil }
        if nativeOwnerId != nil {
            let previousDocumentRevision = baseDocumentRevision
            guard let update = performNativeIntent(
                nativeIntent("setSelection", anchor: anchor, head: head)
            )?.updateJSON else {
                return nil
            }
            if let mapping = Self.textDocumentSelection(from: update) {
                publishCollaborationSelection(docAnchor: mapping.docAnchor, docHead: mapping.docHead)
            }
            return baseDocumentRevision == previousDocumentRevision ? nil : update
        }
        var refreshedUpdateJSON: String?
        let mapping: (docAnchor: UInt32, docHead: UInt32)?
        switch ensureSelection(anchor: anchor, head: head) {
        case .ok:
            guard let selection = lastSyncedScalarSelection else { return nil }
            mapping = resolveSelectionMapping(
                scalarAnchor: selection.anchor,
                scalarHead: selection.head
            )
        case .refreshed(let updateJSON):
            refreshedUpdateJSON = updateJSON
            mapping = Self.textDocumentSelection(from: updateJSON)
        case .failed:
            return nil
        }
        guard let mapping else { return refreshedUpdateJSON }
        publishCollaborationSelection(
            docAnchor: mapping.docAnchor,
            docHead: mapping.docHead
        )
        return refreshedUpdateJSON
    }

    static let awarenessCellSelectionStaleCode = "AWARENESS_CELL_SELECTION_STALE"

    private func cachedCollaborationCells() -> (anchor: UInt32, head: UInt32)? {
        cachedAtomicRenderSelection().flatMap(EditorCellSelection.endpointPositions)
    }

    func publishCollaborationCellsIfChanged() {
        guard roomBound else { return }
        let cells = cachedCollaborationCells()
        guard (cells?.anchor, cells?.head)
                != (publishedCollaborationCells?.anchor, publishedCollaborationCells?.head)
        else { return }
        publishCachedCollaborationSelection()
    }

    func publishCachedCollaborationSelection() {
        if let cells = cachedCollaborationCells() {
            publishAwarenessSelection(
                ["type": "cell", "anchorCell": Int(cells.anchor), "headCell": Int(cells.head)],
                cells: cells
            )
            return
        }
        guard let selection = cachedAuthoritativeScalarSelection,
              let mapping = resolveSelectionMapping(
                  scalarAnchor: selection.anchor,
                  scalarHead: selection.head
              )
        else {
            return
        }
        publishCollaborationSelection(
            docAnchor: mapping.docAnchor,
            docHead: mapping.docHead
        )
    }

    private func publishCollaborationSelection(docAnchor: UInt32, docHead: UInt32) {
        publishAwarenessSelection(
            ["type": "text", "anchor": Int(docAnchor), "head": Int(docHead)],
            cells: nil
        )
    }

    private func publishAwarenessSelection(
        _ selection: [String: Any],
        cells: (anchor: UInt32, head: UInt32)?
    ) {
        guard roomBound, let nativeEditorId = UInt64(editorId) else { return }
        guard let data = try? JSONSerialization.data(withJSONObject: selection),
              let selectionJSON = String(data: data, encoding: .utf8)
        else {
            emit(contractError("awareness selection serialization failed"))
            return
        }
        switch Self.normalizeJsonResult(
            setAwarenessSelection(editorId, selectionJSON)
        ) {
        case .failure(let error) where error.code == Self.awarenessCellSelectionStaleCode:
            return
        case .failure(let error):
            emit(error)
        case .success(let value):
            guard let data = value.data(using: .utf8),
                  let object = try? JSONSerialization.jsonObject(with: data) as? [String: Any],
                  Set(object.keys) == ["outboundChanged"],
                  let outboundChanged = Self.exactBool(object["outboundChanged"])
            else {
                emit(contractError("awareness selection result violates the frozen shape"))
                return
            }
            publishedCollaborationCells = cells
            if outboundChanged {
                collaborationWake(nativeEditorId, .awareness)
            }
        }
    }

    /// Lenient scalar→doc mapping through the v2 accessor (clamps at
    /// the document extent, exactly the legacy `editorScalarToDoc` semantics).
    func documentPosition(forScalar scalar: UInt32) -> UInt32? {
        guard beginRuntimeOperation() else { return nil }
        defer { endRuntimeOperation() }
        switch Self.normalizeJsonResult(editorV2ScalarToDoc(editorId: editorId, scalar: scalar)) {
        case .failure(let error):
            emit(error)
            return nil
        case .success(let json):
            guard let data = json.data(using: .utf8),
                  let object = try? JSONSerialization.jsonObject(with: data) as? [String: Any]
            else {
                return nil
            }
            return Self.uint32Field(object, "doc")
        }
    }

    /// Lenient doc→scalar mapping through the v2 accessor (`docPos`
    /// of `UInt32.max` yields the document's scalar extent).
    func scalarPosition(forDoc docPos: UInt32) -> UInt32? {
        guard beginRuntimeOperation() else { return nil }
        defer { endRuntimeOperation() }
        switch Self.normalizeJsonResult(editorV2DocToScalar(editorId: editorId, docPos: docPos)) {
        case .failure(let error):
            emit(error)
            return nil
        case .success(let json):
            guard let data = json.data(using: .utf8),
                  let object = try? JSONSerialization.jsonObject(with: data) as? [String: Any]
            else {
                return nil
            }
            return Self.uint32Field(object, "scalar")
        }
    }

}
