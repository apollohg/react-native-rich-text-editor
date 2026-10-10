package com.apollohg.editor.viewer

import android.content.Context
import android.content.res.Configuration
import android.graphics.Canvas
import android.graphics.Color
import android.graphics.Paint
import android.graphics.Rect
import android.graphics.RectF
import android.graphics.RenderNode
import android.os.Build
import android.os.Bundle
import android.util.AttributeSet
import android.view.MotionEvent
import android.view.View
import android.view.ViewConfiguration
import android.view.ViewTreeObserver
import android.view.accessibility.AccessibilityEvent
import android.view.accessibility.AccessibilityManager
import android.view.accessibility.AccessibilityNodeInfo
import android.view.accessibility.AccessibilityNodeProvider
import androidx.core.view.NestedScrollingChild3
import androidx.core.view.NestedScrollingChildHelper
import androidx.core.view.ViewCompat
import androidx.core.view.accessibility.AccessibilityNodeInfoCompat
import com.apollohg.editor.AndroidApiCompat
import com.apollohg.editor.DecodedBitmapBudget
import com.apollohg.editor.DecodedBitmapLease
import com.apollohg.editor.tables.PreparedViewerTableCell
import com.apollohg.editor.tables.TableAccessibility
import com.apollohg.editor.tables.TableAccessibilityCell
import com.apollohg.editor.tables.TableAccessibilityEditing
import com.apollohg.editor.tables.TableAccessibilityItem
import com.apollohg.editor.tables.TableAccessibilityLocation
import com.apollohg.editor.tables.TableAccessibilityNodes
import com.apollohg.editor.tables.TableInteractionController
import com.apollohg.editor.tables.TableSurfaceSource
import com.apollohg.editor.tables.ViewerTablePresentation
import com.apollohg.editor.tables.ViewerTablePresentationOwner
import com.apollohg.editor.tables.ViewerTablePresentationSnapshot
import com.apollohg.editor.tables.ViewerTablePresentationViewport
import com.apollohg.editor.tables.ViewerTablePresentedAccessibilityNode
import com.apollohg.editor.tables.ViewerTablePresentedBlock
import com.apollohg.editor.tables.ViewerTablePresentedCell
import com.apollohg.editor.tables.ViewerTablePresentedSurface
import com.apollohg.editor.tables.ViewerTableSurface
import java.util.Collections
import java.util.IdentityHashMap
import kotlin.math.ceil
import kotlin.math.floor
import kotlin.math.pow
import org.json.JSONArray
import org.json.JSONObject

internal enum class TableSelectionHandleRole { ANCHOR, HEAD }

internal data class TableSelectionHandle(
    val role: TableSelectionHandleRole,
    val tableId: String,
    val sourcePosition: Int,
    val x: Float,
    val y: Float
)

internal data class RemoteTableCellSelection(
    val tableId: String,
    val sourceIndices: Set<Int>,
    val color: Int
)

internal data class TableResizeEdge(val tableId: String, val column: Int)

internal data class TableCellDropTarget(val tableId: String, val sourceIndex: Int)

