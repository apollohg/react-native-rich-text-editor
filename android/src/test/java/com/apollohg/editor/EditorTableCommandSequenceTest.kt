package com.apollohg.editor

import android.app.Activity
import android.graphics.RectF
import android.os.SystemClock
import android.view.KeyEvent
import android.view.View
import android.view.accessibility.AccessibilityNodeInfo
import android.view.inputmethod.EditorInfo
import com.apollohg.editor.tables.TableAccessibility
import com.apollohg.editor.tables.TableAccessibilityAction
import com.apollohg.editor.tables.TableAccessibilityNodes
import com.apollohg.editor.viewer.PreparedProseDrawingView
import org.json.JSONArray
import org.json.JSONObject
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotEquals
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.RuntimeEnvironment
import org.robolectric.annotation.Config
import org.robolectric.annotation.GraphicsMode
import org.robolectric.shadows.ShadowLooper

@RunWith(RobolectricTestRunner::class)
@Config(sdk = [34])
@GraphicsMode(GraphicsMode.Mode.NATIVE)
internal class EditorTableCommandSequenceTest {
    private var keyEventOrdinal = 0L
    private data class Cell(
        val paragraphs: List<String>,
        val type: String = CELL_NODE,
        val colspan: Int = UNIT_SPAN,
        val rowspan: Int = UNIT_SPAN,
        val colwidth: List<Int>? = null
    )

    private data class Caret(
        val row: Int?,
        val column: Int = FIRST_COLUMN,
        val offset: Int = START_OFFSET
    )

    private class Harness(var view: RichTextEditorView, val adapter: EditorV2Adapter) {
        val input: EditorEditText get() = view.activeTextInput
        val drawing: PreparedProseDrawingView get() = view.editorTableSurface.drawingView

        fun document(): JSONObject = JSONObject(requireNotNull(adapter.documentJson()))
        fun selection(): JSONObject = requireNotNull(adapter.cachedAtomicRenderSelection())
        fun table(): JSONObject? = document().getJSONArray("content").objects()
            .singleOrNull { it.getString("type") == TABLE_NODE }
        fun tableKey(): String = adapter.tableIndex.rootExtents.keys.single()
        fun openings(): List<Int> = adapter.tableRecordsForTesting.values.single()
            .getJSONArray("cells").objects().map { it.getInt("sourcePos") }

        fun layout() {
            ShadowLooper.idleMainLooper()
            measureMountedEditor(view, EDITOR_WIDTH, EDITOR_HEIGHT)
            drawing.measure(
                View.MeasureSpec.makeMeasureSpec(
                    view.editorEditText.width,
                    View.MeasureSpec.EXACTLY
                ),
                View.MeasureSpec.makeMeasureSpec(
                    view.editorEditText.height,
                    View.MeasureSpec.EXACTLY
                )
            )
            drawing.layout(0, 0, drawing.measuredWidth, drawing.measuredHeight)
        }

        fun perform(actionId: Int, sourceIndex: Int, label: String) {
            layout()
            val provider = drawing.accessibilityNodeProvider
            val nodeIds = generateSequence(TableAccessibilityNodes.FIRST_TABLE_NODE_ID) {
                it +
                    UNIT_SPAN
            }
                .map { id -> provider.createAccessibilityNodeInfo(id)?.let { id to it } }
                .takeWhile { it != null }.filterNotNull()
                .filter { (_, info) -> info.collectionItemInfo != null }.map { it.first }.toList()
            assertTrue(
                "$label accessibility action rejected; selection=${selection()} " +
                    "notes=${adapter.debugNotes} " +
                    "trace=${input.imeTraceSnapshotForTesting()}",
                provider.performAction(nodeIds[sourceIndex], actionId, null)
            )
        }

        fun resize(column: Int, label: String) {
            val update = requireNotNull(
                adapter.resizeTableColumn(
                    column,
                    RESIZED_COLUMN_WIDTH,
                    adapter.tableMutationAdmission(tableKey())
                )
            ) { "$label resize rejected: ${adapter.debugNotes}" }
            assertTrue("$label resize update rejected", input.applyUpdateJSON(update))
        }

        fun selectCells(anchorIndex: Int, headIndex: Int, label: String) {
            val positions = openings()
            fun point(position: Int) =
                JSONObject().put("kind", DOCUMENT_POSITION).put("offset", position)
            val requested = JSONObject().put("type", CELL_SELECTION)
                .put(
                    "anchorCell",
                    point(positions[anchorIndex])
                ).put("headCell", point(positions[headIndex]))
            val result = adapter.callWithEnvelope(JSONObject().put("selection", requested)) {
                UniffiEditorV2Backend.setSelection(adapter.editorId, it)
            }
            assertTrue("$label rectangle rejected: $result", result is EditorV2CallResult.Ok)
            assertTrue(
                "$label rectangle update rejected",
                view.editorEditText.applyUpdateJSON(
                    requireNotNull(adapter.refreshFromRustState(null))
                )
            )
        }

