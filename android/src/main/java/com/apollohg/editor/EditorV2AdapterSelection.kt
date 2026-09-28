package com.apollohg.editor

import com.apollohg.editor.tables.EditorCellSelection
import com.apollohg.editor.tables.resolveEditorCellSelection
import org.json.JSONArray
import org.json.JSONObject

internal sealed interface SelectionSyncOutcome {
    object Ok : SelectionSyncOutcome
    data class Refreshed(val updateJson: String) : SelectionSyncOutcome
    object Failed : SelectionSyncOutcome
}

internal data class TableMutationAdmission(
    val tableId: String,
    val documentRevision: ULong,
    val presentationGeneration: Long,
    val ownerId: String?,
    val ownerToken: Long?
)

internal fun EditorV2Adapter.tableMutationAdmission(tableId: String): TableMutationAdmission =
    TableMutationAdmission(tableId, baseDocumentRevision, tablePresentationDocumentGeneration,
        nativeOwnerId, currentNativeOwnerToken)

internal fun EditorV2Adapter.admitsTableMutation(admission: TableMutationAdmission): Boolean =
    !destroyed && baseDocumentRevision == admission.documentRevision &&
        cachedAtomicRenderDocumentRevision == admission.documentRevision &&
        tablePresentationDocumentGeneration == admission.presentationGeneration &&
        nativeOwnerId == admission.ownerId && currentNativeOwnerToken == admission.ownerToken &&
        cachedTableRecords[admission.tableId]?.optBoolean("readOnlyDescendants", true) == false

internal fun EditorV2Adapter.selectedTableCellsMutationAdmission(): TableMutationAdmission? {
    val selection = cachedAtomicRenderSelection() ?: return null
    val cells = resolveEditorCellSelection(selection, cachedTableRecords)
        as? EditorCellSelection.Drawable ?: return null
    return tableMutationAdmission(cells.tableId).takeIf(::admitsTableMutation)
}

internal fun EditorV2Adapter.cachedAtomicRenderSelection(): JSONObject? =
    cachedAtomicRenderSelectionObject?.let { JSONObject(it.toString()) }

internal const val AWARENESS_CELL_SELECTION_STALE = "AWARENESS_CELL_SELECTION_STALE"

internal fun cellSelectionEndpoints(selection: JSONObject): Pair<Int, Int>? {
    if (selection.optString("type") != "cell") return null
    val anchor = exactV2ScalarInt(selection.opt("anchorCell") as? Number) ?: return null
    val head = exactV2ScalarInt(selection.opt("headCell") as? Number) ?: return null
    return anchor to head
}

internal fun EditorV2Adapter.selectExactTableCells(
    anchorCell: Int,
    headCell: Int,
    admission: TableMutationAdmission,
    expectedEpoch: String,
    expectedAnchor: Int,
    expectedHead: Int
): String? {
    if (!admitsTableMutation(admission) || positionEpoch != expectedEpoch) return null
    val current = cachedAtomicRenderSelection() ?: return null
    if (cellSelectionEndpoints(current) != expectedAnchor to expectedHead) return null
    fun point(opening: Int) = JSONObject().put("kind", "document").put("offset", opening)
    val selection = JSONObject().put("type", "cell")
        .put("anchorCell", point(anchorCell)).put("headCell", point(headCell))
    val result = callWithEnvelope(JSONObject().put("selection", selection)) {
        backend.setSelection(editorId, it)
    }
    if (result is EditorV2CallResult.Err) {
        if (result.error.code != "REVISION_MISMATCH" && result.error.code != "POSITION_INVALID") {
            emit(result.error)
        }
        return null
    }
    val update = refreshFromRustState(null) ?: return null
    val admitted = runCatching { updateSelection(update) }.getOrNull()
        ?: return null
    if (cellSelectionEndpoints(admitted) != anchorCell to headCell ||
        !admitsTableMutation(admission) || positionEpoch == null) return null
    publishCollaborationCellsIfChanged()
    return update
}

private fun EditorV2Adapter.applyAdmittedTableCommand(
    command: JSONObject,
    admission: TableMutationAdmission,
    targetsTable: Boolean
): String? {
    if (!admitsTableMutation(admission)) return null
    if (targetsTable) {
        val tablePos = exactV2ScalarInt(cachedTableRecords[admission.tableId]?.opt("tablePos") as? Number)
            ?: return null
        command.put("tablePos", tablePos)
    }
    return performMutation(adoptEngineSelection = true) {
        callWithEnvelope(JSONObject().put("command", command)) { requestJson ->
            backend.applyCommand(editorId, requestJson)
        }
    }
}

