package com.apollohg.editor

import android.graphics.Color
import android.graphics.Typeface
import android.text.Annotation
import android.text.NoCopySpan
import android.text.SpannableStringBuilder
import android.text.Spanned
import android.text.SpannedString
import android.text.style.StyleSpan
import android.view.inputmethod.BaseInputConnection
import android.view.inputmethod.EditorInfo
import android.view.inputmethod.InputConnection
import org.json.JSONArray
import org.json.JSONObject
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotSame
import org.junit.Assert.assertNull
import org.junit.Assert.assertSame
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.RuntimeEnvironment
import org.robolectric.annotation.Config

@RunWith(RobolectricTestRunner::class)
@Config(sdk = [24, 34])
internal class EditorRenderedTextSnapshotTest : RichTextEditorViewTestFixture() {
    private data class Metadata(
        val key: String,
        val value: String,
        val start: Int,
        val end: Int,
        val flags: Int
    )

    @Test
    fun `nested snapshot queries preserve immutable Android ordering at every boundary`() {
        val text = "first😀\nsecond\nlast"
        val spanCounts = listOf(0, 1, 2, 3, 7, 8, 15, 16, 31, 32, 63)
        val queryTypes = listOf(Any::class.java, Annotation::class.java, StyleSpan::class.java)
        for (spanCount in spanCounts) {
            val source = SpannableStringBuilder(text)
            for (ordinal in (0 until spanCount).reversed()) {
                val start = ordinal % (text.length + 1)
                val end = if (ordinal % 3 == 0) start else text.length
                val span = if (ordinal % 2 == 0) {
                    Annotation("range", ordinal.toString())
                } else {
                    StyleSpan(Typeface.BOLD)
                }
                val priority = (ordinal % 3) shl Spanned.SPAN_PRIORITY_SHIFT
                source.setSpan(span, start, end, Spanned.SPAN_MARK_MARK or priority)
            }
            val expected = SpannedString(source)
            val actual = immutableRenderedText(source, excludeNoCopySpans = true)
            source.clearSpans()
            source.replace(0, source.length, "changed")
            for (start in 0..text.length) {
                for (end in start..text.length) {
                    for (type in queryTypes) {
                        val context = "spans=$spanCount query=$start..$end type=${type.simpleName}"
                        assertEquals(
                            context,
                            expected.getSpans(start, end, type).toList(),
                            actual.getSpans(start, end, type).toList()
                        )
                        assertEquals(
                            "$context transition",
                            expected.nextSpanTransition(start, end, type),
                            actual.nextSpanTransition(start, end, type)
                        )
                    }
                    assertEquals(
                        "spans=$spanCount query=$start..$end untyped transition",
                        expected.nextSpanTransition(start, end, null),
                        actual.nextSpanTransition(start, end, null)
                    )
                }
            }
        }
    }

    @Test
    fun `later fragment ranges and priorities retain shared identity insertion order`() {
        val first = SpannableStringBuilder("first")
        val second = SpannableStringBuilder("second")
        val shared = Annotation("shared", "moved")
        val firstPeer = Annotation("first", "priority")
        val lastPeer = Annotation("last", "equal")
        val ordinaryFlags = Spanned.SPAN_EXCLUSIVE_EXCLUSIVE
        val priorityFlags = ordinaryFlags or Spanned.SPAN_PRIORITY
        first.setSpan(shared, 0, 2, priorityFlags)
        first.setSpan(firstPeer, 0, first.length, priorityFlags)
        second.setSpan(shared, 1, 4, ordinaryFlags)
        second.setSpan(lastPeer, 0, 3, ordinaryFlags)
        val expected = SpannableStringBuilder(first.toString() + second.toString())
        expected.setSpan(shared, first.length + 1, first.length + 4, ordinaryFlags)
        expected.setSpan(firstPeer, 0, first.length, priorityFlags)
        expected.setSpan(lastPeer, first.length, first.length + 3, ordinaryFlags)
        val actual = RenderedTextFragments(listOf(first, second))

        assertEquals(
            "shared span takes its final fragment range",
            first.length + 1,
            actual.getSpanStart(shared)
        )
        assertEquals(
            "shared span takes its final priority",
            ordinaryFlags,
            actual.getSpanFlags(shared)
        )
        assertEquals(
            "final priority precedes equal-priority insertion order",
            listOf(firstPeer, shared, lastPeer),
            actual.getSpans(0, actual.length, Annotation::class.java).toList()
        )
        for (start in 0..actual.length) {
            for (end in start..actual.length) {
                assertEquals(
                    "fragment query=$start..$end",
                    expected.getSpans(start, end, Any::class.java).toList(),
                    actual.getSpans(start, end, Any::class.java).toList()
                )
            }
        }
    }

