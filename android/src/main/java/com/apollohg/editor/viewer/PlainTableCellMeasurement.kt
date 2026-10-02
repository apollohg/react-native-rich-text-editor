package com.apollohg.editor.viewer

import android.text.StaticLayout
import com.apollohg.editor.tables.PreparedTableCellContent
import com.apollohg.editor.tables.TableAccessibility

internal data class PlainTableCellMeasurement(
    val key: ProseLayoutKey,
    val text: String,
    val prepare: () -> PreparedProseLayout
)

internal class PendingPlainTableCellMeasurement(
    val input: PlainTableCellMeasurement,
    val measure: (List<PlainTableCellMeasurement>) -> List<PreparedTableCellContent.MeasuredPlain>
) {
    val retainedBytes: Long get() = METADATA_BYTES + input.text.length * 2L

    companion object {
        // Charge the shared plain paint and shaper to every remaining owner.
        private const val METADATA_BYTES = 256L
    }
}

internal object PlainTableCellMeasurer {
    const val MAXIMUM_BATCH_CELLS = 256
    private const val MAXIMUM_BATCH_UTF16_UNITS = 64 * 1024
    const val EMPTY_TEXT = "\u200B"

    fun supports(text: String): Boolean = text.length <= MAXIMUM_BATCH_UTF16_UNITS

    fun measure(
        cells: List<PlainTableCellMeasurement>,
        layout: (String, Int) -> StaticLayout
    ): List<PreparedTableCellContent.MeasuredPlain> {
        val result = ArrayList<PreparedTableCellContent.MeasuredPlain>(cells.size)
        var start = 0
        while (start < cells.size) {
            val width = cells[start].key.widthPx
            var end = start
            var units = 0
            while (end < cells.size && end - start < MAXIMUM_BATCH_CELLS && cells[end].key.widthPx == width) {
                val additional = cells[end].text.length + if (end == start) 0 else 1
                if (additional > MAXIMUM_BATCH_UTF16_UNITS - units) break
                units += additional
                end++
            }
            check(end > start)
            val offsets = IntArray(end - start)
            val joined = buildString(units) {
                for (index in start until end) {
                    if (index > start) append('\n')
                    offsets[index - start] = length
                    append(cells[index].text)
                }
            }
            val measured = layout(joined, width)
            for (index in start until end) {
                val cell = cells[index]
                val firstLine = measured.getLineForOffset(offsets[index - start])
                val endLine = if (index + 1 == end) measured.lineCount else measured.getLineForOffset(offsets[index + 1 - start])
                result += PreparedTableCellContent.MeasuredPlain(cell.key, width,
                    maxOf(1, measured.getLineTop(endLine) - measured.getLineTop(firstLine)),
                    TableAccessibility.plainText(cell.text), cell.prepare)
            }
            start = end
        }
        return result
    }
}
