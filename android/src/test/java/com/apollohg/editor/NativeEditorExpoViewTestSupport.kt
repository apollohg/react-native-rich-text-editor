package com.apollohg.editor

import android.graphics.Rect
import android.view.MotionEvent
import android.view.View
import android.view.inputmethod.EditorInfo
import com.apollohg.editor.viewer.PreparedProseDrawingView
import org.json.JSONArray
import org.json.JSONObject
import org.junit.Assert.assertTrue

abstract class NativeEditorExpoViewTestSupport {
    protected fun tapCell(view: NativeEditorExpoView, index: Int) {
        val canvas = (0 until view.richTextView.editorContentFrame.childCount)
            .map { view.richTextView.editorContentFrame.getChildAt(it) }
            .filterIsInstance<PreparedProseDrawingView>().single()
        val root = view.richTextView.editorEditText
        canvas.measure(View.MeasureSpec.makeMeasureSpec(root.width, View.MeasureSpec.EXACTLY),
            View.MeasureSpec.makeMeasureSpec(root.height, View.MeasureSpec.EXACTLY))
        canvas.layout(0, 0, canvas.measuredWidth, canvas.measuredHeight)
        val table = requireNotNull(canvas.preparedLayout?.blocks?.singleOrNull())
        val cell = requireNotNull(table.tableSurface?.cells?.get(index))
        val bounds = requireNotNull(table.tableBounds)
        val canvasOrigin = Rect(0, 0, 1, 1)
        view.richTextView.offsetDescendantRectToMyCoords(canvas, canvasOrigin)
        val x = canvasOrigin.left + bounds.left + table.tableSurface!!.frameOfCell(cell).left + cell.contentOrigin.first + 8f
        val y = canvasOrigin.top + bounds.top + table.tableSurface!!.frameOfCell(cell).top + cell.contentOrigin.second + 8f
        val down = MotionEvent.obtain(0, 0, MotionEvent.ACTION_DOWN, x, y, 0)
        val up = MotionEvent.obtain(0, 10, MotionEvent.ACTION_UP, x, y, 0)
        try {
            assertTrue(view.richTextView.dispatchTouchEvent(down))
            val handled = view.richTextView.dispatchTouchEvent(up)
            val adapter = root.v2Driver as? EditorV2Adapter
            assertTrue("root=${root.width}x${root.height} canvas=${canvas.width}x${canvas.height}" +
                " origin=$canvasOrigin tap=$x,$y revision=${adapter?.baseDocumentRevision}" +
                " applied=${root.lastAppliedDocumentVersion} epoch=${adapter?.positionEpoch}" +
                " owns=${adapter?.let(root::ownsNativeBinding)} mappings=${adapter?.cachedTableInputMappings?.tables?.keys}" +
                " rootMap=${root.rootTablePositionMap != null} rootTrace=${root.imeTraceSnapshotForTesting()}",
                handled)
        } finally {
            down.recycle()
            up.recycle()
        }
    }

    protected fun renderUpdateJson(text: String): String = JSONObject()
        .put(
            "renderBlocks",
            JSONArray().put(
                JSONArray()
                    .put(
                        JSONObject()
                            .put("type", "blockStart")
                            .put("nodeType", "paragraph")
                            .put("depth", 0)
                    )
                    .put(
                        JSONObject()
                            .put("type", "textRun")
                            .put("text", text)
                            .put("marks", JSONArray())
                    )
                    .put(JSONObject().put("type", "blockEnd"))
            )
        )
        .put("documentVersion", "1")
        .toString()

    protected fun atomicRenderUpdateJson(text: String, revision: String): String = JSONObject()
        .put(
            "renderBlocks",
            JSONArray().put(
                JSONArray()
                    .put(
                        JSONObject().put(
                            "type",
                            "blockStart"
                        ).put("nodeType", "paragraph").put("depth", 0)
                    )
                    .put(
                        JSONObject().put(
                            "type",
                            "textRun"
                        ).put("text", text).put("marks", JSONArray())
                    )
                    .put(JSONObject().put("type", "blockEnd"))
            )
        )
        .put("renderPatch", JSONObject.NULL)
        .put(
            "selection",
            JSONObject().put(
                "type",
                "text"
            ).put("anchor", 1).put("head", 1).put("anchorScalar", 0).put("headScalar", 0)
        )
        .put(
            "activeState",
            JSONObject()
                .put("marks", JSONObject())
                .put("markAttrs", JSONObject())
                .put("nodes", JSONObject().put("paragraph", true))
                .put("commands", JSONObject())
                .put("allowedMarks", JSONArray().put("bold"))
                .put("insertableNodes", JSONArray().put("hardBreak"))
        )
        .put("historyState", JSONObject().put("canUndo", true).put("canRedo", false))
        .put("documentVersion", revision)
        .put("stateRevision", revision)
        .put("scalarLength", text.length)
        .put("documentIsEmpty", text.isEmpty())
        .toString()

    protected fun commitBoundText(view: NativeEditorExpoView, text: String): Boolean {
        val editText = view.richTextView.editorEditText
        editText.setSelection(editText.selectionStart.coerceAtLeast(0))
        val inputConnection = editText.onCreateInputConnection(EditorInfo()) ?: return false
        return inputConnection.commitText(text, 1)
    }

    internal fun attachAdapterForViewTest(
        backend: FakeEditorV2Backend,
        configJson: String = "{\"initialization\":{\"type\":\"localEmpty\"}}"
    ): EditorV2Adapter {
        val created = backend.create(configJson, null)
            as EditorV2CallResult.Ok
        return EditorV2Adapter.attach(
            backend,
            JSONObject(created.value).getString("editorId"),
            roomBound = false
        )!!
    }
}
