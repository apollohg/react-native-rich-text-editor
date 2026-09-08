package com.apollohg.editor

import android.content.ClipData
import android.content.ClipboardManager
import android.content.Context
import android.os.PersistableBundle
import android.view.KeyEvent
import android.view.accessibility.AccessibilityNodeInfo
import org.json.JSONObject
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.RuntimeEnvironment
import org.robolectric.annotation.Config

@RunWith(RobolectricTestRunner::class)
@Config(sdk = [34])
class EditorClipboardTest : EditorInputConnectionTestSupport() {
    @Test
    @Config(sdk = [24])
    fun `private clipboard representations round trip on the minimum Android API`() {
        val payload = EditorClipboardPayload(
            fragment = "{\"version\":1}",
            html = "<strong>hello</strong>",
            text = "hello"
        )

        val decoded = EditorClipboard.read(
            EditorClipboard.create(payload),
            RuntimeEnvironment.getApplication()
        )

        assertEquals(payload, decoded)
    }

    @Test
    fun `copy publishes authoritative fragment html and text`() {
        val harness = externalCompositionHarness("Hello world")
        try {
            harness.editText.setSelection(6, 11)
            harness.backend.nextClipboardJson = JSONObject()
                .put("fragment", "{\"version\":1,\"content\":\"world\"}")
                .put("html", "<strong>world</strong>")
                .put("text", "world")
                .toString()
            harness.backend.calls.clear()

            assertTrue(harness.editText.onTextContextMenuItem(android.R.id.copy))

            val clipboard = clipboard()
            val clip = requireNotNull(clipboard.primaryClip)
            assertEquals("world", clip.getItemAt(0).text.toString())
            assertEquals("<strong>world</strong>", clip.getItemAt(0).htmlText)
            assertTrue(clip.description.hasMimeType(EditorClipboard.MIME_TYPE_FRAGMENT))
            assertEquals(
                "{\"version\":1,\"content\":\"world\"}",
                clip.description.extras?.getString(EditorClipboard.EXTRA_FRAGMENT)
            )
            assertTrue(
                harness.backend.calls.indexOf("setSelection") <
                    harness.backend.calls.indexOf("getClipboard")
            )
        } finally {
            harness.adapter.destroy()
        }
    }

    @Test
    fun `empty authoritative copy leaves the existing clipboard unchanged`() {
        val harness = externalCompositionHarness("Hello")
        try {
            harness.editText.setSelection(5)
            clipboard().setPrimaryClip(ClipData.newPlainText("existing", "keep"))
            harness.backend.nextClipboardJson = JSONObject().put("empty", true).toString()

            assertTrue(harness.editText.onTextContextMenuItem(android.R.id.copy))

            assertEquals("keep", clipboard().primaryClip?.getItemAt(0)?.text?.toString())
        } finally {
            harness.adapter.destroy()
        }
    }

    @Test
    fun `plain clipboard text cannot spoof a private fragment`() {
        val fakeFragment = "{\"version\":1,\"content\":\"spoof\"}"
        val clip = ClipData.newPlainText("plain", fakeFragment)

        val payload = EditorClipboard.read(clip, RuntimeEnvironment.getApplication())

        assertNull(payload.fragment)
        assertEquals(fakeFragment, payload.text)
    }

    @Test
    fun `rich paste sends every available representation in one paste command`() {
        val harness = externalCompositionHarness("Hello ")
        try {
            harness.editText.setSelection(6)
            clipboard().setPrimaryClip(
                privateClip(
                    fragment = "{\"version\":1,\"content\":\"world\"}",
                    html = "<strong>world</strong>",
                    text = "world"
                )
            )

            assertTrue(harness.editText.onTextContextMenuItem(android.R.id.paste))

            val command = harness.backend.sessions.getValue(harness.editorId).commands.last()
            assertEquals("paste", command.getString("type"))
            assertEquals(
                "{\"version\":1,\"content\":\"world\"}",
                command.getString("fragment")
            )
            assertEquals("<strong>world</strong>", command.getString("html"))
            assertEquals("world", command.getString("text"))
            assertFalse(command.optBoolean("plainText", false))
            assertEquals(
                "Hello world",
                harness.backend.sessions.getValue(harness.editorId).text.toString()
            )
        } finally {
            harness.adapter.destroy()
        }
    }

