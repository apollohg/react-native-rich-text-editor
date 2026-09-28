package com.apollohg.editor

import android.app.Activity
import android.content.pm.ApplicationInfo
import android.os.Looper
import android.view.InputDevice
import android.view.MotionEvent
import android.view.View
import android.widget.FrameLayout
import com.apollohg.editor.tables.TableLayoutDirection
import com.apollohg.editor.tables.TableStyle
import com.apollohg.editor.viewer.PreparedProseDrawingView
import com.apollohg.editor.viewer.TableResizeEdge
import com.apollohg.editor.viewer.TableSelectionHandleRole
import com.apollohg.editor.tables.ViewerTablePresentedCell
import org.json.JSONArray
import org.json.JSONObject
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.Robolectric
import org.robolectric.RobolectricTestRunner
import org.robolectric.RuntimeEnvironment
import org.robolectric.Shadows
import org.robolectric.annotation.Config
import org.robolectric.annotation.LooperMode
import java.util.concurrent.TimeUnit

@RunWith(RobolectricTestRunner::class)
@Config(sdk = [34])
@LooperMode(LooperMode.Mode.PAUSED)
internal class EditorTableColumnResizeTest {
    private val config = """{"schema":{"nodes":[{"name":"doc","content":"block+","role":"doc"},{"name":"paragraph","content":"inline*","group":"block","role":"textBlock"},{"name":"text","content":"","group":"inline","role":"text"},{"name":"table","content":"table_row+","group":"block","role":"block","tableRole":"table"},{"name":"table_row","content":"(table_cell | table_header)*","role":"block","tableRole":"row"},{"name":"table_cell","content":"block+","role":"block","tableRole":"cell","attrs":{"colspan":{"type":"number","default":1,"min":1},"rowspan":{"type":"number","default":1,"min":1},"colwidth":{"default":null}}},{"name":"table_header","content":"block+","role":"block","tableRole":"header_cell","attrs":{"colspan":{"type":"number","default":1,"min":1},"rowspan":{"type":"number","default":1,"min":1},"colwidth":{"default":null}}}],"marks":[]},"initialization":{"type":"localEmpty"}}"""
    private val rtlConfig = config.replace(
        "\"tableRole\":\"table\"}",
        "\"tableRole\":\"table\",\"attrs\":{\"dir\":{\"default\":null}}}"
    )
    private val fixedWidthGrid = """{"type":"doc","content":[{"type":"table","content":[{"type":"table_row","content":[{"type":"table_cell","attrs":{"colwidth":[120]},"content":[{"type":"paragraph","content":[{"type":"text","text":"first"}]}]},{"type":"table_cell","attrs":{"colwidth":[120]},"content":[{"type":"paragraph","content":[{"type":"text","text":"second"}]}]}]},{"type":"table_row","content":[{"type":"table_cell","attrs":{"colwidth":[120]},"content":[{"type":"paragraph","content":[{"type":"text","text":"third"}]}]},{"type":"table_cell","attrs":{"colwidth":[120]},"content":[{"type":"paragraph","content":[{"type":"text","text":"fourth"}]}]}]}]}]}"""
    private val proseThenFixedWidthTable = """{"type":"doc","content":[{"type":"paragraph","content":[{"type":"text","text":"before"}]},{"type":"table","content":[{"type":"table_row","content":[{"type":"table_cell","attrs":{"colwidth":[120]},"content":[{"type":"paragraph","content":[{"type":"text","text":"first"}]}]},{"type":"table_cell","attrs":{"colwidth":[120]},"content":[{"type":"paragraph","content":[{"type":"text","text":"second"}]}]}]}]}]}"""
    private val wideTwoCellTable = """{"type":"doc","content":[{"type":"paragraph","content":[{"type":"text","text":"before"}]},{"type":"table","content":[{"type":"table_row","content":[{"type":"table_cell","attrs":{"colwidth":[500]},"content":[{"type":"paragraph","content":[{"type":"text","text":"one"}]}]},{"type":"table_cell","attrs":{"colwidth":[500]},"content":[{"type":"paragraph","content":[{"type":"text","text":"two"}]}]}]}]}]}"""

    private fun narrowColumnGrid(headerRow: Boolean, rtl: Boolean = false): String {
        fun cell(type: String, text: String) = JSONObject().put("type", type)
            .put("attrs", JSONObject().put("colwidth", JSONArray().put(NARROW_COLUMN_WIDTH)))
            .put("content", JSONArray().put(JSONObject().put("type", "paragraph")
                .put("content", JSONArray().put(JSONObject().put("type", "text").put("text", text)))))
        val rows = JSONArray()
        repeat(GRID_SIZE) { row ->
            val type = if (headerRow && row == 0) "table_header" else "table_cell"
            val cells = JSONArray()
            repeat(GRID_SIZE) { column -> cells.put(cell(type, "r${row}c$column")) }
            rows.put(JSONObject().put("type", "table_row").put("content", cells))
        }
        val table = JSONObject().put("type", "table").put("content", rows)
        if (rtl) table.put("attrs", JSONObject().put("dir", "rtl"))
        return JSONObject().put("type", "doc").put("content", JSONArray().put(table)).toString()
    }

    private fun tableDocument(rtl: Boolean, vararg rows: List<Triple<String, Int, Int>>): String {
        val content = JSONArray()
        rows.forEach { row ->
            val cells = JSONArray()
            row.forEach { (text, colspan, rowspan) ->
                val widths = JSONArray()
                repeat(colspan) { widths.put(NARROW_COLUMN_WIDTH) }
                cells.put(JSONObject().put("type", "table_cell")
                    .put("attrs", JSONObject().put("colspan", colspan).put("rowspan", rowspan).put("colwidth", widths))
                    .put("content", JSONArray().put(JSONObject().put("type", "paragraph")
                        .put("content", JSONArray().put(JSONObject().put("type", "text").put("text", text))))))
            }
            content.put(JSONObject().put("type", "table_row").put("content", cells))
        }
        val table = JSONObject().put("type", "table").put("content", content)
        if (rtl) table.put("attrs", JSONObject().put("dir", "rtl"))
        return JSONObject().put("type", "doc").put("content", JSONArray().put(table)).toString()
    }

    private fun mergedTitleRowDocument(rtl: Boolean) = tableDocument(rtl,
        listOf(Triple("Release plan", 2, 1), Triple("Status", 1, 1)),
        listOf(Triple("Core editor", 1, 2), Triple("iOS", 1, 1), Triple("Ready", 1, 1)),
        listOf(Triple("Android", 1, 1), Triple("In review", 1, 1)),
        listOf(Triple("a", 1, 1), Triple("b", 1, 1), Triple("c", 1, 1)))

