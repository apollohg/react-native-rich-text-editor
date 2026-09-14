import assert from 'node:assert/strict';
import test from 'node:test';
import * as Y from 'yjs';
import {
    initProseMirrorDoc,
    prosemirrorJSONToYDoc,
    yXmlFragmentToProsemirrorJSON,
} from 'y-prosemirror';
import { Schema } from 'prosemirror-model';
import { schema as basic } from 'prosemirror-schema-basic';
import { tableNodes } from 'prosemirror-tables';
import {
    call,
    exchangeUntilIdle,
    flushDocumentEvents,
    seedFrom,
    snapshot,
    tableFixture,
    withPeers,
} from '../controller.js';
import { assertConverged } from '../assertions.js';
import { cell as rawCell, row, table } from '../table-schema.js';
import * as semantics from '../presentation-semantics.js';
import type {
    EffectiveCell,
    EffectiveDocument,
    EffectiveTable,
} from '../presentation-semantics.js';
import { observePresentation } from '../browser/presentation-observer.js';
import { overlapWitness, validateFallback } from '../presentation-overlap.js';

const paragraph = {
    type: 'paragraph',
    content: [{ type: 'text', text: 'same' }],
};
test('overlap witness near the end of a tall table stays within observation budget', () => {
    const boxes = Array.from({ length: 3000 }, (_, i) => ({
        source: String(i),
        position: i,
        tableSource: '0',
        left: 0,
        right: 20,
        top: i * 20,
        bottom: i * 20 + 10,
    }));
    boxes.push({
        ...boxes.at(-1)!,
        source: 'last',
        position: 3000,
        top: 59985,
        bottom: 60000,
    });
    assert.deepEqual(
        overlapWitness(boxes)
            ?.map((b) => b.source)
            .sort(),
        ['2999', 'last'],
    );
});
const cell = (options: Parameters<typeof rawCell>[0] = {}): semantics.JsonNode =>
    rawCell(options) as semantics.JsonNode;
