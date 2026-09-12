import assert from 'node:assert/strict';
import test from 'node:test';
import {
    DEFAULT_EXCHANGE_SEED,
    call,
    exchangeUntilIdle,
    seedFrom,
    snapshot,
    tableFixture,
    withPeers,
} from '../controller.js';
import type { Peer } from '../peer-protocol.js';
import { cell, cellAnchors, row, table, tableOf } from '../table-schema.js';

const TABLE_START = 0;
const DOC_NODE = 'doc';
const LEFT_WIDTH = 140;
const RIGHT_WIDTH = 220;
const REVERSED_EXCHANGE_SEED = DEFAULT_EXCHANGE_SEED ^ 0x5a5a_5a5a;

const DELIVERY_ORDERS = ['forward', 'reversed'] as const;

type ResizeScenario = {
    name: string;
    table: Record<string, unknown>;
    leftCell: number;
    rightCell: number;
};

const SCENARIOS: ResizeScenario[] = [
    {
        name: 'both peers resize the same logical column',
        table: table([
            row([cell({ text: 'a0' }), cell({ text: 'a1' })]),
            row([cell({ text: 'b0' }), cell({ text: 'b1' })]),
        ]),
        leftCell: 0,
        rightCell: 2,
    },
    {
        name: 'the peers resize different logical columns',
        table: table([
            row([cell({ text: 'a0' }), cell({ text: 'a1' })]),
            row([cell({ text: 'b0' }), cell({ text: 'b1' })]),
        ]),
        leftCell: 0,
        rightCell: 1,
    },
    {
        name: 'the peers write conflicting per row width arrays',
        table: table([
            row([cell({ colwidth: [100], text: 'a0' }), cell({ colwidth: [180], text: 'a1' })]),
            row([cell({ colwidth: [120], text: 'b0' }), cell({ colwidth: [160], text: 'b1' })]),
        ]),
        leftCell: 0,
        rightCell: 2,
    },
    {
        name: 'the peers resize columns a spanning cell covers',
        table: table([
            row([cell({ colspan: 2, colwidth: [100, 180], text: 'wide' })]),
            row([cell({ text: 'b0' }), cell({ text: 'b1' })]),
        ]),
        leftCell: 1,
        rightCell: 2,
    },
];

async function seedPair(
    source: Peer,
    target: Peer,
    fixture: Record<string, unknown>,
): Promise<number[]> {
    await call(source, 'command', {
        type: 'insertContentJson',
        json: { type: DOC_NODE, content: [fixture] },
    });
    await seedFrom(source, [target]);
    await exchangeUntilIdle([source, target]);
    assert.deepEqual(
        tableOf((await snapshot(source)).documentJson),
        tableOf((await snapshot(target)).documentJson),
        'the peers must start from the same table',
    );
    return cellAnchors(fixture, TABLE_START);
}

async function concurrentResize(
    scenario: ResizeScenario,
    order: (typeof DELIVERY_ORDERS)[number],
): Promise<unknown> {
    let converged: unknown = null;
    await withPeers(
        ['rust', 'rust'] as const,
        async ([left, right]) => {
            const anchors = await seedPair(left, right, scenario.table);
            const leftAt = anchors[scenario.leftCell];
            const rightAt = anchors[scenario.rightCell];
            assert.ok(
                leftAt !== undefined && rightAt !== undefined,
                'the scenario addresses cells the fixture holds',
            );

            await call(left, 'command', {
                type: 'setTableColumnWidth',
                width: LEFT_WIDTH,
                at: leftAt,
            });
            await call(right, 'command', {
                type: 'setTableColumnWidth',
                width: RIGHT_WIDTH,
                at: rightAt,
            });

            const peers = order === 'forward' ? [left, right] : [right, left];
            const seed = order === 'forward' ? DEFAULT_EXCHANGE_SEED : REVERSED_EXCHANGE_SEED;
            await exchangeUntilIdle(peers, seed);
            await exchangeUntilIdle(peers, seed);

            const leftDocument = (await snapshot(left)).documentJson;
            assert.deepEqual(
                leftDocument,
                (await snapshot(right)).documentJson,
                `the peers diverged after ${scenario.name} delivered ${order}`,
            );
            converged = tableOf(leftDocument);
        },
        tableFixture('prosemirror'),
    );
    return converged;
}

test('TBL-13 concurrent column widths converge on both replicas', async (context) => {
    for (const scenario of SCENARIOS) {
        for (const order of DELIVERY_ORDERS) {
            await context.test(`${scenario.name}, delivered ${order}`, async () => {
                const converged = await concurrentResize(scenario, order);
                assert.notEqual(converged, null, 'the scenario produced a converged table');
            });
        }
    }
});

test('a concurrent width write never provokes an orphan cleanup write', async () => {
    await withPeers(
        ['rust', 'rust'] as const,
        async ([left, right]) => {
            const fixture = table([
                row([cell({ colspan: 2, colwidth: [100, 180], text: 'wide' })]),
                row([cell({ text: 'b0' }), cell({ text: 'b1' })]),
            ]);
            const anchors = await seedPair(left, right, fixture);
            const leftAt = anchors[1];
            const rightAt = anchors[2];
            assert.ok(leftAt !== undefined && rightAt !== undefined);

            await call(left, 'command', {
                type: 'setTableColumnWidth',
                width: LEFT_WIDTH,
                at: leftAt,
            });
            await call(right, 'command', {
                type: 'setTableColumnWidth',
                width: RIGHT_WIDTH,
                at: rightAt,
            });
            const before = [
                (await snapshot(left)).autonomousRepairWrites,
                (await snapshot(right)).autonomousRepairWrites,
            ];
            await exchangeUntilIdle([left, right]);
            await exchangeUntilIdle([left, right]);

            assert.deepEqual(
                [
                    (await snapshot(left)).autonomousRepairWrites,
                    (await snapshot(right)).autonomousRepairWrites,
                ],
                before,
                'receiving a concurrent width must not make a replica write a repair',
            );
            assert.deepEqual(
                (await snapshot(left)).documentJson,
                (await snapshot(right)).documentJson,
            );
        },
        tableFixture('prosemirror'),
    );
});
