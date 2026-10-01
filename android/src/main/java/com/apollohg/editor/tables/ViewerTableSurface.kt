package com.apollohg.editor.tables

import android.graphics.Rect
import android.graphics.RectF
import com.apollohg.editor.ProseViewerError
import com.apollohg.editor.viewer.INVALID_CELL_SOURCE_INDEX
import com.apollohg.editor.viewer.cellSemanticSourceIndex
import com.apollohg.editor.viewer.ProseLayoutKey
import com.apollohg.editor.viewer.promotedCodeHighlightBlocks
import com.apollohg.editor.viewer.PreparedProseLayout
import com.apollohg.editor.viewer.PreparedProseBlock
import com.apollohg.editor.viewer.PreparedProseInteraction
import com.apollohg.editor.viewer.PreparedProseAccessibilityNode
import com.apollohg.editor.viewer.PreparedViewerAtom
import com.apollohg.editor.viewer.ViewerImageAttachment

internal class PreparedViewerTableCell : TableGridCellPosition {
    override val sourceIndex: Int
    override val row: Int
    override val column: Int
    override val rowspan: Int
    override val colspan: Int
    val contentOrigin: Pair<Int, Int>
    val isHeader: Boolean
    val attributesKey: String?
    val layoutStore: TableCellLayoutStore
    val contentKey: ProseLayoutKey
    val contentWidthPx: Int
    val contentHeightPx: Int
    val contentError: ProseViewerError?
    val codeHighlightBlocks: List<com.apollohg.editor.CodeHighlightBlock>
    val highlightedCodeKeys: Set<String>
    val accessibilityText: String
    val hasNestedTables: Boolean
    val hasAtoms: Boolean
    val hasImages: Boolean
    val isPositionFree: Boolean
    val metadataRetainedBytes: Long
    private val prepareContent: () -> PreparedProseLayout

    constructor(
        sourceIndex: Int,
        row: Int,
        column: Int,
        rowspan: Int,
        colspan: Int,
        contentOrigin: Pair<Int, Int>,
        content: PreparedProseLayout,
        isHeader: Boolean,
        attributesKey: String?,
        layoutStore: TableCellLayoutStore = TableCellLayoutStore(),
        retainContent: Boolean = true,
        prepareContent: () -> PreparedProseLayout = { content }
    ) {
        this.sourceIndex = sourceIndex
        this.row = row
        this.column = column
        this.rowspan = rowspan
        this.colspan = colspan
        this.contentOrigin = contentOrigin
        this.isHeader = isHeader
        this.attributesKey = attributesKey
        this.layoutStore = layoutStore
        this.prepareContent = content.cellPreparation ?: prepareContent
        contentKey = content.key
        contentWidthPx = content.widthPx
        contentHeightPx = content.heightPx
        contentError = content.error
        codeHighlightBlocks = promotedCodeHighlightBlocks(content.blocks, content.codeHighlightBlocks)
        highlightedCodeKeys = content.highlightedCodeKeys + content.blocks.flatMap { block ->
            block.tableSurface?.cells.orEmpty().flatMap { it.highlightedCodeKeys }
        }
        accessibilityText = TableAccessibility.text(content).joinToString(TableAccessibility.LABEL_SEPARATOR)
        hasNestedTables = content.blocks.any { it.tableSurface != null }
        hasAtoms = content.viewerAtoms.isNotEmpty() || content.blocks.any { it.tableSurface?.hasAtoms == true }
        hasImages = content.imageAttachments.isNotEmpty() || content.blocks.any {
            it.imageAttachment != null || it.tableSurface?.cells?.any { cell -> cell.hasImages } == true
        }
        isPositionFree = content.error == null && !hasNestedTables && !hasAtoms && !hasImages &&
            content.interactions.all { it.docPos == null }
        metadataRetainedBytes = METADATA_RETAINED_BYTES +
            accessibilityText.length * 2L + codeHighlightBlocks.sumOf { CODE_DESCRIPTOR_RETAINED_BYTES + it.text.length * 2L } +
            highlightedCodeKeys.sumOf { it.length * 2L }
        if (retainContent || content.cellPreparation == null) layoutStore.insert(content)
    }


