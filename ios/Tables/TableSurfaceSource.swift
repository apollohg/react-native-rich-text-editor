import UIKit

struct TableSurfaceCell: Equatable {
    let sourceIndex: Int
    let row: Int
    let column: Int
    let rowspan: Int
    let colspan: Int
    let header: Bool
    let attrsKey: String
    let contentKey: String
    let elements: [FfiViewerElement]
}

struct TableSurfaceSource {
    let rows: Int
    let columns: Int
    let columnWidths: [CGFloat?]
    let direction: String?
    let irregular: Bool
    let readOnlyDescendants: Bool
    let attrsKey: String
    let cells: [TableSurfaceCell]
    let syntheticRegions: [TableRenderSyntheticRegion]
    let failure: TableRenderFailure?
    let compatibilityDiagnostic: TableCompatibilityDiagnostic?

    init(viewerTable: FfiViewerTable) {
        rows = Int(viewerTable.rows)
        columns = Int(viewerTable.columns)
        columnWidths = viewerTable.columnWidths.map { $0.map(CGFloat.init) }
        direction = viewerTable.direction
        irregular = viewerTable.irregular
        readOnlyDescendants = viewerTable.readOnlyDescendants
        attrsKey = viewerTable.attrsKey
        cells = viewerTable.cells.enumerated().map { index, cell in
            TableSurfaceCell(sourceIndex: index, row: Int(cell.row), column: Int(cell.column),
                             rowspan: Int(cell.rowspan), colspan: Int(cell.colspan), header: cell.header,
                             attrsKey: cell.attrsKey, contentKey: cell.contentKey, elements: cell.elements)
        }
        syntheticRegions = viewerTable.syntheticRegions
        failure = viewerTable.failure
        compatibilityDiagnostic = viewerTable.compatibilityDiagnostic
    }
    init(frameRecord: FfiTableRecord) {
        rows = Int(frameRecord.rows)
        columns = Int(frameRecord.columns)
        columnWidths = frameRecord.columnWidths.map { $0.map(CGFloat.init) }
        direction = frameRecord.direction
        irregular = frameRecord.irregular
        readOnlyDescendants = frameRecord.readOnlyDescendants
        attrsKey = frameRecord.attrsKey
        cells = frameRecord.cells.enumerated().map { index, cell in
            TableSurfaceCell(sourceIndex: index, row: Int(cell.row), column: Int(cell.column),
                             rowspan: Int(cell.rowspan), colspan: Int(cell.colspan), header: cell.header,
                             attrsKey: cell.attrsKey, contentKey: cell.contentKey, elements: cell.elements)
        }
        syntheticRegions = frameRecord.syntheticRegions
        failure = frameRecord.failure
        compatibilityDiagnostic = frameRecord.compatibilityDiagnostic
    }

}