/** Rendering-only consumer of fully prepared StaticLayout and geometry fragments. */
internal class PreparedProseDrawingView @JvmOverloads constructor(
    context: Context,
    attrs: AttributeSet? = null
) : View(context, attrs),
    NestedScrollingChild3 {
    private val accessibilityManager = context.getSystemService(AccessibilityManager::class.java)
    var preparedLayout: PreparedProseLayout? = null
        private set
    internal var usesEditAnchoredNodes = false
    internal var preparesTableCellsBeforeDrawing = false
    internal var onPrepareTableGeometry: ((Rect) -> Boolean)? = null
    internal var onPrepareTableCellGeometry: ((String, Int) -> Boolean)? = null
    private val prepareTableCellsListener = ViewTreeObserver.OnPreDrawListener {
        val viewport = (presentationViewport() as? ViewerTablePresentationViewport.Known)?.rect
        if (viewport != null && onPrepareTableGeometry?.invoke(viewport) == true) {
            invalidate()
            false
        } else {
            preparePresentedTableCells()
            updateSelectionGestureExclusionRects()
            true
        }
    }

    private fun preparePresentedTableCells() {
        if (preparesTableCellsBeforeDrawing) presentationSnapshot()
    }

    internal var tableNodeRevision: ULong? = null
    internal var tableNodeChanges: com.apollohg.editor.tables.TableFrameChanges? = null
    internal var tableNodeAppearance = ""
    internal val aboveNode = if (android.os.Build.VERSION.SDK_INT >=
        android.os.Build.VERSION_CODES.Q
    ) {
        RenderNode(NODE_ABOVE)
    } else {
        null
    }
    internal val boundRowNode = if (android.os.Build.VERSION.SDK_INT >=
        android.os.Build.VERSION_CODES.Q
    ) {
        RenderNode(NODE_BOUND_ROW)
    } else {
        null
    }
    internal val boundCellNode = if (android.os.Build.VERSION.SDK_INT >=
        android.os.Build.VERSION_CODES.Q
    ) {
        RenderNode(NODE_BOUND_CELL)
    } else {
        null
    }
    internal val belowNode = if (android.os.Build.VERSION.SDK_INT >=
        android.os.Build.VERSION_CODES.Q
    ) {
        RenderNode(NODE_BELOW)
    } else {
        null
    }
    internal val nodeRecordsForTesting = mutableMapOf<String, Int>()
    private data class TableNodeState(
        val cell: Pair<String, Int>?,
        val window: Rect,
        val frame: RectF,
        val row: RectF,
        val rowOffsets: List<Float>,
        val revision: ULong?,
        val appearance: String
    )
    private var tableNodeState: TableNodeState? = null
    private val tableNodes get() = listOfNotNull(aboveNode, boundRowNode, boundCellNode, belowNode)

    private fun clearTableNodes() {
        tableNodeState = null
        if (android.os.Build.VERSION.SDK_INT >= android.os.Build.VERSION_CODES.Q) {
            tableNodes.forEach {
                it.discardDisplayList()
                it.translationY = 0f
            }
        }
    }

    var onCodeHighlightsReady: (() -> Unit)? = null
    private val codeHighlighting = ViewerCodeHighlighting(this)
    var onUsableMetricsChanged: (() -> Unit)? = null
    var onVisibleRectChanged: ((Rect) -> Unit)? = null
    var onVisibleImagesChanged: ((Rect, List<ViewerImageAttachment>) -> Unit)? = null
    var onFontConfigurationChanged: ((Configuration) -> Unit)? = null
    var onInteractionActivated: ((PreparedProseInteraction) -> Boolean)? = null
    internal var onTableTap: ((Float, Float, Float, Float) -> Boolean)? = null
    var onTableGeometryChanged: (() -> Unit)? = null
    internal var onMountedTableCellsDrawnForTesting: ((Int) -> Unit)? = null
    internal var onTableChromeDrawnForTesting: ((Int) -> Unit)? = null
    internal var onTableRichFragmentDrawnForTesting: (() -> Unit)? = null
    internal var tableDocumentPosition: ((String) -> Int?)? = null
    internal var tableCellDocumentPosition: ((String, Int) -> Int?)? = null
    internal var suppressedTableCell: Pair<String, Int>? = null
        set(value) {
            if (field == value) return
            field = value
            tableNodeState = null
            invalidate()
        }
    internal var selectedTableCellSourceIndices: Map<String, Set<Int>> = emptyMap()
        set(value) {
            if (field == value) return
            field = value
            tableNodeState = null
            invalidate()
        }
    internal var selectedTableCellEndpoints: Triple<String, Int, Int>? = null
        set(value) {
            if (field == value) return
            field = value
            tableNodeState = null
            invalidate()
        }
    internal var remoteTableCellSelections: List<RemoteTableCellSelection> = emptyList()
        set(value) {
            if (field == value) return
            field = value
            tableNodeState = null
            invalidate()
        }
    internal var activeTableResizeEdge: TableResizeEdge? = null
        set(value) {
            if (field == value) return
            field = value
            tableNodeState = null
            invalidate()
        }
    internal var tableCellDropTarget: TableCellDropTarget? = null
        set(value) {
            if (field == value) return
            field = value
            tableNodeState = null
            invalidate()
        }
    private var tablePresentationOwner = ViewerTablePresentationOwner()
    private var replacementTablePresentationOwner: ViewerTablePresentationOwner? = null
    private var replacementTableSemanticOwner: String? = null
    private val nestedScrolling = NestedScrollingChildHelper(this)
    private val tableInteraction = TableInteractionController(
        context, this, { tablePresentationOwner }, { presentationSnapshot() },
        { tableOffsetChanged() },
        { startNestedScroll(ViewCompat.SCROLL_AXIS_HORIZONTAL, it) },
        { stopNestedScroll(it) },
        { dx, consumed, offset, type -> dispatchNestedPreScroll(dx, 0, consumed, offset, type) },
        { consumedX, remainingX, consumed, offset, type ->
            dispatchNestedScroll(consumedX, 0, remainingX, 0, offset, type, consumed)
        },
        { dispatchNestedPreFling(it, 0f) },
        { velocity, consumed -> dispatchNestedFling(velocity, 0f, consumed) }
    )
    private val imagePixelsLock = Any()
    private val imagePixels = mutableMapOf<String, DecodedBitmapLease>()

    /** Map overhead only; decoded allocation bytes are charged by the shared lease budget. */
    internal val retainedImagePixelsBytesForTesting: Long
        get() = synchronized(imagePixelsLock) { retainedImagePixelsBytes(imagePixels) }

    /** False when a public host owns this view's virtual subtree and notifications. */
    var publishesAccessibilitySubtree: Boolean = true
    internal var accessibilityVisibilityForTesting: ((Rect) -> Boolean)? = null
    var linkInteractionsEnabled: Boolean = true
        set(value) {
            if (field == value) return
            clearVirtualAccessibilityFocus()
            field = value
            invalidateTableAccessibility()
            announceAccessibilitySubtreeChanged()
        }
    var mentionInteractionsEnabled: Boolean = false
        set(value) {
            if (field == value) return
            clearVirtualAccessibilityFocus()
            field = value
            invalidateTableAccessibility()
            announceAccessibilitySubtreeChanged()
        }
    private val paint = Paint(Paint.ANTI_ALIAS_FLAG)
    private val touchSlop = ViewConfiguration.get(context).scaledTouchSlop.toFloat()
    private var pendingTap: PendingTap? = null
    private var pendingTableTap: Pair<Float, Float>? = null
    private var focusedVirtualNode: FocusedVirtualNode? = null
    internal var tableAccessibilityGeneration = 0L
        private set
    private val tableAccessibility = TableAccessibilityNodes(
        this,
        this,
        { tableAccessibilityItems() },
        { tableAccessibilityGeneration },
        { contentOriginXPx to contentOriginYPx },
        { bounds ->
            accessibilityVisibilityForTesting?.invoke(bounds)
                ?: accessibilityNodeVisibleOnScreen(bounds)
        },
        { clearVirtualAccessibilityFocus() }
    )
    internal var tableAccessibilityEditing: TableAccessibilityEditing?
        get() = tableAccessibility.editing
        set(value) {
            tableAccessibility.editing = value
        }
    private var contentOriginXPx = 0
    private var contentOriginYPx = 0
    private var drawnPresentationWindow: Rect? = null
    private val scrollChangedListener = ViewTreeObserver.OnScrollChangedListener {
        preparePresentedTableCells()
        reconcileVirtualAccessibilityFocus()
        redrawIfVisibleRectLeftDrawnWindow()
        onTableGeometryChanged?.invoke()
    }

    private fun redrawIfVisibleRectLeftDrawnWindow() {
        val visible =
            (presentationViewport() as? ViewerTablePresentationViewport.Known)?.rect ?: return
        val margin = Rect(visible).apply {
            inset(
                -(visible.width() * REDRAW_HYSTERESIS_VIEWPORTS).toInt(),
                -(
                    visible.height() *
                        REDRAW_HYSTERESIS_VIEWPORTS
                    ).toInt()
            )
        }
        if (drawnPresentationWindow?.contains(margin) != true) invalidate()
    }

    init {
        DecodedBitmapBudget.shared(context)
        isNestedScrollingEnabled = true
    }

    internal companion object {
        private const val NODE_ABOVE = "above"
        private const val NODE_BOUND_ROW = "boundRow"
        private const val NODE_BOUND_CELL = "boundCell"
        private const val NODE_BELOW = "below"
        private const val HANDLE_RADIUS_DP = 8f
        private const val HANDLE_INSET_DP = 8f
        private const val HANDLE_HIT_SIZE_DP = 48f
        private const val RESIZE_INDICATOR_WIDTH_DP = 2f
        private const val REDRAW_HYSTERESIS_VIEWPORTS = 0.5f
        private const val HIT_PROBE_PX = 1f
        const val IMAGE_PIXEL_MAP_RETAINED_BYTES = 48L
        const val IMAGE_PIXEL_ENTRY_RETAINED_BYTES = 48L

        fun retainedImagePixelsBytes(pixels: Map<String, *>): Long {
            if (pixels.isEmpty()) return 0L
            var retained = IMAGE_PIXEL_MAP_RETAINED_BYTES
            pixels.values.forEach {
                retained = saturatingAdd(retained, IMAGE_PIXEL_ENTRY_RETAINED_BYTES)
            }
            return retained
        }

        private fun saturatingAdd(left: Long, right: Long): Long =
            if (right > 0 && left > Long.MAX_VALUE - right) Long.MAX_VALUE else left + right

        private fun saturatingMultiply(left: Long, right: Long): Long = when {
            left <= 0L || right <= 0L -> 0L
            left > Long.MAX_VALUE / right -> Long.MAX_VALUE
            else -> left * right
        }
    }

    fun putImageLease(id: String, lease: DecodedBitmapLease) {
        synchronized(imagePixelsLock) { imagePixels.put(id, lease) }?.close()
        tableNodeState = null
        reportRetainedImagePixels()
        postInvalidate()
    }

    fun removeImageLeases(ids: Set<String>) {
        val released = synchronized(imagePixelsLock) { ids.mapNotNull(imagePixels::remove) }
        if (released.isEmpty()) return
        released.forEach(DecodedBitmapLease::close)
        tableNodeState = null
        reportRetainedImagePixels()
        postInvalidate()
    }

    fun clearImageLeases() {
        val released = synchronized(imagePixelsLock) {
            imagePixels.values.toList().also { imagePixels.clear() }
        }
        if (released.isEmpty()) return
        released.forEach(DecodedBitmapLease::close)
        tableNodeState = null
        reportRetainedImagePixels()
        postInvalidate()
    }

    private fun reportRetainedImagePixels() {
        PreparedProseInstrumentation.retained(
            PreparedProseInstrumentation.Owner.IMAGE,
            "drawing-${System.identityHashCode(this)}",
            synchronized(imagePixelsLock) { retainedImagePixelsBytes(imagePixels) }
        )
    }

    internal val tablePresentationRetainedBytesForTesting: Long
        get() = tablePresentationOwner.retainedBytes +
            if (android.os.Build.VERSION.SDK_INT >= android.os.Build.VERSION_CODES.Q) {
                tableNodes.sumOf { it.computeApproximateMemoryUsage() }
            } else {
                0L
            }

    private fun reportRetainedTablePresentation() {
        PreparedProseInstrumentation.retained(
            PreparedProseInstrumentation.Owner.SIDECARS,
            "table-presentation-${System.identityHashCode(this)}",
            if (preparedLayout == null) 0L else tablePresentationRetainedBytesForTesting
        )
    }

    /**
     * Publishes a prepared artifact. Replacement owners suppress the transient
     * clear announcement and let the final install report the one logical
     * subtree transition; focus-cleared events remain immediate.
     */
    fun install(
        layout: PreparedProseLayout?,
        announceAccessibilitySubtree: Boolean = true,
        contentOriginXPx: Int = 0,
        contentOriginYPx: Int = 0,
        preserveTablePresentationForReplacement: Boolean = false
    ) {
        if (
            preparedLayout === layout &&
            this.contentOriginXPx == contentOriginXPx &&
            this.contentOriginYPx == contentOriginYPx
        ) {
            return
        }
        clearVirtualAccessibilityFocus()
        if (preparedLayout !== layout) {
            tablePresentationOwner.clearPreparedCells()
            tableInteraction.cancel()
            val priorLayout = preparedLayout
            if (layout == null && preserveTablePresentationForReplacement && priorLayout != null) {
                replacementTablePresentationOwner = tablePresentationOwner
                replacementTableSemanticOwner = priorLayout.key.semanticGenerationIdentity
            }
            val preserve = layout != null &&
                (
                    priorLayout?.key?.semanticGenerationIdentity ==
                        layout.key.semanticGenerationIdentity ||
                        replacementTableSemanticOwner == layout.key.semanticGenerationIdentity
                    )
            tablePresentationOwner = if (preserve) {
                if (priorLayout != null) {
                    tablePresentationOwner
                } else {
                    requireNotNull(replacementTablePresentationOwner)
                }
            } else {
                ViewerTablePresentationOwner()
            }
            if (layout != null || !preserveTablePresentationForReplacement) {
                replacementTablePresentationOwner = null
                replacementTableSemanticOwner = null
            }
            pendingTap = null
            pendingTableTap = null
        }
        if (layout == null) {
            clearTableNodes()
        } else if (tableNodeState?.revision == tableNodeRevision) {
            tableNodeState = null
        }
        preparedLayout = layout
        invalidateTableAccessibility()
        if (layout != null) {
            tablePresentationOwner.reconcile(ViewerTablePresentation.surfaces(layout))
        }
        codeHighlighting.update()
        this.contentOriginXPx = contentOriginXPx
        this.contentOriginYPx = contentOriginYPx
        preparePresentedTableCells()
        reportRetainedTablePresentation()
        if (announceAccessibilitySubtree) announceAccessibilitySubtreeChanged()
        invalidate()
    }

    internal fun discardPendingTableReplacement() {
        replacementTablePresentationOwner = null
        replacementTableSemanticOwner = null
    }

    /** Mounted-only seam; direction and gesture transport remain host-owned. */
    internal fun setTableLogicalOffset(sourceIdentity: String, offset: Float) {
        val artifact = preparedLayout ?: return
        val surface =
            ViewerTablePresentation.surfaces(artifact).firstOrNull { it.identity == sourceIdentity }
                ?: return
        tablePresentationOwner.setLogicalOffset(offset, surface)
        tableOffsetChanged()
    }

    internal fun invalidateTableAccessibility() {
        tableAccessibilityGeneration += 1
        accessibilityNodeRegistry.clear()
    }

    private fun tableOffsetChanged() {
        preparePresentedTableCells()
        tableNodeState = null
        reportRetainedTablePresentation()
        clearVirtualAccessibilityFocus()
        onTableGeometryChanged?.invoke()
        announceAccessibilitySubtreeChanged()
        invalidate()
    }

    internal fun tablePhysicalOffsetForTesting(sourceIdentity: String): Float {
        val artifact = preparedLayout ?: return 0f
        val surface =
            ViewerTablePresentation.surfaces(artifact).firstOrNull { it.identity == sourceIdentity }
                ?: return 0f
        return tablePresentationOwner.physicalOffset(surface)
    }

    private fun presentationSnapshot() = preparedLayout?.let { artifact ->
        ViewerTablePresentation.project(
            artifact,
            tablePresentationOwner,
            presentationViewport()
        )
    }

    internal fun presentedTableCells(): List<ViewerTablePresentedCell> =
        presentationSnapshot()?.cells.orEmpty()

    internal fun rootTableCellAt(x: Float, y: Float): ViewerTablePresentedCell? {
        val table = preparedLayout?.let(ViewerTablePresentation::rootTables)
            ?.lastOrNull { it.bounds.contains(x, y) && it.clip.contains(x, y) } ?: return null
        val contentX = table.bounds.left - tablePresentationOwner.physicalOffset(table.surface)
        val left = x - contentX
        val top = y - table.bounds.top
        return table.surface.cellsIntersecting(left, top, left + HIT_PROBE_PX, top + HIT_PROBE_PX)
            .lastOrNull()?.let {
                ViewerTablePresentation.present(it, table, tablePresentationOwner)
            }
            ?.takeIf { it.bounds.contains(x, y) }
    }

    internal fun presentedTableCell(
        tableId: String,
        locate: (ViewerTableSurface) -> PreparedViewerTableCell?
    ): ViewerTablePresentedCell? {
        val artifact = preparedLayout ?: return null
        val table = ViewerTablePresentation.tableWithId(
            artifact,
            tablePresentationOwner,
            presentationViewport(),
            tableId
        ) ?: return null
        val cell = locate(table.surface) ?: return null
        return ViewerTablePresentation.present(cell, table, tablePresentationOwner)
    }

    internal fun selectionHandles(): List<TableSelectionHandle> =
        presentationSnapshot()?.let(::selectionHandles).orEmpty()

    private fun ViewerTablePresentationSnapshot.tableWithId(
        tableId: String
    ): ViewerTablePresentedSurface? = tables.firstOrNull { it.surface.editorTableId == tableId }

    private fun ViewerTablePresentationSnapshot.rootTables(): List<ViewerTablePresentedSurface> {
        val roots = blocks.filter {
            it.layout === preparedLayout
        }.mapNotNull { it.block.tableSurface }
        return tables.filter { presented -> roots.any { it === presented.surface } }
    }

    private fun columnTrailingEdgeX(table: ViewerTablePresentedSurface, column: Int): Float? {
        val widths = table.surface.layout.columnWidths
        if (column !in widths.indices) return null
        val logical = widths.take(column + 1).sum()
        val origin = table.bounds.left - tablePresentationOwner.physicalOffset(table.surface)
        return if (table.surface.isRightToLeft) {
            origin + table.surface.bounds.width() - logical
        } else {
            origin + logical
        }
    }

    internal fun hitResizeEdge(x: Float, y: Float): TableResizeEdge? {
        val snapshot = presentationSnapshot() ?: return null
        val visible = (presentationViewport() as? ViewerTablePresentationViewport.Known)?.rect
        if (visible != null && (
                x < visible.left || x >= visible.right ||
                    y < visible.top || y >= visible.bottom
                )
        ) {
            return null
        }
        val reach = HANDLE_HIT_SIZE_DP * resources.displayMetrics.density / 2f
        var best: Pair<TableResizeEdge, Float>? = null
        snapshot.rootTables().forEach { table ->
            if (y < table.clip.top || y >= table.clip.bottom) return@forEach
            val tableId = table.surface.editorTableId ?: return@forEach
            val sourceTable = table.surface.sourceTable ?: return@forEach
            val sourceCells = sourceTable.cells
            val columns = table.surface.layout.columnWidths.size
            val rowOffsets = table.surface.layout.rowOffsets
            val selectedColumns = selectedWholeColumns(tableId, sourceTable)
            snapshot.cells.filter { it.surface === table.surface }.forEach { cell ->
                val source = sourceCells.getOrNull(cell.sourceIndex) ?: return@forEach
                if (y < cell.bounds.top || y >= cell.bounds.bottom) return@forEach
                val column = (source.column + source.colspan).toInt() - 1
                val row = source.row.toInt()
                val handleRowHeight = rowOffsets.getOrNull(row + 1)?.minus(rowOffsets[row]) ?: 0f
                val inHandleRow = row == table.surface.columnEdgeHandleRows[column] &&
                    y < cell.bounds.top + handleRowHeight
                if (!inHandleRow && column !in selectedColumns) return@forEach
                val edgeX = if (table.surface.isRightToLeft) cell.bounds.left else cell.bounds.right
                val distance = kotlin.math.abs(x - edgeX)
                if (distance > reach || edgeX < table.clip.left || edgeX > table.clip.right ||
                    (visible != null && (edgeX < visible.left || edgeX > visible.right))
                ) {
                    return@forEach
                }
                if (column !in 0 until columns) return@forEach
                val current = best
                if (current != null &&
                    (
                        current.second < distance ||
                            (current.second == distance && current.first.column <= column)
                        )
                ) {
                    return@forEach
                }
                best = TableResizeEdge(tableId, column) to distance
            }
        }
        return best?.first
    }

    private fun selectedWholeColumns(tableId: String, table: TableSurfaceSource): IntRange {
        val (selectedTableId, anchor, head) = selectedTableCellEndpoints ?: return IntRange.EMPTY
        val positions = selectedTableCellSourceIndices[tableId]
        if (selectedTableId != tableId || positions == null) return IntRange.EMPTY
        val endpoints = listOf(anchor, head).map { position ->
            table.cells.firstOrNull {
                tableCellDocumentPosition?.invoke(tableId, it.sourceIndex) ==
                    position
            }
                ?: return IntRange.EMPTY
        }
        val selected = table.cells.filter { it.sourceIndex in positions }
        if (selected.isEmpty() || endpoints.minOf { it.row } != 0 ||
            endpoints.maxOf { it.row + it.rowspan } != table.rows
        ) {
            return IntRange.EMPTY
        }
        return selected.minOf { it.column }.toInt() until
            selected.maxOf { it.column + it.colspan }.toInt()
    }

    internal fun tableLogicalOffset(tableId: String): Float? {
        val surface = presentationSnapshot()?.tableWithId(tableId)?.surface ?: return null
        return tablePresentationOwner.logicalOffset(surface)
    }

    private fun selectionHandles(
        snapshot: ViewerTablePresentationSnapshot
    ): List<TableSelectionHandle> {
        val (tableId, anchor, head) = selectedTableCellEndpoints ?: return emptyList()
        val surface = snapshot.tableWithId(tableId) ?: return emptyList()
        val positions = selectedTableCellSourceIndices[tableId].orEmpty()
        val cells = snapshot.cells.filter {
            it.surface === surface.surface &&
                it.sourceIndex in positions
        }
        val selected = positions.sorted().mapNotNull(surface.surface::cell)
        if (selected.none {
                tableCellDocumentPosition?.invoke(tableId, it.sourceIndex) == anchor
            } ||
            selected.none { tableCellDocumentPosition?.invoke(tableId, it.sourceIndex) == head }
        ) {
            return emptyList()
        }
        val inset = HANDLE_INSET_DP * resources.displayMetrics.density
        val forward = anchor <= head
        val rtl = surface.surface.isRightToLeft
        val firstCell = ViewerTablePresentation.present(
            selected.minWith(
                compareBy<PreparedViewerTableCell> {
                    surface.surface.frameOfCell(it).top
                }
                    .thenBy {
                        surface.surface.frameOfCell(it).let { frame ->
                            if (rtl) {
                                -(
                                    frame.left +
                                        frame.width
                                    )
                            } else {
                                frame.left
                            }
                        }
                    }
            ),
            surface,
            tablePresentationOwner
        )
        val lastCell = ViewerTablePresentation.present(
            selected.maxWith(
                compareBy<PreparedViewerTableCell> {
                    surface.surface.frameOfCell(it).let { frame ->
                        frame.top +
                            frame.height
                    }
                }
                    .thenBy {
                        surface.surface.frameOfCell(it).let { frame ->
                            if (rtl) {
                                -frame.left
                            } else {
                                frame.left +
                                    frame.width
                            }
                        }
                    }
            ),
            surface,
            tablePresentationOwner
        )
        val firstX = if (rtl) firstCell.bounds.right - inset else firstCell.bounds.left + inset
        val lastX = if (rtl) lastCell.bounds.left + inset else lastCell.bounds.right - inset
        val first = TableSelectionHandle(
            if (forward) TableSelectionHandleRole.ANCHOR else TableSelectionHandleRole.HEAD,
            tableId,
            if (forward) anchor else head,
            firstX,
            firstCell.bounds.top + inset
        )
        val last = TableSelectionHandle(
            if (forward) TableSelectionHandleRole.HEAD else TableSelectionHandleRole.ANCHOR,
            tableId,
            if (forward) head else anchor,
            lastX,
            lastCell.bounds.bottom - inset
        )
        val visible = (presentationViewport() as? ViewerTablePresentationViewport.Known)?.rect
        return listOf(first, last).filter {
            surface.clip.contains(it.x, it.y) &&
                (visible == null || visible.contains(it.x.toInt(), it.y.toInt())) &&
                cells.any { cell -> cell.bounds.contains(it.x, it.y) }
        }
    }

    private fun updateSelectionGestureExclusionRects() {
        if (Build.VERSION.SDK_INT < Build.VERSION_CODES.Q) return
        val radius = HANDLE_HIT_SIZE_DP * resources.displayMetrics.density / 2f
        val rects = if (visibility != VISIBLE || width == 0 || height == 0 ||
            selectedTableCellEndpoints == null
        ) {
            emptyList()
        } else {
            selectionHandles().map { handle ->
                Rect(
                    floor(handle.x - radius).toInt().coerceIn(0, width),
                    floor(handle.y - radius).toInt().coerceIn(0, height),
                    ceil(handle.x + radius).toInt().coerceIn(0, width),
                    ceil(handle.y + radius).toInt().coerceIn(0, height)
                )
            }
        }
        if (systemGestureExclusionRects != rects) systemGestureExclusionRects = rects
    }

    internal fun hitSelectionHandle(x: Float, y: Float): TableSelectionHandle? {
        val snapshot = presentationSnapshot() ?: return null
        val visible = (presentationViewport() as? ViewerTablePresentationViewport.Known)?.rect
        if (visible != null && (
                x < visible.left || x >= visible.right ||
                    y < visible.top || y >= visible.bottom
                )
        ) {
            return null
        }
        val radius = HANDLE_HIT_SIZE_DP * resources.displayMetrics.density / 2f
        return selectionHandles(snapshot).mapNotNull { handle ->
            val distance = (x - handle.x).pow(2) + (y - handle.y).pow(2)
            if (distance <= radius * radius) handle to distance else null
        }.minWithOrNull(
            compareBy<Pair<TableSelectionHandle, Float>> { it.second }
                .thenBy { it.first.role.ordinal }
        )?.first
    }

    internal fun selectedTableCellAt(x: Float, y: Float, tableId: String): Int? {
        val snapshot = presentationSnapshot() ?: return null
        val visible = (presentationViewport() as? ViewerTablePresentationViewport.Known)?.rect
        if (visible != null && (
                x < visible.left || x >= visible.right ||
                    y < visible.top || y >= visible.bottom
                )
        ) {
            return null
        }
        val surface = snapshot.tableWithId(tableId) ?: return null
        return snapshot.cells.firstOrNull {
            it.surface === surface.surface && it.bounds.contains(x, y) && it.clip.contains(x, y)
        }?.sourceIndex?.let { tableCellDocumentPosition?.invoke(tableId, it) }
    }

    internal fun scrollSelectedTablePhysical(tableId: String, delta: Float): Float {
        val surface = presentationSnapshot()?.tableWithId(tableId)?.surface ?: return 0f
        val consumed = tablePresentationOwner.scrollPhysical(delta, surface)
        if (consumed != 0f) tableOffsetChanged()
        return consumed
    }

    internal fun cancelTableInteraction() = tableInteraction.cancel()

    internal fun selectedTableViewport(tableId: String): RectF? =
        presentationSnapshot()?.tableWithId(tableId)?.clip

    internal fun selectedTableCellRects(tableId: String): List<RectF>? =
        tableCellRects(tableId, selectedTableCellSourceIndices[tableId].orEmpty())

    internal fun tableCellRects(tableId: String, sourceIndices: Set<Int>): List<RectF>? {
        val snapshot = presentationSnapshot() ?: return null
        val table = snapshot.tableWithId(tableId) ?: return null
        return snapshot.cells.filter {
            it.surface === table.surface &&
                isRealTableCell(it, sourceIndices)
        }
            .sortedBy { it.sourceIndex }
            .mapNotNull { cell ->
                RectF(cell.bounds).takeIf { it.intersect(cell.clip) }
                    ?.apply { offset(contentOriginXPx.toFloat(), contentOriginYPx.toFloat()) }
            }
    }

    private fun isSelectedTableCell(cell: ViewerTablePresentedCell): Boolean {
        val tableId = cell.surface.editorTableId ?: return false
        return isRealTableCell(cell, selectedTableCellSourceIndices[tableId].orEmpty())
    }

    private fun isRealTableCell(cell: ViewerTablePresentedCell, sourceIndices: Set<Int>): Boolean =
        cell.sourceIndex in sourceIndices

    internal fun atomLayoutsJson(density: Float): String {
        val artifact = preparedLayout ?: return "[]"
        if (!density.isFinite() || density <= 0f) return "[]"
        val snapshot = ViewerTablePresentation.project(
            artifact,
            tablePresentationOwner,
            presentationViewport()
        )
        val mountedLayouts = snapshot.mountedCells.mapTo(
            Collections.newSetFromMap(IdentityHashMap<PreparedProseLayout, Boolean>())
        ) { it.content }

        return JSONArray().apply {
            snapshot.atoms.forEach { presented ->
                put(
                    JSONObject().apply {
                        put("nodeType", presented.atom.nodeType)
                        put("docPos", presented.atom.docPos)
                        put("attrsJson", presented.atom.attrsJson)
                        put("x", (presented.bounds.left + contentOriginXPx) / density)
                        put("y", (presented.bounds.top + contentOriginYPx) / density)
                        put("width", presented.atom.bounds.width().toFloat() / density)
                        put("height", presented.atom.bounds.height().toFloat() / density)
                        if (presented.layout !== artifact) {
                            put(
                                "presentation",
                                JSONObject().apply {
                                    put(
                                        "clip",
                                        JSONObject().apply {
                                            put(
                                                "x",
                                                (presented.clip.left + contentOriginXPx) / density
                                            )
                                            put(
                                                "y",
                                                (presented.clip.top + contentOriginYPx) / density
                                            )
                                            put(
                                                "width",
                                                presented.clip.width().coerceAtLeast(0f) / density
                                            )
                                            put(
                                                "height",
                                                presented.clip.height().coerceAtLeast(0f) / density
                                            )
                                        }
                                    )
                                    put("candidate", presented.layout in mountedLayouts)
                                }
                            )
                        }
                    }
                )
            }
        }.toString()
    }

    private fun presentationViewport(): ViewerTablePresentationViewport {
        if (windowToken == null) {
            return if (preparesTableCellsBeforeDrawing) {
                ViewerTablePresentationViewport.Known(Rect())
            } else {
                ViewerTablePresentationViewport.Unknown
            }
        }
        if (!isShown || alpha <= 0f) return ViewerTablePresentationViewport.Known(Rect())
        val visible = Rect()
        if (!getLocalVisibleRect(visible)) return ViewerTablePresentationViewport.Known(Rect())
        visible.offset(-contentOriginXPx, -contentOriginYPx)
        return ViewerTablePresentationViewport.Known(visible)
    }

    private fun presentedInteractions() = presentationSnapshot()?.interactions.orEmpty()

    private fun accessibleRole(node: ViewerTablePresentedAccessibilityNode): Boolean =
        when (node.node.role) {
            PreparedProseAccessibilityNode.Role.LINK -> linkInteractionsEnabled
            PreparedProseAccessibilityNode.Role.MENTION -> mentionInteractionsEnabled
        }

    internal fun rootAccessibilityNodes(): List<ViewerTablePresentedAccessibilityNode> {
        val artifact = preparedLayout ?: return emptyList()
        return presentationSnapshot()?.accessibilityNodes.orEmpty().filter {
            it.layout === artifact &&
                accessibleRole(it)
        }
    }

    internal val tablePresentationOwnerForAccessibility: ViewerTablePresentationOwner
        get() = tablePresentationOwner

    internal fun presentedRootTable(surface: ViewerTableSurface): ViewerTablePresentedSurface? =
        preparedLayout?.let(ViewerTablePresentation::rootTables)?.firstOrNull {
            it.surface ===
                surface
        }

    internal fun presentedCellAccessibilityNodes(
        cell: ViewerTablePresentedCell
    ): List<ViewerTablePresentedAccessibilityNode> =
        ViewerTablePresentation.contentAccessibilityNodes(
            cell,
            tablePresentationOwner
        ).filter(::accessibleRole)

    override fun onDraw(canvas: Canvas) {
        super.onDraw(canvas)
        reconcileVirtualAccessibilityFocus()
        val artifact = preparedLayout ?: return
        val saved = canvas.save()
        canvas.translate(contentOriginXPx.toFloat(), contentOriginYPx.toFloat())
        canvas.clipRect(0, 0, artifact.widthPx, artifact.heightPx)
        try {
            artifact.contentBox?.let {
                com.apollohg.editor.EditorBoxDrawing.draw(
                    canvas,
                    RectF(0f, 0f, artifact.widthPx.toFloat(), artifact.heightPx.toFloat()),
                    it
                )
            }
            recordPreparedProseDraw {
                onVisibleRectChanged?.invoke(Rect(canvas.clipBounds))
                val paintClip = Rect(canvas.clipBounds)
                val visibleRect = Rect()
                val presentationViewport = presentationViewport()
                drawnPresentationWindow =
                    (presentationViewport as? ViewerTablePresentationViewport.Known)?.window
                val snapshot = ViewerTablePresentation.project(
                    artifact,
                    tablePresentationOwner,
                    presentationViewport
                )
                onMountedTableCellsDrawnForTesting?.invoke(snapshot.mountedCells.size)
                onVisibleImagesChanged?.invoke(
                    if (presentationViewport is ViewerTablePresentationViewport.Known) {
                        presentationViewport.rect
                    } else {
                        paintClip
                    },

                    snapshot.images.mapNotNull { image ->
                        val bounds = RectF(image.bounds)
                        if (!bounds.intersect(image.clip) || bounds.isEmpty) return@mapNotNull null
                        image.attachment.copy(
                            bounds = Rect(
                                bounds.left.toInt(),
                                bounds.top.toInt(),
                                bounds.right.toInt(),
                                bounds.bottom.toInt()
                            )
                        )
                    }
                )
                if (usesEditAnchoredNodes &&
                    android.os.Build.VERSION.SDK_INT >= android.os.Build.VERSION_CODES.Q &&
                    canvas.isHardwareAccelerated
                ) {
                    val window =
                        if (presentationViewport is ViewerTablePresentationViewport.Known) {
                            presentationViewport.rect
                        } else {
                            paintClip
                        }
                    drawTableNodes(canvas, artifact, snapshot, window)
                    snapshot.blocks.size
                } else {
                    paintTableWindow(canvas, artifact, snapshot, paintClip)
                }
            }
        } finally {
            canvas.restoreToCount(saved)
        }
    }

    private fun paintTableWindow(
        canvas: Canvas,
        artifact: PreparedProseLayout,
        snapshot: com.apollohg.editor.tables.ViewerTablePresentationSnapshot,
        paintClip: Rect,
        excluded: RectF? = null,
        filterCells: Boolean = false
    ): Int {
        val cells = if (filterCells) {
            snapshot.mountedCells.filter { cell ->
                val intersection = RectF(cell.bounds)
                intersection.intersect(RectF(paintClip)) && !intersection.isEmpty &&
                    excluded?.contains(intersection) != true
            }
        } else {
            snapshot.mountedCells
        }
        val mountedLayouts = Collections.newSetFromMap(
            IdentityHashMap<PreparedProseLayout, Boolean>()
        )
        cells.forEach { mountedLayouts += it.content }
        val visible = snapshot.blocks.filter { presented ->
            (presented.layout === artifact || presented.layout in mountedLayouts) &&
                presented.block.bounds.let { bounds ->
                    bounds.right + presented.originX > paintClip.left &&
                        bounds.left + presented.originX < paintClip.right &&
                        bounds.bottom + presented.originY > paintClip.top &&
                        bounds.top + presented.originY < paintClip.bottom
                }
        }
        drawHierarchicalBackgrounds(canvas, artifact, snapshot, mountedLayouts, paintClip, cells)
        remoteTableCellSelections.forEach { remote ->
            cells.filter {
                it.surface.editorTableId == remote.tableId &&
                    isRealTableCell(it, remote.sourceIndices)
            }.forEach { fillTableCell(canvas, it, remote.color) }
        }
        cells.filter(::isSelectedTableCell).forEach {
            fillTableCell(canvas, it, it.surface.style.selectionColor)
        }
        tableCellDropTarget?.let { target ->
            cells.filter {
                it.surface.editorTableId == target.tableId &&
                    isRealTableCell(it, setOf(target.sourceIndex))
            }.forEach { fillTableCell(canvas, it, it.surface.style.selectionColor) }
        }
        cells.forEach { drawTableChromeBorder(canvas, it) }
        visible.forEach { drawPresented(canvas, it, snapshot) { drawBorderOrRule(canvas, it) } }
        visible.filter {
            it.block.tableSurface?.layout?.failure != null
        }.forEach { drawTableFailure(canvas, it) }
        val attachmentsByBlock = snapshot.images.mapNotNull { image ->
            image.block?.let { block -> block to image.attachment }
        }.toMap()
        val activeCellContent = cells.firstOrNull {
            (it.surface.editorTableId to it.sourceIndex) == suppressedTableCell
        }?.content
        visible.filter { it.layout !== activeCellContent }.forEach { presented ->
            drawPresented(canvas, presented, snapshot) {
                val attachment = attachmentsByBlock[presented.block]
                if (presented.layout !== artifact && it.kind == PreparedProseFragmentKind.TEXT) {
                    onTableRichFragmentDrawnForTesting?.invoke()
                }
                drawForeground(canvas, it, attachment)
            }
        }
        val handleTable = selectedTableCellEndpoints?.first?.let { tableId ->
            snapshot.tableWithId(tableId)
        }
        if (handleTable != null) {
            paint.style = Paint.Style.FILL
            val handleColor = handleTable.surface.style.selectionColor
            paint.color =
                Color.rgb(Color.red(handleColor), Color.green(handleColor), Color.blue(handleColor))
            val radius = HANDLE_RADIUS_DP * resources.displayMetrics.density
            val handleClip = canvas.save()
            canvas.clipRect(handleTable.clip)
            selectionHandles(snapshot).forEach { handle ->
                canvas.drawCircle(handle.x, handle.y, radius, paint)
            }
            canvas.restoreToCount(handleClip)
        }
        val resizeEdge = activeTableResizeEdge
        val resizeTable = resizeEdge?.let { edge ->
            snapshot.rootTables().firstOrNull { it.surface.editorTableId == edge.tableId }
        }
        val resizeX = resizeTable?.let {
            columnTrailingEdgeX(it, requireNotNull(resizeEdge).column)
        }
        if (resizeTable != null && resizeX != null) {
            val halfWidth = RESIZE_INDICATOR_WIDTH_DP * resources.displayMetrics.density / 2f
            val indicatorClip = canvas.save()
            canvas.clipRect(resizeTable.clip)
            paint.style = Paint.Style.FILL
            paint.color = resizeTable.surface.style.resizeHandleColor
            canvas.drawRect(
                resizeX - halfWidth,
                resizeTable.bounds.top,
                resizeX + halfWidth,
                resizeTable.bounds.bottom,
                paint
            )
            canvas.restoreToCount(indicatorClip)
        }
        return visible.size
    }

    @androidx.annotation.RequiresApi(android.os.Build.VERSION_CODES.Q)
    private fun recordTableNode(
        node: RenderNode,
        name: String,
        region: RectF,
        artifact: PreparedProseLayout,
        snapshot: com.apollohg.editor.tables.ViewerTablePresentationSnapshot,
        excluded: RectF? = null
    ) {
        if (region.isEmpty) {
            node.discardDisplayList()
            return
        }
        val bounds = Rect().also(region::roundOut)
        node.setPosition(bounds)
        node.translationY = 0f
        val recording = node.beginRecording(bounds.width(), bounds.height())
        try {
            recording.translate(-bounds.left.toFloat(), -bounds.top.toFloat())
            recording.clipRect(region)
            if (excluded != null) recording.clipOutRect(excluded)
            paintTableWindow(recording, artifact, snapshot, bounds, excluded, filterCells = true)
        } finally {
            node.endRecording()
        }
        nodeRecordsForTesting[name] = nodeRecordsForTesting.getOrDefault(name, 0) + 1
    }

    @androidx.annotation.RequiresApi(android.os.Build.VERSION_CODES.Q)
    private fun drawTableNodes(
        canvas: Canvas,
        artifact: PreparedProseLayout,
        snapshot: com.apollohg.editor.tables.ViewerTablePresentationSnapshot,
        window: Rect
    ) {
        if (window.isEmpty) {
            clearTableNodes()
            return
        }
        val above = requireNotNull(aboveNode)
        val rowNode = requireNotNull(boundRowNode)
        val cellNode = requireNotNull(boundCellNode)
        val below = requireNotNull(belowNode)
        val binding = suppressedTableCell
        val presented = binding?.let { (id, index) ->
            snapshot.tableWithId(id)?.let { table ->
                table.surface.cell(index)?.let {
                    ViewerTablePresentation.present(it, table, tablePresentationOwner)
                }
            }
        }
        if (binding == null || presented == null) {
            if (tableNodeState?.cell != null || tableNodeState?.window != window ||
                tableNodeState?.revision != tableNodeRevision ||
                tableNodeState?.appearance != tableNodeAppearance ||
                !above.hasDisplayList()
            ) {
                clearTableNodes()
                recordTableNode(above, NODE_ABOVE, RectF(window), artifact, snapshot)
            }
            tableNodeState =
                TableNodeState(
                    null,
                    Rect(window),
                    RectF(),
                    RectF(),
                    emptyList(),
                    tableNodeRevision,
                    tableNodeAppearance
                )
        } else {
            val frame = presented.bounds
            var firstRow = presented.cell.row
            var lastRow = firstRow + presented.cell.rowspan
            val cells = snapshot.mountedCells.filter { it.surface.editorTableId == binding.first }
            do {
                val previous = firstRow until lastRow
                for (cell in cells) {
                    if (cell.cell.row < lastRow &&
                        cell.cell.row + cell.cell.rowspan > firstRow
                    ) {
                        firstRow = minOf(firstRow, cell.cell.row)
                        lastRow = maxOf(lastRow, cell.cell.row + cell.cell.rowspan)
                    }
                }
            } while (previous != firstRow until lastRow)
            val offsets = presented.surface.layout.rowOffsets
            val originY = frame.top - offsets[presented.cell.row]
            val row =
                RectF(
                    window.left.toFloat(),
                    originY + offsets[firstRow],
                    window.right.toFloat(),
                    originY + offsets[lastRow]
                )
            val rowOffsets = offsets.subList(firstRow, lastRow + 1).map { it - offsets[firstRow] }
            val old = tableNodeState
            val changedRevision = old?.revision != tableNodeRevision
            val changesAreBoundCellOnly = tableNodeChanges?.let { changes ->
                !changes.fullReset && changes.replacedTables.isEmpty() &&
                    changes.removedTables.isEmpty() &&
                    changes.changedCells.all { (key, indexes) ->
                        key == binding.first &&
                            indexes.all { it == binding.second }
                    }
            } == true
            val reusable =
                old?.cell == binding && old.window == window &&
                    old.appearance == tableNodeAppearance &&
                    old.row.top == row.top && old.frame.left == frame.left &&
                    old.frame.width() == frame.width() &&
                    (!changedRevision || changesAreBoundCellOnly) &&
                    (row.top <= window.top || above.hasDisplayList()) &&
                    (row.bottom >= window.bottom || below.hasDisplayList())
            fun belowRegion() = RectF(
                window.left.toFloat(),
                maxOf(row.bottom, window.top.toFloat()),
                window.right.toFloat(),
                window.bottom.toFloat()
            )
            fun clipped(region: RectF) =
                RectF(region).apply { if (!intersect(RectF(window))) setEmpty() }
            if (!reusable) {
                clearTableNodes()
                recordTableNode(
                    above,
                    NODE_ABOVE,
                    RectF(
                        window.left.toFloat(),
                        window.top.toFloat(),
                        window.right.toFloat(),
                        maxOf(window.top.toFloat(), minOf(row.top, window.bottom.toFloat()))
                    ),
                    artifact,
                    snapshot
                )
                recordTableNode(below, NODE_BELOW, belowRegion(), artifact, snapshot)
            } else if (old != null && old.row.height() != row.height()) {
                below.translationY += row.height() - old.row.height()
                if (below.bottom + below.translationY < window.bottom) {
                    recordTableNode(below, NODE_BELOW, belowRegion(), artifact, snapshot)
                }
            }
            if (!reusable || old?.rowOffsets != rowOffsets || !rowNode.hasDisplayList()) {
                recordTableNode(rowNode, NODE_BOUND_ROW, clipped(row), artifact, snapshot, frame)
            }
            if (!reusable || changedRevision || old?.frame != frame || !cellNode.hasDisplayList()) {
                recordTableNode(cellNode, NODE_BOUND_CELL, clipped(frame), artifact, snapshot)
            }
            tableNodeState =
                TableNodeState(
                    binding,
                    Rect(window),
                    RectF(frame),
                    row,
                    rowOffsets,
                    tableNodeRevision,
                    tableNodeAppearance
                )
        }
        for (node in tableNodes) if (node.hasDisplayList()) canvas.drawRenderNode(node)
        reportRetainedTablePresentation()
    }

    private fun fillTableCell(canvas: Canvas, cell: ViewerTablePresentedCell, color: Int) {
        val saved = canvas.save()
        canvas.clipRect(cell.clip)
        paint.style = Paint.Style.FILL
        paint.color = color
        canvas.drawRect(cell.bounds, paint)
        canvas.restoreToCount(saved)
    }

    private fun drawTableFailure(canvas: Canvas, presented: ViewerTablePresentedBlock) {
        val bounds = presented.block.tableBounds ?: return
        val saved = canvas.save()
        canvas.clipRect(presented.clip)
        canvas.translate(presented.originX, presented.originY)
        paint.style = Paint.Style.FILL
        paint.color = 0x30f44336
        canvas.drawRect(bounds, paint)
        paint.style = Paint.Style.STROKE
        paint.strokeWidth = 1f
        paint.color = 0xfff44336.toInt()
        canvas.drawRect(bounds, paint)
        canvas.restoreToCount(saved)
    }

    private fun drawTableChromeBackground(canvas: Canvas, cell: ViewerTablePresentedCell) {
        if (!cell.cell.isHeader) return
        val saved = canvas.save()
        canvas.clipRect(cell.clip)
        paint.style = Paint.Style.FILL
        paint.color = cell.surface.style.headerBackgroundColor
        canvas.drawRect(cell.bounds, paint)
        canvas.restoreToCount(saved)
    }

    private fun drawHierarchicalBackgrounds(
        canvas: Canvas,
        root: PreparedProseLayout,
        snapshot: com.apollohg.editor.tables.ViewerTablePresentationSnapshot,
        mountedLayouts: Set<PreparedProseLayout>,
        paintClip: Rect,
        visibleCells: List<ViewerTablePresentedCell>
    ) {
        val layouts =
            IdentityHashMap<
                PreparedProseLayout,
                com.apollohg.editor.tables.ViewerTablePresentedLayout
                >()
        snapshot.layouts.forEach { layouts[it.layout] = it }
        val blocks = IdentityHashMap<PreparedProseLayout, MutableList<ViewerTablePresentedBlock>>()
        snapshot.blocks.forEach { blocks.getOrPut(it.layout) { mutableListOf() }.add(it) }
        val cells = IdentityHashMap<ViewerTableSurface, MutableList<ViewerTablePresentedCell>>()
        visibleCells.forEach { cells.getOrPut(it.surface) { mutableListOf() }.add(it) }

        fun drawLayout(presented: com.apollohg.editor.tables.ViewerTablePresentedLayout) {
            blocks[presented.layout].orEmpty().forEach { block ->
                if (presented.layout !== root || block.block.intersects(paintClip)) {
                    drawPresented(canvas, block, snapshot) { drawBackground(canvas, it) }
                }
                val surface = block.block.tableSurface ?: return@forEach
                val surfaceCells = cells[surface].orEmpty()
                surfaceCells.forEach { drawTableChromeBackground(canvas, it) }
                surfaceCells.forEach { cell ->
                    if (cell.content in mountedLayouts) layouts[cell.content]?.let(::drawLayout)
                }
            }
        }
        snapshot.layouts.firstOrNull()?.let(::drawLayout)
    }

    private fun drawTableChromeBorder(canvas: Canvas, cell: ViewerTablePresentedCell) {
        onTableChromeDrawnForTesting?.invoke(cell.sourceIndex)
        val saved = canvas.save()
        canvas.clipRect(cell.clip)
        paint.style = Paint.Style.STROKE
        paint.strokeWidth = cell.surface.style.borderWidth
        paint.color = cell.surface.style.borderColor
        val inset = paint.strokeWidth / 2f
        canvas.drawRect(RectF(cell.bounds).apply { inset(inset, inset) }, paint)
        canvas.restoreToCount(saved)
    }

    private inline fun drawPresented(
        canvas: Canvas,
        presented: ViewerTablePresentedBlock,
        snapshot: com.apollohg.editor.tables.ViewerTablePresentationSnapshot,
        draw: (PreparedProseFragment) -> Unit
    ) {
        val saved = canvas.save()
        canvas.clipRect(presented.clip)
        canvas.translate(presented.originX, presented.originY)
        presented.block.fragments.forEach(draw)
        canvas.restoreToCount(saved)
    }

    private fun drawBackground(canvas: Canvas, fragment: PreparedProseFragment) {
        if (fragment.kind != PreparedProseFragmentKind.BACKGROUND &&
            fragment.kind != PreparedProseFragmentKind.ATOM &&
            fragment.kind != PreparedProseFragmentKind.IMAGE
        ) {
            return
        }
        fragment.box?.let { box ->
            val saved = canvas.save()
            canvas.clipRect(fragment.bounds)
            com.apollohg.editor.EditorBoxDrawing.draw(
                canvas,
                RectF(fragment.decorationBounds ?: fragment.bounds),
                box
            )
            canvas.restoreToCount(saved)
            return
        }
        paint.style = Paint.Style.FILL
        paint.color = fragment.color ?: return
        canvas.drawRoundRect(
            RectF(fragment.bounds),
            fragment.cornerRadius,
            fragment.cornerRadius,
            paint
        )
    }

    private fun drawBorderOrRule(canvas: Canvas, fragment: PreparedProseFragment) {
        when (fragment.kind) {
            PreparedProseFragmentKind.BORDER, PreparedProseFragmentKind.RULE -> {
                paint.style = Paint.Style.FILL
                paint.color = fragment.color ?: return
                canvas.drawRect(fragment.bounds, paint)
            }

            PreparedProseFragmentKind.ATOM -> if (fragment.strokeWidth > 0f) {
                paint.style = Paint.Style.STROKE
                paint.strokeWidth = fragment.strokeWidth
                paint.color = fragment.borderColor ?: fragment.color ?: return
                val inset = fragment.strokeWidth / 2f
                canvas.drawRoundRect(
                    RectF(fragment.bounds).apply { inset(inset, inset) },
                    maxOf(
                        0f,
                        fragment.cornerRadius - inset
                    ),
                    maxOf(0f, fragment.cornerRadius - inset),
                    paint
                )
            }

            else -> Unit
        }
    }

    private fun drawForeground(
        canvas: Canvas,
        fragment: PreparedProseFragment,
        attachment: ViewerImageAttachment? = null
    ) {
        when (fragment.kind) {
            PreparedProseFragmentKind.TEXT, PreparedProseFragmentKind.MARKER -> {
                fragment.layout?.let { layout ->
                    val saved = canvas.save()
                    canvas.translate(fragment.layoutX.toFloat(), fragment.layoutY.toFloat())
                    layout.draw(canvas)
                    com.apollohg.editor.EditorTextDecorationDrawing.draw(canvas, layout)
                    canvas.restoreToCount(saved)
                }
                    ?: if (fragment.kind ==
                        PreparedProseFragmentKind.MARKER
                    ) {
                        drawTaskMarker(canvas, fragment)
                    } else {
                        Unit
                    }
            }

            PreparedProseFragmentKind.ATOM -> fragment.labelLayout?.let { layout ->
                val saved = canvas.save()
                canvas.translate(fragment.labelX.toFloat(), fragment.labelY.toFloat())
                layout.draw(canvas)
                com.apollohg.editor.EditorTextDecorationDrawing.draw(canvas, layout)
                canvas.restoreToCount(saved)
            }

            PreparedProseFragmentKind.STRIKE -> {
                paint.style = Paint.Style.FILL
                paint.color = fragment.color ?: return
                canvas.drawRect(fragment.bounds, paint)
            }

            PreparedProseFragmentKind.IMAGE -> {
                val attachment =
                    attachment
                        ?: preparedLayout?.imageAttachments?.firstOrNull {
                            it.bounds ==
                                fragment.bounds
                        }
                        ?: return
                val bitmap =
                    synchronized(imagePixelsLock) { imagePixels[attachment.id]?.bitmap } ?: return
                fragment.box?.let {
                    com.apollohg.editor.EditorBoxDrawing.drawImage(
                        canvas,
                        bitmap,
                        RectF(fragment.bounds),
                        it,
                        fragment.resizeMode
                    )
                } ?: canvas.drawBitmap(bitmap, null, fragment.bounds, paint)
            }

            else -> Unit
        }
    }

    private fun drawTaskMarker(canvas: Canvas, fragment: PreparedProseFragment) {
        val bounds = RectF(fragment.bounds)
        fragment.box?.let {
            com.apollohg.editor.drawCheckbox(
                canvas,
                bounds,
                it,
                fragment.checked,
                fragment.borderColor ?: fragment.color ?: android.graphics.Color.BLACK
            )
            return
        }
        val inset = maxOf(1f, bounds.height() * 0.2f)
        val box = RectF(bounds).apply { inset(inset, inset) }
        paint.style = Paint.Style.STROKE
        paint.strokeWidth = maxOf(1f, box.width() * 0.1f)
        paint.color = fragment.color ?: return
        canvas.drawRoundRect(box, box.width() * 0.2f, box.width() * 0.2f, paint)
        if (!fragment.checked) return
        paint.style = Paint.Style.STROKE
        paint.strokeWidth = maxOf(1.4f, box.width() * 0.12f)
        paint.strokeCap = Paint.Cap.ROUND
        paint.strokeJoin = Paint.Join.ROUND
        val path = android.graphics.Path().apply {
            moveTo(box.left + box.width() * 0.2f, box.centerY())
            lineTo(box.left + box.width() * 0.43f, box.bottom - box.height() * 0.2f)
            lineTo(box.right - box.width() * 0.16f, box.top + box.height() * 0.2f)
        }
        canvas.drawPath(path, paint)
        paint.strokeCap = Paint.Cap.BUTT
        paint.strokeJoin = Paint.Join.MITER
    }

    override fun onSizeChanged(width: Int, height: Int, oldWidth: Int, oldHeight: Int) {
        super.onSizeChanged(width, height, oldWidth, oldHeight)
        preparePresentedTableCells()
        reconcileVirtualAccessibilityFocus()
        if (width > 0) onUsableMetricsChanged?.invoke()
    }

    override fun onLayout(changed: Boolean, left: Int, top: Int, right: Int, bottom: Int) {
        super.onLayout(changed, left, top, right, bottom)
        preparePresentedTableCells()
    }

    override fun onAttachedToWindow() {
        super.onAttachedToWindow()
        codeHighlighting.update()
        viewTreeObserver.addOnScrollChangedListener(scrollChangedListener)
        viewTreeObserver.addOnPreDrawListener(prepareTableCellsListener)
        preparePresentedTableCells()
        reconcileVirtualAccessibilityFocus()
        if (width > 0) onUsableMetricsChanged?.invoke()
    }

    override fun onDetachedFromWindow() {
        tablePresentationOwner.clearPreparedCells()
        replacementTablePresentationOwner?.clearPreparedCells()
        clearTableNodes()
        tableInteraction.cancel()
        pendingTap = null
        pendingTableTap = null
        codeHighlighting.cancel()
        if (viewTreeObserver.isAlive) {
            viewTreeObserver.removeOnScrollChangedListener(scrollChangedListener)
            viewTreeObserver.removeOnPreDrawListener(prepareTableCellsListener)
        }
        clearVirtualAccessibilityFocus()
        super.onDetachedFromWindow()
    }

    override fun onVisibilityChanged(changedView: View, visibility: Int) {
        super.onVisibilityChanged(changedView, visibility)
        preparePresentedTableCells()
        reconcileVirtualAccessibilityFocus()
    }

    override fun onWindowVisibilityChanged(visibility: Int) {
        super.onWindowVisibilityChanged(visibility)
        reconcileVirtualAccessibilityFocus()
    }

    override fun onConfigurationChanged(newConfig: Configuration) {
        super.onConfigurationChanged(newConfig)
        onFontConfigurationChanged?.invoke(newConfig)
    }

    override fun onTouchEvent(event: MotionEvent): Boolean {
        val contentX = event.x - contentOriginXPx
        val contentY = event.y - contentOriginYPx
        val tableHandled = tableInteraction.onTouch(event, contentX, contentY)
        if (tableInteraction.ownsHorizontalGesture) {
            pendingTap = null
            pendingTableTap = null
            return true
        }
        fun targetAt(): PreparedProseInteraction? =
            presentedInteractions().firstOrNull { presented ->
                if (contentX < 0f || contentY < 0f) return@firstOrNull false
                interactionEnabled(presented.interaction.kind) &&
                    presented.rects.any { it.contains(contentX, contentY) } &&
                    presented.clip.contains(contentX, contentY)
            }?.interaction
        when (event.actionMasked) {
            MotionEvent.ACTION_DOWN -> {
                pendingTableTap = if (tableHandled) event.x to event.y else null
                pendingTap =
                    if (event.pointerCount ==
                        1
                    ) {
                        targetAt()?.let {
                            PendingTap(
                                it,
                                event.getPointerId(event.actionIndex),
                                event.x,
                                event.y
                            )
                        }
                    } else {
                        null
                    }
                return tableHandled || pendingTap != null
            }

            MotionEvent.ACTION_MOVE -> {
                pendingTap?.let { tap ->
                    if (event.pointerCount != 1 ||
                        event.findPointerIndex(tap.pointerId) < 0 ||
                        exceedsSlop(event, tap)
                    ) {
                        pendingTap = null
                    }
                }
                pendingTableTap?.let { (x, y) ->
                    val dx = event.x - x
                    val dy = event.y - y
                    if (dx * dx + dy * dy > touchSlop * touchSlop) pendingTableTap = null
                }
                return tableHandled || pendingTap != null || pendingTableTap != null
            }

            MotionEvent.ACTION_CANCEL,
            MotionEvent.ACTION_POINTER_DOWN,
            MotionEvent.ACTION_POINTER_UP -> {
                pendingTap =
                    null
                pendingTableTap = null
                return false
            }

            MotionEvent.ACTION_UP -> {
                if (tableHandled) {
                    pendingTap = null
                    pendingTableTap = null
                    return true
                }
                val tap = pendingTap
                pendingTap = null
                val tableTap = pendingTableTap
                pendingTableTap = null
                if (tableTap != null && onTableTap?.invoke(
                        tableTap.first - contentOriginXPx,
                        tableTap.second - contentOriginYPx,
                        contentX,
                        contentY
                    ) == true
                ) {
                    return true
                }
                if (tap != null && event.pointerCount == 1 &&
                    event.getPointerId(event.actionIndex) == tap.pointerId &&
                    !exceedsSlop(event, tap) &&
                    targetAt() == tap.target
                ) {
                    return onInteractionActivated?.invoke(tap.target) ?: false
                }
            }
        }
        return false
    }

    override fun computeScroll() {
        super.computeScroll()
        tableInteraction.computeScroll()
    }

    internal val ownsHorizontalTableGesture: Boolean get() = tableInteraction.ownsHorizontalGesture
    internal val tracksTableGesture: Boolean get() = tableInteraction.tracksGesture

    internal fun canConsumeTableDragAt(x: Float, y: Float, dx: Float): Boolean =
        tableInteraction.canConsumeAt(x - contentOriginXPx, y - contentOriginYPx, dx)

    internal fun hasTableAt(x: Float, y: Float): Boolean =
        tableInteraction.hasTableAt(x - contentOriginXPx, y - contentOriginYPx)

    override fun setNestedScrollingEnabled(enabled: Boolean) =
        nestedScrolling.setNestedScrollingEnabled(enabled)
    override fun isNestedScrollingEnabled(): Boolean = nestedScrolling.isNestedScrollingEnabled
    override fun startNestedScroll(axes: Int): Boolean = nestedScrolling.startNestedScroll(axes)
    override fun startNestedScroll(axes: Int, type: Int): Boolean =
        nestedScrolling.startNestedScroll(axes, type)
    override fun stopNestedScroll() = nestedScrolling.stopNestedScroll()
    override fun stopNestedScroll(type: Int) = nestedScrolling.stopNestedScroll(type)
    override fun hasNestedScrollingParent(): Boolean = nestedScrolling.hasNestedScrollingParent()
    override fun hasNestedScrollingParent(type: Int): Boolean =
        nestedScrolling.hasNestedScrollingParent(type)
    override fun dispatchNestedScroll(
        dxConsumed: Int,
        dyConsumed: Int,
        dxUnconsumed: Int,
        dyUnconsumed: Int,
        offsetInWindow: IntArray?
    ): Boolean = nestedScrolling.dispatchNestedScroll(
        dxConsumed,
        dyConsumed,
        dxUnconsumed,
        dyUnconsumed,
        offsetInWindow
    )
    override fun dispatchNestedScroll(
        dxConsumed: Int,
        dyConsumed: Int,
        dxUnconsumed: Int,
        dyUnconsumed: Int,
        offsetInWindow: IntArray?,
        type: Int
    ): Boolean = nestedScrolling.dispatchNestedScroll(
        dxConsumed,
        dyConsumed,
        dxUnconsumed,
        dyUnconsumed,
        offsetInWindow,
        type
    )
    override fun dispatchNestedScroll(
        dxConsumed: Int,
        dyConsumed: Int,
        dxUnconsumed: Int,
        dyUnconsumed: Int,
        offsetInWindow: IntArray?,
        type: Int,
        consumed: IntArray
    ) = nestedScrolling.dispatchNestedScroll(
        dxConsumed,
        dyConsumed,
        dxUnconsumed,
        dyUnconsumed,
        offsetInWindow,
        type,
        consumed
    )
    override fun dispatchNestedPreScroll(
        dx: Int,
        dy: Int,
        consumed: IntArray?,
        offsetInWindow: IntArray?
    ): Boolean = nestedScrolling.dispatchNestedPreScroll(dx, dy, consumed, offsetInWindow)
    override fun dispatchNestedPreScroll(
        dx: Int,
        dy: Int,
        consumed: IntArray?,
        offsetInWindow: IntArray?,
        type: Int
    ): Boolean = nestedScrolling.dispatchNestedPreScroll(dx, dy, consumed, offsetInWindow, type)
    override fun dispatchNestedFling(
        velocityX: Float,
        velocityY: Float,
        consumed: Boolean
    ): Boolean = nestedScrolling.dispatchNestedFling(velocityX, velocityY, consumed)
    override fun dispatchNestedPreFling(velocityX: Float, velocityY: Float): Boolean =
        nestedScrolling.dispatchNestedPreFling(velocityX, velocityY)

    private fun exceedsSlop(event: MotionEvent, tap: PendingTap): Boolean {
        val dx = event.x - tap.downX
        val dy = event.y - tap.downY
        return dx * dx + dy * dy > touchSlop * touchSlop
    }

    private data class PendingTap(
        val target: PreparedProseInteraction,
        val pointerId: Int,
        val downX: Float,
        val downY: Float
    )

    override fun onInitializeAccessibilityNodeInfo(info: AccessibilityNodeInfo) {
        super.onInitializeAccessibilityNodeInfo(info)
        info.className = android.widget.TextView::class.java.name
        accessibilityChildIds().forEach { info.addChild(this, it) }
    }

    internal fun accessibilityChildIds(): List<Int> =
        tableAccessibility.hostChildren { node -> rootNodeId(node) }

    internal fun tableAccessibilityItems(): List<TableAccessibilityItem> {
        val artifact = preparedLayout ?: return emptyList()
        return TableAccessibility.items(artifact, rootAccessibilityNodes())
    }

    private fun rootNodeId(node: ViewerTablePresentedAccessibilityNode): Int? {
        val identity = node.sourceIdentity
        return accessibilityNodeRegistry.idOf(identity, null) {
            rootAccessibilityNodes().firstOrNull { it.sourceIdentity == identity }
        }
    }

    private fun cellNodeId(
        node: ViewerTablePresentedAccessibilityNode,
        parentId: Int,
        resolve: () -> ViewerTablePresentedAccessibilityNode?
    ): Int? = accessibilityNodeRegistry.idOf(node.sourceIdentity, parentId, resolve)

    private val accessibilityNodeRegistry = PreparedProseAccessibilityNodeRegistry()

    private fun registeredAccessibilityNode(id: Int): ViewerTablePresentedAccessibilityNode? =
        accessibilityNodeRegistry.node(id) ?: run {
            accessibilityChildIds()
            accessibilityNodeRegistry.node(id)
        }

    private fun registeredAccessibilityParent(id: Int): Int? =
        accessibilityNodeRegistry.parentId(id)

    internal fun accessibilityVirtualIdForTesting(sourceIdentity: String): Int? {
        accessibilityChildIds()
        tableAccessibility.registerInteractionsForTesting(::cellNodeId)
        return accessibilityNodeRegistry.idOf(sourceIdentity)
    }

    internal fun presentedAccessibilityCell(
        cell: TableAccessibilityCell
    ): ViewerTablePresentedCell? = tableAccessibility.presentedCell(cell)

    internal fun tableAccessibilityLocation(
        surface: ViewerTableSurface,
        sourceIndex: Int
    ): TableAccessibilityLocation? = tableAccessibility.locate(surface, sourceIndex)

    internal fun revealTableAccessibilityCell(target: TableAccessibilityCell) {
        if (onPrepareTableCellGeometry?.invoke(target.surface.identity, target.sourceIndex) ==
            false
        ) {
            return
        }
        val root = preparedLayout ?: return
        val surface =
            ViewerTablePresentation.surfaces(root).firstOrNull {
                it.identity ==
                    target.surface.identity
            }
                ?: return
        val cell = tableAccessibility.locate(surface, target.sourceIndex)?.cell ?: return
        val presented = tableAccessibility.presentedCell(cell) ?: return
        val visible = RectF(presented.bounds)
        val fullyVisible = visible.intersect(presented.clip) &&
            visible.width() >= minOf(presented.bounds.width(), presented.clip.width())
        val logical = surface.layout.columnWidths.take(cell.column).sum()
        if (!fullyVisible && tablePresentationOwner.logicalOffset(surface) != logical) {
            tablePresentationOwner.setLogicalOffset(logical, surface)
            tableOffsetChanged()
        }
        val revealed = tableAccessibility.presentedCell(cell) ?: return
        requestRectangleOnScreen(
            Rect(
                revealed.bounds.left.toInt(),
                revealed.bounds.top.toInt(),
                revealed.bounds.right.toInt(),
                revealed.bounds.bottom.toInt()
            ).apply { offset(contentOriginXPx, contentOriginYPx) },
            true
        )
    }

    override fun getAccessibilityNodeProvider(): AccessibilityNodeProvider = provider

    // The replacement constructors are API 30 and this module's minSdk is 24;
    // on API 30+ obtain() only delegates to them. setBoundsInParent has no
    // replacement at all, and API 24-28 services still read it.
    @Suppress("DEPRECATION")
    private val provider = object : AccessibilityNodeProvider() {
        override fun createAccessibilityNodeInfo(id: Int): AccessibilityNodeInfo? {
            if (id ==
                View.NO_ID
            ) {
                return AccessibilityNodeInfo.obtain(
                    this@PreparedProseDrawingView
                ).also(::onInitializeAccessibilityNodeInfo)
            }
            if (tableAccessibility.isTableNode(id)) {
                return tableAccessibility.create(id, ::cellNodeId)
            }
            val presented = registeredAccessibilityNode(id) ?: return null
            val node = presented.node
            val parentCell = registeredAccessibilityParent(id)
            val parentBounds = accessibilityParentBounds(presented)
            val screen = accessibilityScreenBounds(parentBounds)
            val visibleToUser = accessibilityNodeVisible(presented)
            reconcileVirtualAccessibilityFocus()
            val identity = identity(presented)
            return AccessibilityNodeInfo.obtain().apply {
                packageName = context.packageName
                className = android.widget.Button::class.java.name
                setSource(this@PreparedProseDrawingView, id)
                if (parentCell != null) {
                    setParent(this@PreparedProseDrawingView, parentCell)
                } else {
                    setParent(this@PreparedProseDrawingView)
                }
                text = node.label
                contentDescription = node.label
                isClickable = true
                isFocusable = true
                AndroidApiCompat.setScreenReaderFocusable(this, true)
                isAccessibilityFocused = focusedVirtualNode?.identity == identity
                isVisibleToUser = visibleToUser
                setBoundsInParent(parentBounds)
                setBoundsInScreen(screen)
                addAction(AccessibilityNodeInfo.AccessibilityAction.ACTION_CLICK)
                addAction(
                    if (isAccessibilityFocused) {
                        AccessibilityNodeInfo.AccessibilityAction.ACTION_CLEAR_ACCESSIBILITY_FOCUS
                    } else {
                        AccessibilityNodeInfo.AccessibilityAction.ACTION_ACCESSIBILITY_FOCUS
                    }
                )
                AccessibilityNodeInfoCompat.wrap(this).roleDescription =
                    if (node.role == PreparedProseAccessibilityNode.Role.LINK) "link" else "mention"
            }
        }

        override fun performAction(id: Int, action: Int, arguments: Bundle?): Boolean {
            if (tableAccessibility.isTableNode(id)) {
                return tableAccessibility.perform(id, action)
            }
            val node = registeredAccessibilityNode(id) ?: return false
            return when (action) {
                AccessibilityNodeInfo.ACTION_CLICK -> if (accessibilityNodeVisible(node)) {
                    node.node.interactionIndex?.let { index ->
                        node.layout.interactions.getOrNull(index)
                    }?.let {
                        onInteractionActivated?.invoke(it) ?: false
                    } ?: false
                } else {
                    false
                }

                AccessibilityNodeInfo.ACTION_ACCESSIBILITY_FOCUS ->
                    requestVirtualAccessibilityFocus(
                        id
                    )

                AccessibilityNodeInfo.ACTION_CLEAR_ACCESSIBILITY_FOCUS ->
                    clearVirtualAccessibilityFocus(
                        id
                    )

                else -> false
            }
        }
    }

    private fun interactionEnabled(kind: PreparedProseInteraction.Kind): Boolean = when (kind) {
        PreparedProseInteraction.Kind.LINK -> linkInteractionsEnabled
        PreparedProseInteraction.Kind.MENTION -> mentionInteractionsEnabled
    }

    private fun requestVirtualAccessibilityFocus(id: Int): Boolean {
        val node = registeredAccessibilityNode(id) ?: return false
        if (!accessibilityNodeVisible(node)) return false
        val identity = identity(node)
        if (focusedVirtualNode?.identity == identity) return false
        clearVirtualAccessibilityFocus()
        tableAccessibility.clearFocus()
        focusedVirtualNode = FocusedVirtualNode(id, identity)
        invalidate()
        sendVirtualAccessibilityEvent(id, AccessibilityEvent.TYPE_VIEW_ACCESSIBILITY_FOCUSED)
        return true
    }

    private fun clearVirtualAccessibilityFocus(
        id: Int = focusedVirtualNode?.virtualId ?: View.NO_ID
    ): Boolean {
        val focused = focusedVirtualNode ?: return false
        if (id == View.NO_ID || id != focused.virtualId) return false
        focusedVirtualNode = null
        invalidate()
        sendVirtualAccessibilityEvent(id, AccessibilityEvent.TYPE_VIEW_ACCESSIBILITY_FOCUS_CLEARED)
        return true
    }

    private fun reconcileVirtualAccessibilityFocus() {
        tableAccessibility.reconcile()
        val focused = focusedVirtualNode ?: return
        val node = registeredAccessibilityNode(focused.virtualId)
        if (node == null || identity(node) != focused.identity || !accessibilityNodeVisible(node)) {
            clearVirtualAccessibilityFocus(focused.virtualId)
        }
    }

    private fun accessibilityNodeVisible(node: ViewerTablePresentedAccessibilityNode): Boolean =
        accessibilityParentBounds(node).let { parentBounds ->
            if (parentBounds.isEmpty) return false
            accessibilityScreenBounds(parentBounds).let { bounds ->
                accessibilityVisibilityForTesting?.invoke(bounds)
                    ?: accessibilityNodeVisibleOnScreen(bounds)
            }
        }

    private fun accessibilityParentBounds(node: ViewerTablePresentedAccessibilityNode): Rect {
        val bounds = RectF(node.bounds)
        if (!bounds.intersect(node.clip)) return Rect()
        return Rect(
            bounds.left.toInt(),
            bounds.top.toInt(),
            bounds.right.toInt(),
            bounds.bottom.toInt()
        ).apply {
            offset(contentOriginXPx, contentOriginYPx)
        }
    }

    private fun accessibilityScreenBounds(parentBounds: Rect): Rect {
        val bounds = Rect(parentBounds)
        val location = IntArray(2)
        getLocationOnScreen(location)
        bounds.offset(location[0], location[1])
        return bounds
    }

    private fun identity(node: ViewerTablePresentedAccessibilityNode) = AccessibilityNodeIdentity(
        node.sourceIdentity,
        node.node.interactionIndex ?: -1,
        node.node.role,
        node.node.label
    )

    // AccessibilityEvent(Int) is API 30; see the node provider above.
    @Suppress("DEPRECATION")
    private fun sendVirtualAccessibilityEvent(id: Int, type: Int) {
        if (!accessibilityManager.isEnabled) return
        val event = AccessibilityEvent.obtain(type).apply {
            packageName = context.packageName
            className = android.widget.Button::class.java.name
            setSource(this@PreparedProseDrawingView, id)
        }
        parent?.requestSendAccessibilityEvent(this, event)
    }

    /** Publishes a logical prepared-subtree transition without changing its artifact. */
    @Suppress("DEPRECATION")
    internal fun announceAccessibilitySubtreeChanged() {
        if (!publishesAccessibilitySubtree || !accessibilityManager.isEnabled) return
        val event = AccessibilityEvent.obtain(
            AccessibilityEvent.TYPE_WINDOW_CONTENT_CHANGED
        ).apply {
            packageName = context.packageName
            className = android.widget.TextView::class.java.name
            contentChangeTypes = AccessibilityEvent.CONTENT_CHANGE_TYPE_SUBTREE
            setSource(this@PreparedProseDrawingView)
        }
        parent?.requestSendAccessibilityEvent(this, event)
    }

    private data class AccessibilityNodeIdentity(
        val generation: String,
        val interactionIndex: Int,
        val role: PreparedProseAccessibilityNode.Role,
        val label: String
    )

    private data class FocusedVirtualNode(
        val virtualId: Int,
        val identity: AccessibilityNodeIdentity
    )
}

internal fun View.accessibilityNodeVisibleOnScreen(screenBounds: Rect): Boolean {
    val visibleBounds = Rect()
    val hasGlobalVisibleBounds = getGlobalVisibleRect(visibleBounds)
    return accessibilityBoundsVisible(
        screenBounds,
        visibleBounds.takeIf { hasGlobalVisibleBounds },
        isShown,
        windowVisibility == View.VISIBLE,
        hasVisibleAlpha()
    )
}

internal fun accessibilityBoundsVisible(
    screenBounds: Rect,
    globalVisibleBounds: Rect?,
    shown: Boolean,
    windowVisible: Boolean,
    alphaVisible: Boolean
): Boolean {
    if (!shown || !windowVisible || !alphaVisible || globalVisibleBounds?.isEmpty != false) {
        return false
    }
    return Rect(globalVisibleBounds).run {
        intersect(screenBounds) && !isEmpty
    }
}

private fun View.hasVisibleAlpha(): Boolean {
    var current: View? = this
    while (current != null) {
        if (current.alpha <= 0f) return false
        current = current.parent as? View
    }
    return true
}