    private constructor(cell: PreparedViewerTableCell, position: TableGridCell, store: TableCellLayoutStore) {
        sourceIndex = position.sourceIndex
        row = position.row
        column = position.column
        rowspan = position.rowspan
        colspan = position.colspan
        contentOrigin = cell.contentOrigin
        isHeader = cell.isHeader
        attributesKey = cell.attributesKey
        layoutStore = store
        contentKey = cell.contentKey
        contentWidthPx = cell.contentWidthPx
        contentHeightPx = cell.contentHeightPx
        contentError = cell.contentError
        codeHighlightBlocks = cell.codeHighlightBlocks
        highlightedCodeKeys = cell.highlightedCodeKeys
        accessibilityText = cell.accessibilityText
        hasNestedTables = cell.hasNestedTables
        hasAtoms = cell.hasAtoms
        hasImages = cell.hasImages
        isPositionFree = cell.isPositionFree
        metadataRetainedBytes = cell.metadataRetainedBytes
        prepareContent = cell.prepareContent
        if (store !== cell.layoutStore) cell.cachedContent?.let { store.insert(it) }
    }

    fun relocated(position: TableGridCell, store: TableCellLayoutStore): PreparedViewerTableCell =
        PreparedViewerTableCell(this, position, store)

    val content: PreparedProseLayout get() = layoutStore.value(contentKey) {
        prepareContent().copy(cellPreparation = prepareContent)
    }
    val cachedContent: PreparedProseLayout? get() = layoutStore.peek(contentKey)
    val retainedBytes: Long get() = metadataRetainedBytes + (cachedContent?.retainedBytes ?: 0L)

    private companion object {
        const val METADATA_RETAINED_BYTES = 384L
        const val CODE_DESCRIPTOR_RETAINED_BYTES = 64L
    }
}

