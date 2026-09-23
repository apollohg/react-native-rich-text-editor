package com.apollohg.editor

import android.text.Annotation
import android.text.Spanned
import android.text.style.ForegroundColorSpan
import android.graphics.Color
import android.view.View
import android.view.MotionEvent
import android.view.KeyEvent
import android.view.inputmethod.EditorInfo
import com.apollohg.editor.tables.RootTableHeightSpan
import com.apollohg.editor.viewer.PreparedProseDrawingView
import org.json.JSONObject
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNotSame
import org.junit.Assert.assertNotEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.RuntimeEnvironment
import org.robolectric.annotation.Config
import org.robolectric.annotation.GraphicsMode

@RunWith(RobolectricTestRunner::class)
@Config(sdk = [34])
internal class EditorTableSurfaceMountTest {
    private val config = """{"schema":{"nodes":[{"name":"doc","content":"block+","role":"doc"},{"name":"paragraph","content":"inline*","group":"block","role":"textBlock"},{"name":"text","content":"","group":"inline","role":"text"},{"name":"table","content":"table_row+","group":"block","role":"block","tableRole":"table"},{"name":"table_row","content":"(table_cell | table_header)*","role":"block","tableRole":"row"},{"name":"table_cell","content":"block+","role":"block","tableRole":"cell","attrs":{"colspan":{"type":"number","default":1,"min":1},"rowspan":{"type":"number","default":1,"min":1},"colwidth":{"default":null}}},{"name":"table_header","content":"block+","role":"block","tableRole":"header_cell","attrs":{"colspan":{"type":"number","default":1,"min":1},"rowspan":{"type":"number","default":1,"min":1},"colwidth":{"default":null}}}],"marks":[]},"initialization":{"type":"localEmpty"}}"""
    private val tableDocument = """{"type":"doc","content":[{"type":"table","content":[{"type":"table_row","content":[{"type":"table_cell","content":[{"type":"paragraph","content":[{"type":"text","text":"Cell text"}]}]}]}]},{"type":"paragraph","content":[{"type":"text","text":"after"}]}]}"""
    private val nestedTableDocument = """{"type":"doc","content":[{"type":"paragraph","content":[{"type":"text","text":"before"}]},{"type":"table","content":[{"type":"table_row","content":[{"type":"table_cell","content":[{"type":"paragraph","content":[{"type":"text","text":"Alpha"}]}]},{"type":"table_cell","content":[{"type":"table","content":[{"type":"table_row","content":[{"type":"table_cell","content":[{"type":"paragraph","content":[{"type":"text","text":"Nested"}]}]}]}]}]},{"type":"table_cell","content":[{"type":"paragraph","content":[{"type":"text","text":"Owner"}]}]}]}]},{"type":"paragraph","content":[{"type":"text","text":"after"}]}]}"""

    private fun measure(view: RichTextEditorView, width: Int) {
        view.measure(View.MeasureSpec.makeMeasureSpec(width, View.MeasureSpec.EXACTLY),
            View.MeasureSpec.makeMeasureSpec(500, View.MeasureSpec.EXACTLY))
        view.layout(0, 0, width, 500)
    }

    private fun drawing(view: RichTextEditorView): PreparedProseDrawingView? =
        (0 until view.editorContentFrame.childCount)
            .map { view.editorContentFrame.getChildAt(it) }
            .filterIsInstance<PreparedProseDrawingView>().singleOrNull()

    private fun withMountedView(
        document: String = tableDocument,
        block: (RichTextEditorView, EditorV2Adapter, String) -> Unit
    ) {
        val created = UniffiEditorV2Backend.create(config, null) as EditorV2CallResult.Ok
        val adapter = requireNotNull(EditorV2Adapter.attach(
            UniffiEditorV2Backend, JSONObject(created.value).getString("editorId"), false
        ))
        val token = EditorV2Registry.register(adapter)
        try {
            val update = requireNotNull(adapter.setContentJson(document))
            val view = RichTextEditorView(RuntimeEnvironment.getApplication())
            view.editorId = token
            assertTrue(view.editorEditText.applyUpdateJSON(update))
            measure(view, 600)
            block(view, adapter, update)
        } finally {
            EditorV2Registry.remove(adapter.editorId)
            adapter.destroy()
        }
    }

    private fun heightSpan(view: RichTextEditorView): RootTableHeightSpan {
        val text = view.editorEditText.text
        return text.getSpans(0, text.length, RootTableHeightSpan::class.java).single()
    }

    private fun cellText(adapter: EditorV2Adapter, cellIndex: Int): String =
        JSONObject(requireNotNull(adapter.documentJson()))
            .getJSONArray("content").let { content ->
                (0 until content.length()).map { content.getJSONObject(it) }
                    .first { it.getString("type") == "table" }
            }
            .getJSONArray("content").getJSONObject(0)
            .getJSONArray("content").getJSONObject(cellIndex)
            .getJSONArray("content").getJSONObject(0)
            .getJSONArray("content").getJSONObject(0).getString("text")

