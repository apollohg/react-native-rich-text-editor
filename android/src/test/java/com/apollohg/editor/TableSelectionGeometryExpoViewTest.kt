package com.apollohg.editor

import android.app.Activity
import android.graphics.Rect
import android.graphics.RectF
import android.os.Looper
import android.view.View
import android.widget.FrameLayout
import com.apollohg.editor.viewer.PreparedProseDrawingView
import java.time.Duration
import org.json.JSONObject
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotEquals
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.Robolectric
import org.robolectric.RobolectricTestRunner
import org.robolectric.Shadows.shadowOf
import org.robolectric.annotation.Config

@RunWith(RobolectricTestRunner::class)
@Config(sdk = [34], qualifiers = "xhdpi")
internal class TableSelectionGeometryExpoViewTest : NativeEditorExpoViewTestSupport() {
    private companion object {
        const val HOST_LEFT = 48
        const val HOST_TOP = 144
        const val HOST_WIDTH = 560
        const val HOST_HEIGHT = 520
        const val RECT_TOLERANCE = 0.01
        val FRAME: Duration = Duration.ofMillis(50)
        val GEOMETRY_KEYS = setOf(
            "editorId", "documentRevision", "layoutEpoch", "tablePos", "coordinateSpace", "rects", "viewport"
        )
    }

    private val config = """{"schema":{"nodes":[{"name":"doc","content":"block+","role":"doc"},{"name":"paragraph","content":"inline*","group":"block","role":"textBlock"},{"name":"text","content":"","group":"inline","role":"text"},{"name":"table","content":"table_row+","group":"block","role":"block","tableRole":"table"},{"name":"table_row","content":"(table_cell | table_header)*","role":"block","tableRole":"row"},{"name":"table_cell","content":"block+","role":"block","tableRole":"cell","attrs":{"colspan":{"type":"number","default":1,"min":1},"rowspan":{"type":"number","default":1,"min":1},"colwidth":{"default":null}}},{"name":"table_header","content":"block+","role":"block","tableRole":"header_cell","attrs":{"colspan":{"type":"number","default":1,"min":1},"rowspan":{"type":"number","default":1,"min":1},"colwidth":{"default":null}}}],"marks":[]},"initialization":{"type":"localEmpty"}}"""
    private val fourCellTable = """{"type":"doc","content":[{"type":"paragraph","content":[{"type":"text","text":"before"}]},{"type":"table","content":[{"type":"table_row","content":[{"type":"table_cell","content":[{"type":"paragraph","content":[{"type":"text","text":"one"}]}]},{"type":"table_cell","content":[{"type":"paragraph","content":[{"type":"text","text":"two"}]}]}]},{"type":"table_row","content":[{"type":"table_cell","content":[{"type":"paragraph","content":[{"type":"text","text":"three"}]}]},{"type":"table_cell","content":[{"type":"paragraph","content":[{"type":"text","text":"four"}]}]}]}]}]}"""
    private val wideTwoCellTable = """{"type":"doc","content":[{"type":"paragraph","content":[{"type":"text","text":"before"}]},{"type":"table","content":[{"type":"table_row","content":[{"type":"table_cell","attrs":{"colwidth":[500]},"content":[{"type":"paragraph","content":[{"type":"text","text":"one"}]}]},{"type":"table_cell","attrs":{"colwidth":[500]},"content":[{"type":"paragraph","content":[{"type":"text","text":"two"}]}]}]}]}]}"""

