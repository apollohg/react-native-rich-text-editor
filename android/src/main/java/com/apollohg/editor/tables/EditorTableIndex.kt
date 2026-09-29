package com.apollohg.editor.tables

import uniffi.editor_core.FfiViewerElement
import com.apollohg.editor.canonicalV2U64
import org.json.JSONException
import org.json.JSONObject
import uniffi.editor_core.FfiCellNestedTable
import uniffi.editor_core.FfiTableCellRecord
import uniffi.editor_core.FfiTableExtent
import uniffi.editor_core.FfiTableFrame
import uniffi.editor_core.FfiTableFrameKind
import uniffi.editor_core.FfiTableHost
import uniffi.editor_core.FfiTableRecord

internal data class TableFrameChanges(
    val fullReset: Boolean, val replacedTables: Set<String>, val removedTables: Set<String>,
    val changedCells: Map<String, Set<Int>>
)

internal sealed class TableFrameRejection : Exception() {
    data class BaseRevisionMismatch(val expected: ULong?, val actual: ULong) : TableFrameRejection()
    data class UnknownTable(val key: String) : TableFrameRejection()
    data class CellIndexOutOfRange(val key: String, val index: Int) : TableFrameRejection()
    data class CellStructureChanged(val key: String, val index: Int) : TableFrameRejection()
    data class DocSizeMismatch(val key: String, val expected: UInt, val actual: UInt) : TableFrameRejection()
    data class ScalarSizeMismatch(val key: String, val expected: UInt, val actual: UInt) : TableFrameRejection()
    data class InputBlockOutOfStride(val key: String, val index: Int) : TableFrameRejection()
    data class MissingAttribute(val key: String) : TableFrameRejection()
    data class DuplicateTableKey(val key: String) : TableFrameRejection()
    data class HostMissing(val key: String) : TableFrameRejection()
    data object ExtentsIncomplete : TableFrameRejection()
}

internal sealed class TableFrameAdoption {
    data class Adopted(val changes: TableFrameChanges) : TableFrameAdoption()
    data class Rejected(val rejection: TableFrameRejection) : TableFrameAdoption()
}

internal class EditorTableIndex {
    private data class Entry(
        val record: FfiTableRecord, val docPrefix: LongArray, val scalarPrefix: LongArray,
        val attributeCounts: Map<String, Int>, val nestedCells: Map<String, Int>
    ) {
        val scalarSize: UInt get() = scalarPrefix.last().toUInt()
    }

    private data class Origin(val doc: UInt, val scalar: UInt?)

    private var entries: Map<String, Entry> = emptyMap()
    private var attributes: Map<String, String> = emptyMap()
    var attributeObjects: Map<String, JSONObject> = emptyMap()
        private set
    private var extents: Map<String, FfiTableExtent> = emptyMap()
    private var origins: Map<String, Origin> = emptyMap()
    private var roots: List<String> = emptyList()

    fun adopt(frame: FfiTableFrame, installedRevision: ULong?, frameRevision: ULong): TableFrameAdoption = try {
        TableFrameAdoption.Adopted(stage(frame, installedRevision, frameRevision))
    } catch (rejection: TableFrameRejection) {
        TableFrameAdoption.Rejected(rejection)
    }

