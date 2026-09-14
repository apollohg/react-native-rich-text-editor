import assert from 'node:assert/strict';
import test from 'node:test';
import { createHash } from 'node:crypto';
import { CONVERGENCE_CORPUS, CORPUS_SCENARIOS, runSchedule, scenarioCoverage } from '../corpus.js';
import { call, withPeers, tableFixture, snapshot, flushDocumentEvents } from '../controller.js';
import { observeEvidence } from '../evidence-observer.js';
import type { FamilyEvidence } from '../scenario-evidence.js';
import type { JsonNode } from '../peer-protocol.js';
import {
    assertActionEvidence,
    assertHistoryEvidence,
    assertLifetimeEvidence,
    coverageStatus,
    assertFamilyEvidence,
    FAMILY_INTENTS,
    resolveWidthContributions,
} from '../scenario-evidence.js';

const paragraph = (text: string) => ({
    type: 'paragraph',
    content: [{ type: 'text', text, marks: [{ type: 'strong' }] }],
});
const cell = (text: string) => ({
    type: 'table_cell',
    attrs: { colspan: 1, rowspan: 1, colwidth: null, label: 'opaque' },
    content: [paragraph(text)],
});
const observation = (text: string) => ({
    tables: [
        {
            source: '0',
            parentCell: null,
            pathWithinCell: '0',
            position: 0,
            node: { type: 'table' },
            rows: 1,
            columns: 2,
            widths: [null, null],
            cells: [
                {
                    source: '0.0.0',
                    sourceId: 'cell-a',
                    rawPosition: 2,
                    position: 2,
                    row: 0,
                    column: 0,
                    rowspan: 1,
                    colspan: 1,
                    node: cell(text),
                },
                {
                    source: '0.0.1',
                    sourceId: 'cell-b',
                    rawPosition: 9,
                    position: 9,
                    row: 0,
                    column: 1,
                    rowspan: 1,
                    colspan: 1,
                    node: cell('remote'),
                },
            ],
        },
    ],
});
function typing() {
    return {
        actor: 1,
        kind: 'rust' as const,
        operation: 'insertText',
        target: observation('a').tables[0]!.cells[0]!,
        before: observation('a'),
        after: observation('typed-a'),
        reply: { documentChanged: true },
        passes: 0,
        autonomous: 0,
        text: 'typed-',
        targetGridValid: true,
    };
}

test('frozen base manifest remains exactly 400', () => {
    const manifest = CONVERGENCE_CORPUS.map(
        ({ name, topology, kinds, participants, preset, scenario, actorOffset, seed }) => ({
            name,
            topology,
            kinds,
            participants,
            preset,
            scenario: scenario.name,
            actorOffset,
            seed,
        }),
    );
    assert.equal(manifest.length, 400);
    assert.equal(
        createHash('sha256').update(JSON.stringify(manifest)).digest('hex'),
        '603770fda8be1800bc64a2680d92ab380f27980a3f45f6bbeec65b8b14300b0d',
    );
});
test('TBL10 typing proves declared actor/source rich delta', () => {
    assertActionEvidence(typing(), {
        actor: 1,
        operation: 'insertText',
        source: '0.0.0',
        text: 'typed-',
    });
});
for (const [name, mutate, invariant] of [
    [
        'skipped action',
        (e: ReturnType<typeof typing>) => {
            e.after = observation('a');
        },
        /TEXT_EFFECT/,
    ],
    [
        'false reply',
        (e: ReturnType<typeof typing>) => {
            e.reply.documentChanged = false;
        },
        /ACTION_APPLIED/,
    ],
    [
        'wrong source',
        (e: ReturnType<typeof typing>) => {
            e.target = e.before.tables[0]!.cells[1]!;
        },
        /TARGET/,
    ],
    [
        'lost remote',
        (e: ReturnType<typeof typing>) => {
            e.after.tables[0]!.cells.pop();
        },
        /PRESERVATION/,
    ],
    [
        'duplicated remote',
        (e: ReturnType<typeof typing>) => {
            e.after.tables[0]!.cells.push(structuredClone(e.after.tables[0]!.cells[1]!));
        },
        /PRESERVATION/,
    ],
    [
        'attribute corruption',
        (e: ReturnType<typeof typing>) => {
            e.after.tables[0]!.cells[1]!.node.attrs.label = 'corrupt';
        },
        /PRESERVATION/,
    ],
    [
        'typing normalization',
        (e: ReturnType<typeof typing>) => {
            e.passes = 1;
        },
        /NORMALIZATION/,
    ],
] as const)
    test(`TBL21 canary ${name}`, () => {
        const evidence = typing();
        mutate(evidence);
        assert.throws(
            () =>
                assertActionEvidence(evidence, {
                    actor: 1,
                    operation: 'insertText',
                    source: '0.0.0',
                    text: 'typed-',
                }),
            invariant,
        );
    });
