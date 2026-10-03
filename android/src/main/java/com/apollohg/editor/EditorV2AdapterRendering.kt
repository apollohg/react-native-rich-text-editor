package com.apollohg.editor

import com.apollohg.editor.tables.EditorTablePresentationSnapshot
import com.apollohg.editor.tables.TableFrameAdoption
import com.apollohg.editor.tables.resolveEditorCellSelection
import com.apollohg.editor.viewer.PreparedProseInstrumentation
import org.json.JSONArray
import org.json.JSONObject
import uniffi.editor_core.FfiNativeRenderFrame
import uniffi.editor_core.FfiTableFrameKind

private fun EditorV2Adapter.adopt(
    frame: FfiNativeRenderFrame,
    stripViewSelection: Boolean,
    engineOwnedSelection: Boolean
): String? {
    return PreparedProseInstrumentation.measureTableStage(
        PreparedProseInstrumentation.TableStage.ADAPTER_ADOPTION
    ) {
        val snapshot = parseAtomicRenderSnapshot(frame.snapshotJson) ?: return null
        val nextIndex = tableIndex.copy()
        val adoption =
            nextIndex.adopt(frame.tables, installedFrameRevision, snapshot.documentRevision)
                as? TableFrameAdoption.Adopted ?: return null
        if (nextIndex.rootExtents.values.any {
                it.scalarEnd.toLong() > snapshot.scalarLength
            }
        ) {
            return null
        }
        fun blocks(value: JSONArray): List<List<Any?>> = (0 until value.length()).map { index ->
            val block = value.getJSONArray(index)
            (0 until block.length()).map { block.opt(it) }
        }
        var candidate = snapshot.renderObject.optJSONArray("renderBlocks")?.let(::blocks)
        if (candidate == null) {
            val patch = snapshot.renderObject.optJSONObject("renderPatch") ?: return null
            val retained = cachedSemanticRenderBlocks ?: return null
            val start = patch.optLong("startIndex", -1)
            val delete = patch.optLong("deleteCount", -1)
            if (patch.optString("baseDocumentVersion").toULongOrNull() !=
                cachedSemanticRenderBlocksRevision ||
                start < 0 || delete < 0 || start + delete > retained.size
            ) {
                return null
            }
            candidate =
                retained.take(start.toInt()) + blocks(patch.getJSONArray("renderBlocks")) +
                retained.drop((start + delete).toInt())
        }
        if (!validSemanticRenderElements(candidate.flatten(), nextIndex)) return null
        val roots = candidate.flatten().mapNotNull { element ->
            (element as? JSONObject)?.takeIf { it.opt("type") == "table" }?.optString("tableId")
        }.toSet()
        if (roots != nextIndex.rootExtents.keys) return null
        val selection = snapshot.renderObject.optJSONObject("selection")
        if (selection?.opt("type") == "cell" &&
            resolveEditorCellSelection(selection, nextIndex) == null
        ) {
            return null
        }
        val updateObject = if (stripViewSelection) {
            JSONObject(snapshot.viewUpdateJson).apply {
                remove("selection")
            }
        } else {
            snapshot.renderObject
        }
        val updateJson =
            if (stripViewSelection) updateObject.toString() else snapshot.viewUpdateJson
        tableIndex = nextIndex
        cachedTablePresentation = EditorTablePresentationSnapshot(
            snapshot.documentRevision,
            if (adoption.changes.fullReset) null else installedFrameRevision,
            snapshot.positionEpoch,
            nextIndex,
            adoption.changes
        )
        installedFrameRevision = snapshot.documentRevision
        if (adoption.changes.fullReset) {
            fullFrameAdoptionCountForTesting++
        } else {
            deltaFrameAdoptionCountForTesting++
        }
        baseDocumentRevision = snapshot.documentRevision
        stateRevision = snapshot.stateRevision
        cachedScalarLength = snapshot.scalarLength
        cachedAuthoritativeScalarSelection = snapshot.scalarSelection?.copyOf()
        lastSyncedScalarSelection =
            if (engineOwnedSelection) snapshot.scalarSelection?.copyOf() else null
        cachedActiveState = snapshot.activeState
        cachedHistoryState = snapshot.historyState
        cachedViewUpdateJson = updateJson
        cachedViewUpdateObject = updateObject
        cachedAtomicRenderJson = frame.snapshotJson
        cachedAtomicRenderSelectionObject = selection
        cachedAtomicRenderDocumentRevision = snapshot.documentRevision
        cachedSemanticRenderBlocks = candidate
        cachedSemanticRenderBlocksRevision = snapshot.documentRevision
        snapshot.positionEpoch?.let { positionEpoch = it }
        updateJson
    }
}

internal fun EditorV2Adapter.initialUpdateJson(): String? {
    val update = refreshInternal(null, stripViewSelection = false) ?: return null
    val blocks = cachedSemanticRenderBlocks ?: return null
    val objectValue = JSONObject(update).put(
        "renderBlocks",
        JSONArray(
            blocks.map {
                JSONArray(it)
            }
        )
    )
        .put("renderPatch", JSONObject.NULL)
    val complete = objectValue.toString()
    cachedViewUpdateJson = complete
    cachedViewUpdateObject = objectValue
    return complete
}

internal fun EditorV2Adapter?.readOnlyParsedUpdate(updateJson: String): JSONObject =
    this?.cachedViewUpdateObject?.takeIf { updateJson === cachedViewUpdateJson }
        ?: parseSharedStringJsonObject(updateJson)

internal fun EditorV2Adapter?.updateSelection(updateJson: String): JSONObject? =
    readOnlyParsedUpdate(updateJson).optJSONObject("selection")?.let { JSONObject(it.toString()) }

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
    fun fetch(): FfiNativeRenderFrame? {
        renderUpdateCallCountForTesting++
        return PreparedProseInstrumentation.measureTableStage(
            PreparedProseInstrumentation.TableStage.NATIVE_FRAME_AND_FFI
        ) {
            when (
                val result = backend.renderNativeFrame(
                    editorId,
                    nativeOwnerId,
                    mirrorSelection?.get(0),
                    mirrorSelection?.get(1)
                )
            ) {
                is EditorV2CallResult.Err -> {
                    emit(result.error)
                    null
                }

                is EditorV2CallResult.Ok -> result.value
            }
        }
    }
    fun install(frame: FfiNativeRenderFrame): String? {
        val update = adopt(frame, stripViewSelection, mirrorSelection == null) ?: return null
        return if (controlledPropSnapshot) frame.snapshotJson else update
    }
    val frame = fetch() ?: return null
    install(frame)?.let { return it }
    if (frame.tables.kind == FfiTableFrameKind.DELTA) {
        nativeOwnerId?.let { backend.releaseNativeBinding(editorId, it) }
        val full = fetch() ?: return null
        if (full.tables.kind == FfiTableFrameKind.FULL) install(full)?.let { return it }
    }
    emit(EditorV2Adapter.contractError("native table frame violates the frozen shape"))
    return null
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
