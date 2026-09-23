package com.apollohg.editor.tables

import android.text.SpannableStringBuilder
import com.apollohg.editor.AtomRenderConfiguration
import com.apollohg.editor.EditorTheme
import com.apollohg.editor.PositionBridge
import com.apollohg.editor.RenderBridge
import com.apollohg.editor.TableInputBlock
import com.apollohg.editor.TableInputTable
import com.apollohg.editor.canonicalV2U64
import com.apollohg.editor.exactV2ScalarInt
import org.json.JSONObject

internal object EditorTableCellProjection {
    data class Projection(
        val target: EditorTableInputCoordinator.Target,
        val text: SpannableStringBuilder,
        val positionMap: TableCellPositionMap
    )

    fun project(
        cellIndex: Int,
        table: JSONObject,
        mapping: TableInputTable,
        documentRevision: String,
        positionEpoch: String,
        baseFontSize: Float,
        textColor: Int,
        theme: EditorTheme? = null,
        density: Float = 1f,
        atomConfiguration: AtomRenderConfiguration? = null
    ): Projection? {
        if (cellIndex < 0 || canonicalV2U64(documentRevision) == null ||
            canonicalV2U64(positionEpoch) == null ||
            table.opt("readOnlyDescendants") != false
        ) return null
        val rawCell = table.optJSONArray("cells")?.optJSONObject(cellIndex) ?: return null
        val inputCell = mapping.cells.getOrNull(cellIndex) ?: return null
        val sourcePos = exactV2ScalarInt(rawCell.opt("sourcePos") as? Number) ?: return null
        val sourceEnd = exactV2ScalarInt(rawCell.opt("sourceEnd") as? Number) ?: return null
        val elements = rawCell.optJSONArray("elements") ?: return null
        if (inputCell.cellIndex != cellIndex || inputCell.sourcePos != sourcePos ||
            inputCell.sourceEnd != sourceEnd || sourcePos >= sourceEnd ||
            inputCell.excluded.isNotEmpty() || inputCell.blocks.isEmpty()
        ) return null

        val binding = TableCellPositionMap.Binding(sourcePos.toLong(), documentRevision, positionEpoch)
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
        if (ranges.keys != inputCell.blocks.map { it.elementIndex }.toSet()) return null
        val text = rendered.toString()
        val segments = mutableListOf<TableCellPositionMap.Segment>()
        var previousBlock: TableInputBlock? = null
        var previousLocalEnd = 0
        for (block in inputCell.blocks) {
            val range = ranges[block.elementIndex] ?: return null
            if (range.first < 0 || range.second < range.first || range.second > text.length ||
                block.scalarStart < 0 || block.contentScalarStart < block.scalarStart ||
                block.scalarEnd < block.contentScalarStart || block.elementIndex < 0
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
            segments.add(TableCellPositionMap.Segment(localStart.toInt(), localEndExclusive.toInt(), block.scalarStart))
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