function real(
    source: string,
    row: number,
    column: number,
    options: Partial<EffectiveCell> = {},
): EffectiveCell {
    return {
        source,
        position: 0,
        row,
        column,
        rowspan: 1,
        colspan: 1,
        node: { type: 'table_cell', content: [paragraph] },
        ...options,
    };
}
function fixture(cells: EffectiveCell[], rows = 2, columns = 2): EffectiveDocument {
    return {
        tables: [
            {
                source: '0',
                parentCell: null,
                pathWithinCell: '0',
                position: 0,
                node: { type: 'table' },
                rows,
                columns,
                widths: Array(columns).fill(null),
                cells,
            },
        ],
    };
}
function overlapPair(): [EffectiveDocument, EffectiveDocument] {
    const native = fixture(
        [
            real('0.0.0', 0, 0, { node: cell({ text: 'a' }) }),
            real('0.0.1', 0, 1, {
                rowspan: 2,
                node: cell({ text: 'b', rowspan: 2 }),
            }),
            real('0.1.0', 1, 2, {
                rowspan: 2,
                colspan: 2,
                node: cell({ text: 'c', colspan: 2, rowspan: 3 }),
            }),
        ],
        3,
        4,
    );
    const n = native.tables[0]!;
    n.cells.forEach((c, i) => {
        c.position = 2 + i * 5;
    });
    n.node = table([
        row(n.cells.slice(0, 2).map((c) => c.node)),
        row([n.cells[2]!.node]),
        row([]),
    ]) as semantics.JsonNode;
    n.overlap = {
        kind: 'native-fallback',
        reason: 'overlapping-reference-cells',
    };
    const web = structuredClone(native);
    const w = web.tables[0]!;
    w.overlap = {
        kind: 'web-overlap',
        logicalGeometry: 'unavailable',
        boxes: [
            {
                source: '0.0.1',
                tableSource: '0',
                position: 7,
                left: 10,
                top: 0,
                right: 20,
                bottom: 20,
            },
            {
                source: '0.1.0',
                tableSource: '0',
                position: 12,
                left: 0,
                top: 10,
                right: 20,
                bottom: 30,
            },
        ],
    };
    w.widths = null;
    for (const c of w.cells) c.row = c.column = c.rowspan = c.colspan = null;
    return [native, web];
}
test('overlap checker accepts independently declared evidence', () => {
    assert.deepEqual(semantics.assertEffectivePresentation(...overlapPair()).tables, [
        { source: '0', kind: 'overlap-fallback', evidence: 'live-overlap' },
    ]);
});
const overlapControls: [string, (n: EffectiveTable, w: EffectiveTable) => void, RegExp][] = [
    ...(['row', 'column', 'rowspan', 'colspan'] as const).map(
        (field): [string, (n: EffectiveTable, w: EffectiveTable) => void, RegExp] => [
            `populated unavailable ${field}`,
            (_, w) => {
                w.cells[0]![field] = 1;
            },
            /unavailable logical geometry must be null/,
        ],
    ),
    [
        'populated unavailable widths',
        (_, w) => {
            w.widths = [];
        },
        /unavailable logical geometry must be null/,
    ],
    [
        'absent evidence',
        (_, w) => {
            delete w.overlap;
        },
        /overlap evidence/,
    ],
    [
        'duplicate witness',
        (_, w) => {
            if (w.overlap?.kind === 'web-overlap') w.overlap.boxes[1] = { ...w.overlap.boxes[0]! };
        },
        /distinct/,
    ],
    [
        'wrong owner',
        (_, w) => {
            if (w.overlap?.kind === 'web-overlap') w.overlap.boxes[0]!.tableSource = 'other';
        },
        /attribution/,
    ],
    [
        'unknown source',
        (_, w) => {
            if (w.overlap?.kind === 'web-overlap') w.overlap.boxes[0]!.source = 'missing';
        },
        /attribution/,
    ],
    [
        'nonintersection',
        (_, w) => {
            if (w.overlap?.kind === 'web-overlap') w.overlap.boxes[1]!.left = 20;
        },
        /positive|intersection/,
    ],
    [
        'nonfinite box',
        (_, w) => {
            if (w.overlap?.kind === 'web-overlap') w.overlap.boxes[0]!.left = NaN;
        },
        /positive/,
    ],
    [
        'native overlap',
        (n) => {
            n.cells[2]!.column = 1;
        },
        /overlapping content/,
    ],
    [
        'wrong fallback',
        (n) => {
            n.cells[2]!.column = 0;
            n.cells[2]!.colspan = 1;
        },
        /fallback geometry/,
    ],
    [
        'other diagnostic',
        (n) => {
            n.overlap = {
                kind: 'native-fallback',
                reason: 'unsupported-default',
            } as any;
        },
        /diagnostic/,
    ],
    [
        'lost cell',
        (_, w) => {
            w.cells.pop();
        },
        /cell count|attribution/,
    ],
    [
        'duplicate cell',
        (_, w) => {
            w.cells.push(w.cells[0]!);
        },
        /duplicated cell/,
    ],
    [
        'changed content',
        (_, w) => {
            w.cells[0]!.node.content = [{ type: 'paragraph' }];
        },
        /content\/header\/attributes/,
    ],
    [
        'changed header',
        (_, w) => {
            w.cells[0]!.node.type = 'table_header';
        },
        /content\/header\/attributes/,
    ],
    [
        'changed opaque attribute',
        (_, w) => {
            w.cells[0]!.node.attrs!.payload = {
                type: 'table',
                content: ['secret'],
            };
        },
        /content\/header\/attributes/,
    ],
    [
        'native lost content',
        (n) => {
            n.cells[0]!.node = cell({ text: 'bad' });
        },
        /source content|content\/header\/attributes/,
    ],
    [
        'native wrong widths',
        (n) => {
            n.widths![0] = 100;
        },
        /fallback geometry\/widths/,
    ],
    [
        'ordinary falsely labelled',
        (n, w) => {
            const regular = fixture([real('0.0.0', 0, 0), real('0.0.1', 0, 1)], 1, 2).tables[0]!;
            regular.node = table([row(regular.cells.map((c) => c.node))]) as semantics.JsonNode;
            Object.assign(n, regular);
            w.cells = structuredClone(n.cells);
            w.cells.forEach((c, i) => {
                c.position = i;
            });
            w.overlap = {
                kind: 'web-overlap',
                logicalGeometry: 'unavailable',
                boxes: [
                    {
                        source: '0.0.0',
                        tableSource: '0',
                        position: 0,
                        left: 0,
                        top: 0,
                        right: 20,
                        bottom: 20,
                    },
                    {
                        source: '0.0.1',
                        tableSource: '0',
                        position: 1,
                        left: 10,
                        top: 0,
                        right: 30,
                        bottom: 20,
                    },
                ],
            };
            w.node = structuredClone(n.node);
            n.overlap = {
                kind: 'native-fallback',
                reason: 'overlapping-reference-cells',
            };
        },
        /regular table/,
    ],
    [
        'meaningful scaffold',
        (_, w) => {
            w.cells.push({ ...w.cells[0]!, source: null });
        },
        /meaningful synthetic/,
    ],
    [
        'table attributes',
        (_, w) => {
            w.node.attrs = { title: 'changed' };
        },
        /table attributes/,
    ],
    [
        'row attributes',
        (_, w) => {
            w.node.content![0]!.attrs = { title: 'changed' };
        },
        /row attributes/,
    ],
];
for (const [name, mutate, reason] of overlapControls)
    test(`overlap rejects ${name}`, () => {
        const [n, w] = overlapPair();
        mutate(n.tables[0]!, w.tables[0]!);
        assert.throws(() => semantics.assertEffectivePresentation(n, w), reason);
    });
