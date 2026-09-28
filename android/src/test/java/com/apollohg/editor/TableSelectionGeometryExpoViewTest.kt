package com.apollohg.editor

import android.app.Activity
import android.graphics.Rect
import android.graphics.RectF
import android.os.Looper
import android.view.MotionEvent
import android.view.View
import android.widget.FrameLayout
import androidx.core.graphics.Insets
import androidx.core.view.WindowInsetsCompat
import com.apollohg.editor.viewer.PreparedProseDrawingView
import java.time.Duration
import org.json.JSONObject
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotEquals
import org.junit.Assert.assertNotSame
import org.junit.Assert.assertSame
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
            "editorId", "documentRevision", "layoutEpoch", "tablePos", "coordinateSpace", "rects", "viewport",
            "safeArea", "editMenuVisible"
        )
        const val STATUS_BAR_PX = 48
        const val NAVIGATION_BAR_PX = 96
        const val KEYBOARD_PX = 600
        const val OUTSIDE_TOUCH_INSET = 4f
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

        fun expectedRects(sourceIndices: Set<Int>? = null): List<RectF> {
            val visible = Rect()
            assertTrue(drawing.getLocalVisibleRect(visible))
            val selected = sourceIndices ?: requireNotNull(drawing.selectedTableCellSourceIndices[tableId])
            return drawing.presentedTableCells().filter {
                it.surface.editorTableId == "t$tablePos" && it.sourceIndex in selected
            }.mapNotNull { cell ->
                RectF(cell.bounds).takeIf { it.intersect(cell.clip) && it.intersect(RectF(visible)) }
            }.map(::windowRect)
        }

        fun cellWindowRect(index: Int): RectF = windowRect(RectF(drawing.presentedTableCells().first {
            it.surface.editorTableId == "t$tablePos" && it.sourceIndex == index
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
        val safeArea = rect(payload["safeArea"])
        assertTrue("the visible editor lies inside the window safe area $safeArea", safeArea.contains(viewport))
        assertFalse("no keyboard is reported while the IME is hidden", payload.containsKey("keyboard"))
        assertEquals("no native edit menu is showing", false, payload["editMenuVisible"])
    }

    @Test
    fun `a window touch outside the editor closes the cell edit menu`() =
        withFocusedTable(fourCellTable) { fixture ->
            fixture.selectCells(0, 3)
            fixture.nextFrame()
            val surface = fixture.view.richTextView.editorTableSurface
            surface.presentCellEditMenu()
            assertTrue(surface.isCellEditMenuVisible)

            fun windowTouch(x: Float, y: Float): NativeEditorOutsideTapDecision {
                val event = MotionEvent.obtain(0L, 0L, MotionEvent.ACTION_DOWN, x, y, 0)
                try {
                    val decision = fixture.view.prepareOutsideTapDecisionForWindowEvent(event)
                    fixture.view.handleOutsideTapDecisionFromWindowDispatcher(decision)
                    return decision
                } finally { event.recycle() }
            }
            val inside = IntArray(2).also(fixture.view.richTextView::getLocationOnScreen)
            assertEquals(NativeEditorOutsideTapDecision.PRESERVE_FOCUS,
                windowTouch(inside[0] + OUTSIDE_TOUCH_INSET, inside[1] + OUTSIDE_TOUCH_INSET))
            assertTrue("a touch inside the editor keeps the menu", surface.isCellEditMenuVisible)

            assertEquals(NativeEditorOutsideTapDecision.OUTSIDE_EDITOR,
                windowTouch(OUTSIDE_TOUCH_INSET, OUTSIDE_TOUCH_INSET))
            assertFalse("a touch outside the editor closes the menu", surface.isCellEditMenuVisible)
            fixture.view.cancelOutsideTapBlurFromWindowDispatcher()
        }

    @Test
    fun `native cell edit menu visibility is republished so the toolbar yields`() =
        withFocusedTable(fourCellTable) { fixture ->
            fixture.selectCells(0, 3)
            fixture.nextFrame()
            assertEquals(1, fixture.payloads.size)
            val surface = fixture.view.richTextView.editorTableSurface

            surface.presentCellEditMenu()
            assertTrue("the menu presents over the focused cell selection", surface.isCellEditMenuVisible)
            fixture.nextFrame()
            assertEquals("showing the menu republishes once: ${fixture.payloads}", 2, fixture.payloads.size)
            assertEquals(true, fixture.payloads[1]["editMenuVisible"])
            assertRects("the menu does not move the selection geometry", rects(fixture.payloads[0]),
                rects(fixture.payloads[1]))

            surface.dismissCellEditMenu()
            fixture.nextFrame()
            assertEquals("${fixture.payloads}", 3, fixture.payloads.size)
            assertEquals("the toolbar returns once the menu closes", false, fixture.payloads[2]["editMenuVisible"])
        }

    @Test
    fun `window insets republish the safe area and the IME rectangle in dp window space`() =
        withFocusedTable(fourCellTable) { fixture ->
            fixture.selectCells(0, 3)
            fixture.nextFrame()
            assertEquals(1, fixture.payloads.size)
            val window = fixture.view.rootView
            val density = fixture.view.resources.displayMetrics.density
            val bars = Insets.of(0, STATUS_BAR_PX, 0, NAVIGATION_BAR_PX)
            val withKeyboard = WindowInsetsCompat.Builder()
                .setInsets(WindowInsetsCompat.Type.systemBars(), bars)
                .setInsets(WindowInsetsCompat.Type.ime(), Insets.of(0, 0, 0, KEYBOARD_PX))
                .build()

            fixture.view.rootWindowInsetsForTesting = withKeyboard
            fixture.view.dispatchApplyWindowInsets(requireNotNull(withKeyboard.toWindowInsets()))
            assertTrue("an insets change schedules a geometry frame",
                fixture.view.tableSelectionGeometryPublisher.hasScheduledFlushForTesting)
            fixture.nextFrame()

            assertEquals("${fixture.payloads}", 2, fixture.payloads.size)
            val shown = fixture.payloads[1]
            assertRects("the safe area excludes the system bars",
                listOf(RectF(0f, STATUS_BAR_PX / density, window.width / density,
                    (window.height - NAVIGATION_BAR_PX) / density)),
                listOf(rect(shown["safeArea"])))
            assertRects("the keyboard is the IME rectangle at the window bottom",
                listOf(RectF(0f, (window.height - KEYBOARD_PX) / density, window.width / density,
                    window.height / density)),
                listOf(rect(shown["keyboard"])))

            val withoutKeyboard = WindowInsetsCompat.Builder()
                .setInsets(WindowInsetsCompat.Type.systemBars(), bars)
                .build()
            fixture.view.rootWindowInsetsForTesting = withoutKeyboard
            fixture.view.dispatchApplyWindowInsets(requireNotNull(withoutKeyboard.toWindowInsets()))
            fixture.nextFrame()

            assertEquals("${fixture.payloads}", 3, fixture.payloads.size)
            assertFalse("a hidden IME no longer obstructs", fixture.payloads[2].containsKey("keyboard"))
            assertEquals(GEOMETRY_KEYS, fixture.payloads[2].keys)
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
    fun `parent moving the host republishes shifted rects once`() = withFocusedTable(fourCellTable) { fixture ->
        fixture.selectCells(0, 3)
        fixture.nextFrame()
        assertEquals(1, fixture.payloads.size)
        val before = rects(fixture.payloads[0])
        val shiftPx = 40
        val density = fixture.view.resources.displayMetrics.density

        val params = fixture.view.layoutParams as FrameLayout.LayoutParams
        params.topMargin += shiftPx
        fixture.view.layoutParams = params
        fixture.nextFrame()

        assertEquals("${fixture.payloads}", 2, fixture.payloads.size)
        val after = rects(fixture.payloads[1])
        assertRects("rects follow the moved host",
            before.map { RectF(it).apply { offset(0f, shiftPx / density) } }, after)
        assertRects("rects match the drawn selection", fixture.expectedRects(), after)
        assertRects("viewport follows the moved host", listOf(fixture.expectedViewport()),
            listOf(rect(fixture.payloads[1]["viewport"])))
    }

    @Test
    fun `detached host stops observing window layout`() = withFocusedTable(fourCellTable) { fixture ->
        fixture.selectCells(0, 3)
        fixture.nextFrame()
        val parent = fixture.view.parent as FrameLayout
        val tree = parent.viewTreeObserver

        parent.removeView(fixture.view)
        assertEquals(mapOf("editorId" to fixture.adapter.editorId), fixture.payloads.last())
        val published = fixture.payloads.size
        tree.dispatchOnGlobalLayout()

        assertFalse(fixture.view.tableSelectionGeometryPublisher.hasScheduledFlushForTesting)
        fixture.nextFrame()
        assertEquals(published, fixture.payloads.size)
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

    @Test
    fun `caret in a tapped cell publishes the active cell geometry`() = withFocusedTable(fourCellTable) { fixture ->
        assertEquals("a prose caret has no geometry", emptyList<Map<String, Any>>(), fixture.payloads)
        tapCell(fixture.view, 1)
        val input = fixture.view.richTextView.activeTextInput
        assertNotSame("the tapped cell owns the cell input", fixture.view.richTextView.editorEditText, input)
        assertTrue(input.hasFocus())
        assertEquals("activation leaves a caret", input.selectionStart, input.selectionEnd)
        assertTrue("a caret draws no cell rectangle", fixture.drawing.selectedTableCellSourceIndices.isEmpty())
        fixture.nextFrame()

        assertEquals("${fixture.payloads}", 1, fixture.payloads.size)
        val payload = fixture.payloads.single()
        assertEquals(fixture.adapter.editorId, payload["editorId"])
        assertEquals(fixture.tablePos.toLong(), payload["tablePos"])
        assertRects("the active cell anchors the table toolbar",
            fixture.expectedRects(setOf(1)), rects(payload))

        fixture.view.richTextView.editorTableSurface.invalidateCell()
        fixture.nextFrame()
        assertEquals("${fixture.payloads}", 2, fixture.payloads.size)
        assertEquals("releasing the cell clears its geometry", mapOf("editorId" to fixture.adapter.editorId),
            fixture.payloads[1])
    }

    @Test
    fun `blur clears the active cell geometry and keeps the cell bound`() = withFocusedTable(fourCellTable) { fixture ->
        tapCell(fixture.view, 1)
        val input = fixture.view.richTextView.activeTextInput
        assertTrue(input.hasFocus())
        fixture.nextFrame()
        assertEquals("${fixture.payloads}", 1, fixture.payloads.size)

        fixture.view.blur()
        assertFalse(input.hasFocus())
        assertEquals("blur clears synchronously: ${fixture.payloads}", 2, fixture.payloads.size)
        assertEquals(mapOf("editorId" to fixture.adapter.editorId), fixture.payloads[1])
        assertSame("blur keeps the cell bound", input, fixture.view.richTextView.activeTextInput)
        fixture.nextFrame()
        assertEquals("a blurred cell publishes no geometry", 2, fixture.payloads.size)
    }
}
