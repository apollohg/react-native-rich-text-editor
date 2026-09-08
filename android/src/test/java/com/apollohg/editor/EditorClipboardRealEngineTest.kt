package com.apollohg.editor

import android.content.ClipData
import android.content.ClipDescription
import android.content.ClipboardManager
import android.content.Context
import android.os.PersistableBundle
import org.json.JSONObject
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.RuntimeEnvironment
import org.robolectric.annotation.Config

@RunWith(RobolectricTestRunner::class)
@Config(sdk = [34])
class EditorClipboardRealEngineTest {
    @Test
    fun `native clipboard round trip preserves marked text through the real engine`() {
        val source = realEditor("<p>Hello <strong>world</strong></p>")
        val destination = realEditor("<p>Start </p>")
        try {
            source.editText.setSelection(6, 11)
            assertTrue(source.editText.onTextContextMenuItem(android.R.id.copy))
            assertTrue(
                clipboard().primaryClip?.description?.hasMimeType(
                    EditorClipboard.MIME_TYPE_FRAGMENT
                )
                    == true
            )

            destination.editText.setSelection(destination.editText.length())
            assertTrue(destination.editText.onTextContextMenuItem(android.R.id.paste))

            val html = requireNotNull(destination.adapter.documentHtml())
            assertTrue(html.contains("<strong>world</strong>"))
        } finally {
            source.adapter.destroy()
            destination.adapter.destroy()
        }
    }

    @Test
    fun `plain text mode strips marks from a real editor fragment`() {
        val source = realEditor("<p><strong>world</strong></p>")
        val destination = realEditor("<p>Start </p>")
        try {
            source.editText.setSelection(0, 5)
            assertTrue(source.editText.onTextContextMenuItem(android.R.id.copy))

            destination.editText.pasteMode = EditorPasteMode.PLAIN_TEXT
            destination.editText.setSelection(destination.editText.length())
            assertTrue(destination.editText.onTextContextMenuItem(android.R.id.paste))

            val html = requireNotNull(destination.adapter.documentHtml())
            assertTrue(html.contains("world"))
            assertTrue(!html.contains("<strong>world</strong>"))
        } finally {
            source.adapter.destroy()
            destination.adapter.destroy()
        }
    }

    @Test
    fun `malformed private fragment falls back to external HTML in the real engine`() {
        val destination = realEditor("<p>Start </p>")
        try {
            val description = ClipDescription(
                "external",
                arrayOf(
                    ClipDescription.MIMETYPE_TEXT_HTML,
                    ClipDescription.MIMETYPE_TEXT_PLAIN,
                    EditorClipboard.MIME_TYPE_FRAGMENT
                )
            )
            description.extras = PersistableBundle().apply {
                putString(EditorClipboard.EXTRA_FRAGMENT, "{")
            }
            clipboard().setPrimaryClip(
                ClipData(
                    description,
                    ClipData.Item("world", "<strong>world</strong>", null, null)
                )
            )
            destination.editText.setSelection(destination.editText.length())

            assertTrue(destination.editText.onTextContextMenuItem(android.R.id.paste))

            assertTrue(
                requireNotNull(destination.adapter.documentHtml())
                    .contains("<strong>world</strong>")
            )
        } finally {
            destination.adapter.destroy()
        }
    }

    @Test
    fun `native clipboard round trip preserves an atom through the real engine`() {
        val source = realEditor(
            "<img src=\"https://example.com/cat.png\" alt=\"Cat\" width=\"320\" height=\"180\">"
        )
        val destination = realEditor("<p>Before</p>")
        try {
            val blocks = requireNotNull(source.editText.currentRenderBlocksJson)
            val atom = (0 until blocks.length())
                .asSequence()
                .mapNotNull { blocks.optJSONArray(it) }
                .flatMap { block ->
                    (0 until block.length()).asSequence().mapNotNull(block::optJSONObject)
                }
                .first { it.optString("type") == "voidBlock" }
            val selectionUpdate = requireNotNull(
                source.adapter.selectAtomNode(atom.getInt("docPos"))
            )
            source.editText.applyUpdateJSON(selectionUpdate, notifyListener = false)
            assertTrue(source.editText.onTextContextMenuItem(android.R.id.copy))
            val fragment = clipboard().primaryClip?.description?.extras
                ?.getString(EditorClipboard.EXTRA_FRAGMENT)
            assertTrue(requireNotNull(fragment).contains("\"type\":\"image\""))

            destination.editText.setSelection(destination.editText.length())
            assertTrue(destination.editText.onTextContextMenuItem(android.R.id.paste))

            val content = JSONObject(requireNotNull(destination.adapter.documentJson()))
                .getJSONArray("content")
            val image = (0 until content.length())
                .asSequence()
                .map(content::getJSONObject)
                .first { it.getString("type") == "image" }
            val attrs = image.getJSONObject("attrs")
            assertEquals("https://example.com/cat.png", attrs.getString("src"))
            assertEquals("Cat", attrs.getString("alt"))
            assertEquals(320, attrs.getInt("width"))
            assertEquals(180, attrs.getInt("height"))
        } finally {
            source.adapter.destroy()
            destination.adapter.destroy()
        }
    }

    private fun realEditor(html: String): RealEditor {
        val created = UniffiEditorV2Backend.create(
            """{"initialization":{"type":"localEmpty"}}""",
            null
        ) as EditorV2CallResult.Ok
        val editorId = JSONObject(created.value).getString("editorId")
        val adapter = requireNotNull(
            EditorV2Adapter.attach(UniffiEditorV2Backend, editorId, roomBound = false)
        )
        val editText = EditorEditText(RuntimeEnvironment.getApplication()).apply {
            this.editorId = editorId.toLong()
            v2Driver = adapter
        }
        val update = requireNotNull(adapter.setContentHtml(html))
        assertTrue(editText.applyUpdateJSON(update, notifyListener = false))
        return RealEditor(adapter, editText)
    }

    private fun clipboard(): ClipboardManager = RuntimeEnvironment.getApplication()
        .getSystemService(Context.CLIPBOARD_SERVICE) as ClipboardManager

    private data class RealEditor(val adapter: EditorV2Adapter, val editText: EditorEditText)
}