internal class ViewerTableSurface private constructor(
    val identity: String,
    val hostViewportWidth: Float,
    val style: TableStyle,
    val isRightToLeft: Boolean,
    val layout: TableLayoutResult,
    val cells: List<PreparedViewerTableCell>,
    val preparationError: ProseViewerError?,
    val sourceTable: TableSurfaceSource? = null,
    val sourceAttributes: Map<String, org.json.JSONObject> = emptyMap(),
    val editorTableId: String? = null,
    val displayScale: Float,
    reusableCellIndex: ViewerTableCellIndex?,
    reusableColumnEdgeHandleRows: Map<Int, Int>?,
    reusableCellKeyCertificate: Boolean?
) {
    constructor(
        identity: String, hostViewportWidth: Float, style: TableStyle, isRightToLeft: Boolean,
        layout: TableLayoutResult, cells: List<PreparedViewerTableCell>, preparationError: ProseViewerError?,
        sourceTable: TableSurfaceSource? = null,
        sourceAttributes: Map<String, org.json.JSONObject> = emptyMap(),
        editorTableId: String? = null, displayScale: Float = 1f
    ) : this(identity, hostViewportWidth, style, isRightToLeft, layout, cells, preparationError,
        sourceTable, sourceAttributes, editorTableId, displayScale, null, null, false)

    val layoutStore = cells.firstOrNull()?.layoutStore ?: TableCellLayoutStore()
    private val hasSingleLayoutStore = cells.all { it.layoutStore === layoutStore }
    private var retainedBytesRevision = -1L
    private var cellRetainedBytes = 0L

    val cachedContents: List<PreparedProseLayout> get() = if (hasSingleLayoutStore) {
        layoutStore.peekAll(cells.asSequence().map { it.contentKey })
    } else cells.mapNotNull { it.cachedContent }

    val cellShapeOwnerLayouts: List<PreparedProseLayout> get() = if (hasSingleLayoutStore) {
        layoutStore.residentLayouts
    } else cachedContents

    private data class Preparation(
        val layout: TableLayoutResult,
        val cells: List<PreparedViewerTableCell>,
        val error: ProseViewerError?
    )

    private constructor(
        identity: String, hostViewportWidth: Float, style: TableStyle, isRightToLeft: Boolean,
        prepared: Preparation, sourceTable: TableSurfaceSource?,
        sourceAttributes: Map<String, org.json.JSONObject>, editorTableId: String?, displayScale: Float
    ) : this(identity, hostViewportWidth, style, isRightToLeft, prepared.layout, prepared.cells,
        prepared.error, sourceTable, sourceAttributes, editorTableId, displayScale, null, null, null)

    constructor(
        identity: String, record: TableGridRecord, hostViewportWidth: Float, style: TableStyle,
        isRightToLeft: Boolean, displayScale: Float = 1f, themeDigest: String = "",
        fontEnvironmentRevision: Long = 0, textScale: Float = 1f,
        sourceTable: TableSurfaceSource? = null,
        sourceAttributes: Map<String, org.json.JSONObject> = emptyMap(),
        editorTableId: String? = null,
        layoutStore: TableCellLayoutStore = TableCellLayoutStore(),
        reuseCell: ((TableGridCell, Float) -> PreparedViewerTableCell?)? = null,
        prepareCellWorkers: List<(TableGridCell, Float) -> PreparedProseLayout> = emptyList(),
        parallelCellIndices: Set<Int> = emptySet(),
        transientCellIndices: Set<Int> = emptySet(),
        prepareCell: (TableGridCell, Float) -> PreparedProseLayout
    ) : this(identity, hostViewportWidth, style, isRightToLeft,
        prepare(record, hostViewportWidth, style, isRightToLeft, displayScale, themeDigest,
            fontEnvironmentRevision, textScale, sourceTable, layoutStore, reuseCell, prepareCellWorkers,
            parallelCellIndices, transientCellIndices, prepareCell),
        sourceTable, sourceAttributes, editorTableId, displayScale)

    // Geometry reuse requires the engine's certified physical-pixel layout and unchanged column inputs.
    fun replacingCells(contents: Map<Int, PreparedProseLayout>,
                       gridRecord: () -> TableGridRecord, sourceTable: TableSurfaceSource,
                       sourceAttributes: Map<String, org.json.JSONObject>,
                       reusePreparedGeometry: Boolean = false,
                       prepareCell: (TableGridCell, Float) -> PreparedProseLayout): ViewerTableSurface {
        var heightsUnchanged = true
        var membershipUnchanged = true
        var cellKeysCertified = hasCertifiedCellKeys
        fun replacing(cell: PreparedViewerTableCell): PreparedViewerTableCell {
            val content = contents[cell.sourceIndex] ?: return cell
            val source = sourceTable.cells[cell.sourceIndex]
            heightsUnchanged = heightsUnchanged && content.heightPx == cell.contentHeightPx
            val width = content.widthPx.toFloat()
            val gridCell = TableGridCell.from(source)
            return PreparedViewerTableCell(cell.sourceIndex, cell.row, cell.column, cell.rowspan, cell.colspan,
                cell.contentOrigin, content, source.header, source.attrsKey, layoutStore) { prepareCell(gridCell, width) }.also {
                membershipUnchanged = membershipUnchanged && cell.hasNestedTables == it.hasNestedTables && cell.hasAtoms == it.hasAtoms
                cellKeysCertified = cellKeysCertified && certifiesCellKey(it, sourceTable)
            }
        }
        val updated = if (cellIndex.bySourceIndex.size == cells.size) {
            val changed = contents.keys.mapNotNull { cellIndex.bySourceIndex[it] }.sorted()
            cells.toMutableList().also { result ->
                changed.forEach { index -> result[index] = replacing(cells[index]) }
            }
        } else cells.map(::replacing)
        val previousSource = this.sourceTable
        // The caller certifies unchanged cell positions, spans, and presentation inputs.
        val structureUnchanged = previousSource != null && layout.failure == null &&
            layout.typedFailure == null && previousSource.failure == null && sourceTable.failure == null &&
            previousSource.rows == sourceTable.rows && previousSource.columns == sourceTable.columns &&
            previousSource.columnWidths == sourceTable.columnWidths
        fun relayoutFromRecord(): TableLayoutResult {
            val heights = updated.associate { it.sourceIndex to it.contentHeightPx.toFloat() }
            return TableGridLayout(displayScale).relayout(gridRecord(), hostViewportWidth, style, isRightToLeft, heights)
        }
        var sourceIndex = 0
        val next = if (heightsUnchanged && structureUnchanged) {
            layout
        } else if (reusePreparedGeometry && structureUnchanged &&
            previousSource?.cells?.size == sourceTable.cells.size && updated.size == sourceTable.cells.size &&
            updated.all { cell ->
                val index = sourceIndex++
                val source = sourceTable.cells[index]
                cell.sourceIndex == index && source.sourceIndex == index &&
                    cell.row == source.row && cell.column == source.column &&
                    cell.rowspan == source.rowspan && cell.colspan == source.colspan
            }) {
            TableGridLayout(displayScale).relayoutPrepared(layout, sourceTable.rows, sourceTable.columns, style, isRightToLeft,
                updated, sourceTable.compatibilityDiagnostic) { relayoutFromRecord() }
        } else {
            relayoutFromRecord()
        }
        return ViewerTableSurface(identity, hostViewportWidth, style, isRightToLeft, next, updated,
            updated.firstNotNullOfOrNull { it.contentError }, sourceTable, sourceAttributes, editorTableId, displayScale,
            if (structureUnchanged && membershipUnchanged) cellIndex else null,
            if (structureUnchanged) columnEdgeHandleRows else null, cellKeysCertified)
    }

    val bounds: RectF get() = RectF(0f, 0f, layout.contentWidth, layout.contentHeight)
    val columnEdgeHandleRows: Map<Int, Int> = reusableColumnEdgeHandleRows ?: sourceTable?.cells.orEmpty()
        .groupBy { (it.column + it.colspan).toInt() - 1 }
        .mapValues { (_, cells) -> cells.minOf { it.row }.toInt() }
    val retainedBytes: Long get() {
        if (!hasSingleLayoutStore) return metadataRetainedBytes + cells.sumOf { it.cachedContent?.retainedBytes ?: 0L }
        return synchronized(layoutStore) {
            val revision = layoutStore.revision
            if (revision != retainedBytesRevision) {
                cellRetainedBytes = if (hasCertifiedCellKeys && layoutStore.count < cells.size) {
                    layoutStore.retainedBytesMatching { key ->
                        // Full equality checks the suffix against an already certified member key.
                        val sourceIndex = cellSemanticSourceIndex(key.semanticKey, validateContentHash = false)
                        if (sourceIndex == INVALID_CELL_SOURCE_INDEX) false else {
                            val member = cells.getOrNull(sourceIndex)?.takeIf { it.sourceIndex == sourceIndex }
                                ?: cell(sourceIndex)
                            member?.contentKey == key
                        }
                    }
                } else layoutStore.retainedBytes(cells.asSequence().map { it.contentKey })
                retainedBytesRevision = revision
            }
            metadataRetainedBytes + cellRetainedBytes
        }
    }
    val metadataRetainedBytes: Long = 256L + ACCOUNTING_CACHE_RETAINED_BYTES + cells.sumOf { it.metadataRetainedBytes } +
        (sourceTable?.cells?.size ?: 0) * 16L + layout.columnWidths.size * 16L +
        layout.columnOffsets.size * 16L + layout.rowOffsets.size * 16L + layout.rectangles.size * TABLE_RECTANGLE_RETAINED_BYTES + layout.sourceOrder.size * TABLE_SOURCE_ORDER_RETAINED_BYTES +
        columnEdgeHandleRows.size * 16L

    companion object {
        // Two Longs and the key certificate fit within the existing fixed allowance.
        private const val ACCOUNTING_CACHE_RETAINED_BYTES = 32L

        private fun certifiesCellKey(cell: PreparedViewerTableCell, source: TableSurfaceSource?): Boolean {
            val key = cell.contentKey.semanticKey
            val sourceCell = source?.cells?.getOrNull(cell.sourceIndex) ?: return false
            return sourceCell.sourceIndex == cell.sourceIndex &&
                cellSemanticSourceIndex(key) == cell.sourceIndex && key.endsWith(":" + sourceCell.contentKey)
        }

        private fun prepareParallelCells(
            inputs: List<Pair<TableGridCell, Int>>, scale: Float, indices: Set<Int>,
            workers: List<(TableGridCell, Float) -> PreparedProseLayout>,
            prepare: (TableGridCell, Float) -> PreparedProseLayout,
            capture: (TableGridCell, Float, PreparedProseLayout) -> PreparedViewerTableCell
        ): Map<Int, PreparedViewerTableCell> {
            val prepared = java.util.concurrent.ConcurrentHashMap<Int, PreparedViewerTableCell>()
            fun measure(input: Pair<TableGridCell, Int>, prepare: (TableGridCell, Float) -> PreparedProseLayout) {
                val (cell, pixels) = input
                val width = pixels / scale
                prepared[cell.sourceIndex] = capture(cell, width, prepare(cell, width))
            }
            inputs.filter { it.first.sourceIndex !in indices }.forEach { measure(it, prepare) }
            val executor = java.util.concurrent.Executors.newFixedThreadPool(workers.size)
            try {
                val tasks = workers.mapIndexed { worker, prepareWorker ->
                    java.util.concurrent.CompletableFuture.runAsync({
                        val start = inputs.size * worker / workers.size
                        val end = inputs.size * (worker + 1) / workers.size
                        for (index in start until end) {
                            if (inputs[index].first.sourceIndex in indices) measure(inputs[index], prepareWorker)
                        }
                    }, executor)
                }
                java.util.concurrent.CompletableFuture.allOf(*tasks.toTypedArray()).join()
            } finally {
                executor.shutdown()
            }
            return prepared
        }

        private fun prepare(
            record: TableGridRecord, hostViewportWidth: Float, style: TableStyle,
            isRightToLeft: Boolean, displayScale: Float, themeDigest: String,
            fontEnvironmentRevision: Long, textScale: Float, sourceTable: TableSurfaceSource?,
            layoutStore: TableCellLayoutStore,
            reuseCell: ((TableGridCell, Float) -> PreparedViewerTableCell?)?,
            prepareCellWorkers: List<(TableGridCell, Float) -> PreparedProseLayout>,
            parallelCellIndices: Set<Int>,
            transientCellIndices: Set<Int>,
            prepareCell: (TableGridCell, Float) -> PreparedProseLayout
        ): Preparation {
            val scale = displayScale.takeIf { it.isFinite() && it > 0f } ?: 1f
            val prepared = mutableMapOf<Int, PreparedViewerTableCell>()
            val inset = (style.cellPadding + style.borderWidth).toInt()
            fun capture(cell: TableGridCell, width: Float, content: PreparedProseLayout): PreparedViewerTableCell {
                val source = sourceTable?.cells?.getOrNull(cell.sourceIndex)
                return PreparedViewerTableCell(cell.sourceIndex, cell.row, cell.column, cell.rowspan, cell.colspan,
                    inset to inset, content, source?.header ?: false, source?.attrsKey, layoutStore,
                    retainContent = cell.sourceIndex !in transientCellIndices) { prepareCell(cell, width) }
            }
            fun prepare(cell: TableGridCell, width: Float): PreparedViewerTableCell =
                reuseCell?.invoke(cell, width)?.relocated(cell, layoutStore)
                    ?: capture(cell, width, prepareCell(cell, width))
            var error: ProseViewerError? = null
            val grid = TableGridLayout(scale)
            val layout = if (prepareCellWorkers.size > 1 && parallelCellIndices.isNotEmpty()) {
                prepared.putAll(prepareParallelCells(grid.measurementInputs(record, hostViewportWidth, style),
                    scale, parallelCellIndices, prepareCellWorkers, prepareCell, ::capture))
                error = record.cells.sortedBy { it.sourceIndex }.firstNotNullOfOrNull { prepared[it.sourceIndex]?.contentError }
                grid.relayout(record, hostViewportWidth, style, isRightToLeft,
                    prepared.mapValues { it.value.contentHeightPx.toFloat() })
            } else {
                val sourceCells = record.cells.associateBy { it.sourceIndex }
                val measurementRecord = record.copy(cells = record.cells.map { cell ->
                    cell.copy(contentKey = "${cell.contentKey}:${cell.sourceIndex}")
                })
                grid.layout(measurementRecord, hostViewportWidth, style, isRightToLeft, themeDigest, fontEnvironmentRevision, textScale) { measuredCell, width ->
                    val cell = sourceCells[measuredCell.sourceIndex] ?: return@layout null
                    prepare(cell, width).also { artifact ->
                        prepared[cell.sourceIndex] = artifact
                        if (error == null) error = artifact.contentError
                    }.contentHeightPx.toFloat()
                }
            }
            record.cells.forEach { cell ->
                if (cell.sourceIndex !in prepared) {
                    val frame = layout.rectangles[cell.sourceIndex] ?: return@forEach
                    val width = maxOf(0f, frame.width - 2f * (style.cellPadding + style.borderWidth))
                    val pixels = kotlin.math.round(width * scale)
                    if (pixels.isFinite() && pixels in 0f..Int.MAX_VALUE.toFloat()) {
                        prepare(cell, pixels.toInt() / scale).also { artifact ->
                            prepared[cell.sourceIndex] = artifact
                            if (error == null) error = artifact.contentError
                        }
                    }
                }
            }
            val cells = layout.sourceOrder.mapNotNull { prepared[it] }
            return Preparation(layout, cells, error)
        }
    }

    private val cellIndex = reusableCellIndex ?: ViewerTableCellIndex(cells, isRightToLeft)
    private val hasCertifiedCellKeys = reusableCellKeyCertificate ?: (
        hasSingleLayoutStore && cellIndex.bySourceIndex.size == cells.size &&
            cells.all { certifiesCellKey(it, sourceTable) })

    val nestedTableCells: List<PreparedViewerTableCell>
        get() = cellIndex.nestedTableCells.map { cells[it] }

    fun cell(sourceIndex: Int): PreparedViewerTableCell? =
        cellIndex.bySourceIndex[sourceIndex]?.let(cells::get)

    fun frameOfCell(sourceIndex: Int): TableCellRect? = cell(sourceIndex)?.let(::frameOfCell)

    fun frameOfCell(cell: PreparedViewerTableCell): TableCellRect = tableCellRect(
        cell.row, cell.column, cell.rowspan, cell.colspan, layout.columnOffsets, layout.rowOffsets,
        layout.contentWidth, isRightToLeft)

    fun cellsIntersecting(left: Float, top: Float, right: Float, bottom: Float): List<PreparedViewerTableCell> =
        cellIndex.indexesIntersecting(left, top, right, bottom, this).map { cells[it] }

    val hasAtoms: Boolean
        get() = cellIndex.atomCells.isNotEmpty()

    fun presentationCells(left: Float, top: Float, right: Float, bottom: Float): List<PreparedViewerTableCell> =
        (cellIndex.indexesIntersecting(left, top, right, bottom, this) + cellIndex.atomCells).toSortedSet()
            .map { cells[it] }

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
            surface.cells.filter { it.hasImages }.forEach { cell ->
                val frame = surface.frameOfCell(cell)
                append(cell.content, originX + frame.left.toInt() + cell.contentOrigin.first,
                    originY + frame.top.toInt() + cell.contentOrigin.second)
            }
        }
        appendSurface.invoke(this, tableOrigin.left, tableOrigin.top)
        return result
    }

    fun visibleCells(viewport: RectF, horizontalOffset: Float = 0f): List<PreparedViewerTableCell> {
        val left = viewport.left + horizontalOffset
        val right = viewport.right + horizontalOffset
        if (right <= left || viewport.bottom <= viewport.top) return cells
        return cellsIntersecting(left, viewport.top, right, viewport.bottom)
    }
}

