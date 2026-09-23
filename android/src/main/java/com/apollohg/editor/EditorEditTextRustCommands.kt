package com.apollohg.editor

import android.text.Spanned
import org.json.JSONObject

/**
 * Insert text at a scalar position via the Rust editor.
 */
internal fun EditorEditText.insertTextInRust(text: String, atScalarPos: Int) {
    if (!hasLiveEditor()) return
    val globalPos = inputScalar(atScalarPos) ?: return
    onInsertTextInRustForTesting?.let { callback ->
        callback(text, globalPos)
        return
    }
    if (!canDispatchTableCellMutation()) return
    v2Driver?.let { driver ->
        driver.insertText(text, globalPos)?.let { applyRustUpdateJSON(it) }
    }
}

internal fun EditorEditText.replaceTextRangeInRust(
    scalarFrom: Int,
    scalarTo: Int,
    text: String
): Boolean {
    if (!hasLiveEditor()) return false
    val globalRange = inputScalarRange(scalarFrom, scalarTo) ?: return false
    onReplaceTextInRustForTesting?.let { callback ->
        callback(globalRange.first, globalRange.second, text)
        return true
    }
    if (!canDispatchTableCellMutation()) return false
    val update = v2Driver?.replaceTextRange(globalRange.first, globalRange.second, text)
        ?: return false
    applyRustUpdateJSON(update)
    return true
}

internal fun EditorEditText.insertPlainTextRangeInRust(
    scalarFrom: Int,
    scalarTo: Int,
    text: String,
    requestedCursorScalar: Int? = null
) {
    if (!hasLiveEditor()) return
    recordImeTraceForTesting(
        "rustPlainTextRoute",
        "range=$scalarFrom..$scalarTo textLength=${text.length} requestedCursor=$requestedCursorScalar"
    )
    if (text.isEmpty()) {
        if (scalarFrom != scalarTo) {
            deleteRangeInRust(scalarFrom, scalarTo)
        }
        applyRequestedCursorScalar(requestedCursorScalar)
        return
    }
    if (text.indexOf('\n') >= 0 || text.indexOf('\r') >= 0) {
        if (!replaceTextRangeInRust(scalarFrom, scalarTo, text)) {
            restoreAuthorizedTextSnapshotForEditor()
            return
        }
        applyRequestedCursorScalar(requestedCursorScalar)
        return
    }

    if (scalarFrom != scalarTo) {
        replaceTextRangeInRust(scalarFrom, scalarTo, text)
    } else {
        insertTextInRust(text, scalarFrom)
    }
    applyRequestedCursorScalar(requestedCursorScalar)
}

internal fun EditorEditText.requestedCursorScalar(
    scalarFrom: Int,
    scalarTo: Int,
    currentText: String,
    insertedText: String,
    newCursorPosition: Int
): Int? {
    if (newCursorPosition == 1) return null
    val rawStart = PositionBridge.scalarToUtf16(scalarFrom, currentText)
    val inCodeBlock =
        (text as? Spanned)?.getSpans(rawStart, rawStart, CodeBlockSpan::class.java)?.isNotEmpty() ==
            true
    val effectiveText = if (inCodeBlock) {
        insertedText
    } else {
        insertedText.replace(
            "\r\n",
            "\n"
        ).replace('\r', '\n')
    }
    val insertedScalarLength = effectiveText.codePointCount(0, effectiveText.length)
    val currentScalarLength = currentText.codePointCount(0, currentText.length)
    val nextScalarLength =
        (currentScalarLength - (scalarTo - scalarFrom) + insertedScalarLength).coerceAtLeast(0)
    val requested = if (newCursorPosition > 0) {
        scalarFrom + insertedScalarLength + newCursorPosition - 1
    } else {
        scalarFrom + newCursorPosition
    }
    return requested.coerceIn(0, nextScalarLength)
}

internal fun EditorEditText.applyRequestedCursorScalar(requestedCursorScalar: Int?) {
    val requested = requestedCursorScalar ?: return
    if (!hasLiveEditor()) return
    val safeScalar = requested.coerceAtLeast(0)
    val globalScalar = inputScalar(safeScalar) ?: return
    val cursorDriver = v2Driver
    if (cursorDriver != null) {
        if (!canDispatchTableCellMutation()) return
        cursorDriver.syncSelectionQuiet(globalScalar, globalScalar)?.let(::applyRustUpdateJSON)
    } else {
        onSetSelectionScalarInRustForTesting?.let { callback ->
            callback(globalScalar, globalScalar)
        }
    }
    val currentText = text?.toString().orEmpty()
    val localScalar = safeScalar.coerceIn(0, currentText.codePointCount(0, currentText.length))
    val safeUtf16 = PositionBridge.scalarToUtf16(localScalar, currentText)
        .coerceIn(0, currentText.length)
    if (selectionStart != safeUtf16 || selectionEnd != safeUtf16) {
        setSelection(safeUtf16, safeUtf16)
    }
}

/**
 * Delete a scalar range via the Rust editor.
 *
 * @param scalarFrom Start scalar offset (inclusive).
 * @param scalarTo End scalar offset (exclusive).
 */
internal fun EditorEditText.deleteRangeInRust(scalarFrom: Int, scalarTo: Int) {
    if (!hasLiveEditor()) return
    if (scalarFrom >= scalarTo) return
    val globalRange = inputScalarRange(scalarFrom, scalarTo) ?: return
    onDeleteRangeInRustForTesting?.let { callback ->
        callback(globalRange.first, globalRange.second)
        return
    }
    if (!canDispatchTableCellMutation()) return
    v2Driver?.let { driver ->
        driver.deleteScalarRange(globalRange.first, globalRange.second)
            ?.let { applyRustUpdateJSON(it) }
    }
}

