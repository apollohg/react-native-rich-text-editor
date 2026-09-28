package com.apollohg.editor.tables

import com.apollohg.editor.EditorV2Adapter
import com.apollohg.editor.EditorV2CallResult
import com.apollohg.editor.UniffiEditorV2Backend
import org.json.JSONObject
import uniffi.editor_core.editorV2RenderNativeFrame
import uniffi.editor_core.editorV2RenderUpdate
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config
import uniffi.editor_core.FfiCellInputBlock
import uniffi.editor_core.FfiCellNestedTable
import uniffi.editor_core.FfiTableAttribute
import uniffi.editor_core.FfiTableCellRecord
import uniffi.editor_core.FfiTableCellUpdate
import uniffi.editor_core.FfiTableExtent
import uniffi.editor_core.FfiTableFrame
import uniffi.editor_core.FfiTableFrameKind
import uniffi.editor_core.FfiTableHost
import uniffi.editor_core.FfiTableRecord
import uniffi.editor_core.FfiTableSourceRow
import uniffi.editor_core.FfiViewerElement
import uniffi.editor_core.TableRenderFailure

@RunWith(RobolectricTestRunner::class)
@Config(sdk = [34])
internal class EditorTableIndexTest {
    private companion object {
        const val ROOT_KEY = "root"
        const val ATTRIBUTE_KEY = "plain"
        const val REVISION = 1uL
        const val ROOT_DOC_START = 10u
        const val ROOT_SCALAR_START = 7u
    }

    private fun cell(column: UInt, stride: UInt) = FfiTableCellRecord(
        0u, 0u, column, 1u, 1u, false, ATTRIBUTE_KEY, "cell-$column", 5u, stride,
        listOf(FfiViewerElement.BlockStart("paragraph", null, 0u, null),
            FfiViewerElement.TextRun("a", emptyList()), FfiViewerElement.BlockEnd),
        listOf(FfiCellInputBlock(0u, 2u, 3u, 0u, 0u, 1u, stride, false)), emptyList()
    )

    private fun frame(): FfiTableFrame {
        val table = FfiTableRecord(ROOT_KEY, null, 14u, 1u, 2u, listOf(null, null), null,
            false, false, ATTRIBUTE_KEY, listOf(FfiTableSourceRow(ATTRIBUTE_KEY, 2u)),
            listOf(cell(0u, 2u), cell(1u, 1u)), emptyList(), null, null)
        return FfiTableFrame(FfiTableFrameKind.FULL, null, listOf(FfiTableAttribute(ATTRIBUTE_KEY, "{}")),
            emptyList(), listOf(table), emptyList(), emptyList(),
            listOf(FfiTableExtent(ROOT_KEY, ROOT_DOC_START, table.docSize, ROOT_SCALAR_START, ROOT_SCALAR_START + 3u)))
    }

    private fun delta() = FfiTableFrame(FfiTableFrameKind.DELTA, REVISION.toString(), emptyList(),
        emptyList(), emptyList(), emptyList(), emptyList(), frame().extents)

    private fun adopt(index: EditorTableIndex, frame: FfiTableFrame, installed: ULong? = null,
                      revision: ULong = REVISION): TableFrameChanges {
        val result = index.adopt(frame, installed, revision)
        assertTrue("frame rejected: $result", result is TableFrameAdoption.Adopted)
        return (result as TableFrameAdoption.Adopted).changes
    }

    private fun withEngineFrame(source: String, check: (FfiTableFrame, JSONObject, ULong) -> Unit) {
        val created = UniffiEditorV2Backend.create(PlainTableFixture.CONFIG, null) as EditorV2CallResult.Ok
        val adapter = requireNotNull(EditorV2Adapter.attach(UniffiEditorV2Backend,
            JSONObject(created.value).getString("editorId"), false))
        try {
            requireNotNull(adapter.setContentJson(source))
            val native = editorV2RenderNativeFrame(adapter.editorId.toString(), null, null, null)
            assertNull(native.error)
            val legacy = editorV2RenderUpdate(adapter.editorId.toString(), null, null)
            assertNull(legacy.error)
            check(requireNotNull(native.frame).tables, JSONObject(requireNotNull(legacy.value)), adapter.baseDocumentRevision)
        } finally {
            adapter.destroy()
        }
    }

