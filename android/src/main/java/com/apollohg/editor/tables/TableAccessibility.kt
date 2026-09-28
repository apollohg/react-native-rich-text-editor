package com.apollohg.editor.tables

import android.graphics.Rect
import android.graphics.RectF
import android.view.View
import android.view.accessibility.AccessibilityEvent
import android.view.accessibility.AccessibilityManager
import android.view.accessibility.AccessibilityNodeInfo
import androidx.core.view.accessibility.AccessibilityNodeInfoCompat
import com.apollohg.editor.AndroidApiCompat
import com.apollohg.editor.EditorEditText
import com.apollohg.editor.LayoutConstants
import com.apollohg.editor.R
import com.apollohg.editor.viewer.PreparedProseDrawingView
import com.apollohg.editor.viewer.PreparedProseFragmentKind
import com.apollohg.editor.viewer.PreparedProseLayout
import java.util.IdentityHashMap
import org.json.JSONObject

internal data class TableAccessibilityAction(
    val id: Int,
    val label: Int,
    val applicability: String,
    val command: Map<String, String>
) {
    fun commandJson(): JSONObject = JSONObject(command)

    companion object {
        val DELETE_TABLE = TableAccessibilityAction(
            R.id.table_accessibility_delete_table, R.string.table_accessibility_delete_table,
            "deleteTable", mapOf("type" to "deleteTable")
        )

        val ALL = listOf(
            TableAccessibilityAction(R.id.table_accessibility_add_row_before,
                R.string.table_accessibility_add_row_before, "addTableRowBefore",
                mapOf("type" to "addTableRow", "side" to "before")),
            TableAccessibilityAction(R.id.table_accessibility_add_row_after,
                R.string.table_accessibility_add_row_after, "addTableRowAfter",
                mapOf("type" to "addTableRow", "side" to "after")),
            TableAccessibilityAction(R.id.table_accessibility_delete_rows,
                R.string.table_accessibility_delete_rows, "deleteTableRows", mapOf("type" to "deleteTableRows")),
            TableAccessibilityAction(R.id.table_accessibility_select_rows,
                R.string.table_accessibility_select_rows, "selectTableRows", mapOf("type" to "selectTableRows")),
            TableAccessibilityAction(R.id.table_accessibility_add_column_before,
                R.string.table_accessibility_add_column_before, "addTableColumnBefore",
                mapOf("type" to "addTableColumn", "side" to "before")),
            TableAccessibilityAction(R.id.table_accessibility_add_column_after,
                R.string.table_accessibility_add_column_after, "addTableColumnAfter",
                mapOf("type" to "addTableColumn", "side" to "after")),
            TableAccessibilityAction(R.id.table_accessibility_delete_columns,
                R.string.table_accessibility_delete_columns, "deleteTableColumns",
                mapOf("type" to "deleteTableColumns")),
            TableAccessibilityAction(R.id.table_accessibility_select_columns,
                R.string.table_accessibility_select_columns, "selectTableColumns",
                mapOf("type" to "selectTableColumns")),
            TableAccessibilityAction(R.id.table_accessibility_toggle_header_row,
                R.string.table_accessibility_toggle_header_row, "toggleTableHeaderRow",
                mapOf("type" to "toggleTableHeader", "target" to "row")),
            TableAccessibilityAction(R.id.table_accessibility_toggle_header_column,
                R.string.table_accessibility_toggle_header_column, "toggleTableHeaderColumn",
                mapOf("type" to "toggleTableHeader", "target" to "column")),
            TableAccessibilityAction(R.id.table_accessibility_toggle_header_cell,
                R.string.table_accessibility_toggle_header_cell, "toggleTableHeaderCell",
                mapOf("type" to "toggleTableHeader", "target" to "cell")),
            TableAccessibilityAction(R.id.table_accessibility_merge_cells,
                R.string.table_accessibility_merge_cells, "mergeTableCells", mapOf("type" to "mergeTableCells")),
            TableAccessibilityAction(R.id.table_accessibility_split_cell,
                R.string.table_accessibility_split_cell, "splitTableCell", mapOf("type" to "splitTableCell")),
            TableAccessibilityAction(R.id.table_accessibility_clear_cells,
                R.string.table_accessibility_clear_cells, "clearTableCells", mapOf("type" to "clearTableCells")),
            DELETE_TABLE
        )
    }
}