        fun history(undo: Boolean, label: String) {
            val update = requireNotNull(if (undo) adapter.undo() else adapter.redo()) {
                "$label history rejected: ${adapter.debugNotes}"
            }
            assertTrue("$label history update rejected", input.applyUpdateJSON(update))
        }

        fun remount(label: String) {
            val token = view.editorId
            val context = view.context as Activity
            view.editorId = NO_EDITOR
            view = RichTextEditorView(context)
            context.setContentView(view)
            layout()
            assertTrue("$label root focus rejected", view.editorEditText.requestFocus())
            view.editorId = token
        }
    }

    @Test fun `row insertion before preserves every cell caret and accepts continued typing`() =
        runMatrix(ADD_ROW_BEFORE)

    @Test fun `row insertion after preserves every cell caret and accepts continued typing`() =
        runMatrix(ADD_ROW_AFTER)

    @Test fun `column insertion before preserves every cell caret and accepts continued typing`() =
        runMatrix(ADD_COLUMN_BEFORE)

    @Test fun `column insertion after preserves every cell caret and accepts continued typing`() =
        runMatrix(ADD_COLUMN_AFTER)

    @Test fun `row deletion resolves every cell caret and accepts continued typing`() =
        runMatrix(DELETE_ROWS)

    @Test fun `column deletion resolves every cell caret and accepts continued typing`() =
        runMatrix(DELETE_COLUMNS)

    @Test fun `header cell toggles preserve every cell caret and accept continued typing`() =
        runMatrix(HEADER_CELL)

    @Test fun `header row toggles preserve every cell caret and accept continued typing`() =
        runMatrix(HEADER_ROW)

    @Test fun `header column toggles preserve every cell caret and accept continued typing`() =
        runMatrix(HEADER_COLUMN)

    @Test fun `column resize preserves every cell caret and accepts continued typing`() =
        runMatrix(RESIZE_COLUMN)

    @Test fun `table deletion returns every cell caret to editable prose`() = runMatrix(
        DELETE_TABLE
    )

    private fun runMatrix(actionId: Int) {
        val config = corpus().getJSONObject("config").toString()
        val action = TableAccessibilityAction.ALL.singleOrNull { it.id == actionId }
        for (row in FIRST_ROW until GRID_ROWS) {
            for (column in FIRST_COLUMN until GRID_COLUMNS) {
                for (offset in CARET_OFFSETS) {
                    val label = "${action?.applicability ?: RESIZE_LABEL}/cell $row,$column/" +
                        "caret $offset"
                    withMountedEditorView(seedDocument(), config, windowed = true) {
                            view,
                            adapter,
                            _
                        ->
                        val harness = Harness(view, adapter)
                        val sourceIndex = row * GRID_COLUMNS + column
                        tapMountedTableCell(view, sourceIndex)
                        harness.input.setSelection(offset)
                        val expected = expectedAfter(actionId, row, column, offset)
                        if (actionId == RESIZE_COLUMN) {
                            harness.resize(column, label)
                        } else {
                            harness.perform(actionId, sourceIndex, label)
                        }
                        check(harness, expected.first, expected.second, label)
                        probe(harness, expected.first, expected.second, label)
                        if (actionId in HEADER_ACTIONS) {
                            harness.perform(actionId, sourceIndex, "$label/toggle back")
                            check(harness, grid(), Caret(row, column, offset), "$label/toggle back")
                            probe(harness, grid(), Caret(row, column, offset), "$label/toggle back")
                        }
                    }
                }
            }
        }
    }

