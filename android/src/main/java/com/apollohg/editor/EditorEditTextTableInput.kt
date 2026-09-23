package com.apollohg.editor

import com.apollohg.editor.tables.TableCellPositionMap

internal fun EditorEditText.isAuthorizedForTableCellInput(): Boolean {
    if (!isTableCellInput) return true
    val map = tableCellPositionMap ?: return false
    val adapter = EditorV2Registry.adapterForViewToken(editorId) ?: return false
    return v2Driver === adapter &&
        map.isCurrent(adapter.baseDocumentRevision.toString(), adapter.positionEpoch ?: "") &&
        tableCellInputAuthority?.invoke() == true
}

internal fun EditorEditText.canDispatchTableCellMutation(): Boolean =
    (!isTableCellInput && !rootTableSelectionInputBlocked && isAuthorizedForRootTableInput()) ||
        (isTableCellInput && tableCellUpdateConsumer != null && isAuthorizedForTableCellInput())

internal fun EditorEditText.hasAuthorizedNativeTableOwner(adapter: EditorV2Adapter): Boolean =
    rootTableNativeOwnerAuthority?.invoke(adapter) ?: ownsNativeBinding(adapter)

internal fun EditorEditText.isAuthorizedForRootTableInput(): Boolean {
    if (rootTablePositionMap == null) return true
    val adapter = v2Driver as? EditorV2Adapter ?: return false
    if (rootTableHasUnmappedExtent || !hasAuthorizedNativeTableOwner(adapter)) return false
    if (rootTableMapDocumentVersion != adapter.baseDocumentRevision.toString()) return false
    val currentEpoch = adapter.positionEpoch ?: return false
    if (adapter.cachedAtomicRenderDocumentRevision != adapter.baseDocumentRevision) return false
    val mappings = adapter.cachedTableInputMappings ?: return false
    val currentRootIds = adapter.cachedTableRecords.filterValues {
        !it.optBoolean("readOnlyDescendants", true)
    }.keys
    if (currentRootIds != rootTableMapTableIds ||
        currentRootIds.any { mappings.tables[it]?.extent != rootTableMapExtents[it] }
    ) return false
    rootTableMapPositionEpoch = currentEpoch
    return true
}

internal fun EditorEditText.inputScalar(localScalar: Int): Int? {
    if (!isTableCellInput) {
        if (rootTableRenderNeedsRefresh || rootTableSelectionInputBlocked) return null
        val map = rootTablePositionMap ?: return localScalar
        if (!isAuthorizedForRootTableInput()) return null
        return map.globalScalar(localScalar)
    }
    if (!isAuthorizedForTableCellInput()) return null
    return tableCellPositionMap?.globalScalarForLocalScalar(localScalar)
}

internal fun EditorEditText.inputScalarRange(fromLocal: Int, toLocal: Int): Pair<Int, Int>? {
    if (fromLocal > toLocal) return null
    if (!isTableCellInput) {
        if (rootTableRenderNeedsRefresh || rootTableSelectionInputBlocked) return null
        val map = rootTablePositionMap ?: return fromLocal to toLocal
        if (!isAuthorizedForRootTableInput()) return null
        return map.globalRange(fromLocal, toLocal)
    }
    if (!isAuthorizedForTableCellInput()) return null
    val mapped = tableCellPositionMap?.globalScalarRange(fromLocal, toLocal) ?: return null
    return mapped.from to mapped.to
}

internal fun EditorEditText.inputScalarAtLocalUtf16(offset: Int, text: String): Int? =
    if (offset in 0..text.length) inputScalar(PositionBridge.utf16ToScalar(offset, text)) else null

internal fun EditorEditText.inputScalarRangeAtLocalUtf16(
    start: Int,
    end: Int,
    text: String
): Pair<Int, Int>? = if (start >= 0 && start <= end && end <= text.length) {
    inputScalarRange(
        PositionBridge.utf16ToScalar(start, text),
        PositionBridge.utf16ToScalar(end, text)
    )
} else null

internal fun EditorEditText.inputScalarSelection(anchor: Int, head: Int): Pair<Int, Int>? {
    val range = inputScalarRange(minOf(anchor, head), maxOf(anchor, head)) ?: return null
    return if (anchor <= head) range else range.second to range.first
}

internal fun EditorEditText.localScalarSelection(anchor: Int, head: Int): Pair<Int, Int>? {
    if (!isTableCellInput) {
        if (rootTableRenderNeedsRefresh) return null
        val map = rootTablePositionMap ?: return anchor to head
        if (!isAuthorizedForRootTableInput()) return null
        val localAnchor = map.localScalar(anchor) ?: return null
        val localHead = map.localScalar(head) ?: return null
        val range = map.globalRange(minOf(localAnchor, localHead), maxOf(localAnchor, localHead))
            ?: return null
        val roundTrip = if (localAnchor <= localHead) range else range.second to range.first
        return (localAnchor to localHead).takeIf { roundTrip == (anchor to head) }
    }
    if (!isAuthorizedForTableCellInput()) return null
    val map = tableCellPositionMap ?: return null
    val localAnchor = map.localScalarForGlobalScalar(anchor) ?: return null
    val localHead = map.localScalarForGlobalScalar(head) ?: return null
    val roundTrip = inputScalarSelection(localAnchor, localHead) ?: return null
    return if (roundTrip == (anchor to head)) localAnchor to localHead else null
}

internal fun EditorEditText.canDeleteBackwardAtLocalSelection(anchor: Int, head: Int): Boolean =
    anchor != head || rootTablePositionMap?.isImmediatelyAfterTable(anchor) != true
