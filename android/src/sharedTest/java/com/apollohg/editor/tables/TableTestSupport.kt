package com.apollohg.editor.tables

import com.apollohg.editor.EditorEditText
import com.apollohg.editor.EditorV2Adapter
import com.apollohg.editor.EditorV2CallResult
import com.apollohg.editor.EditorV2LeaseResult
import com.apollohg.editor.RichTextEditorView
import com.apollohg.editor.UniffiEditorV2Backend
import com.apollohg.editor.viewer.PreparedProseDrawingView
import org.json.JSONObject
import org.junit.Assert.assertTrue
import org.junit.Assert.fail

private const val CELL_SELECTION_TYPE = "cell"
private const val DOCUMENT_POSITION_KIND = "document"
private const val COLLABORATION_NOW_MILLIS = "0"
private const val MAXIMUM_RELAY_ROUNDS = 64

internal class RemoteTablePeer(private val adapter: EditorV2Adapter, requestIdBase: Long) {
    private var nextRequestId = requestIdBase

    fun apply(payload: JSONObject, call: (String, String) -> EditorV2CallResult<String>) {
        nextRequestId += 1
        val envelope = payload.put("version", 1).put("requestId", nextRequestId.toString())
            .put("baseDocumentRevision", adapter.baseDocumentRevision.toString()).toString()
        val result = call(adapter.editorId, envelope)
        assertTrue("the remote peer's change was refused: $result", result is EditorV2CallResult.Ok)
    }

    fun applyCommand(command: JSONObject) =
        apply(JSONObject().put("command", command)) { id, request -> UniffiEditorV2Backend.applyCommand(id, request) }

    fun applySelection(selection: JSONObject) =
        apply(selection) { id, request -> UniffiEditorV2Backend.setSelection(id, request) }
}

internal fun documentCellSelection(anchor: Int, head: Int): JSONObject {
    fun point(opening: Int) = JSONObject().put("kind", DOCUMENT_POSITION_KIND).put("offset", opening)
    return JSONObject().put("type", CELL_SELECTION_TYPE).put("anchorCell", point(anchor)).put("headCell", point(head))
}

internal fun EditorV2Adapter.tableCellPositions(tableId: String): List<Int> {
    val cells = requireNotNull(cachedTableRecords[tableId]) { "table $tableId is not rendered" }.getJSONArray("cells")
    return (0 until cells.length()).map { cells.getJSONObject(it).getInt("sourcePos") }
}

internal fun PreparedProseDrawingView.presentedRealCell(tableId: String, position: Int): ViewerTablePresentedCell =
    requireNotNull(presentedTableCells().firstOrNull {
        it.surface.editorTableId == tableId && it.sourcePosition == position && it.cell.sourceCellIndex != null
    }) { "cell $position is not presented" }

internal val RichTextEditorView.activeTableCellPosition: Long?
    get() = activeTextInput.tableCellPositionMap?.binding?.cellSourcePos

internal fun EditorV2Adapter.applyLocalSelection(selection: JSONObject): EditorV2CallResult<String> =
    callWithEnvelope(JSONObject().put("selection", selection)) { UniffiEditorV2Backend.setSelection(editorId, it) }

internal fun EditorEditText.selectTableCells(adapter: EditorV2Adapter, anchor: Int, head: Int) {
    val admitted = adapter.applyLocalSelection(documentCellSelection(anchor, head))
    assertTrue("engine rejected the selection: $admitted", admitted is EditorV2CallResult.Ok)
    assertTrue(applyUpdateJSON(requireNotNull(adapter.refreshFromRustState(null))))
}

private fun <T> EditorV2CallResult<T>.required(operation: String): T = when (this) {
    is EditorV2CallResult.Ok -> value
    is EditorV2CallResult.Err -> throw AssertionError("$operation failed: ${error.code}: ${error.message}")
}

internal class TableCollaborationRelay(editorIds: List<String>) {
    private val generations = editorIds.associateWith { editorId ->
        val driven = JSONObject(UniffiEditorV2Backend.collaborationDrive(editorId, COLLABORATION_NOW_MILLIS)
            .required("drive"))
        val generation = driven.getString("generationToOpen")
        UniffiEditorV2Backend.collaborationSocketOpen(editorId, generation, COLLABORATION_NOW_MILLIS)
            .required("socket open")
        generation
    }

    fun exchangeUntilIdle(): Set<String> {
        val committed = mutableSetOf<String>()
        repeat(MAXIMUM_RELAY_ROUNDS) {
            var delivered = false
            generations.forEach { (from, fromGeneration) ->
                val lease = when (val result = UniffiEditorV2Backend.collaborationLeaseOutbound(from, fromGeneration)) {
                    is EditorV2LeaseResult.Value -> result.lease
                    EditorV2LeaseResult.Empty -> return@forEach
                    is EditorV2LeaseResult.Err -> throw AssertionError("$from could not lease: ${result.error.code}")
                }
                generations.filterKeys { it != from }.forEach { (to, toGeneration) ->
                    val received = JSONObject(UniffiEditorV2Backend.collaborationReceive(
                        to, toGeneration, lease.frame, COLLABORATION_NOW_MILLIS
                    ).required("receive"))
                    if (received.optBoolean("remoteCommitApplied", false)) committed += to
                }
                UniffiEditorV2Backend.collaborationAckOutbound(from, fromGeneration, lease.leaseId).required("ack")
                delivered = true
            }
            if (!delivered) return committed
        }
        fail("the peers never went quiet")
        return committed
    }
}

internal class TableRoomSeed(localConfigJson: String, documentJson: String) {
    val configJson: String
    val encodedState: ByteArray

    init {
        val builderId = JSONObject(UniffiEditorV2Backend.create(localConfigJson, null).required("create"))
            .getString("editorId")
        val builder = requireNotNull(EditorV2Adapter.attach(UniffiEditorV2Backend, builderId, roomBound = false))
        try {
            requireNotNull(builder.setContentJson(documentJson))
            val (metadataJson, state) = UniffiEditorV2Backend.snapshotExport(builderId).required("snapshot export")
            val metadata = JSONObject(metadataJson)
            configJson = JSONObject(localConfigJson).put("initialization", JSONObject()
                .put("type", "room")
                .put("documentId", metadata.getString("documentId"))
                .put("lineageId", metadata.getString("lineageId"))
                .put("snapshot", metadata)).toString()
            encodedState = state
        } finally {
            builder.destroy()
        }
    }

    fun makeAdapter(): EditorV2Adapter {
        val editorId = JSONObject(UniffiEditorV2Backend.create(configJson, encodedState).required("room create"))
            .getString("editorId")
        return requireNotNull(EditorV2Adapter.attach(UniffiEditorV2Backend, editorId, roomBound = true))
    }
}
