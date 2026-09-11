export const TABLE_NODE = 'table';
export const ROW_NODE = 'table_row';
export const CELL_NODE = 'table_cell';
export const HEADER_CELL_NODE = 'table_header';
export const PARAGRAPH_NODE = 'paragraph';
export const NO_COLLISIONS = 0;
export const CELL_ATTRIBUTES = {
    colspan: { type: 'number', default: 1, min: 1 },
    rowspan: { type: 'number', default: 1, min: 1 },
    colwidth: { default: null },
};

export const TABLE_SCHEMA = {
    nodes: [
        { name: 'doc', content: 'block+', role: 'doc' },
        { name: PARAGRAPH_NODE, content: 'inline*', group: 'block', role: 'textBlock' },
        { name: 'text', content: '', group: 'inline', role: 'text' },
        {
            name: TABLE_NODE,
            content: `${ROW_NODE}+`,
            group: 'block',
            role: 'block',
            tableRole: 'table',
        },
        {
            name: ROW_NODE,
            content: `(${CELL_NODE} | ${HEADER_CELL_NODE})*`,
            role: 'block',
            tableRole: 'row',
        },
        {
            name: CELL_NODE,
            content: 'block+',
            role: 'block',
            tableRole: 'cell',
            attrs: CELL_ATTRIBUTES,
        },
        {
            name: HEADER_CELL_NODE,
            content: 'block+',
            role: 'block',
            tableRole: 'header_cell',
            attrs: CELL_ATTRIBUTES,
        },
    ],
    marks: [],
};

export function geometryOf(projection: Record<string, unknown>): Record<string, unknown> {
    return {
        rows: projection['rows'],
        columns: projection['columns'],
        widths: projection['widths'],
        irregular: projection['irregular'],
        slots: projection['slots'],
    };
}
