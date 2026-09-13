import assert from 'node:assert/strict';
import test from 'node:test';
import * as Y from 'yjs';
import {
    EMPTY_STATE_VECTOR_BASE64,
    PeerError,
    call,
    exchangeUntilIdle,
    flushDocumentEvents,
    peerKindOf,
    seedFrom,
    snapshot,
    tableFixture,
    withPeers,
} from '../controller.js';
import type { Peer } from '../peer-protocol.js';
import {
    ADMISSIBLE_IRREGULAR_INPUT,
    GEOMETRY_ADMITTED,
    GEOMETRY_PROJECTION_FAILED,
    GEOMETRY_RAW_JSON_DISAGREEMENT,
    TOPOLOGY_NATIVE_NATIVE,
    TOPOLOGY_NATIVE_TWO_WEB,
    TOPOLOGY_NATIVE_WEB,
    TOPOLOGY_TWO_WEB_CONTROL,
    UNSAFE_INPUT,
    CONVERGENCE_TOPOLOGIES,
    convergenceScalarsPassed,
    createConvergenceReport,
    describeConvergenceReport,
    recordAdmission,
    recordSettledRun,
} from '../convergence-report.js';
import type {
    AdmissionClassification,
    ConvergenceReport,
    ConvergenceTopology,
    SettledGeometry,
    SettledRun,
} from '../convergence-report.js';
import {
    CELL_NODE,
    PARAGRAPH_NODE,
    ROW_NODE,
    TABLE_NODE,
    TABLE_SCHEMA,
    cell,
    cellAnchors,
    row,
    table,
    tableOf,
} from '../table-schema.js';

const DOC_NODE = 'doc';
const COLLABORATION_FRAGMENT_NAME = 'prosemirror';
const NATIVE_PEER_KIND = 'rust';
const TABLE_START = 0;
const FIRST_CELL_POSITION = 3;
const NO_LOOPS = 0;
const NO_FAILURES = 0;
const ONE_FAILURE = 1;
const ONE_SETTLED_RUN = 1;
const UNSETTLED_WEB_REPAIRS = 2;
const TEXT_NODE = 'text';
const OVERLONG_ROWSPAN = 100_000_000;
const COLLIDING_ROWSPAN = 2;
const ZERO_COLSPAN = 0;
const NEGATIVE_COLSPAN = -3;
const NON_NUMERIC_COLSPAN = 'two';
const FRACTIONAL_COLSPAN = 1.5;
const BUDGET_BUSTING_COLSPAN = 4_000_000_000;
const NON_NUMERIC_COLWIDTH = 'wide';
const UPPER_COLUMN_WIDTH = 100;
const LOWER_COLUMN_WIDTH = 180;
const FIRST_CHILD = 0;
const SECOND_CHILD = 1;
const ONE_CHILD = 1;

const suiteReport = createConvergenceReport();
const chargedTopologies = new Set<ConvergenceTopology>();

function chargeRun(local: ConvergenceReport, run: SettledRun): void {
    recordSettledRun(local, run);
    recordSettledRun(suiteReport, run);
    chargedTopologies.add(run.topology);
}

function webCell(text: string): Record<string, unknown> {
    return {
        type: CELL_NODE,
        attrs: { colspan: 1, rowspan: 1, colwidth: null },
        content: [{ type: PARAGRAPH_NODE, content: [{ type: TEXT_NODE, text }] }],
    };
}

function webRow(cells: Record<string, unknown>[]): Record<string, unknown> {
    return { type: ROW_NODE, content: cells };
}

function regularTable(): Record<string, unknown> {
    return {
        type: TABLE_NODE,
        content: [
            webRow([webCell('a'), webCell('b')]),
            webRow([webCell('c'), webCell('d')]),
        ],
    };
}

function raggedTable(): Record<string, unknown> {
    return table([
        row([cell({ text: 'a' }), cell({ text: 'b' }), cell({ text: 'c' })]),
        row([cell({ text: 'd' })]),
    ]);
}

