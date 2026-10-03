package com.apollohg.editor.tables

import android.app.Activity
import android.content.Context
import android.view.InputDevice
import android.view.MotionEvent
import android.view.View
import android.widget.FrameLayout
import androidx.core.view.NestedScrollingParent3
import androidx.core.view.NestedScrollingParentHelper
import androidx.core.view.ViewCompat
import com.apollohg.editor.ProseViewerConfiguration
import com.apollohg.editor.ProseViewerSource
import com.apollohg.editor.viewer.FabricReplacementAccessibilityTransaction
import com.apollohg.editor.viewer.PreparedProseDrawingView
import com.apollohg.editor.viewer.PreparedProseLayout
import com.apollohg.editor.viewer.PreparedProseTheme
import com.apollohg.editor.viewer.ProseLayoutKey
import com.apollohg.editor.viewer.ProseViewerRequest
import com.apollohg.editor.viewer.StaticLayoutAndroidProseLayoutEngine
import com.apollohg.editor.viewer.compileWithRust
import java.time.Duration
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.Robolectric
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config
import org.robolectric.shadows.ShadowSystemClock

@RunWith(RobolectricTestRunner::class)
@Config(sdk = [34])
internal class TableInteractionTest {
    private companion object {
        const val WIDTH = 320
        const val TABLE_SOURCE = """{"type":"doc","content":[{"type":"table",""" +
            """"content":[{"type":"table_row",""" +
            """"content":[{"type":"table_cell",""" +
            """"attrs":{"colwidth":[600]},""" +
            """"content":[{"type":"paragraph",""" +
            """"content":[{"type":"text","text":"Left"}]}]},""" +
            """{"type":"table_cell","attrs":{"colwidth":[600]},""" +
            """"content":[{"type":"paragraph",""" +
            """"content":[{"type":"text","text":"Right"}]}]}]}]}]}"""
        const val NESTED_SOURCE = """{"type":"doc","content":[{"type":"table",""" +
            """"content":[{"type":"table_row",""" +
            """"content":[{"type":"table_cell",""" +
            """"attrs":{"colwidth":[600]},"content":[{"type":"table",""" +
            """"content":[{"type":"table_row",""" +
            """"content":[{"type":"table_cell",""" +
            """"attrs":{"colwidth":[500]},""" +
            """"content":[{"type":"paragraph",""" +
            """"content":[{"type":"text","text":"Inner A"}]}]},""" +
            """{"type":"table_cell","attrs":{"colwidth":[500]},""" +
            """"content":[{"type":"paragraph",""" +
            """"content":[{"type":"text","text":"Inner B"}]}]}]}]}]},""" +
            """{"type":"table_cell","attrs":{"colwidth":[600]},""" +
            """"content":[{"type":"paragraph",""" +
            """"content":[{"type":"text","text":"Outer"}]}]}]}]}]}"""
        const val GAP_SOURCE = """{"type":"doc","content":[{"type":"table",""" +
            """"content":[{"type":"table_row",""" +
            """"content":[{"type":"table_cell","attrs":{"rowspan":2,""" +
            """"colwidth":[100]},"content":[{"type":"paragraph",""" +
            """"content":[{"type":"text","text":"Tall"}]}]}]},""" +
            """{"type":"table_row","content":[{"type":"table_cell",""" +
            """"attrs":{"colwidth":[600]},""" +
            """"content":[{"type":"paragraph",""" +
            """"content":[{"type":"text","text":"Second """ +
            """row"}]}]}]}]}]}"""
    }

    private fun prepare(
        width: Int = WIDTH,
        source: String = TABLE_SOURCE,
        rtl: Boolean = false
    ): PreparedProseLayout {
        val configuredSource = if (rtl) {
            source.replaceFirst(
                "\"type\":\"table\",\"content\"",
                "\"type\":\"table\",\"attrs\":{\"dir\":\"rtl\"},\"content\""
            )
        } else {
            source
        }
        val config = if (rtl) {
            ViewerTableTest.CONFIG.replace(
                "\"class\":{\"default\":null}",
                "\"class\":{\"default\":null},\"dir\":{\"default\":null}"
            )
        } else {
            ViewerTableTest.CONFIG
        }
        val document = compileWithRust(
            ProseViewerRequest(
                ProseViewerSource.Json(configuredSource),
                ProseViewerConfiguration(config)
            )
        )
        val key =
            ProseLayoutKey(
                document.semanticKey,
                width,
                "scroll-test",
                0,
                0,
                1,
                0,
                document.semanticKey
            )
        return StaticLayoutAndroidProseLayoutEngine().prepare(
            document,
            key,
            PreparedProseTheme.resolve(null, 1f),
            width,
            1f,
            false
        )
    }

