import assert from 'node:assert/strict';
import test from 'node:test';
import { call, paragraphFixture, withPeers } from '../controller.js';
import type { Peer } from '../peer-protocol.js';

const TABLE_NODE = 'table';
const ROW_NODE = 'table_row';
const CELL_NODE = 'table_cell';
const HEADER_CELL_NODE = 'table_header';
const PARAGRAPH_NODE = 'paragraph';
const CELL_ATTRIBUTES = {
    colspan: { type: 'number', default: 1, min: 1 },
    rowspan: { type: 'number', default: 1, min: 1 },
    colwidth: { default: null },
};

const TABLE_SCHEMA = {
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

type CellOptions = {
    colspan?: number;
    rowspan?: number;
    colwidth?: number[] | null;
    header?: boolean;
};

function cell(options: CellOptions = {}): Record<string, unknown> {
    return {
        type: options.header === true ? HEADER_CELL_NODE : CELL_NODE,
        attrs: {
            colspan: options.colspan ?? 1,
            rowspan: options.rowspan ?? 1,
            colwidth: options.colwidth ?? null,
        },
        content: [{ type: PARAGRAPH_NODE }],
    };
}

function row(cells: Record<string, unknown>[]): Record<string, unknown> {
    return { type: ROW_NODE, content: cells };
}

function table(rows: Record<string, unknown>[]): Record<string, unknown> {
    return { type: TABLE_NODE, content: rows };
}

type Fixture = {
    name: string;
    table: Record<string, unknown>;
    pinnedWidths?: (number | null)[];
};

const FIXTURES: Fixture[] = [
    {
        name: 'a candidate counted once is replaced by the next row',
        table: table([
            row([cell({ colwidth: [100] })]),
            row([cell({ colwidth: [140] })]),
        ]),
        pinnedWidths: [140],
    },
    {
        name: 'a candidate counted twice outlives a later disagreement',
        table: table([
            row([cell({ colwidth: [100] })]),
            row([cell({ colwidth: [100] })]),
            row([cell({ colwidth: [140] })]),
        ]),
        pinnedWidths: [100],
    },
    {
        name: 'a rowspan contributes its width once per covered row',
        table: table([
            row([cell({ rowspan: 2, colwidth: [100] }), cell({ colwidth: [140] })]),
            row([cell({ colwidth: [180] })]),
        ]),
    },
    {
        name: 'a merged header spans two columns',
        table: table([
            row([cell({ header: true, colspan: 2, colwidth: [120, 120] })]),
            row([cell({ colwidth: [120] }), cell({ colwidth: [90] })]),
        ]),
    },
    {
        name: 'a merged cell disagrees with the rows below it',
        table: table([
            row([cell({ colspan: 2, colwidth: [100, 160] })]),
            row([cell({ colwidth: [140] }), cell({ colwidth: [160] })]),
            row([cell({ colwidth: [140] }), cell({ colwidth: [160] })]),
        ]),
    },
    {
        name: 'unset widths leave every column unresolved',
        table: table([
            row([cell(), cell()]),
            row([cell(), cell()]),
        ]),
    },
    {
        name: 'a short row leaves a hole without disturbing the widths',
        table: table([
            row([cell({ colwidth: [100] }), cell({ colwidth: [140] }), cell({ colwidth: [180] })]),
            row([cell({ colwidth: [100] }), cell({ colwidth: [140] })]),
        ]),
    },
    {
        name: 'a zero width never becomes a candidate',
        table: table([
            row([cell({ colwidth: [0] })]),
            row([cell({ colwidth: [140] })]),
        ]),
    },
];

async function projectionOf(
    peer: Peer,
    payload: Record<string, unknown>,
): Promise<Record<string, unknown>> {
    const projection = await call(peer, 'projectTable', payload);
    return {
        rows: projection['rows'],
        columns: projection['columns'],
        widths: projection['widths'],
        irregular: projection['irregular'],
    };
}

test('the engine resolves column widths exactly like prosemirror-tables 1.8.5', async (context) => {
    await withPeers(
        ['rust', 'prosemirror'] as const,
        async ([engine, web]) => {
            for (const fixture of FIXTURES) {
                await context.test(fixture.name, async () => {
                    const projected = await projectionOf(engine, {
                        schema: TABLE_SCHEMA,
                        table: fixture.table,
                    });
                    const oracle = await projectionOf(web, { table: fixture.table });

                    assert.deepEqual(
                        projected,
                        oracle,
                        `the engine and prosemirror-tables disagree about ${fixture.name}`,
                    );
                    if (fixture.pinnedWidths !== undefined) {
                        assert.deepEqual(
                            oracle['widths'],
                            fixture.pinnedWidths,
                            'the pinned reference no longer matches the fixture TBL-12 froze',
                        );
                    }
                });
            }
        },
        paragraphFixture('prosemirror'),
    );
});