async function projectedGeometry(judge: Peer, tableJson: unknown): Promise<SettledGeometry> {
    if (peerKindOf(judge) !== NATIVE_PEER_KIND) {
        throw new Error(
            `TBL-11 admissibility is the Rust projectTable result; a ${peerKindOf(judge)} peer `
                + 'cannot judge a settled table',
        );
    }
    try {
        const projection = await call(judge, 'projectTable', {
            schema: TABLE_SCHEMA,
            table: tableJson,
        });
        return { kind: GEOMETRY_ADMITTED, irregular: projection['irregular'] === true };
    } catch (error) {
        if (error instanceof PeerError) {
            return {
                kind: GEOMETRY_PROJECTION_FAILED,
                code: error.code,
                message: error.message,
            };
        }
        throw error;
    }
}

async function settledGeometryOf(peers: Peer[], judge: Peer): Promise<SettledGeometry> {
    const tables: string[] = [];
    for (const peer of peers) {
        tables.push(JSON.stringify(tableOf((await snapshot(peer)).documentJson)));
    }
    const [first] = tables;
    if (first === undefined) {
        throw new Error('a settled run compared an empty peer set');
    }
    for (const [index, candidate] of tables.entries()) {
        if (candidate !== first) {
            return {
                kind: GEOMETRY_RAW_JSON_DISAGREEMENT,
                detail: `peer ${index} holds ${candidate}; peer 0 holds ${first}`,
            };
        }
    }
    return projectedGeometry(judge, JSON.parse(first));
}

async function assertJudgeStayedOutside(judge: Peer): Promise<void> {
    const judged = await snapshot(judge);
    assert.equal(
        judged.mounted,
        false,
        'the control topology judge must never join the room it judges',
    );
    assert.equal(judged.documentJson, null);
}

async function webRepairWrites(peers: Peer[]): Promise<number> {
    let total = NO_LOOPS;
    for (const peer of peers) {
        total += (await snapshot(peer)).autonomousRepairWrites;
    }
    return total;
}

test('TBL-21 an admitted irregular settled table charges neither settled-geometry scalar', () => {
    const report = createConvergenceReport();
    recordSettledRun(report, {
        name: 'web plugin settles irregular',
        topology: TOPOLOGY_NATIVE_TWO_WEB,
        webControlLoops: NO_LOOPS,
        geometry: { kind: GEOMETRY_ADMITTED, irregular: true },
    });
    assert.equal(report.invalidSettledNativeTables, NO_FAILURES);
    assert.equal(report.invalidSettledWebControlTables, NO_FAILURES);
    assert.equal(report.settledRuns, ONE_SETTLED_RUN);
    assert.ok(convergenceScalarsPassed(report), describeConvergenceReport(report));
});

test('TBL-21 the settled-geometry scalars route by topology class', () => {
    const nativeReport = createConvergenceReport();
    recordSettledRun(nativeReport, {
        name: 'native integration fault',
        topology: TOPOLOGY_NATIVE_TWO_WEB,
        webControlLoops: NO_LOOPS,
        geometry: { kind: GEOMETRY_PROJECTION_FAILED, code: 'TABLE_PROJECTION_FAILED', message: 'x' },
    });
    assert.equal(nativeReport.invalidSettledNativeTables, ONE_FAILURE);
    assert.equal(nativeReport.invalidSettledWebControlTables, NO_FAILURES);
    assert.equal(convergenceScalarsPassed(nativeReport), false);

    const controlReport = createConvergenceReport();
    recordSettledRun(controlReport, {
        name: 'upstream loop',
        topology: TOPOLOGY_TWO_WEB_CONTROL,
        webControlLoops: NO_LOOPS,
        geometry: { kind: GEOMETRY_PROJECTION_FAILED, code: 'TABLE_PROJECTION_FAILED', message: 'x' },
    });
    assert.equal(controlReport.invalidSettledNativeTables, NO_FAILURES);
    assert.equal(controlReport.invalidSettledWebControlTables, ONE_FAILURE);
    assert.equal(convergenceScalarsPassed(controlReport), false);
});

test('TBL-21 an invalid geometry in a still-repairing web run charges no settled scalar', () => {
    const report = createConvergenceReport();
    recordSettledRun(report, {
        name: 'web plugin still repairing',
        topology: TOPOLOGY_TWO_WEB_CONTROL,
        webControlLoops: UNSETTLED_WEB_REPAIRS,
        geometry: { kind: GEOMETRY_PROJECTION_FAILED, code: 'TABLE_PROJECTION_FAILED', message: 'x' },
    });
    assert.equal(report.invalidSettledNativeTables, NO_FAILURES);
    assert.equal(report.invalidSettledWebControlTables, NO_FAILURES);
    assert.equal(report.webControlLoops, UNSETTLED_WEB_REPAIRS);
    assert.equal(
        convergenceScalarsPassed(report),
        false,
        'an unsettled web plugin still fails the core gate through webControlLoops',
    );
    assert.match(report.findings.join('\n'), /still wrote 2 repair transactions/);
});

