package com.apollohg.editor

import android.text.Annotation
import android.text.Spanned
import org.json.JSONObject

private const val RENDER_CHILD_INDEX_FIELD = "topLevelChildIndex"

private data class RetainedVoidMetadata(
    val annotation: Annotation,
    val documentPosition: Long,
    val atom: AtomBlockSpan?
)

private data class RetainedBlockMetadata(
    val annotations: List<Annotation>,
    val childIndex: Int,
    val voidNodes: List<RetainedVoidMetadata>
)

private fun EditorEditText.renderBlockVisuallyEquals(
    left: org.json.JSONArray,
    right: org.json.JSONArray
): Boolean {
    if (left === right) return true
    if (left.length() != right.length()) return false
    for (index in 0 until left.length()) {
        val leftElement = left.optJSONObject(index) ?: return false
        val rightElement = right.optJSONObject(index) ?: return false
        if (leftElement === rightElement) continue
        val leftCount = leftElement.length() -
            (if (leftElement.has(RENDER_DOC_POSITION_FIELD)) 1 else 0) -
            (if (leftElement.has(RENDER_CHILD_INDEX_FIELD)) 1 else 0)
        val rightCount = rightElement.length() -
            (if (rightElement.has(RENDER_DOC_POSITION_FIELD)) 1 else 0) -
            (if (rightElement.has(RENDER_CHILD_INDEX_FIELD)) 1 else 0)
        if (leftCount != rightCount) return false
        val fields = leftElement.keys()
        while (fields.hasNext()) {
            val field = fields.next()
            if (field == RENDER_DOC_POSITION_FIELD || field == RENDER_CHILD_INDEX_FIELD) continue
            if (!rightElement.has(field) ||
                !jsonValuesEqual(leftElement.opt(field), rightElement.opt(field))
            ) {
                return false
            }
        }
    }
    return true
}

private fun EditorEditText.renderBlockVisuallyEquals(
    current: List<RetainedRenderBlock>,
    currentIndex: Int,
    updated: ResolvedRenderBlocks,
    updatedIndex: Int
): Boolean = current[currentIndex].contentEquals(updated.blocks[updatedIndex]) ||
    renderBlockVisuallyEquals(current[currentIndex].parse(), updated.block(updatedIndex))

private fun EditorEditText.deriveVisualRenderPatch(
    updated: ResolvedRenderBlocks
): ParsedRenderPatch? {
    val current = currentRenderBlocks ?: return null
    val sharedCount = minOf(current.size, updated.size)
    var prefix = 0
    while (prefix < sharedCount && renderBlockVisuallyEquals(current, prefix, updated, prefix)) {
        prefix++
    }
    var suffix = 0
    while (suffix < sharedCount - prefix && renderBlockVisuallyEquals(
            current,
            current.size - suffix - 1,
            updated,
            updated.size - suffix - 1
        )
    ) {
        suffix++
    }
    val startIndex = if (current.size != updated.size && prefix > 0) prefix - 1 else prefix
    val replacement = org.json.JSONArray()
    for (index in startIndex until updated.size - suffix) replacement.put(updated.block(index))
    return ParsedRenderPatch(
        currentRenderBlocksDocumentVersion,
        startIndex,
        current.size - startIndex - suffix,
        replacement
    )
}