    private fun expectedAfter(
        actionId: Int,
        row: Int,
        column: Int,
        offset: Int
    ): Pair<List<List<Cell>>, Caret> {
        val initial = grid()
        val preserved = Caret(row, column, offset)
        return when (actionId) {
            ADD_ROW_BEFORE, ADD_ROW_AFTER -> {
                val insertion = row + if (actionId == ADD_ROW_AFTER) UNIT_SPAN else START_OFFSET
                val expected = initial.toMutableList().apply {
                    add(insertion, List(GRID_COLUMNS) { emptyCell() })
                }
                expected to
                    preserved.copy(
                        row =
                            row + if (actionId == ADD_ROW_BEFORE) UNIT_SPAN else START_OFFSET
                    )
            }

            ADD_COLUMN_BEFORE, ADD_COLUMN_AFTER -> {
                val insertion =
                    column + if (actionId == ADD_COLUMN_AFTER) UNIT_SPAN else START_OFFSET
                val expected = initial.map { cells ->
                    cells.toMutableList().apply { add(insertion, emptyCell()) }
                }
                expected to
                    preserved.copy(
                        column =
                            column + if (actionId == ADD_COLUMN_BEFORE) UNIT_SPAN else START_OFFSET
                    )
            }

            DELETE_ROWS -> initial.filterIndexed { index, _ -> index != row } to
                if (row == LAST_ROW) Caret(null) else Caret(row)

            DELETE_COLUMNS -> initial.map { cells ->
                cells.filterIndexed { index, _ ->
                    index !=
                        column
                }
            } to
                when {
                    column < LAST_COLUMN -> Caret(row, column)
                    row < LAST_ROW -> Caret(row + UNIT_SPAN)
                    else -> Caret(null)
                }

            HEADER_CELL, HEADER_ROW, HEADER_COLUMN -> initial.mapIndexed { currentRow, cells ->
                cells.mapIndexed { currentColumn, cell ->
                    val targeted = when (actionId) {
                        HEADER_ROW -> currentRow == row
                        HEADER_COLUMN -> currentColumn == column
                        else -> currentRow == row && currentColumn == column
                    }
                    if (targeted) cell.copy(type = HEADER_NODE) else cell
                }
            } to preserved

            RESIZE_COLUMN -> initial.map { cells ->
                cells.mapIndexed { index, cell ->
                    if (index ==
                        column
                    ) {
                        cell.copy(colwidth = listOf(RESIZED_COLUMN_WIDTH))
                    } else {
                        cell
                    }
                }
            } to preserved

            DELETE_TABLE -> emptyList<List<Cell>>() to Caret(null)

            else -> throw AssertionError("unknown table action $actionId")
        }
    }

