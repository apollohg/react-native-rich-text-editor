package com.apollohg.editor.tables

import org.json.JSONArray
import org.json.JSONObject
import uniffi.editor_core.FfiViewerElement

internal data class EditorTablePresentationSnapshot(
    val documentRevision: ULong,
    val baseDocumentRevision: ULong?,
    val positionEpoch: String?,
    val index: EditorTableIndex,
    val changes: TableFrameChanges
)

internal fun inputElements(
    elements: List<FfiViewerElement>,
    voidElementIndices: List<UInt>,
    absoluteDocPos: (UInt) -> UInt?
): JSONArray? {
    val result = JSONArray()
    val voidIndices = voidElementIndices.toSet()
    for ((index, element) in elements.withIndex()) {
        val value = JSONObject()
        when (element) {
            is FfiViewerElement.Table -> value.put("type", "table").put("tableId", element.tableId)
            is FfiViewerElement.BlockStart -> {
                value.put("type", "blockStart").put("nodeType", element.nodeType).put("depth", element.depth.toInt())
                element.language?.let { value.put("language", it) }
                element.listContextJson?.let { value.put("listContext", JSONObject(it)) }
            }
            FfiViewerElement.BlockEnd -> value.put("type", "blockEnd")
            is FfiViewerElement.TextRun -> {
                val marks = JSONArray()
                element.marks.forEach { mark ->
                    val attrs = JSONObject(mark.attrsJson)
                    marks.put(if (attrs.length() == 0) mark.markType else attrs.put("type", mark.markType))
                }
                value.put("type", "textRun").put("text", element.text).put("marks", marks)
            }
            is FfiViewerElement.InlineAtom -> value.put("type", if (index.toUInt() in voidIndices) "voidInline" else "opaqueInlineAtom")
                .put("nodeType", element.nodeType).put("docPos", (absoluteDocPos(element.docPos) ?: return null).toLong())
                .put("attrs", JSONObject(element.attrsJson)).put("label", element.label)
            is FfiViewerElement.BlockAtom -> value.put("type", if (index.toUInt() in voidIndices) "voidBlock" else "opaqueBlockAtom")
                .put("nodeType", element.nodeType).put("docPos", (absoluteDocPos(element.docPos) ?: return null).toLong())
                .put("attrs", JSONObject(element.attrsJson)).put("label", element.label)
        }
        result.put(value)
    }
    return result
}