    private fun mounted(
        hostFactory: (Context) -> FrameLayout = ::FrameLayout,
        layout: PreparedProseLayout = prepare(),
        block: (FrameLayout, PreparedProseDrawingView, PreparedProseLayout) -> Unit
    ) {
        val controller = Robolectric.buildActivity(Activity::class.java).setup()
        try {
            val host = hostFactory(controller.get())
            controller.get().setContentView(host)
            val drawing = PreparedProseDrawingView(controller.get())
            drawing.install(layout)
            val height = layout.heightPx.coerceAtLeast(100)
            host.addView(drawing, FrameLayout.LayoutParams(WIDTH, height))
            host.measure(
                View.MeasureSpec.makeMeasureSpec(WIDTH, View.MeasureSpec.EXACTLY),
                View.MeasureSpec.makeMeasureSpec(height, View.MeasureSpec.EXACTLY)
            )
            host.layout(0, 0, WIDTH, height)
            block(host, drawing, layout)
        } finally {
            controller.pause().stop().destroy()
        }
    }

    private fun dispatchPoints(
        host: View,
        points: List<Pair<Float, Float>>,
        ending: Int = MotionEvent.ACTION_CANCEL
    ) {
        val events = points.mapIndexed { index, point ->
            val action = when (index) {
                0 -> MotionEvent.ACTION_DOWN
                points.lastIndex -> ending
                else -> MotionEvent.ACTION_MOVE
            }
            MotionEvent.obtain(0, index * 16L, action, point.first, point.second, 0)
        }
        try {
            events.forEach(host::dispatchTouchEvent)
        } finally {
            events.forEach(MotionEvent::recycle)
        }
    }

    private fun dispatch(
        host: View,
        downX: Float,
        downY: Float,
        moveX: Float,
        moveY: Float,
        end: Int = MotionEvent.ACTION_UP
    ) {
        val events = listOf(
            MotionEvent.obtain(0, 0, MotionEvent.ACTION_DOWN, downX, downY, 0),
            MotionEvent.obtain(0, 16, MotionEvent.ACTION_MOVE, moveX, moveY, 0),
            MotionEvent.obtain(0, 32, end, moveX, moveY, 0)
        )
        try {
            events.forEach {
                assertTrue("event action=${it.actionMasked}", host.dispatchTouchEvent(it))
            }
        } finally {
            events.forEach(MotionEvent::recycle)
        }
    }

    @Test
    fun `mounted viewer dispatch scrolls overflowing table without activating its cell`() =
        mounted {
                host,
                drawing,
                layout
            ->
            val table = requireNotNull(layout.blocks.single().tableSurface)
            var activations = 0
            drawing.onInteractionActivated = {
                activations++
                true
            }
            val y = requireNotNull(layout.blocks.single().tableBounds).centerY().toFloat()
            dispatch(host, 200f, y, 100f, y)
            assertTrue(
                "physical offset must move on real dispatch",
                drawing.tablePhysicalOffsetForTesting(table.identity) > 0f
            )
            assertEquals("drag must not activate a cell interaction", 0, activations)
        }

    @Test
    fun `same semantic reflow keeps offset while a different owner resets it`() {
        val layout = prepare()
        mounted(layout = layout) { _, drawing, artifact ->
            val key = artifact.key
            val surface = requireNotNull(artifact.blocks.single().tableSurface)
            drawing.setTableLogicalOffset(surface.identity, 150f)
            assertEquals(150f, drawing.tablePhysicalOffsetForTesting(surface.identity), 0.01f)

            drawing.install(artifact.copy(key = key.copy(widthPx = 321)))
            assertEquals(150f, drawing.tablePhysicalOffsetForTesting(surface.identity), 0.01f)

            drawing.install(
                artifact.copy(key = key.copy(semanticGenerationIdentity = "replacement"))
            )
            assertEquals(0f, drawing.tablePhysicalOffsetForTesting(surface.identity), 0.01f)
        }
    }

