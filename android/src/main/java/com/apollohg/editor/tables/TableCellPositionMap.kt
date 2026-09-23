package com.apollohg.editor.tables

import com.apollohg.editor.PositionBridge

internal class TableCellPositionMap(
    val binding: Binding,
    segments: List<Segment>
) {
    data class Binding(
        val cellSourcePos: Long,
        val revision: String,
        val epoch: String
    )

    data class Segment(
        val localScalarStart: Int,
        val localScalarEndExclusive: Int,
        val globalScalarStart: Int
    )

    data class ScalarRange(val from: Int, val to: Int)

    private val segments = segments.sortedBy { it.localScalarStart }

    fun globalScalarForLocalScalar(
        localScalar: Int,
        currentRevision: String? = null,
        currentEpoch: String? = null
    ): Int? {
        if (!matchesCurrent(currentRevision, currentEpoch)) return null
        var resolved: Int? = null
        for (segment in segments) {
            if (localScalar < segment.localScalarStart || localScalar >= segment.localScalarEndExclusive) continue
            val candidate = add(
                segment.globalScalarStart.toLong(),
                localScalar.toLong() - segment.localScalarStart.toLong()
            ) ?: return null
            if (resolved != null && resolved != candidate) return null
            resolved = candidate
        }
        return resolved
    }

    fun localScalarForGlobalScalar(
        globalScalar: Int,
        currentRevision: String? = null,
        currentEpoch: String? = null
    ): Int? {
        if (!matchesCurrent(currentRevision, currentEpoch)) return null
        var resolved: Int? = null
        for (segment in segments) {
            val width = segment.localScalarEndExclusive.toLong() - segment.localScalarStart.toLong()
            val offset = globalScalar.toLong() - segment.globalScalarStart.toLong()
            if (width <= 0 || offset < 0 || offset >= width) continue
            val candidate = add(segment.localScalarStart.toLong(), offset) ?: return null
            if (resolved != null && resolved != candidate) return null
            resolved = candidate
        }
        return resolved
    }

    fun globalScalarRange(fromLocalScalar: Int, toLocalScalar: Int): ScalarRange? {
        if (fromLocalScalar > toLocalScalar) return null
        val globalStart = globalScalarForLocalScalar(fromLocalScalar) ?: return null
        val globalEnd = globalScalarForLocalScalar(toLocalScalar) ?: return null
        if (globalEnd.toLong() - globalStart.toLong() != toLocalScalar.toLong() - fromLocalScalar.toLong()) return null

        val rangeEndExclusive = toLocalScalar.toLong() + 1L
        var covered = fromLocalScalar.toLong()
        for (segment in segments) {
            val lower = maxOf(segment.localScalarStart.toLong(), fromLocalScalar.toLong())
            val upper = minOf(segment.localScalarEndExclusive.toLong(), rangeEndExclusive)
            if (lower >= upper) continue
            if (lower > covered) return null
            val actual = segment.globalScalarStart.toLong() + lower - segment.localScalarStart.toLong()
            val expected = globalStart.toLong() + lower - fromLocalScalar.toLong()
            if (actual != expected) return null
            covered = maxOf(covered, upper)
        }
        return if (covered == rangeEndExclusive) ScalarRange(globalStart, globalEnd) else null
    }

    fun globalScalarForLocalUtf16(offset: Int, text: String): Int? {
        if (offset !in 0..text.length) return null
        return globalScalarForLocalScalar(PositionBridge.utf16ToScalar(offset, text))
    }

    fun globalScalarRangeForLocalUtf16(start: Int, end: Int, text: String): ScalarRange? {
        if (start < 0 || end < start || end > text.length) return null
        return globalScalarRange(
            PositionBridge.utf16ToScalar(start, text),
            PositionBridge.utf16ToScalar(end, text)
        )
    }

    fun isCurrent(currentRevision: String, currentEpoch: String): Boolean =
        binding.revision == currentRevision && binding.epoch == currentEpoch

    fun hasValidSegments(): Boolean = segments.isNotEmpty() && segments.all { segment ->
        segment.localScalarStart >= 0 &&
            segment.localScalarEndExclusive > segment.localScalarStart &&
            segment.globalScalarStart >= 0 &&
            hasRepresentableGlobalEnd(segment)
    }

    private fun matchesCurrent(currentRevision: String?, currentEpoch: String?): Boolean =
        (currentRevision == null || binding.revision == currentRevision) &&
            (currentEpoch == null || binding.epoch == currentEpoch)

    private fun add(start: Long, offset: Long): Int? {
        val value = start + offset
        return value.takeIf { it in Int.MIN_VALUE.toLong()..Int.MAX_VALUE.toLong() }?.toInt()
    }

    private fun hasRepresentableGlobalEnd(segment: Segment): Boolean =
        segment.globalScalarStart.toLong() +
            (segment.localScalarEndExclusive.toLong() - segment.localScalarStart.toLong() - 1L) <=
            Int.MAX_VALUE.toLong()
}
