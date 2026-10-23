package com.apollohg.editor

import android.graphics.Color
import android.graphics.Typeface
import android.text.Annotation
import android.text.Spanned
import android.text.TextPaint
import android.text.style.CharacterStyle
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config
import org.robolectric.annotation.GraphicsMode

@RunWith(RobolectricTestRunner::class)
@Config(sdk = [34])
@GraphicsMode(GraphicsMode.Mode.NATIVE)
internal class RenderBridgeTextColorTest {
    @Test
    fun `explicit color overrides link and code while retaining other marks and link target`() {
        themes.forEach { theme ->
            val text = render(theme, "\"$COLOR\"")
            val paint = paint(text)
            assertEquals(
                "Explicit CSS RGBA must override theme and link foreground",
                EXPECTED_COLOR,
                paint.color
            )
            assertTrue("Bold survives the color mark", paint.typeface.isBold)
            assertEquals(
                "Monospace survives the color mark",
                paint.measureText("i"),
                paint.measureText("W"),
                GLYPH_WIDTH_TOLERANCE
            )
            assertEquals(HREF, text.getSpans(0, 1, Annotation::class.java).single().value)
        }
    }

    @Test
    fun `null or removed color restores link styling`() {
        themes.forEach { theme ->
            val removed = paint(render(theme, null)).color
            assertEquals(
                "Null color must leave the link foreground unchanged",
                removed,
                paint(render(theme, "null")).color
            )
            assertEquals(
                "The link foreground returns when the mark is removed",
                Color.BLUE,
                removed
            )
        }
    }

    private fun render(theme: EditorTheme?, color: String?): Spanned {
        val colorMark = color?.let { "{\"type\":\"textStyle\",\"color\":$it}," }.orEmpty()
        return RenderBridge.buildSpannable(
            """[{"type":"blockStart","nodeType":"paragraph","depth":0},
                {"type":"textRun","text":"colored link","marks":[$colorMark"bold","code",{"type":"link","href":"$HREF"}]},
                {"type":"blockEnd"}]""",
            FONT_SIZE,
            Color.BLACK,
            theme
        )
    }

    private fun paint(text: Spanned): TextPaint = TextPaint().apply {
        typeface = Typeface.DEFAULT
        text.getSpans(0, 1, CharacterStyle::class.java).forEach { it.updateDrawState(this) }
    }

    private val themes = listOf(
        EditorTheme.fromJson("""{"links":{"color":"#0000ffff"}}"""),
        EditorTheme.fromJson(
            """{"version":1,"styles":{"paragraph":{"color":"#008000ff"},"link":{"color":"#0000ffff"},"inlineCode":{"color":"#ffff00ff"}}}"""
        )
    )

    private companion object {
        const val COLOR = "#12345680"
        const val EXPECTED_COLOR = 0x80123456.toInt()
        const val FONT_SIZE = 16f
        const val GLYPH_WIDTH_TOLERANCE = 0.01f
        const val HREF = "https://example.test/color"
    }
}
