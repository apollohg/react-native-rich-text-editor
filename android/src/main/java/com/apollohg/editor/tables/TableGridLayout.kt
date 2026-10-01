package com.apollohg.editor.tables

import android.text.TextUtils
import android.view.View
import uniffi.editor_core.TableCompatibilityDiagnostic
import uniffi.editor_core.TableRenderFailure
import java.util.Locale
import kotlin.math.ceil

internal enum class TableLayoutDirection {
    LEFT_TO_RIGHT,
    RIGHT_TO_LEFT;

    companion object {
        fun fromRaw(value: String?): TableLayoutDirection? = when (value) {
            "ltr" -> LEFT_TO_RIGHT
            "rtl" -> RIGHT_TO_LEFT
            else -> null
        }

        fun fromLayoutDirection(layoutDirection: Int): TableLayoutDirection =
            if (layoutDirection == View.LAYOUT_DIRECTION_RTL) RIGHT_TO_LEFT else LEFT_TO_RIGHT

        fun fromDefaultLocale(): TableLayoutDirection =
            fromLayoutDirection(TextUtils.getLayoutDirectionFromLocale(Locale.getDefault()))

        fun isRightToLeft(declared: String?, fallback: TableLayoutDirection): Boolean =
            (fromRaw(declared) ?: fallback) == RIGHT_TO_LEFT
    }
}

internal fun tablePhysicalX(logicalX: Float, width: Float, totalWidth: Float, rtl: Boolean): Float =
    if (rtl) totalWidth - logicalX - width else logicalX

interface TableGridCellPosition {
    val sourceIndex: Int
    val row: Int
    val column: Int
    val rowspan: Int
    val colspan: Int
}

data class TableGridCell(override val sourceIndex: Int, override val row: Int, override val column: Int,
    override val rowspan: Int = 1, override val colspan: Int = 1, val contentKey: String,
    val attachmentRevision: Long = 0) : TableGridCellPosition {
    companion object {
        internal fun from(cell: TableSurfaceCell) = TableGridCell(
            cell.sourceIndex, cell.row, cell.column, cell.rowspan, cell.colspan, cell.contentKey)
    }
}

enum class TableLayoutFailure { GRID_LIMIT, WORK_LIMIT, ALLOCATION, INVALID_STRUCTURE, INVALID_ATTRIBUTES }

data class TableGridRecord(
    val documentOwner: String, val columns: Int, val rows: Int, val columnWidths: List<Float?>,
    val cells: List<TableGridCell>, val failure: TableLayoutFailure? = null,
    val compatibilityDiagnostic: TableCompatibilityDiagnostic? = null,
    val typedFailure: TableRenderFailure? = null
) {
    companion object {
        fun from(table: TableSurfaceSource, documentOwner: String) = TableGridRecord(
            documentOwner, table.columns, table.rows, table.columnWidths,
            table.cells.map(TableGridCell::from),
            table.failure?.let { TableLayoutFailure.valueOf(it.name) }, table.compatibilityDiagnostic, table.failure
        )
    }
}

internal fun TableGridRecord.physical(scale: Float): TableGridRecord {
    val unit = scale.takeIf { it.isFinite() && it > 0f } ?: 1f
    return copy(columnWidths = columnWidths.map { it?.times(unit) })
}

data class TableCellRect(val left: Float, val top: Float, val width: Float, val height: Float)
data class TableLayoutResult(
    val columnWidths: List<Float>, val columnOffsets: List<Float>, val rowOffsets: List<Float>, val rectangles: Map<Int, TableCellRect>,
    val sourceOrder: List<Int>, val contentWidth: Float, val contentHeight: Float,
    val failure: TableLayoutFailure?, val compatibilityDiagnostic: TableCompatibilityDiagnostic?, val typedFailure: TableRenderFailure?
)

