package com.apollohg.editor

import android.app.Activity
import android.content.ClipData
import android.content.pm.ApplicationInfo
import android.text.Annotation
import android.text.Spanned
import android.text.style.ForegroundColorSpan
import android.graphics.Color
import android.graphics.drawable.ColorDrawable
import android.view.DragEvent
import android.view.View
import android.view.MotionEvent
import android.view.InputDevice
import android.view.KeyEvent
import android.view.ViewConfiguration
import android.view.accessibility.AccessibilityEvent
import android.view.accessibility.AccessibilityNodeInfo
import android.view.inputmethod.EditorInfo
import android.view.inputmethod.InputConnection
import android.widget.FrameLayout
import com.apollohg.editor.tables.RootTableHeightSpan
import com.apollohg.editor.viewer.PreparedProseDrawingView
import java.util.concurrent.TimeUnit
import kotlin.math.floor
import kotlin.math.hypot
import kotlin.math.sqrt
import com.apollohg.editor.tables.PlainTableFixture
import org.json.JSONObject
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNotSame
import org.junit.Assert.assertNotEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertSame
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.RuntimeEnvironment
import org.robolectric.Robolectric
import org.robolectric.annotation.Config
import org.robolectric.shadows.ShadowLooper
import org.robolectric.annotation.GraphicsMode

internal fun replaceTableDocumentExternallyForTest(adapter: EditorV2Adapter, document: String): String {
    val request = JSONObject().put("version", 1).put("requestId", "1")
        .put("history", "resetAndClear").put("setJson", JSONObject(document))
    val replaced = UniffiEditorV2Backend.replaceDocument(adapter.editorId, request.toString())
    assertTrue("external replacement=$replaced", replaced is EditorV2CallResult.Ok)
    return requireNotNull(adapter.refreshFromRustState(null))
}