test('overlap remains scoped to nested table alongside exact regular tables', () => {
    const [native, web] = overlapPair();
    const nestedSource = '0.0.0.1';
    for (const doc of [native, web]) {
        const nested = doc.tables[0]!;
        nested.source = nestedSource;
        nested.parentCell = '0.0.0';
        nested.pathWithinCell = '1';
        for (const c of nested.cells) c.source = nestedSource + c.source!.slice(1);
        if (nested.overlap?.kind === 'web-overlap')
            for (const box of nested.overlap.boxes) {
                box.source = nestedSource + box.source.slice(1);
                box.tableSource = nestedSource;
            }
        const parent = fixture([real('0.0.0', 0, 0)], 1, 1).tables[0]!;
        parent.cells[0]!.node.content!.push(nested.node);
        doc.tables.unshift(parent);
        const unrelated = structuredClone(parent);
        unrelated.source = '1';
        unrelated.pathWithinCell = '1';
        unrelated.cells[0]!.source = '1.0.0';
        unrelated.cells[0]!.node.content!.pop();
        doc.tables.push(unrelated);
    }
    assert.deepEqual(semantics.assertEffectivePresentation(native, web).tables, [
        { source: '0', kind: 'exact' },
        {
            source: nestedSource,
            kind: 'overlap-fallback',
            evidence: 'live-overlap',
        },
        { source: '1', kind: 'exact' },
    ]);
    web.tables[2]!.cells[0]!.column = 1;
    assert.throws(() => semantics.assertEffectivePresentation(native, web), /invalid rectangle/);
});
test('overlap native equality is explicitly distinct from live evidence', () => {
    const [native] = overlapPair();
    assert.deepEqual(
        semantics.assertEffectivePresentation(native, structuredClone(native)).tables,
        [
            {
                source: '0',
                kind: 'overlap-fallback',
                evidence: 'native-equality',
            },
        ],
    );
});
test('overlap fallback widths count repeated rowspan contributions independently', () => {
    const [doc] = overlapPair();
    const n = doc.tables[0]!;
    n.cells[0]!.node.attrs!.colwidth = [100];
    n.cells[1]!.node.attrs!.colwidth = [110];
    n.cells[2]!.node.attrs!.colwidth = [200, 220];
    const d = real('0.2.0', 2, 0, {
        node: cell({ text: 'd', colwidth: [140] }),
    });
    const e = real('0.2.1', 2, 1, {
        node: cell({ text: 'e', colwidth: [150] }),
    });
    n.cells.push(d, e);
    n.node.content![2]!.content = [d.node, e.node];
    n.widths = [140, 110, 200, 220];
    validateFallback(n);
    n.widths[1] = 150;
    assert.throws(() => validateFallback(n), /fallback geometry\/widths/);
});
const fixtures: [string, EffectiveDocument][] = [
    ['regular', fixture([real('a', 0, 0), real('b', 0, 1), real('c', 1, 0), real('d', 1, 1)])],
    ['missing-slot', fixture([real('a', 0, 0), real('b', 0, 1), real('c', 1, 0)])],
    [
        'collision',
        fixture(
            [real('a', 0, 0), real('b', 0, 1, { rowspan: 2 }), real('c', 1, 2, { colspan: 2 })],
            2,
            4,
        ),
    ],
    [
        'overlong-rowspan',
        fixture([
            real('a', 0, 0, { rowspan: 2, node: cell({ text: 'same', rowspan: 9 }) }),
            real('b', 0, 1),
            real('c', 1, 1),
        ]),
    ],
    [
        'inconsistent-width',
        fixture(
            [
                real('a', 0, 0, { node: cell({ text: 'same', colwidth: [100] }) }),
                real('b', 1, 0, { node: cell({ text: 'same', colwidth: [140] }) }),
            ],
            2,
            1,
        ),
    ],
];
fixtures[4]![1].tables[0]!.widths = [140];
for (const [name, expected] of fixtures) {
    test(`TBL-21-P independently declared ${name} semantics`, () => {
        const actual = structuredClone(expected);
        for (const entry of actual.tables[0]!.cells) {
            entry.position += 99;
            entry.node.attrs = {
                colspan: entry.colspan,
                rowspan: entry.rowspan,
                colwidth: name === 'inconsistent-width' ? [140] : null,
            };
        }
        semantics.assertEffectivePresentation(expected, actual);
    });
}

test('TBL-21-P permits empty display-only gap grouping and declared defaults', () => {
    const expected = fixtures[1]![1];
    const actual = structuredClone(expected);
    actual.tables[0]!.cells.push(
        real('gap', 1, 1, {
            source: null,
            node: {
                type: 'table_cell',
                attrs: { colspan: 1, rowspan: 1, colwidth: null },
                content: [{ type: 'paragraph', content: [] }],
            },
        }),
    );
    semantics.assertEffectivePresentation(expected, actual);
});

const headerGap = fixture([
    real('a', 0, 1),
    real('gap', 0, 0, {
        source: null,
        node: { type: 'table_header', attrs: { background: 'ivory' }, content: [{ type: 'paragraph' }] },
    }),
], 1, 2);
test('TBL-21-P compares independently represented meaningful synthetic semantics', () => {
    semantics.assertEffectivePresentation(headerGap, structuredClone(headerGap));
});
for (const [name, mutate] of [
    ['header', (node: semantics.JsonNode) => { node.type = 'table_cell'; }],
    ['default', (node: semantics.JsonNode) => { node.attrs!.background = 'red'; }],
    ['payload', (node: semantics.JsonNode) => { node.content![0]!.content = [{ type: 'text', text: 'unexpected' }]; }],
] as const) {
    test(`TBL-21-P rejects changed synthetic ${name}`, () => {
        const actual = structuredClone(headerGap);
        mutate(actual.tables[0]!.cells[1]!.node);
        assert.throws(() => semantics.assertEffectivePresentation(headerGap, actual));
    });
}