    private fun firstCellText(adapter: EditorV2Adapter): String = cellText(adapter, 0)

    private fun externalReplacement(adapter: EditorV2Adapter, document: String): String {
        val request = JSONObject().put("version", 1).put("requestId", "1")
            .put("history", "resetAndClear").put("setJson", JSONObject(document))
        val replaced = UniffiEditorV2Backend.replaceDocument(adapter.editorId, request.toString())
        assertTrue("external replacement=$replaced", replaced is EditorV2CallResult.Ok)
        return requireNotNull(adapter.refreshFromRustState(null))
    }

    private fun tapFirstCell(view: RichTextEditorView, cellIndex: Int = 0) {
        val canvas = requireNotNull(drawing(view))
        canvas.measure(View.MeasureSpec.makeMeasureSpec(view.editorEditText.width, View.MeasureSpec.EXACTLY),
            View.MeasureSpec.makeMeasureSpec(view.editorEditText.height, View.MeasureSpec.EXACTLY))
        canvas.layout(0, 0, canvas.measuredWidth, canvas.measuredHeight)
        val block = requireNotNull(canvas.preparedLayout?.blocks?.singleOrNull())
        val cell = requireNotNull(block.tableSurface?.cells?.getOrNull(cellIndex))
        val bounds = requireNotNull(block.tableBounds)
        val x = bounds.left + cell.frame.left + cell.contentOrigin.first + 8f
        val y = bounds.top + cell.frame.top + cell.contentOrigin.second + 8f
        val down = MotionEvent.obtain(0, 0, MotionEvent.ACTION_DOWN, x, y, 0)
        val up = MotionEvent.obtain(0, 10, MotionEvent.ACTION_UP, x, y, 0)
        try {
            assertTrue(view.dispatchTouchEvent(down))
            val handled = view.dispatchTouchEvent(up)
            assertTrue("tap up focus=${view.activeTextInput === view.editorEditText} rootTrace=${view.editorEditText.imeTraceSnapshotForTesting()} doc=${(view.editorEditText.v2Driver as? EditorV2Adapter)?.documentJson()}",
                handled)
        } finally {
            down.recycle()
            up.recycle()
        }
    }

    @Test
    fun `tap mounts one editable cell and its input connection types through the document`() = withMountedView { view, adapter, _ ->
        measure(view, 600)
        val canvas = requireNotNull(drawing(view))
        canvas.measure(View.MeasureSpec.makeMeasureSpec(view.editorEditText.width, View.MeasureSpec.EXACTLY),
            View.MeasureSpec.makeMeasureSpec(view.editorEditText.height, View.MeasureSpec.EXACTLY))
        canvas.layout(0, 0, canvas.measuredWidth, canvas.measuredHeight)
        val block = requireNotNull(canvas.preparedLayout?.blocks?.singleOrNull())
        val cell = requireNotNull(block.tableSurface?.cells?.singleOrNull())
        val frame = requireNotNull(block.tableBounds)
        assertTrue("canvas ${canvas.width}x${canvas.height}", canvas.width > 0)
        assertNotNull("source cell index", cell.sourceCellIndex)
        val x = frame.left + cell.frame.left + cell.contentOrigin.first + 8f
        val y = frame.top + cell.frame.top + cell.contentOrigin.second + 8f
        val rootConnection = requireNotNull(view.editorEditText.onCreateInputConnection(EditorInfo()))
        val beforeTap = adapter.documentJson()
        val down = MotionEvent.obtain(0, 0, MotionEvent.ACTION_DOWN, x, y, 0)
        val up = MotionEvent.obtain(0, 10, MotionEvent.ACTION_UP, x, y, 0)
        try {
            assertTrue(view.dispatchTouchEvent(down))
            assertTrue(view.dispatchTouchEvent(up))
        } finally {
            down.recycle()
            up.recycle()
        }

        val cellInput = (0 until view.editorContentFrame.childCount)
            .map { view.editorContentFrame.getChildAt(it) }
            .filterIsInstance<EditorEditText>()
            .singleOrNull { it !== view.editorEditText }
        assertNotNull("cell input after host tap", cellInput)
        val mountedInput = requireNotNull(cellInput)
        assertTrue(mountedInput.hasFocus())
        assertTrue("cell authority", mountedInput.isAuthorizedForTableCellInput())
        assertTrue(view.editorEditText.ownsNativeBinding(adapter))
        assertTrue(!rootConnection.beginBatchEdit())
        rootConnection.commitText("stale", 1)
        assertEquals(beforeTap, adapter.documentJson())
        view.forceLayout()
        measure(view, 600)
        assertNotNull("table persists after selection-triggered relayout", drawing(view))
        assertTrue("cell remains mounted after relayout", mountedInput.parent === view.editorContentFrame)
        assertEquals(beforeTap, adapter.documentJson())
        val epochBeforeCaretMove = adapter.positionEpoch
        mountedInput.setSelection(if (mountedInput.selectionStart == 0) mountedInput.text.length else 0)
        assertNotEquals(epochBeforeCaretMove, adapter.positionEpoch)
        assertTrue("selection-only epoch must rebind", mountedInput.isAuthorizedForTableCellInput())
        mountedInput.setSelection(mountedInput.text.length)
        assertTrue("selection-only epoch must rebind", mountedInput.isAuthorizedForTableCellInput())
        assertNotNull(mountedInput.inputScalar(mountedInput.selectionStart))
        val connection = requireNotNull(mountedInput.onCreateInputConnection(EditorInfo()))
        assertTrue(connection.commitText("Q", 1))
        assertEquals("Cell textQ", firstCellText(adapter))
        assertTrue(connection.setSelection(0, 0))
        assertTrue("IME selection-only epoch must rebind", mountedInput.isAuthorizedForTableCellInput())
        assertTrue(connection.commitText("R", 1))
        assertEquals("RCell textQ", firstCellText(adapter))
    }