test('TBL-21 a drained native/native run settles on admissible geometry', async () => {
    const report = createConvergenceReport();
    await withPeers(['rust', 'rust'] as const, async ([source, replica]) => {
        await call(source, 'command', {
            type: 'insertContentJson',
            json: { type: DOC_NODE, content: [raggedTable()] },
        });
        await seedFrom(source, [replica]);
        await exchangeUntilIdle([source, replica]);
        const settled = await settledGeometryOf([source, replica], source);
        chargeRun(report, {
            name: 'native/native ragged table, drained',
            topology: TOPOLOGY_NATIVE_NATIVE,
            webControlLoops: NO_LOOPS,
            geometry: settled,
        });
        assert.equal(settled.kind, GEOMETRY_ADMITTED, describeConvergenceReport(report));
        assert.equal(
            (await snapshot(replica)).autonomousRepairWrites,
            NO_FAILURES,
            'a native replica admits ragged geometry without repairing it',
        );
        assert.ok(convergenceScalarsPassed(report), describeConvergenceReport(report));
    }, tableFixture('prosemirror'));
});

test('TBL-21 a drained native/web run settles on admissible geometry', async () => {
    const report = createConvergenceReport();
    await withPeers(['prosemirror', 'rust'] as const, async ([web, native]) => {
        await seedFrom(web, [native]);
        await exchangeUntilIdle([web, native]);
        const before = await webRepairWrites([web]);

        await call(web, 'command', { type: 'insertNode', node: regularTable() });
        await call(web, 'command', {
            type: 'tableCommand',
            name: 'addRowAfter',
            at: FIRST_CELL_POSITION,
        });
        await exchangeUntilIdle([web, native]);

        const settled = await settledGeometryOf([web, native], native);
        chargeRun(report, {
            name: 'native/web structural edit, drained',
            topology: TOPOLOGY_NATIVE_WEB,
            webControlLoops: (await webRepairWrites([web])) - before,
            geometry: settled,
        });
        assert.equal(settled.kind, GEOMETRY_ADMITTED, describeConvergenceReport(report));
        assert.equal(
            (await snapshot(native)).autonomousRepairWrites,
            NO_FAILURES,
            'native carries the web geometry without writing a repair',
        );
        assert.ok(convergenceScalarsPassed(report), describeConvergenceReport(report));
    }, tableFixture('prosemirror'));
});

test('TBL-21 a drained native/two-web run with reordered native delivery stays admissible', async () => {
    const report = createConvergenceReport();
    await withPeers(
        ['prosemirror', 'prosemirror', 'rust'] as const,
        async ([first, second, native]) => {
            await call(first, 'command', { type: 'insertNode', node: regularTable() });
            await seedFrom(first, [second, native]);
            await exchangeUntilIdle([first, second, native]);
            const before = await webRepairWrites([first, second]);

            await call(first, 'command', {
                type: 'tableCommand',
                name: 'addRowAfter',
                at: FIRST_CELL_POSITION,
            });
            await call(second, 'command', {
                type: 'tableCommand',
                name: 'addColumnAfter',
                at: FIRST_CELL_POSITION,
            });
            const fromFirst = await flushDocumentEvents(first);
            const fromSecond = await flushDocumentEvents(second);
            assert.ok(
                fromFirst.length > 0 && fromSecond.length > 0,
                'both web peers produced concurrent table edits',
            );
            for (const event of fromFirst) {
                await call(second, 'applyUpdate', { updateBase64: event.bytesBase64 });
            }
            for (const event of fromSecond) {
                await call(first, 'applyUpdate', { updateBase64: event.bytesBase64 });
            }
            for (const event of [...fromFirst, ...fromSecond].reverse()) {
                await call(native, 'applyUpdate', { updateBase64: event.bytesBase64 });
            }
            await exchangeUntilIdle([first, second, native]);

            const settled = await settledGeometryOf([first, second, native], native);
            chargeRun(report, {
                name: 'native/two-web concurrent row and column, reversed native delivery, drained',
                topology: TOPOLOGY_NATIVE_TWO_WEB,
                webControlLoops: (await webRepairWrites([first, second])) - before,
                geometry: settled,
            });
            assert.equal(settled.kind, GEOMETRY_ADMITTED, describeConvergenceReport(report));
            assert.equal(
                settled.irregular,
                true,
                'the concurrent row and column insertion settles on irregular geometry',
            );
            assert.equal(
                (await snapshot(native)).autonomousRepairWrites,
                NO_FAILURES,
                'native carries the web-settled geometry without writing a repair',
            );
            assert.ok(convergenceScalarsPassed(report), describeConvergenceReport(report));
        },
        tableFixture('prosemirror'),
    );
});

