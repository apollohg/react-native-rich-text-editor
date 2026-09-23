package com.apollohg.editor.tables

import android.content.Context
import android.graphics.Canvas
import android.graphics.Paint
import android.graphics.Rect
import android.text.Annotation
import android.text.SpannableStringBuilder
import android.text.Spanned
import android.text.style.ReplacementSpan
import android.view.View
import android.view.ViewGroup
import android.view.MotionEvent
import android.view.KeyEvent
import android.view.inputmethod.InputMethodManager
import android.widget.FrameLayout
import com.apollohg.editor.EditorEditText
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
import com.apollohg.editor.RichTextEditorView
import com.apollohg.editor.canonicalV2U64
import com.apollohg.editor.applyRenderedSpannable
import com.apollohg.editor.applySelectionFromJSON
import com.apollohg.editor.isAuthorizedForRootTableInput
import com.apollohg.editor.hasAuthorizedNativeTableOwner
import com.apollohg.editor.retireInputConnectionForEditor
import com.apollohg.editor.updateAtomBoundaryCursorVisibility
import com.apollohg.editor.viewer.PreparedProseBlock
import com.apollohg.editor.viewer.PreparedProseDrawingView
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

internal class EditorTableSurface(private val host: RichTextEditorView) {
    private data class Entry(val surface: ViewerTableSurface, val localBounds: Rect,
                             val occupiedHeight: Int)

    val drawingView = PreparedProseDrawingView(host.context).apply {
        isFocusable = false
        linkInteractionsEnabled = false
        mentionInteractionsEnabled = false
        importantForAccessibility = View.IMPORTANT_FOR_ACCESSIBILITY_NO
        setBackgroundColor(android.graphics.Color.TRANSPARENT)
        setOnTouchListener { _, event -> handleTableTouch(event) }
    }
    private data class ActiveCell(val tableId: String, val cellIndex: Int, val sourcePos: Long)
    private var activeCell: ActiveCell? = null
    private var applyingCellUpdate = false
    private var activeAppearanceRevision: Long? = null
    private var blockedRootGesture = false
    private var touchTarget: Pair<String, Int>? = null
    private var coordinator: EditorTableInputCoordinator? = null
    val activeInput: EditorEditText? get() = coordinator?.cellInput?.takeIf { activeCell != null }
    private var entries: Map<String, Entry> = emptyMap()
    private var key: Triple<EditorV2Adapter, ULong, Pair<Int, Long>>? = null
    private var positionedBlocks: List<PreparedProseBlock> = emptyList()

    fun clear() {
        invalidateCell()
        drawingView.selectedTableCellSourcePositions = emptyMap()
        entries = emptyMap()
        key = null
        positionedBlocks = emptyList()
        reserve(emptyMap())
        drawingView.install(null)
        (drawingView.parent as? ViewGroup)?.removeView(drawingView)
    }

