package com.apollohg.editor

import android.content.Context
import android.graphics.Color
import android.graphics.Typeface
import android.text.Annotation
import android.text.Spanned
import android.view.inputmethod.BaseInputConnection
import android.view.inputmethod.EditorInfo
import android.view.inputmethod.ExtractedTextRequest
import android.view.inputmethod.InputConnection
import android.view.inputmethod.InputMethodManager
import org.json.JSONArray
import org.json.JSONObject
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertSame
import org.junit.Assert.assertThrows
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.RuntimeEnvironment
import org.robolectric.annotation.Config
import org.robolectric.annotation.GraphicsMode
import org.robolectric.shadow.api.Shadow

@RunWith(RobolectricTestRunner::class)
@Config(
    sdk = [24, 34],
    shadows = [EditorInputReportingTest.RecordingInputMethodManager::class]
)
@GraphicsMode(GraphicsMode.Mode.NATIVE)
internal class EditorInputPublicationBatchTest : EditorInputConnectionTestFixture() {
    private class PublicationProbe : Annotation(PROBE_KEY, PROBE_VALUE) {
        var scans = 0
        var failure: IllegalStateException? = null

        override fun getKey(): String {
            if (Thread.currentThread().stackTrace.any {
                    it.className == INPUT_REPORTING_CLASS &&
                        it.methodName == INPUT_REPORTING_METHOD
                }
            ) {
                scans++
                failure?.let { error ->
                    failure = null
                    throw error
                }
            }
            return super.getKey()
        }
    }

    @Test
    fun `medium public-theme insert scans input state only at completed mutation boundaries`() {
        withMediumInput { harness, input, probe, recorder, fixture, caret ->
            val editor = harness.editText
            val before = editor.text.toString()
            val revision = harness.adapter.baseDocumentRevision
            val updates = recordUpdates(editor)
            assertTrue(input.commitText(INSERTED_TEXT, 1))
            val scans = probe.scans
            val expected = before.substring(0, caret) + INSERTED_TEXT + before.substring(caret)
            assertEquals(expected, editor.text.toString())
            assertEquals(expected, harness.expectedText())
            assertEquals(revision + 1uL, harness.adapter.baseDocumentRevision)
            assertEquals("one real engine update must remain observable", 1, updates.size)
            assertFinalInputState(editor, input, recorder, expected, caret + INSERTED_TEXT.length)
            val paragraphs = JSONArray(fixture.getJSONArray("paragraphs").toString())
            val operation = fixture.getJSONObject("operations").getJSONObject("typingInsert")
            val patch = operation.getJSONObject("expectedParagraphPatch")
            paragraphs.put(
                patch.getInt("startParagraphIndex"),
                patch.getJSONArray("paragraphs").getJSONObject(0)
            )
            assertMarkedRanges(input, paragraphs)
            assertEquals(
                "shifted spans must not cause a full IME scan each; visible mutation and accepted render each publish once",
                COMPLETED_MUTATION_BOUNDARIES,
                scans
            )
        }
    }

    @Test
    fun `medium public-theme delete scans input state only at completed mutation boundaries`() {
        withMediumInput { harness, input, probe, recorder, fixture, caret ->
            val editor = harness.editText
            assertTrue(input.commitText(INSERTED_TEXT, 1))
            probe.scans = 0
            recorder.selections.clear()
            val revision = harness.adapter.baseDocumentRevision
            val updates = recordUpdates(editor)
            assertTrue(input.deleteSurroundingText(1, 0))
            val scans = probe.scans
            val expected = fixture.getString("visibleText")
            assertEquals(expected, editor.text.toString())
            assertEquals(expected, harness.expectedText())
            assertEquals(revision + 1uL, harness.adapter.baseDocumentRevision)
            assertEquals("one real engine update must remain observable", 1, updates.size)
            assertFinalInputState(editor, input, recorder, expected, caret)
            assertMarkedRanges(input, fixture.getJSONArray("paragraphs"))
            assertEquals(
                "shifted spans must not cause a full IME scan each; visible mutation and accepted render each publish once",
                COMPLETED_MUTATION_BOUNDARIES,
                scans
            )
        }
    }

