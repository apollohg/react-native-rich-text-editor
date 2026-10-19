package com.apollohg.editor

import android.text.SpannableStringBuilder
import android.text.Spanned

internal open class RenderTextBuilder : SpannableStringBuilder() {
    private var topLevelChildIndex: Int? = null
    private var topLevelChildStart = 0
    private var topLevelChildEnd = 0

    fun annotateTopLevelChild(start: Int, end: Int, index: Int) {
        if (index == topLevelChildIndex && start <= topLevelChildEnd && end >= topLevelChildStart) {
            topLevelChildStart = minOf(topLevelChildStart, start)
            topLevelChildEnd = maxOf(topLevelChildEnd, end)
            return
        }
        flushTopLevelChild()
        topLevelChildIndex = index
        topLevelChildStart = start
        topLevelChildEnd = end
    }

    fun annotateTopLevelSeparator(start: Int, end: Int, index: Int) {
        flushTopLevelChild()
        setTopLevelChildSpan(start, end, index)
    }

    fun flushTopLevelChild() {
        val index = topLevelChildIndex ?: return
        topLevelChildIndex = null
        setTopLevelChildSpan(topLevelChildStart, topLevelChildEnd, index)
    }

    private fun setTopLevelChildSpan(start: Int, end: Int, index: Int) {
        setSpan(
            RenderMetadataAnnotation(
                RenderBridge.NATIVE_TOP_LEVEL_CHILD_INDEX_ANNOTATION,
                index.toString()
            ),
            start,
            end,
            Spanned.SPAN_INCLUSIVE_EXCLUSIVE
        )
    }
}