test('TBL-21 a drained two-web control is judged by the Rust projection, not by its own peers', async () => {
    const report = createConvergenceReport();
    await withPeers(
        ['prosemirror', 'prosemirror', 'rust'] as const,
        async ([first, second, judge]) => {
            await call(first, 'command', { type: 'insertNode', node: regularTable() });
            await seedFrom(first, [second]);
            await exchangeUntilIdle([first, second]);
            const before = await webRepairWrites([first, second]);

            await call(first, 'command', {
                type: 'tableCommand',
                name: 'addRowAfter',
                at: FIRST_CELL_POSITION,
            });
            await exchangeUntilIdle([first, second]);
            await assertJudgeStayedOutside(judge);

            const settled = await settledGeometryOf([first, second], judge);
            chargeRun(report, {
                name: 'two-web control structural edit, drained',
                topology: TOPOLOGY_TWO_WEB_CONTROL,
                webControlLoops: (await webRepairWrites([first, second])) - before,
                geometry: settled,
            });
            assert.equal(settled.kind, GEOMETRY_ADMITTED, describeConvergenceReport(report));
            assert.ok(convergenceScalarsPassed(report), describeConvergenceReport(report));
        },
        tableFixture('prosemirror'),
    );
});

test('TBL-21 a drained two-web control settling concurrent overlapping merges stays admissible', async () => {
    const report = createConvergenceReport();
    await withPeers(
        ['prosemirror', 'prosemirror', 'rust'] as const,
        async ([first, second, judge]) => {
            await call(first, 'command', { type: 'insertNode', node: regularTable() });
            await seedFrom(first, [second]);
            await exchangeUntilIdle([first, second]);
            const anchors = cellAnchors(regularTable(), TABLE_START);
            const [topLeft, topRight, bottomLeft] = anchors;
            assert.ok(
                topLeft !== undefined && topRight !== undefined && bottomLeft !== undefined,
                'the two by two fixture exposes three distinct cell anchors',
            );
            const before = await webRepairWrites([first, second]);

            await call(first, 'command', {
                type: 'tableCommand',
                name: 'mergeCells',
                at: topLeft,
                head: topRight,
            });
            await call(second, 'command', {
                type: 'tableCommand',
                name: 'mergeCells',
                at: bottomLeft,
                head: topLeft,
            });
            await exchangeUntilIdle([first, second]);
            await exchangeUntilIdle([first, second]);
            await assertJudgeStayedOutside(judge);

            const settled = await settledGeometryOf([first, second], judge);
            chargeRun(report, {
                name: 'two-web control concurrent overlapping merges, drained',
                topology: TOPOLOGY_TWO_WEB_CONTROL,
                webControlLoops: (await webRepairWrites([first, second])) - before,
                geometry: settled,
            });
            assert.equal(
                report.invalidSettledWebControlTables,
                NO_FAILURES,
                `pinned prosemirror-tables settled the adversarial merge pair as `
                    + describeConvergenceReport(report),
            );
            assert.equal(report.invalidSettledNativeTables, NO_FAILURES);
        },
        tableFixture('prosemirror'),
    );
});

test('TBL-21 an undrained run is reported as raw JSON disagreement, not as settled geometry', async () => {
    await withPeers(['rust', 'rust'] as const, async ([source, starved]) => {
        await call(source, 'command', {
            type: 'insertContentJson',
            json: { type: DOC_NODE, content: [raggedTable()] },
        });
        await seedFrom(source, [starved]);
        await exchangeUntilIdle([source, starved]);
        const [firstAnchor] = cellAnchors(raggedTable(), TABLE_START);
        assert.ok(firstAnchor !== undefined, 'the ragged fixture exposes a cell anchor');

        await call(source, 'command', {
            type: 'addTableRow',
            side: 'after',
            at: firstAnchor,
        });
        const withheld = await flushDocumentEvents(source);
        assert.ok(withheld.length > 0, 'the structural edit produced a delivery to withhold');

        const undrained = await settledGeometryOf([source, starved], source);
        assert.equal(
            undrained.kind,
            GEOMETRY_RAW_JSON_DISAGREEMENT,
            'an undelivered structural edit leaves the replicas holding different raw tables',
        );
    }, tableFixture('prosemirror'));
});

