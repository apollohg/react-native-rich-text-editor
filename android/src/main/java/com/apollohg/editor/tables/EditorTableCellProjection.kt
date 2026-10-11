package com.apollohg.editor.tables

import android.graphics.Paint
import android.text.Annotation
import android.text.SpannableStringBuilder
import android.text.Spanned
import android.text.style.AbsoluteSizeSpan
import android.text.style.LeadingMarginSpan
import com.apollohg.editor.AtomRenderConfiguration
import com.apollohg.editor.EditorBlockBoxSpan
import com.apollohg.editor.EditorCheckboxSpan
import com.apollohg.editor.EditorResolvedTextSpan
import com.apollohg.editor.EditorTheme
import com.apollohg.editor.MarkerGapSpan
import com.apollohg.editor.PositionBridge
import com.apollohg.editor.RenderBridge
import com.apollohg.editor.canonicalV2U64
import com.apollohg.editor.defaultParagraphEnd
import com.apollohg.editor.viewer.PreparedProseFragment
import uniffi.editor_core.FfiCellInputBlock

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
        atomConfiguration: AtomRenderConfiguration? = null,
        taskMarkers: List<PreparedProseFragment> = emptyList()
    ): Projection? {
        if (cellIndex < 0 || canonicalV2U64(documentRevision) == null ||
            canonicalV2U64(positionEpoch) == null
        ) {
            return null
        }
        val table = index.record(tableKey) ?: return null
        val cell = table.cells.getOrNull(cellIndex) ?: return null
        if (table.readOnlyDescendants || cell.nestedTables.isNotEmpty() ||
            cell.inputBlocks.isEmpty()
        ) {
            return null
        }
        val elements =
            inputElements(cell.elements, cell.voidElementIndices) { relative ->
                index.absoluteDocPos(tableKey, cellIndex, relative)
            }
                ?: return null
        val binding = TableCellPositionMap.Binding(
            tableKey,
            cellIndex,
            documentRevision,
            positionEpoch
        )
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
        if (taskMarkers.isNotEmpty() && !applyTaskMarkerGeometry(rendered, taskMarkers)) return null
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
            ) {
                return null
            }
            val start = PositionBridge.utf16ToScalar(range.first, text)
            val end = PositionBridge.utf16ToScalar(range.second, text)
            val prefix = block.contentScalarStart.toLong() - block.scalarStart.toLong()
            val localStart = start.toLong() - prefix
            val localEndExclusive = end.toLong() + 1L
            if (localStart < 0 || localEndExclusive > Int.MAX_VALUE ||
                end.toLong() - localStart != block.scalarEnd.toLong() - block.scalarStart.toLong()
            ) {
                return null
            }
            previousBlock?.let { previous ->
                val renderedBreak = localStart - previousLocalEnd.toLong()
                val mappedBreak = previous.breakScalarEnd.toLong() - previous.scalarEnd.toLong()
                if (renderedBreak != mappedBreak ||
                    block.scalarStart != previous.breakScalarEnd
                ) {
                    return null
                }
            }
            val segment = segments[blockIndex]
            if (segment.localScalarStart.toLong() != localStart ||
                segment.localScalarEndExclusive.toLong() != localEndExclusive
            ) {
                return null
            }
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

    private fun applyTaskMarkerGeometry(
        text: SpannableStringBuilder,
        markers: List<PreparedProseFragment>
    ): Boolean {
        val annotations = text.getSpans(0, text.length, Annotation::class.java)
            .filter { it.key == RenderBridge.NATIVE_TASK_LIST_MARKER_ANNOTATION }
            .sortedBy(text::getSpanStart)
        if (annotations.size != markers.size) return false
        for ((index, annotation) in annotations.withIndex()) {
            val start = text.getSpanStart(annotation)
            val end = text.getSpanEnd(annotation)
            val bodyEnd = (end + 1).coerceAtMost(text.length)
            val bodyStyle = text.getSpans(end, bodyEnd, EditorResolvedTextSpan::class.java)
                .lastOrNull { text.getSpanStart(it) >= end }
            val bodySize = text.getSpans(end, bodyEnd, AbsoluteSizeSpan::class.java)
                .lastOrNull { text.getSpanStart(it) >= end }
            text.getSpans(start, end, Any::class.java).filter {
                (
                    it is AbsoluteSizeSpan ||
                        it is EditorResolvedTextSpan ||
                        it is EditorCheckboxSpan
                    ) &&
                    text.getSpanStart(it) >= start && text.getSpanEnd(it) <= end
            }.forEach(text::removeSpan)
            val metrics = bodyStyle?.let { EditorResolvedTextSpan(it.style, it.density) }
                ?: bodySize?.let { AbsoluteSizeSpan(it.size, it.dip) }
            if (metrics != null) text.setSpan(metrics, start, end, Spanned.SPAN_EXCLUSIVE_EXCLUSIVE)
            val marker = markers[index]
            text.setSpan(
                EditorCheckboxSpan(
                    marker.bounds.height(),
                    marker.box,
                    marker.checked,
                    marker.borderColor,
                    marker.color,
                    reserveLineHeight = false
                ),
                start,
                end - 1,
                Spanned.SPAN_EXCLUSIVE_EXCLUSIVE
            )
            val paragraphEnd = RenderBridge.defaultParagraphEnd(text.toString(), text.length, start)
            val margins = text.getSpans(start, paragraphEnd, LeadingMarginSpan::class.java)
            val boxes = margins.filterIsInstance<EditorBlockBoxSpan>()
                .sumOf { it.box.outerInset.left.toDouble() }.toInt()
            val preservedMargins = margins.filter {
                it !is EditorBlockBoxSpan && it !is LeadingMarginSpan.Standard
            }
            margins.filterIsInstance<LeadingMarginSpan.Standard>().forEach(text::removeSpan)
            val gap = text.getSpans(end - 1, end, MarkerGapSpan::class.java).singleOrNull()
                ?.getSize(Paint(), text, end - 1, end, null) ?: 0
            text.setSpan(
                LeadingMarginSpan.Standard(
                    (
                        marker.bounds.left - boxes -
                            preservedMargins.sumOf { it.getLeadingMargin(true) }
                        )
                        .coerceAtLeast(0),
                    (
                        marker.bounds.right + gap - boxes -
                            preservedMargins.sumOf { it.getLeadingMargin(false) }
                        )
                        .coerceAtLeast(0)
                ),
                start,
                paragraphEnd,
                Spanned.SPAN_PARAGRAPH
            )
        }
        return true
    }
}
