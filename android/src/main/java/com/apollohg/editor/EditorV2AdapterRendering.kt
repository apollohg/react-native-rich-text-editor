package com.apollohg.editor

import org.json.JSONArray
import org.json.JSONObject

internal fun EditorV2Adapter.adopt(
    snapshot: AtomicRenderSnapshot,
    stripViewSelection: Boolean,
    engineOwnedSelection: Boolean,
    resolvedPositionEpoch: String? = snapshot.positionEpoch
): String? {
    fun blocks(value: JSONArray): List<List<Any?>> = (0 until value.length()).map { index ->
        val block = value.getJSONArray(index)
        (0 until block.length()).map { block.opt(it) }
    }
    var candidate = snapshot.renderObject.optJSONArray("renderBlocks")?.let(::blocks)
    if (candidate == null) {
        val patch = snapshot.renderObject.optJSONObject("renderPatch") ?: return null
        val retained = cachedSemanticRenderBlocks
        val start = patch.optLong("startIndex", -1)
        val delete = patch.optLong("deleteCount", -1)
        if (retained != null && patch.optString("baseDocumentVersion").toULongOrNull() == cachedAtomicRenderDocumentRevision &&
            start >= 0 && delete >= 0 && start + delete <= retained.size) {
            candidate = retained.take(start.toInt()) + blocks(patch.getJSONArray("renderBlocks")) + retained.drop((start + delete).toInt())
        } else if (snapshot.renderObject.has("tableAttributes") || snapshot.renderObject.has("tableRecords") || cachedTableAttributes.isNotEmpty() || cachedTableRecords.isNotEmpty()) {
            return null
        }
    }
    if (candidate != null && !validSemanticRenderElements(candidate.flatten(), snapshot.tableAttributes, snapshot.tableRecords)) return null
    val update = JSONObject(snapshot.viewUpdateJson)
    if (stripViewSelection) update.remove("selection")
    val updateJson = update.toString()
    baseDocumentRevision = snapshot.documentRevision
    stateRevision = snapshot.stateRevision
    cachedScalarLength = snapshot.scalarLength
    cachedAuthoritativeScalarSelection = snapshot.scalarSelection?.copyOf()
    lastSyncedScalarSelection =
        if (engineOwnedSelection) snapshot.scalarSelection?.copyOf() else null
    cachedActiveState = snapshot.activeState
    cachedHistoryState = snapshot.historyState
    cachedViewUpdateJson = updateJson
    cachedAtomicRenderJson = snapshot.atomicRenderJson
    cachedAtomicRenderDocumentRevision = snapshot.documentRevision
    cachedSemanticRenderBlocks = candidate
    cachedTableAttributes = snapshot.tableAttributes
    cachedTableRecords = snapshot.tableRecords
    cachedTableInputMappings = snapshot.tableInputMappings
    if (resolvedPositionEpoch != null) positionEpoch = resolvedPositionEpoch
    return updateJson
}

internal fun EditorV2Adapter.fetchDocumentJson(): String? =
    when (val result = backend.getDocumentJson(editorId)) {
        is EditorV2CallResult.Err -> {
            emit(result.error)
            null
        }

        is EditorV2CallResult.Ok -> result.value
    }

internal fun EditorV2Adapter.refreshInternal(
    mirrorSelection: IntArray?,
    stripViewSelection: Boolean = mirrorSelection == null,
    controlledPropSnapshot: Boolean = false
): String? {
    if (destroyed) {
        emit(EditorV2Adapter.destroyedError())
        return null
    }
    val ownerId = nativeOwnerId
    val renderResult = if (ownerId == null) {
        backend.renderUpdate(editorId, mirrorSelection?.get(0), mirrorSelection?.get(1))
    } else {
        backend.renderNative(editorId, ownerId, mirrorSelection?.get(0), mirrorSelection?.get(1))
    }
    val derived = when (val result = renderResult) {
        is EditorV2CallResult.Err -> {
            // A render update that fails or violates the frozen shape is a
            // boundary failure like any other. Returning null without
            // reporting it leaves every caller — the paired view and the
            // stateless render probe alike — holding a bare null with no
            // cause to surface, so the engine's own error is what travels.
            emit(result.error)
            return null
        }

        is EditorV2CallResult.Ok -> result.value
    }
    renderUpdateCallCountForTesting += 1
    val snapshot = parseAtomicRenderSnapshot(derived)
    return if (snapshot == null) {
        emit(EditorV2Adapter.contractError("v2 render update violates the frozen shape"))
        null
    } else {
        // Preserve an IME-owned caret only after authoritative active and
        // history state has been adopted from the post-operation snapshot.
        val viewUpdateJson = adopt(
            snapshot,
            stripViewSelection = stripViewSelection,
            engineOwnedSelection = mirrorSelection == null
        ) ?: return null
        if (controlledPropSnapshot) snapshot.atomicRenderJson else viewUpdateJson
    }
}

internal fun EditorV2Adapter.pinPositionEpochCandidate(documentRevision: ULong): String? {
    val ownerId = nativeOwnerId ?: return positionEpoch
    return when (
        val result = backend.pinPositionEpoch(
            editorId,
            ownerId,
            documentRevision.toString()
        )
    ) {
        is EditorV2CallResult.Err -> {
            emit(result.error)
            null
        }

        is EditorV2CallResult.Ok -> try {
            canonicalV2U64(JSONObject(result.value).opt("positionEpoch") as? String)
                ?: run {
                    emit(
                        EditorV2Adapter.contractError(
                            "v2 position epoch result violates the frozen shape"
                        )
                    )
                    null
                }
        } catch (_: Exception) {
            emit(
                EditorV2Adapter.contractError("v2 position epoch result violates the frozen shape")
            )
            null
        }
    }
}

internal fun EditorV2Adapter.pinCurrentPositionEpoch(documentRevision: ULong): Boolean {
    if (nativeOwnerId == null) return true
    val candidate = pinPositionEpochCandidate(documentRevision) ?: return false
    positionEpoch = candidate
    return true
}
