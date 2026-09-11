import assert from 'node:assert/strict';
import test from 'node:test';
import { call, snapshot, tableFixture, withPeers } from '../controller.js';
import type { Peer } from '../peer-protocol.js';
import {
    CELL_NODE,
    HEADER_CELL_NODE,
    PARAGRAPH_NODE,
    ROW_NODE,
    TABLE_NODE,
    TABLE_SCHEMA,
} from '../table-schema.js';

const NO_NORMALIZATION_PASSES = 0;
const NO_COLLISIONS = 0;
const ONE_NORMALIZATION_PASS = 1;
const SINGLE_SPAN = 1;

type CellOptions = {
    colspan?: number;
    rowspan?: number;
    colwidth?: number[] | null;
    header?: boolean;
    text?: string;
};

function cell(options: CellOptions = {}): Record<string, unknown> {
    const paragraph = options.text === undefined
        ? { type: PARAGRAPH_NODE }
        : { type: PARAGRAPH_NODE, content: [{ type: 'text', text: options.text }] };
    return {
        type: options.header === true ? HEADER_CELL_NODE : CELL_NODE,
        attrs: {
            colspan: options.colspan ?? SINGLE_SPAN,
            rowspan: options.rowspan ?? SINGLE_SPAN,
            colwidth: options.colwidth ?? null,
        },
        content: [paragraph],
    };
}

function row(cells: Record<string, unknown>[]): Record<string, unknown> {
    return { type: ROW_NODE, content: cells };
}

