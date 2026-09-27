package com.apollohg.editor

import android.app.Activity
import android.content.ClipboardManager
import android.content.Context
import android.graphics.Color
import android.os.SystemClock
import android.util.Base64
import android.view.MotionEvent
import android.view.ViewGroup
import android.view.inputmethod.EditorInfo
import android.widget.FrameLayout
import androidx.test.core.app.ActivityScenario
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.filters.LargeTest
import androidx.test.platform.app.InstrumentationRegistry
import com.apollohg.editor.tables.TableAccessibilityNodes
import com.apollohg.editor.tables.TableLayoutDirection
import com.apollohg.editor.tables.ViewerTablePresentedCell
import com.apollohg.editor.tables.editorTableId
import com.apollohg.editor.viewer.PreparedProseDrawingView
import java.io.File
import org.json.JSONArray
import org.json.JSONObject
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNull
import org.junit.Assert.assertSame
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith

@RunWith(AndroidJUnit4::class)
@LargeTest
class NativeTableAcceptanceTest {
    private val instrumentation = InstrumentationRegistry.getInstrumentation()

    private data class Cell(
        val type: String,
        val paragraphs: List<String>,
        val colspan: Int,
        val rowspan: Int,
        val colwidth: List<Int>?
    )

    private inner class Harness(private val scenario: ActivityScenario<NativeEditorOutsideTapActivity>) {
        lateinit var adapter: EditorV2Adapter
        lateinit var view: RichTextEditorView
        var token = 0L
        private var nextRemoteRequestId = REMOTE_REQUEST_ID_BASE

        val root: EditorEditText get() = view.editorEditText
        val drawing: PreparedProseDrawingView get() = view.editorTableSurface.drawingView

        fun <T> onMain(block: (Activity) -> T): T {
            var result: Result<T>? = null
            scenario.onActivity { activity -> result = runCatching { block(activity) } }
            instrumentation.waitForIdleSync()
            return requireNotNull(result).getOrThrow()
        }

        fun create() = onMain { activity ->
            val created = when (val result = UniffiEditorV2Backend.create(CONFIG, null)) {
                is EditorV2CallResult.Ok -> result.value
                is EditorV2CallResult.Err -> error("create failed: ${result.error.code}: ${result.error.message}")
            }
            adapter = requireNotNull(EditorV2Adapter.attach(
                UniffiEditorV2Backend, JSONObject(created).getString("editorId"), roomBound = false
            ))
            requireNotNull(adapter.setContentJson(seedDocument()))
            token = EditorV2Registry.register(adapter)
            mount(activity)
        }

        fun mount(activity: Activity) {
            view = RichTextEditorView(activity).apply {
                applyTheme(EditorTheme.fromJson(THEME))
                editorId = token
            }
            activity.setContentView(FrameLayout(activity).apply {
                setBackgroundColor(Color.WHITE)
                addView(view, FrameLayout.LayoutParams(
                    ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.MATCH_PARENT
                ).apply {
                    val margin = (EDITOR_MARGIN_DP * activity.resources.displayMetrics.density).toInt()
                    setMargins(margin, margin * TOP_MARGIN_FACTOR, margin, margin)
                })
            })
        }

        fun release() = onMain {
            view.editorId = 0L
            if (token != 0L) releasePairedV2TestEditor(token)
        }

        fun tableId(): String = requireNotNull(adapter.cachedTableRecords.entries.firstOrNull {
            !it.value.optBoolean("readOnlyDescendants", true)
        }?.key) { "no editable table is rendered" }

        fun positions(): List<Int> {
            val cells = requireNotNull(adapter.cachedTableRecords[tableId()]).getJSONArray("cells")
            return (0 until cells.length()).map { cells.getJSONObject(it).getInt("sourcePos") }
        }

        fun activeCell(): Long? = view.activeTextInput.tableCellPositionMap?.binding?.cellSourcePos

        fun selectedCells(): Set<Int> = drawing.selectedTableCellSourcePositions[tableId()].orEmpty()

        fun presentedCell(position: Int): ViewerTablePresentedCell {
            val id = tableId()
            return requireNotNull(drawing.presentedTableCells().firstOrNull {
                it.surface.editorTableId == id && it.sourcePosition == position && it.cell.sourceCellIndex != null
            }) { "cell $position is not presented" }
        }

        fun documentJson(): String = requireNotNull(adapter.documentJson())

        fun blocks(): List<JSONObject> = JSONObject(documentJson()).getJSONArray("content").objects()

        fun grid(): List<List<Cell>> {
            val table = blocks().first { it.getString("type") == TABLE_NODE }
            return table.getJSONArray("content").objects().map { row ->
                assertEquals(ROW_NODE, row.getString("type"))
                row.optJSONArray("content")?.objects().orEmpty().map(::cell)
            }
        }

        private fun cell(node: JSONObject): Cell {
            val attrs = node.optJSONObject("attrs") ?: JSONObject()
            val paragraphs = node.getJSONArray("content").objects().map { block ->
                assertEquals(PARAGRAPH_NODE, block.getString("type"))
                block.optJSONArray("content")?.objects().orEmpty().joinToString("") { run ->
                    val marks = run.optJSONArray("marks")?.objects().orEmpty().map { it.getString("type") }
                    val text = run.getString("text")
                    if (marks.isEmpty()) text else "<${marks.joinToString(",")}>$text</>"
                }
            }
            val widths = attrs.optJSONArray("colwidth")?.let { array -> (0 until array.length()).map(array::getInt) }
            return Cell(node.getString("type"), paragraphs, attrs.optInt("colspan", 1), attrs.optInt("rowspan", 1), widths)
        }

        fun applyLocalCommand(command: JSONObject) {
            val result = adapter.callWithEnvelope(JSONObject().put("command", command)) {
                UniffiEditorV2Backend.applyCommand(adapter.editorId, it)
            }
            assertTrue("the local command $command was refused: $result", result is EditorV2CallResult.Ok)
            assertTrue(root.applyUpdateJSON(requireNotNull(adapter.refreshFromRustState(null))))
        }

        fun selectCells(anchor: Int, head: Int) {
            val result = adapter.callWithEnvelope(JSONObject().put("selection", cellSelection(anchor, head))) {
                UniffiEditorV2Backend.setSelection(adapter.editorId, it)
            }
            assertTrue("engine rejected the selection: $result", result is EditorV2CallResult.Ok)
            assertTrue(root.applyUpdateJSON(requireNotNull(adapter.refreshFromRustState(null))))
        }

        private fun remoteEnvelope(payload: JSONObject): String {
            nextRemoteRequestId += 1
            return payload.put("version", 1).put("requestId", nextRemoteRequestId.toString())
                .put("baseDocumentRevision", adapter.baseDocumentRevision.toString()).toString()
        }

        fun applyRemote(payload: JSONObject, call: (String, String) -> EditorV2CallResult<String>) {
            val result = call(adapter.editorId, remoteEnvelope(payload))
            assertTrue("the remote peer's change was refused: $result", result is EditorV2CallResult.Ok)
        }

        fun deliverRemoteCommit() {
            val preflight = view.activeTextInput.prepareForExternalEditorUpdateWithResult()
            assertTrue("the remote commit could not be prepared", preflight.ready)
            val update = preflight.adoptedUpdateJSON ?: requireNotNull(adapter.refreshFromRustState(null))
            assertTrue(root.applyUpdateJSON(update, refreshInputConnectionForExternalUpdate = true))
        }

        fun cellNodeIds(): List<Int> {
            val provider = drawing.accessibilityNodeProvider
            return generateSequence(TableAccessibilityNodes.FIRST_TABLE_NODE_ID) { it + 1 }
                .map { id -> provider.createAccessibilityNodeInfo(id)?.let { id to it } }
                .takeWhile { it != null }.filterNotNull()
                .filter { (_, info) -> info.collectionItemInfo != null }
                .map { it.first }.toList()
        }

        fun actionIds(position: Int): List<Int> {
            val info = requireNotNull(drawing.accessibilityNodeProvider.createAccessibilityNodeInfo(
                cellNodeIds()[positions().indexOf(position)]
            ))
            return info.actionList.map { it.id }
        }

        fun perform(actionId: Int, position: Int) {
            val node = cellNodeIds()[positions().indexOf(position)]
            assertTrue("action $actionId on cell $position was refused",
                drawing.accessibilityNodeProvider.performAction(node, actionId, null))
        }

        fun screenPoint(position: Int, trailingEdge: Boolean): Pair<Float, Float> = onMain {
            val cell = presentedCell(position)
            val location = IntArray(2)
            drawing.getLocationOnScreen(location)
            val x = if (trailingEdge) {
                if (cell.surface.isRightToLeft) cell.bounds.left else cell.bounds.right
            } else {
                cell.bounds.centerX()
            }
            location[0] + x to location[1] + cell.bounds.centerY()
        }

        fun tapCell(position: Int) {
            val (x, y) = screenPoint(position, trailingEdge = false)
            gesture(listOf(x to y, x to y))
        }

        fun drag(from: Pair<Float, Float>, to: Pair<Float, Float>) {
            val steps = (0..DRAG_STEPS).map { step ->
                val fraction = step.toFloat() / DRAG_STEPS
                from.first + (to.first - from.first) * fraction to from.second
            }
            gesture(listOf(from) + steps + listOf(to))
        }

        private fun gesture(points: List<Pair<Float, Float>>) {
            val start = SystemClock.uptimeMillis()
            points.forEachIndexed { index, (x, y) ->
                val action = when (index) {
                    0 -> MotionEvent.ACTION_DOWN
                    points.lastIndex -> MotionEvent.ACTION_UP
                    else -> MotionEvent.ACTION_MOVE
                }
                val event = MotionEvent.obtain(start, start + index * GESTURE_STEP_MS, action, x, y, 0)
                try { instrumentation.sendPointerSync(event) } finally { event.recycle() }
            }
            instrumentation.waitForIdleSync()
        }

        fun commit(text: String) {
            val input = view.activeTextInput
            assertTrue("commit '$text' refused",
                requireNotNull(input.onCreateInputConnection(EditorInfo())).commitText(text, 1))
        }

        fun screenshot(name: String) {
            instrumentation.waitForIdleSync()
            SystemClock.sleep(SCREENSHOT_SETTLE_MS)
            instrumentation.waitForIdleSync()
            instrumentation.saveDeviceScreenshot(name)
        }

        fun export() = onMain {
            val exported = UniffiEditorV2Backend.snapshotExport(adapter.editorId)
            assertTrue("snapshot export failed: $exported", exported is EditorV2CallResult.Ok)
            val (metadata, state) = (exported as EditorV2CallResult.Ok).value
            val payload = JSONObject()
                .put("platform", PLATFORM)
                .put("documentJson", JSONObject(documentJson()))
                .put("encodedStateBase64", Base64.encodeToString(state, Base64.NO_WRAP))
                .put("metadata", JSONObject(metadata))
            val directory = requireNotNull(instrumentation.targetContext.getExternalFilesDir(null))
            File(directory, EXPORT_FILE_NAME).writeText(payload.toString())
        }
    }

