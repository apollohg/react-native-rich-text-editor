package com.apollohg.editor

import com.apollohg.editor.NativeEditorExpoView.ActiveExternalTextComposition
import org.json.JSONObject

internal fun NativeEditorExpoView.beginExternalTextCompositionImpl(sessionId: String): String {
    val input = richTextView.activeTextInput
    val resultJson = input.beginExternalTextComposition(sessionId)
    val started = runCatching {
        val result = JSONObject(resultJson)
        result.optString("type") == "active" && result.opt("sessionId") == sessionId
    }.getOrDefault(false)
    if (started) {
        activeExternalTextComposition = ActiveExternalTextComposition(
            sessionId = sessionId,
            editorId = eventEditorId(richTextView.editorId),
            input = input
        )
    }
    return resultJson
}

internal fun NativeEditorExpoView.updateExternalTextCompositionImpl(
    sessionId: String,
    text: String
): String = externalCompositionInput(sessionId).updateExternalTextComposition(sessionId, text)

internal fun NativeEditorExpoView.commitExternalTextCompositionImpl(
    sessionId: String,
    finalText: String
): String = externalCompositionInput(sessionId).commitExternalTextComposition(sessionId, finalText)

internal fun NativeEditorExpoView.cancelExternalTextCompositionImpl(
    sessionId: String,
    cause: String
): String = externalCompositionInput(sessionId).cancelExternalTextComposition(sessionId, cause)

private fun NativeEditorExpoView.externalCompositionInput(sessionId: String): EditorEditText =
    activeExternalTextComposition?.takeIf { it.sessionId == sessionId }?.input
        ?: richTextView.activeTextInput

internal fun NativeEditorExpoView.cancelActiveExternalTextComposition(cause: String) {
    val composition = activeExternalTextComposition ?: return
    composition.input.cancelExternalTextComposition(composition.sessionId, cause)
}
