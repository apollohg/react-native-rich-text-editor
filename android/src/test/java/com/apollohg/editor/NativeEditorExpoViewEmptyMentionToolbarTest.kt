package com.apollohg.editor

import android.os.Looper
import android.view.inputmethod.EditorInfo
import org.json.JSONObject
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertSame
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.Shadows.shadowOf
import org.robolectric.annotation.Config

@RunWith(RobolectricTestRunner::class)
@Config(sdk = [34])
internal class NativeEditorExpoViewEmptyMentionToolbarTest :
    NativeEditorExpoViewControlledUpdateTestFixture() {
    @Test
    fun `real selection and format dispatch retain an empty mention toolbar and publish state`() {
        withFocusedViewForNativeInput(INITIAL_HTML) { _, view, adapter, commits ->
            view.setShowToolbar(false)
            val editor = view.richTextView.editorEditText
            val input = requireNotNull(editor.onCreateInputConnection(EditorInfo()))
            shadowOf(Looper.getMainLooper()).idle()
            commits.clear()
            val selections = mutableListOf<Map<String, Any>>()
            val addonEvents = mutableListOf<JSONObject>()
            view.onSelectionChangeForTesting = { selections += it }
            view.onAddonEventForTesting = { addonEvents += JSONObject(it["eventJson"] as String) }
            val originalBold =
                requireNotNull(view.keyboardToolbarView.buttonAtForTesting(BOLD_INDEX))
            val initialDocumentRevision = adapter.baseDocumentRevision
            val firstStateRevision = adapter.stateRevision

            assertTrue(
                "the live connection accepts the first caret move",
                input.setSelection(FIRST_CARET, FIRST_CARET)
            )
            val secondStateRevision = adapter.stateRevision
            assertTrue(
                "a real caret move advances selection state",
                secondStateRevision > firstStateRevision
            )
            assertTrue(
                "the live connection accepts the second caret move",
                input.setSelection(SECOND_CARET, SECOND_CARET)
            )
            assertTrue(
                "the second move advances selection state",
                adapter.stateRevision > secondStateRevision
            )
            assertEquals(
                "caret moves retain the document revision",
                initialDocumentRevision,
                adapter.baseDocumentRevision
            )
            assertEquals(
                "caret moves retain authoritative content",
                INITIAL_HTML,
                adapter.documentHtml()
            )
            assertEquals("both genuine selection callbacks are delivered", 2, selections.size)
            for ((index, caret) in listOf(FIRST_CARET, SECOND_CARET).withIndex()) {
                val state = JSONObject(selections[index]["stateJson"] as String)
                val selection = state.getJSONObject("selection")
                assertEquals(
                    "callback $index contains the admitted scalar caret",
                    caret,
                    selection.getInt("anchorScalar")
                )
                assertEquals(
                    "callback $index retains a collapsed caret",
                    caret,
                    selection.getInt("headScalar")
                )
                assertFalse(
                    "public selection state never exposes the position epoch",
                    state.has("positionEpoch")
                )
            }

            assertTrue(
                "the same live connection accepts a real formatting range",
                input.setSelection(FORMAT_FROM, FORMAT_TO)
            )
            editor.performToolbarToggleMark("bold")
            shadowOf(Looper.getMainLooper()).idle()
            assertEquals(
                "formatting changes authoritative content",
                FORMATTED_HTML,
                adapter.documentHtml()
            )
            assertTrue(
                "formatting advances the document revision",
                adapter.baseDocumentRevision > initialDocumentRevision
            )
            assertEquals("one genuine format commit is delivered", 1, commits.size)
            val commit = JSONObject(commits.single()["updateJson"] as String)
            assertEquals(
                "the published format commit contains current state",
                adapter.stateRevision.toString(),
                commit.getString("stateRevision")
            )
            assertEquals(
                "the published format commit contains current document revision",
                adapter.baseDocumentRevision.toString(),
                commit.getString("documentVersion")
            )
            assertTrue(
                "the format callback still reports active bold",
                commit.getJSONObject("activeState").getJSONObject("marks").getBoolean("bold")
            )
            assertFalse(
                "format commits never expose the position epoch",
                commit.has("positionEpoch")
            )
            assertTrue(
                "the hidden toolbar host still records the new format state",
                view.toolbarState.marks["bold"] == true
            )
            assertTrue(
                "document changes still publish the inactive mention query",
                addonEvents.any { event ->
                    event.optString("type") == "mentionsQueryChange" &&
                        event.optString("documentVersion") ==
                        adapter.baseDocumentRevision.toString() &&
                        !event.getBoolean("isActive") && event.getString("query").isEmpty()
                }
            )
            assertSame(
                "empty mention refreshes through real selection and format dispatch retain bold",
                originalBold,
                view.keyboardToolbarView.buttonAtForTesting(BOLD_INDEX)
            )
        }
    }

    private companion object {
        private const val BOLD_INDEX = 0
        private const val FIRST_CARET = 2
        private const val SECOND_CARET = 4
        private const val FORMAT_FROM = 2
        private const val FORMAT_TO = 7
        private const val INITIAL_HTML = "<p>alpha beta</p>"
        private const val FORMATTED_HTML = "<p>al<strong>pha b</strong>eta</p>"
    }
}