    private inner class Fixture(
        val view: RichTextEditorView,
        val adapter: EditorV2Adapter,
        val drawing: PreparedProseDrawingView,
        var tableId: String,
        val positions: List<Int>
    ) {
        private var eventTime = 0L
        val published = mutableListOf<String>()
        val selections = mutableListOf<Pair<Int, Int>>()

        fun cell(index: Int): ViewerTablePresentedCell = drawing.presentedTableCells().first {
            it.surface.editorTableId == tableId &&
                it.sourceIndex == index
        }

        fun trailingEdge(index: Int): Pair<Float, Float> {
            val cell = cell(index)
            val x = if (cell.surface.isRightToLeft) cell.bounds.left else cell.bounds.right
            return x to cell.bounds.centerY()
        }

        fun columnWidths(row: Int): List<List<Int>?> {
            val table = documentObject().getJSONArray("content").let { content ->
                (0 until content.length()).map { content.getJSONObject(it) }.first { it.getString("type") == "table" }
            }
            val cells = table.getJSONArray("content").getJSONObject(row).getJSONArray("content")
            return (0 until cells.length()).map { index ->
                cells.getJSONObject(index).optJSONObject("attrs")?.optJSONArray("colwidth")?.let { widths ->
                    (0 until widths.length()).map { widths.getInt(it) }
                }
            }
        }

        fun documentObject(): JSONObject = JSONObject(requireNotNull(adapter.documentJson()))

        fun engineSelection(): JSONObject {
            val result = UniffiEditorV2Backend.renderUpdate(adapter.editorId, null, null)
            assertTrue("render update failed: $result", result is EditorV2CallResult.Ok)
            return JSONObject((result as EditorV2CallResult.Ok).value).getJSONObject("selection")
        }

        fun hostPoint(x: Float, y: Float): Pair<Float, Float> {
            var hostX = x
            var hostY = y
            var child: View = drawing
            while (child !== view) {
                val parent = child.parent as View
                hostX += child.left - parent.scrollX
                hostY += child.top - parent.scrollY
                child = parent
            }
            return hostX to hostY
        }

        fun dispatch(action: Int, x: Float, y: Float): Boolean {
            val (hostX, hostY) = hostPoint(x, y)
            return dispatchHost(action, hostX, hostY)
        }

        fun dispatchHost(action: Int, hostX: Float, hostY: Float): Boolean {
            eventTime += 16L
            val event = MotionEvent.obtain(0, eventTime, action, hostX, hostY, 0)
            event.source = InputDevice.SOURCE_TOUCHSCREEN
            return try { view.dispatchTouchEvent(event) } finally { event.recycle() }
        }

        fun secondPointerDown(x: Float, y: Float) {
            val (hostX, hostY) = hostPoint(x, y)
            eventTime += 16L
            val properties = arrayOf(0, 1).map { pointerId ->
                MotionEvent.PointerProperties().apply {
                    id = pointerId
                    toolType = MotionEvent.TOOL_TYPE_FINGER
                }
            }.toTypedArray()
            val coordinates = arrayOf(0f, 40f).map { offset ->
                MotionEvent.PointerCoords().apply {
                    this.x = hostX + offset
                    this.y = hostY
                    pressure = 1f
                    size = 1f
                }
            }.toTypedArray()
            val event = MotionEvent.obtain(0, eventTime, MotionEvent.ACTION_POINTER_DOWN or
                (1 shl MotionEvent.ACTION_POINTER_INDEX_SHIFT), 2, properties, coordinates,
                0, 0, 1f, 1f, 0, 0, InputDevice.SOURCE_TOUCHSCREEN, 0)
            try { view.dispatchTouchEvent(event) } finally { event.recycle() }
        }

        fun swipe(from: Pair<Float, Float>, distance: Float) {
            dispatch(MotionEvent.ACTION_DOWN, from.first, from.second)
            for (step in 1..SWIPE_STEPS) {
                dispatch(MotionEvent.ACTION_MOVE, from.first + distance * step / SWIPE_STEPS, from.second)
            }
            dispatch(MotionEvent.ACTION_UP, from.first + distance, from.second)
        }

        fun drag(from: Pair<Float, Float>, to: Pair<Float, Float>) {
            dispatch(MotionEvent.ACTION_DOWN, from.first, from.second)
            dispatch(MotionEvent.ACTION_MOVE, to.first, to.second)
            dispatch(MotionEvent.ACTION_UP, to.first, to.second)
        }
    }

    private fun withMountedTable(
        document: String,
        width: Int = 360,
        height: Int = 500,
        schemaConfig: String = config,
        theme: EditorTheme? = null,
        cellSelection: Pair<Int, Int>? = null,
        block: (Fixture) -> Unit
    ) {
        val created = UniffiEditorV2Backend.create(schemaConfig, null) as EditorV2CallResult.Ok
        val adapter = requireNotNull(EditorV2Adapter.attach(
            UniffiEditorV2Backend, JSONObject(created.value).getString("editorId"), false
        ))
        val token = EditorV2Registry.register(adapter)
        val activity = Robolectric.buildActivity(Activity::class.java).setup()
        try {
            val initial = requireNotNull(adapter.setContentJson(document))
            val view = RichTextEditorView(RuntimeEnvironment.getApplication())
            view.editorId = token
            assertTrue(view.editorEditText.applyUpdateJSON(initial))
            if (theme != null) view.applyTheme(theme)
            view.measure(View.MeasureSpec.makeMeasureSpec(width, View.MeasureSpec.EXACTLY),
                View.MeasureSpec.makeMeasureSpec(height, View.MeasureSpec.EXACTLY))
            view.layout(0, 0, width, height)
            val container = FrameLayout(activity.get())
            container.addView(view, FrameLayout.LayoutParams(width, height))
            activity.get().setContentView(container)
            container.measure(View.MeasureSpec.makeMeasureSpec(width, View.MeasureSpec.EXACTLY),
                View.MeasureSpec.makeMeasureSpec(height, View.MeasureSpec.EXACTLY))
            container.layout(0, 0, width, height)
            val root = adapter.tableRecordsForTesting.values.minBy { it.getInt("tablePos") }
            val cells = root.getJSONArray("cells")
            val positions = (0 until cells.length()).map { cells.getJSONObject(it).getInt("sourcePos") }
            if (cellSelection != null) {
                fun point(index: Int) = JSONObject().put("kind", "document").put("offset", positions[index])
                val selection = JSONObject().put("type", "cell")
                    .put("anchorCell", point(cellSelection.first)).put("headCell", point(cellSelection.second))
                val admitted = adapter.callWithEnvelope(JSONObject().put("selection", selection)) {
                    UniffiEditorV2Backend.setSelection(adapter.editorId, it)
                }
                assertTrue("engine rejected exact selection: $admitted", admitted is EditorV2CallResult.Ok)
                assertTrue(view.editorEditText.applyUpdateJSON(requireNotNull(adapter.refreshFromRustState(null))))
            }
            val drawing = (0 until view.editorContentFrame.childCount)
                .map { view.editorContentFrame.getChildAt(it) }
                .filterIsInstance<PreparedProseDrawingView>().single()
            val fixture = Fixture(view, adapter, drawing, root.getString("sourceId"), positions)
            view.editorEditText.editorListener = object : EditorEditText.EditorListener {
                override fun onEditorUpdate(updateJSON: String) {
                    fixture.published += updateJSON
                }
                override fun onSelectionChanged(anchor: Int, head: Int) {
                    fixture.selections += anchor to head
                }
            }
            block(fixture)
        } finally {
            activity.pause().stop().destroy()
            EditorV2Registry.remove(adapter.editorId)
            adapter.destroy()
        }
    }

