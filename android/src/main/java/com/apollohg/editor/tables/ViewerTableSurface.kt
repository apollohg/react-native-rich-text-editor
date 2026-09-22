package com.apollohg.editor.tables

import android.graphics.Rect
import android.graphics.RectF
import com.apollohg.editor.ProseViewerError
import com.apollohg.editor.viewer.PreparedProseLayout
import com.apollohg.editor.viewer.PreparedProseBlock
import com.apollohg.editor.viewer.PreparedProseInteraction
import com.apollohg.editor.viewer.PreparedProseAccessibilityNode
import com.apollohg.editor.viewer.PreparedViewerAtom
import com.apollohg.editor.viewer.ViewerImageAttachment
import uniffi.editor_core.FfiViewerTable

internal data class PreparedViewerTableCell(
    val sourcePosition: Int,
    val frame: TableCellRect,
    val contentOrigin: Pair<Int, Int>,
    val content: PreparedProseLayout,
    val sourceCellIndex: Int?,
    val isHeader: Boolean,
    val attributesKey: String?
) {
    val retainedBytes: Long get() = 96L + content.retainedBytes
}

/** Immutable table preparation; mounted scrolling state belongs to the drawing view. */
internal class ViewerTableSurface(
    val identity: String,
    record: TableGridRecord,
    val hostViewportWidth: Float,
    val style: TableStyle,
    val isRightToLeft: Boolean,
    displayScale: Float = 1f,
    themeDigest: String = "",
    fontEnvironmentRevision: Long = 0,
    textScale: Float = 1f,
    val sourceTable: FfiViewerTable? = null,
    val sourceAttributes: Map<String, org.json.JSONObject> = emptyMap(),
    prepareCell: (TableGridCell, Float) -> PreparedProseLayout
) {
    val layout: TableLayoutResult
    val cells: List<PreparedViewerTableCell>
    val preparationError: ProseViewerError?
    val bounds: RectF get() = RectF(0f, 0f, layout.contentWidth, layout.contentHeight)
    val retainedBytes: Long get() = 256L + cells.sumOf { it.retainedBytes } +
        (sourceTable?.cells?.size ?: 0) * 16L + layout.columnWidths.size * 16L +
        layout.rowOffsets.size * 16L + layout.rectangles.size * 48L + layout.sourceOrder.size * 16L

    init {
        val scale = displayScale.takeIf { it.isFinite() && it > 0f } ?: 1f
        val indexes = sourceTable?.cells?.mapIndexed { index, cell -> cell.sourcePos.toInt() to index }?.toMap().orEmpty()
        val sourceCells = record.cells.associateBy { it.sourcePosition }
        val measurementRecord = record.copy(cells = record.cells.map { cell ->
            cell.copy(contentKey = "${cell.contentKey}:${cell.sourcePosition}")
        })
        val prepared = mutableMapOf<Int, PreparedProseLayout>()
        var error: ProseViewerError? = null
        layout = TableGridLayout(scale).layout(measurementRecord, hostViewportWidth, style, isRightToLeft, themeDigest, fontEnvironmentRevision, textScale) { measuredCell, width ->
            val cell = sourceCells[measuredCell.sourcePosition] ?: return@layout null
            prepareCell(cell, width).also { artifact ->
                prepared[cell.sourcePosition] = artifact
                if (error == null) error = artifact.error
            }.heightPx.toFloat()
        }
        record.cells.forEach { cell ->
            if (cell.sourcePosition !in prepared) {
                val frame = layout.rectangles[cell.sourcePosition] ?: return@forEach
                val width = maxOf(0f, frame.width - 2f * (style.cellPadding + style.borderWidth))
                val pixels = kotlin.math.round(width * scale)
                if (pixels.isFinite() && pixels in 0f..Int.MAX_VALUE.toFloat()) {
                    prepareCell(cell, pixels.toInt() / scale).also { artifact ->
                        prepared[cell.sourcePosition] = artifact
                        if (error == null) error = artifact.error
                    }
                }
            }
        }
        preparationError = error
        val inset = (style.cellPadding + style.borderWidth).toInt()
        cells = layout.sourceOrder.mapNotNull { position ->
            val frame = layout.rectangles[position] ?: return@mapNotNull null
            val content = prepared[position] ?: return@mapNotNull null
            val sourceIndex = indexes[position]
            val source = sourceIndex?.let { sourceTable?.cells?.get(it) }
            PreparedViewerTableCell(position, frame, inset to inset, content, sourceIndex, source?.header ?: false, source?.attrsKey)
        }
    }

    fun parentImageAttachments(offset: Int, tableOrigin: Rect): List<ViewerImageAttachment> {
        var ordinal = offset
        val result = mutableListOf<ViewerImageAttachment>()
        lateinit var appendSurface: (ViewerTableSurface, Int, Int) -> Unit
        fun append(layout: PreparedProseLayout, originX: Int, originY: Int) {
            val byId = layout.imageAttachments.associateBy { it.id }
            val emitted = mutableSetOf<String>()
            fun emit(attachment: ViewerImageAttachment) {
                if (!emitted.add(attachment.id)) return
                result += attachment.copy(ordinal = ordinal++, bounds = Rect(attachment.bounds).apply { offset(originX, originY) })
            }
            layout.blocks.forEach { block ->
                block.imageAttachment?.let { byId[it.id]?.let(::emit) }
                block.tableSurface?.let { nested ->
                    val frame = block.tableBounds ?: block.bounds
                    appendSurface(nested, originX + frame.left, originY + frame.top)
                }
            }
            layout.imageAttachments.forEach(::emit)
        }
        appendSurface = { surface, originX, originY ->
            surface.cells.forEach { cell ->
                append(cell.content, originX + cell.frame.left.toInt() + cell.contentOrigin.first,
                    originY + cell.frame.top.toInt() + cell.contentOrigin.second)
            }
        }
        appendSurface.invoke(this, tableOrigin.left, tableOrigin.top)
        return result
    }

    fun visibleCells(viewport: RectF, horizontalOffset: Float = 0f): List<PreparedViewerTableCell> {
        val left = viewport.left + horizontalOffset
        val right = viewport.right + horizontalOffset
        if (right <= left || viewport.bottom <= viewport.top) return cells
        return cells.filter { it.frame.left < right && it.frame.left + it.frame.width > left && it.frame.top < viewport.bottom && it.frame.top + it.frame.height > viewport.top }
    }
}