    @Test
    fun `empty external clipboard content cannot delete the selection`() {
        val harness = externalCompositionHarness("Hello")
        try {
            harness.editText.setSelection(0, 5)
            clipboard().setPrimaryClip(ClipData.newPlainText("empty", ""))
            harness.backend.calls.clear()

            assertTrue(harness.editText.onTextContextMenuItem(android.R.id.paste))

            assertEquals(
                "Hello",
                harness.backend.sessions.getValue(harness.editorId).text.toString()
            )
            assertFalse(harness.backend.calls.contains("applyCommand"))
        } finally {
            harness.adapter.destroy()
        }
    }

    @Test
    fun `plain text mode ignores rich representations for regular and explicit paste`() {
        val harness = externalCompositionHarness("Hello ")
        try {
            val clip = privateClip(
                fragment = "{\"version\":1,\"content\":\"rich\"}",
                html = "<strong>rich</strong>",
                text = "plain"
            )
            harness.editText.pasteMode = EditorPasteMode.PLAIN_TEXT
            harness.editText.setSelection(6)
            clipboard().setPrimaryClip(clip)

            assertTrue(harness.editText.onTextContextMenuItem(android.R.id.paste))

            var command = harness.backend.sessions.getValue(harness.editorId).commands.last()
            assertEquals("paste", command.getString("type"))
            assertTrue(command.getBoolean("plainText"))
            assertEquals("plain", command.getString("text"))
            assertEquals(
                "{\"version\":1,\"content\":\"rich\"}",
                command.getString("fragment")
            )
            assertEquals("<strong>rich</strong>", command.getString("html"))

            harness.editText.pasteMode = EditorPasteMode.RICH
            harness.editText.setSelection(harness.editText.length())
            clipboard().setPrimaryClip(clip)
            assertTrue(harness.editText.onTextContextMenuItem(android.R.id.pasteAsPlainText))

            command = harness.backend.sessions.getValue(harness.editorId).commands.last()
            assertTrue(command.getBoolean("plainText"))
            assertTrue(command.has("fragment"))
            assertTrue(command.has("html"))
        } finally {
            harness.adapter.destroy()
        }
    }

    @Test
    fun `control shift V requests an explicit plain text paste`() {
        val harness = externalCompositionHarness("Hello ")
        try {
            harness.editText.setSelection(6)
            clipboard().setPrimaryClip(
                privateClip(
                    fragment = "{\"version\":1,\"content\":\"rich\"}",
                    html = "<strong>rich</strong>",
                    text = "plain"
                )
            )

            assertTrue(
                harness.editText.dispatchKeyEvent(
                    KeyEvent(
                        0L,
                        0L,
                        KeyEvent.ACTION_DOWN,
                        KeyEvent.KEYCODE_V,
                        0,
                        KeyEvent.META_CTRL_ON or KeyEvent.META_SHIFT_ON
                    )
                )
            )

            val command = harness.backend.sessions.getValue(harness.editorId).commands.last()
            assertEquals("paste", command.getString("type"))
            assertTrue(command.getBoolean("plainText"))
            assertEquals("plain", command.getString("text"))
        } finally {
            harness.adapter.destroy()
        }
    }

