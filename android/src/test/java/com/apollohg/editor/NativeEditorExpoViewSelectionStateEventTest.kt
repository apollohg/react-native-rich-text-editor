package com.apollohg.editor

import android.os.Looper
import android.view.inputmethod.EditorInfo
import android.view.inputmethod.InputConnection
import org.json.JSONArray
import org.json.JSONObject
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.Shadows.shadowOf
import org.robolectric.annotation.Config

@RunWith(RobolectricTestRunner::class)
@Config(sdk = [34])
internal class NativeEditorExpoViewSelectionStateEventTest :
    NativeEditorExpoViewControlledUpdateTestFixture() {
    @Test
    fun `selection events omit an unchanged active state until it changes`() {
        withFocusedViewForNativeInput(INITIAL_HTML) { _, view, _, _ ->
            val selections = recordSelections(view)
            val input = connection(view)

            val first = moveCaret(input, selections, PLAIN_CARET)
            val unchanged = moveCaret(input, selections, NEXT_PLAIN_CARET)
            val bold = moveCaret(input, selections, BOLD_CARET)
            val plainAgain = moveCaret(input, selections, PLAIN_CARET)
            val repeated = moveCaret(input, selections, NEXT_PLAIN_CARET)

            assertTrue("the first event carries the active state: $first", first.has(ACTIVE_STATE))
            assertFalse(
                "an unchanged active state is omitted: $unchanged",
                unchanged.has(ACTIVE_STATE)
            )
            assertEquals(
                "the omission names the revision that carried the active state",
                first.getString(STATE_REVISION),
                unchanged.getString(ACTIVE_STATE_REVISION)
            )
            assertTrue("a changed active state is sent again: $bold", bold.has(ACTIVE_STATE))
            assertTrue(bold.getJSONObject(ACTIVE_STATE).getJSONObject("marks").getBoolean("bold"))
            assertTrue(
                "leaving bold sends the plain state: $plainAgain",
                plainAgain.has(ACTIVE_STATE)
            )
            assertFalse(repeated.has(ACTIVE_STATE))
            assertEquals(
                plainAgain.getString(STATE_REVISION),
                repeated.getString(ACTIVE_STATE_REVISION)
            )
            assertFalse(
                "the toolbar follows the final plain caret",
                view.toolbarState.marks["bold"] == true
            )
        }
    }

    @Test
    fun `document replacement from JS and rebinding resend the active state`() {
        withFocusedViewForNativeInput(INITIAL_HTML) { _, view, adapter, _ ->
            val selections = recordSelections(view)
            moveCaret(connection(view), selections, PLAIN_CARET)
            assertFalse(moveCaret(connection(view), selections, NEXT_PLAIN_CARET).has(ACTIVE_STATE))

            val commit = adapter.callWithEnvelope(
                JSONObject().put("setHtml", REPLACED_HTML).put("history", "undoableBoundary"),
                includeBaseRevision = false
            ) { UniffiEditorV2Backend.replaceDocument(adapter.editorId, it) }
            val revision =
                JSONObject((commit as EditorV2CallResult.Ok).value).getString("documentRevision")
            assertTrue(view.applyEditorUpdate("{\"documentVersion\":\"$revision\"}"))
            shadowOf(Looper.getMainLooper()).idle()
            val afterReplace = moveCaret(connection(view), selections, PLAIN_CARET)
            assertTrue(
                "a JS document replacement resets the channel",
                afterReplace.has(ACTIVE_STATE)
            )

            val token = view.richTextView.editorId
            view.setEditorId(0L)
            view.setEditorId(token)
            shadowOf(Looper.getMainLooper()).idle()
            val afterRebind = moveCaret(connection(view), selections, NEXT_PLAIN_CARET)
            assertTrue("rebinding resets the channel", afterRebind.has(ACTIVE_STATE))
        }
    }

    @Test
    fun `dispatching the event first leaves toolbar mention and remote selection state settled`() {
        withFocusedViewForNativeInput(MENTION_HTML) { _, view, adapter, _ ->
            view.onAddonEventForTesting = {}
            view.setAddonsJson(MENTION_ADDONS_JSON)
            view.setRemoteSelectionsJson(REMOTE_SELECTIONS_JSON)
            val selections = recordSelections(view)
            val input = connection(view)
            moveCaret(input, selections, MENTION_PLAIN_CARET)
            val toolbarsAtDispatch = mutableListOf<NativeToolbarState>()
            view.onSelectionChangeForTesting = {
                selections += it
                toolbarsAtDispatch += view.toolbarState
            }

            val bold = moveCaret(input, selections, MENTION_BOLD_CARET)

            assertEquals(
                "the event carries the selection state the adapter reports",
                JSONObject(requireNotNull(adapter.currentSelectionStateJson())).toString(),
                bold.toString()
            )
            assertFalse(
                "the toolbar is presented after the event: $toolbarsAtDispatch",
                toolbarsAtDispatch.single().marks["bold"] == true
            )
            assertTrue(view.toolbarState.marks["bold"] == true)
            assertSettled(view)

            moveCaret(input, selections, MENTION_QUERY_CARET)

            assertEquals(MENTION_QUERY, view.mentionQueryState?.query)
            assertTrue(view.keyboardToolbarView.isShowingMentionSuggestions)
            assertSettled(view)
        }
    }

    private fun assertSettled(view: NativeEditorExpoView) {
        val afterMove = settledState(view)
        view.refreshToolbarStateFromEditorSelection()
        view.refreshMentionQuery()
        view.richTextView.refreshRemoteSelections()
        assertEquals("selection side effects match a fresh refresh", settledState(view), afterMove)
        assertTrue(
            "the remote peer stays decorated: $afterMove",
            afterMove.remoteSelections.isNotEmpty()
        )
    }

    private data class SelectionSideEffects(
        val toolbar: NativeToolbarState,
        val mentionQuery: MentionQueryState?,
        val showsMentionSuggestions: Boolean,
        val remoteSelections: List<RemoteSelectionDebugSnapshot>
    )

    private fun settledState(view: NativeEditorExpoView) = SelectionSideEffects(
        view.toolbarState,
        view.mentionQueryState,
        view.keyboardToolbarView.isShowingMentionSuggestions,
        view.richTextView.remoteSelectionDebugSnapshotsForTesting()
    )

    private fun recordSelections(view: NativeEditorExpoView): MutableList<Map<String, Any>> {
        val selections = mutableListOf<Map<String, Any>>()
        view.onSelectionChangeForTesting = { selections += it }
        return selections
    }

    private fun connection(view: NativeEditorExpoView): InputConnection = requireNotNull(
        view.richTextView.editorEditText.onCreateInputConnection(EditorInfo())
    ).also {
        shadowOf(Looper.getMainLooper()).idle()
    }

    private fun moveCaret(
        input: InputConnection,
        selections: List<Map<String, Any>>,
        caret: Int
    ): JSONObject {
        val before = selections.size
        assertTrue(input.setSelection(caret, caret))
        assertEquals("caret $caret publishes one selection event", before + 1, selections.size)
        return JSONObject(selections.last()["stateJson"] as String)
    }

    private companion object {
        const val ACTIVE_STATE = "activeState"
        const val ACTIVE_STATE_REVISION = "activeStateRevision"
        const val STATE_REVISION = "stateRevision"
        const val BOLD_CARET = 4
        const val PLAIN_CARET = 10
        const val NEXT_PLAIN_CARET = 12
        const val INITIAL_HTML = "<p>al<strong>pha b</strong>eta gamma</p>"
        const val REPLACED_HTML = "<p>replaced paragraph text</p>"
        const val MENTION_QUERY = "al"
        const val MENTION_HTML = "<p><strong>bold</strong> @$MENTION_QUERY</p>"
        const val MENTION_BOLD_CARET = 2
        const val MENTION_PLAIN_CARET = 5
        const val MENTION_QUERY_CARET = 8
        const val REMOTE_PEER_POSITION = 3
        val MENTION_ADDONS_JSON = JSONObject().put(
            "mentions",
            JSONObject().put(
                "suggestions",
                JSONArray().put(
                    JSONObject().put("key", "u1").put("title", "Alice").put("label", "@Alice")
                        .put("attrs", JSONObject().put("id", "u1"))
                )
            )
        ).toString()
        val REMOTE_SELECTIONS_JSON = JSONArray().put(
            JSONObject().put("clientId", "7").put("anchor", REMOTE_PEER_POSITION)
                .put("head", REMOTE_PEER_POSITION).put("color", "#ff6b35").put("name", "Alice")
                .put("isFocused", true)
        ).toString()
    }
}