private fun EditorEditText.retainedBlockMetadata(
    content: Spanned,
    patch: ParsedRenderPatch,
    updated: ResolvedRenderBlocks
): List<RetainedBlockMetadata>? {
    val annotations = content.getSpans(0, content.length, Annotation::class.java)
    val byChild = annotations.filter {
        it.key == RenderBridge.NATIVE_TOP_LEVEL_CHILD_INDEX_ANNOTATION
    }.groupBy { it.value.toIntOrNull() ?: return null }
    val previousCount = currentRenderBlocks?.size ?: return null
    val childBounds = (0 until previousCount).map { index ->
        val owned = byChild[index] ?: return null
        owned.minOf(content::getSpanStart) to owned.maxOf(content::getSpanEnd)
    }
    val nodeTypes = annotations.filter {
        it.key == RenderBridge.NATIVE_VOID_NODE_TYPE_ANNOTATION
    }.groupBy { content.getSpanStart(it) to content.getSpanEnd(it) }
    val positionsByChild = mutableMapOf<Int, MutableList<Annotation>>()
    annotations.filter { it.key == RenderBridge.NATIVE_DOC_POS_ANNOTATION }
        .sortedBy(content::getSpanStart).forEach { annotation ->
            val start = content.getSpanStart(annotation)
            val end = content.getSpanEnd(annotation)
            var low = 0
            var high = childBounds.lastIndex
            while (low < high) {
                val middle = (low + high + 1) / 2
                if (childBounds[middle].first <= start) low = middle else high = middle - 1
            }
            if (start < childBounds[low].first || end > childBounds[low].second) return null
            positionsByChild.getOrPut(low) { mutableListOf() }.add(annotation)
        }
    val oldEnd = patch.startIndex + patch.deleteCount
    val delta = patch.renderBlocks.length() - patch.deleteCount
    val replaceRange = replacementRangeForRenderPatch(content, patch.startIndex, patch.deleteCount)
        ?: return null
    val metadata = mutableListOf<RetainedBlockMetadata>()
    for (oldIndex in 0 until previousCount) {
        if (oldIndex in patch.startIndex until oldEnd) continue
        val childAnnotations = byChild[oldIndex]?.filterNot {
            replaceRange.start < replaceRange.endExclusive &&
                content.getSpanStart(it) >= replaceRange.start &&
                content.getSpanEnd(it) <= replaceRange.endExclusive
        }?.takeIf { it.isNotEmpty() } ?: return null
        val childIndex = if (oldIndex >= oldEnd) oldIndex + delta else oldIndex
        val atomNodes = updated.blocks.getOrNull(childIndex)?.atomNodes ?: return null
        val start = childAnnotations.minOf(content::getSpanStart)
        val end = childAnnotations.maxOf(content::getSpanEnd)
        if (start < 0 || end < start) return null
        val registeredNodeTypes = atomRenderConfiguration?.registeredNodeTypes.orEmpty()
        val expected = atomNodes.mapNotNull { node ->
            if (!node.isVoidBlock || node.nodeType in registeredNodeTypes) {
                node.nodeType to (node.documentPosition ?: return null)
            } else {
                null
            }
        }
        val actual = positionsByChild[oldIndex].orEmpty()
        if (actual.size != expected.size) return null
        if (childIndex == oldIndex && actual.isEmpty()) continue
        val positions = actual.zip(expected).map { (annotation, node) ->
            val nodeStart = content.getSpanStart(annotation)
            val nodeEnd = content.getSpanEnd(annotation)
            val nodeType = nodeTypes[nodeStart to nodeEnd]?.singleOrNull()?.value ?: return null
            if (nodeType != node.first) return null
            val atom = content.getSpans(
                nodeStart,
                nodeEnd,
                AtomBlockSpan::class.java
            ).singleOrNull()
            if (atom != null && !atom.hasStableAtomId) return null
            RetainedVoidMetadata(annotation, node.second, atom)
        }
        metadata.add(RetainedBlockMetadata(childAnnotations, childIndex, positions))
    }
    return metadata
}

private fun EditorEditText.refreshRetainedBlockMetadata(
    metadata: List<RetainedBlockMetadata>
): Boolean {
    if (metadata.any { block ->
            (block.annotations + block.voidNodes.map { it.annotation }).any {
                editableText.getSpanStart(it) < 0
            }
        }
    ) {
        return false
    }
    if (metadata.isEmpty()) return true
    updateAnnotationSpans {
        for (block in metadata) {
            val childIndex = block.childIndex.toString()
            for (annotation in block.annotations) {
                if (annotation.value == childIndex) continue
                if (annotation is RenderMetadataAnnotation) {
                    annotation.updateValue(childIndex)
                    continue
                }
                val start = editableText.getSpanStart(annotation)
                val end = editableText.getSpanEnd(annotation)
                val flags = editableText.getSpanFlags(annotation)
                editableText.removeSpan(annotation)
                editableText.setSpan(Annotation(annotation.key, childIndex), start, end, flags)
            }
            for (node in block.voidNodes) {
                val documentPosition = node.documentPosition.toString()
                if (node.annotation.value != documentPosition) {
                    if (node.annotation is RenderMetadataAnnotation) {
                        node.annotation.updateValue(documentPosition)
                        node.atom?.docPos = node.documentPosition.toInt()
                        continue
                    }
                    val start = editableText.getSpanStart(node.annotation)
                    val end = editableText.getSpanEnd(node.annotation)
                    val flags = editableText.getSpanFlags(node.annotation)
                    editableText.removeSpan(node.annotation)
                    editableText.setSpan(
                        Annotation(RenderBridge.NATIVE_DOC_POS_ANNOTATION, documentPosition),
                        start,
                        end,
                        flags
                    )
                }
                node.atom?.docPos = node.documentPosition.toInt()
            }
        }
    }
    return true
}

