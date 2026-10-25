package com.apollohg.editor

import android.text.Annotation
import android.text.GetChars
import android.text.SpannableStringBuilder
import android.text.Spanned
import android.text.TextUtils
import java.util.IdentityHashMap

internal fun frozenRenderedTextSnapshot(text: CharSequence): SpannableStringBuilder =
    SpannableStringBuilder(if (text is Spanned) FrozenRenderedSpans(text) else text)

internal class FrozenRenderedSpans(private val source: Spanned) :
    Spanned,
    GetChars,
    CharSequence by source {
    private val frozenByOriginal = IdentityHashMap<Any, Any>()
    private val originalByFrozen = IdentityHashMap<Any, Any>()

    private fun frozenSpan(original: Any): Any = frozenByOriginal.getOrPut(original) {
        val frozen = when (original) {
            is RenderMetadataAnnotation -> Annotation(original.key, original.value)

            is AtomBlockSpan -> AtomBlockSpan(
                original.atomKey,
                original.nodeType,
                original.docPos,
                original.reservedHeightPx,
                original.hasStableAtomId,
                original.isDirectRootChild
            )

            else -> original
        }
        originalByFrozen[frozen] = original
        frozen
    }

    override fun <T : Any?> getSpans(start: Int, end: Int, type: Class<T>): Array<T> {
        val spans = source.getSpans(start, end, Any::class.java)
            .map(::frozenSpan).filter(type::isInstance)

        @Suppress("UNCHECKED_CAST")
        val result = java.lang.reflect.Array.newInstance(type, spans.size) as Array<T>
        spans.forEachIndexed { index, span -> result[index] = requireNotNull(type.cast(span)) }
        return result
    }

    override fun getSpanStart(tag: Any?): Int = source.getSpanStart(originalByFrozen[tag] ?: tag)

    override fun getSpanEnd(tag: Any?): Int = source.getSpanEnd(originalByFrozen[tag] ?: tag)

    override fun getSpanFlags(tag: Any?): Int = source.getSpanFlags(originalByFrozen[tag] ?: tag)

    override fun nextSpanTransition(start: Int, limit: Int, type: Class<*>?): Int {
        var next = limit
        for (original in source.getSpans(start, limit, Any::class.java)) {
            if (type != null && !type.isInstance(frozenSpan(original))) continue
            val spanStart = source.getSpanStart(original)
            val spanEnd = source.getSpanEnd(original)
            if (spanStart > start && spanStart < next) next = spanStart
            if (spanEnd > start && spanEnd < next) next = spanEnd
        }
        return next
    }

    override fun getChars(start: Int, end: Int, destination: CharArray, offset: Int) =
        TextUtils.getChars(source, start, end, destination, offset)

    override fun subSequence(startIndex: Int, endIndex: Int): CharSequence =
        frozenRenderedTextSnapshot(source.subSequence(startIndex, endIndex))

    override fun toString(): String = source.toString()
}
