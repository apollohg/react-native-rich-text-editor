package com.apollohg.editor

import android.text.Annotation
import android.text.GetChars
import android.text.SpannableStringBuilder
import android.text.Spanned

internal class ImeTextCoordinateMapper private constructor(
    val visibleText: CharSequence,
    val generation: Long,
    private val hiddenRawOffsets: IntArray
) {
    enum class Affinity {
        BEFORE,
        AFTER
    }

    private val rawLength = visibleText.length + hiddenRawOffsets.size

    fun rawToIme(offset: Int): Int = hiddenRawOffsets.visibleOffsetOf(offset.coerceIn(0, rawLength))

    fun imeToRaw(offset: Int, affinity: Affinity): Int =
        hiddenRawOffsets.rawOffsetOf(offset.coerceIn(0, visibleText.length), affinity)

    companion object {
        internal fun generatedMarkerRanges(raw: CharSequence): List<IntRange> =
            (raw as? Spanned)?.let { content ->
                content.getSpans(0, content.length, Annotation::class.java)
                    .filter { it.key == RenderBridge.NATIVE_LIST_MARKER_ANNOTATION }
                    .map { content.getSpanStart(it) until content.getSpanEnd(it) }
            }.orEmpty()

        fun build(raw: CharSequence, generation: Long): ImeTextCoordinateMapper {
            val hiddenRawOffsets = HiddenImeOffsets(raw, generatedMarkerRanges(raw)).scanAll()
            val visibleText = if (raw is Spanned) {
                FrozenRenderedSpans(
                    ImeVisibleText(
                        immutableRenderedText(raw, excludeNoCopySpans = true),
                        hiddenRawOffsets
                    )
                )
            } else {
                ImeVisibleText(raw, hiddenRawOffsets).toString()
            }
            return ImeTextCoordinateMapper(visibleText, generation, hiddenRawOffsets)
        }
    }
}

private fun IntArray.visibleOffsetOf(rawOffset: Int): Int = rawOffset - countBelow(rawOffset, size)

private fun IntArray.countBelow(value: Int, size: Int): Int =
    partitionPoint(size) { this[it] < value }

private fun IntArray.hiddenBeforeVisible(imeOffset: Int): Int =
    partitionPoint { this[it] - it <= imeOffset }

private fun IntArray.rawOffsetOf(imeOffset: Int, affinity: ImeTextCoordinateMapper.Affinity): Int =
    when (affinity) {
        ImeTextCoordinateMapper.Affinity.BEFORE -> if (imeOffset == 0) {
            0
        } else {
            imeOffset + hiddenBeforeVisible(imeOffset - 1)
        }

        ImeTextCoordinateMapper.Affinity.AFTER -> imeOffset + hiddenBeforeVisible(imeOffset)
    }

