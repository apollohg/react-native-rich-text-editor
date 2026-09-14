import assert from 'node:assert/strict';
import test from 'node:test';
import * as corpus from '../corpus.js';
import { snapshot } from '../controller.js';
import { TOPOLOGY_NATIVE_WEB } from '../convergence-report.js';
import * as evidence from '../continuity-evidence.js';
import type { EffectiveDocument } from '../peer-protocol.js';

function document(text = 'a'): EffectiveDocument {
    return {
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
                cells: [text, 'remote'].map((value, index) => ({
                    source: `0.0.${index}`,
                    sourceId: `id-${index}`,
                    position: 2 + index * 5,
                    row: 0,
                    column: index,
                    rowspan: 1,
                    colspan: 1,
                    node: {
                        type: 'table_cell',
                        attrs: { label: 'opaque', colspan: 1, rowspan: 1 },
                        content: [{ type: 'paragraph', content: [{ type: 'text', text: value }] }],
                    },
                })),
            },
        ],
    };
}

function typed() {
    return {
        actor: 0,
        kind: 'rust' as const,
        operation: 'insertText',
        target: document().tables[0]!.cells[0]!,
        head: null,
        before: document(),
        after: document('typed-a'),
        rawBefore: {},
        rawAfter: {},
        reply: { documentChanged: true },
        passes: 0,
        autonomous: 0,
        text: 'typed-',
    };
}

test('continuation typing checks declared source and all immediate and settled content', () => {
    assert.equal(typeof evidence.assertTypingContinuation, 'function');
    const action = typed();
    evidence.assertTypingContinuation(action, { actor: 0, sourceId: 'id-0', text: 'typed-' }, [
        action.after,
    ]);
    for (const mutate of [
        (e: ReturnType<typeof typed>) => {
            e.reply.documentChanged = false;
        },
        (e: ReturnType<typeof typed>) => {
            e.target.sourceId = 'wrong';
        },
        (e: ReturnType<typeof typed>) => {
            e.after = e.before;
        },
        (e: ReturnType<typeof typed>) => {
            e.after.tables[0]!.cells.pop();
        },
        (e: ReturnType<typeof typed>) => {
            e.after.tables[0]!.cells.push(e.after.tables[0]!.cells[1]!);
        },
        (e: ReturnType<typeof typed>) => {
            e.after.tables[0]!.cells[1]!.node.attrs!.label = 'corrupt';
        },
        (e: ReturnType<typeof typed>) => {
            e.passes = 1;
        },
    ]) {
        const changed = typed();
        mutate(changed);
        assert.throws(
            () =>
                evidence.assertTypingContinuation(
                    changed,
                    { actor: 0, sourceId: 'id-0', text: 'typed-' },
                    [typed().after],
                ),
            /CONTINUITY/,
        );
    }
    assert.throws(
        () =>
            evidence.assertTypingContinuation(
                action,
                { actor: 0, sourceId: 'id-0', text: 'typed-' },
                [action.before],
            ),
        /CONTINUITY/,
    );
});

test('structural cursor uses the live cell and rich typing cursor traverses blocks with scalar offsets', () => {
    assert.equal(typeof evidence.typingCursor, 'function');
    const cell = document().tables[0]!.cells[0]!;
    cell.node.content = [
        {
            type: 'blockquote',
            content: [
                { type: 'heading', content: [{ type: 'text', text: '😀' }] },
                { type: 'paragraph', content: [{ type: 'text', text: 'target' }] },
            ],
        },
    ];
    assert.equal(evidence.typingCursor(cell, 'rust'), 8);
    assert.equal(evidence.typingCursor(cell, 'prosemirror'), 9);
});

