package com.apollohg.editor

import android.graphics.Bitmap
import android.graphics.Canvas
import android.graphics.Color
import android.graphics.Matrix
import android.graphics.Paint
import android.graphics.Path
import android.graphics.Rect
import android.graphics.RectF
import android.text.StaticLayout
import android.text.TextPaint
import org.junit.Assert.assertArrayEquals
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
class EditorStyleSheetDrawingTest {
    private data class DrawingCase(val name: String, val bounds: RectF, val matrix: Matrix)

    private class RecordingCanvas(bitmap: Bitmap) : Canvas(bitmap) {
        var pathDrawCount = 0
            private set

        override fun drawPath(path: Path, paint: Paint) {
            pathDrawCount++
            super.drawPath(path, paint)
        }
    }

    @Test
    @Config(sdk = [28, 34])
    fun `fully clipped decoration boxes issue no path draws`() {
        val bitmap = Bitmap.createBitmap(100, 100, Bitmap.Config.ARGB_8888)
        val canvas = RecordingCanvas(bitmap)
        canvas.clipRect(20, 20, 80, 80)
        val box = EditorBoxStyle(
            backgroundColor = Color.YELLOW,
            border = EditorEdges(4f, 4f, 4f, 4f),
            corners = EditorCorners(8f, 8f, 8f, 8f)
        )
        val outsideBounds = listOf(
            RectF(25f, -40f, 75f, 10f),
            RectF(25f, 90f, 75f, 140f),
            RectF(-40f, 25f, 10f, 75f),
            RectF(90f, 25f, 140f, 75f)
        )
        for (bounds in outsideBounds) {
            EditorBoxDrawing.draw(canvas, bounds, box)
            assertEquals(
                "clipped bounds $bounds must not issue draw commands",
                0,
                canvas.pathDrawCount
            )
        }
    }

    @Test
    @Config(sdk = [28, 34])
    fun `clipped transformed decoration pixels match unculled drawing`() {
        val width = 100
        val height = 80
        val clip = Rect(18, 12, 75, 65)
        val cases = listOf(
            DrawingCase(
                "rotated",
                RectF(8f, 4f, 88f, 74f),
                Matrix().apply {
                    setRotate(12f, width / 2f, height / 2f)
                    postTranslate(5.25f, -2.5f)
                }
            ),
            DrawingCase(
                "translated into clip",
                RectF(-190f, 4f, -110f, 74f),
                Matrix().apply { setTranslate(200f, 0f) }
            ),
            DrawingCase(
                "fractional clip edge",
                RectF(75.25f, 20f, 115.25f, 60f),
                Matrix().apply { setTranslate(-0.5f, 0f) }
            )
        )
        val rounded = EditorBoxStyle(
            backgroundColor = Color.YELLOW,
            border = EditorEdges(4f, 4f, 4f, 4f),
            borderColors = List(4) { Color.RED },
            corners = EditorCorners(20f, 20f, 20f, 20f)
        )
        val asymmetric = rounded.copy(
            border = EditorEdges(4f, 0f, 6f, 8f),
            borderColors = listOf(Color.RED, Color.BLUE, Color.GREEN, Color.MAGENTA),
            corners = EditorCorners(30f, 0f, 12f, 6f)
        )
        val styles = listOf(
            rounded,
            asymmetric,
            asymmetric.copy(borderStyle = "dashed"),
            asymmetric.copy(borderStyle = "dotted"),
            rounded.copy(corners = EditorCorners(200f, 100f, 150f, 300f))
        )
        for (case in cases) {
            for (style in styles) {
                val complete = Bitmap.createBitmap(width, height, Bitmap.Config.ARGB_8888)
                val clipped = Bitmap.createBitmap(width, height, Bitmap.Config.ARGB_8888)
                complete.eraseColor(Color.LTGRAY)
                clipped.eraseColor(Color.LTGRAY)
                val completeCanvas = object : Canvas(complete) {
                    @Suppress("DEPRECATION")
                    override fun quickReject(rect: RectF, type: EdgeType): Boolean = false
                }.apply {
                    clipRect(clip)
                    concat(case.matrix)
                }
                val clippedCanvas = Canvas(clipped).apply {
                    clipRect(clip)
                    concat(case.matrix)
                }
                EditorBoxDrawing.draw(completeCanvas, case.bounds, style)
                EditorBoxDrawing.draw(clippedCanvas, case.bounds, style)
                val expected = IntArray(width * height)
                complete.getPixels(expected, 0, width, 0, 0, width, height)
                val actual = IntArray(width * height)
                clipped.getPixels(actual, 0, width, 0, 0, width, height)
                val description = "${case.name}: $style"
                assertTrue(
                    "reference must draw visible pixels for $description",
                    expected.any {
                        it !=
                            Color.LTGRAY
                    }
                )
                assertArrayEquals("clipped pixels for $description", expected, actual)
            }
        }
    }

