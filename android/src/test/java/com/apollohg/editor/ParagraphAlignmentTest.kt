package com.apollohg.editor

import android.text.Layout
import android.text.style.AlignmentSpan
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config

@RunWith(RobolectricTestRunner::class)
@Config(sdk = [34])
internal class ParagraphAlignmentTest : RenderBridgeTestFixture() {
    @Test
    fun `document alignment overrides theme for each block and reset restores inheritance`() {
        val theme = EditorTheme.fromJson("""{"version":1,"styles":{"paragraph":{"textAlign":"center"}}}""")
        val result = RenderBridge.buildSpannable(
            """[{"type":"blockStart","nodeType":"paragraph","depth":0,"textAlign":"left"},
                {"type":"textRun","text":"left","marks":[]},{"type":"blockEnd"},
                {"type":"blockStart","nodeType":"paragraph","depth":0,"textAlign":"right"},
                {"type":"textRun","text":"right","marks":[]},{"type":"blockEnd"},
                {"type":"blockStart","nodeType":"paragraph","depth":0,"textAlign":null},
                {"type":"textRun","text":"reset","marks":[]},{"type":"blockEnd"}]""",
            baseFontSize, textColor, theme = theme
        )
        for ((word, alignment) in listOf("left" to "left", "right" to "right", "reset" to "center")) {
            val start = result.indexOf(word)
            assertEquals(word, listOf(alignment), result.getSpans(start, start + word.length,
                EditorParagraphAlignmentSpan::class.java).map { it.value })
        }
    }

    @Test
    fun `explicit directions are physical for RTL and empty heading retains alignment`() {
        for ((align, resolved) in listOf("left" to Layout.Alignment.ALIGN_OPPOSITE,
            "right" to Layout.Alignment.ALIGN_NORMAL)) {
            val result = RenderBridge.buildSpannable(
                """[{"type":"blockStart","nodeType":"paragraph","depth":0,"textAlign":"$align"},
                    {"type":"textRun","text":"العربية","marks":[]},{"type":"blockEnd"}]""",
                baseFontSize, textColor
            )
            assertEquals(align, listOf(resolved), result.getSpans(0, result.length,
                AlignmentSpan::class.java).map { it.alignment })
        }
        val empty = RenderBridge.buildSpannable(
            """[{"type":"blockStart","nodeType":"h2","depth":0,"textAlign":"right"},{"type":"blockEnd"}]""",
            baseFontSize, textColor
        )
        assertTrue(empty.getSpans(0, empty.length, EditorParagraphAlignmentSpan::class.java)
            .any { it.value == "right" })
    }
    @Test
    @org.robolectric.annotation.GraphicsMode(org.robolectric.annotation.GraphicsMode.Mode.NATIVE)
    fun `alignment only layout replacement moves caret and hit testing and reset clears stale spans`() {
        val width = 320
        val paint = android.text.TextPaint().apply { textSize = baseFontSize }
        val theme = EditorTheme.fromJson("""{"version":1,"styles":{"paragraph":{"textAlign":"center"}}}""")
        for (text in listOf("content", "", "العربية")) {
            var previous: EditorDocumentLayout? = null
            for (alignment in listOf("left", "right", null)) {
                val elements = org.json.JSONArray()
                    .put(org.json.JSONObject().put("type", "blockStart").put("nodeType", "paragraph")
                        .put("depth", 0).put("textAlign", alignment ?: org.json.JSONObject.NULL))
                    .put(org.json.JSONObject().put("type", "textRun").put("text", text)
                        .put("marks", org.json.JSONArray()))
                    .put(org.json.JSONObject().put("type", "blockEnd"))
                val result = RenderBridge.buildSpannable(elements.toString(), baseFontSize, textColor, theme)
                val layout = EditorDocumentLayout(result, paint, width, previous = previous)
                when (alignment) {
                    "left" -> assertEquals("left text=$text", 0f, layout.getLineLeft(0), 0.1f)
                    "right" -> assertEquals("right text=$text", width.toFloat(), layout.getLineRight(0), 0.1f)
                    else -> assertEquals("reset text=$text", width / 2f,
                        (layout.getLineLeft(0) + layout.getLineRight(0)) / 2f, 0.1f)
                }
                val caret = layout.getPrimaryHorizontal(0)
                assertEquals("hit-test caret text=$text align=$alignment", 0,
                    layout.getOffsetForHorizontal(0, caret))
                previous = layout
            }
        }
    }

}
