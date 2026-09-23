package com.apollohg.editor

import android.content.Intent
import android.graphics.Bitmap
import android.os.SystemClock
import android.text.Spanned
import android.view.View
import android.view.ViewGroup
import android.view.MotionEvent
import android.view.inputmethod.EditorInfo
import android.view.inputmethod.InputConnection
import android.widget.LinearLayout
import androidx.test.core.app.ActivityScenario
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import androidx.test.filters.SdkSuppress
import com.apollohg.editor.viewer.PreparedProseDrawingView
import java.io.File
import java.util.concurrent.CountDownLatch
import java.util.concurrent.TimeUnit
import org.json.JSONObject
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Assert.assertSame
import org.junit.Test
import org.junit.runner.RunWith

@RunWith(AndroidJUnit4::class)
class NativeTableHostTest {
    private val instrumentation = InstrumentationRegistry.getInstrumentation()
    private data class Widths(val editor: Int, val host: Int, val prepared: Int)

    @Test
    fun tableHostReservesGridHeightAndKeepsFollowingProseMapped() {
        runFixture(dark = false, screenshotName = "native-table-host-light.png", reflow = true)
    }

    @Test
    fun darkTableHostRendersOnDevice() {
        runFixture(dark = true, screenshotName = "native-table-host-dark.png", reflow = false)
    }

    @Test
    @SdkSuppress(minSdkVersion = 29)
    fun tappingCellsReusesInputAndRetiresPreviousConnections() {
        val intent = Intent(instrumentation.targetContext, NativeTableHostActivity::class.java)
        ActivityScenario.launch<NativeTableHostActivity>(intent).use { scenario ->
            awaitTableLayout(scenario, "native-table-cell-timeout.png")
            tapCell(scenario, 2)
            saveDeviceScreenshot("native-table-cell-tapped.png")
            lateinit var cellInput: EditorEditText
            lateinit var firstConnection: InputConnection
            scenario.onActivity { activity ->
                val root = activity.richTextView.editorEditText
                assertEquals("tapping must not mutate the document", activity.documentBeforeMount,
                    activity.adapter.documentJson())
                assertTrue("focus=${activity.currentFocus} active=${activity.richTextView.activeTextInput} " +
                    "inputs=${countEditorInputs(activity.richTextView)} rootFocused=${root.hasFocus()} " +
                    "hosts=${tableHosts(activity.richTextView).size} rootText=${root.text} " +
                    "revision=${activity.adapter.baseDocumentRevision} applied=${root.lastAppliedDocumentVersion} " +
                    "tables=${activity.adapter.cachedTableRecords.keys} maps=${root.rootTableMapTableIds}",
                    activity.currentFocus is EditorEditText)
                cellInput = activity.currentFocus as EditorEditText
                assertTrue("cell tap must focus the reusable cell input", cellInput !== root)
                assertEquals("Alpha", cellInput.text.toString())
                assertEquals(2, countEditorInputs(activity.richTextView))
                cellInput.setSelection(cellInput.text.length)
                firstConnection = requireNotNull(cellInput.onCreateInputConnection(EditorInfo()))
                assertTrue(firstConnection.commitText("😀", 1))
                assertEquals("Alpha😀", cellText(activity, 1, 0))
                assertTrue(firstConnection.commitText("Z", 1))
                assertEquals("Alpha😀Z", cellText(activity, 1, 0))
                assertTrue(firstConnection.deleteSurroundingTextInCodePoints(1, 0))
                assertEquals("Alpha😀", cellText(activity, 1, 0))
                assertTrue(firstConnection.deleteSurroundingTextInCodePoints(1, 0))
                assertEquals("Alpha", cellText(activity, 1, 0))
                assertTrue(firstConnection.commitText("X", 1))
                assertEquals("AlphaX", cellText(activity, 1, 0))
            }
            instrumentation.waitForIdleSync()
            scenario.onActivity { activity ->
                assertEquals("AlphaX", cellText(activity, 1, 0))
                assertEquals("AlphaX", cellInput.text.toString())
                assertSame(cellInput, activity.currentFocus)
            }
            awaitCommittedFrame(scenario)
            saveDeviceScreenshot("native-table-cell-editing.png")
            tapCell(scenario, 3)
            lateinit var secondConnection: InputConnection
            var reflowWidth = 0
            lateinit var beforeReflow: Widths
            scenario.onActivity { activity ->
                assertSame(cellInput, activity.currentFocus)
                assertEquals("Owner", cellInput.text.toString())
                val beforeStale = activity.adapter.documentJson()
                assertFalse(firstConnection.beginBatchEdit())
                firstConnection.commitText("stale", 1)
                assertEquals(beforeStale, activity.adapter.documentJson())
                assertEquals("Owner", cellInput.text.toString())
                cellInput.setSelection(cellInput.text.length)
                secondConnection = requireNotNull(cellInput.onCreateInputConnection(EditorInfo()))
                assertTrue(secondConnection.setComposingText("pending", 1))
                assertEquals(beforeStale, activity.adapter.documentJson())
                beforeReflow = widths(activity.richTextView)
                reflowWidth = (activity.richTextView.width * 0.78f).toInt()
                activity.richTextView.layoutParams = activity.richTextView.layoutParams.apply {
                    width = reflowWidth
                }
            }
            awaitTableLayout(scenario, "native-table-cell-composition-timeout.png", reflowWidth, beforeReflow)
            scenario.onActivity { activity ->
                assertSame(cellInput, activity.currentFocus)
                assertEquals("Ownerpending", cellInput.text.toString())
                assertEquals("Owner", cellText(activity, 1, 1))
                assertTrue(secondConnection.finishComposingText())
                assertEquals("Ownerpending", cellText(activity, 1, 1))
                assertEquals("AlphaX", cellText(activity, 1, 0))
            }
            awaitCommittedFrame(scenario)
            saveDeviceScreenshot("native-table-cell-composed.png")
            tapFollowingProse(scenario)
            scenario.onActivity { activity ->
                assertSame(activity.richTextView.editorEditText, activity.currentFocus)
                val beforeStale = activity.adapter.documentJson()
                val beforeRootText = activity.richTextView.editorEditText.text.toString()
                assertFalse(secondConnection.beginBatchEdit())
                secondConnection.commitText("stale", 1)
                assertEquals(beforeStale, activity.adapter.documentJson())
                assertEquals(beforeRootText, activity.richTextView.editorEditText.text.toString())
            }
        }
    }

