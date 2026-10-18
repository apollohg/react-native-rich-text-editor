package com.apollohg.editor

import android.graphics.Typeface
import android.os.Looper
import android.text.TextPaint
import android.text.style.CharacterStyle
import android.view.inputmethod.BaseInputConnection
import android.view.inputmethod.CorrectionInfo
import android.view.inputmethod.EditorInfo
import org.json.JSONObject
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertSame
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.Shadows.shadowOf
import org.robolectric.annotation.Config

@RunWith(RobolectricTestRunner::class)
@Config(sdk = [34])
internal class EditorInputConnectionDeferredCompositionTest : EditorInputConnectionTestFixture() {
    @Test
    fun `accepted deferred render preserves a reopened newer composition until its candidate`() {
        withBoldInput { harness, input, listener ->
            val editText = harness.editText
            val initialRevision = harness.adapter.baseDocumentRevision
            val initialGeneration = editText.inputConnectionGenerationForTesting()
            val originalRenderRevision = editText.currentRenderBlocksDocumentVersion
            assertTrue(input.setComposingText(FIRST_READING, 1))
            assertTrue(input.finishComposingText())
            assertAuthoritativeBoldText(
                harness,
                committedFirstReading(),
                COMPOSING_START + FIRST_READING.length,
                initialRevision + 1uL
            )
            assertTrue(editText.hasDeferredRustUpdateApplicationForTesting())
            assertEquals(originalRenderRevision, editText.currentRenderBlocksDocumentVersion)
            assertTrue(listener.receivedUpdates.isEmpty())

            reopenReading(input)
            assertEquals(readingText(), editText.text.toString())
            assertEquals(
                COMPOSING_START to COMPOSING_START + FIRST_READING.length,
                editText.compositionReplacementRange()
            )
            assertEquals(COMPOSING_START, BaseInputConnection.getComposingSpanStart(editText.text))
            assertEquals(
                COMPOSING_START + READING.length,
                BaseInputConnection.getComposingSpanEnd(editText.text)
            )
            val replacementRevision = editText.composingReplacementAuthorizedTextRevision
            val coreBeforeIdle = harness.adapter.cachedAtomicRenderJson
            shadowOf(Looper.getMainLooper()).idle()

            val textAfterIdle = editText.text.toString()
            val rangeAfterIdle = editText.compositionReplacementRange()
            val generationAfterIdle = editText.inputConnectionGenerationForTesting()
            val originalInputAfterIdle = editText.activeInputConnection === input
            val pendingAfterIdle = editText.hasDeferredRustUpdateApplicationForTesting()
            val coreAfterIdle = harness.adapter.cachedAtomicRenderJson
            val authorizedRevisionAfterIdle = editText.lastAuthorizedTextRevision
            val updatesAfterIdle = listener.receivedUpdates.toList()
            val candidateInput = editText.activeInputConnection
                ?: requireNotNull(editText.onCreateInputConnection(EditorInfo()))
            assertTrue(candidateInput.commitText(CANDIDATE, 1))
            shadowOf(Looper.getMainLooper()).idle()

            assertEquals(
                "candidate must replace the reopened reading; afterIdle=" +
                    "$textAfterIdle range=$rangeAfterIdle generation=$generationAfterIdle " +
                    "sameInput=$originalInputAfterIdle pending=$pendingAfterIdle " +
                    "trace=${editText.imeTraceSnapshotForTesting()}",
                candidateText(),
                editText.text.toString()
            )
            assertEquals(readingText(), textAfterIdle)
            assertEquals(
                COMPOSING_START to COMPOSING_START + FIRST_READING.length,
                rangeAfterIdle
            )
            assertEquals(initialGeneration, generationAfterIdle)
            assertTrue(originalInputAfterIdle)
            assertTrue(pendingAfterIdle)
            assertEquals(coreBeforeIdle, coreAfterIdle)
            assertTrue(updatesAfterIdle.isEmpty())
            assertEquals(replacementRevision, authorizedRevisionAfterIdle)
            assertSame(input, editText.activeInputConnection)
            assertAuthoritativeBoldText(
                harness,
                candidateText(),
                COMPOSING_START + CANDIDATE.length,
                initialRevision + 2uL
            )
            assertFalse(editText.hasDeferredRustUpdateApplicationForTesting())
            assertEquals(
                harness.adapter.baseDocumentRevision.toString(),
                editText.currentRenderBlocksDocumentVersion
            )
            assertEquals(1, listener.receivedUpdates.size)
            assertEquals(
                harness.adapter.baseDocumentRevision.toString(),
                JSONObject(listener.receivedUpdates.single()).getString("documentVersion")
            )
            assertEquals(-1, BaseInputConnection.getComposingSpanStart(editText.text))
            assertEquals(-1, BaseInputConnection.getComposingSpanEnd(editText.text))
        }
    }

