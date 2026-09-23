package com.apollohg.editor

import org.json.JSONObject
import android.view.View
import android.view.MotionEvent
import android.view.inputmethod.EditorInfo
import android.graphics.Bitmap
import android.graphics.Canvas
import android.graphics.Color
import com.apollohg.editor.viewer.PreparedProseDrawingView
import com.apollohg.editor.tables.TableStyle
import com.apollohg.editor.tables.EditorCellSelection
import com.apollohg.editor.tables.resolveEditorCellSelection
import org.robolectric.RuntimeEnvironment
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotEquals
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNull
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config
import org.robolectric.annotation.GraphicsMode

@RunWith(RobolectricTestRunner::class)
@Config(sdk = [34])
internal class EditorCellSelectionAdmissionTest {
    private val config = """{"schema":{"nodes":[{"name":"doc","content":"block+","role":"doc"},{"name":"paragraph","content":"inline*","group":"block","role":"textBlock"},{"name":"text","content":"","group":"inline","role":"text"},{"name":"table","content":"table_row+","group":"block","role":"block","tableRole":"table"},{"name":"table_row","content":"(table_cell | table_header)*","role":"block","tableRole":"row"},{"name":"table_cell","content":"block+","role":"block","tableRole":"cell","attrs":{"colspan":{"type":"number","default":1,"min":1},"rowspan":{"type":"number","default":1,"min":1},"colwidth":{"default":null}}},{"name":"table_header","content":"block+","role":"block","tableRole":"header_cell","attrs":{"colspan":{"type":"number","default":1,"min":1},"rowspan":{"type":"number","default":1,"min":1},"colwidth":{"default":null}}}],"marks":[]},"initialization":{"type":"localEmpty"}}"""
    private val document = """{"type":"doc","content":[{"type":"table","content":[{"type":"table_row","content":[{"type":"table_cell","content":[{"type":"paragraph","content":[{"type":"text","text":"one"}]}]},{"type":"table_cell","content":[{"type":"paragraph","content":[{"type":"text","text":"two"}]}]}]}]},{"type":"paragraph","content":[{"type":"text","text":"after"}]}]}"""

    private fun selectCells(adapter: EditorV2Adapter, anchorIndex: Int = 0, headIndex: Int = 1): Pair<Int, Int> {
        val cells = adapter.cachedTableRecords.values.single().getJSONArray("cells")
        val anchor = cells.getJSONObject(anchorIndex).getInt("sourcePos")
        val head = cells.getJSONObject(headIndex).getInt("sourcePos")
        fun point(docPos: Int) = JSONObject().put("offset", requireNotNull(adapter.scalarPositionForDoc(docPos + 2)))
            .put("kind", "scalar")
        val selection = JSONObject().put("type", "cell")
            .put("anchorCell", point(anchor)).put("headCell", point(head))
        val result = adapter.callWithEnvelope(JSONObject().put("selection", selection)) {
            UniffiEditorV2Backend.setSelection(adapter.editorId, it)
        }
        assertTrue("engine rejected cell selection: $result", result is EditorV2CallResult.Ok)
        return anchor to head
    }

    @Test
    fun `engine generated cell selection survives atomic Android admission`() {
        val created = UniffiEditorV2Backend.create(config, null) as EditorV2CallResult.Ok
        val adapter = requireNotNull(EditorV2Adapter.attach(
            UniffiEditorV2Backend, JSONObject(created.value).getString("editorId"), false
        ))
        try {
            assertNotNull(adapter.setContentJson(document))
            val (anchor, head) = selectCells(adapter)
            val raw = (UniffiEditorV2Backend.renderUpdate(adapter.editorId, null, null)
                as EditorV2CallResult.Ok).value
            val wire = JSONObject(raw).getJSONObject("selection")
            assertEquals(setOf("type", "anchorCell", "headCell"), wire.keys().asSequence().toSet())
            assertEquals(anchor, wire.getInt("anchorCell"))
            assertEquals(head, wire.getInt("headCell"))
            assertNotNull("Android rejects genuine engine selection", parseAtomicRenderSnapshot(raw))
            val mismatched = JSONObject(raw).put("selection", JSONObject(wire.toString())
                .put("headCell", head + 10000))
            assertNull("cell opening outside the admitted table", parseAtomicRenderSnapshot(mismatched.toString()))
            assertNull(parseAtomicRenderSnapshot(JSONObject(raw).put("selection", JSONObject(wire.toString())
                .put("anchorScalar", 0)).toString()))
            assertNull(parseAtomicRenderSnapshot(JSONObject(raw).put("selection", JSONObject(wire.toString())
                .put("headCell", "9")).toString()))
            val failureFrame = JSONObject(raw)
            val failureRecord = failureFrame.getJSONObject("tableRecords").getJSONObject("t0")
            failureRecord.put("rows", 0).put("columns", 0)
                .put("columnWidths", org.json.JSONArray())
                .put("sourceRows", org.json.JSONArray())
                .put("cells", org.json.JSONArray())
                .put("syntheticRegions", org.json.JSONArray())
                .put("failure", "invalidStructure")
            failureFrame.getJSONObject("tableInputMappings").getJSONObject("tables")
                .getJSONObject("t0").put("extent", JSONObject.NULL)
                .put("cells", org.json.JSONArray())
            assertNotNull("preserved selection must survive an unavailable projection",
                parseAtomicRenderSnapshot(failureFrame.toString()))
        } finally {
            adapter.destroy()
        }
    }

