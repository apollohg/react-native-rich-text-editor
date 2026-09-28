package com.apollohg.editor

import android.graphics.text.LineBreaker
import android.graphics.text.MeasuredText
import android.text.Layout
import android.text.StaticLayout
import android.text.TextPaint
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.filters.SdkSuppress
import androidx.test.platform.app.InstrumentationRegistry
import com.apollohg.editor.tables.TableStyle
import com.apollohg.editor.tables.TableLayoutDirection
import com.apollohg.editor.viewer.PreparedProseTheme
import com.apollohg.editor.viewer.ProseLayoutKey
import com.apollohg.editor.viewer.StaticLayoutAndroidProseLayoutEngine
import com.apollohg.editor.viewer.ViewerBlock
import com.apollohg.editor.viewer.ViewerDocument
import com.apollohg.editor.viewer.ViewerInline
import org.junit.Assert.assertEquals
import org.junit.Test
import org.junit.runner.RunWith
import java.util.Locale
import kotlin.math.roundToInt
import kotlin.system.measureNanoTime

@RunWith(AndroidJUnit4::class)
@SdkSuppress(minSdkVersion = 29)
class TableCellPreparationBenchmarkTest {
    private companion object {
        const val ROWS = 1_000
        const val COLUMNS = 20
        const val CELL_COUNT = ROWS * COLUMNS
        const val WARMUP_RUNS = 1
        const val MEASURED_RUNS = 5
        const val CHANGED_CELL_EDITS = 200
        const val NANOS_PER_MICROSECOND = 1_000.0
        const val NANOS_PER_MILLISECOND = 1_000_000.0
        const val PROFILE_DENSITY = 3f
        const val PROFILE_WIDTH_PX = 186
        const val PROFILE_LINES_PER_CELL = 2
        const val TABLE_MEASUREMENT_BATCH_CELLS = 256
        const val SEMANTIC_GENERATION = "table-cell-preparation-benchmark"
    }

    @Test
    fun benchmarkPrepareDiscardMeasureAndWarmChangedCell() {
        val density = InstrumentationRegistry.getInstrumentation().targetContext.resources.displayMetrics.density
        val style = TableStyle()
        val width = ((style.minColumnWidth - 2f * (style.cellPadding + style.borderWidth)) * density).roundToInt()
        val theme = PreparedProseTheme.resolve(null, density).copy(
            insetTopPx = 0, insetBottomPx = 0, insetLeftPx = 0, insetRightPx = 0)
        val texts = List(CELL_COUNT) { index -> String.format(Locale.ROOT, "R%04dC%04dXY", index / COLUMNS, index % COLUMNS) }
        val documents = texts.map(::document)
        val changed = List(CHANGED_CELL_EDITS) { document(texts[CELL_COUNT / 2] + "x".repeat(it + 1)) }
        fun key(document: ViewerDocument) = ProseLayoutKey(document.semanticKey, width, SEMANTIC_GENERATION,
            0, 0, density.toBits().toLong(), 0, SEMANTIC_GENERATION)
        val keys = documents.map(::key)
        val changedKeys = changed.map(::key)
        val engine = StaticLayoutAndroidProseLayoutEngine()
        val prepareSamples = mutableListOf<Double>()
        val measureSamples = mutableListOf<Double>()
        val changedSamples = mutableListOf<Double>()
        val batchSamples = mutableListOf<Double>()
        val directSamples = mutableListOf<Double>()
        var changedHeightChecksum = 0L
        repeat(WARMUP_RUNS + MEASURED_RUNS) { run ->
            val preparedHeights = IntArray(CELL_COUNT)
            val measuredHeights = IntArray(CELL_COUNT)
            val prepareNanos = measureNanoTime {
                for (index in documents.indices) preparedHeights[index] = engine.prepare(
                    documents[index], keys[index], theme, width, density, false).heightPx
            }
            val measureNanos = measureNanoTime {
                for (index in texts.indices) measuredHeights[index] = StaticLayout.Builder.obtain(
                    texts[index], 0, texts[index].length, theme.paragraph.newTextPaint(), width)
                    .setAlignment(Layout.Alignment.ALIGN_NORMAL).setIncludePad(false)
                    .setBreakStrategy(Layout.BREAK_STRATEGY_HIGH_QUALITY)
                    .setHyphenationFrequency(Layout.HYPHENATION_FREQUENCY_NONE).build().height
            }
            val batchHeights = IntArray(CELL_COUNT)
            val batchNanos = measureNanoTime {
                texts.chunked(TABLE_MEASUREMENT_BATCH_CELLS).forEachIndexed { batchIndex, batch ->
                    val measurement = measureBatch(batch, theme.paragraph.newTextPaint(), width, collectLineEnds = false)
                    measurement.heights.copyInto(batchHeights, batchIndex * TABLE_MEASUREMENT_BATCH_CELLS)
                }
            }
            val directHeights = IntArray(CELL_COUNT)
            val directNanos = measureNanoTime {
                texts.chunked(TABLE_MEASUREMENT_BATCH_CELLS).forEachIndexed { batchIndex, batch ->
                    val measurement = measureDirect(batch, theme.paragraph.newTextPaint(), width, collectLineEnds = false)
                    measurement.heights.copyInto(directHeights, batchIndex * TABLE_MEASUREMENT_BATCH_CELLS)
                }
            }
            for (index in texts.indices) assertEquals("direct run $run cell ${texts[index]} at width $width",
                preparedHeights[index], directHeights[index])
            for (index in texts.indices) assertEquals("batch run $run cell ${texts[index]} at width $width",
                preparedHeights[index], batchHeights[index])
            for (index in texts.indices) assertEquals("run $run cell ${texts[index]} at width $width",
                preparedHeights[index], measuredHeights[index])
            val edits = changed.indices.map { index ->
                measureNanoTime {
                    changedHeightChecksum += engine.prepare(changed[index], changedKeys[index], theme, width, density, false).heightPx
                } / NANOS_PER_MILLISECOND
            }
            if (run >= WARMUP_RUNS) {
                directSamples += directNanos / NANOS_PER_MICROSECOND / CELL_COUNT
                batchSamples += batchNanos / NANOS_PER_MICROSECOND / CELL_COUNT
                prepareSamples += prepareNanos / NANOS_PER_MICROSECOND / CELL_COUNT
                measureSamples += measureNanos / NANOS_PER_MICROSECOND / CELL_COUNT
                changedSamples += median(edits)
            }
        }
        println("TABLE_CELL_BENCHMARK platform=android build=${BuildConfig.BUILD_TYPE} density=$density width=$width cells=$CELL_COUNT " +
            "directUs=${median(directSamples)} directRuns=$directSamples batchUs=${median(batchSamples)} batchRuns=$batchSamples prepareUs=${median(prepareSamples)} measureUs=${median(measureSamples)} changedMs=${median(changedSamples)} " +
            "prepareRuns=$prepareSamples measureRuns=$measureSamples changedRuns=$changedSamples changedHeightChecksum=$changedHeightChecksum")
    }