    @Test
    fun `rounded border fills the corners around its rounded interior`() {
        val bitmap = Bitmap.createBitmap(100, 100, Bitmap.Config.ARGB_8888)
        val box =
            EditorBoxStyle(
                backgroundColor = Color.WHITE,
                border = EditorEdges(10f, 10f, 10f, 10f),
                borderColors = List(4) {
                    Color.RED
                },
                corners = EditorCorners(30f, 30f, 30f, 30f)
            )
        EditorBoxDrawing.draw(Canvas(bitmap), RectF(0f, 0f, 100f, 100f), box)
        assertEquals(Color.RED, bitmap.getPixel(13, 13))
        assertEquals(Color.WHITE, bitmap.getPixel(30, 30))
        assertEquals(Color.TRANSPARENT, bitmap.getPixel(0, 0))
    }

    @Test
    fun `asymmetric edges omit zero side and clip loaded image corners`() {
        val bitmap = Bitmap.createBitmap(80, 60, Bitmap.Config.ARGB_8888)
        val canvas = Canvas(bitmap)
        val box =
            EditorBoxStyle(
                backgroundColor = Color.YELLOW,
                border = EditorEdges(4f, 0f, 4f, 6f),
                borderColors = listOf(Color.RED, Color.BLUE, Color.GREEN, Color.MAGENTA),
                corners = EditorCorners(12f, 0f, 0f, 12f)
            )
        val bounds = RectF(0f, 0f, 80f, 60f)
        EditorBoxDrawing.draw(canvas, bounds, box)
        assertEquals(Color.TRANSPARENT, bitmap.getPixel(0, 0))
        assertEquals(Color.RED, bitmap.getPixel(40, 1))
        assertEquals(Color.GREEN, bitmap.getPixel(40, 58))
        assertEquals(Color.MAGENTA, bitmap.getPixel(1, 30))
        assertEquals(Color.YELLOW, bitmap.getPixel(79, 30))
        val pixels = Bitmap.createBitmap(80, 60, Bitmap.Config.ARGB_8888).apply {
            eraseColor(Color.BLUE)
        }
        EditorBoxDrawing.drawImage(canvas, pixels, bounds, box, "cover")
        assertEquals(Color.TRANSPARENT, bitmap.getPixel(0, 0))
        assertEquals(Color.MAGENTA, bitmap.getPixel(1, 30))
        assertEquals(Color.BLUE, bitmap.getPixel(40, 30))
    }

    @Test
    fun `custom decoration draws requested color below text`() {
        val theme = EditorTheme.fromJson(

            """{"version":1,"styles":{"underline":{"textDecorationColor":"#ff000""" +
                """0ff","textDecorationStyle":"double"}}}"""
        )!!
        val text = RenderBridge.buildSpannable(

            """[{"type":"blockStart","nodeType":"paragraph","depth":0},""" +
                """{"type":"textRun","text":"underline","marks":["underline"]},""" +
                """{"type":"blockEnd"}]""",
            24f,
            Color.BLACK,
            theme,
            1f
        )
        val layout = StaticLayout.Builder.obtain(
            text,
            0,
            text.length,
            TextPaint().apply {
                textSize =
                    24f
            },
            200
        ).setIncludePad(false).build()
        val bitmap = Bitmap.createBitmap(200, 60, Bitmap.Config.ARGB_8888)
        val canvas = Canvas(bitmap)
        layout.draw(canvas)
        EditorTextDecorationDrawing.draw(canvas, layout)
        val redPixels = (0 until bitmap.width).sumOf { x ->
            (0 until bitmap.height).count { y ->
                val color = bitmap.getPixel(x, y)
                Color.red(color) >
                    200 &&
                    Color.green(color) < 20 &&
                    Color.alpha(color) > 128
            }
        }
        assertTrue("red pixels: $redPixels", redPixels > 100)
    }
}