function table(rows: Record<string, unknown>[]): Record<string, unknown> {
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

function canonical(value: unknown): unknown {
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

async function nativeNormalization(
    peer: Peer,
    fixture: Record<string, unknown>,
): Promise<Record<string, unknown>> {
    return call(peer, 'normalizeTable', { schema: TABLE_SCHEMA, table: fixture });
}

async function referenceNormalization(
    peer: Peer,
    fixture: Record<string, unknown>,
): Promise<Record<string, unknown>> {
    return call(peer, 'normalizeTable', { table: fixture });
}

type AgreeingFixture = { name: string; table: Record<string, unknown> };

const AGREEING_FIXTURES: AgreeingFixture[] = [
    {
        name: 'a trailing gap is filled at the end of its own row',
        table: table([
            row([cell({ text: 'a' }), cell({ text: 'b' })]),
            row([cell({ text: 'c' })]),
        ]),
    },
    {
        name: 'a gap in the only short first row is filled at the reference side',
        table: table([
            row([cell({ text: 'a' })]),
            row([cell({ text: 'b' }), cell({ text: 'c' })]),
        ]),
    },
    {
        name: 'an overlong rowspan is clamped to the remaining rows',
        table: table([row([cell({ rowspan: 4, text: 'tall' }), cell({ text: 'b' })])]),
    },
    {
        name: 'a width disagreement adopts the resolved column width',
        table: table([
            row([cell({ colwidth: [100], text: 'a' })]),
            row([cell({ colwidth: [140], text: 'b' })]),
        ]),
    },
    {
        name: 'a span aware width disagreement rewrites only its own slice',
        table: table([
            row([cell({ colspan: 2, colwidth: [100, 160], text: 'a' })]),
            row([cell({ colwidth: [140], text: 'b' }), cell({ colwidth: [160], text: 'c' })]),
            row([cell({ colwidth: [140], text: 'd' }), cell({ colwidth: [160], text: 'e' })]),
        ]),
    },
    {
        name: 'a valid grid is left exactly as it is',
        table: table([
            row([cell({ header: true, text: 'a' }), cell({ header: true, text: 'b' })]),
            row([cell({ text: 'c' }), cell({ text: 'd' })]),
        ]),
    },
];

test('one native normalization pass agrees with prosemirror-tables 1.8.5', async (context) => {
    await withPeers(
        ['rust', 'prosemirror'] as const,
        async ([engine, web]) => {
            for (const fixture of AGREEING_FIXTURES) {
                await context.test(fixture.name, async () => {
                    const native = await nativeNormalization(engine, fixture.table);
                    const reference = await referenceNormalization(web, fixture.table);
                    assert.deepEqual(
                        canonical(native['table']),
                        canonical(reference['table']),
                        `the engine and prosemirror-tables disagree about ${fixture.name}`,
                    );
                });
            }
        },
        tableFixture('prosemirror'),
    );
});

test('the spec sanctioned divergences are pinned, not hidden', async (context) => {
    await withPeers(
        ['rust', 'prosemirror'] as const,
        async ([engine, web]) => {
            await context.test('a collision shifts right natively and overlaps in the reference', async () => {
                const fixture = table([
                    row([cell({ text: 'a' }), cell({ rowspan: 2, text: 'b' })]),
                    row([cell({ colspan: 2, text: 'c' })]),
                ]);
                const projection = await call(web, 'projectTable', { table: fixture });
                assert.ok(
                    (projection['collisions'] as number) > NO_COLLISIONS,
                    'this fixture only pins a divergence while the reference reports a collision',
                );
                const native = await nativeNormalization(engine, fixture);
                const reference = await referenceNormalization(web, fixture);
                assert.notDeepEqual(
                    canonical(native['table']),
                    canonical(reference['table']),
                    'TBL-11 mandates shift-right placement, so this fixture must diverge',
                );
                assert.equal(
                    (native['table'] as Record<string, unknown>)['type'],
                    TABLE_NODE,
                    'native normalization never deletes the table it was asked to repair',
                );
            });

            await context.test('a zero size table survives natively and is deleted by the reference', async () => {
                const fixture = table([row([]), row([])]);
                const native = await nativeNormalization(engine, fixture);
                const reference = await referenceNormalization(web, fixture);
                assert.equal(
                    native['operations'],
                    0,
                    'TBL-10 keeps an empty table frame instead of repairing it',
                );
                assert.deepEqual(canonical(native['table']), canonical(fixture));
                const referenceTable = reference['table'] as Record<string, unknown> | null;
                assert.notEqual(
                    referenceTable === null ? null : referenceTable['type'],
                    TABLE_NODE,
                    'the pinned reference deletes a zero sized table',
                );
            });
        },
        tableFixture('prosemirror'),
    );
});

test('only an explicit normalization request advances the native pass counter', async () => {
    await withPeers(
        ['rust'] as const,
        async ([engine]) => {
            const initial = await snapshot(engine);
            assert.equal(initial.normalizationPassesAfterLastAction, NO_NORMALIZATION_PASSES);

            await nativeNormalization(
                engine,
                table([row([cell({ text: 'a' }), cell({ text: 'b' })]), row([cell({ text: 'c' })])]),
            );
            const requested = await snapshot(engine);
            assert.equal(
                requested.normalizationPassesAfterLastAction,
                ONE_NORMALIZATION_PASS,
                'the counter must be able to rise, or its zeroes prove nothing',
            );

            await call(engine, 'command', { type: 'insertText', text: 'typing' });
            const afterTyping = await snapshot(engine);
            assert.equal(afterTyping.normalizationPassesAfterLastAction, NO_NORMALIZATION_PASSES);

            const diff = await call(engine, 'stateDiff', {
                stateVectorBase64: initial.stateVectorBase64,
            });
            await call(engine, 'applyUpdate', {
                updateBase64: diff['updateBase64'] as string,
            });
            const afterRemote = await snapshot(engine);
            assert.equal(afterRemote.normalizationPassesAfterLastAction, NO_NORMALIZATION_PASSES);

            await call(engine, 'undo', {});
            const afterUndo = await snapshot(engine);
            assert.equal(afterUndo.normalizationPassesAfterLastAction, NO_NORMALIZATION_PASSES);
        },
        tableFixture('prosemirror'),
    );
});