internal data class TableAccessibilityCell(
    val presented: ViewerTablePresentedCell,
    val sourceCellIndex: Int,
    val row: Int,
    val column: Int,
    val rowSpan: Int,
    val columnSpan: Int,
    val isHeader: Boolean,
    val label: String,
    val interactions: List<ViewerTablePresentedAccessibilityNode>
) {
    val sourcePosition: Int get() = presented.sourcePosition

    fun coversRow(index: Int): Boolean = index in row until row + rowSpan
    fun coversColumn(index: Int): Boolean = index in column until column + columnSpan
}

internal data class TableAccessibilityTable(
    val presented: ViewerTablePresentedSurface,
    val rowCount: Int,
    val columnCount: Int,
    val cells: List<TableAccessibilityCell>
) {
    enum class Frame { EMPTY, FAILED }

    val surface: ViewerTableSurface get() = presented.surface
    val tableId: String? get() = surface.editorTableId
    val tablePos: Int? get() = surface.sourceTable?.tablePos?.toInt()

    val frame: Frame?
        get() {
            if (cells.isNotEmpty()) return null
            val unfilled = surface.sourceTable?.failure == null && (rowCount == 0 || columnCount == 0)
            return if (unfilled) Frame.EMPTY else Frame.FAILED
        }

    fun cellAt(row: Int, column: Int): TableAccessibilityCell? =
        cells.firstOrNull { it.coversRow(row) && it.coversColumn(column) }

    fun columnHeaders(column: Int): List<TableAccessibilityCell> =
        cells.filter { it.isHeader && it.coversColumn(column) && isHeaderRow(it.row) }

    fun rowHeaders(row: Int): List<TableAccessibilityCell> =
        cells.filter { it.isHeader && it.coversRow(row) && isHeaderColumn(it.column) }

    private fun isHeaderRow(row: Int): Boolean =
        cells.filter { it.row == row }.let { starting -> starting.isNotEmpty() && starting.all { it.isHeader } }

    private fun isHeaderColumn(column: Int): Boolean =
        cells.filter { it.column == column }.let { starting -> starting.isNotEmpty() && starting.all { it.isHeader } }
}

internal sealed interface TableAccessibilityItem {
    data class Node(val node: ViewerTablePresentedAccessibilityNode) : TableAccessibilityItem
    data class Table(val table: TableAccessibilityTable) : TableAccessibilityItem
}

internal interface TableAccessibilityEditing {
    fun tableAccessibilityActions(cell: TableAccessibilityCell): List<TableAccessibilityAction>
    fun performTableAccessibilityAction(action: TableAccessibilityAction, cell: TableAccessibilityCell): Boolean
    fun activateTableAccessibilityCell(cell: TableAccessibilityCell): Boolean
    fun activeTableAccessibilityInput(cell: TableAccessibilityCell): EditorEditText?
    fun detachedTableAccessibilityFrames(): List<TableAccessibilityDetachedFrame>
    fun canDeleteTableAccessibilityFrame(tableId: String): Boolean
    fun deleteTableAccessibilityFrame(tableId: String): Boolean
}

internal data class TableAccessibilityDetachedFrame(
    val tableId: String,
    val tablePos: Int,
    val kind: TableAccessibilityTable.Frame,
    val bounds: () -> RectF
)

internal data class TableAccessibilityLocation(
    val tableNodeId: Int,
    val table: TableAccessibilityTable,
    val cell: TableAccessibilityCell
)

internal object TableAccessibility {
    private const val LABEL_SEPARATOR = " "
    private const val DESCRIPTION_SEPARATOR = ", "