    private fun awaitCommittedFrame(scenario: ActivityScenario<NativeTableHostActivity>) {
        val committed = CountDownLatch(1)
        val callback = Runnable { committed.countDown() }
        scenario.onActivity { activity ->
            val decor = activity.window.decorView
            assertTrue(decor.isHardwareAccelerated)
            decor.viewTreeObserver.registerFrameCommitCallback(callback)
            decor.invalidate()
        }
        try {
            assertTrue("edited cell frame must be submitted", committed.await(3, TimeUnit.SECONDS))
        } finally {
            scenario.onActivity { activity ->
                activity.window.decorView.viewTreeObserver.unregisterFrameCommitCallback(callback)
            }
        }
    }

    private fun cellText(activity: NativeTableHostActivity, row: Int, column: Int): String {
        val document = JSONObject(requireNotNull(activity.adapter.documentJson()))
        return document.getJSONArray("content").getJSONObject(1)
            .getJSONArray("content").getJSONObject(row)
            .getJSONArray("content").getJSONObject(column)
            .getJSONArray("content").getJSONObject(0)
            .getJSONArray("content").getJSONObject(0).getString("text")
    }

    private fun tapCell(scenario: ActivityScenario<NativeTableHostActivity>, cellIndex: Int) {
        var x = 0f
        var y = 0f
        instrumentation.waitForIdleSync()
        scenario.onActivity { activity ->
            val host = tableHosts(activity.richTextView).single()
            val block = requireNotNull(host.preparedLayout).blocks.single { it.tableSurface != null }
            val table = requireNotNull(block.tableSurface)
            val sourcePosition = requireNotNull(table.sourceTable).cells[cellIndex].sourcePos.toInt()
            val frame = table.cells.single { it.sourcePosition == sourcePosition }.frame
            val origin = requireNotNull(block.tableBounds)
            val location = IntArray(2)
            host.getLocationOnScreen(location)
            x = location[0] + origin.left + frame.left + frame.width / 2f
            y = location[1] + origin.top + frame.top + frame.height / 2f
        }
        tap(x, y)
    }

    private fun tapFollowingProse(scenario: ActivityScenario<NativeTableHostActivity>) {
        var x = 0f
        var y = 0f
        instrumentation.waitForIdleSync()
        scenario.onActivity { activity ->
            val input = activity.richTextView.editorEditText
            val offset = input.text.indexOf("After table.") + 4
            val layout = requireNotNull(input.layout)
            val line = layout.getLineForOffset(offset)
            val location = IntArray(2)
            input.getLocationOnScreen(location)
            x = location[0] + input.totalPaddingLeft + layout.getPrimaryHorizontal(offset)
            y = location[1] + input.totalPaddingTop + (layout.getLineTop(line) + layout.getLineBottom(line)) / 2f
        }
        tap(x, y)
    }

