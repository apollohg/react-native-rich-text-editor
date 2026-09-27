package com.apollohg.editor.tables

import com.apollohg.editor.EditorV2Adapter
import com.apollohg.editor.EditorV2CallResult
import com.apollohg.editor.RichTextEditorView
import com.apollohg.editor.UniffiEditorV2Backend
import com.apollohg.editor.viewer.PreparedProseDrawingView
import org.json.JSONObject
import org.junit.Assert.assertTrue

private const val CELL_SELECTION_TYPE = "cell"
private const val DOCUMENT_POSITION_KIND = "document"

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
