import * as Y from 'yjs';
import { Schema } from 'prosemirror-model';
import { schema as basic } from 'prosemirror-schema-basic';
import { tableNodes } from 'prosemirror-tables';
import { prosemirrorJSONToYDoc } from 'y-prosemirror';
import { supplementaryFixture, type SupplementarySlot } from './supplementary-continuity.js';
import assert from 'node:assert/strict';
import { assertConverged, canonicalDocumentShape } from './assertions.js';
import { call, exchangeUntilIdle, snapshot, tableFixture, withPeers } from './controller.js';
import {
    nativeRepairWrites,
    settledGeometryOf,
    type ScheduleOutcome,
    type SettledSetup,
} from './corpus.js';

export function supplementarySeed(slot: SupplementarySlot): string {
    const nodes = tableNodes({
        tableGroup: 'block',
        cellContent: 'block+',
        cellAttributes: {},
    });
    const schema = new Schema({
        nodes: basic.spec.nodes.append(
            slot.preset === 'prosemirror'
                ? nodes
                : {
                      table: { ...nodes.table, content: 'tableRow+' },
                      tableRow: {
                          ...nodes.table_row,
                          content: '(tableCell | tableHeader)*',
                      },
                      tableCell: nodes.table_cell,
                      tableHeader: nodes.table_header,
                  },
        ),
        marks: basic.spec.marks,
    });
    const authored = prosemirrorJSONToYDoc(
        schema,
        supplementaryFixture(slot.family, slot.preset),
        'prosemirror',
    );
    try {
        return Buffer.from(Y.encodeStateAsUpdate(authored)).toString('base64');
    } finally {
        authored.destroy();
    }
}

export async function withSupplementarySetup(
    slot: SupplementarySlot,
    body: (setup: SettledSetup) => Promise<void>,
): Promise<ScheduleOutcome> {
    let outcome: ScheduleOutcome | undefined;
    await withPeers(
        [slot.preset, ...slot.schedule.kinds],
        async (peers) => {
            const reference = peers[0]!;
            const participants = peers.slice(1, slot.schedule.participants + 1);
            const judge = peers.at(-1)!;
            const updateBase64 = supplementarySeed(slot);
            for (const peer of peers) await call(peer, 'applyUpdate', { updateBase64 });
            await exchangeUntilIdle(participants, slot.schedule.seed);
            await assertConverged(participants);
            const expected = canonicalDocumentShape(supplementaryFixture(slot.family, slot.preset));
            for (const peer of participants)
                assert.deepEqual(
                    canonicalDocumentShape((await snapshot(peer)).documentJson),
                    expected,
                    'TBL21 CONTINUITY original supplementary raw fixture',
                );
            const nativeAutonomousRepairWrites = await nativeRepairWrites(participants);
            assert.equal(
                nativeAutonomousRepairWrites,
                0,
                'TBL21 CONTINUITY supplementary seed has no native repair',
            );
            outcome = {
                peers: participants,
                geometry: await settledGeometryOf(participants, judge, slot.preset),
                nativeAutonomousRepairWrites,
                rawConvergence: { passed: true },
                evidence: { status: 'proven', actions: [], failures: [] },
            };
            await body({
                participants,
                judge,
                baseline: outcome,
                ...(slot.topology === 'native/native' ? { reference } : {}),
            });
        },
        tableFixture(slot.preset),
        slot.schedule.seed,
    );
    assert.ok(outcome);
    return outcome;
}
