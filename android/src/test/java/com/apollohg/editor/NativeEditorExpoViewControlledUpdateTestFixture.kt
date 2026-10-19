package com.apollohg.editor
import android.app.Activity
import android.os.Handler
import android.os.Looper
import android.view.inputmethod.EditorInfo
import java.time.Duration
import java.util.concurrent.CountDownLatch
import java.util.concurrent.atomic.AtomicBoolean
import java.util.concurrent.atomic.AtomicReference
import org.json.JSONObject
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.Robolectric
import org.robolectric.RobolectricTestRunner
import org.robolectric.RuntimeEnvironment
import org.robolectric.Shadows.shadowOf
import org.robolectric.annotation.Config

internal abstract class NativeEditorExpoViewControlledUpdateTestFixture :
    NativeEditorExpoViewTestSupport() {
    protected fun withFocusedViewForNativeInput(
        initialHtml: String? = null,
        block: (
            Activity,
            NativeEditorExpoView,
            EditorV2Adapter,
            MutableList<Map<String, Any>>
        ) -> Unit
    ) {
        val activity = Robolectric.buildActivity(Activity::class.java).setup().get()
        val expoContext = testExpoContext(activity)
        val view = NativeEditorExpoView(expoContext.context, expoContext.appContext)
        val backend = UniffiEditorV2Backend
        val adapter = if (initialHtml == null) {
            attachAdapterForViewTest(backend)
        } else {
            attachAdapterForViewTest(
                backend,
                JSONObject().put(
                    "initialization",
                    JSONObject().put("type", "localHtml").put("html", initialHtml)
                ).toString()
            )
        }
        val token = EditorV2Registry.register(adapter)
        val payloads = mutableListOf<Map<String, Any>>()
        try {
            bindFocusedViewForTypingTest(activity, view, token, payloads)
            block(activity, view, adapter, payloads)
        } finally {
            view.handleDetachedFromWindowForTesting()
            EditorV2Registry.remove(adapter.editorId)
            NativeEditorViewRegistry.unregister(token, view)
            assertNull(backend.destroy(adapter.editorId))
            activity.finish()
        }
    }

    protected fun bindFocusedViewForTypingTest(
        activity: Activity,
        view: NativeEditorExpoView,
        viewToken: Long,
        payloads: MutableList<Map<String, Any>>
    ) {
        view.onEditorUpdateForTesting = { payloads += it }
        view.onAddonEventForTesting = {}
        view.onEditorReadyForTesting = {}
        view.onSelectionChangeForTesting = {}
        view.onFocusChangeForTesting = {}
        view.onContentHeightChangeForTesting = {}
        activity.setContentView(view)
        view.setAttachedToNativeWindowForTesting(true)
        view.setEditorId(viewToken)
        assertTrue(view.richTextView.editorEditText.requestFocus())
    }

    protected fun NativeEditorExpoView.imeTraceSnapshotForTypingTest(): List<String> =
        richTextView.editorEditText.imeTraceSnapshotForTesting()
}