type HostileMutation = {
    readonly name: string;
    readonly classification: AdmissionClassification;
    readonly apply: (
        cellElement: Y.XmlElement,
        rowElement: Y.XmlElement,
        tableElement: Y.XmlElement,
    ) => void;
};

const HOSTILE_MUTATIONS: HostileMutation[] = [
    {
        name: 'colspan below the span domain',
        classification: UNSAFE_INPUT,
        apply: (cellElement) => cellElement.setAttribute('colspan', ZERO_COLSPAN as never),
    },
    {
        name: 'negative colspan',
        classification: UNSAFE_INPUT,
        apply: (cellElement) => cellElement.setAttribute('colspan', NEGATIVE_COLSPAN as never),
    },
    {
        name: 'colspan of the wrong attribute type',
        classification: UNSAFE_INPUT,
        apply: (cellElement) => cellElement.setAttribute('colspan', NON_NUMERIC_COLSPAN as never),
    },
    {
        name: 'fractional colspan',
        classification: UNSAFE_INPUT,
        apply: (cellElement) => cellElement.setAttribute('colspan', FRACTIONAL_COLSPAN as never),
    },
    {
        name: 'colspan beyond the update budget',
        classification: UNSAFE_INPUT,
        apply: (cellElement) => cellElement.setAttribute('colspan', BUDGET_BUSTING_COLSPAN as never),
    },
    {
        name: 'colwidth of the wrong attribute type',
        classification: UNSAFE_INPUT,
        apply: (cellElement) => cellElement.setAttribute('colwidth', NON_NUMERIC_COLWIDTH as never),
    },
    {
        name: 'a paragraph where a cell belongs',
        classification: UNSAFE_INPUT,
        apply: (_cellElement, rowElement) => rowElement.insert(0, [new Y.XmlElement(PARAGRAPH_NODE)]),
    },
    {
        name: 'loose text where a cell belongs',
        classification: UNSAFE_INPUT,
        apply: (_cellElement, rowElement) => rowElement.insert(0, [new Y.XmlText('loose')]),
    },
    {
        name: 'an overlong rowspan',
        classification: ADMISSIBLE_IRREGULAR_INPUT,
        apply: (cellElement) => cellElement.setAttribute('rowspan', OVERLONG_ROWSPAN as never),
    },
    {
        name: 'a missing slot',
        classification: ADMISSIBLE_IRREGULAR_INPUT,
        apply: (_cellElement, rowElement) => rowElement.delete(SECOND_CHILD, ONE_CHILD),
    },
    {
        name: 'a span collision',
        classification: ADMISSIBLE_IRREGULAR_INPUT,
        apply: (cellElement) => cellElement.setAttribute('rowspan', COLLIDING_ROWSPAN as never),
    },
    {
        name: 'a width disagreement between rows',
        classification: ADMISSIBLE_IRREGULAR_INPUT,
        apply: (cellElement, _rowElement, tableElement) => {
            const lowerRow = tableElement.toArray()[SECOND_CHILD];
            if (!(lowerRow instanceof Y.XmlElement)) {
                throw new Error('the width fixture needs a second row');
            }
            const lowerCell = lowerRow.toArray()[FIRST_CHILD];
            if (!(lowerCell instanceof Y.XmlElement)) {
                throw new Error('the width fixture needs a second row cell');
            }
            cellElement.setAttribute('colwidth', [UPPER_COLUMN_WIDTH] as never);
            lowerCell.setAttribute('colwidth', [LOWER_COLUMN_WIDTH] as never);
        },
    },
    {
        name: 'a table with no rows',
        classification: ADMISSIBLE_IRREGULAR_INPUT,
        apply: (_cellElement, _rowElement, tableElement) =>
            tableElement.delete(FIRST_CHILD, tableElement.length),
    },
];

