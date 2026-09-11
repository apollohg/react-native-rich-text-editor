package com.apollohg.editor

import android.graphics.Paint
import android.text.TextPaint
import org.json.JSONArray
import org.json.JSONObject
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNull
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config

@RunWith(RobolectricTestRunner::class)
@Config(sdk = [34])
internal class EditorMentionPaddingTest : EditorV2AdapterTestFixture() {
    @Test
    fun `mention padding admission accepts zero and rejects invalid side values`() {
        for (side in listOf("Top", "Right", "Bottom", "Left")) {
            fun snapshot(value: String): String {
                val root = JSONObject(atomicRenderSnapshot("Ada", "1"))
                root.put(
                    "renderBlocks",
                    JSONArray(
                        """[[{"type":"opaqueInlineAtom","nodeType":"mention","label":"Ada","docPos":1,"mentionTheme":{"node":{"style":{"padding$side":$value}}}}]]"""
                    )
                )
                return root.toString()
            }
            assertNotNull(parseAtomicRenderSnapshot(snapshot("0")))
            assertNotNull(parseAtomicRenderSnapshot(snapshot("3.5")))
            for (invalid in listOf("-1", "true", "\"Infinity\"", "\"NaN\"", "\"3\"")) {
                assertNull("padding$side: $invalid", parseAtomicRenderSnapshot(snapshot(invalid)))
            }
            assertNull(
                parseAtomicRenderSnapshot(
                    snapshot("0").replace("\"padding$side\":0", "\"padding$side\":1e999")
                )
            )
        }
    }

    @Test
    fun `omitted mention padding preserves defaults and explicit sides override independently`() {
        val base = EditorTextStyle(fontSize = 17f)
        assertEquals(
            EditorEdges(2f, 4f, 2f, 4f),
            resolvedMentionStyle(base, null, null).box.padding
        )
        val theme = EditorTheme.fromJson(
            """{"version":1,"styles":{"mention":{"paddingTop":7,"paddingRight":9}},"mentions":{"node":{"style":{"paddingRight":0,"paddingBottom":5}}}}"""
        )!!
        assertEquals(
            EditorEdges(7f, 0f, 5f, 4f),
            resolvedMentionStyle(base, theme, null).box.padding
        )
        val local = EditorMentionTheme.fromJson(
            JSONObject("""{"node":{"style":{"paddingTop":0,"paddingLeft":8}}}""")
        )
        assertEquals(
            EditorEdges(0f, 0f, 5f, 8f),
            resolvedMentionStyle(base, theme, local).box.padding
        )
    }

    @Test
    fun `zero mention padding removes default width and vertical insets`() {
        val base = EditorTextStyle(fontSize = 17f)
        val theme = EditorTheme.fromJson(
            """{"version":1,"styles":{"mention":{"paddingTop":0,"paddingRight":0,"paddingBottom":0,"paddingLeft":0}}}"""
        )!!
        val paint = TextPaint().apply { textSize = 17f }
        val defaultMetrics = Paint.FontMetricsInt()
        val zeroMetrics = Paint.FontMetricsInt()
        val defaultWidth = EditorMentionSpan(resolvedMentionStyle(base, null, null), 2f)
            .getSize(paint, "Ada", 0, 3, defaultMetrics)
        val zeroWidth = EditorMentionSpan(resolvedMentionStyle(base, theme, null), 2f)
            .getSize(paint, "Ada", 0, 3, zeroMetrics)
        assertEquals(16, defaultWidth - zeroWidth)
        assertEquals(4, zeroMetrics.ascent - defaultMetrics.ascent)
        assertEquals(4, defaultMetrics.descent - zeroMetrics.descent)
    }
}