    @Test
    fun `immutable snapshot range queries match Android across Unicode and span boundaries`() {
        val source = SpannableStringBuilder("A😀BC\nD")
        source.setSpan(
            StyleSpan(Typeface.ITALIC),
            3,
            source.length,
            Spanned.SPAN_EXCLUSIVE_EXCLUSIVE
        )
        source.setSpan(StyleSpan(Typeface.BOLD), 0, 4, Spanned.SPAN_EXCLUSIVE_EXCLUSIVE)
        source.setSpan(
            Annotation("outer", "paragraph"),
            0,
            source.length,
            Spanned.SPAN_INCLUSIVE_INCLUSIVE
        )
        source.setSpan(
            Annotation("priority", "first"),
            1,
            5,
            Spanned.SPAN_INCLUSIVE_INCLUSIVE or Spanned.SPAN_PRIORITY
        )
        source.setSpan(Annotation("point", "boundary"), 3, 3, Spanned.SPAN_MARK_MARK)
        source.setSpan(NoCopySpan.Concrete(), 0, source.length, Spanned.SPAN_INCLUSIVE_INCLUSIVE)
        val expected = SpannableStringBuilder(source)
        val actual = immutableRenderedText(source, excludeNoCopySpans = true)
        source.clearSpans()
        source.replace(0, source.length, "changed")

        assertEquals("detached Unicode characters", expected.toString(), actual.toString())
        for (start in 0..actual.length) {
            for (end in start..actual.length) {
                val context = "query=$start..$end"
                val expectedSpans = expected.getSpans(start, end, Any::class.java)
                assertEquals(
                    context,
                    expectedSpans.toList(),
                    actual.getSpans(start, end, Any::class.java).toList()
                )
                assertEquals(
                    "$context style ordering",
                    expected.getSpans(start, end, StyleSpan::class.java).toList(),
                    actual.getSpans(start, end, StyleSpan::class.java).toList()
                )
                for (span in expectedSpans) {
                    assertEquals(
                        "$context start",
                        expected.getSpanStart(span),
                        actual.getSpanStart(span)
                    )
                    assertEquals("$context end", expected.getSpanEnd(span), actual.getSpanEnd(span))
                    assertEquals(
                        "$context flags",
                        expected.getSpanFlags(span),
                        actual.getSpanFlags(span)
                    )
                }
                assertEquals(
                    "$context transition",
                    expected.nextSpanTransition(start, end, Any::class.java),
                    actual.nextSpanTransition(start, end, Any::class.java)
                )
                assertEquals(
                    "$context style transition",
                    expected.nextSpanTransition(start, end, StyleSpan::class.java),
                    actual.nextSpanTransition(start, end, StyleSpan::class.java)
                )
                val expectedSlice = expected.subSequence(start, end) as Spanned
                val actualSlice = actual.subSequence(start, end) as Spanned
                assertEquals("$context slice", expectedSlice.toString(), actualSlice.toString())
                assertEquals(
                    "$context slice spans",
                    expectedSlice.getSpans(0, expectedSlice.length, Any::class.java).toList(),
                    actualSlice.getSpans(0, actualSlice.length, Any::class.java).toList()
                )
            }
        }
    }