async function observeAdmission(
    mutation: HostileMutation,
): Promise<{ admitted: boolean; detail: string }> {
    let observed: { admitted: boolean; detail: string } | null = null;
    await withPeers(['rust', 'rust'] as const, async ([author, target]) => {
        await call(author, 'command', {
            type: 'insertContentJson',
            json: { type: DOC_NODE, content: [regularTable()] },
        });
        const diff = await call(author, 'stateDiff', {
            stateVectorBase64: EMPTY_STATE_VECTOR_BASE64,
        });
        const seed = diff['updateBase64'];
        if (typeof seed !== 'string') {
            throw new Error('the authoring peer produced no seed update');
        }
        await call(target, 'applyUpdate', { updateBase64: seed });

        const mirror = new Y.Doc();
        Y.applyUpdate(mirror, Buffer.from(seed, 'base64'));
        const fragment = mirror.getXmlFragment(COLLABORATION_FRAGMENT_NAME);
        const tableElement = fragment
            .toArray()
            .find((node) => node instanceof Y.XmlElement && node.nodeName === TABLE_NODE);
        if (!(tableElement instanceof Y.XmlElement)) {
            throw new Error('the seeded document carried no table element');
        }
        const rowElement = tableElement.toArray()[FIRST_CHILD];
        if (!(rowElement instanceof Y.XmlElement)) {
            throw new Error('the seeded table carried no row element');
        }
        const cellElement = rowElement.toArray()[FIRST_CHILD];
        if (!(cellElement instanceof Y.XmlElement)) {
            throw new Error('the seeded row carried no cell element');
        }
        const baseline = Y.encodeStateVector(mirror);
        mirror.transact(() => {
            mutation.apply(cellElement, rowElement, tableElement);
        });
        const hostile = Buffer.from(Y.encodeStateAsUpdate(mirror, baseline)).toString('base64');
        try {
            await call(target, 'applyUpdate', { updateBase64: hostile });
            observed = {
                admitted: true,
                detail: JSON.stringify((await snapshot(target)).documentJson),
            };
        } catch (error) {
            if (!(error instanceof PeerError)) {
                throw error;
            }
            observed = { admitted: false, detail: `${error.code}: ${error.message}` };
        }
    }, tableFixture('prosemirror'));
    if (observed === null) {
        throw new Error(`the ${mutation.name} admission probe recorded no outcome`);
    }
    return observed;
}

test('TBL-10 native admission separates unsafe table input from admissible irregularity', async () => {
    for (const mutation of HOSTILE_MUTATIONS) {
        const observed = await observeAdmission(mutation);
        recordAdmission(suiteReport, {
            name: mutation.name,
            classification: mutation.classification,
            admitted: observed.admitted,
            detail: observed.detail,
        });
        assert.equal(
            observed.admitted,
            mutation.classification === ADMISSIBLE_IRREGULAR_INPUT,
            `${mutation.name} is classified ${mutation.classification} but the engine `
                + `${observed.admitted ? 'admitted' : 'rejected'} it: ${observed.detail}`,
        );
    }
    assert.equal(suiteReport.unsafeAdmissions, NO_FAILURES, describeConvergenceReport(suiteReport));
});

test('TBL-10 unsafeAdmissions charges an unsafe update the engine admitted', () => {
    const report = createConvergenceReport();
    recordSettledRun(report, {
        name: 'native/native settled run carrying the admitted update',
        topology: TOPOLOGY_NATIVE_NATIVE,
        webControlLoops: NO_LOOPS,
        geometry: { kind: GEOMETRY_ADMITTED, irregular: true },
    });
    recordAdmission(report, {
        name: 'colspan below the span domain',
        classification: UNSAFE_INPUT,
        admitted: true,
        detail: 'an engine that stopped enforcing the span domain',
    });
    assert.equal(report.unsafeAdmissions, ONE_FAILURE, describeConvergenceReport(report));
    assert.equal(convergenceScalarsPassed(report), false, describeConvergenceReport(report));
});

test('TBL-21 the convergence corpus passes the settled-geometry gate', () => {
    assert.deepEqual(
        [...CONVERGENCE_TOPOLOGIES].filter((topology) => !chargedTopologies.has(topology)),
        [],
        `the corpus left topologies unrun: ${describeConvergenceReport(suiteReport)}`,
    );
    assert.ok(convergenceScalarsPassed(suiteReport), describeConvergenceReport(suiteReport));
});