    fun items(
        snapshot: ViewerTablePresentationSnapshot,
        root: PreparedProseLayout,
        nodes: List<ViewerTablePresentedAccessibilityNode>
    ): List<TableAccessibilityItem> {
        val rootSurfaces = root.blocks.mapNotNull { it.tableSurface }
        val rootTables = snapshot.tables.filter { presented -> rootSurfaces.any { it === presented.surface } }
        val rootCells = rootTables.map { table -> snapshot.cells.filter { it.surface === table.surface } }
        val owners = IdentityHashMap<PreparedProseLayout, Pair<Int, Int>>()
        fun claim(layout: PreparedProseLayout, owner: Pair<Int, Int>) {
            owners[layout] = owner
            layout.blocks.forEach { block -> block.tableSurface?.cells?.forEach { claim(it.content, owner) } }
        }
        rootCells.forEachIndexed { tableIndex, cells ->
            cells.forEachIndexed { cellIndex, cell -> claim(cell.content, tableIndex to cellIndex) }
        }
        val rootNodes = mutableListOf<ViewerTablePresentedAccessibilityNode>()
        val cellNodes = rootCells.map { cells -> cells.map { mutableListOf<ViewerTablePresentedAccessibilityNode>() } }
        nodes.forEach { node ->
            if (node.layout === root) rootNodes += node
            else owners[node.layout]?.let { (table, cell) -> cellNodes[table][cell] += node }
        }
        val pending = rootTables.mapIndexed { index, presented ->
            val blockIndex = root.blocks.indexOfFirst { it.tableSurface === presented.surface }
            blockIndex to table(presented, rootCells[index], cellNodes[index])
        }.toMutableList()
        val items = mutableListOf<TableAccessibilityItem>()
        fun flushTables(before: Int) {
            while (pending.isNotEmpty() && pending.first().first < before) {
                items += TableAccessibilityItem.Table(pending.removeAt(0).second)
            }
        }
        rootNodes.forEach { node ->
            flushTables(node.node.sourceBlockIndex ?: Int.MAX_VALUE)
            items += TableAccessibilityItem.Node(node)
        }
        flushTables(Int.MAX_VALUE)
        return items
    }

    private fun table(
        presented: ViewerTablePresentedSurface,
        cells: List<ViewerTablePresentedCell>,
        nodes: List<List<ViewerTablePresentedAccessibilityNode>>
    ): TableAccessibilityTable {
        val source = presented.surface.sourceTable
        val accessibleCells = cells.zip(nodes).mapNotNull { (cell, cellNodes) ->
            val index = cell.cell.sourceCellIndex ?: return@mapNotNull null
            val sourceCell = source?.cells?.getOrNull(index) ?: return@mapNotNull null
            TableAccessibilityCell(
                cell, index, sourceCell.row.toInt(), sourceCell.column.toInt(),
                sourceCell.rowspan.toInt(), sourceCell.colspan.toInt(), sourceCell.header,
                text(cell.content).joinToString(LABEL_SEPARATOR), cellNodes
            )
        }
        return TableAccessibilityTable(
            presented,
            source?.rows?.toInt() ?: (presented.surface.layout.rowOffsets.size - 1).coerceAtLeast(0),
            source?.columns?.toInt() ?: presented.surface.layout.columnWidths.size,
            accessibleCells
        )
    }

    private fun text(layout: PreparedProseLayout): List<String> = layout.blocks.flatMap { block ->
        block.fragments.mapNotNull { fragment ->
            when (fragment.kind) {
                PreparedProseFragmentKind.TEXT -> fragment.layout?.text?.toString()
                PreparedProseFragmentKind.ATOM -> fragment.labelLayout?.text?.toString() ?: fragment.label
                else -> null
            }?.replace(LayoutConstants.OBJECT_REPLACEMENT_CHARACTER, "")?.trim()?.takeIf { it.isNotEmpty() }
        } + block.tableSurface?.cells.orEmpty().flatMap { text(it.content) }
    }