    @Test
    fun integratedNativeTableWorkflowKeepsTheDocumentAndRealCellSelectionAtEveryStep() {
        ActivityScenario.launch(NativeEditorOutsideTapActivity::class.java).use { scenario ->
            val harness = Harness(scenario)
            harness.create()
            try {
                runWorkflow(harness)
            } finally {
                harness.release()
            }
        }
    }

    private fun runWorkflow(harness: Harness) {
        val richCell = listOf(
            "<$STRONG_MARK>${CELL_TEXT.take(BOLD_PREFIX_LENGTH)}</>${CELL_TEXT.drop(BOLD_PREFIX_LENGTH)}",
            SECOND_PARAGRAPH
        )
        harness.onMain {
            assertTrue("the seed holds no table yet", harness.adapter.cachedTableRecords.isEmpty())
            harness.root.requestFocus()
            harness.root.setSelection(INTRO_TEXT.length)
            harness.applyLocalCommand(JSONObject().put("type", INSERT_TABLE).put("rows", TABLE_ROWS)
                .put("columns", TABLE_COLUMNS).put("withHeaderRow", true))
            val grid = harness.grid()
            assertEquals("$grid", List(TABLE_ROWS) { TABLE_COLUMNS }, grid.map { it.size })
            assertEquals(List(TABLE_COLUMNS) { HEADER_NODE }, grid[0].map { it.type })
            assertEquals(setOf(CELL_NODE), grid.drop(1).flatten().map { it.type }.toSet())
            assertEquals(PARAGRAPH_NODE, harness.blocks().first().getString("type"))
            assertEquals(TABLE_ROWS * TABLE_COLUMNS, harness.positions().size)
            assertEquals("an inserted table leaves a caret, not a cell rectangle", emptySet<Int>(),
                harness.selectedCells())
        }

        val bodyStart = harness.onMain { harness.positions()[TABLE_COLUMNS] }
        harness.tapCell(bodyStart)
        lateinit var cellInput: EditorEditText
        harness.onMain {
            cellInput = harness.view.activeTextInput
            assertFalse("the tapped cell must own the reusable cell input", cellInput === harness.root)
            assertEquals(bodyStart.toLong(), harness.activeCell())
            harness.commit(CELL_TEXT)
            cellInput.setSelection(0, BOLD_PREFIX_LENGTH)
            cellInput.performToolbarToggleMark(STRONG_MARK)
            cellInput.setSelection(CELL_TEXT.length)
            harness.commit(PARAGRAPH_BREAK)
            harness.commit(SECOND_PARAGRAPH)
            val grid = harness.grid()
            assertEquals("$grid", richCell, grid[1][0].paragraphs)
            assertSame("typing keeps the single cell input", cellInput, harness.view.activeTextInput)
            assertEquals(harness.positions()[TABLE_COLUMNS].toLong(), harness.activeCell())
        }

        val secondBody = harness.onMain { harness.positions()[TABLE_COLUMNS + 1] }
        harness.tapCell(secondBody)
        harness.onMain {
            assertSame("rebinding reuses the single cell input", cellInput, harness.view.activeTextInput)
            assertEquals(secondBody.toLong(), harness.activeCell())
            cellInput.setSelection(cellInput.text.length)
            harness.commit(BODY_TEXT)
            val positions = harness.positions()
            harness.selectCells(positions[TABLE_COLUMNS], positions[TABLE_COLUMNS + 1])
            assertEquals(setOf(positions[TABLE_COLUMNS], positions[TABLE_COLUMNS + 1]), harness.selectedCells())
            assertTrue(harness.root.onTextContextMenuItem(android.R.id.copy))
            val clipboard = it.getSystemService(Context.CLIPBOARD_SERVICE) as ClipboardManager
            assertEquals("copy exports the real cell rectangle", PASTED_TSV_ROW,
                clipboard.primaryClip?.getItemAt(0)?.coerceToText(it)?.toString())
            harness.perform(MERGE_CELLS, positions[TABLE_COLUMNS])
        }
        harness.screenshot("native-table-acceptance-merged.png")
        harness.onMain {
            val grid = harness.grid()
            assertEquals("$grid", TABLE_COLUMNS - 1, grid[1].size)
            assertEquals(2, grid[1][0].colspan)
            assertEquals(richCell + BODY_TEXT, grid[1][0].paragraphs)
            val positions = harness.positions()
            assertEquals("the merged cell is the only selected real cell", setOf(positions[TABLE_COLUMNS]),
                harness.selectedCells())
            harness.perform(SPLIT_CELL, positions[TABLE_COLUMNS])
            val split = harness.grid()
            assertEquals("$split", List(TABLE_ROWS) { TABLE_COLUMNS }, split.map { it.size })
            assertEquals("split keeps the content in the anchor", richCell + BODY_TEXT, split[1][0].paragraphs)
            assertEquals(listOf(""), split[1][1].paragraphs)
            val afterSplit = harness.positions()
            assertEquals("split keeps the content-holding top-left cell selected", setOf(afterSplit[TABLE_COLUMNS]),
                harness.selectedCells())

            harness.selectCells(afterSplit[TABLE_COLUMNS * 2], afterSplit[TABLE_COLUMNS * 2 + 1])
            assertTrue(harness.root.onTextContextMenuItem(android.R.id.paste))
            val pasted = harness.grid()
            assertEquals("paste keeps the copied rich content: $pasted", richCell, pasted[2][0].paragraphs)
            assertEquals("$pasted", listOf(BODY_TEXT), pasted[2][1].paragraphs)

            val rowAnchor = harness.positions()[TABLE_COLUMNS * 2]
            harness.selectCells(rowAnchor, rowAnchor)
            harness.perform(ADD_ROW_AFTER, rowAnchor)
            val grown = harness.grid()
            assertEquals("$grown", TABLE_ROWS + 1, grown.size)
            assertEquals(List(TABLE_COLUMNS) { listOf("") }, grown[3].map { it.paragraphs })
            val added = harness.positions()[TABLE_COLUMNS * 3]
            harness.selectCells(added, added)
            harness.perform(DELETE_ROWS, added)
            assertEquals(pasted, harness.grid())

            val lastHeader = harness.positions()[TABLE_COLUMNS - 1]
            harness.selectCells(lastHeader, lastHeader)
            harness.perform(ADD_COLUMN_AFTER, lastHeader)
            val widened = harness.grid()
            assertEquals("$widened", List(TABLE_ROWS) { TABLE_COLUMNS + 1 }, widened.map { it.size })
            assertEquals("the header row stays a header row", HEADER_NODE, widened[0][TABLE_COLUMNS].type)
            val addedHeader = harness.positions()[TABLE_COLUMNS]
            harness.selectCells(addedHeader, addedHeader)
            harness.perform(DELETE_COLUMNS, addedHeader)
            assertEquals(pasted, harness.grid())
            val target = harness.positions()[TABLE_COLUMNS * 2]
            harness.selectCells(target, target)
        }

        val settled = harness.onMain { harness.grid() }
        val (resizeTarget, bodyRow) = harness.onMain {
            harness.positions()[TABLE_COLUMNS * 2] to harness.positions()[TABLE_COLUMNS]
        }
        val edge = harness.screenPoint(bodyRow, trailingEdge = true)
        val density = instrumentation.targetContext.resources.displayMetrics.density
        harness.drag(edge, edge.first + RESIZE_DELTA_DP * density to edge.second)
        val resized = harness.onMain {
            val grid = harness.grid()
            val width = requireNotNull(grid[0][0].colwidth?.firstOrNull()) { "$grid" }
            assertTrue("the column grew: $grid", width >= MINIMUM_RESIZED_WIDTH)
            assertEquals("every cell of the column carries the width", List(TABLE_ROWS) { listOf(width) },
                grid.map { it[0].colwidth })
            assertEquals(settled.map { row -> row.map { it.paragraphs } }, grid.map { row -> row.map { it.paragraphs } })
            assertEquals("a resize keeps the selected real cell", setOf(resizeTarget), harness.selectedCells())
            assertTrue(harness.root.applyUpdateJSON(requireNotNull(harness.adapter.undo())))
            assertEquals("one undo removes the whole resize", settled, harness.grid())
            assertTrue(harness.root.applyUpdateJSON(requireNotNull(harness.adapter.redo())))
            assertEquals("redo restores the resize", grid, harness.grid())
            grid
        }

        val lastCell = harness.onMain { harness.positions().last() }
        harness.tapCell(lastCell)
        val revisionBeforeScroll = harness.onMain {
            val input = harness.view.activeTextInput
            assertEquals(lastCell.toLong(), harness.activeCell())
            assertTrue(requireNotNull(input.onCreateInputConnection(EditorInfo()))
                .setComposingText(COMPOSED_TEXT, 1))
            harness.adapter.baseDocumentRevision
        }
        harness.onMain {
            val scroll = harness.view.editorScrollView
            scroll.scrollTo(0, scroll.getChildAt(0).height)
        }
        harness.onMain {
            val viewLocation = IntArray(2)
            harness.view.getLocationOnScreen(viewLocation)
            val drawingLocation = IntArray(2)
            harness.drawing.getLocationOnScreen(drawingLocation)
            val cell = harness.drawing.presentedTableCells().firstOrNull { it.sourcePosition == lastCell }
            val cellBottom = cell?.let { drawingLocation[1] + it.bounds.bottom }
            assertTrue("the active cell must have scrolled out of the viewport: $cellBottom vs ${viewLocation[1]} " +
                "scrollY=${harness.view.editorScrollView.scrollY} content=${harness.view.editorScrollView.getChildAt(0).height} " +
                "view=${harness.view.height} rootScroll=${harness.root.scrollY} focus=${it.currentFocus}",
                cellBottom == null || cellBottom <= viewLocation[1])
            assertEquals("the offscreen active input stays pinned", lastCell.toLong(), harness.activeCell())
            assertFalse(harness.view.activeTextInput === harness.root)
            assertEquals("scrolling is not a mutation", revisionBeforeScroll, harness.adapter.baseDocumentRevision)
            assertTrue(requireNotNull(harness.view.activeTextInput.onCreateInputConnection(EditorInfo()))
                .finishComposingText())
            val grid = harness.grid()
            assertEquals("the pinned composition lands in its cell: $grid", listOf(COMPOSED_TEXT), grid[2][2].paragraphs)
            harness.view.editorScrollView.scrollTo(0, 0)
        }

        harness.onMain {
            val active = requireNotNull(harness.activeCell()).toInt()
            harness.applyRemote(JSONObject().put("selection", cellSelection(active, active))) { id, request ->
                UniffiEditorV2Backend.setSelection(id, request)
            }
            harness.applyRemote(JSONObject().put("command", JSONObject().put("type", DELETE_TABLE_ROWS))) { id, request ->
                UniffiEditorV2Backend.applyCommand(id, request)
            }
            harness.deliverRemoteCommit()
            val grid = harness.grid()
            assertEquals("the remote peer removed the active cell's row: $grid", TABLE_ROWS - 1, grid.size)
            assertEquals(resized.take(TABLE_ROWS - 1).map { row -> row.map { it.paragraphs } },
                grid.map { row -> row.map { it.paragraphs } })
            assertSame("the dead cell releases the input", harness.root, harness.view.activeTextInput)
            assertNull(harness.activeCell())
            val surviving = harness.selectedCells()
            assertTrue("only surviving real cells may stay selected: $surviving",
                harness.positions().containsAll(surviving))
        }

        harness.onMain {
            val before = harness.documentJson()
            val positions = harness.positions()
            val ltrFirst = harness.presentedCell(positions[0]).bounds
            val ltrSecond = harness.presentedCell(positions[1]).bounds
            assertTrue(ltrFirst.left < ltrSecond.left)
            harness.view.tableDirection = TableLayoutDirection.RIGHT_TO_LEFT
            harness.view.requestLayout()
            assertEquals("direction is presentation only", before, harness.documentJson())
        }
        harness.onMain {
            val positions = harness.positions()
            val rtlFirst = harness.presentedCell(positions[0]).bounds
            val rtlSecond = harness.presentedCell(positions[1]).bounds
            assertTrue("right-to-left mirrors the logical columns: $rtlFirst $rtlSecond", rtlFirst.left > rtlSecond.left)
        }
        harness.screenshot("native-table-acceptance-rtl.png")

        val beforeRemount = harness.onMain { harness.documentJson() }
        harness.onMain { activity ->
            harness.view.editorId = 0L
            harness.mount(activity)
        }
        harness.onMain {
            assertEquals("destroying the view never touches the document", beforeRemount, harness.documentJson())
            assertEquals((TABLE_ROWS - 1) * TABLE_COLUMNS, harness.positions().size)
            assertSame(harness.root, harness.view.activeTextInput)
            assertNotNull(harness.presentedCell(harness.positions()[0]))
        }
        val reboundCell = harness.onMain { harness.positions()[TABLE_COLUMNS] }
        harness.tapCell(reboundCell)
        harness.onMain {
            assertEquals(reboundCell.toLong(), harness.activeCell())
            assertTrue("table actions are available again after the rebind",
                ADD_ROW_AFTER in harness.actionIds(reboundCell))
            harness.perform(ADD_ROW_AFTER, reboundCell)
            val grid = harness.grid()
            assertEquals("$grid", TABLE_ROWS, grid.size)
            assertEquals(List(TABLE_COLUMNS) { listOf("") }, grid[2].map { it.paragraphs })
        }
        harness.screenshot("native-table-acceptance-final.png")
        harness.export()
    }

