package com.apollohg.editor.viewer

import android.graphics.Typeface
import android.text.Layout
import android.text.StaticLayout
import androidx.test.ext.junit.runners.AndroidJUnit4
import com.apollohg.editor.tables.PreparedTableCellContent
import com.apollohg.editor.tables.PreparedViewerTableCell
import org.junit.Assert.assertEquals
import org.junit.Test
import org.junit.runner.RunWith

@RunWith(AndroidJUnit4::class)
class MeasuredPlainCellsDeviceTest {
    @Test
    fun boundedMeasurementsPreserveFullLayoutGeometryAccessibilityAndMetadataCharges() {
        val corpus = listOf(
            "", " ", "word ", " word", "a  b", "R0001C0019XY", "fi ffi AV",
            "e\u0301", "👩‍👩‍👧‍👦🔥", "中文表格文字",
            "abc אבג def", "אבג abc", "العربية نص", "कर्म हिन्दी",
            "a\nb",
            "a\n",
            "a\n\n",
            "\n",
            "a\tb",
            "a\r\nb",
            "a\u2028b",
            "a\u2029b",
            "a\u00adbc",
            "\uFFFC",

            "x".repeat(
                257
            )
        )
        for (density in listOf(1f, 2.625f)) {
            for (fontScale in listOf(1f, 1.3f, 2f)) {
                val base = PreparedProseTheme.resolve(
                    null,
                    density,
                    fontScale
                ).copy(insetTopPx = 0, insetBottomPx = 0, insetLeftPx = 0, insetRightPx = 0)
                for (typeface in listOf(
                    base.paragraph.typeface,
                    Typeface.SERIF,
                    Typeface.MONOSPACE
                )) {
                    val theme = base.copy(paragraph = base.paragraph.copy(typeface = typeface))
                    for (width in listOf(1, 31, 163, 300)) {
                        for (rotation in corpus.indices) {
                            val texts = corpus.drop(rotation) + corpus.take(rotation)
                            val cells = texts.mapIndexed { index, text ->
                                val document =
                                    ViewerDocument(
                                        "$index:$text",
                                        listOf(
                                            ViewerBlock(
                                                "paragraph",
                                                0,
                                                false,
                                                null,
                                                null,
                                                listOf(ViewerInline.Text(text, emptyList()))
                                            )
                                        ),
                                        false,
                                        0
                                    )
                                val key = ProseLayoutKey(
                                    document.semanticKey,
                                    width,
                                    "batch-parity",
                                    0,
                                    0,
                                    density.toBits().toLong(),
                                    0,
                                    "batch-parity"
                                )
                                PlainTableCellMeasurement(
                                    key,
                                    text.ifEmpty {
                                        PlainTableCellMeasurer.EMPTY_TEXT
                                    }
                                ) {
                                    StaticLayoutAndroidProseLayoutEngine().prepare(
                                        document,
                                        key,
                                        theme,
                                        width,
                                        density,
                                        false
                                    )
                                }
                            }
                            val measured = PlainTableCellMeasurer.measure(cells) { text, pixels ->
                                StaticLayout.Builder.obtain(
                                    text,
                                    0,
                                    text.length,
                                    theme.paragraph.newTextPaint(),
                                    pixels
                                )
                                    .setAlignment(
                                        Layout.Alignment.ALIGN_NORMAL
                                    ).setIncludePad(false)
                                    .setBreakStrategy(Layout.BREAK_STRATEGY_HIGH_QUALITY).build()
                            }
                            for ((index, cell) in cells.withIndex()) {
                                val full = cell.prepare()
                                val detail = (
                                    "text=${texts[index]} width=$width density=$density " +
                                        "scale=$fontScale rotation=$rotation font=$typeface"
                                    )
                                assertEquals(
                                    "height $detail",
                                    full.heightPx,
                                    measured[index].heightPx
                                )
                                fun prepared(content: PreparedTableCellContent) =
                                    PreparedViewerTableCell(
                                        index, index, 0, 1, 1,
                                        0 to 0,
                                        content,
                                        false,
                                        null,
                                        retainContent =
                                        false,
                                        prepareContent =
                                            cell.prepare
                                    )
                                val expected = prepared(PreparedTableCellContent.Full(full))
                                val actual = prepared(measured[index])
                                assertEquals(
                                    "accessibility $detail",
                                    expected.accessibilityText,
                                    actual.accessibilityText
                                )
                                assertEquals(
                                    "metadata accounting $detail",
                                    expected.metadataRetainedBytes,
                                    actual.metadataRetainedBytes
                                )
                                assertEquals("key $detail", expected.contentKey, actual.contentKey)
                                assertEquals(
                                    "position certificate $detail",
                                    expected.isPositionFree,
                                    actual.isPositionFree
                                )
                            }
                        }
                    }
                }
            }
        }
    }
}
