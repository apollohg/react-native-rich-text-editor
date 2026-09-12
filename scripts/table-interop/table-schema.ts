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

export const SINGLE_SPAN = 1;
const NODE_TOKENS = 2;
const CELL_INTERIOR_OFFSET = 2;
const ONE_TABLE = 1;
const TEXT_NODE = 'text';

export type CellOptions = {
    colspan?: number;
    rowspan?: number;
    colwidth?: number[] | null;
    header?: boolean;
    text?: string;
    blocks?: string[];
};

function paragraph(text: string | undefined): Record<string, unknown> {
    return text === undefined
        ? { type: PARAGRAPH_NODE }
        : { type: PARAGRAPH_NODE, content: [{ type: TEXT_NODE, text }] };
}

export function cell(options: CellOptions = {}): Record<string, unknown> {
    const blocks = options.blocks === undefined
        ? [paragraph(options.text)]
        : options.blocks.map((text) => paragraph(text));
    return {
        type: options.header === true ? HEADER_CELL_NODE : CELL_NODE,
        attrs: {
            colspan: options.colspan ?? SINGLE_SPAN,
            rowspan: options.rowspan ?? SINGLE_SPAN,
            colwidth: options.colwidth ?? null,
        },
        content: blocks,
    };
}

export function row(cells: Record<string, unknown>[]): Record<string, unknown> {
    return { type: ROW_NODE, content: cells };
}

export function table(rows: Record<string, unknown>[]): Record<string, unknown> {
    return { type: TABLE_NODE, content: rows };
}

const CELL_ATTRIBUTE_DEFAULTS: Record<string, unknown> = {
    colspan: SINGLE_SPAN,
    rowspan: SINGLE_SPAN,
    colwidth: null,
};

function withoutDefaultCellAttributes(attrs: Record<string, unknown>): Record<string, unknown> {
    const kept: Record<string, unknown> = {};
    for (const key of Object.keys(attrs).sort()) {
        const value = attrs[key];
        if (key in CELL_ATTRIBUTE_DEFAULTS && value === CELL_ATTRIBUTE_DEFAULTS[key]) {
            continue;
        }
        kept[key] = canonical(value);
    }
    return kept;
}

export function canonical(value: unknown): unknown {
    if (Array.isArray(value)) {
        return value.map(canonical);
    }
    if (value === null || typeof value !== 'object') {
        return value;
    }
    const record = value as Record<string, unknown>;
    const result: Record<string, unknown> = {};
    for (const key of Object.keys(record).sort()) {
        const entry = record[key];
        if (key === 'content' && Array.isArray(entry) && entry.length === 0) {
            continue;
        }
        if (key === 'attrs' && entry !== null && typeof entry === 'object') {
            const attrs = withoutDefaultCellAttributes(entry as Record<string, unknown>);
            if (Object.keys(attrs).length === 0) {
                continue;
            }
            result[key] = attrs;
            continue;
        }
        result[key] = canonical(entry);
    }
    return result;
}

function nodeSize(node: Record<string, unknown>): number {
    if (node['type'] === TEXT_NODE) {
        return String(node['text'] ?? '').length;
    }
    const content = node['content'];
    const inner = Array.isArray(content)
        ? content.reduce(
            (total: number, child) => total + nodeSize(child as Record<string, unknown>),
            0,
        )
        : 0;
    return inner + NODE_TOKENS;
}

export function cellAnchors(
    tableJson: Record<string, unknown>,
    tableStart: number,
): number[] {
    const anchors: number[] = [];
    let rowStart = tableStart + 1;
    for (const rowJson of (tableJson['content'] as Record<string, unknown>[] | undefined) ?? []) {
        let cellStart = rowStart + 1;
        for (const cellJson of (rowJson['content'] as Record<string, unknown>[] | undefined) ?? []) {
            anchors.push(cellStart + CELL_INTERIOR_OFFSET);
            cellStart += nodeSize(cellJson);
        }
        rowStart += nodeSize(rowJson);
    }
    return anchors;
}

export function tableOf(documentJson: Record<string, unknown> | null): unknown {
    const content = documentJson?.['content'];
    if (!Array.isArray(content)) {
        throw new Error('the peer document carried no content array');
    }
    const tables = content.filter(
        (node): node is Record<string, unknown> =>
            typeof node === 'object' && node !== null
            && (node as Record<string, unknown>)['type'] === TABLE_NODE,
    );
    if (tables.length !== ONE_TABLE) {
        throw new Error(`the fixture document holds ${tables.length} tables, not ${ONE_TABLE}`);
    }
    return canonical(tables[0]);
}
