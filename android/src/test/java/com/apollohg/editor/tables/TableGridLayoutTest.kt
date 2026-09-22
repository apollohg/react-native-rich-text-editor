package com.apollohg.editor.tables

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test
import com.apollohg.editor.viewer.PreparedProseLayout
import com.apollohg.editor.viewer.ProseLayoutKey

class TableGridLayoutTest {
    @Test fun `physical table adapter scales declared widths and chrome once`() {
        val source = TableGridRecord("density", 2, 1, listOf(100f, 100f), listOf(
            TableGridCell(1, 0, 0, contentKey = "one"), TableGridCell(2, 0, 1, contentKey = "two")
        ))
        val layout = TableGridLayout().layout(source.physical(2f), 300f, TableStyle().physical(2f), false) { _, width ->
            assertEquals(164f, width)
            20f
        }
        assertEquals(listOf(100f, 100f), source.columnWidths)
        assertEquals(listOf(200f, 200f), layout.columnWidths)
        assertEquals(400f, layout.contentWidth)
        assertEquals(18f, TableStyle().physical(2f).cellPadding + TableStyle().physical(2f).borderWidth)
        val minimum = TableGridLayout().layout(TableGridRecord("min", 1, 1, listOf(null), emptyList()).physical(2f), 100f, TableStyle().physical(2f), false) { _, _ -> 0f }
        assertEquals(160f, minimum.contentWidth)
    }
    @Test fun `viewer surface retains one prepared cell for each source anchor`() {
        val cells = listOf(TableGridCell(10, 0, 0, contentKey = "a"), TableGridCell(20, 0, 1, contentKey = "b"))
        val surface = ViewerTableSurface("t1", TableGridRecord("viewer", 2, 1, listOf(null, null), cells), 160f, TableStyle(), false) { cell, width ->
            artifact(width.toInt(), 20, cell.contentKey)
        }

        assertEquals(listOf(10, 20), surface.cells.map { it.sourcePosition })
        assertEquals(2, surface.visibleCells(surface.bounds).size)
        assertTrue(surface.layout.contentHeight.isFinite())
    }

    @Test fun `viewer surface measures equal content once per source and keeps source artifacts`() {
        val cells = listOf(TableGridCell(10, 0, 0, contentKey = "same"), TableGridCell(20, 0, 1, contentKey = "same"))
        val preparedSources = mutableListOf<Int>()
        val preparedContentKeys = mutableListOf<String>()
        val surface = ViewerTableSurface("t1", TableGridRecord("viewer", 2, 1, listOf(null, null), cells), 160f, TableStyle(), false) { cell, width ->
            preparedSources += cell.sourcePosition
            preparedContentKeys += cell.contentKey
            artifact(width.toInt(), 20, "${cell.sourcePosition}")
        }

        assertEquals(listOf(10, 20), surface.cells.map { it.sourcePosition })
        assertEquals(listOf("10", "20"), surface.cells.map { it.content.key.semanticKey })
        assertEquals(listOf(10, 20), preparedSources)
        assertEquals(listOf("same", "same"), preparedContentKeys)
    }

    private fun record(columns: Int = 2, rows: Int = 1, widths: List<Float?> = listOf(null, null), cells: List<TableGridCell> = emptyList()) =
        TableGridRecord("test-document", columns, rows, widths, cells)

    private fun artifact(width: Int, height: Int, identity: String) = PreparedProseLayout(
        ProseLayoutKey(identity, width, "", 0, 0, 1, 0, identity), width, height, emptyList(), retainedBytes = 0
    )

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

    @Test fun `horizontal span keeps synthetic gap anchor free and processes later single row minimum`() {
        val cells = listOf(
            TableGridCell(10, 0, 0, rowspan = 2, contentKey = "span"),
            TableGridCell(20, 0, 1, colspan = 2, contentKey = "wide"),
            TableGridCell(30, 1, 2, contentKey = "later")
        )
        val result = TableGridLayout().layout(record(columns = 3, rows = 2, widths = listOf(80f, 80f, 80f), cells = cells), 240f, TableStyle(), false) { cell, _ ->
            when (cell.sourcePosition) {
                10 -> 102f
                20 -> 20f
                else -> 60f
            }
        }

        assertEquals(160f, result.rectangles[20]?.width)
        assertEquals(listOf(0f, 38f, 120f), result.rowOffsets)
        assertEquals(cells.size, result.rectangles.size)
        assertEquals(cells.map { it.sourcePosition }.toSet(), result.rectangles.keys)
        assertEquals(listOf(10, 20, 30), result.sourceOrder)
    }

    @Test fun `warm layouts reuse measurements and dependency changes invalidate only their keys`() {
        val grid = TableGridLayout(cache = TableCellMeasurementCache())
        var cells = listOf(TableGridCell(10, 0, 0, contentKey = "a"), TableGridCell(20, 1, 0, contentKey = "b"))
        var calls = 0
        val measuredPositions = mutableListOf<Int>()
        fun layout(width: Float = 100f, theme: String = "theme", fontRevision: Long = 1L): TableLayoutResult =
            grid.layout(record(columns = 1, rows = 2, widths = listOf(width), cells = cells), width, TableStyle(), false, theme, fontRevision) { cell, _ ->
                calls += 1
                measuredPositions += cell.sourcePosition
                20f
            }

        val first = layout()
        val warm = layout()
        assertEquals(first.rectangles, warm.rectangles)
        assertEquals(2, calls)
        cells = listOf(TableGridCell(10, 0, 0, contentKey = "a", attachmentRevision = 1), cells[1])
        layout()
        assertEquals(3, calls)
        assertEquals(10, measuredPositions.last())
        cells = listOf(TableGridCell(10, 0, 0, contentKey = "updated", attachmentRevision = 1), cells[1])
        val contentChanged = layout()
        assertEquals(4, calls)
        assertEquals(10, measuredPositions.last())
        assertEquals(contentChanged.rectangles, layout().rectangles)
        assertEquals(4, calls)
        layout(theme = "new-theme")
        assertEquals(6, calls)
        layout(fontRevision = 2)
        assertEquals(8, calls)
        layout(width = 120f)
        assertEquals(10, calls)
    }
}
