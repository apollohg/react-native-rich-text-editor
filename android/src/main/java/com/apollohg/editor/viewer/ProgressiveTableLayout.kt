package com.apollohg.editor.viewer

import android.graphics.Rect
import android.view.View
import com.apollohg.editor.ProseViewerError
import com.apollohg.editor.tables.ViewerTableSurface
import java.util.concurrent.atomic.AtomicLong

internal enum class TableGeometryPolicy(val stateValue: Int) {
    INITIAL(0),
    STAGED(1),
    EAGER(2)
}

internal class TableGeometryRevisionRequired(val widthPx: Int, val heightPx: Int) :
    RuntimeException("The exact progressive geometry was evicted; a new Yoga revision is required.")

private val tableGeometryRevision = AtomicLong()
private const val MAXIMUM_EXACT_STATE_REVISION = (1L shl 53) - 1L

internal fun nextTableGeometryRevision(): Long = tableGeometryRevision.updateAndGet {
    check(it < MAXIMUM_EXACT_STATE_REVISION) { "Table geometry revision space exhausted." }
    it + 1
}

internal class ProgressiveTableAnchor private constructor(
    private val blockIndex: Int,
    private val tableIdentity: String?,
    private val row: Int,
    private val offset: Int,
    private val original: Int
) {
    fun resolve(layout: PreparedProseLayout): Int {
        val block = layout.blocks.getOrNull(blockIndex) ?: return original
        val surface = block.tableSurface
        val bounds = block.tableBounds
        return if (surface != null && bounds != null && surface.identity == tableIdentity) {
            bounds.top + (surface.layout.rowOffsets.getOrNull(row)?.toInt() ?: 0) + offset
        } else {
            block.bounds.top + offset
        }
    }

    companion object {
        fun capture(layout: PreparedProseLayout, top: Int): ProgressiveTableAnchor {
            val tableIndex = layout.blocks.indexOfFirst { block ->
                block.tableBounds?.let { top >= it.top && top < it.bottom } == true
            }
            if (tableIndex >= 0) {
                val block = layout.blocks[tableIndex]
                val surface = requireNotNull(block.tableSurface)
                val local = top - requireNotNull(block.tableBounds).top
                val row = surface.layout.rowOffsets.indexOfLast { it <= local }.coerceAtLeast(0)
                return ProgressiveTableAnchor(
                    tableIndex,
                    surface.identity,
                    row,
                    local - surface.layout.rowOffsets[row].toInt(),
                    top
                )
            }
            val index = layout.blocks.indexOfLast { it.bounds.top <= top }.coerceAtLeast(0)
            return ProgressiveTableAnchor(
                index,
                null,
                0,
                top - (layout.blocks.getOrNull(index)?.bounds?.top ?: 0),
                top
            )
        }
    }
}

internal fun reflowTableSurfaces(
    layout: PreparedProseLayout,
    replacements: Map<String, ViewerTableSurface>
): PreparedProseLayout {
    data class Change(val top: Int, val bottom: Int, val height: Int) {
        val delta: Long get() = height.toLong() - (bottom.toLong() - top)
    }
    val changes = layout.blocks.mapNotNull { block ->
        val previous = block.tableSurface ?: return@mapNotNull null
        val next =
            replacements[previous.identity]?.takeIf { it !== previous } ?: return@mapNotNull null
        val bounds = requireNotNull(block.tableBounds)
        require(
            next.identity == previous.identity &&
                next.hostViewportWidth == previous.hostViewportWidth
        )
        Change(bounds.top, bounds.bottom, next.layout.contentHeight.toInt())
    }.sortedBy { it.top }
    if (changes.isEmpty()) return layout
    fun y(value: Int): Int {
        var result = value.toLong()
        for (change in changes) {
            if (value >= change.bottom) result += change.delta
        }
        if (result !in Int.MIN_VALUE.toLong()..Int.MAX_VALUE.toLong()) {
            throw ProseViewerError.layout(
                "Deferred table geometry exceeds Android coordinate bounds."
            )
        }
        return result.toInt()
    }
    fun rect(value: Rect): Rect = Rect(value.left, y(value.top), value.right, y(value.bottom))
    val height = y(layout.heightPx)
    if (height !in 0..View.MEASURED_SIZE_MASK) {
        throw ProseViewerError.layout("Deferred document height exceeds Android layout bounds.")
    }
    val blocks = layout.blocks.map { block ->
        block.copy(
            bounds = rect(block.bounds),
            tableBounds = block.tableBounds?.let(::rect),
            tableSurface = block.tableSurface?.let { replacements[it.identity] ?: it },
            imageAttachment = block.imageAttachment?.let { it.copy(bounds = rect(it.bounds)) },
            fragments = block.fragments.map { fragment ->
                fragment.copy(
                    bounds = rect(fragment.bounds),
                    layoutY = y(fragment.layoutY),
                    labelY = y(fragment.labelY),
                    decorationBounds = fragment.decorationBounds?.let(::rect)
                )
            }
        )
    }
    val nextTableBytes = blocks.sumOf { it.tableSurface?.retainedBytes ?: 0L }
    return layout.copy(
        key = layout.key.copy(tableGeometryRevision = nextTableGeometryRevision()),
        heightPx = height, blocks = blocks,
        interactions = layout.interactions.map { it.copy(rects = it.rects.map(::rect)) },
        accessibilityNodes = layout.accessibilityNodes.map { it.copy(bounds = rect(it.bounds)) },
        imageAttachments = layout.imageAttachments.map { it.copy(bounds = rect(it.bounds)) },
        viewerAtoms = layout.viewerAtoms.map { it.copy(bounds = rect(it.bounds)) },
        retainedBytes = layout.nonTableRetainedBytes + nextTableBytes,
        tableRetainedBytesAtPreparation = nextTableBytes
    )
}