    private fun assertWidth(message: String, expected: Float, actual: Float) =
        assertEquals(message, expected, actual, 0.5f)

    @Test
    @Config(qualifiers = "xhdpi")
    fun `drag previews column locally then commits one undoable width`() =
        withMountedTable(fixedWidthGrid, width = 600, cellSelection = 3 to 3) { fixture ->
            val density = fixture.view.resources.displayMetrics.density
            assertEquals("fixture must exercise dp conversion", 2f, density)
            val beforeDocument = fixture.documentObject().toString()
            val beforeRevision = fixture.adapter.baseDocumentRevision
            assertEquals(false, fixture.adapter.historyCanUndo())
            val edge = fixture.trailingEdge(0)
            val dragged = edge.first + 60f * density to edge.second
            fixture.dispatch(MotionEvent.ACTION_DOWN, edge.first, edge.second)
            fixture.dispatch(MotionEvent.ACTION_MOVE, dragged.first, dragged.second)
            assertEquals(TableResizeEdge(fixture.tableId, 0), fixture.drawing.activeTableResizeEdge)
            assertWidth("preview must remeasure the dragged column", 180f * density,
                fixture.cell(0).bounds.width())
            assertWidth("every cell covering the column follows the preview", 180f * density,
                fixture.cell(2).bounds.width())
            assertWidth("untouched column keeps its width", 120f * density, fixture.cell(1).bounds.width())
            assertWidth("neighbour starts at the previewed edge", fixture.cell(0).bounds.right,
                fixture.cell(1).bounds.left)
            assertEquals("moves must not mutate the document", beforeDocument, fixture.documentObject().toString())
            assertEquals(beforeRevision, fixture.adapter.baseDocumentRevision)
            assertEquals(false, fixture.adapter.historyCanUndo())
            assertTrue("moves must not publish updates: ${fixture.published}", fixture.published.isEmpty())

            fixture.dispatch(MotionEvent.ACTION_UP, dragged.first, dragged.second)
            assertNull(fixture.drawing.activeTableResizeEdge)
            assertEquals(listOf(listOf(180), listOf(120)), fixture.columnWidths(0))
            assertEquals(listOf(listOf(180), listOf(120)), fixture.columnWidths(1))
            assertTrue(fixture.adapter.baseDocumentRevision > beforeRevision)
            assertWidth("authoritative geometry replaces the preview", 180f * density,
                fixture.cell(0).bounds.width())
            val selection = fixture.engineSelection()
            assertEquals("explicit column resize keeps the cell selection: $selection",
                fixture.positions[3], selection.getInt("anchorCell"))
            assertEquals(fixture.positions[3], selection.getInt("headCell"))
            assertEquals(fixture.positions[3],
                JSONObject(fixture.published.last()).getJSONObject("selection").getInt("headCell"))
            assertEquals(true, fixture.adapter.historyCanUndo())

            assertTrue(fixture.view.editorEditText.applyUpdateJSON(requireNotNull(fixture.adapter.undo()) { "adapter undo returned no update" }))
            assertEquals("one undo restores the pre-resize document", beforeDocument,
                fixture.documentObject().toString())
            assertEquals(false, fixture.adapter.historyCanUndo())
            assertEquals(true, fixture.adapter.historyCanRedo())
            assertWidth("undo restores geometry", 120f * density, fixture.cell(0).bounds.width())
        }

    @Test
    fun `resize from prose caret keeps caret and adds one history entry`() =
        withMountedTable(proseThenFixedWidthTable) { fixture ->
            val root = fixture.view.editorEditText
            root.requestFocus()
            root.setSelection(2)
            root.syncCurrentSelectionToRust()
            val caret = fixture.engineSelection()
            assertEquals("text", caret.getString("type"))
            val beforeDocument = fixture.documentObject().toString()
            val edge = fixture.trailingEdge(0)
            fixture.dispatch(MotionEvent.ACTION_DOWN, edge.first, edge.second)
            fixture.dispatch(MotionEvent.ACTION_MOVE, edge.first + 40f, edge.second)
            assertEquals("preview must not move the engine selection", caret.toString(),
                fixture.engineSelection().toString())
            fixture.dispatch(MotionEvent.ACTION_UP, edge.first + 40f, edge.second)
            assertEquals(listOf(listOf(160), listOf(120)), fixture.columnWidths(0))
            assertEquals("an explicit table target leaves the prose caret alone", caret.toString(),
                fixture.engineSelection().toString())
            assertEquals(2, root.selectionStart)
            assertEquals(2, root.selectionEnd)
            assertTrue(fixture.view.activeTextInput === root)
            assertEquals("text", JSONObject(fixture.published.last()).getJSONObject("selection").getString("type"))
            assertEquals(true, fixture.adapter.historyCanUndo())
            assertTrue(root.applyUpdateJSON(requireNotNull(fixture.adapter.undo()) { "adapter undo returned no update" }))
            assertEquals(beforeDocument, fixture.documentObject().toString())
            assertEquals("one resize is one history entry", false, fixture.adapter.historyCanUndo())
            assertEquals(caret.toString(), fixture.engineSelection().toString())
        }

    @Test
    fun `rtl edge is the logical trailing edge and inverts the delta`() {
        val document = """{"type":"doc","content":[{"type":"table","attrs":{"dir":"rtl"},"content":[{"type":"table_row","content":[{"type":"table_cell","attrs":{"colwidth":[120]},"content":[{"type":"paragraph","content":[{"type":"text","text":"first"}]}]},{"type":"table_cell","attrs":{"colwidth":[120]},"content":[{"type":"paragraph","content":[{"type":"text","text":"second"}]}]}]}]}]}"""
        withMountedTable(document, schemaConfig = rtlConfig) { fixture ->
            val first = fixture.cell(0)
            val second = fixture.cell(1)
            assertTrue("logical column 0 renders at the right in RTL",
                second.bounds.right <= first.bounds.left + 0.5f)
            assertEquals(TableResizeEdge(fixture.tableId, 0),
                fixture.drawing.hitResizeEdge(first.bounds.left, first.bounds.centerY()))
            assertNull("the table's physical right border is not a trailing edge in RTL",
                fixture.drawing.hitResizeEdge(first.bounds.right - 1f, first.bounds.centerY()))
            val edge = fixture.trailingEdge(0)
            fixture.dispatch(MotionEvent.ACTION_DOWN, edge.first, edge.second)
            fixture.dispatch(MotionEvent.ACTION_MOVE, edge.first - 40f, edge.second)
            assertWidth("dragging toward the physical left widens the RTL column", 160f,
                fixture.cell(0).bounds.width())
            assertWidth("sibling keeps its width", 120f, fixture.cell(1).bounds.width())
            assertTrue(fixture.cell(1).bounds.right <= fixture.cell(0).bounds.left + 0.5f)
            fixture.dispatch(MotionEvent.ACTION_UP, edge.first - 40f, edge.second)
            assertEquals(listOf(listOf(160), listOf(120)), fixture.columnWidths(0))
        }
    }