class TableGridLayout(private val displayScale: Float = 1f, private val cache: TableCellMeasurementCache = TableCellMeasurementCache()) {
    fun layout(record: TableGridRecord, viewportWidth: Float, style: TableStyle, rtl: Boolean,
               themeDigest: String = "", fontEnvironmentRevision: Long = 0, textScale: Float = 1f,
               measureCell: (TableGridCell, Float) -> Float?): TableLayoutResult {
        val heights = mutableMapOf<Int, Float>()
        for ((cell, pixelWidth) in measurementInputs(record, viewportWidth, style)) {
            val key = TableCellMeasurementKey(record.documentOwner, cell.contentKey, pixelWidth,
                themeDigest, fontEnvironmentRevision, textScale, cell.attachmentRevision)
            val measured = cache.get(key) ?: measureCell(cell, pixelWidth / scale())
                ?.takeIf { it.isFinite() && it >= 0f }?.also { cache.put(key, it) } ?: break
            heights[cell.sourceIndex] = measured
        }
        return relayout(record, viewportWidth, style, rtl, heights)
    }

    internal fun measurementInputs(record: TableGridRecord, viewportWidth: Float, style: TableStyle): List<Pair<TableGridCell, Int>> {
        if (record.failure != null || !style.isValid() || !viewportWidth.isFinite() || viewportWidth < 0f ||
            record.columns <= 0 || record.rows <= 0 || !record.cells.all { valid(it, record.rows, record.columns) } ||
            !record.columnWidths.all { it == null || it.isFinite() && it >= 0f }) return emptyList()
        val (_, offsets) = columnGeometry(record, viewportWidth, style) ?: return emptyList()
        val inputs = mutableListOf<Pair<TableGridCell, Int>>()
        for (cell in record.cells.sortedBy { it.sourceIndex }) {
            val inner = maxOf(0f, offsets[cell.column + cell.colspan] - offsets[cell.column] -
                2 * (style.cellPadding + style.borderWidth))
            val pixels = inner * scale()
            if (!pixels.isFinite() || pixels < 0f || pixels > Int.MAX_VALUE.toFloat()) break
            inputs += cell to kotlin.math.round(pixels).toInt()
        }
        return inputs
    }

    fun relayout(record: TableGridRecord, viewportWidth: Float, style: TableStyle, rtl: Boolean,
                 cachedContentHeights: Map<Int, Float>): TableLayoutResult {
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
        val geometry = columnGeometry(record, viewportWidth, style)
            ?: return fallback(TableLayoutFailure.INVALID_ATTRIBUTES, record, fallbackWidth, fallbackHeight)
        return relayoutCells(record.rows, record.columns, geometry, style, rtl,
            record.cells.sortedBy { it.sourceIndex }, record.compatibilityDiagnostic,
            { cachedContentHeights[it.sourceIndex] }) { failure ->
            fallback(failure, record, fallbackWidth, fallbackHeight)
        }
    }

    internal fun relayoutPrepared(
        previous: TableLayoutResult, rows: Int, columns: Int, style: TableStyle, rtl: Boolean,
        cells: List<PreparedViewerTableCell>, compatibilityDiagnostic: TableCompatibilityDiagnostic?,
        failed: (TableLayoutFailure) -> TableLayoutResult
    ): TableLayoutResult = relayoutCells(rows, columns, previous.columnWidths to previous.columnOffsets,
        style, rtl, cells, compatibilityDiagnostic, { it.contentHeightPx.toFloat() }, failed)

