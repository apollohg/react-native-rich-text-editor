package com.apollohg.editor

import android.content.ClipData
import android.content.ClipDescription
import android.content.ClipboardManager
import android.content.Context
import android.os.PersistableBundle
import org.json.JSONObject

internal enum class EditorPasteMode {
    RICH,
    PLAIN_TEXT,
    DISABLED;

    companion object {
        fun fromRaw(value: String?): EditorPasteMode = when (value) {
            "plainText" -> PLAIN_TEXT
            "disabled" -> DISABLED
            else -> RICH
        }
    }
}

internal data class EditorClipboardPayload(
    val fragment: String? = null,
    val html: String? = null,
    val text: String? = null
)

internal object EditorClipboard {
    const val MIME_TYPE_FRAGMENT = "application/vnd.apollohg.native-editor.fragment+json"
    const val EXTRA_FRAGMENT = "com.apollohg.editor.clipboard.FRAGMENT"

    fun fromExportJson(json: String): EditorClipboardPayload? = try {
        val value = JSONObject(json)
        if (value.optBoolean("empty", false)) {
            null
        } else {
            val fragment = value.optString("fragment").takeIf { it.isNotEmpty() }
            val html = value.optString("html").takeIf { it.isNotEmpty() }
            val text = value.optString("text").takeIf { it.isNotEmpty() }
            if (fragment == null && html == null && text == null) {
                null
            } else {
                EditorClipboardPayload(fragment, html, text)
            }
        }
    } catch (_: Exception) {
        null
    }

    fun create(payload: EditorClipboardPayload): ClipData {
        val mimeTypes = buildList {
            if (payload.html != null) add(ClipDescription.MIMETYPE_TEXT_HTML)
            add(ClipDescription.MIMETYPE_TEXT_PLAIN)
            if (payload.fragment != null) add(MIME_TYPE_FRAGMENT)
        }.distinct().toTypedArray()
        val description = ClipDescription("rich text", mimeTypes)
        payload.fragment?.let { fragment ->
            description.extras = PersistableBundle().apply {
                putString(EXTRA_FRAGMENT, fragment)
            }
        }
        return ClipData(
            description,
            ClipData.Item(payload.text ?: "", payload.html, null, null)
        )
    }

    fun read(clip: ClipData, context: Context): EditorClipboardPayload {
        if (clip.itemCount == 0) return EditorClipboardPayload()
        val item = clip.getItemAt(0)
        val hasPrivateMime = (0 until clip.description.mimeTypeCount).any {
            clip.description.getMimeType(it) == MIME_TYPE_FRAGMENT
        }
        val fragment = if (hasPrivateMime) {
            runCatching {
                clip.description.extras?.getString(EXTRA_FRAGMENT)
            }.getOrNull()
        } else {
            null
        }
        return EditorClipboardPayload(
            fragment = fragment,
            html = item.htmlText,
            text = item.text?.toString() ?: runCatching {
                item.coerceToText(context)?.toString()
            }.getOrNull()
        )
    }
}

internal fun EditorEditText.isMutatingContextMenuItem(id: Int): Boolean =
    id == android.R.id.paste ||
        id == android.R.id.pasteAsPlainText ||
        id == android.R.id.cut

internal fun EditorEditText.prepareForExternalInteractionMutation(): Boolean =
    commitExternalTextCompositionBeforeInteractionIfNeeded() &&
        prepareForExternalEditorUpdate()

internal fun EditorEditText.publishClipboard(payload: EditorClipboardPayload): Boolean = try {
    val clip = EditorClipboard.create(payload)
    val testWriter = onSetPrimaryClipForTesting
    if (testWriter != null) {
        testWriter(clip)
    } else {
        val clipboard = context.getSystemService(Context.CLIPBOARD_SERVICE) as? ClipboardManager
            ?: return false
        clipboard.setPrimaryClip(clip)
    }
    true
} catch (_: Exception) {
    false
}

internal fun EditorEditText.handleCopy(): Boolean {
    if (editorId == 0L || v2Driver == null) return baseTextContextMenuItem(android.R.id.copy)
    if (discardTransientInputForDestroyedEditorIfNeeded()) return false
    if (!prepareForExternalInteractionMutation()) return false
    syncCurrentSelectionToRust()
    val payload = v2Driver?.clipboardJson()?.let(EditorClipboard::fromExportJson) ?: return false
    return publishClipboard(payload)
}

