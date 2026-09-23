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
        currentEpoch: String
    ): Boolean {
        if (phase is TableInputPhase.Composing ||
            !canBind(target) || !positionMap.hasValidSegments() || positionMap.binding != target.binding ||
            !positionMap.isCurrent(currentRevision, currentEpoch)
        ) return false

        cellInput.discardTransientNativeInputForEditorRebind()
        this.positionMap = positionMap
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

    fun invalidateBinding(): Boolean {
        if (phase == TableInputPhase.Inactive && positionMap == null) return false
        cellInput.discardTransientNativeInputForEditorRebind()
        positionMap = null
        phase = TableInputPhase.Inactive
        return true
    }

    companion object {
        fun canBind(target: Target): Boolean =
            !target.isSynthetic && !target.isNestedTarget && !target.hasExcludedContent
    }
}
