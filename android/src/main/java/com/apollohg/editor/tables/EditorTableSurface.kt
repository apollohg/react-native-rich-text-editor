package com.apollohg.editor.tables

import com.apollohg.editor.viewer.PreparedProseInstrumentation
import com.apollohg.editor.TableScalarExtent
import com.apollohg.editor.renderedTextMatches
import android.content.Context
import android.graphics.Canvas
import android.graphics.Paint
import android.graphics.Rect
import android.graphics.RectF
import android.text.Annotation
import android.text.SpannableStringBuilder
import android.text.Spanned
import android.text.style.ReplacementSpan
import android.view.DragEvent
import android.view.Gravity
import android.view.View
import android.view.ViewGroup
import android.view.MotionEvent
import android.view.KeyEvent
import android.view.ViewConfiguration
import android.view.inputmethod.InputMethodManager
import android.widget.FrameLayout
import kotlin.math.ceil
import kotlin.math.roundToInt
import com.apollohg.editor.EditorClipboard
import com.apollohg.editor.EditorEditText
import com.apollohg.editor.EditorPasteMode
import com.apollohg.editor.canMutateSelectedTableCells
import com.apollohg.editor.prepareForExternalInteractionMutation
import com.apollohg.editor.EditorTextStyle
import com.apollohg.editor.EditorV2Adapter
import com.apollohg.editor.EditorV2Registry
import com.apollohg.editor.exactV2ScalarInt
import com.apollohg.editor.isAuthorizedForTableCellInput
import com.apollohg.editor.inputScalarSelection
import com.apollohg.editor.syncCurrentSelectionToRust
import com.apollohg.editor.PositionBridge
import com.apollohg.editor.commandAtSelection
import com.apollohg.editor.RenderBridge
import com.apollohg.editor.selectExactTableCells
import com.apollohg.editor.resizeTableColumn
import com.apollohg.editor.deleteTable
import com.apollohg.editor.applyTableCommandAtSelection
import com.apollohg.editor.TableMutationAdmission
import com.apollohg.editor.tableMutationAdmission
import com.apollohg.editor.admitsTableMutation
import com.apollohg.editor.adoptCurrentRootTableMapEpoch
import com.apollohg.editor.cachedAtomicRenderSelection
import com.apollohg.editor.readOnlyParsedUpdate
import com.apollohg.editor.updateSelection
import com.apollohg.editor.cellSelectionEndpoints
import com.apollohg.editor.RichTextEditorView
import com.apollohg.editor.canonicalV2U64
import com.apollohg.editor.applyRenderedSpannable
import com.apollohg.editor.applySelectionFromJSON
import com.apollohg.editor.isAuthorizedForRootTableInput
import com.apollohg.editor.hasAuthorizedNativeTableOwner
import com.apollohg.editor.retireInputConnectionForEditor
import com.apollohg.editor.updateAtomBoundaryCursorVisibility
import com.apollohg.editor.viewer.PreparedCellShapeCatalog
import com.apollohg.editor.viewer.PreparedProseBlock
import com.apollohg.editor.viewer.PreparedProseDrawingView
import com.apollohg.editor.viewer.RemoteTableCellSelection
import com.apollohg.editor.viewer.TableCellDropTarget
import com.apollohg.editor.viewer.TableSelectionHandleRole
import com.apollohg.editor.viewer.TableResizeEdge
import com.apollohg.editor.viewer.PreparedProseLayout
import com.apollohg.editor.viewer.PreparedProseTheme
import com.apollohg.editor.viewer.ProseLayoutKey
import com.apollohg.editor.viewer.StaticLayoutAndroidProseLayoutEngine
import com.apollohg.editor.viewer.ViewerBlock
import com.apollohg.editor.viewer.ViewerDocument
import org.json.JSONObject

internal class RootTableHeightSpan(val heightPx: Int) : ReplacementSpan() {
    override fun getSize(paint: Paint, text: CharSequence?, start: Int, end: Int,
                         fm: Paint.FontMetricsInt?): Int {
        fm?.let {
            it.ascent = -heightPx
            it.top = it.ascent
            it.descent = 0
            it.bottom = 0
        }
        return 0
    }

    override fun draw(canvas: Canvas, text: CharSequence?, start: Int, end: Int,
                      x: Float, top: Int, y: Int, bottom: Int, paint: Paint) = Unit
}

internal data class TableSelectionObstructions(val safeArea: RectF, val keyboard: RectF?)

internal data class TableSelectionGeometry(
    val editorId: String,
    val documentRevision: String,
    val layoutEpoch: String,
    val tablePos: UInt,
    val rects: List<RectF>,
    val viewport: RectF,
    val obstructions: TableSelectionObstructions,
    val editMenuVisible: Boolean
) {
    fun eventPayload(): Map<String, Any> = buildMap {
        put("editorId", editorId)
        put("documentRevision", documentRevision)
        put("layoutEpoch", layoutEpoch)
        put("tablePos", tablePos.toLong())
        put("coordinateSpace", COORDINATE_SPACE)
        put("rects", rects.map(::rectPayload))
        put("viewport", rectPayload(viewport))
        put("safeArea", rectPayload(obstructions.safeArea))
        put("editMenuVisible", editMenuVisible)
        obstructions.keyboard?.let { put("keyboard", rectPayload(it)) }
    }

    private fun rectPayload(rect: RectF): Map<String, Double> = mapOf(
        "x" to rect.left.toDouble(),
        "y" to rect.top.toDouble(),
        "width" to rect.width().toDouble(),
        "height" to rect.height().toDouble()
    )

    private companion object {
        const val COORDINATE_SPACE = "window"
    }
}

internal class EditorTableSurface(private val host: RichTextEditorView) : TableAccessibilityEditing {
    private companion object {
        const val HANDLE_EDGE_BAND_DP = 48f
        const val HANDLE_SCROLL_STEP_DP = 12f
        const val MAXIMUM_COLUMN_WIDTH = 10_000
    }
    private data class Entry(val surface: ViewerTableSurface, val localBounds: Rect,
                             val occupiedHeight: Int, val minimumColumnWidth: Int, val appearance: String)

    private fun cellDocumentPosition(tableId: String, sourceIndex: Int): Int? {
        val adapter = host.editorEditText.v2Driver as? EditorV2Adapter ?: return null
        return adapter.tableIndex.docStart(tableId, sourceIndex)?.toLong()?.takeIf { it <= Int.MAX_VALUE }?.toInt()
    }

    private class ReusableCellContents(entry: Entry?, appearance: String) {
        private data class Key(val contentKey: String, val header: Boolean, val attributesKey: String, val widthPx: Int)

        private val contents = mutableMapOf<Key, ArrayDeque<PreparedViewerTableCell>>()

        init {
            val reusable = entry?.takeIf { it.appearance == appearance }
            val source = reusable?.surface?.sourceTable
            reusable?.surface?.cells?.forEach { cell ->
                val sourceCell = source?.cells?.getOrNull(cell.sourceIndex)
                if (sourceCell != null && cell.isPositionFree) {
                    val key = Key(sourceCell.contentKey, sourceCell.header, sourceCell.attrsKey, cell.contentKey.widthPx)
                    contents.getOrPut(key) { ArrayDeque() }.addLast(cell)
                }
            }
        }

        fun take(cell: TableSurfaceCell, widthPx: Int): PreparedViewerTableCell? =
            contents[Key(cell.contentKey, cell.header, cell.attrsKey, widthPx)]?.removeFirstOrNull()
    }
    private data class TableResizePreview(val edge: TableResizeEdge, val width: Int)
    private data class PreparationKey(val adapter: EditorV2Adapter, val revision: ULong,
                                      val width: Int, val appearanceRevision: Long,
                                      val documentGeneration: Long,
                                      val resizePreview: TableResizePreview?,
                                      val tableDirection: TableLayoutDirection)

    var hostTableDirection: TableLayoutDirection? = null
    var onSelectionGeometryMayChange: (() -> Unit)? = null