const rich = fixture(
    [
        real('a', 0, 0, {
            node: {
                type: 'table_header',
                attrs: { opaque: { type: 'table_cell', attrs: { colspan: 1 }, content: [] } },
                content: [
                    {
                        type: 'paragraph',
                        content: [
                            { type: 'text', text: 'same', marks: [{ type: 'strong' }] },
                            {
                                type: 'image',
                                attrs: {
                                    src: 'atom.png',
                                    payload: { type: 'table_cell', attrs: { rowspan: 1 } },
                                },
                            },
                        ],
                    },
                ],
            },
        }),
    ],
    1,
    1,
);
const mutations: [string, (table: EffectiveTable) => void, string][] = [
    [
        'dropped real cell',
        (t) => {
            t.cells.pop();
        },
        'UNACCOUNTED_CONTENT',
    ],
    [
        'duplicated real cell',
        (t) => {
            t.cells.push(structuredClone(t.cells[0]!));
        },
        'UNACCOUNTED_CONTENT',
    ],
    [
        'wrong slot',
        (t) => {
            t.cells[0]!.column = 1;
        },
        'PRESENTATION_MISMATCH',
    ],
    [
        'header role',
        (t) => {
            t.cells[0]!.node.type = 'table_cell';
        },
        'PRESENTATION_MISMATCH',
    ],
    [
        'mark',
        (t) => {
            t.cells[0]!.node.content![0]!.content![0]!.marks![0]!.type = 'em';
        },
        'PRESENTATION_MISMATCH',
    ],
    [
        'atom payload',
        (t) => {
            t.cells[0]!.node.content![0]!.content![1]!.attrs!.src = 'corrupt.png';
        },
        'PRESENTATION_MISMATCH',
    ],
    [
        'opaque attribute',
        (t) => {
            t.cells[0]!.node.attrs!.opaque = { type: 'table_cell', attrs: {}, content: [] };
        },
        'PRESENTATION_MISMATCH',
    ],
    [
        'resolved width',
        (t) => {
            t.widths![0] = 180;
        },
        'PRESENTATION_MISMATCH',
    ],
    [
        'unexplained span attribute',
        (t) => {
            t.cells[0]!.node.attrs!.rowspan = 7;
        },
        'PRESENTATION_MISMATCH',
    ],
    [
        'ambiguous mapping',
        (t) => {
            t.cells[0]!.source = null;
        },
        'UNACCOUNTED_CONTENT',
    ],
];
for (const [name, mutate, code] of mutations) {
    test(`TBL-21-P rejects ${name}`, () => {
        const actual = structuredClone(rich);
        mutate(actual.tables[0]!);
        assert.throws(() => semantics.assertEffectivePresentation(rich, actual), { code });
    });
}
test('TBL-21-P retains opaque plain data and rich content, merging only equivalent text runs', () => {
    const actual = structuredClone(rich);
    actual.tables[0]!.cells[0]!.node.content![0]!.content!.splice(
        0,
        1,
        { type: 'text', text: 'sa', marks: [{ type: 'strong' }] },
        { type: 'text', text: 'me', marks: [{ type: 'strong' }] },
    );
    semantics.assertEffectivePresentation(rich, actual);
});
test('TBL-21-P meaningful attributes on display-only gaps are not empty', () => {
    const expected = fixtures[1]![1];
    const actual = structuredClone(expected);
    actual.tables[0]!.cells.push(
        real('gap', 1, 1, {
            source: null,
            node: {
                type: 'table_cell',
                attrs: { background: 'red' },
                content: [{ type: 'paragraph' }],
            },
        }),
    );
    assert.throws(() => semantics.assertEffectivePresentation(expected, actual), {
        code: 'UNACCOUNTED_CONTENT',
    });
});
test('TBL-21-P display-only gap widths must follow the resolved columns', () => {
    const expected = fixtures[1]![1];
    const actual = structuredClone(expected);
    actual.tables[0]!.cells.push(
        real('gap', 1, 1, {
            source: null,
            node: {
                type: 'table_cell',
                attrs: { colwidth: [99] },
                content: [{ type: 'paragraph' }],
            },
        }),
    );
    assert.throws(() => semantics.assertEffectivePresentation(expected, actual), {
        code: 'UNACCOUNTED_CONTENT',
    });
});
test('TBL-21-P an unspecified display gap width inherits its resolved column', () => {
    const expected = structuredClone(fixtures[1]![1]);
    expected.tables[0]!.widths = [100, 140];
    const actual = structuredClone(expected);
    actual.tables[0]!.cells.push(real('gap', 1, 1, { source: null, node: cell() }));
    semantics.assertEffectivePresentation(expected, actual);
});
test('TBL-21-P nested geometry delegates to the nested effective table', () => {
    const expected = fixture([real('outer', 0, 0, { node: cell() })], 1, 1);
    expected.tables[0]!.cells[0]!.node.content!.push(
        table([row([cell()]), row([cell(), cell()])]) as semantics.JsonNode,
    );
    expected.tables.push({
        ...structuredClone(fixtures[1]![1].tables[0]!),
        source: 'nested',
        parentCell: 'outer',
        pathWithinCell: '1',
    });
    const actual = structuredClone(expected);
    actual.tables[0]!.cells[0]!.node.content![1] = table([
        row([cell(), cell()]),
        row([cell(), cell()]),
    ]) as semantics.JsonNode;
    semantics.assertEffectivePresentation(expected, actual);
    actual.tables[1]!.cells[0]!.colspan = 2;
    assert.throws(() => semantics.assertEffectivePresentation(expected, actual), {
        code: 'PRESENTATION_MISMATCH',
    });
});
test('TBL-21-P rejects unexplained row attributes', () => {
    const expected = structuredClone(fixtures[0]![1]);
    expected.tables[0]!.node = table([
        row([cell(), cell()]),
        row([cell(), cell()]),
    ]) as semantics.JsonNode;
    const actual = structuredClone(expected);
    actual.tables[0]!.node.content![0]!.attrs = { background: 'red' };
    assert.throws(() => semantics.assertEffectivePresentation(expected, actual), {
        code: 'PRESENTATION_MISMATCH',
    });
});