    @Test
    fun `read only copies preserve rendering order and exclude native ownership for all sources`() {
        val source = SpannableStringBuilder(MARKED_TEXT)
        val later = StyleSpan(Typeface.ITALIC)
        val earlier = StyleSpan(Typeface.BOLD)
        val priority = Annotation("priority", "first")
        val empty = Annotation("empty", "boundary")
        val watcher = NoCopySpan.Concrete()
        source.setSpan(later, 1, source.length, Spanned.SPAN_EXCLUSIVE_EXCLUSIVE)
        source.setSpan(earlier, 0, source.length, Spanned.SPAN_EXCLUSIVE_EXCLUSIVE)
        source.setSpan(
            priority,
            0,
            source.length,
            Spanned.SPAN_INCLUSIVE_INCLUSIVE or Spanned.SPAN_PRIORITY
        )
        source.setSpan(empty, 1, 1, Spanned.SPAN_MARK_MARK)
        source.setSpan(watcher, 0, source.length, Spanned.SPAN_INCLUSIVE_INCLUSIVE)
        val expected = source.getSpans(0, source.length, Any::class.java).filterNot {
            it is NoCopySpan
        }
        val sources = listOf(source, SpannedString(source), RenderedTextFragments(listOf(source)))
        val snapshots = sources.map { immutableRenderedText(it, excludeNoCopySpans = true) }
        for ((index, snapshot) in snapshots.withIndex()) {
            assertEquals(
                "source=$index ordered spans",
                expected,
                snapshot.getSpans(
                    0,
                    snapshot.length,
                    Any::class.java
                ).toList()
            )
            for (span in expected) {
                assertEquals(
                    "source=$index start",
                    source.getSpanStart(span),
                    snapshot.getSpanStart(span)
                )
                assertEquals(
                    "source=$index end",
                    source.getSpanEnd(span),
                    snapshot.getSpanEnd(span)
                )
                assertEquals(
                    "source=$index flags",
                    source.getSpanFlags(span),
                    snapshot.getSpanFlags(span)
                )
            }
            assertEquals("source=$index excludes watcher", -1, snapshot.getSpanStart(watcher))
        }
        assertTrue(
            "replacement copies retain NoCopySpan",
            immutableRenderedText(source).getSpanStart(watcher) >= 0
        )
        source.clearSpans()
        source.clear()
        for (snapshot in snapshots) {
            assertEquals("snapshot keeps original characters", MARKED_TEXT, snapshot.toString())
            assertEquals("snapshot keeps original range", 1, snapshot.getSpanStart(later))
        }
    }

    @Test
    fun `accepted same character mark patch excludes native composing styles from rollback`() {
        val editor = EditorEditText(RuntimeEnvironment.getApplication()).apply {
            editorId = VIEW_EDITOR_TOKEN
        }
        val initial = JSONArray().put(paragraphRenderBlock(MARKED_TEXT))
        editor.applyUpdateJSON(renderUpdateJson(initial), notifyListener = false)
        editor.setSelection(MARKED_TEXT.length)
        val input = editor.onCreateInputConnection(EditorInfo())!!
        assertTrue(input.setComposingRegion(0, MARKED_TEXT.length))
        val composingStyle = StyleSpan(Typeface.ITALIC)
        editor.text.setSpan(
            composingStyle,
            0,
            MARKED_TEXT.length,
            Spanned.SPAN_EXCLUSIVE_EXCLUSIVE
        )
        BaseInputConnection.setComposingSpans(editor.text)
        assertTrue(editor.hasCompositionTrackingForEditor())
        assertTrue(editor.text.getSpanFlags(composingStyle) and Spanned.SPAN_COMPOSING != 0)

        val accepted = paragraphRenderBlock(MARKED_TEXT).apply {
            getJSONObject(1).put("marks", JSONArray().put(BOLD_MARK))
        }
        applyBlocksPatch(editor, initial.length(), JSONArray().put(accepted))

        assertTrue(
            "accepted mark update must exercise the patch path",
            editor.lastRenderAppliedPatch()
        )
        assertEquals(MARKED_TEXT, editor.text.toString())
        assertStyles(editor.text, Typeface.BOLD)
        assertEquals(-1, editor.text.getSpanStart(composingStyle))
        val authorized = editor.captureAuthoritativeInputSnapshotForEditor()
        assertStyles(authorized.renderedText as Spanned, Typeface.BOLD)
        assertEquals(
            "authorized rollback must not retain the retired native composing style",
            -1,
            (authorized.renderedText as Spanned).getSpanStart(composingStyle)
        )
        editor.runWithTransientInputMutationGuard {
            editor.text.insert(0, "rejected ")
            true
        }
        editor.restoreAuthoritativeInputForEditor(authorized)
        assertEquals(MARKED_TEXT, editor.text.toString())
        assertStyles(editor.text, Typeface.BOLD)
        input.closeConnection()
    }

