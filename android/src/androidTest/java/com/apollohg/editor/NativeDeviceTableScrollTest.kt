package com.apollohg.editor

import android.content.Intent
import android.graphics.Rect
import android.os.SystemClock
import android.view.MotionEvent
import android.view.View
import android.view.ViewGroup
import android.view.ViewConfiguration
import android.view.inputmethod.EditorInfo
import android.view.inputmethod.InputConnection
import android.widget.FrameLayout
import android.widget.LinearLayout
import android.widget.ScrollView
import androidx.test.core.app.ActivityScenario
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.filters.LargeTest
import androidx.test.platform.app.InstrumentationRegistry
import com.apollohg.editor.viewer.PreparedProseDrawingView
import com.apollohg.editor.viewer.PreparedProseTheme
import com.apollohg.editor.viewer.ProseLayoutKey
import com.apollohg.editor.viewer.ProseViewerRequest
import com.apollohg.editor.viewer.StaticLayoutAndroidProseLayoutEngine
import com.apollohg.editor.viewer.compileWithRust
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertSame
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith
import kotlin.math.hypot

@RunWith(AndroidJUnit4::class)
@LargeTest
class NativeDeviceTableScrollTest {
    private val instrumentation = InstrumentationRegistry.getInstrumentation()
    private data class Point(val x: Float, val y: Float)

    @Test
    fun editorHorizontalDragMovesTableWithoutChangingDocumentOrVerticalScroll() =
        withEditor { scenario ->
            val start = editorTableBodyPoint(scenario)
            var identity = ""
            var initialVertical = 0
            scenario.onActivity { activity ->
                val host = editorTableHost(activity)
                val surface = host.preparedLayout!!.blocks.single { it.tableSurface != null }
                    .tableSurface!!
                identity = surface.identity
                assertTrue("fixture must overflow on this device",
                    surface.bounds.width() > surface.hostViewportWidth)
                initialVertical = activity.richTextView.editorScrollView.scrollY
                assertEquals(0f, host.tablePhysicalOffsetForTesting(identity), OFFSET_TOLERANCE_PX)
            }
            val distance = dragDistance(scenario)
            drag(start, -distance, 0f)
            scenario.onActivity { activity ->
                val host = editorTableHost(activity)
                assertEquals("the table must follow the whole ${distance}px drag", distance,
                    host.tablePhysicalOffsetForTesting(identity), DRAG_ROUNDING_TOLERANCE_PX)
                assertEquals(initialVertical, activity.richTextView.editorScrollView.scrollY)
                assertUnchanged(activity)
            }
            instrumentation.saveDeviceScreenshot("native-table-scroll-editor-light.png")
        }

    @Test
    fun editorDragStartingOnABodyRowColumnEdgeScrollsTheTable() = withEditor { scenario ->
        val start = editorBodyRowColumnEdgePoint(scenario)
        var identity = ""
        scenario.onActivity { activity ->
            val host = editorTableHost(activity)
            identity = host.preparedLayout!!.blocks.single { it.tableSurface != null }
                .tableSurface!!.identity
            assertEquals(0f, host.tablePhysicalOffsetForTesting(identity), OFFSET_TOLERANCE_PX)
        }
        val distance = dragDistance(scenario)
        drag(start, -distance, 0f)
        scenario.onActivity { activity ->
            val host = editorTableHost(activity)
            assertNull("a body-row edge drag must not start a resize", host.activeTableResizeEdge)
            assertEquals("the table must follow the whole ${distance}px drag from a body-row column edge",
                distance, host.tablePhysicalOffsetForTesting(identity), DRAG_ROUNDING_TOLERANCE_PX)
            assertUnchanged(activity)
        }
    }

