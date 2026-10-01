package com.apollohg.editor.tables

import android.text.TextUtils
import android.view.View
import uniffi.editor_core.TableCompatibilityDiagnostic
import uniffi.editor_core.TableRenderFailure
import java.util.Locale
import java.util.AbstractMap.SimpleImmutableEntry
import java.util.RandomAccess
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

internal const val TABLE_RECTANGLE_RETAINED_BYTES = 48L
internal const val TABLE_SOURCE_ORDER_RETAINED_BYTES = 16L

internal fun tableCellRect(row: Int, column: Int, rowspan: Int, colspan: Int,
                           xOffsets: List<Float>, yOffsets: List<Float>, total: Float, rtl: Boolean): TableCellRect {
    val logical = xOffsets[column]
    val width = xOffsets[column + colspan] - logical
    val top = yOffsets[row]
    return TableCellRect(tablePhysicalX(logical, width, total, rtl), top, width, yOffsets[row + rowspan] - top)
}

private class TableOffsets(private val values: FloatArray) : AbstractList<Float>(), RandomAccess {
    override val size: Int get() = values.size
    override fun get(index: Int): Float = values[index]
}

private class DenseTableSourceOrder(override val size: Int) : AbstractList<Int>(), RandomAccess {
    override fun get(index: Int): Int {
        if (index < 0 || index >= size) throw IndexOutOfBoundsException("Source index $index outside $size cells")
        return index
    }
}