    @Test
    fun `cached styled IME readback refreshes after a same character mark patch`() {
        val editor = EditorEditText(RuntimeEnvironment.getApplication())
        val initial = paragraphRenderBlock(MARKED_TEXT).apply {
            getJSONObject(1).put("marks", JSONArray().put(BOLD_MARK))
        }
        editor.applyUpdateJSON(renderUpdateJson(JSONArray().put(initial)), notifyListener = false)
        editor.setSelection(MARKED_TEXT.length)
        val input = editor.onCreateInputConnection(EditorInfo())!!
        val captured = input.getTextBeforeCursor(
            MARKED_TEXT.length,
            InputConnection.GET_TEXT_WITH_STYLES
        ) as Spanned
        assertStyles(captured, Typeface.BOLD)
        (input as EditorInputConnection).publishInputStateIfNeeded()
        val accepted = paragraphRenderBlock(MARKED_TEXT).apply {
            getJSONObject(1).put("marks", JSONArray().put(ITALIC_MARK))
        }

        applyBlocksPatch(editor, 1, JSONArray().put(accepted))

        assertTrue(editor.lastRenderAppliedPatch())
        assertStyles(editor.text, Typeface.ITALIC)
        assertNull(
            "unchanged IME selection must not eagerly rebuild the rich readback snapshot",
            editor.cachedImeTextCoordinateMapper
        )
        val updated = input.getTextBeforeCursor(
            MARKED_TEXT.length,
            InputConnection.GET_TEXT_WITH_STYLES
        ) as Spanned
        assertEquals(MARKED_TEXT, updated.toString())
        assertStyles(updated, Typeface.ITALIC)
        assertStyles(captured, Typeface.BOLD)
        input.closeConnection()
    }

    @Test
    fun `cached styled IME readback refreshes a positional only atom patch`() {
        val editor = editorWithAtoms()
        val initial = blocks(listOf(INITIAL_PREFIX), positionShift = 0)
        editor.applyUpdateJSON(renderUpdateJson(initial), notifyListener = false)
        editor.setSelection(editor.text.length)
        val input = editor.onCreateInputConnection(EditorInfo())!!
        val captured = input.getTextBeforeCursor(
            editor.text.length,
            InputConnection.GET_TEXT_WITH_STYLES
        ) as Spanned
        (input as EditorInputConnection).publishInputStateIfNeeded()
        val originalMetadata = metadata(captured)
        val originalText = editor.text.toString()
        val originalRenders = editor.inputRerendersForTesting

        applyBlocksPatch(
            editor,
            initial.length(),
            blocks(listOf(INITIAL_PREFIX), positionShift = POSITION_ONLY_SHIFT)
        )

        assertEquals(
            "metadata-only updates must not render text",
            originalRenders,
            editor.inputRerendersForTesting
        )
        assertEquals(originalText, editor.text.toString())
        assertNull(
            "positional annotation changes must leave styled readback lazy " +
                "when IME text and selection stay unchanged",
            editor.cachedImeTextCoordinateMapper
        )
        assertEquals(
            "live atom metadata must adopt the accepted position before IME readback",
            INITIAL_ATOM_POSITION + POSITION_ONLY_SHIFT,
            editor.text.getSpans(0, editor.text.length, AtomBlockSpan::class.java).single().docPos
        )
        val updated = input.getTextBeforeCursor(
            editor.text.length,
            InputConnection.GET_TEXT_WITH_STYLES
        ) as Spanned
        assertEquals(originalText, updated.toString())
        assertEquals(metadata(editor.text), metadata(updated))
        assertEquals(
            INITIAL_ATOM_POSITION + POSITION_ONLY_SHIFT,
            updated.getSpans(0, updated.length, AtomBlockSpan::class.java).single().docPos
        )
        assertEquals(originalMetadata, metadata(captured))
        assertEquals(
            INITIAL_ATOM_POSITION,
            captured.getSpans(0, captured.length, AtomBlockSpan::class.java).single().docPos
        )
        input.closeConnection()
    }

