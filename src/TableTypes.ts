export type TableRole = 'table' | 'row' | 'cell' | 'header_cell';

export type TableDirection = 'ltr' | 'rtl';

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

export interface TableSelectionGeometry {
    editorId: string;
    ownerId: number;
    documentRevision: string;
    layoutEpoch: string;
    tablePos: number;
    coordinateSpace: 'window';
    rects: Array<{ x: number; y: number; width: number; height: number }>;
    viewport: { x: number; y: number; width: number; height: number };
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
