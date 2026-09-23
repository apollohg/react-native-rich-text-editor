package com.apollohg.editor.tables

import com.apollohg.editor.EditorEditText

internal sealed interface TableInputPhase {
    data object Inactive : TableInputPhase
    data class Bound(val cellSourcePos: Long, val revision: String, val epoch: String) : TableInputPhase
    data class Composing(val cellSourcePos: Long, val revision: String, val epoch: String) : TableInputPhase
}

internal class EditorTableInputCoordinator(val cellInput: EditorEditText) {
    data class Target(
        val binding: TableCellPositionMap.Binding,
        val isSynthetic: Boolean = false,
        val isNestedTarget: Boolean = false,
        val hasExcludedContent: Boolean = false
    )

    var phase: TableInputPhase = TableInputPhase.Inactive
        private set
    var positionMap: TableCellPositionMap? = null
        private set
    val inputInstanceCountForTesting: Int get() = 1

    fun bind(
        target: Target,
        positionMap: TableCellPositionMap,
        currentRevision: String,
        currentEpoch: String,
        authority: (() -> Boolean)? = null,
        updateConsumer: ((String, Boolean, Boolean) -> Boolean)? = null
    ): Boolean {
        if (phase is TableInputPhase.Composing ||
            !canBind(target) || !positionMap.hasValidSegments() || positionMap.binding != target.binding ||
            !positionMap.isCurrent(currentRevision, currentEpoch)
        ) return false

        cellInput.discardTransientNativeInputForEditorRebind()
        cellInput.logicalSelectionSnapshot = null
        cellInput.authoritativeNodeSelectionRange = null
        this.positionMap = positionMap
        cellInput.isTableCellInput = true
        cellInput.tableCellPositionMap = positionMap
        cellInput.tableCellInputAuthority = authority
        cellInput.tableCellUpdateConsumer = updateConsumer
        phase = TableInputPhase.Bound(
            target.binding.cellSourcePos,
            target.binding.revision,
            target.binding.epoch
        )
        return true
    }

    fun beginComposition(): Boolean {
        val bound = phase as? TableInputPhase.Bound ?: return false
        phase = TableInputPhase.Composing(bound.cellSourcePos, bound.revision, bound.epoch)
        return true
    }

    fun refreshBinding(target: Target, map: TableCellPositionMap,
                       currentRevision: String, currentEpoch: String): Boolean {
        val active = phase
        val sourcePos = when (active) {
            is TableInputPhase.Bound -> active.cellSourcePos
            is TableInputPhase.Composing -> active.cellSourcePos
            TableInputPhase.Inactive -> return false
        }
        if (sourcePos != target.binding.cellSourcePos ||
            !canBind(target) || !map.hasValidSegments() || map.binding != target.binding ||
            !map.isCurrent(currentRevision, currentEpoch)) return false
        positionMap = map
        cellInput.tableCellPositionMap = map
        phase = if (active is TableInputPhase.Composing) {
            TableInputPhase.Composing(target.binding.cellSourcePos, currentRevision, currentEpoch)
        } else {
            TableInputPhase.Bound(target.binding.cellSourcePos, currentRevision, currentEpoch)
        }
        return true
    }

    fun invalidateBinding(): Boolean {
        if (phase == TableInputPhase.Inactive && positionMap == null) return false
        cellInput.discardTransientNativeInputForEditorRebind()
        cellInput.logicalSelectionSnapshot = null
        cellInput.authoritativeNodeSelectionRange = null
        positionMap = null
        cellInput.isTableCellInput = true
        cellInput.tableCellPositionMap = null
        cellInput.tableCellInputAuthority = null
        cellInput.tableCellUpdateConsumer = null
        phase = TableInputPhase.Inactive
        return true
    }

    companion object {
        fun canBind(target: Target): Boolean =
            !target.isSynthetic && !target.isNestedTarget && !target.hasExcludedContent
    }
}
