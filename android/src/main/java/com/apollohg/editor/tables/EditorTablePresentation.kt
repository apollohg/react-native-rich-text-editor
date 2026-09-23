package com.apollohg.editor.tables

import com.apollohg.editor.exactV2U32
import org.json.JSONArray
import org.json.JSONObject
import uniffi.editor_core.FfiViewerElement
import uniffi.editor_core.FfiViewerMark
import uniffi.editor_core.FfiViewerTable
import uniffi.editor_core.FfiViewerTableCell
import uniffi.editor_core.TableCompatibilityDiagnostic
import uniffi.editor_core.TableRenderFailure
import uniffi.editor_core.TableRenderRow
import uniffi.editor_core.TableRenderSyntheticRegion

internal fun lowerEditorTableRecords(records: Map<String, JSONObject>): Map<String, FfiViewerTable>? {
    fun JSONObject.uint(name: String): UInt? = exactV2U32(opt(name) as? Number)?.toUInt()
    fun JSONObject.string(name: String): String? = opt(name) as? String
    fun JSONObject.bool(name: String): Boolean? = opt(name) as? Boolean
    fun JSONArray.objects(): List<JSONObject>? = (0 until length()).map { optJSONObject(it) ?: return null }
    fun mark(value: Any?): FfiViewerMark? = when (value) {
        is String -> FfiViewerMark(value, "{}")
        is JSONObject -> {
            val kind = value.string("type") ?: return null
            FfiViewerMark(kind, JSONObject(value.toString()).apply { remove("type") }.toString())
        }
        else -> null
    }
    fun elements(array: JSONArray): List<FfiViewerElement>? = (0 until array.length()).map { index ->
        val element = array.optJSONObject(index) ?: return null
        when (element.string("type")) {
            "table" -> FfiViewerElement.Table(element.string("tableId") ?: return null)
            "textRun" -> {
                val marks = element.optJSONArray("marks") ?: return null
                FfiViewerElement.TextRun(element.string("text") ?: return null,
                    (0 until marks.length()).map { mark(marks.opt(it)) ?: return null })
            }
            "voidInline", "opaqueInlineAtom", "voidBlock", "opaqueBlockAtom" -> {
                val kind = element.string("type")!!
                val node = element.string("nodeType") ?: return null
                val position = element.uint("docPos") ?: return null
                val attrs = element.optJSONObject("attrs") ?: JSONObject()
                val label = if (kind.startsWith("opaque")) element.string("label") ?: return null else {
                    val base = attrs.string("label")?.takeIf { it.isNotEmpty() } ?: node
                    val trigger = if (node == "mention") attrs.string("mentionSuggestionChar") else null
                    if (!trigger.isNullOrEmpty() && !base.startsWith(trigger)) trigger + base else base
                }
                if (kind == "voidInline" || kind == "opaqueInlineAtom")
                    FfiViewerElement.InlineAtom(node, position, attrs.toString(), label)
                else FfiViewerElement.BlockAtom(node, position, attrs.toString(), label)
            }
            "blockStart" -> {
                val depth = element.uint("depth")?.takeIf { it <= UShort.MAX_VALUE.toUInt() } ?: return null
                val context = element.optJSONObject("listContext")?.let {
                    JSONObject(it.toString()).apply {
                        if (!has("kind")) put("kind", JSONObject.NULL)
                        if (!has("checked")) put("checked", JSONObject.NULL)
                    }.toString()
                }
                FfiViewerElement.BlockStart(element.string("nodeType") ?: return null,
                    element.string("language"), depth.toUShort(), context)
            }
            "blockEnd" -> FfiViewerElement.BlockEnd
            else -> return null
        }
    }
    fun failure(value: String?): TableRenderFailure? { return when (value) {
        null -> null
        "gridLimit" -> TableRenderFailure.GRID_LIMIT
        "workLimit" -> TableRenderFailure.WORK_LIMIT
        "allocation" -> TableRenderFailure.ALLOCATION
        "invalidStructure" -> TableRenderFailure.INVALID_STRUCTURE
        "invalidAttributes" -> TableRenderFailure.INVALID_ATTRIBUTES
        else -> return null
    } }
    fun diagnostic(value: String?): TableCompatibilityDiagnostic? { return when (value) {
        null -> null
        "virtual-grid-limit" -> TableCompatibilityDiagnostic.VIRTUAL_GRID_LIMIT
        "empty-reference-surface" -> TableCompatibilityDiagnostic.EMPTY_REFERENCE_SURFACE
        "unsupported-row-role" -> TableCompatibilityDiagnostic.UNSUPPORTED_ROW_ROLE
        "unsupported-cell-role" -> TableCompatibilityDiagnostic.UNSUPPORTED_CELL_ROLE
        "ambiguous-source-map" -> TableCompatibilityDiagnostic.AMBIGUOUS_SOURCE_MAP
        "unsupported-gap-default" -> TableCompatibilityDiagnostic.UNSUPPORTED_GAP_DEFAULT
        "overlapping-reference-cells" -> TableCompatibilityDiagnostic.OVERLAPPING_REFERENCE_CELLS
        "unmapped-reference-cell" -> TableCompatibilityDiagnostic.UNMAPPED_REFERENCE_CELL
        "nonrectangular-reference-cell" -> TableCompatibilityDiagnostic.NONRECTANGULAR_REFERENCE_CELL
        "zero-span-after-reference-pass" -> TableCompatibilityDiagnostic.ZERO_SPAN_AFTER_REFERENCE_PASS
        else -> return null
    } }
    return records.mapValues { (id, record) ->
        val widths = record.optJSONArray("columnWidths") ?: return null
        val rows = record.optJSONArray("sourceRows")?.objects() ?: return null
        val cells = record.optJSONArray("cells")?.objects() ?: return null
        val synthetic = record.optJSONArray("syntheticRegions")?.objects() ?: return null
        val table = FfiViewerTable(
            tablePos = record.uint("tablePos") ?: return null,
            sourceEnd = record.uint("sourceEnd") ?: return null,
            rows = record.uint("rows") ?: return null,
            columns = record.uint("columns") ?: return null,
            columnWidths = (0 until widths.length()).map { index ->
                if (widths.isNull(index)) null else recordValueUInt(widths.opt(index)) ?: return null
            },
            direction = record.string("direction"),
            irregular = record.bool("irregular") ?: return null,
            readOnlyDescendants = record.bool("readOnlyDescendants") ?: return null,
            attrsKey = record.string("attrsKey") ?: return null,
            sourceRows = rows.map { row -> TableRenderRow(row.uint("sourcePos") ?: return null,
                row.uint("sourceEnd") ?: return null, row.string("attrsKey") ?: return null) },
            cells = cells.map { cell -> FfiViewerTableCell(
                cell.uint("sourcePos") ?: return null, cell.uint("sourceEnd") ?: return null,
                cell.uint("row") ?: return null, cell.uint("column") ?: return null,
                cell.uint("rowspan") ?: return null, cell.uint("colspan") ?: return null,
                cell.bool("header") ?: return null, cell.string("attrsKey") ?: return null,
                cell.string("contentKey") ?: return null,
                elements(cell.optJSONArray("elements") ?: return null) ?: return null
            ) },
            syntheticRegions = synthetic.map { region -> TableRenderSyntheticRegion(
                region.uint("row") ?: return null, region.uint("column") ?: return null,
                region.uint("rowspan") ?: return null, region.uint("colspan") ?: return null,
                region.bool("header") ?: return null, region.string("attrsKey") ?: return null
            ) },
            failure = failure(record.string("failure")),
            compatibilityDiagnostic = diagnostic(record.string("compatibilityDiagnostic"))
        )
        if (id != "t${table.tablePos}") return null
        table
    }
}

private fun recordValueUInt(value: Any?): UInt? = exactV2U32(value as? Number)?.toUInt()
