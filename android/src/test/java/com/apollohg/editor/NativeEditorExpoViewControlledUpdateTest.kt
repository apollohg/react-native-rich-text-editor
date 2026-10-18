package com.apollohg.editor
import android.graphics.Typeface
import android.os.Looper
import android.text.TextPaint
import android.text.style.CharacterStyle
import android.view.ViewGroup
import android.view.inputmethod.EditorInfo
import android.widget.Button
import java.time.Duration
import java.util.concurrent.CountDownLatch
import org.json.JSONObject
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNull
import org.junit.Assert.assertSame
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.RuntimeEnvironment
import org.robolectric.Shadows.shadowOf
import org.robolectric.annotation.Config

@RunWith(RobolectricTestRunner::class)
@Config(sdk = [34])
internal class NativeEditorExpoViewControlledUpdateTest :
    NativeEditorExpoViewControlledUpdateTestFixture() {
    @Test
    fun `native typing burst delivers every captured revision on the next main loop turn`() {
        assertNativeTypingBurstDispatch(blurBeforeDispatch = false)
    }

    @Test
    @Config(sdk = [24, 34])
    fun `native typing burst retains every captured revision after blur before dispatch`() {
        assertNativeTypingBurstDispatch(blurBeforeDispatch = true)
    }

    @Test
    fun `native surrounding delete burst captures every accepted marked Unicode revision`() {
        withFocusedViewForNativeInput("<p><strong>Laβ😀R</strong></p>") {
                _,
                view,
                adapter,
                payloads
            ->
            val editText = view.richTextView.editorEditText
            editText.setSelection(DELETE_INITIAL_CARET)
            val input = requireNotNull(editText.onCreateInputConnection(EditorInfo()))
            shadowOf(Looper.getMainLooper()).idleFor(Duration.ofMillis(CALLBACK_SETUP_IDLE_MS))
            payloads.clear()
            assertEquals(0, view.pendingEditorUpdateEventCountForTesting())
            val expected = mutableListOf<String>()
            val initialRevision = JSONObject(requireNotNull(adapter.cachedAtomicRenderJson))
                .getString("documentVersion").toULong()
            val deleteSteps = listOf(
                Triple(2, "LaβR", 3),
                Triple(1, "LaR", 2),
                Triple(1, "LR", 1)
            )
            for ((index, step) in deleteSteps.withIndex()) {
                val (beforeLength, expectedText, expectedCaret) = step
                assertTrue(
                    "the original InputConnection must accept delete $index",
                    input.deleteSurroundingText(beforeLength, 0)
                )
                assertSame(
                    "delete $index must preserve the input session",
                    input,
                    editText.activeInputConnection
                )
                assertEquals(
                    "accepted delete $index must change visible text",
                    expectedText,
                    editText.text.toString()
                )
                assertEquals(expectedCaret, editText.selectionStart)
                assertEquals(expectedCaret, editText.selectionEnd)
                val html = "<p><strong>$expectedText</strong></p>"
                assertEquals(
                    "accepted delete $index must change authoritative HTML",
                    html,
                    adapter.documentHtml()
                )
                assertBoldPrefix(editText, expectedText.length)
                assertNotNull(adapter.positionEpoch)
                val snapshot = requireNotNull(adapter.cachedAtomicRenderJson)
                val parsed = JSONObject(snapshot)
                assertEquals(ATOMIC_RENDER_SNAPSHOT_KEYS, parsed.keys().asSequence().toSet())
                assertEquals(
                    (initialRevision + (index + 1).toULong()).toString(),
                    parsed.getString("documentVersion")
                )
                val selection = parsed.getJSONObject("selection")
                assertEquals(expectedCaret, selection.getInt("anchorScalar"))
                assertEquals(expectedCaret, selection.getInt("headScalar"))
                expected.add(snapshot)
            }
            assertTrue(
                "accepted deletes must not dispatch public callbacks inline",
                payloads.isEmpty()
            )
            shadowOf(Looper.getMainLooper()).idle()
            assertEquals(
                "each accepted delete must retain its own exact captured public snapshot",
                expected,
                payloads.map { it["updateJson"] as String }
            )
            assertEquals(
                expected.map { JSONObject(it).getString("documentVersion") },
                payloads.map { it["documentRevision"] }
            )
            assertTrue(payloads.all { it["editorId"] == adapter.editorId })
            assertEquals(0, view.pendingEditorUpdateEventCountForTesting())
            assertEquals("<p><strong>LR</strong></p>", adapter.documentHtml())
            assertEquals("LR", editText.text.toString())
            assertEquals(1, editText.selectionStart)
            assertEquals(1, editText.selectionEnd)
        }
    }

    @Test
    fun `native bulk delete preserves input for an empty paragraph autocorrect commit`() {
        val initialText = "teh"
        val correctedText = "the"
        withFocusedViewForNativeInput("<p>$initialText</p>") { _, view, adapter, payloads ->
            val editText = view.richTextView.editorEditText
            editText.setSelection(initialText.length)
            val input = requireNotNull(editText.onCreateInputConnection(EditorInfo()))
            shadowOf(Looper.getMainLooper()).idleFor(Duration.ofMillis(CALLBACK_SETUP_IDLE_MS))
            payloads.clear()
            assertEquals(0, view.pendingEditorUpdateEventCountForTesting())

            assertTrue(input.deleteSurroundingText(initialText.length, 0))

            assertEquals("<p></p>", adapter.documentHtml())
            assertEquals("", input.getTextBeforeCursor(initialText.length, 0).toString())
            assertEquals("", input.getTextAfterCursor(initialText.length, 0).toString())
            assertSame(input, editText.activeInputConnection)
            val deleted = requireNotNull(adapter.cachedAtomicRenderJson)
            assertTrue(JSONObject(deleted).getBoolean("documentIsEmpty"))
            val emptyNativeCaret = LayoutConstants.SYNTHETIC_PLACEHOLDER_CHARACTER.length
            assertEquals(emptyNativeCaret, editText.selectionStart)
            assertEquals(emptyNativeCaret, editText.selectionEnd)
            assertEquals(
                "native empty paragraph coordinates include its generated placeholder",
                emptyNativeCaret,
                JSONObject(deleted).getJSONObject("selection").getInt("anchorScalar")
            )

            assertTrue(input.commitText(correctedText, 1))

            assertEquals("<p>$correctedText</p>", adapter.documentHtml())
            assertEquals(correctedText, editText.text.toString())
            assertEquals(correctedText.length, editText.selectionStart)
            assertEquals(correctedText.length, editText.selectionEnd)
            assertSame(input, editText.activeInputConnection)
            val inserted = requireNotNull(adapter.cachedAtomicRenderJson)
            val expected = listOf(deleted, inserted)
            assertTrue(payloads.isEmpty())
            shadowOf(Looper.getMainLooper()).idle()
            assertEquals(expected, payloads.map { it["updateJson"] as String })
            assertEquals(
                expected.map { JSONObject(it).getString("documentVersion") },
                payloads.map { it["documentRevision"] }
            )
            assertTrue(
                expected.all {
                    JSONObject(it).keys().asSequence().toSet() == ATOMIC_RENDER_SNAPSHOT_KEYS
                }
            )
            assertEquals("<p>$correctedText</p>", adapter.documentHtml())
        }
    }

    @Test
    fun `queued commit preserves newer native selection and toolbar without losing its snapshot`() {
        val markedText = TOOLBAR_MARKED_TEXT
        val unmarkedText = TOOLBAR_UNMARKED_TEXT
        val insertedText = "X"
        withFocusedViewForNativeInput(TOOLBAR_INITIAL_HTML) {
                _,
                view,
                adapter,
                payloads
            ->
            val editText = view.richTextView.editorEditText
            val initialCaret = markedText.length / 2
            editText.setSelection(initialCaret)
            val input = requireNotNull(editText.onCreateInputConnection(EditorInfo()))
            shadowOf(Looper.getMainLooper()).idleFor(Duration.ofMillis(CALLBACK_SETUP_IDLE_MS))
            payloads.clear()
            assertEquals(0, view.pendingEditorUpdateEventCountForTesting())
            val selectionPayloads = mutableListOf<Map<String, Any>>()
            view.onSelectionChangeForTesting = { selectionPayloads += it }

            assertTrue(input.commitText(insertedText, 1))

            val committedMarkedText = markedText.take(initialCaret) + insertedText +
                markedText.drop(initialCaret)
            val expectedHtml = "<p><strong>$committedMarkedText</strong>$unmarkedText</p>"
            val captured = requireNotNull(adapter.cachedAtomicRenderJson)
            val capturedJson = JSONObject(captured)
            assertEquals(ATOMIC_RENDER_SNAPSHOT_KEYS, capturedJson.keys().asSequence().toSet())
            assertNotNull(adapter.positionEpoch)
            assertTrue(
                capturedJson.getJSONObject("activeState").getJSONObject("marks")
                    .getBoolean(TOOLBAR_BOLD_MARK)
            )
            assertEquals(
                initialCaret + insertedText.length,
                capturedJson.getJSONObject("selection").getInt("anchorScalar")
            )
            assertEquals(expectedHtml, adapter.documentHtml())
            assertTrue(payloads.isEmpty())
            assertEquals(1, view.pendingEditorUpdateEventCountForTesting())
            val commitStateRevision = adapter.stateRevision
            val commitDocumentRevision = adapter.baseDocumentRevision
            val latestCaret = committedMarkedText.length + unmarkedText.length - 1

            assertTrue(input.setSelection(latestCaret, latestCaret))

            assertSame(input, editText.activeInputConnection)
            assertEquals(latestCaret, editText.selectionStart)
            assertEquals(latestCaret, editText.selectionEnd)
            assertEquals(commitDocumentRevision, adapter.baseDocumentRevision)
            assertTrue(
                "selection must advance the same document's native state",
                adapter.stateRevision > commitStateRevision
            )
            val currentSelectionJson = requireNotNull(adapter.currentSelectionStateJson())
            val currentSelection = JSONObject(currentSelectionJson)
            assertEquals(
                latestCaret,
                currentSelection.getJSONObject("selection").getInt("anchorScalar")
            )
            assertFalse(
                currentSelection.getJSONObject("activeState").getJSONObject("marks")
                    .getBoolean(TOOLBAR_BOLD_MARK)
            )
            assertFalse(
                "the public selection path must first clear bold",
                view.toolbarState.marks[TOOLBAR_BOLD_MARK] == true
            )
            assertTrue(payloads.isEmpty())
            println(
                "$QUEUED_SELECTION_PROOF_PREFIX ${JSONObject()
                    .put("historicalPublicSnapshot", captured)
                    .put("newerSelectionStateJson", currentSelectionJson)
                    .put("newerSelectionPayload", JSONObject(selectionPayloads.last()))
                    .put("commitStateRevision", commitStateRevision.toString())
                    .put("selectionStateRevision", adapter.stateRevision.toString())}"
            )

            shadowOf(Looper.getMainLooper()).idle()

            assertEquals(
                "the original commit must dispatch exactly once",
                listOf(captured),
                payloads.map { it["updateJson"] as String }
            )
            assertEquals(
                listOf(capturedJson.getString("documentVersion")),
                payloads.map { it["documentRevision"] }
            )
            assertEquals(0, view.pendingEditorUpdateEventCountForTesting())
            assertEquals(expectedHtml, adapter.documentHtml())
            assertEquals(committedMarkedText + unmarkedText, editText.text.toString())
            assertBoldPrefix(editText, committedMarkedText.length)
            assertSame(input, editText.activeInputConnection)
            assertEquals(latestCaret, editText.selectionStart)
            assertEquals(latestCaret, editText.selectionEnd)
            assertFalse(
                "a historical commit must not restore bold after a newer selection",
                view.toolbarState.marks[TOOLBAR_BOLD_MARK] == true
            )
        }
    }

    @Test
    fun `reentrant selection keeps later historical commits and current toolbar`() {
        withFocusedViewForNativeInput(TOOLBAR_INITIAL_HTML) { _, view, adapter, payloads ->
            val editText = view.richTextView.editorEditText
            val initialCaret = TOOLBAR_MARKED_TEXT.length / 2
            editText.setSelection(initialCaret)
            val input = requireNotNull(editText.onCreateInputConnection(EditorInfo()))
            shadowOf(Looper.getMainLooper()).idleFor(Duration.ofMillis(CALLBACK_SETUP_IDLE_MS))
            payloads.clear()
            assertEquals(0, view.pendingEditorUpdateEventCountForTesting())
            val captured = mutableListOf<String>()
            var markedText = TOOLBAR_MARKED_TEXT
            var caret = initialCaret
            for (text in listOf("X", "Y")) {
                assertTrue(input.commitText(text, 1))
                markedText = markedText.take(caret) + text + markedText.drop(caret)
                caret += text.length
                captured += requireNotNull(adapter.cachedAtomicRenderJson)
                assertEquals(
                    TOOLBAR_INITIAL_HTML.replace(TOOLBAR_MARKED_TEXT, markedText),
                    adapter.documentHtml()
                )
                assertSame(input, editText.activeInputConnection)
            }
            assertTrue(payloads.isEmpty())
            assertEquals(captured.size, view.pendingEditorUpdateEventCountForTesting())
            val latestDocumentRevision = adapter.baseDocumentRevision
            val latestCaret = markedText.length + TOOLBAR_UNMARKED_TEXT.length - 1
            var movedSelectionFromCallback = false
            view.onEditorUpdateForTesting = { payload ->
                payloads += payload
                if (payloads.size == 1) {
                    assertEquals(captured.first(), payload["updateJson"])
                    assertEquals(latestDocumentRevision, adapter.baseDocumentRevision)
                    val beforeSelectionRevision = adapter.stateRevision
                    assertTrue(input.setSelection(latestCaret, latestCaret))
                    assertTrue(adapter.stateRevision > beforeSelectionRevision)
                    assertFalse(view.toolbarState.marks[TOOLBAR_BOLD_MARK] == true)
                    movedSelectionFromCallback = true
                }
            }

            shadowOf(Looper.getMainLooper()).idle()

            assertTrue(
                "the earlier document commit must reach its actual callback",
                movedSelectionFromCallback
            )
            assertEquals(
                "reentrant selection must retain both exact FIFO commits",
                captured,
                payloads.map {
                    it["updateJson"] as String
                }
            )
            assertEquals(
                captured.map {
                    JSONObject(it).getString("documentVersion")
                },
                payloads.map { it["documentRevision"] }
            )
            assertTrue(
                captured.all {
                    JSONObject(it).keys().asSequence().toSet() ==
                        ATOMIC_RENDER_SNAPSHOT_KEYS
                }
            )
            assertEquals(0, view.pendingEditorUpdateEventCountForTesting())
            assertEquals(latestDocumentRevision, adapter.baseDocumentRevision)
            assertEquals(
                TOOLBAR_INITIAL_HTML.replace(TOOLBAR_MARKED_TEXT, markedText),
                adapter.documentHtml()
            )
            assertEquals(markedText + TOOLBAR_UNMARKED_TEXT, editText.text.toString())
            assertBoldPrefix(editText, markedText.length)
            assertSame(input, editText.activeInputConnection)
            assertEquals(latestCaret, editText.selectionStart)
            assertEquals(latestCaret, editText.selectionEnd)
            assertFalse(
                "later captured view state must not undo a selection made during dispatch",
                view.toolbarState.marks[TOOLBAR_BOLD_MARK] == true
            )
        }
    }

    @Test
    fun `native selection state projects its adopted revision without private epoch`() {
        withFocusedViewForNativeInput(TOOLBAR_INITIAL_HTML) { _, view, adapter, _ ->
            val editText = view.richTextView.editorEditText
            editText.setSelection(TOOLBAR_MARKED_TEXT.length / 2)
            val input = requireNotNull(editText.onCreateInputConnection(EditorInfo()))
            shadowOf(Looper.getMainLooper()).idleFor(Duration.ofMillis(CALLBACK_SETUP_IDLE_MS))
            val documentRevision = adapter.baseDocumentRevision
            val previousStateRevision = adapter.stateRevision
            val selectionPayloads = mutableListOf<Map<String, Any>>()
            view.onSelectionChangeForTesting = { selectionPayloads += it }
            val caret = TOOLBAR_MARKED_TEXT.length + TOOLBAR_UNMARKED_TEXT.length - 1

            assertTrue(input.setSelection(caret, caret))

            assertSame(input, editText.activeInputConnection)
            assertEquals(documentRevision, adapter.baseDocumentRevision)
            assertTrue(adapter.stateRevision > previousStateRevision)
            assertEquals(TOOLBAR_INITIAL_HTML, adapter.documentHtml())
            assertEquals(TOOLBAR_MARKED_TEXT + TOOLBAR_UNMARKED_TEXT, editText.text.toString())
            assertBoldPrefix(editText, TOOLBAR_MARKED_TEXT.length)
            assertEquals(caret, editText.selectionStart)
            assertEquals(caret, editText.selectionEnd)
            val snapshot = JSONObject(requireNotNull(adapter.cachedAtomicRenderJson))
            assertEquals(ATOMIC_RENDER_SNAPSHOT_KEYS, snapshot.keys().asSequence().toSet())
            assertNotNull(adapter.positionEpoch)
            assertEquals(
                adapter.stateRevision.toString(),
                snapshot.getString("stateRevision")
            )
            val selectionJson = requireNotNull(adapter.currentSelectionStateJson())
            val selection = JSONObject(selectionJson)
            assertEquals(documentRevision.toString(), selection.getString("documentVersion"))
            assertEquals(caret, selection.getJSONObject("selection").getInt("anchorScalar"))
            assertEquals(caret, selection.getJSONObject("selection").getInt("headScalar"))
            assertFalse(
                selection.getJSONObject(
                    "activeState"
                ).getJSONObject("marks").getBoolean(TOOLBAR_BOLD_MARK)
            )
            assertFalse(selection.has("positionEpoch"))
            assertEquals(selectionJson, selectionPayloads.last()["stateJson"])
            println(
                "$CURRENT_SELECTION_PROOF_PREFIX ${JSONObject()
                    .put("cachedPublicSnapshot", adapter.cachedAtomicRenderJson)
                    .put("selectionStateJson", selectionJson)
                    .put("selectionPayload", JSONObject(selectionPayloads.last()))
                    .put("adapterStateRevision", adapter.stateRevision.toString())}"
            )
            assertTrue(
                "native selection projection must include its adopted state revision",
                selection.has("stateRevision")
            )
            assertEquals(adapter.stateRevision.toString(), selection.getString("stateRevision"))
        }
    }

    private fun assertBoldPrefix(editText: EditorEditText, boldEnd: Int) {
        val content = requireNotNull(editText.text)
        for (offset in content.indices) {
            val paint = TextPaint(editText.paint)
            content.getSpans(offset, offset + 1, CharacterStyle::class.java)
                .forEach { it.updateDrawState(paint) }
            assertEquals(
                "visible native bold state at UTF16 offset $offset",
                offset < boldEnd,
                (paint.typeface?.style ?: Typeface.NORMAL) and Typeface.BOLD != 0
            )
        }
    }

    private fun assertNativeTypingBurstDispatch(blurBeforeDispatch: Boolean) {
        withFocusedViewForNativeInput { activity, view, adapter, payloads ->
            val blurTarget = if (blurBeforeDispatch) {
                Button(activity).apply {
                    text = BLUR_TARGET_LABEL
                    isFocusableInTouchMode = true
                    activity.findViewById<ViewGroup>(android.R.id.content).addView(
                        this,
                        ViewGroup.LayoutParams(
                            ViewGroup.LayoutParams.WRAP_CONTENT,
                            ViewGroup.LayoutParams.WRAP_CONTENT
                        )
                    )
                }
            } else {
                null
            }
            val input =
                requireNotNull(
                    view.richTextView.editorEditText.onCreateInputConnection(EditorInfo())
                )
            shadowOf(Looper.getMainLooper()).idleFor(Duration.ofMillis(CALLBACK_SETUP_IDLE_MS))
            payloads.clear()
            assertEquals(
                "focus and input-session setup must finish before the burst",
                0,
                view.pendingEditorUpdateEventCountForTesting()
            )
            val expected = mutableListOf<String>()
            val expectedText = StringBuilder()
            for (text in listOf("a", "😀", "b")) {
                assertTrue("real input must commit $text", input.commitText(text, 1))
                expectedText.append(text)
                val editText = view.richTextView.editorEditText
                assertEquals(
                    "each commit must retain its UTF16 caret",
                    expectedText.length,
                    editText.selectionStart
                )
                assertEquals(
                    "each commit must collapse its UTF16 caret",
                    expectedText.length,
                    editText.selectionEnd
                )
                assertEquals(
                    "each commit must update authoritative HTML",
                    "<p>$expectedText</p>",
                    adapter.documentHtml()
                )
                assertNotNull("native frame must retain its binding epoch", adapter.positionEpoch)
                val commit = requireNotNull(adapter.cachedAtomicRenderJson)
                val selection = JSONObject(commit).getJSONObject("selection")
                val scalarCaret = expectedText.codePointCount(0, expectedText.length)
                assertEquals(
                    "public snapshot must retain its scalar anchor",
                    scalarCaret,
                    selection.getInt("anchorScalar")
                )
                assertEquals(
                    "public snapshot must retain its scalar head",
                    scalarCaret,
                    selection.getInt("headScalar")
                )
                assertEquals(
                    "each real Unicode commit must use the exact public snapshot contract",
                    ATOMIC_RENDER_SNAPSHOT_KEYS,
                    JSONObject(commit).keys().asSequence().toSet()
                )
                expected.add(commit)
            }
            assertEquals(
                "typing must preserve the complete Unicode text",
                "a😀b",
                view.richTextView.editorEditText.text.toString()
            )
            assertTrue("native callbacks must defer until input returns", payloads.isEmpty())
            assertEquals(expected.size, view.pendingEditorUpdateEventCountForTesting())

            if (blurBeforeDispatch) {
                assertTrue(
                    "a real sibling control must receive focus before callbacks dispatch",
                    requireNotNull(blurTarget).requestFocus()
                )
                assertTrue(
                    "blur must finish before callbacks dispatch",
                    !view.richTextView.editorEditText.hasFocus()
                )
                assertTrue("blur must not dispatch pending commits inline", payloads.isEmpty())
            }

            shadowOf(Looper.getMainLooper()).idle()

            assertEquals(
                "every revision must retain its captured atomic payload",
                expected,
                payloads.map { it["updateJson"] as String }
            )
            assertEquals(
                "no accepted commit may remain pending",
                0,
                view.pendingEditorUpdateEventCountForTesting()
            )
            assertEquals(
                expected.map { JSONObject(it).getString("documentVersion") },
                payloads.map { it["documentRevision"] }
            )
            assertTrue(payloads.all { it["editorId"] == adapter.editorId })
            assertEquals("<p>$expectedText</p>", adapter.documentHtml())
            assertEquals(expectedText.length, view.richTextView.editorEditText.selectionStart)
            assertEquals(expectedText.length, view.richTextView.editorEditText.selectionEnd)
            if (!blurBeforeDispatch) {
                assertEquals(
                    "the same-state latest ordinary commit must still update native toolbar state",
                    NativeToolbarState.fromUpdateJson(expected.last()),
                    view.toolbarState
                )
            }
        }
    }

    private companion object {
        const val BLUR_TARGET_LABEL = "Next control"
        const val CALLBACK_SETUP_IDLE_MS = 100L
        const val DELETE_INITIAL_CARET = 5
        const val TOOLBAR_BOLD_MARK = "bold"
        const val QUEUED_SELECTION_PROOF_PREFIX = "NATIVE_EDITOR_QUEUED_SELECTION_PROOF"
        const val CURRENT_SELECTION_PROOF_PREFIX = "NATIVE_EDITOR_CURRENT_SELECTION_PROOF"
        const val TOOLBAR_MARKED_TEXT = "Bold"
        const val TOOLBAR_UNMARKED_TEXT = " plain"
        const val TOOLBAR_INITIAL_HTML =
            "<p><strong>$TOOLBAR_MARKED_TEXT</strong>$TOOLBAR_UNMARKED_TEXT</p>"
    }

    @Test
    fun `delayed editor updates drain with captured identity across rebind`() {
        val expoContext = testExpoContext(RuntimeEnvironment.getApplication())
        val view = NativeEditorExpoView(expoContext.context, expoContext.appContext)
        val backend = FakeEditorV2Backend()
        fun registerAdapter(): Pair<EditorV2Adapter, Long> {
            val editorId = (
                backend.create(
                    "{\"initialization\":{\"type\":\"localEmpty\"}}",
                    null
                ) as EditorV2CallResult.Ok
                ).value
            val adapter = EditorV2Adapter.attach(
                backend,
                JSONObject(editorId).getString("editorId"),
                roomBound = false
            )!!
            return adapter to EditorV2Registry.register(adapter)
        }
        val (adapterA, tokenA) = registerAdapter()
        val (adapterB, tokenB) = registerAdapter()
        val payloads = mutableListOf<Map<String, Any>>()
        try {
            backend.sessions.getValue(adapterA.editorId).apply {
                text = StringBuilder("A")
                revision =
                    7uL
            }
            assertNotNull(adapterA.refreshFromRustState(null))
            backend.sessions.getValue(adapterB.editorId).apply {
                text = StringBuilder("B")
                revision =
                    8uL
            }
            assertNotNull(adapterB.refreshFromRustState(null))
            view.onEditorUpdateForTesting = { payloads += it }
            view.onAddonEventForTesting = {}

            view.setEditorId(tokenA)
            view.onEditorUpdate(
                JSONObject(renderUpdateJson("A")).put("documentVersion", "7").toString()
            )
            view.setEditorId(tokenB)
            view.onEditorUpdate(
                JSONObject(renderUpdateJson("B")).put("documentVersion", "8").toString()
            )
            shadowOf(Looper.getMainLooper()).idleFor(Duration.ofMillis(100))

            assertEquals(2, payloads.size)
            assertEquals(adapterA.editorId, payloads[0]["editorId"])
            assertEquals("7", payloads[0]["documentRevision"])
            assertEquals(adapterB.editorId, payloads[1]["editorId"])
            assertEquals("8", payloads[1]["documentRevision"])
            assertEquals(
                "B",
                JSONObject(payloads[1]["updateJson"] as String)
                    .getJSONArray("renderBlocks").getJSONArray(0).getJSONObject(1).getString("text")
            )
        } finally {
            EditorV2Registry.remove(adapterA.editorId)
            EditorV2Registry.remove(adapterB.editorId)
            NativeEditorViewRegistry.unregister(tokenA, view)
            NativeEditorViewRegistry.unregister(tokenB, view)
        }
    }

    @Test
    fun `queued native update from prior A binding drains before A to B to A rebind`() {
        val expoContext = testExpoContext(RuntimeEnvironment.getApplication())
        val view = NativeEditorExpoView(expoContext.context, expoContext.appContext)
        val backend = FakeEditorV2Backend()
        fun registerAdapter(): Pair<EditorV2Adapter, Long> {
            val editorId = (
                backend.create(
                    "{\"initialization\":{\"type\":\"localEmpty\"}}",
                    null
                ) as EditorV2CallResult.Ok
                ).value
            val adapter = EditorV2Adapter.attach(
                backend,
                JSONObject(editorId).getString("editorId"),
                roomBound = false
            )!!
            return adapter to EditorV2Registry.register(adapter)
        }
        val (adapterA, tokenA) = registerAdapter()
        val (adapterB, tokenB) = registerAdapter()
        val payloads = mutableListOf<Map<String, Any>>()
        try {
            backend.sessions.getValue(adapterA.editorId).apply {
                text = StringBuilder("stale A")
                revision =
                    7uL
            }
            assertNotNull(adapterA.refreshFromRustState(null))
            view.onEditorUpdateForTesting = { payloads += it }
            view.onAddonEventForTesting = {}

            view.setEditorId(tokenA)
            view.onEditorUpdate(
                JSONObject(renderUpdateJson("stale A")).put("documentVersion", "7").toString()
            )
            assertEquals(1, view.pendingEditorUpdateEventCountForTesting())

            view.setEditorId(tokenB)
            view.setEditorId(tokenA)
            shadowOf(Looper.getMainLooper()).idleFor(Duration.ofMillis(100))

            assertEquals(1, payloads.size)
            assertEquals(adapterA.editorId, payloads.single()["editorId"])
            assertEquals("7", payloads.single()["documentRevision"])
            assertEquals(0, view.pendingEditorUpdateEventCountForTesting())
        } finally {
            EditorV2Registry.remove(adapterA.editorId)
            EditorV2Registry.remove(adapterB.editorId)
            NativeEditorViewRegistry.unregister(tokenA, view)
            NativeEditorViewRegistry.unregister(tokenB, view)
        }
    }

    @Test
    fun `editor id set while detached preserves identity without binding input`() {
        val expoContext = testExpoContext(RuntimeEnvironment.getApplication())
        val view = NativeEditorExpoView(expoContext.context, expoContext.appContext)
        val editorId = 23456L

        view.setEditorId(editorId)

        assertEquals(editorId, view.richTextView.editorId)
        assertEquals(0L, view.richTextView.editorEditText.editorId)

        NativeEditorViewRegistry.unregister(editorId, view)
    }

    @Test
    fun `editor id change preserves pending update until matching editor id arrives`() {
        val expoContext = testExpoContext(RuntimeEnvironment.getApplication())
        val view = NativeEditorExpoView(expoContext.context, expoContext.appContext)
        val updateJson = """{"renderElements":[],"selection":{"type":"text","anchor":0,"head":0}}"""

        view.setPendingEditorUpdateJson(updateJson)
        view.setPendingEditorUpdateRevision(7)
        view.setEditorId(33445L)

        assertEquals(updateJson, view.pendingEditorUpdateJsonForTesting())
        assertEquals(7, view.pendingEditorUpdateRevisionForTesting())
        assertNull(view.pendingEditorUpdateEditorIdForTesting())

        view.setPendingEditorUpdateEditorId(33445L)

        assertEquals(33445L, view.pendingEditorUpdateEditorIdForTesting())
        assertEquals(updateJson, view.pendingEditorUpdateJsonForTesting())

        NativeEditorViewRegistry.unregister(33445L, view)
    }

    @Test
    fun `editor id change drops pending update scoped to a different editor`() {
        val expoContext = testExpoContext(RuntimeEnvironment.getApplication())
        val view = NativeEditorExpoView(expoContext.context, expoContext.appContext)
        val updateJson = """{"renderElements":[],"selection":{"type":"text","anchor":0,"head":0}}"""

        view.setPendingEditorUpdateJson(updateJson)
        view.setPendingEditorUpdateEditorId(111L)
        view.setPendingEditorUpdateRevision(3)

        view.setEditorId(222L)

        assertNull(view.pendingEditorUpdateJsonForTesting())
        assertEquals(0, view.pendingEditorUpdateRevisionForTesting())

        NativeEditorViewRegistry.unregister(222L, view)
    }

    @Test
    fun `editor id change resets last document version`() {
        val expoContext = testExpoContext(RuntimeEnvironment.getApplication())
        val view = NativeEditorExpoView(expoContext.context, expoContext.appContext)

        view.setLastDocumentVersionForTesting("20")

        assertEquals("20", view.lastDocumentVersionForTesting())

        view.setEditorId(66778L)

        assertNull(view.lastDocumentVersionForTesting())

        NativeEditorViewRegistry.unregister(66778L, view)
    }

    @Test
    fun `pending JS update reapplies without redelivery of unchanged editor id`() {
        val expoContext = testExpoContext(RuntimeEnvironment.getApplication())
        val view = NativeEditorExpoView(expoContext.context, expoContext.appContext)
        val editText = view.richTextView.editorEditText

        view.onAddonEventForTesting = {}
        view.onRefreshToolbarStateFromEditorSelectionForTesting = { null }
        view.onEditorReadyForTesting = {}
        val bound = bindFakeEditorForViewTest(view, "")
        val editorId = bound.viewToken

        view.setPendingEditorUpdateJson(bound.setEngineText("first"))
        view.setPendingEditorUpdateEditorId(editorId)
        view.setPendingEditorUpdateRevision(1)
        view.applyPendingEditorUpdateIfNeeded()

        assertEquals("first", editText.text?.toString())
        assertNull(view.pendingEditorUpdateEditorIdForTesting())

        view.setPendingEditorUpdateJson(bound.setEngineText(""))
        view.setPendingEditorUpdateRevision(2)
        view.applyPendingEditorUpdateIfNeeded()

        assertEquals("", editText.text?.toString())
        assertNull(view.pendingEditorUpdateJsonForTesting())
        assertEquals(0, view.pendingEditorUpdateRevisionForTesting())

        bound.release(view)
    }

    @Test
    fun `off main view command update is ignored after editor rebind`() {
        val expoContext = testExpoContext(RuntimeEnvironment.getApplication())
        val view = NativeEditorExpoView(expoContext.context, expoContext.appContext)
        val firstEditorId = 99101L
        val secondEditorId = 99102L
        val editText = view.richTextView.editorEditText
        val updateJson = renderUpdateJson("stale")

        view.richTextView.setEditorIdWhileDetached(firstEditorId)
        editText.applyUpdateJSON(renderUpdateJson("first"), notifyListener = false)
        editText.setSelection(0)
        editText.editorId = firstEditorId
        view.setAttachedToNativeWindowForTesting(true)

        val posted = CountDownLatch(1)
        Thread {
            view.applyEditorUpdate(updateJson)
            posted.countDown()
        }.start()
        assertTrue(posted.await(2, java.util.concurrent.TimeUnit.SECONDS))

        view.richTextView.setEditorIdWhileDetached(secondEditorId)
        editText.applyUpdateJSON(renderUpdateJson("second"), notifyListener = false)
        editText.setSelection(0)
        editText.editorId = secondEditorId

        shadowOf(Looper.getMainLooper()).idle()

        assertEquals("second", editText.text?.toString())
        assertNull(view.pendingViewCommandUpdateJsonForTesting())
    }
}
