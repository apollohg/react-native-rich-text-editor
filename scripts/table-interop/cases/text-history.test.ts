import assert from 'node:assert/strict';
import test from 'node:test';
import * as corpus from '../corpus.js';
import { supplementaryRequirements } from '../supplementary-continuity.js';
import * as evidence from '../text-history-evidence.js';
import type { EffectiveDocument, JsonNode } from '../peer-protocol.js';
import type { RecordedAction } from '../scenario-evidence.js';
import { spawnSync } from 'node:child_process';

function fixture(): EffectiveDocument {
    const cell = (text: string): JsonNode => ({
        type: 'table_header',
        attrs: { label: text, colspan: 1, rowspan: 1 },
        content: [
            {
                type: 'paragraph',
                content: [
                    { type: 'text', text, marks: [{ type: 'strong' }] },
                    { type: 'image', attrs: { src: 'asset' } },
                ],
            },
        ],
    });
    const nodes = [cell('original'), cell('unrelated')];
    return {
        tables: [
            {
                source: '0',
                parentCell: null,
                pathWithinCell: '0',
                position: 0,
                rows: 1,
                columns: 2,
                widths: null,
                node: {
                    type: 'table',
                    content: [{ type: 'table_row', content: nodes }],
                },
                cells: nodes.map((node, index) => ({
                    source: `0.0.${index}`,
                    sourceId: `source-${index}`,
                    position: 2 + index * 20,
                    row: 0,
                    column: index,
                    rowspan: 1,
                    colspan: 1,
                    node,
                })),
            },
        ],
    };
}
const raw = (view: EffectiveDocument): JsonNode => ({
    type: 'doc',
    content: view.tables.filter((table) => table.parentCell === null).map((table) => table.node),
});

function history(before = fixture()) {
    const intent = evidence.textHistoryIntent(before, raw(before), {
        actor: 0,
        kind: 'rust',
        sourceId: 'source-0',
        text: 'unique-marker-',
    });
    const typed = structuredClone(before);
    typed.tables[0]!.cells[0]!.node.content![0]!.content![0]!.text = 'unique-marker-original';
    const actions = ['insertText', 'undo', 'redo'].map(
        (operation, index): RecordedAction => ({
            actor: 0,
            kind: 'rust',
            operation,
            before: index === 1 ? typed : before,
            after: index === 1 ? before : typed,
            rawBefore: raw(index === 1 ? typed : before),
            rawAfter: raw(index === 1 ? before : typed),
            target: operation === 'insertText' ? before.tables[0]!.cells[0]! : null,
            head: null,
            text: operation === 'insertText' ? 'unique-marker-' : undefined,
            reply: operation === 'insertText' ? { documentChanged: true } : { applied: true },
            passes: 0,
            autonomous: 0,
        }),
    );
    return { intent, actions };
}

test('text-history predicates derive exact rich source effects before commands', () => {
    assert.equal(typeof evidence.textHistoryIntent, 'function');
    const { intent, actions } = history();
    actions.forEach((action, index) =>
        evidence.assertTextHistoryBoundary(action, intent, index, [
            {
                kind: action.kind,
                document: action.after,
                raw: action.rawAfter as JsonNode,
            },
        ]),
    );
});

test('text-history retains nested sources and checks raw-only unrelated mutations', () => {
    const before = fixture();
    const nested = fixture().tables[0]!;
    nested.source = '0.0.1.1';
    nested.parentCell = '0.0.1';
    nested.cells.forEach((cell, index) => {
        cell.source = `${nested.source}.0.${index}`;
        cell.sourceId = `nested-${index}`;
    });
    before.tables[0]!.cells[1]!.node.content!.push(nested.node);
    before.tables.push(nested);
    const { intent, actions } = history(before);
    for (const [index, action] of actions.entries()) {
        const observation = {
            kind: action.kind,
            document: action.after,
            raw: action.rawAfter as JsonNode,
        };
        evidence.assertTextHistoryBoundary(action, intent, index, [observation]);
        const lost = structuredClone(action);
        lost.after.tables[1]!.cells[0]!.node.content = [];
        assert.throws(
            () => evidence.assertTextHistoryBoundary(lost, intent, index, [observation]),
            /CONTINUITY/,
        );
        const rawOnly = structuredClone(observation);
        rawOnly.raw.content![0]!.content![0]!.content![1]!.content![1]!.content![0]!.content![0]!.attrs!.label =
            'corrupt';
        assert.throws(
            () => evidence.assertTextHistoryBoundary(action, intent, index, [rawOnly]),
            /CONTINUITY/,
        );
    }
});