    fun spanDescription(view: View, cell: TableAccessibilityCell): String? = listOfNotNull(
        view.context.getString(R.string.table_accessibility_row_span, cell.rowSpan).takeIf { cell.rowSpan > 1 },
        view.context.getString(R.string.table_accessibility_column_span, cell.columnSpan)
            .takeIf { cell.columnSpan > 1 }
    ).takeIf { it.isNotEmpty() }?.joinToString(DESCRIPTION_SEPARATOR)

    fun describeCell(view: View, info: AccessibilityNodeInfo, table: TableAccessibilityTable, cell: TableAccessibilityCell) {
        val compat = AccessibilityNodeInfoCompat.wrap(info)
        compat.setCollectionItemInfo(
            AccessibilityNodeInfoCompat.CollectionItemInfoCompat.Builder()
                .setRowIndex(cell.row)
                .setRowSpan(cell.rowSpan)
                .setColumnIndex(cell.column)
                .setColumnSpan(cell.columnSpan)
                .setHeading(cell.isHeader)
                .setRowTitle(headerTitle(view, (cell.row until cell.row + cell.rowSpan).flatMap(table::rowHeaders), cell))
                .setColumnTitle(headerTitle(
                    view, (cell.column until cell.column + cell.columnSpan).flatMap(table::columnHeaders), cell
                ))
                .build()
        )
        compat.stateDescription = spanDescription(view, cell)
    }

    private fun headerTitle(view: View, headers: List<TableAccessibilityCell>, cell: TableAccessibilityCell): String? =
        headers.distinct().filter { it.sourcePosition != cell.sourcePosition }
            .joinToString(LABEL_SEPARATOR) { cellLabel(view, it) }.takeIf { it.isNotEmpty() }

    fun cellLabel(view: View, cell: TableAccessibilityCell): String =
        cell.label.ifEmpty { view.context.getString(R.string.table_accessibility_empty_cell) }

    fun addActions(view: View, info: AccessibilityNodeInfo, actions: List<TableAccessibilityAction>) {
        actions.forEach {
            info.addAction(AccessibilityNodeInfo.AccessibilityAction(it.id, view.context.getString(it.label)))
        }
    }
}

internal class TableCellAccessibility(
    private val drawing: PreparedProseDrawingView,
    private val surface: () -> ViewerTableSurface?,
    private val sourceCellIndex: Int,
    private val editing: TableAccessibilityEditing
) {
    private fun located(): TableAccessibilityLocation? =
        surface()?.let { drawing.tableAccessibilityLocation(it, sourceCellIndex) }

    fun isPlacedInTable(): Boolean = located() != null

    fun populate(input: View, info: AccessibilityNodeInfo) {
        val location = located() ?: return
        info.setParent(drawing, location.tableNodeId)
        TableAccessibility.describeCell(input, info, location.table, location.cell)
        TableAccessibility.addActions(input, info, editing.tableAccessibilityActions(location.cell))
    }

    fun perform(action: Int): Boolean {
        val cell = located()?.cell ?: return false
        val requested = editing.tableAccessibilityActions(cell).firstOrNull { it.id == action } ?: return false
        return editing.performTableAccessibilityAction(requested, cell)
    }
}

