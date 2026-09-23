package com.apollohg.editor.tables

import com.apollohg.editor.exactV2U32
import org.json.JSONObject

internal sealed interface EditorCellSelection {
    val tableId: String

    data class Drawable(override val tableId: String, val sourcePositions: Set<Int>) : EditorCellSelection
    data class Unavailable(override val tableId: String) : EditorCellSelection
}

internal fun resolveEditorCellSelection(
    selection: JSONObject,
    records: Map<String, JSONObject>
): EditorCellSelection? {
    if (selection.opt("type") != "cell" ||
        selection.keys().asSequence().toSet() != setOf("type", "anchorCell", "headCell")
    ) return null
    val anchor = exactV2U32(selection.opt("anchorCell") as? Number)?.toLong() ?: return null
    val head = exactV2U32(selection.opt("headCell") as? Number)?.toLong() ?: return null

    data class Cell(val position: Int, val row: Int, val column: Int, val rowEnd: Int, val columnEnd: Int) {
        fun intersects(top: Int, left: Int, bottom: Int, right: Int): Boolean =
            row < bottom && rowEnd > top && column < right && columnEnd > left
    }

    val candidates = records.mapNotNull { (id, record) ->
        val tableStart = exactV2U32(record.opt("tablePos") as? Number)?.toLong() ?: return@mapNotNull null
        val tableEnd = exactV2U32(record.opt("sourceEnd") as? Number)?.toLong() ?: return@mapNotNull null
        if (!record.isNull("failure")) {
            return@mapNotNull if (anchor in tableStart until tableEnd && head in tableStart until tableEnd) {
                (tableEnd - tableStart) to EditorCellSelection.Unavailable(id)
            } else null
        }
        val rowCount = exactV2U32(record.opt("rows") as? Number)?.toLong()
            ?.takeIf { it <= Int.MAX_VALUE }?.toInt() ?: return@mapNotNull null
        val columnCount = exactV2U32(record.opt("columns") as? Number)?.toLong()
            ?.takeIf { it <= Int.MAX_VALUE }?.toInt() ?: return@mapNotNull null
        val rawCells = record.optJSONArray("cells") ?: return@mapNotNull null
        val cells = (0 until rawCells.length()).map { index ->
            val cell = rawCells.optJSONObject(index) ?: return@mapNotNull null
            val position = exactV2U32(cell.opt("sourcePos") as? Number)?.toLong()
                ?.takeIf { it <= Int.MAX_VALUE }?.toInt() ?: return@mapNotNull null
            val row = exactV2U32(cell.opt("row") as? Number)?.toLong() ?: return@mapNotNull null
            val column = exactV2U32(cell.opt("column") as? Number)?.toLong() ?: return@mapNotNull null
            val rowspan = exactV2U32(cell.opt("rowspan") as? Number)?.toLong() ?: return@mapNotNull null
            val colspan = exactV2U32(cell.opt("colspan") as? Number)?.toLong() ?: return@mapNotNull null
            if (rowspan == 0L || colspan == 0L || row + rowspan > rowCount.toLong() ||
                column + colspan > columnCount.toLong() || position.toLong() !in tableStart until tableEnd
            ) return@mapNotNull null
            Cell(position, row.toInt(), column.toInt(), (row + rowspan).toInt(), (column + colspan).toInt())
        }
        val first = cells.singleOrNull { it.position.toLong() == anchor } ?: return@mapNotNull null
        val last = cells.singleOrNull { it.position.toLong() == head } ?: return@mapNotNull null
        var top = minOf(first.row, last.row)
        var left = minOf(first.column, last.column)
        var bottom = maxOf(first.rowEnd, last.rowEnd)
        var right = maxOf(first.columnEnd, last.columnEnd)
        while (true) {
            val prior = listOf(top, left, bottom, right)
            cells.filter { it.intersects(top, left, bottom, right) }.forEach { cell ->
                top = minOf(top, cell.row)
                left = minOf(left, cell.column)
                bottom = maxOf(bottom, cell.rowEnd)
                right = maxOf(right, cell.columnEnd)
            }
            if (prior == listOf(top, left, bottom, right)) break
        }
        val selected = cells.filter { it.intersects(top, left, bottom, right) }
            .map { it.position }.toSet()
        (tableEnd - tableStart) to EditorCellSelection.Drawable(id, selected)
    }
    val drawable = candidates.mapNotNull { it.second as? EditorCellSelection.Drawable }
    if (drawable.size > 1) return null
    if (drawable.size == 1) return drawable.single()
    val nearest = candidates.minOfOrNull { it.first } ?: return null
    return candidates.singleOrNull { it.first == nearest }?.second
}