    @Test
    fun diagonalVerticalDragScrollsEditorWhileTableOffsetStaysPut() = withEditor { scenario ->
        val start = editorTableBodyPoint(scenario)
        var identity = ""
        scenario.onActivity { activity ->
            val host = editorTableHost(activity)
            identity = host.preparedLayout!!.blocks.single { it.tableSurface != null }
                .tableSurface!!.identity
            assertTrue("fixture must permit vertical host movement",
                activity.richTextView.editorScrollView.canScrollVertically(1))
        }
        var distance = 0f
        scenario.onActivity { activity ->
            distance = verticalDragDistance(start, activity.richTextView.editorScrollView)
        }
        drag(start, -distance * VERTICAL_DRAG_X_FRACTION, -distance)
        scenario.onActivity { activity ->
            assertTrue("vertical drag did not scroll the editor host",
                activity.richTextView.editorScrollView.scrollY > 0)
            assertEquals(0f, editorTableHost(activity).tablePhysicalOffsetForTesting(identity),
                OFFSET_TOLERANCE_PX)
            assertUnchanged(activity)
        }
    }

    @Test
    fun dragOverActiveCellMovesInputAndTypingStillEditsThatCell() = withEditor { scenario ->
        val cellPoint = editorCellPoint(scenario, EDITABLE_ROW, ALPHA_COLUMN)
        tap(cellPoint)
        var input: EditorEditText? = null
        lateinit var compositionConnection: InputConnection
        var oldLeft = 0
        var identity = ""
        scenario.onActivity { activity ->
            input = activity.currentFocus as EditorEditText
            assertTrue(input !== activity.richTextView.editorEditText)
            assertEquals("Alpha", input!!.text.toString())
            oldLeft = input!!.left
            identity = editorTableHost(activity).preparedLayout!!.blocks
                .single { it.tableSurface != null }.tableSurface!!.identity
            input!!.setSelection(input!!.text.length)
            compositionConnection = requireNotNull(input!!.onCreateInputConnection(EditorInfo()))
            assertTrue(compositionConnection.setComposingText("pending", 1))
            assertUnchanged(activity)
        }
        drag(cellPoint, -activeCellDragDistance(scenario), 0f)
        instrumentation.waitForIdleSync()
        scenario.onActivity { activity ->
            val active = input!!
            assertSame(active, activity.currentFocus)
            assertTrue("active input did not follow its scrolled cell", active.left < oldLeft)
            assertTrue("active input lost its visible clip", requireNotNull(active.clipBounds).width() > 0)
            val host = editorTableHost(activity)
            assertTrue(host.tablePhysicalOffsetForTesting(identity) > 0f)
            val alpha = presentedCell(host, EDITABLE_ROW, ALPHA_COLUMN)
            assertTrue("active cell should be partially clipped after the drag",
                alpha.bounds.left < alpha.clip.left && alpha.bounds.right > alpha.clip.left)
            assertUnchanged(activity)
            assertEquals("Alphapending", active.text.toString())
            assertTrue(compositionConnection.finishComposingText())
            active.setSelection(active.text.length)
            assertTrue(requireNotNull(active.onCreateInputConnection(EditorInfo())).commitText("!", 1))
            val document = org.json.JSONObject(requireNotNull(activity.adapter.documentJson()))
            val cell = document.getJSONArray("content").getJSONObject(TABLE_BLOCK_INDEX)
                .getJSONArray("content").getJSONObject(EDITABLE_ROW)
                .getJSONArray("content").getJSONObject(0)
            assertEquals("Alphapending!", cell.getJSONArray("content").getJSONObject(0)
                .getJSONArray("content").getJSONObject(0).getString("text"))
        }
        instrumentation.saveDeviceScreenshot("native-table-scroll-active-input.png")
        tap(editorCellPoint(scenario, EDITABLE_ROW, OWNER_COLUMN))
        scenario.onActivity { activity ->
            assertSame("cell switch must reuse the native input", input, activity.currentFocus)
            assertEquals("Owner", input!!.text.toString())
            assertTrue("new cell binding must accept keyboard input",
                input!!.onCreateInputConnection(EditorInfo()) != null)
        }
    }

