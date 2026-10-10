package com.apollohg.editor

import android.graphics.Typeface
import android.os.Looper
import android.os.SystemClock
import android.text.Annotation
import android.text.style.AbsoluteSizeSpan
import android.text.style.StyleSpan
import android.view.KeyEvent
import android.view.inputmethod.EditorInfo
import android.view.inputmethod.InputConnection
import org.json.JSONArray
import org.json.JSONObject
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotEquals
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.RuntimeEnvironment
import org.robolectric.Shadows.shadowOf
import org.robolectric.annotation.Config
import org.robolectric.annotation.GraphicsMode

@RunWith(RobolectricTestRunner::class)
@Config(sdk = [34])
@GraphicsMode(GraphicsMode.Mode.NATIVE)
internal class EditorCommandSequenceTest {
    private var keyEventOrdinal = 0L

    private fun backspace(
        input: EditorEditText,
        label: String,
        connection: InputConnection? = null
    ) = pressKey(input, KeyEvent.KEYCODE_DEL, label, connection)

    private fun pressKey(
        input: EditorEditText,
        keyCode: Int,
        label: String,
        existingConnection: InputConnection? = null
    ) {
        val connection =
            existingConnection ?: requireNotNull(input.onCreateInputConnection(EditorInfo()))
        val time = SystemClock.uptimeMillis() + ++keyEventOrdinal * KEY_EVENT_INTERVAL_MS
        assertTrue(
            "$label key down rejected",
            connection.sendKeyEvent(
                KeyEvent(time, time, KeyEvent.ACTION_DOWN, keyCode, 0)
            )
        )
        assertTrue(
            "$label key up rejected",
            connection.sendKeyEvent(
                KeyEvent(time, time, KeyEvent.ACTION_UP, keyCode, 0)
            )
        )
    }

    @Test fun `root node pairs with selection deletion`() = runPairs(ROOT_CONTEXT, "selection")

    @Test fun `root node pairs with repeated backspace`() = runPairs(ROOT_CONTEXT, "backspace")

    @Test fun `body cell node pairs with selection deletion`() = runPairs("table_cell", "selection")

    @Test fun `body cell node pairs with repeated backspace`() = runPairs("table_cell", "backspace")

    @Test fun `header cell node pairs with selection deletion`() = runPairs(
        "table_header",
        "selection"
    )

    @Test fun `header cell node pairs with repeated backspace`() = runPairs(
        "table_header",
        "backspace"
    )

    @Test fun `root unicode grapheme and code point sequences`() = runUnicode(ROOT_CONTEXT)

    @Test fun `body cell unicode grapheme and code point sequences`() = runUnicode("table_cell")

    @Test fun `header cell unicode grapheme and code point sequences`() = runUnicode("table_header")

    @Test
    fun `protected image deletion remains usable after removing an earlier rule`() =
        runPairs(ROOT_CONTEXT, "backspace", firstName = "horizontalRule", secondName = "image")

    @Test
    fun `reverse partial replacements accept continued typing in prose and both cell roles`() {
        val corpus = corpus()
        val original = corpus.getString("prefix") + corpus.getString("suffix")
        val replacement = corpus.getString("recovery")
        for (context in CONTEXTS) {
            withSequenceEditor(corpus, context, true) { view, adapter, neighbour ->
                val label = "$context/reverse replacement"
                val input = view.activeTextInput
                val end = original.length - PARTIAL_SELECTION_EDGE_LENGTH
                input.setSelection(end, PARTIAL_SELECTION_EDGE_LENGTH)
                check(view, adapter, context, original, neighbour, "$label/selected")
                val selected = requireNotNull(adapter.cachedAtomicRenderSelection())
                assertTrue(
                    "$label reversed engine selection was lost: $selected",
                    selected.getInt("anchor") > selected.getInt("head")
                )
                assertEquals("$label native reverse anchor", end, input.selectionStart)
                assertEquals(
                    "$label native reverse head",
                    PARTIAL_SELECTION_EDGE_LENGTH,
                    input.selectionEnd
                )
                val connection = requireNotNull(input.onCreateInputConnection(EditorInfo()))
                assertTrue(
                    "$label replacement rejected",
                    connection.commitText(replacement, CURSOR_AFTER_COMMIT)
                )
                val left = original.take(PARTIAL_SELECTION_EDGE_LENGTH)
                val right = original.takeLast(PARTIAL_SELECTION_EDGE_LENGTH)
                check(
                    view,
                    adapter,
                    context,
                    left + replacement + right,
                    neighbour,
                    "$label/replaced"
                )
                checkPlainCaret(
                    adapter,
                    context,
                    input,
                    left.length + replacement.length,
                    left.length + replacement.length,
                    "$label/replaced"
                )
                probe(
                    view,
                    adapter,
                    corpus,
                    context,
                    left + replacement + corpus.getString("probe") + right,
                    neighbour,
                    label,
                    connection
                )
            }
        }
    }

    @Test
    fun `Return followed immediately by Backspace rejoins editable prose and both cell roles`() {
        val corpus = corpus()
        val left = corpus.getString("prefix")
        val right = corpus.getString("suffix")
        for (context in CONTEXTS) {
            withSequenceEditor(corpus, context, true) { view, adapter, neighbour ->
                val label = "$context/Return Backspace"
                val input = view.activeTextInput
                input.setSelection(left.length)
                val before = adapter.documentJson()
                val connection = requireNotNull(input.onCreateInputConnection(EditorInfo()))
                assertTrue(
                    "$label Return rejected",
                    connection.commitText(PARAGRAPH_SEPARATOR, CURSOR_AFTER_COMMIT)
                )
                check(view, adapter, context, left + right, neighbour, "$label/split")
                assertEquals(
                    "$label paragraph split",
                    SPLIT_PARAGRAPH_COUNT,
                    scope(
                        JSONObject(requireNotNull(adapter.documentJson())),
                        context
                    ).getJSONArray("content").length()
                )
                assertEquals(
                    "$label native split text",
                    left + PARAGRAPH_SEPARATOR + right,
                    input.text.toString()
                )
                checkPlainCaret(
                    adapter,
                    context,
                    input,
                    left.length + PARAGRAPH_SEPARATOR.length,
                    START_OFFSET,
                    "$label/split",
                    paragraphIndex = SECOND_PARAGRAPH
                )
                backspace(input, "$label/join", connection)
                check(view, adapter, context, left + right, neighbour, "$label/join")
                assertEquals(
                    "$label join failed to restore exact document",
                    before,
                    adapter.documentJson()
                )
                assertEquals(
                    "$label native join left a separator",
                    left + right,
                    input.text.toString()
                )
                checkPlainCaret(adapter, context, input, left.length, left.length, "$label/join")
                probe(
                    view,
                    adapter,
                    corpus,
                    context,
                    left + corpus.getString("probe") + right,
                    neighbour,
                    "$label/continued"
                )
            }
        }
    }