    @Test
    fun batchedParagraphsPreserveIndependentCellLineBreaksAndHeights() {
        val texts = listOf("", "a", " ", "  ", "word ", " word", "word  word", "a-b/c.d, e! f?",
            "R0001C0001XY", "x".repeat(200), "short", "last  ")
        val engine = StaticLayoutAndroidProseLayoutEngine()
        for (density in listOf(1f, 1.5f, 3f)) {
            for (fontScale in listOf(1f, 1.3f, 2f)) {
                val theme = PreparedProseTheme.resolve(null, density, fontScale)
                for (width in listOf(31, 62, 186)) {
                    val batch = measureBatch(texts, theme.paragraph.newTextPaint(), width)
                    val direct = measureDirect(texts, theme.paragraph.newTextPaint(), width)
                    texts.forEachIndexed { index, text ->
                        val separate = StaticLayout.Builder.obtain(text, 0, text.length,
                            theme.paragraph.newTextPaint(), width)
                            .setIncludePad(false).setBreakStrategy(Layout.BREAK_STRATEGY_HIGH_QUALITY)
                            .setHyphenationFrequency(Layout.HYPHENATION_FREQUENCY_NONE).build()
                        val context = "text=<$text> density=$density fontScale=$fontScale width=$width"
                        for (direction in TableLayoutDirection.entries) {
                            val cellTheme = theme.copy(insetTopPx = 0, insetBottomPx = 0,
                                insetLeftPx = 0, insetRightPx = 0, tableDirection = direction)
                            val cell = document(text)
                            val key = ProseLayoutKey(cell.semanticKey, width, SEMANTIC_GENERATION,
                                0, 0, density.toBits().toLong(), 0, SEMANTIC_GENERATION)
                            val prepared = engine.prepare(cell, key, cellTheme, width, density, false)
                            assertEquals("engine height $context direction=$direction", prepared.heightPx, batch.heights[index])
                            val engineLines = prepared.blocks.flatMap { it.fragments }.mapNotNull { it.layout }
                                .flatMap { layout -> (0 until layout.lineCount).map(layout::getLineEnd) }
                            if (text.isNotEmpty()) {
                                assertEquals("engine lines $context direction=$direction", engineLines, batch.lineEnds[index])
                            }
                        }
                        assertEquals("direct height $context", separate.height, direct.heights[index])
                        assertEquals("direct line ends $context", (0 until separate.lineCount).map(separate::getLineEnd),
                            direct.lineEnds[index])
                        assertEquals("height $context", separate.height, batch.heights[index])
                        assertEquals("line ends $context", (0 until separate.lineCount).map(separate::getLineEnd),
                            batch.lineEnds[index])
                    }
                }
            }
        }
    }

