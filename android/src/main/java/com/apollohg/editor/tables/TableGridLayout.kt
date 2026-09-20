package com.apollohg.editor.tables

import uniffi.editor_core.FfiViewerTable
import uniffi.editor_core.TableCompatibilityDiagnostic
import uniffi.editor_core.TableRenderFailure
import kotlin.math.ceil

internal fun tablePhysicalX(logicalX: Float, width: Float, totalWidth: Float, rtl: Boolean): Float =
    if (rtl) totalWidth - logicalX - width else logicalX

data class TableGridCell(val sourcePosition: Int, val row: Int, val column: Int, val rowspan: Int = 1, val colspan: Int = 1, val contentKey: String, val attachmentRevision: Long = 0)

enum class TableLayoutFailure { GRID_LIMIT, WORK_LIMIT, ALLOCATION, INVALID_STRUCTURE, INVALID_ATTRIBUTES }

data class TableGridRecord(
    val documentOwner: String, val columns: Int, val rows: Int, val columnWidths: List<Float?>,
    val cells: List<TableGridCell>, val failure: TableLayoutFailure? = null,
    val compatibilityDiagnostic: TableCompatibilityDiagnostic? = null,
    val typedFailure: TableRenderFailure? = null
) {
    companion object {
        fun from(table: FfiViewerTable, documentOwner: String) = TableGridRecord(
            documentOwner, table.columns.toInt(), table.rows.toInt(), table.columnWidths.map { it?.toFloat() },
            table.cells.map { TableGridCell(it.sourcePos.toInt(), it.row.toInt(), it.column.toInt(), it.rowspan.toInt(), it.colspan.toInt(), it.contentKey) },
            table.failure?.let { TableLayoutFailure.valueOf(it.name) }, table.compatibilityDiagnostic, table.failure
        )
    }
}

data class TableCellRect(val left: Float, val top: Float, val width: Float, val height: Float)
data class TableLayoutResult(
    val columnWidths: List<Float>, val rowOffsets: List<Float>, val rectangles: Map<Int, TableCellRect>,
    val sourceOrder: List<Int>, val contentWidth: Float, val contentHeight: Float,
    val failure: TableLayoutFailure?, val compatibilityDiagnostic: TableCompatibilityDiagnostic?, val typedFailure: TableRenderFailure?
)

