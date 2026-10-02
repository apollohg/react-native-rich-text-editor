package com.apollohg.editor.viewer

import android.graphics.Bitmap
import android.graphics.Canvas
import android.graphics.Color
import android.graphics.Typeface
import android.text.Spanned
import android.text.StaticLayout
import androidx.test.ext.junit.runners.AndroidJUnit4
import org.junit.Assert.assertArrayEquals
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith
import uniffi.editor_core.FfiViewerMark

@RunWith(AndroidJUnit4::class)
class PreparedPlainTextLayoutDeviceTest {
    private companion object {
        const val GENERATION = "plain-text-layout"
        const val WIDTH = 163
        const val DENSITY = 2.625f
        const val COLOR = "#2468AC"
        const val CUSTOM_LETTER_SPACING = .03f
    }

    private fun theme(density: Float = DENSITY, fontScale: Float = 1f): PreparedProseTheme =
        PreparedProseTheme.resolve(null, density, fontScale).let {
            it.copy(insetTopPx = 0, insetBottomPx = 0, insetLeftPx = 0, insetRightPx = 0,
                paragraph = it.paragraph.copy(color = Color.parseColor(COLOR)))
        }

    private fun prepare(inlines: List<ViewerInline>, theme: PreparedProseTheme, width: Int = WIDTH,
                        nodeType: String = "paragraph"): PreparedProseLayout {
        val document = ViewerDocument(GENERATION, listOf(
            ViewerBlock(nodeType, 0, false, null, null, inlines)), false, 0)
        val key = ProseLayoutKey(GENERATION, width, GENERATION, 0, 0,
            theme.density.toBits().toLong(), 0, GENERATION)
        return StaticLayoutAndroidProseLayoutEngine().prepare(document, key, theme, width, theme.density, false)
    }

    private fun prepare(text: String, theme: PreparedProseTheme, width: Int, explicitColor: Boolean): PreparedProseLayout =
        prepare(listOf(ViewerInline.Text(text, if (explicitColor) {
            listOf(FfiViewerMark("textColor", """{"color":"$COLOR"}"""))
        } else emptyList())), theme, width)

    private fun textLayout(layout: PreparedProseLayout): StaticLayout = requireNotNull(
        layout.blocks.single().fragments.single { it.kind == PreparedProseFragmentKind.TEXT }.layout)

    @Test
    fun unmarkedSingleRunAvoidsStyledMeasurementButFormattingRetainsIt() {
        val theme = theme()
        val plain = prepare("plain paragraph", theme, WIDTH, false)
        assertFalse("ordinary text must avoid Android's styled measurement path", textLayout(plain).text is Spanned)
        val explicit = prepare("plain paragraph", theme, WIDTH, true)
        assertTrue(textLayout(explicit).text is Spanned)
        assertEquivalent("explicit base color", explicit, plain)
        val multipleRuns = prepare(listOf(ViewerInline.Text("first", emptyList()),
            ViewerInline.Text("second", emptyList())), theme)
        assertTrue("multiple run boundaries retain their existing shaping", textLayout(multipleRuns).text is Spanned)
        val bold = prepare(listOf(ViewerInline.Text("bold", listOf(FfiViewerMark("bold", "{}")))), theme)
        assertTrue("mark styling must remain present", textLayout(bold).text is Spanned)
        for (nodeType in listOf("codeBlock", "heading")) {
            val nonParagraph = prepare(listOf(ViewerInline.Text("other block", emptyList())), theme,
                nodeType = nodeType)
            assertTrue("non-paragraph blocks retain their spans: $nodeType", textLayout(nonParagraph).text is Spanned)
        }
        val sheet = PreparedProseTheme.resolve(
            """{"version":1,"styles":{"paragraph":{"fontSize":17}}}""", DENSITY)
        assertTrue("style sheet resolution must remain present",
            textLayout(prepare("styled paragraph", sheet, WIDTH, false)).text is Spanned)
    }

