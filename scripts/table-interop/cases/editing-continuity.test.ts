import assert from 'node:assert/strict';
import test from 'node:test';
import * as corpus from '../corpus.js';
import { snapshot } from '../controller.js';
import { TOPOLOGY_NATIVE_WEB } from '../convergence-report.js';
import * as evidence from '../continuity-evidence.js';
import type { EffectiveDocument, EffectiveTable, JsonNode, PeerKind } from '../peer-protocol.js';
import type { RecordedAction } from '../scenario-evidence.js';

function observed(
    document: EffectiveDocument,
    kind: PeerKind = 'rust',
): evidence.ContinuationObservation[] {
    return [{ kind, document }];
}

function preserve(
    before: EffectiveDocument,
    after: EffectiveDocument,
    changed = new Map<string, JsonNode>(),
    allowEmptyAdditions = false,
    afterKind: PeerKind = 'rust',
): void {
    evidence.assertSourcePreservation(before, after, changed, allowEmptyAdditions, {
        before: 'rust',
        after: afterKind,
    });
}

function layout(table: EffectiveTable, kind: PeerKind = 'rust'): void {
    let position = table.position + 1;
    for (const row of table.node.content ?? []) {
        let cellPosition = position + 1;
        for (const node of row.content ?? []) {
            const cell = table.cells.find((candidate) => candidate.node === node);
            assert.ok(cell);
            cell.position = cellPosition;
            cellPosition += evidence.nodeSize(node, kind);
        }
        position += evidence.nodeSize(row, kind);
    }
}

function rowDocument(): EffectiveDocument {
    const cells = ['same😀', 'same😀'].map((text, index) => ({
        source: `0.${index}.0`,
        sourceId: `row-cell-${index}`,
        position: 0,
        row: index,
        column: 0,
        rowspan: 1,
        colspan: 1,
        node: {
            type: 'table_cell',
            content: [{ type: 'paragraph', content: [{ type: 'text', text }] }],
        },
    }));
    const table: EffectiveTable = {
        source: '0',
        parentCell: null,
        pathWithinCell: '0',
        position: 0,
        rows: 2,
        columns: 1,
        widths: [null],
        cells,
        node: {
            type: 'table',
            content: cells.map((cell, index) => ({
                type: 'table_row',
                attrs: { label: index === 0 ? 'A' : 'B' },
                content: [cell.node],
            })),
        },
    };
    layout(table);
    return { tables: [table] };
}

function rowInsertion(): RecordedAction {
    const before = rowDocument();
    const after = structuredClone(before);
    const table = after.tables[0]!;
    const added = {
        ...structuredClone(table.cells[0]!),
        source: '0.1.0',
        sourceId: 'inserted-cell',
        row: 1,
        node: {
            type: 'table_cell',
            content: [{ type: 'paragraph' }],
        } as JsonNode,
    };
    table.cells[1]!.row = 2;
    table.cells[1]!.source = '0.2.0';
    table.cells.splice(1, 0, added);
    table.node.content!.splice(1, 0, {
        type: 'table_row',
        content: [added.node],
    });
    table.rows = 3;
    layout(table);
    return {
        actor: 0,
        kind: 'rust',
        operation: 'addRow',
        target: before.tables[0]!.cells[0]!,
        head: null,
        before,
        after,
        rawBefore: {},
        rawAfter: {},
        reply: { documentChanged: true },
        passes: 1,
        autonomous: 0,
        targetGridValid: true,
    };
}

test('source preservation rejects row attribute reassignment between surviving identities', () => {
    const before = rowDocument();
    const swapped = structuredClone(before);
    swapped.tables[0]!.node.content![0]!.attrs = { label: 'B' };
    swapped.tables[0]!.node.content![1]!.attrs = { label: 'A' };
    assert.throws(() => preserve(before, swapped), /row attributes/);
});