    @Test
    fun recognizedNativeTextSelectionKeepsPrecedenceOverHorizontalTableDrag() =
        withEditor { scenario ->
            tap(editorCellPoint(scenario, EDITABLE_ROW, ALPHA_COLUMN))
            val textPoint = activeInputTextPoint(scenario)
            var identity = ""
            val downTime = SystemClock.uptimeMillis()
            send(downTime, downTime, MotionEvent.ACTION_DOWN, textPoint)
            var released = false
            try {
                SystemClock.sleep(ViewConfiguration.getLongPressTimeout() + LONG_PRESS_SETTLE_MS)
                instrumentation.waitForIdleSync()
                scenario.onActivity { activity ->
                    val input = activity.currentFocus as EditorEditText
                    assertTrue(input !== activity.richTextView.editorEditText)
                    assertTrue("native long press did not select text",
                        input.selectionEnd > input.selectionStart)
                    identity = editorTableHost(activity).preparedLayout!!.blocks
                        .single { it.tableSurface != null }.tableSurface!!.identity
                }
                moveAndRelease(downTime, textPoint,
                    -SELECTION_DRAG_DP * instrumentation.targetContext.resources.displayMetrics.density,
                    0f)
                released = true
            } finally {
                if (!released) send(downTime, SystemClock.uptimeMillis(),
                    MotionEvent.ACTION_CANCEL, textPoint)
            }
            scenario.onActivity { activity ->
                assertEquals("selected text must own the drag", 0f,
                    editorTableHost(activity).tablePhysicalOffsetForTesting(identity),
                    OFFSET_TOLERANCE_PX)
                assertUnchanged(activity)
            }
            instrumentation.saveDeviceScreenshot("native-table-scroll-selection.png")
        }

    @Test
    fun rtlTableStartsAtInlineStartAndPhysicalRightDragMovesTowardLeft() =
        withEditor(rtl = true, dark = true) { scenario ->
            val start = editorTableBodyPoint(scenario)
            var identity = ""
            var initial = 0f
            scenario.onActivity { activity ->
                val host = editorTableHost(activity)
                val surface = host.preparedLayout!!.blocks.single { it.tableSurface != null }
                    .tableSurface!!
                assertTrue("fixture direction was lost", surface.isRightToLeft)
                identity = surface.identity
                initial = host.tablePhysicalOffsetForTesting(identity)
                assertTrue("RTL table must start at its physical right edge", initial > 0f)
            }
            val distance = dragDistance(scenario)
            drag(start, distance, 0f)
            scenario.onActivity { activity ->
                assertEquals("a ${distance}px right drag must move the RTL table as far toward its physical left",
                    initial - distance, editorTableHost(activity).tablePhysicalOffsetForTesting(identity),
                    DRAG_ROUNDING_TOLERANCE_PX)
                assertUnchanged(activity)
            }
            instrumentation.saveDeviceScreenshot("native-table-scroll-editor-rtl-dark.png")
        }

    @Test
    fun readOnlyViewerUsesTouchToScrollTableAndLetsVerticalHostScroll() =
        withViewer { scenario, viewer, scroll ->
            val start = viewerTablePoint(scenario, viewer)
            var identity = ""
            scenario.onActivity {
                val surface = viewer.preparedLayout!!.blocks.single { it.tableSurface != null }
                    .tableSurface!!
                identity = surface.identity
                assertTrue(surface.bounds.width() > surface.hostViewportWidth)
                assertTrue(scroll.canScrollVertically(1))
            }
            drag(start, -viewerDragDistance(viewer), 0f)
            scenario.onActivity {
                assertTrue("viewer table did not move", viewer.tablePhysicalOffsetForTesting(identity) > 0f)
                assertEquals(0, scroll.scrollY)
            }
            instrumentation.saveDeviceScreenshot("native-table-scroll-viewer-light.png")
            val verticalStart = viewerTablePoint(scenario, viewer)
            var distance = 0f
            scenario.onActivity { distance = verticalDragDistance(verticalStart, scroll) }
            drag(verticalStart, -distance * VERTICAL_DRAG_X_FRACTION, -distance)
            scenario.onActivity {
                assertTrue("viewer host did not accept vertical drag", scroll.scrollY > 0)
                assertTrue(viewer.tablePhysicalOffsetForTesting(identity) > 0f)
            }
        }