    @Test
    fun `forward delete joins a marked paragraph boundary in prose and both cell roles`() {
        val corpus = corpus()
        val left = corpus.getString("prefix")
        val right = corpus.getString("suffix")
        for (context in CONTEXTS) {
            withSequenceEditor(corpus, context, false) { view, adapter, neighbour ->
                val label = "$context/forward rich boundary"
                val input = view.activeTextInput
                input.setSelection(START_OFFSET, left.length)
                toggleMark(input, adapter, BOLD_MARK, "$label/bold")
                input.setSelection(left.length)
                check(view, adapter, context, left + right, neighbour, "$label/before")
                assertEquals(
                    "$label needs a real block boundary",
                    SPLIT_PARAGRAPH_COUNT,
                    scope(
                        JSONObject(requireNotNull(adapter.documentJson())),
                        context
                    ).getJSONArray("content").length()
                )
                pressKey(input, KeyEvent.KEYCODE_FORWARD_DEL, label)
                check(view, adapter, context, left + right, neighbour, "$label/join")
                val blocks = scope(
                    JSONObject(requireNotNull(adapter.documentJson())),
                    context
                ).getJSONArray("content")
                assertEquals("$label did not join paragraphs", ONE_PARAGRAPH, blocks.length())
                assertEquals(
                    "$label native separator survived",
                    left + right,
                    input.text.toString()
                )
                checkPlainCaret(adapter, context, input, left.length, left.length, "$label/join")
                checkMarkedText(
                    input,
                    adapter,
                    context,
                    left,
                    setOf(BOLD_MARK),
                    START_OFFSET,
                    label
                )
                checkMarkedText(input, adapter, context, right, emptySet(), left.length, label)
                probe(
                    view,
                    adapter,
                    corpus,
                    context,
                    left + corpus.getString("probe") + right,
                    neighbour,
                    "$label/continued"
                )
            }
        }
    }

    @Test
    fun `stored marks typing history keeps all contexts editable`() {
        val corpus = corpus()
        val left = corpus.getString("prefix")
        val right = corpus.getString("suffix")
        val typed = corpus.getString("probe")
        val recovery = corpus.getString("recovery")
        for (context in CONTEXTS) {
            withSequenceEditor(corpus, context, true) { view, adapter, neighbour ->
                val label = "$context/mark history"
                val input = view.activeTextInput
                input.setSelection(left.length)
                toggleMark(input, adapter, BOLD_MARK, "$label/bold")
                check(view, adapter, context, left + right, neighbour, "$label/bold")
                checkPlainCaret(adapter, context, input, left.length, left.length, "$label/bold")
                toggleMark(input, adapter, ITALIC_MARK, "$label/italic")
                check(view, adapter, context, left + right, neighbour, "$label/italic")
                checkPlainCaret(adapter, context, input, left.length, left.length, "$label/italic")
                val before = adapter.documentJson()
                assertTrue(
                    "$label typing rejected",
                    requireNotNull(
                        input.onCreateInputConnection(EditorInfo())
                    ).commitText(typed, CURSOR_AFTER_COMMIT)
                )
                check(view, adapter, context, left + typed + right, neighbour, "$label/type")
                checkMarkedText(
                    input,
                    adapter,
                    context,
                    typed,
                    setOf(BOLD_MARK, ITALIC_MARK),
                    left.length,
                    "$label/type"
                )
                val marked = adapter.documentJson()
                applyHistory(input, adapter, undo = true, "$label/undo")
                check(view, adapter, context, left + right, neighbour, "$label/undo")
                assertEquals("$label undo changed earlier content", before, adapter.documentJson())
                checkPlainCaret(adapter, context, input, left.length, left.length, "$label/undo")
                applyHistory(input, adapter, undo = false, "$label/redo")
                check(view, adapter, context, left + typed + right, neighbour, "$label/redo")
                assertEquals(
                    "$label redo did not restore marked text",
                    marked,
                    adapter.documentJson()
                )
                checkMarkedText(
                    input,
                    adapter,
                    context,
                    typed,
                    setOf(BOLD_MARK, ITALIC_MARK),
                    left.length,
                    "$label/redo"
                )
                toggleMark(input, adapter, BOLD_MARK, "$label/unbold")
                check(view, adapter, context, left + typed + right, neighbour, "$label/unbold")
                checkPlainCaret(
                    adapter,
                    context,
                    input,
                    left.length + typed.length,
                    left.length + typed.length,
                    "$label/unbold"
                )
                assertTrue(
                    "$label continued typing rejected",
                    requireNotNull(
                        input.onCreateInputConnection(EditorInfo())
                    ).commitText(recovery, CURSOR_AFTER_COMMIT)
                )
                check(
                    view,
                    adapter,
                    context,
                    left + typed + recovery + right,
                    neighbour,
                    "$label/continued"
                )
                checkMarkedText(
                    input,
                    adapter,
                    context,
                    recovery,
                    setOf(ITALIC_MARK),
                    left.length + typed.length,
                    "$label/continued"
                )
                checkPlainCaret(
                    adapter,
                    context,
                    input,
                    left.length + typed.length + recovery.length,
                    left.length + typed.length + recovery.length,
                    "$label/continued"
                )
            }
        }
    }

    @Test
    fun `native rich range deletion history restores the submitted selection`() {
        val corpus = corpus()
        for (context in CONTEXTS) {
            for (fragment in corpus.getJSONArray("fragments").objects()) {
                withSequenceEditor(corpus, context, fragment.optBoolean("inline")) {
                        view,
                        adapter,
                        neighbour
                    ->
                    val label = "$context/native rich history/${fragment.getString("name") }"
                    val original =
                        corpus.getString("prefix") + fragment.getString("text") +
                            corpus.getString("suffix")
                    val input = view.activeTextInput
                    input.setSelection(corpus.getString("prefix").length)
                    val local = requireNotNull(input.currentScalarSelection())
                    val global =
                        requireNotNull(input.inputScalarSelection(local.first, local.second))
                    val content = JSONObject().put("type", "doc")
                        .put("content", JSONArray().put(fragment.getJSONObject("node")))
                    assertTrue(
                        "$label insert rejected",
                        input.applyUpdateJSON(
                            requireNotNull(
                                adapter.insertContentJsonAtSelection(
                                    content.toString(),
                                    global.first,
                                    global.second
                                )
                            )
                        )
                    )
                    check(view, adapter, context, original, neighbour, "$label/insert")
                    val before = requireNotNull(adapter.documentJson())
                    val oldSelection = requireNotNull(
                        adapter.cachedAtomicRenderSelection()
                    ).toString()
                    selectNativeRangeWithoutSync(input, START_OFFSET, input.text.length, label)
                    assertEquals(
                        "$label native selection unexpectedly synchronized the old caret",
                        oldSelection,
                        requireNotNull(adapter.cachedAtomicRenderSelection()).toString()
                    )
                    val nativeRange =
                        requireNotNull(input.rawScalarSelection(input.text.toString()))
                    val selected = requireNotNull(
                        input.inputScalarSelection(nativeRange.first, nativeRange.second)
                    )
                        .let {
                            requireNotNull(adapter.docPositionForScalar(it.first)) to
                                requireNotNull(adapter.docPositionForScalar(it.second))
                        }
                    backspace(input, "$label/clear")
                    check(view, adapter, context, "", neighbour, "$label/clear")
                    val cleared = requireNotNull(adapter.documentJson())
                    val clearedSelection = requireNotNull(
                        adapter.cachedAtomicRenderSelection()
                    ).let {
                        it.getInt("anchor") to
                            it.getInt("head")
                    }
                    fun restored(stage: String) {
                        check(view, adapter, context, original, neighbour, stage)
                        assertTrue(
                            "$stage undo did not restore exact nodes and attrs",
                            input.jsonValuesEqual(
                                JSONObject(before),
                                JSONObject(requireNotNull(adapter.documentJson()))
                            )
                        )
                        val actual = requireNotNull(adapter.cachedAtomicRenderSelection())
                        assertEquals(
                            "$stage restored old engine caret instead of native range",
                            selected,
                            actual.getInt("anchor") to actual.getInt("head")
                        )
                        assertEquals(
                            "$stage native restored selection start",
                            START_OFFSET,
                            input.selectionStart
                        )
                        assertEquals(
                            "$stage native restored selection end",
                            input.text.length,
                            input.selectionEnd
                        )
                    }
                    applyHistory(input, adapter, undo = true, "$label/undo")
                    restored("$label/undo")
                    applyHistory(input, adapter, undo = false, "$label/redo")
                    check(view, adapter, context, "", neighbour, "$label/redo")
                    assertTrue(
                        "$label redo changed cleared structure",
                        input.jsonValuesEqual(
                            JSONObject(cleared),
                            JSONObject(requireNotNull(adapter.documentJson()))
                        )
                    )
                    val redone = requireNotNull(adapter.cachedAtomicRenderSelection())
                    assertEquals(
                        "$label redo changed caret",
                        clearedSelection,
                        redone.getInt("anchor") to redone.getInt("head")
                    )
                    applyHistory(input, adapter, undo = true, "$label/restore range")
                    restored("$label/restore range")
                    val recovery = corpus.getString("recovery")
                    val connection = requireNotNull(input.onCreateInputConnection(EditorInfo()))
                    assertTrue(
                        "$label restored-range replacement rejected",
                        connection.commitText(recovery, CURSOR_AFTER_COMMIT)
                    )
                    check(
                        view,
                        adapter,
                        context,
                        recovery,
                        neighbour,
                        "$label/replace restored range"
                    )
                    val replaced = requireNotNull(adapter.documentJson())
                    applyHistory(input, adapter, undo = true, "$label/undo replacement")
                    restored("$label/undo replacement")
                    applyHistory(input, adapter, undo = false, "$label/redo replacement")
                    check(view, adapter, context, recovery, neighbour, "$label/redo replacement")
                    assertTrue(
                        "$label replacement redo changed structure",
                        input.jsonValuesEqual(
                            JSONObject(replaced),
                            JSONObject(requireNotNull(adapter.documentJson()))
                        )
                    )
                    probe(
                        view,
                        adapter,
                        corpus,
                        context,
                        recovery + corpus.getString("probe"),
                        neighbour,
                        "$label/continued"
                    )
                }
            }
        }
    }