    @Test
    fun `ordinary composition finish still flushes its accepted frame on the next main turn`() {
        withBoldInput { harness, input, listener ->
            val initialRevision = harness.adapter.baseDocumentRevision
            assertTrue(input.setComposingText(FIRST_READING, 1))
            assertTrue(input.finishComposingText())
            assertTrue(harness.editText.hasDeferredRustUpdateApplicationForTesting())
            assertTrue(listener.receivedUpdates.isEmpty())
            shadowOf(Looper.getMainLooper()).idle()
            assertFalse(harness.editText.hasDeferredRustUpdateApplicationForTesting())
            assertSame(input, harness.editText.activeInputConnection)
            assertAuthoritativeBoldText(
                harness,
                committedFirstReading(),
                COMPOSING_START + FIRST_READING.length,
                initialRevision + 1uL
            )
            assertEquals(1, listener.receivedUpdates.size)
        }
    }

    @Test
    fun `candidate before the deferred turn supersedes the old generation with its real frame`() {
        withBoldInput { harness, input, listener ->
            val initialRevision = harness.adapter.baseDocumentRevision
            assertTrue(input.setComposingText(FIRST_READING, 1))
            assertTrue(input.finishComposingText())
            val oldGeneration = harness.editText.deferredRustUpdateGeneration
            reopenReading(input)
            assertTrue(input.commitText(CANDIDATE, 1))
            assertTrue(harness.editText.deferredRustUpdateGeneration > oldGeneration)
            assertTrue(harness.editText.hasDeferredRustUpdateApplicationForTesting())
            assertTrue(listener.receivedUpdates.isEmpty())
            shadowOf(Looper.getMainLooper()).idle()
            assertAuthoritativeBoldText(
                harness,
                candidateText(),
                COMPOSING_START + CANDIDATE.length,
                initialRevision + 2uL
            )
            assertSame(input, harness.editText.activeInputConnection)
            assertFalse(harness.editText.hasDeferredRustUpdateApplicationForTesting())
            assertEquals(1, listener.receivedUpdates.size)
            assertTrue(
                harness.editText.imeTraceSnapshotForTesting().any {
                    it.startsWith("rustUpdateDeferredSkip:reason=generation")
                }
            )
        }
    }

    @Test
    fun `Samsung finish then space preserves the same connection and both accepted mutations`() {
        withBoldInput { harness, input, listener ->
            val initialRevision = harness.adapter.baseDocumentRevision
            assertTrue(input.setComposingText(FIRST_READING, 1))
            assertTrue(input.finishComposingText())
            assertTrue(harness.editText.hasDeferredRustUpdateApplicationForTesting())
            assertTrue(input.commitText(SPACE, 1))
            assertSame(input, harness.editText.activeInputConnection)
            shadowOf(Looper.getMainLooper()).idle()
            assertAuthoritativeBoldText(
                harness,
                PREFIX + FIRST_READING + SPACE + SUFFIX,
                COMPOSING_START + FIRST_READING.length + SPACE.length,
                initialRevision + 2uL
            )
            assertFalse(harness.editText.hasDeferredRustUpdateApplicationForTesting())
            assertEquals(1, listener.receivedUpdates.size)
        }
    }

    @Test
    fun `null candidate after held unchanged reading publishes the accepted frame`() {
        withHeldReopenedComposition(FIRST_READING) { harness, input, listener, initialRevision ->
            val editText = harness.editText
            assertEquals(editText.lastAuthorizedText, editText.text.toString())
            assertTrue(input.commitText(null, 1))
            shadowOf(Looper.getMainLooper()).idle()

            assertAuthoritativeBoldText(
                harness,
                committedFirstReading(),
                COMPOSING_START + FIRST_READING.length,
                initialRevision + 1uL
            )
            assertAcceptedRenderPublished(harness, input, listener)
        }
    }