    @Test
    fun `caret moves publish without rescanning until text or generated markers change`() {
        withMediumInput { harness, input, probe, recorder, _, caret ->
            val editor = harness.editText
            for (target in listOf(caret + 1, caret + 5, caret - 3)) {
                editor.setSelection(target)
                assertEquals(
                    "caret move to $target publishes its raw offset",
                    listOf(target, target, -1, -1),
                    recorder.selections.last()
                )
            }
            assertEquals("caret-only moves reuse the published content", 0, probe.scans)

            val marker = Annotation(RenderBridge.NATIVE_LIST_MARKER_ANNOTATION, MARKER_VALUE)
            editor.text.setSpan(marker, 0, MARKER_LENGTH, Spanned.SPAN_EXCLUSIVE_EXCLUSIVE)
            assertEquals("a new generated marker forces a rescan", 1, probe.scans)
            val shifted = caret - 3 - MARKER_LENGTH
            assertEquals(
                "the new marker shifts the published caret",
                listOf(shifted, shifted, -1, -1),
                recorder.selections.last()
            )
            editor.setSelection(caret)
            assertEquals(
                "a later caret move keeps the new marker",
                listOf(caret - MARKER_LENGTH, caret - MARKER_LENGTH, -1, -1),
                recorder.selections.last()
            )
            assertEquals("the later caret move reuses the rescanned content", 1, probe.scans)

            editor.text.removeSpan(marker)
            assertEquals("removing a generated marker forces a rescan", 2, probe.scans)
            assertEquals(listOf(caret, caret, -1, -1), recorder.selections.last())

            probe.scans = 0
            assertTrue(input.commitText(INSERTED_TEXT, 1))
            assertEquals(
                "text changes still rescan at completed mutation boundaries",
                COMPLETED_MUTATION_BOUNDARIES,
                probe.scans
            )
            val inserted = caret + INSERTED_TEXT.length
            assertEquals(listOf(inserted, inserted, -1, -1), recorder.selections.last())
            editor.setSelection(inserted - 2)
            assertEquals(listOf(inserted - 2, inserted - 2, -1, -1), recorder.selections.last())
            assertEquals(
                "caret moves after an edit reuse its published content",
                COMPLETED_MUTATION_BOUNDARIES,
                probe.scans
            )
        }
    }

    @Test
    fun `unbatched replacement publishes coordinates from the replaced content`() {
        val context = RuntimeEnvironment.getApplication()
        val editor = EditorEditText(context)
        editor.setText(PLAIN_TEXT)
        assertTrue("the editor owns a versioned editable", editor.text is EditorEditable)
        editor.setSelection(PLAIN_TEXT.length)
        val input = editor.onCreateInputConnection(EditorInfo()) as EditorInputConnection
        try {
            val recorder = Shadow.extract<EditorInputReportingTest.RecordingInputMethodManager>(
                context.getSystemService(Context.INPUT_METHOD_SERVICE) as InputMethodManager
            )
            editor.setSelection(0)
            editor.setSelection(PLAIN_TEXT.length)
            assertEquals(
                "the caret publication before the replacement",
                listOf(PLAIN_TEXT.length, PLAIN_TEXT.length, -1, -1),
                recorder.selections.last()
            )
            val marker = android.text.SpannableString(MARKER_TEXT).apply {
                setSpan(
                    Annotation(RenderBridge.NATIVE_LIST_MARKER_ANNOTATION, MARKER_VALUE),
                    0,
                    MARKER_TEXT.length,
                    Spanned.SPAN_EXCLUSIVE_EXCLUSIVE
                )
            }
            editor.text.replace(0, 0, marker)
            assertEquals(MARKER_TEXT + PLAIN_TEXT, editor.text.toString())
            assertEquals(MARKER_TEXT.length + PLAIN_TEXT.length, editor.selectionEnd)
            assertEquals(
                "the replacement's own publication excludes its generated marker",
                listOf(PLAIN_TEXT.length, PLAIN_TEXT.length, -1, -1),
                recorder.selections.last()
            )
            assertEquals(
                "the published content is the replaced text",
                editor.text.toString(),
                input.lastPublishedRawInputState?.text
            )
        } finally {
            input.closeConnection()
        }
    }

