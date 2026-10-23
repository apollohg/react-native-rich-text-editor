package com.apollohg.editor

import org.json.JSONObject
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.RuntimeEnvironment
import org.robolectric.annotation.Config

@RunWith(RobolectricTestRunner::class)
@Config(sdk = [34])
internal class ParagraphNodeSelectionRenderingTest : EditorV2AdapterTestFixture() {
    @Test
    fun `node endpoints accept valid ranges and reject reversed or invalid ranges`() {
        val adapter = makeAdapter()
        assertNotNull(adapter.setContentHtml("<p>example</p>"))
        val snapshot = JSONObject(requireNotNull(adapter.cachedAtomicRenderJson))
        for ((start, end) in listOf(0 to 0, 0 to 4, 3 to 7)) {
            snapshot.put(
                "selection",
                JSONObject().put("type", "node").put("pos", 0)
                    .put("posScalar", start).put("endScalar", end)
            )
            assertNotNull("node range=$start..$end", parseAtomicRenderSnapshot(snapshot.toString()))
        }
        for (end in listOf(-1, 2, "4", JSONObject.NULL)) {
            snapshot.put(
                "selection",
                JSONObject().put("type", "node").put("pos", 0)
                    .put("posScalar", 3).put("endScalar", end)
            )
            org.junit.Assert.assertNull(
                "Invalid endpoint=$end",
                parseAtomicRenderSnapshot(snapshot.toString())
            )
        }
    }

    @Test
    fun `node endpoint selects whole displayed text block without adjacent content`() {
        val adapter = makeAdapter()
        val token = EditorV2Registry.register(adapter)
        val input = EditorEditText(RuntimeEnvironment.getApplication())
        try {
            input.editorId = token
            input.v2Driver = adapter
            for ((text, start, end) in listOf(
                Triple("a😀bc\nnext", 0, 4),
                Triple("• word\nnext", 2, 6),
                Triple("\nnext", 0, 0),
                Triple("", 0, 0)
            )) {
                input.isApplyingRustState = true
                input.setText(text)
                input.isApplyingRustState = false
                input.lastAuthorizedText = text
                val selection = JSONObject().put("type", "node").put("pos", 0)
                    .put("posScalar", start).put("endScalar", end)
                input.applySelectionFromJSON(selection, adapter.baseDocumentRevision.toString())
                val from = PositionBridge.scalarToUtf16(start, text)
                val to = PositionBridge.scalarToUtf16(end, text)
                assertEquals("start text=$text", from, input.selectionStart)
                assertEquals("end text=$text", to, input.selectionEnd)
                assertNotNull(input.authoritativeNodeSelectionRange)
                assertEquals(ImageSelectionRange(from, to), input.authoritativeNodeSelectionRange)
                backend.calls.clear()
                input.syncCurrentSelectionToRust()
                assertTrue(
                    "Rendered node selection must stay authoritative",
                    backend.calls.isEmpty()
                )
            }
            input.isApplyingRustState = true
            input.setText("\uFFFCnext")
            input.isApplyingRustState = false
            input.applySelectionFromJSON(
                JSONObject().put("type", "node").put("pos", 0)
                    .put("posScalar", 0),
                adapter.baseDocumentRevision.toString()
            )
            assertEquals(
                "Legacy atom snapshots keep one replacement character",
                1,
                input.selectionEnd
            )
        } finally {
            input.v2Driver = null
            EditorV2Registry.remove(adapter.editorId)
            adapter.destroy()
        }
    }
}
