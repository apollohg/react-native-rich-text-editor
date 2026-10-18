package com.apollohg.editor

import android.content.Context
import android.graphics.Typeface
import android.text.Annotation
import android.text.SpannableStringBuilder
import android.text.Spanned
import android.text.style.StyleSpan
import android.view.View
import android.view.inputmethod.CursorAnchorInfo
import android.view.inputmethod.EditorInfo
import android.view.inputmethod.ExtractedText
import android.view.inputmethod.ExtractedTextRequest
import android.view.inputmethod.InputConnection
import android.view.inputmethod.InputMethodManager
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.RuntimeEnvironment
import org.robolectric.annotation.Config
import org.robolectric.annotation.GraphicsMode
import org.robolectric.annotation.Implementation
import org.robolectric.annotation.Implements
import org.robolectric.shadow.api.Shadow
import org.robolectric.shadows.ShadowInputMethodManager

@RunWith(RobolectricTestRunner::class)
@Config(sdk = [34], shadows = [EditorInputReportingTest.RecordingInputMethodManager::class])
@GraphicsMode(GraphicsMode.Mode.NATIVE)
internal class EditorInputReportingTest : EditorInputConnectionTestFixture() {
    @Implements(InputMethodManager::class)
    class RecordingInputMethodManager : ShadowInputMethodManager() {
        val extracted = mutableListOf<Pair<Int, ExtractedText>>()
        val selections = mutableListOf<List<Int>>()
        val styledCacheAtSelection = mutableListOf<Boolean>()

        @Implementation
        fun updateExtractedText(view: View, token: Int, text: ExtractedText) {
            extracted +=
                token to text
        }

        @Implementation
        fun updateSelection(
            view: View,
            start: Int,
            end: Int,
            composingStart: Int,
            composingEnd: Int
        ) {
            selections += listOf(start, end, composingStart, composingEnd)
            styledCacheAtSelection +=
                (view as? EditorEditText)?.cachedImeTextCoordinateMapper != null
        }
    }

    @Test
    @Config(sdk = [24, 34])
    fun `accepted native insert publishes caret without materializing styled readback`() {
        val harness = structuredDeleteHarness("<p><strong>$MARKER_BODY</strong></p>")
        try {
            val editor = harness.editText
            val insertion = MARKER_BODY.indexOf(EMOJI)
            editor.setSelection(insertion)
            val input = editor.onCreateInputConnection(EditorInfo()) as EditorInputConnection
            try {
                val captured = input.getExtractedText(
                    ExtractedTextRequest().apply { flags = InputConnection.GET_TEXT_WITH_STYLES },
                    0
                )!!.text as Spanned
                val manager = editor.context.getSystemService(
                    Context.INPUT_METHOD_SERVICE
                ) as InputMethodManager
                val recorder = Shadow.extract<RecordingInputMethodManager>(manager)
                recorder.selections.clear()

                assertTrue(input.commitText(INSERTED_TEXT, 1))
                val expected = MARKER_BODY.substring(0, insertion) + INSERTED_TEXT +
                    MARKER_BODY.substring(insertion)
                assertEquals(expected, editor.text.toString())
                assertEquals(
                    listOf(
                        insertion + INSERTED_TEXT.length,
                        insertion + INSERTED_TEXT.length,
                        -1,
                        -1
                    ),
                    recorder.selections.last()
                )
                assertFalse(
                    "actual IMM caret delivery must not construct a styled snapshot",
                    recorder.styledCacheAtSelection.last()
                )
                assertNull(
                    "native caret publication must not eagerly freeze every rich span",
                    editor.cachedImeTextCoordinateMapper
                )
                val updated = input.getExtractedText(
                    ExtractedTextRequest().apply { flags = InputConnection.GET_TEXT_WITH_STYLES },
                    0
                )!!.text as Spanned
                assertEquals(expected, updated.toString())
                val style = updated.getSpans(0, updated.length, StyleSpan::class.java).single()
                assertEquals(Typeface.BOLD, style.style)
                assertEquals(0, updated.getSpanStart(style))
                assertEquals(expected.length, updated.getSpanEnd(style))
                assertEquals(MARKER_BODY, captured.toString())
                val oldStyle = captured.getSpans(0, captured.length, StyleSpan::class.java).single()
                assertEquals(Typeface.BOLD, oldStyle.style)
                assertEquals(MARKER_BODY.length, captured.getSpanEnd(oldStyle))
                assertEquals("<p><strong>$expected</strong></p>", harness.adapter.documentHtml())
            } finally {
                input.closeConnection()
            }
        } finally {
            harness.adapter.destroy()
        }
    }