    @Test
    fun `merged cell trailing edge resizes its last covered column`() {
        val document = """{"type":"doc","content":[{"type":"table","content":[{"type":"table_row","content":[{"type":"table_cell","attrs":{"colspan":2,"colwidth":[100,100]},"content":[{"type":"paragraph","content":[{"type":"text","text":"wide"}]}]}]},{"type":"table_row","content":[{"type":"table_cell","attrs":{"colwidth":[100]},"content":[{"type":"paragraph","content":[{"type":"text","text":"left"}]}]},{"type":"table_cell","attrs":{"colwidth":[100]},"content":[{"type":"paragraph","content":[{"type":"text","text":"right"}]}]}]}]}]}"""
        withMountedTable(document) { fixture ->
            val merged = fixture.cell(0)
            assertNull("a merged cell has no edge where column 0 ends",
                fixture.drawing.hitResizeEdge(merged.bounds.left + 100f, merged.bounds.centerY()))
            assertEquals(1, fixture.drawing.hitResizeEdge(merged.bounds.right, merged.bounds.centerY())?.column)
            val edge = fixture.trailingEdge(0)
            fixture.dispatch(MotionEvent.ACTION_DOWN, edge.first, edge.second)
            fixture.dispatch(MotionEvent.ACTION_MOVE, edge.first + 30f, edge.second)
            assertWidth("merged cell spans the widened column", 230f, fixture.cell(0).bounds.width())
            assertWidth("first column is untouched", 100f, fixture.cell(1).bounds.width())
            assertWidth("last covered column is widened", 130f, fixture.cell(2).bounds.width())
            fixture.dispatch(MotionEvent.ACTION_UP, edge.first + 30f, edge.second)
            assertEquals(listOf(listOf(100, 130)), fixture.columnWidths(0))
            assertEquals(listOf(listOf(100), listOf(130)), fixture.columnWidths(1))
        }
    }

    @Test
    fun `document reset and owner loss cancel a held resize without mutating`() =
        withMountedTable(fixedWidthGrid) { fixture ->
            val edge = fixture.trailingEdge(0)
            fixture.dispatch(MotionEvent.ACTION_DOWN, edge.first, edge.second)
            fixture.dispatch(MotionEvent.ACTION_MOVE, edge.first + 60f, edge.second)
            assertWidth("preview is active before the reset", 180f, fixture.cell(0).bounds.width())
            val beforeResetRevision = fixture.adapter.baseDocumentRevision
            assertTrue(fixture.view.editorEditText.applyUpdateJSON(
                replaceTableDocumentExternallyForTest(fixture.adapter, fixedWidthGrid.replace("first", "fresh"))))
            assertNull("a document reset discards the preview", fixture.drawing.activeTableResizeEdge)
            fixture.tableId = fixture.adapter.tableIndex.rootExtents.keys.single()
            assertWidth("reset restores authoritative geometry", 120f, fixture.cell(0).bounds.width())
            val resetRevision = fixture.adapter.baseDocumentRevision
            assertTrue("the reset must advance the revision", resetRevision > beforeResetRevision)
            fixture.dispatch(MotionEvent.ACTION_MOVE, edge.first + 80f, edge.second)
            fixture.dispatch(MotionEvent.ACTION_UP, edge.first + 80f, edge.second)
            assertEquals(listOf(listOf(120), listOf(120)), fixture.columnWidths(0))
            assertEquals(resetRevision, fixture.adapter.baseDocumentRevision)

            val secondEdge = fixture.trailingEdge(0)
            fixture.dispatch(MotionEvent.ACTION_DOWN, secondEdge.first, secondEdge.second)
            fixture.dispatch(MotionEvent.ACTION_MOVE, secondEdge.first + 60f, secondEdge.second)
            assertWidth("second preview is active", 180f, fixture.cell(0).bounds.width())
            val beforeDocument = fixture.documentObject().toString()
            val owner = requireNotNull(fixture.adapter.currentNativeOwnerToken) { "fixture must hold a native owner" }
            fixture.adapter.releaseNativeBindingOwner(owner)
            fixture.dispatch(MotionEvent.ACTION_UP, secondEdge.first + 60f, secondEdge.second)
            assertNull(fixture.drawing.activeTableResizeEdge)
            assertEquals("a lost owner must not commit", beforeDocument, fixture.documentObject().toString())
            assertEquals(resetRevision, fixture.adapter.baseDocumentRevision)
        }

    @Test
    fun `read only composition and active cell input govern resize admission`() =
        withMountedTable(fixedWidthGrid) { fixture ->
            val root = fixture.view.editorEditText
            val edge = fixture.trailingEdge(0)
            root.isEditable = false
            fixture.drag(edge, edge.first + 60f to edge.second)
            assertNull(fixture.drawing.activeTableResizeEdge)
            assertEquals("read-only editors never resize", listOf(listOf(120), listOf(120)),
                fixture.columnWidths(0))
            root.isEditable = true

            fixture.dispatch(MotionEvent.ACTION_DOWN, edge.first, edge.second)
            fixture.dispatch(MotionEvent.ACTION_MOVE, edge.first + 60f, edge.second)
            assertWidth("preview is active before composition", 180f, fixture.cell(0).bounds.width())
            root.externalTextComposition = ExternalTextCompositionState(
                "pending", "x", 0, 0, root.lastAuthorizedText, root.lastAuthorizedRenderedText)
            fixture.dispatch(MotionEvent.ACTION_MOVE, edge.first + 70f, edge.second)
            assertNull("composition cancels a held resize", fixture.drawing.activeTableResizeEdge)
            assertWidth("composition cancel restores geometry", 120f, fixture.cell(0).bounds.width())
            root.externalTextComposition = null
            fixture.dispatch(MotionEvent.ACTION_UP, edge.first + 70f, edge.second)
            assertEquals(listOf(listOf(120), listOf(120)), fixture.columnWidths(0))

            val content = fixture.cell(0).contentBounds
            fixture.dispatch(MotionEvent.ACTION_DOWN, content.left + 8f, content.top + 8f)
            fixture.dispatch(MotionEvent.ACTION_UP, content.left + 8f, content.top + 8f)
            val input = fixture.view.activeTextInput
            assertTrue("tapping the cell binds its input", input !== root)
            val frame = input.layoutParams as FrameLayout.LayoutParams
            val inputWidth = frame.width
            val insideInput = (frame.leftMargin + frame.width - 2f - fixture.drawing.left) to
                (frame.topMargin + frame.height / 2f - fixture.drawing.top)
            assertNotNull("fixture point must be near a resize edge",
                fixture.drawing.hitResizeEdge(insideInput.first, insideInput.second))
            fixture.drag(insideInput, insideInput.first + 50f to insideInput.second)
            assertEquals("touches inside the active cell input stay text gestures",
                listOf(listOf(120), listOf(120)), fixture.columnWidths(0))
            assertTrue(fixture.view.activeTextInput === input)

            val border = fixture.trailingEdge(0)
            fixture.dispatch(MotionEvent.ACTION_DOWN, border.first, border.second)
            fixture.dispatch(MotionEvent.ACTION_MOVE, border.first + 50f, border.second)
            assertWidth("preview follows the drag", 170f, fixture.cell(0).bounds.width())
            assertEquals("the active input follows the previewed cell", inputWidth + 50,
                input.layoutParams.width)
            fixture.dispatch(MotionEvent.ACTION_UP, border.first + 50f, border.second)
            assertEquals(listOf(listOf(170), listOf(120)), fixture.columnWidths(0))
            assertTrue("committing keeps the bound cell input", fixture.view.activeTextInput === input)
            assertEquals(inputWidth + 50, input.layoutParams.width)
        }