test('only web actions may publish finite geometry changes alongside exact source text', () => {
    const { intent, actions } = history();
    const action = structuredClone(actions[0]!);
    action.after.tables[0]!.cells[0]!.node.attrs!.rowspan = 2;
    action.rawAfter = raw(action.after);
    const observations = () => [
        {
            kind: action.kind,
            document: action.after,
            raw: action.rawAfter as JsonNode,
        },
    ];
    assert.throws(
        () => evidence.assertTextHistoryBoundary(action, intent, 0, observations()),
        /CONTINUITY/,
    );
    action.kind = 'prosemirror';
    intent.kind = 'prosemirror';
    evidence.assertTextHistoryBoundary(action, intent, 0, observations());
});

test('text-history rejects skipped, misdirected, lossy and normalization effects at every boundary', () => {
    const { intent, actions } = history();
    for (const [index, action] of actions.entries()) {
        for (const mutate of [
            (a: RecordedAction) => {
                a.operation = 'snapshot';
            },
            (a: RecordedAction) => {
                a.reply = { documentChanged: false, applied: false };
            },
            (a: RecordedAction) => {
                a.actor = 1;
            },
            (a: RecordedAction) => {
                a.after = a.before;
                a.rawAfter = a.rawBefore;
            },
            (a: RecordedAction) => {
                a.after.tables[0]!.cells[0]!.node.content![0]!.content![0]!.text +=
                    'unique-marker-';
            },
            (a: RecordedAction) => {
                a.after.tables[0]!.cells[0]!.node.content![0]!.content![0]!.marks = [];
            },
            (a: RecordedAction) => {
                a.after.tables[0]!.cells[0]!.node.attrs!.label = 'changed';
            },
            (a: RecordedAction) => {
                a.after.tables[0]!.cells[0]!.node.content![0]!.content!.pop();
            },
            (a: RecordedAction) => {
                a.after.tables[0]!.cells[1]!.node.content = [];
            },
            (a: RecordedAction) => {
                a.after.tables[0]!.cells[0]!.sourceId = 'wrong-source';
            },
            (a: RecordedAction) => {
                a.passes = 1;
            },
            (a: RecordedAction) => {
                a.autonomous = 1;
            },
            (a: RecordedAction) => {
                a.observationFailure = 'missing';
            },
            (a: RecordedAction) => {
                a.rawAfter = a.rawBefore;
            },
        ]) {
            const invalid = structuredClone(action);
            mutate(invalid);
            assert.throws(
                () =>
                    evidence.assertTextHistoryBoundary(invalid, intent, index, [
                        {
                            kind: invalid.kind,
                            document: invalid.after,
                            raw: invalid.rawAfter as JsonNode,
                        },
                    ]),
                /CONTINUITY/,
            );
        }
        assert.throws(
            () => evidence.assertTextHistoryBoundary(action, intent, index, []),
            /CONTINUITY/,
        );
        const settled = structuredClone(action.after);
        settled.tables[0]!.cells[1]!.node.attrs!.label = 'settled corruption';
        assert.throws(
            () =>
                evidence.assertTextHistoryBoundary(action, intent, index, [
                    { kind: action.kind, document: settled, raw: raw(settled) },
                ]),
            /CONTINUITY/,
        );
    }
    const wrong = structuredClone(actions[0]!);
    wrong.target = wrong.before.tables[0]!.cells[1]!;
    assert.throws(
        () =>
            evidence.assertTextHistoryBoundary(wrong, intent, 0, [
                {
                    kind: wrong.kind,
                    document: wrong.after,
                    raw: wrong.rawAfter as JsonNode,
                },
            ]),
        /CONTINUITY/,
    );
});

test('text-history companions are additive and retain exact originating obligations', () => {
    assert.equal(typeof corpus.textHistoryRequirements, 'function');
    const original = supplementaryRequirements();
    const histories = original.filter(
        (slot) => slot.family === 'overlap' && slot.proof === 'history',
    );
    const companions = corpus.textHistoryRequirements(histories);
    assert.equal(original.length, 186);
    assert.equal(corpus.continuationRequirements().length, 4600);
    assert.equal(companions.length, 18);
    assert.deepEqual(
        ['rust', 'prosemirror', 'tiptap'].map(
            (kind) => companions.filter((slot) => slot.actorKind === kind).length,
        ),
        [8, 5, 5],
    );
    for (const [index, slot] of companions.entries()) {
        assert.equal(slot.companionOf, histories[index]!.key);
        assert.equal(slot.key, `${slot.companionOf} :: text-history`);
        assert.equal(slot.proof, 'text-history');
        assert.equal(slot.schedule, histories[index]!.schedule);
        assert.equal(slot.actor, histories[index]!.actor);
    }
    assert.equal(
        corpus.textHistoryRequirements(
            corpus.continuationRequirements().filter((slot) => slot.proof === 'history'),
        ).length,
        900,
    );
    assert.throws(
        () => corpus.textHistoryRequirements([histories[0]!, histories[0]!]),
        /duplicate/,
    );
    assert.throws(
        () => corpus.textHistoryRequirements([original.find((slot) => slot.proof === 'typing')!]),
        /history/,
    );
});

