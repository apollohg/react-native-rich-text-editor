package com.apollohg.editor

import org.json.JSONArray
import org.json.JSONObject

internal fun ulongField(jsonObject: JSONObject, key: String): ULong? =
    canonicalV2U64(jsonObject.opt(key) as? String)?.toULong()

internal fun scalarField(jsonObject: JSONObject, key: String): Int? =
    exactV2ScalarInt(jsonObject.opt(key) as? Number)

internal fun exactBool(value: Any?): Boolean? = value as? Boolean

private fun exactKeys(jsonObject: JSONObject, keys: Set<String>): Boolean {
    val actual = mutableSetOf<String>()
    val iterator = jsonObject.keys()
    while (iterator.hasNext()) actual += iterator.next()
    return actual == keys
}

private fun onlyKeys(jsonObject: JSONObject, keys: Set<String>): Boolean {
    val iterator = jsonObject.keys()
    while (iterator.hasNext()) if (iterator.next() !in keys) return false
    return true
}

private fun validJsonValue(value: Any?): Boolean = when (value) {
    null, JSONObject.NULL, is String, is Boolean -> true

    is Number -> value.toDouble().isFinite()

    is JSONArray -> (0 until value.length()).all { validJsonValue(value.opt(it)) }

    is JSONObject -> {
        val iterator = value.keys()
        var valid = true
        while (iterator.hasNext()) {
            if (!validJsonValue(value.opt(iterator.next()))) valid = false
        }
        valid
    }

    else -> false
}

private fun validRenderMark(value: Any?): Boolean = when (value) {
    is String -> true
    is JSONObject -> value.opt("type") is String && validJsonValue(value)
    else -> false
}

private fun validListContext(value: Any?): Boolean {
    val jsonObject = value as? JSONObject ?: return false
    if (!onlyKeys(
            jsonObject,
            setOf("ordered", "index", "total", "start", "isFirst", "isLast", "kind", "checked")
        )
    ) {
        return false
    }
    if (exactBool(jsonObject.opt("ordered")) == null || scalarField(jsonObject, "index") == null ||
        scalarField(jsonObject, "total") == null || scalarField(jsonObject, "start") == null ||
        exactBool(jsonObject.opt("isFirst")) == null || exactBool(jsonObject.opt("isLast")) == null
    ) {
        return false
    }
    val kind = jsonObject.opt("kind")
    if (kind != null && kind !== JSONObject.NULL && kind !is String) return false
    val checked = jsonObject.opt("checked")
    return checked == null || checked === JSONObject.NULL || exactBool(checked) != null
}

private fun validMentionThemeSection(
    value: Any?,
    stringKeys: Set<String>,
    extraKeys: Set<String>
): Boolean {
    val jsonObject = value as? JSONObject ?: return false
    val numberKeys = setOf("borderWidth", "borderRadius")
    if (!onlyKeys(jsonObject, stringKeys + numberKeys + extraKeys)) return false
    if (stringKeys.any { jsonObject.has(it) && jsonObject.opt(it) !is String }) return false
    if (numberKeys.any {
            jsonObject.has(it) &&
                (
                    jsonObject.opt(it) !is Number ||
                        !(jsonObject.opt(it) as Number).toDouble().isFinite()
                    )
        }
    ) {
        return false
    }
    val weight = jsonObject.opt("fontWeight")
    return weight == null ||
        weight in
        setOf("normal", "bold", "100", "200", "300", "400", "500", "600", "700", "800", "900")
}

private fun validMentionTheme(value: Any?): Boolean {
    val jsonObject = value as? JSONObject ?: return false
    if (!onlyKeys(jsonObject, setOf("node", "suggestions"))) return false

    if (jsonObject.has("node") && !validMentionThemeSection(
            jsonObject.opt("node"),
            setOf("textColor", "backgroundColor", "borderColor"),
            setOf("fontWeight", "style")
        )
    ) {
        return false
    }
    val node = jsonObject.optJSONObject("node")
    if (node?.has("style") == true && !validMentionNodeStyle(node.opt("style"))) return false
    if (!jsonObject.has("suggestions")) return true
    val suggestions = jsonObject.opt("suggestions")
    if (!validMentionThemeSection(
            suggestions,
            setOf("backgroundColor", "borderColor", "shadowColor"),
            setOf("option")
        )
    ) {
        return false
    }
    val option = (suggestions as? JSONObject)?.opt("option") ?: return true
    return validMentionThemeSection(
        option,
        setOf(
            "textColor",
            "secondaryTextColor",
            "backgroundColor",
            "borderColor",
            "highlightedBackgroundColor",
            "highlightedTextColor"
        ),
        setOf("fontWeight")
    )
}

