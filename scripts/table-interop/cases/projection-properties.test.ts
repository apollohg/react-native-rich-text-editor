import assert from 'node:assert/strict';
import test from 'node:test';
import {
    call,
    exchangeUntilIdle,
    flushDocumentEvents,
    seedFrom,
    snapshot,
    tableFixture,
    withPeers,
} from '../controller.js';
import type { Peer } from '../peer-protocol.js';
import { assertEffectivePresentation, observeNativePresentation } from '../presentation-semantics.js';
import {
    convergenceScalarsPassed,
    createConvergenceReport,
    describeConvergenceReport,
    lostSourceCells,
    recordSettledRun,
    recordSourceCellCoverage,
    TOPOLOGY_NATIVE_NATIVE,
    GEOMETRY_ADMITTED,
} from '../convergence-report.js';
import {
    TABLE_SCHEMA,
    cell,
    cellAnchors,
    cellSourceAnchors,
    row,
    table,
    tableOf,
} from '../table-schema.js';

const DOC_NODE = 'doc';
const TABLE_START = 0;
const NO_LOSSES = 0;
const ONE_LOSS = 1;
const NO_REPAIR_WRITES = 0;
const NO_PENDING_UPDATES = 0;
const NO_NORMALIZATION_PASSES = 0;
const PROJECTION_REPEATS = 3;
const OVERLONG_ROWSPAN = 9;
const FIRST_SLOT = 0;
const ONE_REPAIR_WRITE = 1;

type ProjectionCase = {
    readonly name: string;
    readonly table: Record<string, unknown>;
};

const PROJECTION_CASES: ProjectionCase[] = [
    {
        name: 'a regular two by two grid',
        table: table([
            row([cell({ text: 'a' }), cell({ text: 'b' })]),
            row([cell({ text: 'c' }), cell({ text: 'd' })]),
        ]),
    },
    {
        name: 'a ragged grid with a missing slot',
        table: table([
            row([cell({ text: 'a' }), cell({ text: 'b' }), cell({ text: 'c' })]),
            row([cell({ text: 'd' })]),
        ]),
    },
    {
        name: 'a grid with a spanning cell',
        table: table([
            row([cell({ colspan: 2, text: 'wide' }), cell({ text: 'b' })]),
            row([cell({ text: 'c' }), cell({ text: 'd' }), cell({ text: 'e' })]),
        ]),
    },
    {
        name: 'a grid with a rowspan longer than the table',
        table: table([
            row([cell({ rowspan: OVERLONG_ROWSPAN, text: 'tall' }), cell({ text: 'b' })]),
            row([cell({ text: 'c' })]),
        ]),
    },
    {
        name: 'a grid with header cells',
        table: table([
            row([cell({ header: true, text: 'h0' }), cell({ header: true, text: 'h1' })]),
            row([cell({ text: 'a' }), cell({ text: 'b' })]),
        ]),
    },
];

test('TBL-11 synthetic header records retain schema defaults and never own source slots', async () => {
    await withPeers(['rust'] as const, async ([native]) => {
        const schema = structuredClone(TABLE_SCHEMA);
        const header = (schema.nodes as { name: string; attrs: Record<string, unknown> }[]).find((node) => node.name === 'table_header')!;
        Object.assign(header.attrs!, { background: { default: 'ivory' } });
        const projected = await call(native, 'projectTable', {
            schema,
            table: table([row([cell({ header: true, text: 'same' })]), row([cell({ text: 'same' }), cell({ text: 'same' })])]),
        });
        assert.equal(projected.compatibilityDiagnostic, null);
        const gaps = projected.synthetic as { row: number; column: number; node: { type: string; attrs: Record<string, unknown> } }[];
        assert.equal(gaps.length, 1);
        assert.equal(gaps[0]!.row, 0);
        assert.equal(gaps[0]!.column, 0);
        assert.equal(gaps[0]!.node.type, 'table_header');
        assert.equal(gaps[0]!.node.attrs.background, 'ivory');
        assert.equal((projected.slots as unknown[])[0], null);
    }, tableFixture('prosemirror'));
});

test('TBL-11 overlap fallback retains content and exposes native equality separately', async () => {
    await withPeers(['rust'] as const, async ([native]) => {
        const fixture = table([
            row([cell({ text: 'same' }), cell({ rowspan: 2, text: 'same' })]),
            row([cell({ colspan: 2, rowspan: 3, text: '😀same' })]),
            row([]),
        ]);
        await call(native, 'command', { type: 'insertContentJson', json: { type: 'doc', content: [fixture] } });
        const before = await snapshot(native);
        const projected = await call(native, 'projectTable', { schema: TABLE_SCHEMA, table: fixture });
        assert.equal(projected.compatibilityDiagnostic, 'overlapping-reference-cells');
        assert.equal(new Set((projected.slots as unknown[]).filter((slot) => slot !== null)).size, 3);
        const observed = await observeNativePresentation(native);
        assert.deepEqual(observed.tables[0]!.overlap, { kind: 'native-fallback', reason: 'overlapping-reference-cells' });
        assert.deepEqual(assertEffectivePresentation(observed, await observeNativePresentation(native)).tables,
            [{ source: '0', kind: 'overlap-fallback', evidence: 'native-equality' }]);
        assert.deepEqual((await snapshot(native)).documentJson, before.documentJson);
    }, tableFixture('prosemirror'));
});