    @Test
    fun `Fabric clear and replacement retains same owner but releases abandoned state`() {
        val layout = prepare()
        mounted(layout = layout) { _, drawing, artifact ->
            val surface = requireNotNull(artifact.blocks.single().tableSurface)
            val transaction = FabricReplacementAccessibilityTransaction()
            drawing.setTableLogicalOffset(surface.identity, 150f)
            transaction.clearReplacing(drawing)
            transaction.installMountedReplacement(drawing, artifact.copy())
            assertEquals(150f, drawing.tablePhysicalOffsetForTesting(surface.identity), 0.01f)

            transaction.clearReplacing(drawing)
            transaction.finishWithoutMountedReplacement(drawing)
            drawing.install(artifact.copy())
            assertEquals(0f, drawing.tablePhysicalOffsetForTesting(surface.identity), 0.01f)
        }
    }

    private class RecordingParent(context: Context) :
        FrameLayout(context),
        NestedScrollingParent3 {
        private val helper = NestedScrollingParentHelper(this)
        val post = mutableListOf<Pair<Int, Int>>()
        val disallow = mutableListOf<Boolean>()
        val flings = mutableListOf<Pair<Float, Boolean>>()
        var preConsumedX = 30
        var shiftOnPre = 0
        var onPreScroll: (() -> Unit)? = null
        var onDisallow: ((Boolean) -> Unit)? = null
        var onStopNested: ((Int) -> Unit)? = null
        val stoppedTypes = mutableListOf<Int>()

        override fun requestDisallowInterceptTouchEvent(disallowIntercept: Boolean) {
            disallow += disallowIntercept
            onDisallow?.invoke(disallowIntercept)
            super.requestDisallowInterceptTouchEvent(disallowIntercept)
        }

        override fun onStartNestedScroll(child: View, target: View, axes: Int, type: Int): Boolean =
            axes and ViewCompat.SCROLL_AXIS_HORIZONTAL != 0
        override fun onNestedScrollAccepted(child: View, target: View, axes: Int, type: Int) =
            helper.onNestedScrollAccepted(child, target, axes, type)
        override fun onStopNestedScroll(target: View, type: Int) {
            stoppedTypes += type
            onStopNested?.invoke(type)
            helper.onStopNestedScroll(target, type)
        }
        override fun getNestedScrollAxes(): Int = helper.nestedScrollAxes
        override fun onNestedPreScroll(
            target: View,
            dx: Int,
            dy: Int,
            consumed: IntArray,
            type: Int
        ) {
            consumed[0] = preConsumedX.coerceAtMost(dx.coerceAtLeast(0))
            onPreScroll?.invoke()
            if (shiftOnPre != 0) target.offsetLeftAndRight(shiftOnPre)
        }
        override fun onNestedScroll(
            target: View,
            dxConsumed: Int,
            dyConsumed: Int,
            dxUnconsumed: Int,
            dyUnconsumed: Int,
            type: Int
        ) = Unit
        override fun onNestedScroll(
            target: View,
            dxConsumed: Int,
            dyConsumed: Int,
            dxUnconsumed: Int,
            dyUnconsumed: Int,
            type: Int,
            consumed: IntArray
        ) {
            post += dxConsumed to dxUnconsumed
        }
        override fun onStartNestedScroll(child: View, target: View, axes: Int): Boolean =
            onStartNestedScroll(child, target, axes, ViewCompat.TYPE_TOUCH)
        override fun onNestedScrollAccepted(child: View, target: View, axes: Int) =
            onNestedScrollAccepted(child, target, axes, ViewCompat.TYPE_TOUCH)
        override fun onStopNestedScroll(target: View) =
            onStopNestedScroll(target, ViewCompat.TYPE_TOUCH)
        override fun onNestedPreScroll(target: View, dx: Int, dy: Int, consumed: IntArray) =
            onNestedPreScroll(target, dx, dy, consumed, ViewCompat.TYPE_TOUCH)
        override fun onNestedScroll(
            target: View,
            dxConsumed: Int,
            dyConsumed: Int,
            dxUnconsumed: Int,
            dyUnconsumed: Int
        ) = onNestedScroll(
            target,
            dxConsumed,
            dyConsumed,
            dxUnconsumed,
            dyUnconsumed,
            ViewCompat.TYPE_TOUCH
        )
        override fun onNestedFling(
            target: View,
            velocityX: Float,
            velocityY: Float,
            consumed: Boolean
        ): Boolean {
            flings += velocityX to consumed
            return false
        }
        override fun onNestedPreFling(target: View, velocityX: Float, velocityY: Float) = false
    }