internal fun EditorEditText.parseRenderPatch(raw: org.json.JSONObject?): ParsedRenderPatch? {
    if (raw == null) return null
    val renderBlocks = raw.optJSONArray("renderBlocks") ?: return null
    val baseDocumentVersion = if (raw.has("baseDocumentVersion")) {
        canonicalV2U64(raw.opt("baseDocumentVersion") as? String) ?: return null
    } else {
        null
    }
    return ParsedRenderPatch(
        baseDocumentVersion = baseDocumentVersion,
        startIndex = raw.optInt("startIndex", -1),
        deleteCount = raw.optInt("deleteCount", -1),
        renderBlocks = renderBlocks
    ).takeIf { it.startIndex >= 0 && it.deleteCount >= 0 }
}

internal fun EditorEditText.patchMatchesCurrentRenderBlocks(
    patch: ParsedRenderPatch,
    updateDocumentVersion: String?
): Boolean = if (patch.baseDocumentVersion == null) {
    updateDocumentVersion == null && currentRenderBlocksDocumentVersion == null
} else {
    patch.baseDocumentVersion == currentRenderBlocksDocumentVersion
}

internal fun EditorEditText.retainCurrentRenderBlocks(
    blocks: ResolvedRenderBlocks?,
    documentVersion: String?,
    needFullApply: Boolean
) {
    currentRenderBlocks = blocks?.blocks
    currentRenderBlocksDocumentVersion = documentVersion.takeIf { blocks != null }
    currentRenderBlocksNeedFullApply = blocks != null && needFullApply
}

internal fun EditorEditText.invalidateCurrentRenderBlocks() {
    currentRenderBlocks = null
    currentRenderBlocksDocumentVersion = null
    currentRenderBlocksNeedFullApply = false
}

internal fun EditorEditText.recoverRenderPatchBaseMismatch(
    notifyListener: Boolean,
    refreshInputConnectionForExternalUpdate: Boolean
): Boolean {
    invalidateCurrentRenderBlocks()
    if (recoveringRenderPatchBaseMismatch) return false
    val adapter = v2Driver as? EditorV2Adapter ?: return false
    val recovery = adapter.recoverNativeRender() ?: return false
    recoveringRenderPatchBaseMismatch = true
    return try {
        applyUpdateJSON(
            recovery,
            notifyListener = notifyListener,
            refreshInputConnectionForExternalUpdate = refreshInputConnectionForExternalUpdate
        )
    } finally {
        recoveringRenderPatchBaseMismatch = false
    }
}

internal fun EditorEditText.hasTopLevelChildMetadata(content: Spanned): Boolean =
    content.getSpans(0, content.length, Annotation::class.java).any {
        it.key == RenderBridge.NATIVE_TOP_LEVEL_CHILD_INDEX_ANNOTATION
    }

internal fun EditorEditText.firstCharacterOffsetForTopLevelChildIndex(
    content: Spanned,
    index: Int
): Int? {
    val targetValue = index.toString()
    return content
        .getSpans(0, content.length, Annotation::class.java)
        .asSequence()
        .filter {
            it.key == RenderBridge.NATIVE_TOP_LEVEL_CHILD_INDEX_ANNOTATION &&
                it.value == targetValue
        }
        .mapNotNull { span ->
            val spanStart = content.getSpanStart(span)
            val spanEnd = content.getSpanEnd(span)
            if (spanStart < 0 || spanEnd < spanStart) {
                null
            } else if (spanEnd == spanStart) {
                spanStart
            } else {
                val separator = content.getSpans(spanStart, spanEnd, Annotation::class.java)
                    .firstOrNull {
                        it.key == RenderBridge.NATIVE_INTER_BLOCK_SEPARATOR_ANNOTATION &&
                            content.getSpanStart(it) >= spanStart &&
                            content.getSpanEnd(it) == spanEnd
                    }
                var candidate = separator?.let(content::getSpanStart) ?: spanStart
                while (candidate < spanEnd && candidate < content.length) {
                    when (content[candidate]) {
                        '\n', '\r' -> {
                            val isHardBreak = content.getSpans(
                                candidate,
                                candidate + 1,
                                Annotation::class.java
                            ).any {
                                it.key == RenderBridge.NATIVE_VOID_NODE_TYPE_ANNOTATION &&
                                    EditorNodeTypes.isHardBreak(it.value) &&
                                    content.getSpanStart(it) <= candidate &&
                                    content.getSpanEnd(it) > candidate
                            }
                            if (isHardBreak) return@mapNotNull candidate
                            candidate += 1
                        }

                        else -> return@mapNotNull candidate
                    }
                }
                null
            }
        }
        .minOrNull()
}

