package com.apollohg.editor

import android.text.Annotation
import android.text.SpannableStringBuilder
import android.text.Spanned
import android.text.StaticLayout
import android.text.TextPaint
import android.text.style.AbsoluteSizeSpan
import android.text.style.ForegroundColorSpan
import android.text.style.LeadingMarginSpan
import android.text.style.StyleSpan
import org.json.JSONArray
import org.json.JSONObject
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config

@RunWith(RobolectricTestRunner::class)
@Config(sdk = [24, 32, 34])
internal class RenderBridgeBlockAssemblyTest : RichTextEditorViewTestFixture() {
    @Test
    fun `block assembly preserves flat rich styles and geometry across adjacent quotes`() {
        val original = JSONObject(NativePerformanceFixtureFactory.largeUpdateJson())
            .getJSONArray("renderBlocks")
        val quote = (0 until original.length()).map(original::getJSONArray).first {
            it.getJSONObject(0).optString("nodeType") == BLOCKQUOTE_NODE
        }
        val unicode = paragraphRenderBlock(UNICODE_TEXT).apply {
            getJSONObject(1).put("marks", JSONArray().put(BOLD_MARK).put(ITALIC_MARK))
        }
        val code = paragraphRenderBlock(CODE_TEXT).apply {
            getJSONObject(0).put("nodeType", CODE_BLOCK_NODE)
        }
        val blocks = JSONArray().put(quote).put(quote).put(unicode)
            .put(JSONArray(singleBulletListRenderJson()))
            .put(JSONArray(singleTaskListRenderJson())).put(code)
        for (index in 0 until original.length()) blocks.put(original.get(index))
        assertFlatRenderingEqualsBlocks(blocks)
    }

    @Test
    fun `cross block open containers retain the flat rendering fallback`() {
        val block = JSONArray().put(
            JSONObject().put("type", RenderBridge.RENDER_BLOCK_START_TYPE)
                .put("nodeType", BLOCKQUOTE_NODE).put("depth", 0)
        ).put(
            JSONObject().put("type", RenderBridge.RENDER_BLOCK_START_TYPE)
                .put("nodeType", PARAGRAPH_NODE).put("depth", 1)
        ).put(
            JSONObject().put("type", TEXT_RUN).put("text", UNICODE_TEXT).put("marks", JSONArray())
        )
        val closing = JSONArray().put(JSONObject().put("type", RenderBridge.RENDER_BLOCK_END_TYPE))
            .put(JSONObject().put("type", RenderBridge.RENDER_BLOCK_END_TYPE))
        assertFlatRenderingEqualsBlocks(
            JSONArray().put(block).put(closing).put(paragraphRenderBlock(TAIL_TEXT))
        )
    }

    @Test
    fun `each top level block carries one content child marker however many runs it has`() {
        fun markedParagraph(runs: Int): JSONArray = JSONArray().apply {
            put(
                JSONObject().put(
                    "type",
                    "blockStart"
                ).put("nodeType", PARAGRAPH_NODE).put("depth", 0)
            )
            for (run in 0 until runs) {
                put(
                    JSONObject().put("type", "textRun").put("text", "run $run ")
                        .put("marks", if (run % 2 == 0) JSONArray() else JSONArray().put(BOLD_MARK))
                )
            }
            put(JSONObject().put("type", "blockEnd"))
        }
        val blocks = JSONArray().put(markedParagraph(MANY_RUNS)).put(markedParagraph(MANY_RUNS))
        val rendered = RenderBridge.buildSpannableFromBlocks(
            blocks,
            startIndex = FIRST_PATCHED_CHILD,
            baseFontSize = FONT_SIZE,
            textColor = TEXT_COLOR,
            theme = exampleTheme()
        )

        val markers = rendered.getSpans(0, rendered.length, Annotation::class.java)
            .filter { it.key == RenderBridge.NATIVE_TOP_LEVEL_CHILD_INDEX_ANNOTATION }
            .map { Triple(it.value, rendered.getSpanStart(it), rendered.getSpanEnd(it)) }
            .sortedBy { it.second }
        val separator = rendered.indexOf('\n')
        assertEquals(
            "one content marker per block plus the separator marker, independent of $MANY_RUNS runs",
            listOf(
                Triple(FIRST_PATCHED_CHILD.toString(), 0, separator),
                Triple((FIRST_PATCHED_CHILD + 1).toString(), separator, separator + 1),
                Triple((FIRST_PATCHED_CHILD + 1).toString(), separator + 1, rendered.length)
            ),
            markers
        )
    }