    @Test
    fun `nested parent preconsumption leaves only local delta in post scroll`() =
        mounted(::RecordingParent) { host, drawing, layout ->
            val table = requireNotNull(layout.blocks.single().tableSurface)
            val y = requireNotNull(layout.blocks.single().tableBounds).centerY().toFloat()
            dispatch(host, 200f, y, 100f, y)
            assertEquals(70f, drawing.tablePhysicalOffsetForTesting(table.identity), 0.01f)
            assertEquals(70 to 0, (host as RecordingParent).post.single())
        }

    @Test
    fun `touch slop and diagonal tie stay vertical while one pixel beyond bias scrolls`() =
        mounted { host, drawing, layout ->
            val surface = requireNotNull(layout.blocks.single().tableSurface)
            val y = requireNotNull(layout.blocks.single().tableBounds).centerY().toFloat()
            val down = 250f to y
            dispatchPoints(host, listOf(down, 246f to y, down))
            assertEquals(0f, drawing.tablePhysicalOffsetForTesting(surface.identity), 0.01f)

            dispatchPoints(
                host,
                listOf(
                    down,
                    125f to (y + 100f),
                    50f to (y + 100f),
                    50f to (y + 100f)
                )
            )
            assertEquals(
                "vertical tie must not flip on later horizontal move",
                0f,
                drawing.tablePhysicalOffsetForTesting(surface.identity),
                0.01f
            )

            dispatchPoints(
                host,
                listOf(
                    down,
                    124f to (y + 100f),
                    124f to (y + 100f)
                )
            )
            assertEquals(126f, drawing.tablePhysicalOffsetForTesting(surface.identity), 0.01f)
        }

    @Test
    fun `no overflow table does not take horizontal gesture`() {
        val noOverflow = TABLE_SOURCE.replace("\"colwidth\":[600]", "\"colwidth\":[100]")
        mounted(layout = prepare(source = noOverflow)) { host, drawing, layout ->
            val surface = requireNotNull(layout.blocks.single().tableSurface)
            assertTrue(surface.bounds.width() <= surface.hostViewportWidth)
            val y = requireNotNull(layout.blocks.single().tableBounds).centerY().toFloat()
            dispatchPoints(host, listOf(250f to y, 124f to y, 124f to y))
            assertEquals(0f, drawing.tablePhysicalOffsetForTesting(surface.identity), 0.01f)
            assertFalse(drawing.ownsHorizontalTableGesture)
        }
    }

    @Test
    fun `RTL begins at inline start and drag preserves logical offset on reflow`() {
        val layout = prepare(rtl = true)
        mounted(layout = layout) { host, drawing, artifact ->
            val surface = requireNotNull(artifact.blocks.single().tableSurface)
            val maximum = surface.bounds.width() - surface.hostViewportWidth
            assertEquals(maximum, drawing.tablePhysicalOffsetForTesting(surface.identity), 0.01f)
            val y = requireNotNull(artifact.blocks.single().tableBounds).centerY().toFloat()
            dispatchPoints(host, listOf(100f to y, 200f to y, 200f to y))
            assertEquals(
                maximum - 100f,
                drawing.tablePhysicalOffsetForTesting(surface.identity),
                0.01f
            )
            val wider = prepare(width = 400, rtl = true)
            drawing.install(wider)
            val next = requireNotNull(wider.blocks.single().tableSurface)
            assertEquals(
                next.bounds.width() - next.hostViewportWidth - 100f,
                drawing.tablePhysicalOffsetForTesting(next.identity),
                0.01f
            )
        }
    }