test('text-history result acceptance requires complete matching evidence and every checkpoint', () => {
    const slot = corpus.textHistoryRequirements(
        supplementaryRequirements().filter(
            (slot) =>
                slot.family === 'overlap' && slot.proof === 'history' && slot.actorKind === 'rust',
        ),
    )[0]!;
    const { intent, actions } = history();
    const result = {
        slot,
        required: true,
        status: 'proven',
        disposition: 'edited',
        failures: [],
        dependencies: [],
        actions,
        textHistory: intent,
        baseline: {
            evidence: { status: 'proven' },
            rawConvergence: { passed: true },
        },
        checkpoints: ['baseline', 'typed', 'undo', 'redo'].map((boundary, index) => ({
            boundary,
            raw: { passed: true },
            presentation: { passed: true },
            drain: { passed: true, rounds: 1, emitted: 0 },
            nativeAutonomousRepairWrites: 0,
            remoteBoundaries: [],
            observationFailures: [],
            observations: [
                {
                    kind: 'rust',
                    document: index === 0 ? intent.before : actions[index - 1]!.after,
                },
            ],
            textHistoryObservations: [
                {
                    kind: 'rust',
                    document: index === 0 ? intent.before : actions[index - 1]!.after,
                    raw: index === 0 ? intent.rawBefore : actions[index - 1]!.rawAfter,
                },
            ],
        })),
    } as unknown as corpus.ContinuationResult;
    for (const checkpoint of result.checkpoints)
        checkpoint.textHistoryObservations = Array.from(
            { length: slot.schedule.participants },
            () => structuredClone(checkpoint.textHistoryObservations![0]!),
        );
    assert.equal(corpus.continuationPassed(result), true);
    for (const mutate of [
        (r: corpus.ContinuationResult) => {
            delete r.textHistory;
        },
        (r: corpus.ContinuationResult) => {
            r.actions.splice(0, 1);
        },
        (r: corpus.ContinuationResult) => {
            r.actions.splice(1, 1);
        },
        (r: corpus.ContinuationResult) => {
            r.actions.splice(2, 1);
        },
        (r: corpus.ContinuationResult) => {
            r.actions[1]!.reply = { applied: false };
        },
        (r: corpus.ContinuationResult) => {
            r.checkpoints.splice(2, 1);
        },
        (r: corpus.ContinuationResult) => {
            r.checkpoints[2] = r.checkpoints[1]!;
        },
        (r: corpus.ContinuationResult) => {
            r.checkpoints[1]!.raw.passed = false;
        },
        (r: corpus.ContinuationResult) => {
            r.checkpoints[1]!.textHistoryObservations = [];
        },
        (r: corpus.ContinuationResult) => {
            r.textHistory!.sourceId = 'other';
        },
        (r: corpus.ContinuationResult) => {
            r.textHistory!.typed.content = [];
        },
        (r: corpus.ContinuationResult) => {
            r.checkpoints[0]!.textHistoryObservations = [];
        },
        (r: corpus.ContinuationResult) => {
            r.checkpoints[0]!.textHistoryObservations![0]!.raw = {
                type: 'doc',
            };
        },
        (r: corpus.ContinuationResult) => {
            (r.slot as { companionOf?: string }).companionOf = 'different history';
        },
    ]) {
        const invalid = JSON.parse(JSON.stringify(result)) as corpus.ContinuationResult;
        mutate(invalid);
        assert.equal(corpus.continuationPassed(invalid), false);
    }
    assert.equal(corpus.continuationCoverage([slot], [])[0]!.status, 'unexercised');
    assert.throws(() => corpus.continuationCoverage([slot, slot], []), /duplicate/);
    const wrong = { ...result, slot: { ...slot, companionOf: 'wrong' } };
    assert.throws(() => corpus.continuationCoverage([slot], [wrong]), /mismatch/);
});

test('scoped runner separately declares all18 text-history companions', () => {
    const result = spawnSync(
        process.execPath,
        [
            '--import',
            './node_modules/tsx/dist/loader.mjs',
            'run-continuations.ts',
            '--text-history',
            '--manifest',
        ],
        { encoding: 'utf8' },
    );
    assert.equal(result.status, 0, result.stderr);
    const slots = JSON.parse(result.stdout);
    assert.equal(slots.length, 18);
    assert.ok(
        slots.every(
            (slot: corpus.ContinuationSlot) => slot.proof === 'text-history' && slot.companionOf,
        ),
    );
});
