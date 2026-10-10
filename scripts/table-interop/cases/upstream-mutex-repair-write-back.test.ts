import assert from 'node:assert/strict';
import { test } from 'node:test';
import {
    call,
    exchangeUntilIdle,
    flushDocumentEvents,
    seedFrom,
    snapshot,
    tableFixture,
    withPeers,
} from '../controller.js';
import type { PeerSnapshot } from '../controller.js';
import { isRecord } from '../peer-protocol.js';
import type { Peer } from '../peer-protocol.js';
import { TABLE_SCHEMA, cell, row, table, tableOf } from '../table-schema.js';

const FIRST_CELL_POSITION = 3;
const AT_LEAST_ONE_FIXTABLES_PASS = 1;
const CONCURRENT_TABLE_EDITS = 2;
const UNIFORM_ROW_WIDTHS = 1;
const REPAIRED_RAGGED_ROW_WIDTHS = [2, 2];
const NO_REPAIR_WRITE_REACHED_THE_CRDT = 0;
const NO_REPAIR_UPDATE_WAS_BROADCAST = 0;
const LOCAL_PATH_WRITES_THE_REPAIR_BACK = true;
const REMOTE_PATH_LOSES_THE_REPAIR = false;

const MUTEX_MECHANISM =
    'y-prosemirror 1.3.7 _typeChanged dispatches the remote-apply transaction inside this.mux(...); '
    + 'lib0/mutex suppresses re-entrancy, and the sync plugin update() hook is the only write-back '
    + 'path and itself writes through binding.mux(...), so it is skipped while that mutex is held';

function raggedTable(): Record<string, unknown> {
    return table([
        row([cell({ text: 'a' }), cell({ text: 'b' })]),
        row([cell({ text: 'c' })]),
    ]);
}

function rowWidths(tableJson: unknown): number[] {
    if (!isRecord(tableJson)) {
        throw new Error(`the peer document held ${JSON.stringify(tableJson)} where a table was expected`);
    }
    const rows = tableJson['content'];
    if (!Array.isArray(rows)) {
        throw new Error('the table node carried no rows array');
    }
    return rows.map((candidate) => {
        if (!isRecord(candidate) || !Array.isArray(candidate['content'])) {
            throw new Error('a table row carried no cells array');
        }
        return candidate['content'].length;
    });
}

function displayWidths(captured: PeerSnapshot): number[] {
    return rowWidths(tableOf(captured.displayJson));
}

function documentWidths(captured: PeerSnapshot): number[] {
    return rowWidths(tableOf(captured.documentJson));
}

function repairReachedTheCrdt(captured: PeerSnapshot): boolean {
    return JSON.stringify(documentWidths(captured)) === JSON.stringify(displayWidths(captured));
}

async function irregularityOf(
    peer: Peer,
    tableJson: unknown,
): Promise<unknown> {
    const projection = await call(peer, 'projectTable', { schema: TABLE_SCHEMA, table: tableJson });
    return projection['irregular'];
}

test(
    'TBL-10 a fixTables repair raised while y-prosemirror holds the lib0/mutex reaches the view and never the CRDT, '
        + 'while the identical repair on the local path is written back',
    async () => {
        await withPeers(['prosemirror', 'prosemirror', 'prosemirror'], async ([author, contributor, admitter]) => {
            await call(author, 'command', { type: 'insertNode', node: raggedTable() });
            const locallyRepaired = await snapshot(author);
            assert.ok(
                locallyRepaired.normalizationPassesAfterLastAction >= AT_LEAST_ONE_FIXTABLES_PASS,
                'the local ragged insert must provoke a fixTables repair, or the local counter-example proves nothing',
            );
            assert.deepEqual(
                displayWidths(locallyRepaired),
                REPAIRED_RAGGED_ROW_WIDTHS,
                'fixTables pads the short row in the view on the local path',
            );
            assert.deepEqual(
                documentWidths(locallyRepaired),
                REPAIRED_RAGGED_ROW_WIDTHS,
                'LOCAL COUNTER-EXAMPLE: outside a remote-apply window the sync plugin update() hook runs, '
                    + 'so the very same fixTables repair is written back into the CRDT; the wiring under test is sound',
            );
            assert.equal(
                await irregularityOf(author, tableOf(locallyRepaired.documentJson)),
                false,
                'the locally repaired table is regular in the CRDT itself, not only on screen',
            );

            await seedFrom(author, [contributor, admitter]);
            await exchangeUntilIdle([author, contributor, admitter]);
            for (const peer of [author, contributor, admitter]) {
                await flushDocumentEvents(peer);
            }

            await call(author, 'command', {
                type: 'tableCommand',
                name: 'addRowAfter',
                at: FIRST_CELL_POSITION,
            });
            await call(contributor, 'command', {
                type: 'tableCommand',
                name: 'addColumnAfter',
                at: FIRST_CELL_POSITION,
            });
            const concurrent = [
                ...(await flushDocumentEvents(author)),
                ...(await flushDocumentEvents(contributor)),
            ];
            assert.ok(
                concurrent.length >= CONCURRENT_TABLE_EDITS,
                'both contributors must produce a table edit, so their merge presents ragged geometry to the admitter',
            );

            for (const event of concurrent) {
                await call(admitter, 'applyUpdate', { updateBase64: event.bytesBase64 });
            }
            const admitted = await snapshot(admitter);

            assert.ok(
                admitted.normalizationPassesAfterLastAction >= AT_LEAST_ONE_FIXTABLES_PASS,
                'fixTables must append a repair transaction while the remote update is being admitted, '
                    + 'otherwise there is no repair for the mutex to swallow',
            );
            assert.equal(
                new Set(displayWidths(admitted)).size,
                UNIFORM_ROW_WIDTHS,
                `the fixTables repair reaches the view: display row widths ${displayWidths(admitted).join(', ')}`,
            );
            assert.ok(
                new Set(documentWidths(admitted)).size > UNIFORM_ROW_WIDTHS,
                `the CRDT keeps the unrepaired ragged merge: document row widths ${documentWidths(admitted).join(', ')}; ${MUTEX_MECHANISM}`,
            );
            assert.equal(
                await irregularityOf(admitter, tableOf(admitted.displayJson)),
                false,
                'the admitter displays regular geometry',
            );
            assert.equal(
                await irregularityOf(admitter, tableOf(admitted.documentJson)),
                true,
                `the admitter's own CRDT stays irregular behind that display; ${MUTEX_MECHANISM}`,
            );
            assert.equal(
                admitted.autonomousRepairWrites,
                NO_REPAIR_WRITE_REACHED_THE_CRDT,
                `no repair write reaches the Y.Doc at all during the remote window; ${MUTEX_MECHANISM}`,
            );
            assert.equal(
                (await call(admitter, 'drain', {}))['count'],
                NO_REPAIR_UPDATE_WAS_BROADCAST,
                `the swallowed repair is never broadcast either, so no peer can learn of it; ${MUTEX_MECHANISM}`,
            );

            assert.deepEqual(
                {
                    local: repairReachedTheCrdt(locallyRepaired),
                    remote: repairReachedTheCrdt(admitted),
                },
                {
                    local: LOCAL_PATH_WRITES_THE_REPAIR_BACK,
                    remote: REMOTE_PATH_LOSES_THE_REPAIR,
                },
                'one plugin stack, two paths: the local path writes its fixTables repair back and the remote path loses it. '
                    + `The only difference between them is that ${MUTEX_MECHANISM}`,
            );
        }, tableFixture('prosemirror'));
    },
);
