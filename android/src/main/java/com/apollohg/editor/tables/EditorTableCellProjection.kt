package com.apollohg.editor.tables

import android.text.SpannableStringBuilder
import com.apollohg.editor.AtomRenderConfiguration
import com.apollohg.editor.EditorTheme
import com.apollohg.editor.PositionBridge
import com.apollohg.editor.RenderBridge
import uniffi.editor_core.FfiCellInputBlock
import com.apollohg.editor.canonicalV2U64

internal object EditorTableCellProjection {
    data class Projection(
        val target: EditorTableInputCoordinator.Target,
        val text: SpannableStringBuilder,
        val positionMap: TableCellPositionMap
    )

    fun project(
        cellIndex: Int,
        tableKey: String,
        index: EditorTableIndex,
        documentRevision: String,
        positionEpoch: String,
        baseFontSize: Float,
        textColor: Int,
        theme: EditorTheme? = null,
        density: Float = 1f,
        atomConfiguration: AtomRenderConfiguration? = null
    ): Projection? {
        if (cellIndex < 0 || canonicalV2U64(documentRevision) == null || canonicalV2U64(positionEpoch) == null) return null
        val table = index.record(tableKey) ?: return null
        val cell = table.cells.getOrNull(cellIndex) ?: return null
        if (table.readOnlyDescendants || cell.nestedTables.isNotEmpty() || cell.inputBlocks.isEmpty()) return null
        val elements = inputElements(cell.elements, cell.voidElementIndices) { relative -> index.absoluteDocPos(tableKey, cellIndex, relative) }
            ?: return null
        val binding = TableCellPositionMap.Binding(tableKey, cellIndex, documentRevision, positionEpoch)
        val target = EditorTableInputCoordinator.Target(binding)
        if (!EditorTableInputCoordinator.canBind(target)) return null
        val ranges = mutableMapOf<Int, Pair<Int, Int>>()
        var duplicate = false
        val rendered = RenderBridge.buildSpannableFromArray(
            elements, baseFontSize, textColor, theme, density,
            atomConfiguration = atomConfiguration,
            blockRangeObserver = { index, start, end ->
                if (ranges.put(index, start to end) != null) duplicate = true
            },
            synthesizeEmptyBlocks = true,
            synthesizeTrailingHardBreakPlaceholders = false
        )
        if (duplicate) return null
        if (ranges.keys != cell.inputBlocks.map { it.elementIndex.toInt() }.toSet()) return null
        val text = rendered.toString()
        val segments = index.inputSegments(tableKey, cellIndex) ?: return null
        var previousBlock: FfiCellInputBlock? = null
        var previousLocalEnd = 0
        for ((blockIndex, block) in cell.inputBlocks.withIndex()) {
            val range = ranges[block.elementIndex.toInt()] ?: return null
            if (range.first < 0 || range.second < range.first || range.second > text.length ||
                block.contentScalarStart < block.scalarStart ||
                block.scalarEnd < block.contentScalarStart
            ) return null
            val start = PositionBridge.utf16ToScalar(range.first, text)
            val end = PositionBridge.utf16ToScalar(range.second, text)
            val prefix = block.contentScalarStart.toLong() - block.scalarStart.toLong()
            val localStart = start.toLong() - prefix
            val localEndExclusive = end.toLong() + 1L
            if (localStart < 0 || localEndExclusive > Int.MAX_VALUE ||
                end.toLong() - localStart != block.scalarEnd.toLong() - block.scalarStart.toLong()
            ) return null
            previousBlock?.let { previous ->
                val renderedBreak = localStart - previousLocalEnd.toLong()
                val mappedBreak = previous.breakScalarEnd.toLong() - previous.scalarEnd.toLong()
                if (renderedBreak != mappedBreak ||
                    block.scalarStart != previous.breakScalarEnd
                ) return null
            }
            val segment = segments[blockIndex]
            if (segment.localScalarStart.toLong() != localStart || segment.localScalarEndExclusive.toLong() != localEndExclusive) return null
            previousBlock = block
            previousLocalEnd = end
        }
        val positionMap = TableCellPositionMap(binding, segments)
        if (!positionMap.hasValidSegments()) return null
        val totalScalars = text.codePointCount(0, text.length)
        var covered = 0L
        for (segment in segments) {
            if (segment.localScalarStart.toLong() != covered) return null
            covered = segment.localScalarEndExclusive.toLong()
        }
        if (covered != totalScalars.toLong() + 1L) return null
        return Projection(target, rendered, positionMap)
    }
}
