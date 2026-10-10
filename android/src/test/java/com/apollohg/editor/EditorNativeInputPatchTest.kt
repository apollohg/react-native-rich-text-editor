package com.apollohg.editor

import android.app.Activity
import android.graphics.Color
import android.graphics.Typeface
import android.os.Looper
import android.text.StaticLayout
import android.text.TextPaint
import android.text.style.CharacterStyle
import android.view.inputmethod.EditorInfo
import java.util.concurrent.LinkedBlockingQueue
import java.util.concurrent.TimeUnit
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.Robolectric
import org.robolectric.RobolectricTestRunner
import org.robolectric.Shadows.shadowOf
import org.robolectric.annotation.Config
import org.robolectric.annotation.GraphicsMode

@RunWith(RobolectricTestRunner::class)
@Config(sdk = [34])
@GraphicsMode(GraphicsMode.Mode.NATIVE)
internal class EditorNativeInputPatchTest : EditorInputConnectionTestFixture() {
    @Test
    fun `committed text at heading end keeps its heading appearance before backspace`() =
        assertHeadingEndTypingAppearance(composing = false)

    @Test
    fun `composing text at heading end keeps its heading appearance before backspace`() =
        assertHeadingEndTypingAppearance(composing = true)

    @Test
    fun `stylesheet heading composition keeps its appearance through repeated updates`() =
        assertHeadingEndTypingAppearance(
            composing = true,
            theme = requireNotNull(
                EditorTheme.fromJson(
                    """
                    {"version":1,"styles":{
                        "paragraph":{"fontSize":$BODY_FONT_SIZE,"color":"#000000"},
                        "h1":{"fontSize":$HEADING_FONT_SIZE,"fontWeight":"bold",
                            "fontFamily":"serif","color":"#ff00ff"}
                    }}
                    """.trimIndent()
                )
            )
        )

    @Test
    fun `composition at paragraph start does not inherit preceding heading appearance`() =
        withStyledEditor { harness ->
            val editor = harness.editText
            val paragraphStart = editor.text.toString().indexOf(BODY_TEXT)
            val expected = appearanceAt(editor, paragraphStart)
            editor.setSelection(paragraphStart)
            val input = requireNotNull(editor.onCreateInputConnection(EditorInfo()))
            assertTrue(input.setComposingText(TYPED_TEXT, 1))

            for (offset in TYPED_TEXT.indices) {
                assertEquals(
                    "paragraph start offset=$offset text=${editor.text}",
                    expected,
                    appearanceAt(editor, paragraphStart + offset)
                )
            }
            assertTrue(input.finishComposingText())
            shadowOf(Looper.getMainLooper()).idle()
            assertEquals(
                "<h1>$HEADING_TEXT</h1><p>$TYPED_TEXT$BODY_TEXT</p>",
                harness.adapter.documentHtml()
            )
            assertEquals(paragraphStart + TYPED_TEXT.length, editor.selectionStart)
        }

    @Test
    fun `normal heading composition does not inherit bold paragraph fallback`() = withStyledEditor(
        HEADING_THEME.copy(
            paragraph = EditorTextStyle(fontWeight = "bold", fontStyle = "italic"),
            headings = mapOf(
                "h1" to HEADING_THEME.headings.getValue("h1").copy(
                    fontWeight = "normal",
                    fontStyle = "normal"
                )
            )
        )
    ) { harness ->
        val editor = harness.editText
        val expected = appearanceAt(editor, 0)
        assertEquals(Typeface.NORMAL, expected.third.style)
        editor.setSelection(HEADING_TEXT.length)
        val input = requireNotNull(editor.onCreateInputConnection(EditorInfo()))
        assertTrue(input.setComposingText(TYPED_TEXT, 1))
        assertEquals(expected, appearanceAt(editor, HEADING_TEXT.length))
    }

    @Test
    fun `wrapped stylesheet heading composition retains its line height`() =
        assertWrappedHeadingLineHeight(withFollowingParagraph = true)

    @Test
    fun `terminal stylesheet heading composition retains its line height`() =
        assertWrappedHeadingLineHeight(withFollowingParagraph = false)