internal fun EditorV2Adapter.resizeTableColumn(
    column: Int,
    width: Int,
    admission: TableMutationAdmission
): String? {
    if (column < 0) return null
    return applyAdmittedTableCommand(
        JSONObject().put("type", "setTableColumnWidth").put("width", width).put("column", column),
        admission,
        targetsTable = true
    )
}

internal fun EditorV2Adapter.deleteTable(admission: TableMutationAdmission): String? =
    applyAdmittedTableCommand(JSONObject().put("type", "deleteTable"), admission, targetsTable = true)

internal fun EditorV2Adapter.applyTableCommandAtSelection(
    command: JSONObject,
    admission: TableMutationAdmission
): String? = applyAdmittedTableCommand(command, admission, targetsTable = false)

internal fun EditorV2Adapter.clampScalar(scalar: Int): Int {
    val extent = cachedScalarLength ?: return scalar
    return scalar.coerceIn(0, extent)
}

internal fun EditorV2Adapter.selectAtomNode(docPos: Int): String? {
    val selection = JSONObject().put("type", "atom").put("docPos", docPos).put("edge", "node")
    return when (
        val result = callWithEnvelope(JSONObject().put("selection", selection)) {
            backend.setSelection(editorId, it)
        }
    ) {
        is EditorV2CallResult.Err -> handleMutationError(result.error)

        is EditorV2CallResult.Ok -> {
            invalidateCachedAtomicState(null)
            recoverNativeRender()?.also { update ->
                val selection = runCatching { updateSelection(update) }.getOrNull()
                val pos = exactV2ScalarInt(selection?.opt("pos") as? Number)
                if (selection?.optString("type") == "node" && pos != null) {
                    publishCollaborationSelection(pos, pos + 1)
                }
            }
        }
    }
}

internal fun EditorV2Adapter.invalidateCachedAtomicState(selection: IntArray?) {
    cachedAuthoritativeScalarSelection = selection?.copyOf()
    cachedScalarLength = null
    cachedActiveState = null
    cachedHistoryState = null
    cachedViewUpdateJson = null
    cachedViewUpdateObject = null
    cachedAtomicRenderJson = null
    cachedAtomicRenderSelectionObject = null
    cachedTableAttributes = emptyMap()
    cachedTableRecords = emptyMap()
    cachedTableInputMappings = null
    cachedAtomicRenderDocumentRevision = null
}

internal fun EditorV2Adapter.ensureSelection(anchor: Int, head: Int): SelectionSyncOutcome {
    val clampedAnchor = clampScalar(anchor)
    val clampedHead = clampScalar(head)
    val last = lastSyncedScalarSelection
    if (last != null && last[0] == clampedAnchor && last[1] == clampedHead) {
        return SelectionSyncOutcome.Ok
    }
    // Affinity policy mirrors the engine's own cursor resolution: a
    // collapsed caret prefers After with a deterministic Before fallback
    // at text-boundary positions; a range uses Before. The fallback
    // changes only the stickiness of the SAME position.
    val collapsed = clampedAnchor == clampedHead
    var result = callWithEnvelope(
        selectionEnvelope(clampedAnchor, clampedHead, if (collapsed) "after" else "before")
    ) { requestJson ->
        backend.setSelection(editorId, requestJson)
    }
    if (collapsed &&
        result is EditorV2CallResult.Err &&
        result.error.code == "POSITION_INVALID"
    ) {
        result =
            callWithEnvelope(
                selectionEnvelope(clampedAnchor, clampedHead, "before")
            ) { requestJson ->
                backend.setSelection(editorId, requestJson)
            }
    }
    return when (result) {
        is EditorV2CallResult.Ok -> {
            parseMutationOutcome(result.value)?.let { outcome ->
                if (outcome is MutationOutcome.Transaction) {
                    baseDocumentRevision = outcome.revision
                }
            }
            val synchronizedSelection = intArrayOf(clampedAnchor, clampedHead)
            lastSyncedScalarSelection = synchronizedSelection
            cachedAuthoritativeScalarSelection = synchronizedSelection.copyOf()
            // Keep the last history result until a document mutation replaces it.
            cachedActiveState = null
            cachedViewUpdateJson = null
            cachedViewUpdateObject = null
            cachedAtomicRenderJson = null
            cachedAtomicRenderSelectionObject = null
            cachedTableAttributes = emptyMap()
            cachedTableRecords = emptyMap()
            cachedTableInputMappings = null
            cachedAtomicRenderDocumentRevision = null
            SelectionSyncOutcome.Ok
        }

        is EditorV2CallResult.Err -> {
            if (result.error.code == "REVISION_MISMATCH") {
                val update = refreshInternal(null, stripViewSelection = false)
                if (update != null) {
                    SelectionSyncOutcome.Refreshed(update)
                } else {
                    SelectionSyncOutcome.Failed
                }
            } else {
                emit(result.error)
                SelectionSyncOutcome.Failed
            }
        }
    }
}