    val drawingView = PreparedProseDrawingView(host.context).apply {
        usesEditAnchoredNodes = true
        isFocusable = false
        linkInteractionsEnabled = false
        mentionInteractionsEnabled = false
        tableAccessibilityEditing = this@EditorTableSurface
        tableCellDocumentPosition = ::cellDocumentPosition
        tableDocumentPosition = { tableId ->
            (host.editorEditText.v2Driver as? EditorV2Adapter)?.tableIndex?.tableDocStart(tableId)?.toInt()
        }
        setBackgroundColor(android.graphics.Color.TRANSPARENT)
        onTableGeometryChanged = {
            positionActiveInput()
            selectionGeometryMayChange()
        }
        onTableTap = { downX, downY, upX, upY ->
            val target = hitCell(downX, downY)?.takeIf { it == hitCell(upX, upY) }
            cellDragLifted || tapCellSelection(target, upX, upY) ||
                target != null && activateCell(target.first, target.second, upX, upY)
        }
    }
    private val cellEditMenu by lazy {
        TableCellEditMenu(host.editorEditText, ::cellEditMenuAnchor) { onSelectionGeometryMayChange?.invoke() }
    }
    private var presentedCellEditMenuSelection: Triple<String, Int, Int>? = null
    private val doubleTapTimeoutMs = ViewConfiguration.getDoubleTapTimeout().toLong()
    private var pendingCellEditMenuToggle: Runnable? = null
    private val longPressTimeoutMs = ViewConfiguration.getLongPressTimeout().toLong()
    private val touchSlop = ViewConfiguration.get(host.context).scaledTouchSlop.toFloat()
    private data class PendingCellDrag(val start: Runnable, val downX: Float, val downY: Float)
    private var pendingCellDrag: PendingCellDrag? = null
    private var cellDragLifted = false
    val isCellEditMenuVisible: Boolean get() = cellEditMenu.isVisible
    internal var incrementalRelayoutsForTesting = 0
        private set
    internal var onTableCellPreparedForTesting: ((Int, String) -> Unit)? = null
    private val cellShapes = PreparedCellShapeCatalog()
    private data class ActiveCell(val tableId: String, val cellIndex: Int)
    private var activeCell: ActiveCell? = null
    private var pinnedInputCell: PreparedViewerTableCell? = null
    private var applyingCellUpdate = false
    private var accessibilityKey: Triple<ULong?, Int, Int?>? = null
    private var activeAppearanceRevision: Long? = null
    private var coordinator: EditorTableInputCoordinator? = null
    val activeInput: EditorEditText? get() = coordinator?.cellInput?.takeIf { activeCell != null }
    fun nativeTextSelectionActive(): Boolean = activeInput?.let {
        it.selectionStart != it.selectionEnd
    } == true
    private var entries: Map<String, Entry> = emptyMap()
    private var key: PreparationKey? = null
    private var positionedBlocks: List<PreparedProseBlock> = emptyList()
    private val hostGestureAxis = TableGestureAxisLock(host.context)
    private sealed class TableDrag(
        val adapter: EditorV2Adapter,
        val admission: TableMutationAdmission,
        val pointerId: Int,
        var screenX: Float,
        var screenY: Float
    )
    private class HandleDrag(
        adapter: EditorV2Adapter,
        admission: TableMutationAdmission,
        val role: TableSelectionHandleRole,
        pointerId: Int,
        var epoch: String,
        val offsetX: Float,
        val offsetY: Float,
        var anchor: Int,
        var head: Int,
        screenX: Float,
        screenY: Float
    ) : TableDrag(adapter, admission, pointerId, screenX, screenY)
    private class ResizeDrag(
        adapter: EditorV2Adapter,
        admission: TableMutationAdmission,
        val edge: TableResizeEdge,
        val startWidth: Float,
        private val minimumWidth: Int,
        val directionSign: Float,
        pointerId: Int,
        screenX: Float,
        screenY: Float
    ) : TableDrag(adapter, admission, pointerId, screenX, screenY) {
        val startX = screenX
        var scrolledLogical = 0f
        var previewWidth = clampedWidth(startWidth)

        fun clampedWidth(requested: Float): Int =
            requested.roundToInt().coerceIn(minimumWidth, MAXIMUM_COLUMN_WIDTH)
    }
    private data class ResizeCandidate(
        val adapter: EditorV2Adapter,
        val admission: TableMutationAdmission,
        val edge: TableResizeEdge,
        val pointerId: Int,
        val screenX: Float,
        val screenY: Float
    )
    private var activeDrag: TableDrag? = null
    private var resizeCandidate: ResizeCandidate? = null
    private val resizeGestureAxis = TableGestureAxisLock(host.context)
    private var resizePreview: TableResizePreview? = null
    private var dragFramePosted = false
    private var runningDragFrame = false
    private val dragFrame = object : Runnable {
        override fun run() {
            dragFramePosted = false
            val drag = activeDrag ?: return
            if (runningDragFrame || !validDrag(drag)) {
                cancelActiveDrag()
                return
            }
            runningDragFrame = true
            try {
                val density = host.resources.displayMetrics.density
                val band = HANDLE_EDGE_BAND_DP * density
                val step = HANDLE_SCROLL_STEP_DP * density
                val tableId = drag.admission.tableId
                val viewport = drawingView.selectedTableViewport(tableId)
                var scrolled = false
                if (viewport != null) {
                    val x = drag.screenX - drawingView.left
                    val horizontal = when {
                        x < viewport.left + band -> -step
                        x > viewport.right - band -> step
                        else -> 0f
                    }
                    if (horizontal != 0f) {
                        val before = drawingView.tableLogicalOffset(tableId)
                        scrolled = drawingView.scrollSelectedTablePhysical(tableId, horizontal) != 0f
                        if (scrolled && drag is ResizeDrag && before != null) {
                            drag.scrolledLogical += (drawingView.tableLogicalOffset(tableId) ?: before) - before
                        }
                    }
                }
                if (activeDrag !== drag || !validDrag(drag)) {
                    if (activeDrag === drag) cancelActiveDrag()
                    return
                }
                if (drag is HandleDrag) {
                    val scroll = host.editorScrollView
                    val visible = Rect()
                    if (!scroll.getLocalVisibleRect(visible)) {
                        cancelActiveDrag()
                        return
                    }
                    val viewportTop = maxOf(visible.top.toFloat() - scroll.scrollY,
                        scroll.paddingTop.toFloat())
                    val viewportBottom = minOf(visible.bottom.toFloat() - scroll.scrollY,
                        (scroll.height - scroll.paddingBottom).toFloat())
                    val vertical = when {
                        drag.screenY < viewportTop + band -> -step
                        drag.screenY > viewportBottom - band -> step
                        else -> 0f
                    }
                    if (vertical != 0f) {
                        val prior = scroll.scrollY
                        scroll.scrollBy(0, vertical.roundToInt())
                        scrolled = scrolled || scroll.scrollY != prior
                    }
                }
                if (scrolled && validDrag(drag)) retargetDrag(drag)
                if (scrolled && activeDrag === drag) scheduleDragFrame()
            } finally {
                runningDragFrame = false
            }
        }
    }

    fun beginHostGesture() = hostGestureAxis.reset()

    private fun viewportY(frameY: Float): Float =
        frameY + host.editorContentFrame.top - host.editorScrollView.scrollY

    private fun tableInteractionAdapter(expected: EditorV2Adapter? = null): EditorV2Adapter? {
        val root = host.editorEditText
        val adapter = root.v2Driver as? EditorV2Adapter ?: return null
        if (expected != null && adapter !== expected) return null
        if (!root.isEnabled || !root.isEditable || root.hasPendingCompositionForExternalRefresh() ||
            activeInput?.hasPendingCompositionForExternalRefresh() == true ||
            nativeTextSelectionActive() || !root.hasAuthorizedNativeTableOwner(adapter) ||
            adapter.cachedAtomicRenderDocumentRevision != adapter.baseDocumentRevision ||
            root.lastAppliedDocumentVersion != adapter.baseDocumentRevision.toString()) return null
        return adapter
    }

    fun beginFrameGesture(event: MotionEvent): Boolean {
        resizeCandidate = null
        resizeGestureAxis.reset()
        cancelPendingCellDrag()
        cellDragLifted = false
        if (event.actionMasked == MotionEvent.ACTION_DOWN &&
            !cellSelectionContains(event.x - drawingView.left, event.y - drawingView.top)) {
            cancelPendingCellEditMenuToggle()
            dismissCellEditMenu()
        }
        if (event.actionMasked != MotionEvent.ACTION_DOWN || event.pointerCount != 1) return false
        if (startHandleDrag(event)) return true
        armResize(event)
        armCellDrag(event)
        return false
    }

    fun trackFrameGesture(event: MotionEvent) {
        val pending = pendingCellDrag ?: return
        val moved = event.actionMasked == MotionEvent.ACTION_MOVE &&
            (event.x - pending.downX) * (event.x - pending.downX) +
            (event.y - pending.downY) * (event.y - pending.downY) > touchSlop * touchSlop
        if (moved || event.actionMasked != MotionEvent.ACTION_MOVE) cancelPendingCellDrag()
    }

    private fun armCellDrag(event: MotionEvent) {
        if (resizeCandidate != null) return
        val x = event.x - drawingView.left
        val y = event.y - drawingView.top
        if (cellDragSource(x, y) == null) return
        val start = Runnable {
            pendingCellDrag = null
            startCellDrag(x, y)
        }
        pendingCellDrag = PendingCellDrag(start, event.x, event.y)
        drawingView.postDelayed(start, longPressTimeoutMs)
    }

    private fun cancelPendingCellDrag() {
        pendingCellDrag?.let { drawingView.removeCallbacks(it.start) }
        pendingCellDrag = null
    }

    private fun cellDragSource(x: Float, y: Float): TableCellDragSource? {
        if (activeDrag != null || activeCell != null || !cellSelectionContains(x, y) ||
            drawingView.hitSelectionHandle(x, y) != null || drawingView.hitResizeEdge(x, y) != null ||
            host.editorEditText.hasPendingCompositionForExternalRefresh() ||
            drawingView.ownsHorizontalTableGesture) return null
        val (tableId, anchor, head) = cellEditMenuSelection() ?: return null
        val sourceIndices = drawingView.selectedTableCellSourceIndices[tableId] ?: return null
        return TableCellDragSource(tableId, anchor, head, sourceIndices)
    }

    fun startCellDrag(x: Float, y: Float): TableCellDragState? {
        val source = cellDragSource(x, y) ?: return null
        val root = host.editorEditText
        val adapter = root.v2Driver as? EditorV2Adapter ?: return null
        val payload = adapter.clipboardJson()?.let(EditorClipboard::fromExportJson) ?: return null
        val visible = Rect()
        if (!drawingView.getLocalVisibleRect(visible)) return null
        val cellRects = clipped(drawingView.selectedTableCellRects(source.tableId), RectF(visible))
            ?.takeIf { it.isNotEmpty() }
            ?: return null
        val state = TableCellDragState(root.editorId, adapter, adapter.baseDocumentRevision, source, payload,
            root.isEditable && root.canMutateSelectedTableCells())
        if (!drawingView.startDragAndDrop(EditorClipboard.create(payload),
                TableCellDragShadow(drawingView, cellRects, x, y), state, View.DRAG_FLAG_GLOBAL)) return null
        cellDragLifted = true
        cancelPendingCellEditMenuToggle()
        dismissCellEditMenu()
        drawingView.cancelTableInteraction()
        return state
    }

    private sealed interface TableCellDropResolution {
        data object SelfDrop : TableCellDropResolution
        data object Refused : TableCellDropResolution
        data class Accepted(
            val target: TableCellDropTarget,
            val adapter: EditorV2Adapter,
            val revision: ULong,
            val dragged: TableCellDragState?,
            val moved: TableCellDragSource?,
            val pastesIntoSelection: Boolean
        ) : TableCellDropResolution
    }

    fun onRootDragEvent(event: DragEvent): Boolean? = when (event.action) {
        DragEvent.ACTION_DRAG_LOCATION -> {
            val resolution = resolveTableCellDrop(event)
            drawingView.tableCellDropTarget = (resolution as? TableCellDropResolution.Accepted)?.target
            resolution?.let { true }
        }
        DragEvent.ACTION_DROP -> {
            drawingView.tableCellDropTarget = null
            when (val resolution = resolveTableCellDrop(event)) {
                null -> null
                TableCellDropResolution.SelfDrop, TableCellDropResolution.Refused -> false
                is TableCellDropResolution.Accepted -> performTableCellDrop(resolution, event)
            }
        }
        DragEvent.ACTION_DRAG_EXITED, DragEvent.ACTION_DRAG_ENDED -> {
            drawingView.tableCellDropTarget = null
            null
        }
        else -> null
    }