    @Test
    fun `caller-owned input batch retains the complete render and publishes when its batch ends`() {
        withMediumInput { harness, input, probe, recorder, _, caret ->
            val editor = harness.editText
            val before = editor.text.toString()
            input.beginBatchEdit()
            try {
                assertTrue(input.commitText(INSERTED_TEXT, 1))
                assertEquals(0, probe.scans)
                assertTrue(recorder.selections.isEmpty())
                assertEquals(editor.text.toString(), harness.expectedText())
                assertFalse(editor.hasDeferredRustUpdateApplicationForTesting())
            } finally {
                input.endBatchEdit()
            }
            assertEquals(1, probe.scans)
            assertFinalInputState(
                editor,
                input,
                recorder,
                before.substring(0, caret) + INSERTED_TEXT + before.substring(caret),
                caret + INSERTED_TEXT.length
            )
        }
    }

    @Test
    fun `non-batched generated-marker edits still publish actual IME coordinates immediately`() {
        withMediumInput { harness, _, probe, recorder, _, caret ->
            val editor = harness.editText
            val marker = Annotation(RenderBridge.NATIVE_LIST_MARKER_ANNOTATION, MARKER_VALUE)
            editor.text.setSpan(marker, 0, 1, Spanned.SPAN_EXCLUSIVE_EXCLUSIVE)
            assertEquals(1, probe.scans)
            assertEquals(listOf(caret - 1, caret - 1, -1, -1), recorder.selections.single())
            probe.scans = 0
            recorder.selections.clear()
            editor.text.removeSpan(marker)
            assertEquals(1, probe.scans)
            assertEquals(listOf(caret, caret, -1, -1), recorder.selections.single())
        }
    }

    @Test
    fun `transient mutation failure closes its batch and restores the previous native guard`() {
        withMediumInput { harness, _, probe, _, _, _ ->
            val editor = harness.editText
            for (previousGuard in listOf(false, true)) {
                editor.isApplyingRustState = previousGuard
                val failure = IllegalStateException(MUTATION_FAILURE)
                assertSame(
                    failure,
                    assertThrows(IllegalStateException::class.java) {
                        editor.runWithTransientInputMutationGuard {
                            editor.text.setSpan(probe, 0, 1, Spanned.SPAN_EXCLUSIVE_EXCLUSIVE)
                            throw failure
                        }
                    }
                )
                assertEquals(previousGuard, editor.isApplyingRustState)
                probe.scans = 0
                editor.text.setSpan(probe, 0, 2, Spanned.SPAN_EXCLUSIVE_EXCLUSIVE)
                assertEquals(
                    "a leaked batch must not suppress the next genuine span edit",
                    1,
                    probe.scans
                )
            }
            editor.isApplyingRustState = false
        }
    }

    @Test
    fun `publication failure closes its batch and restores the previous native guard`() {
        withMediumInput { harness, _, probe, _, _, _ ->
            val editor = harness.editText
            for (previousGuard in listOf(false, true)) {
                editor.isApplyingRustState = previousGuard
                val failure = IllegalStateException(PUBLICATION_FAILURE)
                probe.failure = failure
                assertSame(
                    failure,
                    assertThrows(IllegalStateException::class.java) {
                        editor.runWithTransientInputMutationGuard {
                            editor.text.setSpan(probe, 0, 1, Spanned.SPAN_EXCLUSIVE_EXCLUSIVE)
                            true
                        }
                    }
                )
                assertEquals(previousGuard, editor.isApplyingRustState)
                probe.scans = 0
                editor.text.setSpan(probe, 0, 2, Spanned.SPAN_EXCLUSIVE_EXCLUSIVE)
                assertEquals("failed publication must not leave a suspended batch", 1, probe.scans)
            }
            editor.isApplyingRustState = false
        }
    }