    private fun stage(frame: FfiTableFrame, installedRevision: ULong?, frameRevision: ULong): TableFrameChanges {
        val full = frame.kind == FfiTableFrameKind.FULL
        if (!full) {
            val base = canonicalV2U64(frame.baseDocumentRevision)?.toULong()
            if (base == null || base != installedRevision) {
                throw TableFrameRejection.BaseRevisionMismatch(installedRevision, base ?: frameRevision)
            }
        }
        val next = if (full) mutableMapOf() else entries.toMutableMap()
        val pool = if (full) mutableMapOf() else attributes.toMutableMap()
        val objects = if (full) mutableMapOf() else attributeObjects.toMutableMap()
        var nextExtents = if (full) mutableMapOf() else extents.toMutableMap()
        val removed = mutableSetOf<String>()
        val replaced = mutableSetOf<String>()
        val changed = mutableMapOf<String, Set<Int>>()
        frame.removedAttributeKeys.forEach { key -> pool.remove(key); objects.remove(key) }
        frame.attributes.forEach { attribute ->
            val parsed = try {
                JSONObject(attribute.json)
            } catch (_: JSONException) {
                throw TableFrameRejection.MissingAttribute(attribute.key)
            }
            pool[attribute.key] = attribute.json
            objects[attribute.key] = parsed
        }
        frame.removedTableKeys.forEach { key ->
            if (!removed.add(key)) throw TableFrameRejection.DuplicateTableKey(key)
            if (next.remove(key) == null) throw TableFrameRejection.UnknownTable(key)
        }
        frame.tables.forEach { table ->
            val key = table.tableKey
            if (!replaced.add(key) || key in removed) throw TableFrameRejection.DuplicateTableKey(key)
            next[key] = entry(table, pool)
        }
        frame.cellUpdates.groupBy { it.tableKey }.forEach { (key, updates) ->
            val prior = next[key] ?: throw TableFrameRejection.UnknownTable(key)
            val indexes = sortedSetOf<Int>()
            updates.forEach { update ->
                val index = update.cellIndex.toInt()
                if (index !in prior.record.cells.indices) throw TableFrameRejection.CellIndexOutOfRange(key, index)
                if (key in replaced || !indexes.add(index) || !sameStructure(prior.record.cells[index], update.cell)) {
                    throw TableFrameRejection.CellStructureChanged(key, index)
                }
                validate(update.cell, key, index, pool)
            }
            val nestedCells = prior.nestedCells.toMutableMap()
            indexes.forEach { index -> prior.record.cells[index].nestedTables.forEach { nestedCells.remove(it.tableKey) } }
            val cells = prior.record.cells.toMutableList()
            val counts = prior.attributeCounts.toMutableMap()
            var docSize = prior.record.docSize
            updates.forEach { update ->
                val index = update.cellIndex.toInt()
                val old = cells[index]
                val size = docSize.toLong() - old.docSize.toLong() + update.cell.docSize.toLong()
                docSize = checkedUInt(size) ?: throw TableFrameRejection.DocSizeMismatch(key, docSize, update.cell.docSize)
                update.cell.nestedTables.forEach { nested ->
                    if (nestedCells.put(nested.tableKey, index) != null) throw TableFrameRejection.DuplicateTableKey(nested.tableKey)
                }
                cells[index] = update.cell
                val oldCount = counts.getValue(old.attrsKey) - 1
                if (oldCount == 0) counts.remove(old.attrsKey) else counts[old.attrsKey] = oldCount
                counts[update.cell.attrsKey] = (counts[update.cell.attrsKey] ?: 0) + 1
            }
            val updated = Entry(prior.record.copy(docSize = docSize, cells = cells), prior.docPrefix.copyOf(),
                prior.scalarPrefix.copyOf(), counts, nestedCells)
            indexes.firstOrNull()?.let { rebuildPrefixes(updated, it) }
            next[key] = updated
            changed[key] = indexes
        }
        frame.removedAttributeKeys.filter { it !in pool }.forEach { key ->
            if (next.values.any { key in it.attributeCounts }) throw TableFrameRejection.MissingAttribute(key)
        }
        if (full || installedRevision != frameRevision || frame.extents.isNotEmpty()) {
            nextExtents = mutableMapOf()
            frame.extents.forEach { extent ->
                if (nextExtents.put(extent.tableKey, extent) != null) throw TableFrameRejection.DuplicateTableKey(extent.tableKey)
            }
        }
        next.forEach { (key, entry) ->
            val host = entry.record.host
            if (host != null) {
                val parent = next[host.tableKey] ?: throw TableFrameRejection.HostMissing(key)
                val cell = parent.record.cells.getOrNull(host.cellIndex.toInt()) ?: throw TableFrameRejection.HostMissing(key)
                val nested = cell.nestedTables.firstOrNull { it.tableKey == key } ?: throw TableFrameRejection.HostMissing(key)
                validateNested(nested, entry, cell)
            } else {
                val extent = nextExtents[key] ?: throw TableFrameRejection.ExtentsIncomplete
                if (extent.docSize != entry.record.docSize) {
                    throw TableFrameRejection.DocSizeMismatch(key, entry.record.docSize, extent.docSize)
                }
                if (extent.scalarEnd < extent.scalarStart) throw TableFrameRejection.ScalarSizeMismatch(key, entry.scalarSize, 0u)
                val width = extent.scalarEnd - extent.scalarStart
                if (entry.record.failure == null && width != entry.scalarSize) {
                    throw TableFrameRejection.ScalarSizeMismatch(key, entry.scalarSize, width)
                }
                if (checkedUInt(extent.docStart.toLong() + extent.docSize.toLong()) == null) {
                    throw TableFrameRejection.ExtentsIncomplete
                }
            }
        }
        next.forEach { (key, entry) ->
            entry.nestedCells.forEach { (childKey, index) ->
                val child = next[childKey] ?: throw TableFrameRejection.UnknownTable(childKey)
                if (child.record.host != FfiTableHost(key, index.toUInt())) throw TableFrameRejection.HostMissing(childKey)
            }
        }
        val rootKeys = next.filterValues { it.record.host == null }.keys
        if (rootKeys != nextExtents.keys) throw TableFrameRejection.ExtentsIncomplete
        val orderedRoots = rootKeys.sortedBy { nextExtents.getValue(it).docStart }
        var previousDocEnd = 0L
        var previousScalarEnd = 0u
        orderedRoots.forEach { key ->
            val extent = nextExtents.getValue(key)
            if (extent.docStart.toLong() < previousDocEnd || extent.scalarStart < previousScalarEnd) {
                throw TableFrameRejection.ExtentsIncomplete
            }
            previousDocEnd = extent.docStart.toLong() + extent.docSize.toLong()
            previousScalarEnd = extent.scalarEnd
        }
        val nextOrigins = mutableMapOf<String, Origin>()
        next.keys.forEach { key ->
            val path = mutableListOf<String>()
            val visited = mutableSetOf<String>()
            var current = key
            while (current !in nextOrigins) {
                if (!visited.add(current)) throw TableFrameRejection.HostMissing(current)
                val entry = next[current] ?: throw TableFrameRejection.HostMissing(current)
                path += current
                val host = entry.record.host
                if (host == null) {
                    val extent = nextExtents.getValue(current)
                    nextOrigins[current] = Origin(extent.docStart, extent.scalarStart)
                    break
                }
                current = host.tableKey
            }
            path.asReversed().filter { it !in nextOrigins }.forEach { childKey ->
                val host = requireNotNull(next.getValue(childKey).record.host)
                val parent = next.getValue(host.tableKey)
                val origin = nextOrigins.getValue(host.tableKey)
                val index = host.cellIndex.toInt()
                val nested = parent.record.cells[index].nestedTables.first { it.tableKey == childKey }
                val doc = origin.doc.toLong() + relativeDocStart(parent, index) + nested.docOffset.toLong()
                val docStart = checkedUInt(doc) ?: throw TableFrameRejection.HostMissing(childKey)
                val scalar = origin.scalar?.let { start -> nested.scalarStart?.let {
                    checkedUInt(start.toLong() + parent.scalarPrefix[index] + it.toLong())
                } }
                nextOrigins[childKey] = Origin(docStart, scalar)
            }
        }
        entries = next
        attributes = pool
        attributeObjects = objects
        extents = nextExtents
        origins = nextOrigins
        roots = orderedRoots
        return TableFrameChanges(full, replaced, removed, changed)
    }

