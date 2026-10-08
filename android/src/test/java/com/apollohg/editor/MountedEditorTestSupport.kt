package com.apollohg.editor

import android.app.Activity
import android.view.MotionEvent
import android.view.View
import com.apollohg.editor.viewer.PreparedProseDrawingView
import org.json.JSONObject
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.robolectric.Robolectric
import org.robolectric.RuntimeEnvironment

internal fun measureMountedEditor(view: RichTextEditorView, width: Int, height: Int) {
    view.measure(
        View.MeasureSpec.makeMeasureSpec(width, View.MeasureSpec.EXACTLY),
        View.MeasureSpec.makeMeasureSpec(height, View.MeasureSpec.EXACTLY)
    )
    view.layout(0, 0, width, height)
}

internal fun tapMountedTableCell(view: RichTextEditorView, cellIndex: Int = 0) {
    val canvas = (0 until view.editorContentFrame.childCount)
        .map {
            view.editorContentFrame.getChildAt(it)
        }.filterIsInstance<PreparedProseDrawingView>().single()
    canvas.measure(
        View.MeasureSpec.makeMeasureSpec(view.editorEditText.width, View.MeasureSpec.EXACTLY),
        View.MeasureSpec.makeMeasureSpec(view.editorEditText.height, View.MeasureSpec.EXACTLY)
    )
    canvas.layout(0, 0, canvas.measuredWidth, canvas.measuredHeight)
    val block = requireNotNull(canvas.preparedLayout?.blocks?.singleOrNull())
    val surface = requireNotNull(block.tableSurface)
    val cell = surface.cells[cellIndex]
    val bounds = requireNotNull(block.tableBounds)
    val frame = surface.frameOfCell(cell)
    val x = bounds.left + frame.left + cell.contentOrigin.first + CELL_TAP_INSET
    val y = bounds.top + frame.top + cell.contentOrigin.second + CELL_TAP_INSET
    val down = MotionEvent.obtain(0, 0, MotionEvent.ACTION_DOWN, x, y, 0)
    val up = MotionEvent.obtain(0, TAP_DURATION_MS, MotionEvent.ACTION_UP, x, y, 0)
    try {
        assertTrue(view.dispatchTouchEvent(down))
        assertTrue(
            "tap focus=${view.activeTextInput === view.editorEditText} " +
                "rootTrace=${view.editorEditText.imeTraceSnapshotForTesting()}",
            view.dispatchTouchEvent(up)
        )
    } finally {
        down.recycle()
        up.recycle()
    }
}

internal fun withMountedEditorView(
    document: String,
    configJSON: String,
    backend: EditorV2Backend = UniffiEditorV2Backend,
    windowed: Boolean = false,
    block: (RichTextEditorView, EditorV2Adapter, String) -> Unit
) {
    val created = UniffiEditorV2Backend.create(configJSON, null) as EditorV2CallResult.Ok
    val adapter = requireNotNull(
        EditorV2Adapter.attach(backend, JSONObject(created.value).getString("editorId"), false)
    )
    val token = EditorV2Registry.register(adapter)
    val activity = if (windowed) Robolectric.buildActivity(Activity::class.java).setup() else null
    try {
        val update = requireNotNull(adapter.setContentJson(document))
        val view = RichTextEditorView(activity?.get() ?: RuntimeEnvironment.getApplication())
        activity?.get()?.setContentView(view)
        view.editorId = token
        val snapshot = requireNotNull(adapter.cachedViewUpdateJson)
        val applied = view.editorEditText.applyUpdateJSON(snapshot)
        assertEquals(
            "admission=$applied trace=${view.editorEditText.imeTraceSnapshotForTesting()} " +
                "extents=${adapter.tableIndex.rootExtents} scalar=${adapter.cachedScalarLength} " +
                "blocks=${adapter.cachedSemanticRenderBlocks}",
            JSONObject(snapshot).getString("documentVersion"),
            view.editorEditText.lastAppliedDocumentVersion
        )
        measureMountedEditor(view, EDITOR_WIDTH, EDITOR_HEIGHT)
        block(view, adapter, update)
    } finally {
        EditorV2Registry.remove(adapter.editorId)
        adapter.destroy()
        activity?.pause()?.stop()?.destroy()
    }
}

private const val EDITOR_WIDTH = 600
private const val EDITOR_HEIGHT = 500
private const val CELL_TAP_INSET = 8f
private const val TAP_DURATION_MS = 10L