    @Test
    fun `merged multirow rectangle closes over spans in either layout direction`() {
        val merged = """{"type":"doc","content":[{"type":"table","content":[{"type":"table_row","content":[{"type":"table_cell","attrs":{"colspan":2},"content":[{"type":"paragraph","content":[{"type":"text","text":"A"}]}]},{"type":"table_cell","content":[{"type":"paragraph","content":[{"type":"text","text":"B"}]}]}]},{"type":"table_row","content":[{"type":"table_cell","content":[{"type":"paragraph","content":[{"type":"text","text":"C"}]}]},{"type":"table_cell","content":[{"type":"paragraph","content":[{"type":"text","text":"D"}]}]},{"type":"table_cell","content":[{"type":"paragraph","content":[{"type":"text","text":"E"}]}]}]}]}]}"""
        val created = UniffiEditorV2Backend.create(config, null) as EditorV2CallResult.Ok
        val adapter = requireNotNull(EditorV2Adapter.attach(
            UniffiEditorV2Backend, JSONObject(created.value).getString("editorId"), false
        ))
        try {
            assertNotNull(adapter.setContentJson(merged))
            selectCells(adapter, anchorIndex = 1, headIndex = 3)
            val raw = (UniffiEditorV2Backend.renderUpdate(adapter.editorId, null, null)
                as EditorV2CallResult.Ok).value
            val selection = JSONObject(raw).getJSONObject("selection")
            val record = adapter.cachedTableRecords.values.single()
            val realCells = record.getJSONArray("cells")
            val expected = (0 until realCells.length()).map { realCells.getJSONObject(it).getInt("sourcePos") }.toSet()
            assertEquals(5, expected.size)
            assertEquals(expected, (resolveEditorCellSelection(selection, adapter.cachedTableRecords)
                as EditorCellSelection.Drawable).sourcePositions)
            val rtl = JSONObject(record.toString()).put("direction", "rtl")
            assertEquals(expected, (resolveEditorCellSelection(selection, mapOf("t0" to rtl))
                as EditorCellSelection.Drawable).sourcePositions)
            val unavailable = JSONObject(record.toString()).put("failure", "invalidStructure")
            assertTrue(resolveEditorCellSelection(selection, mapOf("t0" to unavailable))
                is EditorCellSelection.Unavailable)
            val outerFailure = JSONObject().put("tablePos", 0).put("sourceEnd", 100)
                .put("failure", "invalidStructure")
            val innerFailure = JSONObject().put("tablePos", 20).put("sourceEnd", 50)
                .put("failure", "invalidStructure")
            val nestedSelection = JSONObject().put("type", "cell")
                .put("anchorCell", 25).put("headCell", 30)
            assertEquals("t20", resolveEditorCellSelection(nestedSelection,
                mapOf("t0" to outerFailure, "t20" to innerFailure))?.tableId)

            val separateTables = JSONObject(document).apply {
                val content = getJSONArray("content")
                content.put(JSONObject(content.getJSONObject(0).toString()))
            }
            assertNotNull(adapter.setContentJson(separateTables.toString()))
            val tableRecords = adapter.cachedTableRecords
            assertEquals(2, tableRecords.size)
            val openings = tableRecords.values.map { it.getJSONArray("cells").getJSONObject(0).getInt("sourcePos") }
            val crossTable = JSONObject().put("type", "cell")
                .put("anchorCell", openings[0]).put("headCell", openings[1])
            assertNull(resolveEditorCellSelection(crossTable, tableRecords))
            val atomic = JSONObject(requireNotNull(adapter.cachedAtomicRenderJson))
                .put("selection", crossTable)
            assertNull(parseAtomicRenderSnapshot(atomic.toString()))
        } finally {
            adapter.destroy()
        }
    }