    @Test
    @GraphicsMode(GraphicsMode.Mode.NATIVE)
    fun `cell composition survives width reflow and commits through the same connection`() = withMountedView { view, adapter, _ ->
        val canvas = requireNotNull(drawing(view))
        canvas.measure(View.MeasureSpec.makeMeasureSpec(600, View.MeasureSpec.EXACTLY),
            View.MeasureSpec.makeMeasureSpec(500, View.MeasureSpec.EXACTLY))
        canvas.layout(0, 0, 600, 500)
        val block = requireNotNull(canvas.preparedLayout?.blocks?.singleOrNull())
        val cell = requireNotNull(block.tableSurface?.cells?.singleOrNull())
        val bounds = requireNotNull(block.tableBounds)
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
        input.setSelection(input.text.length)
        val connection = requireNotNull(input.onCreateInputConnection(EditorInfo()))
        val generation = input.inputConnectionGenerationForTesting()
        assertTrue(connection.setComposingText("pending", 1))
        assertEquals("Cell textpending", input.text.toString())
        assertEquals("Cell text", firstCellText(adapter))

        view.forceLayout()
        measure(view, 320)

        assertTrue(input === view.activeTextInput)
        assertEquals("Cell textpending", input.text.toString())
        assertEquals("Cell text", firstCellText(adapter))
        assertEquals(generation, input.inputConnectionGenerationForTesting())
        assertTrue(connection.finishComposingText())
        assertEquals("Cell textpending", firstCellText(adapter))
    }

    @Test
    fun `root composition commits before a cell takes focus`() = withMountedView { view, adapter, _ ->
        val root = view.editorEditText
        root.setSelection(root.text.length)
        val connection = requireNotNull(root.onCreateInputConnection(EditorInfo()))
        assertTrue(connection.setComposingText("tail", 1))
        assertTrue(root.hasPendingCompositionForExternalRefresh())
        val before = adapter.documentJson()

        tapFirstCell(view)

        assertTrue(view.activeTextInput !== root)
        assertTrue(view.activeTextInput.hasFocus())
        assertEquals("Cell text", firstCellText(adapter))
        assertTrue(adapter.documentJson() != before)
        assertTrue(adapter.documentJson()?.contains("aftertail") == true)
        assertTrue(!root.hasPendingCompositionForExternalRefresh())
    }

    @Test
    fun `root composition before table retargets the same cell after position shift`() {
        val document = tableDocument.replace("[{\"type\":\"table\"",
            "[{\"type\":\"paragraph\",\"content\":[{\"type\":\"text\",\"text\":\"before\"}]},{\"type\":\"table\"")
        withMountedView(document) { view, adapter, _ ->
            val root = view.editorEditText
            val originalTableId = requireNotNull(adapter.cachedTableInputMappings).tables.keys.single()
            root.setSelection(root.text.toString().indexOf("before") + "before".length)
            val connection = requireNotNull(root.onCreateInputConnection(EditorInfo()))
            assertTrue(connection.setComposingText("tail", 1))
            assertTrue(root.hasPendingCompositionForExternalRefresh())

            tapFirstCell(view)

            val content = JSONObject(requireNotNull(adapter.documentJson())).getJSONArray("content")
            assertEquals("beforetail", content.getJSONObject(0).getJSONArray("content")
                .getJSONObject(0).getString("text"))
            assertEquals("Cell text", firstCellText(adapter))
            assertTrue(view.activeTextInput !== root)
            assertTrue(view.activeTextInput.hasFocus())
            assertTrue(originalTableId != requireNotNull(adapter.cachedTableInputMappings).tables.keys.single())
        }
    }