test('structural continuation preserves original unrelated and nested tables at every boundary', () => {
    for (const nested of [false, true]) {
        const action = rowInsertion();
        for (const view of [action.before, action.after]) {
            const other = rowDocument().tables[0]!;
            other.source = '1';
            other.position = evidence.nodeSize(view.tables[0]!.node, 'rust');
            other.node.attrs = { label: 'original' };
            other.cells.forEach((cell, index) => {
                cell.sourceId = `other-${index}`;
                cell.source = `${other.source}.${index}.0`;
            });
            layout(other);
            view.tables.push(other);
            if (nested) {
                const child = rowDocument().tables[0]!;
                const parent = other.cells[0]!;
                child.source = '1.0.0.1';
                child.parentCell = parent.source;
                child.node.attrs = { label: 'nested-original' };
                child.position =
                    parent.position + 1 + evidence.nodeSize(parent.node.content![0]!, 'rust');
                child.cells.forEach((cell, index) => {
                    cell.sourceId = `nested-${index}`;
                    cell.source = `${child.source}.${index}.0`;
                });
                parent.node.content!.push(child.node);
                layout(other);
                layout(child);
                view.tables.push(child);
            }
        }
        const intent = { actor: 0, source: '0.0.0' };
        evidence.assertStructuralContinuation(action, intent, observed(action.after));
        for (const mutate of [
            (table: EffectiveTable) => {
                table.node.attrs!.label = 'corrupted';
            },
            (table: EffectiveTable) => {
                table.cells[0]!.node.content![0]!.content![0]!.text = 'lost';
            },
            (table: EffectiveTable) => {
                table.node.content![0]!.attrs = { label: 'B' };
            },
        ]) {
            const immediate = structuredClone(action);
            mutate(immediate.after.tables[nested ? 2 : 1]!);
            assert.throws(
                () =>
                    evidence.assertStructuralContinuation(
                        immediate,
                        intent,
                        observed(immediate.after),
                    ),
                /CONTINUITY/,
            );
            const settled = structuredClone(action.after);
            mutate(settled.tables[nested ? 2 : 1]!);
            assert.throws(
                () => evidence.assertStructuralContinuation(action, intent, observed(settled)),
                /CONTINUITY/,
            );
        }
    }
});

test('structural continuation retains newly authored source attributes through settlement', () => {
    const action = rowInsertion();
    action.after.tables[0]!.cells[1]!.node.attrs = { label: 'authored' };
    const intent = { actor: 0, source: '0.0.0' };
    evidence.assertStructuralContinuation(action, intent, observed(action.after));
    for (const mutate of [
        (cell: JsonNode) => {
            cell.attrs!.label = 'lost';
        },
        (cell: JsonNode) => {
            cell.content![0]!.content = [{ type: 'text', text: 'undeclared' }];
        },
    ]) {
        const settled = structuredClone(action.after);
        mutate(settled.tables[0]!.cells[1]!.node);
        assert.throws(
            () => evidence.assertStructuralContinuation(action, intent, observed(settled)),
            /CONTINUITY/,
        );
    }
});

test('structural preservation permits declared crossing spans and empty normalization cells', () => {
    const action = rowInsertion();
    for (const [view, span] of [
        [action.before, 2],
        [action.after, 3],
    ] as const) {
        const table = view.tables[0]!;
        const crossing = {
            ...structuredClone(table.cells[0]!),
            sourceId: 'crossing',
            source: '0.0.1',
            column: 1,
            rowspan: span,
            node: {
                type: 'table_cell',
                attrs: { rowspan: span },
                content: [{ type: 'paragraph' }],
            },
        };
        table.cells.push(crossing);
        table.columns = 2;
        table.widths = [null, null];
        table.node.content![0]!.content!.push(crossing.node);
        layout(table);
    }
    evidence.assertStructuralContinuation(
        action,
        { actor: 0, source: '0.0.0' },
        observed(action.after),
    );
    const irregular = rowDocument();
    irregular.tables[0]!.columns = 2;
    irregular.tables[0]!.widths = [null, null];
    const normalized = structuredClone(irregular);
    const table = normalized.tables[0]!;
    for (const row of [0, 1]) {
        const filler = {
            ...structuredClone(table.cells[0]!),
            sourceId: `normalization-${row}`,
            source: `0.${row}.1`,
            column: 1,
            row,
            node: { type: 'table_cell', content: [{ type: 'paragraph' }] },
        };
        table.cells.push(filler);
        table.node.content![row]!.content!.push(filler.node);
    }
    layout(table);
    preserve(irregular, normalized, new Map(), true);
});

test('row ownership survives legal insertion, native/web offsets and unavailable logical geometry', () => {
    const action = rowInsertion();
    evidence.assertStructuralContinuation(
        action,
        { actor: 0, source: '0.0.0' },
        observed(action.after),
    );
    const web = structuredClone(action.after);
    layout(web.tables[0]!, 'prosemirror');
    for (const cell of web.tables[0]!.cells) {
        cell.row = null;
        cell.column = null;
        cell.source = 'deliberately-unusable-path';
        cell.node.attrs = { rowspan: 1, colspan: 1, colwidth: null };
    }
    preserve(action.before, web, new Map(), true, 'prosemirror');
    assert.throws(
        () => preserve(action.before, web, new Map(), true),
        /row source attribution unavailable/,
    );
    assert.throws(
        () => evidence.assertSourcePreservation(action.before, web, new Map(), true),
        /requires peer coordinates/,
    );
    const reassigned = structuredClone(action.after);
    reassigned.tables[0]!.node.content![1]!.attrs = { label: 'B' };
    delete reassigned.tables[0]!.node.content![2]!.attrs;
    assert.throws(() => preserve(action.before, reassigned, new Map(), true), /row attributes/);
});