    @Test
    fun `column anchor survives width and direction changes then clamps after column removal`() {
        val initial = requireNotNull(prepare().blocks.single().tableSurface)
        val owner = ViewerTablePresentationOwner()
        val withinSecondColumn = 20f
        owner.setLogicalOffset(initial.layout.columnWidths.first() + withinSecondColumn, initial)

        val widerFirstColumn = TABLE_SOURCE.replaceFirst("\"colwidth\":[600]", "\"colwidth\":[800]")
        val changed = requireNotNull(
            prepare(source = widerFirstColumn, rtl = true)
                .blocks.single().tableSurface
        )
        owner.reconcile(listOf(changed))
        val expectedLogical = changed.layout.columnWidths.first() + withinSecondColumn
        assertEquals(expectedLogical, owner.logicalOffset(changed), 0.01f)
        assertEquals(
            owner.maximumOffset(changed) - expectedLogical,
            owner.physicalOffset(changed),
            0.01f
        )

        val oneColumn = """{"type":"doc","content":[{"type":"table",""" +
            """"content":[{"type":"table_row",""" +
            """"content":[{"type":"table_cell",""" +
            """"attrs":{"colwidth":[1000]},""" +
            """"content":[{"type":"paragraph",""" +
            """"content":[{"type":"text","text":"Only"}]}]}]}]}]}"""
        val removed = requireNotNull(prepare(source = oneColumn).blocks.single().tableSurface)
        owner.reconcile(listOf(removed))
        assertEquals(owner.maximumOffset(removed), owner.logicalOffset(removed), 0.01f)
    }

    @Test
    fun `nested table consumes first and passes remaining distance to enclosing table`() {
        val layout = prepare(source = NESTED_SOURCE)
        mounted(layout = layout) { host, drawing, artifact ->
            val outer = requireNotNull(artifact.blocks.single().tableSurface)
            fun nestedPoint(): Pair<Float, Float> {
                val inner = drawing.presentedTableCells().first {
                    it.surface !== outer &&
                        it.bounds.right > it.clip.left && it.bounds.left < it.clip.right
                }
                val left = maxOf(inner.bounds.left, inner.clip.left)
                val top = maxOf(inner.bounds.top, inner.clip.top)
                return (
                    left + minOf(
                        100f,
                        minOf(inner.bounds.right, inner.clip.right) - left - 1f
                    )
                    ) to
                    (top + minOf(10f, minOf(inner.bounds.bottom, inner.clip.bottom) - top - 1f))
            }
            val inner = drawing.presentedTableCells().first { it.surface !== outer }.surface
            val first = nestedPoint()
            dispatchPoints(
                host,
                listOf(
                    first,
                    (first.first - 100f) to first.second,
                    (first.first - 100f) to first.second
                )
            )
            assertEquals(100f, drawing.tablePhysicalOffsetForTesting(inner.identity), 0.01f)
            assertEquals(0f, drawing.tablePhysicalOffsetForTesting(outer.identity), 0.01f)

            val innerMaximum = inner.bounds.width() - inner.hostViewportWidth
            drawing.setTableLogicalOffset(inner.identity, innerMaximum - 20f)
            val second = nestedPoint()
            dispatchPoints(
                host,
                listOf(
                    second,
                    (second.first - 100f) to second.second,
                    (second.first - 100f) to second.second
                )
            )
            assertEquals(innerMaximum, drawing.tablePhysicalOffsetForTesting(inner.identity), 0.01f)
            assertEquals(80f, drawing.tablePhysicalOffsetForTesting(outer.identity), 0.01f)
        }
    }