    @Test
    fun nestedReadOnlyTableConsumesHorizontalDragBeforeItsOuterTable() =
        withViewer(NativeTableHostActivity.nestedOverflowingDocument()) { scenario, viewer, _ ->
            var innerIdentity = ""
            var outerIdentity = ""
            var nestedPoint = Point(0f, 0f)
            var innerWidth = 0f
            scenario.onActivity {
                val surfaces = viewer.presentedTableCells().map { it.surface }.distinct()
                assertEquals("fixture must mount one nested and one outer table", 2, surfaces.size)
                val inner = surfaces.minBy { it.hostViewportWidth }
                val outer = surfaces.maxBy { it.hostViewportWidth }
                assertTrue(inner.hostViewportWidth < outer.hostViewportWidth)
                assertTrue(inner.bounds.width() > inner.hostViewportWidth)
                innerWidth = inner.hostViewportWidth
                innerIdentity = inner.identity
                outerIdentity = outer.identity
                val cell = viewer.presentedTableCells().first { presented ->
                    presented.surface === inner &&
                        presented.bounds.right > presented.clip.left &&
                        presented.bounds.left < presented.clip.right &&
                        presented.bounds.bottom > presented.clip.top &&
                        presented.bounds.top < presented.clip.bottom
                }
                val left = maxOf(cell.bounds.left, cell.clip.left)
                val right = minOf(cell.bounds.right, cell.clip.right)
                val top = maxOf(cell.bounds.top, cell.clip.top)
                val bottom = minOf(cell.bounds.bottom, cell.clip.bottom)
                val location = IntArray(2)
                viewer.getLocationOnScreen(location)
                nestedPoint = Point(location[0] + left +
                    (right - left) * NESTED_START_FRACTION,
                    location[1] + (top + bottom) / 2f)
            }
            drag(nestedPoint, -innerWidth * NESTED_DRAG_FRACTION, 0f)
            scenario.onActivity {
                assertTrue("nested table did not consume the drag",
                    viewer.tablePhysicalOffsetForTesting(innerIdentity) > 0f)
                assertEquals("outer table moved before nested table reached its edge", 0f,
                    viewer.tablePhysicalOffsetForTesting(outerIdentity), OFFSET_TOLERANCE_PX)
            }
            instrumentation.saveDeviceScreenshot("native-table-scroll-viewer-nested.png")
        }

    private fun withEditor(
        rtl: Boolean = false,
        dark: Boolean = false,
        test: (ActivityScenario<NativeTableHostActivity>) -> Unit
    ) {
        val intent = Intent(instrumentation.targetContext, NativeTableHostActivity::class.java)
            .putExtra(NativeTableHostActivity.EXTRA_OVERFLOW, true)
            .putExtra(NativeTableHostActivity.EXTRA_RTL, rtl)
            .putExtra(NativeTableHostActivity.EXTRA_DARK, dark)
        ActivityScenario.launch<NativeTableHostActivity>(intent).use { scenario ->
            awaitEditorTable(scenario)
            test(scenario)
        }
    }