private class PreparedTableRectangles(
    private val positions: IntArray, private val xOffsets: TableOffsets,
    private val yOffsets: TableOffsets, private val rtl: Boolean
) : AbstractMap<Int, TableCellRect>() {
    override val size: Int get() = positions.size / POSITION_FIELDS
    override fun containsKey(key: Int): Boolean = key >= 0 && key < size
    override fun get(key: Int): TableCellRect? {
        if (!containsKey(key)) return null
        val offset = key * POSITION_FIELDS
        return tableCellRect(positions[offset + ROW], positions[offset + COLUMN],
            positions[offset + ROWSPAN], positions[offset + COLSPAN], xOffsets, yOffsets, xOffsets.last(), rtl)
    }
    override val entries: Set<Map.Entry<Int, TableCellRect>> = object : AbstractSet<Map.Entry<Int, TableCellRect>>() {
        override val size: Int get() = this@PreparedTableRectangles.size
        override fun iterator(): Iterator<Map.Entry<Int, TableCellRect>> = object : Iterator<Map.Entry<Int, TableCellRect>> {
            private var index = 0
            override fun hasNext(): Boolean = index < size
            override fun next(): Map.Entry<Int, TableCellRect> {
                if (!hasNext()) throw NoSuchElementException()
                val key = index++
                return SimpleImmutableEntry(key, getValue(key))
            }
        }
    }

    private fun matches(cells: List<TableGridCellPosition>): Boolean = size == cells.size &&
        cells.withIndex().all { (index, cell) ->
            val offset = index * POSITION_FIELDS
            cell.sourceIndex == index && positions[offset + ROW] == cell.row &&
                positions[offset + COLUMN] == cell.column && positions[offset + ROWSPAN] == cell.rowspan &&
                positions[offset + COLSPAN] == cell.colspan
        }

    companion object {
        private const val ROW = 0
        private const val COLUMN = 1
        private const val ROWSPAN = 2
        private const val COLSPAN = 3
        private const val POSITION_FIELDS = 4
        // Covers map/views, dense order, offset wrappers and primitive-array headers.
        private const val FIXED_RETAINED_BYTES = 512L
        private const val POSITION_RETAINED_BYTES = POSITION_FIELDS * Int.SIZE_BYTES

        fun create(previous: TableLayoutResult, cells: List<TableGridCellPosition>,
                   xOffsets: List<Float>, yOffsets: TableOffsets, rtl: Boolean): PreparedTableRectangles? {
            if (xOffsets !is TableOffsets || cells.size > Int.MAX_VALUE / POSITION_FIELDS ||
                cells.size.toLong() * (TABLE_RECTANGLE_RETAINED_BYTES + TABLE_SOURCE_ORDER_RETAINED_BYTES -
                    POSITION_RETAINED_BYTES) < FIXED_RETAINED_BYTES ||
                cells.withIndex().any { (index, cell) -> cell.sourceIndex != index }) return null
            val old = previous.rectangles as? PreparedTableRectangles
            val positions = if (old != null && old.matches(cells)) old.positions else {
                IntArray(cells.size * POSITION_FIELDS).also { result ->
                    cells.forEachIndexed { index, cell ->
                        val offset = index * POSITION_FIELDS
                        result[offset + ROW] = cell.row
                        result[offset + COLUMN] = cell.column
                        result[offset + ROWSPAN] = cell.rowspan
                        result[offset + COLSPAN] = cell.colspan
                    }
                }
            }
            return PreparedTableRectangles(positions, xOffsets, yOffsets, rtl)
        }
    }
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
            record.cells.sortedBy { it.sourceIndex }, null, record.compatibilityDiagnostic,
            { cachedContentHeights[it.sourceIndex] }) { failure ->
            fallback(failure, record, fallbackWidth, fallbackHeight)
        }
    }

    internal fun relayoutPrepared(
        previous: TableLayoutResult, rows: Int, columns: Int, style: TableStyle, rtl: Boolean,
        cells: List<PreparedViewerTableCell>, compatibilityDiagnostic: TableCompatibilityDiagnostic?,
        failed: (TableLayoutFailure) -> TableLayoutResult
    ): TableLayoutResult = relayoutCells(rows, columns, previous.columnWidths to previous.columnOffsets,
        style, rtl, cells, previous, compatibilityDiagnostic, { it.contentHeightPx.toFloat() }, failed)

    private fun <T : TableGridCellPosition> relayoutCells(
        rowsCount: Int, columnsCount: Int, geometry: Pair<List<Float>, List<Float>>,
        style: TableStyle, rtl: Boolean, ordered: List<T>, compactPrevious: TableLayoutResult?,
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
        val rowValues = FloatArray(rowsCount + 1)
        for ((index, height) in heights.withIndex()) {
            val offset = rowValues[index] + snapOutward(height)
            if (!offset.isFinite()) return failed(TableLayoutFailure.INVALID_ATTRIBUTES)
            rowValues[index + 1] = offset
        }
        val rows = TableOffsets(rowValues)
        val total = xOffsets.last()
        val compact = compactPrevious?.let { PreparedTableRectangles.create(it, ordered, xOffsets, rows, rtl) }
        val rectangles = compact ?: ordered.filter { valid(it, rowsCount, columnsCount) }.associate { cell ->
            cell.sourceIndex to tableCellRect(cell.row, cell.column, cell.rowspan, cell.colspan, xOffsets, rows, total, rtl)
        }
        val sourceOrder = if (compact != null) DenseTableSourceOrder(ordered.size) else ordered.map { it.sourceIndex }
        return TableLayoutResult(widths, xOffsets, rows, rectangles, sourceOrder, total, rows.last(), null, compatibilityDiagnostic, null)
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
        val offsets = FloatArray(widths.size + 1)
        for ((index, width) in widths.withIndex()) {
            val offset = offsets[index] + width
            if (!offset.isFinite()) return null
            offsets[index + 1] = offset
        }
        return widths to TableOffsets(offsets)
    }

    private fun fallback(failure: TableLayoutFailure, record: TableGridRecord, width: Float, height: Float) =
        TableLayoutResult(emptyList(), listOf(0f), listOf(0f, height), emptyMap(), emptyList(), width, height, failure, record.compatibilityDiagnostic, record.typedFailure)

    private fun valid(cell: TableGridCellPosition, rows: Int, columns: Int) = cell.row >= 0 && cell.column >= 0 && cell.rowspan > 0 && cell.colspan > 0 && cell.rowspan <= rows - cell.row && cell.colspan <= columns - cell.column
    private fun scale() = if (displayScale.isFinite() && displayScale > 0f) displayScale else 1f
    private fun snapOutward(value: Float) = ceil(value * scale()) / scale()
}