    @Test
    fun `synthetic gap inside overflowing table can start scrolling without a cell target`() {
        mounted(layout = prepare(source = GAP_SOURCE)) { host, drawing, layout ->
            val surface = requireNotNull(layout.blocks.single().tableSurface)
            val first = surface.cells.first { surface.frameOfCell(it).top == 0f }
            val x = surface.hostViewportWidth / 2f
            val y = surface.frameOfCell(first).top + surface.frameOfCell(first).height / 4f
            assertTrue(
                "x=$x viewport=${surface.hostViewportWidth} " +
                    "widths=${surface.layout.columnWidths} cells=${surface.cells.map {
                        surface.frameOfCell(it)
                    }}",
                x < surface.hostViewportWidth
            )
            assertTrue(
                surface.cells.none { cell ->
                    x >= surface.frameOfCell(cell).left &&
                        x < surface.frameOfCell(cell).left + surface.frameOfCell(cell).width &&
                        y >= surface.frameOfCell(cell).top &&
                        y < surface.frameOfCell(cell).top + surface.frameOfCell(cell).height
                }
            )
            dispatchPoints(host, listOf(x to y, (x - 80f) to y, (x - 80f) to y))
            assertEquals(80f, drawing.tablePhysicalOffsetForTesting(surface.identity), 0.01f)
        }
    }

    @Test
    fun `parent receives remaining edge delta after local table reaches both ends`() =
        mounted(::RecordingParent) { host, drawing, layout ->
            val parent = host as RecordingParent
            parent.preConsumedX = 0
            val surface = requireNotNull(layout.blocks.single().tableSurface)
            val maximum = surface.bounds.width() - surface.hostViewportWidth
            val y = requireNotNull(layout.blocks.single().tableBounds).centerY().toFloat()
            dispatchPoints(host, listOf(200f to y, -1800f to y, -1800f to y))
            assertEquals(maximum, drawing.tablePhysicalOffsetForTesting(surface.identity), 0.01f)
            assertEquals(maximum.toInt() to (2000 - maximum.toInt()), parent.post.single())

            parent.post.clear()
            dispatchPoints(host, listOf(100f to y, 2100f to y, 2100f to y))
            assertEquals(0f, drawing.tablePhysicalOffsetForTesting(surface.identity), 0.01f)
            assertEquals(-maximum.toInt() to (-2000 + maximum.toInt()), parent.post.single())
        }

    @Test
    fun `parent window movement preserves following drag delta`() =
        mounted(::RecordingParent) { host, drawing, layout ->
            val parent = host as RecordingParent
            parent.shiftOnPre = 10
            val surface = requireNotNull(layout.blocks.single().tableSurface)
            val y = requireNotNull(layout.blocks.single().tableBounds).centerY().toFloat()
            dispatchPoints(host, listOf(250f to y, 150f to y, 50f to y, 50f to y))
            assertEquals(140f, drawing.tablePhysicalOffsetForTesting(surface.identity), 0.01f)
            assertEquals(listOf(70 to 0, 70 to 0), parent.post)
        }

    @Test
    fun `undecided down waits for slop then disallows only won horizontal until cancel`() =
        mounted(::RecordingParent) { host, drawing, layout ->
            val parent = host as RecordingParent
            parent.preConsumedX = 0
            val surface = requireNotNull(layout.blocks.single().tableSurface)
            val y = requireNotNull(layout.blocks.single().tableBounds).centerY().toFloat()
            val points = listOf(200f to y, 196f to y, 100f to y, 100f to y)
            val actions = listOf(
                MotionEvent.ACTION_DOWN,
                MotionEvent.ACTION_MOVE,
                MotionEvent.ACTION_MOVE,
                MotionEvent.ACTION_CANCEL
            )
            points.forEachIndexed { index, point ->
                val event = MotionEvent.obtain(
                    0,
                    index * 16L,
                    actions[index],
                    point.first,
                    point.second,
                    0
                )
                try {
                    host.dispatchTouchEvent(event)
                } finally {
                    event.recycle()
                }
                when (index) {
                    0, 1 -> assertTrue(parent.disallow.isEmpty())
                    2 -> assertEquals(listOf(true), parent.disallow)
                    3 -> assertEquals(listOf(true, false), parent.disallow)
                }
            }
            assertEquals(100f, drawing.tablePhysicalOffsetForTesting(surface.identity), 0.01f)
        }

    @Test
    fun `fling advertises consumed velocity and edge release advertises remainder`() =
        mounted(::RecordingParent) { host, drawing, layout ->
            val parent = host as RecordingParent
            parent.preConsumedX = 0
            val surface = requireNotNull(layout.blocks.single().tableSurface)
            val maximum = surface.bounds.width() - surface.hostViewportWidth
            val y = requireNotNull(layout.blocks.single().tableBounds).centerY().toFloat()
            dispatch(host, 250f, y, 150f, y)
            assertTrue(parent.flings.single().first > 0f)
            assertEquals(true, parent.flings.single().second)

            drawing.setTableLogicalOffset(surface.identity, maximum - 20f)
            parent.flings.clear()
            dispatch(host, 250f, y, 150f, y)
            assertEquals(maximum, drawing.tablePhysicalOffsetForTesting(surface.identity), 0.01f)
            assertTrue(parent.flings.single().first > 0f)
            assertEquals(false, parent.flings.single().second)
        }

    @Test
    fun `fling advances real frames and sends remaining edge velocity to nested parent`() =
        mounted(::RecordingParent) { host, drawing, layout ->
            val parent = host as RecordingParent
            parent.preConsumedX = 0
            val surface = requireNotNull(layout.blocks.single().tableSurface)
            val maximum = surface.bounds.width() - surface.hostViewportWidth
            drawing.setTableLogicalOffset(surface.identity, maximum - 220f)
            val y = requireNotNull(layout.blocks.single().tableBounds).centerY().toFloat()
            dispatch(host, 250f, y, 210f, y)
            assertEquals(listOf(true), parent.flings.map { it.second })
            repeat(60) {
                ShadowSystemClock.advanceBy(Duration.ofMillis(16))
                drawing.computeScroll()
            }
            assertEquals(maximum, drawing.tablePhysicalOffsetForTesting(surface.identity), 0.01f)
            assertTrue(
                "parent must receive remaining edge velocity after animated frames: flings=${parent.flings} post=${parent.post} axes=${parent.nestedScrollAxes}",
                parent.flings.drop(1).any { !it.second && it.first > 0f }
            )
        }

    @Test
    fun `reentrant replacement from pre scroll cancels old gesture before local or post scroll`() =
        mounted(::RecordingParent) { host, drawing, layout ->
            val parent = host as RecordingParent
            parent.preConsumedX = 0
            val surface = requireNotNull(layout.blocks.single().tableSurface)
            val replacement = layout.copy(
                key = layout.key.copy(semanticGenerationIdentity = "replacement")
            )
            parent.onPreScroll = {
                parent.onPreScroll = null
                drawing.install(replacement)
            }
            val y = requireNotNull(layout.blocks.single().tableBounds).centerY().toFloat()
            dispatchPoints(host, listOf(250f to y, 150f to y, 150f to y))
            assertEquals(0f, drawing.tablePhysicalOffsetForTesting(surface.identity), 0.01f)
            assertTrue("stale gesture must not dispatch its post scroll", parent.post.isEmpty())
        }

    @Test
    fun `reentrant replacement during intercept release cannot release old gesture twice`() =
        mounted(::RecordingParent) { host, drawing, layout ->
            val parent = host as RecordingParent
            parent.preConsumedX = 0
            parent.onDisallow = { disallow ->
                if (!disallow) {
                    parent.onDisallow = null
                    drawing.install(
                        layout.copy(
                            key = layout.key.copy(semanticGenerationIdentity = "replacement")
                        )
                    )
                }
            }
            val y = requireNotNull(layout.blocks.single().tableBounds).centerY().toFloat()
            dispatchPoints(host, listOf(250f to y, 150f to y, 150f to y))
            assertEquals(listOf(true, false), parent.disallow)
        }

    @Test
    fun `reentrant replacement during nested stop stops each gesture type once`() =
        mounted(::RecordingParent) { host, drawing, layout ->
            val parent = host as RecordingParent
            parent.preConsumedX = 0
            parent.onStopNested = { type ->
                if (type == ViewCompat.TYPE_TOUCH) {
                    parent.onStopNested = null
                    drawing.install(
                        layout.copy(
                            key = layout.key.copy(semanticGenerationIdentity = "replacement")
                        )
                    )
                }
            }
            val y = requireNotNull(layout.blocks.single().tableBounds).centerY().toFloat()
            dispatchPoints(host, listOf(250f to y, 150f to y, 150f to y))
            assertEquals(listOf(ViewCompat.TYPE_TOUCH), parent.stoppedTypes)
        }

    @Test
    fun `cancel detach and replacement reject stale motion`() = mounted { host, drawing, layout ->
        val surface = requireNotNull(layout.blocks.single().tableSurface)
        val y = requireNotNull(layout.blocks.single().tableBounds).centerY().toFloat()
        dispatchPoints(host, listOf(250f to y, 150f to y, 150f to y))
        assertEquals(100f, drawing.tablePhysicalOffsetForTesting(surface.identity), 0.01f)
        val stale = MotionEvent.obtain(0, 48, MotionEvent.ACTION_MOVE, 50f, y, 0)
        try {
            host.dispatchTouchEvent(stale)
        } finally {
            stale.recycle()
        }
        assertEquals(100f, drawing.tablePhysicalOffsetForTesting(surface.identity), 0.01f)

        val down = MotionEvent.obtain(100, 100, MotionEvent.ACTION_DOWN, 250f, y, 0)
        val move = MotionEvent.obtain(100, 116, MotionEvent.ACTION_MOVE, 150f, y, 0)
        try {
            host.dispatchTouchEvent(down)
            host.dispatchTouchEvent(move)
            drawing.install(
                layout.copy(key = layout.key.copy(semanticGenerationIdentity = "replacement"))
            )
            host.dispatchTouchEvent(move)
            assertEquals(0f, drawing.tablePhysicalOffsetForTesting(surface.identity), 0.01f)
            host.removeView(drawing)
            drawing.dispatchTouchEvent(move)
            assertEquals(0f, drawing.tablePhysicalOffsetForTesting(surface.identity), 0.01f)
        } finally {
            down.recycle()
            move.recycle()
        }
    }

    @Test
    fun `active pointer loss releases horizontal gesture and rejects later motion`() =
        mounted(::RecordingParent) { host, drawing, layout ->
            val parent = host as RecordingParent
            parent.preConsumedX = 0
            val surface = requireNotNull(layout.blocks.single().tableSurface)
            val y = requireNotNull(layout.blocks.single().tableBounds).centerY().toFloat()
            val down = MotionEvent.obtain(0, 0, MotionEvent.ACTION_DOWN, 250f, y, 0)
            val move = MotionEvent.obtain(0, 16, MotionEvent.ACTION_MOVE, 150f, y, 0)
            val properties = arrayOf(
                MotionEvent.PointerProperties().apply {
                    id = 7
                    toolType = MotionEvent.TOOL_TYPE_FINGER
                }
            )
            val coordinates = arrayOf(
                MotionEvent.PointerCoords().apply {
                    x = 50f
                    this.y = y
                    pressure = 1f
                    size = 1f
                }
            )
            val lost = MotionEvent.obtain(
                0, 32, MotionEvent.ACTION_MOVE, 1,
                properties, coordinates, 0, 0, 1f, 1f, 0, 0,
                InputDevice.SOURCE_TOUCHSCREEN, 0
            )
            val stale = MotionEvent.obtain(0, 48, MotionEvent.ACTION_MOVE, 20f, y, 0)
            val cancel = MotionEvent.obtain(0, 64, MotionEvent.ACTION_CANCEL, 20f, y, 0)
            try {
                host.dispatchTouchEvent(down)
                host.dispatchTouchEvent(move)
                assertEquals(100f, drawing.tablePhysicalOffsetForTesting(surface.identity), 0.01f)
                assertEquals(7, lost.getPointerId(0))
                drawing.dispatchTouchEvent(lost)
                assertEquals(
                    "after missing-id move",
                    100f,
                    drawing.tablePhysicalOffsetForTesting(surface.identity),
                    0.01f
                )
                host.dispatchTouchEvent(stale)
                host.dispatchTouchEvent(cancel)
                assertEquals(100f, drawing.tablePhysicalOffsetForTesting(surface.identity), 0.01f)
                assertEquals(listOf(true, false), parent.disallow)
            } finally {
                listOf(down, move, lost, stale, cancel).forEach(MotionEvent::recycle)
            }
        }
}