private fun validMentionNodeStyle(value: Any?): Boolean {
    val style = value as? JSONObject ?: return false
    val colors =
        setOf("color", "backgroundColor", "textDecorationColor") +
            listOf("Top", "Right", "Bottom", "Left").map { "border${it}Color" }
    val nonnegative =
        listOf("Top", "Right", "Bottom", "Left").map { "border${it}Width" } +
            listOf("Top", "Right", "Bottom", "Left").map { "padding$it" } +
            listOf("TopLeft", "TopRight", "BottomLeft", "BottomRight").map { "border${it}Radius" }
    val enums = mapOf(
        "fontWeight" to
            setOf("normal", "bold", "100", "200", "300", "400", "500", "600", "700", "800", "900"),
        "fontStyle" to setOf("normal", "italic"),
        "borderStyle" to setOf("solid", "dashed", "dotted"),
        "textDecorationLine" to setOf(
            "none",
            "underline",
            "line-through",
            "underline line-through"
        ),
        "textDecorationStyle" to setOf("solid", "double", "dashed", "dotted")
    )
    return style.keys().asSequence().all { key ->
        val field = style.opt(key)
        when {
            key in colors -> field is String && field.matches(Regex("#[0-9a-fA-F]{8}"))

            key in nonnegative -> field is Number && field.toDouble().isFinite() &&
                field.toDouble() >= 0

            key == "fontSize" || key == "lineHeight" ->
                field is Number &&
                    field.toDouble().isFinite() &&
                    field.toDouble() > 0

            key == "letterSpacing" -> field is Number && field.toDouble().isFinite()

            key == "fontFamily" -> field is String && field.isNotBlank()

            key in enums -> field in enums.getValue(key)

            else -> false
        }
    }
}

private fun validRenderElement(value: Any?): Boolean {
    val jsonObject = value as? JSONObject ?: return false
    return when (jsonObject.opt("type") as? String) {
        "textRun" -> exactKeys(jsonObject, setOf("type", "text", "marks")) &&
            jsonObject.opt("text") is String &&
            (jsonObject.opt("marks") as? JSONArray)?.let { marks ->
                (0 until marks.length()).all { validRenderMark(marks.opt(it)) }
            } ==
            true

        "blockStart" -> onlyKeys(
            jsonObject,
            setOf("type", "nodeType", "depth", "listContext", "language")
        ) &&
            jsonObject.opt("nodeType") is String &&
            scalarField(jsonObject, "depth") != null &&
            (!jsonObject.has("listContext") || validListContext(jsonObject.opt("listContext"))) &&
            (
                !jsonObject.has("language") || jsonObject.isNull("language") ||
                    jsonObject.opt("language") is String
                )

        "blockEnd" -> exactKeys(jsonObject, setOf("type"))

        "voidInline" -> onlyKeys(jsonObject, setOf("type", "nodeType", "docPos", "attrs")) &&
            jsonObject.opt("nodeType") is String &&
            scalarField(jsonObject, "docPos") != null &&
            (!jsonObject.has("attrs") || jsonObject.opt("attrs") is JSONObject)

        "voidBlock" -> onlyKeys(
            jsonObject,
            setOf("type", "nodeType", "docPos", "attrs", "atomId")
        ) &&
            jsonObject.opt("nodeType") is String &&
            scalarField(jsonObject, "docPos") != null &&
            (!jsonObject.has("attrs") || jsonObject.opt("attrs") is JSONObject) &&
            (!jsonObject.has("atomId") || jsonObject.opt("atomId") is String)

        "opaqueInlineAtom" -> onlyKeys(
            jsonObject,
            setOf("type", "nodeType", "label", "docPos", "attrs", "mentionTheme")
        ) &&
            jsonObject.opt("nodeType") is String && jsonObject.opt("label") is String &&
            scalarField(jsonObject, "docPos") != null &&
            (!jsonObject.has("attrs") || jsonObject.opt("attrs") is JSONObject) &&
            (!jsonObject.has("mentionTheme") || validMentionTheme(jsonObject.opt("mentionTheme")))

        "opaqueBlockAtom" -> onlyKeys(
            jsonObject,
            setOf("type", "nodeType", "label", "docPos", "attrs")
        ) &&
            jsonObject.opt("nodeType") is String && jsonObject.opt("label") is String &&
            scalarField(jsonObject, "docPos") != null &&
            (!jsonObject.has("attrs") || jsonObject.opt("attrs") is JSONObject)

        else -> false
    }
}