    fun refresh() {
        val input = host.editorEditText
        val adapter = input.v2Driver as? EditorV2Adapter
        val revision = adapter?.cachedAtomicRenderDocumentRevision
        val width = (input.measuredWidth - input.compoundPaddingLeft - input.compoundPaddingRight)
            .coerceAtLeast(0)
        val markers = markers(input)
        val admittedMappings = adapter?.cachedTableInputMappings?.tables
        val rootTableIds = adapter?.cachedTableRecords?.filterValues {
            !it.optBoolean("readOnlyDescendants", true)
        }?.keys
        val rootExtents = rootTableIds?.mapNotNull { id ->
            admittedMappings?.get(id)?.extent?.let { id to it }
        }?.toMap()
        if (adapter != null && input.rootTableMapPositionEpoch != adapter.positionEpoch) {
            input.isAuthorizedForRootTableInput()
        }
        if (adapter == null || revision == null ||
            input.lastAppliedDocumentVersion != revision.toString() ||
            input.rootTableMapDocumentVersion != revision.toString() ||
            input.rootTableMapPositionEpoch != adapter.positionEpoch ||
            rootTableIds != input.rootTableMapTableIds ||
            rootTableIds?.all { admittedMappings?.containsKey(it) == true } != true ||
            rootExtents != input.rootTableMapExtents ||
            input.rootTableMapExtents.keys != markers.keys ||
            input.rootTablePositionMap == null || markers.isEmpty() || width <= 0 ||
            adapter.cachedTableRecords.keys.containsAll(markers.keys).not()
        ) {
            val restoreCellSelectionFocus = input.authoritativeCellSelectionActive && activeCell != null
            if (entries.isNotEmpty() || drawingView.parent != null) clear()
            else {
                invalidateCell()
                drawingView.selectedTableCellSourcePositions = emptyMap()
            }
            if (restoreCellSelectionFocus) input.requestFocus()
            return
        }
        val cellSelection = adapter.cachedAtomicRenderJson?.let { raw ->
            runCatching { JSONObject(raw).optJSONObject("selection") }.getOrNull()
        }?.takeIf { it.optString("type") == "cell" }
            ?.let { resolveEditorCellSelection(it, adapter.cachedTableRecords) }
        if (input.authoritativeCellSelectionActive && activeCell != null) {
            invalidateCell()
            input.requestFocus()
        }
        val nextKey = Triple(adapter, revision, width to input.renderAppearanceRevision)
        if (key != nextKey) {
            val records = lowerEditorTableRecords(adapter.cachedTableRecords) ?: run { clear(); return }
            val density = input.resources.displayMetrics.density
            val base = EditorTextStyle(fontSize = input.baseFontSize / density,
                color = input.baseTextColor)
            val theme = input.theme?.copy(text = base.mergedWith(input.theme?.text))
                ?: com.apollohg.editor.EditorTheme(text = base)
            val preparedTheme = PreparedProseTheme.resolve(null, density,
                semanticGeneration = "editor-table", editorTheme = theme)
                .copy(insetTopPx = 0, insetRightPx = 0, insetBottomPx = 0, insetLeftPx = 0)
            val engine = StaticLayoutAndroidProseLayoutEngine()
            val prepared = markers.mapNotNull { (id, _) ->
                val table = records[id] ?: return@mapNotNull null
                val semantic = "editor-table-$id-$revision"
                val document = ViewerDocument(semantic,
                    listOf(ViewerBlock("table", 0, false, null, null, emptyList(), table = table)),
                    false, 256, tableAttributes = adapter.cachedTableAttributes,
                    tableRecords = records)
                val layoutKey = ProseLayoutKey(semantic, width, "editor-table-${input.renderAppearanceRevision}",
                    0, 0, density.toBits().toLong(), revision.toLong(), semantic)
                val result = engine.prepare(document, layoutKey, preparedTheme, width, density, false)
                val block = result.blocks.firstOrNull { it.tableSurface != null }
                    ?: return@mapNotNull null
                val bounds = block.tableBounds ?: return@mapNotNull null
                if (result.error != null || result.heightPx <= 0) return@mapNotNull null
                id to Entry(requireNotNull(block.tableSurface), bounds, result.heightPx)
            }.toMap()
            entries = prepared
            key = nextKey
        }
        reserve(entries.mapValues { it.value.occupiedHeight })
        if (entries.isEmpty()) {
            drawingView.selectedTableCellSourcePositions = emptyMap()
            drawingView.install(null)
            (drawingView.parent as? ViewGroup)?.removeView(drawingView)
            return
        }
        if (drawingView.parent !== host.editorContentFrame) {
            (drawingView.parent as? ViewGroup)?.removeView(drawingView)
            host.editorContentFrame.addView(drawingView,
                FrameLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT,
                    ViewGroup.LayoutParams.MATCH_PARENT))
        }
        updateGeometry()
        drawingView.selectedTableCellSourcePositions = when (cellSelection) {
            is EditorCellSelection.Drawable -> mapOf(cellSelection.tableId to cellSelection.sourcePositions)
            else -> emptyMap()
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
        if (blocks == positionedBlocks && drawingView.preparedLayout != null) return
        positionedBlocks = blocks
        val width = host.editorContentFrame.width.coerceAtLeast(input.measuredWidth).coerceAtLeast(1)
        val height = host.editorContentFrame.height.coerceAtLeast(input.measuredHeight).coerceAtLeast(1)
        val key = ProseLayoutKey("editor-table-canvas", width, "editor-table-canvas", 0, 0,
            input.resources.displayMetrics.density.toBits().toLong(), 0, "editor-table-canvas")
        drawingView.install(PreparedProseLayout(key, width, height, blocks,
            retainedBytes = blocks.sumOf { it.retainedBytes }))
        positionActiveInput()
    }