    @Test
    fun `multiline rich replacement keeps native input and history usable`() {
        val corpus = corpus()
        val left = corpus.getString("prefix")
        val right = corpus.getString("suffix")
        val rich = paragraph(left, setOf(BOLD_MARK))
        rich.getJSONArray("content").put(JSONObject().put("type", HARD_BREAK_NODE))
            .put(textNode(right))
        val source = JSONArray().put(rich)
        val lines = MULTILINE_REPLACEMENT.split(PARAGRAPH_SEPARATOR)
        val fullText = lines.joinToString("")
        val shortenedLines = lines.dropLast(ONE_PARAGRAPH) + lines.last().dropLast(ONE_CHARACTER)
        val shortened = shortenedLines.joinToString("")
        val typed = corpus.getString("probe")
        for (context in CONTEXTS) {
            withSequenceEditor(corpus, context, true, source) { view, adapter, neighbour ->
                val label = "$context/multiline rich replacement"
                val input = view.activeTextInput
                check(view, adapter, context, left + right, neighbour, "$label/source")
                checkMarkedText(
                    input,
                    adapter,
                    context,
                    left,
                    setOf(BOLD_MARK),
                    START_OFFSET,
                    label
                )
                val oldSelection = requireNotNull(adapter.cachedAtomicRenderSelection()).toString()
                selectNativeRangeWithoutSync(input, START_OFFSET, input.text.length, label)
                assertEquals(
                    "$label native range was pre-synchronized",
                    oldSelection,
                    requireNotNull(adapter.cachedAtomicRenderSelection()).toString()
                )
                val native = requireNotNull(input.rawScalarSelection(input.text.toString()))
                val global = requireNotNull(input.inputScalarSelection(native.first, native.second))
                val requested = requireNotNull(adapter.docPositionForScalar(global.first)) to
                    requireNotNull(adapter.docPositionForScalar(global.second))
                val original = SequenceSnapshot(
                    requireNotNull(adapter.documentJson()),
                    requested,
                    START_OFFSET to input.text.length
                )
                val connection = requireNotNull(input.onCreateInputConnection(EditorInfo()))
                assertTrue(
                    "$label replacement rejected",
                    connection.commitText(MULTILINE_REPLACEMENT, CURSOR_AFTER_COMMIT)
                )
                check(view, adapter, context, fullText, neighbour, "$label/replaced")
                checkParagraphs(input, adapter, context, lines, setOf(BOLD_MARK), label)
                checkPlainCaret(
                    adapter,
                    context,
                    input,
                    MULTILINE_REPLACEMENT.length,
                    lines.last().length,
                    "$label/replaced",
                    paragraphIndex = LAST_MULTILINE_PARAGRAPH
                )
                val replaced = sequenceSnapshot(input, adapter)
                backspace(input, "$label/immediate Backspace", connection)
                check(view, adapter, context, shortened, neighbour, "$label/backspace")
                checkParagraphs(input, adapter, context, shortenedLines, setOf(BOLD_MARK), label)
                checkPlainCaret(
                    adapter,
                    context,
                    input,
                    MULTILINE_REPLACEMENT.length - ONE_CHARACTER,
                    shortenedLines.last().length,
                    "$label/backspace",
                    paragraphIndex = LAST_MULTILINE_PARAGRAPH
                )
                val deleted = sequenceSnapshot(input, adapter)
                assertTrue(
                    "$label continued typing rejected",
                    connection.commitText(typed, CURSOR_AFTER_COMMIT)
                )
                check(view, adapter, context, shortened + typed, neighbour, "$label/continued")
                checkParagraphs(
                    input,
                    adapter,
                    context,
                    shortenedLines.dropLast(ONE_PARAGRAPH) + (shortenedLines.last() + typed),
                    setOf(BOLD_MARK),
                    label
                )
                val continued = sequenceSnapshot(input, adapter)
                for ((frame, text) in listOf(
                    deleted to shortened,
                    replaced to fullText,
                    original to (left + right)
                )) {
                    applyHistory(input, adapter, undo = true, "$label/undo")
                    checkRestoredSnapshot(
                        view,
                        adapter,
                        context,
                        neighbour,
                        frame,
                        text,
                        "$label/undo"
                    )
                }
                for ((frame, text) in listOf(
                    replaced to fullText,
                    deleted to shortened,
                    continued to (shortened + typed)
                )) {
                    applyHistory(input, adapter, undo = false, "$label/redo")
                    checkRestoredSnapshot(
                        view,
                        adapter,
                        context,
                        neighbour,
                        frame,
                        text,
                        "$label/redo"
                    )
                }
                probe(
                    view,
                    adapter,
                    corpus,
                    context,
                    shortened + typed + typed,
                    neighbour,
                    "$label/after history"
                )
            }
        }
    }