    @Test
    fun `disabled mode gates menu keyboard and accessibility paste`() {
        val harness = externalCompositionHarness("Hello")
        try {
            harness.editText.pasteMode = EditorPasteMode.DISABLED
            harness.editText.setSelection(5)
            clipboard().setPrimaryClip(ClipData.newPlainText("plain", " world"))
            harness.backend.calls.clear()

            assertTrue(harness.editText.onTextContextMenuItem(android.R.id.paste))
            assertTrue(harness.editText.onTextContextMenuItem(android.R.id.pasteAsPlainText))
            assertTrue(
                harness.editText.dispatchKeyEvent(
                    KeyEvent(
                        0L,
                        0L,
                        KeyEvent.ACTION_DOWN,
                        KeyEvent.KEYCODE_V,
                        0,
                        KeyEvent.META_CTRL_ON
                    )
                )
            )
            assertFalse(
                harness.editText.performAccessibilityAction(
                    AccessibilityNodeInfo.ACTION_PASTE,
                    null
                )
            )
            val info = AccessibilityNodeInfo.obtain()
            harness.editText.onInitializeAccessibilityNodeInfo(info)
            assertFalse(info.actionList.any { it.id == AccessibilityNodeInfo.ACTION_PASTE })
            assertFalse(harness.backend.calls.contains("applyCommand"))
        } finally {
            harness.adapter.destroy()
        }
    }

    @Test
    fun `read only view still copies through the authoritative export`() {
        val harness = externalCompositionHarness("Hello world")
        try {
            harness.editText.setSelection(6, 11)
            harness.editText.isEditable = false
            harness.backend.nextClipboardJson = JSONObject()
                .put("fragment", "{\"version\":1,\"content\":\"world\"}")
                .put("html", "<strong>world</strong>")
                .put("text", "world")
                .toString()

            assertTrue(harness.editText.onTextContextMenuItem(android.R.id.copy))

            assertEquals("world", clipboard().primaryClip?.getItemAt(0)?.text?.toString())
            assertTrue(harness.backend.calls.contains("getClipboard"))
        } finally {
            harness.adapter.destroy()
        }
    }

    @Test
    fun `cut writes export before deleting authoritative selection`() {
        val harness = externalCompositionHarness("Hello world")
        try {
            harness.editText.setSelection(6, 11)
            harness.backend.nextClipboardJson = JSONObject()
                .put("fragment", "{\"version\":1,\"content\":\"world\"}")
                .put("html", "<em>world</em>")
                .put("text", "world")
                .toString()
            harness.backend.calls.clear()

            assertTrue(harness.editText.onTextContextMenuItem(android.R.id.cut))

            val calls = harness.backend.calls
            assertTrue(calls.indexOf("getClipboard") < calls.indexOf("applyCommand"))
            assertEquals("world", clipboard().primaryClip?.getItemAt(0)?.text?.toString())
            assertEquals(
                "Hello ",
                harness.backend.sessions.getValue(harness.editorId).text.toString()
            )
        } finally {
            harness.adapter.destroy()
        }
    }

    @Test
    fun `cut does not delete when Android rejects the clipboard payload`() {
        val harness = externalCompositionHarness("Hello world")
        try {
            harness.editText.setSelection(6, 11)
            harness.backend.nextClipboardJson = JSONObject()
                .put("fragment", "{\"version\":1,\"content\":\"world\"}")
                .put("html", "<em>world</em>")
                .put("text", "world")
                .toString()
            harness.editText.onSetPrimaryClipForTesting = {
                throw android.os.TransactionTooLargeException()
            }

            assertTrue(harness.editText.onTextContextMenuItem(android.R.id.cut))

            assertEquals(
                "Hello world",
                harness.backend.sessions.getValue(harness.editorId).text.toString()
            )
            assertEquals(0, harness.backend.sessions.getValue(harness.editorId).commands.size)
        } finally {
            harness.adapter.destroy()
        }
    }

    private fun clipboard(): ClipboardManager = RuntimeEnvironment.getApplication()
        .getSystemService(Context.CLIPBOARD_SERVICE) as ClipboardManager

    private fun privateClip(fragment: String, html: String, text: String): ClipData {
        val description = android.content.ClipDescription(
            "rich text",
            arrayOf(
                android.content.ClipDescription.MIMETYPE_TEXT_HTML,
                android.content.ClipDescription.MIMETYPE_TEXT_PLAIN,
                EditorClipboard.MIME_TYPE_FRAGMENT
            )
        )
        description.extras = PersistableBundle().apply {
            putString(EditorClipboard.EXTRA_FRAGMENT, fragment)
        }
        return ClipData(description, ClipData.Item(text, html, null, null))
    }
}