    @Test
    fun `tap into an empty cell inserts through its mounted input`() {
        val document = tableDocument.replace("[{\"type\":\"text\",\"text\":\"Cell text\"}]", "[]")
        withMountedView(document) { view, adapter, _ ->
            tapFirstCell(view)
            val input = view.activeTextInput
            assertTrue(input !== view.editorEditText)
            val connection = requireNotNull(input.onCreateInputConnection(EditorInfo()))
            assertTrue(connection.commitText("Z", 1))
            assertEquals("Z", firstCellText(adapter))
        }
    }

    @Test
    fun `theme-only reflow updates mounted cell spans without replacing input`() = withMountedView { view, adapter, _ ->
        view.applyTheme(EditorTheme.fromJson("""{"text":{"color":"#112233"}}"""))
        tapFirstCell(view)
        val input = view.activeTextInput
        val before = adapter.documentJson()
        view.applyTheme(EditorTheme.fromJson("""{"text":{"color":"#DDEEFF"}}"""))
        val colors = input.text.getSpans(0, input.text.length, ForegroundColorSpan::class.java)
            .map { it.foregroundColor }
        assertTrue("colors=$colors", Color.parseColor("#DDEEFF") in colors)
        assertTrue(view.activeTextInput === input)
        assertEquals(before, adapter.documentJson())
    }

    @Test
    fun `leaving a composing cell commits its text before root focus`() = withMountedView { view, adapter, _ ->
        tapFirstCell(view)
        val input = view.activeTextInput
        assertTrue(input !== view.editorEditText)
        input.setSelection(input.text.length)
        val connection = requireNotNull(input.onCreateInputConnection(EditorInfo()))
        assertTrue(connection.setComposingText("tail", 1))
        assertEquals("Cell text", firstCellText(adapter))

        val root = view.editorEditText
        val offset = root.text.toString().indexOf("after") + 2
        val line = root.layout.getLineForOffset(offset)
        val x = root.left + root.totalPaddingLeft + root.layout.getPrimaryHorizontal(offset)
        val y = root.top + root.totalPaddingTop +
            (root.layout.getLineTop(line) + root.layout.getLineBottom(line)) / 2f
        val down = MotionEvent.obtain(0, 0, MotionEvent.ACTION_DOWN, x, y, 0)
        val up = MotionEvent.obtain(0, 10, MotionEvent.ACTION_UP, x, y, 0)
        try {
            assertTrue(view.dispatchTouchEvent(down))
            assertEquals("after down trace=${input.imeTraceSnapshotForTesting()}",
                "Cell texttail", firstCellText(adapter))
            assertTrue(view.dispatchTouchEvent(up))
        } finally {
            down.recycle()
            up.recycle()
        }

        assertTrue(view.activeTextInput === root)
        assertEquals("Cell texttail", firstCellText(adapter))
        assertTrue(!connection.beginBatchEdit())
    }

    @Test
    fun `blocked composition preflight keeps the cell active on prose tap`() = withMountedView { view, adapter, _ ->
        tapFirstCell(view)
        val input = view.activeTextInput
        input.setSelection(input.text.length)
        val connection = requireNotNull(input.onCreateInputConnection(EditorInfo()))
        assertTrue(connection.setComposingText("tail", 1))
        val before = adapter.documentJson()
        input.blockExternalEditorUpdatePreparationForTesting = true

        val root = view.editorEditText
        val offset = root.text.toString().indexOf("after") + 2
        val line = root.layout.getLineForOffset(offset)
        val x = root.left + root.totalPaddingLeft + root.layout.getPrimaryHorizontal(offset)
        val y = root.top + root.totalPaddingTop +
            (root.layout.getLineTop(line) + root.layout.getLineBottom(line)) / 2f
        val down = MotionEvent.obtain(0, 0, MotionEvent.ACTION_DOWN, x, y, 0)
        val up = MotionEvent.obtain(0, 10, MotionEvent.ACTION_UP, x, y, 0)
        try {
            assertTrue(view.dispatchTouchEvent(down))
            assertTrue(view.dispatchTouchEvent(up))
        } finally {
            down.recycle()
            up.recycle()
            input.blockExternalEditorUpdatePreparationForTesting = false
        }

        assertTrue(view.activeTextInput === input)
        assertTrue(input.hasFocus())
        assertEquals("Cell texttail", input.text.toString())
        assertEquals(before, adapter.documentJson())
    }

