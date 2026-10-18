package com.apollohg.editor

import android.text.Annotation
import android.text.NoCopySpan
import android.text.SpanWatcher
import android.text.SpannableStringBuilder
import android.text.Spanned
import android.text.SpannedString
import android.text.TextWatcher
import java.util.concurrent.atomic.AtomicLong

internal class TextReplacement(val start: Int, val before: Int, val count: Int) {
    fun map(offset: Int): Int = when {
        offset >= start + before -> offset + count - before
        offset > start -> start + count
        else -> offset
    }
}

internal class EditorEditable(text: CharSequence = "") : SpannableStringBuilder(text) {
    private val watcherSpans = mutableListOf<Any>()
    private var platformQueryDepth = 0
    var contentVersion = nextContentVersion()
        private set
    var replacementInProgress: TextReplacement? = null
        private set
    private var spanVersion = 0L
    private var renderedSnapshot: Spanned? = null
    private var renderedSnapshotVersion = -1L

    fun renderedSnapshot(): Spanned {
        renderedSnapshot?.takeIf { renderedSnapshotVersion == spanVersion }?.let { return it }
        return RenderedTextFragments(listOf(this), excludeNoCopySpans = true).also {
            renderedSnapshot = it
            renderedSnapshotVersion = spanVersion
        }
    }

    init {
        rebuildWatcherIndex()
    }

    private fun rememberWatcher(span: Any?) {
        if ((span is SpanWatcher || span is TextWatcher) && watcherSpans.none { it === span }) {
            watcherSpans.add(requireNotNull(span))
        }
    }

    private fun rebuildWatcherIndex() {
        watcherSpans.clear()
        super.getSpans(0, length, SpanWatcher::class.java).forEach(::rememberWatcher)
        super.getSpans(0, length, TextWatcher::class.java).forEach(::rememberWatcher)
    }

    override fun setSpan(what: Any?, start: Int, end: Int, flags: Int) {
        if (what !is NoCopySpan) spanVersion++
        if (what is Annotation) contentVersion = nextContentVersion()
        rememberWatcher(what)
        try {
            super.setSpan(what, start, end, flags)
        } finally {
            if (super.getSpanStart(what) < 0) watcherSpans.removeAll { it === what }
        }
    }

    override fun removeSpan(what: Any?) {
        if (what !is NoCopySpan) spanVersion++
        if (what is Annotation) contentVersion = nextContentVersion()
        watcherSpans.removeAll { it === what }
        super.removeSpan(what)
    }

    override fun clearSpans() {
        spanVersion++
        contentVersion = nextContentVersion()
        platformQueryDepth++
        try {
            super.clearSpans()
        } finally {
            platformQueryDepth--
            rebuildWatcherIndex()
        }
    }

    override fun replace(
        start: Int,
        end: Int,
        source: CharSequence,
        sourceStart: Int,
        sourceEnd: Int
    ): SpannableStringBuilder {
        // Mutable sources and filters can add watchers inside Android's private replacement loop.
        val platformQueries = filters.isNotEmpty() || (
            source is Spanned && (
                (source !is SpannedString && source !is RenderedTextFragments) ||
                    source.getSpans(sourceStart, sourceEnd, SpanWatcher::class.java).isNotEmpty() ||
                    source.getSpans(sourceStart, sourceEnd, TextWatcher::class.java).isNotEmpty()
                )
            )
        if (platformQueries) platformQueryDepth++
        spanVersion++
        contentVersion = nextContentVersion()
        val enclosingReplacement = replacementInProgress
        replacementInProgress = TextReplacement(start, end - start, sourceEnd - sourceStart)
        try {
            return super.replace(start, end, source, sourceStart, sourceEnd)
        } finally {
            replacementInProgress = enclosingReplacement
            spanVersion++
            contentVersion = nextContentVersion()
            if (platformQueries) {
                platformQueryDepth--
                rebuildWatcherIndex()
            }
        }
    }

    override fun <T : Any?> getSpans(start: Int, end: Int, kind: Class<T>?): Array<T> {
        if (platformQueryDepth > 0 ||
            (kind != SpanWatcher::class.java && kind != TextWatcher::class.java)
        ) {
            return super.getSpans(start, end, kind)
        }
        watcherSpans.removeAll { super.getSpanStart(it) < 0 }
        if (watcherSpans.size > 1) return super.getSpans(start, end, kind)
        val watcher = watcherSpans.singleOrNull()?.takeIf {
            requireNotNull(kind).isInstance(it) &&
                spanIntersectsQuery(super.getSpanStart(it), super.getSpanEnd(it), start, end)
        }

        @Suppress("UNCHECKED_CAST")
        val result = java.lang.reflect.Array.newInstance(
            kind,
            if (watcher ==
                null
            ) {
                0
            } else {
                1
            }
        ) as Array<T>
        if (watcher != null) result[0] = requireNotNull(kind).cast(watcher)
        return result
    }

    private companion object {
        private val contentVersions = AtomicLong()

        fun nextContentVersion(): Long = contentVersions.incrementAndGet()
    }
}