/** Mutable mounted state deliberately kept outside the immutable prepared surface. */
internal class ViewerTablePresentationOwner {
    companion object {
        private const val FIXED_RETAINED_BYTES = 48L
        private const val ENTRY_RETAINED_BYTES = 48L
    }

    private val logicalOffsets = mutableMapOf<String, Float>()

    val retainedBytes: Long
        get() = FIXED_RETAINED_BYTES + logicalOffsets.entries.sumOf { entry ->
            ENTRY_RETAINED_BYTES + entry.key.length.toLong() * 2L
        }

    fun logicalOffset(surface: ViewerTableSurface): Float = logicalOffsets[surface.identity] ?: 0f

    fun setLogicalOffset(offset: Float, surface: ViewerTableSurface) {
        logicalOffsets[surface.identity] = clamp(offset, surface)
    }

    fun physicalOffset(surface: ViewerTableSurface): Float {
        val logical = logicalOffset(surface)
        val maximum = maximumOffset(surface)
        return if (surface.isRightToLeft) maximum - logical else logical
    }

    private fun clamp(offset: Float, surface: ViewerTableSurface): Float =
        if (!offset.isFinite()) 0f else offset.coerceIn(0f, maximumOffset(surface))

    private fun maximumOffset(surface: ViewerTableSurface): Float =
        (surface.bounds.width() - surface.hostViewportWidth).takeIf { it.isFinite() }?.coerceAtLeast(0f) ?: 0f
}

internal sealed interface ViewerTablePresentationViewport {
    data object Unknown : ViewerTablePresentationViewport
    data class Known(val rect: Rect) : ViewerTablePresentationViewport
}

internal data class ViewerTablePresentedBlock(
    val layout: PreparedProseLayout,
    val block: PreparedProseBlock,
    val originX: Float,
    val originY: Float,
    val clip: RectF
)

/** One entry per immutable layout reached by the mounted traversal. */
internal data class ViewerTablePresentedLayout(
    val layout: PreparedProseLayout,
    val originX: Float,
    val originY: Float,
    val clip: RectF
)

internal data class ViewerTablePresentedCell(
    val surface: ViewerTableSurface,
    val cell: PreparedViewerTableCell,
    val sourcePosition: Int,
    val content: PreparedProseLayout,
    val bounds: RectF,
    val contentBounds: RectF,
    val clip: RectF
)