test('TBL-21-P mapping rejects absent witnesses instead of matching duplicate text or positions', () => {
    const authored = prosemirrorJSONToYDoc(schema, {
        type: 'doc',
        content: [table([row([cell(), cell()])])],
    });
    const fragment = authored.getXmlFragment('prosemirror');
    const { doc } = initProseMirrorDoc(fragment, schema);
    assert.throws(
        () =>
            observePresentation(
                fragment,
                new Map(),
                doc,
                yXmlFragmentToProsemirrorJSON(fragment) as semantics.JsonNode,
            ),
        { code: 'AMBIGUOUS_SOURCE_MAPPING' },
    );
    authored.destroy();
});
test('TBL-21-P serializes the cell identity, independent of its first content item', () => {
    const authored = prosemirrorJSONToYDoc(schema, {
        type: 'doc',
        content: [table([row([cell()])])],
    });
    const fragment = authored.getXmlFragment('prosemirror');
    const { doc, mapping } = initProseMirrorDoc(fragment, schema);
    const sharedCell = (fragment.get(0) as Y.XmlElement).get(0) as Y.XmlElement;
    const identity = Y.relativePositionToJSON(
        Y.createRelativePositionFromTypeIndex(sharedCell.get(0) as Y.XmlElement, 0),
    );
    const observed = observePresentation(
        fragment,
        mapping,
        doc,
        yXmlFragmentToProsemirrorJSON(fragment) as semantics.JsonNode,
    );
    assert.equal(observed.tables[0]!.cells[0]!.sourceId, JSON.stringify({ type: identity.type }));
    authored.destroy();
});
test('TBL-21-P Fragment.empty cannot establish source ownership', () => {
    const authored = prosemirrorJSONToYDoc(schema, {
        type: 'doc',
        content: [table([row([{ type: 'table_cell' }])])],
    });
    const fragment = authored.getXmlFragment('prosemirror');
    const doc = schema.nodeFromJSON(yXmlFragmentToProsemirrorJSON(fragment));
    const mapping: Parameters<typeof observePresentation>[1] = new Map();
    const source = ((fragment.get(0) as Y.XmlElement).get(0) as Y.XmlElement).get(
        0,
    ) as Y.XmlElement;
    mapping.set(source as unknown as Y.AbstractType<unknown>, schema.nodes.table_cell!.create());
    assert.throws(
        () =>
            observePresentation(
                fragment,
                mapping,
                doc,
                yXmlFragmentToProsemirrorJSON(fragment) as semantics.JsonNode,
            ),
        { code: 'AMBIGUOUS_SOURCE_MAPPING' },
    );
    authored.destroy();
});

