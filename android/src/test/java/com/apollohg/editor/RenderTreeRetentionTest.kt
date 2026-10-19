package com.apollohg.editor

import java.lang.ref.WeakReference
import org.json.JSONArray
import org.json.JSONObject
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.RuntimeEnvironment
import org.robolectric.annotation.Config

@RunWith(RobolectricTestRunner::class)
@Config(sdk = [34])
class RenderTreeRetentionTest {
    @Test
    fun `parsed render elements are released after load and an incremental patch`() {
        val created = UniffiEditorV2Backend.create(
            LOCAL_EMPTY_CONFIG,
            null
        ) as EditorV2CallResult.Ok
        val adapter = requireNotNull(
            EditorV2Adapter.attach(
                UniffiEditorV2Backend,
                JSONObject(created.value).getString("editorId"),
                roomBound = false
            )
        )
        val editText = EditorEditText(RuntimeEnvironment.getApplication()).apply {
            editorId = adapter.editorId.toLong()
            v2Driver = adapter
        }
        try {
            val loadedElement = applyAndObserveTextRun(
                editText,
                requireNotNull(adapter.setContentHtml(DOCUMENT_HTML))
            )
            val patchedElement = applyAndObserveTextRun(
                editText,
                requireNotNull(adapter.insertText(INSERTED_TEXT, INSERTION_SCALAR))
            )

            assertTrue(
                "the second paragraph must be patched incrementally: " +
                    editText.imeTraceSnapshotForTesting(),
                editText.lastRenderAppliedPatch()
            )
            assertEquals(EXPECTED_TEXT, editText.text.toString())
            awaitCollection(loadedElement)
            awaitCollection(patchedElement)
            assertNull("loaded render element is still reachable", loadedElement.get())
            assertNull("patched render element is still reachable", patchedElement.get())
            assertEquals(
                "retained blocks must match a full engine render after the patch",
                JSONObject(requireNotNull(adapter.recoverNativeRender()))
                    .getJSONArray("renderBlocks").toString(),
                ResolvedRenderBlocks.retained(requireNotNull(editText.currentRenderBlocks))
                    .toJsonArray().toString()
            )
        } finally {
            adapter.destroy()
        }
    }

    private fun applyAndObserveTextRun(
        editText: EditorEditText,
        update: String
    ): WeakReference<JSONObject> {
        val parsed = (editText.v2Driver as EditorV2Adapter).readOnlyParsedUpdate(update)
        val blocks = parsed.optJSONArray("renderBlocks")
            ?: parsed.getJSONObject("renderPatch").getJSONArray("renderBlocks")
        val element = WeakReference(firstTextRun(blocks))
        assertTrue(editText.applyUpdateJSON(update, notifyListener = false))
        return element
    }

    private fun firstTextRun(blocks: JSONArray): JSONObject = (0 until blocks.length())
        .asSequence()
        .map(blocks::getJSONArray)
        .flatMap { block -> (0 until block.length()).asSequence().map(block::getJSONObject) }
        .first { it.getString("type") == TEXT_RUN_TYPE }

    private fun awaitCollection(reference: WeakReference<*>) {
        repeat(COLLECTION_ATTEMPTS) {
            if (reference.get() == null) return
            System.gc()
            System.runFinalization()
        }
    }

    private companion object {
        const val LOCAL_EMPTY_CONFIG = """{"initialization":{"type":"localEmpty"}}"""
        const val DOCUMENT_HTML = "<p>alpha</p><p>beta <strong>bold</strong> 😀</p>"
        const val INSERTED_TEXT = "X"
        const val INSERTION_SCALAR = 7
        const val EXPECTED_TEXT = "alpha\nbXeta bold 😀"
        const val TEXT_RUN_TYPE = "textRun"
        const val COLLECTION_ATTEMPTS = 20
    }
}