    @Test
    fun `handle drag reports the cell rectangle through the selection listener`() =
        withMountedTable(fixedWidthGrid, cellSelection = 0 to 0) { fixture ->
            val head = fixture.drawing.selectionHandles().single { it.role == TableSelectionHandleRole.HEAD }
            val target = fixture.cell(3).bounds

            fixture.drag(head.x to head.y, target.centerX() to target.centerY())

            assertEquals("the listener receives the new cell endpoints in document positions",
                listOf(fixture.positions[0] to fixture.positions[3]), fixture.selections)
            val state = JSONObject(requireNotNull(fixture.adapter.currentStateJson()))
                .getJSONObject("selection")
            assertEquals("the state published beside the event keeps the rectangle",
                listOf("cell", fixture.positions[0], fixture.positions[3]),
                listOf(state.getString("type"), state.getInt("anchorCell"), state.getInt("headCell")))
        }

    @Test
    fun `host table direction mirrors undeclared tables and yields to a declared direction`() {
        withMountedTable(fixedWidthGrid) { fixture ->
            assertTrue("an undeclared table defaults to LTR", fixture.cell(0).bounds.right <= fixture.cell(1).bounds.left + 0.5f)

            fixture.view.tableDirection = TableLayoutDirection.RIGHT_TO_LEFT

            assertTrue("the host direction lays logical column 0 out on the right",
                fixture.cell(0).surface.isRightToLeft &&
                    fixture.cell(1).bounds.right <= fixture.cell(0).bounds.left + 0.5f)
            val edge = fixture.trailingEdge(0)
            assertEquals(TableResizeEdge(fixture.tableId, 0),
                fixture.drawing.hitResizeEdge(edge.first, edge.second))

            fixture.view.tableDirection = null

            assertTrue("clearing the host direction restores LTR",
                !fixture.cell(0).surface.isRightToLeft &&
                    fixture.cell(0).bounds.right <= fixture.cell(1).bounds.left + 0.5f)
        }
        val declaredLtr = fixedWidthGrid.replaceFirst("{\"type\":\"table\",", "{\"type\":\"table\",\"attrs\":{\"dir\":\"ltr\"},")
        withMountedTable(declaredLtr, schemaConfig = rtlConfig) { fixture ->
            fixture.view.tableDirection = TableLayoutDirection.RIGHT_TO_LEFT

            assertTrue("a declared table direction outranks the host direction",
                !fixture.cell(0).surface.isRightToLeft &&
                    fixture.cell(0).bounds.right <= fixture.cell(1).bounds.left + 0.5f)
        }
    }

    @Test
    fun `host view layout direction mirrors undeclared tables unless the host prop overrides it`() {
        val applicationInfo = RuntimeEnvironment.getApplication().applicationInfo
        val originalFlags = applicationInfo.flags
        applicationInfo.flags = originalFlags or ApplicationInfo.FLAG_SUPPORTS_RTL
        try {
            withRtlHostView()
        } finally {
            applicationInfo.flags = originalFlags
        }
    }

    private fun withRtlHostView() =
        withMountedTable(fixedWidthGrid) { fixture ->
            fixture.view.layoutDirection = View.LAYOUT_DIRECTION_RTL
            fixture.view.measure(View.MeasureSpec.makeMeasureSpec(fixture.view.width, View.MeasureSpec.EXACTLY),
                View.MeasureSpec.makeMeasureSpec(fixture.view.height, View.MeasureSpec.EXACTLY))
            fixture.view.layout(0, 0, fixture.view.width, fixture.view.height)

            assertEquals(View.LAYOUT_DIRECTION_RTL, fixture.view.layoutDirection)
            assertTrue("an RTL host view mirrors an undeclared table",
                fixture.cell(0).surface.isRightToLeft &&
                    fixture.cell(1).bounds.right <= fixture.cell(0).bounds.left + 0.5f)

            fixture.view.tableDirection = TableLayoutDirection.LEFT_TO_RIGHT

            assertTrue("a host ltr prop outranks the RTL view direction",
                !fixture.cell(0).surface.isRightToLeft &&
                    fixture.cell(0).bounds.right <= fixture.cell(1).bounds.left + 0.5f)
        }

    @Test
    fun `selection handle takes precedence over a shared trailing edge`() =
        withMountedTable(fixedWidthGrid, cellSelection = 0 to 2) { fixture ->
            val head = fixture.drawing.selectionHandles().single { it.role == TableSelectionHandleRole.HEAD }
            val corner = fixture.cell(2).bounds.right to head.y
            assertNotNull("the handle sits on a selected column's body-row resize edge",
                fixture.drawing.hitResizeEdge(corner.first, corner.second))
            assertNotNull(fixture.drawing.hitSelectionHandle(corner.first, corner.second))
            val target = fixture.cell(3).bounds
            fixture.drag(corner, target.centerX() to target.centerY())
            assertEquals("the handle drag extended the selection", fixture.positions[3],
                fixture.engineSelection().getInt("headCell"))
            assertEquals("the shared edge did not resize", listOf(listOf(120), listOf(120)),
                fixture.columnWidths(0))

            val edge = fixture.trailingEdge(0)
            assertNull(fixture.drawing.hitSelectionHandle(edge.first, edge.second))
            fixture.drag(edge, edge.first + 40f to edge.second)
            assertEquals(listOf(listOf(160), listOf(120)), fixture.columnWidths(1))
            val selection = fixture.engineSelection()
            assertEquals(fixture.positions[0], selection.getInt("anchorCell"))
            assertEquals(fixture.positions[3], selection.getInt("headCell"))
        }

