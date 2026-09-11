export type TableRole = 'table' | 'row' | 'cell' | 'header_cell';

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

export function assertCellPosition(value: number): void {
    if (!Number.isSafeInteger(value) || value < 0 || value > 0xffff_ffff) {
        throw new Error('Invalid table cell position');
    }
}

export interface TablesSchemaOptions {
    preset?: TableNamingPreset;
    names?: TableNodeNames;
}