internal fun parseTableAttributes(value: Any?): Map<String, JSONObject>? {
    val raw = if (value == null) JSONObject() else value as? JSONObject ?: return null
    val pool = mutableMapOf<String, JSONObject>()
    val unique = mutableSetOf<String>()
    var bytes = 0L
    var entries = 0
    for (key in raw.keys()) {
        val json = raw.opt(key) as? String ?: return null
        entries++
        bytes += json.toByteArray(Charsets.UTF_8).size
        if (!Regex("^[0-9a-f]{64}$").matches(key) || entries > 7_000_000 || !unique.add(json) || bytes > 192L * 1024 * 1024) return null
        val root = try { JSONObject(json) } catch (_: Exception) { return null }
        val pending = java.util.ArrayDeque<Pair<Any, Int>>()
        pending.add(root to 0)
        var work = 0
        while (pending.isNotEmpty()) {
            val (item, depth) = pending.removeLast()
            if (++work > json.length || depth > 1024) return null
            when (item) {
                is Number -> if (!item.toDouble().isFinite()) return null
                is JSONObject -> item.keys().forEach { pending.add(item.get(it) to depth + 1) }
                is JSONArray -> for (index in 0 until item.length()) pending.add(item.get(index) to depth + 1)
            }
        }
        pool[key] = root
    }
    return pool.toMap()
}