async function projectionOf(peer: Peer, tableJson: Record<string, unknown>): Promise<{
    rows: number;
    columns: number;
    slots: (number | null)[];
    irregular: boolean;
}> {
    const projection = await call(peer, 'projectTable', {
        schema: TABLE_SCHEMA,
        table: tableJson,
    });
    const rows = projection['rows'];
    const columns = projection['columns'];
    const slots = projection['slots'];
    const irregular = projection['irregular'];
    if (typeof rows !== 'number' || typeof columns !== 'number') {
        throw new Error(`the projection reported a non-numeric extent ${JSON.stringify(projection)}`);
    }
    if (!Array.isArray(slots)) {
        throw new Error(`the projection reported no slot array ${JSON.stringify(projection)}`);
    }
    if (typeof irregular !== 'boolean') {
        throw new Error(`the projection reported a non-boolean irregular flag`);
    }
    return { rows, columns, slots: slots as (number | null)[], irregular };
}

function slotIndicesByAnchor(slots: (number | null)[]): Map<number, number[]> {
    const byAnchor = new Map<number, number[]>();
    for (const [index, anchor] of slots.entries()) {
        if (anchor === null) {
            continue;
        }
        const held = byAnchor.get(anchor) ?? [];
        held.push(index);
        byAnchor.set(anchor, held);
    }
    return byAnchor;
}

function assertRectangular(
    name: string,
    anchor: number,
    indices: number[],
    columns: number,
): void {
    const rows = indices.map((index) => Math.floor(index / columns));
    const cols = indices.map((index) => index % columns);
    const minRow = Math.min(...rows);
    const maxRow = Math.max(...rows);
    const minColumn = Math.min(...cols);
    const maxColumn = Math.max(...cols);
    const area = (maxRow - minRow + 1) * (maxColumn - minColumn + 1);
    assert.equal(
        indices.length,
        area,
        `${name}: source cell ${anchor} covers ${indices.length} slots inside a ${area} slot `
            + `rectangle ${JSON.stringify({ minRow, maxRow, minColumn, maxColumn })}`,
    );
}

test('TBL-11 projection is deterministic, finitely bounded, rectangular and loss free', async () => {
    const report = createConvergenceReport();
    await withPeers(['rust', 'rust'] as const, async ([native]) => {
        for (const projectionCase of PROJECTION_CASES) {
            const first = await projectionOf(native, projectionCase.table);
            for (let repeat = 1; repeat < PROJECTION_REPEATS; repeat += 1) {
                assert.deepEqual(
                    await projectionOf(native, projectionCase.table),
                    first,
                    `${projectionCase.name} projected differently on repeat ${repeat}`,
                );
            }

            assert.equal(
                first.slots.length,
                first.rows * first.columns,
                `${projectionCase.name} reported ${first.slots.length} slots for a `
                    + `${first.rows} by ${first.columns} grid`,
            );
            assert.ok(
                first.rows > FIRST_SLOT && first.columns > FIRST_SLOT,
                `${projectionCase.name} projected an empty grid`,
            );

            const byAnchor = slotIndicesByAnchor(first.slots);
            for (const [anchor, indices] of byAnchor) {
                assertRectangular(projectionCase.name, anchor, indices, first.columns);
            }

            const sourceAnchors = cellSourceAnchors(projectionCase.table, TABLE_START);
            const coverage = {
                name: projectionCase.name,
                sourceCellAnchors: sourceAnchors,
                projectedSlots: first.slots,
            };
            recordSourceCellCoverage(report, coverage);
            assert.deepEqual(
                lostSourceCells(coverage),
                [],
                `${projectionCase.name} lost source cells from its projection`,
            );
            assert.equal(
                byAnchor.size,
                sourceAnchors.length,
                `${projectionCase.name} projected ${byAnchor.size} display regions for `
                    + `${sourceAnchors.length} source cells`,
            );
        }
        assert.equal(
            report.unexpectedSourceCellLosses,
            NO_LOSSES,
            describeConvergenceReport(report),
        );
    }, tableFixture('prosemirror'));
});