    @Test
    fun `wide table edge is actionable only when visible and keeps its scroll anchor`() =
        withMountedTable(wideTwoCellTable) { fixture ->
            val offscreen = fixture.trailingEdge(0)
            assertNull("an edge past the host viewport is not grabbable",
                fixture.drawing.hitResizeEdge(offscreen.first, offscreen.second))
            val surface = fixture.cell(0).surface
            fixture.drawing.setTableLogicalOffset(surface.identity, 300f)
            val edge = fixture.trailingEdge(0)
            fixture.dispatch(MotionEvent.ACTION_DOWN, edge.first, edge.second)
            fixture.dispatch(MotionEvent.ACTION_MOVE, edge.first + 50f, edge.second)
            assertWidth("preview widens the column", 550f, fixture.cell(0).bounds.width())
            assertEquals("the table did not steal the horizontal drag", 300f,
                requireNotNull(fixture.drawing.tableLogicalOffset(fixture.tableId)), 1f)
            fixture.dispatch(MotionEvent.ACTION_UP, edge.first + 50f, edge.second)
            assertEquals(listOf(listOf(550), listOf(500)), fixture.columnWidths(0))
            assertEquals("the logical scroll anchor survives the width change", 300f,
                requireNotNull(fixture.drawing.tableLogicalOffset(fixture.tableId)), 1f)
        }

    @Test
    fun `stationary pointer at the table edge autoscrolls and grows the column`() =
        withMountedTable(wideTwoCellTable) { fixture ->
            val surface = fixture.cell(0).surface
            fixture.drawing.setTableLogicalOffset(surface.identity, 300f)
            val clip = requireNotNull(fixture.drawing.selectedTableViewport(fixture.tableId))
            val edge = fixture.trailingEdge(0)
            val held = clip.right - 6f
            val fingerDelta = held - edge.first
            fixture.dispatch(MotionEvent.ACTION_DOWN, edge.first, edge.second)
            fixture.dispatch(MotionEvent.ACTION_MOVE, held, edge.second)
            assertWidth("finger delta previews first", 500f + fingerDelta, fixture.cell(0).bounds.width())
            val looper = Shadows.shadowOf(Looper.getMainLooper())
            repeat(64) {
                if (requireNotNull(fixture.drawing.tableLogicalOffset(fixture.tableId)) < 340f) looper.runOneTask()
            }
            val scrolled = requireNotNull(fixture.drawing.tableLogicalOffset(fixture.tableId)) - 300f
            assertTrue("a held pointer at the table edge scrolls the table: scrolled=$scrolled", scrolled >= 20f)
            assertWidth("scrolled distance keeps growing the column under a stationary finger",
                500f + fingerDelta + scrolled, fixture.cell(0).bounds.width())
            assertEquals("autoscroll must not mutate the document", listOf(listOf(500), listOf(500)),
                fixture.columnWidths(0))
            fixture.dispatch(MotionEvent.ACTION_CANCEL, held, edge.second)
            assertNull(fixture.drawing.activeTableResizeEdge)
            assertWidth("cancel restores authoritative geometry", 500f, fixture.cell(0).bounds.width())
            val stopped = requireNotNull(fixture.drawing.tableLogicalOffset(fixture.tableId))
            looper.idleFor(250, TimeUnit.MILLISECONDS)
            assertEquals(stopped, requireNotNull(fixture.drawing.tableLogicalOffset(fixture.tableId)), 0.5f)
            assertEquals(listOf(listOf(500), listOf(500)), fixture.columnWidths(0))
        }

    @Test
    fun `frame step drops a resize whose owner is gone and restores geometry`() =
        withMountedTable(wideTwoCellTable) { fixture ->
            val surface = fixture.cell(0).surface
            fixture.drawing.setTableLogicalOffset(surface.identity, 300f)
            val clip = requireNotNull(fixture.drawing.selectedTableViewport(fixture.tableId))
            val edge = fixture.trailingEdge(0)
            fixture.dispatch(MotionEvent.ACTION_DOWN, edge.first, edge.second)
            fixture.dispatch(MotionEvent.ACTION_MOVE, clip.right - 6f, edge.second)
            assertTrue(fixture.cell(0).bounds.width() > 500.5f)
            val before = fixture.documentObject().toString()
            val release = fixture.hostPoint(clip.right - 6f, edge.second)
            fixture.adapter.releaseNativeBindingOwner(requireNotNull(fixture.adapter.currentNativeOwnerToken))
            val looper = Shadows.shadowOf(Looper.getMainLooper())
            looper.runOneTask()
            assertNull("the frame step must drop a drag whose owner is gone",
                fixture.drawing.activeTableResizeEdge)
            assertTrue("a frame-step cancel must not leave previewed widths on screen",
                fixture.drawing.presentedTableCells().none { it.bounds.width() > 500.5f })
            looper.idleFor(250, TimeUnit.MILLISECONDS)
            fixture.dispatchHost(MotionEvent.ACTION_UP, release.first, release.second)
            assertEquals(before, fixture.documentObject().toString())
            assertEquals(listOf(listOf(500), listOf(500)), fixture.columnWidths(0))
        }

    @Test
    fun `no net movement commits nothing and shrinking clamps to the ceiled theme minimum`() =
        withMountedTable(fixedWidthGrid, theme = EditorTheme(table = TableStyle(minColumnWidth = 72.5f))) { fixture ->
            val beforeDocument = fixture.documentObject().toString()
            val beforeRevision = fixture.adapter.baseDocumentRevision
            val edge = fixture.trailingEdge(0)
            fixture.dispatch(MotionEvent.ACTION_DOWN, edge.first, edge.second)
            fixture.dispatch(MotionEvent.ACTION_MOVE, edge.first + 60f, edge.second)
            assertWidth("preview grows", 180f, fixture.cell(0).bounds.width())
            fixture.dispatch(MotionEvent.ACTION_MOVE, edge.first, edge.second)
            fixture.dispatch(MotionEvent.ACTION_UP, edge.first, edge.second)
            assertEquals(beforeDocument, fixture.documentObject().toString())
            assertEquals(beforeRevision, fixture.adapter.baseDocumentRevision)
            assertEquals(false, fixture.adapter.historyCanUndo())
            assertWidth("no-op release leaves authoritative geometry", 120f, fixture.cell(0).bounds.width())

            fixture.drag(edge, edge.first - 200f to edge.second)
            assertEquals("shrink clamps to ceil(72.5)", listOf(listOf(73), listOf(120)), fixture.columnWidths(0))
            assertEquals(listOf(listOf(73), listOf(120)), fixture.columnWidths(1))
        }