    private fun resolveTableCellDrop(event: DragEvent): TableCellDropResolution? {
        val root = host.editorEditText
        val x = event.x + root.left - drawingView.left
        val y = event.y + root.top - drawingView.top
        val (tableId, presented) = rootCellAt(x, y)
            ?: return if (drawingView.hasTableAt(x, y)) TableCellDropResolution.Refused else null
        val target = TableCellDropTarget(tableId, presented.sourceIndex)
        val dragged = event.localState as? TableCellDragState
        val sameEditor = dragged?.takeIf { it.editorId == root.editorId && it.adapter === root.v2Driver }
        if (sameEditor != null && sameEditor.source.tableId == tableId &&
            target.sourceIndex in sameEditor.source.sourceIndices) return TableCellDropResolution.SelfDrop
        if (root.pasteMode == EditorPasteMode.DISABLED) return TableCellDropResolution.Refused
        val (adapter) = tableMutationContext(tableId) ?: return TableCellDropResolution.Refused
        val revision = adapter.baseDocumentRevision
        val moved = if (sameEditor?.movable == true) {
            if (sameEditor.documentRevision != revision ||
                tableMutationContext(sameEditor.source.tableId) == null) return TableCellDropResolution.Refused
            sameEditor.source
        } else null
        val pastesIntoSelection = sameEditor == null &&
            drawingView.selectedTableCellSourceIndices[tableId]?.contains(target.sourceIndex) == true &&
            root.isEditable && root.canMutateSelectedTableCells()
        return TableCellDropResolution.Accepted(target, adapter, revision, dragged, moved, pastesIntoSelection)
    }

    private fun performTableCellDrop(drop: TableCellDropResolution.Accepted, event: DragEvent): Boolean {
        val root = host.editorEditText
        val payload = drop.dragged?.payload ?: event.clipData?.let { EditorClipboard.read(it, root.context) }
            ?: return false
        if (payload.fragment == null && payload.html == null && payload.text.isNullOrEmpty()) return false
        if (!(activeInput ?: root).prepareForExternalInteractionMutation() ||
            drop.adapter.baseDocumentRevision != drop.revision ||
            tableMutationContext(drop.target.tableId) == null) return false
        val plainText = root.pasteMode == EditorPasteMode.PLAIN_TEXT
        val update = if (drop.pastesIntoSelection) {
            drop.adapter.pasteAtEngineSelection(payload.fragment, payload.html, payload.text, plainText)
        } else {
            drop.adapter.pasteIntoTableCell(payload, plainText,
                cellDocumentPosition(drop.target.tableId, drop.target.sourceIndex) ?: return false,
                drop.moved?.let { it.anchor to it.head })
        } ?: return false
        return applyTableMutationUpdate(update)
    }

    private fun startHandleDrag(event: MotionEvent): Boolean {
        if (activeCell != null) return false
        val adapter = tableInteractionAdapter() ?: return false
        val epoch = adapter.positionEpoch ?: return false
        val handle = drawingView.hitSelectionHandle(event.x - drawingView.left,
            event.y - drawingView.top) ?: return false
        if (handle.tableId !in entries) return false
        val admission = adapter.tableMutationAdmission(handle.tableId)
        if (!adapter.admitsTableMutation(admission)) return false
        val selection = adapter.cachedAtomicRenderSelection() ?: return false
        if (selection.optString("type") != "cell") return false
        val anchor = exactV2ScalarInt(selection.opt("anchorCell") as? Number) ?: return false
        val head = exactV2ScalarInt(selection.opt("headCell") as? Number) ?: return false
        val resolved = resolveEditorCellSelection(selection, adapter.tableIndex)
            as? EditorCellSelection.Drawable ?: return false
        if (resolved.tableId != handle.tableId) return false
        cancelActiveDrag()
        dismissCellEditMenu()
        drawingView.cancelTableInteraction()
        activeDrag = HandleDrag(adapter, admission, handle.role, event.getPointerId(0), epoch,
            event.x - drawingView.left - handle.x, event.y - drawingView.top - handle.y,
            anchor, head, event.x, viewportY(event.y))
        host.editorContentFrame.parent?.requestDisallowInterceptTouchEvent(true)
        return true
    }

    private fun activeInputContains(x: Float, y: Float): Boolean {
        val frame = activeInput?.layoutParams as? FrameLayout.LayoutParams ?: return false
        return x >= frame.leftMargin && x < frame.leftMargin + frame.width &&
            y >= frame.topMargin && y < frame.topMargin + frame.height
    }

    private fun armResize(event: MotionEvent) {
        if (activeDrag != null || activeInputContains(event.x, event.y)) return
        val adapter = tableInteractionAdapter() ?: return
        val x = event.x - drawingView.left
        val y = event.y - drawingView.top
        if (drawingView.hitSelectionHandle(x, y) != null) return
        val edge = drawingView.hitResizeEdge(x, y) ?: return
        if (edge.tableId !in entries) return
        val admission = adapter.tableMutationAdmission(edge.tableId)
        if (!adapter.admitsTableMutation(admission)) return
        resizeCandidate = ResizeCandidate(adapter, admission, edge, event.getPointerId(0),
            event.x, viewportY(event.y))
    }

    fun resizeClaimsGesture(event: MotionEvent, dx: Float, dy: Float): Boolean {
        activeDrag?.let { return it is ResizeDrag }
        val candidate = resizeCandidate ?: return false
        if (event.actionMasked != MotionEvent.ACTION_MOVE || event.pointerCount != 1 ||
            event.getPointerId(0) != candidate.pointerId) {
            resizeCandidate = null
            return false
        }
        return when (resizeGestureAxis.update(dx, dy) { true }) {
            TableGestureAxis.UNDECIDED -> true
            TableGestureAxis.VERTICAL -> {
                resizeCandidate = null
                false
            }
            TableGestureAxis.HORIZONTAL -> {
                resizeCandidate = null
                beginResizeDrag(candidate, candidate.screenX + dx)
            }
        }
    }

    private fun beginResizeDrag(candidate: ResizeCandidate, screenX: Float): Boolean {
        if (tableInteractionAdapter(candidate.adapter) == null ||
            !candidate.adapter.admitsTableMutation(candidate.admission)) return false
        val entry = entries[candidate.edge.tableId] ?: return false
        val width = entry.surface.layout.columnWidths.getOrNull(candidate.edge.column) ?: return false
        drawingView.cancelTableInteraction()
        val drag = ResizeDrag(candidate.adapter, candidate.admission, candidate.edge,
            width / host.resources.displayMetrics.density, entry.minimumColumnWidth,
            if (entry.surface.isRightToLeft) -1f else 1f, candidate.pointerId,
            candidate.screenX, candidate.screenY)
        activeDrag = drag
        drawingView.activeTableResizeEdge = candidate.edge
        host.editorContentFrame.parent?.requestDisallowInterceptTouchEvent(true)
        drag.screenX = screenX
        updateResizePreview(drag)
        if (activeDrag === drag) scheduleDragFrame()
        return activeDrag === drag
    }

    fun dragActive(): Boolean = activeDrag != null

    fun onDragTouch(event: MotionEvent): Boolean {
        val drag = activeDrag ?: return false
        when (event.actionMasked) {
            MotionEvent.ACTION_DOWN -> return true
            MotionEvent.ACTION_POINTER_DOWN, MotionEvent.ACTION_POINTER_UP,
            MotionEvent.ACTION_CANCEL -> {
                cancelActiveDrag()
                return true
            }
            MotionEvent.ACTION_MOVE -> {
                if (event.pointerCount != 1) {
                    cancelActiveDrag()
                    return true
                }
                val index = event.findPointerIndex(drag.pointerId)
                if (index < 0 || !validDrag(drag)) {
                    cancelActiveDrag()
                    return true
                }
                drag.screenX = event.getX(index)
                drag.screenY = viewportY(event.getY(index))
                retargetDrag(drag)
                if (activeDrag === drag) scheduleDragFrame()
                return true
            }
            MotionEvent.ACTION_UP -> {
                val completes = event.pointerCount == 1 && event.getPointerId(0) == drag.pointerId &&
                    validDrag(drag)
                if (completes) {
                    drag.screenX = event.x
                    drag.screenY = viewportY(event.y)
                    retargetDrag(drag)
                }
                if (completes && drag is ResizeDrag && activeDrag === drag) commitResizeDrag(drag)
                else cancelActiveDrag()
                return true
            }
        }
        return true
    }

    private fun validDrag(drag: TableDrag): Boolean = when (drag) {
        is HandleDrag -> validHandleDrag(drag)
        is ResizeDrag -> validResizeDrag(drag)
    }

    private fun retargetDrag(drag: TableDrag) = when (drag) {
        is HandleDrag -> updateHandleTarget(drag)
        is ResizeDrag -> updateResizePreview(drag)
    }

    private fun validHandleDrag(drag: HandleDrag): Boolean {
        val adapter = drag.adapter
        if (activeDrag !== drag || activeCell != null || tableInteractionAdapter(adapter) == null ||
            !adapter.admitsTableMutation(drag.admission) || adapter.positionEpoch != drag.epoch) return false
        val selection = adapter.cachedAtomicRenderSelection() ?: return false
        return selection.optString("type") == "cell" &&
            exactV2ScalarInt(selection.opt("anchorCell") as? Number) == drag.anchor &&
            exactV2ScalarInt(selection.opt("headCell") as? Number) == drag.head &&
            (resolveEditorCellSelection(selection, adapter.tableIndex)
                as? EditorCellSelection.Drawable)?.tableId == drag.admission.tableId
    }

    private fun validResizeDrag(drag: ResizeDrag): Boolean =
        activeDrag === drag && tableInteractionAdapter(drag.adapter) != null &&
            drag.edge.column < (entries[drag.edge.tableId]?.surface?.layout?.columnWidths?.size ?: 0) &&
            drag.adapter.admitsTableMutation(drag.admission)