    @Test
    fun `merge split clear navigation history and remount retain a usable native caret`() {
        withMountedEditorView(
            seedDocument(),
            corpus().getJSONObject("config").toString(),
            windowed = true
        ) {
                view,
                adapter,
                _
            ->
            val harness = Harness(view, adapter)
            var expected = grid()
            tapMountedTableCell(view, MIDDLE_CELL)
            harness.input.setSelection(MIDDLE_OFFSET)
            harness.selectCells(MIDDLE_CELL, MIDDLE_CELL + UNIT_SPAN, "trace/select")
            checkRectangle(harness, expected, MIDDLE_CELL, MIDDLE_CELL + UNIT_SPAN, "trace/select")
            val original = expected
            harness.perform(MERGE_CELLS, MIDDLE_CELL, "trace/merge")
            expected = expected.mapIndexed { row, cells ->
                if (row != MIDDLE_ROW) {
                    cells
                } else {
                    listOf(
                        cells[FIRST_COLUMN],
                        cells[MIDDLE_COLUMN].copy(
                            paragraphs =
                                cells[MIDDLE_COLUMN].paragraphs + cells[LAST_COLUMN].paragraphs,
                            colspan = DOUBLE_SPAN
                        )
                    )
                }
            }
            checkRectangle(harness, expected, MIDDLE_CELL, MIDDLE_CELL, "trace/merge")
            val merged = expected
            harness.history(undo = true, "trace/undo merge")
            checkRectangle(
                harness,
                original,
                MIDDLE_CELL,
                MIDDLE_CELL + UNIT_SPAN,
                "trace/undo merge"
            )
            harness.history(undo = false, "trace/redo merge")
            checkRectangle(harness, merged, MIDDLE_CELL, MIDDLE_CELL, "trace/redo merge")
            harness.perform(SPLIT_CELL, MIDDLE_CELL, "trace/split")
            expected = expected.mapIndexed { row, cells ->
                if (row != MIDDLE_ROW) {
                    cells
                } else {
                    listOf(
                        cells[FIRST_COLUMN],
                        cells[MIDDLE_COLUMN].copy(colspan = UNIT_SPAN),
                        emptyCell()
                    )
                }
            }
            checkRectangle(harness, expected, MIDDLE_CELL, MIDDLE_CELL + UNIT_SPAN, "trace/split")
            val split = expected
            harness.history(undo = true, "trace/undo split")
            checkRectangle(harness, merged, MIDDLE_CELL, MIDDLE_CELL, "trace/undo split")
            harness.history(undo = false, "trace/redo split")
            checkRectangle(harness, split, MIDDLE_CELL, MIDDLE_CELL + UNIT_SPAN, "trace/redo split")
            harness.perform(CLEAR_CELLS, MIDDLE_CELL, "trace/clear")
            expected = expected.mapIndexed { row, cells ->
                if (row != MIDDLE_ROW) {
                    cells
                } else {
                    cells.mapIndexed { column, cell ->
                        if (column >=
                            MIDDLE_COLUMN
                        ) {
                            emptyCell()
                        } else {
                            cell
                        }
                    }
                }
            }
            checkRectangle(harness, expected, MIDDLE_CELL, MIDDLE_CELL + UNIT_SPAN, "trace/clear")
            harness.history(undo = true, "trace/undo clear")
            checkRectangle(harness, split, MIDDLE_CELL, MIDDLE_CELL + UNIT_SPAN, "trace/undo clear")
            harness.history(undo = false, "trace/redo clear")
            checkRectangle(
                harness,
                expected,
                MIDDLE_CELL,
                MIDDLE_CELL + UNIT_SPAN,
                "trace/redo clear"
            )

            harness.perform(
                AccessibilityNodeInfo.ACTION_CLICK,
                MIDDLE_CELL,
                "trace/enter cleared cell"
            )
            check(harness, expected, Caret(MIDDLE_ROW, MIDDLE_COLUMN), "trace/enter cleared cell")
            probe(harness, expected, Caret(MIDDLE_ROW, MIDDLE_COLUMN), "trace/empty cell")

            navigate(harness, backward = false, "trace/tab")
            check(harness, expected, Caret(MIDDLE_ROW, LAST_COLUMN), "trace/tab")
            probe(harness, expected, Caret(MIDDLE_ROW, LAST_COLUMN), "trace/tab")
            navigate(harness, backward = true, "trace/shift tab")
            check(harness, expected, Caret(MIDDLE_ROW, MIDDLE_COLUMN), "trace/shift tab")

            harness.perform(ADD_ROW_BEFORE, MIDDLE_CELL, "trace/add row")
            expected =
                expected.toMutableList().apply {
                    add(MIDDLE_ROW, List(GRID_COLUMNS) { emptyCell() })
                }
            var caret = Caret(MIDDLE_ROW + UNIT_SPAN, MIDDLE_COLUMN)
            check(harness, expected, caret, "trace/add row")
            probe(harness, expected, caret, "trace/add row")
            harness.perform(HEADER_CELL, caret.row!! * GRID_COLUMNS + caret.column, "trace/header")
            expected = expected.mapIndexed { row, cells ->
                cells.mapIndexed { column, cell ->
                    if (row == caret.row &&
                        column == caret.column
                    ) {
                        cell.copy(type = HEADER_NODE)
                    } else {
                        cell
                    }
                }
            }
            check(harness, expected, caret, "trace/header")
            probe(harness, expected, caret, "trace/header")
            harness.resize(MIDDLE_COLUMN, "trace/resize")
            expected = expected.map { cells ->
                cells.mapIndexed { column, cell ->
                    if (column ==
                        MIDDLE_COLUMN
                    ) {
                        cell.copy(colwidth = listOf(RESIZED_COLUMN_WIDTH))
                    } else {
                        cell
                    }
                }
            }
            check(harness, expected, caret, "trace/resize")
            probe(harness, expected, caret, "trace/resize")

            harness.perform(
                ADD_COLUMN_BEFORE,
                caret.row!! * GRID_COLUMNS + caret.column,
                "trace/add column before"
            )
            expected =
                expected.map { cells ->
                    cells.toMutableList().apply { add(MIDDLE_COLUMN, emptyCell()) }
                }
            caret = caret.copy(column = caret.column + UNIT_SPAN)
            check(harness, expected, caret, "trace/add column before")
            probe(harness, expected, caret, "trace/add column before")
            var columns = GRID_COLUMNS + UNIT_SPAN
            harness.perform(
                ADD_COLUMN_AFTER,
                caret.row!! * columns + caret.column,
                "trace/add column after"
            )
            expected = expected.map { cells ->
                cells.toMutableList().apply {
                    add(caret.column + UNIT_SPAN, emptyCell().copy(type = cells[caret.column].type))
                }
            }
            columns += UNIT_SPAN
            check(harness, expected, caret, "trace/add column after")
            probe(harness, expected, caret, "trace/add column after")
            harness.perform(
                DELETE_COLUMNS,
                caret.row!! * columns + caret.column,
                "trace/delete column"
            )
            expected =
                expected.map { cells ->
                    cells.filterIndexed { column, _ -> column != caret.column }
                }
            columns -= UNIT_SPAN
            check(harness, expected, caret, "trace/delete column")
            probe(harness, expected, caret, "trace/delete column")
            harness.perform(DELETE_ROWS, caret.row!! * columns + caret.column, "trace/delete row")
            expected = expected.filterIndexed { row, _ -> row != caret.row }
            caret = caret.copy(column = FIRST_COLUMN)
            check(harness, expected, caret, "trace/delete row")
            probe(harness, expected, caret, "trace/delete row")

            harness.perform(
                DELETE_TABLE,
                caret.row!! * columns + caret.column,
                "trace/delete table"
            )
            check(harness, emptyList(), Caret(null), "trace/delete table")
            harness.history(undo = true, "trace/undo delete")
            check(harness, expected, caret, "trace/undo delete")
            harness.history(undo = false, "trace/redo delete")
            check(harness, emptyList(), Caret(null), "trace/redo delete")
            harness.history(undo = true, "trace/restore table")
            check(harness, expected, caret, "trace/restore table")
            val beforeRemount = harness.document().toString()
            harness.remount("trace/remount")
            assertEquals(
                "trace/remount changed the engine document",
                beforeRemount,
                harness.document().toString()
            )
            check(harness, expected, caret, "trace/remount")
            probe(harness, expected, caret, "trace/remount")
        }
    }