private class ViewerTableCellIndex(cells: List<PreparedViewerTableCell>, rtl: Boolean) {
    private class Band(val row: Int, val cells: List<Int>)
    private val spanning = cells.indices.filter { cells[it].rowspan > 1 || cells[it].colspan > 1 }
    private val bands = cells.indices.filter { cells[it].rowspan == 1 && cells[it].colspan == 1 }
        .groupBy { cells[it].row }.toSortedMap().map { (row, indexes) ->
            Band(row, indexes.sortedBy { if (rtl) -cells[it].column else cells[it].column })
        }
    val nestedTableCells = cells.indices.filter { index -> cells[index].hasNestedTables }
    val atomCells = cells.indices.filter { index ->
        cells[index].hasAtoms
    }
    val bySourceIndex = cells.indices.reversed().associateBy { cells[it].sourceIndex }

    fun indexesIntersecting(left: Float, top: Float, right: Float, bottom: Float, surface: ViewerTableSurface): List<Int> {
        if (right <= left || bottom <= top) return emptyList()
        fun frame(index: Int) = surface.frameOfCell(surface.cells[index])
        fun intersects(frame: TableCellRect) = frame.left < right && frame.left + frame.width > left &&
            frame.top < bottom && frame.top + frame.height > top
        val result = spanning.filterTo(mutableListOf()) { intersects(frame(it)) }
        val firstBand = partition(bands.size) { surface.layout.rowOffsets[bands[it].row + 1] > top }
        for (bandIndex in firstBand until bands.size) {
            val band = bands[bandIndex]
            if (surface.layout.rowOffsets[band.row] >= bottom) break
            val firstCell = partition(band.cells.size) { frame(band.cells[it]).let { it.left + it.width > left } }
            for (cellIndex in firstCell until band.cells.size) {
                val index = band.cells[cellIndex]
                val cellFrame = frame(index)
                if (cellFrame.left >= right) break
                if (intersects(cellFrame)) result += index
            }
        }
        return result.sorted()
    }

