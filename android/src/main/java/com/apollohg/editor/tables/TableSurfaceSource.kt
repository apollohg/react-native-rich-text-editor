package com.apollohg.editor.tables

import uniffi.editor_core.FfiTableRecord
import uniffi.editor_core.FfiViewerElement
import uniffi.editor_core.FfiViewerTable
import uniffi.editor_core.TableCompatibilityDiagnostic
import uniffi.editor_core.TableRenderFailure
import uniffi.editor_core.TableRenderSyntheticRegion

data class TableSurfaceCell(
    val sourceIndex: Int, val row: Int, val column: Int, val rowspan: Int, val colspan: Int,
    val header: Boolean, val attrsKey: String, val contentKey: String, val elements: List<FfiViewerElement>
)

data class TableSurfaceSource(
    val rows: Int, val columns: Int, val columnWidths: List<Float?>, val direction: String?,
    val irregular: Boolean, val readOnlyDescendants: Boolean, val attrsKey: String,
    val cells: List<TableSurfaceCell>, val syntheticRegions: List<TableRenderSyntheticRegion>,
    val failure: TableRenderFailure?, val compatibilityDiagnostic: TableCompatibilityDiagnostic?
) {
    companion object {
        fun from(table: FfiTableRecord) = TableSurfaceSource(
            table.rows.toInt(), table.columns.toInt(), table.columnWidths.map { it?.toFloat() },
            table.direction, table.irregular, table.readOnlyDescendants, table.attrsKey,
            table.cells.mapIndexed { index, cell ->
                TableSurfaceCell(index, cell.row.toInt(), cell.column.toInt(), cell.rowspan.toInt(),
                    cell.colspan.toInt(), cell.header, cell.attrsKey, cell.contentKey, cell.elements)
            }, table.syntheticRegions, table.failure, table.compatibilityDiagnostic
        )

        fun from(table: FfiViewerTable) = TableSurfaceSource(
            table.rows.toInt(), table.columns.toInt(), table.columnWidths.map { it?.toFloat() },
            table.direction, table.irregular, table.readOnlyDescendants, table.attrsKey,
            table.cells.mapIndexed { index, cell ->
                TableSurfaceCell(index, cell.row.toInt(), cell.column.toInt(), cell.rowspan.toInt(),
                    cell.colspan.toInt(), cell.header, cell.attrsKey, cell.contentKey, cell.elements)
            }, table.syntheticRegions, table.failure, table.compatibilityDiagnostic
        )
    }
}
