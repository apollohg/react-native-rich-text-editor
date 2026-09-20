package com.apollohg.editor.tables

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

class TableGridLayoutTest {
    private fun record(columns: Int = 2, rows: Int = 1, widths: List<Float?> = listOf(null, null), cells: List<TableGridCell> = emptyList()) =
        TableGridRecord("test-document", columns, rows, widths, cells)

    @Test fun `unassigned columns share surplus and narrow view overflows`() {
        assertEquals(listOf(120f, 120f), TableGridLayout().layout(record(), 240f, TableStyle(), false) { _, _ -> 10f }.columnWidths)
        val narrow = TableGridLayout().layout(record(), 100f, TableStyle(), false) { _, _ -> 10f }
        assertEquals(listOf(80f, 80f), narrow.columnWidths)
        assertEquals(160f, narrow.contentWidth)
    }

    @Test fun `span content adds chrome and rowspan expands final covered row`() {
        val cells = listOf(TableGridCell(10, 0, 0, 2, 1, "a"), TableGridCell(20, 0, 1, contentKey = "b"), TableGridCell(30, 1, 1, contentKey = "c"))
        val result = TableGridLayout().layout(record(rows = 2, cells = cells), 160f, TableStyle(), false) { cell, width ->
            assertEquals(62f, width)
            if (cell.sourcePosition == 10) 80f else 20f
        }
        assertEquals(listOf(0f, 38f, 98f), result.rowOffsets)
        assertEquals(98f, result.rectangles[10]?.height)
    }

    @Test fun `rtl mirrors x while retaining source order`() {
        val cells = listOf(TableGridCell(10, 0, 0, contentKey = "a"), TableGridCell(20, 0, 1, contentKey = "b"))
        val result = TableGridLayout().layout(record(cells = cells), 160f, TableStyle(), true) { _, _ -> 10f }
        assertEquals(80f, result.rectangles[10]?.left)
        assertEquals(0f, result.rectangles[20]?.left)
        assertEquals(listOf(10, 20), result.sourceOrder)
    }

    @Test fun `failure produces finite frame with preserved failure`() {
        val result = TableGridLayout().layout(TableGridRecord("test", 0, 0, emptyList(), emptyList(), TableLayoutFailure.GRID_LIMIT), 200f, TableStyle(), false) { _, _ -> 0f }
        assertEquals(TableLayoutFailure.GRID_LIMIT, result.failure)
        assertTrue(result.contentHeight > 0f)
        assertEquals(2, result.rowOffsets.size)
    }

    @Test fun `cache key tracks owner attachment and remains bounded`() {
        val cache = TableCellMeasurementCache(capacity = 1)
        val key = TableCellMeasurementKey("a", "cell", 120, "theme", 1, 1f, 1)
        cache.put(key, 20f)
        assertEquals(20f, cache.get(key))
        assertNull(cache.get(key.copy(attachmentRevision = 2)))
        cache.put(key.copy(attachmentRevision = 2), 30f)
        assertNull(cache.get(key))
    }

    @Test fun `fractional scale snaps outward`() {
        val result = TableGridLayout(displayScale = 2f).layout(record(), 100f, TableStyle(minColumnWidth = 80.2f), false) { _, _ -> 10f }
        assertTrue(result.columnWidths[0] >= 80.2f)
        assertEquals(result.columnWidths[0] * 2, (result.columnWidths[0] * 2).toInt().toFloat())
    }

    @Test fun `cache measures at its pixel key width across fractional chrome changes`() {
        val grid = TableGridLayout(2f, TableCellMeasurementCache())
        val input = record(columns = 1, widths = listOf(100f), cells = listOf(TableGridCell(1, 0, 0, contentKey = "cell")))
        val widths = mutableListOf<Float>()
        grid.layout(input, 100f, TableStyle(cellPadding = 8.05f), false) { _, width -> widths += width; width }
        grid.layout(input, 100f, TableStyle(cellPadding = 8.1f), false) { _, width -> widths += width; width }
        assertEquals(listOf(82f), widths)
    }

    @Test fun `extreme finite inputs return finite failure frame`() {
        val result = TableGridLayout().layout(record(columns = 2, widths = listOf(Float.MAX_VALUE, Float.MAX_VALUE)), Float.MAX_VALUE, TableStyle(), false) { _, _ -> 10f }
        assertEquals(TableLayoutFailure.INVALID_ATTRIBUTES, result.failure)
        assertTrue(result.contentWidth.isFinite())
        assertTrue(result.contentHeight.isFinite())
    }

    @Test fun `nonfinite measurement returns failure instead of zero height success`() {
        val result = TableGridLayout().layout(record(cells = listOf(TableGridCell(1, 0, 0, contentKey = "cell"))), 160f, TableStyle(), false) { _, _ -> Float.POSITIVE_INFINITY }
        assertEquals(TableLayoutFailure.INVALID_ATTRIBUTES, result.failure)
        assertEquals(18f, result.contentHeight)
    }
}