    @Test
    @Config(sdk = [24, 34])
    fun `marker union and placeholders publish caret without freezing foreign spans`() {
        val placeholder = LayoutConstants.SYNTHETIC_PLACEHOLDER_CHARACTER
        val raw = "$MARKER_PREFIX$MARKER_BODY".replace(
            MASKED_WORD_PREFIX,
            "$MASKED_WORD_PREFIX$placeholder"
        )
        val metadata = RenderMetadataAnnotation(RenderBridge.NATIVE_DOC_POS_ANNOTATION, OLD_DOC_POS)
        val foreign = Annotation(FOREIGN_ANNOTATION, FOREIGN_VALUE)
        val editor = EditorEditText(RuntimeEnvironment.getApplication())
        editor.setText(
            SpannableStringBuilder(raw).apply {
                setSpan(StyleSpan(Typeface.BOLD), 0, length, Spanned.SPAN_EXCLUSIVE_EXCLUSIVE)
                setSpan(metadata, raw.indexOf(EMOJI), length, Spanned.SPAN_EXCLUSIVE_EXCLUSIVE)
                setSpan(foreign, 0, length, Spanned.SPAN_EXCLUSIVE_EXCLUSIVE)
            }
        )
        editor.setSelection(raw.length)
        val input = editor.onCreateInputConnection(EditorInfo()) as EditorInputConnection
        try {
            val captured = input.getExtractedText(
                ExtractedTextRequest().apply { flags = InputConnection.GET_TEXT_WITH_STYLES },
                0
            )!!.text as Spanned
            val manager = editor.context.getSystemService(
                Context.INPUT_METHOD_SERVICE
            ) as InputMethodManager
            val recorder = Shadow.extract<RecordingInputMethodManager>(manager)
            val firstMarkerEnd = MARKER_PREFIX.length + 1
            val secondMarkerStart = MARKER_PREFIX.length - 1
            val secondMarkerEnd = raw.indexOf(placeholder)
            val annotated = SpannableStringBuilder(editor.text).apply {
                getSpans(0, length, StyleSpan::class.java).forEach(::removeSpan)
                setSpan(StyleSpan(Typeface.ITALIC), 0, length, Spanned.SPAN_EXCLUSIVE_EXCLUSIVE)
                setSpan(
                    Annotation(RenderBridge.NATIVE_LIST_MARKER_ANNOTATION, MARKER_LABEL),
                    0,
                    firstMarkerEnd,
                    Spanned.SPAN_EXCLUSIVE_EXCLUSIVE
                )
                setSpan(
                    Annotation(RenderBridge.NATIVE_LIST_MARKER_ANNOTATION, MARKER_LABEL),
                    secondMarkerStart,
                    secondMarkerEnd,
                    Spanned.SPAN_EXCLUSIVE_EXCLUSIVE
                )
            }
            metadata.updateValue(NEW_DOC_POS)
            recorder.selections.clear()
            editor.beginBatchEdit()
            editor.setText(annotated)
            editor.setSelection(firstMarkerEnd)
            editor.endBatchEdit()
            assertEquals(listOf(0, 0, -1, -1), recorder.selections.single())
            assertFalse(recorder.styledCacheAtSelection.last())
            assertNull(
                "overlapping masks must be projected without populating styled cache",
                editor.cachedImeTextCoordinateMapper
            )
            val updated = input.getExtractedText(
                ExtractedTextRequest().apply { flags = InputConnection.GET_TEXT_WITH_STYLES },
                0
            )!!.text as Spanned
            assertEquals(VISIBLE_MASKED_BODY, updated.toString())
            assertEquals(
                listOf(Typeface.ITALIC),
                updated.getSpans(0, updated.length, StyleSpan::class.java).map { it.style }
            )
            val currentMetadata = updated.getSpans(0, updated.length, Annotation::class.java)
                .single { it.key == RenderBridge.NATIVE_DOC_POS_ANNOTATION }
            assertEquals(NEW_DOC_POS, currentMetadata.value)
            assertEquals(VISIBLE_MASKED_BODY.indexOf(EMOJI), updated.getSpanStart(currentMetadata))
            assertEquals(updated.length, updated.getSpanEnd(currentMetadata))
            assertEquals(
                FOREIGN_VALUE,
                updated.getSpans(0, updated.length, Annotation::class.java)
                    .single { it.key == FOREIGN_ANNOTATION }.value
            )
            assertEquals(MARKER_TEXT, captured.toString())
            assertEquals(
                OLD_DOC_POS,
                captured.getSpans(0, captured.length, Annotation::class.java)
                    .single { it.key == RenderBridge.NATIVE_DOC_POS_ANNOTATION }.value
            )
        } finally {
            input.closeConnection()
        }
    }

