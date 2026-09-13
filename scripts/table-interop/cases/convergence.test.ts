import assert from 'node:assert/strict';
import test from 'node:test';
import * as Y from 'yjs';
import {
    EMPTY_STATE_VECTOR_BASE64,
    PeerError,
    call,
    exchangeUntilIdle,
    flushDocumentEvents,
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
    TOPOLOGY_TWO_WEB_CONTROL,
    UNSAFE_INPUT,
    convergenceScalarsPassed,
    createConvergenceReport,
    describeConvergenceReport,
    recordAdmission,
    recordSettledRun,
} from '../convergence-report.js';
import type { AdmissionClassification, SettledGeometry } from '../convergence-report.js';
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
const TABLE_START = 0;
const FIRST_CELL_POSITION = 3;
const NO_LOOPS = 0;
const NO_FAILURES = 0;
const ONE_FAILURE = 1;
const ONE_SETTLED_RUN = 1;
const TWO_SETTLED_RUNS = 2;
const UNSETTLED_WEB_REPAIRS = 2;
const TEXT_NODE = 'text';
const OVERLONG_ROWSPAN = 100_000_000;
const ZERO_COLSPAN = 0;
const NEGATIVE_COLSPAN = -3;
const NON_NUMERIC_COLSPAN = 'two';
const FRACTIONAL_COLSPAN = 1.5;
const BUDGET_BUSTING_COLSPAN = 4_000_000_000;
const NON_NUMERIC_COLWIDTH = 'wide';

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

