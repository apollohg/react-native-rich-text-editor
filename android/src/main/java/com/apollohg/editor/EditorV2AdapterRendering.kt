package com.apollohg.editor

import com.apollohg.editor.tables.EditorTableIndex
import com.apollohg.editor.tables.EditorTablePresentationSnapshot
import com.apollohg.editor.tables.TableFrameAdoption
import com.apollohg.editor.tables.resolveEditorCellSelection
import com.apollohg.editor.viewer.PreparedProseInstrumentation
import java.lang.ref.WeakReference
import org.json.JSONArray
import org.json.JSONObject
import uniffi.editor_core.FfiNativeRenderFrame
import uniffi.editor_core.FfiTableFrameKind

private fun EditorTableIndex.admitsScalarLength(scalarLength: Int): Boolean =
    rootExtents.values.none { it.scalarEnd.toLong() > scalarLength }

private fun JSONObject?.resolvesCellSelection(index: EditorTableIndex): Boolean =
    this?.opt("type") != "cell" || resolveEditorCellSelection(this, index) != null

private fun EditorV2Adapter.adoptRenderState(
    snapshot: AtomicRenderSnapshot,
    updateObject: JSONObject,
    updateJson: String,
    atomicRenderJson: String,
    engineOwnedSelection: Boolean
) {
    stateRevision = snapshot.stateRevision
    cachedScalarLength = snapshot.scalarLength
    cachedAuthoritativeScalarSelection = snapshot.scalarSelection?.copyOf()
    lastSyncedScalarSelection =
        if (engineOwnedSelection) snapshot.scalarSelection?.copyOf() else null
    cachedActiveState = snapshot.activeState
    cachedHistoryState = snapshot.historyState
    cachedViewUpdateJson = updateJson
    cachedViewUpdateObject = WeakReference(updateObject)
    cachedAtomicRenderJson = atomicRenderJson
    cachedAtomicRenderSelectionObject = snapshot.renderObject.optJSONObject("selection")
    cachedAtomicRenderDocumentRevision = snapshot.documentRevision
}

internal fun EditorV2Adapter.adoptSelectionState(selectionState: JSONObject): String? {
    val revision = installedFrameRevision ?: return null
    val retained = cachedSemanticBlockTableIds ?: return null
    if (destroyed || baseDocumentRevision != revision ||
        cachedSemanticBlockTableIdsRevision != revision
    ) {
        return null
    }
    val frame = selectionStateRenderFrame(selectionState, revision, retained.size) ?: return null
    val snapshot = frame.snapshot
    if (snapshot.documentRevision != revision ||
        !tableIndex.admitsScalarLength(snapshot.scalarLength) ||
        !snapshot.renderObject.optJSONObject("selection").resolvesCellSelection(tableIndex)
    ) {
        return null
    }
    adoptRenderState(
        snapshot,
        snapshot.renderObject,
        snapshot.viewUpdateJson,
        frame.atomicRenderJson,
        engineOwnedSelection = true
    )
    return snapshot.viewUpdateJson
}

private fun EditorV2Adapter.adopt(
    frame: FfiNativeRenderFrame,
    stripViewSelection: Boolean,
    engineOwnedSelection: Boolean
): String? {
    return PreparedProseInstrumentation.measureTableStage(
        PreparedProseInstrumentation.TableStage.ADAPTER_ADOPTION
    ) {
        val snapshot = parseAtomicRenderSnapshot(frame.snapshotJson) ?: return null
        val framePositionEpoch = frame.positionEpoch?.let { canonicalV2U64(it) ?: return null }
        val nextIndex = tableIndex.copy()
        val adoption =
            nextIndex.adopt(frame.tables, installedFrameRevision, snapshot.documentRevision)
                as? TableFrameAdoption.Adopted ?: return null
        if (!nextIndex.admitsScalarLength(snapshot.scalarLength)) return null
        fun tableIds(value: JSONArray): List<List<String>>? = (0 until value.length()).map {
            semanticBlockTableIds(value.getJSONArray(it)) ?: return null
        }
        val fullBlocks = snapshot.renderObject.optJSONArray("renderBlocks")
        val unvalidatedBlocks: JSONArray
        val candidate: List<List<String>>
        if (fullBlocks != null) {
            unvalidatedBlocks = fullBlocks
            candidate = tableIds(fullBlocks) ?: return null
        } else {
            val patch = snapshot.renderObject.optJSONObject("renderPatch") ?: return null
            val retained = cachedSemanticBlockTableIds ?: return null
            val start = patch.optLong("startIndex", -1)
            val delete = patch.optLong("deleteCount", -1)
            if (patch.optString("baseDocumentVersion").toULongOrNull() !=
                cachedSemanticBlockTableIdsRevision ||
                start < 0 || delete < 0 || start + delete > retained.size
            ) {
                return null
            }
            unvalidatedBlocks = patch.getJSONArray("renderBlocks")
            val inserted = tableIds(unvalidatedBlocks) ?: return null
            candidate = if (delete == 0L && inserted.isEmpty()) {
                retained
            } else {
                retained.take(start.toInt()) + inserted + retained.drop((start + delete).toInt())
            }
        }
        val unchangedSemanticFrame = candidate === cachedSemanticBlockTableIds &&
            snapshot.documentRevision == cachedSemanticBlockTableIdsRevision &&
            snapshot.documentRevision == installedFrameRevision &&
            frame.tables.kind == FfiTableFrameKind.DELTA &&
            frame.tables.attributes.isEmpty() && frame.tables.removedAttributeKeys.isEmpty() &&
            frame.tables.tables.isEmpty() && frame.tables.removedTableKeys.isEmpty() &&
            frame.tables.cellUpdates.isEmpty() && frame.tables.extents.isEmpty()
        if (!unchangedSemanticFrame) {
            val insertedContent = (0 until unvalidatedBlocks.length()).flatMap { blockIndex ->
                val block = unvalidatedBlocks.getJSONArray(blockIndex)
                (0 until block.length()).map(block::opt).filterNot {
                    (it as? JSONObject)?.opt("type") == "table"
                }
            }
            val roots = candidate.flatten()
            val uniqueRoots = roots.toSet()
            if (!validSemanticRenderElements(insertedContent) ||
                uniqueRoots.size != roots.size ||
                roots.any { nextIndex.record(it) == null } ||
                uniqueRoots != nextIndex.rootExtents.keys
            ) {
                return null
            }
        }
        if (!snapshot.renderObject.optJSONObject("selection").resolvesCellSelection(nextIndex)) {
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
            framePositionEpoch,
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
        adoptRenderState(
            snapshot,
            updateObject,
            updateJson,
            frame.snapshotJson,
            engineOwnedSelection
        )
        cachedSemanticBlockTableIds = candidate
        cachedSemanticBlockTableIdsRevision = snapshot.documentRevision
        framePositionEpoch?.let { positionEpoch = it }
        updateJson
    }
}

internal fun EditorV2Adapter?.readOnlyParsedUpdate(updateJson: String): JSONObject =
    this?.cachedViewUpdateObject?.get()?.takeIf { updateJson === cachedViewUpdateJson }
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