internal fun EditorV2Adapter.textDocumentSelection(updateJson: String): IntArray? {
    return try {
        val selection = JSONObject(updateJson).getJSONObject("selection")
        if (selection.optString("type") != "text") return null
        intArrayOf(
            scalarField(selection, "anchor") ?: return null,
            scalarField(selection, "head") ?: return null
        )
    } catch (error: Exception) {
        null
    }
}

internal fun EditorV2Adapter.resolveSelectionMapping(anchor: Int, head: Int): IntArray? {
    // Engine-authoritative scalar→doc selection mapping for the delegate
    // callback's doc positions (v2 accessor).
    val resolved = when (val result = backend.resolveScalarSelection(editorId, anchor, head)) {
        is EditorV2CallResult.Err -> {
            debugNotes.add("resolveScalarSelection ${result.error.domain}/${result.error.code}")
            return null
        }

        is EditorV2CallResult.Ok -> result.value
    }
    return try {
        val selection = JSONObject(resolved)
        intArrayOf(
            scalarField(selection, "anchor") ?: return null,
            scalarField(selection, "head") ?: return null
        )
    } catch (error: Exception) {
        null
    }
}

private fun EditorV2Adapter.cachedCollaborationCells(): Pair<Int, Int>? =
    cachedAtomicRenderSelection()?.let(::cellSelectionEndpoints)

internal fun EditorV2Adapter.publishCollaborationCellsIfChanged() {
    if (roomBound && cachedCollaborationCells() != publishedCollaborationCells) {
        publishCachedCollaborationSelection()
    }
}

internal fun EditorV2Adapter.publishCachedCollaborationSelection() {
    if (!roomBound) return
    val cells = cachedCollaborationCells()
    if (cells != null) {
        publishAwarenessSelection(
            JSONObject().put("type", "cell").put("anchorCell", cells.first)
                .put("headCell", cells.second),
            cells
        )
        return
    }
    val selection = cachedAuthoritativeScalarSelection ?: return
    val mapping = resolveSelectionMapping(selection[0], selection[1]) ?: return
    publishCollaborationSelection(mapping[0], mapping[1])
}

internal fun EditorV2Adapter.publishCollaborationSelection(docAnchor: Int, docHead: Int) {
    publishAwarenessSelection(
        JSONObject().put("type", "text").put("anchor", docAnchor).put("head", docHead),
        null
    )
}

private fun EditorV2Adapter.publishAwarenessSelection(
    selection: JSONObject,
    cells: Pair<Int, Int>?
) {
    if (!roomBound) return
    val selectionJson = selection.toString()
    when (
        val result = backend.collaborationSetAwarenessSelection(
            editorId,
            selectionJson
        )
    ) {
        is EditorV2CallResult.Err -> if (result.error.code != AWARENESS_CELL_SELECTION_STALE) {
            emit(result.error)
        }

        is EditorV2CallResult.Ok -> {
            val outboundChanged = try {
                val value = JSONObject(result.value)
                if (value.length() != 1 ||
                    !value.has("outboundChanged") ||
                    value.opt("outboundChanged") !is Boolean
                ) {
                    null
                } else {
                    value.getBoolean("outboundChanged")
                }
            } catch (error: Exception) {
                null
            }
            if (outboundChanged == null) {
                emit(
                    EditorV2Adapter.contractError(
                        "awareness selection result violates the frozen shape"
                    )
                )
            } else {
                publishedCollaborationCells = cells
                if (outboundChanged) {
                    collaborationWake(editorId, CollaborationWakeReason.AWARENESS)
                }
            }
        }
    }
}

internal fun EditorV2Adapter.mapPosition(result: EditorV2CallResult<String>, key: String): Int? =
    when (result) {
        is EditorV2CallResult.Err -> {
            emit(result.error)
            null
        }

        is EditorV2CallResult.Ok -> try {
            scalarField(JSONObject(result.value), key)
        } catch (error: Exception) {
            null
        }
    }