    private fun navigate(harness: Harness, backward: Boolean, label: String) {
        requireNotNull(harness.input.onTableCellTab) { "$label missing table navigation handler" }
        val time = SystemClock.uptimeMillis() + ++keyEventOrdinal * KEY_EVENT_INTERVAL_MS
        val input = harness.input
        assertTrue(
            "$label key rejected",
            harness.input.dispatchKeyEvent(
                KeyEvent(
                    time,
                    time,
                    KeyEvent.ACTION_DOWN,
                    KeyEvent.KEYCODE_TAB,
                    NO_REPEATS,
                    if (backward) KeyEvent.META_SHIFT_ON else NO_MODIFIERS
                )
            )
        )
        assertTrue(
            "$label key release rejected",
            input.dispatchKeyEvent(
                KeyEvent(
                    time,
                    time,
                    KeyEvent.ACTION_UP,
                    KeyEvent.KEYCODE_TAB,
                    NO_REPEATS,
                    if (backward) KeyEvent.META_SHIFT_ON else NO_MODIFIERS
                )
            )
        )
    }

    private fun checkGrid(harness: Harness, expected: List<List<Cell>>, label: String) {
        harness.layout()
        val table = harness.table()
        val actual = table?.getJSONArray("content")?.objects().orEmpty().map { row ->
            assertEquals("$label wrong row node", ROW_NODE, row.getString("type"))
            row.getJSONArray("content").objects().map { cell ->
                val attrs = cell.optJSONObject("attrs") ?: JSONObject()
                Cell(
                    cell.getJSONArray("content").objects().map { paragraph ->
                        assertEquals(
                            "$label wrong paragraph node",
                            PARAGRAPH_NODE,
                            paragraph.getString("type")
                        )
                        paragraph.optJSONArray("content").objects().joinToString("") {
                            it.getString("text")
                        }
                    },
                    cell.getString("type"),
                    attrs.optInt("colspan", UNIT_SPAN),
                    attrs.optInt("rowspan", UNIT_SPAN),
                    attrs.optJSONArray("colwidth")?.let { array ->
                        (0 until array.length()).map(array::getInt)
                    }
                )
            }
        }
        val diagnostic = "$label document=${harness.document()} input=${harness.input.text} " +
            "selection=${harness.selection()} notes=${harness.adapter.debugNotes} " +
            "trace=${harness.input.imeTraceSnapshotForTesting()}"
        assertEquals(
            "$diagnostic wrong cells, paragraphs, spans, headers or widths",
            expected,
            actual
        )
        assertEquals(
            "$label trailing prose changed",
            TRAILING_TEXT,
            harness.document().getJSONArray("content").objects().last().getJSONArray("content")
                .objects().joinToString("") { it.getString("text") }
        )
        val surfaces = harness.drawing.preparedLayout?.blocks.orEmpty().mapNotNull {
            it.tableSurface
        }
        if (expected.isEmpty()) {
            assertTrue("$diagnostic deleted table still rendered", surfaces.isEmpty())
            assertTrue(
                "$diagnostic deleted table still indexed",
                harness.adapter.tableRecordsForTesting.isEmpty()
            )
            return
        }
        val surface = surfaces.single()
        assertEquals(
            "$diagnostic rendered row count",
            expected.size,
            surface.sourceTable!!.rows.toInt()
        )
        assertEquals(
            "$diagnostic rendered column count",
            expected.first().sumOf {
                it.colspan
            },
            surface.sourceTable!!.columns.toInt()
        )
        assertEquals(
            "$diagnostic stale rendered column widths",
            expected.first().flatMap { cell ->
                cell.colwidth?.map(Int::toFloat) ?: List(cell.colspan) { null }
            },
            surface.sourceTable!!.columnWidths
        )
        assertEquals(
            "$diagnostic stale visible cell texts",
            expected.flatten().map {
                it.paragraphs.filter(
                    String::isNotEmpty
                ).joinToString(TableAccessibility.LABEL_SEPARATOR)
            },
            surface.cells.map {
                it.accessibilityText.replace(LayoutConstants.SYNTHETIC_PLACEHOLDER_CHARACTER, "")
            }
        )
        assertEquals(
            "$diagnostic stale visible headers",
            expected.flatten().map {
                it.type ==
                    HEADER_NODE
            },
            surface.cells.map { it.isHeader }
        )
        assertEquals(
            "$diagnostic stale visible spans",
            expected.flatten().map {
                it.rowspan to
                    it.colspan
            },
            surface.cells.map { it.rowspan to it.colspan }
        )
        for (cell in surface.cells) {
            val frame = surface.frameOfCell(cell)
            assertTrue(
                "$diagnostic invalid cell geometry $frame",
                frame.width > 0f && frame.height > 0f
            )
        }
        surface.sourceTable!!.columnWidths.forEachIndexed { column, width ->
            if (width !=
                null
            ) {
                assertEquals(
                    "$diagnostic resized column geometry $column",
                    width * surface.displayScale,
                    surface.layout.columnWidths[column],
                    GEOMETRY_TOLERANCE
                )
            }
        }
    }

