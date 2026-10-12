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
import uniffi.editor_core.editorV2GetDocumentMarkdown
import uniffi.editor_core.editorV2GetState
import uniffi.editor_core.editorV2ReplaceDocument

@RunWith(RobolectricTestRunner::class)
@Config(sdk = [34])
internal class MarkdownExportTest {
    @Test
    fun `native binding defaults to CommonMark and exports the current document as GFM`() {
        val created = editorV2Create(
            """{"initialization":{"type":"localHtml","html":"<h2>Title</h2><p><s>Gone</s></p>"}}""",
            null
        )
        assertNull(created.error)
        val editorId = JSONObject(requireNotNull(created.value)).getString("editorId")
        try {
            val before = editorV2GetState(editorId)
            assertNull(before.error)
            val vanilla = editorV2GetDocumentMarkdown(editorId, null)
            assertNull(vanilla.error)
            assertEquals("## Title\n\n<s>Gone</s>", JSONObject(requireNotNull(vanilla.value)).getString("markdown"))
            val gfm = editorV2GetDocumentMarkdown(editorId, "gfm")
            assertNull(gfm.error)
            assertEquals("## Title\n\n~~Gone~~", JSONObject(requireNotNull(gfm.value)).getString("markdown"))
            assertEquals(before.value, editorV2GetState(editorId).value)

            val replaced = editorV2ReplaceDocument(
                editorId,
                """{"version":1,"requestId":"1","history":"undoableBoundary","setHtml":"<p>Changed</p>"}"""
            )
            assertNull(replaced.error)
            val current = editorV2GetDocumentMarkdown(editorId, null)
            assertNull(current.error)
            assertEquals("Changed", JSONObject(requireNotNull(current.value)).getString("markdown"))
        } finally {
            assertNull(editorV2Destroy(editorId).error)
        }
    }
}
