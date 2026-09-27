package com.apollohg.editor

import android.app.Activity
import android.content.ClipboardManager
import android.content.Context
import android.graphics.Color
import android.graphics.RectF
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
import com.apollohg.editor.tables.TableCollaborationRelay
import com.apollohg.editor.tables.TableRoomSeed
import com.apollohg.editor.tables.TableToolbarTestItems
import com.apollohg.editor.tables.applyLocalSelection
import com.apollohg.editor.tables.TableAccessibilityAction
import com.apollohg.editor.tables.TableAccessibilityNodes
import com.apollohg.editor.tables.TableLayoutDirection
import com.apollohg.editor.tables.ViewerTablePresentedCell
import com.apollohg.editor.tables.activeTableCellPosition
import com.apollohg.editor.tables.documentCellSelection
import com.apollohg.editor.tables.presentedRealCell
import com.apollohg.editor.tables.pressKeyboardToolbarButton
import com.apollohg.editor.tables.required
import com.apollohg.editor.tables.selectTableCells
import com.apollohg.editor.tables.tableCellPositions
import com.apollohg.editor.viewer.PreparedProseDrawingView
import com.apollohg.editor.viewer.PreparedProseTheme
import com.apollohg.editor.viewer.ProseLayoutKey
import com.apollohg.editor.viewer.ProseViewerRequest
import com.apollohg.editor.viewer.StaticLayoutAndroidProseLayoutEngine
import com.apollohg.editor.viewer.ViewerDocument
import com.apollohg.editor.viewer.compileWithRust
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
        lateinit var expo: NativeEditorExpoView
        var token = 0L

        val view: RichTextEditorView get() = expo.richTextView
        val root: EditorEditText get() = view.editorEditText
        val drawing: PreparedProseDrawingView get() = view.editorTableSurface.drawingView

        fun <T> onMain(block: (Activity) -> T): T {
            var result: Result<T>? = null
            scenario.onActivity { activity -> result = runCatching { block(activity) } }
            instrumentation.waitForIdleSync()
            return requireNotNull(result).getOrThrow()
        }

        fun create(document: String) = onMain { activity ->
            val created = UniffiEditorV2Backend.create(CONFIG, null).required("create")
            adapter = requireNotNull(EditorV2Adapter.attach(
                UniffiEditorV2Backend, JSONObject(created).getString("editorId"), roomBound = false
            ))
            requireNotNull(adapter.setContentJson(document))
            token = EditorV2Registry.register(adapter)
            mount(activity)
        }

        fun createRoom(seed: TableRoomSeed) = onMain { activity ->
            adapter = seed.makeAdapter()
            token = EditorV2Registry.register(adapter)
            mount(activity)
        }

        fun mount(activity: Activity) {
            val context = instrumentedExpoContext(activity)
            expo = NativeEditorExpoView(context.context, context.appContext).apply {
                onFocusChangeForTesting = {}
                onAddonEventForTesting = {}
                onEditorUpdateForTesting = {}
                onEditorReadyForTesting = {}
                onSelectionChangeForTesting = {}
                onContentHeightChangeForTesting = {}
                onAtomLayoutForTesting = {}
                onTableSelectionGeometryForTesting = {}
                setThemeJson(THEME)
            }
            activity.setContentView(FrameLayout(activity).apply {
                setBackgroundColor(Color.WHITE)
                addView(expo, FrameLayout.LayoutParams(
                    ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.MATCH_PARENT
                ).apply {
                    val margin = (EDITOR_MARGIN_DP * activity.resources.displayMetrics.density).toInt()
                    setMargins(margin, margin * TOP_MARGIN_FACTOR, margin, margin)
                })
            })
            expo.setEditorId(token)
        }

        fun release() = onMain {
            expo.setEditorId(0L)
            if (token != 0L) releasePairedV2TestEditor(token)
        }

        fun tableId(): String = requireNotNull(adapter.cachedTableRecords.entries.firstOrNull {
            !it.value.optBoolean("readOnlyDescendants", true)
        }?.key) { "no editable table is rendered" }

        fun positions(): List<Int> = adapter.tableCellPositions(tableId())

        fun cellsAt(vararg indices: Int): Set<Int> = positions().let { positions -> indices.map(positions::get).toSet() }

        fun activeCell(): Long? = view.activeTableCellPosition

        fun engineSelection(): JSONObject {
            return JSONObject(UniffiEditorV2Backend.renderUpdate(adapter.editorId, null, null).required("render update"))
                .getJSONObject("selection")
        }

        fun rowWidths(): List<Int> = grid().map { row -> row.sumOf { it.colspan } }

        fun selectedCells(): Set<Int> = drawing.selectedTableCellSourcePositions[tableId()].orEmpty()

        fun presentedCell(position: Int): ViewerTablePresentedCell = drawing.presentedRealCell(tableId(), position)

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
            adapter.callWithEnvelope(JSONObject().put("command", command)) {
                UniffiEditorV2Backend.applyCommand(adapter.editorId, it)
            }.required("local command $command")
            assertTrue(root.applyUpdateJSON(requireNotNull(adapter.refreshFromRustState(null))))
        }

        fun selectCells(anchor: Int, head: Int) = root.selectTableCells(adapter, anchor, head)

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
            return info.actionList.map { it.id }.filter { id -> TableAccessibilityAction.ALL.any { it.id == id } }
        }

        fun remount() = onMain { activity ->
            expo.setEditorId(0L)
            mount(activity)
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
            val (metadata, state) = UniffiEditorV2Backend.snapshotExport(adapter.editorId).required("snapshot export")
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
            val room = TableRoomSeed(CONFIG, seedDocument())
            harness.createRoom(room)
            try {
                runWorkflow(harness, room)
            } finally {
                harness.release()
            }
        }
    }

    @Test
    fun editorAndViewerShareTableGeometryUnderOneThemeFontAndWidth() {
        ActivityScenario.launch(NativeEditorOutsideTapActivity::class.java).use { scenario ->
            val harness = Harness(scenario)
            harness.create(PARITY_DOCUMENT)
            try {
                val document = compileWithRust(ProseViewerRequest(ProseViewerSource.Json(PARITY_DOCUMENT),
                    ProseViewerConfiguration(CONFIG)))
                for (direction in TableLayoutDirection.entries) {
                    harness.onMain { harness.view.tableDirection = direction; harness.view.requestLayout() }
                    harness.onMain {
                        val editorCells = harness.drawing.presentedTableCells()
                            .filter { it.cell.sourceCellIndex != null }
                            .associate { it.sourcePosition to it.bounds }
                        val viewerCells = viewerCellFrames(harness, document, direction)
                        assertEquals("$direction", viewerCells.keys.sorted(), editorCells.keys.sorted())
                        val editorGeometry = normalized(editorCells)
                        normalized(viewerCells).forEach { (position, viewer) ->
                            val editor = requireNotNull(editorGeometry[position])
                            viewer.zip(editor).forEach { (viewerEdge, editorEdge) ->
                                assertEquals("$direction cell $position: editor $editor viewer $viewer",
                                    viewerEdge, editorEdge, PARITY_TOLERANCE_PX)
                            }
                        }
                    }
                }
            } finally {
                harness.release()
            }
        }
    }

    @Test
    fun rawIrregularImportProjectsWithoutRepairAndKeepsNativeActionsAcrossRebind() {
        ActivityScenario.launch(NativeEditorOutsideTapActivity::class.java).use { scenario ->
            val harness = Harness(scenario)
            harness.create(IRREGULAR_DOCUMENT)
            try {
                val (raw, revision, shortRow) = harness.onMain {
                    val raw = harness.grid()
                    assertEquals("the import keeps its raw row widths: $raw", RAW_ROW_WIDTHS, harness.rowWidths())
                    val record = requireNotNull(harness.adapter.cachedTableRecords[harness.tableId()])
                    assertTrue(record.getBoolean("irregular"))
                    assertTrue("the projection fills the raw gaps", record.getJSONArray("syntheticRegions").length() > 0)
                    Triple(raw, harness.adapter.baseDocumentRevision, harness.positions()[IRREGULAR_SHORT_ROW_CELL])
                }
                harness.onMain {
                    harness.selectCells(shortRow, shortRow)
                    assertEquals("a real cell of the raw table offers exactly these actions", IRREGULAR_CELL_ACTIONS,
                        harness.actionIds(shortRow))
                    assertEquals("rendering and selecting never repair the raw table", raw, harness.grid())
                    assertEquals(revision, harness.adapter.baseDocumentRevision)
                }
                harness.remount()
                harness.onMain {
                    assertEquals("rebinding never repairs the raw table", raw, harness.grid())
                    assertEquals(revision, harness.adapter.baseDocumentRevision)
                    harness.selectCells(shortRow, shortRow)
                    assertEquals("the same actions are offered after the rebind", IRREGULAR_CELL_ACTIONS,
                        harness.actionIds(shortRow))
                }
                harness.tapCell(shortRow)
                harness.onMain {
                    assertEquals(shortRow.toLong(), harness.activeCell())
                    harness.view.activeTextInput.setSelection(0)
                    harness.commit(COMPOSED_TEXT)
                    val typed = harness.grid()
                    assertEquals("a uniquely anchored real cell stays typeable: $typed",
                        listOf(COMPOSED_TEXT + SHORT_ROW_TEXT), typed[1][1].paragraphs)
                    assertEquals("typing never normalizes the grid", RAW_ROW_WIDTHS, harness.rowWidths())
                    assertEquals(revision + 1uL, harness.adapter.baseDocumentRevision)
                }
            } finally {
                harness.release()
            }
        }
    }

    private fun viewerCellFrames(
        harness: Harness,
        document: ViewerDocument,
        direction: TableLayoutDirection
    ): Map<Int, RectF> {
        val input = harness.root
        val density = input.resources.displayMetrics.density
        val width = input.measuredWidth - input.compoundPaddingLeft - input.compoundPaddingRight
        val theme = PreparedProseTheme.resolve(THEME, density).copy(insetTopPx = 0, insetRightPx = 0,
            insetBottomPx = 0, insetLeftPx = 0, tableDirection = direction)
        val key = ProseLayoutKey(document.semanticKey, width, PARITY_LAYOUT_KEY, 0, 0, density.toRawBits().toLong(), 0,
            PARITY_LAYOUT_KEY, tableDirection = direction)
        val layout = StaticLayoutAndroidProseLayoutEngine().prepare(document, key, theme, width, density, false)
        val surface = requireNotNull(layout.blocks.firstNotNullOfOrNull { it.tableSurface })
        return surface.cells.associate { cell ->
            cell.sourcePosition to RectF(cell.frame.left, cell.frame.top, cell.frame.left + cell.frame.width,
                cell.frame.top + cell.frame.height)
        }
    }

    private fun normalized(cells: Map<Int, RectF>): Map<Int, List<Float>> {
        val left = cells.values.minOf { it.left }
        val top = cells.values.minOf { it.top }
        return cells.mapValues { (_, rect) -> listOf(rect.left - left, rect.top - top, rect.width(), rect.height()) }
    }

    private fun runWorkflow(harness: Harness, room: TableRoomSeed) {
        val richCell = listOf(
            "<$STRONG_MARK>${CELL_TEXT.take(BOLD_PREFIX_LENGTH)}</>${CELL_TEXT.drop(BOLD_PREFIX_LENGTH)}",
            SECOND_PARAGRAPH
        )
        harness.onMain {
            assertEquals("the room opens on its seed", TRAILING_PARAGRAPHS + 1, harness.blocks().size)
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

        val bodyStart = harness.onMain {
            harness.expo.setToolbarItemsJson(TableToolbarTestItems.STRONG_JSON)
            harness.positions()[TABLE_COLUMNS]
        }
        harness.tapCell(bodyStart)
        lateinit var cellInput: EditorEditText
        harness.onMain {
            cellInput = harness.view.activeTextInput
            assertFalse("the tapped cell must own the reusable cell input", cellInput === harness.root)
            assertEquals(bodyStart.toLong(), harness.activeCell())
            harness.commit(CELL_TEXT)
            cellInput.setSelection(0, BOLD_PREFIX_LENGTH)
            harness.expo.pressKeyboardToolbarButton(TableToolbarTestItems.STRONG_LABEL)
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
            assertEquals("paste selects the pasted rectangle", harness.cellsAt(TABLE_COLUMNS * 2, TABLE_COLUMNS * 2 + 1),
                harness.selectedCells())

            val rowAnchor = harness.positions()[TABLE_COLUMNS * 2]
            harness.selectCells(rowAnchor, rowAnchor)
            harness.perform(ADD_ROW_AFTER, rowAnchor)
            val grown = harness.grid()
            assertEquals("$grown", TABLE_ROWS + 1, grown.size)
            assertEquals(List(TABLE_COLUMNS) { listOf("") }, grown[3].map { it.paragraphs })
            assertEquals("adding a row leaves no cell rectangle", emptySet<Int>(), harness.selectedCells())
            val added = harness.positions()[TABLE_COLUMNS * 3]
            harness.selectCells(added, added)
            harness.perform(DELETE_ROWS, added)
            assertEquals(pasted, harness.grid())
            assertEquals("deleting the selected row selects the first real cell of the row above", harness.cellsAt(TABLE_COLUMNS * 2),
                harness.selectedCells())

            val lastHeader = harness.positions()[TABLE_COLUMNS - 1]
            harness.selectCells(lastHeader, lastHeader)
            harness.perform(ADD_COLUMN_AFTER, lastHeader)
            val widened = harness.grid()
            assertEquals("$widened", List(TABLE_ROWS) { TABLE_COLUMNS + 1 }, widened.map { it.size })
            assertEquals("the header row stays a header row", HEADER_NODE, widened[0][TABLE_COLUMNS].type)
            assertEquals("adding a column leaves no cell rectangle", emptySet<Int>(), harness.selectedCells())
            val addedHeader = harness.positions()[TABLE_COLUMNS]
            harness.selectCells(addedHeader, addedHeader)
            harness.perform(DELETE_COLUMNS, addedHeader)
            assertEquals(pasted, harness.grid())
            assertEquals("deleting the selected column selects the cell before it", harness.cellsAt(TABLE_COLUMNS - 1),
                harness.selectedCells())
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
            assertEquals("undo keeps the selected real cell", setOf(resizeTarget), harness.selectedCells())
            assertTrue(harness.root.applyUpdateJSON(requireNotNull(harness.adapter.redo())))
            assertEquals("redo restores the resize", grid, harness.grid())
            assertEquals("redo keeps the selected real cell", setOf(resizeTarget), harness.selectedCells())
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
            assertTrue("the active cell must have scrolled out of the viewport: bottom $cellBottom, viewport top ${viewLocation[1]}",
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

        val remote = room.makeAdapter()
        try {
            harness.onMain {
                val relay = TableCollaborationRelay(listOf(harness.adapter.editorId, remote.editorId))
                assertEquals("catching up is a remote commit on the remote peer only, so its adapter must refresh " +
                    "before editing", setOf(remote.editorId), relay.exchangeUntilIdle())
                assertEquals("the remote peer catches up once", harness.documentJson(), remote.documentJson())
                val local = harness.engineSelection()
                assertEquals("the local selection is the caret in the active cell: $local", TEXT_SELECTION,
                    local.getString("type"))
                assertEquals(lastCell.toLong(), harness.activeCell())
                val canRedoBefore = harness.adapter.historyCanRedo()
                requireNotNull(remote.refreshFromRustState(null))
                remote.applyLocalSelection(documentCellSelection(lastCell, lastCell)).required("remote row selection")
                remote.callWithEnvelope(JSONObject().put("command", JSONObject().put("type", DELETE_TABLE_ROWS))) {
                    UniffiEditorV2Backend.applyCommand(remote.editorId, it)
                }.required("remote row delete")
                assertEquals("only the local editor receives a remote commit", setOf(harness.adapter.editorId),
                    relay.exchangeUntilIdle())
                harness.deliverRemoteCommit()
                val grid = harness.grid()
                assertEquals("the remote peer removed the active cell's row: $grid", TABLE_ROWS - 1, grid.size)
                assertEquals(resized.take(TABLE_ROWS - 1), grid)
                assertEquals("both peers converge", remote.documentJson(), harness.documentJson())
                assertSame("the dead cell releases the input", harness.root, harness.view.activeTextInput)
                assertNull(harness.activeCell())
                val tableEnd = requireNotNull(harness.adapter.cachedTableRecords[harness.tableId()]).getInt("sourceEnd")
                val resolved = harness.engineSelection()
                assertEquals("$resolved", TEXT_SELECTION, resolved.getString("type"))
                assertEquals("KNOWN DEFECT: local caret not remapped after a remote row delete: $resolved before $local",
                    local.getInt("anchor"), resolved.getInt("anchor"))
                assertEquals("KNOWN DEFECT: local caret not remapped after a remote row delete: $resolved",
                    local.getInt("head"), resolved.getInt("head"))
                assertTrue("KNOWN DEFECT: local caret not remapped after a remote row delete, so it lands after the table",
                    resolved.getInt("anchor") > tableEnd)
                assertEquals("no cell rectangle survives the remote deletion", emptySet<Int>(), harness.selectedCells())
                val afterRemote = harness.documentJson()
                assertEquals("the remote change adds no local history", canRedoBefore, harness.adapter.historyCanRedo())
                assertEquals("KNOWN DEFECT: canUndo stays true after a remote row delete removed the only local undo item",
                    true, harness.adapter.historyCanUndo())
                assertNull("KNOWN DEFECT: local undo is refused after a remote row delete while canUndo is true",
                    harness.adapter.undo())
                assertEquals("local undo never restores the remote deletion", afterRemote, harness.documentJson())
            }
        } finally {
            remote.destroy()
        }

        val rectangleBeforeDirection = harness.onMain {
            val positions = harness.positions()
            harness.selectCells(positions[0], positions[1])
            val rectangle = harness.cellsAt(0, 1)
            assertEquals(rectangle, harness.selectedCells())
            rectangle
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
            assertEquals("direction never changes the selected cells", rectangleBeforeDirection, harness.selectedCells())
        }
        harness.screenshot("native-table-acceptance-rtl.png")

        val beforeRemount = harness.onMain { harness.documentJson() }
        harness.remount()
        harness.onMain {
            assertEquals("destroying the view never touches the document", beforeRemount, harness.documentJson())
            assertEquals((TABLE_ROWS - 1) * TABLE_COLUMNS, harness.positions().size)
            assertSame(harness.root, harness.view.activeTextInput)
            assertNotNull(harness.presentedCell(harness.positions()[0]))
            val preTap = harness.engineSelection()
            assertEquals("the engine keeps the pre-remount cell rectangle, so the tap must replace it", CELL_SELECTION,
                preTap.getString("type"))
            assertEquals("$preTap", harness.positions()[0], preTap.getInt("anchorCell"))
            assertEquals("$preTap", harness.positions()[1], preTap.getInt("headCell"))
            assertEquals("the remounted view draws the pre-remount rectangle", rectangleBeforeDirection,
                harness.selectedCells())
        }
        val reboundCell = harness.onMain { harness.positions()[TABLE_COLUMNS] }
        harness.tapCell(reboundCell)
        harness.onMain {
            assertEquals(reboundCell.toLong(), harness.activeCell())
            assertEquals("tapping a cell replaces the pre-remount cell rectangle with a caret", TEXT_SELECTION,
                harness.engineSelection().getString("type"))
            assertEquals("tapping a cell clears the drawn rectangle", emptySet<Int>(), harness.selectedCells())
            assertEquals("a really tapped cell offers its single-cell table actions before any keystroke",
                SINGLE_CELL_ACTIONS, harness.actionIds(reboundCell))
            harness.commit(COMPOSED_TEXT)
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
        private const val TEXT_SELECTION = "text"
        private const val CELL_SELECTION = "cell"
        private const val PARITY_DOCUMENT = """{"type":"doc","content":[{"type":"paragraph","content":[{"type":"text","text":"Before"}]},{"type":"table","content":[{"type":"table_row","content":[{"type":"table_header","attrs":{"colspan":2},"content":[{"type":"paragraph","content":[{"type":"text","text":"Merged header across two columns"}]}]},{"type":"table_header","content":[{"type":"paragraph","content":[{"type":"text","text":"Status"}]}]}]},{"type":"table_row","content":[{"type":"table_cell","attrs":{"rowspan":2},"content":[{"type":"paragraph","content":[{"type":"text","text":"A tall cell whose text wraps over several lines"}]},{"type":"paragraph","content":[{"type":"text","text":"second"}]}]},{"type":"table_cell","content":[{"type":"paragraph","content":[{"type":"text","text":"abcdefghijkl"}]}]},{"type":"table_cell","attrs":{"colwidth":[140]},"content":[{"type":"paragraph","content":[{"type":"text","text":"Ready"}]}]}]},{"type":"table_row","content":[{"type":"table_cell","content":[{"type":"paragraph","content":[{"type":"text","text":"x"}]}]},{"type":"table_cell","content":[{"type":"paragraph"}]}]}]},{"type":"paragraph","content":[{"type":"text","text":"After"}]}]}"""
        private const val IRREGULAR_DOCUMENT = """{"type":"doc","content":[{"type":"paragraph","content":[{"type":"text","text":"Raw"}]},{"type":"table","content":[{"type":"table_row","content":[{"type":"table_header","attrs":{"colspan":2},"content":[{"type":"paragraph","content":[{"type":"text","text":"Wide header"}]}]},{"type":"table_header","content":[{"type":"paragraph","content":[{"type":"text","text":"Status"}]}]}]},{"type":"table_row","content":[{"type":"table_cell","attrs":{"rowspan":3},"content":[{"type":"paragraph","content":[{"type":"text","text":"Overhang"}]}]},{"type":"table_cell","content":[{"type":"paragraph","content":[{"type":"text","text":"Short row"}]}]}]},{"type":"table_row","content":[{"type":"table_cell","content":[{"type":"paragraph","content":[{"type":"text","text":"One"}]}]},{"type":"table_cell","content":[{"type":"paragraph","content":[{"type":"text","text":"Two"}]}]},{"type":"table_cell","content":[{"type":"paragraph","content":[{"type":"text","text":"Three"}]}]},{"type":"table_cell","content":[{"type":"paragraph","content":[{"type":"text","text":"Four"}]}]}]}]}]}"""
        private const val IRREGULAR_SHORT_ROW_CELL = 3
        private const val SHORT_ROW_TEXT = "Short row"
        private val RAW_ROW_WIDTHS = listOf(3, 2, 4)
        private const val PARITY_LAYOUT_KEY = "table-acceptance-parity"
        private const val PARITY_TOLERANCE_PX = 1f
        private const val RESIZE_DELTA_DP = 60f
        private const val MINIMUM_RESIZED_WIDTH = 100
        private const val DRAG_STEPS = 8
        private const val SCREENSHOT_SETTLE_MS = 750L
        private const val GESTURE_STEP_MS = 24L
        private const val EDITOR_MARGIN_DP = 16
        private const val TOP_MARGIN_FACTOR = 4
        private const val PLATFORM = "android"
        private const val EXPORT_FILE_NAME = "android-table-acceptance.json"
        private val MERGE_CELLS = R.id.table_accessibility_merge_cells
        private val SPLIT_CELL = R.id.table_accessibility_split_cell
        private val ADD_ROW_AFTER = R.id.table_accessibility_add_row_after
        private val DELETE_ROWS = R.id.table_accessibility_delete_rows
        private val ADD_COLUMN_AFTER = R.id.table_accessibility_add_column_after
        private val DELETE_COLUMNS = R.id.table_accessibility_delete_columns
        private val SINGLE_CELL_ACTIONS = listOf(
            R.id.table_accessibility_add_row_before,
            R.id.table_accessibility_add_row_after,
            R.id.table_accessibility_delete_rows,
            R.id.table_accessibility_select_rows,
            R.id.table_accessibility_add_column_before,
            R.id.table_accessibility_add_column_after,
            R.id.table_accessibility_delete_columns,
            R.id.table_accessibility_select_columns,
            R.id.table_accessibility_toggle_header_row,
            R.id.table_accessibility_toggle_header_column,
            R.id.table_accessibility_toggle_header_cell,
            R.id.table_accessibility_clear_cells,
            R.id.table_accessibility_delete_table
        )
        private val IRREGULAR_CELL_ACTIONS = listOf(
            R.id.table_accessibility_clear_cells,
            R.id.table_accessibility_delete_table
        )

        private fun JSONArray.objects(): List<JSONObject> = (0 until length()).map(::getJSONObject)
    }
}
