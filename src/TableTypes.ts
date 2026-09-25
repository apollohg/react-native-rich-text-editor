import type { RenderElement } from './NativeEditorTypes';

export type TableRole = 'table' | 'row' | 'cell' | 'header_cell';

export type TableDirection = 'ltr' | 'rtl';

export type TableRenderFailure =
    | 'gridLimit'
    | 'workLimit'
    | 'allocation'
    | 'invalidStructure'
    | 'invalidAttributes';
export type TableCompatibilityDiagnostic =
    | 'virtual-grid-limit'
    | 'empty-reference-surface'
    | 'unsupported-row-role'
    | 'unsupported-cell-role'
    | 'ambiguous-source-map'
    | 'unsupported-gap-default'
    | 'overlapping-reference-cells'
    | 'unmapped-reference-cell'
    | 'nonrectangular-reference-cell'
    | 'zero-span-after-reference-pass';

export interface TableRenderRegion {
    row: number;
    column: number;
    rowspan: number;
    colspan: number;
    header: boolean;
    attrsKey: string;
}

export interface TableRenderCell extends TableRenderRegion {
    sourcePos: number;
    sourceEnd: number;
    contentKey: string;
    elements: RenderElement[];
}

export interface TableRenderRow {
    sourcePos: number;
    sourceEnd: number;
    attrsKey: string;
}

export interface TableRenderRecord {
    tablePos: number;
    sourceId: string;
    sourceEnd: number;
    rows: number;
    columns: number;
    columnWidths: Array<number | null>;
    direction: TableDirection | null;
    irregular: boolean;
    readOnlyDescendants: boolean;
    attrsKey: string;
    sourceRows: TableRenderRow[];
    cells: TableRenderCell[];
    syntheticRegions: TableRenderRegion[];
    failure: TableRenderFailure | null;
    compatibilityDiagnostic: TableCompatibilityDiagnostic | null;
}

export interface TableInputExtent {
    scalarStart: number;
    scalarEnd: number;
}

export interface TableInputBlock extends TableInputExtent {
    elementIndex: number;
    docStart: number;
    docEnd: number;
    contentScalarStart: number;
    breakScalarEnd: number;
    void: boolean;
}

export interface TableInputCell {
    cellIndex: number;
    sourcePos: number;
    sourceEnd: number;
    blocks: TableInputBlock[];
    excluded: Array<{
        elementIndex: number;
        tableId: string;
        extent: TableInputExtent | null;
    }>;
}

/** Coordinates belong to the enclosing atomic snapshot, not cached cell content. */
export interface TableInputMappings {
    version: 1;
    tables: Record<
        string,
        {
            extent: TableInputExtent | null;
            cells: TableInputCell[];
        }
    >;
}

export type TableNamingPreset = 'prosemirror' | 'tiptap';

export interface TableNodeNames {
    table: string;
    row: string;
    cell: string;
    headerCell: string;
}

export interface TableCellSelection {
    type: 'cell';
    anchorCell: number;
    headCell: number;
}

export type TableEdge = 'before' | 'after';

export type TableHeaderTarget = 'row' | 'column' | 'cell';

export type TableCellStep = -1 | 1;

export type TableCommand =
    | { type: 'insertTable'; rows?: number; columns?: number; withHeaderRow?: boolean }
    | { type: 'deleteTable' }
    | { type: 'addTableRow'; side: TableEdge }
    | { type: 'deleteTableRows' }
    | { type: 'addTableColumn'; side: TableEdge }
    | { type: 'deleteTableColumns' }
    | { type: 'toggleTableHeader'; target: TableHeaderTarget }
    | { type: 'mergeTableCells' }
    | { type: 'splitTableCell' }
    | { type: 'resizeTableColumn'; tablePos: number; column: number; width: number }
    | { type: 'selectTableCells'; anchorCell: number; headCell: number }
    | { type: 'selectTableRows' }
    | { type: 'selectTableColumns' }
    | { type: 'clearTableCells' }
    | { type: 'goToTableCell'; direction: TableCellStep; appendRow: boolean };

export function assertCellPosition(value: number): void {
    if (!Number.isSafeInteger(value) || value < 0 || value > 0xffff_ffff) {
        throw new Error('Invalid table cell position');
    }
}

export interface TablesSchemaOptions {
    preset?: TableNamingPreset;
    names?: TableNodeNames;
}