private class ImeVisibleText(
    private val raw: CharSequence,
    private val hiddenRawOffsets: IntArray
) : Spanned,
    GetChars {
    override val length: Int = raw.length - hiddenRawOffsets.size

    override fun get(index: Int): Char = raw[index + hiddenRawOffsets.hiddenBeforeVisible(index)]

    override fun getChars(start: Int, end: Int, destination: CharArray, offset: Int) {
        var hiddenIndex = hiddenRawOffsets.hiddenBeforeVisible(start)
        var rawOffset = start + hiddenIndex
        for (visibleOffset in start until end) {
            while (hiddenIndex < hiddenRawOffsets.size &&
                hiddenRawOffsets[hiddenIndex] == rawOffset
            ) {
                hiddenIndex++
                rawOffset++
            }
            destination[offset + visibleOffset - start] = raw[rawOffset++]
        }
    }

    override fun subSequence(startIndex: Int, endIndex: Int): CharSequence =
        SpannableStringBuilder(this, startIndex, endIndex)

    override fun toString(): String = String(
        CharArray(length).also {
            android.text.TextUtils.getChars(this, 0, length, it, 0)
        }
    )

    private val spanned get() = raw as? Spanned

    private fun visibleSpan(span: Any?): Boolean = spanned?.let {
        it.getSpanStart(span) >= 0 && it.getSpanFlags(span) and Spanned.SPAN_COMPOSING == 0
    } == true

    override fun getSpanStart(tag: Any?): Int = if (visibleSpan(tag)) {
        hiddenRawOffsets.visibleOffsetOf(requireNotNull(spanned).getSpanStart(tag))
    } else {
        -1
    }

    override fun getSpanEnd(tag: Any?): Int = if (visibleSpan(tag)) {
        hiddenRawOffsets.visibleOffsetOf(requireNotNull(spanned).getSpanEnd(tag))
    } else {
        -1
    }

    override fun getSpanFlags(tag: Any?): Int =
        if (visibleSpan(tag)) requireNotNull(spanned).getSpanFlags(tag) else 0

    override fun <T : Any?> getSpans(start: Int, end: Int, type: Class<T>): Array<T> {
        val source = spanned ?: return newSpanArray(type, 0)
        val rawStart = (
            hiddenRawOffsets.rawOffsetOf(start, ImeTextCoordinateMapper.Affinity.BEFORE) - 1
            ).coerceAtLeast(0)
        val rawEnd = (
            hiddenRawOffsets.rawOffsetOf(end, ImeTextCoordinateMapper.Affinity.AFTER) + 1
            ).coerceAtMost(source.length)
        val matching = source.getSpans(rawStart, rawEnd, type).filter {
            source.getSpanFlags(it) and Spanned.SPAN_COMPOSING == 0 &&
                spanIntersectsQuery(getSpanStart(it), getSpanEnd(it), start, end)
        }
        return newSpanArray(type, matching.size).also { result ->
            matching.forEachIndexed { index, span -> result[index] = span }
        }
    }

    override fun nextSpanTransition(start: Int, limit: Int, type: Class<*>?): Int {
        var next = limit
        for (span in getSpans(start, limit, type ?: Any::class.java)) {
            val spanStart = getSpanStart(span)
            val spanEnd = getSpanEnd(span)
            if (spanStart in (start + 1) until next) next = spanStart
            if (spanEnd in (start + 1) until next) next = spanEnd
        }
        return next
    }

    @Suppress("UNCHECKED_CAST")
    private fun <T> newSpanArray(type: Class<T>, size: Int): Array<T> =
        java.lang.reflect.Array.newInstance(type, size) as Array<T>
}

private class HiddenImeOffsets(private val text: CharSequence, markerRanges: List<IntRange>) {
    private val markers = if (markerRanges.size > 1) {
        markerRanges.sortedBy { it.first }
    } else {
        markerRanges
    }
    private var markerIndex = 0
    private var offsets = IntArray(INITIAL_HIDDEN_OFFSET_CAPACITY)
    private var count = 0
    private var scannedOffset = 0

    fun countBefore(rawOffset: Int): Int {
        while (scannedOffset < rawOffset) {
            if (hides(scannedOffset)) {
                if (count == offsets.size) {
                    offsets = offsets.copyOf(count * 2)
                }
                offsets[count++] = scannedOffset
            }
            scannedOffset += 1
        }
        return offsets.countBelow(rawOffset, count)
    }

    fun scanAll(): IntArray {
        countBefore(text.length)
        return offsets.copyOf(count)
    }

    private fun hides(offset: Int): Boolean {
        while (markerIndex < markers.size && markers[markerIndex].last < offset) {
            markerIndex += 1
        }
        return (markerIndex < markers.size && markers[markerIndex].first <= offset) ||
            text[offset] == LayoutConstants.SYNTHETIC_PLACEHOLDER_CHARACTER[0]
    }

    private companion object {
        const val INITIAL_HIDDEN_OFFSET_CAPACITY = 16
    }
}

internal class ImeSelectionProjection(
    val text: CharSequence,
    val generatedMarkers: List<IntRange>
) {
    private val hidden = HiddenImeOffsets(text, generatedMarkers)

    fun project(selection: List<Int>): List<Int> = selection.map { offset ->
        if (offset < 0) -1 else offset.coerceAtMost(text.length).let { it - hidden.countBefore(it) }
    }
}
