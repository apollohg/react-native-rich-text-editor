package com.apollohg.editor

import org.json.JSONObject

internal const val SELECTION_STATE_ACTIVE_STATE_KEY = "activeState"
internal const val SELECTION_STATE_ACTIVE_STATE_REVISION_KEY = "activeStateRevision"

internal class EditorSelectionState(
    val documentVersion: String,
    val stateRevision: String,
    val selection: JSONObject,
    val activeState: JSONObject,
    val historyState: JSONObject
) {
    fun json(): String = json(SELECTION_STATE_ACTIVE_STATE_KEY, activeState.toString())

    fun json(activeStateKey: String, activeStateValue: String): String = buildString {
        append("{\"documentVersion\":").append(JSONObject.quote(documentVersion))
        append(",\"stateRevision\":").append(JSONObject.quote(stateRevision))
        append(",\"selection\":").append(selection.toString())
        append(',').append(JSONObject.quote(activeStateKey)).append(':').append(activeStateValue)
        append(",\"historyState\":").append(historyState.toString())
        append('}')
    }
}

internal class SelectionEventActiveStateChannel {
    private var emittedActiveState: String? = null
    private var emittedStateRevision: String? = null

    fun stateJson(state: EditorSelectionState): String {
        val activeState = state.activeState.toString()
        val revision = emittedStateRevision
        if (revision != null && activeState == emittedActiveState) {
            return state.json(SELECTION_STATE_ACTIVE_STATE_REVISION_KEY, JSONObject.quote(revision))
        }
        emittedActiveState = activeState
        emittedStateRevision = state.stateRevision
        return state.json(SELECTION_STATE_ACTIVE_STATE_KEY, activeState)
    }

    fun reset() {
        emittedActiveState = null
        emittedStateRevision = null
    }
}