    @Test fun engineFramePositionsMatchIndependentLegacyMappings() = withEngineFrame(PlainTableFixture.document(2, 2)) { frame, legacy, revision ->
        val index = EditorTableIndex()
        adopt(index, frame, revision = revision)
        val table = frame.tables.single()
        val mappings = legacy.getJSONObject("tableInputMappings").getJSONObject("tables")
        val oldCells = mappings.getJSONObject(mappings.keys().next()).getJSONArray("cells")
        assertEquals(table.cells.size, oldCells.length())
        table.cells.indices.forEach { cellIndex ->
            val old = oldCells.getJSONObject(cellIndex)
            assertEquals(old.getLong("sourcePos").toUInt(), index.docStart(table.tableKey, cellIndex))
            val blocks = old.getJSONArray("blocks")
            assertEquals(blocks.getJSONObject(0).getLong("scalarStart").toUInt(), index.scalarStart(table.tableKey, cellIndex))
            val segments = requireNotNull(index.inputSegments(table.tableKey, cellIndex))
            assertEquals(blocks.length(), segments.size)
            segments.forEachIndexed { blockIndex, segment ->
                val block = blocks.getJSONObject(blockIndex)
                assertEquals(block.getInt("scalarStart"), segment.globalScalarStart)
                assertEquals(block.getInt("scalarEnd") - block.getInt("scalarStart") + 1,
                    segment.localScalarEndExclusive - segment.localScalarStart)
                assertEquals(cellIndex, index.cellIndexContainingScalar(table.tableKey, block.getLong("scalarEnd").toUInt()))
            }
        }
    }

    @Test fun nestedInputSegmentsCollapseMarkersBeforeFollowingProse() {
        val source = """{"type":"doc","content":[{"type":"table","content":[{"type":"table_row","content":[{"type":"table_cell","content":[{"type":"paragraph","content":[{"type":"text","text":"before"}]},{"type":"table","content":[{"type":"table_row","content":[{"type":"table_cell","content":[{"type":"paragraph","content":[{"type":"text","text":"nested text"}]}]}]}]},{"type":"paragraph","content":[{"type":"text","text":"after"}]}]}]}]}]}"""
        withEngineFrame(source) { frame, _, revision ->
            val index = EditorTableIndex()
            adopt(index, frame, revision = revision)
            val root = frame.tables.single { it.host == null }
            val segments = requireNotNull(index.inputSegments(root.tableKey, 0))
            assertEquals(listOf(0 until 7, 9 until 15), segments.map { it.localScalarStart until it.localScalarEndExclusive })
            assertEquals(listOf(0, 19), segments.map { it.globalScalarStart })
        }
    }

    @Test fun oneDeltaCanExchangeNestedTablesBetweenCellsWithoutMutatingThePriorIndex() {
        val full = frame()
        val children = full.tables.single().cells.indices.map { index ->
            frame().tables.single().copy(tableKey = "child-$index", host = FfiTableHost(ROOT_KEY, index.toUInt()))
        }
        val parent = full.tables.single().copy(docSize = 36u, cells = full.tables.single().cells.mapIndexed { index, cell ->
            cell.copy(docSize = children[index].docSize + 2u, scalarStride = if (index == 0) 4u else 3u,
                elements = listOf(FfiViewerElement.Table(children[index].tableKey)), inputBlocks = emptyList(),
                nestedTables = listOf(FfiCellNestedTable(0u, children[index].tableKey, 1u, children[index].docSize, 0u, 3u)))
        })
        full.tables = listOf(parent) + children
        full.extents = full.extents.map { it.copy(docSize = parent.docSize, scalarEnd = ROOT_SCALAR_START + 7u) }
        val original = EditorTableIndex()
        adopt(original, full)
        val updated = original.copy()
        val update = delta().copy(extents = full.extents,
            tables = children.mapIndexed { index, child -> child.copy(host = FfiTableHost(ROOT_KEY, (1 - index).toUInt())) },
            cellUpdates = parent.cells.mapIndexed { index, cell -> FfiTableCellUpdate(ROOT_KEY, index.toUInt(),
                cell.copy(elements = parent.cells[1 - index].elements, nestedTables = parent.cells[1 - index].nestedTables)) })
        adopt(updated, update, REVISION, REVISION + 1uL)
        assertEquals(ROOT_DOC_START + 5u, updated.docStart(children[1].tableKey, 0))
        assertEquals(ROOT_DOC_START + 21u, updated.docStart(children[0].tableKey, 0))
        assertEquals(ROOT_DOC_START + 5u, original.docStart(children[0].tableKey, 0))
        assertEquals(ROOT_DOC_START + 21u, original.docStart(children[1].tableKey, 0))
        assertEquals(parent, original.record(ROOT_KEY))
    }