    private fun updateHandleTarget(drag: HandleDrag) {
        if (!validHandleDrag(drag)) {
            cancelActiveDrag()
            return
        }
        val x = drag.screenX - drawingView.left - drag.offsetX
        val y = drag.screenY + host.editorScrollView.scrollY - host.editorContentFrame.top -
            drawingView.top - drag.offsetY
        val target = drawingView.selectedTableCellAt(x, y, drag.admission.tableId) ?: return
        val anchor = if (drag.role == TableSelectionHandleRole.ANCHOR) target else drag.anchor
        val head = if (drag.role == TableSelectionHandleRole.HEAD) target else drag.head
        if (anchor == drag.anchor && head == drag.head) return
        val update = drag.adapter.selectExactTableCells(anchor, head, drag.admission,
            drag.epoch, drag.anchor, drag.head) ?: run {
            cancelActiveDrag()
            return
        }
        drag.anchor = anchor
        drag.head = head
        drag.epoch = requireNotNull(drag.adapter.positionEpoch)
        if (!host.editorEditText.applyUpdateJSON(update) || !validHandleDrag(drag)) {
            cancelActiveDrag()
            return
        }
        host.editorEditText.editorListener?.onSelectionChanged(anchor, head)
    }

    private fun updateResizePreview(drag: ResizeDrag) {
        if (!validResizeDrag(drag)) {
            cancelActiveDrag()
            return
        }
        val logicalDelta = drag.directionSign * (drag.screenX - drag.startX) + drag.scrolledLogical
        val requested = drag.startWidth + logicalDelta / host.resources.displayMetrics.density
        if (!requested.isFinite()) return
        val width = drag.clampedWidth(requested)
        if (width == drag.previewWidth) return
        drag.previewWidth = width
        resizePreview = TableResizePreview(drag.edge, width)
        refresh()
    }

    private fun commitResizeDrag(drag: ResizeDrag) {
        val committed = validResizeDrag(drag) && drag.previewWidth != drag.clampedWidth(drag.startWidth)
        discardActiveDrag()
        val update = if (committed) {
            drag.adapter.resizeTableColumn(drag.edge.column, drag.previewWidth, drag.admission)
        } else null
        if (update != null) {
            if (activeCell != null) applyCellUpdate(update, notify = true, external = false)
            else host.editorEditText.applyUpdateJSON(update)
        }
        refresh()
    }

    private fun scheduleDragFrame() {
        if (dragFramePosted || activeDrag == null) return
        dragFramePosted = true
        host.editorContentFrame.postOnAnimation(dragFrame)
    }

    private fun discardActiveDrag() {
        val held = activeDrag != null
        activeDrag = null
        resizePreview = null
        drawingView.activeTableResizeEdge = null
        if (dragFramePosted) host.editorContentFrame.removeCallbacks(dragFrame)
        dragFramePosted = false
        if (held) host.editorContentFrame.parent?.requestDisallowInterceptTouchEvent(false)
    }

    fun cancelActiveDrag() {
        val previewed = resizePreview != null
        discardActiveDrag()
        if (previewed) refresh()
    }

    fun hasTableAt(x: Float, y: Float): Boolean = drawingView.hasTableAt(
        x - drawingView.left, y - drawingView.top
    )

    fun hostGestureAxis(x: Float, y: Float, dx: Float, dy: Float): TableGestureAxis =
        hostGestureAxis.update(dx, dy) {
            drawingView.canConsumeTableDragAt(x - drawingView.left, y - drawingView.top, dx)
        }

    fun presentRemoteCellSelections(selections: List<RemoteTableCellSelection>) {
        drawingView.remoteTableCellSelections = selections
    }

    fun clear() {
        cancelPendingCellEditMenuToggle()
        cancelPendingCellDrag()
        drawingView.tableCellDropTarget = null
        discardActiveDrag()
        resizeCandidate = null
        invalidateCell()
        drawingView.selectedTableCellSourceIndices = emptyMap()
        drawingView.selectedTableCellEndpoints = null
        entries = emptyMap()
        key = null
        positionedBlocks = emptyList()
        reserve(emptyMap())
        drawingView.install(null)
        (drawingView.parent as? ViewGroup)?.removeView(drawingView)
        selectionGeometryMayChange()
    }

    fun refresh() {
        refreshPresentation()
        val root = host.editorEditText
        if (root.cellEditMenuReplacesTextMenu) {
            root.cellEditMenuReplacesTextMenu = false
            presentCellEditMenu()
        }
        selectionGeometryMayChange()
    }

    private fun selectionGeometryMayChange() {
        refreshCellEditMenu()
        onSelectionGeometryMayChange?.invoke()
    }

    private fun displayedCellSelection(adapter: EditorV2Adapter): Triple<String, Int, Int>? {
        val selection = adapter.cachedAtomicRenderSelection() ?: return null
        val cells = resolveEditorCellSelection(selection, adapter.tableIndex)
            as? EditorCellSelection.Drawable ?: return null
        if (cells.tableId !in drawingView.selectedTableCellSourceIndices) return null
        val (anchor, head) = cellSelectionEndpoints(selection) ?: return null
        return Triple(cells.tableId, anchor, head)
    }

    private fun cellEditMenuSelection(): Triple<String, Int, Int>? {
        val root = host.editorEditText
        val adapter = root.v2Driver as? EditorV2Adapter ?: return null
        if (adapter.destroyed || !root.isAttachedToWindow || !root.hasFocus() || activeCell != null ||
            !root.authoritativeCellSelectionActive) return null
        return displayedCellSelection(adapter)
    }

    private fun clipped(rects: List<RectF>?, viewport: RectF): List<RectF>? =
        rects?.mapNotNull { rect -> RectF(rect).takeIf { it.intersect(viewport) } }

    private fun toolbarAnchorCells(): Pair<String, Set<Int>>? {
        drawingView.selectedTableCellSourceIndices.entries.firstOrNull()?.let { return it.key to it.value }
        val active = activeCell ?: return null
        return active.tableId to setOf(active.cellIndex)
    }

    private fun cellEditMenuAnchor(): Rect? {
        val root = host.editorEditText
        val tableId = cellEditMenuSelection()?.first ?: return null
        val visible = Rect()
        if (!drawingView.getLocalVisibleRect(visible)) return null
        val union = clipped(drawingView.selectedTableCellRects(tableId), RectF(visible))
            ?.reduceOrNull { total, rect -> total.apply { union(rect) } } ?: return null
        val drawingOrigin = IntArray(2).also(drawingView::getLocationInWindow)
        val rootOrigin = IntArray(2).also(root::getLocationInWindow)
        union.offset((drawingOrigin[0] - rootOrigin[0]).toFloat(), (drawingOrigin[1] - rootOrigin[1]).toFloat())
        return Rect().also(union::roundOut)
    }

    private fun refreshCellEditMenu() {
        if (!cellEditMenu.isVisible) return
        val selection = cellEditMenuSelection()
        if (selection == null || selection != presentedCellEditMenuSelection) {
            dismissCellEditMenu()
            return
        }
        cellEditMenu.reanchor()
    }

    fun presentCellEditMenu() {
        if (activeDrag != null) return
        val selection = cellEditMenuSelection() ?: return
        presentedCellEditMenuSelection = selection
        cellEditMenu.present()
    }

    fun dismissCellEditMenu() = cellEditMenu.dismiss()

    private fun cellSelectionContains(x: Float, y: Float): Boolean {
        val tableId = cellEditMenuSelection()?.first ?: return false
        return drawingView.selectedTableCellRects(tableId)?.any { it.contains(x, y) } == true
    }

    private fun tapCellSelection(target: Pair<String, Int>?, x: Float, y: Float): Boolean {
        if (!cellSelectionContains(x, y)) return false
        if (pendingCellEditMenuToggle != null) {
            cancelPendingCellEditMenuToggle()
            dismissCellEditMenu()
            target?.let { (tableId, cellIndex) -> activateCell(tableId, cellIndex, x, y) }
            return true
        }
        val toggle = Runnable {
            pendingCellEditMenuToggle = null
            if (cellEditMenu.isVisible) dismissCellEditMenu() else presentCellEditMenu()
        }
        pendingCellEditMenuToggle = toggle
        drawingView.postDelayed(toggle, doubleTapTimeoutMs)
        return true
    }

    private fun cancelPendingCellEditMenuToggle() {
        pendingCellEditMenuToggle?.let(drawingView::removeCallbacks)
        pendingCellEditMenuToggle = null
    }