    private fun check(harness: Harness, expected: List<List<Cell>>, caret: Caret, label: String) {
        checkGrid(harness, expected, label)
        val input = harness.input
        val selection = harness.selection()
        val documentCaret = if (caret.row == null) {
            (
                harness.adapter.tableRecordsForTesting.values.singleOrNull()?.getInt("sourceEnd")
                    ?: START_OFFSET
                ) +
                PARAGRAPH_TEXT_OFFSET +
                caret.offset
        } else {
            harness.openings()[expected.take(caret.row).sumOf { it.size } + caret.column] +
                CELL_TEXT_OFFSET +
                caret.offset
        }
        val diagnostic =
            "$label selection=$selection native=${input.selectionStart}..${input.selectionEnd} " +
                "input=${input.text} map=${input.tableCellPositionMap?.binding} " +
                "notes=${harness.adapter.debugNotes} " +
                "trace=${input.imeTraceSnapshotForTesting()}"
        assertEquals(
            "$diagnostic caret selection type",
            TEXT_SELECTION,
            selection.getString("type")
        )
        assertEquals("$diagnostic engine anchor", documentCaret, selection.getInt("anchor"))
        assertEquals("$diagnostic engine head", documentCaret, selection.getInt("head"))
        assertEquals(
            "$diagnostic native selection must collapse",
            input.selectionStart,
            input.selectionEnd
        )
        assertTrue("$diagnostic stale mutation authority", input.canDispatchTableCellMutation())
        val local =
            requireNotNull(input.currentScalarSelection()) { "$diagnostic unmappable local caret" }
        val global =
            requireNotNull(input.inputScalarSelection(local.first, local.second)) {
                "$diagnostic unmappable global caret"
            }
        assertEquals(
            "$diagnostic native anchor maps to wrong document position",
            documentCaret,
            harness.adapter.docPositionForScalar(global.first)
        )
        assertEquals(
            "$diagnostic native head maps to wrong document position",
            documentCaret,
            harness.adapter.docPositionForScalar(global.second)
        )
        if (caret.row == null) {
            assertTrue(
                "$diagnostic prose input not restored",
                input === harness.view.editorEditText
            )
            assertEquals(
                "$diagnostic native trailing prose is stale",
                TRAILING_TEXT,
                input.text.toString().substringAfterLast(PARAGRAPH_SEPARATOR)
                    .trimEnd(LayoutConstants.SYNTHETIC_PLACEHOLDER_CHARACTER[0])
            )
        } else {
            assertFalse("$diagnostic cell input was retired", input === harness.view.editorEditText)
            assertEquals(
                "$diagnostic wrong active source cell",
                expected.take(caret.row).sumOf { it.size } + caret.column,
                input.tableCellPositionMap?.binding?.cellIndex
            )
            assertEquals(
                "$diagnostic native cell text is stale",
                expected[caret.row][caret.column].paragraphs.filter(
                    String::isNotEmpty
                ).joinToString(PARAGRAPH_SEPARATOR),
                input.text.toString().trimEnd(LayoutConstants.SYNTHETIC_PLACEHOLDER_CHARACTER[0])
            )
            assertEquals(
                "$diagnostic stale position map revision",
                harness.adapter.baseDocumentRevision.toString(),
                input.tableCellPositionMap?.binding?.revision
            )
            assertEquals(
                "$diagnostic stale position map epoch",
                harness.adapter.positionEpoch,
                input.tableCellPositionMap?.binding?.epoch
            )
        }
        val rect = requireNotNull(input.caretRect()) { "$diagnostic missing native caret geometry" }
        assertTrue(
            "$diagnostic invalid caret geometry $rect",
            rect.left.isFinite() && rect.top.isFinite() && rect.height() > 0f
        )
        assertTrue(
            "$diagnostic caret outside active input $rect ${input.width}x${input.height}",
            rect.bottom > 0f && rect.top < input.height && rect.right > 0f &&
                rect.left < input.width
        )
    }