    @Test
    fun `cell selection blocks stale root caret and input without a document write`() {
        val created = UniffiEditorV2Backend.create(config, null) as EditorV2CallResult.Ok
        val adapter = requireNotNull(EditorV2Adapter.attach(
            UniffiEditorV2Backend, JSONObject(created.value).getString("editorId"), false
        ))
        val token = EditorV2Registry.register(adapter)
        try {
            val initial = requireNotNull(adapter.setContentJson(document))
            val view = RichTextEditorView(RuntimeEnvironment.getApplication())
            view.editorId = token
            assertTrue(view.editorEditText.applyUpdateJSON(initial))
            view.measure(View.MeasureSpec.makeMeasureSpec(600, View.MeasureSpec.EXACTLY),
                View.MeasureSpec.makeMeasureSpec(500, View.MeasureSpec.EXACTLY))
            view.layout(0, 0, 600, 500)
            val root = view.editorEditText
            root.requestFocus()
            val staleConnection = requireNotNull(root.onCreateInputConnection(EditorInfo()))
            val before = adapter.documentJson()
            val (_, headOpening) = selectCells(adapter)
            val selected = requireNotNull(adapter.refreshFromRustState(null))
            assertTrue(root.applyUpdateJSON(selected))
            assertTrue(root.rootTableSelectionInputBlocked)
            assertTrue("cell selection must preserve editor focus", root.hasFocus())
            assertEquals(null, root.caretRect())

            root.setSelection(root.text.length)
            staleConnection.commitText("stale", 1)

            assertTrue("cell authority was replaced by stale root caret", root.rootTableSelectionInputBlocked)
            assertEquals("cell", JSONObject(requireNotNull(adapter.selectionJson())).optString("type", ""))
            assertEquals(before, adapter.documentJson())

            val textScalar = requireNotNull(adapter.scalarPositionForDoc(headOpening + 2))
            val point = JSONObject().put("offset", textScalar).put("kind", "scalar")
            val textSelection = JSONObject().put("type", "text")
                .put("anchor", point).put("head", point)
            val textResult = adapter.callWithEnvelope(JSONObject().put("selection", textSelection)) {
                UniffiEditorV2Backend.setSelection(adapter.editorId, it)
            }
            assertTrue(textResult is EditorV2CallResult.Ok)
            assertTrue(root.applyUpdateJSON(requireNotNull(adapter.refreshFromRustState(null))))
            assertFalse("external text-in-cell retained cell authority", root.authoritativeCellSelectionActive)
            assertTrue("root input must remain blocked for cell text", root.rootTableSelectionInputBlocked)
        } finally {
            EditorV2Registry.remove(adapter.editorId)
            adapter.destroy()
        }
    }