    private fun refreshPresentation() {
        val input = host.editorEditText
        val adapter = input.v2Driver as? EditorV2Adapter
        val revision = adapter?.cachedAtomicRenderDocumentRevision
        drawingView.tableNodeRevision = revision
        drawingView.tableNodeChanges = adapter?.cachedTablePresentation?.changes
        drawingView.tableNodeAppearance = "${input.renderAppearanceRevision}:$hostTableDirection:${host.layoutDirection}:${adapter?.tablePresentationDocumentGeneration}"
        val width = (input.measuredWidth - input.compoundPaddingLeft - input.compoundPaddingRight)
            .coerceAtLeast(0)
        val markers = markers(input)
        val accessibilityKey = Triple(revision, width, input.layout?.height)
        if (accessibilityKey != this.accessibilityKey) {
            this.accessibilityKey = accessibilityKey
            drawingView.invalidateTableAccessibility()
        }
        val rootTableIds = adapter?.tableIndex?.tableKeys?.filter { adapter.tableIndex.record(it)?.host == null }?.toSet()
        val rootExtents = adapter?.tableIndex?.rootExtents?.filterValues { it.scalarEnd > it.scalarStart }?.mapValues { (_, extent) ->
            TableScalarExtent(extent.scalarStart.toInt(), extent.scalarEnd.toInt())
        }
        if (adapter != null && input.rootTableMapPositionEpoch != adapter.positionEpoch) {
            input.adoptCurrentRootTableMapEpoch(adapter)
        }
        if (adapter == null || revision == null ||
            input.lastAppliedDocumentVersion != revision.toString() ||
            input.rootTableMapDocumentVersion != revision.toString() ||
            input.rootTableMapPositionEpoch != adapter.positionEpoch ||
            rootTableIds != input.rootTableMapTableIds ||
            rootExtents != input.rootTableMapExtents ||
            input.rootTableMapExtents.keys != markers.keys ||
            input.rootTablePositionMap == null || markers.isEmpty() || width <= 0 ||
            adapter.tableIndex.tableKeys.containsAll(markers.keys).not()
        ) {
            val restoreCellSelectionFocus = input.authoritativeCellSelectionActive && activeCell != null
            if (entries.isNotEmpty() || drawingView.parent != null) clear()
            else {
                invalidateCell()
                drawingView.selectedTableCellSourceIndices = emptyMap()
                drawingView.selectedTableCellEndpoints = null
            }
            if (restoreCellSelectionFocus) input.requestFocus()
            mountDetachedFrameAccessibility()
            return
        }
        val cellSelection = adapter.cachedAtomicRenderSelection()?.takeIf { it.optString("type") == "cell" }
            ?.let { resolveEditorCellSelection(it, adapter.tableIndex) }
        activeDrag?.let { drag -> if (!validDrag(drag)) discardActiveDrag() }
        if (input.authoritativeCellSelectionActive && activeCell != null) {
            invalidateCell()
            input.requestFocus()
        }
        val tableDirection = hostTableDirection
            ?: TableLayoutDirection.fromLayoutDirection(host.layoutDirection)
        val nextKey = PreparationKey(adapter, revision, width, input.renderAppearanceRevision,
            adapter.tablePresentationDocumentGeneration, resizePreview, tableDirection)
        if (key != nextKey) {
            val tableLayoutStarted = PreparedProseInstrumentation.now()
            val index = adapter.tableIndex
            val density = input.resources.displayMetrics.density
            val base = EditorTextStyle(fontSize = input.baseFontSize / density,
                color = input.baseTextColor)
            val theme = input.theme?.copy(text = base.mergedWith(input.theme?.text))
                ?: com.apollohg.editor.EditorTheme(text = base)
            val preparedTheme = PreparedProseTheme.resolve(null, density,
                semanticGeneration = "editor-table", editorTheme = theme)
                .copy(insetTopPx = 0, insetRightPx = 0, insetBottomPx = 0, insetLeftPx = 0,
                    tableDirection = tableDirection)
            val engine = StaticLayoutAndroidProseLayoutEngine().apply {
                tableCellPreparationObserver = { index, contentKey -> onTableCellPreparedForTesting?.invoke(index, contentKey) }
                tableIncrementalRelayoutObserver = { incrementalRelayoutsForTesting += 1 }
            }
            val appearance = "${input.renderAppearanceRevision}:$tableDirection:$density"
            val presentationIdentities = index.tableKeys.associateWith { tableKey ->
                "${adapter.editorId}:${adapter.tablePresentationDocumentGeneration}:$tableKey"
            }
            val minimumColumnWidth = ceil(preparedTheme.tableStyle.minColumnWidth).toInt()
                .coerceAtMost(MAXIMUM_COLUMN_WIDTH)
            val preview = resizePreview
            val shapes = cellShapes.newBuildContext()
            val prepared = try {
                markers.mapNotNull { (id, _) ->
                    val source = index.record(id) ?: return@mapNotNull null
                    val table = preview?.takeIf {
                        it.edge.tableId == id && it.edge.column in source.columnWidths.indices
                    }?.let { resized ->
                        source.copy(columnWidths = source.columnWidths.toMutableList().apply {
                            set(resized.edge.column, resized.width.toUInt())
                        })
                    } ?: source
                    val semantic = "editor-table-$id-$revision"
                    val document = ViewerDocument(semantic,
                        listOf(ViewerBlock("table", 0, false, null, null, emptyList(), frameRecord = table)),
                        false, 256, tableAttributes = index.attributeObjects,
                        frameIndex = index, tablePresentationIdentities = presentationIdentities)
                    val layoutKey = ProseLayoutKey(semantic, width, "editor-table-${input.renderAppearanceRevision}",
                        0, 0, density.toBits().toLong(), revision.toLong(), semantic,
                        tableDirection = tableDirection)
                    val reusable by lazy { ReusableCellContents(entries[id], appearance) }
                    engine.reusableTableCellStore = entries[id]?.surface?.layoutStore
                    engine.reusableTableCell = if (entries[id] != null) {
                        { cell, cellWidth -> reusable.take(cell, cellWidth) }
                    } else null
                    val changes = adapter.cachedTablePresentation?.changes
                    engine.incrementalTableSurface = { tableKey ->
                        val previous = entries[tableKey]?.takeIf { it.appearance == appearance }
                        if (preview == null && key?.resizePreview == null && previous != null &&
                            changes != null && !changes.fullReset && tableKey !in changes.replacedTables &&
                            previous.surface.cells.all { it.isPositionFree }) {
                            previous.surface to changes.changedCells[tableKey].orEmpty()
                        } else null
                    }
                    val result = engine.prepare(document, layoutKey, preparedTheme, width, density, false,
                        layoutKey.semanticGenerationIdentity, shapes)
                    val block = result.blocks.firstOrNull { it.tableSurface != null }
                        ?: return@mapNotNull null
                    val bounds = block.tableBounds ?: return@mapNotNull null
                    if (result.error != null || result.heightPx <= 0) return@mapNotNull null
                    id to Entry(requireNotNull(block.tableSurface), bounds, result.heightPx,
                        minimumColumnWidth, appearance)
                }.toMap()
            } finally {
                shapes.close()
                engine.reusableTableCell = null
                engine.reusableTableCellStore = null
                engine.incrementalTableSurface = null
                engine.tableIncrementalRelayoutObserver = null
            }
            cellShapes.synchronizeOwners(prepared.values.flatMap { entry -> entry.surface.cells.mapNotNull { it.cachedContent } })
            entries = prepared
            key = nextKey
            PreparedProseInstrumentation.laidOut(tableLayoutStarted, "editor-table-$revision")
        }
        reserve(entries.mapValues { it.value.occupiedHeight })
        if (entries.isEmpty()) {
            drawingView.selectedTableCellSourceIndices = emptyMap()
            drawingView.selectedTableCellEndpoints = null
            drawingView.install(null)
            (drawingView.parent as? ViewGroup)?.removeView(drawingView)
            mountDetachedFrameAccessibility()
            return
        }
        if (drawingView.parent !== host.editorContentFrame) {
            (drawingView.parent as? ViewGroup)?.removeView(drawingView)
            host.editorContentFrame.addView(drawingView,
                topLeftFrameParams(ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.MATCH_PARENT))
        }
        updateGeometry()
        drawingView.selectedTableCellSourceIndices = when (cellSelection) {
            is EditorCellSelection.Drawable -> mapOf(cellSelection.tableId to cellSelection.sourceIndices)
            else -> emptyMap()
        }
        drawingView.selectedTableCellEndpoints = displayedCellSelection(adapter)?.takeIf { (tableId) ->
            tableId in entries && input.isEnabled && input.isEditable && !input.hasPendingCompositionForExternalRefresh() &&
                input.hasAuthorizedNativeTableOwner(adapter) &&
                adapter.tableIndex.record(tableId)?.readOnlyDescendants == false
        }
        if (!applyingCellUpdate) reconcileActiveCell()
    }

    fun updateGeometry() {
        val input = host.editorEditText
        if (entries.isEmpty()) return
        val layout = input.layout
        val markers = markers(input)
        val blocks = markers.mapNotNull { (id, start) ->
            val entry = entries[id] ?: return@mapNotNull null
            val line = layout.getLineForOffset(start)
            val x = input.left + input.totalPaddingLeft + entry.localBounds.left
            val y = input.top + input.totalPaddingTop + layout.getLineTop(line) + entry.localBounds.top
            val rect = Rect(x, y, x + entry.localBounds.width(), y + entry.localBounds.height())
            PreparedProseBlock(emptyList(), rect, tableSurface = entry.surface, tableBounds = rect)
        }.sortedBy { it.bounds.top }
        val width = host.editorContentFrame.width.coerceAtLeast(input.measuredWidth)
            .coerceAtLeast(blocks.maxOfOrNull { it.bounds.right } ?: 0).coerceIn(1, View.MEASURED_SIZE_MASK)
        val height = host.editorContentFrame.height.coerceAtLeast(input.measuredHeight)
            .coerceAtLeast(blocks.maxOfOrNull { it.bounds.bottom } ?: 0).coerceIn(1, View.MEASURED_SIZE_MASK)
        val installed = drawingView.preparedLayout
        if (blocks == positionedBlocks && installed != null && installed.widthPx == width && installed.heightPx == height) {
            host.layoutEditorContentChild(drawingView)
            return
        }
        positionedBlocks = blocks
        val key = ProseLayoutKey("editor-table-canvas", width, "editor-table-canvas", 0, 0,
            input.resources.displayMetrics.density.toBits().toLong(), 0, "editor-table-canvas")
        drawingView.install(PreparedProseLayout(key, width, height, blocks,
            retainedBytes = blocks.sumOf { it.retainedBytes }))
        host.layoutEditorContentChild(drawingView)
        positionActiveInput()
        selectionGeometryMayChange()
    }

    fun selectionGeometry(obstructions: TableSelectionObstructions): TableSelectionGeometry? {
        val adapter = host.editorEditText.v2Driver as? EditorV2Adapter ?: return null
        val documentRevision = adapter.cachedAtomicRenderDocumentRevision ?: return null
        val layoutEpoch = canonicalV2U64(adapter.positionEpoch) ?: return null
        val (tableId, sourceIndices) = toolbarAnchorCells() ?: return null
        val tablePos = adapter.tableIndex.tableDocStart(tableId)
            ?: return null
        val visible = Rect()
        if (drawingView.windowToken == null || !drawingView.getLocalVisibleRect(visible)) return null
        val viewport = RectF(visible)
        val cellRects = clipped(drawingView.tableCellRects(tableId, sourceIndices), viewport) ?: return null
        val origin = IntArray(2).also(drawingView::getLocationInWindow)
        val density = drawingView.resources.displayMetrics.density
        fun windowRect(rect: RectF) = RectF(
            (rect.left + origin[0]) / density, (rect.top + origin[1]) / density,
            (rect.right + origin[0]) / density, (rect.bottom + origin[1]) / density
        )
        return TableSelectionGeometry(
            editorId = adapter.editorId,
            documentRevision = documentRevision.toString(),
            layoutEpoch = layoutEpoch,
            tablePos = tablePos,
            rects = cellRects.map(::windowRect),
            viewport = windowRect(viewport),
            obstructions = obstructions,
            editMenuVisible = cellEditMenu.isVisible
        )
    }

    fun onRootTouch(event: MotionEvent) {
        val root = host.editorEditText
        when (event.actionMasked) {
            MotionEvent.ACTION_DOWN -> if (root.authoritativeCellSelectionActive) root.cellSelectionRootTouchPending = true
            MotionEvent.ACTION_UP -> if (root.cellSelectionRootTouchPending) {
                root.post { root.cellSelectionRootTouchPending = false }
            }
            MotionEvent.ACTION_CANCEL -> root.cellSelectionRootTouchPending = false
        }
    }