    @Test
    fun `composition at heading start inherits heading after preceding paragraph`() =
        withStyledEditor(initialHtml = "<p>$BODY_TEXT</p><h1>$HEADING_TEXT</h1>") { harness ->
            val editor = harness.editText
            val headingStart = editor.text.toString().indexOf(HEADING_TEXT)
            val expected = appearanceAt(editor, headingStart)
            editor.setSelection(headingStart)
            val input = requireNotNull(editor.onCreateInputConnection(EditorInfo()))
            assertTrue(input.setComposingText(TYPED_TEXT, 1))
            for (offset in TYPED_TEXT.indices) {
                assertEquals(expected, appearanceAt(editor, headingStart + offset))
            }
        }

    private fun assertWrappedHeadingLineHeight(withFollowingParagraph: Boolean) = withStyledEditor(
        requireNotNull(
            EditorTheme.fromJson(
                """
                    {"version":1,"styles":{
                        "h1":{"fontSize":$HEADING_FONT_SIZE,"lineHeight":$HEADING_LINE_HEIGHT}
                    }}
                """.trimIndent()
            )
        ),
        initialHtml = "<h1>$HEADING_TEXT</h1>" +
            if (withFollowingParagraph) "<p>$BODY_TEXT</p>" else ""
    ) { harness ->
        val editor = harness.editText
        editor.setSelection(HEADING_TEXT.length)
        val input = requireNotNull(editor.onCreateInputConnection(EditorInfo()))
        val composing = " composing".repeat(COMPOSING_WORD_COUNT)
        assertTrue(input.setComposingText(composing, 1))
        val layout = StaticLayout.Builder.obtain(
            editor.text,
            0,
            editor.text.length,
            TextPaint(editor.paint),
            WRAPPED_HEADING_WIDTH
        ).setIncludePad(false).build()
        val lastHeadingLine = layout.getLineForOffset(
            HEADING_TEXT.length + composing.length - 1
        )
        val expectedHeight = (HEADING_LINE_HEIGHT * editor.resources.displayMetrics.density).toInt()
        assertTrue("heading must wrap into composing-only lines", lastHeadingLine > 1)
        for (line in 1..lastHeadingLine) {
            assertTrue(
                "line=$line start=${layout.getLineStart(line)} " +
                    "height=${layout.getLineBottom(line) - layout.getLineTop(line)}",
                layout.getLineBottom(line) - layout.getLineTop(line) >= expectedHeight
            )
        }
    }

    @Test
    fun `syntax highlighting refreshes after matching optimistic deletion`() {
        val requests = LinkedBlockingQueue<String>()
        CodeHighlightingRegistry.register(object : CodeHighlightingProvider {
            override val id = HIGHLIGHT_PROVIDER
            override val version = 1
            override fun highlight(
                text: String,
                language: String?,
                theme: String
            ): List<CodeHighlightRange> {
                requests.add(text)
                return listOf(CodeHighlightRange(0, text.length, HIGHLIGHT_COLOR, 0))
            }
        })
        val harness = structuredDeleteHarness("<pre><code>$CODE_TEXT</code></pre><p>$BODY_TEXT</p>")
        val controller = Robolectric.buildActivity(Activity::class.java).setup()
        val editor = harness.editText
        try {
            controller.get().setContentView(editor)
            harness.adapter.claimNativeBindingIfUnowned(1L)
            editor.setCodeHighlighting(NativeCodeHighlightingConfig(HIGHLIGHT_PROVIDER, "theme"))
            assertEquals(CODE_TEXT, requests.poll(HIGHLIGHT_TIMEOUT_SECONDS, TimeUnit.SECONDS))
            awaitHighlight(editor)
            editor.setSelection(CODE_TEXT.length)
            val input = requireNotNull(editor.onCreateInputConnection(EditorInfo()))

            assertTrue(input.setComposingText(TYPED_TEXT, 1))
            assertTrue(input.finishComposingText())
            assertTrue(input.deleteSurroundingText(1, 0))
            shadowOf(Looper.getMainLooper()).idle()

            assertEquals(
                CODE_TEXT + TYPED_TEXT.dropLast(1),
                requests.poll(HIGHLIGHT_TIMEOUT_SECONDS, TimeUnit.SECONDS)
            )
            awaitHighlight(editor)
            assertTrue(editor.text.toString().endsWith(BODY_TEXT))
            assertEquals(CODE_TEXT.length + TYPED_TEXT.length - 1, editor.selectionStart)
        } finally {
            editor.setCodeHighlighting(null)
            harness.adapter.destroy()
            controller.pause().stop().destroy()
        }
    }