internal fun EditorEditText.handlePaste(plainTextOnly: Boolean) {
    val forcePlainText = plainTextOnly || pasteMode == EditorPasteMode.PLAIN_TEXT
    if (editorId == 0L) {
        baseTextContextMenuItem(
            if (forcePlainText) android.R.id.pasteAsPlainText else android.R.id.paste
        )
        return
    }
    if (discardTransientInputForDestroyedEditorIfNeeded()) return
    if (!prepareForExternalInteractionMutation()) return

    val clipboard = context.getSystemService(Context.CLIPBOARD_SERVICE) as? ClipboardManager
        ?: return
    val clip = runCatching { clipboard.primaryClip }.getOrNull() ?: return
    val payload = EditorClipboard.read(clip, context)
    if (payload.fragment == null && payload.html == null && payload.text.isNullOrEmpty()) return

    if (payload.fragment == null && payload.html == null &&
        (onInsertTextInRustForTesting != null || onReplaceTextInRustForTesting != null)
    ) {
        payload.text?.let(::pastePlainText)
        return
    }
    val driver = v2Driver
    if (driver == null) {
        if (forcePlainText) {
            payload.text?.let(::pastePlainText)
        } else if (payload.html != null) {
            pasteHTML(payload.html)
        } else {
            payload.text?.let(::pastePlainText)
        }
        return
    }
    val (anchor, head) = currentScalarSelection() ?: return
    val preserveEngineSelection = authoritativeNodeSelectionRange?.let { range ->
        selectionStart == range.start && selectionEnd == range.end
    } == true
    driver.pasteAtSelection(
        fragment = payload.fragment,
        html = payload.html,
        text = payload.text,
        plainText = forcePlainText,
        anchor = anchor,
        head = head,
        preserveEngineSelection = preserveEngineSelection
    )?.let { applyUpdateJSON(it) }
}

internal fun EditorEditText.handleCut() {
    if (editorId == 0L) {
        baseTextContextMenuItem(android.R.id.cut)
        return
    }
    if (discardTransientInputForDestroyedEditorIfNeeded()) return
    if (!prepareForExternalInteractionMutation()) return
    if (v2Driver == null) {
        val currentText = text?.toString() ?: return
        val (selectionStart, selectionEnd) = normalizedUtf16SelectionRange(currentText) ?: return
        val (utf16Start, utf16End) = PositionBridge.snapRangeToScalarBoundaries(
            selectionStart,
            selectionEnd,
            currentText
        )
        if (utf16Start >= utf16End) return
        val selectedText = currentText.substring(utf16Start, utf16End)
        if (!publishClipboard(EditorClipboardPayload(text = selectedText))) return
        deleteRangeInRust(
            PositionBridge.utf16ToScalar(utf16Start, currentText),
            PositionBridge.utf16ToScalar(utf16End, currentText)
        )
        return
    }
    syncCurrentSelectionToRust()

    val payload = v2Driver?.clipboardJson()?.let(EditorClipboard::fromExportJson) ?: return
    if (!publishClipboard(payload)) return

    val (anchor, head) = currentScalarSelection() ?: return
    if (onDeleteRangeInRustForTesting != null) {
        deleteRangeInRust(minOf(anchor, head), maxOf(anchor, head))
        return
    }
    val preserveEngineSelection = authoritativeNodeSelectionRange?.let { range ->
        selectionStart == range.start && selectionEnd == range.end
    } == true
    v2Driver?.pasteAtSelection(
        fragment = null,
        html = null,
        text = "",
        plainText = true,
        anchor = anchor,
        head = head,
        preserveEngineSelection = preserveEngineSelection
    )?.let { applyUpdateJSON(it) }
}

internal fun EditorEditText.handleAccessibilitySetText(arguments: android.os.Bundle?): Boolean {
    val replacement = arguments
        ?.getCharSequence(
            android.view.accessibility.AccessibilityNodeInfo.ACTION_ARGUMENT_SET_TEXT_CHARSEQUENCE
        )
        ?.toString()
        ?: return false
    if (editorId == 0L) {
        return baseAccessibilityAction(
            android.view.accessibility.AccessibilityNodeInfo.ACTION_SET_TEXT,
            arguments
        )
    }
    if (discardTransientInputForDestroyedEditorIfNeeded()) return false
    if (!prepareForExternalInteractionMutation()) return false

    val currentText = text?.toString() ?: return false
    val scalarStart = 0
    val scalarEnd = currentText.codePointCount(0, currentText.length)
    insertPlainTextRangeInRust(scalarStart, scalarEnd, replacement)
    return true
}
