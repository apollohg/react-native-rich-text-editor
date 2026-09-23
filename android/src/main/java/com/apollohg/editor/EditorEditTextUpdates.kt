package com.apollohg.editor

import android.text.SpannableStringBuilder
import com.apollohg.editor.EditorEditText.ApplyUpdateTrace
import org.json.JSONObject

private data class RootTableRender(
    val extents: Map<String, TableInputExtent>,
    val scalarLength: Int,
    val tableIds: Set<String>
)

private fun containsRootTableElement(update: JSONObject): Boolean {
    fun hasTable(elements: org.json.JSONArray?): Boolean = elements != null &&
        (0 until elements.length()).any { elements.optJSONObject(it)?.optString("type") == "table" }
    if (hasTable(update.optJSONArray("renderElements"))) return true
    val blocks = update.optJSONArray("renderBlocks")
        ?: update.optJSONObject("renderPatch")?.optJSONArray("renderBlocks")
        ?: return false
    return (0 until blocks.length()).any { hasTable(blocks.optJSONArray(it)) }
}

private fun EditorEditText.rootTableRenderForUpdate(
    updateJSON: String,
    update: JSONObject,
    blocks: org.json.JSONArray?
): RootTableRender? {
    val tableIds = buildSet {
        if (blocks != null) for (blockIndex in 0 until blocks.length()) {
            val block = blocks.optJSONArray(blockIndex) ?: continue
            for (elementIndex in 0 until block.length()) {
                val element = block.optJSONObject(elementIndex) ?: continue
                if (element.optString("type") == "table") {
                    val id = element.opt("tableId") as? String ?: return null
                    add(id)
                }
            }
        }
    }
    if (tableIds.isEmpty()) {
        val elements = update.optJSONArray("renderElements")
        if (elements != null && (0 until elements.length()).any {
            elements.optJSONObject(it)?.optString("type") == "table"
        }) return null
    }
    val adapter = v2Driver as? EditorV2Adapter
    val paired = if (adapter != null &&
        (updateJSON == adapter.cachedViewUpdateJson || updateJSON == adapter.cachedAtomicRenderJson) &&
        canonicalV2U64(update.opt("documentVersion") as? String)?.toULong() == adapter.cachedAtomicRenderDocumentRevision
    ) adapter else null
    if (tableIds.isEmpty()) {
        return if ((rootTablePositionMap == null && !rootTableRenderNeedsRefresh) || paired != null) {
            RootTableRender(emptyMap(), 0, emptySet())
        } else null
    }
    if (adapter != null && paired == null) return null
    val atomic = if (adapter == null && update.has("scalarLength")) {
        parseAtomicRenderSnapshot(updateJSON)
    } else null
    val mappings = atomic?.tableInputMappings ?: paired?.cachedTableInputMappings ?: return null
    val scalarLength = atomic?.scalarLength ?: paired?.cachedScalarLength ?: return null
    if (!tableIds.all { mappings.tables.containsKey(it) }) return null
    return RootTableRender(tableIds.mapNotNull { id ->
        mappings.tables.getValue(id).extent?.let { id to it }
    }.toMap(), scalarLength, tableIds)
}

/**
 * Apply a full render update from Rust to the EditText.
 *
 * Parses the update JSON, converts render elements to [android.text.SpannableStringBuilder]
 * via [RenderBridge], and replaces the EditText's content.
 *
 * @param updateJSON The JSON string from an [EditorV2Driver] transaction result.
 */