    private fun assertStyles(content: Spanned, expectedStyle: Int) {
        val styles = content.getSpans(0, content.length, StyleSpan::class.java)
        assertEquals(
            "unexpected styles in '$content': ${styles.map {
                it.style
            }}",
            listOf(expectedStyle),
            styles.map { it.style }
        )
        assertEquals(0, content.getSpanStart(styles.single()))
        assertEquals(content.length, content.getSpanEnd(styles.single()))
    }

    @Test
    fun `held authoritative snapshot keeps its revision before a structural suffix update`() {
        val editor = editorWithAtoms()
        val initial = blocks(listOf(INITIAL_PREFIX), positionShift = 0)
        editor.applyUpdateJSON(renderUpdateJson(initial), notifyListener = false)
        val snapshot = editor.captureAuthoritativeInputSnapshotForEditor().renderedText as Spanned
        val originalText = snapshot.toString()
        val originalMetadata = metadata(snapshot)
        val originalAtom = snapshot.getSpans(0, snapshot.length, AtomBlockSpan::class.java).single()

        applyBlocksPatch(
            editor,
            initial.length(),
            blocks(
                listOf(INSERTED_PREFIX, INITIAL_PREFIX),
                INSERTED_PREFIX.length + BLOCK_BOUNDARIES
            )
        )

        assertTrue("structural edit must retain the atom suffix", editor.lastRenderAppliedPatch())
        assertEquals(originalText, snapshot.toString())
        assertEquals(
            "held metadata must still describe its original text and offsets",
            originalMetadata,
            metadata(snapshot)
        )
        assertEquals(
            "held atom position must still address the captured revision",
            INITIAL_ATOM_POSITION,
            originalAtom.docPos
        )
        assertNotSame(
            originalAtom,
            editor.text.getSpans(0, editor.text.length, AtomBlockSpan::class.java).single()
        )
    }

    @Test
    fun `restoring a held snapshot detaches live atoms before another committed update`() {
        val editor = editorWithAtoms()
        val initial = blocks(listOf(INITIAL_PREFIX), positionShift = 0)
        editor.applyUpdateJSON(renderUpdateJson(initial), notifyListener = false)
        val saved = editor.captureAuthoritativeInputSnapshotForEditor()
        val snapshot = saved.renderedText as Spanned
        val originalMetadata = metadata(snapshot)
        editor.runWithTransientInputMutationGuard {
            editor.text.insert(0, "optimistic ")
            true
        }
        editor.restoreAuthoritativeInputForEditor(saved)
        assertEquals(INITIAL_PREFIX, editor.text.toString().substringBefore('\n'))

        applyBlocksPatch(
            editor,
            initial.length(),
            blocks(listOf("$INITIAL_PREFIX!"), positionShift = 1)
        )

        assertTrue(editor.lastRenderAppliedPatch())
        assertEquals(
            "a restored snapshot must remain reusable after a later frame",
            originalMetadata,
            metadata(snapshot)
        )
        assertEquals(
            INITIAL_ATOM_POSITION,
            snapshot.getSpans(0, snapshot.length, AtomBlockSpan::class.java).single().docPos
        )
        assertEquals(
            INITIAL_ATOM_POSITION + 1,
            editor.text.getSpans(0, editor.text.length, AtomBlockSpan::class.java).single().docPos
        )
    }

    @Test
    fun `snapshot preserves rich styles and image ownership while excluding native watchers`() {
        val editor = editorWithAtoms()
        val initial = blocks(listOf(INITIAL_PREFIX), positionShift = 0)
        val rendered = RenderBridge.buildSpannableFromBlocks(
            initial,
            baseFontSize = FONT_SIZE,
            textColor = Color.BLACK,
            atomConfiguration = editor.atomRenderConfiguration
        )
        val image =
            BlockImageSpan("prepared-image", null, 1f, null, null, preparedSizePx = IMAGE_SIZE)
        val watcher = NoCopySpan.Concrete()
        rendered.setSpan(image, 0, 1, Spanned.SPAN_EXCLUSIVE_EXCLUSIVE)
        rendered.setSpan(watcher, 0, rendered.length, Spanned.SPAN_INCLUSIVE_INCLUSIVE)
        editor.lastAuthorizedRenderedText = rendered
        try {
            val snapshot =
                editor.captureAuthoritativeInputSnapshotForEditor().renderedText as Spanned
            assertEquals(rendered.toString(), snapshot.toString())
            assertEquals(metadata(rendered), metadata(snapshot))
            assertSame(
                "image leases must keep their original owner",
                image,
                snapshot.getSpans(0, snapshot.length, BlockImageSpan::class.java).single()
            )
            assertSame(
                rendered.getSpans(0, rendered.length, StyleSpan::class.java).single(),
                snapshot.getSpans(0, snapshot.length, StyleSpan::class.java).single()
            )
            assertEquals(-1, snapshot.getSpanStart(watcher))
            assertFalse(
                snapshot.getSpans(0, snapshot.length, Annotation::class.java).any {
                    it is RenderMetadataAnnotation
                }
            )
        } finally {
            image.close()
        }
    }

    @Test
    fun `IME styled readback remains frozen after live metadata and atom updates`() {
        val editor = editorWithAtoms()
        val initial = blocks(listOf(INITIAL_PREFIX), positionShift = 0)
        editor.applyUpdateJSON(renderUpdateJson(initial), notifyListener = false)
        val input = editor.onCreateInputConnection(EditorInfo())!!
        editor.setSelection(editor.text.length)
        val readback = input.getTextBeforeCursor(
            editor.text.length,
            InputConnection.GET_TEXT_WITH_STYLES
        ) as Spanned
        val originalText = readback.toString()
        val originalMetadata = metadata(readback)

        applyBlocksPatch(
            editor,
            initial.length(),
            blocks(listOf("$INITIAL_PREFIX!"), positionShift = 1)
        )

        assertEquals(originalText, readback.toString())
        assertEquals(
            "held IME readback must not inherit a later document position",
            originalMetadata,
            metadata(readback)
        )
        assertEquals(
            INITIAL_ATOM_POSITION,
            readback.getSpans(0, readback.length, AtomBlockSpan::class.java).single().docPos
        )
        input.closeConnection()
    }

    @Test
    fun `external composition capture stays frozen after cancel and a real paragraph split`() {
        val document = JSONObject().put(
            "type",
            "doc"
        ).put(
            "content",
            JSONArray().put(documentParagraph(INITIAL_PREFIX)).put(documentParagraph(RETAINED_TEXT))
        )
        val config = JSONObject().put(
            "initialization",
            JSONObject().put("type", "localJson").put("json", document)
        )
        val created = UniffiEditorV2Backend.create(config.toString(), null) as EditorV2CallResult.Ok
        val editorId = JSONObject(created.value).getString("editorId")
        val adapter = EditorV2Adapter.attach(UniffiEditorV2Backend, editorId, roomBound = false)!!
        val editor = EditorEditText(RuntimeEnvironment.getApplication()).apply {
            this.editorId = VIEW_EDITOR_TOKEN
            v2Driver = adapter
            applyUpdateJSON(requireNotNull(adapter.recoverNativeRender()), notifyListener = false)
            setSelection(COMPOSITION_OFFSET)
        }
        try {
            assertEquals(
                "active",
                JSONObject(editor.beginExternalTextComposition(COMPOSITION_ID)).getString("type")
            )
            val capture = requireNotNull(
                editor.externalTextComposition?.startingAuthorizedRenderedText
            ) as Spanned
            val capturedText = capture.toString()
            val capturedMetadata = metadata(capture)
            assertEquals(
                "active",
                JSONObject(
                    editor.updateExternalTextComposition(COMPOSITION_ID, "draft")
                ).getString("type")
            )
            assertEquals(
                "cancelled",
                JSONObject(
                    editor.cancelExternalTextComposition(COMPOSITION_ID, "consumer")
                ).getString("outcome")
            )
            assertEquals(capturedText, editor.text.toString())
            val revision = adapter.baseDocumentRevision

            editor.setSelection(COMPOSITION_OFFSET)
            editor.handleTextCommit("\n")

            assertTrue(
                "split must commit in the real Rust session",
                adapter.baseDocumentRevision > revision
            )
            assertEquals(capturedText, capture.toString())
            assertEquals(
                "ended composition capture must retain its original sibling indexes",
                capturedMetadata,
                metadata(capture)
            )
            assertEquals(
                setOf("0", "1", "2"),
                metadata(editor.text).filter {
                    it.key ==
                        RenderBridge.NATIVE_TOP_LEVEL_CHILD_INDEX_ANNOTATION
                }.map { it.value }.toSet()
            )
        } finally {
            editor.v2Driver = null
            adapter.destroy()
        }
    }

    private fun editorWithAtoms(): EditorEditText =
        EditorEditText(RuntimeEnvironment.getApplication()).apply {
            applyAtomRenderConfiguration(
                AtomRenderConfiguration(
                    setOf(ATOM_TYPE),
                    mapOf(ATOM_TYPE to ATOM_HEIGHT),
                    emptyMap()
                )
            )
        }

    private fun blocks(prefix: List<String>, positionShift: Int): JSONArray {
        val rich = paragraphRenderBlock(RETAINED_TEXT)
        rich.getJSONObject(1).put("marks", JSONArray().put("bold"))
        rich.put(
            rich.length() - 1,
            JSONObject().put(
                "type",
                "opaqueInlineAtom"
            ).put("nodeType", "mention").put("label", "@person").put(
                "docPos",
                INITIAL_MENTION_POSITION + positionShift
            )
        )
        rich.put(JSONObject().put("type", "blockEnd"))
        return JSONArray().apply {
            prefix.forEach { put(paragraphRenderBlock(it)) }
            put(rich)
            put(
                JSONArray().put(
                    JSONObject().put(
                        "type",
                        "voidBlock"
                    ).put("nodeType", ATOM_TYPE).put("atomId", "saved-counter").put(
                        "docPos",
                        INITIAL_ATOM_POSITION + positionShift
                    )
                )
            )
            put(paragraphRenderBlock("Tail"))
        }
    }

    private fun applyBlocksPatch(editor: EditorEditText, previousCount: Int, updated: JSONArray) {
        val patch = JSONObject().put(
            "startIndex",
            0
        ).put("deleteCount", previousCount).put("renderBlocks", updated)
        editor.applyUpdateJSON(
            renderUpdateJson(JSONArray(), includeFullRenderBlocks = false, renderPatch = patch),
            notifyListener = false
        )
    }

    private fun metadata(content: Spanned): List<Metadata> =
        content.getSpans(0, content.length, Annotation::class.java).map {
            Metadata(
                it.key,
                it.value,
                content.getSpanStart(it),
                content.getSpanEnd(it),
                content.getSpanFlags(it)
            )
        }.sortedWith(compareBy({ it.start }, { it.end }, { it.key }))

    private fun documentParagraph(text: String): JSONObject = JSONObject().put(
        "type",
        "paragraph"
    ).put("content", JSONArray().put(JSONObject().put("type", "text").put("text", text)))

    private companion object {
        const val INITIAL_PREFIX = "Before"
        const val INSERTED_PREFIX = "Front"
        const val RETAINED_TEXT = "Retained 😀"
        const val INITIAL_MENTION_POSITION = 24
        const val INITIAL_ATOM_POSITION = 25
        const val BLOCK_BOUNDARIES = 2
        const val FONT_SIZE = 16f
        const val ATOM_TYPE = "counterCard"
        const val ATOM_HEIGHT = 120f
        const val VIEW_EDITOR_TOKEN = 1L
        const val COMPOSITION_OFFSET = 2
        const val COMPOSITION_ID = "snapshot-composition"
        const val MARKED_TEXT = "Alpha"
        const val BOLD_MARK = "bold"
        const val ITALIC_MARK = "italic"
        const val POSITION_ONLY_SHIFT = 7
        val IMAGE_SIZE = 32 to 24
    }
}