    private fun tap(x: Float, y: Float) {
        val start = SystemClock.uptimeMillis()
        val down = MotionEvent.obtain(start, start, MotionEvent.ACTION_DOWN, x, y, 0)
        val up = MotionEvent.obtain(start, start + 40, MotionEvent.ACTION_UP, x, y, 0)
        try {
            instrumentation.sendPointerSync(down)
            instrumentation.sendPointerSync(up)
            instrumentation.waitForIdleSync()
        } finally {
            down.recycle()
            up.recycle()
        }
    }

    private fun runFixture(dark: Boolean, screenshotName: String, reflow: Boolean) {
        val intent = Intent(instrumentation.targetContext, NativeTableHostActivity::class.java)
            .putExtra(NativeTableHostActivity.EXTRA_DARK, dark)
        ActivityScenario.launch<NativeTableHostActivity>(intent).use { scenario ->
            awaitTableLayout(scenario, "${screenshotName.removeSuffix(".png")}-timeout.png")
            lateinit var before: Widths
            scenario.onActivity { activity ->
                assertTableLayout(activity)
                before = widths(activity.richTextView)
            }
            saveDeviceScreenshot(screenshotName)
            if (reflow) {
                var targetWidth = 0
                scenario.onActivity { activity ->
                    val editor = activity.richTextView
                    val params = editor.layoutParams as LinearLayout.LayoutParams
                    targetWidth = (editor.width * 0.78f).toInt()
                    params.width = targetWidth
                    editor.layoutParams = params
                }
                awaitTableLayout(
                    scenario,
                    "${screenshotName.removeSuffix(".png")}-reflow-timeout.png",
                    targetWidth,
                    before
                )
                scenario.onActivity { activity ->
                    assertTableLayout(activity)
                    val after = widths(activity.richTextView)
                    assertEquals(targetWidth, after.editor)
                    assertTrue("table host width did not reflow", after.host != before.host)
                    assertTrue("prepared table width did not reflow", after.prepared != before.prepared)
                }
            }
        }
    }

    private fun awaitTableLayout(
        scenario: ActivityScenario<NativeTableHostActivity>,
        timeoutScreenshotName: String,
        expectedEditorWidth: Int? = null,
        previous: Widths? = null
    ) {
        val deadline = SystemClock.uptimeMillis() + 5_000L
        var lastReadinessState = "activity not observed"
        do {
            instrumentation.waitForIdleSync()
            var ready = false
            scenario.onActivity { activity ->
                val editor = activity.richTextView
                val input = editor.editorEditText
                val adapter = activity.adapter
                val host = tableHosts(editor).singleOrNull()
                val prepared = host?.preparedLayout
                ready = editor.width > 0 && host != null && host.height > 0 &&
                    prepared != null && input.layout != null &&
                    !editor.isLayoutRequested && !host.isLayoutRequested &&
                    (expectedEditorWidth == null || editor.width == expectedEditorWidth) &&
                    (previous == null ||
                        (host.width != previous.host && prepared.widthPx != previous.prepared))
                lastReadinessState = buildString {
                    append("editor=${editor.width}x${editor.height}")
                    append(" expectedEditorWidth=$expectedEditorWidth previous=$previous")
                    append(" frame=${editor.editorContentFrame.width}x${editor.editorContentFrame.height}")
                    append(" input=${input.width}x${input.height} inputLayout=${input.layout != null}")
                    append(" hostCount=${tableHosts(editor).size} host=${host?.width}x${host?.height}")
                    append(" prepared=${prepared?.widthPx}x${prepared?.heightPx}")
                    append(" preparedBlocks=${prepared?.blocks?.size}")
                    append(" preparedTables=${prepared?.blocks?.count { it.tableSurface != null }}")
                    append(" layoutRequested(editor/frame/input/host)=")
                    append("${editor.isLayoutRequested}/${editor.editorContentFrame.isLayoutRequested}/")
                    append("${input.isLayoutRequested}/${host?.isLayoutRequested}")
                    append(" rootText=${JSONObject.quote(input.text.toString().take(180))}")
                    append(" cachedRevision=${adapter.cachedAtomicRenderDocumentRevision}")
                    append(" baseRevision=${adapter.baseDocumentRevision}")
                    append(" lastAppliedVersion=${input.lastAppliedDocumentVersion}")
                    append(" cachedMappingIds=${adapter.cachedTableInputMappings?.tables?.keys}")
                    append(" rootMapIds=${input.rootTableMapTableIds}")
                    append(" rootMapVersion=${input.rootTableMapDocumentVersion}")
                    append(" rootMapPresent=${input.rootTablePositionMap != null}")
                }
            }
            if (ready) return
            SystemClock.sleep(16L)
        } while (SystemClock.uptimeMillis() < deadline)
        val screenshot = runCatching { saveDeviceScreenshot(timeoutScreenshotName) }
            .fold(onSuccess = { it.absolutePath }, onFailure = { "failed: $it" })
        error("Native table host did not finish layout: $lastReadinessState; screenshot=$screenshot")
    }

