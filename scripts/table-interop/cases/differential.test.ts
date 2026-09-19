import assert from 'node:assert/strict';
import test from 'node:test';
import {
    call,
    exchangeUntilIdle,
    seedFrom,
    snapshot,
    tableFixture,
    withPeers,
} from '../controller.js';
import type { Peer, TableCommand } from '../peer-protocol.js';
import {
    TABLE_NODE,
    TABLE_SCHEMA,
    canonical,
    cell,
    row,
    table,
    tableOf,
} from '../table-schema.js';

const NO_NORMALIZATION_PASSES = 0;
const NO_COLLISIONS = 0;
const ONE_NORMALIZATION_PASS = 1;

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

function remoteTable(): Record<string, unknown> {
    return table([
        row([cell({ text: 'r1' }), cell({ text: 'r2' })]),
        row([cell({ text: 'r3' }), cell({ text: 'r4' })]),
    ]);
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
        name: 'a valid merged grid with agreeing widths is left exactly as it is',
        table: table([
            row([cell({ colspan: 3, colwidth: [100, 140, 180], text: 'h' })]),
            row([
                cell({ rowspan: 2, colwidth: [100], text: 'v' }),
                cell({ colwidth: [140], text: 'b' }),
                cell({ colwidth: [180], text: 'c' }),
            ]),
            row([cell({ colwidth: [140], text: 'd' }), cell({ colwidth: [180], text: 'e' })]),
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

test('TBL-10 normalization preserves ordered reference repairs and the empty-frame exception', async (context) => {
    await withPeers(
        ['rust', 'prosemirror'] as const,
        async ([engine, web]) => {
            for (const rowspan of [1, 2, 3]) {
                await context.test(`collision with rowspan ${rowspan} matches exactly one stock pass`, async () => {
                    const fixture = table([
                        row([cell({ text: 'a' }), cell({ rowspan: 2, text: 'b' })]),
                        row([cell({ colspan: 2, rowspan, text: 'c' })]),
                        ...(rowspan > 1 ? [row([])] : []),
                    ]);
                    const expected = rowspan === 1
                        ? table([
                            row([cell({ text: 'a' }), cell({ rowspan: 2, text: 'b' }), cell()]),
                            row([cell(), cell(), cell({ text: 'c' })]),
                        ])
                        : table([
                            row([cell({ text: 'a' }), cell({ rowspan: 2, text: 'b' }), cell()]),
                            row([cell({ colspan: rowspan === 3 ? 2 : 1, rowspan: 2, text: 'c' }), cell(), cell()]),
                            row([cell(), cell()]),
                        ]);
                    const projection = await call(web, 'projectTable', { table: fixture });
                    assert.ok((projection['collisions'] as number) > NO_COLLISIONS);
                    const reference = await referenceNormalization(web, fixture);
                    assert.deepEqual(canonical(reference['table']), canonical(expected));
                    const native = await nativeNormalization(engine, fixture);
                    assert.deepEqual(canonical(native['table']), canonical(expected));
                });
            }

            await context.test('raw native normalization retains an empty table while reference normalization deletes it', async () => {
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
        ['rust', 'prosemirror'] as const,
        async ([engine, web]) => {
            await seedFrom(engine, [web]);
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

            await call(web, 'command', { type: 'insertNode', node: remoteTable() });
            await call(web, 'command', { type: 'insertText', text: 'peer' });
            await exchangeUntilIdle([web, engine]);
            const converged = await snapshot(engine);
            assert.deepEqual(
                converged.documentJson,
                (await snapshot(web)).documentJson,
                'the remote table must actually have arrived before its zero means anything',
            );
            assert.equal(
                converged.normalizationPassesAfterLastAction,
                NO_NORMALIZATION_PASSES,
                'a remote update from another replica never plans a normalization pass',
            );

            await call(engine, 'undo', {});
            const afterUndo = await snapshot(engine);
            assert.equal(afterUndo.normalizationPassesAfterLastAction, NO_NORMALIZATION_PASSES);

            await call(engine, 'redo', {});
            const afterRedo = await snapshot(engine);
            assert.equal(afterRedo.normalizationPassesAfterLastAction, NO_NORMALIZATION_PASSES);
        },
        tableFixture('prosemirror'),
    );
});

const ANCHOR_CELL_POSITION = 3;

type CommandScenario = {
    name: string;
    native: TableCommand;
    reference: string;
};

const COMMAND_SCENARIOS: CommandScenario[] = [
    {
        name: 'a row added after the anchored cell',
        native: { type: 'addTableRow', side: 'after' },
        reference: 'addRowAfter',
    },
    {
        name: 'a row added before the anchored cell',
        native: { type: 'addTableRow', side: 'before' },
        reference: 'addRowBefore',
    },
    {
        name: 'the anchored row deleted',
        native: { type: 'deleteTableRows' },
        reference: 'deleteRow',
    },
    {
        name: 'a column added after the anchored cell',
        native: { type: 'addTableColumn', side: 'after' },
        reference: 'addColumnAfter',
    },
    {
        name: 'a column added before the anchored cell',
        native: { type: 'addTableColumn', side: 'before' },
        reference: 'addColumnBefore',
    },
    {
        name: 'the anchored column deleted',
        native: { type: 'deleteTableColumns' },
        reference: 'deleteColumn',
    },
    {
        name: 'the anchored header row toggled on',
        native: { type: 'toggleTableHeader', target: 'row' },
        reference: 'toggleHeaderRow',
    },
    {
        name: 'the anchored header column toggled on',
        native: { type: 'toggleTableHeader', target: 'column' },
        reference: 'toggleHeaderColumn',
    },
    {
        name: 'the anchored header cell toggled on',
        native: { type: 'toggleTableHeader', target: 'cell' },
        reference: 'toggleHeaderCell',
    },
];

function commandFixture(): Record<string, unknown> {
    return table([
        row([cell({ text: 'a' }), cell({ text: 'b' })]),
        row([cell({ text: 'c' }), cell({ text: 'd' })]),
        row([cell({ text: 'e' }), cell({ text: 'f' })]),
    ]);
}

test('TBL-06 row, column and header commands agree with prosemirror-tables 1.8.5', async (context) => {
    for (const scenario of COMMAND_SCENARIOS) {
        await context.test(scenario.name, async () => {
            await withPeers(
                ['prosemirror', 'rust'] as const,
                async ([web, engine]) => {
                    await call(web, 'command', { type: 'insertNode', node: commandFixture() });
                    await seedFrom(web, [engine]);
                    await exchangeUntilIdle([web, engine]);
                    assert.deepEqual(
                        tableOf((await snapshot(engine)).documentJson),
                        tableOf((await snapshot(web)).documentJson),
                        'the peers must start from the same table',
                    );

                    await call(web, 'command', {
                        type: 'tableCommand',
                        name: scenario.reference,
                        at: ANCHOR_CELL_POSITION,
                    });
                    await call(engine, 'command', {
                        ...scenario.native,
                        at: ANCHOR_CELL_POSITION,
                    });

                    assert.deepEqual(
                        tableOf((await snapshot(engine)).documentJson),
                        tableOf((await snapshot(web)).documentJson),
                        `the engine and prosemirror-tables disagree about ${scenario.name}`,
                    );
                },
                tableFixture('prosemirror'),
            );
        });
    }
});