internal fun validSemanticRenderElements(elements: List<Any?>, tableAttributes: Map<String, JSONObject> = emptyMap(), tableRecords: Map<String, JSONObject> = emptyMap(), requireCompletePool: Boolean = true): Boolean {
    data class Pending(val value: Any?, val depth: Int, val start: Long, val end: Long)
    val pending = java.util.ArrayDeque<Pending>()
    elements.forEach { pending.add(Pending(it, 0, 0, 0xffff_ffffL)) }
    var nodes = 0L
    var gridSlots = 0L
    val referenced = mutableSetOf<String>()
    val referencedAttributes = mutableSetOf<String>()
    fun number(value: JSONObject, key: String): Long? {
        val raw = value.opt(key) as? Number ?: return null
        val double = raw.toDouble()
        return if (double.isFinite() && double >= 0 && double <= 0xffff_ffffL && double == double.toLong().toDouble()) double.toLong() else null
    }
    fun attrs(value: Any?): Boolean {
        if (value !is String || !tableAttributes.containsKey(value)) return false
        referencedAttributes.add(value)
        return true
    }
    val failureCodes = setOf("gridLimit", "workLimit", "allocation", "invalidStructure", "invalidAttributes")
    val diagnostics = setOf("virtual-grid-limit", "empty-reference-surface", "unsupported-row-role", "unsupported-cell-role", "ambiguous-source-map", "unsupported-gap-default", "overlapping-reference-cells", "unmapped-reference-cell", "nonrectangular-reference-cell", "zero-span-after-reference-pass")
    while (pending.isNotEmpty()) {
        val entry = pending.removeLast()
        if (++nodes + pending.size > 7_000_000 || entry.depth > 1024) return false
        val element = entry.value as? JSONObject ?: return false
        if (element.opt("type") != "table") {
            if (!validRenderElement(element)) return false
            if (element.has("docPos")) {
                val pos = number(element, "docPos") ?: return false
                if (pos < entry.start || pos >= entry.end) return false
            }
            continue
        }
        if (!exactKeys(element, setOf("type", "tableId"))) return false
        val tableId = element.opt("tableId") as? String ?: return false
        if (!Regex("^t(?:0|[1-9][0-9]*)$").matches(tableId) || !referenced.add(tableId)) return false
        val table = tableRecords[tableId] ?: return false
        if (!exactKeys(table, setOf("tablePos", "sourceEnd", "rows", "columns", "columnWidths", "direction", "irregular", "readOnlyDescendants", "attrsKey", "sourceRows", "cells", "syntheticRegions", "failure", "compatibilityDiagnostic"))) return false
        val pos = number(table, "tablePos") ?: return false
        if (tableId != "t$pos") return false
        val end = number(table, "sourceEnd") ?: return false
        val rows = number(table, "rows") ?: return false
        val columns = number(table, "columns") ?: return false
        val widths = table.optJSONArray("columnWidths") ?: return false
        val sourceRows = table.optJSONArray("sourceRows") ?: return false
        val cells = table.optJSONArray("cells") ?: return false
        val synthetic = table.optJSONArray("syntheticRegions") ?: return false
        val failure = table.opt("failure")
        val diagnostic = table.opt("compatibilityDiagnostic")
        if (pos < entry.start || end > entry.end || end <= pos || widths.length().toLong() != columns ||
            table.opt("direction") !in setOf(JSONObject.NULL, "ltr", "rtl") || exactBool(table.opt("irregular")) == null ||
            exactBool(table.opt("readOnlyDescendants")) != (entry.depth > 0) || !attrs(table.opt("attrsKey")) ||
            (failure !== JSONObject.NULL && failure !in failureCodes) || (diagnostic !== JSONObject.NULL && diagnostic !in diagnostics)) return false
        if (rows > 4_000_000 || columns > 4_000_000) return false
        gridSlots += rows * columns
        if (gridSlots > 4_000_000) return false
        for (index in 0 until widths.length()) {
            val width = widths.opt(index)
            if (width !== JSONObject.NULL && (width !is Number || !width.toDouble().isFinite() || width.toDouble() <= 0 || width.toDouble() > 0xffff_ffffL || width.toDouble() != width.toLong().toDouble())) return false
        }
        if (failure !== JSONObject.NULL) {
            if (rows != 0L || columns != 0L || cells.length() != 0 || sourceRows.length() != 0 || synthetic.length() != 0 || diagnostic !== JSONObject.NULL) return false
            continue
        }
        nodes += sourceRows.length() + cells.length() + synthetic.length()
        if (nodes > 7_000_000) return false
        var rowEnd = pos + 1
        for (index in 0 until sourceRows.length()) {
            val row = sourceRows.optJSONObject(index) ?: return false
            val start = number(row, "sourcePos") ?: return false
            val finish = number(row, "sourceEnd") ?: return false
            if (!exactKeys(row, setOf("sourcePos", "sourceEnd", "attrsKey")) || start < rowEnd || finish <= start || finish >= end || !attrs(row.opt("attrsKey"))) return false
            rowEnd = finish
        }
        val occupied = HashSet<Long>()
        var cellEnd = pos + 1
        var sourceRowIndex = 0
        for ((isSynthetic, regions) in listOf(false to cells, true to synthetic)) {
            for (index in 0 until regions.length()) {
                val region = regions.optJSONObject(index) ?: return false
                val keys = setOf("row", "column", "rowspan", "colspan", "header", "attrsKey") + if (isSynthetic) emptySet() else setOf("sourcePos", "sourceEnd", "contentKey", "elements")
                val row = number(region, "row") ?: return false
                val column = number(region, "column") ?: return false
                val rowspan = number(region, "rowspan") ?: return false
                val colspan = number(region, "colspan") ?: return false
                if (!exactKeys(region, keys) || rowspan == 0L || colspan == 0L || row + rowspan > rows || column + colspan > columns || exactBool(region.opt("header")) == null || !attrs(region.opt("attrsKey"))) return false
                for (r in row until row + rowspan) for (c in column until column + colspan) if (!occupied.add(r * columns + c)) return false
                if (isSynthetic) continue
                val start = number(region, "sourcePos") ?: return false
                val finish = number(region, "sourceEnd") ?: return false
                val key = region.opt("contentKey") as? String ?: return false
                val children = region.optJSONArray("elements") ?: return false
                if (start < cellEnd || finish <= start || key.isEmpty()) return false
                while (sourceRowIndex < sourceRows.length() && number(sourceRows.getJSONObject(sourceRowIndex), "sourceEnd")!! <= start) sourceRowIndex++
                val sourceRow = sourceRows.optJSONObject(sourceRowIndex) ?: return false
                if (number(sourceRow, "sourcePos")!! >= start || number(sourceRow, "sourceEnd")!! <= finish) return false
                cellEnd = finish
                for (child in 0 until children.length()) pending.add(Pending(children.opt(child), entry.depth + 1, start + 1, finish - 1))
            }
        }
    }
    return !requireCompletePool || (referenced == tableRecords.keys && referencedAttributes == tableAttributes.keys)
}

