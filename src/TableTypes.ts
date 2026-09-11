export type TableRole = 'table' | 'row' | 'cell' | 'header_cell';

export type TableNamingPreset = 'prosemirror' | 'tiptap';

export interface TableNodeNames {
    table: string;
    row: string;
    cell: string;
    headerCell: string;
}

export interface TablesSchemaOptions {
    preset?: TableNamingPreset;
    names?: TableNodeNames;
}
