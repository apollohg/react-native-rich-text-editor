package com.apollohg.editor

import android.view.KeyEvent
import org.json.JSONObject
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config

@RunWith(RobolectricTestRunner::class)
@Config(sdk = [34])
internal class EditorEditTextHardwareListTabTest : EditorInputConnectionTestFixture() {
    @Test
    fun `hardware Tab indents a prose list item and Shift Tab outdents it`() {
        val harness = structuredDeleteHarness(
            "<ul><li><p>First</p></li><li><p>Second</p></li></ul>"
        )
        try {
            val input = harness.editText
            input.setSelection(input.text.toString().indexOf("Second") + 2)
            assertTrue(input.dispatchKeyEvent(KeyEvent(401L, 401L,
                KeyEvent.ACTION_DOWN, KeyEvent.KEYCODE_TAB, 0)))

            val indented = JSONObject(requireNotNull(harness.adapter.documentJson()))
                .getJSONArray("content").getJSONObject(0).getJSONArray("content")
            assertEquals(1, indented.length())
            val nestedList = indented.getJSONObject(0).getJSONArray("content").getJSONObject(1)
            assertEquals("bullet_list", nestedList.getString("type"))
            assertEquals("Second", nestedList.getJSONArray("content").getJSONObject(0)
                .getJSONArray("content").getJSONObject(0).getJSONArray("content")
                .getJSONObject(0).getString("text"))

            input.setSelection(input.text.toString().indexOf("Second") + 2)
            assertTrue(input.dispatchKeyEvent(KeyEvent(402L, 402L,
                KeyEvent.ACTION_DOWN, KeyEvent.KEYCODE_TAB, 0, KeyEvent.META_SHIFT_ON)))

            val outdented = JSONObject(requireNotNull(harness.adapter.documentJson()))
                .getJSONArray("content").getJSONObject(0).getJSONArray("content")
            assertEquals(2, outdented.length())
            assertEquals("First", outdented.getJSONObject(0).getJSONArray("content")
                .getJSONObject(0).getJSONArray("content").getJSONObject(0).getString("text"))
            assertEquals("Second", outdented.getJSONObject(1).getJSONArray("content")
                .getJSONObject(0).getJSONArray("content").getJSONObject(0).getString("text"))
        } finally {
            harness.adapter.destroy()
        }
    }
}