    @Test
    fun `nested table borders and synthetic gaps are never resize targets`() {
        val nested = """{"type":"doc","content":[{"type":"table","content":[{"type":"table_row","content":[{"type":"table_cell","attrs":{"colwidth":[200]},"content":[{"type":"table","content":[{"type":"table_row","content":[{"type":"table_cell","content":[{"type":"paragraph","content":[{"type":"text","text":"in"}]}]},{"type":"table_cell","content":[{"type":"paragraph","content":[{"type":"text","text":"ner"}]}]}]}]}]},{"type":"table_cell","attrs":{"colwidth":[120]},"content":[{"type":"paragraph","content":[{"type":"text","text":"outer"}]}]}]}]}]}"""
        withMountedTable(nested) { fixture ->
            val nestedCells = fixture.drawing.presentedTableCells().filter {
                it.surface.editorTableId != fixture.tableId
            }
            assertEquals(2, nestedCells.size)
            val inner = nestedCells.minBy { it.bounds.left }
            val point = inner.bounds.right to inner.bounds.centerY()
            assertNull("a nested table border is not a resize edge",
                fixture.drawing.hitResizeEdge(point.first, point.second))
            val before = fixture.documentObject().toString()
            fixture.drag(point, point.first + 40f to point.second)
            assertEquals("dragging a nested border mutates nothing", before, fixture.documentObject().toString())
            val outer = fixture.cell(0)
            assertEquals(TableResizeEdge(fixture.tableId, 0),
                fixture.drawing.hitResizeEdge(outer.bounds.right, outer.bounds.centerY()))
        }
        val irregular = """{"type":"doc","content":[{"type":"table","content":[{"type":"table_row","content":[{"type":"table_cell","attrs":{"rowspan":2,"colwidth":[100]},"content":[{"type":"paragraph","content":[{"type":"text","text":"tall"}]}]},{"type":"table_cell","attrs":{"colspan":2,"colwidth":[100,100]},"content":[{"type":"paragraph","content":[{"type":"text","text":"wide"}]}]}]},{"type":"table_row","content":[{"type":"table_cell","attrs":{"colwidth":[100]},"content":[{"type":"paragraph","content":[{"type":"text","text":"later"}]}]}]}]}]}"""
        withMountedTable(irregular, cellSelection = 1 to 2) { fixture ->
            assertTrue("the irregular fixture must project a synthetic gap",
                requireNotNull(fixture.adapter.tableIndex.record(fixture.tableId)).syntheticRegions.isNotEmpty())
            val wide = fixture.cell(1)
            val later = fixture.cell(2)
            assertTrue("the gap sits after the last real cell of row 1", wide.bounds.right > later.bounds.right)
            assertNull("the gap's outer border is not a resize edge",
                fixture.drawing.hitResizeEdge(wide.bounds.right, later.bounds.centerY()))
            assertNull("the gap interior is not a resize edge",
                fixture.drawing.hitResizeEdge((later.bounds.right + wide.bounds.right) / 2f, later.bounds.centerY()))
            assertEquals(2, fixture.drawing.hitResizeEdge(wide.bounds.right, wide.bounds.centerY())?.column)
            assertEquals(1, fixture.drawing.hitResizeEdge(later.bounds.right, later.bounds.centerY())?.column)
            val tall = fixture.cell(0)
            assertEquals(0, fixture.drawing.hitResizeEdge(tall.bounds.right, wide.bounds.centerY())?.column)
            assertNull("a first-row cell spanning into an unselected body row has no handle below the first row",
                fixture.drawing.hitResizeEdge(tall.bounds.right, later.bounds.centerY()))
        }
    }

    @Test
    fun `only the first row carries resize handles whether or not it is a header row`() {
        listOf(true, false).forEach { headerRow ->
            withMountedTable(narrowColumnGrid(headerRow)) { fixture ->
                val label = if (headerRow) "header row" else "first row of a table without a header row"
                val handle = fixture.trailingEdge(0)
                assertEquals("the $label edge is a handle", TableResizeEdge(fixture.tableId, 0),
                    fixture.drawing.hitResizeEdge(handle.first, handle.second))
                fixture.drag(handle, handle.first + 40f to handle.second)
                assertEquals("the $label edge resizes", listOf(listOf(NARROW_COLUMN_WIDTH + 40)),
                    fixture.columnWidths(0).take(1))

                val body = fixture.trailingEdge(GRID_SIZE)
                assertNull("a body-row edge below the $label is not a handle",
                    fixture.drawing.hitResizeEdge(body.first, body.second))
                val before = fixture.documentObject().toString()
                fixture.swipe(body, -BODY_SWIPE_DISTANCE)
                assertNull(fixture.drawing.activeTableResizeEdge)
                assertEquals("a drag on a body-row edge mutates nothing", before, fixture.documentObject().toString())
                assertTrue("a drag on a body-row edge scrolls the table, offset=" +
                    fixture.drawing.tableLogicalOffset(fixture.tableId),
                    requireNotNull(fixture.drawing.tableLogicalOffset(fixture.tableId)) > 0f)
            }
        }
    }

    @Test
    fun `a column edge hidden by a merged first row takes its handle from the first row where it exists`() {
        listOf(false, true).forEach { rtl ->
            withMountedTable(mergedTitleRowDocument(rtl), schemaConfig = if (rtl) rtlConfig else config) { fixture ->
                val label = if (rtl) "RTL" else "LTR"
                val edgeX = fixture.trailingEdge(MERGED_CORE_EDITOR).first
                fun hit(cell: Int) = fixture.drawing.hitResizeEdge(edgeX, fixture.cell(cell).bounds.centerY())
                assertNull("$label: the merged title cell has no edge where column 0 ends", hit(MERGED_RELEASE_PLAN))
                assertEquals("$label: column 0's edge first exists in row 1, so row 1 carries its handle",
                    TableResizeEdge(fixture.tableId, 0), hit(MERGED_ROW_ONE_NEIGHBOUR))
                assertNull("$label: the spanning cell below row 1 is not a handle", hit(MERGED_ROW_TWO_NEIGHBOUR))
                assertNull("$label: a later body row is not a handle", hit(MERGED_LAST_ROW_FIRST_CELL))
                val titleEdge = fixture.trailingEdge(MERGED_RELEASE_PLAN)
                assertEquals("$label: the merged title's own trailing edge keeps its first-row handle", 1,
                    fixture.drawing.hitResizeEdge(titleEdge.first, titleEdge.second)?.column)
                val start = edgeX to fixture.cell(MERGED_ROW_ONE_NEIGHBOUR).bounds.centerY()
                fixture.drag(start, start.first + (if (rtl) -40f else 40f) to start.second)
                assertEquals("$label: row 1 resizes column 0", listOf(NARROW_COLUMN_WIDTH + 40),
                    fixture.columnWidths(1).first())
            }
        }
    }

    @Test
    fun `a merged cell that grows the selection to the first row is not a column selection`() {
        val document = tableDocument(false,
            listOf(Triple("a", 1, 1), Triple("b", 1, 2)),
            listOf(Triple("c", 1, 1)),
            listOf(Triple("d", 1, 1), Triple("e", 1, 1)))
        withMountedTable(document, cellSelection = 4 to 2) { fixture ->
            assertTrue("the merged cell must grow the drawn rectangle to the first row",
                fixture.positions[0] in fixture.drawing.selectedTableCellSourceIndices[fixture.tableId].orEmpty())
            listOf(2, 3).forEach { index ->
                val edge = fixture.trailingEdge(index)
                assertNull("body cell $index: endpoints below the first row do not make a column selection",
                    fixture.drawing.hitResizeEdge(edge.first, edge.second))
            }
        }
    }