    private inner class Fixture(
        val view: NativeEditorExpoView,
        val adapter: EditorV2Adapter,
        val tableId: String,
        val tablePos: Int,
        val positions: List<Int>,
        val payloads: MutableList<Map<String, Any>>
    ) {
        val drawing: PreparedProseDrawingView
            get() = (0 until view.richTextView.editorContentFrame.childCount)
                .map { view.richTextView.editorContentFrame.getChildAt(it) }
                .filterIsInstance<PreparedProseDrawingView>().single()
        private val density get() = view.resources.displayMetrics.density

        fun nextFrame() = shadowOf(Looper.getMainLooper()).idleFor(FRAME)

        fun select(selection: JSONObject) {
            val admitted = adapter.callWithEnvelope(JSONObject().put("selection", selection)) {
                UniffiEditorV2Backend.setSelection(adapter.editorId, it)
            }
            assertTrue("engine rejected selection $selection: $admitted", admitted is EditorV2CallResult.Ok)
            assertTrue(view.richTextView.editorEditText.applyUpdateJSON(requireNotNull(adapter.refreshFromRustState(null))))
        }

        fun selectCells(anchor: Int, head: Int) {
            fun point(index: Int) = JSONObject().put("kind", "document").put("offset", positions[index])
            select(JSONObject().put("type", "cell").put("anchorCell", point(anchor)).put("headCell", point(head)))
        }

        fun selectText(anchor: Int, head: Int) {
            fun point(offset: Int) = JSONObject().put("kind", "scalar").put("offset", offset)
            select(JSONObject().put("type", "text").put("anchor", point(anchor)).put("head", point(head)))
        }

        private fun windowRect(drawingRect: RectF): RectF {
            val drawingOrigin = Rect()
            view.offsetDescendantRectToMyCoords(drawing, drawingOrigin)
            val host = IntArray(2).also(view::getLocationInWindow)
            val dx = drawingOrigin.left + host[0]
            val dy = drawingOrigin.top + host[1]
            return RectF((drawingRect.left + dx) / density, (drawingRect.top + dy) / density,
                (drawingRect.right + dx) / density, (drawingRect.bottom + dy) / density)
        }

        fun expectedViewport(): RectF {
            val visible = Rect()
            assertTrue(drawing.getGlobalVisibleRect(visible))
            return RectF(visible.left / density, visible.top / density, visible.right / density,
                visible.bottom / density)
        }

        fun expectedRects(): List<RectF> {
            val visible = Rect()
            assertTrue(drawing.getLocalVisibleRect(visible))
            val selected = requireNotNull(drawing.selectedTableCellSourcePositions[tableId])
            return drawing.presentedTableCells().filter {
                it.surface.sourceTable?.tablePos?.toInt() == tablePos && it.sourcePosition in selected
            }.mapNotNull { cell ->
                RectF(cell.bounds).takeIf { it.intersect(cell.clip) && it.intersect(RectF(visible)) }
            }.map(::windowRect)
        }

        fun cellWindowRect(index: Int): RectF = windowRect(RectF(drawing.presentedTableCells().first {
            it.surface.sourceTable?.tablePos?.toInt() == tablePos && it.sourcePosition == positions[index]
        }.bounds))
    }

    @Suppress("UNCHECKED_CAST")
    private fun rect(raw: Any?): RectF {
        val values = raw as Map<String, Double>
        assertEquals(setOf("x", "y", "width", "height"), values.keys)
        val x = requireNotNull(values["x"]).toFloat()
        val y = requireNotNull(values["y"]).toFloat()
        return RectF(x, y, x + requireNotNull(values["width"]).toFloat(), y + requireNotNull(values["height"]).toFloat())
    }

    @Suppress("UNCHECKED_CAST")
    private fun rects(payload: Map<String, Any>): List<RectF> =
        (payload["rects"] as List<Any?>).map(::rect)

    private fun assertRects(message: String, expected: List<RectF>, actual: List<RectF>) {
        val matches = expected.size == actual.size && expected.zip(actual).all { (lhs, rhs) ->
            listOf(lhs.left - rhs.left, lhs.top - rhs.top, lhs.right - rhs.right, lhs.bottom - rhs.bottom)
                .all { kotlin.math.abs(it) <= RECT_TOLERANCE }
        }
        assertTrue("$message expected=$expected actual=$actual", matches)
    }

