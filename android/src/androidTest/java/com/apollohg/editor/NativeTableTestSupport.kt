package com.apollohg.editor

import android.app.Instrumentation
import android.graphics.Bitmap
import android.graphics.PointF
import android.graphics.Rect
import android.graphics.RectF
import android.os.SystemClock
import androidx.test.core.app.ActivityScenario
import com.apollohg.editor.viewer.PreparedProseDrawingView
import java.io.File
import org.junit.Assert.assertTrue

internal fun tableHosts(editor: RichTextEditorView): List<PreparedProseDrawingView> =
    (0 until editor.editorContentFrame.childCount)
        .map(editor.editorContentFrame::getChildAt)
        .filterIsInstance<PreparedProseDrawingView>()

internal fun PreparedProseDrawingView.unobstructedTableViewport(): RectF {
    val local = Rect()
    if (!getLocalVisibleRect(local)) return RectF()
    val window = Rect().also(::getWindowVisibleDisplayFrame)
    val location = IntArray(2).also(::getLocationOnScreen)
    window.offset(-location[0], -location[1])
    if (!local.intersect(window)) return RectF()
    return RectF(local)
}

internal fun Instrumentation.tableCellScreenPoint(
    scenario: ActivityScenario<NativeTableHostActivity>,
    sourceIndex: Int
): PointF {
    val deadline = SystemClock.uptimeMillis() + CELL_REVEAL_TIMEOUT_MS
    var point: PointF? = null
    var previous: PointF? = null
    var detail = "cell $sourceIndex"
    do {
        waitForIdleSync()
        scenario.onActivity { activity ->
            val host = tableHosts(activity.richTextView).single()
            val surface = requireNotNull(host.preparedLayout).blocks.mapNotNull {
                it.tableSurface
            }.single()
            val target = requireNotNull(host.tableAccessibilityLocation(surface, sourceIndex)).cell
            host.revealTableAccessibilityCell(target)
            val cell = requireNotNull(host.presentedAccessibilityCell(target))
            val viewport = host.unobstructedTableViewport()
            val visible = RectF(cell.bounds)
            detail = "cell $sourceIndex bounds=${cell.bounds} clip=${cell.clip} viewport=$viewport"
            point = if (visible.intersect(cell.clip) && visible.intersect(viewport) &&
                activity.hasWindowFocus() && !activity.window.decorView.isLayoutRequested
            ) {
                val location = IntArray(2).also(host::getLocationOnScreen)
                PointF(
                    location[0] + visible.centerX(),
                    location[1] + visible.centerY()
                )
            } else {
                null
            }
        }
        if (point != null && point == previous) return requireNotNull(point)
        previous = point
        SystemClock.sleep(CELL_REVEAL_FRAME_MS)
    } while (SystemClock.uptimeMillis() < deadline)
    error("No settled, unobstructed tap target: $detail")
}

internal fun Instrumentation.saveDeviceScreenshot(filename: String): File {
    waitForIdleSync()
    val directory = requireNotNull(targetContext.getExternalFilesDir(null))
    val file = File(directory, filename)
    val bitmap = requireNotNull(uiAutomation.takeScreenshot())
    try {
        file.outputStream().use {
            assertTrue(bitmap.compress(Bitmap.CompressFormat.PNG, PNG_QUALITY, it))
        }
    } finally {
        bitmap.recycle()
    }
    return file
}

private const val PNG_QUALITY = 100
private const val CELL_REVEAL_TIMEOUT_MS = 3_000L
private const val CELL_REVEAL_FRAME_MS = 16L