    @Test fun fullAdoptionBuildsPositionsAndInputSegments() {
        val index = EditorTableIndex()
        val full = frame()
        val changes = adopt(index, full)
        assertTrue(changes.fullReset)
        assertEquals(setOf(ROOT_KEY), changes.replacedTables)
        assertEquals(full.tables.single(), index.record(ROOT_KEY))
        assertEquals(ROOT_DOC_START + 2u, index.docStart(ROOT_KEY, 0))
        assertEquals(ROOT_DOC_START + 7u, index.docStart(ROOT_KEY, 1))
        assertEquals(ROOT_SCALAR_START + 2u, index.scalarStart(ROOT_KEY, 1))
        assertEquals(1, index.cellIndexContainingDoc(ROOT_KEY, ROOT_DOC_START + 8u))
        assertEquals(1, index.cellIndexContainingScalar(ROOT_KEY, ROOT_SCALAR_START + 2u))
        assertEquals(1, index.cellIndexContainingScalar(ROOT_KEY, ROOT_SCALAR_START + 3u))
        assertNull(index.cellIndexContainingScalar(ROOT_KEY, ROOT_SCALAR_START + 4u))
        assertNull(index.cellIndexContainingDoc(ROOT_KEY, ROOT_DOC_START))
        assertEquals(ROOT_KEY, index.tableKeyContainingDoc(ROOT_DOC_START + 8u))
        assertEquals(ROOT_KEY, index.tableKeyContainingScalar(ROOT_SCALAR_START + 1u))
        assertEquals(ROOT_DOC_START + 9u, index.absoluteDocPos(ROOT_KEY, 1, 2u))
        assertEquals(listOf(TableCellPositionMap.Segment(0, 2, ROOT_SCALAR_START.toInt())), index.inputSegments(ROOT_KEY, 0))
        assertNull(index.docStart(ROOT_KEY, -1))
        assertNull(index.absoluteDocPos(ROOT_KEY, 0, 6u))
    }

    @Test fun oneCellDeltaRecomputesFollowingPrefixes() {
        val index = EditorTableIndex()
        val full = frame()
        adopt(index, full)
        val original = full.tables.single().cells.first()
        val growth = 3u
        val changed = original.copy(docSize = original.docSize + growth, scalarStride = original.scalarStride + growth,
            contentKey = "changed", elements = original.elements.toMutableList().apply {
                set(1, FfiViewerElement.TextRun("aaaa", emptyList()))
            }, inputBlocks = original.inputBlocks.map { it.copy(docEnd = it.docEnd + growth,
                scalarEnd = it.scalarEnd + growth, breakScalarEnd = it.breakScalarEnd + growth) })
        val update = delta().apply {
            cellUpdates = listOf(FfiTableCellUpdate(ROOT_KEY, 0u, changed))
            extents = extents.map { it.copy(docSize = it.docSize + growth, scalarEnd = it.scalarEnd + growth) }
        }
        val changes = adopt(index, update, REVISION, REVISION + 1uL)
        assertEquals(TableFrameChanges(false, emptySet(), emptySet(), mapOf(ROOT_KEY to setOf(0))), changes)
        assertEquals(ROOT_DOC_START + 7u + growth, index.docStart(ROOT_KEY, 1))
        assertEquals(ROOT_SCALAR_START + 2u + growth, index.scalarStart(ROOT_KEY, 1))
        assertEquals(full.tables.single().cells[1], index.record(ROOT_KEY)!!.cells[1])
        assertEquals(14u, full.tables.single().docSize)
        assertEquals(original, full.tables.single().cells[0])
    }

