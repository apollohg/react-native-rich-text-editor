import assert from 'node:assert/strict';
import test from 'node:test';
import {
    PeerError,
    call,
    exchangeUntilIdle,
    seedFrom,
    snapshot,
    tableFixture,
    withPeers,
} from '../controller.js';
import type { PeerSnapshot } from '../controller.js';
import type { Peer, TableCommand } from '../peer-protocol.js';
import { cell, cellAnchors, row, table, tableOf } from '../table-schema.js';

const TABLE_START = 0;
const NOT_APPLICABLE = 'notApplicable';
const FIRST_CELL = 0;
const LAST_SURVIVING_CELL = 3;
const REMOTE_TEXT = 'remote';
const SPAN_CUT_ANCHOR = 2;
const SPAN_CUT_HEAD = 3;
const DOC_NODE = 'doc';

async function seedNative(
    source: Peer,
    targets: Peer[],
    fixture: Record<string, unknown>,
): Promise<void> {
    await call(source, 'command', {
        type: 'insertContentJson',
        json: { type: DOC_NODE, content: [fixture] },
    });
    await seedFrom(source, targets);
    await exchangeUntilIdle([source, ...targets]);
}

function anchorsOf(peerSnapshot: PeerSnapshot): number[] {
    return cellAnchors(tableOf(peerSnapshot.documentJson) as Record<string, unknown>, TABLE_START);
}

type CommandScenario = {
    name: string;
    table: Record<string, unknown>;
    anchor: number;
    head: number;
    native: TableCommand;
    reference: string;
};

const AGREEING_SCENARIOS: CommandScenario[] = [
    {
        name: 'a rectangle of rich cells merges into its top left cell',
        table: table([
            row([cell({ blocks: ['a0', 'a1'] }), cell({ text: 'b' })]),
            row([cell({ text: 'c' }), cell({ blocks: ['d0', 'd1'] })]),
        ]),
        anchor: 0,
        head: 3,
        native: { type: 'mergeTableCells' },
        reference: 'mergeCells',
    },
    {
        name: 'an empty source contributes no block and an empty survivor drops its placeholder',
        table: table([
            row([cell(), cell({ text: 'b' })]),
            row([cell({ text: 'c' }), cell()]),
        ]),
        anchor: 0,
        head: 3,
        native: { type: 'mergeTableCells' },
        reference: 'mergeCells',
    },
    {
        name: 'a merge of mixed cell types keeps the surviving type',
        table: table([
            row([cell({ header: true, text: 'h' }), cell({ text: 'b' })]),
            row([cell({ text: 'c' }), cell({ text: 'd' })]),
        ]),
        anchor: 0,
        head: 1,
        native: { type: 'mergeTableCells' },
        reference: 'mergeCells',
    },
    {
        name: 'a merge that swallows a horizontal span widens the survivor',
        table: table([
            row([cell({ text: 'a' }), cell({ text: 'b' }), cell({ text: 'c' })]),
            row([cell({ text: 'd' }), cell({ colspan: 2, text: 'wide' })]),
        ]),
        anchor: 0,
        head: 4,
        native: { type: 'mergeTableCells' },
        reference: 'mergeCells',
    },
    {
        name: 'a merge that swallows a vertical span heightens the survivor',
        table: table([
            row([cell({ text: 'a' }), cell({ rowspan: 2, text: 'tall' })]),
            row([cell({ text: 'c' })]),
        ]),
        anchor: 0,
        head: 2,
        native: { type: 'mergeTableCells' },
        reference: 'mergeCells',
    },
    {
        name: 'a horizontal span splits into unit spans with sliced widths',
        table: table([
            row([cell({ colspan: 2, colwidth: [120, 160], text: 'wide' })]),
            row([cell({ colwidth: [120], text: 'b' }), cell({ colwidth: [160], text: 'c' })]),
        ]),
        anchor: 0,
        head: 0,
        native: { type: 'splitTableCell' },
        reference: 'splitCell',
    },
    {
        name: 'a vertical span splits into a fresh cell in every covered row',
        table: table([
            row([cell({ rowspan: 3, text: 'tall' }), cell({ text: 'a' })]),
            row([cell({ text: 'b' })]),
            row([cell({ text: 'c' })]),
        ]),
        anchor: 0,
        head: 0,
        native: { type: 'splitTableCell' },
        reference: 'splitCell',
    },
    {
        name: 'a cell spanning both ways splits into a full rectangle',
        table: table([
            row([cell({ colspan: 2, rowspan: 2, text: 'block' }), cell({ text: 'a' })]),
            row([cell({ text: 'b' })]),
        ]),
        anchor: 0,
        head: 0,
        native: { type: 'splitTableCell' },
        reference: 'splitCell',
    },
];

