package com.apollohg.editor

import android.icu.text.BreakIterator

/**
 * Converts between Android UTF-16 offsets and Rust editor-core scalar offsets,
 * then snaps UTF-16 positions to grapheme boundaries when Android reports a
 * cursor inside a composed character.
 */
object PositionBridge {
    private class SurrogatePairIndex(val text: String, val pairStarts: IntArray)

    private val cacheLock = Any()

    @Volatile private var cachedIndex: SurrogatePairIndex? = null

    /**
     * Counts code points from the start of the string up to the given UTF-16 offset.
     */
    fun utf16ToScalar(utf16Offset: Int, text: String): Int {
        val pairStarts = surrogatePairStartsFor(text)
        val offset = utf16Offset.coerceIn(0, text.length)
        return offset - pairStarts.partitionPoint { pairStarts[it] < offset - 1 }
    }

    /**
     * Counts UTF-16 code units from the start of the string up to the given scalar offset.
     */
    fun scalarToUtf16(scalarOffset: Int, text: String): Int {
        if (scalarOffset <= 0) return 0

        val pairStarts = surrogatePairStartsFor(text)
        val scalar = scalarOffset.coerceAtMost(text.length - pairStarts.size)
        return scalar + pairStarts.partitionPoint { pairStarts[it] - it < scalar }
    }

    /**
     * Biases forward to the next grapheme boundary when Android reports an
     * offset inside a composed character sequence.
     */
    fun snapToGraphemeBoundary(utf16Offset: Int, text: String): Int {
        if (text.isEmpty()) return 0

        val clampedOffset = utf16Offset.coerceIn(0, text.length)

        if (clampedOffset == 0 || clampedOffset == text.length) {
            return clampedOffset
        }

        val breakIterator = BreakIterator.getCharacterInstance()
        breakIterator.setText(text)

        if (breakIterator.isBoundary(clampedOffset)) {
            return clampedOffset
        }

        val nextBoundary = breakIterator.following(clampedOffset)
        return if (nextBoundary == BreakIterator.DONE) text.length else nextBoundary
    }

    internal fun graphemeBoundaryBefore(utf16Offset: Int, text: String): Int {
        val offset = utf16Offset.coerceIn(0, text.length)
        if (offset == 0) return offset
        val iterator = BreakIterator.getCharacterInstance()
        iterator.setText(text)
        return iterator.preceding(offset).takeUnless { it == BreakIterator.DONE } ?: 0
    }

    /**
     * Snaps a UTF-16 offset out of the middle of a surrogate pair without
     * applying full grapheme-cluster expansion.
     */
    fun snapToScalarBoundary(utf16Offset: Int, text: String, biasForward: Boolean): Int {
        val clampedOffset = utf16Offset.coerceIn(0, text.length)
        if (clampedOffset <= 0 || clampedOffset >= text.length) return clampedOffset

        val previous = text[clampedOffset - 1]
        val current = text[clampedOffset]
        if (Character.isHighSurrogate(previous) && Character.isLowSurrogate(current)) {
            return if (biasForward) clampedOffset + 1 else clampedOffset - 1
        }
        return clampedOffset
    }

    fun snapRangeToScalarBoundaries(start: Int, end: Int, text: String): Pair<Int, Int> {
        val lower = minOf(start, end).coerceIn(0, text.length)
        val upper = maxOf(start, end).coerceIn(0, text.length)
        return snapToScalarBoundary(lower, text, biasForward = false) to
            snapToScalarBoundary(upper, text, biasForward = true)
    }

    private fun surrogatePairStartsFor(text: String): IntArray {
        cachedIndex?.takeIf { it.text == text }?.let { return it.pairStarts }

        synchronized(cacheLock) {
            cachedIndex?.takeIf { it.text == text }?.let { return it.pairStarts }

            val pairStarts = IntArray(text.length - text.codePointCount(0, text.length))
            var pairIndex = 0
            var utf16Pos = 0
            while (utf16Pos < text.length) {
                val charCount = Character.charCount(Character.codePointAt(text, utf16Pos))
                if (charCount == SURROGATE_PAIR_LENGTH) pairStarts[pairIndex++] = utf16Pos
                utf16Pos += charCount
            }
            cachedIndex = SurrogatePairIndex(text, pairStarts)
            return pairStarts
        }
    }

    private const val SURROGATE_PAIR_LENGTH = 2
}