test('gap proof requires new attributable content at the declared live gap and survives drain', () => {
    assert.equal(typeof evidence.assertGapContinuation, 'function');
    const before = document();
    before.tables[0]!.cells.forEach((cell) => {
        cell.position += 4;
    });
    const gap = {
        ...structuredClone(before.tables[0]!.cells[0]!),
        source: null,
        sourceId: undefined,
        position: 2,
        node: { type: 'table_cell', attrs: { label: 'opaque' }, content: [{ type: 'paragraph' }] },
    };
    before.tables[0]!.cells.unshift(gap);
    const action = {
        ...typed(),
        before,
        target: null,
        text: 'gap-',
        after: structuredClone(before),
    };
    Object.assign(action.after.tables[0]!.cells[0]!, {
        source: '0.0.new',
        sourceId: 'new',
        node: {
            ...gap.node,
            content: [{ type: 'paragraph', content: [{ type: 'text', text: 'gap-' }] }],
        },
    });
    const intent = { actor: 0, tableSource: '0', position: 2, text: 'gap-' };
    evidence.assertGapContinuation(action, intent, [action.after]);
    for (const mutate of [
        (changed: typeof action) => {
            changed.reply.documentChanged = false;
        },
        (changed: typeof action) => {
            changed.after = changed.before;
        },
        (changed: typeof action) => {
            changed.after.tables[0]!.cells[0]!.position = 100;
        },
        (changed: typeof action) => {
            changed.after.tables[0]!.cells[0]!.sourceId = 'id-0';
        },
    ]) {
        const changed = structuredClone(action);
        mutate(changed);
        assert.throws(
            () => evidence.assertGapContinuation(changed, intent, [action.after]),
            /CONTINUITY/,
        );
    }
    assert.throws(() => evidence.assertGapContinuation(action, intent, [before]), /CONTINUITY/);
});

test('outside typing requires a top-level paragraph and preserves all other outside content', () => {
    assert.equal(typeof evidence.assertUnrelatedContinuation, 'function');
    const action = {
        ...typed(),
        target: null,
        rawBefore: { type: 'doc', content: [{ type: 'table' }, { type: 'paragraph' }] },
        rawAfter: {
            type: 'doc',
            content: [
                { type: 'table' },
                { type: 'paragraph', content: [{ type: 'text', text: 'outside-' }] },
            ],
        },
        before: document(),
        after: document(),
    };
    const intent = { actor: 0, index: 1, text: 'outside-' };
    evidence.assertUnrelatedContinuation(action, intent, [action.rawAfter], [action.after]);
    assert.throws(
        () =>
            evidence.assertUnrelatedContinuation(
                { ...action, target: typed().target },
                intent,
                [action.rawAfter],
                [action.after],
            ),
        /CONTINUITY/,
    );
    assert.throws(
        () =>
            evidence.assertUnrelatedContinuation(
                { ...action, rawAfter: action.rawBefore },
                intent,
                [action.rawBefore],
                [action.after],
            ),
        /CONTINUITY/,
    );
    assert.throws(
        () =>
            evidence.assertUnrelatedContinuation(
                action,
                intent,
                [action.rawBefore],
                [action.after],
            ),
        /CONTINUITY/,
    );
});

test('explicit gap refusal cannot contain writes or replace successful edit proof', () => {
    const action = {
        ...typed(),
        target: null,
        reply: { documentChanged: false },
        before: document(),
        after: document(),
    };
    evidence.assertGapRefusal(action, 0);
    assert.throws(() => evidence.assertGapRefusal(action, 1), /CONTINUITY/);
    assert.throws(
        () => evidence.assertGapRefusal({ ...action, reply: { documentChanged: true } }, 0),
        /CONTINUITY/,
    );
    assert.throws(
        () => evidence.assertGapRefusal({ ...action, rawAfter: { changed: true } }, 0),
        /CONTINUITY/,
    );
});

test('history evidence rejects applied:false and successful-but-skipped undo or redo', () => {
    assert.equal(typeof evidence.assertContinuationHistory, 'function');
    const change = {
        ...typed(),
        operation: 'addRow',
        rawBefore: { rows: 2 },
        rawAfter: { rows: 3 },
    };
    const undo = {
        ...typed(),
        operation: 'undo',
        rawBefore: { rows: 3 },
        rawAfter: { rows: 2 },
        reply: { applied: true },
    };
    const redo = {
        ...typed(),
        operation: 'redo',
        rawBefore: { rows: 2 },
        rawAfter: { rows: 3 },
        reply: { applied: true },
    };
    evidence.assertContinuationHistory([change, undo, redo], 0);
    assert.throws(() => evidence.assertContinuationHistory([change, undo], 0), /CONTINUITY/);
    assert.throws(
        () =>
            evidence.assertContinuationHistory(
                [change, { ...undo, reply: { applied: false } }, redo],
                0,
            ),
        /HISTORY_APPLIED/,
    );
    assert.throws(
        () =>
            evidence.assertContinuationHistory(
                [change, { ...undo, rawAfter: { rows: 3 } }, redo],
                0,
            ),
        /UNDO_EFFECT/,
    );
    assert.throws(
        () =>
            evidence.assertContinuationHistory(
                [change, undo, { ...redo, rawAfter: { rows: 2 } }],
                0,
            ),
        /REDO_EFFECT/,
    );
});