private fun validRenderBlocks(value: Any?, tableAttributes: Map<String, JSONObject> = emptyMap(), tableRecords: Map<String, JSONObject> = emptyMap(), requireCompletePool: Boolean = true): Boolean {
    val blocks = value as? JSONArray ?: return false
    val elements = mutableListOf<Any?>()
    for (blockIndex in 0 until blocks.length()) {
        val block = blocks.opt(blockIndex) as? JSONArray ?: return false
        for (index in 0 until block.length()) elements.add(block.opt(index))
    }
    return validSemanticRenderElements(elements, tableAttributes, tableRecords, requireCompletePool)
}

private fun validRenderPatch(value: Any?, tableAttributes: Map<String, JSONObject> = emptyMap(), tableRecords: Map<String, JSONObject> = emptyMap()): Boolean {
    if (value === JSONObject.NULL) return true
    val patch = value as? JSONObject ?: return false
    return exactKeys(
        patch,
        setOf("baseDocumentVersion", "startIndex", "deleteCount", "renderBlocks")
    ) &&
        canonicalV2U64(patch.opt("baseDocumentVersion") as? String) != null &&
        scalarField(patch, "startIndex") != null && scalarField(patch, "deleteCount") != null &&
        validRenderBlocks(patch.opt("renderBlocks"), tableAttributes, tableRecords, false)
}

internal fun parseTableRecords(value: Any?): Map<String, JSONObject>? {
    val raw = if (value == null) JSONObject() else value as? JSONObject ?: return null
    val records = mutableMapOf<String, JSONObject>()
    raw.keys().forEach { id ->
        if (id.isEmpty()) return null
        records[id] = raw.optJSONObject(id) ?: return null
    }
    return records.toMap()
}

private fun parseTableInputExtent(value: Any?, scalarLength: Int): TableInputExtent? {
    val extent = value as? JSONObject ?: return null
    val start = scalarField(extent, "scalarStart") ?: return null
    val end = scalarField(extent, "scalarEnd") ?: return null
    if (!exactKeys(extent, setOf("scalarStart", "scalarEnd")) || start > end || end > scalarLength) return null
    return TableInputExtent(start, end)
}

private fun validCompleteTablePool(
    tableAttributes: Map<String, JSONObject>,
    tableRecords: Map<String, JSONObject>
): Boolean {
    val roots = tableRecords.mapNotNull { (id, record) ->
        if (exactBool(record.opt("readOnlyDescendants")) == false) JSONObject().put("type", "table").put("tableId", id) else null
    }
    return roots.isNotEmpty() && validSemanticRenderElements(roots, tableAttributes, tableRecords)
}