const schema = new Schema({
    nodes: basic.spec.nodes.append(
        tableNodes({ tableGroup: 'block', cellContent: 'block+', cellAttributes: {} }),
    ),
    marks: basic.spec.marks,
});
const tableSpecs = tableNodes({ tableGroup: 'block', cellContent: 'block+', cellAttributes: {} });
const tiptapSchema = new Schema({
    nodes: basic.spec.nodes.append({
        table: { ...tableSpecs.table, content: 'tableRow+' },
        tableRow: { ...tableSpecs.table_row, content: '(tableCell | tableHeader)*' },
        tableCell: tableSpecs.table_cell,
        tableHeader: tableSpecs.table_header,
    }),
    marks: basic.spec.marks,
});
function presetNode(value: unknown, preset: string): unknown {
    if (Array.isArray(value)) return value.map((v) => presetNode(v, preset));
    if (value === null || typeof value !== 'object') return value;
    const node = value as Record<string, unknown>;
    return {
        ...node,
        ...(typeof node.type === 'string'
            ? {
                  type:
                      preset === 'tiptap'
                          ? ((
                                {
                                    table_row: 'tableRow',
                                    table_cell: 'tableCell',
                                    table_header: 'tableHeader',
                                } as Record<string, string>
                            )[node.type] ?? node.type)
                          : node.type,
              }
            : {}),
        ...(Array.isArray(node.content)
            ? { content: node.content.map((v) => presetNode(v, preset)) }
            : {}),
    };
}
for (const preset of ['prosemirror', 'tiptap'] as const) {
    test(`TBL-21-P ${preset} measures nested overlap without waiving surrounding tables`, async () => {
        const inner = table([
            row([cell({ text: 'a' }), cell({ text: 'b', rowspan: 2 })]),
            row([cell({ text: 'c', colspan: 2, rowspan: 3 })]),
            row([]),
        ]);
        const outerCell = cell({ text: 'parent' });
        outerCell.content!.push(inner as semantics.JsonNode);
        const authored = prosemirrorJSONToYDoc(
            preset === 'tiptap' ? tiptapSchema : schema,
            presetNode(
                {
                    type: 'doc',
                    content: [
                        table([row([outerCell])]),
                        table([row([cell({ text: 'unrelated' })])]),
                    ],
                },
                preset,
            ),
            'prosemirror',
        );
        try {
            const updateBase64 = Buffer.from(Y.encodeStateAsUpdate(authored)).toString('base64');
            await withPeers(
                [preset, 'rust'],
                async ([web, native]) => {
                    await call(native, 'applyUpdate', { updateBase64 });
                    await call(web, 'applyUpdate', { updateBase64 });
                    await exchangeUntilIdle([native, web]);
                    await assertConverged([native, web]);
                    const observed = await semantics.observeWebPresentation(web);
                    assert.deepEqual(
                        semantics.assertEffectivePresentation(
                            await semantics.observeNativePresentation(native),
                            observed,
                        ).tables,
                        [
                            { source: '0', kind: 'exact' },
                            {
                                source: '0.0.0.1',
                                kind: 'overlap-fallback',
                                evidence: 'live-overlap',
                            },
                            { source: '1', kind: 'exact' },
                        ],
                    );
                    assert.equal(observed.tables[0]!.overlap, undefined);
                    const nested = observed.tables[1]!;
                    assert.ok(nested.overlap?.kind === 'web-overlap');
                    assert.ok(
                        nested.overlap.boxes.every((box) => box.tableSource === nested.source),
                    );
                },
                tableFixture(preset),
            );
        } finally {
            authored.destroy();
        }
    });
    test(`TBL-21-P ${preset} verifies overlap fallback`, async () => {
        const selectedSchema = preset === 'tiptap' ? tiptapSchema : schema;
        const authored = prosemirrorJSONToYDoc(
            selectedSchema,
            presetNode(
                {
                    type: 'doc',
                    content: [
                        table([
                            row([cell({ text: 'a' }), cell({ text: 'b', rowspan: 2 })]),
                            row([cell({ text: 'c', colspan: 2, rowspan: 3 })]),
                            row([]),
                        ]),
                    ],
                },
                preset,
            ),
            'prosemirror',
        );
        try {
            const updateBase64 = Buffer.from(Y.encodeStateAsUpdate(authored)).toString('base64');
            await withPeers(
                [preset, 'rust'],
                async ([web, native]) => {
                    await call(native, 'applyUpdate', { updateBase64 });
                    await call(web, 'applyUpdate', { updateBase64 });
                    await exchangeUntilIdle([native, web]);
                    await assertConverged([native, web]);
                    const beforeObservation = [await snapshot(native), await snapshot(web)];
                    const nativeView = await semantics.observeNativePresentation(native);
                    const webView = await semantics.observeWebPresentation(web);
                    const checked = semantics.assertEffectivePresentation(nativeView, webView);
                    assert.deepEqual(checked.tables, [
                        {
                            source: '0',
                            kind: 'overlap-fallback',
                            evidence: 'live-overlap',
                        },
                    ]);
                    assert.deepEqual(
                        [await snapshot(native), await snapshot(web)],
                        beforeObservation,
                    );
                    assert.equal((await flushDocumentEvents(native)).length, 0);
                    assert.equal((await flushDocumentEvents(web)).length, 0);
                    assert.equal(webView.tables[0]!.widths, null);
                    assert.ok(
                        webView.tables[0]!.cells.every(
                            (c) =>
                                c.row === null &&
                                c.column === null &&
                                c.rowspan === null &&
                                c.colspan === null,
                        ),
                    );
                    const target = nativeView.tables[0]!.cells.find((c) => c.source === '0.0.0')!;
                    const typed = await call(native, 'command', {
                        type: 'insertText',
                        text: 'typed-',
                        at: target.rawPosition! + 2,
                    });
                    assert.equal(typed.documentChanged, true);
                    assert.equal((await snapshot(native)).normalizationPassesAfterLastAction, 0);
                    await exchangeUntilIdle([native, web]);
                    await assertConverged([native, web]);
                    const freshNative = await semantics.observeNativePresentation(native);
                    const freshWeb = await semantics.observeWebPresentation(web);
                    assert.equal(
                        freshNative.tables[0]!.cells.find((c) => c.source === target.source)!.node
                            .content![0]!.content![0]!.text,
                        'typed-a',
                    );
                    assert.deepEqual(
                        semantics.assertEffectivePresentation(freshNative, freshWeb).tables,
                        [
                            {
                                source: '0',
                                kind: 'overlap-fallback',
                                evidence: 'live-overlap',
                            },
                        ],
                    );
                    const beforeStructure = await snapshot(native);
                    const structure = await call(native, 'command', {
                        type: 'addTableRow',
                        side: 'after',
                        at: target.rawPosition! + 2,
                    });
                    const afterStructure = await snapshot(native);
                    if (structure.documentChanged === true) {
                        assert.equal(
                            (afterStructure.documentJson as semantics.JsonNode).content![0]!
                                .content!.length,
                            (beforeStructure.documentJson as semantics.JsonNode).content![0]!
                                .content!.length + 1,
                        );
                    } else {
                        assert.equal(structure.type, 'notApplicable');
                        assert.deepEqual(afterStructure.documentJson, beforeStructure.documentJson);
                        assert.equal((await flushDocumentEvents(native)).length, 0);
                    }
                    await exchangeUntilIdle([native, web]);
                    await assertConverged([native, web]);
                    semantics.assertEffectivePresentation(
                        await semantics.observeNativePresentation(native),
                        await semantics.observeWebPresentation(web),
                    );
                },
                tableFixture(preset),
            );
        } finally {
            authored.destroy();
        }
    });
    for (const placement of ['following cell', 'nested table'] as const) {
        test(`TBL-21-P ${preset} Unicode scalar native offsets before ${placement}`, async () => {
            await withPeers(
                ['rust', preset],
                async ([native, web]) => {
                    const first = cell({ text: '😀' });
                    const grid =
                        placement === 'following cell'
                            ? table([row([first, cell({ text: 'next' })])])
                            : table([
                                  row([
                                      {
                                          ...first,
                                          content: [
                                              ...first.content!,
                                              table([
                                                  row([cell({ text: 'nested' })]),
                                              ]) as semantics.JsonNode,
                                          ],
                                      },
                                  ]),
                              ]);
                    const authored = prosemirrorJSONToYDoc(
                        preset === 'tiptap' ? tiptapSchema : schema,
                        presetNode({ type: 'doc', content: [grid] }, preset),
                        'prosemirror',
                    );
                    try {
                        const updateBase64 = Buffer.from(Y.encodeStateAsUpdate(authored)).toString(
                            'base64',
                        );
                        await call(native, 'applyUpdate', { updateBase64 });
                        await call(web, 'applyUpdate', { updateBase64 });
                        const expected = await semantics.observeNativePresentation(native);
                        const actual = await semantics.observeWebPresentation(web);
                        if (placement === 'following cell') {
                            assert.equal(expected.tables[0]!.cells[1]!.position, 7);
                            assert.equal(expected.tables[0]!.cells[1]!.rawPosition, 7);
                            assert.equal(actual.tables[0]!.cells[1]!.position, 8);
                        } else {
                            assert.equal(expected.tables[1]!.position, 6);
                            assert.equal(expected.tables[1]!.cells[0]!.position, 8);
                            assert.equal(expected.tables[1]!.cells[0]!.rawPosition, 8);
                            assert.equal(actual.tables[1]!.position, 7);
                            assert.equal(actual.tables[1]!.cells[0]!.position, 9);
                        }
                        semantics.assertEffectivePresentation(expected, actual);
                    } finally {
                        authored.destroy();
                    }
                },
                tableFixture(preset),
            );
        });
    }
    test(`TBL-21-P ${preset} concurrent-merges preserves stock placement after raw convergence`, async () => {
        await withPeers(
            [preset, preset, 'rust'],
            async ([horizontal, vertical, native]) => {
                const grid = table([
                    row([cell({ text: 'a' }), cell({ text: 'b' })]),
                    row([cell({ text: 'c' }), cell({ text: 'd' })]),
                ]);
                await call(horizontal, 'command', {
                    type: 'insertNode',
                    node: presetNode(grid, preset),
                });
                await seedFrom(horizontal, [vertical, native]);
                await exchangeUntilIdle([horizontal, vertical, native]);
                const surface = await semantics.observeWebPresentation(horizontal);
                const cells = surface.tables[0]!.cells;
                await call(horizontal, 'command', {
                    type: 'tableCommand',
                    name: 'mergeCells',
                    at: cells[0]!.position + 1,
                    head: cells[1]!.position + 1,
                });
                await call(vertical, 'command', {
                    type: 'tableCommand',
                    name: 'mergeCells',
                    at: cells[2]!.position + 1,
                    head: cells[0]!.position + 1,
                });
                await exchangeUntilIdle([horizontal, vertical, native]);
                await assertConverged([horizontal, vertical, native]);
                const expected = await semantics.observeNativePresentation(native);
                for (const web of [horizontal, vertical]) {
                    const actual = await semantics.observeWebPresentation(web);
                    semantics.assertEffectivePresentation(expected, actual);
                    assert.equal(expected.tables[0]!.cells[0]!.column, 1);
                    assert.equal(
                        actual.tables[0]!.cells.find(
                            (c) => c.source === expected.tables[0]!.cells[0]!.source,
                        )!.column,
                        1,
                    );
                }
            },
            tableFixture(preset),
        );
    });
    for (const name of [
        'regular',
        'missing-slot',
        'collision',
        'overlong-rowspan',
        'inconsistent-width',
        'nested',
        'header-gap',
    ] as const) {
        test(`TBL-21-P live ${preset} ${name} preserves attributable cells through remote repair`, async () => {
            await withPeers(
                ['rust', preset],
                async ([native, web]) => {
                    const regular = table([
                        row([cell({ text: 'same', header: name === 'header-gap' }), cell({ text: 'same', header: name === 'header-gap' })]),
                        row([cell({ text: 'same' }), cell({ text: 'same' })]),
                    ]);
                    const selectedSchema = preset === 'tiptap' ? tiptapSchema : schema;
                    const authored = prosemirrorJSONToYDoc(
                        selectedSchema,
                        presetNode({ type: 'doc', content: [regular] }, preset),
                        'prosemirror',
                    );
                    const initial = Buffer.from(Y.encodeStateAsUpdate(authored)).toString('base64');
                    await call(web, 'applyUpdate', { updateBase64: initial });
                    await call(native, 'applyUpdate', { updateBase64: initial });
                    const initialObservation = await semantics.observeWebPresentation(web);
                    const vector = Y.encodeStateVector(authored);
                    const rawTable = authored.getXmlFragment('prosemirror').get(0) as Y.XmlElement;
                    const firstRow = rawTable.get(0) as Y.XmlElement;
                    const secondRow = rawTable.get(1) as Y.XmlElement;
                    authored.transact(() => {
                        if (name === 'header-gap') firstRow.delete(1, 1);
                        if (
                            name === 'missing-slot' ||
                            name === 'collision' ||
                            name === 'overlong-rowspan'
                        )
                            secondRow.delete(1, 1);
                        if (name === 'collision') {
                            (firstRow.get(1) as Y.XmlElement).setAttribute('rowspan', 2 as never);
                            (secondRow.get(0) as Y.XmlElement).setAttribute('colspan', 2 as never);
                        }
                        if (name === 'overlong-rowspan')
                            (firstRow.get(0) as Y.XmlElement).setAttribute('rowspan', 9 as never);
                        if (name === 'inconsistent-width') {
                            (firstRow.get(0) as Y.XmlElement).setAttribute('colwidth', [
                                100,
                            ] as never);
                            (secondRow.get(0) as Y.XmlElement).setAttribute('colwidth', [
                                140,
                            ] as never);
                        }
                        if (name === 'nested') {
                            const nestedDoc = prosemirrorJSONToYDoc(
                                selectedSchema,
                                presetNode(
                                    {
                                        type: 'doc',
                                        content: [
                                            table([
                                                row([cell({ text: 'same' })]),
                                                row([
                                                    cell({ text: 'same' }),
                                                    cell({ text: 'same' }),
                                                ]),
                                            ]),
                                        ],
                                    },
                                    preset,
                                ),
                                'prosemirror',
                            );
                            const nested = (
                                nestedDoc.getXmlFragment('prosemirror').get(0) as Y.XmlElement
                            ).clone();
                            (firstRow.get(0) as Y.XmlElement).insert(1, [nested]);
                            nestedDoc.destroy();
                        }
                    });
                    const updateBase64 = Buffer.from(
                        Y.encodeStateAsUpdate(authored, vector),
                    ).toString('base64');
                    await call(native, 'applyUpdate', { updateBase64 });
                    await call(web, 'applyUpdate', { updateBase64 });
                    await flushDocumentEvents(web);
                    const before = await snapshot(web);
                    const observed = await semantics.observeWebPresentation(web);
                    const expected = await semantics.observeNativePresentation(native);
                    const nativeRectangles = expected.tables[0]!.cells.filter((c) => c.source !== null).map((c) => [
                        c.row,
                        c.column,
                        c.rowspan,
                        c.colspan,
                    ]);
                    const declaredRectangles =
                        name === 'collision'
                            ? [
                                  [0, 0, 1, 1],
                                  [0, 1, 2, 1],
                                  [1, 3, 1, 1],
                              ]
                            : name === 'header-gap'
                              ? [[0, 1, 1, 1], [1, 0, 1, 1], [1, 1, 1, 1]]
                            : name === 'overlong-rowspan'
                              ? [
                                    [0, 0, 2, 1],
                                    [0, 1, 1, 1],
                                    [1, 1, 1, 1],
                                ]
                              : name === 'missing-slot'
                                ? [
                                      [0, 0, 1, 1],
                                      [0, 1, 1, 1],
                                      [1, 0, 1, 1],
                                  ]
                                : [
                                      [0, 0, 1, 1],
                                      [0, 1, 1, 1],
                                      [1, 0, 1, 1],
                                      [1, 1, 1, 1],
                                  ];
                    assert.deepEqual(nativeRectangles, declaredRectangles);
                    assert.deepEqual(
                        expected.tables[0]!.widths,
                        name === 'collision'
                            ? [null, null, null, null]
                            : name === 'inconsistent-width'
                              ? [140, null]
                              : [null, null],
                    );
                    for (const source of initialObservation.tables[0]!.cells) {
                        const surviving = observed.tables[0]!.cells.find(
                            (c) => c.source === source.source,
                        );
                        if (surviving)
                            assert.equal(
                                surviving.sourceId,
                                source.sourceId,
                                'wrapper replacement preserves the shared cell identity',
                            );
                    }
                    assert.equal(observed.tables.length, name === 'nested' ? 2 : 1);
                    assert.ok(
                        observed.tables
                            .flatMap((t) => t.cells)
                            .filter((c) => c.source !== null)
                            .every(
                                (c) =>
                                    c.sourceId &&
                                    Number.isInteger(c.position) &&
                                    Number.isInteger(c.rawPosition),
                            ),
                    );
                    if (name === 'collision') {
                        semantics.assertEffectivePresentation(expected, observed);
                        assert.equal(expected.tables[0]!.cells[2]!.column, 3);
                        assert.equal(expected.tables[0]!.cells[2]!.colspan, 1);
                        assert.equal(
                            observed.tables[0]!.cells.find(
                                (c) => c.source === expected.tables[0]!.cells[2]!.source,
                            )!.column,
                            3,
                        );
                        assert.equal(
                            observed.tables[0]!.cells.find(
                                (c) => c.source === expected.tables[0]!.cells[2]!.source,
                            )!.colspan,
                            1,
                        );
                    } else if (name === 'nested') {
                        semantics.assertEffectivePresentation(expected, observed);
                        assert.equal(expected.tables[1]!.cells[0]!.column, 1);
                        assert.equal(
                            observed.tables[1]!.cells.find(
                                (c) => c.source === expected.tables[1]!.cells[0]!.source,
                            )!.column,
                            1,
                        );
                    } else {
                        semantics.assertEffectivePresentation(expected, observed);
                    }
                    assert.deepEqual(
                        await snapshot(web),
                        before,
                        'observation must not write, dispatch, or refresh binding state',
                    );
                    assert.equal((await flushDocumentEvents(web)).length, 0);
                    if (name !== 'regular')
                        assert.notDeepEqual(before.documentJson, before.displayJson);
                    if (name === 'missing-slot') {
                        const target = observed.tables[0]!.cells.find((c) => c.source !== null)!;
                        await call(web, 'command', {
                            type: 'insertText',
                            text: 'typed',
                            at: target.position + 2,
                        });
                        const after = await semantics.observeWebPresentation(web);
                        const typed = after.tables[0]!.cells.find(
                            (c) => c.sourceId === target.sourceId,
                        );
                        assert.equal(typed?.node.content?.[0]?.content?.[0]?.text, 'typedsame');
                    }
                    authored.destroy();
                },
                tableFixture(preset),
            );
        });
    }
}