    private fun partition(count: Int, isAfter: (Int) -> Boolean): Int {
        var low = 0
        var high = count
        while (low < high) {
            val middle = (low + high) ushr 1
            if (isAfter(middle)) high = middle else low = middle + 1
        }
        return low
    }
}

/** Mutable mounted state deliberately kept outside the immutable prepared surface. */
internal class ViewerTablePresentationOwner {
    companion object {
        private const val FIXED_RETAINED_BYTES = 48L
        private const val ENTRY_RETAINED_BYTES = 48L
    }

    private data class OffsetState(val logical: Float, val columnWidths: List<Float>)
    private val logicalOffsets = mutableMapOf<String, OffsetState>()
    private var pinnedStores = emptyMap<TableCellLayoutStore, Set<ProseLayoutKey>>()

    fun retainCells(cells: List<ViewerTablePresentedCell>) {
        val next = cells.groupBy { it.cell.layoutStore }.mapValues { (_, cells) -> cells.map { it.cell.contentKey }.toSet() }
        pinnedStores.forEach { (store, keys) -> (keys - next[store].orEmpty()).forEach(store::unpin) }
        next.forEach { (store, keys) -> (keys - pinnedStores[store].orEmpty()).forEach(store::pin) }
        cells.filter { it.cell.cachedContent == null }.forEach { it.cell.layoutStore.insert(it.content, it.cell.contentKey) }
        pinnedStores = next
    }