internal fun EditorEditText.applyUpdateJSONImpl(
    updateJSON: String,
    notifyListener: Boolean = true,
    refreshInputConnectionForExternalUpdate: Boolean = false
): Boolean {
    if (isTableCellInput) {
        return tableCellUpdateConsumer?.invoke(
            updateJSON, notifyListener, refreshInputConnectionForExternalUpdate
        ) == true
    }
    throwOnNextApplyUpdateForTesting?.let { error ->
        throwOnNextApplyUpdateForTesting = null
        throw error
    }
    val totalStartedAt = System.nanoTime()
    val previousVisibleText = text?.toString().orEmpty()
    val parseStartedAt = totalStartedAt
    val update = try {
        org.json.JSONObject(updateJSON)
    } catch (error: Exception) {
        recordImeTraceForTesting(
            "applyUpdateJSONNoop",
            "reason=parseError jsonLength=${updateJSON.length} error=${error.javaClass.simpleName}"
        )
        return false
    }
    val tableSensitiveUpdate = rootTablePositionMap != null || rootTableRenderNeedsRefresh ||
        update.has("tableInputMappings") || containsRootTableElement(update)
    fun advanceDeferred() {
        deferredRustUpdateJSON?.let { deferredUpdateJSON ->
            if (deferredUpdateJSON != updateJSON) {
                advanceRenderBlocksThroughDeferredUpdate(deferredUpdateJSON)
            }
            cancelDeferredRustUpdateApplication(invalidateRenderBlocks = false)
        }
    }
    if (!tableSensitiveUpdate) advanceDeferred()
    val parseNanos = System.nanoTime() - parseStartedAt

    val resolveRenderBlocksStartedAt = System.nanoTime()
    val updateDocumentVersion = canonicalV2U64(update.opt("documentVersion") as? String)
    val renderElements = update.optJSONArray("renderElements")
    val renderBlocks = update.optJSONArray("renderBlocks")
    val renderPatch = parseRenderPatch(update.optJSONObject("renderPatch"))
    val resolvedRenderBlocks = renderBlocks
        ?: renderPatch
            ?.takeIf { patchMatchesCurrentRenderBlocks(it, updateDocumentVersion) }
            ?.let { patch ->
                currentRenderBlocksJson?.let { mergeRenderBlocks(it, patch) }
            }
    val rootRender = rootTableRenderForUpdate(updateJSON, update, resolvedRenderBlocks)
        ?: run { recordImeTraceForTesting("rootTableRenderRejected", "admission"); return false }
    val hasRootTable = rootRender.tableIds.isNotEmpty()
    val prebuiltRootRender = if (hasRootTable) {
        val blocks = resolvedRenderBlocks ?: return false
        RenderBridge.buildSpannableFromBlocks(
            blocks,
            baseFontSize = baseFontSize,
            textColor = baseTextColor,
            theme = theme,
            density = resources.displayMetrics.density,
            hostView = this,
            atomConfiguration = atomRenderConfiguration,
            rootTableIds = rootRender.extents.keys,
            synthesizeTrailingHardBreakPlaceholders = false
        )
    } else null
    val nextRootMap = if (prebuiltRootRender != null) {
        RootTablePositionMap.fromRendered(
            prebuiltRootRender, rootRender.extents, rootRender.scalarLength
        ) ?: run { recordImeTraceForTesting("rootTableRenderRejected", "coordinates"); return false }
    } else null
    if (tableSensitiveUpdate) advanceDeferred()
    val resolveRenderBlocksNanos = System.nanoTime() - resolveRenderBlocksStartedAt
    if (
        renderBlocks == null &&
        renderElements == null &&
        renderPatch != null &&
        resolvedRenderBlocks == null
    ) {
        return recoverRenderPatchBaseMismatch(
            notifyListener,
            refreshInputConnectionForExternalUpdate
        )
    }

    // The core is the authority on empty state; adopt it before anything
    // reconsiders the placeholder.
    setCoreReportedDocumentIsEmpty(
        if (update.has("documentIsEmpty")) update.optBoolean("documentIsEmpty") else null
    )
    val shouldSkipRender = !hasRootTable && rootTablePositionMap == null &&
        !refreshInputConnectionForExternalUpdate &&
        !currentRenderBlocksNeedFullApply &&
        !authorizedVisibleTextNeedsRebuild &&
        resolvedRenderBlocks != null &&
        currentRenderBlocksJson?.let { current ->
            renderBlocksEqual(current, resolvedRenderBlocks)
        } == true &&
        text?.toString() == lastAuthorizedText &&
        lastAppliedRenderAppearanceRevision == renderAppearanceRevision
    val previousScrollX = scrollX
    val previousScrollY = scrollY

    explicitSelectedImageRange = null
    val buildRenderNanos: Long
    val applyRenderNanos: Long
    val patchTrace = if (
        !shouldSkipRender && !hasRootTable && rootTablePositionMap == null &&
        !currentRenderBlocksNeedFullApply &&
        renderPatch != null &&
        resolvedRenderBlocks != null &&
        lastAppliedRenderAppearanceRevision == renderAppearanceRevision
    ) {
        applyRenderPatchIfPossible(renderPatch, refreshInputConnectionForExternalUpdate)
    } else {
        null
    }
    val appliedPatch = patchTrace?.applied == true
    if (shouldSkipRender) {
        pendingOptimisticRenderText = null
        lastRenderAppliedPatchForTesting = false
        retainCurrentRenderBlocks(
            resolvedRenderBlocks,
            updateDocumentVersion,
            needFullApply = false
        )
        clearNativeTextMutationAdoptionSuppression()
        clearNativeTextMutationAfterBlurWindow()
        buildRenderNanos = 0L
        applyRenderNanos = 0L
    } else if (appliedPatch) {
        pendingOptimisticRenderText = null
        retainCurrentRenderBlocks(
            resolvedRenderBlocks,
            updateDocumentVersion,
            needFullApply = false
        )
        lastAppliedRenderAppearanceRevision = renderAppearanceRevision
        buildRenderNanos = patchTrace?.buildRenderNanos ?: 0L
        applyRenderNanos = patchTrace?.applyRenderNanos ?: 0L
    } else {
        val buildStartedAt = System.nanoTime()
        val fullSpannable = if (prebuiltRootRender != null) {
            prebuiltRootRender
        } else if (resolvedRenderBlocks != null) {
            RenderBridge.buildSpannableFromBlocks(
                resolvedRenderBlocks,
                baseFontSize = baseFontSize,
                textColor = baseTextColor,
                theme = theme,
                density = resources.displayMetrics.density,
                hostView = this,
                atomConfiguration = atomRenderConfiguration
            )
        } else if (renderElements != null) {
            RenderBridge.buildSpannableFromArray(
                renderElements,
                baseFontSize,
                baseTextColor,
                theme,
                resources.displayMetrics.density,
                this,
                atomRenderConfiguration
            )
        } else {
            recordImeTraceForTesting(
                "applyUpdateJSONNoop",
                "reason=noRenderPayload jsonLength=${updateJSON.length}"
            )
            return false
        }
        buildRenderNanos = System.nanoTime() - buildStartedAt
        retainCurrentRenderBlocks(
            resolvedRenderBlocks,
            updateDocumentVersion,
            needFullApply = false
        )
        val applyStartedAt = System.nanoTime()
        val optimisticText = pendingOptimisticRenderText
        val canReuseOptimisticVisibleText =
            optimisticText != null &&
                text?.toString() == optimisticText &&
                fullSpannable.toString() == optimisticText &&
                !spannedContainsImageSpan(fullSpannable)
        if (canReuseOptimisticVisibleText) {
            authorizeVisibleTextForMatchedOptimisticRender(fullSpannable)
        } else {
            applyRenderedSpannable(
                fullSpannable,
                usedPatch = false,
                preserveInputConnectionForExternalUpdate = refreshInputConnectionForExternalUpdate
            )
        }
        pendingOptimisticRenderText = null
        applyRenderNanos = System.nanoTime() - applyStartedAt
        lastAppliedRenderAppearanceRevision = renderAppearanceRevision
    }

    val rootInputWasBlocked = rootTableSelectionInputBlocked
    rootTablePositionMap = nextRootMap
    rootTableRenderNeedsRefresh = false
    rootTableMapDocumentVersion = updateDocumentVersion.takeIf { nextRootMap != null }
    rootTableMapPositionEpoch = (v2Driver as? EditorV2Adapter)?.positionEpoch
    rootTableMapTableIds = rootRender.tableIds
    rootTableMapExtents = rootRender.extents
    rootTableHasUnmappedExtent = rootRender.tableIds.size != rootRender.extents.size
    rootTableSelectionInputBlocked = nextRootMap != null && rootInputWasBlocked

    val selectionStartedAt = System.nanoTime()
    val selection = update.optJSONObject("selection")
    if (selection != null) {
        applySelectionFromJSON(
            selection,
            updateDocumentVersion
        )
    } else {
        logicalSelectionSnapshot = null
    }
    lastAppliedDocumentVersion = updateDocumentVersion
    authorizedVisibleTextNeedsRebuild = false
    val selectionNanos = System.nanoTime() - selectionStartedAt

    val postApplyStartedAt = System.nanoTime()
    if (notifyListener) {
        editorListener?.onEditorUpdate(updateJSON)
    }
    if (!shouldSkipRender) {
        onContentSizeMayChange?.invoke()
    }
    onSelectionOrContentMayChange?.invoke()
    if (heightBehavior == EditorHeightBehavior.AUTO_GROW) {
        requestLayout()
    } else {
        preserveScrollPosition(previousScrollX, previousScrollY)
    }
    refreshInputConnectionAfterExternalTextReplacementIfNeeded(
        enabled = refreshInputConnectionForExternalUpdate,
        previousVisibleText = previousVisibleText
    )
    val postApplyNanos = System.nanoTime() - postApplyStartedAt

    val totalNanos = System.nanoTime() - totalStartedAt
    recordImeTraceForTesting(
        "applyUpdateJSON",
        "notify=$notifyListener skippedRender=$shouldSkipRender " +
            "attemptedPatch=${renderPatch != null} jsonLength=${updateJSON.length} " +
            "parseUs=${nanosToMicros(
                parseNanos
            )} resolveUs=${nanosToMicros(
                resolveRenderBlocksNanos
            )} buildUs=${nanosToMicros(
                buildRenderNanos
            )} applyUs=${nanosToMicros(
                applyRenderNanos
            )} selectionUs=${nanosToMicros(
                selectionNanos
            )} postUs=${nanosToMicros(postApplyNanos)} totalUs=${nanosToMicros(totalNanos)}"
    )

    if (captureApplyUpdateTraceForTesting) {
        lastApplyUpdateTraceForTesting = ApplyUpdateTrace(
            attemptedPatch = renderPatch != null,
            usedPatch = appliedPatch,
            skippedRender = shouldSkipRender,
            parseNanos = parseNanos,
            resolveRenderBlocksNanos = resolveRenderBlocksNanos,
            patchEligibilityNanos = patchTrace?.eligibilityNanos ?: 0L,
            buildRenderNanos = buildRenderNanos,
            applyRenderNanos = applyRenderNanos,
            selectionNanos = selectionNanos,
            postApplyNanos = postApplyNanos,
            totalNanos = totalNanos
        )
    }
    return !shouldSkipRender
}
