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
import { TABLE_SCHEMA, cell, cellAnchors, row, table, tableOf } from '../table-schema.js';

const TABLE_START = 0;
const DOC_NODE = 'doc';
const LEFT_WIDTH = 140;
const RIGHT_WIDTH = 220;
const SEEDED_FIRST_COLUMN = 100;
const SEEDED_SECOND_COLUMN = 180;
const SEEDED_LOWER_FIRST_COLUMN = 120;
const SEEDED_LOWER_SECOND_COLUMN = 160;
const UNRESOLVED = null;
const REVERSED_EXCHANGE_SEED = DEFAULT_EXCHANGE_SEED ^ 0x5a5a_5a5a;

const DELIVERY_ORDERS = ['forward', 'reversed'] as const;

type ResizeScenario = {
    name: string;
    table: Record<string, unknown>;
    leftCell: number;
    rightCell: number;
    seeded: (number | null)[];
    admissible: (number | null)[][];
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
        seeded: [UNRESOLVED, UNRESOLVED],
        admissible: [
            [LEFT_WIDTH, UNRESOLVED],
            [RIGHT_WIDTH, UNRESOLVED],
        ],
    },
    {
        name: 'the peers resize different logical columns',
        table: table([
            row([cell({ text: 'a0' }), cell({ text: 'a1' })]),
            row([cell({ text: 'b0' }), cell({ text: 'b1' })]),
        ]),
        leftCell: 0,
        rightCell: 1,
        seeded: [UNRESOLVED, UNRESOLVED],
        admissible: [[LEFT_WIDTH, RIGHT_WIDTH]],
    },
    {
        name: 'the peers write conflicting per row width arrays',
        table: table([
            row([
                cell({ colwidth: [SEEDED_FIRST_COLUMN], text: 'a0' }),
                cell({ colwidth: [SEEDED_SECOND_COLUMN], text: 'a1' }),
            ]),
            row([
                cell({ colwidth: [SEEDED_LOWER_FIRST_COLUMN], text: 'b0' }),
                cell({ colwidth: [SEEDED_LOWER_SECOND_COLUMN], text: 'b1' }),
            ]),
        ]),
        leftCell: 0,
        rightCell: 2,
        seeded: [SEEDED_LOWER_FIRST_COLUMN, SEEDED_LOWER_SECOND_COLUMN],
        admissible: [
            [LEFT_WIDTH, SEEDED_LOWER_SECOND_COLUMN],
            [RIGHT_WIDTH, SEEDED_LOWER_SECOND_COLUMN],
        ],
    },
    {
        name: 'the peers resize columns a spanning cell covers',
        table: table([
            row([
                cell({
                    colspan: 2,
                    colwidth: [SEEDED_FIRST_COLUMN, SEEDED_SECOND_COLUMN],
                    text: 'wide',
                }),
            ]),
            row([cell({ text: 'b0' }), cell({ text: 'b1' })]),
        ]),
        leftCell: 1,
        rightCell: 2,
        seeded: [SEEDED_FIRST_COLUMN, SEEDED_SECOND_COLUMN],
        admissible: [
            [LEFT_WIDTH, SEEDED_SECOND_COLUMN],
            [SEEDED_FIRST_COLUMN, RIGHT_WIDTH],
        ],
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

async function resolvedWidths(
    peer: Peer,
    documentJson: Record<string, unknown> | null,
): Promise<(number | null)[]> {
    const projection = await call(peer, 'projectTable', {
        schema: TABLE_SCHEMA,
        table: tableOf(documentJson),
    });
    const widths = projection['widths'];
    assert.ok(Array.isArray(widths), 'the projection reports the resolved column widths');
    return widths as (number | null)[];
}

function admits(scenario: ResizeScenario, widths: (number | null)[]): boolean {
    return scenario.admissible.some((candidate) => {
        try {
            assert.deepEqual(widths, candidate);
            return true;
        } catch {
            return false;
        }
    });
}

async function concurrentResize(
    scenario: ResizeScenario,
    order: (typeof DELIVERY_ORDERS)[number],
): Promise<void> {
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
            assert.deepEqual(
                await resolvedWidths(left, (await snapshot(left)).documentJson),
                scenario.seeded,
                'the seeded widths must be what the scenario claims, or the outcome proves nothing',
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

            const widths = await resolvedWidths(left, leftDocument);
            assert.notDeepEqual(
                widths,
                scenario.seeded,
                `${scenario.name} left the seeded widths untouched, so nothing was written`,
            );
            assert.ok(
                admits(scenario, widths),
                `${scenario.name} delivered ${order} settled on ${JSON.stringify(widths)}, `
                    + `which is none of ${JSON.stringify(scenario.admissible)}`,
            );
        },
        tableFixture('prosemirror'),
    );
}

test('TBL-13 concurrent column widths converge on both replicas', async (context) => {
    for (const scenario of SCENARIOS) {
        for (const order of DELIVERY_ORDERS) {
            await context.test(`${scenario.name}, delivered ${order}`, async () => {
                await concurrentResize(scenario, order);
            });
        }
    }
});

test('a concurrent width write never provokes an orphan cleanup write', async () => {
    await withPeers(
        ['rust', 'rust'] as const,
        async ([left, right]) => {
            const fixture = table([
                row([
                    cell({
                        colspan: 2,
                        colwidth: [SEEDED_FIRST_COLUMN, SEEDED_SECOND_COLUMN],
                        text: 'wide',
                    }),
                ]),
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