    @Test
    @Config(sdk = [24, 34])
    fun `original connection publishes provisional composition without rich snapshots`() {
        val harness = structuredDeleteHarness("<p><strong>$MARKER_BODY</strong></p>")
        try {
            val editor = harness.editText
            val insertion = MARKER_BODY.indexOf(EMOJI)
            editor.setSelection(insertion)
            val input = editor.onCreateInputConnection(EditorInfo()) as EditorInputConnection
            try {
                val manager = editor.context.getSystemService(
                    Context.INPUT_METHOD_SERVICE
                ) as InputMethodManager
                val recorder = Shadow.extract<RecordingInputMethodManager>(manager)
                for (composing in listOf(PROVISIONAL_COMPOSITION, FINAL_COMPOSITION)) {
                    recorder.selections.clear()
                    assertTrue(input.setComposingText(composing, 1))
                    assertEquals(
                        MARKER_BODY.substring(0, insertion) + composing +
                            MARKER_BODY.substring(insertion),
                        editor.text.toString()
                    )
                    assertEquals(
                        listOf(
                            insertion + composing.length,
                            insertion + composing.length,
                            insertion,
                            insertion + composing.length
                        ),
                        recorder.selections.last()
                    )
                    assertFalse(
                        "actual IMM composing-range delivery only needs coordinates",
                        recorder.styledCacheAtSelection.last()
                    )
                    assertNull(
                        "provisional composition must not create an unused styled snapshot",
                        editor.cachedImeTextCoordinateMapper
                    )
                }
                assertTrue(input.finishComposingText())
                val expected = MARKER_BODY.substring(0, insertion) + FINAL_COMPOSITION +
                    MARKER_BODY.substring(insertion)
                assertEquals(expected, editor.text.toString())
                assertEquals("<p><strong>$expected</strong></p>", harness.adapter.documentHtml())
            } finally {
                input.closeConnection()
            }
        } finally {
            harness.adapter.destroy()
        }
    }

