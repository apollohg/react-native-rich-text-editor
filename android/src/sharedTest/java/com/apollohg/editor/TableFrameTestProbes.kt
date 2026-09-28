package com.apollohg.editor

import org.json.JSONArray
import org.json.JSONObject
import com.apollohg.editor.tables.inputElements

internal typealias TableInputExtent = TableScalarExtent
internal data class TableInputBlock(
    val elementIndex: Int,
    val docStart: Int,
    val docEnd: Int,
    val scalarStart: Int,
    val contentScalarStart: Int,
    val scalarEnd: Int,
    val breakScalarEnd: Int,
    val isVoid: Boolean
)
internal data class TableInputExcluded(val elementIndex: Int, val tableId: String, val extent: TableInputExtent?)
internal data class TableInputCell(
    val cellIndex: Int,
    val sourcePos: Int,
    val sourceEnd: Int,
    val blocks: List<TableInputBlock>,
    val excluded: List<TableInputExcluded>
)
internal data class TableInputTable(val extent: TableInputExtent?, val cells: List<TableInputCell>)
internal data class TableInputMappings(val tables: Map<String, TableInputTable>)


internal val EditorV2Adapter.tableRecordsForTesting: Map<String, JSONObject>
    get() = tableIndex.tableKeys.associateWith { key ->
        val record = requireNotNull(tableIndex.record(key))
        val start = requireNotNull(tableIndex.tableDocStart(key)).toLong()
        val cells = JSONArray()
        record.cells.forEachIndexed { index, cell ->
            val opening = requireNotNull(tableIndex.docStart(key, index)).toLong()
            cells.put(JSONObject().put("sourcePos", opening).put("sourceEnd", opening + cell.docSize.toLong())
                .put("row", cell.row.toLong()).put("column", cell.column.toLong())
                .put("rowspan", cell.rowspan.toLong()).put("colspan", cell.colspan.toLong())
                .put("header", cell.header).put("attrsKey", cell.attrsKey).put("contentKey", cell.contentKey)
                .put("elements", inputElements(cell.elements, cell.voidElementIndices) { tableIndex.absoluteDocPos(key, index, it) }))
        }
        JSONObject().put("sourceId", key).put("tablePos", start).put("sourceEnd", start + record.docSize.toLong())
            .put("rows", record.rows.toLong()).put("columns", record.columns.toLong())
            .put("readOnlyDescendants", record.readOnlyDescendants).put("cells", cells)
            .put("columnWidths", JSONArray(record.columnWidths.map { it?.toLong() }))
            .put("direction", record.direction ?: JSONObject.NULL).put("irregular", record.irregular)
            .put("attrsKey", record.attrsKey).put("failure", record.failure?.name ?: JSONObject.NULL)
    }

internal val EditorV2Adapter.tableMappingsForTesting: TableInputMappings?
    get() = installedFrameRevision?.let {
        TableInputMappings(tableIndex.tableKeys.associateWith { key ->
            val record = requireNotNull(tableIndex.record(key))
            val extent = tableIndex.rootExtents[key]?.takeIf { it.scalarEnd > it.scalarStart }?.let { TableScalarExtent(it.scalarStart.toInt(), it.scalarEnd.toInt()) }
            TableInputTable(extent, record.cells.mapIndexed { index, cell ->
                val doc = requireNotNull(tableIndex.docStart(key, index)).toInt()
                val scalar = tableIndex.scalarStart(key, index)?.toInt() ?: 0
                TableInputCell(index, doc, doc + cell.docSize.toInt(), cell.inputBlocks.map { block ->
                    TableInputBlock(block.elementIndex.toInt(), doc + block.docStart.toInt(), doc + block.docEnd.toInt(),
                        scalar + block.scalarStart.toInt(), scalar + block.contentScalarStart.toInt(),
                        scalar + block.scalarEnd.toInt(), scalar + block.breakScalarEnd.toInt(), block.`void`)
                }, cell.nestedTables.map { nested ->
                    TableInputExcluded(nested.elementIndex.toInt(), nested.tableKey,
                        nested.scalarStart?.let { lower -> nested.scalarEnd?.let { upper ->
                            TableScalarExtent(scalar + lower.toInt(), scalar + upper.toInt())
                        } })
                })
            })
        })
    }

internal fun com.apollohg.editor.tables.EditorTableIndex.replacingRecordsForTesting(
    transform: (uniffi.editor_core.FfiTableRecord) -> uniffi.editor_core.FfiTableRecord
): com.apollohg.editor.tables.EditorTableIndex {
    val index = com.apollohg.editor.tables.EditorTableIndex()
    val records = tableKeys.map { transform(requireNotNull(record(it))) }
    val excludedParents = records.filter { it.failure != null }.map { it.tableKey }.toMutableSet()
    while (true) {
        val count = excludedParents.size
        records.filter { it.host?.tableKey in excludedParents }.forEach { excludedParents.add(it.tableKey) }
        if (excludedParents.size == count) break
    }
    val retained = records.filter { it.host?.tableKey !in excludedParents }.map {
        if (it.failure == null) it else it.copy(cells = emptyList(), sourceRows = emptyList(), syntheticRegions = emptyList())
    }
    val frame = uniffi.editor_core.FfiTableFrame(uniffi.editor_core.FfiTableFrameKind.FULL, null,
        attributeObjects.map { uniffi.editor_core.FfiTableAttribute(it.key, it.value.toString()) }, emptyList(),
        retained, emptyList(), emptyList(), rootExtents.values.toList())
    check(index.adopt(frame, null, 0uL) is com.apollohg.editor.tables.TableFrameAdoption.Adopted)
    return index
}

internal fun assertFramePositionsMatchEngine(adapter: EditorV2Adapter) {
    val revision = adapter.baseDocumentRevision
    org.junit.Assert.assertEquals(revision, adapter.installedFrameRevision)
    for (key in adapter.tableIndex.tableKeys) {
        val table = requireNotNull(adapter.tableIndex.record(key))
        table.cells.forEachIndexed { cellIndex, cell ->
            val doc = requireNotNull(adapter.tableIndex.docStart(key, cellIndex)).toLong()
            val scalar = requireNotNull(adapter.tableIndex.scalarStart(key, cellIndex)).toLong()
            cell.inputBlocks.forEach { block ->
                val context = "table $key cell $cellIndex block ${block.elementIndex} revision $revision"
                val documentStart = doc + block.docStart.toLong()
                val contentStart = scalar + block.contentScalarStart.toLong()
                val docResult = UniffiEditorV2Backend.scalarToDoc(adapter.editorId, contentStart.toInt()) as EditorV2CallResult.Ok
                org.junit.Assert.assertEquals(context, documentStart, JSONObject(docResult.value).getLong("doc"))
                val scalarResult = UniffiEditorV2Backend.docToScalar(adapter.editorId, documentStart.toInt()) as EditorV2CallResult.Ok
                val expected = if (block.docStart == block.docEnd && !block.`void`) scalar + block.scalarEnd.toLong() else contentStart
                org.junit.Assert.assertEquals(context, expected, JSONObject(scalarResult.value).getLong("scalar"))
            }
        }
    }
    org.junit.Assert.assertEquals(revision, adapter.baseDocumentRevision)
}