    @Test fun everyRejectionLeavesInstalledRecordsAndPositionsUnchanged() {
        val full = frame()
        val first = full.tables.single().cells.first()
        val cases = listOf(
            "base" to (delta().copy(baseDocumentRevision = "0") to TableFrameRejection.BaseRevisionMismatch(REVISION, 0uL)),
            "unknown table" to (delta().copy(removedTableKeys = listOf("missing")) to TableFrameRejection.UnknownTable("missing")),
            "cell index" to (delta().copy(cellUpdates = listOf(FfiTableCellUpdate(ROOT_KEY, 2u, first))) to TableFrameRejection.CellIndexOutOfRange(ROOT_KEY, 2)),
            "structure" to (delta().copy(cellUpdates = listOf(FfiTableCellUpdate(ROOT_KEY, 0u, first.copy(header = true)))) to TableFrameRejection.CellStructureChanged(ROOT_KEY, 0)),
            "doc size" to (delta().let { it.copy(extents = it.extents.map { e -> e.copy(docSize = e.docSize + 1u) }) } to TableFrameRejection.DocSizeMismatch(ROOT_KEY, 14u, 15u)),
            "scalar size" to (delta().let { it.copy(extents = it.extents.map { e -> e.copy(scalarEnd = e.scalarEnd + 1u) }) } to TableFrameRejection.ScalarSizeMismatch(ROOT_KEY, 3u, 4u)),
            "input stride" to (delta().copy(cellUpdates = listOf(FfiTableCellUpdate(ROOT_KEY, 0u,
                first.copy(inputBlocks = first.inputBlocks.map { it.copy(breakScalarEnd = first.scalarStride + 1u) })) )) to TableFrameRejection.InputBlockOutOfStride(ROOT_KEY, 0)),
            "attribute" to (delta().copy(removedAttributeKeys = listOf(ATTRIBUTE_KEY)) to TableFrameRejection.MissingAttribute(ATTRIBUTE_KEY)),
            "duplicate" to (delta().copy(tables = listOf(full.tables.single(), full.tables.single())) to TableFrameRejection.DuplicateTableKey(ROOT_KEY)),
            "host" to (delta().copy(tables = listOf(full.tables.single().copy(host = FfiTableHost("missing", 0u))), extents = emptyList()) to TableFrameRejection.HostMissing(ROOT_KEY)),
            "extents" to (delta().copy(extents = emptyList()) to TableFrameRejection.ExtentsIncomplete)
        )
        val index = EditorTableIndex()
        adopt(index, full)
        for ((label, candidate) in cases) {
            val (update, rejection) = candidate
            assertEquals(label, TableFrameAdoption.Rejected(rejection), index.adopt(update, REVISION, REVISION + 1uL))
            assertEquals("$label mutated records", full.tables.single(), index.record(ROOT_KEY))
            assertEquals("$label mutated doc prefix", ROOT_DOC_START + 7u, index.docStart(ROOT_KEY, 1))
            assertEquals("$label mutated scalar prefix", ROOT_SCALAR_START + 2u, index.scalarStart(ROOT_KEY, 1))
        }
    }

