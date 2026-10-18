package com.apollohg.editor

import android.graphics.Typeface
import android.text.NoCopySpan
import android.text.Selection
import android.text.SpannableString
import android.text.SpannableStringBuilder
import android.text.Spanned
import android.text.style.AbsoluteSizeSpan
import android.text.style.StyleSpan
import android.view.inputmethod.BaseInputConnection
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config

@RunWith(RobolectricTestRunner::class)
@Config(sdk = [24, 32, 34])
class ImeTextCoordinateMapperTest {
    @Test
    fun `selection publication reads no text beyond the last requested raw offset`() {
        val prefix = "A\u200B😀éZ"
        val rawText = prefix + RICH_TEXT.repeat(SELECTION_TAIL_REPETITIONS)
        val raw = ReadCountingText(rawText)
        val markers = listOf(2..3, 1..2, raw.length - 2 until raw.length)
        val selection = listOf(-1, prefix.length, 0, 3, prefix.length - 1, 3)
        val expected = selection.map { offset ->
            if (offset < 0) -1 else visiblePrefixLength(rawText, markers, offset)
        }

        assertEquals(expected, ImeSelectionProjection(raw, markers).project(selection))
        assertTrue(
            "early-caret publication read ${raw.readCount} characters for ${raw.length} raw UTF16 units",
            raw.readCount <= prefix.length
        )
        assertTrue(
            "selection projection reached the unrelated document suffix at ${raw.furthestReadOffset}",
            raw.furthestReadOffset < prefix.length
        )
    }

    @Test
    fun `caret moves over unchanged content read each character once`() {
        val rawText = RICH_TEXT.repeat(SELECTION_TAIL_REPETITIONS)
        val raw = ReadCountingText(rawText)
        val markers = listOf(1..2)
        val moves = listOf(rawText.length / 2, rawText.length / 4, rawText.length, 3)
        val projection = ImeSelectionProjection(raw, markers)
        for (offset in moves) {
            assertEquals(
                "caret at $offset",
                listOf(offset, offset, -1, -1).map {
                    if (it < 0) -1 else visiblePrefixLength(rawText, markers, it)
                },
                projection.project(listOf(offset, offset, -1, -1))
            )
        }
        assertTrue(
            "${moves.size} caret moves read ${raw.readCount} characters of ${raw.length}",
            raw.readCount <= raw.length
        )
    }

    @Test
    fun `empty and inactive selections do not inspect document text`() {
        for (selection in listOf(emptyList(), listOf(-1, -2, Int.MIN_VALUE))) {
            val raw = ReadCountingText(RICH_TEXT.repeat(SELECTION_TAIL_REPETITIONS))
            assertEquals(
                selection.map { -1 },
                ImeSelectionProjection(raw, listOf(0 until raw.length)).project(selection)
            )
            assertEquals("inactive coordinates need no text reads", 0, raw.readCount)
        }
    }

