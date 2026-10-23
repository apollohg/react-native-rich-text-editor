package com.apollohg.editor

import org.json.JSONObject
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config
import uniffi.editor_core.editorV2Create
import uniffi.editor_core.editorV2Destroy
import uniffi.editor_core.editorV2GetDocumentIsEmpty
import uniffi.editor_core.editorV2GetDocumentText
import uniffi.editor_core.editorV2GetState
import uniffi.editor_core.editorV2ReplaceDocument

@RunWith(RobolectricTestRunner::class)
@Config(sdk = [34])
internal class DocumentQueriesTest {
    @Test
    fun `queries return current raw text and structural emptiness without changing state`() {
        val created = editorV2Create(
            """{"initialization":{"type":"localHtml","html":"<p>&lt;tag&gt; &amp; 👩🏽‍💻</p><p>tail</p>"}}""",
            null
        )
        assertNull(created.error)
        val editorId = JSONObject(requireNotNull(created.value)).getString("editorId")
        try {
            val before = editorV2GetState(editorId)
            assertNull(before.error)
            val text = editorV2GetDocumentText(editorId)
            assertNull(text.error)
            assertEquals(
                "<tag> & 👩🏽‍💻tail",
                JSONObject(requireNotNull(text.value)).getString("text")
            )
            val nonempty = editorV2GetDocumentIsEmpty(editorId)
            assertNull(nonempty.error)
            assertEquals(false, JSONObject(requireNotNull(nonempty.value)).getBoolean("isEmpty"))
            assertEquals(before.value, editorV2GetState(editorId).value)
            val replaced = editorV2ReplaceDocument(
                editorId,
                """{"version":1,"requestId":"1","history":"undoableBoundary","setHtml":"<p></p>"}"""
            )
            assertNull(replaced.error)
            val empty = editorV2GetDocumentIsEmpty(editorId)
            assertNull(empty.error)
            assertEquals(true, JSONObject(requireNotNull(empty.value)).getBoolean("isEmpty"))
        } finally {
            assertNull(editorV2Destroy(editorId).error)
        }
    }
}