    @Test fun nestedPositionsUseHostCellAndRelativeExclusion() {
        val full = frame()
        val childKey = "child"
        val child = full.tables.single().copy(tableKey = childKey, host = FfiTableHost(ROOT_KEY, 0u), readOnlyDescendants = true)
        val parent = full.tables.single().let { table -> table.copy(docSize = 25u,
            cells = listOf(table.cells[0].copy(docSize = child.docSize + 2u, scalarStride = 4u,
                elements = listOf(FfiViewerElement.Table(childKey)), inputBlocks = emptyList(),
                nestedTables = listOf(FfiCellNestedTable(0u, childKey, 1u, child.docSize, 0u, 3u))), table.cells[1])) }
        full.tables = listOf(child, parent)
        full.extents = full.extents.map { it.copy(docSize = parent.docSize, scalarEnd = ROOT_SCALAR_START + 5u) }
        val index = EditorTableIndex()
        adopt(index, full)
        assertEquals(ROOT_DOC_START + 5u, index.docStart(childKey, 0))
        assertEquals(ROOT_SCALAR_START + 2u, index.scalarStart(childKey, 1))
        assertEquals(childKey, index.tableKeyContainingDoc(ROOT_DOC_START + 5u))
        assertEquals(childKey, index.tableKeyContainingScalar(ROOT_SCALAR_START + 2u))
        assertEquals(ROOT_DOC_START + 12u, index.absoluteDocPos(childKey, 1, 2u))
        val removal = delta().copy(extents = full.extents, removedTableKeys = listOf(childKey))
        assertEquals(TableFrameAdoption.Rejected(TableFrameRejection.UnknownTable(childKey)),
            index.adopt(removal, REVISION, REVISION + 1uL))
        assertEquals(child, index.record(childKey))
        assertEquals(parent, index.record(ROOT_KEY))
    }

    @Test fun sameRevisionEmptyDeltaPreservesExtentsAndFullResetRemovesTables() {
        val index = EditorTableIndex()
        adopt(index, frame())
        val changes = adopt(index, delta().copy(extents = emptyList()), REVISION)
        assertEquals(TableFrameChanges(false, emptySet(), emptySet(), emptyMap()), changes)
        assertEquals(ROOT_KEY, index.tableKeyContainingScalar(ROOT_SCALAR_START))
        assertEquals(ROOT_DOC_START + 7u, index.docStart(ROOT_KEY, 1))
        adopt(index, frame().copy(tables = emptyList(), extents = emptyList(), attributes = emptyList()), REVISION, REVISION + 1uL)
        assertNull(index.record(ROOT_KEY))
        assertNull(index.tableKeyContainingScalar(ROOT_SCALAR_START))
    }

    @Test fun failedTableAcceptsAnEmptyExtent() {
        val full = frame()
        full.tables = full.tables.map { it.copy(failure = TableRenderFailure.GRID_LIMIT, cells = emptyList(), sourceRows = emptyList()) }
        full.extents = full.extents.map { it.copy(scalarEnd = it.scalarStart) }
        val index = EditorTableIndex()
        adopt(index, full)
        assertEquals(full.tables.single(), index.record(ROOT_KEY))
        assertNull(index.docStart(ROOT_KEY, 0))
    }

    @Test fun emptySourceRowsContributeDocumentBoundaries() {
        val full = frame()
        full.tables = full.tables.map { it.copy(rows = 3u, docSize = it.docSize + 4u,
            sourceRows = listOf(FfiTableSourceRow(ATTRIBUTE_KEY, 0u), FfiTableSourceRow(ATTRIBUTE_KEY, 2u), FfiTableSourceRow(ATTRIBUTE_KEY, 0u)),
            cells = it.cells.map { cell -> cell.copy(sourceRow = 1u, row = 1u) }) }
        full.extents = full.extents.map { it.copy(docSize = it.docSize + 4u) }
        val index = EditorTableIndex()
        adopt(index, full)
        assertEquals(ROOT_DOC_START + 4u, index.docStart(ROOT_KEY, 0))
        assertEquals(ROOT_DOC_START + 9u, index.docStart(ROOT_KEY, 1))
        assertNull(index.cellIndexContainingDoc(ROOT_KEY, ROOT_DOC_START + 2u))
    }
}