    @Test
    fun `switching cells commits only the previous composition and reuses the input`() {
        val document = JSONObject(tableDocument)
        val cells = document.getJSONArray("content").getJSONObject(0)
            .getJSONArray("content").getJSONObject(0).getJSONArray("content")
        val second = JSONObject(cells.getJSONObject(0).toString())
        second.getJSONArray("content").getJSONObject(0).getJSONArray("content")
            .getJSONObject(0).put("text", "Other")
        cells.put(second)
        withMountedView(document.toString()) { view, adapter, _ ->
            tapFirstCell(view)
            val input = view.activeTextInput
            input.setSelection(input.text.length)
            val firstConnection = requireNotNull(input.onCreateInputConnection(EditorInfo()))
            assertTrue(firstConnection.setComposingText("tail", 1))
            assertEquals("Cell text", cellText(adapter, 0))

            tapFirstCell(view, 1)

            assertTrue(view.activeTextInput === input)
            assertEquals("Other", input.text.toString())
            assertEquals("Cell texttail", cellText(adapter, 0))
            assertEquals("Other", cellText(adapter, 1))
            assertTrue(!firstConnection.beginBatchEdit())
            val secondConnection = requireNotNull(input.onCreateInputConnection(EditorInfo()))
            input.setSelection(input.text.length)
            assertTrue(secondConnection.commitText("Q", 1))
            assertEquals("OtherQ", cellText(adapter, 1))
            assertEquals("Cell texttail", cellText(adapter, 0))
        }
    }

    @Test
    fun `external same-position replacement retires mounted cell connection`() = withMountedView { view, adapter, _ ->
        tapFirstCell(view)
        val connection = requireNotNull(view.activeTextInput.onCreateInputConnection(EditorInfo()))
        val replacement = tableDocument.replace("Cell text", "Replacement")
        val update = externalReplacement(adapter, replacement)
        assertTrue(view.editorEditText.applyUpdateJSON(update))
        measure(view, 600)

        assertTrue(view.activeTextInput === view.editorEditText)
        assertEquals("Replacement", firstCellText(adapter))
        assertTrue(!connection.beginBatchEdit())
    }

    @Test
    fun `reentrant replacement cannot be adopted as the local cell update`() = withMountedView { view, adapter, _ ->
        tapFirstCell(view)
        val root = view.editorEditText
        val input = view.activeTextInput
        val connection = requireNotNull(input.onCreateInputConnection(EditorInfo()))
        var replaced = false
        root.editorListener = object : EditorEditText.EditorListener {
            override fun onSelectionChanged(anchor: Int, head: Int) = Unit
            override fun onEditorUpdate(updateJSON: String) {
                if (replaced) return
                replaced = true
                val replacement = tableDocument.replace("Cell text", "Replacement")
                val next = externalReplacement(adapter, replacement)
                assertTrue(root.applyUpdateJSON(next))
            }
        }

        assertTrue(connection.commitText("Q", 1))

        assertTrue(replaced)
        assertEquals("Replacement", firstCellText(adapter))
        assertTrue(view.activeTextInput === root)
        assertTrue(!connection.beginBatchEdit())
    }

    @Test
    fun `lost table owner authority retires cell before another mutation`() = withMountedView { view, adapter, _ ->
        tapFirstCell(view)
        val connection = requireNotNull(view.activeTextInput.onCreateInputConnection(EditorInfo()))
        val before = adapter.documentJson()
        view.editorEditText.rootTableNativeOwnerAuthority = { false }
        assertTrue(!connection.beginBatchEdit())
        connection.commitText("wrong", 1)
        assertEquals(before, adapter.documentJson())
        view.applyTheme(EditorTheme.fromJson("""{"text":{"color":"#112233"}}"""))
        assertTrue(view.activeTextInput === view.editorEditText)
    }

    @Test
    fun `rebind retires mounted cell connection`() = withMountedView { view, adapter, _ ->
        tapFirstCell(view)
        val connection = requireNotNull(view.activeTextInput.onCreateInputConnection(EditorInfo()))
        val before = adapter.documentJson()
        view.editorId = 0L
        assertTrue(view.activeTextInput === view.editorEditText)
        assertTrue(!connection.beginBatchEdit())
        connection.commitText("wrong", 1)
        assertEquals(before, adapter.documentJson())
    }