    private fun seedDocument(): String {
        val content = JSONArray().put(paragraph(INTRO_TEXT))
        repeat(TRAILING_PARAGRAPHS) { content.put(paragraph("$TRAILING_TEXT $it")) }
        return JSONObject().put("type", "doc").put("content", content).toString()
    }

    private fun paragraph(text: String): JSONObject = JSONObject().put("type", PARAGRAPH_NODE)
        .put("content", JSONArray().put(JSONObject().put("type", "text").put("text", text)))

    companion object {
        private const val CONFIG = """{"schema":{"nodes":[{"name":"doc","content":"block+","role":"doc"},{"name":"paragraph","content":"inline*","group":"block","role":"textBlock"},{"name":"text","content":"","group":"inline","role":"text"},{"name":"table","content":"table_row+","group":"block","role":"block","tableRole":"table"},{"name":"table_row","content":"(table_cell | table_header)*","role":"block","tableRole":"row"},{"name":"table_cell","content":"block+","role":"block","tableRole":"cell","attrs":{"colspan":{"type":"number","default":1,"min":1},"rowspan":{"type":"number","default":1,"min":1},"colwidth":{"default":null}}},{"name":"table_header","content":"block+","role":"block","tableRole":"header_cell","attrs":{"colspan":{"type":"number","default":1,"min":1},"rowspan":{"type":"number","default":1,"min":1},"colwidth":{"default":null}}}],"marks":[{"name":"strong"},{"name":"em"}]},"initialization":{"type":"localHtml","html":"","snapshotScope":{"documentId":"table-acceptance","lineageId":"native-editor|table-acceptance"}}}"""
        private const val THEME = """{"text":{"fontSize":17,"color":"#1b1f2aff"},"backgroundColor":"#ffffffff","table":{"borderColor":"#a0acb7ff","headerBackgroundColor":"#e8f0f5ff","minColumnWidth":80,"cellPadding":8}}"""
        private const val INTRO_TEXT = "Intro"
        private const val TRAILING_TEXT = "Trailing"
        private const val TRAILING_PARAGRAPHS = 150
        private const val TABLE_ROWS = 3
        private const val TABLE_COLUMNS = 3
        private const val CELL_TEXT = "abcdefghijkl"
        private const val BOLD_PREFIX_LENGTH = 4
        private const val SECOND_PARAGRAPH = "second"
        private const val BODY_TEXT = "Body"
        private const val PASTED_TSV_ROW = "\"abcdefghijkl\nsecond\"\tBody"
        private const val COMPOSED_TEXT = "Z"
        private const val STRONG_MARK = "strong"
        private const val PARAGRAPH_BREAK = "\n"
        private const val INSERT_TABLE = "insertTable"
        private const val DELETE_TABLE_ROWS = "deleteTableRows"
        private const val TABLE_NODE = "table"
        private const val ROW_NODE = "table_row"
        private const val CELL_NODE = "table_cell"
        private const val HEADER_NODE = "table_header"
        private const val PARAGRAPH_NODE = "paragraph"
        private const val CELL_SELECTION = "cell"
        private const val RESIZE_DELTA_DP = 60f
        private const val MINIMUM_RESIZED_WIDTH = 100
        private const val DRAG_STEPS = 8
        private const val SCREENSHOT_SETTLE_MS = 750L
        private const val GESTURE_STEP_MS = 24L
        private const val EDITOR_MARGIN_DP = 16
        private const val TOP_MARGIN_FACTOR = 4
        private const val REMOTE_REQUEST_ID_BASE = 23_000_000L
        private const val PLATFORM = "android"
        private const val EXPORT_FILE_NAME = "android-table-acceptance.json"
        private val MERGE_CELLS = R.id.table_accessibility_merge_cells
        private val SPLIT_CELL = R.id.table_accessibility_split_cell
        private val ADD_ROW_AFTER = R.id.table_accessibility_add_row_after
        private val DELETE_ROWS = R.id.table_accessibility_delete_rows
        private val ADD_COLUMN_AFTER = R.id.table_accessibility_add_column_after
        private val DELETE_COLUMNS = R.id.table_accessibility_delete_columns

        private fun cellSelection(anchor: Int, head: Int): JSONObject {
            fun point(opening: Int) = JSONObject().put("kind", "document").put("offset", opening)
            return JSONObject().put("type", CELL_SELECTION).put("anchorCell", point(anchor)).put("headCell", point(head))
        }

        private fun JSONArray.objects(): List<JSONObject> = (0 until length()).map(::getJSONObject)
    }
}