    @Test
    fun `selection and both inverse affinities match independently filtered prefixes`() {
        val fixtures = listOf("", "\u200B\u200B", "A\u200B😀éZ\uFFFC\u200B", RICH_TEXT)
        for (rawText in fixtures) {
            val all = 0 until rawText.length
            val masks = listOf(
                emptyList(),
                listOf(all),
                listOf(all, 0 until minOf(2, rawText.length), all),
                listOf(rawText.length / 2 until rawText.length, 0 until rawText.length / 2),
                listOf(
                    0 until minOf(3, rawText.length),
                    minOf(1, rawText.length) until minOf(4, rawText.length)
                ),
                listOf(rawText.length until rawText.length)
            )
            for (markers in masks) {
                val raw = SpannableStringBuilder(rawText)
                markers.forEach { range ->
                    raw.setSpan(
                        android.text.Annotation(
                            RenderBridge.NATIVE_LIST_MARKER_ANNOTATION,
                            "marker"
                        ),
                        range.first,
                        range.last + 1,
                        Spanned.SPAN_INCLUSIVE_INCLUSIVE
                    )
                }
                val positions = listOf(-1, raw.length + 1) + (0..raw.length).reversed()
                val prefixLengths = (0..raw.length).map {
                    visiblePrefixLength(rawText, markers, it)
                }
                val expectedSelection = positions.map { offset ->
                    if (offset < 0) -1 else prefixLengths[offset.coerceAtMost(raw.length)]
                }
                val description = "raw=$rawText markers=$markers"
                assertEquals(
                    description,
                    expectedSelection,
                    ImeSelectionProjection(raw, markers).project(positions)
                )
                val mapper = ImeTextCoordinateMapper.build(raw, MAPPER_GENERATION)
                val expectedText = rawText.filterIndexed { index, character ->
                    character != LayoutConstants.SYNTHETIC_PLACEHOLDER_CHARACTER[0] &&
                        markers.none { index in it }
                }
                assertEquals(description, expectedText, mapper.visibleText.toString())
                for (rawOffset in prefixLengths.indices) {
                    assertEquals(description, prefixLengths[rawOffset], mapper.rawToIme(rawOffset))
                }
                for (imeOffset in 0..expectedText.length) {
                    val matching = prefixLengths.indices.filter { prefixLengths[it] == imeOffset }
                    assertEquals(
                        description,
                        matching.first(),
                        mapper.imeToRaw(imeOffset, ImeTextCoordinateMapper.Affinity.BEFORE)
                    )
                    assertEquals(
                        description,
                        matching.last(),
                        mapper.imeToRaw(imeOffset, ImeTextCoordinateMapper.Affinity.AFTER)
                    )
                }
            }
        }
    }

    @Test
    fun `unchanged IME coordinates freeze metadata and omit composing styles`() {
        val raw = SpannableStringBuilder(RICH_TEXT)
        val bold = StyleSpan(Typeface.BOLD)
        val composing = StyleSpan(Typeface.ITALIC)
        val metadata = RenderMetadataAnnotation(
            RenderBridge.NATIVE_TOP_LEVEL_CHILD_INDEX_ANNOTATION,
            ORIGINAL_CHILD_INDEX
        )
        raw.setSpan(bold, 0, raw.length, Spanned.SPAN_EXCLUSIVE_EXCLUSIVE)
        raw.setSpan(
            composing,
            0,
            raw.length,
            Spanned.SPAN_EXCLUSIVE_EXCLUSIVE or Spanned.SPAN_COMPOSING
        )
        raw.setSpan(metadata, 0, raw.length, Spanned.SPAN_INCLUSIVE_EXCLUSIVE)
        Selection.setSelection(raw, raw.length)

        val mapper = ImeTextCoordinateMapper.build(raw, generation = MAPPER_GENERATION)
        val visible = mapper.visibleText.subSequence(0, mapper.visibleText.length) as Spanned
        metadata.updateValue(UPDATED_CHILD_INDEX)

        assertEquals(RICH_TEXT, visible.toString())
        assertEquals(raw.length, mapper.rawToIme(raw.length))
        assertEquals(
            raw.length,
            mapper.imeToRaw(raw.length, ImeTextCoordinateMapper.Affinity.AFTER)
        )
        assertEquals(
            listOf(Typeface.BOLD),
            visible.getSpans(0, visible.length, StyleSpan::class.java).map {
                it.style
            }
        )
        assertEquals(0, visible.getSpanStart(bold))
        assertEquals(raw.length, visible.getSpanEnd(bold))
        assertEquals(-1, visible.getSpanStart(composing))
        assertEquals(-1, Selection.getSelectionStart(visible))
        assertEquals(
            ORIGINAL_CHILD_INDEX,
            visible.getSpans(0, visible.length, android.text.Annotation::class.java).single().value
        )
    }