    @Test
    @GraphicsMode(GraphicsMode.Mode.NATIVE)
    fun `engine rectangle paints real cell frames from prepared geometry`() {
        val created = UniffiEditorV2Backend.create(config, null) as EditorV2CallResult.Ok
        val adapter = requireNotNull(EditorV2Adapter.attach(
            UniffiEditorV2Backend, JSONObject(created.value).getString("editorId"), false
        ))
        val token = EditorV2Registry.register(adapter)
        try {
            val threeCells = JSONObject(document).apply {
                getJSONArray("content").getJSONObject(0).getJSONArray("content")
                    .getJSONObject(0).getJSONArray("content")
                    .put(JSONObject("""{"type":"table_cell","content":[{"type":"paragraph","content":[{"type":"text","text":"three"}]}]}"""))
            }
            val initial = requireNotNull(adapter.setContentJson(threeCells.toString()))
            val view = RichTextEditorView(RuntimeEnvironment.getApplication())
            view.editorId = token
            assertTrue(view.editorEditText.applyUpdateJSON(initial))
            val selectionColor = 0x66A54122
            view.applyTheme(EditorTheme(table = TableStyle(selectionColor = selectionColor)))
            view.measure(View.MeasureSpec.makeMeasureSpec(600, View.MeasureSpec.EXACTLY),
                View.MeasureSpec.makeMeasureSpec(500, View.MeasureSpec.EXACTLY))
            view.layout(0, 0, 600, 500)
            fun paint(): List<Int> {
                val drawing = (0 until view.editorContentFrame.childCount)
                    .map { view.editorContentFrame.getChildAt(it) }
                    .filterIsInstance<PreparedProseDrawingView>().single()
                drawing.measure(View.MeasureSpec.makeMeasureSpec(600, View.MeasureSpec.EXACTLY),
                    View.MeasureSpec.makeMeasureSpec(500, View.MeasureSpec.EXACTLY))
                drawing.layout(0, 0, 600, 500)
                val bitmap = Bitmap.createBitmap(600, 500, Bitmap.Config.ARGB_8888)
                drawing.draw(Canvas(bitmap))
                val block = drawing.preparedLayout!!.blocks.single()
                val bounds = block.tableBounds!!
                val samples = block.tableSurface!!.cells.map { cell ->
                    bitmap.getPixel((bounds.left + cell.frame.left + cell.frame.width - 20).toInt(),
                        (bounds.top + cell.frame.top + cell.frame.height / 2).toInt())
                }
                bitmap.recycle()
                return samples
            }
            val before = paint()
            selectCells(adapter)
            assertTrue(view.editorEditText.applyUpdateJSON(requireNotNull(adapter.refreshFromRustState(null))))
            val selectedDrawing = (0 until view.editorContentFrame.childCount)
                .map { view.editorContentFrame.getChildAt(it) }
                .filterIsInstance<PreparedProseDrawingView>().single()
            assertEquals(2, selectedDrawing.selectedTableCellSourcePositions.values.single().size)
            val after = paint()
            assertEquals(3, after.size)
            assertNotEquals("first selected frame unchanged", before[0], after[0])
            assertNotEquals("second selected frame unchanged", before[1], after[1])
            assertEquals("unselected real cell changed", before[2], after[2])
            fun assertThemeColor(actual: Int) {
                assertEquals(Color.alpha(selectionColor), Color.alpha(actual))
                assertTrue(kotlin.math.abs(Color.red(selectionColor) - Color.red(actual)) <= 2)
                assertTrue(kotlin.math.abs(Color.green(selectionColor) - Color.green(actual)) <= 2)
                assertTrue(kotlin.math.abs(Color.blue(selectionColor) - Color.blue(actual)) <= 2)
            }
            assertThemeColor(after[0])
            assertThemeColor(after[1])
        } finally {
            EditorV2Registry.remove(adapter.editorId)
            adapter.destroy()
        }
    }

    @Test
    fun `selection from focused cell retires its connection and retains editor focus`() {
        val created = UniffiEditorV2Backend.create(config, null) as EditorV2CallResult.Ok
        val adapter = requireNotNull(EditorV2Adapter.attach(
            UniffiEditorV2Backend, JSONObject(created.value).getString("editorId"), false
        ))
        val token = EditorV2Registry.register(adapter)
        try {
            val initial = requireNotNull(adapter.setContentJson(document))
            val view = RichTextEditorView(RuntimeEnvironment.getApplication())
            view.editorId = token
            assertTrue(view.editorEditText.applyUpdateJSON(initial))
            view.measure(View.MeasureSpec.makeMeasureSpec(600, View.MeasureSpec.EXACTLY),
                View.MeasureSpec.makeMeasureSpec(500, View.MeasureSpec.EXACTLY))
            view.layout(0, 0, 600, 500)
            val drawing = (0 until view.editorContentFrame.childCount)
                .map { view.editorContentFrame.getChildAt(it) }
                .filterIsInstance<PreparedProseDrawingView>().single()
            drawing.measure(View.MeasureSpec.makeMeasureSpec(600, View.MeasureSpec.EXACTLY),
                View.MeasureSpec.makeMeasureSpec(500, View.MeasureSpec.EXACTLY))
            drawing.layout(0, 0, 600, 500)
            val block = drawing.preparedLayout!!.blocks.single()
            val cell = block.tableSurface!!.cells.first()
            val bounds = block.tableBounds!!
            val x = bounds.left + cell.frame.left + cell.contentOrigin.first + 8f
            val y = bounds.top + cell.frame.top + cell.contentOrigin.second + 8f
            val down = MotionEvent.obtain(0, 0, MotionEvent.ACTION_DOWN, x, y, 0)
            val up = MotionEvent.obtain(0, 10, MotionEvent.ACTION_UP, x, y, 0)
            try {
                assertTrue(view.dispatchTouchEvent(down))
                assertTrue(view.dispatchTouchEvent(up))
            } finally {
                down.recycle()
                up.recycle()
            }
            val input = view.activeTextInput
            assertTrue(input !== view.editorEditText)
            assertTrue(input.hasFocus())
            val staleConnection = requireNotNull(input.onCreateInputConnection(EditorInfo()))
            val before = adapter.documentJson()

            selectCells(adapter)
            assertTrue(view.editorEditText.applyUpdateJSON(requireNotNull(adapter.refreshFromRustState(null))))

            assertTrue(view.activeTextInput === view.editorEditText)
            assertTrue(view.editorEditText.hasFocus())
            staleConnection.commitText("stale", 1)
            assertEquals(before, adapter.documentJson())
            assertEquals("cell", JSONObject(requireNotNull(adapter.selectionJson())).optString("type", ""))
        } finally {
            EditorV2Registry.remove(adapter.editorId)
            adapter.destroy()
        }
    }