    @Test
    fun `partial cross paragraph rich selections delete and replace with native history`() {
        val corpus = corpus()
        val first = paragraph(CROSS_PARAGRAPH_PREFIX)
        first.getJSONArray("content").put(JSONObject().put("type", HARD_BREAK_NODE))
            .put(textNode(CROSS_PARAGRAPH_MIDDLE))
        val source = JSONArray().put(first).put(paragraph(CROSS_PARAGRAPH_SUFFIX))
        for (context in CONTEXTS) {
            for (replace in listOf(false, true)) {
                withSequenceEditor(corpus, context, false, source) { view, adapter, neighbour ->
                    val label = "$context/partial rich boundary/replace=$replace"
                    val input = view.activeTextInput
                    val sourceText = CROSS_PARAGRAPH_PREFIX + CROSS_PARAGRAPH_MIDDLE +
                        CROSS_PARAGRAPH_SUFFIX
                    check(view, adapter, context, sourceText, neighbour, "$label/source")
                    val oldSelection = requireNotNull(
                        adapter.cachedAtomicRenderSelection()
                    ).toString()
                    selectNativeRangeWithoutSync(
                        input,
                        ONE_CHARACTER,
                        input.text.length - ONE_CHARACTER,
                        label
                    )
                    assertEquals(
                        "$label native range was pre-synchronized",
                        oldSelection,
                        requireNotNull(adapter.cachedAtomicRenderSelection()).toString()
                    )
                    val native = requireNotNull(input.rawScalarSelection(input.text.toString()))
                    val global = requireNotNull(
                        input.inputScalarSelection(native.first, native.second)
                    )
                    val requested = requireNotNull(adapter.docPositionForScalar(global.first)) to
                        requireNotNull(adapter.docPositionForScalar(global.second))
                    val original = SequenceSnapshot(
                        requireNotNull(adapter.documentJson()),
                        requested,
                        input.selectionStart to input.selectionEnd
                    )
                    val connection = requireNotNull(input.onCreateInputConnection(EditorInfo()))
                    val inserted = if (replace) CROSS_PARAGRAPH_REPLACEMENT else ""
                    if (replace) {
                        assertTrue(
                            "$label replacement rejected",
                            connection.commitText(inserted, CURSOR_AFTER_COMMIT)
                        )
                    } else {
                        backspace(input, "$label/delete", connection)
                    }
                    val expected = CROSS_PARAGRAPH_PREFIX.take(ONE_CHARACTER) + inserted +
                        CROSS_PARAGRAPH_SUFFIX.takeLast(ONE_CHARACTER)
                    check(view, adapter, context, expected, neighbour, "$label/edited")
                    checkParagraphs(input, adapter, context, listOf(expected), emptySet(), label)
                    checkPlainCaret(
                        adapter,
                        context,
                        input,
                        ONE_CHARACTER + inserted.length,
                        ONE_CHARACTER + inserted.length,
                        "$label/edited"
                    )
                    val edited = sequenceSnapshot(input, adapter)
                    applyHistory(input, adapter, undo = true, "$label/undo")
                    checkRestoredSnapshot(
                        view,
                        adapter,
                        context,
                        neighbour,
                        original,
                        sourceText,
                        "$label/undo"
                    )
                    applyHistory(input, adapter, undo = false, "$label/redo")
                    checkRestoredSnapshot(
                        view,
                        adapter,
                        context,
                        neighbour,
                        edited,
                        expected,
                        "$label/redo"
                    )
                    probe(
                        view,
                        adapter,
                        corpus,
                        context,
                        CROSS_PARAGRAPH_PREFIX.take(ONE_CHARACTER) + inserted +
                            corpus.getString("probe") +
                            CROSS_PARAGRAPH_SUFFIX.takeLast(ONE_CHARACTER),
                        neighbour,
                        "$label/continued"
                    )
                }
            }
        }
    }

    private data class SequenceSnapshot(
        val document: String,
        val documentSelection: Pair<Int, Int>,
        val nativeSelection: Pair<Int, Int>
    )

    private fun sequenceSnapshot(
        input: EditorEditText,
        adapter: EditorV2Adapter
    ): SequenceSnapshot {
        val selection = requireNotNull(adapter.cachedAtomicRenderSelection())
        return SequenceSnapshot(
            requireNotNull(adapter.documentJson()),
            selection.getInt("anchor") to selection.getInt("head"),
            input.selectionStart to input.selectionEnd
        )
    }

    private fun checkRestoredSnapshot(
        view: RichTextEditorView,
        adapter: EditorV2Adapter,
        context: String,
        neighbour: String?,
        snapshot: SequenceSnapshot,
        expected: String,
        label: String
    ) {
        check(view, adapter, context, expected, neighbour, label)
        val input = view.activeTextInput
        assertTrue(
            "$label history changed exact nodes or attrs",
            input.jsonValuesEqual(
                JSONObject(snapshot.document),
                JSONObject(requireNotNull(adapter.documentJson()))
            )
        )
        val selection = requireNotNull(adapter.cachedAtomicRenderSelection())
        assertEquals(
            "$label history changed document selection",
            snapshot.documentSelection,
            selection.getInt("anchor") to selection.getInt("head")
        )
        assertEquals(
            "$label history changed native selection",
            snapshot.nativeSelection,
            input.selectionStart to input.selectionEnd
        )
    }

    private fun checkParagraphs(
        input: EditorEditText,
        adapter: EditorV2Adapter,
        context: String,
        texts: List<String>,
        marks: Set<String>,
        label: String
    ) {
        val expected = JSONArray().apply { texts.forEach { put(paragraph(it, marks)) } }
        val actual = scope(JSONObject(requireNotNull(adapter.documentJson())), context)
            .getJSONArray("content")
        assertTrue(
            "$label wrong paragraph structure: expected=$expected actual=$actual",
            input.jsonValuesEqual(expected, actual)
        )
        assertEquals(
            "$label wrong native paragraph text",
            texts.joinToString(PARAGRAPH_SEPARATOR),
            input.text.toString()
        )
        var offset = START_OFFSET
        for (text in texts) {
            checkMarkedText(input, adapter, context, text, marks, offset, label)
            offset += text.length + PARAGRAPH_SEPARATOR.length
        }
    }

    private fun textNode(text: String, marks: Set<String> = emptySet()): JSONObject =
        JSONObject().put("type", TEXT_NODE).put("text", text).apply {
            if (marks.isNotEmpty()) {
                put(
                    "marks",
                    JSONArray().apply {
                        marks.forEach { put(JSONObject().put("type", it)) }
                    }
                )
            }
        }

    private fun paragraph(text: String, marks: Set<String> = emptySet()): JSONObject =
        JSONObject().put("type", "paragraph").put("content", JSONArray().put(textNode(text, marks)))

    private fun selectNativeRangeWithoutSync(
        input: EditorEditText,
        start: Int,
        end: Int,
        label: String
    ) {
        assertTrue(
            "$label transient native selection rejected",
            input.runWithTransientInputMutationGuard {
                input.setSelection(start, end)
                true
            }
        )
    }