class TableGridLayout(private val displayScale: Float = 1f, private val cache: TableCellMeasurementCache = TableCellMeasurementCache()) {
    fun layout(record: TableGridRecord, viewportWidth: Float, style: TableStyle, rtl: Boolean,
               themeDigest: String = "", fontEnvironmentRevision: Long = 0, textScale: Float = 1f,
               measureCell: (TableGridCell, Float) -> Float?): TableLayoutResult {
        val minimumRow = style.cellPadding * 2f + style.borderWidth * 2f
        val fallbackHeight = if (minimumRow.isFinite() && minimumRow >= 1f) minimumRow else 1f
        val fallbackWidth = if (viewportWidth.isFinite() && viewportWidth >= 0f) {
            maxOf(fallbackHeight, minOf(viewportWidth, style.minColumnWidth.takeIf { it.isFinite() } ?: fallbackHeight))
        } else {
            fallbackHeight
        }
        val invalidInput = !style.isValid() || !viewportWidth.isFinite() || viewportWidth < 0f || record.columns <= 0 || record.rows <= 0 || record.columnWidths.any { it != null && (!it.isFinite() || it < 0f) }
        if (invalidInput || record.failure != null) {
            return fallback(record.failure ?: TableLayoutFailure.INVALID_ATTRIBUTES, record, fallbackWidth, fallbackHeight)
        }
        val widths = MutableList(record.columns) { index -> maxOf(style.minColumnWidth, record.columnWidths.getOrNull(index) ?: 0f) }
        val unspecified = (0 until record.columns).filter { record.columnWidths.getOrNull(it) == null }
        val minimumWidth = widths.fold(0f) { total, width -> total + width }
        if (!minimumWidth.isFinite()) return fallback(TableLayoutFailure.INVALID_ATTRIBUTES, record, fallbackWidth, fallbackHeight)
        val surplus = viewportWidth - minimumWidth
        if (surplus > 0f && unspecified.isNotEmpty()) unspecified.forEach { widths[it] += surplus / unspecified.size }
        for (index in widths.indices) widths[index] = snapOutward(widths[index])
        if (widths.any { !it.isFinite() }) return fallback(TableLayoutFailure.INVALID_ATTRIBUTES, record, fallbackWidth, fallbackHeight)
        val xOffsets = mutableListOf(0f)
        for (width in widths) {
            val offset = xOffsets.last() + width
            if (!offset.isFinite()) return fallback(TableLayoutFailure.INVALID_ATTRIBUTES, record, fallbackWidth, fallbackHeight)
            xOffsets += offset
        }
        val heights = MutableList(record.rows) { minimumRow }
        val ordered = record.cells.sortedBy { it.sourcePosition }
        if (!ordered.all { valid(it, record) }) {
            return fallback(TableLayoutFailure.INVALID_STRUCTURE, record, fallbackWidth, fallbackHeight)
        }
        fun contentHeight(cell: TableGridCell, inner: Float): Float? {
            val pixels = inner * scale()
            if (!pixels.isFinite() || pixels < 0f || pixels > Int.MAX_VALUE.toFloat()) return null
            val pixelWidth = kotlin.math.round(pixels).toInt()
            val measuredWidth = pixelWidth / scale()
            val key = TableCellMeasurementKey(record.documentOwner, cell.contentKey, pixelWidth, themeDigest, fontEnvironmentRevision, textScale, cell.attachmentRevision)
            cache.get(key)?.let { return it }
            val content = measureCell(cell, measuredWidth)?.takeIf { it.isFinite() && it >= 0f } ?: return null
            cache.put(key, content)
            return content
        }
        ordered.filter { it.rowspan == 1 }.forEach { cell ->
            val inner = maxOf(0f, xOffsets[cell.column + cell.colspan] - xOffsets[cell.column] - 2 * (style.cellPadding + style.borderWidth))
            val content = contentHeight(cell, inner) ?: return fallback(TableLayoutFailure.INVALID_ATTRIBUTES, record, fallbackWidth, fallbackHeight)
            val wanted = maxOf(fallbackHeight, content + 2 * (style.cellPadding + style.borderWidth))
            if (!wanted.isFinite()) return fallback(TableLayoutFailure.INVALID_ATTRIBUTES, record, fallbackWidth, fallbackHeight)
            heights[cell.row] = maxOf(heights[cell.row], wanted)
        }
        ordered.filter { it.rowspan > 1 }.forEach { cell ->
            val inner = maxOf(0f, xOffsets[cell.column + cell.colspan] - xOffsets[cell.column] - 2 * (style.cellPadding + style.borderWidth))
            val content = contentHeight(cell, inner) ?: return fallback(TableLayoutFailure.INVALID_ATTRIBUTES, record, fallbackWidth, fallbackHeight)
            val wanted = maxOf(fallbackHeight, content + 2 * (style.cellPadding + style.borderWidth))
            if (!wanted.isFinite()) return fallback(TableLayoutFailure.INVALID_ATTRIBUTES, record, fallbackWidth, fallbackHeight)
            val current = (cell.row until cell.row + cell.rowspan).sumOf { heights[it].toDouble() }.toFloat()
            if (!current.isFinite()) return fallback(TableLayoutFailure.INVALID_ATTRIBUTES, record, fallbackWidth, fallbackHeight)
            if (wanted > current) {
                val height = heights[cell.row + cell.rowspan - 1] + wanted - current
                if (!height.isFinite()) return fallback(TableLayoutFailure.INVALID_ATTRIBUTES, record, fallbackWidth, fallbackHeight)
                heights[cell.row + cell.rowspan - 1] = height
            }
        }
        val rows = mutableListOf(0f)
        for (height in heights) {
            val offset = rows.last() + snapOutward(height)
            if (!offset.isFinite()) return fallback(TableLayoutFailure.INVALID_ATTRIBUTES, record, fallbackWidth, fallbackHeight)
            rows += offset
        }
        val total = xOffsets.last()
        val rectangles = ordered.filter { valid(it, record) }.associate { cell ->
            val logical = xOffsets[cell.column]; val width = xOffsets[cell.column + cell.colspan] - logical
            cell.sourcePosition to TableCellRect(tablePhysicalX(logical, width, total, rtl), rows[cell.row], width, rows[cell.row + cell.rowspan] - rows[cell.row])
        }
        return TableLayoutResult(widths, rows, rectangles, ordered.map { it.sourcePosition }, total, rows.last(), null, record.compatibilityDiagnostic, null)
    }

    private fun fallback(failure: TableLayoutFailure, record: TableGridRecord, width: Float, height: Float) =
        TableLayoutResult(emptyList(), listOf(0f, height), emptyMap(), emptyList(), width, height, failure, record.compatibilityDiagnostic, record.typedFailure)

    private fun valid(cell: TableGridCell, record: TableGridRecord) = cell.row >= 0 && cell.column >= 0 && cell.rowspan > 0 && cell.colspan > 0 && cell.rowspan <= record.rows - cell.row && cell.colspan <= record.columns - cell.column
    private fun scale() = if (displayScale.isFinite() && displayScale > 0f) displayScale else 1f
    private fun snapOutward(value: Float) = ceil(value * scale()) / scale()
}