@RunWith(RobolectricTestRunner::class)
@Config(sdk = [34])
internal class EditorTableSurfaceMountTest {
    private val config = """{"schema":{"nodes":[{"name":"doc","content":"block+","role":"doc"},{"name":"paragraph","content":"inline*","group":"block","role":"textBlock"},{"name":"text","content":"","group":"inline","role":"text"},{"name":"table","content":"table_row+","group":"block","role":"block","tableRole":"table"},{"name":"table_row","content":"(table_cell | table_header)*","role":"block","tableRole":"row"},{"name":"table_cell","content":"block+","role":"block","tableRole":"cell","attrs":{"colspan":{"type":"number","default":1,"min":1},"rowspan":{"type":"number","default":1,"min":1},"colwidth":{"default":null}}},{"name":"table_header","content":"block+","role":"block","tableRole":"header_cell","attrs":{"colspan":{"type":"number","default":1,"min":1},"rowspan":{"type":"number","default":1,"min":1},"colwidth":{"default":null}}}],"marks":[]},"initialization":{"type":"localEmpty"}}"""
    private val gridDocument = PlainTableFixture.document(GRID_ROWS, GRID_COLUMNS, GRID_TEXT)
    private val tableDocument = """{"type":"doc","content":[{"type":"table","content":[{"type":"table_row","content":[{"type":"table_cell","content":[{"type":"paragraph","content":[{"type":"text","text":"Cell text"}]}]}]}]},{"type":"paragraph","content":[{"type":"text","text":"after"}]}]}"""
    internal companion object {
        private const val GRID_ROWS = 10
        private const val GRID_COLUMNS = 4
        private const val GRID_CELLS = GRID_ROWS * GRID_COLUMNS
        private const val GRID_TEXT = "Cell text"
        private const val TYPED = "X"
        private const val UNIQUE_GRID_SHAPES = 1
        const val TABLE_HOST_WIDTH = 600
        const val REFLOW_WIDTH = 400
        const val SWIPE_STEPS = 4
        const val SWIPE_STEP_MS = 16L
        const val DRAG_STEP_SLOP_FACTOR = 2
        const val OFFSET_ROUNDING_TOLERANCE_PX = 1f
        const val FLING_FRAMES = 10
        const val LONG_PRESS_UP_MS = 1_000L
        const val OUTSIDE_TAP_JITTER_FRACTION = 0.85f
        const val SUB_PIXEL_JITTER = 0.9f
        const val LONG_PRESS_HOLD_FACTOR = 2L
        val nestedTableDocument = """{"type":"doc","content":[{"type":"paragraph","content":[{"type":"text","text":"before"}]},{"type":"table","content":[{"type":"table_row","content":[{"type":"table_cell","content":[{"type":"paragraph","content":[{"type":"text","text":"Alpha"}]}]},{"type":"table_cell","content":[{"type":"table","content":[{"type":"table_row","content":[{"type":"table_cell","content":[{"type":"paragraph","content":[{"type":"text","text":"Nested"}]}]}]}]}]},{"type":"table_cell","content":[{"type":"paragraph","content":[{"type":"text","text":"Owner"}]}]}]}]},{"type":"paragraph","content":[{"type":"text","text":"after"}]}]}"""
    }
    private val wideTableDocument = """{"type":"doc","content":[{"type":"table","content":[{"type":"table_row","content":[{"type":"table_cell","attrs":{"colwidth":[600]},"content":[{"type":"paragraph","content":[{"type":"text","text":"Left"}]}]},{"type":"table_cell","attrs":{"colwidth":[600]},"content":[{"type":"paragraph","content":[{"type":"text","text":"Right"}]}]}]}]},{"type":"paragraph","content":[{"type":"text","text":"after"}]}]}"""
    private val wideTableWithBefore = wideTableDocument.replace("[{\"type\":\"table\"",
        "[{\"type\":\"paragraph\",\"content\":[{\"type\":\"text\",\"text\":\"before\"}]},{\"type\":\"table\"")

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
        configJSON: String = config,
        block: (RichTextEditorView, EditorV2Adapter, String) -> Unit
    ) {
        val created = UniffiEditorV2Backend.create(configJSON, null) as EditorV2CallResult.Ok
        val adapter = requireNotNull(EditorV2Adapter.attach(
            UniffiEditorV2Backend, JSONObject(created.value).getString("editorId"), false
        ))
        val token = EditorV2Registry.register(adapter)
        try {
            val update = requireNotNull(adapter.setContentJson(document))
            val view = RichTextEditorView(RuntimeEnvironment.getApplication())
            view.editorId = token
            val applied = view.editorEditText.applyUpdateJSON(requireNotNull(adapter.cachedViewUpdateJson))
            assertTrue("admission=$applied trace=${view.editorEditText.imeTraceSnapshotForTesting()} extents=${adapter.tableIndex.rootExtents} scalar=${adapter.cachedScalarLength} blocks=${adapter.cachedSemanticRenderBlocks}", applied)
            measure(view, 600)
            block(view, adapter, update)
        } finally {
            EditorV2Registry.remove(adapter.editorId)
            adapter.destroy()
        }
    }

    private fun withAttachedMountedView(
        document: String,
        block: (RichTextEditorView, EditorV2Adapter) -> Unit
    ) = withMountedView(document) { view, adapter, _ ->
        val activityController = Robolectric.buildActivity(Activity::class.java).setup()
        try {
            activityController.get().setContentView(view)
            measure(view, 600)
            block(view, adapter)
        } finally {
            activityController.pause().stop().destroy()
        }
    }

    @Test
    fun `nested only outer cell has a representable selection endpoint`() =
        withMountedView(nestedTableDocument) { _, adapter, _ ->
            val outer = adapter.tableRecordsForTesting.values.minBy { it.getInt("tablePos") }
            val cells = outer.getJSONArray("cells")
            val first = cells.getJSONObject(0).getInt("sourcePos")
            val nestedOnly = cells.getJSONObject(1).getInt("sourcePos")
            val sibling = cells.getJSONObject(2).getInt("sourcePos")
            val anchor = requireNotNull(adapter.scalarPositionForDoc(first + 2))
            val matches = mutableListOf<Pair<Int, String>>()
            val accepted = mutableListOf<String>()
            val rejected = mutableMapOf<String, Int>()
            val extent = requireNotNull(adapter.cachedScalarLength)
            for (scalar in 0..extent) for (affinity in listOf("before", "after")) {
                fun point(offset: Int) = JSONObject().put("kind", "scalar")
                    .put("offset", offset).put("affinity", affinity)
                val selection = JSONObject().put("type", "cell")
                    .put("anchorCell", point(anchor)).put("headCell", point(scalar))
                val result = adapter.callWithEnvelope(JSONObject().put("selection", selection)) {
                    UniffiEditorV2Backend.setSelection(adapter.editorId, it)
                }
                if (result is EditorV2CallResult.Ok) {
                    val rendered = UniffiEditorV2Backend.renderUpdate(adapter.editorId, null, null)
                    assertTrue("render after successful admission=$rendered", rendered is EditorV2CallResult.Ok)
                    val canonical = JSONObject((rendered as EditorV2CallResult.Ok).value)
                        .getJSONObject("selection")
                    accepted += "$scalar/$affinity:${canonical.optString("type")}/${canonical.optInt("headCell", -1)}"
                    if (canonical.optString("type") == "cell" &&
                        canonical.optInt("headCell", -1) == nestedOnly) matches += scalar to affinity
                } else if (result is EditorV2CallResult.Err) {
                    rejected[result.error.code] = (rejected[result.error.code] ?: 0) + 1
                }
            }
            val siblingScalar = requireNotNull(adapter.scalarPositionForDoc(sibling + 2))
            fun siblingPoint(offset: Int) = JSONObject().put("kind", "scalar").put("offset", offset)
            val siblingSelection = JSONObject().put("type", "cell")
                .put("anchorCell", siblingPoint(anchor)).put("headCell", siblingPoint(siblingScalar))
            val siblingResult = adapter.callWithEnvelope(JSONObject().put("selection", siblingSelection)) {
                UniffiEditorV2Backend.setSelection(adapter.editorId, it)
            }
            assertTrue("known sibling selection=$siblingResult", siblingResult is EditorV2CallResult.Ok)
            val siblingRender = UniffiEditorV2Backend.renderUpdate(adapter.editorId, null, null)
            assertTrue(siblingRender is EditorV2CallResult.Ok)
            assertEquals(sibling, JSONObject((siblingRender as EditorV2CallResult.Ok).value)
                .getJSONObject("selection").getInt("headCell"))
            assertTrue("legacy scalar probe unexpectedly reached outer opening=$nestedOnly matches=$matches accepted=$accepted rejected=$rejected",
                matches.isEmpty())
            fun documentPoint(opening: Int) = JSONObject().put("kind", "document").put("offset", opening)
            val exactSelection = JSONObject().put("type", "cell")
                .put("anchorCell", documentPoint(first)).put("headCell", documentPoint(nestedOnly))
            val beforeDocument = requireNotNull(adapter.documentJson())
            val beforeRevision = adapter.baseDocumentRevision
            val exactResult = adapter.callWithEnvelope(JSONObject().put("selection", exactSelection)) {
                UniffiEditorV2Backend.setSelection(adapter.editorId, it)
            }
            assertTrue("exact nested-only selection=$exactResult", exactResult is EditorV2CallResult.Ok)
            val exactRender = UniffiEditorV2Backend.renderUpdate(adapter.editorId, null, null)
            assertTrue("render after exact selection=$exactRender", exactRender is EditorV2CallResult.Ok)
            val exactCanonical = JSONObject((exactRender as EditorV2CallResult.Ok).value)
                .getJSONObject("selection")
            assertEquals("cell", exactCanonical.getString("type"))
            assertEquals(first, exactCanonical.getInt("anchorCell"))
            assertEquals(nestedOnly, exactCanonical.getInt("headCell"))
            assertEquals(beforeDocument, adapter.documentJson())
            assertEquals(beforeRevision, adapter.baseDocumentRevision)
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

    private fun tapFirstCell(view: RichTextEditorView, cellIndex: Int = 0) {
        val canvas = requireNotNull(drawing(view))
        canvas.measure(View.MeasureSpec.makeMeasureSpec(view.editorEditText.width, View.MeasureSpec.EXACTLY),
            View.MeasureSpec.makeMeasureSpec(view.editorEditText.height, View.MeasureSpec.EXACTLY))
        canvas.layout(0, 0, canvas.measuredWidth, canvas.measuredHeight)
        val block = requireNotNull(canvas.preparedLayout?.blocks?.singleOrNull())
        val cell = requireNotNull(block.tableSurface?.cells?.getOrNull(cellIndex))
        val bounds = requireNotNull(block.tableBounds)
        val x = bounds.left + block.tableSurface!!.frameOfCell(cell).left + cell.contentOrigin.first + 8f
        val y = bounds.top + block.tableSurface!!.frameOfCell(cell).top + cell.contentOrigin.second + 8f
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
    fun `editor host routes horizontal drag over active cell input without changing document`() =
        withAttachedMountedView(wideTableDocument) { view, adapter ->
            tapFirstCell(view)
            measure(view, 600)
            val input = view.activeTextInput
            assertTrue(input !== view.editorEditText)
            input.setSelection(input.text.length)
            val composition = requireNotNull(input.onCreateInputConnection(EditorInfo()))
            assertTrue(composition.setComposingText("pending", 1))
            val canvas = requireNotNull(drawing(view))
            val surface = requireNotNull(canvas.preparedLayout?.blocks?.single()?.tableSurface)
            val before = adapter.documentJson()
            val beforeRevision = adapter.baseDocumentRevision
            val beforeHistory = adapter.historyCanUndo() to adapter.historyCanRedo()
            val initialLeft = (input.layoutParams as FrameLayout.LayoutParams).leftMargin
            val y = input.top + input.height / 2f
            val downX = input.left + minOf(input.width - 20f, 300f)
            assertTrue("input=${input.left},${input.top} ${input.width}x${input.height} canvas=${canvas.width}x${canvas.height} down=$downX,$y",
                canvas.hasTableAt(downX, y))
            assertTrue(canvas.canConsumeTableDragAt(downX, y, -120f))
            val events = listOf(
                MotionEvent.obtain(0, 0, MotionEvent.ACTION_DOWN, downX, y, 0),
                MotionEvent.obtain(0, 16, MotionEvent.ACTION_MOVE, downX - 120f, y, 0),
                MotionEvent.obtain(0, 32, MotionEvent.ACTION_UP, downX - 120f, y, 0)
            )
            try {
                events.forEach(view::dispatchTouchEvent)
            } finally {
                events.forEach(MotionEvent::recycle)
            }
            assertTrue("table should scroll through active input", canvas.tablePhysicalOffsetForTesting(surface.identity) > 0f)
            val shiftedLeft = (input.layoutParams as FrameLayout.LayoutParams).leftMargin
            assertTrue("active input should follow presented cell", shiftedLeft < initialLeft)
            measure(view, 600)
            assertEquals(shiftedLeft, input.left)
            assertTrue(input === view.activeTextInput)
            assertEquals(before, adapter.documentJson())
            assertEquals(beforeRevision, adapter.baseDocumentRevision)
            assertEquals(beforeHistory, adapter.historyCanUndo() to adapter.historyCanRedo())
            assertEquals("Leftpending", input.text.toString())
            assertTrue(composition.finishComposingText())
            assertEquals("Leftpending", cellText(adapter, 0))
            input.setSelection(input.text.length)
            val connection = requireNotNull(input.onCreateInputConnection(EditorInfo()))
            assertTrue(connection.commitText("X", 1))
            assertEquals("LeftpendingX", cellText(adapter, 0))
            assertTrue(input === view.activeTextInput)
        }

    @Test
    fun `a horizontal drag on the table body keeps scrolling the table with the finger and flings on release`() =
        withAttachedMountedView(wideTableDocument) { view, adapter ->
            ShadowLooper.idleMainLooper()
            val canvas = requireNotNull(drawing(view))
            val block = requireNotNull(canvas.preparedLayout?.blocks?.single())
            val surface = requireNotNull(block.tableSurface)
            val table = requireNotNull(block.tableBounds)
            val x = canvas.left + canvas.width / 2f
            val y = canvas.top + table.exactCenterY()
            assertNull("the drag must start away from every column resize edge",
                canvas.hitResizeEdge(x - canvas.left, y - canvas.top))
            val step = ViewConfiguration.get(view.context).scaledTouchSlop * DRAG_STEP_SLOP_FACTOR
            assertTrue(canvas.canConsumeTableDragAt(x - canvas.left, y - canvas.top, -step * SWIPE_STEPS.toFloat()))
            val before = adapter.documentJson()
            val down = MotionEvent.obtain(0, 0, MotionEvent.ACTION_DOWN, x, y, 0)
            try { assertTrue(view.dispatchTouchEvent(down)) } finally { down.recycle() }
            for (index in 1..SWIPE_STEPS) {
                val move = MotionEvent.obtain(0, index * SWIPE_STEP_MS, MotionEvent.ACTION_MOVE,
                    x - index * step, y, 0)
                try { view.dispatchTouchEvent(move) } finally { move.recycle() }
                assertEquals("table offset after move $index of a ${step}px-per-move drag",
                    (index * step).toFloat(), canvas.tablePhysicalOffsetForTesting(surface.identity),
                    OFFSET_ROUNDING_TOLERANCE_PX)
            }
            val released = SWIPE_STEPS * step.toFloat()
            val up = MotionEvent.obtain(0, (SWIPE_STEPS + 1) * SWIPE_STEP_MS, MotionEvent.ACTION_UP,
                x - (SWIPE_STEPS + 1) * step, y, 0)
            try { view.dispatchTouchEvent(up) } finally { up.recycle() }
            repeat(FLING_FRAMES) {
                ShadowLooper.idleMainLooper(SWIPE_STEP_MS, TimeUnit.MILLISECONDS)
                canvas.computeScroll()
            }
            assertTrue("a release at drag speed must fling the table past the finger's ${released}px, " +
                "offset=${canvas.tablePhysicalOffsetForTesting(surface.identity)}",
                canvas.tablePhysicalOffsetForTesting(surface.identity) > released + OFFSET_ROUNDING_TOLERANCE_PX)
            assertEquals(before, adapter.documentJson())
        }

    @Test
    fun `editor host routes active cell drag with a nonzero touch pointer id`() =
        withAttachedMountedView(wideTableDocument) { view, adapter ->
            tapFirstCell(view)
            measure(view, 600)
            val input = view.activeTextInput
            assertTrue(input !== view.editorEditText)
            val canvas = requireNotNull(drawing(view))
            val surface = requireNotNull(canvas.preparedLayout?.blocks?.single()?.tableSurface)
            val before = adapter.documentJson()
            val pointerId = 7
            val touchDeviceId = 31
            val y = input.top + input.height / 2f
            val downX = input.left + minOf(input.width - 20f, 300f)
            assertTrue(canvas.hasTableAt(downX, y))
            assertTrue(canvas.canConsumeTableDragAt(downX, y, -120f))

            fun touch(action: Int, eventTime: Long, x: Float): MotionEvent {
                val properties = arrayOf(MotionEvent.PointerProperties().apply {
                    id = pointerId
                    toolType = MotionEvent.TOOL_TYPE_FINGER
                })
                val coordinates = arrayOf(MotionEvent.PointerCoords().apply {
                    this.x = x
                    this.y = y
                    pressure = 1f
                    size = 1f
                })
                return MotionEvent.obtain(0, eventTime, action, 1, properties, coordinates,
                    0, 0, 1f, 1f, touchDeviceId, 0, InputDevice.SOURCE_TOUCHSCREEN, 0)
            }

            val events = listOf(
                touch(MotionEvent.ACTION_DOWN, 0, downX),
                touch(MotionEvent.ACTION_MOVE, 16, downX - 120f),
                touch(MotionEvent.ACTION_UP, 32, downX - 120f)
            )
            try {
                events.forEach { event ->
                    assertEquals(pointerId, event.getPointerId(0))
                    assertEquals(touchDeviceId, event.deviceId)
                    view.dispatchTouchEvent(event)
                }
            } finally {
                events.forEach(MotionEvent::recycle)
            }
            assertTrue("nonzero pointer drag should scroll active table",
                canvas.tablePhysicalOffsetForTesting(surface.identity) > 0f)
            assertEquals(before, adapter.documentJson())
            assertTrue(input === view.activeTextInput)
        }

    @Test
    fun `native text selection retains gesture ownership after its range collapses`() =
        withAttachedMountedView(wideTableDocument) { view, adapter ->
            tapFirstCell(view)
            measure(view, 600)
            val input = view.activeTextInput
            val canvas = requireNotNull(drawing(view))
            val surface = requireNotNull(canvas.preparedLayout?.blocks?.single()?.tableSurface)
            val before = adapter.documentJson()
            input.setSelection(0, input.text.length)
            val x = input.left + minOf(input.width - 20f, 300f)
            val y = input.top + input.height / 2f
            val down = MotionEvent.obtain(0, 0, MotionEvent.ACTION_DOWN, x, y, 0)
            val selectedMove = MotionEvent.obtain(0, 16, MotionEvent.ACTION_MOVE, x - 40f, y, 0)
            val collapsedMove = MotionEvent.obtain(0, 32, MotionEvent.ACTION_MOVE, x - 140f, y, 0)
            val up = MotionEvent.obtain(0, 48, MotionEvent.ACTION_UP, x - 140f, y, 0)
            try {
                assertTrue(view.dispatchTouchEvent(down))
                assertTrue(view.dispatchTouchEvent(selectedMove))
                input.setSelection(input.text.length)
                assertTrue(view.dispatchTouchEvent(collapsedMove))
                view.dispatchTouchEvent(up)
            } finally {
                listOf(down, selectedMove, collapsedMove, up).forEach(MotionEvent::recycle)
            }
            assertEquals(0f, canvas.tablePhysicalOffsetForTesting(surface.identity), 0.01f)
            assertEquals(before, adapter.documentJson())
        }

    @Test
    fun `typing before table preserves mounted offset by source identity after positional shift`() =
        withMountedView(wideTableWithBefore) { view, adapter, _ ->
            val canvas = requireNotNull(drawing(view))
            val beforeSurface = requireNotNull(canvas.preparedLayout?.blocks?.single()?.tableSurface)
            val beforeId = requireNotNull(adapter.tableRecordsForTesting.values.single().optString("sourceId"))
            val beforeEpoch = adapter.positionEpoch
            canvas.setTableLogicalOffset(beforeSurface.identity, 180f)
            val oldPosition = requireNotNull(adapter.tableRecordsForTesting.keys.singleOrNull())

            val root = view.editorEditText
            root.setSelection(root.text.toString().indexOf("before") + "before".length)
            val connection = requireNotNull(root.onCreateInputConnection(EditorInfo()))
            assertTrue(connection.commitText(" extended", 1))
            measure(view, 600)

            val nextSurface = requireNotNull(drawing(view)?.preparedLayout?.blocks?.single()?.tableSurface)
            assertEquals(oldPosition, adapter.tableRecordsForTesting.keys.single())
            assertEquals(beforeId, adapter.tableRecordsForTesting.values.single().getString("sourceId"))
            assertEquals("source=$beforeId epochs=$beforeEpoch/${adapter.positionEpoch}",
                beforeSurface.identity, nextSurface.identity)
            assertEquals(180f, canvas.tablePhysicalOffsetForTesting(nextSurface.identity), 0.01f)
        }

    @Test
    fun `resetting document with a new table at the same position clears mounted offset`() =
        withMountedView(wideTableDocument) { view, adapter, _ ->
            val canvas = requireNotNull(drawing(view))
            val beforeSurface = requireNotNull(canvas.preparedLayout?.blocks?.single()?.tableSurface)
            canvas.setTableLogicalOffset(beforeSurface.identity, 180f)
            val beforeSourceId = adapter.tableRecordsForTesting.values.single().getString("sourceId")
            val beforeEpoch = adapter.positionEpoch
            val replacement = wideTableDocument.replace("Left", "Replacement")
            assertTrue(view.editorEditText.applyUpdateJSON(replaceTableDocumentExternallyForTest(adapter, replacement)))
            measure(view, 600)

            val nextSurface = requireNotNull(drawing(view)?.preparedLayout?.blocks?.single()?.tableSurface)
            assertEquals("old=$beforeSourceId/$beforeEpoch next=${adapter.tableRecordsForTesting.values.single().getString("sourceId")}/${adapter.positionEpoch}",
                0f, canvas.tablePhysicalOffsetForTesting(nextSurface.identity), 0.01f)
        }

    @Test
    fun `typing in one cell prepares only that cell`() = withMountedView(gridDocument) { view, adapter, _ ->
        measure(view, 600)
        val canvas = requireNotNull(drawing(view))
        canvas.measure(View.MeasureSpec.makeMeasureSpec(view.editorEditText.width, View.MeasureSpec.EXACTLY),
            View.MeasureSpec.makeMeasureSpec(view.editorEditText.height, View.MeasureSpec.EXACTLY))
        canvas.layout(0, 0, canvas.measuredWidth, canvas.measuredHeight)
        val block = requireNotNull(canvas.preparedLayout?.blocks?.singleOrNull())
        val cell = requireNotNull(block.tableSurface?.cells?.first())
        val frame = requireNotNull(block.tableBounds)
        val x = frame.left + block.tableSurface!!.frameOfCell(cell).left + cell.contentOrigin.first + 8f
        val y = frame.top + block.tableSurface!!.frameOfCell(cell).top + cell.contentOrigin.second + 8f
        val down = MotionEvent.obtain(0, 0, MotionEvent.ACTION_DOWN, x, y, 0)
        val up = MotionEvent.obtain(0, 10, MotionEvent.ACTION_UP, x, y, 0)
        try {
            assertTrue(view.dispatchTouchEvent(down))
            assertTrue(view.dispatchTouchEvent(up))
        } finally {
            down.recycle()
            up.recycle()
        }
        val cellInput = requireNotNull((0 until view.editorContentFrame.childCount)
            .map { view.editorContentFrame.getChildAt(it) }
            .filterIsInstance<EditorEditText>()
            .singleOrNull { it !== view.editorEditText }) { "cell input after host tap" }
        cellInput.setSelection(cellInput.text.length)
        val prepared = mutableListOf<Int>()
        view.editorTableSurface.onTableCellPreparedForTesting = { prepared += it }
        val relayouts = view.editorTableSurface.incrementalRelayoutsForTesting

        assertTrue(requireNotNull(cellInput.onCreateInputConnection(EditorInfo())).commitText(TYPED, 1))
        measure(view, 600)

        println("typing into cell ${cell.sourceIndex} prepared cells $prepared of $GRID_CELLS")
        assertEquals("the keystroke lands in the tapped cell", GRID_TEXT + TYPED, firstCellText(adapter))
        assertEquals("only the edited cell is measured again: $prepared", 1, prepared.size)
        assertEquals(relayouts + 1, view.editorTableSurface.incrementalRelayoutsForTesting)
    }

    @Test
    @GraphicsMode(GraphicsMode.Mode.NATIVE)
    fun `wrapping delta relayouts cached cells without preparing them`() = withMountedView(gridDocument) { view, _, _ ->
        tapFirstCell(view)
        val input = view.activeTextInput
        input.setSelection(input.text.length)
        val before = requireNotNull(drawing(view)?.preparedLayout?.blocks?.single()?.tableSurface)
        val prepared = mutableListOf<Int>()
        view.editorTableSurface.onTableCellPreparedForTesting = { prepared += it }
        val relayouts = view.editorTableSurface.incrementalRelayoutsForTesting
        assertTrue(requireNotNull(input.onCreateInputConnection(EditorInfo())).commitText(" wrapping text".repeat(GRID_ROWS), 1))
        measure(view, TABLE_HOST_WIDTH)
        val after = requireNotNull(drawing(view)?.preparedLayout?.blocks?.single()?.tableSurface)
        assertEquals(listOf(0), prepared)
        assertEquals(relayouts + 1, view.editorTableSurface.incrementalRelayoutsForTesting)
        assertTrue("the next row moves after wrapping", after.layout.rowOffsets[1] > before.layout.rowOffsets[1])
        for (index in 1 until GRID_CELLS) assertSame("unchanged cell $index", before.cells[index].content, after.cells[index].content)
    }

    @Test
    fun `matching authorized cell input skips rendering but appearance changes render`() = withMountedView(gridDocument) { view, adapter, _ ->
        tapFirstCell(view)
        val input = view.activeTextInput
        val before = input.inputRerendersForTesting
        assertTrue(input.applyUpdateJSON(requireNotNull(adapter.refreshFromRustState(null))))
        assertEquals(before, input.inputRerendersForTesting)
        view.editorEditText.setBaseStyle(view.editorEditText.baseFontSize, Color.RED, Color.TRANSPARENT)
        assertTrue(input.applyUpdateJSON(requireNotNull(adapter.refreshFromRustState(null))))
        assertTrue("appearance must still reach the input", input.inputRerendersForTesting > before)
    }

    @Test
    fun `same text with changed marks refreshes the cell input`() = withMountedView(
        gridDocument, config.replace("\"marks\":[]", "\"marks\":[{\"name\":\"bold\"}]")
    ) { view, adapter, _ ->
        tapFirstCell(view)
        val input = view.activeTextInput
        val before = input.inputRerendersForTesting
        val key = adapter.tableIndex.rootExtents.keys.single()
        val scalar = requireNotNull(adapter.tableIndex.scalarStart(key, 0)).toInt()
        val update = requireNotNull(adapter.toggleMark("bold", scalar, scalar + GRID_TEXT.length))
        assertTrue(input.applyUpdateJSON(update))
        assertEquals(GRID_TEXT, input.text.toString())
        assertTrue("changed marks render despite identical text", input.inputRerendersForTesting > before)
        assertTrue(input.text.getSpans(0, input.text.length, android.text.style.StyleSpan::class.java)
            .any { it.style == android.graphics.Typeface.BOLD })
    }

    @Test
    fun `structural row insertion retains original prepared content`() = withMountedView(gridDocument) { view, adapter, _ ->
        tapFirstCell(view)
        val before = requireNotNull(drawing(view)?.preparedLayout?.blocks?.single()?.tableSurface)
        val key = adapter.tableIndex.rootExtents.keys.single()
        val command = com.apollohg.editor.tables.TableAccessibilityAction.ALL.first {
            it.id == R.id.table_accessibility_add_row_after
        }.commandJson()
        val update = requireNotNull(adapter.applyTableCommandAtSelection(command, adapter.tableMutationAdmission(key)))
        assertTrue(view.activeTextInput.applyUpdateJSON(update))
        measure(view, TABLE_HOST_WIDTH)
        val after = requireNotNull(drawing(view)?.preparedLayout?.blocks?.single()?.tableSurface)
        assertEquals(GRID_CELLS + GRID_COLUMNS, after.cells.size)
        assertEquals(GRID_CELLS, after.cells.count { next -> before.cells.any { it.content === next.content } })
    }

    @Test
    fun `identical cells shape once when the table reflows`() = withMountedView(gridDocument) { view, _, _ ->
        measure(view, TABLE_HOST_WIDTH)
        val prepared = mutableListOf<Int>()
        view.editorTableSurface.onTableCellPreparedForTesting = { prepared += it }

        measure(view, REFLOW_WIDTH)

        println("reflowing $GRID_CELLS cells of identical content prepared $prepared")
        assertEquals("identical cells are shaped once: $prepared", UNIQUE_GRID_SHAPES, prepared.size)
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
        assertNotNull("source cell index", cell.sourceIndex)
        val x = frame.left + block.tableSurface!!.frameOfCell(cell).left + cell.contentOrigin.first + 8f
        val y = frame.top + block.tableSurface!!.frameOfCell(cell).top + cell.contentOrigin.second + 8f
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
        val x = bounds.left + block.tableSurface!!.frameOfCell(cell).left + cell.contentOrigin.first + 8f
        val y = bounds.top + block.tableSurface!!.frameOfCell(cell).top + cell.contentOrigin.second + 8f
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
            val originalTableId = requireNotNull(adapter.tableMappingsForTesting).tables.keys.single()
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
            assertEquals(originalTableId, requireNotNull(adapter.tableMappingsForTesting).tables.keys.single())
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
    fun `a bound cell input takes the root theme without the root insets or background`() =
        withMountedView { view, _, _ ->
            val themed = JSONObject().put("backgroundColor", "#FFFFFF")
                .put("contentInsets", JSONObject().put("top", 24).put("right", 16).put("bottom", 24).put("left", 16))
                .put("text", JSONObject().put("color", "#112233"))
            view.applyTheme(EditorTheme.fromJson(themed.toString()))
            tapFirstCell(view)
            val input = view.activeTextInput
            assertNotSame(view.editorEditText, input)
            fun assertCellChrome(step: String) {
                val background = (input.background as? ColorDrawable)?.color
                val state = "$step: padding=${input.paddingLeft},${input.paddingTop},${input.paddingRight}," +
                    "${input.paddingBottom} background=$background themed=${input.theme === view.editorEditText.theme}"
                assertTrue(state, input.theme === view.editorEditText.theme)
                assertEquals(state, listOf(0, 0, 0, 0),
                    listOf(input.paddingLeft, input.paddingTop, input.paddingRight, input.paddingBottom))
                assertEquals(state, Color.TRANSPARENT, background)
            }
            assertCellChrome("bind")
            view.applyTheme(EditorTheme.fromJson(themed.put("text", JSONObject().put("color", "#445566")).toString()))
            assertTrue(view.activeTextInput === input)
            assertCellChrome("appearance change")
        }

    @Test
    fun `a restored caret without a presented cell leaves the root input active`() =
        withMountedView { view, adapter, _ ->
            val cellStart = requireNotNull(adapter.tableMappingsForTesting).tables.values.single()
                .cells.first().blocks.first().scalarStart
            val caret = JSONObject().put("type", "text").put("anchorScalar", cellStart).put("headScalar", cellStart)
            view.editorTableSurface.clear()
            view.editorTableSurface.followRootSelectionIntoCell(caret)
            val input = view.activeTextInput
            assertTrue("bound ${input.width}x${input.height} without a presented cell",
                input === view.editorEditText)
            assertTrue("no cell input may stay mounted", (0 until view.editorContentFrame.childCount)
                .map { view.editorContentFrame.getChildAt(it) }
                .none { it is EditorEditText && it !== view.editorEditText })
        }

    @Test
    fun `leaving a composing cell commits its text before root focus`() = withMountedView { view, adapter, _ ->
        val (input, connection) = composeInFirstCell(view)
        assertEquals("Cell text", firstCellText(adapter))

        val root = view.editorEditText
        val (x, y) = proseTouchPoint(root)
        val down = MotionEvent.obtain(0, 0, MotionEvent.ACTION_DOWN, x, y, 0)
        val up = MotionEvent.obtain(0, 10, MotionEvent.ACTION_UP, x, y, 0)
        try {
            assertTrue(view.dispatchTouchEvent(down))
            assertTrue("a touch down alone must not release the cell", view.activeTextInput === input)
            assertEquals("after down trace=${input.imeTraceSnapshotForTesting()}",
                "Cell text", firstCellText(adapter))
            assertTrue(view.dispatchTouchEvent(up))
        } finally {
            down.recycle()
            up.recycle()
        }

        assertTrue(view.activeTextInput === root)
        assertEquals("Cell texttail", firstCellText(adapter))
        assertTrue(!connection.beginBatchEdit())
    }

    private fun proseTouchPoint(root: EditorEditText): Pair<Float, Float> {
        val offset = root.text.toString().indexOf("after") + 2
        val line = root.layout.getLineForOffset(offset)
        return root.left + root.totalPaddingLeft + root.layout.getPrimaryHorizontal(offset) to
            root.top + root.totalPaddingTop + (root.layout.getLineTop(line) + root.layout.getLineBottom(line)) / 2f
    }

    private fun composeInFirstCell(view: RichTextEditorView): Pair<EditorEditText, InputConnection> {
        tapFirstCell(view)
        val input = view.activeTextInput
        assertNotSame(view.editorEditText, input)
        input.setSelection(input.text.length)
        val connection = requireNotNull(input.onCreateInputConnection(EditorInfo()))
        assertTrue(connection.setComposingText("tail", 1))
        return input to connection
    }

    private fun dispatchPath(view: RichTextEditorView, points: List<Pair<Float, Float>>) {
        val events = points.mapIndexed { index, (x, y) ->
            val action = when (index) {
                0 -> MotionEvent.ACTION_DOWN
                points.lastIndex -> MotionEvent.ACTION_UP
                else -> MotionEvent.ACTION_MOVE
            }
            MotionEvent.obtain(0, index * SWIPE_STEP_MS, action, x, y, 0)
        }
        try {
            events.forEach { view.dispatchTouchEvent(it) }
        } finally {
            events.forEach(MotionEvent::recycle)
        }
    }

    private fun assertComposingCellKeptBoundAndFocused(
        view: RichTextEditorView,
        adapter: EditorV2Adapter,
        input: EditorEditText,
        connection: InputConnection,
        gesture: String
    ) {
        assertTrue("$gesture must keep the cell input: trace=${input.imeTraceSnapshotForTesting()}",
            view.activeTextInput === input)
        assertTrue("the bound cell keeps focus after $gesture", input.hasFocus())
        assertFalse("the prose must not take focus from $gesture", view.editorEditText.hasFocus())
        assertEquals("the composition stays pending", "Cell text", firstCellText(adapter))
        assertEquals("Cell texttail", input.text.toString())
        assertTrue("the cell connection stays live", connection.beginBatchEdit())
        connection.endBatchEdit()
        assertTrue(connection.finishComposingText())
        assertEquals("Cell texttail", firstCellText(adapter))
    }

    @Test
    fun `a horizontal swipe beyond touch slop on the prose keeps the composing cell bound and focused`() =
        withAttachedMountedView(tableDocument) { view, adapter ->
            val (input, connection) = composeInFirstCell(view)
            val (x, y) = proseTouchPoint(view.editorEditText)
            val slop = ViewConfiguration.get(view.context).scaledTouchSlop
            val points = (0..SWIPE_STEPS).map { step -> x + step * slop to y }
            assertTrue("the swipe stays inside the prose", points.last().first < view.editorEditText.width)
            dispatchPath(view, points)
            assertComposingCellKeptBoundAndFocused(view, adapter, input, connection, "a swipe")
        }

    @Test
    fun `a diagonal jitter outside the tap region keeps the composing cell bound and focused`() =
        withAttachedMountedView(tableDocument) { view, adapter ->
            val (input, connection) = composeInFirstCell(view)
            val (x, y) = proseTouchPoint(view.editorEditText)
            val jitter = ViewConfiguration.get(view.context).scaledTouchSlop * OUTSIDE_TAP_JITTER_FRACTION
            dispatchPath(view, listOf(x to y, x + jitter to y + jitter, x + jitter to y + jitter))
            assertComposingCellKeptBoundAndFocused(view, adapter, input, connection, "a diagonal jitter")
        }

    @Test
    fun `a diagonal jitter the surface still counts as a tap releases the composing cell before the prose focuses`() =
        withAttachedMountedView(tableDocument) { view, adapter ->
            val (input, _) = composeInFirstCell(view)
            val (x, y) = proseTouchPoint(view.editorEditText)
            val slop = ViewConfiguration.get(view.context).scaledTouchSlop
            val jitter = floor(slop / sqrt(2f)) + SUB_PIXEL_JITTER
            assertTrue("the jitter leaves a float slop circle of $slop", hypot(jitter, jitter) > slop)
            assertTrue("the jitter stays inside the integer tap region",
                2 * jitter.toInt() * jitter.toInt() <= slop * slop)
            dispatchPath(view, listOf(x to y, x + jitter to y + jitter, x + jitter to y + jitter))
            val root = view.editorEditText
            assertTrue("the prose took focus from the tap", root.hasFocus())
            assertTrue("a focused prose must not leave the cell bound", view.activeTextInput === root)
            assertFalse("the released cell input must not keep focus", input.hasFocus())
            assertEquals("the composition commits before the prose focuses", "Cell texttail", firstCellText(adapter))
        }

    @Test
    fun `a long press on the prose releases the composing cell before the root selects`() =
        withMountedView { view, adapter, _ ->
            val (input, _) = composeInFirstCell(view)
            val (x, y) = proseTouchPoint(view.editorEditText)
            val down = MotionEvent.obtain(0, 0, MotionEvent.ACTION_DOWN, x, y, 0)
            val up = MotionEvent.obtain(0, LONG_PRESS_UP_MS, MotionEvent.ACTION_UP, x, y, 0)
            try {
                view.dispatchTouchEvent(down)
                assertTrue(view.activeTextInput === input)
                ShadowLooper.idleMainLooper(ViewConfiguration.getLongPressTimeout().toLong() * LONG_PRESS_HOLD_FACTOR,
                    TimeUnit.MILLISECONDS)
                assertTrue("the long press must release the cell", view.activeTextInput === view.editorEditText)
                assertTrue("the prose takes focus for its long press", view.editorEditText.hasFocus())
                assertEquals("the composition commits before the root takes over", "Cell texttail",
                    firstCellText(adapter))
                view.dispatchTouchEvent(up)
            } finally {
                down.recycle()
                up.recycle()
            }
            assertTrue(view.activeTextInput === view.editorEditText)
        }

    @Test
    fun `an accessibility click on the prose reports a refusal and releases the cell once it can`() =
        assertRootAccessibilityActionWaitsForCellRelease(
            AccessibilityNodeInfo.ACTION_CLICK, AccessibilityEvent.TYPE_VIEW_CLICKED)

    @Test
    fun `an accessibility focus on the prose reports a refusal and releases the cell once it can`() =
        assertRootAccessibilityActionWaitsForCellRelease(
            AccessibilityNodeInfo.ACTION_FOCUS, AccessibilityEvent.TYPE_VIEW_FOCUSED)

    private fun assertRootAccessibilityActionWaitsForCellRelease(action: Int, forbiddenEvent: Int) =
        withAttachedMountedView(tableDocument) { view, adapter ->
            val (input) = composeInFirstCell(view)
            val root = view.editorEditText
            val sentEvents = mutableListOf<Int>()
            root.accessibilityDelegate = object : View.AccessibilityDelegate() {
                override fun sendAccessibilityEvent(host: View, eventType: Int) {
                    sentEvents += eventType
                    super.sendAccessibilityEvent(host, eventType)
                }
            }
            val name = AccessibilityNodeInfo.AccessibilityAction(action, null).toString()
            input.blockExternalEditorUpdatePreparationForTesting = true
            val refused = try {
                root.performAccessibilityAction(action, null)
            } finally {
                input.blockExternalEditorUpdatePreparationForTesting = false
            }
            assertFalse("a refused $name must not send ${AccessibilityEvent.eventTypeToString(forbiddenEvent)}: " +
                "events=${sentEvents.map(AccessibilityEvent::eventTypeToString)}", forbiddenEvent in sentEvents)
            assertFalse("a refused $name must not report success", refused)
            assertSame("a refused $name keeps the cell input", input, view.activeTextInput)
            assertTrue("a refused $name keeps the cell focused", input.hasFocus())
            assertFalse("a refused $name leaves the prose unfocused", root.hasFocus())
            assertEquals("a refused $name keeps the composition pending", "Cell text", firstCellText(adapter))
            assertTrue("an allowed $name that moves focus to the prose reports success",
                root.performAccessibilityAction(action, null))
            assertProseTookOverFromTheCell(view, adapter)
        }

    @Test
    fun `an allowed text drop on the prose commits and releases the composing cell before inserting`() =
        withAttachedMountedView(tableDocument) { view, adapter ->
            val (input) = composeInFirstCell(view)
            val root = view.editorEditText
            val clip = ClipData.newPlainText("external", "dropped ")
            val offset = root.text.toString().indexOf("after")
            assertTrue(sendTextDragEventForTest(root, DragEvent.ACTION_DRAG_STARTED, clip))
            assertTrue("an allowed drop succeeds", sendTextDragEventForTest(root, DragEvent.ACTION_DROP, clip, offset))
            assertNotSame("the drop releases the cell input", input, view.activeTextInput)
            assertProseTookOverFromTheCell(view, adapter)
            val paragraph = JSONObject(requireNotNull(adapter.documentJson())).getJSONArray("content")
                .getJSONObject(1).getJSONArray("content").getJSONObject(0).getString("text")
            assertEquals("the dropped text lands in the prose", "dropped after", paragraph)
        }

    private fun assertProseTookOverFromTheCell(view: RichTextEditorView, adapter: EditorV2Adapter) {
        assertSame("an allowed action releases the cell", view.editorEditText, view.activeTextInput)
        assertTrue("an allowed action focuses the prose", view.editorEditText.hasFocus())
        assertEquals("the composition commits before the prose focuses", "Cell texttail", firstCellText(adapter))
    }

    @Test
    fun `blocked composition preflight keeps the cell active on prose tap`() = withMountedView { view, adapter, _ ->
        val (input) = composeInFirstCell(view)
        val before = adapter.documentJson()
        input.blockExternalEditorUpdatePreparationForTesting = true

        val root = view.editorEditText
        val (x, y) = proseTouchPoint(root)
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
    fun `binding survives a keystroke before its cell`() = withMountedView(wideTableDocument.replace("600", "120")) { view, adapter, _ ->
        tapFirstCell(view, 1)
        val input = view.activeTextInput
        val binding = requireNotNull(input.tableCellPositionMap).binding
        val key = adapter.tableIndex.rootExtents.keys.single()
        val oldDoc = requireNotNull(adapter.tableIndex.docStart(key, 1))
        val firstScalar = requireNotNull(adapter.tableIndex.scalarStart(key, 0)).toInt()
        assertNotNull(adapter.insertText("X", firstScalar))
        val shifted = requireNotNull(adapter.tableIndex.scalarStart(key, 1)).toInt()
        assertNotNull(adapter.syncSelection(shifted, shifted))
        assertTrue(input.applyUpdateJSON(requireNotNull(adapter.refreshFromRustState(null))))
        val refreshed = requireNotNull(input.tableCellPositionMap).binding
        assertEquals(binding.tableKey, refreshed.tableKey)
        assertEquals(binding.cellIndex, refreshed.cellIndex)
        assertEquals(oldDoc + 1u, adapter.tableIndex.docStart(key, 1))
        assertSame(input, view.activeTextInput)
        assertTrue(requireNotNull(input.onCreateInputConnection(EditorInfo())).commitText("!", 1))
        assertEquals("XLeft", cellText(adapter, 0))
        assertEquals("!Right", cellText(adapter, 1))
    }

    @Test
    fun `structural replacement rebinds before input`() = withMountedView(wideTableDocument.replace("600", "120")) { view, adapter, _ ->
        tapFirstCell(view, 1)
        val input = view.activeTextInput
        val oldBinding = requireNotNull(input.tableCellPositionMap).binding
        val connection = requireNotNull(input.onCreateInputConnection(EditorInfo()))
        val replacement = wideTableDocument.replace("600", "120").replace("Left", "longer").replace("Right", "replacement")
        assertNotNull(adapter.setContentJson(replacement))
        val key = adapter.tableIndex.rootExtents.keys.single()
        val scalar = requireNotNull(adapter.tableIndex.scalarStart(key, 1)).toInt()
        assertNotNull(adapter.syncSelection(scalar, scalar))
        assertTrue(input.applyUpdateJSON(requireNotNull(adapter.refreshFromRustState(null))))
        assertSame(input, view.activeTextInput)
        val refreshed = requireNotNull(input.tableCellPositionMap).binding
        assertNotEquals(oldBinding.tableKey, refreshed.tableKey)
        assertEquals(key, refreshed.tableKey)
        assertEquals(1, refreshed.cellIndex)
        assertEquals("replacement", input.text.toString())
        assertFalse(connection.beginBatchEdit())
        assertTrue(requireNotNull(input.onCreateInputConnection(EditorInfo())).commitText("!", 1))
        assertEquals("longer", cellText(adapter, 0))
        assertEquals("!replacement", cellText(adapter, 1))
    }

    @Test
    fun `external same-position replacement retires mounted cell connection`() = withMountedView { view, adapter, _ ->
        tapFirstCell(view)
        val connection = requireNotNull(view.activeTextInput.onCreateInputConnection(EditorInfo()))
        val replacement = tableDocument.replace("Cell text", "Replacement")
        val update = replaceTableDocumentExternallyForTest(adapter, replacement)
        assertTrue(view.editorEditText.applyUpdateJSON(update))
        measure(view, 600)

        val rebound = view.activeTextInput
        assertTrue("the focused editor rebinds the cell holding the caret", rebound !== view.editorEditText)
        assertTrue(rebound.hasFocus())
        assertEquals("Replacement", rebound.text.toString())
        assertEquals("Replacement", firstCellText(adapter))
        assertTrue("the replaced cell's connection is retired", !connection.beginBatchEdit())
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
                val next = replaceTableDocumentExternallyForTest(adapter, replacement)
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
    fun `table drawing mounted inside the host layout pass covers the editor`() = withMountedView { view, _, _ ->
        val drawing = requireNotNull(drawing(view))
        val input = view.editorEditText
        val state = "drawing ${drawing.width}x${drawing.height} requested=${drawing.isLayoutRequested}, " +
            "editor ${input.measuredWidth}x${input.measuredHeight}"
        assertTrue(state, input.measuredHeight > heightSpan(view).heightPx)
        assertEquals(state, input.measuredWidth to input.measuredHeight, drawing.width to drawing.height)
    }

    private fun assertBoundInputFollowsItsCellThroughReflow(view: RichTextEditorView) {
        tapFirstCell(view)
        val input = view.activeTextInput
        assertNotSame(view.editorEditText, input)
        val before = input.width
        measure(view, REFLOW_WIDTH)
        val frame = view.editorContentFrame
        frame.forceLayout()
        frame.measure(View.MeasureSpec.makeMeasureSpec(frame.width, View.MeasureSpec.EXACTLY),
            View.MeasureSpec.makeMeasureSpec(frame.height, View.MeasureSpec.EXACTLY))
        frame.layout(frame.left, frame.top, frame.right, frame.bottom)
        val params = input.layoutParams as FrameLayout.LayoutParams
        val state = "direction=${view.editorContentFrame.layoutDirection} input ${input.left},${input.top} " +
            "${input.width}x${input.height} before=$before, " +
            "params ${params.leftMargin},${params.topMargin} ${params.width}x${params.height}"
        assertNotEquals(state, before, params.width)
        assertEquals(state, listOf(params.leftMargin, params.topMargin, params.width, params.height),
            listOf(input.left, input.top, input.width, input.height))
    }

    @Test
    fun `a bound cell input follows its cell through a host layout reflow`() = withMountedView { view, _, _ ->
        assertBoundInputFollowsItsCellThroughReflow(view)
    }

    @Test
    fun `a bound cell input follows its cell through a right to left host layout reflow`() {
        val applicationInfo = RuntimeEnvironment.getApplication().applicationInfo
        val originalFlags = applicationInfo.flags
        applicationInfo.flags = originalFlags or ApplicationInfo.FLAG_SUPPORTS_RTL
        try {
            withMountedView { view, _, _ ->
                view.layoutDirection = View.LAYOUT_DIRECTION_RTL
                measure(view, TABLE_HOST_WIDTH)
                assertEquals(View.LAYOUT_DIRECTION_RTL, view.editorContentFrame.layoutDirection)
                assertBoundInputFollowsItsCellThroughReflow(view)
            }
        } finally {
            applicationInfo.flags = originalFlags
        }
    }

    @Test
    fun `a view without table owner authority keeps drawing its table after the position epoch advances`() =
        withMountedView { view, adapter, _ ->
            val input = view.editorEditText
            val reserved = heightSpan(view).heightPx
            input.rootTableNativeOwnerAuthority = { false }
            val advancedEpoch = (requireNotNull(adapter.positionEpoch).toLong() + 1).toString()
            adapter.positionEpoch = advancedEpoch
            view.editorTableSurface.refresh()
            assertEquals("epoch map=${input.rootTableMapPositionEpoch} adapter=${adapter.positionEpoch}",
                advancedEpoch, input.rootTableMapPositionEpoch)
            assertNotNull("the table must stay drawn", drawing(view)?.preparedLayout?.blocks?.singleOrNull()?.tableSurface)
            assertEquals("the table keeps its reserved height", reserved, heightSpan(view).heightPx)
            assertTrue("presentation must not grant root table input", !input.isAuthorizedForRootTableInput())
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
            val extent = requireNotNull(adapter.tableMappingsForTesting?.tables?.values?.single()?.extent)
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
        val mappings = requireNotNull(adapter.tableMappingsForTesting).tables
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
    fun `right arrow skips nested-only outer cell`() =
        withMountedView(nestedTableDocument) { view, adapter, _ ->
            tapFirstCell(view)
            val input = view.activeTextInput
            input.setSelection(input.text.length)
            val revision = adapter.baseDocumentRevision
            assertTrue(input.dispatchKeyEvent(KeyEvent(213L, 213L,
                KeyEvent.ACTION_DOWN, KeyEvent.KEYCODE_DPAD_RIGHT, 0)))
            assertTrue(input === view.activeTextInput)
            assertEquals("Owner", input.text.toString())
            assertEquals(revision, adapter.baseDocumentRevision)
        }

    @Test
    fun `nested snapshot with mismatched root identity or extent clears surface`() =
        withMountedView(nestedTableDocument) { view, adapter, _ ->
            val input = view.editorEditText
            val rootIds = input.rootTableMapTableIds
            val rootExtents = input.rootTableMapExtents
            val originalIndex = adapter.tableIndex
            assertNotNull(drawing(view))

            adapter.tableIndex = com.apollohg.editor.tables.EditorTableIndex()
            view.requestLayout()
            measure(view, 600)
            assertNull(drawing(view))

            adapter.tableIndex = originalIndex
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
    fun `same revision root update preserves measured marker reservation`() = withMountedView { view, adapter, update ->
        val original = heightSpan(view)
        val originalHeight = original.heightPx
        val revision = adapter.baseDocumentRevision
        val document = adapter.documentJson()

        assertTrue(view.editorEditText.applyUpdateJSON(update))
        measure(view, 600)

        val restored = heightSpan(view)
        assertSame(original, restored)
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
    fun `released owner clears mounted surface while retaining frame index`() = withMountedView { view, adapter, _ ->
        val input = view.editorEditText
        val visibleText = input.text.toString()
        val revision = adapter.baseDocumentRevision
        assertNotNull(drawing(view))
        assertNotNull(adapter.tableMappingsForTesting)

        adapter.releaseNativeBindingOwner(input.nativeBindingToken)
        assertNotNull(adapter.tableMappingsForTesting)
        input.setSelection(visibleText.length)

        assertNull(drawing(view))
        assertFalse(input.isAuthorizedForRootTableInput())
        assertEquals(visibleText, input.text.toString())
        assertEquals(revision, adapter.baseDocumentRevision)
    }

    @Test
    fun `zero leaf table beside populated table leaves populated host visible`() {
        val document = """{"type":"doc","content":[{"type":"table","content":[{"type":"table_row","content":[]}]},{"type":"table","content":[{"type":"table_row","content":[{"type":"table_cell","content":[{"type":"paragraph","content":[{"type":"text","text":"visible cell"}]}]}]}]},{"type":"paragraph","content":[{"type":"text","text":"after"}]}]}"""
        withMountedView(document) { view, adapter, _ ->
            val mappings = requireNotNull(adapter.tableMappingsForTesting).tables
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