    private fun assertTableLayout(activity: NativeTableHostActivity) {
        val editor = activity.richTextView
        val input = editor.editorEditText
        val content = input.text as Spanned
        val layout = requireNotNull(input.layout)
        val markerOffset = content.indexOf('\u200B')
        val beforeOffset = content.indexOf("Before table.")
        val afterOffset = content.indexOf("After table.")
        assertTrue("root table marker is missing", markerOffset >= 0)
        assertTrue("before prose is missing", beforeOffset >= 0)
        assertTrue("following prose is missing", afterOffset >= 0)

        val tableLine = layout.getLineForOffset(markerOffset)
        val beforeLine = layout.getLineForOffset(beforeOffset)
        val afterLine = layout.getLineForOffset(afterOffset)
        val tableLineHeight = layout.getLineBottom(tableLine) - layout.getLineTop(tableLine)
        val proseLineHeight = layout.getLineBottom(beforeLine) - layout.getLineTop(beforeLine)
        assertTrue("table marker line must reserve grid height: $tableLineHeight <= $proseLineHeight",
            tableLineHeight > proseLineHeight)

        val host = tableHosts(editor).single()
        assertTrue(host.isShown)
        assertTrue(host.width > 0 && host.height > proseLineHeight)
        val tableBlock = requireNotNull(host.preparedLayout).blocks
            .single { it.tableSurface != null }
        val surface = requireNotNull(tableBlock.tableSurface)
        assertEquals(null, surface.layout.failure)
        assertEquals(6, surface.cells.size)
        assertTrue(surface.cells.any { it.isHeader })
        assertTrue(surface.cells.any { !it.isHeader })
        val sourceCells = requireNotNull(surface.sourceTable).cells
        assertEquals(2, sourceCells.count { it.header })
        assertTrue(sourceCells.any { it.rowspan == 2u })
        assertTrue(sourceCells.any { it.colspan == 2u })
        val frames = surface.layout.rectangles.values.toList()
        assertEquals(6, frames.size)
        frames.forEach { frame -> assertTrue(frame.width > 0f && frame.height > 0f) }
        frames.forEachIndexed { index, left ->
            frames.drop(index + 1).forEach { right ->
                val overlapWidth = minOf(left.left + left.width, right.left + right.width) -
                    maxOf(left.left, right.left)
                val overlapHeight = minOf(left.top + left.height, right.top + right.height) -
                    maxOf(left.top, right.top)
                assertTrue("table cell frames overlap", overlapWidth <= 0f || overlapHeight <= 0f)
            }
        }
        val followingTop = input.top + input.totalPaddingTop + layout.getLineTop(afterLine)
        val tableBottom = host.top + requireNotNull(tableBlock.tableBounds).bottom
        assertTrue("following prose overlaps the table: $tableBottom > $followingTop",
            tableBottom <= followingTop)

        val extent = requireNotNull(
            activity.adapter.cachedTableInputMappings?.tables?.values?.single()?.extent
        )
        assertEquals(extent.scalarEnd + 1,
            input.inputScalarAtLocalUtf16(afterOffset, content.toString()))
        assertEquals(1, countEditorInputs(editor))
        assertFalse(editor.editorContentFrame.getChildAt(0) === host)
        assertEquals(activity.documentBeforeMount, activity.adapter.documentJson())
        assertEquals(activity.historyBeforeMount,
            activity.adapter.historyCanUndo() to activity.adapter.historyCanRedo())
        assertEquals(activity.revisionBeforeMount, activity.adapter.baseDocumentRevision)
    }

    private fun tableHosts(editor: RichTextEditorView): List<PreparedProseDrawingView> =
        (0 until editor.editorContentFrame.childCount)
            .map(editor.editorContentFrame::getChildAt)
            .filterIsInstance<PreparedProseDrawingView>()

    private fun widths(editor: RichTextEditorView): Widths {
        val host = tableHosts(editor).single()
        return Widths(editor.width, host.width, requireNotNull(host.preparedLayout).widthPx)
    }

    private fun countEditorInputs(view: View): Int {
        val self = if (view is EditorEditText) 1 else 0
        val children = view as? ViewGroup ?: return self
        return self + (0 until children.childCount).sumOf { countEditorInputs(children.getChildAt(it)) }
    }

    private fun saveDeviceScreenshot(filename: String): File {
        instrumentation.waitForIdleSync()
        val directory = requireNotNull(instrumentation.targetContext.getExternalFilesDir(null))
        val file = File(directory, filename)
        val bitmap = requireNotNull(instrumentation.uiAutomation.takeScreenshot())
        try {
            file.outputStream().use { assertTrue(bitmap.compress(Bitmap.CompressFormat.PNG, 100, it)) }
        } finally {
            bitmap.recycle()
        }
        return file
    }
}