    @Test
    fun `empty commits and beginning Backspace retain editable roots and cells`() {
        val corpus = corpus()
        val emptyCorpus = JSONObject(corpus.toString()).put("prefix", "").put("suffix", "")
        val unicode = corpus.getJSONArray("unicode").objects().single {
            it.getString("name") ==
                "zwj"
        }.getString("text")
        for (context in CONTEXTS) {
            withSequenceEditor(emptyCorpus, context, true) { view, adapter, neighbour ->
                val label = "$context/empty no-op input"
                val input = view.activeTextInput
                input.setSelection(START_OFFSET)
                check(view, adapter, context, "", neighbour, "$label/before")
                val before = adapter.documentJson()
                val revision = adapter.baseDocumentRevision
                val selected = requireNotNull(adapter.cachedAtomicRenderSelection()).let {
                    it.getInt("anchor") to
                        it.getInt("head")
                }
                val caret = android.graphics.RectF(requireNotNull(input.caretRect()))
                val connection = requireNotNull(input.onCreateInputConnection(EditorInfo()))
                assertTrue(
                    "$label empty commit rejected",
                    connection.commitText("", CURSOR_AFTER_COMMIT)
                )
                check(view, adapter, context, "", neighbour, "$label/empty commit")
                assertEquals("$label empty commit changed document", before, adapter.documentJson())
                assertEquals(
                    "$label empty commit changed revision",
                    revision,
                    adapter.baseDocumentRevision
                )
                assertEquals("$label empty commit moved visible caret", caret, input.caretRect())
                backspace(input, "$label/beginning Backspace", connection)
                check(view, adapter, context, "", neighbour, "$label/beginning Backspace")
                assertEquals(
                    "$label beginning Backspace changed document",
                    before,
                    adapter.documentJson()
                )
                assertEquals(
                    "$label beginning Backspace changed revision",
                    revision,
                    adapter.baseDocumentRevision
                )
                val after = requireNotNull(adapter.cachedAtomicRenderSelection())
                assertEquals(
                    "$label no-op changed document caret",
                    selected,
                    after.getInt("anchor") to after.getInt("head")
                )
                assertEquals("$label no-op moved visible caret", caret, input.caretRect())
                assertTrue(
                    "$label Unicode commit rejected",
                    connection.commitText(unicode, CURSOR_AFTER_COMMIT)
                )
                check(view, adapter, context, unicode, neighbour, "$label/Unicode")
                val typed = adapter.documentJson()
                assertTrue(
                    "$label Return rejected",
                    connection.commitText(PARAGRAPH_SEPARATOR, CURSOR_AFTER_COMMIT)
                )
                check(view, adapter, context, unicode, neighbour, "$label/Return")
                assertEquals(
                    "$label Return failed to split",
                    SPLIT_PARAGRAPH_COUNT,
                    scope(
                        JSONObject(requireNotNull(adapter.documentJson())),
                        context
                    ).getJSONArray("content").length()
                )
                backspace(input, "$label/join", connection)
                check(view, adapter, context, unicode, neighbour, "$label/join")
                assertEquals(
                    "$label immediate join changed Unicode document",
                    typed,
                    adapter.documentJson()
                )
                probe(
                    view,
                    adapter,
                    corpus,
                    context,
                    unicode + corpus.getString("probe"),
                    neighbour,
                    "$label/continued",
                    connection
                )
                clear(view, adapter, context, "selection", TEXT_NODE, false, "$label/clear")
                check(view, adapter, context, "", neighbour, "$label/clear")
                assertTrue(
                    "$label retyping rejected",
                    requireNotNull(
                        input.onCreateInputConnection(EditorInfo())
                    ).commitText(corpus.getString("recovery"), CURSOR_AFTER_COMMIT)
                )
                check(
                    view,
                    adapter,
                    context,
                    corpus.getString("recovery"),
                    neighbour,
                    "$label/retype"
                )
            }
        }
    }

    private fun toggleMark(
        input: EditorEditText,
        adapter: EditorV2Adapter,
        mark: String,
        label: String
    ) {
        val local = requireNotNull(input.currentScalarSelection())
        val global = requireNotNull(input.inputScalarSelection(local.first, local.second))
        val update = requireNotNull(adapter.toggleMark(mark, global.first, global.second)) {
            "$label mark command rejected: ${adapter.debugNotes}"
        }
        input.applyUpdateJSON(update)
        assertTrue(
            "$label mark update lost native authority: ${adapter.debugNotes}",
            input.canDispatchTableCellMutation()
        )
    }

    private fun applyHistory(
        input: EditorEditText,
        adapter: EditorV2Adapter,
        undo: Boolean,
        label: String
    ) {
        val update = requireNotNull(if (undo) adapter.undo() else adapter.redo()) {
            "$label history command rejected: selection=${adapter.cachedAtomicRenderSelection()} " +
                "notes=${adapter.debugNotes} trace=${input.imeTraceSnapshotForTesting()}"
        }
        assertTrue("$label history update rejected", input.applyUpdateJSON(update))
    }

    private fun checkPlainCaret(
        adapter: EditorV2Adapter,
        context: String,
        input: EditorEditText,
        nativeOffset: Int,
        paragraphOffset: Int,
        label: String,
        paragraphIndex: Int = FIRST_PARAGRAPH
    ) {
        assertEquals("$label native caret anchor", nativeOffset, input.selectionStart)
        assertEquals("$label native caret head", nativeOffset, input.selectionEnd)
        val blocks = scope(
            JSONObject(requireNotNull(adapter.documentJson())),
            context
        ).getJSONArray("content").objects()
        val scopeStart = if (context == ROOT_CONTEXT) {
            ROOT_OPENING
        } else {
            adapter.tableRecordsForTesting.values.single().getJSONArray(
                "cells"
            ).getJSONObject(FIRST_CELL).getInt("sourcePos") +
                NODE_OPENING_SIZE
        }
        val position = scopeStart + blocks.take(paragraphIndex).sumOf { block ->
            descendants(block).filter { it.optString("type") == "text" }.sumOf {
                val text = it.getString("text")
                text.codePointCount(START_OFFSET, text.length)
            } + NODE_BOUNDARY_SIZE
        } + NODE_OPENING_SIZE + paragraphOffset
        val selected = requireNotNull(adapter.cachedAtomicRenderSelection())
        assertEquals("$label engine caret anchor: $selected", position, selected.getInt("anchor"))
        assertEquals("$label engine caret head: $selected", position, selected.getInt("head"))
    }

    private fun checkMarkedText(
        input: EditorEditText,
        adapter: EditorV2Adapter,
        context: String,
        text: String,
        marks: Set<String>,
        offset: Int,
        label: String
    ) {
        val nodes = descendants(
            scope(JSONObject(requireNotNull(adapter.documentJson())), context)
        ).filter {
            it.optString("type") ==
                "text" &&
                it.getString("text") == text
        }
        assertEquals("$label marked text node count for $text", ONE_TEXT_NODE, nodes.size)
        assertEquals(
            "$label engine marks for $text",
            marks,
            nodes.single().optJSONArray("marks").objects().map {
                it.getString("type")
            }.toSet()
        )
        val expectedStyle = (if (BOLD_MARK in marks) Typeface.BOLD else Typeface.NORMAL) or
            (if (ITALIC_MARK in marks) Typeface.ITALIC else Typeface.NORMAL)
        for (characterOffset in offset until offset + text.length) {
            val nativeStyles = input.text.getSpans(
                characterOffset,
                characterOffset + ONE_CHARACTER,
                StyleSpan::class.java
            ).filter {
                input.text.getSpanStart(it) <= characterOffset &&
                    input.text.getSpanEnd(it) > characterOffset
            }.fold(Typeface.NORMAL) { style, span -> style or span.style }
            assertEquals(
                "$label native style for $text at $characterOffset",
                expectedStyle,
                nativeStyles
            )
        }
    }

    private fun corpus(): JSONObject = JSONObject(
        RuntimeEnvironment.getApplication().assets.open(CORPUS_ASSET).bufferedReader().use {
            it.readText()
        }
    )

    private fun descendants(node: JSONObject): List<JSONObject> = listOf(node) +
        node.optJSONArray("content").objects().flatMap(::descendants)

    private fun scope(document: JSONObject, context: String): JSONObject {
        if (context == ROOT_CONTEXT) return document
        val tables = document.getJSONArray("content")
        assertEquals("table count", 1, tables.length())
        val rows = tables.getJSONObject(0).getJSONArray("content")
        assertEquals("row count", 1, rows.length())
        val cells = rows.getJSONObject(0).getJSONArray("content")
        assertEquals("cell count", 2, cells.length())
        return cells.getJSONObject(0)
    }