    @Test
    fun `a whole selected column carries resize handles in every row`() {
        val middleColumn = listOf(1, GRID_SIZE + 1, 2 * GRID_SIZE + 1)
        withMountedTable(narrowColumnGrid(headerRow = true),
            cellSelection = middleColumn.first() to middleColumn.last()) { fixture ->
            middleColumn.drop(1).forEach { index ->
                val edge = fixture.trailingEdge(index)
                assertEquals("the selected column's edge in body cell $index is a handle",
                    TableResizeEdge(fixture.tableId, 1), fixture.drawing.hitResizeEdge(edge.first, edge.second))
            }
            val unselected = fixture.trailingEdge(GRID_SIZE)
            assertNull("an unselected column's body edge is not a handle",
                fixture.drawing.hitResizeEdge(unselected.first, unselected.second))
            val edge = fixture.trailingEdge(middleColumn[1])
            fixture.drag(edge, edge.first + 40f to edge.second)
            assertEquals("a selected column resizes from a body row",
                listOf(listOf(NARROW_COLUMN_WIDTH), listOf(NARROW_COLUMN_WIDTH + 40), listOf(NARROW_COLUMN_WIDTH)),
                fixture.columnWidths(GRID_SIZE - 1))
        }
        withMountedTable(narrowColumnGrid(headerRow = true),
            cellSelection = middleColumn[1] to middleColumn.last()) { fixture ->
            val edge = fixture.trailingEdge(middleColumn[1])
            assertNull("a selection that does not reach the first row is not a column selection",
                fixture.drawing.hitResizeEdge(edge.first, edge.second))
        }
    }

    @Test
    fun `rtl body rows scroll while the first row keeps its logical trailing handles`() =
        withMountedTable(narrowColumnGrid(headerRow = false, rtl = true), schemaConfig = rtlConfig) { fixture ->
            assertTrue("the first logical column renders at the right in RTL",
                fixture.cell(1).bounds.right <= fixture.cell(0).bounds.left + 0.5f)
            val handle = fixture.trailingEdge(0)
            assertEquals(TableResizeEdge(fixture.tableId, 0), fixture.drawing.hitResizeEdge(handle.first, handle.second))
            fixture.drag(handle, handle.first - 40f to handle.second)
            assertEquals(listOf(listOf(NARROW_COLUMN_WIDTH + 40)), fixture.columnWidths(0).take(1))
            val body = fixture.trailingEdge(GRID_SIZE)
            assertNull("an RTL body-row edge is not a handle", fixture.drawing.hitResizeEdge(body.first, body.second))
            val before = fixture.documentObject().toString()
            val initialOffset = requireNotNull(fixture.drawing.tableLogicalOffset(fixture.tableId))
            fixture.swipe(body, BODY_SWIPE_DISTANCE)
            assertNull(fixture.drawing.activeTableResizeEdge)
            assertEquals(before, fixture.documentObject().toString())
            assertTrue("an RTL body-row drag scrolls the table",
                requireNotNull(fixture.drawing.tableLogicalOffset(fixture.tableId)) > initialOffset)
        }

    @Test
    fun `owned paste in a table document returns an applicable update and undoes through the adapter`() =
        withMountedTable(proseThenFixedWidthTable) { fixture ->
            val root = fixture.view.editorEditText
            assertNotNull("fixture must hold a native owner", fixture.adapter.nativeOwnerId)
            val before = fixture.documentObject().toString()
            val pasted = fixture.adapter.pasteAtSelection(null, null, "X", true, 2, 2)
            assertNotNull("owned paste must return an update: notes=${fixture.adapter.debugNotes}", pasted)
            assertTrue(root.applyUpdateJSON(requireNotNull(pasted)))
            assertEquals("beXfore", root.text.toString().substringBefore('\n').trim { it.isWhitespace() || it == '\uFFFC' })
            assertEquals(fixture.adapter.baseDocumentRevision.toString(), root.lastAppliedDocumentVersion)
            assertEquals(fixture.adapter.baseDocumentRevision, fixture.adapter.cachedAtomicRenderDocumentRevision)
            val undone = fixture.adapter.undo()
            assertNotNull("owned undo must return an update: notes=${fixture.adapter.debugNotes}", undone)
            assertTrue(root.applyUpdateJSON(requireNotNull(undone)))
            assertEquals(before, fixture.documentObject().toString())
            assertEquals(fixture.adapter.baseDocumentRevision.toString(), root.lastAppliedDocumentVersion)
        }

    @Test
    fun `second pointer cancels a held resize`() =
        withMountedTable(fixedWidthGrid) { fixture ->
            val edge = fixture.trailingEdge(0)
            fixture.dispatch(MotionEvent.ACTION_DOWN, edge.first, edge.second)
            fixture.dispatch(MotionEvent.ACTION_MOVE, edge.first + 40f, edge.second)
            assertWidth("preview before the second pointer", 160f, fixture.cell(0).bounds.width())
            fixture.secondPointerDown(edge.first + 40f, edge.second)
            assertNull(fixture.drawing.activeTableResizeEdge)
            assertWidth("second pointer restores geometry", 120f, fixture.cell(0).bounds.width())
            fixture.dispatch(MotionEvent.ACTION_UP, edge.first + 40f, edge.second)
            assertEquals(listOf(listOf(120), listOf(120)), fixture.columnWidths(0))
            assertEquals(false, fixture.adapter.historyCanUndo())
        }

    @Test
    fun `vertical intent past touch slop never becomes a resize`() =
        withMountedTable(fixedWidthGrid) { fixture ->
            val edge = fixture.trailingEdge(0)
            val slop = android.view.ViewConfiguration.get(fixture.view.context).scaledTouchSlop.toFloat()
            fixture.dispatch(MotionEvent.ACTION_DOWN, edge.first, edge.second)
            fixture.dispatch(MotionEvent.ACTION_MOVE, edge.first + 1f, edge.second + slop / 2f)
            assertNull("no decision inside the slop", fixture.drawing.activeTableResizeEdge)
            fixture.dispatch(MotionEvent.ACTION_MOVE, edge.first + slop, edge.second + slop * 2f)
            assertNull("vertical intent declines the resize", fixture.drawing.activeTableResizeEdge)
            fixture.dispatch(MotionEvent.ACTION_MOVE, edge.first + 80f, edge.second + slop * 2f)
            assertNull("a later horizontal move cannot revive a declined resize",
                fixture.drawing.activeTableResizeEdge)
            fixture.dispatch(MotionEvent.ACTION_UP, edge.first + 80f, edge.second + slop * 2f)
            assertEquals(listOf(listOf(120), listOf(120)), fixture.columnWidths(0))
            assertWidth("geometry is untouched", 120f, fixture.cell(0).bounds.width())
        }

    private companion object {
        const val GRID_SIZE = 3
        const val NARROW_COLUMN_WIDTH = 150
        const val SWIPE_STEPS = 4
        const val BODY_SWIPE_DISTANCE = 80f
        const val MERGED_RELEASE_PLAN = 0
        const val MERGED_CORE_EDITOR = 2
        const val MERGED_ROW_ONE_NEIGHBOUR = 3
        const val MERGED_ROW_TWO_NEIGHBOUR = 5
        const val MERGED_LAST_ROW_FIRST_CELL = 7
    }
}