    @Test
    fun `root table mounts prepared cells and reserves space before following prose`() {
        val created = UniffiEditorV2Backend.create(config, null) as EditorV2CallResult.Ok
        val adapter = requireNotNull(EditorV2Adapter.attach(
            UniffiEditorV2Backend, JSONObject(created.value).getString("editorId"), false
        ))
        val token = EditorV2Registry.register(adapter)
        try {
            val view = RichTextEditorView(RuntimeEnvironment.getApplication())
            val update = requireNotNull(adapter.setContentJson(tableDocument)) { adapter.debugNotes.toString() }
            val documentBeforeMount = adapter.documentJson()
            val historyBeforeMount = adapter.cachedHistoryState?.toString()
            val revisionBeforeMount = adapter.baseDocumentRevision
            view.editorId = token
            assertTrue(view.editorEditText.applyUpdateJSON(update))
            measure(view, 600)

            val drawing = requireNotNull(drawing(view))
            val table = requireNotNull(drawing.preparedLayout?.blocks?.singleOrNull()?.tableSurface)
            assertEquals(1, table.cells.size)
            assertTrue(table.cells.all { it.content.blocks.isNotEmpty() })
            val text = view.editorEditText.text as Spanned
            val marker = text.getSpans(0, text.length, Annotation::class.java)
                .single { it.key == RenderBridge.NATIVE_ROOT_TABLE_MARKER_ANNOTATION }
            val markerLine = view.editorEditText.layout.getLineForOffset(text.getSpanStart(marker))
            val proseLine = view.editorEditText.layout.getLineForOffset(text.toString().indexOf("after"))
            val tableBottom = drawing.top + drawing.preparedLayout!!.blocks.single().tableBounds!!.bottom
            val proseTop = view.editorEditText.top + view.editorEditText.totalPaddingTop +
                view.editorEditText.layout.getLineTop(proseLine)
            assertTrue(view.editorEditText.layout.getLineBottom(markerLine) >= table.layout.contentHeight.toInt())
            assertTrue("table bottom $tableBottom, prose top $proseTop", proseTop >= tableBottom)
            val extent = requireNotNull(adapter.cachedTableInputMappings?.tables?.values?.single()?.extent)
            assertEquals(extent.scalarEnd + 1,
                view.editorEditText.rootTablePositionMap?.globalScalar(text.toString().indexOf("after")))
            assertEquals(documentBeforeMount, adapter.documentJson())
            assertEquals(historyBeforeMount, adapter.cachedHistoryState?.toString())
            assertEquals(revisionBeforeMount, adapter.baseDocumentRevision)
        } finally {
            EditorV2Registry.remove(adapter.editorId)
            adapter.destroy()
        }
    }

    @Test
    fun `nested table snapshot mounts its outer root surface`() = withMountedView(nestedTableDocument) { view, adapter, update ->
        val input = view.editorEditText
        val mappings = requireNotNull(adapter.cachedTableInputMappings).tables
        val before = adapter.documentJson()
        val revision = adapter.baseDocumentRevision
        assertEquals(JSONObject(update).getString("documentVersion"), revision.toString())
        assertEquals(2, mappings.size)
        assertEquals(1, input.rootTableMapTableIds.size)
        assertEquals(1, input.rootTableMapExtents.size)
        assertEquals(1, (input.text as Spanned).getSpans(0, input.text.length,
            Annotation::class.java).count { it.key == RenderBridge.NATIVE_ROOT_TABLE_MARKER_ANNOTATION })
        assertEquals(adapter.baseDocumentRevision.toString(), input.lastAppliedDocumentVersion)
        assertEquals(adapter.baseDocumentRevision.toString(), input.rootTableMapDocumentVersion)
        assertEquals(adapter.positionEpoch, input.rootTableMapPositionEpoch)
        val drawing = requireNotNull(drawing(view))
        val outer = requireNotNull(drawing.preparedLayout?.blocks?.singleOrNull()?.tableSurface)
        assertEquals(3, outer.cells.size)
        val nested = requireNotNull(outer.cells[1].content.blocks.singleOrNull { it.tableSurface != null }?.tableSurface)
        assertEquals(1, nested.cells.size)
        assertTrue(nested.cells.single().content.blocks.flatMap { it.fragments }
            .any { it.layout?.text?.contains("Nested") == true })
        assertTrue(input.text.toString().contains("before"))
        assertTrue(input.text.toString().contains("after"))
        assertTrue(heightSpan(view).heightPx > input.lineHeight)
        assertEquals(before, adapter.documentJson())
        assertEquals(revision, adapter.baseDocumentRevision)
    }

    @Test
    fun `nested-only outer cell is skipped by Tab while direct cells remain editable`() =
        withMountedView(nestedTableDocument) { view, adapter, _ ->
            val original = JSONObject(requireNotNull(adapter.documentJson()))
            val originalNested = original.getJSONArray("content").getJSONObject(1)
                .getJSONArray("content").getJSONObject(0).getJSONArray("content")
                .getJSONObject(1).toString()
            tapFirstCell(view)
            val input = view.activeTextInput
            assertEquals("Alpha", input.text.toString())
            assertTrue(input.dispatchKeyEvent(KeyEvent(100L, 100L,
                KeyEvent.ACTION_DOWN, KeyEvent.KEYCODE_TAB, 0)))
            assertTrue(input === view.activeTextInput)
            assertEquals("Owner", input.text.toString())
            input.setSelection(input.text.length)
            assertTrue(requireNotNull(input.onCreateInputConnection(EditorInfo())).commitText("!", 1))
            assertEquals("Owner!", cellText(adapter, 2))
            assertTrue(input.dispatchKeyEvent(KeyEvent(200L, 200L,
                KeyEvent.ACTION_DOWN, KeyEvent.KEYCODE_TAB, 0, KeyEvent.META_SHIFT_ON)))
            assertEquals("Alpha", input.text.toString())
            val after = JSONObject(requireNotNull(adapter.documentJson())).getJSONArray("content")
            val outer = after.getJSONObject(1).getJSONArray("content").getJSONObject(0)
                .getJSONArray("content")
            assertEquals(originalNested, outer.getJSONObject(1).toString())
            assertEquals("before", after.getJSONObject(0).getJSONArray("content")
                .getJSONObject(0).getString("text"))
            assertEquals("after", after.getJSONObject(2).getJSONArray("content")
                .getJSONObject(0).getString("text"))
        }