internal fun EditorEditText.replacementRangeForRenderPatch(
    content: Spanned,
    startIndex: Int,
    deleteCount: Int
): RenderReplaceRange? {
    val start = firstCharacterOffsetForTopLevelChildIndex(content, startIndex)
        ?: if (deleteCount == 0) content.length else return null
    val endExclusive = firstCharacterOffsetForTopLevelChildIndex(content, startIndex + deleteCount)
        ?: content.length
    if (start > endExclusive) return null
    return RenderReplaceRange(start = start, endExclusive = endExclusive)
}

internal fun EditorEditText.spannedRangeContainsImageSpan(
    content: Spanned,
    start: Int,
    endExclusive: Int
): Boolean {
    if (start >= endExclusive) return false
    return content.getSpans(start, endExclusive, BlockImageSpan::class.java).isNotEmpty()
}

internal fun EditorEditText.spannedContainsImageSpan(content: Spanned): Boolean =
    spannedRangeContainsImageSpan(content, 0, content.length)

internal fun EditorEditText.spannedRangeContainsUnstableAtom(
    content: Spanned,
    start: Int,
    endExclusive: Int
): Boolean {
    if (start >= endExclusive) return false
    return content.getSpans(start, endExclusive, AtomBlockSpan::class.java)
        .any { !it.hasStableAtomId }
}

internal fun EditorEditText.spannedContainsUnstableAtom(content: Spanned): Boolean =
    spannedRangeContainsUnstableAtom(content, 0, content.length)

internal fun EditorEditText.buildPatchedSpannable(
    patch: ParsedRenderPatch,
    includeTrailingInterBlockSeparator: Boolean
): android.text.SpannableStringBuilder = RenderBridge.buildSpannableFromBlocks(
    patch.renderBlocks,
    startIndex = patch.startIndex,
    includeTrailingInterBlockSeparator = includeTrailingInterBlockSeparator,
    baseFontSize = baseFontSize,
    textColor = baseTextColor,
    theme = theme,
    density = resources.displayMetrics.density,
    hostView = this,
    atomConfiguration = atomRenderConfiguration,
    reuseImages = false
)

internal fun EditorEditText.normalizedJsonValue(value: Any?): Any? =
    if (value === org.json.JSONObject.NULL) null else value

internal fun EditorEditText.jsonValuesEqual(left: Any?, right: Any?): Boolean {
    val normalizedLeft = normalizedJsonValue(left)
    val normalizedRight = normalizedJsonValue(right)
    if (normalizedLeft === normalizedRight) return true
    if (normalizedLeft == null || normalizedRight == null) return false

    if (normalizedLeft is org.json.JSONArray && normalizedRight is org.json.JSONArray) {
        if (normalizedLeft.length() != normalizedRight.length()) return false
        for (index in 0 until normalizedLeft.length()) {
            if (!jsonValuesEqual(normalizedLeft.opt(index), normalizedRight.opt(index))) {
                return false
            }
        }
        return true
    }

    if (normalizedLeft is org.json.JSONObject && normalizedRight is org.json.JSONObject) {
        if (normalizedLeft.length() != normalizedRight.length()) return false
        val keys = normalizedLeft.keys()
        while (keys.hasNext()) {
            val key = keys.next()
            if (!normalizedRight.has(key)) return false
            if (!jsonValuesEqual(normalizedLeft.opt(key), normalizedRight.opt(key))) {
                return false
            }
        }
        return true
    }

    if (normalizedLeft is Number && normalizedRight is Number) {
        return normalizedLeft.toDouble() == normalizedRight.toDouble()
    }

    return normalizedLeft == normalizedRight
}