    private fun checkRectangle(
        harness: Harness,
        expected: List<List<Cell>>,
        anchor: Int,
        head: Int,
        label: String
    ) {
        checkGrid(harness, expected, label)
        val positions = harness.openings()
        val selection = harness.selection()
        assertEquals("$label rectangle type", CELL_SELECTION, selection.getString("type"))
        assertEquals("$label rectangle anchor", positions[anchor], selection.getInt("anchorCell"))
        assertEquals("$label rectangle head", positions[head], selection.getInt("headCell"))
        assertTrue(
            "$label rectangle does not own root input",
            harness.input === harness.view.editorEditText
        )
        assertTrue(
            "$label rectangle authority absent",
            harness.input.authoritativeCellSelectionActive
        )
        val selected = (anchor..head).toSet()
        assertEquals(
            "$label stale drawn rectangle",
            selected,
            harness.drawing.selectedTableCellSourceIndices[harness.tableKey()].orEmpty()
        )
    }

    private fun probe(harness: Harness, expected: List<List<Cell>>, caret: Caret, label: String) {
        val before = harness.document().toString()
        val selection = harness.selection().let { it.getInt("anchor") to it.getInt("head") }
        val input = harness.input
        val connection =
            requireNotNull(input.onCreateInputConnection(EditorInfo())) {
                "$label missing input connection"
            }
        val oldRect = RectF(requireNotNull(input.caretRect()))
        assertTrue(
            "$label continuing IME commit rejected",
            connection.commitText(PROBE_TEXT, CURSOR_AFTER_COMMIT)
        )
        if (caret.row == null) {
            harness.layout()
            assertEquals(
                "$label continuing IME inserted at wrong prose position",
                PROBE_TEXT + TRAILING_TEXT,
                harness.document().getJSONArray("content").objects().last().getJSONArray("content")
                    .objects().joinToString("") { it.getString("text") }
            )
            assertEquals(
                "$label continuing IME prose is not visible",
                PROBE_TEXT + TRAILING_TEXT,
                input.text.toString().substringAfterLast(PARAGRAPH_SEPARATOR)
                    .trimEnd(LayoutConstants.SYNTHETIC_PLACEHOLDER_CHARACTER[0])
            )
            assertEquals(
                "$label continuing IME caret did not advance",
                selection.second + PROBE_TEXT.length,
                harness.selection().getInt("head")
            )
        } else {
            check(
                harness,
                withText(expected, caret, PROBE_TEXT),
                caret.copy(
                    offset =
                        caret.offset + PROBE_TEXT.length
                ),
                "$label/type"
            )
        }
        assertTrue("$label typing replaced the active input", input === harness.input)
        assertNotEquals("$label visual caret stayed stuck after typing", oldRect, input.caretRect())
        assertTrue(
            "$label same connection backspace rejected",
            connection.deleteSurroundingTextInCodePoints(ONE_CHARACTER, NO_CHARACTERS)
        )
        assertEquals(
            "$label type and backspace failed to restore structure",
            before,
            harness.document().toString()
        )
        assertEquals(
            "$label type and backspace failed to restore document caret",
            selection,
            harness.selection().let { it.getInt("anchor") to it.getInt("head") }
        )
        check(harness, expected, caret, "$label/backspace")
    }

    private fun withText(expected: List<List<Cell>>, caret: Caret, text: String): List<List<Cell>> =
        expected.mapIndexed {
                row,
                cells
            ->
            cells.mapIndexed { column, cell ->
                if (row != caret.row ||
                    column != caret.column
                ) {
                    cell
                } else {
                    val paragraph = cell.paragraphs.first()
                    cell.copy(
                        paragraphs =
                            listOf(
                                paragraph.take(caret.offset) + text + paragraph.drop(caret.offset)
                            ) +
                                cell.paragraphs.drop(UNIT_SPAN)
                    )
                }
            }
        }

    private fun grid(): List<List<Cell>> = List(GRID_ROWS) { row ->
        List(GRID_COLUMNS) { column ->
            Cell(listOf("${ROW_LABELS[row]}$column"))
        }
    }