    @Test
    fun `retired input connection leaves the document and publication state unchanged`() {
        withMediumInput { harness, input, probe, recorder, _, _ ->
            val editor = harness.editText
            val replacement = editor.onCreateInputConnection(EditorInfo()) as EditorInputConnection
            try {
                probe.scans = 0
                recorder.selections.clear()
                val before = editor.text.toString()
                val revision = harness.adapter.baseDocumentRevision
                assertTrue(input.commitText(INSERTED_TEXT, 1))
                assertTrue(input.deleteSurroundingText(1, 0))
                assertEquals(before, editor.text.toString())
                assertEquals(revision, harness.adapter.baseDocumentRevision)
                assertEquals(0, probe.scans)
                assertTrue(recorder.selections.isEmpty())
            } finally {
                replacement.closeConnection()
            }
        }
    }

    private fun withMediumInput(
        action: (
            StructuredDeleteHarness,
            EditorInputConnection,
            PublicationProbe,
            EditorInputReportingTest.RecordingInputMethodManager,
            JSONObject,
            Int
        ) -> Unit
    ) {
        val context = RuntimeEnvironment.getApplication()
        val corpus = context.assets.open(EDITOR_PUBLIC_THEME_FIXTURES_ASSET).bufferedReader().use {
            JSONObject(it.readText())
        }
        val fixtures = corpus.getJSONArray("fixtures")
        val fixture = (0 until fixtures.length()).map { fixtures.getJSONObject(it) }
            .single { it.getString("id") == MEDIUM_FIXTURE }
        val harness = structuredDeleteHarness(fixture.getString("html"))
        val editor = harness.editText
        try {
            val theme = corpus.getJSONObject("theme").toString()
            editor.applyTheme(EditorTheme.fromJson(theme))
            assertNotNull(editor.theme?.styleSheet)
            assertEquals(fixture.getString("visibleText"), editor.text.toString())
            val caret = fixture.getJSONObject("caret")
            val paragraphs = fixture.getJSONArray("paragraphs")
            val offset = (0 until caret.getInt("paragraphIndex")).sumOf {
                paragraphs.getJSONObject(it).getString("text").length + 1
            } + caret.getInt("offsetUtf16")
            editor.setSelection(offset)
            val input = editor.onCreateInputConnection(EditorInfo()) as EditorInputConnection
            try {
                val manager = context.getSystemService(
                    Context.INPUT_METHOD_SERVICE
                ) as InputMethodManager
                val recorder = Shadow.extract<EditorInputReportingTest.RecordingInputMethodManager>(
                    manager
                )
                val probe = PublicationProbe()
                editor.text.setSpan(probe, 0, 1, Spanned.SPAN_EXCLUSIVE_EXCLUSIVE)
                probe.scans = 0
                editor.text.setSpan(probe, 0, 2, Spanned.SPAN_EXCLUSIVE_EXCLUSIVE)
                assertEquals(
                    "probe must observe a real non-batched input publication",
                    1,
                    probe.scans
                )
                probe.scans = 0
                recorder.selections.clear()
                action(harness, input, probe, recorder, fixture, offset)
            } finally {
                input.closeConnection()
            }
        } finally {
            harness.adapter.destroy()
        }
    }

    private fun assertFinalInputState(
        editor: EditorEditText,
        input: EditorInputConnection,
        recorder: EditorInputReportingTest.RecordingInputMethodManager,
        expected: String,
        caret: Int
    ) {
        assertEquals(caret, editor.selectionStart)
        assertEquals(caret, editor.selectionEnd)
        assertEquals(listOf(caret, caret, -1, -1), recorder.selections.single())
        assertEquals(-1, BaseInputConnection.getComposingSpanStart(editor.text))
        assertEquals(-1, BaseInputConnection.getComposingSpanEnd(editor.text))
        assertFalse(editor.hasDeferredRustUpdateApplicationForTesting())
        val extracted = input.getExtractedText(
            ExtractedTextRequest().apply { flags = InputConnection.GET_TEXT_WITH_STYLES },
            0
        )!!
        assertEquals(expected, extracted.text.toString())
        assertEquals(caret, extracted.selectionStart)
        assertEquals(caret, extracted.selectionEnd)
    }