    @Test
    fun `nested snapshot with mismatched root identity or extent clears surface`() =
        withMountedView(nestedTableDocument) { view, adapter, _ ->
            val input = view.editorEditText
            val rootIds = input.rootTableMapTableIds
            val rootExtents = input.rootTableMapExtents
            val mappings = requireNotNull(adapter.cachedTableInputMappings)
            assertNotNull(drawing(view))

            adapter.cachedTableInputMappings = TableInputMappings(mappings.tables - rootIds.single())
            view.requestLayout()
            measure(view, 600)
            assertNull(drawing(view))

            adapter.cachedTableInputMappings = mappings
            view.requestLayout()
            measure(view, 600)
            assertNotNull(drawing(view))

            input.rootTableMapTableIds = setOf("wrong-root")
            view.requestLayout()
            measure(view, 600)
            assertNull(drawing(view))

            input.rootTableMapTableIds = rootIds
            view.requestLayout()
            measure(view, 600)
            assertNotNull(drawing(view))

            val rootId = rootIds.single()
            val extent = requireNotNull(rootExtents[rootId])
            input.rootTableMapExtents = mapOf(rootId to TableInputExtent(extent.scalarStart, extent.scalarEnd - 1))
            view.requestLayout()
            measure(view, 600)
            assertNull(drawing(view))
        }

    @Test
    fun `table reflows on width and theme change then clears on removal and rebind`() {
        val created = UniffiEditorV2Backend.create(config, null) as EditorV2CallResult.Ok
        val adapter = requireNotNull(EditorV2Adapter.attach(
            UniffiEditorV2Backend, JSONObject(created.value).getString("editorId"), false
        ))
        val token = EditorV2Registry.register(adapter)
        try {
            val update = requireNotNull(adapter.setContentJson(tableDocument))
            val view = RichTextEditorView(RuntimeEnvironment.getApplication())
            view.editorId = token
            assertTrue(view.editorEditText.applyUpdateJSON(update))
            measure(view, 600)
            val wide = requireNotNull(drawing(view)?.preparedLayout?.blocks?.single()?.tableSurface)
            val wideWidth = wide.layout.contentWidth
            val wideHeight = wide.layout.contentHeight

            measure(view, 320)
            val narrow = requireNotNull(drawing(view)?.preparedLayout?.blocks?.single()?.tableSurface)
            assertTrue(narrow.layout.contentWidth < wideWidth)
            assertTrue(narrow !== wide)
            assertTrue(narrow.layout.contentHeight >= wideHeight)

            view.applyTheme(EditorTheme.fromJson("""{"table":{"cellPadding":20,"borderWidth":2}}"""))
            measure(view, 320)
            val themed = requireNotNull(drawing(view)?.preparedLayout?.blocks?.single()?.tableSurface)
            assertTrue(themed !== narrow)
            assertEquals(20f * view.resources.displayMetrics.density, themed.style.cellPadding)

            view.editorId = 0L
            assertTrue(drawing(view) == null)
            view.editorId = token
            measure(view, 320)
            assertNotNull(drawing(view))

            val prose = """{"type":"doc","content":[{"type":"paragraph","content":[{"type":"text","text":"plain"}]}]}"""
            assertTrue(view.editorEditText.applyUpdateJSON(requireNotNull(adapter.setContentJson(prose))))
            measure(view, 320)
            assertTrue(drawing(view) == null)
            assertTrue(view.editorEditText.text.getSpans(0, view.editorEditText.text.length,
                com.apollohg.editor.tables.RootTableHeightSpan::class.java).isEmpty())

        } finally {
            EditorV2Registry.remove(adapter.editorId)
            adapter.destroy()
        }
    }

    @Test
    fun `same revision root redraw restores measured marker reservation`() = withMountedView { view, adapter, update ->
        val original = heightSpan(view)
        val originalHeight = original.heightPx
        val revision = adapter.baseDocumentRevision
        val document = adapter.documentJson()

        assertTrue(view.editorEditText.applyUpdateJSON(update))
        measure(view, 600)

        val restored = heightSpan(view)
        assertNotSame(original, restored)
        assertEquals(originalHeight, restored.heightPx)
        assertNotNull(drawing(view)?.preparedLayout?.blocks?.singleOrNull()?.tableSurface)
        assertEquals(document, adapter.documentJson())
        assertEquals(revision, adapter.baseDocumentRevision)
    }

