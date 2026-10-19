package com.apollohg.editor

import android.app.Activity
import android.os.Looper
import android.view.MotionEvent
import android.view.View
import android.view.inputmethod.EditorInfo
import org.json.JSONObject
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.Robolectric
import org.robolectric.RobolectricTestRunner
import org.robolectric.Shadows.shadowOf
import org.robolectric.annotation.Config

@RunWith(RobolectricTestRunner::class)
@Config(sdk = [34])
internal class NativeEditorExpoViewMountTest : NativeEditorExpoViewTestSupport() {
    @Test
    fun `mounting a localEmpty document publishes no native commit until the user types`() {
        assertMountPublishesOnlyTypedCommits(LOCAL_EMPTY_CONFIG)
    }

    @Test
    fun `mounting a localHtml document publishes no native commit until the user types`() {
        assertMountPublishesOnlyTypedCommits(LOCAL_HTML_CONFIG)
    }

    private fun assertMountPublishesOnlyTypedCommits(configJson: String) {
        val activity = Robolectric.buildActivity(Activity::class.java).setup().get()
        val expoContext = testExpoContext(activity)
        val view = NativeEditorExpoView(expoContext.context, expoContext.appContext)
        val adapter = attachAdapterForViewTest(UniffiEditorV2Backend, configJson)
        val token = EditorV2Registry.register(adapter)
        val commits = mutableListOf<Map<String, Any>>()
        try {
            view.onEditorUpdateForTesting = { commits += it }
            view.onSelectionChangeForTesting = {}
            view.onFocusChangeForTesting = {}
            view.onEditorReadyForTesting = {}
            view.onAddonEventForTesting = {}
            view.onContentHeightChangeForTesting = {}
            view.setEditorId(token)
            activity.setContentView(view)
            shadowOf(Looper.getMainLooper()).idle()
            tapEditor(view)

            val editText = view.richTextView.editorEditText
            assertTrue("the tap focuses the editor", editText.hasFocus())
            assertEquals("mounting and focusing are not local edits: $commits", 0, commits.size)
            assertTrue(
                "native toolbar state after focus: ${view.toolbarState.insertableNodes}",
                view.toolbarState.insertableNodes.contains(IMAGE_NODE)
            )

            val mountedRevision = adapter.baseDocumentRevision
            val input = requireNotNull(editText.onCreateInputConnection(EditorInfo()))
            assertTrue(input.commitText(TYPED_TEXT, 1))
            shadowOf(Looper.getMainLooper()).idle()

            val commit = commits.single()
            val snapshot = JSONObject(commit[UPDATE_JSON_KEY] as String)
            assertEquals(
                "typing publishes one admissible commit: $snapshot",
                ATOMIC_RENDER_SNAPSHOT_KEYS,
                snapshot.keys().asSequence().toSet()
            )
            assertTrue(
                "typing advances the document from $mountedRevision: $snapshot",
                adapter.baseDocumentRevision > mountedRevision
            )
            assertEquals(adapter.baseDocumentRevision.toString(), commit[DOCUMENT_REVISION_KEY])
        } finally {
            view.handleDetachedFromWindowForTesting()
            EditorV2Registry.remove(adapter.editorId)
            NativeEditorViewRegistry.unregister(token, view)
            UniffiEditorV2Backend.destroy(adapter.editorId)
            activity.finish()
        }
    }

    private fun tapEditor(view: NativeEditorExpoView) {
        view.measure(
            View.MeasureSpec.makeMeasureSpec(VIEW_SIZE_PX, View.MeasureSpec.EXACTLY),
            View.MeasureSpec.makeMeasureSpec(VIEW_SIZE_PX, View.MeasureSpec.EXACTLY)
        )
        view.layout(0, 0, VIEW_SIZE_PX, VIEW_SIZE_PX)
        val editText = view.richTextView.editorEditText
        val down = MotionEvent.obtain(0, 0, MotionEvent.ACTION_DOWN, TAP_X, TAP_Y, 0)
        val up = MotionEvent.obtain(0, TAP_DURATION_MS, MotionEvent.ACTION_UP, TAP_X, TAP_Y, 0)
        try {
            editText.dispatchTouchEvent(down)
            editText.dispatchTouchEvent(up)
        } finally {
            down.recycle()
            up.recycle()
        }
        shadowOf(Looper.getMainLooper()).idle()
    }

    private companion object {
        const val LOCAL_EMPTY_CONFIG = """{"initialization":{"type":"localEmpty"}}"""
        const val LOCAL_HTML_CONFIG =
            """{"initialization":{"type":"localHtml","html":"<p>mounted</p>"}}"""
        const val UPDATE_JSON_KEY = "updateJson"
        const val DOCUMENT_REVISION_KEY = "documentRevision"
        const val IMAGE_NODE = "image"
        const val TYPED_TEXT = "a"
        const val VIEW_SIZE_PX = 400
        const val TAP_X = 380f
        const val TAP_Y = 10f
        const val TAP_DURATION_MS = 10L
    }
}