    @Test
    fun `unchanged text and caret still publish changed generated marker coordinates`() {
        val editor = EditorEditText(RuntimeEnvironment.getApplication())
        editor.setText(MARKER_TEXT)
        editor.setSelection(MARKER_TEXT.length)
        val input = editor.onCreateInputConnection(EditorInfo()) as EditorInputConnection
        val manager = editor.context.getSystemService(
            Context.INPUT_METHOD_SERVICE
        ) as InputMethodManager
        val recorder = Shadow.extract<RecordingInputMethodManager>(manager)
        try {
            input.publishInputStateIfNeeded()
            val previous = input.getTextBeforeCursor(MARKER_TEXT.length, 0)
            assertEquals(MARKER_TEXT, previous.toString())
            val annotated = SpannableStringBuilder(editor.text)
            val marker = Annotation(RenderBridge.NATIVE_LIST_MARKER_ANNOTATION, MARKER_LABEL)
            annotated.setSpan(
                marker,
                0,
                MARKER_PREFIX.length,
                Spanned.SPAN_INCLUSIVE_INCLUSIVE
            )
            recorder.selections.clear()
            editor.beginBatchEdit()
            editor.setText(annotated)
            editor.setSelection(MARKER_TEXT.length)
            editor.endBatchEdit()
            assertEquals(MARKER_TEXT, editor.text.toString())
            assertEquals(MARKER_TEXT.length, editor.selectionEnd)
            assertEquals(
                listOf(MARKER_BODY.length, MARKER_BODY.length, -1, -1),
                recorder.selections.single()
            )
            val captured = input.getTextBeforeCursor(MARKER_TEXT.length, 0)
            assertEquals(MARKER_BODY, captured.toString())
            val unmarked = SpannableStringBuilder(editor.text).apply { removeSpan(marker) }
            recorder.selections.clear()
            editor.beginBatchEdit()
            editor.setText(unmarked)
            editor.setSelection(MARKER_TEXT.length)
            editor.endBatchEdit()
            assertEquals(
                listOf(MARKER_TEXT.length, MARKER_TEXT.length, -1, -1),
                recorder.selections.single()
            )
            assertEquals(MARKER_TEXT, input.getTextBeforeCursor(MARKER_TEXT.length, 0).toString())
            assertEquals(MARKER_BODY, captured.toString())
        } finally {
            input.closeConnection()
        }
    }

    @Test
    fun `styled extracted text monitor receives mark changes with unchanged selection`() {
        val harness = structuredDeleteHarness("<p><strong>$MARKER_BODY</strong></p>")
        try {
            val editor = harness.editText
            editor.setSelection(0, MARKER_BODY.length)
            val input = editor.onCreateInputConnection(EditorInfo()) as EditorInputConnection
            val manager = editor.context.getSystemService(
                Context.INPUT_METHOD_SERVICE
            ) as InputMethodManager
            val recorder = Shadow.extract<RecordingInputMethodManager>(manager)
            input.publishInputStateIfNeeded()
            val captured = input.getExtractedText(
                ExtractedTextRequest().apply {
                    token = REPORT_TOKEN
                    flags = InputConnection.GET_TEXT_WITH_STYLES
                },
                InputConnection.GET_EXTRACTED_TEXT_MONITOR
            )!!.text as Spanned
            assertEquals(
                listOf(Typeface.BOLD),
                captured.getSpans(0, captured.length, StyleSpan::class.java).map { it.style }
            )
            recorder.selections.clear()
            recorder.extracted.clear()
            editor.performToolbarToggleMark(BOLD_MARK)
            assertEquals(MARKER_BODY, editor.text.toString())
            assertTrue(
                "mark update must keep IME selection unchanged",
                recorder.selections.isEmpty()
            )
            val report = recorder.extracted.single()
            assertEquals(REPORT_TOKEN, report.first)
            val updated = report.second.text as Spanned
            assertEquals(MARKER_BODY, updated.toString())
            assertTrue(updated.getSpans(0, updated.length, StyleSpan::class.java).isEmpty())
            assertEquals(0, report.second.selectionStart)
            assertEquals(MARKER_BODY.length, report.second.selectionEnd)
            assertEquals(
                listOf(Typeface.BOLD),
                captured.getSpans(0, captured.length, StyleSpan::class.java).map { it.style }
            )
            input.closeConnection()
        } finally {
            harness.adapter.destroy()
        }
    }