    fun releaseActiveCellForRootGesture(): Boolean {
        if (activeCell == null) return true
        if (activeInput?.prepareForExternalEditorUpdate() != true) return false
        invalidateCell()
        return true
    }

    private fun rootCellAt(x: Float, y: Float): Pair<String, ViewerTablePresentedCell>? {
        val presented = drawingView.rootTableCellAt(x, y) ?: return null
        return (tableIdFor(presented.surface) ?: return null) to presented
    }

    private fun hitCell(x: Float, y: Float): Pair<String, Int>? {
        val (tableId, presented) = rootCellAt(x, y) ?: return null
        return tableId to presented.cell.sourceIndex
    }

    private fun projection(tableId: String, cellIndex: Int): EditorTableCellProjection.Projection? {
        val root = host.editorEditText
        val adapter = root.v2Driver as? EditorV2Adapter ?: return null
        if (!root.isEditable || !root.hasAuthorizedNativeTableOwner(adapter) ||
            adapter.cachedAtomicRenderDocumentRevision != adapter.baseDocumentRevision ||
            root.lastAppliedDocumentVersion != adapter.baseDocumentRevision.toString()) return null
        return EditorTableCellProjection.project(cellIndex, tableId, adapter.tableIndex,
            adapter.baseDocumentRevision.toString(), adapter.positionEpoch ?: return null,
            root.baseFontSize, root.baseTextColor, root.theme, root.resources.displayMetrics.density)
    }

    private fun activateCell(tableId: String, cellIndex: Int, x: Float, y: Float): Boolean {
        val root = host.editorEditText
        val current = activeCell
        if (current != null && (current.tableId != tableId || current.cellIndex != cellIndex) &&
            activeInput?.prepareForExternalEditorUpdate() != true) return false
        var resolvedTableId = tableId
        if (root.hasPendingCompositionForExternalRefresh()) {
            val adapter = root.v2Driver as? EditorV2Adapter ?: return false
            val beforeIds = markers(root).entries.sortedBy { it.value }.map { it.key }
            val ordinal = beforeIds.indexOf(tableId).takeIf { it >= 0 } ?: return false
            val beforeTable = adapter.tableIndex.record(tableId) ?: return false
            val beforeCells = beforeTable.cells
            val contentKey = beforeCells.getOrNull(cellIndex)?.contentKey
                ?.takeIf { it.isNotEmpty() } ?: return false
            val preparation = root.prepareForExternalEditorUpdateWithResult()
            if (!preparation.ready) return false
            if (preparation.adoptedUpdateJSON?.let { root.applyUpdateJSON(it) } == false) return false
            if (preparation.adoptedUpdateJSON != null) {
                val afterIds = markers(root).entries.sortedBy { it.value }.map { it.key }
                if (afterIds.size != beforeIds.size) return false
                resolvedTableId = afterIds.getOrNull(ordinal) ?: return false
                val afterCells = adapter.tableIndex.record(resolvedTableId)?.cells ?: return false
                if (afterCells.size != beforeCells.size ||
                    afterCells.getOrNull(cellIndex)?.contentKey != contentKey
                ) return false
            }
        }
        val projected = projection(resolvedTableId, cellIndex) ?: return false
        return bindCell(resolvedTableId, cellIndex, projected, x to y)
    }

    private fun bindCell(tableId: String, cellIndex: Int,
                         projected: EditorTableCellProjection.Projection,
                         touch: Pair<Float, Float>? = null,
                         selection: JSONObject? = null,
                         focus: Boolean = true): Boolean {
        val root = host.editorEditText
        val current = activeCell
        if (current?.cellIndex == cellIndex && current.tableId == tableId) {
            activeInput?.let { input ->
                if (focus) input.requestFocus()
                touch?.let { input.setSelection(input.getOffsetForPosition(
                    it.first - input.left, it.second - input.top)) }
                selection?.let { input.applySelectionFromJSON(it,
                    adapterRevision(root)) }
                if (focus) showKeyboard(input)
            }
            reconcileActiveCell()
            return true
        }
        invalidateCell()
        val adapter = root.v2Driver as? EditorV2Adapter ?: return false
        val input = coordinator?.cellInput ?: EditorEditText(host.context).apply {
            isTableCellInput = true
            setPadding(0, 0, 0, 0)
            setBackgroundColor(android.graphics.Color.TRANSPARENT)
            coordinator = EditorTableInputCoordinator(this)
            host.onTableCellInputCreated?.invoke(this)
        }
        applyRootAppearance(input, root)
        input.isEditable = root.isEditable
        input.setViewportBottomInsetPx(root.viewportBottomInsetPx)
        input.setViewportBottomOcclusionTopOnScreenPx(root.viewportBottomOcclusionTopOnScreenPx)
        input.editorId = root.editorId
        input.v2Driver = adapter
        val bound = coordinator?.bind(projected.target, projected.positionMap,
            adapter.baseDocumentRevision.toString(), adapter.positionEpoch ?: "",
            authority = { root.v2Driver === adapter && root.hasAuthorizedNativeTableOwner(adapter) &&
                root.isEditable && activeCell?.tableId == tableId && activeCell?.cellIndex == cellIndex },
            updateConsumer = { update, notify, external -> applyCellUpdate(update, notify, external) }
        ) == true
        if (!bound) return false
        activeCell = ActiveCell(tableId, cellIndex)
        input.tableCellAccessibility = TableCellAccessibility(drawingView, { entries[tableId]?.surface }, cellIndex, this)
        activeAppearanceRevision = root.renderAppearanceRevision
        input.onTableCellSelectionSynced = {
            val latest = adapter.cachedAtomicRenderSelection()
            if (root.authoritativeCellSelectionActive &&
                adapter.cachedAtomicRenderDocumentRevision == adapter.baseDocumentRevision &&
                latest?.optString("type") == "text" &&
                root.hasAuthorizedNativeTableOwner(adapter)
            ) {
                root.authoritativeCellSelectionActive = false
                root.cellSelectionRootTouchPending = false
                root.updateAtomBoundaryCursorVisibility()
                drawingView.selectedTableCellSourceIndices = emptyMap()
            }
            reconcileActiveCell()
        }
        input.onTableCellTab = ::moveFromActiveCell
        input.onTableCellArrow = ::moveFromActiveCellByArrow
        input.applyRenderedSpannable(projected.text, usedPatch = false)
        if (input.parent !== host.editorContentFrame) {
            (input.parent as? ViewGroup)?.removeView(input)
            host.editorContentFrame.addView(input, topLeftFrameParams(1, 1))
        }
        drawingView.suppressedTableCell = tableId to cellIndex
        if (!positionActiveInput()) {
            invalidateCell()
            return false
        }
        root.retireInputConnectionForEditor()
        if (focus) input.requestFocus()
        touch?.let { input.setSelection(input.getOffsetForPosition(
            it.first - input.left, it.second - input.top)) }
        selection?.let { input.applySelectionFromJSON(it, adapter.baseDocumentRevision.toString()) }
        if (focus) showKeyboard(input)
        reconcileActiveCell()
        selectionGeometryMayChange()
        return true
    }

    private fun adapterRevision(root: EditorEditText): String? =
        (root.v2Driver as? EditorV2Adapter)?.baseDocumentRevision?.toString()

    private fun moveFromActiveCell(shiftPressed: Boolean): Boolean {
        val active = activeCell ?: return false
        val input = activeInput ?: return false
        val root = host.editorEditText
        val adapter = root.v2Driver as? EditorV2Adapter ?: return false
        if (!input.isEditable || !input.isAuthorizedForTableCellInput() ||
            !root.hasAuthorizedNativeTableOwner(adapter)) return false
        if (!input.prepareForExternalEditorUpdate()) return true
        if (activeCell != active || !input.isAuthorizedForTableCellInput()) return true
        val local = input.currentScalarSelection() ?: return true
        val selection = input.inputScalarSelection(local.first, local.second) ?: return true
        val command = JSONObject().put("type", "moveToAdjacentCell")
            .put("step", if (shiftPressed) "backward" else "forward")
            .put("appendRow", !shiftPressed)
        val update = adapter.commandAtSelection(command, selection.first, selection.second)
            ?: return true
        if (!input.applyUpdateJSON(update)) {
            invalidateCell()
            return true
        }
        val targetSelection = runCatching { adapter.updateSelection(update) }
            .getOrNull() ?: run { invalidateCell(); return true }
        val scalar = exactV2ScalarInt(targetSelection.opt("anchorScalar") as? Number)
            ?: run { invalidateCell(); return true }
        if (scalar != exactV2ScalarInt(targetSelection.opt("headScalar") as? Number)) {
            invalidateCell()
            return true
        }
        if (input.tableCellPositionMap?.localScalarForGlobalScalar(scalar) == null ||
            !input.isAuthorizedForTableCellInput()) {
            invalidateCell()
        }
        return true
    }