    @Test
    fun `finish after held newer reading publishes its real replacement frame`() {
        withHeldReopenedComposition(READING) { harness, input, listener, initialRevision ->
            assertTrue(input.finishComposingText())
            shadowOf(Looper.getMainLooper()).idle()

            assertAuthoritativeBoldText(
                harness,
                readingText(),
                COMPOSING_START + READING.length,
                initialRevision + 2uL
            )
            assertAcceptedRenderPublished(harness, input, listener)
        }
    }

    @Test
    fun `no-op finish after held unchanged reading publishes the accepted frame`() {
        withHeldReopenedComposition(FIRST_READING) { harness, input, listener, initialRevision ->
            assertTrue(input.finishComposingText())
            shadowOf(Looper.getMainLooper()).idle()

            assertAuthoritativeBoldText(
                harness,
                committedFirstReading(),
                COMPOSING_START + FIRST_READING.length,
                initialRevision + 1uL
            )
            assertAcceptedRenderPublished(harness, input, listener)
        }
    }

    @Test
    fun `posted correction after held newer reading publishes the candidate frame`() {
        withHeldReopenedComposition(READING) { harness, input, listener, initialRevision ->
            assertTrue(input.commitCorrection(CorrectionInfo(COMPOSING_START, READING, CANDIDATE)))
            assertEquals(readingText(), harness.editText.text.toString())
            assertEquals(boldHtml(committedFirstReading()), harness.adapter.documentHtml())
            assertTrue(listener.receivedUpdates.isEmpty())
            shadowOf(Looper.getMainLooper()).idle()

            assertAuthoritativeBoldText(
                harness,
                candidateText(),
                COMPOSING_START + CANDIDATE.length,
                initialRevision + 2uL
            )
            assertAcceptedRenderPublished(harness, input, listener)
        }
    }

    private fun withHeldReopenedComposition(
        reading: String,
        block: (
            RealExternalCompositionHarness,
            EditorInputConnection,
            RecordingEditorListener,
            ULong
        ) -> Unit
    ) {
        withBoldInput { harness, input, listener ->
            val editText = harness.editText
            val initialRevision = harness.adapter.baseDocumentRevision
            val initialGeneration = editText.inputConnectionGenerationForTesting()
            val originalRenderRevision = editText.currentRenderBlocksDocumentVersion
            assertTrue(input.setComposingText(FIRST_READING, 1))
            assertTrue(input.finishComposingText())
            assertAuthoritativeBoldText(
                harness,
                committedFirstReading(),
                COMPOSING_START + FIRST_READING.length,
                initialRevision + 1uL
            )
            assertTrue(editText.hasDeferredRustUpdateApplicationForTesting())
            assertTrue(listener.receivedUpdates.isEmpty())
            reopenReading(input, reading)
            val replacementRevision = editText.composingReplacementAuthorizedTextRevision
            val acceptedFrame = harness.adapter.cachedAtomicRenderJson
            shadowOf(Looper.getMainLooper()).idle()

            assertEquals(PREFIX + reading + SUFFIX, editText.text.toString())
            assertEquals(
                COMPOSING_START to COMPOSING_START + FIRST_READING.length,
                editText.compositionReplacementRange()
            )
            assertEquals(COMPOSING_START, BaseInputConnection.getComposingSpanStart(editText.text))
            assertEquals(
                COMPOSING_START + reading.length,
                BaseInputConnection.getComposingSpanEnd(editText.text)
            )
            assertEquals(initialGeneration, editText.inputConnectionGenerationForTesting())
            assertSame(input, editText.activeInputConnection)
            assertTrue(editText.hasDeferredRustUpdateApplicationForTesting())
            assertEquals(originalRenderRevision, editText.currentRenderBlocksDocumentVersion)
            assertEquals(replacementRevision, editText.lastAuthorizedTextRevision)
            assertEquals(acceptedFrame, harness.adapter.cachedAtomicRenderJson)
            assertTrue(listener.receivedUpdates.isEmpty())
            block(harness, input, listener, initialRevision)
        }
    }