    private fun initialDocument(
        corpus: JSONObject,
        context: String,
        inline: Boolean,
        scopedBlocks: JSONArray? = null
    ): String {
        val prefix = corpus.getString("prefix")
        val suffix = corpus.getString("suffix")
        val paragraphs = scopedBlocks ?: if (inline) {
            JSONArray().put(paragraph(prefix + suffix))
        } else {
            JSONArray().put(paragraph(prefix)).put(paragraph(suffix))
        }
        val blocks = if (context == ROOT_CONTEXT) {
            paragraphs
        } else {
            JSONArray().put(
                JSONObject().put("type", "table").put(
                    "content",
                    JSONArray().put(
                        JSONObject().put("type", "table_row").put(
                            "content",
                            JSONArray()
                                .put(JSONObject().put("type", context).put("content", paragraphs))
                                .put(
                                    JSONObject().put("type", "table_cell").put(
                                        "content",
                                        JSONArray().put(paragraph(corpus.getString("neighbour")))
                                    )
                                )
                        )
                    )
                )
            )
        }
        return JSONObject().put("type", "doc").put("content", blocks).toString()
    }

    private fun check(
        view: RichTextEditorView,
        adapter: EditorV2Adapter,
        context: String,
        expected: String,
        neighbour: String?,
        label: String
    ) {
        shadowOf(Looper.getMainLooper()).idle()
        measureMountedEditor(view, EDITOR_WIDTH, EDITOR_HEIGHT)
        val input = view.activeTextInput
        val document = JSONObject(requireNotNull(adapter.documentJson()))
        val actual = descendants(scope(document, context)).filter { it.optString("type") == "text" }
            .joinToString("") { it.getString("text") }
        val diagnostic = "$label document=$document input=${input.text} " +
            "selection=${input.selectionStart}..${input.selectionEnd} " +
            "errors=${adapter.debugNotes} " +
            "trace=${input.imeTraceSnapshotForTesting()}"
        assertEquals(diagnostic, expected, actual)
        checkNativeRender(input, adapter, scope(document, context), diagnostic)
        if (context != ROOT_CONTEXT) {
            assertEquals(
                "$diagnostic target cell type changed",
                context,
                scope(document, context).getString("type")
            )
            assertFalse("$diagnostic cell input retired", input === view.editorEditText)
            assertEquals(
                "$diagnostic wrong active cell",
                0,
                input.tableCellPositionMap?.binding?.cellIndex
            )
            val cells = document.getJSONArray("content").getJSONObject(0).getJSONArray("content")
                .getJSONObject(0).getJSONArray("content")
            assertEquals(
                "$diagnostic neighbour changed",
                neighbour,
                cells.getJSONObject(1).toString()
            )
        }
        assertTrue("$diagnostic stale input authority", input.canDispatchTableCellMutation())
        assertTrue(
            "$diagnostic caret out of bounds",
            input.selectionStart in 0..input.text.length &&
                input.selectionEnd in 0..input.text.length
        )
        val local = requireNotNull(
            input.currentLogicalScalarSelectionForInput()
                ?: input.rawScalarSelection(input.text.toString())
        ) {
            "$diagnostic unmappable caret"
        }
        val logical = requireNotNull(input.inputScalarSelection(local.first, local.second))
        val selection = JSONObject(
            requireNotNull(adapter.cachedAtomicRenderJson)
        ).getJSONObject("selection")
        assertEquals(
            "$diagnostic engine anchor mismatch",
            selection.getInt("anchor"),
            adapter.docPositionForScalar(logical.first)
        )
        assertEquals(
            "$diagnostic engine head mismatch",
            selection.getInt("head"),
            adapter.docPositionForScalar(logical.second)
        )
        if (input.selectionStart == input.selectionEnd) {
            val caret = requireNotNull(input.caretRect()) { "$diagnostic missing caret" }
            assertTrue(
                "$diagnostic invalid caret=$caret",
                caret.left.isFinite() && caret.top.isFinite() && caret.height() > 0f
            )
            assertTrue(
                "$diagnostic invisible caret=$caret height=${input.height}",
                caret.bottom > 0f && caret.top < input.height && caret.right > 0f &&
                    caret.left < input.width
            )
        }
    }

    private fun probe(
        view: RichTextEditorView,
        adapter: EditorV2Adapter,
        corpus: JSONObject,
        context: String,
        expected: String,
        neighbour: String?,
        label: String,
        existingConnection: InputConnection? = null
    ) {
        val input = view.activeTextInput
        val connection =
            existingConnection ?: requireNotNull(input.onCreateInputConnection(EditorInfo()))
        val before = adapter.documentJson()
        val beforeText = descendants(scope(JSONObject(requireNotNull(before)), context))
            .filter { it.optString("type") == "text" }.joinToString("") { it.getString("text") }
        val selection = JSONObject(
            requireNotNull(adapter.cachedAtomicRenderJson)
        ).getJSONObject("selection")
        val documentSelection = selection.getInt("anchor") to selection.getInt("head")
        val caret = requireNotNull(input.caretRect())
        val text = corpus.getString("probe")
        assertTrue("$label commit rejected", connection.commitText(text, CURSOR_AFTER_COMMIT))
        check(view, adapter, context, expected, neighbour, "$label/type")
        val selected = requireNotNull(adapter.cachedAtomicRenderSelection())
        val expectedCaret = documentSelection.first + text.codePointCount(START_OFFSET, text.length)
        assertEquals(
            "$label typing left engine anchor behind",
            expectedCaret,
            selected.getInt("anchor")
        )
        assertEquals(
            "$label typing left engine head behind",
            expectedCaret,
            selected.getInt("head")
        )
        val visible = input.selectionEnd - text.length
        assertTrue("$label probe caret before inserted text", visible >= START_OFFSET)
        assertEquals(
            "$label probe missing immediately before the visible caret",
            text,
            input.text.subSequence(visible, input.selectionEnd).toString()
        )
        assertNotEquals("$label visual caret stuck", caret, input.caretRect())
        repeat(text.length) {
            assertTrue(
                "$label backspace rejected",
                connection.deleteSurroundingTextInCodePoints(ONE_CHARACTER, NO_CHARACTERS)
            )
        }
        shadowOf(Looper.getMainLooper()).idle()
        assertEquals(
            "$label typing/backspace changed structure or neighbours",
            before,
            adapter.documentJson()
        )
        val restored = JSONObject(
            requireNotNull(adapter.cachedAtomicRenderJson)
        ).getJSONObject("selection")
        assertEquals(
            "$label typing/backspace did not restore caret",
            documentSelection,
            restored.getInt("anchor") to restored.getInt("head")
        )
        check(view, adapter, context, beforeText, neighbour, "$label/probe backspace")
    }

    private fun checkFragment(input: EditorEditText, name: String, label: String) {
        val content = input.text
        val decoration = NODE_DECORATIONS[name]
        if (decoration != null) {
            val spans = content.getSpans(0, content.length, decoration)
            assertTrue("$label missing native $name decoration", spans.isNotEmpty())
            for (span in spans) {
                val offset = content.getSpanStart(span)
                val line = requireNotNull(input.layout).getLineForOffset(offset)
                assertTrue(
                    "$label collapsed native $name",
                    input.layout.getLineBottom(line) > input.layout.getLineTop(line)
                )
            }
        }
        if (name == HEADING_NODE) {
            val heading = RenderBridge.resolveTextStyle(HEADING_NODE, input.theme)
            val size = requireNotNull(heading.fontSize) * input.resources.displayMetrics.density
            assertTrue(
                "$label missing native heading size $size",
                content.getSpans(0, content.length, AbsoluteSizeSpan::class.java)
                    .any { it.size == size.toInt() }
            )
        }
    }