    private fun emptyCell(): Cell = Cell(listOf(""))
    private fun corpus(): JSONObject = JSONObject(
        RuntimeEnvironment.getApplication().assets.open(CORPUS_ASSET)
            .bufferedReader().use { it.readText() }
    )

    private fun seedDocument(): String {
        fun paragraph(text: String) = JSONObject().put("type", PARAGRAPH_NODE)
            .put("content", JSONArray().put(JSONObject().put("type", TEXT_NODE).put("text", text)))
        val table = JSONObject().put("type", TABLE_NODE).put(
            "content",
            JSONArray(
                grid().map { cells ->
                    JSONObject().put("type", ROW_NODE).put(
                        "content",
                        JSONArray(
                            cells.map { cell ->
                                JSONObject().put(
                                    "type",
                                    cell.type
                                ).put("content", JSONArray(cell.paragraphs.map(::paragraph)))
                            }
                        )
                    )
                }
            )
        )
        return JSONObject().put(
            "type",
            DOCUMENT_NODE
        ).put("content", JSONArray().put(table).put(paragraph(TRAILING_TEXT))).toString()
    }

    private companion object {
        const val CORPUS_ASSET = "editor-command-sequences.json"
        const val DOCUMENT_NODE = "doc"
        const val TABLE_NODE = "table"
        const val ROW_NODE = "table_row"
        const val CELL_NODE = "table_cell"
        const val HEADER_NODE = "table_header"
        const val PARAGRAPH_NODE = "paragraph"
        const val TEXT_NODE = "text"
        const val TEXT_SELECTION = "text"
        const val CELL_SELECTION = "cell"
        const val DOCUMENT_POSITION = "document"
        const val PARAGRAPH_SEPARATOR = "\n"
        const val TRAILING_TEXT = "after"
        const val PROBE_TEXT = "X"
        const val GRID_ROWS = 3
        const val GRID_COLUMNS = 3
        const val FIRST_ROW = 0
        const val FIRST_COLUMN = 0
        const val MIDDLE_ROW = 1
        const val MIDDLE_COLUMN = 1
        const val LAST_ROW = GRID_ROWS - 1
        const val LAST_COLUMN = GRID_COLUMNS - 1
        const val MIDDLE_CELL = MIDDLE_ROW * GRID_COLUMNS + MIDDLE_COLUMN
        const val START_OFFSET = 0
        const val MIDDLE_OFFSET = 1
        const val END_OFFSET = 2
        const val UNIT_SPAN = 1
        const val DOUBLE_SPAN = 2
        const val CELL_TEXT_OFFSET = 2
        const val PARAGRAPH_TEXT_OFFSET = 1
        const val ONE_CHARACTER = 1
        const val NO_CHARACTERS = 0
        const val CURSOR_AFTER_COMMIT = 1
        const val EDITOR_WIDTH = 600
        const val EDITOR_HEIGHT = 500
        const val RESIZED_COLUMN_WIDTH = 140
        const val GEOMETRY_TOLERANCE = 0f
        const val RESIZE_COLUMN = -1
        const val RESIZE_LABEL = "setTableColumnWidth"
        const val NO_EDITOR = 0L
        const val KEY_EVENT_INTERVAL_MS = 1L
        const val NO_REPEATS = 0
        const val NO_MODIFIERS = 0
        val CARET_OFFSETS = listOf(START_OFFSET, MIDDLE_OFFSET, END_OFFSET)
        val ROW_LABELS = listOf("a", "b", "c")
        val ADD_ROW_BEFORE = R.id.table_accessibility_add_row_before
        val ADD_ROW_AFTER = R.id.table_accessibility_add_row_after
        val ADD_COLUMN_BEFORE = R.id.table_accessibility_add_column_before
        val ADD_COLUMN_AFTER = R.id.table_accessibility_add_column_after
        val DELETE_ROWS = R.id.table_accessibility_delete_rows
        val DELETE_COLUMNS = R.id.table_accessibility_delete_columns
        val HEADER_CELL = R.id.table_accessibility_toggle_header_cell
        val HEADER_ROW = R.id.table_accessibility_toggle_header_row
        val HEADER_COLUMN = R.id.table_accessibility_toggle_header_column
        val DELETE_TABLE = R.id.table_accessibility_delete_table
        val MERGE_CELLS = R.id.table_accessibility_merge_cells
        val SPLIT_CELL = R.id.table_accessibility_split_cell
        val CLEAR_CELLS = R.id.table_accessibility_clear_cells
        val HEADER_ACTIONS = setOf(HEADER_CELL, HEADER_ROW, HEADER_COLUMN)
        fun JSONArray?.objects(): List<JSONObject> = if (this ==
            null
        ) {
            emptyList()
        } else {
            (0 until length()).map(::getJSONObject)
        }
    }
}
