package com.apollohg.editor.tables

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test
import com.apollohg.editor.viewer.PreparedProseLayout
import com.apollohg.editor.viewer.ProseLayoutKey

class TableGridLayoutTest {
    @Test fun testLayoutRectanglesAreKeyedBySourceIndex() {
        val cells = listOf(
            TableGridCell(sourceIndex = 1, row = 0, column = 1, contentKey = "second"),
            TableGridCell(sourceIndex = 0, row = 0, column = 0, contentKey = "first")
        )
        val layout = TableGridLayout().layout(record(cells = cells), 160f, TableStyle(), false) { _, _ -> 20f }
        assertEquals(setOf(0, 1), layout.rectangles.keys)
        assertEquals(listOf(0, 1), layout.sourceOrder)
        assertEquals(listOf(0f, 80f, 160f), layout.columnOffsets)
        assertEquals(0f, layout.rectangles.getValue(0).left)
        assertEquals(80f, layout.rectangles.getValue(1).left)
    }

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

        assertEquals(listOf(10, 20), surface.cells.map { it.sourceIndex })
        assertEquals(2, surface.visibleCells(surface.bounds).size)
        assertTrue(surface.layout.contentHeight.isFinite())
    }

    @Test fun `viewer surface measures equal content once per source and keeps source artifacts`() {
        val cells = listOf(TableGridCell(10, 0, 0, contentKey = "same"), TableGridCell(20, 0, 1, contentKey = "same"))
        val preparedSources = mutableListOf<Int>()
        val preparedContentKeys = mutableListOf<String>()
        val surface = ViewerTableSurface("t1", TableGridRecord("viewer", 2, 1, listOf(null, null), cells), 160f, TableStyle(), false) { cell, width ->
            preparedSources += cell.sourceIndex
            preparedContentKeys += cell.contentKey
            artifact(width.toInt(), 20, "${cell.sourceIndex}")
        }

        assertEquals(listOf(10, 20), surface.cells.map { it.sourceIndex })
        assertEquals(listOf("10", "20"), surface.cells.map { it.content.key.semanticKey })
        assertEquals(listOf(10, 20), preparedSources)
        assertEquals(listOf("same", "same"), preparedContentKeys)
    }

    @Test fun preparedRectanglesPreserveSnapshotsAndRejectMutableOffsetAliases() {
        val count = 12
        val grid = TableGridLayout(1.25f)
        val style = TableStyle(cellPadding = 3.25f, borderWidth = 0.65f)
        val source = record(columns = 2, rows = count / 2, cells = List(count) {
            TableGridCell(it, it / 2, it % 2, contentKey = "cell-$it")
        })
        fun prepared(cells: List<TableGridCell>, height: Int) = cells.map { cell ->
            PreparedViewerTableCell(cell.sourceIndex, cell.row, cell.column, cell.rowspan, cell.colspan,
                0 to 0, artifact(80, height, cell.contentKey), false, null)
        }
        fun expected(cells: List<TableGridCell>, height: Int) = grid.relayout(source.copy(cells = cells),
            240f, style, true, cells.associate { it.sourceIndex to height.toFloat() })
        fun update(previous: TableLayoutResult, cells: List<TableGridCell>, height: Int) =
            grid.relayoutPrepared(previous, source.rows, source.columns, style, true, prepared(cells, height), null) {
                throw AssertionError("Unexpected failure: $it")
            }
        val first = update(expected(source.cells, 20), source.cells, 40)
        val frozen = first.rectangles.toMap()
        val moved = source.cells.map { it.copy(column = 1 - it.column) }
        val second = update(first, moved, 60)
        assertEquals("Changed coordinates cannot reuse stale descriptors", expected(moved, 60), second)
        assertEquals("New rows cannot mutate previous offsets", frozen, first.rectangles)
        assertEquals(first.rectangles, frozen)
        assertTrue("Owned offsets cannot be mutated through Kotlin casts", first.columnOffsets !is MutableList<*>)
        assertTrue(first.rowOffsets !is MutableList<*>)
        assertTrue(first.sourceOrder !is MutableList<*>)
        val entry = first.rectangles.entries.first()
        org.junit.Assert.assertThrows(UnsupportedOperationException::class.java) {
            @Suppress("UNCHECKED_CAST")
            (entry as java.util.Map.Entry<Int, TableCellRect>).setValue(TableCellRect(0f, 0f, 0f, 0f))
        }
        val mutableColumns = first.columnOffsets.toMutableList()
        val mutableOrder = first.sourceOrder.toMutableList()
        val untrusted = update(first.copy(columnOffsets = mutableColumns, sourceOrder = mutableOrder), source.cells, 60)
        val untrustedFrozen = untrusted.rectangles.toMap()
        mutableColumns[1] += 11f
        mutableOrder[0] = -1
        assertEquals("Untrusted offset storage must use eager snapshots", untrustedFrozen, untrusted.rectangles)
        assertEquals((0 until count).toList(), untrusted.sourceOrder)
        val sparse = source.copy(rows = 10_000)
        val sparseCells = prepared(source.cells, 20)
        val sparseBase = grid.relayout(sparse, 240f, style, false, source.cells.associate { it.sourceIndex to 20f })
        val sparseResult = grid.relayoutPrepared(sparseBase, sparse.rows, sparse.columns, style, false, sparseCells, null) {
            throw AssertionError("Unexpected sparse failure: $it")
        }
        assertEquals("Sparse layouts preserve exact prefix sums without copying prior offsets", sparseBase, sparseResult)
        org.junit.Assert.assertSame(sparseBase.columnOffsets, sparseResult.columnOffsets)
    }

    @Test fun compactRectanglesReleasePreparedCellsAndPriorLayouts() {
        val references = mutableListOf<java.lang.ref.WeakReference<Any>>()
        fun build(): TableLayoutResult {
            val columns = 20
            val rows = 10
            val cells = List(rows * columns) { TableGridCell(it, it / columns, it % columns, contentKey = "cell-$it") }
            val source = record(columns, rows, cells = cells)
            val grid = TableGridLayout()
            var layout = grid.relayout(source, 390f, TableStyle(), false, cells.associate { it.sourceIndex to 20f })
            repeat(4) { revision ->
                references += java.lang.ref.WeakReference(layout)
                val prepared = cells.map { cell ->
                    val content = artifact(80, 40 + revision, "${cell.contentKey}-$revision")
                    references += java.lang.ref.WeakReference(content)
                    PreparedViewerTableCell(cell.sourceIndex, cell.row, cell.column, cell.rowspan, cell.colspan,
                        0 to 0, content, false, null).also { references += java.lang.ref.WeakReference(it) }
                }
                layout = grid.relayoutPrepared(layout, rows, columns, TableStyle(), false, prepared, null) {
                    throw AssertionError("Unexpected failure: $it")
                }
            }
            return layout
        }
        val retained = build()
        val gcAttempts = 8
        repeat(gcAttempts) { System.gc(); System.runFinalization() }
        assertEquals("Compact maps must release cells, content closures and all previous layouts", 0,
            references.count { it.get() != null })
        assertEquals("Retained primitive geometry remains usable", 200, retained.rectangles.size)
        assertTrue(retained.rectangles.values.all { it.height > 0f })
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
            if (cell.sourceIndex == 10) 80f else 20f
        }
        assertEquals(listOf(0f, 38f, 98f), result.rowOffsets)
        assertEquals(98f, result.rectangles[10]?.height)
    }

    @Test fun `cached relayout preserves rowspan geometry and rejects missing heights`() {
        val cells = listOf(TableGridCell(0, 0, 0, 2, 1, "span"),
            TableGridCell(1, 0, 1, contentKey = "first"), TableGridCell(2, 1, 1, contentKey = "second"))
        val source = record(rows = 2, cells = cells)
        val heights = mapOf(0 to 80f, 1 to 20f, 2 to 20f)
        val grid = TableGridLayout()
        val cached = grid.relayout(source, 160f, TableStyle(), true, heights)
        assertEquals(listOf(0f, 38f, 98f), cached.rowOffsets)
        assertEquals(grid.layout(source, 160f, TableStyle(), true) { cell, _ -> heights[cell.sourceIndex] }, cached)
        assertEquals(TableLayoutFailure.INVALID_ATTRIBUTES,
            grid.relayout(source, 160f, TableStyle(), true, heights - 2).failure)
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
            when (cell.sourceIndex) {
                10 -> 102f
                20 -> 20f
                else -> 60f
            }
        }

        assertEquals(160f, result.rectangles[20]?.width)
        assertEquals(listOf(0f, 38f, 120f), result.rowOffsets)
        assertEquals(cells.size, result.rectangles.size)
        assertEquals(cells.map { it.sourceIndex }.toSet(), result.rectangles.keys)
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
                measuredPositions += cell.sourceIndex
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
