package com.apollohg.editor

import android.app.Instrumentation
import android.graphics.Bitmap
import com.apollohg.editor.viewer.PreparedProseDrawingView
import java.io.File
import org.junit.Assert.assertTrue

internal fun tableHosts(editor: RichTextEditorView): List<PreparedProseDrawingView> =
    (0 until editor.editorContentFrame.childCount)
        .map(editor.editorContentFrame::getChildAt)
        .filterIsInstance<PreparedProseDrawingView>()

internal fun Instrumentation.saveDeviceScreenshot(filename: String): File {
    waitForIdleSync()
    val directory = requireNotNull(targetContext.getExternalFilesDir(null))
    val file = File(directory, filename)
    val bitmap = requireNotNull(uiAutomation.takeScreenshot())
    try {
        file.outputStream().use { assertTrue(bitmap.compress(Bitmap.CompressFormat.PNG, PNG_QUALITY, it)) }
    } finally {
        bitmap.recycle()
    }
    return file
}

private const val PNG_QUALITY = 100