internal fun EditorEditText.applyRenderPatchIfPossible(
    suppliedPatch: ParsedRenderPatch,
    preserveInputConnectionForExternalUpdate: Boolean,
    updatedRenderBlocks: ResolvedRenderBlocks
): PatchApplyTrace {
    val eligibilityStartedAt = System.nanoTime()
    if (suppliedPatch.deleteCount == 0 && suppliedPatch.renderBlocks.length() == 0) {
        return PatchApplyTrace(true, System.nanoTime() - eligibilityStartedAt, 0L, 0L)
    }
    val patch = deriveVisualRenderPatch(updatedRenderBlocks) ?: suppliedPatch
    val content = text as? Spanned ?: return PatchApplyTrace(
        applied = false,
        eligibilityNanos = System.nanoTime() - eligibilityStartedAt,
        buildRenderNanos = 0L,
        applyRenderNanos = 0L
    )
    val metadata =
        retainedBlockMetadata(content, patch, updatedRenderBlocks) ?: return PatchApplyTrace(
            applied = false,
            eligibilityNanos = System.nanoTime() - eligibilityStartedAt,
            buildRenderNanos = 0L,
            applyRenderNanos = 0L
        )
    if (patch.deleteCount == 0 && patch.renderBlocks.length() == 0) {
        val applyStartedAt = System.nanoTime()
        val applied: Boolean
        isApplyingRustState = true
        beginBatchEdit()
        try {
            applied = refreshRetainedBlockMetadata(metadata)
            if (applied) {
                invalidateImeTextCoordinateMapperForEditor()
                lastAuthorizedRenderedText = immutableRenderedText(text, excludeNoCopySpans = true)
            }
        } finally {
            endBatchEdit()
            isApplyingRustState = false
        }
        return PatchApplyTrace(
            applied = applied,
            eligibilityNanos = System.nanoTime() - eligibilityStartedAt,
            buildRenderNanos = 0L,
            applyRenderNanos = System.nanoTime() - applyStartedAt
        )
    }
    if (!hasTopLevelChildMetadata(content)) {
        return PatchApplyTrace(
            applied = false,
            eligibilityNanos = System.nanoTime() - eligibilityStartedAt,
            buildRenderNanos = 0L,
            applyRenderNanos = 0L
        )
    }

    val replaceRange = replacementRangeForRenderPatch(content, patch.startIndex, patch.deleteCount)
        ?: return PatchApplyTrace(
            applied = false,
            eligibilityNanos = System.nanoTime() - eligibilityStartedAt,
            buildRenderNanos = 0L,
            applyRenderNanos = 0L
        )
    if (
        spannedRangeContainsImageSpan(content, replaceRange.start, replaceRange.endExclusive) ||
        spannedRangeContainsUnstableAtom(content, replaceRange.start, replaceRange.endExclusive)
    ) {
        return PatchApplyTrace(
            applied = false,
            eligibilityNanos = System.nanoTime() - eligibilityStartedAt,
            buildRenderNanos = 0L,
            applyRenderNanos = 0L
        )
    }
    val eligibilityNanos = System.nanoTime() - eligibilityStartedAt

    val buildStartedAt = System.nanoTime()
    val patchedSpannable = buildPatchedSpannable(
        patch,
        includeTrailingInterBlockSeparator = replaceRange.endExclusive < content.length
    )
    val buildRenderNanos = System.nanoTime() - buildStartedAt
    if (
        spannedContainsImageSpan(patchedSpannable) ||
        spannedContainsUnstableAtom(patchedSpannable)
    ) {
        return PatchApplyTrace(
            applied = false,
            eligibilityNanos = eligibilityNanos,
            buildRenderNanos = buildRenderNanos,
            applyRenderNanos = 0L
        )
    }

    val applyStartedAt = System.nanoTime()
    val applied = applyRenderedSpannable(
        spannable = patchedSpannable,
        replaceRange = replaceRange,
        replacedTopLevelStartIndex = patch.startIndex,
        replacedTopLevelDeleteCount = patch.deleteCount,
        usedPatch = true,
        preserveInputConnectionForExternalUpdate = preserveInputConnectionForExternalUpdate,
        finalizePositionalMetadata = { refreshRetainedBlockMetadata(metadata) }
    )
    return PatchApplyTrace(
        applied = applied,
        eligibilityNanos = eligibilityNanos,
        buildRenderNanos = buildRenderNanos,
        applyRenderNanos = System.nanoTime() - applyStartedAt
    )
}