    @Test
    fun `rich document leading margins require one full span query and preserve geometry`() {
        val blocks = JSONObject(NativePerformanceFixtureFactory.largeUpdateJson())
            .getJSONArray("renderBlocks")
        val theme = exampleTheme()
        val builder = CountingSpannable()
        val state = RenderBridge.RenderBuildState(result = builder)
        for (blockIndex in 0 until blocks.length()) {
            RenderBridge.appendElements(
                state,
                blocks.getJSONArray(blockIndex),
                FONT_SIZE,
                TEXT_COLOR,
                theme,
                density = 1f,
                hostView = null,
                atomConfiguration = null,
                topLevelChildIndex = blockIndex
            )
            builder.flushTopLevelChild()
        }
        assertTrue(
            "fixture must exercise separate quoted paragraphs",
            state.pendingLeadingMargins.values.count { it.blockquoteStripeColor != null } > 1
        )
        builder.fullLeadingMarginQueries = 0
        RenderBridge.applyPendingLeadingMargins(builder, state.pendingLeadingMargins)
        RenderBridge.applyPendingCodeBlockSpans(builder, state.pendingCodeBlockSpans, theme, 1f)
        val expected = RenderBridge.buildSpannableFromBlocks(
            blocks,
            baseFontSize = FONT_SIZE,
            textColor = TEXT_COLOR,
            theme = theme
        )
        assertEquals("whole rich document", expected.toString(), builder.toString())
        assertEquals("all paragraph spans and styling", spanSummary(expected), spanSummary(builder))
        assertEquals(
            "leading margin assembly must index existing spans once across the whole document",
            1,
            builder.fullLeadingMarginQueries
        )
        assertLayoutEquals(expected, builder)
    }

    private fun assertFlatRenderingEqualsBlocks(blocks: JSONArray) {
        val elements = JSONArray()
        for (blockIndex in 0 until blocks.length()) {
            val block = blocks.getJSONArray(blockIndex)
            for (elementIndex in 0 until block.length()) elements.put(block.get(elementIndex))
        }
        val theme = exampleTheme()
        val flat = RenderBridge.buildSpannableFromArray(elements, FONT_SIZE, TEXT_COLOR, theme)
        val assembled = RenderBridge.buildSpannableFromBlocks(
            blocks,
            baseFontSize = FONT_SIZE,
            textColor = TEXT_COLOR,
            theme = theme
        )
        assertEquals(
            "complete Unicode text and block boundaries",
            flat.toString(),
            assembled.toString()
        )
        assertEquals(
            "all rich span ranges, flags and values",
            spanSummary(flat),
            spanSummary(assembled)
        )
        assertLayoutEquals(flat, assembled)
    }

    private fun assertLayoutEquals(flat: Spanned, assembled: Spanned) {
        val paint = TextPaint().apply { textSize = FONT_SIZE }
        val expected = StaticLayout.Builder.obtain(
            flat,
            0,
            flat.length,
            paint,
            LAYOUT_WIDTH
        ).build()
        val actual = StaticLayout.Builder.obtain(
            assembled,
            0,
            assembled.length,
            paint,
            LAYOUT_WIDTH
        ).build()
        assertEquals("paragraph height and spacing", expected.height, actual.height)
        assertEquals("line wrapping", expected.lineCount, actual.lineCount)
        for (line in 0 until expected.lineCount) {
            assertEquals("line=$line start", expected.getLineStart(line), actual.getLineStart(line))
            assertEquals(
                "line=$line baseline",
                expected.getLineBaseline(line),
                actual.getLineBaseline(line)
            )
            assertEquals(
                "line=$line leading margin",
                expected.getParagraphLeft(line),
                actual.getParagraphLeft(line)
            )
        }
    }

    private fun spanSummary(text: Spanned): List<String> =
        text.getSpans(0, text.length, Any::class.java).filterNot {
            it is Annotation && it.key == RenderBridge.NATIVE_TOP_LEVEL_CHILD_INDEX_ANNOTATION
        }.map { span ->
            val value = when (span) {
                is Annotation -> "${span.key}=${span.value}"

                is ForegroundColorSpan -> span.foregroundColor.toString()

                is AbsoluteSizeSpan -> "${span.size}/${span.dip}"

                is StyleSpan -> span.style.toString()

                is LeadingMarginSpan -> "${span.getLeadingMargin(
                    true
                )}/${span.getLeadingMargin(false)}"

                else -> ""
            }
            "${span.javaClass.name}:${text.getSpanStart(span)}..${text.getSpanEnd(span)}:" +
                "${text.getSpanFlags(span)}:$value"
        }.sorted()

    private class CountingSpannable : RenderTextBuilder() {
        var fullLeadingMarginQueries = 0

        override fun <T : Any?> getSpans(start: Int, end: Int, type: Class<T>?): Array<T> {
            if (start == 0 && end == length && type == LeadingMarginSpan::class.java) {
                fullLeadingMarginQueries += 1
            }
            return super.getSpans(start, end, type)
        }
    }

    private companion object {
        const val MANY_RUNS = 9
        const val FIRST_PATCHED_CHILD = 4
        const val BLOCKQUOTE_NODE = "blockquote"
        const val PARAGRAPH_NODE = "paragraph"
        const val CODE_BLOCK_NODE = "codeBlock"
        const val CODE_TEXT = "val word = \"😀\"\nnext()"
        const val TEXT_RUN = "textRun"
        const val UNICODE_TEXT = "Rich 😀 é text"
        const val TAIL_TEXT = "Tail"
        const val BOLD_MARK = "bold"
        const val ITALIC_MARK = "italic"
        const val FONT_SIZE = 16f
        const val TEXT_COLOR = android.graphics.Color.BLACK
        const val LAYOUT_WIDTH = 600
    }
}