    private fun <T : TableGridCellPosition> relayoutCells(
        rowsCount: Int, columnsCount: Int, geometry: Pair<List<Float>, List<Float>>,
        style: TableStyle, rtl: Boolean, ordered: List<T>,
        compatibilityDiagnostic: TableCompatibilityDiagnostic?, measuredHeight: (T) -> Float?,
        failed: (TableLayoutFailure) -> TableLayoutResult
    ): TableLayoutResult {
        val minimumRow = style.cellPadding * 2f + style.borderWidth * 2f
        val fallbackHeight = if (minimumRow.isFinite() && minimumRow >= 1f) minimumRow else 1f
        val (widths, xOffsets) = geometry
        val heights = MutableList(rowsCount) { minimumRow }
        if (!ordered.all { valid(it, rowsCount, columnsCount) }) {
            return failed(TableLayoutFailure.INVALID_STRUCTURE)
        }
        fun contentHeight(cell: T): Float? = measuredHeight(cell)?.takeIf { it.isFinite() && it >= 0f }
        ordered.filter { it.rowspan == 1 }.forEach { cell ->
            val content = contentHeight(cell) ?: return failed(TableLayoutFailure.INVALID_ATTRIBUTES)
            val wanted = maxOf(fallbackHeight, content + 2 * (style.cellPadding + style.borderWidth))
            if (!wanted.isFinite()) return failed(TableLayoutFailure.INVALID_ATTRIBUTES)
            heights[cell.row] = maxOf(heights[cell.row], wanted)
        }
        ordered.filter { it.rowspan > 1 }.forEach { cell ->
            val content = contentHeight(cell) ?: return failed(TableLayoutFailure.INVALID_ATTRIBUTES)
            val wanted = maxOf(fallbackHeight, content + 2 * (style.cellPadding + style.borderWidth))
            if (!wanted.isFinite()) return failed(TableLayoutFailure.INVALID_ATTRIBUTES)
            val current = (cell.row until cell.row + cell.rowspan).sumOf { heights[it].toDouble() }.toFloat()
            if (!current.isFinite()) return failed(TableLayoutFailure.INVALID_ATTRIBUTES)
            if (wanted > current) {
                val height = heights[cell.row + cell.rowspan - 1] + wanted - current
                if (!height.isFinite()) return failed(TableLayoutFailure.INVALID_ATTRIBUTES)
                heights[cell.row + cell.rowspan - 1] = height
            }
        }
        val rows = mutableListOf(0f)
        for (height in heights) {
            val offset = rows.last() + snapOutward(height)
            if (!offset.isFinite()) return failed(TableLayoutFailure.INVALID_ATTRIBUTES)
            rows += offset
        }
        val total = xOffsets.last()
        val rectangles = ordered.filter { valid(it, rowsCount, columnsCount) }.associate { cell ->
            val logical = xOffsets[cell.column]; val width = xOffsets[cell.column + cell.colspan] - logical
            cell.sourceIndex to TableCellRect(tablePhysicalX(logical, width, total, rtl), rows[cell.row], width, rows[cell.row + cell.rowspan] - rows[cell.row])
        }
        return TableLayoutResult(widths, xOffsets, rows, rectangles, ordered.map { it.sourceIndex }, total, rows.last(), null, compatibilityDiagnostic, null)
    }

    private fun columnGeometry(record: TableGridRecord, viewportWidth: Float, style: TableStyle): Pair<List<Float>, List<Float>>? {
        val widths = MutableList(record.columns) { index -> maxOf(style.minColumnWidth, record.columnWidths.getOrNull(index) ?: 0f) }
        val unspecified = (0 until record.columns).filter { record.columnWidths.getOrNull(it) == null }
        val minimumWidth = widths.fold(0f) { total, width -> total + width }
        if (!minimumWidth.isFinite()) return null
        val surplus = viewportWidth - minimumWidth
        if (surplus > 0f && unspecified.isNotEmpty()) unspecified.forEach { widths[it] += surplus / unspecified.size }
        for (index in widths.indices) widths[index] = snapOutward(widths[index])
        if (widths.any { !it.isFinite() }) return null
        val xOffsets = mutableListOf(0f)
        for (width in widths) {
            val offset = xOffsets.last() + width
            if (!offset.isFinite()) return null
            xOffsets += offset
        }
        return widths to xOffsets
    }

    private fun fallback(failure: TableLayoutFailure, record: TableGridRecord, width: Float, height: Float) =
        TableLayoutResult(emptyList(), listOf(0f), listOf(0f, height), emptyMap(), emptyList(), width, height, failure, record.compatibilityDiagnostic, record.typedFailure)

    private fun valid(cell: TableGridCellPosition, rows: Int, columns: Int) = cell.row >= 0 && cell.column >= 0 && cell.rowspan > 0 && cell.colspan > 0 && cell.rowspan <= rows - cell.row && cell.colspan <= columns - cell.column
    private fun scale() = if (displayScale.isFinite() && displayScale > 0f) displayScale else 1f
    private fun snapOutward(value: Float) = ceil(value * scale()) / scale()
}