    fun onRootTouch(event: MotionEvent): Boolean {
        when (event.actionMasked) {
            MotionEvent.ACTION_DOWN -> {
                blockedRootGesture = false
                if (host.editorEditText.authoritativeCellSelectionActive) {
                    host.editorEditText.cellSelectionRootTouchPending = true
                }
                if (activeCell != null) {
                    if (activeInput?.prepareForExternalEditorUpdate() != true) {
                        blockedRootGesture = true
                        return false
                    }
                    invalidateCell()
                }
            }
            MotionEvent.ACTION_UP -> {
                val root = host.editorEditText
                if (root.cellSelectionRootTouchPending) {
                    root.post { root.cellSelectionRootTouchPending = false }
                }
                if (blockedRootGesture) {
                    blockedRootGesture = false
                    return false
                }
            }
            MotionEvent.ACTION_CANCEL -> {
                host.editorEditText.cellSelectionRootTouchPending = false
                if (blockedRootGesture) {
                    blockedRootGesture = false
                    return false
                }
            }
        }
        return !blockedRootGesture
    }

    private fun handleTableTouch(event: MotionEvent): Boolean {
        when (event.actionMasked) {
            MotionEvent.ACTION_DOWN -> {
                touchTarget = hitCell(event.x, event.y)
                return touchTarget != null
            }
            MotionEvent.ACTION_CANCEL -> touchTarget = null
            MotionEvent.ACTION_UP -> {
                val target = touchTarget
                touchTarget = null
                if (target != null && target == hitCell(event.x, event.y)) {
                    return activateCell(target.first, target.second, event.x, event.y)
                }
            }
        }
        return false
    }

    private fun hitCell(x: Float, y: Float): Pair<String, Int>? {
        val blocks = drawingView.preparedLayout?.blocks.orEmpty()
        for (block in blocks) {
            val table = block.tableSurface ?: continue
            val bounds = block.tableBounds ?: continue
            val tableId = entries.entries.firstOrNull { it.value.surface === table }?.key ?: continue
            for (cell in table.cells) {
                val index = cell.sourceCellIndex ?: continue
                if (x >= bounds.left + cell.frame.left && x < bounds.left + cell.frame.left + cell.frame.width &&
                    y >= bounds.top + cell.frame.top && y < bounds.top + cell.frame.top + cell.frame.height) {
                    return tableId to index
                }
            }
        }
        return null
    }

    private fun projection(tableId: String, cellIndex: Int): EditorTableCellProjection.Projection? {
        val root = host.editorEditText
        val adapter = root.v2Driver as? EditorV2Adapter ?: return null
        if (!root.isEditable || !root.hasAuthorizedNativeTableOwner(adapter) ||
            adapter.cachedAtomicRenderDocumentRevision != adapter.baseDocumentRevision ||
            root.lastAppliedDocumentVersion != adapter.baseDocumentRevision.toString()) return null
        val table = adapter.cachedTableRecords[tableId] ?: return null
        val mapping = adapter.cachedTableInputMappings?.tables?.get(tableId) ?: return null
        return EditorTableCellProjection.project(cellIndex, table, mapping,
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
            val beforeTable = adapter.cachedTableRecords[tableId] ?: return false
            val beforeCells = beforeTable.optJSONArray("cells") ?: return false
            val contentKey = beforeCells.optJSONObject(cellIndex)?.optString("contentKey")
                ?.takeIf { it.isNotEmpty() } ?: return false
            val preparation = root.prepareForExternalEditorUpdateWithResult()
            if (!preparation.ready) return false
            if (preparation.adoptedUpdateJSON?.let { root.applyUpdateJSON(it) } == false) return false
            if (preparation.adoptedUpdateJSON != null) {
                val afterIds = markers(root).entries.sortedBy { it.value }.map { it.key }
                if (afterIds.size != beforeIds.size) return false
                resolvedTableId = afterIds.getOrNull(ordinal) ?: return false
                val afterCells = adapter.cachedTableRecords[resolvedTableId]
                    ?.optJSONArray("cells") ?: return false
                if (afterCells.length() != beforeCells.length() ||
                    afterCells.optJSONObject(cellIndex)?.optString("contentKey") != contentKey
                ) return false
            }
        }
        val projected = projection(resolvedTableId, cellIndex) ?: return false
        return bindCell(resolvedTableId, cellIndex, projected, x to y)
    }

