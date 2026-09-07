package com.apollohg.editor

import androidx.test.ext.junit.runners.AndroidJUnit4
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith

@RunWith(AndroidJUnit4::class)
class MeasureHeightInstrumentedTest {

    @Test
    fun measureHeight_narrowerWidthProducesTallerResult() {
        val longText = "word ".repeat(100)
        val renderJSON =
            """[{"type":"blockStart","nodeType":"paragraph","depth":0},""" +
                """{"type":"textRun","text":"$longText"},{"type":"blockEnd"}]"""
        val narrowHeight = RenderBridge.measureHeight(
            json = renderJSON,
            themeJson = null,
            width = 100f,
            density = 1f
        )
        val wideHeight = RenderBridge.measureHeight(
            json = renderJSON,
            themeJson = null,
            width = 1000f,
            density = 1f
        )
        assertTrue(
            "Narrower width ($narrowHeight) should be taller than wider ($wideHeight)",
            narrowHeight > wideHeight
        )
    }

    @Test
    fun measureHeight_largerFontProducesTallerResult() {
        val renderJSON =
            """[{"type":"blockStart","nodeType":"paragraph","depth":0},""" +
                """{"type":"textRun","text":"Hello world"},{"type":"blockEnd"}]"""
        val smallHeight = RenderBridge.measureHeight(
            json = renderJSON,
            themeJson = """{"text":{"fontSize":12}}""",
            width = 375f,
            density = 1f
        )
        val largeHeight = RenderBridge.measureHeight(
            json = renderJSON,
            themeJson = """{"text":{"fontSize":32}}""",
            width = 375f,
            density = 1f
        )
        assertTrue(
            "Larger font ($largeHeight) should be taller than smaller ($smallHeight)",
            largeHeight > smallHeight
        )
    }
}