    private fun checkNativeRender(
        input: EditorEditText,
        adapter: EditorV2Adapter,
        documentScope: JSONObject,
        label: String
    ) {
        val diagnostic = "$label scope=$documentScope native=${input.text}"
        val nodes = descendants(documentScope)
        val nodeTypes = nodes.map { it.getString("type") }.toSet()
        for ((name, decoration) in NODE_DECORATIONS) {
            if (name in nodeTypes) {
                checkFragment(input, name, diagnostic)
            } else {
                assertTrue(
                    "$diagnostic stale native $name decoration",
                    input.text.getSpans(0, input.text.length, decoration).isEmpty()
                )
            }
        }
        val annotations = input.text.getSpans(0, input.text.length, Annotation::class.java)
        assertEquals(
            "$diagnostic native void annotations disagree with the current engine document",
            nodes.filter {
                it.getString("type") in INLINE_VOID_NODES
            }.groupingBy { it.getString("type") }.eachCount(),
            annotations.filter {
                it.key == RenderBridge.NATIVE_VOID_NODE_TYPE_ANNOTATION &&
                    it.value in INLINE_VOID_NODES
            }
                .groupingBy { it.value }.eachCount()
        )
        assertEquals(
            "$diagnostic stale task marker",
            "taskList" in nodeTypes,
            annotations.any { it.key == RenderBridge.NATIVE_TASK_LIST_MARKER_ANNOTATION }
        )
        assertEquals(
            "$diagnostic native list markers disagree with the current engine document",
            nodes.count { it.getString("type") == LIST_ITEM_NODE },
            annotations.count { it.key == RenderBridge.NATIVE_LIST_MARKER_ANNOTATION }
        )
        assertEquals(
            "$diagnostic native mention labels disagree with the current engine document",
            nodes.filter { it.getString("type") == MENTION_NODE }
                .map { it.getJSONObject("attrs").getString("label") },
            annotations.filter {
                it.key == RenderBridge.NATIVE_VOID_NODE_TYPE_ANNOTATION &&
                    it.value == MENTION_NODE
            }.sortedBy(input.text::getSpanStart).map {
                input.text.subSequence(input.text.getSpanStart(it), input.text.getSpanEnd(it))
                    .toString()
            }
        )
        val nonTextSpans: List<Any> = annotations.filter {
            input.isGeneratedStructureAnnotation(it) ||
                it.key == RenderBridge.NATIVE_VOID_NODE_TYPE_ANNOTATION
        } + input.text.getSpans(START_OFFSET, input.text.length, HorizontalRuleSpan::class.java) +
            input.text.getSpans(START_OFFSET, input.text.length, BlockImageSpan::class.java)
        val raw = input.text.toString()
        val visibleText = input.text.indices.filter { offset ->
            val placeholderDocStart = if (input.text[offset].toString() ==
                LayoutConstants.SYNTHETIC_PLACEHOLDER_CHARACTER
            ) {
                input.inputScalarAtLocalUtf16(offset, raw)?.let(adapter::docPositionForScalar)
            } else {
                null
            }
            val syntheticPlaceholder = placeholderDocStart != null && placeholderDocStart ==
                input.inputScalarAtLocalUtf16(offset + ONE_CHARACTER, raw)
                    ?.let(adapter::docPositionForScalar)
            !syntheticPlaceholder && nonTextSpans.none {
                offset >= input.text.getSpanStart(it) && offset < input.text.getSpanEnd(it)
            }
        }.joinToString("") { input.text[it].toString() }
        assertEquals(
            "$diagnostic native text disagrees with the current engine document",
            nodes.filter { it.getString("type") == TEXT_NODE }
                .joinToString("") { it.getString("text") },
            visibleText
        )
        if (HEADING_NODE in nodeTypes) {
            checkFragment(input, HEADING_NODE, diagnostic)
        } else {
            val headingSize = (HEADING_FONT_SIZE * input.resources.displayMetrics.density).toInt()
            assertTrue(
                "$diagnostic stale native heading styling",
                input.text.getSpans(0, input.text.length, AbsoluteSizeSpan::class.java)
                    .none { it.size == headingSize }
            )
        }
    }

    private fun clear(
        view: RichTextEditorView,
        adapter: EditorV2Adapter,
        context: String,
        mode: String,
        name: String,
        preservesFormatting: Boolean,
        label: String
    ) {
        val input = view.activeTextInput
        if (mode ==
            "selection"
        ) {
            input.setSelection(0, input.text.length)
        } else {
            input.setSelection(input.text.length)
        }
        val budget = if (mode == "selection") {
            SELECTED_DELETE_PRESSES
        } else {
            input.text.length +
                descendants(
                    scope(JSONObject(requireNotNull(adapter.documentJson())), context)
                ).size *
                BOUNDARY_BACKSPACE_ALLOWANCE
        }
        repeat(budget) {
            backspace(input, label)
            shadowOf(Looper.getMainLooper()).idle()
            val currentScope = scope(JSONObject(requireNotNull(adapter.documentJson())), context)
            checkNativeRender(view.activeTextInput, adapter, currentScope, "$label/delete $it")
            if (descendants(currentScope).none {
                    it.optString("type") == "text" || it.optString("type") == name
                }
            ) {
                return
            }
            if (mode == "selection" && preservesFormatting) {
                if (descendants(currentScope).none { it.optString("type") == "text" }) return
            }
        }
        throw AssertionError(
            "$label deletion exhausted $budget presses: ${adapter.documentJson()} " +
                "selection=${adapter.cachedAtomicRenderSelection()} input=${input.text} " +
                "native=${input.selectionStart}..${input.selectionEnd} " +
                "notes=${adapter.debugNotes} " +
                "trace=${input.imeTraceSnapshotForTesting()}"
        )
    }

    private fun withSequenceEditor(
        corpus: JSONObject,
        context: String,
        inline: Boolean,
        scopedBlocks: JSONArray? = null,
        block: (RichTextEditorView, EditorV2Adapter, String?) -> Unit
    ) = withMountedEditorView(
        initialDocument(corpus, context, inline, scopedBlocks),
        corpus.getJSONObject("config").toString(),
        windowed = true
    ) { view, adapter, _ ->
        view.editorEditText.applyTheme(
            EditorTheme(
                text = EditorTextStyle(fontSize = BODY_FONT_SIZE),
                headings = mapOf(HEADING_NODE to EditorTextStyle(fontSize = HEADING_FONT_SIZE))
            )
        )
        var neighbour: String? = null
        if (context != ROOT_CONTEXT) {
            tapMountedTableCell(view)
            neighbour = JSONObject(requireNotNull(adapter.documentJson())).getJSONArray("content")
                .getJSONObject(0).getJSONArray("content").getJSONObject(0).getJSONArray("content")
                .getJSONObject(1).toString()
        }
        assertTrue("focus rejected", view.activeTextInput.requestFocus())
        block(view, adapter, neighbour)
    }