    fun clearPreparedCells() {
        pinnedStores.forEach { (store, keys) -> keys.forEach(store::unpin) }
        pinnedStores = emptyMap()
    }


    val retainedBytes: Long
        get() = FIXED_RETAINED_BYTES + logicalOffsets.entries.sumOf { entry ->
            ENTRY_RETAINED_BYTES + entry.key.length.toLong() * 2L +
                entry.value.columnWidths.size * Float.SIZE_BYTES
        }

    fun logicalOffset(surface: ViewerTableSurface): Float =
        clamp(logicalOffsets[surface.identity]?.logical ?: 0f, surface)

    fun setLogicalOffset(offset: Float, surface: ViewerTableSurface) {
        val clamped = clamp(offset, surface)
        if (clamped == 0f) logicalOffsets.remove(surface.identity)
        else logicalOffsets[surface.identity] = OffsetState(clamped, surface.layout.columnWidths)
    }

    fun reconcile(surfaces: List<ViewerTableSurface>) {
        val previous = logicalOffsets.toMap()
        logicalOffsets.clear()
        surfaces.distinctBy { it.identity }.forEach { surface ->
            val state = previous[surface.identity] ?: return@forEach
            val oldWidths = state.columnWidths
            var column = 0
            var oldPrefix = 0f
            while (column < oldWidths.lastIndex && state.logical >= oldPrefix + oldWidths[column]) {
                oldPrefix += oldWidths[column]
                column++
            }
            val within = state.logical - oldPrefix
            val next = surface.layout.columnWidths.take(column).sum() + within
            setLogicalOffset(next, surface)
        }
    }

