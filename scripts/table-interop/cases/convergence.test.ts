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
import { NATIVE_PEER_KIND } from '../peer-protocol.js';
import type { Peer, PeerKind } from '../peer-protocol.js';
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
    chargedFindings,
    chargedScalars,
    topologyOf,
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
import { canonicalDocumentShape } from '../assertions.js';
import {
    CONVERGENCE_CORPUS,
    CORPUS_PRESETS,
    SCHEDULES_PER_TOPOLOGY,
    nativeRepairWrites,
    nestedTablesOf,
    runSchedule,
    scenariosFor,
    webRepairWrites,
} from '../corpus.js';
import {
    CELL_NODE,
    PARAGRAPH_NODE,
    ROW_NODE,
    SINGLE_SPAN,
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
const SPANNING_CELL = 2;
const WIDER_SPANNING_CELL = 3;
const SEEDED_COLUMN_WIDTH = 120;
const OPAQUE_NODE = 'opaqueMetadata';
const NESTED_TABLE_COUNT = 2;

const UNCOVERED_PEER_KIND = 'quill';

const suiteReport = createConvergenceReport();
const chargedTopologies = new Set<ConvergenceTopology>();
const attemptedRuns: ConvergenceTopology[] = [];

function chargeCorpusRun(run: SettledRun): void {
    attemptedRuns.push(run.topology);
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

test('TBL-21 a settled run label is derived from the peers, never from the caller', async () => {
    await withPeers(
        ['prosemirror', 'prosemirror', 'rust'] as const,
        async ([first, second, native]) => {
            assert.equal(topologyOf([first, second, native].map(peerKindOf)), TOPOLOGY_NATIVE_TWO_WEB);
            assert.equal(topologyOf([first, second].map(peerKindOf)), TOPOLOGY_TWO_WEB_CONTROL);
            assert.equal(topologyOf([first, native].map(peerKindOf)), TOPOLOGY_NATIVE_WEB);
            assert.equal(topologyOf([native, native].map(peerKindOf)), TOPOLOGY_NATIVE_NATIVE);
            assert.throws(
                () => topologyOf([first].map(peerKindOf)),
                /no convergence topology covers 0 native and 1 web peers/,
            );
            assert.equal(
                topologyOf(['tiptap', 'prosemirror']),
                TOPOLOGY_TWO_WEB_CONTROL,
                'both web peer kinds run the same pinned prosemirror-tables repair engine',
            );
            assert.equal(topologyOf(['tiptap', NATIVE_PEER_KIND]), TOPOLOGY_NATIVE_WEB);
            assert.throws(
                () => topologyOf([UNCOVERED_PEER_KIND as PeerKind, NATIVE_PEER_KIND]),
                /covers only rust and prosemirror\/tiptap peers/,
            );
            assert.throws(
                () => recordSettledRun(createConvergenceReport(), {
                    name: 'a control run wearing a native label',
                    peers: [first, second],
                    topology: TOPOLOGY_NATIVE_TWO_WEB,
                    webControlLoops: NO_LOOPS,
                    geometry: { kind: GEOMETRY_ADMITTED, irregular: false },
                }),
                /is labelled native\/two-web but its peers form two-web-control/,
            );
        },
        tableFixture('prosemirror'),
    );
});

test('TBL-21 irregular settled geometry is allowed for native-only runs and charged for mixed runs', async () => {
    await withPeers(
        ['prosemirror', 'prosemirror', 'rust'] as const,
        async ([first, second, native]) => {
            const nativeOnly = createConvergenceReport();
            recordSettledRun(nativeOnly, {
                name: 'a native-only document that stays irregular',
                peers: [native, native],
                topology: TOPOLOGY_NATIVE_NATIVE,
                webControlLoops: NO_LOOPS,
                geometry: { kind: GEOMETRY_ADMITTED, irregular: true },
            });
            assert.equal(nativeOnly.invalidSettledNativeTables, NO_FAILURES);
            assert.equal(nativeOnly.settledRuns, ONE_SETTLED_RUN);
            assert.ok(
                convergenceScalarsPassed(nativeOnly),
                'spec:296 allows native-only irregular raw state under TBL-11 projection: '
                    + describeConvergenceReport(nativeOnly),
            );

            const mixed = createConvergenceReport();
            recordSettledRun(mixed, {
                name: 'a settled mixed run that stays irregular',
                peers: [first, second, native],
                topology: TOPOLOGY_NATIVE_TWO_WEB,
                webControlLoops: NO_LOOPS,
                geometry: { kind: GEOMETRY_ADMITTED, irregular: true },
            });
            assert.equal(
                mixed.invalidSettledNativeTables,
                ONE_FAILURE,
                'spec:145 requires valid shared raw geometry once web repairs quiesce: '
                    + describeConvergenceReport(mixed),
            );
            assert.equal(convergenceScalarsPassed(mixed), false);
        },
        tableFixture('prosemirror'),
    );
});

test('TBL-21 the settled-geometry scalars route by topology class', async () => {
    await withPeers(
        ['prosemirror', 'prosemirror', 'rust'] as const,
        async ([first, second, native]) => {
            const nativeReport = createConvergenceReport();
            recordSettledRun(nativeReport, {
                name: 'native integration fault',
                peers: [first, second, native],
                topology: TOPOLOGY_NATIVE_TWO_WEB,
                webControlLoops: NO_LOOPS,
                geometry: {
                    kind: GEOMETRY_PROJECTION_FAILED,
                    code: 'TABLE_PROJECTION_FAILED',
                    message: 'a deliberately failed projection',
                },
            });
            assert.equal(nativeReport.invalidSettledNativeTables, ONE_FAILURE);
            assert.equal(nativeReport.invalidSettledWebControlTables, NO_FAILURES);
            assert.equal(convergenceScalarsPassed(nativeReport), false);

            const controlReport = createConvergenceReport();
            recordSettledRun(controlReport, {
                name: 'upstream loop',
                peers: [first, second],
                topology: TOPOLOGY_TWO_WEB_CONTROL,
                webControlLoops: NO_LOOPS,
                geometry: {
                    kind: GEOMETRY_PROJECTION_FAILED,
                    code: 'TABLE_PROJECTION_FAILED',
                    message: 'a deliberately failed projection',
                },
            });
            assert.equal(controlReport.invalidSettledNativeTables, NO_FAILURES);
            assert.equal(controlReport.invalidSettledWebControlTables, ONE_FAILURE);
            assert.equal(convergenceScalarsPassed(controlReport), false);
        },
        tableFixture('prosemirror'),
    );
});

test('TBL-21 an invalid geometry in a still-repairing web run charges no settled scalar', async () => {
    await withPeers(['prosemirror', 'prosemirror'] as const, async ([first, second]) => {
        const report = createConvergenceReport();
        recordSettledRun(report, {
            name: 'web plugin still repairing',
            peers: [first, second],
            topology: TOPOLOGY_TWO_WEB_CONTROL,
            webControlLoops: UNSETTLED_WEB_REPAIRS,
            geometry: {
                kind: GEOMETRY_PROJECTION_FAILED,
                code: 'TABLE_PROJECTION_FAILED',
                message: 'a deliberately failed projection',
            },
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
    }, tableFixture('prosemirror'));
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
        recordSettledRun(report, {
            name: 'native/native ragged table, drained',
            peers: [source, replica],
            topology: TOPOLOGY_NATIVE_NATIVE,
            webControlLoops: NO_LOOPS,
            geometry: settled,
        });
        assert.equal(settled.kind, GEOMETRY_ADMITTED, describeConvergenceReport(report));
        assert.equal(
            settled.irregular,
            true,
            'spec:296 allows a native-only document to stay structurally irregular',
        );
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
        recordSettledRun(report, {
            name: 'native/web structural edit, drained',
            peers: [web, native],
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

test('TBL-21 a settled mixed run that stays irregular charges the native scalar', async () => {
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
            recordSettledRun(report, {
                name: 'native/two-web concurrent row and column, reversed native delivery, drained',
                peers: [first, second, native],
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
            assert.equal(
                report.invalidSettledNativeTables,
                ONE_FAILURE,
                'spec:145 requires valid shared raw geometry once a mixed run settles; the '
                    + 'spec:296 allowance for irregular raw state covers native-only documents '
                    + `only: ${describeConvergenceReport(report)}`,
            );
            assert.equal(convergenceScalarsPassed(report), false);
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
            recordSettledRun(report, {
                name: 'two-web control structural edit, drained',
                peers: [first, second],
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

test('TBL-21 a drained two-web control settling concurrent overlapping merges charges the control scalar', async () => {
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
            recordSettledRun(report, {
                name: 'two-web control concurrent overlapping merges, drained',
                peers: [first, second],
                topology: TOPOLOGY_TWO_WEB_CONTROL,
                webControlLoops: (await webRepairWrites([first, second])) - before,
                geometry: settled,
            });
            assert.equal(
                report.invalidSettledWebControlTables,
                ONE_FAILURE,
                'the pinned prosemirror-tables control settles the adversarial merge pair on '
                    + 'geometry TBL-21 does not accept for a mixed-client product: '
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

test('TBL-10 unsafeAdmissions charges an unsafe update the engine admitted', async () => {
    await withPeers(['rust', 'rust'] as const, async ([author, target]) => {
        const report = createConvergenceReport();
        recordSettledRun(report, {
            name: 'native/native settled run carrying the admitted update',
            peers: [author, target],
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
    }, tableFixture('prosemirror'));
});

function cellWith(attrs: Record<string, unknown> | undefined): Record<string, unknown> {
    const node: Record<string, unknown> = {
        type: CELL_NODE,
        content: [{ type: PARAGRAPH_NODE, content: [{ type: TEXT_NODE, text: 'a' }] }],
    };
    if (attrs !== undefined) {
        node['attrs'] = attrs;
    }
    return node;
}

function shapeOf(node: Record<string, unknown>): string {
    return JSON.stringify(canonicalDocumentShape(node));
}

test('TBL-21 the convergence oracle reads an attribute at its schema default as absent', () => {
    assert.equal(
        shapeOf(cellWith({ colspan: SINGLE_SPAN, rowspan: SINGLE_SPAN, colwidth: null })),
        shapeOf(cellWith(undefined)),
        'a cell carrying only default attributes is the same content as one carrying none',
    );
    assert.equal(
        shapeOf(cellWith({ colspan: SPANNING_CELL, rowspan: SINGLE_SPAN, colwidth: null })),
        shapeOf(cellWith({ colspan: SPANNING_CELL })),
        'defaults elide around a non-default attribute without disturbing it',
    );
});

test('TBL-21 the convergence oracle leaves attributes of undeclared node types untouched', () => {
    const opaque = { type: OPAQUE_NODE, attrs: { colspan: SINGLE_SPAN } };
    assert.notEqual(
        JSON.stringify(canonicalDocumentShape(opaque)),
        JSON.stringify(canonicalDocumentShape({ type: OPAQUE_NODE })),
        'a node type the schema declares no cell attributes for is compared untouched',
    );
    assert.notEqual(
        JSON.stringify(canonicalDocumentShape({ attrs: { colspan: SINGLE_SPAN } })),
        JSON.stringify(canonicalDocumentShape({})),
        'metadata with no node type at all is compared untouched',
    );
    assert.notEqual(
        JSON.stringify(canonicalDocumentShape({ type: OPAQUE_NODE, attrs: { colwidth: null } })),
        JSON.stringify(canonicalDocumentShape({ type: OPAQUE_NODE })),
        'the null attribute arm is scoped the same way',
    );
});

test('TBL-21 the convergence oracle treats an attribute payload as an opaque leaf', () => {
    const carried = { type: CELL_NODE, attrs: { colspan: SINGLE_SPAN } };
    assert.notEqual(
        JSON.stringify(cellWith({ metadata: carried })),
        JSON.stringify(canonicalDocumentShape(
            cellWith({ metadata: { type: CELL_NODE } }),
        )),
        'a cell shape nested inside an attribute payload is data, never a node',
    );
    assert.notEqual(
        JSON.stringify(canonicalDocumentShape(cellWith({ metadata: carried }))),
        JSON.stringify(canonicalDocumentShape(cellWith({ metadata: { type: CELL_NODE } }))),
        'normalization never re-enters an attribute payload at any depth',
    );
    assert.notEqual(
        JSON.stringify(canonicalDocumentShape(
            cellWith({ metadata: { content: [{ type: TEXT_NODE, text: 'a' }, { type: TEXT_NODE, text: 'b' }] } }),
        )),
        JSON.stringify(canonicalDocumentShape(
            cellWith({ metadata: { content: [{ type: TEXT_NODE, text: 'ab' }] } }),
        )),
        'text runs inside an attribute payload are not merged',
    );
});

test('TBL-21 the convergence oracle still diverges on a genuinely different attribute', () => {
    assert.notEqual(
        shapeOf(cellWith({ colspan: SPANNING_CELL, rowspan: SINGLE_SPAN, colwidth: null })),
        shapeOf(cellWith({ colspan: WIDER_SPANNING_CELL, rowspan: SINGLE_SPAN, colwidth: null })),
        'two different colspans are different content',
    );
    assert.notEqual(
        shapeOf(cellWith({ colspan: SPANNING_CELL })),
        shapeOf(cellWith(undefined)),
        'a non-default colspan is not the same content as an absent one',
    );
    assert.notEqual(
        shapeOf(cellWith({ colwidth: [SEEDED_COLUMN_WIDTH] })),
        shapeOf(cellWith({ colwidth: null })),
        'a resolved column width is not the same content as an unset one',
    );
});

test('TBL-10 the corpus native repair assertion fires on a deliberate autonomous repair', async () => {
    await withPeers(['rust', 'rust'] as const, async ([native, replica]) => {
        await call(native, 'command', {
            type: 'insertContentJson',
            json: { type: DOC_NODE, content: [raggedTable()] },
        });
        await seedFrom(native, [replica]);
        await exchangeUntilIdle([native, replica]);
        assert.equal(await nativeRepairWrites([native, replica]), NO_FAILURES);
        assert.equal(
            await webRepairWrites([native, replica]),
            NO_FAILURES,
            'the web repair counter must not be reading native peers',
        );

        await call(native, 'repairTableDuringRemoteWindow', {});

        assert.equal(
            await nativeRepairWrites([native, replica]),
            ONE_FAILURE,
            'the canary repair is exactly the write TBL-10 forbids',
        );
        assert.equal(
            await webRepairWrites([native, replica]),
            NO_FAILURES,
            'a native repair must never be charged to webControlLoops',
        );
    }, tableFixture('prosemirror'));
});

test('TBL-21 every applicable scenario meets every schema preset in every topology', () => {
    for (const topology of CONVERGENCE_TOPOLOGIES) {
        const scheduled = new Set(
            CONVERGENCE_CORPUS
                .filter((schedule) => schedule.topology === topology)
                .map((schedule) => `${schedule.scenario.name}|${schedule.preset}`),
        );
        const required: string[] = [];
        for (const scenario of scenariosFor(topology)) {
            for (const preset of CORPUS_PRESETS) {
                required.push(`${scenario.name}|${preset}`);
            }
        }
        assert.deepEqual(
            required.filter((pair) => !scheduled.has(pair)),
            [],
            `${topology} never runs these scenario and preset pairs`,
        );
    }
});

test('TBL-21 a nested table is judged alongside the table that hosts it', () => {
    const inner = { type: TABLE_NODE, content: [{ type: ROW_NODE, content: [webCell('x')] }] };
    const host = {
        type: CELL_NODE,
        attrs: { colspan: SINGLE_SPAN, rowspan: SINGLE_SPAN, colwidth: null },
        content: [inner],
    };
    const outer = { type: TABLE_NODE, content: [webRow([host, webCell('b')])] };
    assert.equal(nestedTablesOf(outer).length, NESTED_TABLE_COUNT);
    assert.equal(
        nestedTablesOf({ type: CELL_NODE, attrs: { carried: outer } }).length,
        NO_FAILURES,
        'a table shape inside an attribute payload is data, never a table to judge',
    );
});

test('TBL-21 the seeded schedule corpus runs every topology and preset', async () => {
    const startedAt = Date.now();
    for (const schedule of CONVERGENCE_CORPUS) {
        const outcome = await runSchedule(schedule);
        assert.equal(
            outcome.nativeAutonomousRepairWrites,
            NO_FAILURES,
            `TBL-10 forbids an autonomous native repair; ${schedule.name} wrote `
                + `${outcome.nativeAutonomousRepairWrites}`,
        );
        chargeCorpusRun({
            name: schedule.name,
            peers: outcome.peers,
            topology: schedule.topology,
            webControlLoops: outcome.webControlLoops,
            geometry: outcome.geometry,
        });
    }
    process.stdout.write(
        `corpus wall time ${Date.now() - startedAt}ms over ${CONVERGENCE_CORPUS.length} schedules\n`,
    );
    assert.equal(attemptedRuns.length, CONVERGENCE_CORPUS.length);
    for (const topology of CONVERGENCE_TOPOLOGIES) {
        assert.equal(
            attemptedRuns.filter((attempted) => attempted === topology).length,
            SCHEDULES_PER_TOPOLOGY,
            `${topology} did not run ${SCHEDULES_PER_TOPOLOGY} schedules`,
        );
    }
});

test('TBL-21 the convergence corpus passes the settled-geometry gate', () => {
    if (attemptedRuns.length < CONVERGENCE_CORPUS.length) {
        assert.deepEqual(
            chargedScalars(suiteReport),
            [],
            `${attemptedRuns.length} of ${CONVERGENCE_CORPUS.length} corpus schedules ran, so `
                + 'the aggregate topology coverage is not asserted: '
                + chargedFindings(suiteReport).join('\n'),
        );
        return;
    }
    assert.deepEqual(
        [...CONVERGENCE_TOPOLOGIES].filter((topology) => !chargedTopologies.has(topology)),
        [],
        'the corpus left topologies unrun',
    );
    assert.deepEqual(
        chargedScalars(suiteReport),
        [],
        `TBL-21 go/no-go: ${chargedFindings(suiteReport).length} of ${suiteReport.settledRuns} `
            + `settled runs charged a gate scalar:\n${chargedFindings(suiteReport).join('\n')}`,
    );
    assert.ok(convergenceScalarsPassed(suiteReport));
});