    val tableKeys: Set<String> get() = entries.keys
    val rootExtents: Map<String, FfiTableExtent> get() = extents

    fun copy(): EditorTableIndex = EditorTableIndex().also {
        it.entries = entries
        it.attributes = attributes
        it.attributeObjects = attributeObjects
        it.extents = extents
        it.origins = origins
        it.roots = roots
    }

    fun subtree(tableKeys: Set<String>): EditorTableIndex {
        val selected = linkedMapOf<String, Entry>()
        val pending = java.util.ArrayDeque(tableKeys)
        while (pending.isNotEmpty()) {
            val key = pending.removeLast()
            if (key in selected) continue
            val entry = entries[key] ?: continue
            selected[key] = entry
            pending.addAll(entry.nestedCells.keys)
        }
        val keys = selected.values.flatMapTo(mutableSetOf()) { it.attributeCounts.keys }
        return EditorTableIndex().also { result ->
            result.entries = selected
            result.attributes = keys.mapNotNull { key -> attributes[key]?.let { key to it } }.toMap()
            result.attributeObjects = keys.mapNotNull { key -> attributeObjects[key]?.let { key to it } }.toMap()
            result.origins = selected.keys.mapNotNull { key -> origins[key]?.let { key to it } }.toMap()
        }
    }