test('TBL13 history proves applied reply and each semantic boundary', () => {
    const e = {
        before: { content: 'a' },
        acted: { content: 'typed-a' },
        undone: { content: 'a' },
        redone: { content: 'typed-a' },
        undo: { applied: true },
        redo: { applied: true },
        passes: [0, 0],
    };
    assertHistoryEvidence(e);
    assert.throws(
        () => assertHistoryEvidence({ ...e, undo: { applied: false } }),
        /HISTORY_APPLIED/,
    );
    assert.throws(() => assertHistoryEvidence({ ...e, undone: e.acted }), /UNDO_EFFECT/);
    assert.throws(() => assertHistoryEvidence({ ...e, redone: e.undone }), /REDO_EFFECT/);
});
test('TBL13 native-owned lifetime rejects a pre-existing repair cell', () => {
    const e = {
        beforeIds: ['a'],
        createdIds: ['a', 'gap'],
        targetId: 'gap',
        afterRemoteIds: ['a', 'gap'],
        remoteOccurrences: [1, 1, 1],
        nativePasses: 1,
    };
    assertLifetimeEvidence(e);
    assert.throws(
        () => assertLifetimeEvidence({ ...e, beforeIds: ['a', 'gap'] }),
        /NATIVE_OWNERSHIP/,
    );
    assert.throws(
        () => assertLifetimeEvidence({ ...e, remoteOccurrences: [1, 0, 1] }),
        /REMOTE_CONTENT/,
    );
    assert.throws(
        () => assertLifetimeEvidence({ ...e, remoteOccurrences: [1, 2, 1] }),
        /REMOTE_CONTENT/,
    );
});
test('TBL21 coverage never credits a refusal or missing predicate', () => {
    assert.equal(coverageStatus(false, false, false), 'unexercised');
    assert.equal(coverageStatus(true, false, true), 'exercised-unproven');
    assert.equal(coverageStatus(true, true, false), 'exercised-unproven');
    assert.equal(coverageStatus(true, true, true), 'proven');
});
test('TBL21 required coverage records preserve every frozen schedule and declared actor', () => {
    const slots = CONVERGENCE_CORPUS.flatMap((schedule) => scenarioCoverage(schedule));
    assert.equal(new Set(slots.map((slot) => slot.schedule)).size, 400);
    for (const slot of slots) {
        assert.equal(slot.required, true);
        assert.equal(slot.status, 'unexercised');
        assert.ok(slot.proof.startsWith('scenario-effect:'));
        const schedule = CONVERGENCE_CORPUS.find((schedule) => schedule.name === slot.schedule)!;
        assert.ok(slot.actor < schedule.participants);
        assert.equal(slot.actorKind, schedule.kinds[slot.actor]);
    }
});
test('TBL21 each frozen family has an independent command and content obligation', () => {
    assert.equal(FAMILY_INTENTS.length, 14);
    for (const intent of FAMILY_INTENTS) {
        assert.ok(intent.steps.length > 0);
        assert.ok(intent.preserve.length > 0);
        assert.throws(
            () =>
                assertFamilyEvidence(intent, {
                    actions: [],
                    deliveries: [],
                    settled: observation('a'),
                    author: 0,
                    actors: [0, 1],
                    kinds: ['rust', 'rust'],
                }),
            /MISSING_ACTION/,
        );
    }
});
test('TBL21 structural skipped command and wrong actor cannot prove insertion', () => {
    const e = typing();
    e.operation = 'addRow';
    assert.throws(
        () => assertActionEvidence(e, { actor: 1, operation: 'addRow', source: '0.0.0' }),
        /ROW_EFFECT/,
    );
    assert.throws(
        () => assertActionEvidence(e, { actor: 0, operation: 'addRow', source: '0.0.0' }),
        /ACTOR_COMMAND/,
    );
});
test('TBL10 structural effects retain unrelated rich content and attributes', () => {
    const e = typing();
    e.operation = 'addRow';
    e.after = observation('a');
    e.after.tables[0]!.rows = 2;
    assertActionEvidence(e, { actor: 1, operation: 'addRow', source: '0.0.0' });
    const corrupted = structuredClone(e);
    corrupted.after.tables[0]!.cells[1]!.node.attrs.label = 'corrupted';
    assert.throws(
        () => assertActionEvidence(corrupted, { actor: 1, operation: 'addRow', source: '0.0.0' }),
        /PRESERVATION/,
    );
    const lost = structuredClone(e);
    lost.after.tables[0]!.cells.pop();
    assert.throws(
        () => assertActionEvidence(lost, { actor: 1, operation: 'addRow', source: '0.0.0' }),
        /PRESERVATION/,
    );
    const swapped = structuredClone(e);
    [
        swapped.after.tables[0]!.cells[0]!.node.content,
        swapped.after.tables[0]!.cells[1]!.node.content,
    ] = [
        swapped.after.tables[0]!.cells[1]!.node.content,
        swapped.after.tables[0]!.cells[0]!.node.content,
    ];
    assert.throws(
        () => assertActionEvidence(swapped, { actor: 1, operation: 'addRow', source: '0.0.0' }),
        /PRESERVATION_CELL_CONTENT/,
    );
});
test('TBL12 widths follow surviving contribution counts, including repeated spans', () => {
    assert.equal(resolveWidthContributions([100, 140]), 140);
    assert.equal(resolveWidthContributions([100, 100, 140]), 100);
    assert.equal(resolveWidthContributions([100, null, 140, 140, 200]), 140);
});
async function capturedFamily(index: number): Promise<FamilyEvidence> {
    const base = CONVERGENCE_CORPUS.find(
        (schedule) => schedule.scenario === CORPUS_SCENARIOS[index],
    )!;
    const capture: { value?: FamilyEvidence } = {};
    const result = await runSchedule({
        ...base,
        scenario: {
            ...base.scenario,
            proves(evidence) {
                capture.value = evidence.boundaries;
                base.scenario.proves!(evidence);
            },
        },
    });
    assert.equal(result.rawConvergence.passed, true);
    assert.equal(result.evidence.status, 'proven', result.evidence.failures.join('\n'));
    assert.ok(capture.value);
    return capture.value;
}
test('TBL12 erased settled resize contributions cannot prove any width family', async () => {
    for (const family of [7, 10, 11]) {
        const evidence = await capturedFamily(family);
        for (const table of evidence.settled.tables) {
            table.widths = Array.from({ length: table.columns }, () => null);
            for (const cell of table.cells)
                if (cell.source !== null) cell.node.attrs = { ...cell.node.attrs, colwidth: null };
        }
        assert.throws(
            () => assertFamilyEvidence(FAMILY_INTENTS[family]!, evidence),
            /WIDTH_SURVIVING_CONTRIBUTION/,
        );
    }
});
test('TBL13 unrelated attribute mutation cannot stand in for the native row undo', async () => {
    const evidence = await capturedFamily(12);
    const undo = evidence.actions.find((action) => action.operation === 'undo')!;
    undo.rawAfter = {
        ...(structuredClone(undo.rawBefore) as JsonNode),
        attrs: { canary: 'not-the-row' },
    };
    undo.after = structuredClone(undo.before);
    evidence.settled = structuredClone(undo.before);
    assert.throws(() => assertFamilyEvidence(FAMILY_INTENTS[12]!, evidence), /HISTORY_ROW_UNDO/);
});
test('TBL12 either authored concurrent winner is permitted on its actual sources', async () => {
    const evidence = await capturedFamily(10);
    for (const width of [180, 220]) {
        for (const table of evidence.settled.tables) {
            table.widths = [width, null];
            for (const cell of table.cells)
                if (cell.column === 0) cell.node.attrs = { ...cell.node.attrs, colwidth: [width] };
        }
        assertFamilyEvidence(FAMILY_INTENTS[10]!, evidence);
    }
});
test('TBL12 authored widths moved onto the wrong sources fail despite consistent resolution', async () => {
    const evidence = await capturedFamily(11);
    for (const table of evidence.settled.tables) {
        table.widths = [220, 180];
        for (const cell of table.cells)
            cell.node.attrs = { ...cell.node.attrs, colwidth: [cell.column === 0 ? 220 : 180] };
    }
    assert.throws(
        () => assertFamilyEvidence(FAMILY_INTENTS[11]!, evidence),
        /WIDTH_SURVIVING_CONTRIBUTION/,
    );
});
test('TBL13 unrelated attribute mutation cannot stand in for remote-gap row redo', async () => {
    const evidence = await capturedFamily(8);
    const redo = evidence.actions.find((action) => action.operation === 'redo')!;
    redo.rawAfter = {
        ...(structuredClone(redo.rawBefore) as JsonNode),
        attrs: { canary: 'not-the-row' },
    };
    redo.after = structuredClone(redo.before);
    assert.throws(() => assertFamilyEvidence(FAMILY_INTENTS[8]!, evidence), /HISTORY_ROW_REDO/);
});
test('TBL10 structural proof requires valid native target and intended insertion boundary', () => {
    const e = typing();
    e.operation = 'addRow';
    e.after = observation('a');
    e.after.tables[0]!.rows = 2;
    e.targetGridValid = false;
    assert.throws(
        () => assertActionEvidence(e, { actor: 1, operation: 'addRow', source: '0.0.0' }),
        /VALID_TARGET/,
    );
    e.targetGridValid = true;
    e.after.tables[0]!.cells[1]!.row = 1;
    assert.throws(
        () => assertActionEvidence(e, { actor: 1, operation: 'addRow', source: '0.0.0' }),
        /INSERTION_FOOTPRINT/,
    );
});
test('TBL21 settled effects reject corruption introduced only during delivery', () => {
    const action = {
        ...typing(),
        text: 'typed',
        after: observation('typeda'),
        head: null,
        rawBefore: {},
        rawAfter: {},
    };
    const settled = structuredClone(action.after);
    settled.tables[0]!.node = {
        type: 'table',
        content: [{ type: 'table_row', content: settled.tables[0]!.cells.map((c) => c.node) }],
    } as (typeof settled.tables)[0]['node'];
    const intent = {
        steps: [{ operation: 'insertText', actor: 'first' as const, target: 'a' }],
        preserve: ['a', 'remote'],
        typed: true,
    };
    const evidence = {
        actions: [action],
        deliveries: [],
        settled,
        author: 0,
        actors: [1, 0],
        kinds: ['rust' as const, 'rust' as const],
    };
    assertFamilyEvidence(intent, evidence);
    settled.tables[0]!.cells[1]!.node.attrs.label = 'corrupt';
    assert.throws(() => assertFamilyEvidence(intent, evidence), /PRESERVATION/);
});
test('TBL21 evidence smoke native typing records the actual source and independently checks R', async () => {
    const schedule = CONVERGENCE_CORPUS.find(
        (s) =>
            s.kinds.every((k) => k === 'rust') &&
            s.scenario.name === 'typing inside a cell without touching geometry',
    )!;
    const outcome = await runSchedule(schedule);
    assert.equal(outcome.evidence?.status, 'proven');
    assert.equal(outcome.rawConvergence?.passed, true);
    assert.ok(outcome.evidence?.actions[0]?.target?.sourceId);
});
test('TBL21 evidence failure leaves successful raw convergence visible', async () => {
    const base = CONVERGENCE_CORPUS.find(
        (s) =>
            s.kinds.every((k) => k === 'rust') &&
            s.scenario.name === 'typing inside a cell without touching geometry',
    )!;
    const result = await runSchedule({
        ...base,
        scenario: { ...base.scenario, act: async () => {} },
    });
    assert.equal(result.rawConvergence?.passed, true);
    assert.equal(result.geometry.kind, 'admitted');
    assert.equal(result.evidence?.status, 'exercised-unproven');
    assert.match(result.evidence!.failures.join(' '), /MISSING_ACTION/);
    assert.ok(scenarioCoverage(base, result).every((slot) => slot.status === 'exercised-unproven'));
});
test('TBL21 native source identities survive shifted paths and duplicate text without observer writes', async () => {
    await withPeers(
        ['rust'],
        async ([native]) => {
            const plainCell = {
                type: 'table_cell',
                attrs: { colspan: 1, rowspan: 1, colwidth: null },
                content: [{ type: 'paragraph', content: [{ type: 'text', text: 'same' }] }],
            };
            await call(native, 'command', {
                type: 'insertContentJson',
                json: {
                    type: 'doc',
                    content: [
                        {
                            type: 'table',
                            content: [{ type: 'table_row', content: [plainCell, plainCell] }],
                        },
                    ],
                },
            });
            await flushDocumentEvents(native);
            const initial = await snapshot(native);
            const before = await observeEvidence(native);
            assert.deepEqual(await snapshot(native), initial);
            assert.equal((await flushDocumentEvents(native)).length, 0);
            const identities = before.tables[0]!.cells.map((c) => c.sourceId);
            assert.equal(new Set(identities).size, 2);
            await call(native, 'command', {
                type: 'addTableRow',
                side: 'before',
                at: before.tables[0]!.cells[0]!.position + 1,
            });
            const after = await observeEvidence(native);
            for (const original of before.tables[0]!.cells) {
                const surviving = after.tables[0]!.cells.find(
                    (c) => c.sourceId === original.sourceId,
                )!;
                assert.ok(surviving);
                assert.notEqual(surviving.source, original.source);
                assert.deepEqual(surviving.node.content, original.node.content);
            }
        },
        tableFixture('prosemirror'),
    );
});
test('TBL21 frozen family evidence smoke across both presets and available actor surfaces', async (t) => {
    for (const scenario of CORPUS_SCENARIOS)
        for (const preset of ['prosemirror', 'tiptap'] as const) {
            for (const surface of ['native', 'mixed'] as const) {
                const schedule = CONVERGENCE_CORPUS.find(
                    (s) =>
                        s.scenario === scenario &&
                        s.preset === preset &&
                        (surface === 'native'
                            ? s.kinds.every((k) => k === 'rust')
                            : s.kinds.slice(0, s.participants).some((k) => k !== 'rust')),
                );
                if (!schedule) continue;
                await t.test(`${surface} ${preset} ${scenario.name}`, async () => {
                    const result = await runSchedule(schedule);
                    assert.equal(
                        result.rawConvergence?.passed,
                        true,
                        JSON.stringify(result.geometry),
                    );
                    assert.equal(
                        result.evidence?.status,
                        'proven',
                        result.evidence?.failures.join('\n'),
                    );
                    assert.equal(
                        result.evidence?.actions.length,
                        FAMILY_INTENTS[CORPUS_SCENARIOS.indexOf(scenario)]!.steps.length,
                    );
                });
            }
        }
});