    @Test
    fun `monitored extracted text and selection follow visible composition and retire together`() {
        val harness = structuredDeleteHarness("<ul><li><p></p></li><li><p>Alpha</p></li></ul>")
        try {
            val editor = harness.editText
            val input = requireNotNull(editor.onCreateInputConnection(EditorInfo()))
            val manager = editor.context.getSystemService(
                Context.INPUT_METHOD_SERVICE
            ) as InputMethodManager
            val recorder = Shadow.extract<RecordingInputMethodManager>(manager)
            input.getExtractedText(
                ExtractedTextRequest().apply {
                    token = REPORT_TOKEN
                },
                InputConnection.GET_EXTRACTED_TEXT_MONITOR
            )
            val visible = requireNotNull(
                editor.imeTextCoordinateMapperForEditor()
            ).visibleText.toString()
            val start = visible.indexOf("Alpha")
            input.setSelection(start, start + 5)
            input.setComposingText("日本", 1)
            assertTrue(recorder.extracted.isNotEmpty())
            val report = recorder.extracted.last()
            assertEquals(REPORT_TOKEN, report.first)
            assertEquals(visible.replace("Alpha", "日本"), report.second.text.toString())
            assertEquals(listOf(start + 2, start + 2, start, start + 2), recorder.selections.last())
            input.finishComposingText()
            val next = requireNotNull(editor.onCreateInputConnection(EditorInfo()))
            recorder.extracted.clear()
            next.commitText("!", 1)
            assertTrue(recorder.extracted.isEmpty())
        } finally {
            harness.adapter.destroy()
        }
    }

    @Test
    fun `cursor report follows owned line bounds and physical padding after scrolling`() {
        val editor = EditorEditText(RuntimeEnvironment.getApplication())
        val text = SpannableStringBuilder("first\nsecond")
        text.setSpan(
            EditorBlockBoxSpan(
                EditorBoxStyle(padding = EditorEdges(top = 23f, bottom = 41f, left = 19f)),
                EditorEdges(),
                0
            ),
            0,
            5,
            Spanned.SPAN_EXCLUSIVE_EXCLUSIVE
        )
        editor.setText(text)
        editor.setPadding(11, 13, 17, 7)
        editor.measure(
            View.MeasureSpec.makeMeasureSpec(320, View.MeasureSpec.EXACTLY),
            View.MeasureSpec.makeMeasureSpec(400, View.MeasureSpec.EXACTLY)
        )
        editor.layout(0, 0, 320, 400)
        editor.setSelection(2)
        editor.scrollTo(3, 31)
        val layout = editor.layout as EditorDocumentLayout
        val report = editor.buildSurfaceCursorAnchorInfo()
        assertEquals(2, report.selectionEnd)
        assertTrue(report.insertionMarkerFlags and CursorAnchorInfo.FLAG_HAS_VISIBLE_REGION != 0)
        assertEquals(
            11f - 3f + layout.getPrimaryHorizontal(2),
            report.insertionMarkerHorizontal,
            0.01f
        )
        assertEquals(13f - 31f + 23f, report.insertionMarkerTop, 0.01f)
        assertEquals(13f - 31f + layout.textLineBottom(0), report.insertionMarkerBottom, 0.01f)
        assertEquals(13f - 31f + layout.getLineBaseline(0), report.insertionMarkerBaseline, 0.01f)
        assertTrue(report.insertionMarkerBottom < 13f - 31f + layout.getLineTop(1))
    }

    private companion object {
        const val MARKER_PREFIX = "1. "
        const val MARKER_BODY = "Alpha😀"
        const val MARKER_TEXT = "$MARKER_PREFIX$MARKER_BODY"
        const val MARKER_LABEL = "ordered"
        const val BOLD_MARK = "bold"
        const val REPORT_TOKEN = 71
        const val MASKED_WORD_PREFIX = "Al"
        const val VISIBLE_MASKED_BODY = "pha😀"
        const val EMOJI = "😀"
        const val INSERTED_TEXT = "!"
        const val OLD_DOC_POS = "7"
        const val NEW_DOC_POS = "15"
        const val FOREIGN_ANNOTATION = "foreign"
        const val FOREIGN_VALUE = "retained"
        const val PROVISIONAL_COMPOSITION = "に"
        const val FINAL_COMPOSITION = "日本"
    }
}