internal data class ViewerTablePresentedImage(
    /** The root attachment remains the publication owner; this is mounted geometry only. */
    val attachment: ViewerImageAttachment,
    val sourceIdentity: String,
    val bounds: RectF,
    val clip: RectF,
    val layout: PreparedProseLayout,
    val block: PreparedProseBlock?
)

internal data class ViewerTablePresentedAtom(
    val atom: PreparedViewerAtom,
    val sourceIdentity: String,
    val bounds: RectF,
    val clip: RectF,
    val layout: PreparedProseLayout,
    val block: PreparedProseBlock?
)

internal data class ViewerTablePresentedInteraction(
    val interaction: PreparedProseInteraction,
    val sourceIdentity: String,
    val rects: List<RectF>,
    val clip: RectF,
    val layout: PreparedProseLayout
)

internal data class ViewerTablePresentedAccessibilityNode(
    val node: PreparedProseAccessibilityNode,
    val sourceIdentity: String,
    val interactionSourceIdentity: String?,
    val bounds: RectF,
    val clip: RectF,
    val layout: PreparedProseLayout
)

internal data class ViewerTablePresentationSnapshot(
    val layouts: List<ViewerTablePresentedLayout>,
    val blocks: List<ViewerTablePresentedBlock>,
    val cells: List<ViewerTablePresentedCell>,
    val mountedCells: List<ViewerTablePresentedCell>,
    val images: List<ViewerTablePresentedImage>,
    val atoms: List<ViewerTablePresentedAtom>,
    val interactions: List<ViewerTablePresentedInteraction>,
    val accessibilityNodes: List<ViewerTablePresentedAccessibilityNode>
)