    private fun moveFromActiveCellByArrow(keyCode: Int, offset: Int): Boolean {
        val active = activeCell ?: return false
        val input = activeInput ?: return false
        val layout = input.layout ?: return false
        if (keyCode == KeyEvent.KEYCODE_DPAD_RIGHT &&
            layout.getOffsetToRightOf(offset) != offset) return false
        if (keyCode == KeyEvent.KEYCODE_DPAD_LEFT &&
            layout.getOffsetToLeftOf(offset) != offset) return false
        if (keyCode == KeyEvent.KEYCODE_DPAD_UP &&
            layout.getLineForOffset(offset) != 0) return false
        if (keyCode == KeyEvent.KEYCODE_DPAD_DOWN &&
            layout.getLineForOffset(offset) != layout.lineCount - 1) return false
        val root = host.editorEditText
        val adapter = root.v2Driver as? EditorV2Adapter ?: return false
        if (!input.isEditable || !input.isAuthorizedForTableCellInput() ||
            !root.hasAuthorizedNativeTableOwner(adapter)) return true
        if (!input.prepareForExternalEditorUpdate()) return true
        if (activeCell != active || !input.isAuthorizedForTableCellInput()) return true
        if (input.selectionStart != offset || input.selectionEnd != offset) return true
        val currentLayout = input.layout ?: return true
        val surface = entries[active.tableId]?.surface ?: return true
        val source = surface.sourceTable ?: return true
        val cell = source.cells.getOrNull(active.cellIndex) ?: return true
        val row = cell.row.toInt()
        val horizontal = keyCode == KeyEvent.KEYCODE_DPAD_LEFT ||
            keyCode == KeyEvent.KEYCODE_DPAD_RIGHT
        val forward = (keyCode == KeyEvent.KEYCODE_DPAD_RIGHT) != surface.isRightToLeft
        val targets = if (horizontal) {
            if (forward) (active.cellIndex + 1 until source.cells.size).toList()
            else (active.cellIndex - 1 downTo 0).toList()
        } else {
            val targetRow = if (keyCode == KeyEvent.KEYCODE_DPAD_DOWN) {
                row + cell.rowspan.toInt()
            } else row - 1
            val activeFrame = surface.cell(active.cellIndex)
                ?: return true
            val activeBounds = surface.frameOfCell(activeFrame)
            val x = (activeBounds.left + activeFrame.contentOrigin.first +
                currentLayout.getPrimaryHorizontal(offset)).coerceIn(
                    activeBounds.left + 0.5f,
                    activeBounds.left + activeBounds.width - 0.5f
                )
            surface.cells.mapNotNull { candidate ->
                val index = candidate.sourceIndex
                val sourceCell = source.cells.getOrNull(index) ?: return@mapNotNull null
                val candidateBounds = surface.frameOfCell(candidate)
                if (sourceCell.row.toInt() <= targetRow &&
                    targetRow < sourceCell.row.toInt() + sourceCell.rowspan.toInt() &&
                    x >= candidateBounds.left && x < candidateBounds.left + candidateBounds.width
                ) index else null
            }
        }
        val target = targets.firstOrNull { projection(active.tableId, it) != null }
        if (target == null) {
            val outside = if (horizontal) true else
                (keyCode == KeyEvent.KEYCODE_DPAD_UP && row == 0) ||
                    (keyCode == KeyEvent.KEYCODE_DPAD_DOWN &&
                        row + cell.rowspan.toInt() == source.rows.toInt())
            if (outside) exitCellToProse(active, if (horizontal) forward else
                keyCode == KeyEvent.KEYCODE_DPAD_DOWN)
            return true
        }
        val projected = projection(active.tableId, target) ?: return true
        if (!bindCell(active.tableId, target, projected)) {
            root.requestFocus()
            return true
        }
        val destination = if (horizontal) {
            val targetLayout = input.layout ?: return true
            val line = if (forward) 0 else targetLayout.lineCount - 1
            val edge = if (keyCode == KeyEvent.KEYCODE_DPAD_RIGHT) {
                targetLayout.getLineLeft(line) - 1f
            } else targetLayout.getLineRight(line) + 1f
            targetLayout.getOffsetForHorizontal(line, edge)
        } else 0
        input.setSelection(destination)
        input.syncCurrentSelectionToRust()
        return true
    }

    private fun exitCellToProse(active: ActiveCell, forward: Boolean) {
        val root = host.editorEditText
        val adapter = root.v2Driver as? EditorV2Adapter ?: return
        if (!root.hasAuthorizedNativeTableOwner(adapter) ||
            !root.isAuthorizedForRootTableInput()) return
        val extent = root.rootTableMapExtents[active.tableId] ?: return
        val scalar = if (forward) extent.scalarEnd + 1 else extent.scalarStart - 1
        val local = root.rootTablePositionMap?.localScalar(scalar) ?: return
        val offset = PositionBridge.scalarToUtf16(local, root.text.toString())
        invalidateCell()
        root.requestFocus()
        root.setSelection(offset)
        root.syncCurrentSelectionToRust()
    }

    private fun showKeyboard(input: EditorEditText) {
        input.post {
            if (activeInput !== input || !input.hasFocus()) return@post
            val manager = host.context.getSystemService(Context.INPUT_METHOD_SERVICE)
                as? InputMethodManager
            manager?.showSoftInput(input, InputMethodManager.SHOW_IMPLICIT)
        }
    }

    private fun applyCellUpdate(update: String, notify: Boolean, external: Boolean): Boolean {
        val active = activeCell ?: return false
        if (applyingCellUpdate) return false
        val root = host.editorEditText
        val adapter = root.v2Driver as? EditorV2Adapter ?: return false
        val editorId = root.editorId
        val boundMap = coordinator?.positionMap ?: return false
        val revision = runCatching { adapter.readOnlyParsedUpdate(update).optString("documentVersion") }
            .getOrNull()?.takeIf { canonicalV2U64(it) != null } ?: return false
        val cellWasFocused = activeInput?.hasFocus() == true
        applyingCellUpdate = true
        val applied = try {
            root.applyUpdateJSON(update, notify, external)
        } finally {
            applyingCellUpdate = false
        }
        val rootCoherent = applied && root.editorId == editorId && root.v2Driver === adapter &&
            EditorV2Registry.adapterForViewToken(editorId) === adapter &&
            root.hasAuthorizedNativeTableOwner(adapter) &&
            root.lastAppliedDocumentVersion == revision &&
            adapter.baseDocumentRevision.toString() == revision &&
            adapter.cachedAtomicRenderDocumentRevision == adapter.baseDocumentRevision
        val coherent = rootCoherent && activeCell == active && coordinator?.positionMap === boundMap
        val selection = adapter.updateSelection(update)
        val range = selection?.let(::selectionScalarRange)
        if (rootCoherent && selection != null && range != null &&
            adapter.cachedTablePresentation?.changes?.replacedTables?.contains(active.tableId) == true) {
            invalidateCell()
            return moveActiveCell(selection, range, adapter, cellWasFocused)
        }
        if (coherent && (range == null || projection(active.tableId, active.cellIndex)?.holds(range) == true)) {
            reconcileActiveCell(selection, localUpdate = true)
            return true
        }
        if (rootCoherent && selection != null && range != null) {
            return moveActiveCell(selection, range, adapter, cellWasFocused)
        }
        invalidateCell()
        return false
    }

    private fun selectionScalarRange(selection: JSONObject): Pair<Int, Int>? =
        when (selection.optString("type")) {
            "text" -> {
                val anchor = exactV2ScalarInt(selection.opt("anchorScalar") as? Number)
                val head = exactV2ScalarInt(selection.opt("headScalar") as? Number)
                if (anchor == null || head == null) null else minOf(anchor, head) to maxOf(anchor, head)
            }
            "node" -> exactV2ScalarInt(selection.opt("posScalar") as? Number)?.let { it to it + 1 }
            else -> null
        }

    private fun EditorTableCellProjection.Projection.holds(range: Pair<Int, Int>): Boolean =
        positionMap.localScalarForGlobalScalar(range.first) != null &&
            positionMap.localScalarForGlobalScalar(range.second) != null

    private fun moveActiveCell(
        selection: JSONObject,
        range: Pair<Int, Int>,
        adapter: EditorV2Adapter,
        cellWasFocused: Boolean
    ): Boolean {
        if (bindCell(holding = selection, range = range, adapter = adapter, focus = cellWasFocused)) return true
        retireActiveCell(refocusRoot = cellWasFocused)
        return true
    }

    private fun retireActiveCell(refocusRoot: Boolean) {
        invalidateCell()
        if (refocusRoot) host.editorEditText.requestFocus()
    }

    fun followRootSelectionIntoCell(selection: JSONObject?) {
        val root = host.editorEditText
        if (selection == null || applyingCellUpdate || activeCell != null || !root.hasFocus()) return
        val adapter = root.v2Driver as? EditorV2Adapter ?: return
        if (adapter.tableIndex.tableKeys.isEmpty() ||
            !root.hasAuthorizedNativeTableOwner(adapter)) return
        val range = selectionScalarRange(selection) ?: return
        bindCell(holding = selection, range = range, adapter = adapter, focus = true)
    }

    private fun bindCell(
        holding: JSONObject,
        range: Pair<Int, Int>,
        adapter: EditorV2Adapter,
        focus: Boolean
    ): Boolean {
        val tableId = adapter.tableIndex.tableKeyContainingScalar(range.first.toUInt()) ?: return false
        val cellIndex = adapter.tableIndex.cellIndexContainingScalar(tableId, range.first.toUInt()) ?: return false
        val projected = projection(tableId, cellIndex)?.takeIf { it.holds(range) } ?: return false
        val target = Triple(tableId, cellIndex, projected)
        return bindCell(target.first, target.second, target.third, selection = holding, focus = focus)
    }

    private fun reconcileActiveCell(selection: JSONObject? = null, localUpdate: Boolean = false) {
        val active = activeCell ?: return
        val adapter = host.editorEditText.v2Driver as? EditorV2Adapter ?: return
        if (!localUpdate && coordinator?.positionMap?.binding?.revision !=
            adapter.baseDocumentRevision.toString()) {
            retireActiveCell(refocusRoot = activeInput?.hasFocus() == true)
            return
        }
        val projected = projection(active.tableId, active.cellIndex)
        if (projected == null || projected.target.binding.tableKey != active.tableId || projected.target.binding.cellIndex != active.cellIndex) {
            invalidateCell()
            return
        }
        val input = coordinator?.cellInput ?: return
        val root = host.editorEditText
        val composing = input.hasPendingCompositionForExternalRefresh()
        if (composing && !localUpdate && projected.text.toString() != input.lastAuthorizedText) {
            invalidateCell()
            return
        }
        if (coordinator?.refreshBinding(projected.target, projected.positionMap,
                adapter.baseDocumentRevision.toString(), adapter.positionEpoch ?: "") != true) {
            invalidateCell()
            return
        }
        val sameText = input.text.toString() == projected.text.toString()
        val appearanceChanged = activeAppearanceRevision != root.renderAppearanceRevision
        val matchesAuthorized = !appearanceChanged && renderedTextMatches(input.text, projected.text) &&
            renderedTextMatches(input.lastAuthorizedRenderedText, projected.text)
        if (!composing && sameText && (localUpdate || appearanceChanged) && !matchesAuthorized) {
            if (appearanceChanged) applyRootAppearance(input, root)
            val previousStyleOnly = input.reuseImagesDuringThemeUpdate
            input.reuseImagesDuringThemeUpdate = true
            try {
                input.applyRenderedSpannable(projected.text, usedPatch = false)
            } finally {
                input.reuseImagesDuringThemeUpdate = previousStyleOnly
            }
        } else if ((!composing || localUpdate) && !sameText) {
            input.applyRenderedSpannable(projected.text, usedPatch = false)
        }
        activeAppearanceRevision = root.renderAppearanceRevision
        selection?.let { input.applySelectionFromJSON(it, adapter.baseDocumentRevision.toString()) }
        positionActiveInput()
    }

