package com.apollohg.editor

import android.content.Intent
import android.graphics.Bitmap
import android.os.SystemClock
import android.text.Spanned
import android.view.View
import android.view.ViewGroup
import android.widget.LinearLayout
import androidx.test.core.app.ActivityScenario
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import com.apollohg.editor.viewer.PreparedProseDrawingView
import java.io.File
import org.json.JSONObject
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
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
