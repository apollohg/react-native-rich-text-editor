package com.apollohg.editor

import android.text.Annotation
import android.text.Spanned

internal class RootTablePositionMap private constructor(
    private val markers: List<Marker>,
    private val localLength: Int,
    private val globalLength: Int
) {
    private data class Marker(val localStart: Int, val globalStart: Int, val globalEnd: Int) {
        val localEnd: Int get() = localStart + 1
        val delta: Int get() = globalEnd - globalStart - 1
    }

    fun globalScalar(local: Int): Int? {
        if (local !in 0..localLength) return null
        var delta = 0
        for (marker in markers) {
            if (local in marker.localStart..marker.localEnd) return null
            if (local < marker.localStart) break
            delta += marker.delta
        }
        return local + delta
    }

    fun globalRange(from: Int, to: Int): Pair<Int, Int>? {
        if (from > to || markers.any { from <= it.localEnd && it.localStart <= to }) return null
        val start = globalScalar(from) ?: return null
        val end = globalScalar(to) ?: return null
        return start to end
    }

    fun isImmediatelyAfterTable(local: Int): Boolean = markers.any { local == it.localEnd + 1 }

    fun localScalar(global: Int): Int? {
        if (global !in 0..globalLength) return null
        var delta = 0
        for (marker in markers) {
            if (global in marker.globalStart..marker.globalEnd) return null
            if (global < marker.globalStart) break
            delta += marker.delta
        }
        val local = global - delta
        return local.takeIf { globalScalar(it) == global }
    }

    companion object {
        fun fromRendered(
            content: Spanned,
            extents: Map<String, TableInputExtent>,
            scalarLength: Int
        ): RootTablePositionMap? {
            val annotations = content.getSpans(0, content.length, Annotation::class.java)
                .filter { it.key == RenderBridge.NATIVE_ROOT_TABLE_MARKER_ANNOTATION }
                .sortedBy(content::getSpanStart)
            if (annotations.map { it.value }.toSet() != extents.keys ||
                annotations.size != extents.size
            ) return null
            val text = content.toString()
            val markers = mutableListOf<Marker>()
            var delta = 0
            for (annotation in annotations) {
                val utf16Start = content.getSpanStart(annotation)
                if (utf16Start < 0 || content.getSpanEnd(annotation) != utf16Start + 1 ||
                    text[utf16Start] != '\u200B'
                ) return null
                val localStart = PositionBridge.utf16ToScalar(utf16Start, text)
                val extent = extents[annotation.value] ?: return null
                if (extent.scalarStart != localStart + delta ||
                    extent.scalarEnd <= extent.scalarStart ||
                    markers.lastOrNull()?.localEnd?.let { it >= localStart } == true
                ) return null
                markers += Marker(localStart, extent.scalarStart, extent.scalarEnd)
                delta += extent.scalarEnd - extent.scalarStart - 1
            }
            val localLength = PositionBridge.utf16ToScalar(text.length, text)
            if (localLength + delta != scalarLength) return null
            return RootTablePositionMap(markers, localLength, scalarLength)
        }
    }
}