internal fun parseTableInputMappings(
    value: Any?,
    tableAttributes: Map<String, JSONObject>,
    tableRecords: Map<String, JSONObject>,
    scalarLength: Int
): TableInputMappings? {
    val root = value as? JSONObject ?: return null
    if (!validCompleteTablePool(tableAttributes, tableRecords) ||
        !exactKeys(root, setOf("version", "tables")) || scalarField(root, "version") != 1
    ) return null
    val rawTables = root.optJSONObject("tables") ?: return null
    if (rawTables.keys().asSequence().toSet() != tableRecords.keys) return null
    val tables = mutableMapOf<String, TableInputTable>()
    for ((tableId, record) in tableRecords) {
        val rawTable = rawTables.optJSONObject(tableId) ?: return null
        val rawCells = rawTable.optJSONArray("cells") ?: return null
        val recordCells = record.optJSONArray("cells") ?: return null
        if (!exactKeys(rawTable, setOf("extent", "cells")) || rawCells.length() != recordCells.length()) return null
        val extent = if (rawTable.isNull("extent")) null else parseTableInputExtent(rawTable.opt("extent"), scalarLength) ?: return null
        val cells = mutableListOf<TableInputCell>()
        var observedTableExtent: TableInputExtent? = null
        for (index in 0 until rawCells.length()) {
            val rawCell = rawCells.optJSONObject(index) ?: return null
            val recordCell = recordCells.optJSONObject(index) ?: return null
            val cellIndex = scalarField(rawCell, "cellIndex") ?: return null
            val sourcePos = scalarField(rawCell, "sourcePos") ?: return null
            val sourceEnd = scalarField(rawCell, "sourceEnd") ?: return null
            val rawBlocks = rawCell.optJSONArray("blocks") ?: return null
            val rawExcluded = rawCell.optJSONArray("excluded") ?: return null
            val elements = recordCell.optJSONArray("elements") ?: return null
            if (!exactKeys(rawCell, setOf("cellIndex", "sourcePos", "sourceEnd", "blocks", "excluded")) ||
                cellIndex != index || sourcePos != scalarField(recordCell, "sourcePos") ||
                sourceEnd != scalarField(recordCell, "sourceEnd") || sourcePos >= sourceEnd
            ) return null
            val expectedExcluded = (0 until elements.length()).mapNotNull { elementIndex ->
                val element = elements.optJSONObject(elementIndex)
                if (element?.opt("type") == "table") (element.opt("tableId") as? String)?.let { elementIndex to it } else null
            }
            if (rawExcluded.length() != expectedExcluded.size) return null
            val blocks = mutableListOf<TableInputBlock>()
            for (blockIndex in 0 until rawBlocks.length()) {
                val rawBlock = rawBlocks.optJSONObject(blockIndex) ?: return null
                val elementIndex = scalarField(rawBlock, "elementIndex") ?: return null
                val docStart = scalarField(rawBlock, "docStart") ?: return null
                val docEnd = scalarField(rawBlock, "docEnd") ?: return null
                val scalarStart = scalarField(rawBlock, "scalarStart") ?: return null
                val contentScalarStart = scalarField(rawBlock, "contentScalarStart") ?: return null
                val scalarEnd = scalarField(rawBlock, "scalarEnd") ?: return null
                val breakScalarEnd = scalarField(rawBlock, "breakScalarEnd") ?: return null
                val isVoid = exactBool(rawBlock.opt("void")) ?: return null
                if (!exactKeys(rawBlock, setOf("elementIndex", "docStart", "docEnd", "scalarStart", "contentScalarStart", "scalarEnd", "breakScalarEnd", "void")) ||
                    elementIndex >= elements.length() ||
                    docStart <= sourcePos || docStart > docEnd || docEnd >= sourceEnd ||
                    scalarStart > contentScalarStart || contentScalarStart > scalarEnd || scalarEnd > breakScalarEnd ||
                    breakScalarEnd > scalarLength || breakScalarEnd - scalarEnd > 1 ||
                    (blockIndex > 0 && blocks.last().elementIndex >= elementIndex)
                ) return null
                val element = elements.optJSONObject(elementIndex) ?: return null
                if (isVoid) {
                    if (element.opt("type") !in setOf("voidBlock", "opaqueBlockAtom") || docStart != docEnd || scalarField(element, "docPos") != docStart) return null
                } else if (element.opt("type") != "blockStart") return null
                blocks.add(TableInputBlock(elementIndex, docStart, docEnd, scalarStart, contentScalarStart, scalarEnd, breakScalarEnd, isVoid))
            }
            val excluded = mutableListOf<TableInputExcluded>()
            for (excludedIndex in 0 until rawExcluded.length()) {
                val rawExcludedEntry = rawExcluded.optJSONObject(excludedIndex) ?: return null
                val elementIndex = scalarField(rawExcludedEntry, "elementIndex") ?: return null
                val nestedId = rawExcludedEntry.opt("tableId") as? String ?: return null
                val nestedRecord = tableRecords[nestedId] ?: return null
                val nestedStart = scalarField(nestedRecord, "tablePos") ?: return null
                val nestedEnd = scalarField(nestedRecord, "sourceEnd") ?: return null
                if (!exactKeys(rawExcludedEntry, setOf("elementIndex", "tableId", "extent")) ||
                    elementIndex != expectedExcluded[excludedIndex].first || nestedId != expectedExcluded[excludedIndex].second ||
                    nestedStart <= sourcePos || nestedStart >= nestedEnd || nestedEnd >= sourceEnd
                ) return null
                val nestedExtent = if (rawExcludedEntry.isNull("extent")) null else parseTableInputExtent(rawExcludedEntry.opt("extent"), scalarLength) ?: return null
                excluded.add(TableInputExcluded(elementIndex, nestedId, nestedExtent))
            }
            data class Ordered(val index: Int, val docStart: Int, val docEnd: Int, val scalarStart: Int?, val scalarEnd: Int?, val breakEnd: Int?)
            val ordered = blocks.map { Ordered(it.elementIndex, it.docStart, it.docEnd, it.scalarStart, it.scalarEnd, it.breakScalarEnd) } +
                excluded.map { item ->
                    val nested = tableRecords.getValue(item.tableId)
                    Ordered(item.elementIndex, scalarField(nested, "tablePos")!!, scalarField(nested, "sourceEnd")!!, item.extent?.scalarStart, item.extent?.scalarEnd, item.extent?.scalarEnd)
                }
            val sourceOrdered = ordered.sortedBy { it.index }
            for ((previous, next) in sourceOrdered.zipWithNext()) {
                if (previous.index >= next.index || previous.docEnd > next.docStart ||
                    (previous.breakEnd != null && next.scalarStart != null && previous.breakEnd > next.scalarStart)
                ) return null
            }
            var cellExtent: TableInputExtent? = null
            var cellBreakEnd: Int? = null
            for (part in sourceOrdered) {
                val start = part.scalarStart ?: continue
                val end = part.scalarEnd ?: continue
                if (cellBreakEnd != null && cellBreakEnd > start) return null
                cellExtent = TableInputExtent(cellExtent?.scalarStart ?: start, end)
                cellBreakEnd = part.breakEnd
            }
            if (cellExtent != null) {
                if ((cellBreakEnd ?: 0) > cellExtent.scalarEnd ||
                    (observedTableExtent != null && observedTableExtent.scalarEnd > cellExtent.scalarStart)
                ) return null
                observedTableExtent = TableInputExtent(observedTableExtent?.scalarStart ?: cellExtent.scalarStart, cellExtent.scalarEnd)
            }
            cells.add(TableInputCell(cellIndex, sourcePos, sourceEnd, blocks, excluded))
        }
        if (record.isNull("failure") && observedTableExtent != extent) return null
        tables[tableId] = TableInputTable(extent, cells)
    }
    if (tables.values.any { table -> table.cells.any { cell -> cell.excluded.any { it.extent != tables[it.tableId]?.extent } } }) return null
    val rootExtents = tableRecords.mapNotNull { (tableId, record) ->
        if (exactBool(record.opt("readOnlyDescendants")) == false) {
            val sourcePos = scalarField(record, "tablePos")
            val extent = tables[tableId]?.extent
            if (sourcePos != null && extent != null) sourcePos to extent else null
        } else null
    }.sortedBy { it.first }
    for ((previous, next) in rootExtents.zipWithNext()) if (previous.second.scalarEnd > next.second.scalarStart) return null
    return TableInputMappings(tables)
}

