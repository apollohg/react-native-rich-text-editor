package com.apollohg.editor

import android.os.Looper
import android.view.View
import android.view.inputmethod.EditorInfo
import org.json.JSONObject
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertSame
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.Shadows.shadowOf
import org.robolectric.annotation.Config

@RunWith(RobolectricTestRunner::class)
@Config(sdk = [34])
internal class NativeEditorExpoViewHiddenToolbarStateTest :
    NativeEditorExpoViewControlledUpdateTestFixture() {
    @Test
    fun `hidden keyboard toolbar shows the latest caret state once it is attached`() {
        withFocusedViewForNativeInput(INITIAL_HTML) { activity, view, _, _ ->
            val content = activity.findViewById<View>(android.R.id.content)
            assertSame(
                "a focused editor attaches its keyboard toolbar",
                content,
                view.keyboardToolbarView.parent
            )
            view.setShowToolbar(false)
            shadowOf(Looper.getMainLooper()).idle()
            assertNull("a hidden keyboard toolbar is detached", view.keyboardToolbarView.parent)

            val input =
                requireNotNull(
                    view.richTextView.editorEditText.onCreateInputConnection(EditorInfo())
                )
            shadowOf(Looper.getMainLooper()).idle()
            val selections = mutableListOf<Map<String, Any>>()
            view.onSelectionChangeForTesting = { selections += it }
            val bold = { requireNotNull(view.keyboardToolbarView.buttonAtForTesting(BOLD_INDEX)) }

            assertTrue(
                "the live connection accepts a caret inside bold text",
                input.setSelection(BOLD_CARET, BOLD_CARET)
            )
            assertEquals("the caret move publishes one selection callback", 1, selections.size)
            assertTrue(
                "the published state reports bold at the caret: ${selections.single()["stateJson"]}",
                JSONObject(selections.single()["stateJson"] as String)
                    .getJSONObject("activeState").getJSONObject("marks").getBoolean("bold")
            )
            assertTrue(
                "the host records the bold caret state",
                view.toolbarState.marks["bold"] == true
            )
            assertFalse("a detached toolbar is not restyled for the bold caret", bold().isSelected)

            view.setShowToolbar(true)
            shadowOf(Looper.getMainLooper()).idle()
            assertSame("showing the toolbar attaches it", content, view.keyboardToolbarView.parent)
            assertTrue("the attached toolbar shows the latest bold caret state", bold().isSelected)

            assertTrue(
                "the live connection accepts a caret in plain text",
                input.setSelection(PLAIN_CARET, PLAIN_CARET)
            )
            assertEquals("the second caret move publishes its callback", 2, selections.size)
            assertFalse("the attached toolbar follows the plain caret", bold().isSelected)
        }
    }

    private companion object {
        private const val BOLD_INDEX = 0
        private const val BOLD_CARET = 4
        private const val PLAIN_CARET = 10
        private const val INITIAL_HTML = "<p>al<strong>pha b</strong>eta gamma</p>"
    }
}
