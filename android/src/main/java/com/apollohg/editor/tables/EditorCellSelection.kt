package com.apollohg.editor.tables

import com.apollohg.editor.exactV2U32
import org.json.JSONObject

internal sealed interface EditorCellSelection {
    val tableId: String

    data class Drawable(override val tableId: String, val sourceIndices: Set<Int>) :
        EditorCellSelection
    data class Unavailable(override val tableId: String) : EditorCellSelection
}

internal fun resolveEditorCellSelection(
    selection: JSONObject,
    index: EditorTableIndex
): EditorCellSelection? {
    if (selection.opt("type") != "cell" ||
        selection.keys().asSequence().toSet() != setOf("type", "anchorCell", "headCell")
    ) {
        return null
    }
    val anchor = exactV2U32(selection.opt("anchorCell") as? Number)?.toLong() ?: return null
    val head = exactV2U32(selection.opt("headCell") as? Number)?.toLong() ?: return null
    return resolveEditorCellSelection(anchor, head, index)
}

internal fun resolveEditorCellSelection(
    anchor: Long,
    head: Long,
    index: EditorTableIndex
): EditorCellSelection? {
    data class Cell(
        val sourceIndex: Int,
        val position: Int,
        val row: Int,
        val column: Int,
        val rowEnd: Int,
        val columnEnd: Int
    ) {
        fun intersects(top: Int, left: Int, bottom: Int, right: Int): Boolean =
            row < bottom && rowEnd > top && column < right && columnEnd > left
    }

    val candidates = index.tableKeys.mapNotNull { id ->
        val record = index.record(id) ?: return@mapNotNull null
        val tableStart = index.tableDocStart(id)?.toLong() ?: return@mapNotNull null
        val tableEnd = tableStart + record.docSize.toLong()
        if (record.failure != null) {
            return@mapNotNull if (anchor in tableStart until tableEnd &&
                head in tableStart until tableEnd
            ) {
                (tableEnd - tableStart) to EditorCellSelection.Unavailable(id)
            } else {
                null
            }
        }
        val cells = record.cells.mapIndexed { cellIndex, cell ->
            val position =
                index.docStart(id, cellIndex)?.toLong()?.takeIf { it <= Int.MAX_VALUE }
                    ?: return@mapNotNull null
            Cell(
                cellIndex,
                position.toInt(),
                cell.row.toInt(),
                cell.column.toInt(),
                (cell.row + cell.rowspan).toInt(),
                (cell.column + cell.colspan).toInt()
            )
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
            .map { it.sourceIndex }.toSet()
        (tableEnd - tableStart) to EditorCellSelection.Drawable(id, selected)
    }
    val drawable = candidates.mapNotNull { it.second as? EditorCellSelection.Drawable }
    if (drawable.size > 1) return null
    if (drawable.size == 1) return drawable.single()
    val nearest = candidates.minOfOrNull { it.first } ?: return null
    return candidates.singleOrNull { it.first == nearest }?.second
}