internal fun EditorEditText.deleteBackwardAtSelectionScalarInRust(
    scalarAnchor: Int,
    scalarHead: Int
) {
    if (!hasLiveEditor()) return
    if (!canDeleteBackwardAtLocalSelection(scalarAnchor, scalarHead)) return
    val globalSelection = inputScalarSelection(scalarAnchor, scalarHead) ?: return
    onDeleteBackwardAtSelectionScalarInRustForTesting?.let { callback ->
        callback(globalSelection.first, globalSelection.second)
        return
    }
    if (!canDispatchTableCellMutation()) return
    v2Driver?.let { driver ->
        if (selectAtomBeforeEmptyTrailingParagraph(driver)) return
        driver.deleteBackwardAtSelection(globalSelection.first, globalSelection.second)
            ?.let { applyRustUpdateJSON(it) }
    }
}

internal fun EditorEditText.toggleTaskItemCheckedAtSelectionScalarInRust(
    scalarAnchor: Int,
    scalarHead: Int
) {
    if (!hasLiveEditor()) return
    val globalSelection = inputScalarSelection(scalarAnchor, scalarHead) ?: return
    onToggleTaskItemCheckedAtSelectionScalarInRustForTesting?.let { callback ->
        callback(globalSelection.first, globalSelection.second)
        return
    }
    if (!canDispatchTableCellMutation()) return
    v2Driver?.let { driver ->
        val selection =
            currentLogicalScalarSelection() ?: rawScalarSelection(text?.toString().orEmpty())
        driver.toggleTaskItemCheckedAtSelection(globalSelection.first, globalSelection.second)?.let { update ->
            applyRustUpdateJSON(update)
            if (selection != null) {
                inputScalarSelection(selection.first, selection.second)?.let { mapped ->
                    driver.syncSelectionQuiet(mapped.first, mapped.second)?.let(::applyRustUpdateJSON)
                }
                val currentText = text?.toString().orEmpty()
                setSelection(
                    PositionBridge.scalarToUtf16(selection.first, currentText),
                    PositionBridge.scalarToUtf16(selection.second, currentText)
                )
            }
        }
    }
}

/**
 * Split a block at a scalar position via the Rust editor.
 */
internal fun EditorEditText.splitBlockInRust(atScalarPos: Int) {
    if (!hasLiveEditor()) return
    val globalPos = inputScalar(atScalarPos) ?: return
    onSplitBlockInRustForTesting?.let { callback ->
        callback(globalPos)
        return
    }
    if (!canDispatchTableCellMutation()) return
    v2Driver?.let { driver ->
        driver.splitBlockAt(globalPos)?.let { result ->
            applyRustUpdateJSON(
                result.updateJson,
                lineBoundaryRefreshSource = if (result.committed) "splitBlock" else null
            )
        }
    }
}

internal fun EditorEditText.deleteAndSplitInRust(scalarFrom: Int, scalarTo: Int) {
    if (!hasLiveEditor()) return
    val globalRange = inputScalarRange(scalarFrom, scalarTo) ?: return
    onDeleteAndSplitScalarInRustForTesting?.let { callback ->
        callback(globalRange.first, globalRange.second)
        return
    }
    if (!canDispatchTableCellMutation()) return
    v2Driver?.let { driver ->
        driver.deleteAndSplit(globalRange.first, globalRange.second)?.let { result ->
            applyRustUpdateJSON(
                result.updateJson,
                lineBoundaryRefreshSource = if (result.committed) "deleteAndSplit" else null
            )
        }
    }
}

internal fun EditorEditText.isSelectionInsideList(): Boolean {
    if (!hasLiveEditor()) return false
    if (isTableCellInput) return false

    return try {
        val stateJson = v2Driver?.currentStateJson() ?: return false
        val state = org.json.JSONObject(stateJson)
        val nodes = state.optJSONObject("activeState")?.optJSONObject("nodes")
        nodes?.keys()?.asSequence()?.any { nodeType ->
            EditorNodeTypes.isListContainer(nodeType) && nodes.optBoolean(nodeType, false)
        } == true
    } catch (_: Exception) {
        false
    }
}

internal fun EditorEditText.preferredHardBreakNodeType(): String {
    if (isTableCellInput) return "hardBreak"
    return try {
        val stateJson = v2Driver?.currentStateJson() ?: return "hardBreak"
        val insertableNodes = org.json.JSONObject(stateJson)
            .optJSONObject("activeState")
            ?.optJSONArray("insertableNodes")
        val names = buildSet {
            if (insertableNodes != null) {
                for (index in 0 until insertableNodes.length()) {
                    insertableNodes.optString(index, null)?.let(::add)
                }
            }
        }
        EditorNodeTypes.preferredHardBreak(names)
    } catch (_: Exception) {
        "hardBreak"
    }
}

/**
 * Paste HTML content through Rust.
 */
internal fun EditorEditText.pasteHTML(html: String) {
    if (!hasLiveEditor()) return
    syncCurrentSelectionToRust()
    onInsertContentHtmlInRustForTesting?.let { callback ->
        callback(html)
        return
    }
    if (!canDispatchTableCellMutation()) return
    v2Driver?.let { driver ->
        val selection = currentScalarSelection()?.let { inputScalarSelection(it.first, it.second) }
        val update = if (selection != null) {
            driver.insertContentHtmlAtSelection(html, selection.first, selection.second)
        } else {
            null
        }
        update?.let { applyUpdateJSON(it) }
    }
}

/**
 * Paste plain text through Rust.
 */
internal fun EditorEditText.pastePlainText(text: String) {
    val (scalarStart, scalarEnd) = currentScalarSelection() ?: return
    insertPlainTextRangeInRust(scalarStart, scalarEnd, text)
}