    @Test
    fun profileDirectMeasurementStages() {
        val paint = PreparedProseTheme.resolve(null, PROFILE_DENSITY).paragraph.newTextPaint()
        val width = PROFILE_WIDTH_PX
        val texts = List(CELL_COUNT) { index -> String.format(Locale.ROOT, "R%04dC%04dXY", index / COLUMNS, index % COLUMNS).toCharArray() }
        val breaker = LineBreaker.Builder().setBreakStrategy(LineBreaker.BREAK_STRATEGY_HIGH_QUALITY)
            .setHyphenationFrequency(LineBreaker.HYPHENATION_FREQUENCY_NONE).build()
        val constraints = LineBreaker.ParagraphConstraints().apply {
            setWidth(width.toFloat())
            setIndent(width.toFloat(), 1)
        }
        val shapeSamples = mutableListOf<Double>()
        val breakSamples = mutableListOf<Double>()
        var checksum = 0L
        repeat(WARMUP_RUNS + MEASURED_RUNS) { run ->
            val measured = arrayOfNulls<MeasuredText>(CELL_COUNT)
            val shapeNanos = measureNanoTime {
                texts.forEachIndexed { index, text ->
                    measured[index] = MeasuredText.Builder(text).setComputeHyphenation(false)
                        .setComputeLayout(false).appendStyleRun(paint, text.size, false).build()
                }
            }
            val breakNanos = measureNanoTime {
                measured.forEach { text -> checksum += breaker.computeLineBreaks(requireNotNull(text), constraints, 0).lineCount }
            }
            if (run >= WARMUP_RUNS) {
                shapeSamples += shapeNanos / NANOS_PER_MICROSECOND / CELL_COUNT
                breakSamples += breakNanos / NANOS_PER_MICROSECOND / CELL_COUNT
            }
        }
        println("TABLE_CELL_STAGE_PROFILE shapeUs=${median(shapeSamples)} breakUs=${median(breakSamples)} " +
            "shapeRuns=$shapeSamples breakRuns=$breakSamples checksum=$checksum")
        assertEquals((CELL_COUNT * (WARMUP_RUNS + MEASURED_RUNS) * PROFILE_LINES_PER_CELL).toLong(), checksum)
    }

    private fun document(text: String) = ViewerDocument(text, listOf(ViewerBlock("paragraph", 0, false,
        null, null, listOf(ViewerInline.Text(text, emptyList())))), false, 0)

    private data class BatchMeasurement(val heights: IntArray, val lineEnds: List<List<Int>>)

    private fun measureDirect(texts: List<String>, paint: TextPaint, width: Int,
                              collectLineEnds: Boolean = true): BatchMeasurement {
        require(texts.size <= TABLE_MEASUREMENT_BATCH_CELLS)
        val breaker = LineBreaker.Builder().setBreakStrategy(LineBreaker.BREAK_STRATEGY_HIGH_QUALITY)
            .setHyphenationFrequency(LineBreaker.HYPHENATION_FREQUENCY_NONE).build()
        val constraints = LineBreaker.ParagraphConstraints().apply {
            setWidth(width.toFloat())
            setIndent(width.toFloat(), 1)
        }
        val metrics = paint.fontMetricsInt
        val lineHeight = metrics.descent - metrics.ascent
        val heights = IntArray(texts.size)
        val ends = texts.mapIndexed { index, text ->
            if (text.isEmpty()) {
                heights[index] = lineHeight
                if (collectLineEnds) listOf(0) else emptyList()
            } else {
                val measured = MeasuredText.Builder(text.toCharArray()).setComputeHyphenation(false)
                    .setComputeLayout(false).appendStyleRun(paint, text.length, false).build()
                val result = breaker.computeLineBreaks(measured, constraints, 0)
                heights[index] = result.lineCount * lineHeight
                if (collectLineEnds) (0 until result.lineCount).map(result::getLineBreakOffset) else emptyList()
            }
        }
        return BatchMeasurement(heights, ends)
    }

    private fun measureBatch(texts: List<String>, paint: TextPaint, width: Int, collectLineEnds: Boolean = true): BatchMeasurement {
        require(texts.size <= TABLE_MEASUREMENT_BATCH_CELLS)
        val starts = IntArray(texts.size)
        val joined = buildString {
            texts.forEachIndexed { index, text ->
                if (index > 0) append('\n')
                starts[index] = length
                append(text)
            }
        }
        val layout = StaticLayout.Builder.obtain(joined, 0, joined.length, paint, width)
            .setIncludePad(false).setBreakStrategy(Layout.BREAK_STRATEGY_HIGH_QUALITY)
            .setHyphenationFrequency(Layout.HYPHENATION_FREQUENCY_NONE).build()
        val heights = IntArray(texts.size)
        val lineEnds = texts.mapIndexed { index, text ->
            val firstLine = layout.getLineForOffset(starts[index])
            val endLine = if (index == texts.lastIndex) layout.lineCount else layout.getLineForOffset(starts[index + 1])
            heights[index] = layout.getLineTop(endLine) - layout.getLineTop(firstLine)
            if (collectLineEnds) (firstLine until endLine).map { line ->
                minOf(text.length, layout.getLineEnd(line) - starts[index])
            } else emptyList()
        }
        return BatchMeasurement(heights, lineEnds)
    }

    private fun median(samples: List<Double>): Double = samples.sorted().let {
        if (it.size % 2 == 0) (it[it.size / 2 - 1] + it[it.size / 2]) / 2 else it[it.size / 2]
    }
}