internal class TableAccessibilityNodes(
    private val host: View,
    private val drawing: PreparedProseDrawingView,
    private val items: () -> List<TableAccessibilityItem>,
    private val generation: () -> Any,
    private val originInHost: () -> Pair<Int, Int>,
    private val visibleOnScreen: (Rect) -> Boolean,
    private val onFocusClaimed: () -> Unit
) {
    companion object {
        const val FIRST_TABLE_NODE_ID = 1 shl 24
    }

    private sealed interface Entry {
        data class Table(val table: TableAccessibilityTable) : Entry
        data class Cell(val table: TableAccessibilityTable, val cell: TableAccessibilityCell) : Entry
        data class Detached(val frame: TableAccessibilityDetachedFrame) : Entry
    }

    private data class Snapshot(val items: List<TableAccessibilityItem>, val entries: List<Entry>)
    private data class Identity(val table: String, val sourcePosition: Int?)
    private data class Focused(val virtualId: Int, val identity: Identity)

    private val accessibilityManager = host.context.getSystemService(AccessibilityManager::class.java)
    private var focused: Focused? = null
    private var cached: Pair<Any, Snapshot>? = null
    var editing: TableAccessibilityEditing? = null
        set(value) {
            field = value
            cached = null
        }

    fun isTableNode(virtualId: Int): Boolean = virtualId >= FIRST_TABLE_NODE_ID

    private fun snapshot(): Snapshot {
        val current = generation()
        cached?.takeIf { it.first == current }?.let { return it.second }
        val items = items()
        val entries = items.filterIsInstance<TableAccessibilityItem.Table>().flatMap { item ->
            listOf(Entry.Table(item.table)) + item.table.cells.map { Entry.Cell(item.table, it) }
        } + editing?.detachedTableAccessibilityFrames().orEmpty().map { Entry.Detached(it) }
        return Snapshot(items, entries).also { cached = current to it }
    }

    private fun identity(entry: Entry) = when (entry) {
        is Entry.Table -> Identity(entry.table.surface.identity, null)
        is Entry.Cell -> Identity(entry.table.surface.identity, entry.cell.sourcePosition)
        is Entry.Detached -> Identity(entry.frame.tableId, null)
    }

    private fun idOf(entries: List<Entry>, table: TableAccessibilityTable): Int =
        FIRST_TABLE_NODE_ID + entries.indexOfFirst { it is Entry.Table && it.table === table }

    fun hostChildren(nodeId: (ViewerTablePresentedAccessibilityNode) -> Int?): List<Int> {
        val (items, entries) = snapshot()
        val detached = entries.indices.filter { entries[it] is Entry.Detached }
            .map { (entries[it] as Entry.Detached).frame.tablePos to FIRST_TABLE_NODE_ID + it }
            .sortedBy { it.first }.toMutableList()
        val children = mutableListOf<Int>()
        fun flushDetached(before: Int) {
            while (detached.isNotEmpty() && detached.first().first < before) children += detached.removeAt(0).second
        }
        items.forEach { item ->
            when (item) {
                is TableAccessibilityItem.Node -> nodeId(item.node)?.let { children += it }
                is TableAccessibilityItem.Table -> {
                    flushDetached(item.table.tablePos ?: Int.MAX_VALUE)
                    children += idOf(entries, item.table)
                }
            }
        }
        flushDetached(Int.MAX_VALUE)
        return children
    }

    fun parentOf(node: ViewerTablePresentedAccessibilityNode): Int? {
        snapshot().entries.forEachIndexed { index, entry ->
            if (entry is Entry.Cell && entry.cell.interactions.any { it.sourceIdentity == node.sourceIdentity }) {
                return FIRST_TABLE_NODE_ID + index
            }
        }
        return null
    }

    fun locate(surface: ViewerTableSurface, sourceCellIndex: Int): TableAccessibilityLocation? {
        val entries = snapshot().entries
        val cell = entries.filterIsInstance<Entry.Cell>().firstOrNull {
            it.table.surface === surface && it.cell.sourceCellIndex == sourceCellIndex
        } ?: return null
        return TableAccessibilityLocation(idOf(entries, cell.table), cell.table, cell.cell)
    }

    private fun parentBounds(bounds: RectF, clip: RectF?): Rect {
        val visible = RectF(bounds)
        val (x, y) = originInHost()
        val chosen = if (clip != null && visible.intersect(clip)) visible else RectF(bounds)
        return Rect(chosen.left.toInt(), chosen.top.toInt(), chosen.right.toInt(), chosen.bottom.toInt())
            .apply { offset(x, y) }
    }

    private fun screenBounds(parent: Rect): Rect {
        val location = IntArray(2)
        host.getLocationOnScreen(location)
        return Rect(parent).apply { offset(location[0], location[1]) }
    }

    private fun geometry(entry: Entry): Pair<RectF, RectF?> = when (entry) {
        is Entry.Table -> entry.table.presented.bounds to entry.table.presented.clip
        is Entry.Cell -> entry.cell.presented.bounds to entry.table.presented.clip
        is Entry.Detached -> entry.frame.bounds() to null
    }

    private fun visibilityGeometry(entry: Entry, own: Pair<RectF, RectF?>): Pair<RectF, RectF?> =
        if (entry is Entry.Cell) geometry(Entry.Table(entry.table)) else own

    private fun visible(geometry: Pair<RectF, RectF?>): Boolean {
        val (bounds, clip) = geometry
        if (clip != null && !RectF(bounds).intersect(clip)) return false
        return visibleOnScreen(screenBounds(parentBounds(bounds, clip)))
    }

    private fun visible(entry: Entry): Boolean {
        val own = geometry(entry)
        return visible(visibilityGeometry(entry, own))
    }

    @Suppress("DEPRECATION")
    fun create(virtualId: Int, nodeId: (ViewerTablePresentedAccessibilityNode) -> Int?): AccessibilityNodeInfo? {
        val entries = snapshot().entries
        val entry = entries.getOrNull(virtualId - FIRST_TABLE_NODE_ID) ?: return null
        reconcile()
        val own = geometry(entry)
        val parent = parentBounds(own.first, own.second)
        return AccessibilityNodeInfo.obtain().apply {
            packageName = host.context.packageName
            setSource(host, virtualId)
            setBoundsInParent(parent)
            setBoundsInScreen(screenBounds(parent))
            isVisibleToUser = visible(visibilityGeometry(entry, own))
            isAccessibilityFocused = focused?.identity == identity(entry)
            addAction(
                if (isAccessibilityFocused) AccessibilityNodeInfo.AccessibilityAction.ACTION_CLEAR_ACCESSIBILITY_FOCUS
                else AccessibilityNodeInfo.AccessibilityAction.ACTION_ACCESSIBILITY_FOCUS
            )
            when (entry) {
                is Entry.Table -> entry.table.frame?.let { describeFrame(this, it, entry.table.tableId) }
                    ?: describeTable(this, entries, entry.table)
                is Entry.Cell -> describeCell(this, entries, entry, nodeId)
                is Entry.Detached -> describeFrame(this, entry.frame.kind, entry.frame.tableId)
            }
        }
    }

    private fun describeFrame(info: AccessibilityNodeInfo, kind: TableAccessibilityTable.Frame, tableId: String?) {
        info.setParent(host)
        info.className = android.widget.TextView::class.java.name
        info.text = host.context.getString(
            if (kind == TableAccessibilityTable.Frame.EMPTY) R.string.table_accessibility_empty_table
            else R.string.table_accessibility_failed_table
        )
        info.isFocusable = true
        AndroidApiCompat.setScreenReaderFocusable(info, true)
        if (tableId != null && editing?.canDeleteTableAccessibilityFrame(tableId) == true) {
            TableAccessibility.addActions(host, info, listOf(TableAccessibilityAction.DELETE_TABLE))
        }
    }

    private fun describeTable(info: AccessibilityNodeInfo, entries: List<Entry>, table: TableAccessibilityTable) {
        info.setParent(host)
        info.className = android.widget.GridView::class.java.name
        info.contentDescription = host.context.getString(R.string.table_accessibility_table)
        AccessibilityNodeInfoCompat.wrap(info).setCollectionInfo(
            AccessibilityNodeInfoCompat.CollectionInfoCompat.obtain(
                table.rowCount, table.columnCount, false,
                AccessibilityNodeInfoCompat.CollectionInfoCompat.SELECTION_MODE_NONE
            )
        )
        entries.forEachIndexed { index, entry ->
            if (entry !is Entry.Cell || entry.table !== table) return@forEachIndexed
            val input = editing?.activeTableAccessibilityInput(entry.cell)
            if (input != null) info.addChild(input) else info.addChild(host, FIRST_TABLE_NODE_ID + index)
        }
    }

    private fun describeCell(
        info: AccessibilityNodeInfo,
        entries: List<Entry>,
        entry: Entry.Cell,
        nodeId: (ViewerTablePresentedAccessibilityNode) -> Int?
    ) {
        info.setParent(host, idOf(entries, entry.table))
        info.className = android.widget.TextView::class.java.name
        info.text = TableAccessibility.cellLabel(host, entry.cell)
        info.isFocusable = true
        AndroidApiCompat.setScreenReaderFocusable(info, true)
        TableAccessibility.describeCell(host, info, entry.table, entry.cell)
        entry.cell.interactions.mapNotNull(nodeId).forEach { info.addChild(host, it) }
        val editing = editing ?: return
        info.isClickable = true
        info.addAction(AccessibilityNodeInfo.AccessibilityAction.ACTION_CLICK)
        TableAccessibility.addActions(host, info, editing.tableAccessibilityActions(entry.cell))
    }

    fun perform(virtualId: Int, action: Int): Boolean {
        val entry = snapshot().entries.getOrNull(virtualId - FIRST_TABLE_NODE_ID) ?: return false
        return when (action) {
            AccessibilityNodeInfo.ACTION_ACCESSIBILITY_FOCUS -> requestFocus(virtualId, entry)
            AccessibilityNodeInfo.ACTION_CLEAR_ACCESSIBILITY_FOCUS -> clearFocus(virtualId)
            else -> when (entry) {
                is Entry.Cell -> performCellAction(entry.cell, action)
                is Entry.Table -> entry.table.tableId?.takeIf { entry.table.frame != null }
                    ?.let { performFrameAction(it, action) } ?: false
                is Entry.Detached -> performFrameAction(entry.frame.tableId, action)
            }
        }
    }

    private fun performFrameAction(tableId: String, action: Int): Boolean {
        val editing = editing ?: return false
        if (action != TableAccessibilityAction.DELETE_TABLE.id || !editing.canDeleteTableAccessibilityFrame(tableId)) {
            return false
        }
        return editing.deleteTableAccessibilityFrame(tableId)
    }

    private fun performCellAction(cell: TableAccessibilityCell, action: Int): Boolean {
        val editing = editing ?: return false
        if (action == AccessibilityNodeInfo.ACTION_CLICK) return editing.activateTableAccessibilityCell(cell)
        val requested = editing.tableAccessibilityActions(cell).firstOrNull { it.id == action } ?: return false
        return editing.performTableAccessibilityAction(requested, cell)
    }

    private fun requestFocus(virtualId: Int, entry: Entry): Boolean {
        if (!visible(entry)) return false
        val identity = identity(entry)
        if (focused?.identity == identity) return false
        (entry as? Entry.Cell)?.let { drawing.revealTableAccessibilityCell(it.cell) }
        clearFocus()
        onFocusClaimed()
        focused = Focused(virtualId, identity)
        host.invalidate()
        send(virtualId, AccessibilityEvent.TYPE_VIEW_ACCESSIBILITY_FOCUSED)
        return true
    }

    fun clearFocus(virtualId: Int = focused?.virtualId ?: View.NO_ID): Boolean {
        val current = focused ?: return false
        if (virtualId == View.NO_ID || virtualId != current.virtualId) return false
        focused = null
        host.invalidate()
        send(virtualId, AccessibilityEvent.TYPE_VIEW_ACCESSIBILITY_FOCUS_CLEARED)
        return true
    }

    fun reconcile() {
        val current = focused ?: return
        val entries = snapshot().entries
        val index = entries.indexOfFirst { identity(it) == current.identity }
        if (index < 0 || FIRST_TABLE_NODE_ID + index != current.virtualId || !visible(entries[index])) {
            clearFocus(current.virtualId)
        }
    }

    @Suppress("DEPRECATION")
    private fun send(virtualId: Int, type: Int) {
        if (!accessibilityManager.isEnabled) return
        val event = AccessibilityEvent.obtain(type).apply {
            packageName = host.context.packageName
            setSource(host, virtualId)
        }
        host.parent?.requestSendAccessibilityEvent(host, event)
    }
}