    @Test
    fun plainTextMatchesExplicitBaseColorForUnicodeCaretsPixelsAndMetadata() {
        val texts = listOf("", " ", "word ", " word", "a  b", "R0001C0019XY", "fi ffi AV",
            "e\u0301", "👩‍👩‍👧‍👦🔥", "中文表格文字", "abc אבג def", "אבג abc", "العربية نص", "कर्म हिन्दी",
            "a\nb", "a\n", "a\n\n", "\n", "a\tb", "a\r\nb", "a\u2028b", "a\u2029b", "a\u00adbc", "x".repeat(80))
        for (density in listOf(1f, DENSITY)) for (fontScale in listOf(1f, 1.3f)) {
            val base = theme(density, fontScale)
            val paints = listOf(base.paragraph,
                base.paragraph.copy(typeface = Typeface.create(Typeface.SERIF, Typeface.BOLD_ITALIC)),
                base.paragraph.copy(typeface = Typeface.MONOSPACE))
            for (paint in paints) for (width in listOf(31, WIDTH)) for (text in texts) {
                val style = base.copy(paragraph = paint)
                val context = "text=<$text> width=$width density=$density fontScale=$fontScale typeface=${paint.typeface}"
                assertEquivalent(context, prepare(text, style, width, true), prepare(text, style, width, false))
            }
        }
    }

    @Test
    fun customSpacingHeightAndAlignmentKeepTheExistingSpanGeometry() {
        val base = theme()
        val paints = listOf(base.paragraph.copy(letterSpacing = CUSTOM_LETTER_SPACING),
            base.paragraph.copy(lineHeightPx = base.paragraph.sizePx.toInt() * 2),
            base.paragraph.copy(textAlign = "right"))
        for (paint in paints) for (text in listOf("fi ffi AV", "abc אבג", "a\n", "\n")) {
            val style = base.copy(paragraph = paint)
            val plain = prepare(text, style, WIDTH, false)
            assertTrue("spacing and paragraph spans must retain their caret semantics: $paint",
                textLayout(plain).text is Spanned)
            assertEquivalent("text=<$text> paint=$paint", prepare(text, style, WIDTH, true), plain)
        }
    }

    private fun assertEquivalent(context: String, expected: PreparedProseLayout, actual: PreparedProseLayout) {
        assertEquals("height $context", expected.heightPx, actual.heightPx)
        assertEquals("retained bytes $context", expected.retainedBytes, actual.retainedBytes)
        assertEquals("accessibility $context", expected.accessibilityNodes, actual.accessibilityNodes)
        assertEquals("interactions $context", expected.interactions, actual.interactions)
        assertEquals("error $context", expected.error, actual.error)
        assertEquals("blocks $context", expected.blocks.size, actual.blocks.size)
        for ((left, right) in expected.blocks.zip(actual.blocks)) {
            assertEquals("block bounds $context", left.bounds, right.bounds)
            assertEquals("fragments $context", left.fragments.size, right.fragments.size)
            for ((a, b) in left.fragments.zip(right.fragments)) {
                assertEquals("fragment bounds $context", a.bounds, b.bounds)
                assertEquals("fragment kind $context", a.kind, b.kind)
                val x = a.layout ?: continue
                val y = requireNotNull(b.layout)
                assertEquals("text $context", x.text.toString(), y.text.toString())
                assertEquals("lines $context", x.lineCount, y.lineCount)
                for (line in 0 until x.lineCount) {
                    assertEquals("start $line $context", x.getLineStart(line), y.getLineStart(line))
                    assertEquals("end $line $context", x.getLineEnd(line), y.getLineEnd(line))
                    assertEquals("top $line $context", x.getLineTop(line), y.getLineTop(line))
                    assertEquals("bottom $line $context", x.getLineBottom(line), y.getLineBottom(line))
                    assertEquals("baseline $line $context", x.getLineBaseline(line), y.getLineBaseline(line))
                    assertEquals("left $line $context", x.getLineLeft(line), y.getLineLeft(line))
                    assertEquals("right $line $context", x.getLineRight(line), y.getLineRight(line))
                }
                for (offset in 0..x.text.length) {
                    assertEquals("primary caret $offset $context", x.getPrimaryHorizontal(offset), y.getPrimaryHorizontal(offset))
                    assertEquals("secondary caret $offset $context", x.getSecondaryHorizontal(offset), y.getSecondaryHorizontal(offset))
                }
                assertArrayEquals("pixels $context", pixels(x), pixels(y))
            }
        }
    }

    private fun pixels(layout: StaticLayout): IntArray {
        val bitmap = Bitmap.createBitmap(layout.width, maxOf(1, layout.height), Bitmap.Config.ARGB_8888)
        try {
            layout.draw(Canvas(bitmap))
            return IntArray(bitmap.width * bitmap.height).also {
                bitmap.getPixels(it, 0, bitmap.width, 0, 0, bitmap.width, bitmap.height)
            }
        } finally { bitmap.recycle() }
    }
}
