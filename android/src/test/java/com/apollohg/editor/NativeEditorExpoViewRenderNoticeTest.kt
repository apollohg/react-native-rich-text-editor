package com.apollohg.editor
import android.os.Looper
import org.json.JSONObject
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.RuntimeEnvironment
import org.robolectric.Shadows.shadowOf
import org.robolectric.annotation.Config

@RunWith(RobolectricTestRunner::class)
@Config(sdk = [34])
internal class NativeEditorExpoViewRenderNoticeTest :
    NativeEditorExpoViewControlledUpdateTestFixture() {
    private fun replaceDocumentFromJs(adapter: EditorV2Adapter, html: String, history: String) {
        val request = JSONObject().put("version", 1).put("requestId", "1")
            .put("history", history).put("setHtml", html)
        val replaced = UniffiEditorV2Backend.replaceDocument(adapter.editorId, request.toString())
        assertTrue("JS replacement=$replaced", replaced is EditorV2CallResult.Ok)
    }

    @Test
    fun `notice-only prop adopts the engine render and survives skipped prop redelivery`() {
        withFocusedViewForNativeInput("<p>first</p>") { _, view, adapter, _ ->
            val editText = view.richTextView.editorEditText
            val viewToken = view.richTextView.editorId

            replaceDocumentFromJs(adapter, "<p>notice body</p>", "undoableBoundary")
            val notice = renderNoticeJson(UniffiEditorV2Backend, adapter)
            view.setPendingEditorUpdateJson(notice)
            view.setPendingEditorUpdateEditorId(viewToken)
            view.setPendingEditorUpdateRevision(1)
            view.applyPendingEditorUpdateIfNeeded()
            shadowOf(Looper.getMainLooper()).idle()

            assertEquals("notice body", editText.text.toString())
            assertNull(view.pendingEditorUpdateJsonForTesting())

            replaceDocumentFromJs(adapter, "<p>second</p>", "undoableBoundary")
            val secondNotice = renderNoticeJson(UniffiEditorV2Backend, adapter)
            view.setPendingEditorUpdateJson(secondNotice)
            view.setPendingEditorUpdateRevision(2)
            view.applyPendingEditorUpdateIfNeeded()
            shadowOf(Looper.getMainLooper()).idle()
            assertEquals("second", editText.text.toString())

            view.setPendingEditorUpdateRevision(3)
            assertEquals(secondNotice, view.pendingEditorUpdateJsonForTesting())
            view.applyPendingEditorUpdateIfNeeded()
            shadowOf(Looper.getMainLooper()).idle()

            assertEquals("second", editText.text.toString())
            assertNull(view.pendingEditorUpdateJsonForTesting())
            assertEquals(0, view.pendingEditorUpdateRevisionForTesting())
        }
    }

    @Test
    fun `notice-only reset applies the reset document and echoes a racing native commit`() {
        withFocusedViewForNativeInput("<p>before</p>") { _, view, adapter, payloads ->
            val editText = view.richTextView.editorEditText
            val viewToken = view.richTextView.editorId

            replaceDocumentFromJs(adapter, "<p>after reset</p>", "resetAndClear")
            val notice = renderNoticeJson(UniffiEditorV2Backend, adapter)
            val resetRevision = JSONObject(notice).getString("documentVersion")
            assertTrue(commitBoundText(view, "raced"))
            shadowOf(Looper.getMainLooper()).idle()
            payloads.clear()

            view.setPendingEditorUpdateJson(notice)
            view.pendingEditorUpdateResetJson = JSONObject()
                .put("setHtml", "<p>after reset</p>")
                .put("history", "resetAndClear")
                .put("documentRevision", resetRevision)
                .toString()
            view.setPendingEditorUpdateEditorId(viewToken)
            view.setPendingEditorUpdateRevision(1)
            view.applyPendingEditorUpdateIfNeeded()
            shadowOf(Looper.getMainLooper()).idle()

            assertEquals("after reset", editText.text.toString())
            assertEquals("<p>after reset</p>", adapter.documentHtml())
            val engineRevision = JSONObject(renderNoticeJson(UniffiEditorV2Backend, adapter))
                .getString("documentVersion")
            assertTrue(engineRevision != resetRevision)
            assertEquals(engineRevision, payloads.last()["documentRevision"])
            assertNull(view.pendingEditorUpdateJsonForTesting())
        }
    }

    @Test
    fun `view commands reject a full render on an unbound view without applying it`() {
        val expoContext = testExpoContext(RuntimeEnvironment.getApplication())
        val view = NativeEditorExpoView(expoContext.context, expoContext.appContext)
        val editText = view.richTextView.editorEditText
        view.setAttachedToNativeWindowForTesting(true)
        val textBefore = editText.text.toString()

        assertFalse(view.applyEditorUpdate(atomicRenderUpdateJson("injected", "1")))
        assertFalse(view.applyEditorResetUpdate(atomicRenderUpdateJson("injected reset", "2")))

        assertEquals(0L, view.richTextView.editorId)
        assertEquals(textBefore, editText.text.toString())
        assertNull(view.pendingViewCommandUpdateJsonForTesting())
    }

    @Test
    fun `view commands reject a full render for an unpaired editor token without applying it`() {
        val expoContext = testExpoContext(RuntimeEnvironment.getApplication())
        val view = NativeEditorExpoView(expoContext.context, expoContext.appContext)
        val editText = view.richTextView.editorEditText
        val unpairedToken = 918273L
        view.richTextView.setEditorIdWhileDetached(unpairedToken)
        editText.editorId = unpairedToken
        view.setAttachedToNativeWindowForTesting(true)
        val textBefore = editText.text.toString()

        assertFalse(view.applyEditorUpdate(atomicRenderUpdateJson("injected", "1")))
        assertFalse(view.applyEditorResetUpdate(atomicRenderUpdateJson("injected reset", "2")))

        assertNull(EditorV2Registry.adapterForViewToken(unpairedToken))
        assertEquals(textBefore, editText.text.toString())
        assertNull(view.pendingViewCommandUpdateJsonForTesting())
        NativeEditorViewRegistry.unregister(unpairedToken, view)
    }
}