    fun maximumOffset(surface: ViewerTableSurface): Float =
        (surface.bounds.width() - surface.hostViewportWidth).takeIf { it.isFinite() }?.coerceAtLeast(0f) ?: 0f

    fun canConsumePhysical(delta: Float, surface: ViewerTableSurface): Boolean {
        val offset = physicalOffset(surface)
        return if (delta > 0f) offset < maximumOffset(surface) else delta < 0f && offset > 0f
    }

    fun scrollPhysical(delta: Float, surface: ViewerTableSurface): Float {
        val before = physicalOffset(surface)
        val after = (before + delta).coerceIn(0f, maximumOffset(surface))
        setLogicalOffset(if (surface.isRightToLeft) maximumOffset(surface) - after else after, surface)
        return after - before
    }

    fun physicalOffset(surface: ViewerTableSurface): Float {
        val logical = logicalOffset(surface)
        val maximum = maximumOffset(surface)
        return if (surface.isRightToLeft) maximum - logical else logical
    }

    private fun clamp(offset: Float, surface: ViewerTableSurface): Float =
        if (!offset.isFinite()) 0f else offset.coerceIn(0f, maximumOffset(surface))

}

internal sealed interface ViewerTablePresentationViewport {
    data object Unknown : ViewerTablePresentationViewport
    data class Known(val rect: Rect) : ViewerTablePresentationViewport {
        val window: Rect?
            get() = rect.takeIf { it.width() > 0 && it.height() > 0 }?.let {
                Rect(it.left - it.width(), it.top - it.height(), it.right + it.width(), it.bottom + it.height())
            }
    }
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
    val sourceIndex: Int,
    val content: PreparedProseLayout,
    val bounds: RectF,
    val contentBounds: RectF,
    val clip: RectF
)

internal data class ViewerTablePresentedSurface(
    val surface: ViewerTableSurface,
    val bounds: RectF,
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
    val tables: List<ViewerTablePresentedSurface>,
    val cells: List<ViewerTablePresentedCell>,
    val mountedCells: List<ViewerTablePresentedCell>,
    val images: List<ViewerTablePresentedImage>,
    val atoms: List<ViewerTablePresentedAtom>,
    val interactions: List<ViewerTablePresentedInteraction>,
    val accessibilityNodes: List<ViewerTablePresentedAccessibilityNode>
)

/** One recursive coordinate seam for drawing, rich hits, media, atoms, and accessibility. */
internal object ViewerTablePresentation {
    fun present(
        cell: PreparedViewerTableCell,
        table: ViewerTablePresentedSurface,
        owner: ViewerTablePresentationOwner
    ): ViewerTablePresentedCell =
        present(cell, table.surface, table.bounds.left - owner.physicalOffset(table.surface), table.bounds.top, table.clip)

    private fun present(
        cell: PreparedViewerTableCell,
        surface: ViewerTableSurface,
        contentX: Float,
        contentY: Float,
        clip: RectF
    ): ViewerTablePresentedCell {
        val frame = surface.frameOfCell(cell)
        val cellBounds = RectF(
            contentX + frame.left,
            contentY + frame.top,
            contentX + frame.left + frame.width,
            contentY + frame.top + frame.height
        )
        val childX = cellBounds.left + cell.contentOrigin.first
        val childY = cellBounds.top + cell.contentOrigin.second
        val contentBounds = RectF(childX, childY, childX + cell.contentWidthPx, childY + cell.contentHeightPx)
        return ViewerTablePresentedCell(surface, cell, cell.sourceIndex, cell.content, cellBounds, contentBounds, clip)
    }

