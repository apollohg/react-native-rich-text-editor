import assert from 'node:assert/strict';
import test from 'node:test';
import { call, paragraphFixture, withPeers } from '../controller.js';
import type { Peer } from '../peer-protocol.js';
import {
    NO_COLLISIONS,
    TABLE_SCHEMA,
    cell,
    geometryOf,
    row,
    table,
} from '../table-schema.js';


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
    return call(peer, 'projectTable', payload);
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

                    assert.ok(
                        Array.isArray(oracle['slots']) && oracle['slots'].length > 0,
                        'the oracle must report a slot anchor for every grid position',
                    );
                    assert.equal(
                        oracle['collisions'],
                        NO_COLLISIONS,
                        'placement is only comparable where the reference reports no collision',
                    );
                    assert.deepEqual(
                        geometryOf(projected),
                        geometryOf(oracle),
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