    fun record(tableKey: String): FfiTableRecord? = entries[tableKey]?.record
    fun tableDocStart(tableKey: String): UInt? = origins[tableKey]?.doc

    fun docStart(tableKey: String, cellIndex: Int): UInt? {
        val entry = entries[tableKey] ?: return null
        if (cellIndex !in entry.record.cells.indices) return null
        val origin = origins[tableKey] ?: return null
        return checkedUInt(origin.doc.toLong() + relativeDocStart(entry, cellIndex))
    }

    fun scalarStart(tableKey: String, cellIndex: Int): UInt? {
        val entry = entries[tableKey] ?: return null
        if (cellIndex !in entry.record.cells.indices) return null
        val start = origins[tableKey]?.scalar ?: return null
        return checkedUInt(start.toLong() + entry.scalarPrefix[cellIndex])
    }

    fun cellIndexContainingDoc(tableKey: String, position: UInt): Int? {
        val entry = entries[tableKey] ?: return null
        val origin = origins[tableKey] ?: return null
        if (position < origin.doc) return null
        val relative = position.toLong() - origin.doc.toLong()
        val index = precedingIndex(entry.record.cells.size) { relativeDocStart(entry, it) <= relative } ?: return null
        return index.takeIf { relative < relativeDocStart(entry, it) + entry.record.cells[it].docSize.toLong() }
    }

    fun cellIndexContainingScalar(tableKey: String, position: UInt): Int? {
        val entry = entries[tableKey] ?: return null
        val start = origins[tableKey]?.scalar ?: return null
        if (position < start) return null
        val relative = position.toLong() - start.toLong()
        val index = precedingIndex(entry.record.cells.size) { entry.scalarPrefix[it] <= relative } ?: return null
        return index.takeIf { relative < entry.scalarPrefix[it + 1] ||
            (it == entry.record.cells.lastIndex && relative == entry.scalarPrefix[it + 1]) }
    }

    fun tableKeyContainingDoc(position: UInt): String? = containingTable(position, false)
    fun tableKeyContainingScalar(position: UInt): String? = containingTable(position, true)

    private fun containingTable(position: UInt, scalar: Boolean): String? {
        val rootIndex = precedingIndex(roots.size) {
            extents.getValue(roots[it]).let { extent -> (if (scalar) extent.scalarStart else extent.docStart) <= position }
        } ?: return null
        var key = roots[rootIndex]
        val extent = extents.getValue(key)
        if (if (scalar) position > extent.scalarEnd else position.toLong() >= extent.docStart.toLong() + extent.docSize.toLong()) return null
        while (true) {
            val index = if (scalar) cellIndexContainingScalar(key, position) else cellIndexContainingDoc(key, position)
            if (index == null) break
            val nested = entries.getValue(key).record.cells[index].nestedTables.firstOrNull { nested ->
                val origin = origins[nested.tableKey]
                val child = entries[nested.tableKey]
                if (origin == null || child == null) false
                else if (scalar) origin.scalar?.let { start ->
                    start <= position && position.toLong() <= start.toLong() + child.scalarSize.toLong()
                } == true
                else origin.doc <= position && position.toLong() < origin.doc.toLong() + child.record.docSize.toLong()
            } ?: break
            key = nested.tableKey
        }
        return key
    }