    private fun topLeftFrameParams(width: Int, height: Int) =
        FrameLayout.LayoutParams(width, height, Gravity.TOP or Gravity.LEFT)

    private fun applyRootAppearance(input: EditorEditText, root: EditorEditText) {
        input.setBaseStyle(root.baseFontSize, root.baseTextColor, android.graphics.Color.TRANSPARENT)
        input.applyTheme(root.theme)
    }

    private fun positionActiveInput(): Boolean {
        val active = activeCell ?: return false
        val presented = drawingView.presentedTableCell(active.tableId) { table ->
            table.cell(active.cellIndex)
        } ?: return false
        val cell = presented.cell
        val input = coordinator?.cellInput ?: return false
        if (pinnedInputCell !== cell) {
            pinnedInputCell?.let { it.layoutStore.unpin(it.contentKey) }
            cell.layoutStore.pin(cell.contentKey)
            cell.layoutStore.insert(presented.content, cell.contentKey)
            pinnedInputCell = cell
        }
        val inset = cell.contentOrigin
        val frame = presented.surface.frameOfCell(cell)
        val width = (frame.width - 2f * inset.first).toInt().coerceAtLeast(1)
        val height = (frame.height - 2f * inset.second).toInt().coerceAtLeast(1)
        val params = topLeftFrameParams(width, height).apply {
            leftMargin = presented.contentBounds.left.toInt()
            topMargin = presented.contentBounds.top.toInt()
        }
        val current = input.layoutParams as? FrameLayout.LayoutParams
        if (current == null || current.width != params.width || current.height != params.height ||
            current.leftMargin != params.leftMargin || current.topMargin != params.topMargin) {
            input.layoutParams = params
        }
        host.layoutEditorContentChild(input)
        input.clipBounds = Rect(
            (presented.clip.left - params.leftMargin).toInt().coerceAtLeast(0),
            (presented.clip.top - params.topMargin).toInt().coerceAtLeast(0),
            (presented.clip.right - params.leftMargin).toInt().coerceAtMost(width),
            (presented.clip.bottom - params.topMargin).toInt().coerceAtMost(height)
        )
        return true
    }

    fun invalidateCell() {
        pinnedInputCell?.let { it.layoutStore.unpin(it.contentKey) }
        pinnedInputCell = null
        activeCell = null
        activeAppearanceRevision = null
        coordinator?.invalidateBinding()
        coordinator?.cellInput?.let { input ->
            input.tableCellAccessibility = null
            input.onTableCellSelectionSynced = null
            input.onTableCellTab = null
            input.onTableCellArrow = null
            input.clearFocus()
            (input.parent as? ViewGroup)?.removeView(input)
        }
        drawingView.suppressedTableCell = null
        selectionGeometryMayChange()
    }

    private fun tableIdFor(surface: ViewerTableSurface): String? =
        entries.entries.firstOrNull { it.value.surface === surface }?.key

    private fun tableMutationContext(tableId: String): Pair<EditorV2Adapter, TableMutationAdmission>? {
        val root = host.editorEditText
        val adapter = root.v2Driver as? EditorV2Adapter ?: return null
        if (!root.isEnabled || !root.isEditable || root.hasPendingCompositionForExternalRefresh() ||
            activeInput?.hasPendingCompositionForExternalRefresh() == true ||
            !root.hasAuthorizedNativeTableOwner(adapter)) return null
        val admission = adapter.tableMutationAdmission(tableId).takeIf(adapter::admitsTableMutation) ?: return null
        return adapter to admission
    }

    private fun ownsAccessibilitySelection(tableId: String, cell: TableAccessibilityCell): Boolean {
        activeCell?.let { return it.tableId == tableId && it.cellIndex == cell.sourceIndex }
        return drawingView.selectedTableCellSourceIndices[tableId]?.contains(cell.sourceIndex) == true
    }

    private fun applyTableMutationUpdate(update: String): Boolean {
        val applied = if (activeCell != null) applyCellUpdate(update, notify = true, external = false)
        else host.editorEditText.applyUpdateJSON(update)
        refresh()
        return applied
    }

    override fun tableAccessibilityActions(cell: TableAccessibilityCell): List<TableAccessibilityAction> {
        val tableId = tableIdFor(cell.surface) ?: return emptyList()
        if (!ownsAccessibilitySelection(tableId, cell)) return emptyList()
        val (adapter) = tableMutationContext(tableId) ?: return emptyList()
        val commands = adapter.cachedActiveState?.optJSONObject("commands") ?: return emptyList()
        return TableAccessibilityAction.ALL.filter { commands.optBoolean(it.applicability, false) }
    }

    override fun performTableAccessibilityAction(action: TableAccessibilityAction, cell: TableAccessibilityCell): Boolean {
        if (action !in tableAccessibilityActions(cell)) return false
        val tableId = tableIdFor(cell.surface) ?: return false
        val (adapter, admission) = tableMutationContext(tableId) ?: return false
        val update = adapter.applyTableCommandAtSelection(action.commandJson(), admission) ?: return false
        return applyTableMutationUpdate(update)
    }

    override fun activateTableAccessibilityCell(cell: TableAccessibilityCell): Boolean {
        val tableId = tableIdFor(cell.surface) ?: return false
        val presented = drawingView.presentedAccessibilityCell(cell) ?: return false
        val visible = RectF(presented.bounds)
        if (!visible.intersect(presented.clip)) return false
        return activateCell(tableId, cell.sourceIndex, visible.centerX(), visible.centerY())
    }

    override fun activeTableAccessibilityInput(cell: TableAccessibilityCell): EditorEditText? {
        val active = activeCell ?: return null
        if (active.tableId != tableIdFor(cell.surface) || active.cellIndex != cell.sourceIndex) return null
        return activeInput
    }

    override fun detachedTableAccessibilityFrames(): List<TableAccessibilityDetachedFrame> {
        val adapter = host.editorEditText.v2Driver as? EditorV2Adapter ?: return emptyList()
        return adapter.tableIndex.tableKeys.mapNotNull { tableId ->
            val record = adapter.tableIndex.record(tableId) ?: return@mapNotNull null
            if (adapter.tableIndex.rootExtents[tableId]?.let { it.scalarEnd > it.scalarStart } == true || record.readOnlyDescendants) return@mapNotNull null
            val tablePos = adapter.tableIndex.tableDocStart(tableId)?.toInt() ?: return@mapNotNull null
            val unfilled = record.failure == null && (record.rows == 0u || record.columns == 0u)
            TableAccessibilityDetachedFrame(
                tableId,
                tablePos,
                if (unfilled) TableAccessibilityTable.Frame.EMPTY else TableAccessibilityTable.Frame.FAILED
            ) { detachedFrameBounds(adapter, tablePos) }
        }.sortedBy { it.tablePos }
    }

    private fun detachedFrameBounds(adapter: EditorV2Adapter, tablePos: Int): RectF {
        val input = host.editorEditText
        val layout = input.layout ?: return RectF()
        val scalar = adapter.scalarPositionForDoc(tablePos) ?: return RectF()
        val text = input.text.toString()
        val line = layout.getLineForOffset(PositionBridge.scalarToUtf16(scalar, text).coerceIn(0, text.length))
        val top = (input.top + input.totalPaddingTop + layout.getLineTop(line)).toFloat()
        val bottom = (input.top + input.totalPaddingTop + layout.getLineBottom(line)).toFloat()
        return RectF((input.left + input.totalPaddingLeft).toFloat(), top,
            (input.right - input.totalPaddingRight).toFloat(), bottom)
    }

    override fun canDeleteTableAccessibilityFrame(tableId: String): Boolean = tableMutationContext(tableId) != null

    override fun deleteTableAccessibilityFrame(tableId: String): Boolean {
        val (adapter, admission) = tableMutationContext(tableId) ?: return false
        val update = adapter.deleteTable(admission) ?: return false
        return applyTableMutationUpdate(update)
    }

    private fun mountDetachedFrameAccessibility() {
        if (detachedTableAccessibilityFrames().isEmpty() || drawingView.parent === host.editorContentFrame) return
        (drawingView.parent as? ViewGroup)?.removeView(drawingView)
        host.editorContentFrame.addView(drawingView,
            topLeftFrameParams(ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.MATCH_PARENT))
        host.layoutEditorContentChild(drawingView)
    }

    private fun markers(input: EditorEditText): Map<String, Int> = markers(input.text)

    private fun markers(text: Spanned): Map<String, Int> {
        return text.getSpans(0, text.length, Annotation::class.java)
            .filter { it.key == RenderBridge.NATIVE_ROOT_TABLE_MARKER_ANNOTATION }
            .associate { it.value to text.getSpanStart(it) }
    }

    private fun reserve(heights: Map<String, Int>) {
        val input = host.editorEditText
        fun apply(text: android.text.Spannable): Boolean {
            val markers = markers(text)
            var changed = false
            text.getSpans(0, text.length, RootTableHeightSpan::class.java).forEach { span ->
                val start = text.getSpanStart(span)
                val expected = markers.entries.firstOrNull { it.value == start }?.key?.let(heights::get)
                if (expected != span.heightPx) {
                    text.removeSpan(span)
                    changed = true
                }
            }
            markers.forEach { (id, start) ->
                val height = heights[id] ?: return@forEach
                val existing = text.getSpans(start, start + 1, RootTableHeightSpan::class.java)
                    .any { text.getSpanStart(it) == start && it.heightPx == height }
                if (!existing) {
                    text.setSpan(RootTableHeightSpan(height), start, start + 1,
                        Spanned.SPAN_EXCLUSIVE_EXCLUSIVE)
                    changed = true
                }
            }
            return changed
        }
        val authorized = SpannableStringBuilder(input.lastAuthorizedRenderedText ?: input.lastAuthorizedText)
        if (apply(authorized)) input.lastAuthorizedRenderedText = authorized
        if (apply(input.text)) {
            input.invalidateTextLayout()
        }
    }
}