/** One recursive coordinate seam for drawing, rich hits, media, atoms, and accessibility. */
internal object ViewerTablePresentation {
    fun project(
        root: PreparedProseLayout,
        owner: ViewerTablePresentationOwner,
        viewport: ViewerTablePresentationViewport
    ): ViewerTablePresentationSnapshot {
        val layouts = mutableListOf<ViewerTablePresentedLayout>()
        val blocks = mutableListOf<ViewerTablePresentedBlock>()
        val cells = mutableListOf<ViewerTablePresentedCell>()
        val images = mutableListOf<ViewerTablePresentedImage>()
        val atoms = mutableListOf<ViewerTablePresentedAtom>()
        val interactions = mutableListOf<ViewerTablePresentedInteraction>()
        val accessibilityNodes = mutableListOf<ViewerTablePresentedAccessibilityNode>()
        val canonicalAttachments = root.imageAttachments.associateBy { it.id }
        val emittedImages = mutableSetOf<String>()

        fun shifted(rect: Rect, x: Float, y: Float) = RectF(
            rect.left + x,
            rect.top + y,
            rect.right + x,
            rect.bottom + y
        )

        fun intersects(left: RectF, right: Rect): Boolean =
            left.right > right.left && left.left < right.right && left.bottom > right.top && left.top < right.bottom

        fun intersect(left: RectF, right: RectF): RectF = RectF(
            maxOf(left.left, right.left),
            maxOf(left.top, right.top),
            minOf(left.right, right.right),
            minOf(left.bottom, right.bottom)
        )

        fun appendLayout(layout: PreparedProseLayout, originX: Float, originY: Float, clip: RectF) {
            layouts += ViewerTablePresentedLayout(layout, originX, originY, RectF(clip))
            val emittedInteractions = mutableSetOf<Int>()
            val emittedAccessibility = mutableSetOf<Int>()
            val interactionsByBlock = layout.interactions.indices.mapNotNull { index ->
                layout.interactions[index].sourceBlockIndex?.let { it to index }
            }.groupBy({ it.first }, { it.second })
            val accessibilityByBlock = layout.accessibilityNodes.indices.mapNotNull { index ->
                layout.accessibilityNodes[index].sourceBlockIndex?.let { it to index }
            }.groupBy({ it.first }, { it.second })

            fun appendInteraction(index: Int) {
                if (index !in layout.interactions.indices || !emittedInteractions.add(index)) return
                val interaction = layout.interactions[index]
                interactions += ViewerTablePresentedInteraction(
                    interaction,
                    "${layout.key.semanticKey}:interaction:$index",
                    interaction.rects.map { shifted(it, originX, originY) },
                    RectF(clip),
                    layout
                )
            }

            fun appendAccessibility(index: Int) {
                if (index !in layout.accessibilityNodes.indices || !emittedAccessibility.add(index)) return
                val node = layout.accessibilityNodes[index]
                accessibilityNodes += ViewerTablePresentedAccessibilityNode(
                    node,
                    "${layout.key.semanticKey}:accessibility:$index",
                    "${layout.key.semanticKey}:interaction:${node.interactionIndex}".takeIf { node.interactionIndex in layout.interactions.indices },
                    shifted(node.bounds, originX, originY),
                    RectF(clip),
                    layout
                )
            }

            layout.blocks.forEachIndexed { blockIndex, block ->
                blocks += ViewerTablePresentedBlock(layout, block, originX, originY, RectF(clip))
                block.imageAttachment?.let { attachment ->
                    if (emittedImages.add(attachment.id)) {
                        images += ViewerTablePresentedImage(
                            canonicalAttachments[attachment.id] ?: attachment,
                            "${layout.key.semanticKey}:image:${attachment.id}",
                            shifted(attachment.bounds, originX, originY),
                            RectF(clip),
                            layout,
                            block
                        )
                    }
                }
                layout.viewerAtoms.filter { atom ->
                    atom.bounds.left >= block.bounds.left && atom.bounds.right <= block.bounds.right &&
                        atom.bounds.top >= block.bounds.top && atom.bounds.bottom <= block.bounds.bottom
                }.forEach { atom ->
                    atoms += ViewerTablePresentedAtom(
                        atom,
                        "${layout.key.semanticKey}:atom:${atom.nodeType}:${atom.docPos}",
                        shifted(atom.bounds, originX, originY),
                        RectF(clip),
                        layout,
                        block
                    )
                }
                interactionsByBlock[blockIndex].orEmpty().forEach(::appendInteraction)
                accessibilityByBlock[blockIndex].orEmpty().forEach(::appendAccessibility)

                val surface = block.tableSurface ?: return@forEachIndexed
                val tableBounds = block.tableBounds ?: return@forEachIndexed
                val hostX = originX + tableBounds.left
                val hostY = originY + tableBounds.top
                val hostWidth = minOf(surface.hostViewportWidth, surface.bounds.width())
                val hostClip = intersect(clip, RectF(hostX, hostY, hostX + hostWidth, hostY + surface.bounds.height()))
                val contentX = hostX - owner.physicalOffset(surface)
                surface.cells.forEach { cell ->
                    val cellBounds = RectF(
                        contentX + cell.frame.left,
                        hostY + cell.frame.top,
                        contentX + cell.frame.left + cell.frame.width,
                        hostY + cell.frame.top + cell.frame.height
                    )
                    val childX = cellBounds.left + cell.contentOrigin.first
                    val childY = cellBounds.top + cell.contentOrigin.second
                    val contentBounds = RectF(childX, childY, childX + cell.content.widthPx, childY + cell.content.heightPx)
                    cells += ViewerTablePresentedCell(
                        surface,
                        cell,
                        cell.sourcePosition,
                        cell.content,
                        cellBounds,
                        contentBounds,
                        hostClip
                    )
                    appendLayout(cell.content, childX, childY, intersect(hostClip, contentBounds))
                }
            }
            layout.imageAttachments.filter { emittedImages.add(it.id) }.forEach { attachment ->
                images += ViewerTablePresentedImage(
                    canonicalAttachments[attachment.id] ?: attachment,
                    "${layout.key.semanticKey}:image:${attachment.id}",
                    shifted(attachment.bounds, originX, originY),
                    RectF(clip),
                    layout,
                    null
                )
            }
            layout.interactions.indices.forEach(::appendInteraction)
            layout.accessibilityNodes.indices.forEach(::appendAccessibility)
        }

        appendLayout(root, 0f, 0f, RectF(-Float.MAX_VALUE, -Float.MAX_VALUE, Float.MAX_VALUE, Float.MAX_VALUE))
        val mountedCells = when (viewport) {
            ViewerTablePresentationViewport.Unknown -> cells
            is ViewerTablePresentationViewport.Known -> {
                val rect = viewport.rect
                if (rect.width() <= 0 || rect.height() <= 0) emptyList() else {
                    val candidate = Rect(
                        rect.left - rect.width(),
                        rect.top - rect.height(),
                        rect.right + rect.width(),
                        rect.bottom + rect.height()
                    )
                    cells.filter { intersects(it.bounds, candidate) }
                }
            }
        }
        return ViewerTablePresentationSnapshot(layouts, blocks, cells, mountedCells, images, atoms, interactions, accessibilityNodes)
    }
}