    @Test
    fun `IME coordinates share the editable rendered snapshot instead of copying the document`() {
        val warmUp = styledParagraphs(1)
        ImeTextCoordinateMapper.build(warmUp, generation = MAPPER_GENERATION).visibleText.toString()
        val raw = styledParagraphs(LARGE_DOCUMENT_PARAGRAPHS)
        Selection.setSelection(raw, raw.length / 2)
        immutableRenderedText(raw, excludeNoCopySpans = true)

        lateinit var mapper: ImeTextCoordinateMapper
        val allocated = bytesAllocatedBy {
            mapper = ImeTextCoordinateMapper.build(raw, generation = MAPPER_GENERATION)
        }

        assertTrue(
            "IME coordinates for ${raw.length} UTF16 units allocated $allocated bytes",
            allocated < LARGE_DOCUMENT_ALLOCATION_LIMIT_BYTES
        )
        val visible = mapper.visibleText as Spanned
        val sliceStart = LARGE_PARAGRAPH.length * (LARGE_DOCUMENT_PARAGRAPHS / 2)
        val slice = visible.subSequence(sliceStart, sliceStart + LARGE_PARAGRAPH.length) as Spanned
        assertEquals(LARGE_PARAGRAPH, slice.toString())
        assertEquals(
            listOf(0 to LARGE_PARAGRAPH_BOLD_LENGTH),
            slice.getSpans(0, slice.length, StyleSpan::class.java).map {
                slice.getSpanStart(it) to slice.getSpanEnd(it)
            }
        )
    }

    @Test
    fun `maps rendered placeholders out of IME visible UTF 16 coordinates`() {
        val mapper = ImeTextCoordinateMapper.build("\u200Ba\uD83D\uDE00\u200Bb", generation = 7)

        assertEquals(7L, mapper.generation)
        assertEquals("a\uD83D\uDE00b", mapper.visibleText.toString())
        assertEquals(0, mapper.rawToIme(1))
        assertEquals(3, mapper.rawToIme(4))
        assertEquals(4, mapper.rawToIme(6))
        assertEquals(4, mapper.imeToRaw(3, ImeTextCoordinateMapper.Affinity.BEFORE))
        assertEquals(5, mapper.imeToRaw(3, ImeTextCoordinateMapper.Affinity.AFTER))
    }

    @Test
    fun `affinity selects either side of consecutive invisible placeholders`() {
        val mapper = ImeTextCoordinateMapper.build("a\u200B\u200Bb", generation = 2)

        assertEquals("ab", mapper.visibleText.toString())
        assertEquals(1, mapper.imeToRaw(1, ImeTextCoordinateMapper.Affinity.BEFORE))
        assertEquals(3, mapper.imeToRaw(1, ImeTextCoordinateMapper.Affinity.AFTER))
        assertEquals(0, mapper.imeToRaw(-20, ImeTextCoordinateMapper.Affinity.BEFORE))
        assertEquals(4, mapper.imeToRaw(20, ImeTextCoordinateMapper.Affinity.AFTER))
    }

    @Test
    fun `trailing invisible placeholders retain both end affinities`() {
        val mapper = ImeTextCoordinateMapper.build("a\u200B", generation = 6)

        assertEquals("a", mapper.visibleText.toString())
        assertEquals(1, mapper.imeToRaw(1, ImeTextCoordinateMapper.Affinity.BEFORE))
        assertEquals(2, mapper.imeToRaw(1, ImeTextCoordinateMapper.Affinity.AFTER))
    }

    @Test
    fun `visible text retains spans across removed placeholders`() {
        val raw = SpannableString("A\u200BB").apply {
            setSpan(AbsoluteSizeSpan(28), 0, length, Spanned.SPAN_EXCLUSIVE_EXCLUSIVE)
        }

        val visible = ImeTextCoordinateMapper.build(raw, generation = 3).visibleText

        assertTrue(visible is Spanned)
        val span = (visible as Spanned).getSpans(
            0,
            visible.length,
            AbsoluteSizeSpan::class.java
        ).single()
        assertEquals(0, visible.getSpanStart(span))
        assertEquals(2, visible.getSpanEnd(span))
    }