private fun validBooleanRecord(value: Any?): Boolean {
    val jsonObject = value as? JSONObject ?: return false
    val iterator = jsonObject.keys()
    while (iterator.hasNext()) if (exactBool(jsonObject.opt(iterator.next())) == null) return false
    return true
}

private fun validStringArray(value: Any?): Boolean {
    val array = value as? JSONArray ?: return false
    return (0 until array.length()).all { array.opt(it) is String }
}

private fun validActiveState(value: Any?): Boolean {
    val jsonObject = value as? JSONObject ?: return false
    if (!exactKeys(
            jsonObject,
            setOf("marks", "markAttrs", "nodes", "commands", "allowedMarks", "insertableNodes")
        )
    ) {
        return false
    }
    val attrs = jsonObject.opt("markAttrs") as? JSONObject ?: return false
    val attrsIterator = attrs.keys()
    while (attrsIterator.hasNext()) if (attrs.opt(attrsIterator.next()) !is JSONObject) return false
    return validBooleanRecord(jsonObject.opt("marks")) &&
        validBooleanRecord(jsonObject.opt("nodes")) &&
        validBooleanRecord(
            jsonObject.opt("commands")
        ) && validStringArray(jsonObject.opt("allowedMarks")) &&
        validStringArray(jsonObject.opt("insertableNodes"))
}

internal fun scalarSelection(value: Any?): IntArray? {
    val selection = value as? JSONObject ?: return null
    if (selection.opt("type") != "text" ||
        !exactKeys(selection, setOf("type", "anchor", "head", "anchorScalar", "headScalar"))
    ) {
        return null
    }
    if (scalarField(selection, "anchor") == null ||
        scalarField(selection, "head") == null
    ) {
        return null
    }
    return intArrayOf(
        scalarField(selection, "anchorScalar") ?: return null,
        scalarField(selection, "headScalar") ?: return null
    )
}

