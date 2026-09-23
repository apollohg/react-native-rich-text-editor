package com.apollohg.editor

import android.text.Annotation
import android.text.Spanned
import android.view.View
import android.view.inputmethod.EditorInfo
import com.apollohg.editor.tables.RootTableHeightSpan
import com.apollohg.editor.viewer.PreparedProseDrawingView
import org.json.JSONObject
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNotSame
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