async function seedBothPeers(
    web: Peer,
    engine: Peer,
    fixture: Record<string, unknown>,
): Promise<void> {
    await call(web, 'command', { type: 'insertNode', node: fixture });
    await seedFrom(web, [engine]);
    await exchangeUntilIdle([web, engine]);
    assert.deepEqual(
        tableOf((await snapshot(engine)).documentJson),
        tableOf((await snapshot(web)).documentJson),
        'the peers must start from the same table',
    );
}

test('TBL-07 merge and split agree with prosemirror-tables 1.8.5', async (context) => {
    for (const scenario of AGREEING_SCENARIOS) {
        await context.test(scenario.name, async () => {
            await withPeers(
                ['prosemirror', 'rust'] as const,
                async ([web, engine]) => {
                    await seedBothPeers(web, engine, scenario.table);
                    const anchors = cellAnchors(scenario.table, TABLE_START);
                    const at = anchors[scenario.anchor];
                    const head = anchors[scenario.head];
                    assert.ok(
                        at !== undefined && head !== undefined,
                        'the scenario addresses cells the fixture holds',
                    );

                    await call(web, 'command', {
                        type: 'tableCommand',
                        name: scenario.reference,
                        at,
                        head,
                    });
                    await call(engine, 'command', { ...scenario.native, at, head });

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

test('TBL-07 a selection cutting a span is refused by both the engine and the reference', async () => {
    await withPeers(
        ['prosemirror', 'rust'] as const,
        async ([web, engine]) => {
            const fixture = table([
                row([cell({ rowspan: 2, text: 'tall' }), cell({ text: 'a' })]),
                row([cell({ text: 'b' })]),
                row([cell({ text: 'c' }), cell({ text: 'd' })]),
            ]);
            await seedBothPeers(web, engine, fixture);
            const anchors = cellAnchors(fixture, TABLE_START);
            const at = anchors[SPAN_CUT_ANCHOR];
            const head = anchors[SPAN_CUT_HEAD];
            assert.ok(at !== undefined && head !== undefined);
            const before = tableOf((await snapshot(engine)).documentJson);

            const refusal = await call(web, 'command', {
                type: 'tableCommand',
                name: 'mergeCells',
                at,
                head,
            }).then(() => null, (error: unknown) => error);
            assert.ok(
                refusal instanceof PeerError,
                'prosemirror-tables refuses a rectangle a span sticks out of',
            );

            const outcome = await call(engine, 'command', { type: 'mergeTableCells', at, head });
            assert.equal(
                outcome['type'],
                NOT_APPLICABLE,
                'the engine must refuse a rectangle a span sticks out of, not expand it',
            );
            assert.deepEqual(
                tableOf((await snapshot(engine)).documentJson),
                before,
                'a refused merge must leave the table exactly as it was',
            );
            assert.deepEqual(
                tableOf((await snapshot(web)).documentJson),
                before,
                'both peers must be left on the same unmerged table',
            );
        },
        tableFixture('prosemirror'),
    );
});

test('undoing a merge keeps a concurrent remote edit and normalizes nothing', async () => {
    await withPeers(
        ['rust', 'rust'] as const,
        async ([native, web]) => {
            await seedNative(native, [web], table([
                row([cell({ text: 'a0' }), cell({ text: 'a1' }), cell({ text: 'a2' })]),
                row([cell({ text: 'b0' }), cell({ text: 'b1' }), cell({ text: 'b2' })]),
            ]));
            const seeded = anchorsOf(await snapshot(native));
            await call(native, 'command', { type: 'selectTableRows', at: seeded[FIRST_CELL] });

            const before = await snapshot(native);
            await call(native, 'command', { type: 'mergeTableCells' });
            await exchangeUntilIdle([native, web]);
            const survivor = anchorsOf(await snapshot(web))[LAST_SURVIVING_CELL];
            assert.ok(survivor !== undefined, 'the merged table keeps an unrelated cell to type in');
            await call(web, 'command', { type: 'insertText', text: REMOTE_TEXT, at: survivor });
            await exchangeUntilIdle([native, web]);
            await call(native, 'undo', {});
            await exchangeUntilIdle([native, web]);
            assert.equal((await snapshot(native)).normalizationPassesAfterLastAction, 0);
            assert.deepEqual(
                (await snapshot(native)).documentJson,
                (await snapshot(web)).documentJson,
            );
            assert.equal(before.documentJson !== null, true);
            assert.ok(
                JSON.stringify((await snapshot(native)).documentJson).includes(REMOTE_TEXT),
                'the selective undo must leave the concurrent remote text alone',
            );
        },
        tableFixture('prosemirror'),
    );
});