    @Test
    fun `tapping a selected cell returns to an engine text selection`() {
        val created = UniffiEditorV2Backend.create(config, null) as EditorV2CallResult.Ok
        val adapter = requireNotNull(EditorV2Adapter.attach(
            UniffiEditorV2Backend, JSONObject(created.value).getString("editorId"), false
        ))
        val token = EditorV2Registry.register(adapter)
        try {
            val initial = requireNotNull(adapter.setContentJson(document))
            val view = RichTextEditorView(RuntimeEnvironment.getApplication())
            view.editorId = token
            assertTrue(view.editorEditText.applyUpdateJSON(initial))
            view.measure(View.MeasureSpec.makeMeasureSpec(600, View.MeasureSpec.EXACTLY),
                View.MeasureSpec.makeMeasureSpec(500, View.MeasureSpec.EXACTLY))
            view.layout(0, 0, 600, 500)
            selectCells(adapter)
            assertTrue(view.editorEditText.applyUpdateJSON(requireNotNull(adapter.refreshFromRustState(null))))
            val drawing = (0 until view.editorContentFrame.childCount)
                .map { view.editorContentFrame.getChildAt(it) }
                .filterIsInstance<PreparedProseDrawingView>().single()
            drawing.measure(View.MeasureSpec.makeMeasureSpec(600, View.MeasureSpec.EXACTLY),
                View.MeasureSpec.makeMeasureSpec(500, View.MeasureSpec.EXACTLY))
            drawing.layout(0, 0, 600, 500)
            val block = drawing.preparedLayout!!.blocks.single()
            val cell = block.tableSurface!!.cells.last()
            val bounds = block.tableBounds!!
            val x = bounds.left + cell.frame.left + cell.contentOrigin.first + 8f
            val y = bounds.top + cell.frame.top + cell.contentOrigin.second + 8f
            val down = MotionEvent.obtain(0, 0, MotionEvent.ACTION_DOWN, x, y, 0)
            val up = MotionEvent.obtain(0, 10, MotionEvent.ACTION_UP, x, y, 0)
            try {
                assertTrue(view.dispatchTouchEvent(down))
                assertTrue(view.dispatchTouchEvent(up))
            } finally {
                down.recycle()
                up.recycle()
            }
            assertTrue(view.activeTextInput !== view.editorEditText)
            assertEquals("text", JSONObject(requireNotNull(adapter.selectionJson())).optString("type", ""))
            assertFalse(view.editorEditText.authoritativeCellSelectionActive)
            assertTrue(drawing.selectedTableCellSourcePositions.isEmpty())

            val before = adapter.documentJson()
            selectCells(adapter)
            assertTrue(view.editorEditText.applyUpdateJSON(requireNotNull(adapter.refreshFromRustState(null))))
            val root = view.editorEditText
            val prose = root.text.toString().indexOf("after")
            val line = root.layout.getLineForOffset(prose)
            val proseX = root.left + root.totalPaddingLeft + 16f
            val proseY = root.top + root.totalPaddingTop +
                (root.layout.getLineTop(line) + root.layout.getLineBottom(line)) / 2f
            val proseDown = MotionEvent.obtain(20, 20, MotionEvent.ACTION_DOWN, proseX, proseY, 0)
            val proseUp = MotionEvent.obtain(20, 30, MotionEvent.ACTION_UP, proseX, proseY, 0)
            try {
                assertTrue(view.dispatchTouchEvent(proseDown))
                assertTrue(view.dispatchTouchEvent(proseUp))
            } finally {
                proseDown.recycle()
                proseUp.recycle()
            }
            assertTrue(view.activeTextInput === root)
            assertEquals("text", JSONObject(requireNotNull(adapter.selectionJson())).optString("type", ""))
            assertFalse(root.authoritativeCellSelectionActive)
            assertFalse(root.rootTableSelectionInputBlocked)
            assertEquals(before, adapter.documentJson())
        } finally {
            EditorV2Registry.remove(adapter.editorId)
            adapter.destroy()
        }
    }
}