test('TBL-11 unexpectedSourceCellLosses counts a source cell the projection did not place', async () => {
    await withPeers(['rust', 'rust'] as const, async ([author, replica]) => {
        const sourceCellAnchors = [2, 7, 14];
        const projectedSlots = [2, 7, null, null];
        assert.deepEqual(
            lostSourceCells({ name: 'a dropped source cell', sourceCellAnchors, projectedSlots }),
            [14],
        );
        const report = createConvergenceReport();
        recordSettledRun(report, {
            name: 'native/native settled run projecting the faulted table',
            peers: [author, replica],
            topology: TOPOLOGY_NATIVE_NATIVE,
            geometry: { kind: GEOMETRY_ADMITTED, irregular: true },
        });
        recordSourceCellCoverage(report, {
            name: 'a dropped source cell',
            sourceCellAnchors,
            projectedSlots,
        });
        assert.equal(report.unexpectedSourceCellLosses, ONE_LOSS, describeConvergenceReport(report));
        assert.equal(convergenceScalarsPassed(report), false, describeConvergenceReport(report));
    }, tableFixture('prosemirror'));
});

test('TBL-11 projecting and snapshotting a table writes nothing to the document', async () => {
    await withPeers(['rust', 'rust'] as const, async ([native]) => {
        const seeded = PROJECTION_CASES[1];
        assert.ok(seeded !== undefined, 'the corpus carries the ragged case');
        await call(native, 'command', {
            type: 'insertContentJson',
            json: { type: DOC_NODE, content: [seeded.table] },
        });
        await flushDocumentEvents(native);
        const before = await snapshot(native);

        for (let repeat = FIRST_SLOT; repeat < PROJECTION_REPEATS; repeat += 1) {
            await projectionOf(native, tableOf(before.documentJson) as Record<string, unknown>);
            await snapshot(native);
        }

        const after = await snapshot(native);
        assert.equal(after.documentRevision, before.documentRevision);
        assert.equal(after.autonomousRepairWrites, NO_REPAIR_WRITES);
        assert.equal(after.normalizationPassesAfterLastAction, NO_NORMALIZATION_PASSES);
        assert.deepEqual(after.documentJson, before.documentJson);
        assert.equal(
            (await call(native, 'drain', {}))['count'],
            NO_PENDING_UPDATES,
            'projection and snapshot must emit no document update',
        );
    }, tableFixture('prosemirror'));
});

test('TBL-11 undo and remote admission add no native normalization writes', async () => {
    await withPeers(['rust', 'rust'] as const, async ([author, admitting]) => {
        const ragged = PROJECTION_CASES[1];
        assert.ok(ragged !== undefined, 'the corpus carries the ragged case');
        await call(author, 'command', {
            type: 'insertContentJson',
            json: { type: DOC_NODE, content: [ragged.table] },
        });
        await seedFrom(author, [admitting]);
        await exchangeUntilIdle([author, admitting]);
        const admittedBefore = await snapshot(admitting);
        assert.equal(admittedBefore.autonomousRepairWrites, NO_REPAIR_WRITES);

        const [firstAnchor] = cellAnchors(ragged.table, TABLE_START);
        assert.ok(firstAnchor !== undefined, 'the ragged fixture exposes a cell anchor');
        await call(author, 'command', {
            type: 'addTableRow',
            side: 'after',
            at: firstAnchor,
        });
        await exchangeUntilIdle([author, admitting]);
        assert.equal(
            (await snapshot(admitting)).autonomousRepairWrites,
            NO_REPAIR_WRITES,
            'admitting a remote structural update writes no native repair',
        );

        await call(author, 'undo', {});
        await exchangeUntilIdle([author, admitting]);
        const afterUndo = await snapshot(admitting);
        assert.equal(
            afterUndo.autonomousRepairWrites,
            NO_REPAIR_WRITES,
            'admitting a remote undo writes no native repair',
        );
        assert.deepEqual(afterUndo.documentJson, (await snapshot(author)).documentJson);
    }, tableFixture('prosemirror'));
});

test('TBL-10 the repair canary drives nativeAutonomousRepairWrites to exactly one', async () => {
    await withPeers(['rust', 'rust'] as const, async ([native]) => {
        const ragged = PROJECTION_CASES[1];
        assert.ok(ragged !== undefined, 'the corpus carries the ragged case');
        await call(native, 'command', {
            type: 'insertContentJson',
            json: { type: DOC_NODE, content: [ragged.table] },
        });
        await flushDocumentEvents(native);
        const before = await snapshot(native);
        assert.equal(before.autonomousRepairWrites, NO_REPAIR_WRITES);
        assert.equal(
            (await projectionOf(native, tableOf(before.documentJson) as Record<string, unknown>))
                .irregular,
            true,
            'the canary needs an irregular table to repair',
        );

        await call(native, 'repairTableDuringRemoteWindow', {});

        const after = await snapshot(native);
        assert.equal(
            after.autonomousRepairWrites,
            ONE_REPAIR_WRITE,
            'a repair committed inside a remote window counts exactly once',
        );
        assert.equal(
            (await projectionOf(native, tableOf(after.documentJson) as Record<string, unknown>))
                .irregular,
            false,
            'the canary repaired the table it was pointed at',
        );
    }, tableFixture('prosemirror'));
});