    private fun assertAcceptedRenderPublished(
        harness: RealExternalCompositionHarness,
        input: EditorInputConnection,
        listener: RecordingEditorListener
    ) {
        val editText = harness.editText
        assertFalse(
            "accepted frame must not remain held after composition ends: " +
                "${editText.imeTraceSnapshotForTesting()}",
            editText.hasDeferredRustUpdateApplicationForTesting()
        )
        assertSame(input, editText.activeInputConnection)
        assertEquals(
            harness.adapter.baseDocumentRevision.toString(),
            editText.currentRenderBlocksDocumentVersion
        )
        assertEquals(1, listener.receivedUpdates.size)
        assertEquals(
            harness.adapter.baseDocumentRevision.toString(),
            JSONObject(listener.receivedUpdates.single()).getString("documentVersion")
        )
        assertEquals(-1, BaseInputConnection.getComposingSpanStart(editText.text))
        assertEquals(-1, BaseInputConnection.getComposingSpanEnd(editText.text))
    }

    private fun withBoldInput(
        block: (
            RealExternalCompositionHarness,
            EditorInputConnection,
            RecordingEditorListener
        ) -> Unit
    ) {
        val harness = realExternalCompositionHarness(INITIAL_TEXT)
        try {
            val update = requireNotNull(harness.adapter.setContentHtml(boldHtml(INITIAL_TEXT)))
            assertTrue(harness.editText.applyUpdateJSON(update, notifyListener = false))
            harness.editText.setSelection(COMPOSING_START)
            val input = requireNotNull(harness.editText.onCreateInputConnection(EditorInfo()))
                as EditorInputConnection
            shadowOf(Looper.getMainLooper()).idle()
            harness.editText.clearImeTraceForTesting()
            val listener = RecordingEditorListener()
            harness.editText.editorListener = listener
            block(harness, input, listener)
        } finally {
            harness.editText.editorListener = null
            harness.editText.cancelDeferredRustUpdateApplication()
            harness.adapter.destroy()
        }
    }

    private fun reopenReading(input: EditorInputConnection, reading: String = READING) {
        assertTrue(
            input.setComposingRegion(COMPOSING_START, COMPOSING_START + FIRST_READING.length)
        )
        assertTrue(input.setComposingText(reading, 1))
    }

    private fun assertAuthoritativeBoldText(
        harness: RealExternalCompositionHarness,
        expectedText: String,
        expectedCaret: Int,
        expectedRevision: ULong
    ) {
        val editText = harness.editText
        assertEquals(expectedText, editText.text.toString())
        assertEquals(expectedCaret, editText.selectionStart)
        assertEquals(expectedCaret, editText.selectionEnd)
        assertEquals(boldHtml(expectedText), harness.adapter.documentHtml())
        assertEquals(expectedRevision, harness.adapter.baseDocumentRevision)
        val snapshot = JSONObject(requireNotNull(harness.adapter.cachedAtomicRenderJson))
        assertEquals(ATOMIC_RENDER_SNAPSHOT_KEYS, snapshot.keys().asSequence().toSet())
        assertEquals(expectedRevision.toString(), snapshot.getString("documentVersion"))
        val expectedScalarCaret = expectedText.codePointCount(0, expectedCaret)
        val selection = snapshot.getJSONObject("selection")
        assertEquals(expectedScalarCaret, selection.getInt("anchorScalar"))
        assertEquals(expectedScalarCaret, selection.getInt("headScalar"))
        assertNotNull(harness.adapter.positionEpoch)
        val content = requireNotNull(editText.text)
        for (offset in content.indices) {
            val paint = TextPaint(editText.paint)
            content.getSpans(offset, offset + 1, CharacterStyle::class.java)
                .forEach { it.updateDrawState(paint) }
            assertTrue(
                "actual draw style must remain bold at UTF16 $offset in $expectedText",
                (paint.typeface?.style ?: Typeface.NORMAL) and Typeface.BOLD != 0
            )
        }
    }

    private fun boldHtml(text: String): String = "<p><strong>$text</strong></p>"
    private fun committedFirstReading(): String = PREFIX + FIRST_READING + SUFFIX
    private fun readingText(): String = PREFIX + READING + SUFFIX
    private fun candidateText(): String = PREFIX + CANDIDATE + SUFFIX

    private companion object {
        const val PREFIX = "L😀"
        const val SUFFIX = "R"
        const val INITIAL_TEXT = PREFIX + SUFFIX
        const val FIRST_READING = "に"
        const val READING = "にほん"
        const val CANDIDATE = "日本"
        const val SPACE = " "
        val COMPOSING_START = PREFIX.length
    }
}