    private fun recordUpdates(editor: EditorEditText): MutableList<String> {
        val previous = requireNotNull(editor.editorListener)
        val updates = mutableListOf<String>()
        editor.editorListener = object : EditorEditText.EditorListener by previous {
            override fun onEditorUpdate(updateJSON: String) {
                updates.add(updateJSON)
                previous.onEditorUpdate(updateJSON)
            }
        }
        return updates
    }

    private fun assertMarkedRanges(input: EditorInputConnection, paragraphs: JSONArray) {
        val content = input.getExtractedText(
            ExtractedTextRequest().apply { flags = InputConnection.GET_TEXT_WITH_STYLES },
            0
        )!!.text as Spanned
        var paragraphStart = 0
        for (index in 0 until paragraphs.length()) {
            val paragraph = paragraphs.getJSONObject(index)
            val marks = paragraph.getJSONArray("marks")
            for (markIndex in 0 until marks.length()) {
                val mark = marks.getJSONObject(markIndex)
                val start = paragraphStart + mark.getInt("startUtf16")
                val end = paragraphStart + mark.getInt("endUtf16")
                val styles = content.getSpans(start, end, EditorResolvedTextSpan::class.java)
                    .filter { content.getSpanStart(it) < end && content.getSpanEnd(it) > start }
                assertTrue(
                    "paragraph $index mark $markIndex must retain a resolved style",
                    styles.isNotEmpty()
                )
                var coveredUntil = start
                for (span in styles.sortedBy(content::getSpanStart)) {
                    assertTrue(
                        "paragraph $index mark $markIndex must not have an unstyled gap",
                        content.getSpanStart(span) <= coveredUntil
                    )
                    coveredUntil = maxOf(coveredUntil, content.getSpanEnd(span))
                }
                assertTrue(coveredUntil >= end)
                for (span in styles) {
                    when (mark.getString("type")) {
                        BOLD_MARK -> assertTrue(
                            span.style.typefaceStyle() and Typeface.BOLD != 0
                        )

                        ITALIC_MARK -> assertTrue(
                            span.style.typefaceStyle() and Typeface.ITALIC != 0
                        )

                        LINK_MARK -> {
                            assertEquals(Color.BLUE, span.style.color)
                            assertTrue(
                                span.style.textDecorationLine.orEmpty().contains(
                                    UNDERLINE_DECORATION
                                )
                            )
                        }
                    }
                }
                if (mark.getString("type") == LINK_MARK) {
                    val href = mark.getJSONObject("attrs").getString("href")
                    assertTrue(
                        content.getSpans(start, end, Annotation::class.java).any {
                            it.key == RenderBridge.NATIVE_LINK_HREF_ANNOTATION &&
                                it.value == href &&
                                content.getSpanStart(it) <= start &&
                                content.getSpanEnd(it) >= end
                        }
                    )
                }
            }
            paragraphStart += paragraph.getString("text").length + 1
        }
    }

    private companion object {
        const val PROBE_KEY = "input-publication-probe"
        const val PROBE_VALUE = "retained"
        const val INPUT_REPORTING_CLASS = "com.apollohg.editor.EditorInputSnapshotsKt"
        const val INPUT_REPORTING_METHOD = "publishInputStateIfNeeded"
        const val MEDIUM_FIXTURE = "medium"
        const val INSERTED_TEXT = "!"
        const val COMPLETED_MUTATION_BOUNDARIES = 2
        const val MARKER_VALUE = "ordered"
        const val MARKER_LENGTH = 2
        const val PLAIN_TEXT = "plain text"
        const val MARKER_TEXT = "1. "
        const val MUTATION_FAILURE = "mutation failure"
        const val PUBLICATION_FAILURE = "publication failure"
        const val BOLD_MARK = "bold"
        const val ITALIC_MARK = "italic"
        const val LINK_MARK = "link"
        const val UNDERLINE_DECORATION = "underline"
    }
}