    private fun withFocusedTable(document: String, block: (Fixture) -> Unit) {
        val activity = Robolectric.buildActivity(Activity::class.java).setup().get()
        val created = UniffiEditorV2Backend.create(config, null) as EditorV2CallResult.Ok
        val adapter = requireNotNull(EditorV2Adapter.attach(
            UniffiEditorV2Backend, JSONObject(created.value).getString("editorId"), false))
        val update = requireNotNull(adapter.setContentJson(document))
        val token = EditorV2Registry.register(adapter)
        try {
            val expo = testExpoContext(activity)
            val view = NativeEditorExpoView(expo.context, expo.appContext)
            view.onFocusChangeForTesting = {}
            view.onAddonEventForTesting = {}
            view.onEditorReadyForTesting = {}
            view.onEditorUpdateForTesting = {}
            view.onSelectionChangeForTesting = {}
            view.onContentHeightChangeForTesting = {}
            view.onAtomLayoutForTesting = {}
            val payloads = mutableListOf<Map<String, Any>>()
            view.onTableSelectionGeometryForTesting = { payloads += it }
            val host = FrameLayout(activity)
            activity.setContentView(host)
            host.addView(view, FrameLayout.LayoutParams(HOST_WIDTH, HOST_HEIGHT).apply {
                leftMargin = HOST_LEFT
                topMargin = HOST_TOP
            })
            view.setAttachedToNativeWindowForTesting(true)
            view.setEditorId(token)
            assertTrue(view.richTextView.editorEditText.applyUpdateJSON(update))
            shadowOf(Looper.getMainLooper()).idle()
            assertTrue(view.richTextView.editorEditText.requestFocus())
            val root = adapter.cachedTableRecords.values.minBy { it.getInt("tablePos") }
            val cells = root.getJSONArray("cells")
            val fixture = Fixture(view, adapter, "t${root.getInt("tablePos")}", root.getInt("tablePos"),
                (0 until cells.length()).map { cells.getJSONObject(it).getInt("sourcePos") }, payloads)
            fixture.nextFrame()
            block(fixture)
        } finally {
            EditorV2Registry.remove(adapter.editorId)
            adapter.destroy()
        }
    }

    @Test
    fun `cell selection publishes window geometry on the next frame`() = withFocusedTable(fourCellTable) { fixture ->
        assertEquals("a caret selection has no geometry", emptyList<Map<String, Any>>(), fixture.payloads)
        fixture.selectCells(0, 3)
        assertEquals("emission waits for the next frame", 0, fixture.payloads.size)
        fixture.nextFrame()

        assertEquals("${fixture.payloads}", 1, fixture.payloads.size)
        val payload = fixture.payloads.single()
        assertEquals(GEOMETRY_KEYS, payload.keys)
        assertEquals(fixture.adapter.editorId, payload["editorId"])
        assertEquals(fixture.adapter.baseDocumentRevision.toString(), payload["documentRevision"])
        assertEquals(fixture.adapter.positionEpoch, payload["layoutEpoch"])
        assertEquals(fixture.tablePos.toLong(), payload["tablePos"])
        assertEquals("window", payload["coordinateSpace"])
        val rects = rects(payload)
        assertEquals("all four selected cells are visible", 4, rects.size)
        assertRects("rects are dp window rects of the drawn selection", fixture.expectedRects(), rects)
        val viewport = rect(payload["viewport"])
        assertRects("viewport is the visible editor in window space", listOf(fixture.expectedViewport()),
            listOf(viewport))
        assertTrue("the host offset reaches window space: $viewport", viewport.left >= HOST_LEFT / 2f &&
            viewport.top >= HOST_TOP / 2f)
        rects.forEach { assertTrue("$it lies inside $viewport", viewport.contains(it)) }
    }

    @Test
    fun `horizontal table scroll moves rects and coalesces steps within one frame`() =
        withFocusedTable(wideTwoCellTable) { fixture ->
            fixture.selectCells(0, 1)
            fixture.nextFrame()
            assertEquals("${fixture.payloads}", 1, fixture.payloads.size)
            val before = rects(fixture.payloads[0])
            val density = fixture.view.resources.displayMetrics.density
            repeat(3) {
                assertEquals(100f * density,
                    fixture.drawing.scrollSelectedTablePhysical(fixture.tableId, 100f * density), 0.01f)
            }
            assertEquals("scroll steps inside one frame stay queued", 1, fixture.payloads.size)
            fixture.nextFrame()

            assertEquals("three scroll steps coalesce into one event", 2, fixture.payloads.size)
            val after = rects(fixture.payloads[1])
            assertRects("rects follow the scrolled table", fixture.expectedRects(), after)
            assertNotEquals(before, after)
            assertEquals("the first cell's trailing edge is reported where the scrolled table draws it",
                fixture.cellWindowRect(0).right, after.first().right, 0.01f)
            assertEquals(fixture.payloads[0]["tablePos"], fixture.payloads[1]["tablePos"])
        }