    private fun awaitHighlight(editor: EditorEditText) {
        val deadline = System.nanoTime() + TimeUnit.SECONDS.toNanos(HIGHLIGHT_TIMEOUT_SECONDS)
        while (System.nanoTime() < deadline) {
            shadowOf(Looper.getMainLooper()).idle()
            if (editor.text.getSpans(0, editor.text.length, EditorCodeHighlightSpan::class.java)
                    .isNotEmpty()
            ) {
                return
            }
            Thread.sleep(HIGHLIGHT_POLL_MS)
        }
        assertTrue("syntax highlighting was not applied to ${editor.text}", false)
    }

    private fun assertHeadingEndTypingAppearance(
        composing: Boolean,
        theme: EditorTheme = HEADING_THEME
    ) = withStyledEditor(theme) { harness ->
        val editor = harness.editText
        val expected = appearanceAt(editor, 0)
        assertEquals(HEADING_FONT_SIZE * editor.resources.displayMetrics.density, expected.first)
        assertEquals(Color.MAGENTA, expected.second)
        assertEquals(Typeface.BOLD, expected.third.style)
        val bodyAppearance = appearanceAt(editor, editor.text.length - 1)
        editor.setSelection(HEADING_TEXT.length)
        val input = requireNotNull(editor.onCreateInputConnection(EditorInfo()))
        if (composing) {
            for (length in 1..TYPED_TEXT.length) {
                assertTrue(input.setComposingText(TYPED_TEXT.take(length), 1))
                for (offset in 0 until length) {
                    assertEquals(
                        "composing length=$length offset=$offset text=${editor.text}",
                        expected,
                        appearanceAt(editor, HEADING_TEXT.length + offset)
                    )
                }
            }
            assertEquals("<h1>$HEADING_TEXT</h1><p>$BODY_TEXT</p>", harness.adapter.documentHtml())
        } else {
            assertTrue(input.commitText(TYPED_TEXT, 1))
        }
        val typedAppearance = TYPED_TEXT.indices.map {
            appearanceAt(editor, HEADING_TEXT.length + it)
        }
        if (composing) assertTrue(input.finishComposingText())
        assertTrue(input.deleteSurroundingText(1, 0))
        shadowOf(Looper.getMainLooper()).idle()
        val afterBackspace = TYPED_TEXT.dropLast(1).indices.map {
            appearanceAt(editor, HEADING_TEXT.length + it)
        }
        val diagnostic = "composing=$composing before=$typedAppearance after=$afterBackspace\n" +
            editor.imeTraceSnapshotForTesting().joinToString("\n")
        assertEquals(
            diagnostic,
            "<h1>$HEADING_TEXT${TYPED_TEXT.dropLast(1)}</h1><p>$BODY_TEXT</p>",
            harness.adapter.documentHtml()
        )
        assertEquals(diagnostic, List(TYPED_TEXT.length) { expected }, typedAppearance)
        assertEquals(diagnostic, List(TYPED_TEXT.length - 1) { expected }, afterBackspace)
        assertEquals(diagnostic, bodyAppearance, appearanceAt(editor, editor.text.length - 1))
        assertEquals(HEADING_TEXT.length + TYPED_TEXT.length - 1, editor.selectionStart)
        assertEquals(editor.selectionStart, editor.selectionEnd)
    }