    fun surfaces(root: PreparedProseLayout): List<ViewerTableSurface> {
        val surfaces = mutableListOf<ViewerTableSurface>()
        fun visit(layout: PreparedProseLayout) {
            layout.blocks.mapNotNull { it.tableSurface }.forEach { surface ->
                surfaces += surface
                surface.nestedTableCells.forEach { visit(it.content) }
            }
        }
        visit(root)
        return surfaces
    }

    private val UNBOUNDED = RectF(-Float.MAX_VALUE, -Float.MAX_VALUE, Float.MAX_VALUE, Float.MAX_VALUE)

    private fun intersect(left: RectF, right: RectF): RectF = RectF(
        maxOf(left.left, right.left),
        maxOf(left.top, right.top),
        minOf(left.right, right.right),
        minOf(left.bottom, right.bottom)
    )

    private fun presentTable(
        surface: ViewerTableSurface,
        tableBounds: Rect,
        originX: Float,
        originY: Float,
        clip: RectF
    ): ViewerTablePresentedSurface {
        val hostX = originX + tableBounds.left
        val hostY = originY + tableBounds.top
        val hostWidth = minOf(surface.hostViewportWidth, surface.bounds.width())
        val bounds = RectF(hostX, hostY, hostX + hostWidth, hostY + surface.bounds.height())
        return ViewerTablePresentedSurface(surface, bounds, intersect(clip, bounds))
    }

    fun rootTables(root: PreparedProseLayout): List<ViewerTablePresentedSurface> = root.blocks.mapNotNull { block ->
        val surface = block.tableSurface ?: return@mapNotNull null
        val tableBounds = block.tableBounds ?: return@mapNotNull null
        presentTable(surface, tableBounds, 0f, 0f, UNBOUNDED)
    }

    fun contentAccessibilityNodes(
        cell: ViewerTablePresentedCell,
        owner: ViewerTablePresentationOwner
    ): List<ViewerTablePresentedAccessibilityNode> = project(
        cell.content, owner, null, cell.contentBounds.left, cell.contentBounds.top,
        intersect(cell.clip, cell.contentBounds)
    ).accessibilityNodes

    fun project(
        root: PreparedProseLayout,
        owner: ViewerTablePresentationOwner,
        viewport: ViewerTablePresentationViewport
    ): ViewerTablePresentationSnapshot {
        val window = when (viewport) {
            ViewerTablePresentationViewport.Unknown -> null
            is ViewerTablePresentationViewport.Known -> viewport.window ?: Rect()
        }
        return project(root, owner, window, 0f, 0f, UNBOUNDED).also { owner.retainCells(it.cells) }
    }

    private fun project(
        root: PreparedProseLayout,
        owner: ViewerTablePresentationOwner,
        window: Rect?,
        rootX: Float,
        rootY: Float,
        rootClip: RectF
    ): ViewerTablePresentationSnapshot {
        val layouts = mutableListOf<ViewerTablePresentedLayout>()
        val blocks = mutableListOf<ViewerTablePresentedBlock>()
        val tables = mutableListOf<ViewerTablePresentedSurface>()
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
                val table = presentTable(surface, tableBounds, originX, originY, clip)
                tables += table
                val hostY = table.bounds.top
                val hostClip = table.clip
                val contentX = table.bounds.left - owner.physicalOffset(surface)
                val windowCells = window?.let {
                    surface.presentationCells(it.left - contentX, it.top - hostY, it.right - contentX, it.bottom - hostY)
                } ?: surface.cells
                windowCells.forEach { cell ->
                    val presented = present(cell, surface, contentX, hostY, hostClip)
                    cells += presented
                    appendLayout(cell.content, presented.contentBounds.left, presented.contentBounds.top,
                        intersect(hostClip, presented.contentBounds))
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

        appendLayout(root, rootX, rootY, rootClip)
        val mountedCells = window?.let { candidate ->
            cells.filter {
                it.bounds.right > candidate.left && it.bounds.left < candidate.right &&
                    it.bounds.bottom > candidate.top && it.bounds.top < candidate.bottom
            }
        } ?: cells
        return ViewerTablePresentationSnapshot(layouts, blocks, tables, cells, mountedCells, images, atoms, interactions, accessibilityNodes)
    }
}