async function projectedGeometry(
    projector: Peer,
    tableJson: unknown,
): Promise<SettledGeometry> {
    try {
        const projection = await call(projector, 'projectTable', {
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

async function settledGeometryOf(peers: Peer[], projector: Peer): Promise<SettledGeometry> {
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
    return projectedGeometry(projector, JSON.parse(first));
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

test('TBL-21 native peers that never received a delivery charge invalidSettledNativeTables', async () => {
    const report = createConvergenceReport();
    await withPeers(['rust', 'rust'] as const, async ([source, starved]) => {
        await call(source, 'command', {
            type: 'insertContentJson',
            json: { type: DOC_NODE, content: [raggedTable()] },
        });
        await seedFrom(source, [starved]);
        await exchangeUntilIdle([source, starved]);
        const settled = await settledGeometryOf([source, starved], source);
        recordSettledRun(report, {
            name: 'native/native ragged table fully delivered',
            topology: TOPOLOGY_NATIVE_NATIVE,
            webControlLoops: NO_LOOPS,
            geometry: settled,
        });
        assert.equal(settled.kind, GEOMETRY_ADMITTED, describeConvergenceReport(report));

        const anchors = cellAnchors(raggedTable(), TABLE_START);
        const [firstAnchor] = anchors;
        assert.ok(firstAnchor !== undefined, 'the ragged fixture exposes a cell anchor');
        await call(source, 'command', {
            type: 'addTableRow',
            side: 'after',
            at: firstAnchor,
        });
        const withheld = await flushDocumentEvents(source);
        assert.ok(withheld.length > 0, 'the structural edit produced a delivery to withhold');
        recordSettledRun(report, {
            name: 'native/native structural edit withheld from one replica',
            topology: TOPOLOGY_NATIVE_NATIVE,
            webControlLoops: NO_LOOPS,
            geometry: await settledGeometryOf([source, starved], source),
        });
    }, tableFixture('prosemirror'));

    assert.equal(report.settledRuns, TWO_SETTLED_RUNS);
    assert.equal(
        report.invalidSettledNativeTables,
        ONE_FAILURE,
        describeConvergenceReport(report),
    );
    assert.equal(report.invalidSettledWebControlTables, NO_FAILURES);
    assert.equal(convergenceScalarsPassed(report), false, describeConvergenceReport(report));
});

test('TBL-21 a two-web control that never received a delivery charges only the control scalar', async () => {
    const report = createConvergenceReport();
    await withPeers(['prosemirror', 'prosemirror'] as const, async ([source, starved]) => {
        await call(source, 'command', { type: 'insertNode', node: regularTable() });
        await seedFrom(source, [starved]);
        await exchangeUntilIdle([source, starved]);
        recordSettledRun(report, {
            name: 'two-web control regular table fully delivered',
            topology: TOPOLOGY_TWO_WEB_CONTROL,
            webControlLoops: NO_LOOPS,
            geometry: await settledGeometryOf([source, starved], source),
        });

        const before = await webRepairWrites([source, starved]);
        await call(source, 'command', {
            type: 'tableCommand',
            name: 'addRowAfter',
            at: FIRST_CELL_POSITION,
        });
        const withheld = await flushDocumentEvents(source);
        assert.ok(withheld.length > 0, 'the structural edit produced a delivery to withhold');
        recordSettledRun(report, {
            name: 'two-web control structural edit withheld from one replica',
            topology: TOPOLOGY_TWO_WEB_CONTROL,
            webControlLoops: (await webRepairWrites([source, starved])) - before,
            geometry: await settledGeometryOf([source, starved], source),
        });
    }, tableFixture('prosemirror'));

    assert.equal(report.settledRuns, TWO_SETTLED_RUNS);
    assert.equal(report.invalidSettledNativeTables, NO_FAILURES);
    assert.equal(
        report.invalidSettledWebControlTables,
        ONE_FAILURE,
        describeConvergenceReport(report),
    );
    assert.equal(convergenceScalarsPassed(report), false, describeConvergenceReport(report));
});

test('TBL-21 a settled native/two-web run with reordered native delivery stays admissible', async () => {
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
            assert.equal(
                settled.kind,
                GEOMETRY_ADMITTED,
                `reordered native delivery settled as ${JSON.stringify(settled)}`,
            );
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
            recordSettledRun(report, {
                name: 'native/two-web concurrent row and column, reversed native delivery',
                topology: TOPOLOGY_NATIVE_TWO_WEB,
                webControlLoops: (await webRepairWrites([first, second])) - before,
                geometry: settled,
            });
        },
        tableFixture('prosemirror'),
    );

    assert.equal(report.invalidSettledNativeTables, NO_FAILURES, describeConvergenceReport(report));
    assert.equal(report.invalidSettledWebControlTables, NO_FAILURES);
    assert.ok(convergenceScalarsPassed(report), describeConvergenceReport(report));
});

test('TBL-21 a two-web control settling concurrent overlapping merges stays admissible', async () => {
    const report = createConvergenceReport();
    await withPeers(['prosemirror', 'prosemirror'] as const, async ([first, second]) => {
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

        recordSettledRun(report, {
            name: 'two-web control concurrent overlapping merges',
            topology: TOPOLOGY_TWO_WEB_CONTROL,
            webControlLoops: (await webRepairWrites([first, second])) - before,
            geometry: await settledGeometryOf([first, second], first),
        });
    }, tableFixture('prosemirror'));

    assert.equal(
        report.invalidSettledWebControlTables,
        NO_FAILURES,
        `pinned prosemirror-tables settled the adversarial merge pair as `
            + describeConvergenceReport(report),
    );
    assert.equal(report.invalidSettledNativeTables, NO_FAILURES);
});

type HostileMutation = {
    readonly name: string;
    readonly classification: AdmissionClassification;
    readonly apply: (cellElement: Y.XmlElement, rowElement: Y.XmlElement) => void;
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
];

async function observeAdmission(
    mutation: HostileMutation,
    classification: AdmissionClassification,
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
        const rowElement = tableElement.toArray()[0];
        if (!(rowElement instanceof Y.XmlElement)) {
            throw new Error('the seeded table carried no row element');
        }
        const cellElement = rowElement.toArray()[0];
        if (!(cellElement instanceof Y.XmlElement)) {
            throw new Error('the seeded row carried no cell element');
        }
        const baseline = Y.encodeStateVector(mirror);
        mirror.transact(() => {
            mutation.apply(cellElement, rowElement);
        });
        const hostile = Buffer.from(Y.encodeStateAsUpdate(mirror, baseline)).toString('base64');
        try {
            await call(target, 'applyUpdate', { updateBase64: hostile });
            observed = {
                admitted: true,
                detail: JSON.stringify(tableOf((await snapshot(target)).documentJson)),
            };
        } catch (error) {
            if (!(error instanceof PeerError)) {
                throw error;
            }
            observed = { admitted: false, detail: `${error.code}: ${error.message}` };
        }
    }, tableFixture('prosemirror'));
    if (observed === null) {
        throw new Error(`the ${classification} admission probe recorded no outcome`);
    }
    return observed;
}

test('TBL-10 native admission separates unsafe table input from admissible irregularity', async () => {
    const report = createConvergenceReport();
    for (const mutation of HOSTILE_MUTATIONS) {
        const observed = await observeAdmission(mutation, mutation.classification);
        recordAdmission(report, {
            name: mutation.name,
            classification: mutation.classification,
            admitted: observed.admitted,
            detail: observed.detail,
        });
        if (mutation.classification === UNSAFE_INPUT) {
            assert.equal(
                observed.admitted,
                false,
                `${mutation.name} must fail admission: ${observed.detail}`,
            );
        } else {
            assert.equal(
                observed.admitted,
                true,
                `${mutation.name} is admissible irregular geometry: ${observed.detail}`,
            );
        }
    }
    assert.equal(report.unsafeAdmissions, NO_FAILURES, describeConvergenceReport(report));
});

test('TBL-10 unsafeAdmissions counts an unsafe-classified update the engine admitted', async () => {
    const overlongRowspan = HOSTILE_MUTATIONS[HOSTILE_MUTATIONS.length - 1];
    assert.ok(overlongRowspan !== undefined, 'the mutation corpus ends with the rowspan case');
    const observed = await observeAdmission(overlongRowspan, UNSAFE_INPUT);
    assert.equal(observed.admitted, true, 'the deliberate fixture needs an admitted update');
    const report = createConvergenceReport();
    recordSettledRun(report, {
        name: 'native/native settled run carrying the admitted update',
        topology: TOPOLOGY_NATIVE_NATIVE,
        webControlLoops: NO_LOOPS,
        geometry: { kind: GEOMETRY_ADMITTED, irregular: true },
    });
    recordAdmission(report, {
        name: overlongRowspan.name,
        classification: UNSAFE_INPUT,
        admitted: observed.admitted,
        detail: observed.detail,
    });
    assert.equal(report.unsafeAdmissions, ONE_FAILURE, describeConvergenceReport(report));
    assert.equal(convergenceScalarsPassed(report), false, describeConvergenceReport(report));
});