    @Test
    fun `reflow during composition retains canonical authorized text`() = withMountedView { view, _, _ ->
        val input = view.editorEditText
        val canonical = input.lastAuthorizedText
        val originalHeight = heightSpan(view).heightPx
        input.setSelection(input.text.length)
        val connection = requireNotNull(input.onCreateInputConnection(EditorInfo()))
        assertTrue(connection.setComposingText("transient", 1))
        assertTrue(input.text.toString() != canonical)

        measure(view, 320)

        assertEquals(canonical, input.lastAuthorizedText)
        assertEquals(canonical, input.lastAuthorizedRenderedText.toString())
        assertTrue(input.text.toString().contains("transient"))
        input.restoreAuthorizedTextSnapshotForEditor()
        measure(view, 320)
        assertEquals(canonical, input.text.toString())
        assertNotNull(drawing(view)?.preparedLayout?.blocks?.singleOrNull()?.tableSurface)
        assertTrue(heightSpan(view).heightPx >= originalHeight)
    }

    @Test
    @GraphicsMode(GraphicsMode.Mode.NATIVE)
    fun `wrapping table reflow during composition keeps following prose below live table`() {
        val longCell = "A long table cell sentence with enough words to wrap on a narrow editor. ".repeat(5)
        withMountedView(tableDocument.replace("Cell text", longCell)) { view, adapter, _ ->
            val input = view.editorEditText
            val wideHeight = requireNotNull(drawing(view)?.preparedLayout?.blocks?.single()?.tableSurface)
                .layout.contentHeight
            val canonical = input.lastAuthorizedText
            val document = adapter.documentJson()
            val history = adapter.cachedHistoryState?.toString()
            input.setSelection(input.text.length)
            val connection = requireNotNull(input.onCreateInputConnection(EditorInfo()))
            assertTrue(connection.setComposingText("transient", 1))
            assertTrue(input.text.toString() != canonical)

            measure(view, 240)

            val drawing = requireNotNull(drawing(view))
            val table = requireNotNull(drawing.preparedLayout?.blocks?.single()?.tableSurface)
            assertTrue("wide table $wideHeight, narrow table ${table.layout.contentHeight}",
                table.layout.contentHeight > wideHeight)
            val marker = (input.text as Spanned).getSpans(0, input.text.length,
                Annotation::class.java).single {
                it.key == RenderBridge.NATIVE_ROOT_TABLE_MARKER_ANNOTATION
            }
            val markerLine = input.layout.getLineForOffset(input.text.getSpanStart(marker))
            val proseLine = input.layout.getLineForOffset(input.text.toString().indexOf("after"))
            val tableBottom = drawing.top + drawing.preparedLayout!!.blocks.single().tableBounds!!.bottom
            val proseTop = input.top + input.totalPaddingTop + input.layout.getLineTop(proseLine)
            assertTrue("live marker ${input.layout.getLineBottom(markerLine)}, prepared table ${table.layout.contentHeight}",
                input.layout.getLineBottom(markerLine) >= table.layout.contentHeight.toInt())
            assertTrue("table bottom $tableBottom, prose top $proseTop", proseTop >= tableBottom)
            assertEquals(canonical, input.lastAuthorizedText)
            assertEquals(canonical, input.lastAuthorizedRenderedText.toString())
            assertEquals(document, adapter.documentJson())
            assertEquals(history, adapter.cachedHistoryState?.toString())
        }
    }

    @Test
    fun `loss of adopted mappings clears mounted table without changing root text`() = withMountedView { view, adapter, _ ->
        val input = view.editorEditText
        val visibleText = input.text.toString()
        val revision = adapter.baseDocumentRevision
        assertNotNull(drawing(view))
        assertNotNull(adapter.cachedTableInputMappings)

        adapter.releaseNativeBindingOwner(input.nativeBindingToken)
        assertNull(adapter.cachedTableInputMappings)
        input.setSelection(visibleText.length)

        assertNull(drawing(view))
        assertEquals(visibleText, input.text.toString())
        assertEquals(revision, adapter.baseDocumentRevision)
    }

    @Test
    fun `zero leaf table beside populated table leaves populated host visible`() {
        val document = """{"type":"doc","content":[{"type":"table","content":[{"type":"table_row","content":[]}]},{"type":"table","content":[{"type":"table_row","content":[{"type":"table_cell","content":[{"type":"paragraph","content":[{"type":"text","text":"visible cell"}]}]}]}]},{"type":"paragraph","content":[{"type":"text","text":"after"}]}]}"""
        withMountedView(document) { view, adapter, _ ->
            val mappings = requireNotNull(adapter.cachedTableInputMappings).tables
            assertEquals(2, mappings.size)
            assertEquals(1, mappings.values.count { it.extent == null })
            assertEquals(1, mappings.values.count { it.extent != null })
            assertEquals(1, view.editorEditText.rootTableMapExtents.size)
            val layout = requireNotNull(drawing(view)?.preparedLayout)
            assertEquals(1, layout.blocks.size)
            assertEquals(1, requireNotNull(layout.blocks.single().tableSurface).cells.size)
            assertTrue(heightSpan(view).heightPx > view.editorEditText.lineHeight)
        }
    }
}