    @Test
    fun `document object replacements remain visible to the IME`() {
        val mapper = ImeTextCoordinateMapper.build("a\uFFFCb", generation = 4)

        assertEquals("a\uFFFCb", mapper.visibleText.toString())
        assertEquals(2, mapper.rawToIme(2))
    }

    @Test
    fun `IME snapshots exclude Android selection and composing markers`() {
        val raw = SpannableString("abc")
        val watcherMarker = NoCopySpan.Concrete()
        Selection.setSelection(raw, 1, 2)
        BaseInputConnection.setComposingSpans(raw)
        raw.setSpan(watcherMarker, 0, raw.length, Spanned.SPAN_INCLUSIVE_INCLUSIVE)

        val visible = ImeTextCoordinateMapper.build(raw, generation = 5).visibleText

        assertEquals(-1, Selection.getSelectionStart(visible))
        assertEquals(-1, Selection.getSelectionEnd(visible))
        assertEquals(-1, (visible as Spanned).getSpanStart(watcherMarker))
        assertEquals(
            "selection, composing and watcher markers must not reach the IME",
            emptyList<Any>(),
            visible.getSpans(0, visible.length, Any::class.java).toList()
        )
    }

    private fun styledParagraphs(count: Int): EditorEditable =
        EditorEditable(LARGE_PARAGRAPH.repeat(count)).apply {
            for (paragraph in 0 until count) {
                val start = paragraph * LARGE_PARAGRAPH.length
                setSpan(
                    StyleSpan(Typeface.BOLD),
                    start,
                    start + LARGE_PARAGRAPH_BOLD_LENGTH,
                    Spanned.SPAN_EXCLUSIVE_EXCLUSIVE
                )
                setSpan(
                    AbsoluteSizeSpan(LARGE_PARAGRAPH_TEXT_SIZE),
                    start,
                    start + LARGE_PARAGRAPH.length,
                    Spanned.SPAN_EXCLUSIVE_EXCLUSIVE
                )
            }
        }

    private class ReadCountingText(private val value: String) : CharSequence {
        var readCount = 0
            private set
        var furthestReadOffset = -1
            private set

        override val length: Int get() = value.length

        override fun get(index: Int): Char {
            readCount += 1
            furthestReadOffset = maxOf(furthestReadOffset, index)
            return value[index]
        }

        override fun subSequence(startIndex: Int, endIndex: Int): CharSequence =
            throw AssertionError("selection projection must not copy a document slice")

        override fun toString(): String =
            throw AssertionError("selection projection must not copy the document")
    }

    private fun visiblePrefixLength(raw: String, markers: List<IntRange>, offset: Int): Int =
        raw.take(offset).filterIndexed { index, character ->
            character != LayoutConstants.SYNTHETIC_PLACEHOLDER_CHARACTER[0] &&
                markers.none { index in it }
        }.length

    private companion object {
        const val SELECTION_TAIL_REPETITIONS = 10_000
        const val RICH_TEXT = "A😀éZ"
        const val ORIGINAL_CHILD_INDEX = "0"
        const val UPDATED_CHILD_INDEX = "1"
        const val MAPPER_GENERATION = 9L
        const val LARGE_PARAGRAPH = "Paragraph text with caf\u00E9 and \uD83D\uDE00 emoji.\n"
        const val LARGE_PARAGRAPH_BOLD_LENGTH = 9
        const val LARGE_PARAGRAPH_TEXT_SIZE = 18
        const val LARGE_DOCUMENT_PARAGRAPHS = 4_000
        const val LARGE_DOCUMENT_ALLOCATION_LIMIT_BYTES = 64L * 1024L
    }
}