    private fun bindCell(tableId: String, cellIndex: Int,
                         projected: EditorTableCellProjection.Projection,
                         touch: Pair<Float, Float>? = null,
                         selection: JSONObject? = null): Boolean {
        val root = host.editorEditText
        val current = activeCell
        val sourcePos = projected.target.binding.cellSourcePos
        if (current?.sourcePos == sourcePos && current.tableId == tableId) {
            activeInput?.let { input ->
                input.requestFocus()
                touch?.let { input.setSelection(input.getOffsetForPosition(
                    it.first - input.left, it.second - input.top)) }
                selection?.let { input.applySelectionFromJSON(it,
                    adapterRevision(root)) }
                showKeyboard(input)
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
        input.setBaseStyle(root.baseFontSize, root.baseTextColor, android.graphics.Color.TRANSPARENT)
        input.isEditable = root.isEditable
        input.editorId = root.editorId
        input.v2Driver = adapter
        val bound = coordinator?.bind(projected.target, projected.positionMap,
            adapter.baseDocumentRevision.toString(), adapter.positionEpoch ?: "",
            authority = { root.v2Driver === adapter && root.hasAuthorizedNativeTableOwner(adapter) &&
                root.isEditable && activeCell?.sourcePos == sourcePos },
            updateConsumer = { update, notify, external -> applyCellUpdate(update, notify, external) }
        ) == true
        if (!bound) return false
        activeCell = ActiveCell(tableId, cellIndex, sourcePos)
        activeAppearanceRevision = root.renderAppearanceRevision
        root.retireInputConnectionForEditor()
        input.onTableCellSelectionSynced = {
            val latest = adapter.cachedAtomicRenderJson?.let { raw ->
                runCatching { JSONObject(raw).optJSONObject("selection") }.getOrNull()
            }
            if (root.authoritativeCellSelectionActive &&
                adapter.cachedAtomicRenderDocumentRevision == adapter.baseDocumentRevision &&
                latest?.optString("type") == "text" &&
                root.hasAuthorizedNativeTableOwner(adapter)
            ) {
                root.authoritativeCellSelectionActive = false
                root.cellSelectionRootTouchPending = false
                root.updateAtomBoundaryCursorVisibility()
                drawingView.selectedTableCellSourcePositions = emptyMap()
            }
            reconcileActiveCell()
        }
        input.onTableCellTab = ::moveFromActiveCell
        input.onTableCellArrow = ::moveFromActiveCellByArrow
        input.applyRenderedSpannable(projected.text, usedPatch = false)
        if (input.parent !== host.editorContentFrame) {
            (input.parent as? ViewGroup)?.removeView(input)
            host.editorContentFrame.addView(input, FrameLayout.LayoutParams(1, 1))
        }
        drawingView.suppressedTableCellSourcePosition = sourcePos.toInt()
        positionActiveInput()
        input.requestFocus()
        touch?.let { input.setSelection(input.getOffsetForPosition(
            it.first - input.left, it.second - input.top)) }
        selection?.let { input.applySelectionFromJSON(it, adapter.baseDocumentRevision.toString()) }
        showKeyboard(input)
        reconcileActiveCell()
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
        val targetSelection = runCatching { JSONObject(update).optJSONObject("selection") }
            .getOrNull() ?: run { invalidateCell(); return true }
        val scalar = exactV2ScalarInt(targetSelection.opt("anchorScalar") as? Number)
            ?: run { invalidateCell(); return true }
        if (scalar != exactV2ScalarInt(targetSelection.opt("headScalar") as? Number)) {
            invalidateCell()
            return true
        }
        if (input.tableCellPositionMap?.localScalarForGlobalScalar(scalar) != null) {
            if (!input.isAuthorizedForTableCellInput()) invalidateCell()
            return true
        }
        val table = adapter.cachedTableInputMappings?.tables?.get(active.tableId)
        val cell = table?.cells?.firstOrNull { candidate ->
            candidate.blocks.any { block ->
                scalar >= block.scalarStart && scalar <= block.breakScalarEnd
            }
        }
        val projected = cell?.let { projection(active.tableId, it.cellIndex) }
        if (projected == null || projected.positionMap.localScalarForGlobalScalar(scalar) == null ||
            !bindCell(active.tableId, cell.cellIndex, projected, selection = targetSelection)) {
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
        if (cell.sourcePos.toLong() != active.sourcePos) return true
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
            val activeFrame = surface.cells.firstOrNull { it.sourceCellIndex == active.cellIndex }
                ?: return true
            val x = (activeFrame.frame.left + activeFrame.contentOrigin.first +
                currentLayout.getPrimaryHorizontal(offset)).coerceIn(
                    activeFrame.frame.left + 0.5f,
                    activeFrame.frame.left + activeFrame.frame.width - 0.5f
                )
            surface.cells.mapNotNull { candidate ->
                val index = candidate.sourceCellIndex ?: return@mapNotNull null
                val sourceCell = source.cells.getOrNull(index) ?: return@mapNotNull null
                if (sourceCell.row.toInt() <= targetRow &&
                    targetRow < sourceCell.row.toInt() + sourceCell.rowspan.toInt() &&
                    x >= candidate.frame.left && x < candidate.frame.left + candidate.frame.width
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
        if (!bindCell(active.tableId, target, projected)) return true
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
        val revision = runCatching { JSONObject(update).optString("documentVersion") }
            .getOrNull()?.takeIf { canonicalV2U64(it) != null } ?: return false
        applyingCellUpdate = true
        val applied = try {
            root.applyUpdateJSON(update, notify, external)
        } finally {
            applyingCellUpdate = false
        }
        val coherent = applied && root.editorId == editorId && root.v2Driver === adapter &&
            EditorV2Registry.adapterForViewToken(editorId) === adapter &&
            root.hasAuthorizedNativeTableOwner(adapter) &&
            root.lastAppliedDocumentVersion == revision &&
            adapter.baseDocumentRevision.toString() == revision &&
            adapter.cachedAtomicRenderDocumentRevision == adapter.baseDocumentRevision &&
            activeCell == active && coordinator?.positionMap === boundMap
        if (coherent) {
            reconcileActiveCell(JSONObject(update).optJSONObject("selection"), localUpdate = true)
        } else {
            invalidateCell()
        }
        return coherent
    }

    private fun reconcileActiveCell(selection: JSONObject? = null, localUpdate: Boolean = false) {
        val active = activeCell ?: return
        val adapter = host.editorEditText.v2Driver as? EditorV2Adapter ?: return
        if (!localUpdate && coordinator?.positionMap?.binding?.revision !=
            adapter.baseDocumentRevision.toString()) {
            invalidateCell()
            return
        }
        val projected = projection(active.tableId, active.cellIndex)
        if (projected == null || projected.target.binding.cellSourcePos != active.sourcePos) {
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
        if (!composing && sameText && (localUpdate || appearanceChanged)) {
            if (appearanceChanged) {
                input.setBaseStyle(root.baseFontSize, root.baseTextColor,
                    android.graphics.Color.TRANSPARENT)
                input.applyTheme(root.theme)
            }
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

    private fun positionActiveInput() {
        val active = activeCell ?: return
        val block = drawingView.preparedLayout?.blocks?.firstOrNull {
            it.tableSurface === entries[active.tableId]?.surface
        } ?: return
        val cell = block.tableSurface?.cells?.firstOrNull {
            it.sourceCellIndex == active.cellIndex || it.sourcePosition.toLong() == active.sourcePos
        } ?: return
        if (cell.sourcePosition.toLong() != active.sourcePos) { invalidateCell(); return }
        val bounds = block.tableBounds ?: return
        val input = coordinator?.cellInput ?: return
        val inset = cell.contentOrigin
        val width = (cell.frame.width - 2f * inset.first).toInt().coerceAtLeast(1)
        val height = (cell.frame.height - 2f * inset.second).toInt().coerceAtLeast(1)
        val params = FrameLayout.LayoutParams(width, height).apply {
            leftMargin = (bounds.left + cell.frame.left + inset.first).toInt()
            topMargin = (bounds.top + cell.frame.top + inset.second).toInt()
        }
        val current = input.layoutParams as? FrameLayout.LayoutParams
        if (current == null || current.width != params.width || current.height != params.height ||
            current.leftMargin != params.leftMargin || current.topMargin != params.topMargin) {
            input.layoutParams = params
        }
    }

    fun invalidateCell() {
        touchTarget = null
        activeCell = null
        activeAppearanceRevision = null
        coordinator?.invalidateBinding()
        coordinator?.cellInput?.let { input ->
            input.onTableCellSelectionSynced = null
            input.onTableCellTab = null
            input.onTableCellArrow = null
            input.clearFocus()
            (input.parent as? ViewGroup)?.removeView(input)
        }
        drawingView.suppressedTableCellSourcePosition = null
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