    fun absoluteDocPos(tableKey: String, cellIndex: Int, relative: UInt): UInt? {
        val cell = entries[tableKey]?.record?.cells?.getOrNull(cellIndex) ?: return null
        if (relative > cell.docSize) return null
        val start = docStart(tableKey, cellIndex) ?: return null
        return checkedUInt(start.toLong() + relative.toLong())
    }

    fun inputSegments(tableKey: String, cellIndex: Int): List<TableCellPositionMap.Segment>? {
        val cell = entries[tableKey]?.record?.cells?.getOrNull(cellIndex) ?: return null
        val start = scalarStart(tableKey, cellIndex) ?: return null
        return cell.inputBlocks.map { block ->
            val collapsed = cell.nestedTables.filter { it.elementIndex < block.elementIndex }.sumOf { nested ->
                val lower = nested.scalarStart
                val upper = nested.scalarEnd
                if (lower != null && upper != null) upper.toLong() - lower.toLong() - 1 else 0L
            }
            val local = checkedInt(block.scalarStart.toLong() - collapsed) ?: return null
            val end = checkedInt(block.scalarEnd.toLong() - collapsed + 1) ?: return null
            val global = checkedInt(start.toLong() + block.scalarStart.toLong()) ?: return null
            TableCellPositionMap.Segment(local, end, global)
        }
    }