test('continuation requirements retain every frozen setup and actual actor', () => {
    assert.equal(typeof corpus.continuationRequirements, 'function');
    const slots = corpus.continuationRequirements();
    assert.equal(new Set(slots.map((slot) => slot.schedule.name)).size, 400);
    assert.equal(new Set(slots.map((slot) => slot.key)).size, slots.length);
    for (const schedule of corpus.CONVERGENCE_CORPUS) {
        for (let actor = 0; actor < schedule.participants; actor += 1) {
            const proofs = slots
                .filter((slot) => slot.schedule === schedule && slot.actor === actor)
                .map((slot) => slot.proof);
            assert.deepEqual(
                proofs,
                schedule.kinds[actor] === 'rust'
                    ? ['typing', 'structure', 'history', 'partition']
                    : ['typing', 'structure', 'history', 'partition', 'unrelated-web', 'web-gap'],
            );
        }
    }
    assert.equal(slots.length, 4600);
    assert.equal(slots.filter((slot) => slot.required === true).length, 4100);
    assert.ok(slots.every((slot) => slot.status === 'unexercised'));
    assert.ok(
        slots.filter((slot) => slot.proof === 'web-gap').every((slot) => slot.required === null),
    );
});

test('settled setup callback observes live converged peers before closure', async () => {
    const schedule = corpus.CONVERGENCE_CORPUS[0]!;
    let called = false;
    const outcome = await corpus.runSchedule(schedule, async ({ participants, baseline }) => {
        called = true;
        assert.equal(baseline.rawConvergence.passed, true);
        assert.equal(participants.length, schedule.participants);
        assert.ok((await snapshot(participants[0]!)).mounted);
    });
    assert.equal(outcome.rawConvergence.passed, true);
    assert.equal(called, true);
});

test('independent native continuations retain actual seed variants and separate proof axes', async () => {
    assert.equal(typeof corpus.runContinuation, 'function');
    const schedule = corpus.CONVERGENCE_CORPUS[1]!;
    for (const slot of corpus.continuationRequirements([schedule])) {
        const result = await corpus.runContinuation(slot);
        assert.equal(result.slot.key, slot.key);
        assert.equal(result.status, 'proven', JSON.stringify(result.failures));
        assert.equal(result.baseline?.rawConvergence.passed, true);
        assert.ok(
            result.checkpoints.every((checkpoint) => checkpoint.raw.passed),
            JSON.stringify(result.checkpoints),
        );
        assert.ok(result.checkpoints.length >= 2);
        assert.ok(result.actions.length > 0);
        assert.equal(corpus.continuationPassed(result), true, JSON.stringify(result.checkpoints));
        assert.equal(
            corpus.continuationPassed({ ...result, actions: [] }),
            false,
            'registered success must not cover skipped actions',
        );
        assert.ok(result.checkpoints.slice(1).every((checkpoint) => checkpoint.drain.rounds > 0));
        for (const checkpoint of result.checkpoints) {
            assert.ok(checkpoint.drain.emitted <= 10000);
            assert.ok(
                checkpoint.remoteBoundaries.every(
                    (boundary) => boundary.kind !== 'rust' || boundary.passes === 0,
                ),
            );
        }
        if (slot.proof === 'partition')
            assert.equal(corpus.continuationPassed({ ...result, dependencies: [] }), false);
        const badRaw = result.checkpoints.map((checkpoint) => ({
            ...checkpoint,
            raw: { passed: false },
        }));
        const badPresentation = result.checkpoints.map((checkpoint) => ({
            ...checkpoint,
            presentation: { ...checkpoint.presentation, passed: false },
        }));
        assert.equal(corpus.continuationPassed({ ...result, checkpoints: badRaw }), false);
        assert.equal(corpus.continuationPassed({ ...result, checkpoints: badPresentation }), false);
        assert.equal(
            corpus.continuationCoverage([slot], [{ ...result, checkpoints: badRaw }])[0]!.status,
            'proven',
        );
        assert.equal(corpus.continuationCoverage([slot], [])[0]!.status, 'unexercised');
    }
});

