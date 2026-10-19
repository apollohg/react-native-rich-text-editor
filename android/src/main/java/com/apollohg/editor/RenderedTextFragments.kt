package com.apollohg.editor

import android.text.GetChars
import android.text.NoCopySpan
import android.text.SpannableStringBuilder
import android.text.Spanned
import android.text.SpannedString
import android.text.TextUtils
import java.util.IdentityHashMap

internal fun concatenateRenderedText(fragments: List<Spanned>): SpannableStringBuilder =
    SpannableStringBuilder(RenderedTextFragments(fragments))

internal fun immutableRenderedText(
    text: CharSequence,
    excludeNoCopySpans: Boolean = false
): Spanned = if (!excludeNoCopySpans && (text is SpannedString || text is RenderedTextFragments)) {
    text as Spanned
} else if (excludeNoCopySpans && text is EditorEditable) {
    text.renderedSnapshot()
} else {
    RenderedTextFragments(
        listOf(if (text is Spanned) text else SpannedString(text)),
        excludeNoCopySpans
    )
}

internal fun spanIntersectsQuery(spanStart: Int, spanEnd: Int, start: Int, end: Int): Boolean =
    spanEnd >= start && spanStart <= end &&
        (spanStart == spanEnd || start == end || (spanStart != end && spanEnd != start))

internal class RenderedTextFragments(
    fragments: List<Spanned>,
    excludeNoCopySpans: Boolean = false
) : Spanned,
    GetChars {
    private inner class SpanIndex {
        private val startOrder = LongArray(spans.size) { index ->
            (spanStarts[index].toLong() shl Int.SIZE_BITS) or index.toLong()
        }.apply { sort() }
        private val maxEnds = IntArray(spans.size)

        init {
            build(0, startOrder.size)
        }

        private fun build(first: Int, last: Int): Int {
            if (first >= last) return 0
            val middle = first + (last - first) / 2
            val end = maxOf(
                spanEnds[startOrder[middle].toInt()],
                build(first, middle),
                build(middle + 1, last)
            )
            maxEnds[middle] = end
            return end
        }

        fun matching(start: Int, end: Int, type: Class<*>): LongArray {
            val count = collect(start, end, type, null, 0, 0, startOrder.size)
            return LongArray(count).also {
                collect(start, end, type, it, 0, 0, startOrder.size)
                it.sort()
            }
        }

        private fun collect(
            start: Int,
            end: Int,
            type: Class<*>,
            result: LongArray?,
            count: Int,
            first: Int,
            last: Int
        ): Int {
            if (first >= last) return count
            val middle = first + (last - first) / 2
            if (spanStarts[startOrder[first].toInt()] > end || maxEnds[middle] < start) return count
            var next = collect(start, end, type, result, count, first, middle)
            val index = startOrder[middle].toInt()
            if (type.isInstance(spans[index]) &&
                spanIntersectsQuery(spanStarts[index], spanEnds[index], start, end)
            ) {
                if (result != null) result[next] = priorityKey(index)
                next++
            }
            return collect(start, end, type, result, next, middle + 1, last)
        }

        fun nextTransition(
            start: Int,
            limit: Int,
            type: Class<*>?,
            first: Int = 0,
            last: Int = startOrder.size
        ): Int {
            if (first >= last) return limit
            val middle = first + (last - first) / 2
            if (spanStarts[startOrder[first].toInt()] >= limit || maxEnds[middle] <= start) {
                return limit
            }
            var next = nextTransition(start, limit, type, first, middle)
            val index = startOrder[middle].toInt()
            if (type == null || type.isInstance(spans[index])) {
                if (spanStarts[index] > start && spanStarts[index] < next) next = spanStarts[index]
                if (spanEnds[index] > start && spanEnds[index] < next) next = spanEnds[index]
            }
            return nextTransition(start, next, type, middle + 1, last)
        }
    }

    private val content = buildString(fragments.sumOf { it.length }) {
        fragments.forEach(::append)
    }
    private val spans: List<Any>
    private val spanStarts: IntArray
    private val spanEnds: IntArray
    private val spanFlags: IntArray
    private val priorityOrder: LongArray
    private val indices = IdentityHashMap<Any, Int>()
    private val spanIndex by lazy { SpanIndex() }

    init {
        val groups = fragments.map { it.getSpans(0, it.length, Any::class.java) }
        val capacity = groups.sumOf { it.size }
        val copied = ArrayList<Any>(capacity)
        spanStarts = IntArray(capacity)
        spanEnds = IntArray(capacity)
        spanFlags = IntArray(capacity)
        var offset = 0
        for ((fragmentIndex, fragment) in fragments.withIndex()) {
            for (span in groups[fragmentIndex]) {
                if (excludeNoCopySpans && span is NoCopySpan) continue
                val index = indices[span] ?: copied.size.also {
                    indices[span] = it
                    copied.add(span)
                }
                spanStarts[index] = offset + fragment.getSpanStart(span)
                spanEnds[index] = offset + fragment.getSpanEnd(span)
                spanFlags[index] = fragment.getSpanFlags(span)
            }
            offset += fragment.length
        }
        spans = copied
        // High bits sort priority; low bits preserve insertion order.
        priorityOrder = LongArray(spans.size) { priorityKey(it) }.apply { sort() }
    }

    override val length: Int get() = content.length
    override fun get(index: Int): Char = content[index]
    override fun toString(): String = content
    override fun subSequence(startIndex: Int, endIndex: Int): CharSequence =
        SpannableStringBuilder(this, startIndex, endIndex)

    override fun getChars(start: Int, end: Int, destination: CharArray, offset: Int) =
        TextUtils.getChars(content, start, end, destination, offset)

    override fun getSpanStart(tag: Any?): Int = indices[tag]?.let { spanStarts[it] } ?: -1
    override fun getSpanEnd(tag: Any?): Int = indices[tag]?.let { spanEnds[it] } ?: -1
    override fun getSpanFlags(tag: Any?): Int = indices[tag]?.let { spanFlags[it] } ?: 0

    private fun priorityKey(index: Int): Long {
        val priority = Spanned.SPAN_PRIORITY - (spanFlags[index] and Spanned.SPAN_PRIORITY)
        return (priority.toLong() shl Int.SIZE_BITS) or index.toLong()
    }

    override fun <T : Any?> getSpans(start: Int, end: Int, type: Class<T>): Array<T> {
        val whole = start == 0 && end == length
        val matching = if (whole) priorityOrder else spanIndex.matching(start, end, type)
        val filterByType = whole && type != Any::class.java
        val count = if (filterByType) {
            matching.count { type.isInstance(spans[it.toInt()]) }
        } else {
            matching.size
        }

        @Suppress("UNCHECKED_CAST")
        val result = java.lang.reflect.Array.newInstance(type, count) as Array<T>
        var destination = 0
        for (key in matching) {
            val span = spans[key.toInt()]
            if (!filterByType || type.isInstance(span)) result[destination++] = type.cast(span)
        }
        return result
    }

    override fun nextSpanTransition(start: Int, limit: Int, type: Class<*>?): Int =
        spanIndex.nextTransition(start, limit, type)
}