    @Test
    fun `unchanged geometry is never published twice`() = withFocusedTable(fourCellTable) { fixture ->
        fixture.selectCells(0, 1)
        fixture.nextFrame()
        assertEquals(1, fixture.payloads.size)

        fixture.view.tableSelectionGeometryPublisher.flush()
        fixture.view.richTextView.requestLayout()
        shadowOf(Looper.getMainLooper()).idle()
        fixture.view.tableSelectionGeometryPublisher.scheduleFlush()
        fixture.nextFrame()

        assertEquals("${fixture.payloads}", 1, fixture.payloads.size)
    }

    @Test
    fun `blur clears geometry and suppresses it until refocus`() = withFocusedTable(wideTwoCellTable) { fixture ->
        fixture.selectCells(0, 1)
        fixture.nextFrame()
        assertEquals(1, fixture.payloads.size)

        val outside = View(fixture.view.context).apply {
            isFocusable = true
            isFocusableInTouchMode = true
        }
        (fixture.view.parent as FrameLayout).addView(outside, FrameLayout.LayoutParams(HOST_LEFT, HOST_LEFT))
        assertTrue(outside.requestFocus())
        assertFalse(fixture.view.richTextView.editorEditText.hasFocus())
        assertEquals("blur clears synchronously", 2, fixture.payloads.size)
        assertEquals(mapOf("editorId" to fixture.adapter.editorId), fixture.payloads[1])
        fixture.drawing.scrollSelectedTablePhysical(fixture.tableId, 120f)
        fixture.nextFrame()
        assertEquals("a blurred editor publishes no geometry", 2, fixture.payloads.size)

        assertTrue(fixture.view.richTextView.editorEditText.requestFocus())
        fixture.nextFrame()
        assertEquals(3, fixture.payloads.size)
        assertRects("refocus republishes the current geometry", fixture.expectedRects(), rects(fixture.payloads[2]))
    }

    @Test
    fun `binding change clears geometry under the previous editor`() = withFocusedTable(fourCellTable) { fixture ->
        fixture.selectCells(0, 3)
        fixture.nextFrame()
        assertEquals(1, fixture.payloads.size)

        fixture.view.setEditorId(0L)

        assertEquals(2, fixture.payloads.size)
        assertEquals(mapOf("editorId" to fixture.adapter.editorId), fixture.payloads[1])
        assertFalse(fixture.view.tableSelectionGeometryPublisher.hasScheduledFlushForTesting)
        fixture.nextFrame()
        assertEquals(2, fixture.payloads.size)
    }

    @Test
    fun `editor destruction clears geometry`() = withFocusedTable(fourCellTable) { fixture ->
        fixture.selectCells(1, 2)
        fixture.nextFrame()
        assertEquals(1, fixture.payloads.size)

        fixture.view.handleEditorDestroyed(fixture.view.richTextView.editorId)

        assertEquals(2, fixture.payloads.size)
        assertEquals(mapOf("editorId" to fixture.adapter.editorId), fixture.payloads[1])
        fixture.nextFrame()
        assertEquals(2, fixture.payloads.size)
    }

    @Test
    fun `text selections publish nothing and end a cell selection's geometry`() =
        withFocusedTable(fourCellTable) { fixture ->
            fixture.selectText(1, 4)
            fixture.nextFrame()
            assertEquals("${fixture.payloads}", 0, fixture.payloads.size)

            fixture.selectCells(0, 1)
            fixture.nextFrame()
            assertEquals(1, fixture.payloads.size)

            fixture.selectText(2, 2)
            fixture.nextFrame()
            assertEquals("${fixture.payloads}", 2, fixture.payloads.size)
            assertEquals(mapOf("editorId" to fixture.adapter.editorId), fixture.payloads[1])
        }
}
