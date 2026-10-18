package com.apollohg.editor

import android.view.inputmethod.BaseInputConnection
import android.view.inputmethod.EditorInfo
import android.view.inputmethod.ExtractedTextRequest
import android.view.inputmethod.InputConnection
import org.json.JSONObject
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config

@RunWith(RobolectricTestRunner::class)
@Config(sdk = [34])
internal class EditorModernInputConnectionTest : EditorInputConnectionTestFixture() {
    @Test
    fun `rejected multiline composition restores authorized display`() {
        val harness = realExternalCompositionHarness(
            COMPOSITION_INITIAL_TEXT,
            compositionOperationLimitConfig(REJECTED_COMPACT_OPERATION_LIMIT)
        )
        try {
            val editor = harness.editText
            val errors = mutableListOf<EditorV2Error>()
            harness.adapter.onAutonomousError = { errors.add(it) }
            val revision = harness.adapter.baseDocumentRevision
            val input = requireNotNull(editor.onCreateInputConnection(EditorInfo()))
            input.setSelection(0, COMPOSITION_INITIAL_TEXT.length)
            input.setComposingText(MULTILINE_COMPOSITION_TEXT, 1)
            input.commitText(MULTILINE_COMPOSITION_TEXT, 1)
            org.robolectric.Shadows.shadowOf(android.os.Looper.getMainLooper()).idle()
            assertTrue(
                "errors=${errors.map { it.code }} html=${harness.adapter.documentHtml()} " +
                    "revision=${harness.adapter.baseDocumentRevision} before=$revision " +
                    "visible=${editor.text} composing=${BaseInputConnection.getComposingSpanStart(
                        editor.editableText
                    )}",
                errors.any { it.code == OPERATION_LIMIT_ERROR }
            )
            assertEquals(listOf(OPERATION_LIMIT_ERROR), errors.map { it.code })
            assertEquals(COMPOSITION_INITIAL_HTML, harness.adapter.documentHtml())
            assertEquals(revision, harness.adapter.baseDocumentRevision)
            assertEquals(COMPOSITION_INITIAL_TEXT, editor.text.toString())
            assertEquals(-1, BaseInputConnection.getComposingSpanStart(editor.editableText))
            assertEquals(-1, BaseInputConnection.getComposingSpanEnd(editor.editableText))
            assertEquals(0, editor.selectionStart)
            assertEquals(COMPOSITION_INITIAL_TEXT.length, editor.selectionEnd)
        } finally {
            harness.adapter.destroy()
        }
    }

    @Test
    fun `compact multiline composition commits at its exact compiled operation budget`() {
        val harness = realExternalCompositionHarness(
            COMPOSITION_INITIAL_TEXT,
            compositionOperationLimitConfig(ACCEPTED_COMPACT_OPERATION_LIMIT)
        )
        try {
            val editor = harness.editText
            val listener = RecordingEditorListener()
            editor.editorListener = listener
            val errors = mutableListOf<EditorV2Error>()
            harness.adapter.onAutonomousError = { errors.add(it) }
            val revision = harness.adapter.baseDocumentRevision
            val input = requireNotNull(editor.onCreateInputConnection(EditorInfo()))
            try {
                assertTrue(input.setSelection(0, COMPOSITION_INITIAL_TEXT.length))
                assertTrue(input.setComposingText(MULTILINE_COMPOSITION_TEXT, 1))
                assertEquals(COMPOSITION_INITIAL_HTML, harness.adapter.documentHtml())
                assertEquals(revision, harness.adapter.baseDocumentRevision)
                assertTrue(listener.receivedUpdates.isEmpty())
                assertTrue(input.commitText(MULTILINE_COMPOSITION_TEXT, 1))
                org.robolectric.Shadows.shadowOf(android.os.Looper.getMainLooper()).idle()
                assertTrue(errors.toString(), errors.isEmpty())
                assertEquals(MULTILINE_COMPOSITION_HTML, harness.adapter.documentHtml())
                assertEquals(revision + 1uL, harness.adapter.baseDocumentRevision)
                assertEquals(MULTILINE_COMPOSITION_TEXT, editor.text.toString())
                assertEquals(MULTILINE_COMPOSITION_TEXT.length, editor.selectionStart)
                assertEquals(MULTILINE_COMPOSITION_TEXT.length, editor.selectionEnd)
                assertEquals(-1, BaseInputConnection.getComposingSpanStart(editor.editableText))
                assertEquals(-1, BaseInputConnection.getComposingSpanEnd(editor.editableText))
                assertEquals(1, listener.receivedUpdates.size)
                assertEquals(
                    (revision + 1uL).toString(),
                    JSONObject(listener.receivedUpdates.single()).getString("documentVersion")
                )
                val extracted = requireNotNull(input.getExtractedText(ExtractedTextRequest(), 0))
                assertEquals(MULTILINE_COMPOSITION_TEXT, extracted.text.toString())
                assertEquals(MULTILINE_COMPOSITION_TEXT.length, extracted.selectionStart)
                assertEquals(MULTILINE_COMPOSITION_TEXT.length, extracted.selectionEnd)
            } finally {
                input.closeConnection()
            }
        } finally {
            harness.adapter.destroy()
        }
    }

    @Test
    fun `rejected multiline composition restores reversed authoritative selection`() {
        val harness = realExternalCompositionHarness(
            COMPOSITION_INITIAL_TEXT,
            compositionOperationLimitConfig(REJECTED_COMPACT_OPERATION_LIMIT)
        )
        try {
            val editor = harness.editText
            val errors = mutableListOf<EditorV2Error>()
            harness.adapter.onAutonomousError = { errors.add(it) }
            val revision = harness.adapter.baseDocumentRevision
            val input = requireNotNull(editor.onCreateInputConnection(EditorInfo()))
            try {
                assertTrue(input.setSelection(COMPOSITION_INITIAL_TEXT.length, 0))
                commitMultilineComposition(input)
                assertEquals(listOf(OPERATION_LIMIT_ERROR), errors.map { it.code })
                assertEquals(COMPOSITION_INITIAL_HTML, harness.adapter.documentHtml())
                assertEquals(revision, harness.adapter.baseDocumentRevision)
                assertEquals(COMPOSITION_INITIAL_TEXT, editor.text.toString())
                assertEquals(-1, BaseInputConnection.getComposingSpanStart(editor.editableText))
                assertEquals(COMPOSITION_INITIAL_TEXT.length, editor.selectionStart)
                assertEquals(0, editor.selectionEnd)
                val extracted = requireNotNull(input.getExtractedText(ExtractedTextRequest(), 0))
                assertEquals(COMPOSITION_INITIAL_TEXT, extracted.text.toString())
                assertEquals(COMPOSITION_INITIAL_TEXT.length, extracted.selectionStart)
                assertEquals(0, extracted.selectionEnd)
            } finally {
                input.closeConnection()
            }
        } finally {
            harness.adapter.destroy()
        }
    }

    @Test
    fun `rejected multiline composition adopts selection changed by its error callback`() {
        val harness = realExternalCompositionHarness(
            COMPOSITION_INITIAL_TEXT,
            compositionOperationLimitConfig(REJECTED_COMPACT_OPERATION_LIMIT)
        )
        try {
            val editor = harness.editText
            val errors = mutableListOf<EditorV2Error>()
            harness.adapter.onAutonomousError = {
                errors.add(it)
                if (it.code == OPERATION_LIMIT_ERROR) {
                    assertNotNull(
                        harness.adapter.syncSelection(
                            CALLBACK_SELECTION_END,
                            CALLBACK_SELECTION_START
                        )
                    )
                }
            }
            val revision = harness.adapter.baseDocumentRevision
            val input = requireNotNull(editor.onCreateInputConnection(EditorInfo()))
            try {
                assertTrue(input.setSelection(0, COMPOSITION_INITIAL_TEXT.length))
                commitMultilineComposition(input)
                assertEquals(listOf(OPERATION_LIMIT_ERROR), errors.map { it.code })
                assertEquals(COMPOSITION_INITIAL_HTML, harness.adapter.documentHtml())
                assertEquals(revision, harness.adapter.baseDocumentRevision)
                assertEquals(COMPOSITION_INITIAL_TEXT, editor.text.toString())
                assertEquals(CALLBACK_SELECTION_END, editor.selectionStart)
                assertEquals(CALLBACK_SELECTION_START, editor.selectionEnd)
                assertEquals(-1, BaseInputConnection.getComposingSpanStart(editor.editableText))
            } finally {
                input.closeConnection()
            }
        } finally {
            harness.adapter.destroy()
        }
    }

    @Test
    fun `rejected multiline composition preserves document changed by its error callback`() {
        val harness = realExternalCompositionHarness(
            COMPOSITION_INITIAL_TEXT,
            compositionOperationLimitConfig(REJECTED_COMPACT_OPERATION_LIMIT)
        )
        try {
            val editor = harness.editText
            val errors = mutableListOf<EditorV2Error>()
            var callbackRevision: ULong? = null
            harness.adapter.onAutonomousError = {
                errors.add(it)
                if (it.code == OPERATION_LIMIT_ERROR) {
                    val update =
                        requireNotNull(harness.adapter.setContentHtml(CALLBACK_DOCUMENT_HTML))
                    assertTrue(editor.applyUpdateJSON(update, notifyListener = false))
                    editor.setSelection(CALLBACK_SELECTION_END, CALLBACK_SELECTION_START)
                    callbackRevision = harness.adapter.baseDocumentRevision
                }
            }
            val input = requireNotNull(editor.onCreateInputConnection(EditorInfo()))
            try {
                assertTrue(input.setSelection(0, COMPOSITION_INITIAL_TEXT.length))
                commitMultilineComposition(input)
                assertEquals(listOf(OPERATION_LIMIT_ERROR), errors.map { it.code })
                assertEquals(CALLBACK_DOCUMENT_HTML, harness.adapter.documentHtml())
                assertEquals(callbackRevision, harness.adapter.baseDocumentRevision)
                assertEquals(CALLBACK_DOCUMENT_TEXT, editor.text.toString())
                assertEquals(CALLBACK_SELECTION_END, editor.selectionStart)
                assertEquals(CALLBACK_SELECTION_START, editor.selectionEnd)
                assertEquals(-1, BaseInputConnection.getComposingSpanStart(editor.editableText))
            } finally {
                input.closeConnection()
            }
        } finally {
            harness.adapter.destroy()
        }
    }

    @Test
    fun `rejected multiline composition cannot recover over a rebound editor`() {
        val harness = realExternalCompositionHarness(
            COMPOSITION_INITIAL_TEXT,
            compositionOperationLimitConfig(REJECTED_COMPACT_OPERATION_LIMIT)
        )
        val replacement = realExternalCompositionHarness(CALLBACK_DOCUMENT_TEXT)
        try {
            val editor = harness.editText
            val errors = mutableListOf<EditorV2Error>()
            var oldRenderCountAtRebind: Int? = null
            harness.adapter.onAutonomousError = {
                errors.add(it)
                if (it.code == OPERATION_LIMIT_ERROR) {
                    editor.retireInputConnectionForEditor()
                    editor.editorId = REBOUND_VIEW_TOKEN
                    editor.v2Driver = replacement.adapter
                    val update = requireNotNull(replacement.adapter.refreshFromRustState(null))
                    assertTrue(editor.applyUpdateJSON(update, notifyListener = false))
                    editor.setSelection(CALLBACK_SELECTION_END, CALLBACK_SELECTION_START)
                    oldRenderCountAtRebind = harness.adapter.renderUpdateCallCountForTesting
                }
            }
            val input = requireNotNull(editor.onCreateInputConnection(EditorInfo()))
            try {
                assertTrue(input.setSelection(0, COMPOSITION_INITIAL_TEXT.length))
                commitMultilineComposition(input)
                assertEquals(listOf(OPERATION_LIMIT_ERROR), errors.map { it.code })
                assertEquals(CALLBACK_DOCUMENT_HTML, replacement.adapter.documentHtml())
                assertEquals(CALLBACK_DOCUMENT_TEXT, editor.text.toString())
                assertEquals(CALLBACK_SELECTION_END, editor.selectionStart)
                assertEquals(CALLBACK_SELECTION_START, editor.selectionEnd)
                assertEquals(
                    oldRenderCountAtRebind,
                    harness.adapter.renderUpdateCallCountForTesting
                )
                assertTrue(input.commitText("retired", 1))
                assertEquals(CALLBACK_DOCUMENT_HTML, replacement.adapter.documentHtml())
            } finally {
                input.closeConnection()
            }
        } finally {
            harness.adapter.destroy()
            replacement.adapter.destroy()
        }
    }

    @Test
    fun `rejected multiline composition cannot restore text after its editor is destroyed`() {
        val harness = realExternalCompositionHarness(
            COMPOSITION_INITIAL_TEXT,
            compositionOperationLimitConfig(REJECTED_COMPACT_OPERATION_LIMIT)
        )
        try {
            val editor = harness.editText
            val errors = mutableListOf<EditorV2Error>()
            var textAtDestroy: String? = null
            var oldRenderCountAtDestroy: Int? = null
            harness.adapter.onAutonomousError = {
                errors.add(it)
                if (it.code == OPERATION_LIMIT_ERROR) {
                    editor.retireInputConnectionForEditor()
                    editor.editorId = 0
                    editor.v2Driver = null
                    harness.adapter.destroy()
                    textAtDestroy = editor.text.toString()
                    oldRenderCountAtDestroy = harness.adapter.renderUpdateCallCountForTesting
                }
            }
            val input = requireNotNull(editor.onCreateInputConnection(EditorInfo()))
            try {
                assertTrue(input.setSelection(0, COMPOSITION_INITIAL_TEXT.length))
                commitMultilineComposition(input)
                assertEquals(listOf(OPERATION_LIMIT_ERROR), errors.map { it.code })
                assertEquals(textAtDestroy, editor.text.toString())
                assertEquals(
                    oldRenderCountAtDestroy,
                    harness.adapter.renderUpdateCallCountForTesting
                )
                assertTrue(input.commitText("retired", 1))
                assertEquals(textAtDestroy, editor.text.toString())
            } finally {
                input.closeConnection()
            }
        } finally {
            harness.adapter.destroy()
        }
    }

    @Test
    fun `unavailable multiline recovery restores unchanged directional authority`() {
        val backend = UnavailableRecoveryBackend()
        val harness = realExternalCompositionHarness(
            COMPOSITION_INITIAL_TEXT,
            compositionOperationLimitConfig(REJECTED_COMPACT_OPERATION_LIMIT),
            backend = backend
        )
        try {
            val editor = harness.editText
            val errors = mutableListOf<EditorV2Error>()
            harness.adapter.onAutonomousError = {
                errors.add(it)
                if (it.code == OPERATION_LIMIT_ERROR) backend.recoveryUnavailable = true
            }
            val revision = harness.adapter.baseDocumentRevision
            val input = requireNotNull(editor.onCreateInputConnection(EditorInfo()))
            try {
                assertTrue(input.setSelection(COMPOSITION_INITIAL_TEXT.length, 0))
                commitMultilineComposition(input)
                assertEquals(COMPOSITION_INITIAL_TEXT, editor.text.toString())
                assertEquals(COMPOSITION_INITIAL_TEXT.length, editor.selectionStart)
                assertEquals(0, editor.selectionEnd)
                assertEquals(COMPOSITION_INITIAL_HTML, harness.adapter.documentHtml())
                assertEquals(revision, harness.adapter.baseDocumentRevision)
                assertEquals(-1, BaseInputConnection.getComposingSpanStart(editor.editableText))
                assertEquals(
                    listOf(OPERATION_LIMIT_ERROR, RECOVERY_ERROR.code),
                    errors.map {
                        it.code
                    }
                )
                assertEquals(1, backend.unavailableRenderCount)
            } finally {
                input.closeConnection()
            }
        } finally {
            harness.adapter.destroy()
        }
    }

    @Test
    fun `unavailable multiline recovery cannot overwrite callback changed authority`() {
        val backend = UnavailableRecoveryBackend()
        val harness = realExternalCompositionHarness(
            COMPOSITION_INITIAL_TEXT,
            compositionOperationLimitConfig(REJECTED_COMPACT_OPERATION_LIMIT),
            backend = backend
        )
        try {
            val editor = harness.editText
            val errors = mutableListOf<EditorV2Error>()
            harness.adapter.onAutonomousError = {
                errors.add(it)
                if (it.code == OPERATION_LIMIT_ERROR) {
                    assertNotNull(
                        harness.adapter.syncSelection(
                            CALLBACK_SELECTION_END,
                            CALLBACK_SELECTION_START
                        )
                    )
                    backend.recoveryUnavailable = true
                }
            }
            val revision = harness.adapter.baseDocumentRevision
            val input = requireNotNull(editor.onCreateInputConnection(EditorInfo()))
            try {
                assertTrue(input.setSelection(0, COMPOSITION_INITIAL_TEXT.length))
                commitMultilineComposition(input)
                assertEquals(MULTILINE_COMPOSITION_TEXT, editor.text.toString())
                assertEquals(MULTILINE_COMPOSITION_TEXT.length, editor.selectionStart)
                assertEquals(editor.selectionStart, editor.selectionEnd)
                assertEquals(COMPOSITION_INITIAL_HTML, harness.adapter.documentHtml())
                assertEquals(revision, harness.adapter.baseDocumentRevision)
                assertEquals(
                    listOf(OPERATION_LIMIT_ERROR, RECOVERY_ERROR.code),
                    errors.map {
                        it.code
                    }
                )
                assertEquals(1, backend.unavailableRenderCount)
                assertTrue(
                    requireNotNull(harness.adapter.cachedAuthoritativeScalarSelection)
                        .contentEquals(intArrayOf(CALLBACK_SELECTION_END, CALLBACK_SELECTION_START))
                )
            } finally {
                input.closeConnection()
            }
        } finally {
            harness.adapter.destroy()
        }
    }

    @Test
    fun `multiline rejection preserves accepted native input and listener rebinding`() {
        for (rebindOnUpdate in listOf(false, true)) {
            val harness = realExternalCompositionHarness(
                COMPOSITION_INITIAL_TEXT,
                compositionOperationLimitConfig(REJECTED_COMPACT_OPERATION_LIMIT)
            )
            val replacement = realExternalCompositionHarness(CALLBACK_DOCUMENT_TEXT)
            try {
                val editor = harness.editText
                val errors = mutableListOf<EditorV2Error>()
                val updates = mutableListOf<String>()
                var queuedAcceptedUpdate = false
                var renderCountAtListenerRebind: Int? = null
                editor.editorListener = object : EditorEditText.EditorListener {
                    override fun onSelectionChanged(anchor: Int, head: Int) = Unit

                    override fun onEditorUpdate(updateJSON: String) {
                        updates.add(updateJSON)
                        if (rebindOnUpdate) {
                            editor.retireInputConnectionForEditor()
                            editor.editorId = REBOUND_VIEW_TOKEN
                            editor.v2Driver = replacement.adapter
                            val update =
                                requireNotNull(replacement.adapter.refreshFromRustState(null))
                            assertTrue(editor.applyUpdateJSON(update, notifyListener = false))
                            editor.setSelection(CALLBACK_SELECTION_END, CALLBACK_SELECTION_START)
                            renderCountAtListenerRebind =
                                harness.adapter.renderUpdateCallCountForTesting
                        }
                    }
                }
                harness.adapter.onAutonomousError = {
                    errors.add(it)
                    if (it.code == OPERATION_LIMIT_ERROR) {
                        editor.insertTextInRust(
                            REENTRANT_TYPED_TEXT,
                            COMPOSITION_INITIAL_TEXT.length
                        )
                        queuedAcceptedUpdate = editor.hasDeferredRustUpdateApplicationForTesting()
                    }
                }
                val revision = harness.adapter.baseDocumentRevision
                val input = requireNotNull(editor.onCreateInputConnection(EditorInfo()))
                try {
                    assertTrue(input.setSelection(0, COMPOSITION_INITIAL_TEXT.length))
                    commitMultilineComposition(input)
                    assertTrue(
                        "accepted update must actually enter deferred input",
                        queuedAcceptedUpdate
                    )
                    assertEquals(listOf(OPERATION_LIMIT_ERROR), errors.map { it.code })
                    assertEquals(REENTRANT_DOCUMENT_HTML, harness.adapter.documentHtml())
                    assertEquals(revision + 1uL, harness.adapter.baseDocumentRevision)
                    assertEquals("rebindOnUpdate=$rebindOnUpdate", 1, updates.size)
                    assertEquals(
                        (revision + 1uL).toString(),
                        JSONObject(updates.single()).getString("documentVersion")
                    )
                    val expectedText = if (rebindOnUpdate) {
                        CALLBACK_DOCUMENT_TEXT
                    } else {
                        COMPOSITION_INITIAL_TEXT + REENTRANT_TYPED_TEXT
                    }
                    assertEquals(expectedText, editor.text.toString())
                    if (rebindOnUpdate) {
                        assertEquals(CALLBACK_SELECTION_END, editor.selectionStart)
                        assertEquals(CALLBACK_SELECTION_START, editor.selectionEnd)
                        assertEquals(
                            renderCountAtListenerRebind,
                            harness.adapter.renderUpdateCallCountForTesting
                        )
                        assertTrue(input.commitText("retired", 1))
                        assertEquals(CALLBACK_DOCUMENT_HTML, replacement.adapter.documentHtml())
                    } else {
                        assertEquals(expectedText.length, editor.selectionStart)
                        assertEquals(editor.selectionStart, editor.selectionEnd)
                    }
                    assertEquals(-1, BaseInputConnection.getComposingSpanStart(editor.editableText))
                } finally {
                    input.closeConnection()
                }
            } finally {
                harness.adapter.destroy()
                replacement.adapter.destroy()
            }
        }
    }

    @Test
    fun `unavailable multiline recovery cannot restore over unadopted backend selection`() {
        assertUnadoptedMutationPreventsFallback(replaceDocument = false)
    }

    @Test
    fun `unavailable multiline recovery cannot restore over unadopted backend document`() {
        assertUnadoptedMutationPreventsFallback(replaceDocument = true)
    }

    @Test
    @Config(sdk = [24])
    fun `minimum API preserves real core composition and extracted text`() {
        val harness = realExternalCompositionHarness("a😀b")
        try {
            val input = requireNotNull(harness.editText.onCreateInputConnection(EditorInfo()))
            input.setSelection(1, 3)
            input.setComposingText("に", 1)
            assertEquals("<p>a😀b</p>", harness.adapter.documentHtml())
            input.commitText("日本", 1)
            assertEquals("<p>a日本b</p>", harness.adapter.documentHtml())
            assertEquals(
                "a日本b",
                requireNotNull(input.getExtractedText(ExtractedTextRequest(), 0)).text.toString()
            )
        } finally {
            harness.adapter.destroy()
        }
    }

    @Test
    fun `modern commit remains Rust authorized`() {
        val harness = realExternalCompositionHarness("a😀b")
        try {
            harness.editText.setSelection(1, 3)
            val input = requireNotNull(harness.editText.onCreateInputConnection(EditorInfo()))
            assertTrue(input.commitText("文", 1, null))
            assertEquals("<p>a文b</p>", harness.adapter.documentHtml())
            assertEquals("a文b", harness.editText.text.toString())
            assertEquals(2, harness.editText.selectionEnd)
        } finally {
            harness.adapter.destroy()
        }
    }

    @Test
    fun `modern composition preserves the original replacement until commit`() {
        val harness = realExternalCompositionHarness("before")
        try {
            harness.editText.setSelection(1, 4)
            val input = requireNotNull(harness.editText.onCreateInputConnection(EditorInfo()))
            assertTrue(input.setComposingText("に", 1, null))
            assertEquals("<p>before</p>", harness.adapter.documentHtml())
            assertTrue(input.setComposingText("日本", 1, null))
            assertTrue(input.commitText("日本", 1, null))
            assertEquals("<p>b日本re</p>", harness.adapter.documentHtml())
            assertEquals("b日本re", harness.editText.text.toString())
        } finally {
            harness.adapter.destroy()
        }
    }

    @Test
    fun `explicit modern replacement uses its range rather than selection`() {
        val harness = structuredDeleteHarness("<p>a😀b</p><p>tail</p>")
        try {
            harness.editText.setSelection(0)
            val input = requireNotNull(harness.editText.onCreateInputConnection(EditorInfo()))
            assertTrue(input.replaceText(1, 6, "文\n字", 1, null))
            assertEquals(
                harness.editText.imeTraceSnapshotForTesting().joinToString("\n"),
                "<p>a文</p><p>字ail</p>",
                harness.adapter.documentHtml()
            )
            assertEquals("a文\n字ail", harness.editText.text.toString())
            assertEquals(4, harness.editText.selectionEnd)
        } finally {
            harness.adapter.destroy()
        }
    }

    @Test
    fun `multiline replacement cursor uses normalized CRLF length`() {
        for (cursor in listOf(1, 2)) {
            val harness = realExternalCompositionHarness("abcdtail")
            try {
                val input = requireNotNull(harness.editText.onCreateInputConnection(EditorInfo()))
                input.replaceText(1, 3, "x\r\ny", cursor, null)
                assertEquals("<p>ax</p><p>ydtail</p>", harness.adapter.documentHtml())
                assertEquals(3 + cursor, harness.editText.selectionEnd)
            } finally {
                harness.adapter.destroy()
            }
        }
    }

    @Test
    fun `explicit replacement retains transient composing text outside its range`() {
        val harness = realExternalCompositionHarness("abc tail")
        try {
            val input = requireNotNull(harness.editText.onCreateInputConnection(EditorInfo()))
            input.setSelection(0, 3)
            input.setComposingText("日本", 1)
            assertTrue(input.replaceText(3, 7, "end", 1, null))
            assertEquals("<p>日本 end</p>", harness.adapter.documentHtml())
            assertEquals("日本 end", harness.editText.text.toString())
            assertEquals(
                -1,
                BaseInputConnection.getComposingSpanStart(harness.editText.editableText)
            )
        } finally {
            harness.adapter.destroy()
        }
    }

    @Test
    fun `generation retirement blocks context actions before base replacement`() {
        val harness = realExternalCompositionHarness("safe")
        try {
            val editor = harness.editText
            val input = requireNotNull(editor.onCreateInputConnection(EditorInfo()))
            val clipboard = editor.context.getSystemService(
                android.content.Context.CLIPBOARD_SERVICE
            ) as android.content.ClipboardManager
            clipboard.setPrimaryClip(android.content.ClipData.newPlainText("", "bad"))
            editor.invalidateInputConnectionsForEditor()
            input.performContextMenuAction(android.R.id.paste)
            input.performEditorAction(EditorInfo.IME_ACTION_UNSPECIFIED)
            assertEquals("<p>safe</p>", harness.adapter.documentHtml())
            assertEquals("safe", editor.text.toString())
        } finally {
            harness.adapter.destroy()
        }
    }

    @Test
    fun `modern mutation entrypoints reject retired sessions`() {
        val harness = realExternalCompositionHarness("safe")
        try {
            val old = requireNotNull(harness.editText.onCreateInputConnection(EditorInfo()))
            harness.editText.onCreateInputConnection(EditorInfo())
            old.replaceText(0, 4, "bad", 1, null)
            old.commitText("bad", 1, null)
            old.setComposingText("bad", 1, null)
            assertEquals("<p>safe</p>", harness.adapter.documentHtml())
            assertEquals("safe", harness.editText.text.toString())
        } finally {
            harness.adapter.destroy()
        }
    }

    @Test
    fun `closed connection cannot authorize later mutations`() {
        val harness = realExternalCompositionHarness("safe")
        try {
            val input = requireNotNull(harness.editText.onCreateInputConnection(EditorInfo()))
            input.closeConnection()
            input.commitText("bad", 1)
            input.replaceText(0, 4, "bad", 1, null)
            assertEquals("<p>safe</p>", harness.adapter.documentHtml())
            assertEquals("safe", harness.editText.text.toString())
        } finally {
            harness.adapter.destroy()
        }
    }

    @Test
    fun `snapshot maps selection and composition through generated list placeholders`() {
        val harness = structuredDeleteHarness("<ul><li><p></p></li><li><p>Alpha Beta</p></li></ul>")
        try {
            val input = requireNotNull(harness.editText.onCreateInputConnection(EditorInfo()))
            val mapper = requireNotNull(harness.editText.imeTextCoordinateMapperForEditor())
            val wordStart = mapper.visibleText.toString().indexOf("Alpha")
            input.setComposingRegion(wordStart + 1, wordStart + 4, null)
            input.setSelection(wordStart + 2, wordStart + 2)
            assertTrue(
                harness.editText.text.toString().contains(
                    LayoutConstants.SYNTHETIC_PLACEHOLDER_CHARACTER
                )
            )
            val snapshot = requireNotNull(input.takeSnapshot())
            assertEquals(mapper.visibleText.toString(), snapshot.surroundingText.text.toString())
            assertEquals(wordStart + 2, snapshot.selectionStart)
            assertEquals(wordStart + 2, snapshot.selectionEnd)
            assertEquals(wordStart + 1, snapshot.compositionStart)
            assertEquals(wordStart + 4, snapshot.compositionEnd)
        } finally {
            harness.adapter.destroy()
        }
    }

    @Test
    fun `extracted and surrounding text use visible IME coordinates`() {
        val harness = structuredDeleteHarness("<ul><li><p></p></li><li><p>Alpha Beta</p></li></ul>")
        try {
            val input = requireNotNull(harness.editText.onCreateInputConnection(EditorInfo()))
            val mapper = requireNotNull(harness.editText.imeTextCoordinateMapperForEditor())
            assertTrue(
                harness.editText.text.toString().contains(
                    LayoutConstants.SYNTHETIC_PLACEHOLDER_CHARACTER
                )
            )
            input.setSelection(1, 4)
            val extracted = requireNotNull(input.getExtractedText(ExtractedTextRequest(), 0))
            assertEquals(mapper.visibleText.toString(), extracted.text.toString())
            assertEquals(1, extracted.selectionStart)
            assertEquals(4, extracted.selectionEnd)
            val surrounding = requireNotNull(input.getSurroundingText(100, 100, 0))
            assertEquals(mapper.visibleText.toString(), surrounding.text.toString())
            assertEquals(1, surrounding.selectionStart)
            assertEquals(4, surrounding.selectionEnd)
        } finally {
            harness.adapter.destroy()
        }
    }

    private fun compositionOperationLimitConfig(limit: Int): String = JSONObject()
        .put("initialization", JSONObject().put("type", "localEmpty"))
        .put(
            "limits",
            JSONObject().put("editing", JSONObject().put("maxOperationsPerTransaction", limit))
        )
        .toString()

    private fun commitMultilineComposition(input: InputConnection) {
        assertTrue(input.setComposingText(MULTILINE_COMPOSITION_TEXT, 1))
        assertTrue(input.commitText(MULTILINE_COMPOSITION_TEXT, 1))
        org.robolectric.Shadows.shadowOf(android.os.Looper.getMainLooper()).idle()
    }

    private fun assertUnadoptedMutationPreventsFallback(replaceDocument: Boolean) {
        val backend = UnavailableRecoveryBackend()
        val harness = realExternalCompositionHarness(
            COMPOSITION_INITIAL_TEXT,
            compositionOperationLimitConfig(REJECTED_COMPACT_OPERATION_LIMIT),
            backend = backend
        )
        try {
            val editor = harness.editText
            val errors = mutableListOf<EditorV2Error>()
            val input = requireNotNull(editor.onCreateInputConnection(EditorInfo()))
            try {
                assertTrue(input.setSelection(0, COMPOSITION_INITIAL_TEXT.length))
                val documentRevision = harness.adapter.baseDocumentRevision
                val stateRevision = harness.adapter.stateRevision
                val selection = requireNotNull(harness.adapter.cachedAuthoritativeScalarSelection)
                    .copyOf()
                val stateReads = backend.stateReadCount
                harness.adapter.onAutonomousError = {
                    errors.add(it)
                    if (it.code == OPERATION_LIMIT_ERROR) {
                        val result = if (replaceDocument) {
                            harness.adapter.callWithEnvelope(
                                JSONObject().put("setHtml", CALLBACK_DOCUMENT_HTML)
                                    .put("history", UNDOABLE_BOUNDARY_HISTORY),
                                includeBaseRevision = false
                            ) { request -> backend.replaceDocument(harness.editorId, request) }
                        } else {
                            harness.adapter.callWithEnvelope(
                                harness.adapter.selectionEnvelope(
                                    CALLBACK_SELECTION_END,
                                    CALLBACK_SELECTION_START,
                                    RANGE_SELECTION_AFFINITY
                                )
                            ) { request -> backend.setSelection(harness.editorId, request) }
                        }
                        assertTrue(
                            "real direct backend mutation must succeed",
                            result is EditorV2CallResult.Ok
                        )
                        backend.recoveryUnavailable = true
                    }
                }
                commitMultilineComposition(input)
                assertEquals(MULTILINE_COMPOSITION_TEXT, editor.text.toString())
                assertEquals(MULTILINE_COMPOSITION_TEXT.length, editor.selectionStart)
                assertEquals(editor.selectionStart, editor.selectionEnd)
                assertEquals(-1, BaseInputConnection.getComposingSpanStart(editor.editableText))
                assertEquals(documentRevision, harness.adapter.baseDocumentRevision)
                assertEquals(stateRevision, harness.adapter.stateRevision)
                assertTrue(
                    selection.contentEquals(harness.adapter.cachedAuthoritativeScalarSelection)
                )
                assertEquals(stateReads + 1, backend.stateReadCount)
                assertEquals(
                    listOf(OPERATION_LIMIT_ERROR, RECOVERY_ERROR.code),
                    errors.map {
                        it.code
                    }
                )
                val engineState = UniffiEditorV2Backend.getState(harness.editorId)
                    as EditorV2CallResult.Ok
                val freshState = JSONObject(engineState.value)
                assertTrue(requireNotNull(ulongField(freshState, "stateRevision")) > stateRevision)
                val expectedRevision = if (replaceDocument) {
                    documentRevision + 1uL
                } else {
                    documentRevision
                }
                assertEquals(expectedRevision, ulongField(freshState, "documentRevision"))
                val expectedHtml = if (replaceDocument) {
                    CALLBACK_DOCUMENT_HTML
                } else {
                    COMPOSITION_INITIAL_HTML
                }
                assertEquals(expectedHtml, harness.adapter.documentHtml())
            } finally {
                input.closeConnection()
            }
        } finally {
            harness.adapter.destroy()
        }
    }

    private class UnavailableRecoveryBackend : EditorV2Backend by UniffiEditorV2Backend {
        var recoveryUnavailable = false
        var unavailableRenderCount = 0
        var stateReadCount = 0

        override fun getState(editorId: String): EditorV2CallResult<String> {
            stateReadCount += 1
            return UniffiEditorV2Backend.getState(editorId)
        }

        override fun renderNativeFrame(
            editorId: String,
            ownerId: String?,
            mirrorAnchor: Int?,
            mirrorHead: Int?
        ): EditorV2CallResult<uniffi.editor_core.FfiNativeRenderFrame> {
            if (recoveryUnavailable) {
                unavailableRenderCount += 1
                return EditorV2CallResult.Err(RECOVERY_ERROR)
            }
            return UniffiEditorV2Backend.renderNativeFrame(
                editorId,
                ownerId,
                mirrorAnchor,
                mirrorHead
            )
        }
    }

    private companion object {
        const val COMPOSITION_INITIAL_TEXT = "safe"
        const val COMPOSITION_INITIAL_HTML = "<p>safe</p>"
        const val MULTILINE_COMPOSITION_TEXT = "x\ny\nz"
        const val MULTILINE_COMPOSITION_HTML = "<p>x</p><p>y</p><p>z</p>"
        const val OPERATION_LIMIT_ERROR = "OPERATION_LIMIT_EXCEEDED"
        const val REJECTED_COMPACT_OPERATION_LIMIT = 1
        const val ACCEPTED_COMPACT_OPERATION_LIMIT = 2
        const val CALLBACK_SELECTION_START = 1
        const val CALLBACK_SELECTION_END = 3
        const val CALLBACK_DOCUMENT_TEXT = "new authorized text"
        const val CALLBACK_DOCUMENT_HTML = "<p>new authorized text</p>"
        const val REBOUND_VIEW_TOKEN = 2L
        const val REENTRANT_TYPED_TEXT = "!"
        const val REENTRANT_DOCUMENT_HTML = "<p>safe!</p>"
        const val UNDOABLE_BOUNDARY_HISTORY = "undoableBoundary"
        const val RANGE_SELECTION_AFFINITY = "before"
        val RECOVERY_ERROR = EditorV2Error("render", "RENDER_FAILED", "unavailable recovery")
    }
}