    private fun withViewer(
        source: String = NativeTableHostActivity.overflowingDocument(),
        test: (ActivityScenario<NativeTableHostActivity>, PreparedProseDrawingView, ScrollView) -> Unit
    ) = withEditor { scenario ->
        lateinit var viewer: PreparedProseDrawingView
        lateinit var scroll: ScrollView
        scenario.onActivity { activity ->
            val document = compileWithRust(ProseViewerRequest(ProseViewerSource.Json(source),
                ProseViewerConfiguration(NativeTableHostActivity.CONFIG)))
            val width = activity.richTextView.width
            val density = activity.resources.displayMetrics.density
            val key = ProseLayoutKey(document.semanticKey, width, "device-table-scroll", 0, 0,
                density.toBits().toLong(), 0, "device-table-scroll")
            val layout = StaticLayoutAndroidProseLayoutEngine().prepare(document, key,
                PreparedProseTheme.resolve(null, density), width, density, false)
            viewer = PreparedProseDrawingView(activity).apply {
                minimumHeight = layout.heightPx
                install(layout)
            }
            scroll = ScrollView(activity).apply {
                addView(viewer, FrameLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT,
                    layout.heightPx))
            }
            val root = activity.richTextView.parent as LinearLayout
            val slot = root.indexOfChild(activity.richTextView)
            val params = activity.richTextView.layoutParams
            root.removeView(activity.richTextView)
            root.addView(scroll, slot, params)
        }
        val deadline = SystemClock.uptimeMillis() + LAYOUT_TIMEOUT_MS
        var ready = false
        do {
            instrumentation.waitForIdleSync()
            scenario.onActivity {
                ready = viewer.width > 0 && viewer.height > 0 && viewer.isShown &&
                    !viewer.isLayoutRequested
            }
            if (ready) break
            SystemClock.sleep(FRAME_WAIT_MS)
        } while (SystemClock.uptimeMillis() < deadline)
        scenario.onActivity {
            assertTrue("viewer did not attach: ${viewer.width}x${viewer.height} " +
                "scroll=${scroll.width}x${scroll.height} child=${scroll.childCount} " +
                "shown=${viewer.isShown} attached=${viewer.isAttachedToWindow} " +
                "layout=${viewer.preparedLayout?.widthPx}x${viewer.preparedLayout?.heightPx}",
                ready)
        }
        instrumentation.saveDeviceScreenshot("native-table-scroll-viewer-before.png")
        test(scenario, viewer, scroll)
    }

    private fun awaitEditorTable(scenario: ActivityScenario<NativeTableHostActivity>) {
        val deadline = SystemClock.uptimeMillis() + LAYOUT_TIMEOUT_MS
        do {
            instrumentation.waitForIdleSync()
            var ready = false
            scenario.onActivity { activity ->
                val host = tableHosts(activity.richTextView).singleOrNull()
                ready = host?.preparedLayout?.blocks?.any { it.tableSurface != null } == true &&
                    host.width > 0 && activity.richTextView.editorEditText.layout != null
            }
            if (ready) return
            SystemClock.sleep(FRAME_WAIT_MS)
        } while (SystemClock.uptimeMillis() < deadline)
        error("overflow table did not mount within $LAYOUT_TIMEOUT_MS ms")
    }

    private fun editorTableHost(activity: NativeTableHostActivity) =
        tableHosts(activity.richTextView).single()

    private fun editorTableBodyPoint(scenario: ActivityScenario<NativeTableHostActivity>): Point {
        var point = Point(0f, 0f)
        scenario.onActivity { activity ->
            val host = editorTableHost(activity)
            val center = visibleTablePoint(host)
            val location = IntArray(2)
            host.getLocationOnScreen(location)
            val y = center.y - location[1]
            val cell = host.presentedTableCells().single { presented ->
                presented.bounds.contains(center.x - location[0], y) &&
                    presented.clip.contains(center.x - location[0], y)
            }
            val x = cell.bounds.centerX()
            assertNull("the drag must start on the table body, away from every column resize edge",
                host.hitResizeEdge(x, y))
            point = Point(location[0] + x, center.y)
        }
        return point
    }

    private fun editorBodyRowColumnEdgePoint(scenario: ActivityScenario<NativeTableHostActivity>): Point {
        var point = Point(0f, 0f)
        scenario.onActivity { activity ->
            val host = editorTableHost(activity)
            val visible = Rect()
            assertTrue(host.getLocalVisibleRect(visible))
            val columns = requireNotNull(host.preparedLayout!!.blocks.single { it.tableSurface != null }
                .tableSurface!!.sourceTable).columns.toInt()
            val column = (0 until columns).last { column ->
                val cell = presentedCell(host, EDITABLE_ROW, column)
                cell.bounds.right < minOf(cell.clip.right, visible.right.toFloat())
            }
            val body = presentedCell(host, EDITABLE_ROW, column)
            val firstRow = presentedCell(host, HANDLE_ROW, column)
            assertEquals("the same edge in the first row must be a resize handle", column,
                host.hitResizeEdge(body.bounds.right, firstRow.bounds.centerY())?.column)
            assertNull("the body-row edge must not be a resize handle",
                host.hitResizeEdge(body.bounds.right, body.bounds.centerY()))
            val location = IntArray(2)
            host.getLocationOnScreen(location)
            point = Point(location[0] + body.bounds.right, location[1] + body.bounds.centerY())
        }
        return point
    }

    private fun editorCellPoint(
        scenario: ActivityScenario<NativeTableHostActivity>,
        row: Int,
        column: Int
    ): Point {
        var point = Point(0f, 0f)
        scenario.onActivity { activity ->
            val host = editorTableHost(activity)
            val cell = presentedCell(host, row, column)
            val visibleLeft = maxOf(cell.bounds.left, cell.clip.left)
            val visibleRight = minOf(cell.bounds.right, cell.clip.right)
            assertTrue("target cell is outside the table clip", visibleRight > visibleLeft)
            val location = IntArray(2)
            host.getLocationOnScreen(location)
            point = Point(location[0] + (visibleLeft + visibleRight) / 2f,
                location[1] + cell.bounds.centerY())
        }
        return point
    }

    private fun viewerTablePoint(
        scenario: ActivityScenario<NativeTableHostActivity>,
        viewer: PreparedProseDrawingView
    ): Point {
        var point = Point(0f, 0f)
        scenario.onActivity {
            point = visibleTablePoint(viewer)
        }
        return point
    }

    private fun dragDistance(scenario: ActivityScenario<NativeTableHostActivity>): Float {
        var distance = 0f
        scenario.onActivity { distance = activityWidth(it) * DRAG_WIDTH_FRACTION }
        return distance
    }

    private fun activityWidth(activity: NativeTableHostActivity) = activity.richTextView.width.toFloat()

    private fun viewerDragDistance(viewer: PreparedProseDrawingView) =
        viewer.width * DRAG_WIDTH_FRACTION

    private fun verticalDragDistance(start: Point, scroll: View): Float {
        val location = IntArray(2)
        scroll.getLocationOnScreen(location)
        val density = scroll.resources.displayMetrics.density
        val distance = minOf(VERTICAL_DRAG_DP * density,
            start.y - location[1] - VERTICAL_END_MARGIN_DP * density)
        assertTrue("table did not leave a usable vertical gesture path", distance >
            ViewConfiguration.get(scroll.context).scaledTouchSlop * 2)
        return distance
    }

    private fun activeCellDragDistance(
        scenario: ActivityScenario<NativeTableHostActivity>
    ): Float {
        var distance = 0f
        scenario.onActivity { activity ->
            distance = presentedCell(editorTableHost(activity), EDITABLE_ROW, ALPHA_COLUMN)
                .bounds.width() * PARTIAL_CELL_DRAG_FRACTION
        }
        return distance
    }

    private fun presentedCell(host: PreparedProseDrawingView, row: Int, column: Int):
        com.apollohg.editor.tables.ViewerTablePresentedCell {
        val surface = host.preparedLayout!!.blocks.single { it.tableSurface != null }
            .tableSurface!!
        val source = requireNotNull(surface.sourceTable).cells.single {
            it.row.toInt() == row && it.column.toInt() == column
        }.sourcePos.toInt()
        return host.presentedTableCells().single { it.sourcePosition == source }
    }

    private fun visibleTablePoint(host: PreparedProseDrawingView): Point {
        val block = host.preparedLayout!!.blocks.single { it.tableSurface != null }
        val surface = requireNotNull(block.tableSurface)
        val table = requireNotNull(block.tableBounds)
        val visible = Rect()
        assertTrue("host has no visible rectangle: ${host.width}x${host.height} " +
            "shown=${host.isShown} attached=${host.isAttachedToWindow}",
            host.getLocalVisibleRect(visible))
        assertTrue("table viewport is not visible", visible.intersect(Rect(table.left, table.top,
            table.left + surface.hostViewportWidth.toInt(), table.bottom)))
        val location = IntArray(2)
        host.getLocationOnScreen(location)
        return Point(location[0] + visible.exactCenterX(),
            location[1] + visible.exactCenterY())
    }

    private fun tap(point: Point) {
        val downTime = SystemClock.uptimeMillis()
        send(downTime, downTime, MotionEvent.ACTION_DOWN, point)
        sendAt(downTime, downTime + TAP_DURATION_MS, MotionEvent.ACTION_UP, point)
        instrumentation.waitForIdleSync()
    }

    private fun activeInputTextPoint(
        scenario: ActivityScenario<NativeTableHostActivity>
    ): Point {
        var point = Point(0f, 0f)
        scenario.onActivity { activity ->
            val input = activity.currentFocus as EditorEditText
            val layout = requireNotNull(input.layout)
            val location = IntArray(2)
            input.getLocationOnScreen(location)
            val offset = minOf(TEXT_SELECTION_OFFSET, input.text.length)
            val line = layout.getLineForOffset(offset)
            point = Point(location[0] + input.totalPaddingLeft + layout.getPrimaryHorizontal(offset),
                location[1] + input.totalPaddingTop +
                    (layout.getLineTop(line) + layout.getLineBottom(line)) / 2f)
        }
        return point
    }

    private fun drag(start: Point, deltaX: Float, deltaY: Float) {
        val downTime = SystemClock.uptimeMillis()
        send(downTime, downTime, MotionEvent.ACTION_DOWN, start)
        moveAndRelease(downTime, start, deltaX, deltaY)
    }

    private fun moveAndRelease(downTime: Long, start: Point, deltaX: Float, deltaY: Float) {
        val slop = ViewConfiguration.get(instrumentation.targetContext).scaledTouchSlop
        val firstFraction = (slop * FIRST_STEP_SLOP_FACTOR / hypot(deltaX, deltaY))
            .coerceIn(1f / DRAG_STEPS, 1f)
        val begin = SystemClock.uptimeMillis()
        for (step in 1..DRAG_STEPS) {
            val fraction = firstFraction + (1f - firstFraction) * (step - 1) / (DRAG_STEPS - 1)
            sendAt(downTime, begin + step * DRAG_STEP_MS, MotionEvent.ACTION_MOVE,
                Point(start.x + deltaX * fraction, start.y + deltaY * fraction))
        }
        val end = Point(start.x + deltaX, start.y + deltaY)
        val settled = begin + DRAG_STEPS * DRAG_STEP_MS + RELEASE_SETTLE_MS
        sendAt(downTime, settled, MotionEvent.ACTION_MOVE, end)
        sendAt(downTime, settled, MotionEvent.ACTION_UP, end)
        instrumentation.waitForIdleSync()
    }

    private fun sendAt(downTime: Long, eventTime: Long, action: Int, point: Point) {
        SystemClock.sleep((eventTime - SystemClock.uptimeMillis()).coerceAtLeast(0))
        send(downTime, eventTime, action, point)
    }

    private fun send(downTime: Long, eventTime: Long, action: Int, point: Point) {
        val event = MotionEvent.obtain(downTime, eventTime, action, point.x, point.y, 0)
        try {
            assertTrue("UiAutomation rejected ${MotionEvent.actionToString(action)}",
                instrumentation.uiAutomation.injectInputEvent(event, false))
        } finally {
            event.recycle()
        }
    }

    private fun assertUnchanged(activity: NativeTableHostActivity) {
        assertEquals(activity.documentBeforeMount, activity.adapter.documentJson())
        assertEquals(activity.revisionBeforeMount, activity.adapter.baseDocumentRevision)
        assertEquals(activity.historyBeforeMount,
            activity.adapter.historyCanUndo() to activity.adapter.historyCanRedo())
    }

    private companion object {
        const val LAYOUT_TIMEOUT_MS = 15_000L
        const val FRAME_WAIT_MS = 16L
        const val TAP_DURATION_MS = 40L
        const val DRAG_STEP_MS = 60L
        const val RELEASE_SETTLE_MS = 250L
        const val LONG_PRESS_SETTLE_MS = 150L
        const val TEXT_SELECTION_OFFSET = 2
        const val DRAG_STEPS = 6
        const val FIRST_STEP_SLOP_FACTOR = 1.5f
        const val DRAG_WIDTH_FRACTION = 0.3f
        const val VERTICAL_DRAG_X_FRACTION = 0.45f
        const val PARTIAL_CELL_DRAG_FRACTION = 0.4f
        const val NESTED_DRAG_FRACTION = 0.35f
        const val NESTED_START_FRACTION = 0.75f
        const val VERTICAL_DRAG_DP = 72f
        const val VERTICAL_END_MARGIN_DP = 16f
        const val SELECTION_DRAG_DP = 24f
        const val OFFSET_TOLERANCE_PX = 0.5f
        const val DRAG_ROUNDING_TOLERANCE_PX = DRAG_STEPS * OFFSET_TOLERANCE_PX
        const val TABLE_BLOCK_INDEX = 1
        const val HANDLE_ROW = 0
        const val EDITABLE_ROW = 1
        const val ALPHA_COLUMN = 0
        const val OWNER_COLUMN = 1
    }
}