    private fun runUnicode(context: String) {
        val corpus = corpus()
        val prefix = corpus.getString("prefix")
        val original = prefix + corpus.getString("suffix")
        for (route in listOf("backspaces", "codePointBackspaces")) {
            for (offset in listOf(0, prefix.length, original.length)) {
                for (sample in corpus.getJSONArray("unicode").objects()) {
                    val label = "$context/unicode ${sample.getString("name")}/offset $offset/$route"
                    withSequenceEditor(corpus, context, true) { view, adapter, neighbour ->
                        val input = view.activeTextInput
                        val left = original.substring(0, offset)
                        val right = original.substring(offset)
                        input.setSelection(offset)
                        val text = sample.getString("text")
                        val connection = requireNotNull(input.onCreateInputConnection(EditorInfo()))
                        assertTrue(
                            label,
                            connection.commitText(text, CURSOR_AFTER_COMMIT)
                        )
                        check(
                            view,
                            adapter,
                            context,
                            left + text + right,
                            neighbour,
                            "$label/insert"
                        )
                        assertEquals(
                            "$label insertion caret",
                            offset + text.length,
                            input.selectionEnd
                        )
                        val expected = sample.getJSONArray(route)
                        for (press in 0 until expected.length()) {
                            if (route == "backspaces") {
                                backspace(input, label, connection)
                            } else {
                                assertTrue(
                                    label,
                                    connection.deleteSurroundingTextInCodePoints(
                                        ONE_CHARACTER,
                                        NO_CHARACTERS
                                    )
                                )
                            }
                            val remaining = expected.getString(press)
                            val stage = "$label/backspace $press"
                            check(
                                view,
                                adapter,
                                context,
                                left + remaining + right,
                                neighbour,
                                stage
                            )
                            assertEquals(
                                "$stage Unicode rendering stale",
                                left + remaining + right,
                                input.text.toString()
                            )
                            assertEquals(
                                "$stage wrong caret",
                                offset + remaining.length,
                                input.selectionEnd
                            )
                        }
                        val recovery = corpus.getString("recovery")
                        assertTrue(
                            label,
                            connection.commitText(recovery, CURSOR_AFTER_COMMIT)
                        )
                        check(
                            view,
                            adapter,
                            context,
                            left + recovery + right,
                            neighbour,
                            "$label/recover"
                        )
                    }
                }
            }
        }
    }

    private fun runPairs(
        context: String,
        mode: String,
        firstName: String? = null,
        secondName: String? = null
    ) {
        val corpus = corpus()
        val fragments = corpus.getJSONArray("fragments").objects()
        val prefix = corpus.getString("prefix")
        val suffix = corpus.getString("suffix")
        for (first in fragments.filter { firstName == null || it.getString("name") == firstName }) {
            for (second in fragments.filter {
                secondName == null ||
                    it.getString("name") == secondName
            }) {
                val label = "$context/$mode/${first.getString(
                    "name"
                )} -> ${second.getString("name") }"
                withSequenceEditor(corpus, context, first.optBoolean("inline")) {
                        view,
                        adapter,
                        neighbour
                    ->
                    val input = view.activeTextInput
                    input.setSelection(prefix.length)
                    for ((phase, fragment) in listOf(first, second).withIndex()) {
                        val stage = "$label/phase $phase"
                        val local = requireNotNull(input.currentScalarSelection())
                        val selection =
                            requireNotNull(input.inputScalarSelection(local.first, local.second))
                        val json = JSONObject().put(
                            "type",
                            "doc"
                        ).put("content", JSONArray().put(fragment.getJSONObject("node")))
                        val update =
                            requireNotNull(
                                adapter.insertContentJsonAtSelection(
                                    json.toString(),
                                    selection.first,
                                    selection.second
                                )
                            ) {
                                "$stage insert failed: ${adapter.debugNotes}"
                            }
                        assertTrue(stage, input.applyUpdateJSON(update))
                        val middle = fragment.getString("text")
                        val left = if (phase == 0) prefix else ""
                        val right = if (phase == 0) suffix else ""
                        check(view, adapter, context, left + middle + right, neighbour, stage)
                        assertTrue(
                            "$stage inserted node missing",
                            descendants(
                                scope(JSONObject(requireNotNull(adapter.documentJson())), context)
                            ).any {
                                it.optString("type") == fragment.getString("name")
                            }
                        )
                        probe(
                            view,
                            adapter,
                            corpus,
                            context,
                            left + middle + corpus.getString("probe") + right,
                            neighbour,
                            stage
                        )
                        clear(
                            view,
                            adapter,
                            context,
                            mode,
                            fragment.getString("name"),
                            fragment.optBoolean("preservesFormatting"),
                            stage
                        )
                        check(view, adapter, context, "", neighbour, "$stage/clear")
                    }
                    val recovery = corpus.getString("recovery")
                    assertTrue(
                        "$label recovery rejected",
                        requireNotNull(
                            input.onCreateInputConnection(EditorInfo())
                        ).commitText(recovery, CURSOR_AFTER_COMMIT)
                    )
                    check(view, adapter, context, recovery, neighbour, "$label/recover")
                }
            }
        }
    }

    private companion object {
        const val ROOT_CONTEXT = "root"
        const val CORPUS_ASSET = "editor-command-sequences.json"
        const val EDITOR_WIDTH = 600
        const val EDITOR_HEIGHT = 500
        const val ONE_CHARACTER = 1
        const val NO_CHARACTERS = 0
        const val CURSOR_AFTER_COMMIT = 1
        const val BOUNDARY_BACKSPACE_ALLOWANCE = 2
        const val SELECTED_DELETE_PRESSES = 1
        const val KEY_EVENT_INTERVAL_MS = 1L
        const val HEADING_NODE = "h2"
        const val LIST_ITEM_NODE = "listItem"
        const val MENTION_NODE = "mention"
        const val TEXT_NODE = "text"
        const val BODY_FONT_SIZE = 17f
        const val HEADING_FONT_SIZE = 28f
        const val START_OFFSET = 0
        const val ROOT_OPENING = 0
        const val FIRST_CELL = 0
        const val FIRST_PARAGRAPH = 0
        const val SECOND_PARAGRAPH = 1
        const val ONE_PARAGRAPH = 1
        const val SPLIT_PARAGRAPH_COUNT = 2
        const val ONE_TEXT_NODE = 1
        const val NODE_OPENING_SIZE = 1
        const val NODE_BOUNDARY_SIZE = 2
        const val PARTIAL_SELECTION_EDGE_LENGTH = 2
        const val PARAGRAPH_SEPARATOR = "\n"
        const val HARD_BREAK_NODE = "hardBreak"
        const val MULTILINE_REPLACEMENT = "one\ntwo\nthree"
        const val LAST_MULTILINE_PARAGRAPH = 2
        const val CROSS_PARAGRAPH_PREFIX = "abc"
        const val CROSS_PARAGRAPH_MIDDLE = "def"
        const val CROSS_PARAGRAPH_SUFFIX = "ghi"
        const val CROSS_PARAGRAPH_REPLACEMENT = "X"
        const val BOLD_MARK = "bold"
        const val ITALIC_MARK = "italic"
        val CONTEXTS = listOf(ROOT_CONTEXT, "table_cell", "table_header")
        val INLINE_VOID_NODES = setOf("hardBreak", "mention")
        val NODE_DECORATIONS = mapOf(
            "horizontalRule" to HorizontalRuleSpan::class.java,
            "image" to BlockImageSpan::class.java,
            "bulletList" to CenteredBulletSpan::class.java,
            "orderedList" to OrderedListMarkerSpan::class.java,
            "blockquote" to BlockquoteSpan::class.java,
            "codeBlock" to CodeBlockSpan::class.java
        )
        fun JSONArray?.objects(): List<JSONObject> = if (this ==
            null
        ) {
            emptyList()
        } else {
            (0 until length()).map(::getJSONObject)
        }
    }
}