test('unanchored empty and span-only rows cannot hide meaningful attributes', () => {
    for (const span of [1, 2]) {
        const before = rowDocument();
        const table = before.tables[0]!;
        table.cells[0]!.rowspan = span;
        table.cells[0]!.node.attrs = { rowspan: span };
        table.node.content!.splice(
            1,
            0,
            span === 1 ? { type: 'table_row' } : { type: 'table_row', content: [] },
        );
        layout(table);
        preserve(before, structuredClone(before));
        const meaningful = structuredClone(before);
        meaningful.tables[0]!.node.content![1]!.attrs = { label: 'unanchored' };
        assert.throws(() => preserve(meaningful, meaningful), /row.*(identity|attribution)/);
        assert.throws(() => preserve(before, meaningful), /row.*(identity|attribution)/);
    }
});

test('table and row attributes retain opaque node-shaped values', () => {
    for (const owner of ['table', 'row'] as const) {
        const before = rowDocument();
        const node =
            owner === 'table' ? before.tables[0]!.node : before.tables[0]!.node.content![0]!;
        node.attrs = {
            type: 'table_cell',
            attrs: { colspan: 1 },
            content: [{ type: 'text', text: 'opaque' }],
        };
        preserve(before, structuredClone(before));
        const changed = structuredClone(before);
        const changedNode =
            owner === 'table' ? changed.tables[0]!.node : changed.tables[0]!.node.content![0]!;
        delete (changedNode.attrs!.attrs as Record<string, unknown>).colspan;
        assert.throws(() => preserve(before, changed), /attributes/);
    }
});

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
                        content: [
                            {
                                type: 'paragraph',
                                content: [{ type: 'text', text: value }],
                            },
                        ],
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
    evidence.assertTypingContinuation(
        action,
        { actor: 0, sourceId: 'id-0', text: 'typed-' },
        observed(action.after),
    );
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
                    observed(typed().after),
                ),
            /CONTINUITY/,
        );
    }
    assert.throws(
        () =>
            evidence.assertTypingContinuation(
                action,
                { actor: 0, sourceId: 'id-0', text: 'typed-' },
                observed(action.before),
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
                {
                    type: 'paragraph',
                    content: [{ type: 'text', text: 'target' }],
                },
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
        node: {
            type: 'table_cell',
            attrs: { label: 'opaque' },
            content: [{ type: 'paragraph' }],
        },
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
            content: [
                {
                    type: 'paragraph',
                    content: [{ type: 'text', text: 'gap-' }],
                },
            ],
        },
    });
    const intent = { actor: 0, tableSource: '0', position: 2, text: 'gap-' };
    evidence.assertGapContinuation(action, intent, observed(action.after));
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
            () => evidence.assertGapContinuation(changed, intent, observed(action.after)),
            /CONTINUITY/,
        );
    }
    assert.throws(
        () => evidence.assertGapContinuation(action, intent, observed(before)),
        /CONTINUITY/,
    );
});

test('outside typing requires a top-level paragraph and preserves all other outside content', () => {
    assert.equal(typeof evidence.assertUnrelatedContinuation, 'function');
    const action = {
        ...typed(),
        target: null,
        rawBefore: {
            type: 'doc',
            content: [{ type: 'table' }, { type: 'paragraph' }],
        },
        rawAfter: {
            type: 'doc',
            content: [
                { type: 'table' },
                {
                    type: 'paragraph',
                    content: [{ type: 'text', text: 'outside-' }],
                },
            ],
        },
        before: document(),
        after: document(),
    };
    const intent = { actor: 0, index: 1, text: 'outside-' };
    evidence.assertUnrelatedContinuation(action, intent, [action.rawAfter], observed(action.after));
    assert.throws(
        () =>
            evidence.assertUnrelatedContinuation(
                { ...action, target: typed().target },
                intent,
                [action.rawAfter],
                observed(action.after),
            ),
        /CONTINUITY/,
    );
    assert.throws(
        () =>
            evidence.assertUnrelatedContinuation(
                { ...action, rawAfter: action.rawBefore },
                intent,
                [action.rawBefore],
                observed(action.after),
            ),
        /CONTINUITY/,
    );
    assert.throws(
        () =>
            evidence.assertUnrelatedContinuation(
                action,
                intent,
                [action.rawBefore],
                observed(action.after),
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
            assert.deepEqual(
                checkpoint.observations.map((view) => view.kind),
                schedule.kinds.slice(0, schedule.participants),
            );
            assert.ok(checkpoint.observations.every((view) => view.document.tables.length > 0));
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
        assert.equal(
            corpus.continuationPassed({
                ...result,
                checkpoints: badPresentation,
            }),
            false,
        );
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
                    for (const checkpoint of result.checkpoints)
                        assert.deepEqual(
                            checkpoint.observations.map((view) => view.kind),
                            schedule.kinds.slice(0, schedule.participants),
                        );
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
                        assert.deepEqual(result.gap, {
                            observed: true,
                            positions: [],
                        });
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