private fun validSelection(value: Any?): Boolean {
    val selection = value as? JSONObject ?: return false
    return when (selection.opt("type") as? String) {
        "text" -> scalarSelection(selection) != null

        "node" -> exactKeys(selection, setOf("type", "pos", "posScalar")) &&
            scalarField(selection, "pos") != null &&
            scalarField(selection, "posScalar") != null

        "all" -> exactKeys(selection, setOf("type"))

        else -> false
    }
}

internal data class AtomicRenderSnapshot(
    val renderObject: JSONObject,
    val tableAttributes: Map<String, JSONObject>,
    val tableRecords: Map<String, JSONObject>,
    val tableInputMappings: TableInputMappings?,
    /** Original validated wire payload for controlled-prop delivery. */
    val atomicRenderJson: String,
    val viewUpdateJson: String,
    val documentRevision: ULong,
    val stateRevision: ULong,
    val scalarLength: Int,
    val scalarSelection: IntArray?,
    val activeState: JSONObject,
    val historyState: JSONObject,
    val positionEpoch: String?
)

internal data class TableInputExtent(val scalarStart: Int, val scalarEnd: Int)
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

internal data class PinnedAtomicRenderSnapshot(
    val snapshot: AtomicRenderSnapshot,
    val positionEpoch: String?
)

internal fun parseAtomicRenderSnapshot(json: String): AtomicRenderSnapshot? {
    return try {
        val jsonObject = JSONObject(json)
        val requiredKeys =
            setOf(
                "renderBlocks",
                "renderPatch",
                "selection",
                "activeState",
                "historyState",
                "documentVersion",
                "stateRevision",
                "scalarLength",
                "documentIsEmpty"
            )
        val renderBlocks = jsonObject.opt("renderBlocks")
        val renderPatch = jsonObject.opt("renderPatch")
        val tableAttributes = parseTableAttributes(jsonObject.opt("tableAttributes")) ?: return null
        val tableRecords = parseTableRecords(jsonObject.opt("tableRecords")) ?: return null
        val validRenderPayload =
            (validRenderBlocks(renderBlocks, tableAttributes, tableRecords) && renderPatch === JSONObject.NULL) ||
                (
                    renderBlocks === JSONObject.NULL && renderPatch is JSONObject &&
                        validRenderPatch(renderPatch, tableAttributes, tableRecords)
                    )
        if (!onlyKeys(jsonObject, requiredKeys + setOf("positionEpoch", "tableAttributes", "tableRecords", "tableInputMappings")) ||
            requiredKeys.any { !jsonObject.has(it) } ||
            !validRenderPayload ||
            !validSelection(jsonObject.opt("selection")) ||
            !validActiveState(jsonObject.opt("activeState")) ||
            exactBool(jsonObject.opt("documentIsEmpty")) == null
        ) {
            return null
        }
        val history = jsonObject.opt("historyState") as? JSONObject ?: return null
        if (!exactKeys(history, setOf("canUndo", "canRedo")) ||
            exactBool(history.opt("canUndo")) == null ||
            exactBool(history.opt("canRedo")) == null
        ) {
            return null
        }
        val revision = ulongField(jsonObject, "documentVersion") ?: return null
        val state = ulongField(jsonObject, "stateRevision") ?: return null
        val scalarLength = scalarField(jsonObject, "scalarLength") ?: return null
        val tableInputMappings = if (jsonObject.has("tableInputMappings")) {
            parseTableInputMappings(jsonObject.opt("tableInputMappings"), tableAttributes, tableRecords, scalarLength) ?: return null
        } else {
            null
        }
        val scalarSelection = scalarSelection(jsonObject.opt("selection"))
        val positionEpoch = if (jsonObject.has("positionEpoch")) {
            canonicalV2U64(jsonObject.opt("positionEpoch") as? String) ?: return null
        } else {
            null
        }
        jsonObject.remove("positionEpoch")
        val atomicRenderJson = jsonObject.toString()
        jsonObject.remove("scalarLength")
        AtomicRenderSnapshot(
            jsonObject,
            tableAttributes,
            tableRecords,
            tableInputMappings,
            atomicRenderJson,
            jsonObject.toString(),
            revision,
            state,
            scalarLength,
            scalarSelection,
            JSONObject(jsonObject.getJSONObject("activeState").toString()),
            JSONObject(history.toString()),
            positionEpoch
        )
    } catch (_: Exception) {
        null
    }
}