    private companion object {
        const val NODE_BOUNDARY_SIZE = 2L

        fun checkedUInt(value: Long): UInt? = value.takeIf { it in 0..UInt.MAX_VALUE.toLong() }?.toUInt()
        fun checkedInt(value: Long): Int? = value.takeIf { it in 0..Int.MAX_VALUE.toLong() }?.toInt()

        fun entry(record: FfiTableRecord, pool: Map<String, String>): Entry {
            val key = record.tableKey
            val counts = mutableMapOf<String, Int>()
            val nestedCells = mutableMapOf<String, Int>()
            (listOf(record.attrsKey) + record.sourceRows.map { it.attrsKey } + record.syntheticRegions.map { it.attrsKey }).forEach {
                if (it !in pool) throw TableFrameRejection.MissingAttribute(it)
                counts[it] = (counts[it] ?: 0) + 1
            }
            var sourceRow = 0
            var rowCellCount = 0u
            record.cells.forEachIndexed { index, cell ->
                while (sourceRow < record.sourceRows.size && rowCellCount == record.sourceRows[sourceRow].cellCount) {
                    sourceRow++
                    rowCellCount = 0u
                }
                if (sourceRow >= record.sourceRows.size || cell.sourceRow != sourceRow.toUInt() ||
                    cell.rowspan == 0u || cell.colspan == 0u || cell.row >= record.rows || cell.column >= record.columns ||
                    cell.rowspan > record.rows - cell.row || cell.colspan > record.columns - cell.column) {
                    throw TableFrameRejection.CellStructureChanged(key, index)
                }
                rowCellCount++
                validate(cell, key, index, pool)
                counts[cell.attrsKey] = (counts[cell.attrsKey] ?: 0) + 1
                cell.nestedTables.forEach { nested ->
                    if (nestedCells.put(nested.tableKey, index) != null) throw TableFrameRejection.DuplicateTableKey(nested.tableKey)
                }
            }
            if (record.failure == null && record.sourceRows.sumOf { it.cellCount.toLong() } != record.cells.size.toLong()) {
                throw TableFrameRejection.CellIndexOutOfRange(key, record.cells.size)
            }
            val entry = Entry(record, LongArray(record.cells.size + 1), LongArray(record.cells.size + 1), counts, nestedCells)
            rebuildPrefixes(entry, 0)
            val expected = NODE_BOUNDARY_SIZE * (record.sourceRows.size.toLong() + 1) + entry.docPrefix.last()
            if ((record.failure == null && expected != record.docSize.toLong()) || (record.failure != null && record.cells.isNotEmpty())) {
                throw TableFrameRejection.DocSizeMismatch(key, expected.coerceAtMost(UInt.MAX_VALUE.toLong()).toUInt(), record.docSize)
            }
            return entry
        }

        fun rebuildPrefixes(entry: Entry, first: Int) {
            for (index in first until entry.record.cells.size) {
                val cell = entry.record.cells[index]
                val doc = checkedUInt(entry.docPrefix[index] + cell.docSize.toLong())
                    ?: throw TableFrameRejection.DocSizeMismatch(entry.record.tableKey, entry.record.docSize, cell.docSize)
                val scalar = checkedUInt(entry.scalarPrefix[index] + cell.scalarStride.toLong())
                    ?: throw TableFrameRejection.ScalarSizeMismatch(entry.record.tableKey, entry.scalarPrefix[index].toUInt(), cell.scalarStride)
                entry.docPrefix[index + 1] = doc.toLong()
                entry.scalarPrefix[index + 1] = scalar.toLong()
            }
        }

        fun sameStructure(first: FfiTableCellRecord, second: FfiTableCellRecord): Boolean =
            first.sourceRow == second.sourceRow && first.row == second.row && first.column == second.column &&
                first.rowspan == second.rowspan && first.colspan == second.colspan && first.header == second.header

        fun validate(cell: FfiTableCellRecord, tableKey: String, index: Int, pool: Map<String, String>) {
            if (cell.attrsKey !in pool) throw TableFrameRejection.MissingAttribute(cell.attrsKey)
            val voidIndices = mutableSetOf<UInt>()
            cell.voidElementIndices.forEach { elementIndex ->
                val element = cell.elements.getOrNull(elementIndex.toInt())
                if (!voidIndices.add(elementIndex) || element !is FfiViewerElement.InlineAtom &&
                    element !is FfiViewerElement.BlockAtom) {
                    throw TableFrameRejection.InputBlockOutOfStride(tableKey, index)
                }
            }
            var previousDoc = 0u
            var previousScalar = 0u
            cell.inputBlocks.forEach { block ->
                if (block.elementIndex.toLong() >= cell.elements.size || previousDoc > block.docStart ||
                    block.docStart > block.docEnd || block.docEnd > cell.docSize ||
                    previousScalar > block.scalarStart || block.scalarStart > block.contentScalarStart ||
                    block.contentScalarStart > block.scalarEnd || block.scalarEnd > block.breakScalarEnd ||
                    block.breakScalarEnd > cell.scalarStride) {
                    throw TableFrameRejection.InputBlockOutOfStride(tableKey, index)
                }
                previousDoc = block.docEnd
                previousScalar = block.breakScalarEnd
            }
        }

        fun validateNested(nested: FfiCellNestedTable, child: Entry, parent: FfiTableCellRecord) {
            val key = child.record.tableKey
            if (nested.elementIndex.toLong() >= parent.elements.size || nested.docOffset.toLong() + nested.docSize.toLong() > parent.docSize.toLong()) {
                throw TableFrameRejection.HostMissing(key)
            }
            if (nested.docSize != child.record.docSize) throw TableFrameRejection.DocSizeMismatch(key, child.record.docSize, nested.docSize)
            val lower = nested.scalarStart
            val upper = nested.scalarEnd
            val width = when {
                lower != null && upper != null && lower <= upper && upper <= parent.scalarStride -> upper - lower
                lower == null && upper == null -> 0u
                else -> throw TableFrameRejection.ScalarSizeMismatch(key, child.scalarSize, 0u)
            }
            if (child.record.failure == null && width != child.scalarSize) throw TableFrameRejection.ScalarSizeMismatch(key, child.scalarSize, width)
        }

        fun relativeDocStart(entry: Entry, index: Int): Long =
            NODE_BOUNDARY_SIZE * (entry.record.cells[index].sourceRow.toLong() + 1) + entry.docPrefix[index]

        fun precedingIndex(count: Int, before: (Int) -> Boolean): Int? {
            var low = 0
            var high = count
            while (low < high) {
                val middle = low + (high - low) / 2
                if (before(middle)) low = middle + 1 else high = middle
            }
            return if (low == 0) null else low - 1
        }
    }
}