test('generic continuation smoke covers both presets and all four actual participant topologies', async (t) => {
    const groups = new Map<string, corpus.CorpusSchedule>();
    for (const schedule of corpus.CONVERGENCE_CORPUS)
        if (schedule.scenario === corpus.CORPUS_SCENARIOS[4]) {
            const group = `${schedule.topology}/${schedule.preset}`;
            if (!groups.has(group)) groups.set(group, schedule);
        }
    assert.equal(groups.size, 8);
    for (const schedule of groups.values())
        for (const slot of corpus.continuationRequirements([schedule])) {
            await t.test(
                `${slot.topology}/${slot.preset}/${slot.actor}/${slot.proof}`,
                async () => {
                    const result = await corpus.runContinuation(slot);
                    assert.equal(
                        corpus.continuationPassed(result),
                        true,
                        JSON.stringify({
                            key: slot.key,
                            status: result.status,
                            failures: result.failures,
                            checkpoints: result.checkpoints.map(
                                ({ boundary, raw, presentation, drain }) => ({
                                    boundary,
                                    raw,
                                    presentation,
                                    drain,
                                }),
                            ),
                        }),
                    );
                    if (slot.proof === 'web-gap') {
                        assert.deepEqual(result.gap, { observed: true, positions: [] });
                        assert.equal(result.required, false);
                        assert.equal(result.disposition, 'no-gap');
                        assert.equal(result.actions.length, 0);
                    }
                },
            );
        }
});

test('required web typing from concurrent merges survives reversed repair/text updates', async () => {
    const schedule = corpus.CONVERGENCE_CORPUS.find((entry) => entry.seed === 1116903952)!;
    assert.ok(schedule);
    const slot = corpus
        .continuationRequirements([schedule])
        .find((entry) => entry.actor === 0 && entry.proof === 'typing')!;
    const result = await corpus.runContinuation(slot);
    assert.equal(
        corpus.continuationPassed(result),
        true,
        JSON.stringify({
            key: slot.key,
            status: result.status,
            failures: result.failures,
            tracePath: result.tracePath,
            checkpoints: result.checkpoints.map(({ boundary, raw, presentation }) => ({
                boundary,
                raw,
                presentation,
            })),
        }),
    );
});

test('actual display gaps materialize attributable shared text in both presets', async (t) => {
    for (const preset of corpus.CORPUS_PRESETS)
        await t.test(preset, async () => {
            const schedule = corpus.CONVERGENCE_CORPUS.find(
                (entry) =>
                    entry.topology === TOPOLOGY_NATIVE_WEB &&
                    entry.preset === preset &&
                    entry.scenario === corpus.CORPUS_SCENARIOS[1],
            )!;
            const slot = corpus
                .continuationRequirements([schedule])
                .find((entry) => entry.actor === 0 && entry.proof === 'web-gap')!;
            const result = await corpus.runContinuation(slot);
            assert.equal(result.required, true);
            assert.equal(result.disposition, 'edited', JSON.stringify(result.failures));
            assert.equal(
                corpus.continuationPassed(result),
                true,
                JSON.stringify({
                    failures: result.failures,
                    checkpoints: result.checkpoints.map(({ boundary, raw, presentation }) => ({
                        boundary,
                        raw,
                        presentation,
                    })),
                }),
            );
        });
});

test('web continuations measure unrelated text and resolve gap applicability from the live surface', async () => {
    const schedule = corpus.CONVERGENCE_CORPUS.find(
        (entry) =>
            entry.topology === TOPOLOGY_NATIVE_WEB && entry.scenario === corpus.CORPUS_SCENARIOS[4],
    )!;
    assert.ok(schedule);
    const slots = corpus
        .continuationRequirements([schedule])
        .filter((slot) => slot.actorKind !== 'rust');
    for (const slot of slots) {
        const result = await corpus.runContinuation(slot);
        assert.ok(result.baseline?.rawConvergence.passed);
        assert.equal(result.status, 'proven', `${slot.proof}: ${JSON.stringify(result.failures)}`);
        assert.equal(corpus.continuationPassed(result), true, JSON.stringify(result.checkpoints));
    }
});
