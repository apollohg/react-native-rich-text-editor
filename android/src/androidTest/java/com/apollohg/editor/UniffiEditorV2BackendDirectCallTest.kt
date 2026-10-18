package com.apollohg.editor

import androidx.test.ext.junit.runners.AndroidJUnit4
import org.json.JSONObject
import org.junit.After
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Before
import org.junit.Test
import org.junit.runner.RunWith
import uniffi.editor_core.editorV2ApplyNativeIntent
import uniffi.editor_core.editorV2GetDocumentJson
import uniffi.editor_core.editorV2RenderNativeFrame

@RunWith(AndroidJUnit4::class)
internal class UniffiEditorV2BackendDirectCallTest {
    private lateinit var editorId: String

    @Before
    fun createEditor() {
        assertTrue("the device binds the direct editor core entry points", EditorCoreJni.isBound)
        val created = UniffiEditorV2Backend.create(CONFIG_JSON, null) as EditorV2CallResult.Ok
        editorId = JSONObject(created.value).getString("editorId")
    }

    @After
    fun destroyEditor() {
        UniffiEditorV2Backend.destroy(editorId)
    }

    @Test
    fun directFramesMatchTheGeneratedBindingWithAndWithoutAMirror() {
        val direct = UniffiEditorV2Backend.renderNativeFrame(editorId, null, null, null)
        val generated = editorV2RenderNativeFrame(editorId, null, null, null)
        assertEquals(
            "ownerless direct frame",
            EditorV2CallResult.Ok(requireNotNull(generated.frame)),
            direct
        )

        val halfMirror = UniffiEditorV2Backend.renderNativeFrame(editorId, null, 1, null)
        val generatedHalfMirror = editorV2RenderNativeFrame(editorId, null, 1u, null)
        assertEquals(
            "a half-specified mirror reports the core error unchanged",
            requireNotNull(generatedHalfMirror.error).code,
            (halfMirror as EditorV2CallResult.Err).error.code
        )
    }

    @Test
    fun directNativeIntentsRoundTripUnicodeTextAndCoreErrors() {
        val frame = UniffiEditorV2Backend.renderNativeFrame(editorId, OWNER_ID, null, null)
            as EditorV2CallResult.Ok
        assertEquals(
            "the owned frame's snapshot carries only the commit keys",
            ATOMIC_RENDER_SNAPSHOT_KEYS,
            JSONObject(frame.value.snapshotJson).keys().asSequence().toSet()
        )
        val epoch = requireNotNull(frame.value.positionEpoch)
        val request = intentRequest(REQUEST_ID, epoch, INSERTED_TEXT)

        val direct = UniffiEditorV2Backend.applyNativeIntent(editorId, request)
        val duplicate = editorV2ApplyNativeIntent(editorId, request)
        assertEquals(
            "the generated binding's idempotent duplicate matches the direct result",
            EditorV2CallResult.Ok(requireNotNull(duplicate.value)),
            direct
        )
        val document = requireNotNull(editorV2GetDocumentJson(editorId).value)
        assertTrue("the core holds the inserted text: $document", document.contains(INSERTED_TEXT))

        val stale = intentRequest(REQUEST_ID + 1, STALE_EPOCH, INSERTED_TEXT)
        val directError = UniffiEditorV2Backend.applyNativeIntent(editorId, stale)
            as EditorV2CallResult.Err
        val generatedError = requireNotNull(editorV2ApplyNativeIntent(editorId, stale).error)
        assertEquals(generatedError.code, directError.error.code)
        assertEquals(generatedError.message, directError.error.message)
    }

    private fun intentRequest(requestId: Int, epoch: String, text: String): String = JSONObject()
        .put("version", 1)
        .put("requestId", requestId.toString())
        .put("ownerId", OWNER_ID)
        .put("positionEpoch", epoch)
        .put(
            "intent",
            JSONObject().put("type", "insertText").put("anchor", 0).put("head", 0)
                .put("text", text)
        )
        .toString()

    private companion object {
        const val CONFIG_JSON = """{"initialization":{"type":"localHtml","html":"<p>alpha</p>"}}"""
        const val OWNER_ID = "9"
        const val REQUEST_ID = 41
        const val STALE_EPOCH = "18446744073709551615"
        const val INSERTED_TEXT = "😀 日本 "
    }
}