    private fun withStyledEditor(
        theme: EditorTheme = HEADING_THEME,
        initialHtml: String = "<h1>$HEADING_TEXT</h1><p>$BODY_TEXT</p>",
        block: (StructuredDeleteHarness) -> Unit
    ) {
        val harness = structuredDeleteHarness(initialHtml)
        try {
            harness.editText.applyTheme(theme)
            harness.adapter.claimNativeBindingIfUnowned(1L)
            block(harness)
        } finally {
            harness.adapter.destroy()
        }
    }

    private fun appearanceAt(editor: EditorEditText, offset: Int): Triple<Float, Int, Typeface> {
        val paint = TextPaint(editor.paint)
        editor.text.getSpans(offset, offset + 1, CharacterStyle::class.java).forEach {
            it.updateDrawState(paint)
        }
        return Triple(paint.textSize, paint.color, paint.typeface ?: Typeface.DEFAULT)
    }

    @Test
    fun `native owner patches include optimistic typing at each block start`() {
        val harness = structuredDeleteHarness("<p>Alpha</p><p>Beta</p>")
        try {
            harness.adapter.claimNativeBindingIfUnowned(1L)
            val editor = harness.editText
            editor.setSelection(0)
            val input = requireNotNull(editor.onCreateInputConnection(EditorInfo()))
            input.commitText("A", 1)
            assertEquals(
                editor.imeTraceSnapshotForTesting().joinToString("\n"),
                "AAlpha\nBeta",
                editor.text.toString()
            )
            assertTrue(editor.lastRenderAppliedPatchForTesting)
            input.commitText(" ", 1)
            assertEquals("<p>A Alpha</p><p>Beta</p>", harness.adapter.documentHtml())
            assertEquals("A Alpha\nBeta", editor.text.toString())
            input.setSelection(8, 8)
            input.commitText("😀", 1)
            assertEquals("<p>A Alpha</p><p>😀Beta</p>", harness.adapter.documentHtml())
            assertEquals("A Alpha\n😀Beta", editor.text.toString())
            assertTrue(editor.lastRenderAppliedPatchForTesting)
        } finally {
            harness.adapter.destroy()
        }
    }

    @Test
    fun `native owner patches include committed composition at a block start`() {
        val harness = structuredDeleteHarness("<p>Alpha</p><p>Beta</p>")
        try {
            harness.adapter.claimNativeBindingIfUnowned(1L)
            val editor = harness.editText
            editor.setSelection(6)
            val input = requireNotNull(editor.onCreateInputConnection(EditorInfo()))
            input.setComposingText("に", 1)
            input.setComposingText("日本", 1)
            input.commitText("日本", 1)
            shadowOf(Looper.getMainLooper()).idle()
            assertEquals("<p>Alpha</p><p>日本Beta</p>", harness.adapter.documentHtml())
            assertEquals(
                editor.imeTraceSnapshotForTesting().joinToString("\n"),
                "Alpha\n日本Beta",
                editor.text.toString()
            )
        } finally {
            harness.adapter.destroy()
        }
    }

    private companion object {
        const val HEADING_TEXT = "Heading"
        const val BODY_TEXT = "Body"
        const val TYPED_TEXT = " words"
        const val BODY_FONT_SIZE = 16f
        const val HEADING_FONT_SIZE = 32f
        const val HEADING_LINE_HEIGHT = 80f
        const val WRAPPED_HEADING_WIDTH = 160
        const val COMPOSING_WORD_COUNT = 6
        const val HIGHLIGHT_PROVIDER = "native-input-patch-highlighting"
        const val HIGHLIGHT_COLOR = 0xff0000ffL
        const val HIGHLIGHT_TIMEOUT_SECONDS = 3L
        const val HIGHLIGHT_POLL_MS = 10L
        const val CODE_TEXT = "let x = 1;"
        val HEADING_THEME = EditorTheme(
            text = EditorTextStyle(fontSize = BODY_FONT_SIZE, color = Color.BLACK),
            headings = mapOf(
                "h1" to EditorTextStyle(
                    fontSize = HEADING_FONT_SIZE,
                    fontWeight = "bold",
                    fontFamily = "serif",
                    color = Color.MAGENTA
                )
            )
        )
    }
}
