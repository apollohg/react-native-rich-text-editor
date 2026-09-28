package com.apollohg.editor

import android.text.Layout
import android.text.StaticLayout
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import com.apollohg.editor.tables.TableStyle
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
        fun document(text: String) = ViewerDocument(text, listOf(ViewerBlock("paragraph", 0, false,
            null, null, listOf(ViewerInline.Text(text, emptyList())))), false, 0)
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
            for (index in texts.indices) assertEquals("run $run cell ${texts[index]} at width $width",
                preparedHeights[index], measuredHeights[index])
            val edits = changed.indices.map { index ->
                measureNanoTime {
                    changedHeightChecksum += engine.prepare(changed[index], changedKeys[index], theme, width, density, false).heightPx
                } / NANOS_PER_MILLISECOND
            }
            if (run >= WARMUP_RUNS) {
                prepareSamples += prepareNanos / NANOS_PER_MICROSECOND / CELL_COUNT
                measureSamples += measureNanos / NANOS_PER_MICROSECOND / CELL_COUNT
                changedSamples += median(edits)
            }
        }
        println("TABLE_CELL_BENCHMARK platform=android build=${BuildConfig.BUILD_TYPE} density=$density width=$width cells=$CELL_COUNT " +
            "prepareUs=${median(prepareSamples)} measureUs=${median(measureSamples)} changedMs=${median(changedSamples)} " +
            "prepareRuns=$prepareSamples measureRuns=$measureSamples changedRuns=$changedSamples changedHeightChecksum=$changedHeightChecksum")
    }

    private fun median(samples: List<Double>): Double = samples.sorted().let {
        if (it.size % 2 == 0) (it[it.size / 2 - 1] + it[it.size / 2]) / 2 else it[it.size / 2]
    }
}
