import assert from 'node:assert/strict';
import test from 'node:test';
import { Schema } from 'prosemirror-model';
import { schema } from 'prosemirror-schema-basic';
import { EditorState, TextSelection } from 'prosemirror-state';
import { addRowAfter, tableEditing, tableNodes } from 'prosemirror-tables';
import { runSupplementary, supplementaryRequirements } from '../supplementary-continuity.js';
import * as corpus from '../corpus.js';

test('live stock insertion observation identifies the visited hole and retains the stock exception', async () => {
    const module = await import('../browser/row-insertion-observer.js');
    assert.equal(typeof module.observeRowInsertion, 'function');
    const tables = new Schema({
        nodes: schema.spec.nodes.append(
            tableNodes({ tableGroup: 'block', cellContent: 'block+', cellAttributes: {} }),
        ),
        marks: schema.spec.marks,
    });
    const cell = (text: string, colspan = 1) =>
        tables.nodes.table_cell!.create(
            { colspan },
            tables.nodes.paragraph!.create(null, tables.text(text)),
        );
    const doc = tables.nodes.doc!.create(
        null,
        tables.nodes.table!.create(null, [
            tables.nodes.table_row!.create(null, cell('a', 2)),
            tables.nodes.table_row!.create(null, cell('b')),
        ]),
    );
    const state = EditorState.create({
        doc,
        selection: TextSelection.near(doc.resolve(10)),
        plugins: [tableEditing()],
    });
    const command = { type: 'tableCommand', name: 'addRowAfter', at: 10 };
    const observed = module.observeRowInsertion(state, command);
    assert.deepEqual(observed.command, command);
    assert.equal(observed.row, 2);
    assert.equal(observed.referenceRow, 1);
    assert.ok(
        observed.slots.some(
            (slot: any) => slot.branch === 'create' && slot.position === 0 && slot.role === 'row',
        ),
    );
    assert.throws(
        () => addRowAfter(state, (tr) => state.applyTransaction(tr)),
        /Invalid content for node table_row/,
    );
    assert.deepEqual(state.doc.toJSON(), doc.toJSON());
});

test('runner keeps unavailable history separate and never undoes prior history', async () => {
    for (const proof of ['structure', 'history'] as const) {
        const slot = supplementaryRequirements().find(
            (slot) =>
                slot.family === 'overlap' && slot.proof === proof && slot.actorKind === 'rust',
        )!;
        const result = await runSupplementary(slot);
        assert.equal(result.availability, 'verified-native-refusal');
        assert.equal(result.actions.length, 1);
        assert.deepEqual(result.failures, []);
        assert.equal(corpus.continuationPassed(result), proof === 'structure');
        const evidence = await import('../availability-evidence.js');
        assert.equal(evidence.availabilityVerified(result), true);
        for (const mutate of [
            (copy: typeof result) => copy.failures.push('unrelated failure'),
            (copy: typeof result) => {
                copy.checkpoints[1]!.raw.passed = false;
            },
            (copy: typeof result) => {
                copy.checkpoints[1]!.presentation.passed = false;
            },
            (copy: typeof result) => {
                copy.checkpoints[1]!.drain.rounds = 101;
            },
            (copy: typeof result) => {
                copy.checkpoints[1] = { ...copy.checkpoints[1]!, nativeAutonomousRepairWrites: 1 };
            },
            (copy: typeof result) => {
                copy.checkpoints[1]!.textHistoryObservations = [];
            },
            (copy: typeof result) => {
                copy.checkpoints[1]!.textHistoryObservations![0]!.raw.content![0]!.content![0]!.content![0]!.attrs!.rowspan = 3;
            },
            (copy: typeof result) => {
                copy.checkpoints[1]!.observations[0]!.document.tables[0]!.rows = 999;
            },
            (copy: typeof result) => {
                delete copy.checkpoints[1]!.presentation.views;
            },
            (copy: typeof result) => {
                const checkpoint = copy.checkpoints[1]!;
                for (const view of [
                    checkpoint.observations[0]!.document,
                    checkpoint.textHistoryObservations![0]!.document,
                    checkpoint.presentation.views!.native[0]!,
                ])
                    view.tables[0]!.cells[0]!.row = -1;
            },
            (copy: typeof result) => {
                copy.actions = [];
            },
        ]) {
            const copy = structuredClone(JSON.parse(JSON.stringify(result)));
            mutate(copy);
            assert.equal(evidence.availabilityVerified(copy), false);
            assert.equal(corpus.continuationPassed(copy), false);
        }
    }
});

test('actual overlap attempt retains request-bound live observation and typed exception', async () => {
    const slot = supplementaryRequirements().find(
        (slot) =>
            slot.family === 'overlap' &&
            slot.proof === 'structure' &&
            slot.actorKind === 'prosemirror',
    )!;
    const result = await runSupplementary(slot);
    const action = result.actions[0]!;
    assert.ok(action);
    assert.equal(action.commandError?.code, 'INTERNAL_ERROR');
    assert.match(action.commandError.message, /TransformError: Invalid content for node table_row/);
    assert.ok(action.stockRowInsertion?.command, JSON.stringify(action.stockRowInsertion));
    assert.equal(action.stockRowInsertion.command.name, 'addRowAfter');
    assert.deepEqual(action.stockRowInsertion.command, action.request?.payload);
    assert.ok(
        action.stockRowInsertion.slots.some((slot) => slot.position === 0 && slot.role === 'row'),
        JSON.stringify(action.stockRowInsertion),
    );
    assert.ok(action.rawAfter);
    assert.ok(action.after.tables.length);
    const schemaWithTables = new Schema({
        nodes: schema.spec.nodes.append(
            tableNodes({ tableGroup: 'block', cellContent: 'block+', cellAttributes: {} }),
        ),
        marks: schema.spec.marks,
    });
    const doc = schemaWithTables.nodeFromJSON(action.stockRowInsertion.document);
    const diagnostic = EditorState.create({
        doc,
        selection: TextSelection.create(doc, action.stockRowInsertion.selection.anchor),
        plugins: [tableEditing()],
    });
    assert.throws(
        () => addRowAfter(diagnostic, (transaction) => diagnostic.applyTransaction(transaction)),
        /Invalid content for node table_row/,
    );
});

test('availability proves actual malformed refusals and limitations with fail-closed negative controls', async () => {
    const evidence = await import('../availability-evidence.js');
    assert.equal(typeof evidence.assertUnavailableAction, 'function');
    for (const actorKind of ['rust', 'prosemirror', 'tiptap'] as const) {
        const slot = supplementaryRequirements().find(
            (slot) =>
                slot.family === 'overlap' &&
                slot.proof === 'structure' &&
                slot.actorKind === actorKind,
        )!;
        const result = await runSupplementary(slot);
        const action = result.actions[0]!;
        const initial = result.checkpoints[0]!.textHistoryObservations![slot.actor]!;
        const expected =
            actorKind === 'rust' ? 'verified-native-refusal' : 'verified-stock-limitation';
        assert.equal(evidence.assertUnavailableAction(action, slot, initial), expected);
        const reject = (change: (copy: typeof action) => void) => {
            const copy = structuredClone(action);
            change(copy);
            assert.throws(() => evidence.assertUnavailableAction(copy, slot, initial));
        };
        reject((copy) => {
            delete copy.request;
        });
        reject((copy) => {
            copy.request!.payload.at = 99;
        });
        reject((copy) => {
            copy.operation = 'insertText';
        });
        reject((copy) => {
            copy.target = copy.before.tables[0]!.cells[1]!;
        });
        reject((copy) => {
            copy.rawAfter = { type: 'doc' };
        });
        reject((copy) => {
            copy.after.tables[0]!.cells[0]!.node.content = [];
        });
        reject((copy) => {
            copy.autonomous = 1;
        });
        reject((copy) => {
            copy.observationFailure = 'missing';
        });
        const valid = JSON.parse(JSON.stringify(action)) as typeof action;
        const validInitial = structuredClone(initial);
        validInitial.raw = {
            type: 'doc',
            content: [
                {
                    type: 'table',
                    content: [
                        {
                            type: 'table_row',
                            content: [
                                {
                                    type: 'table_cell',
                                    content: [
                                        {
                                            type: 'paragraph',
                                            content: [{ type: 'text', text: 'a' }],
                                        },
                                    ],
                                },
                            ],
                        },
                    ],
                },
            ],
        };
        valid.rawBefore = validInitial.raw;
        assert.throws(
            () => evidence.assertUnavailableAction(valid, slot, validInitial),
            /independently malformed/,
        );
        const ambiguous = structuredClone(initial);
        ambiguous.document.tables[0]!.cells.push(
            structuredClone(ambiguous.document.tables[0]!.cells[0]!),
        );
        assert.throws(
            () => evidence.assertUnavailableAction(action, slot, ambiguous),
            /unique declared target/,
        );
        if (actorKind === 'rust') {
            for (const field of [
                'reason',
                'observed',
                'historyUnchanged',
                'historyMetadataUnchanged',
                'outboxUnchanged',
                'encodedStateUnchanged',
                'contentUnchanged',
                'revisionUnchanged',
            ])
                reject((copy) => {
                    (copy.reply.availability as any)[field] = false;
                });
            for (const field of [
                'historyCounts',
                'historyMetadataItems',
                'outboxCountAndBytes',
                'encodedState',
                'documentRevision',
                'stateRevision',
                'yrsStateEpoch',
            ])
                reject((copy) => {
                    (copy.reply.availability as any).after[field] = null;
                });
            reject((copy) => {
                (copy.reply.availability as any).command = '{}';
            });
            reject((copy) => {
                (copy.reply.availability as any).requestId = 'bogus';
            });
            reject((copy) => {
                copy.reply.type = 'transaction';
            });
            reject((copy) => {
                delete copy.availabilityBoundary!.before.queuedEvents;
                delete copy.availabilityBoundary!.after.queuedEvents;
            });
        } else {
            reject((copy) => {
                delete copy.stockRowInsertion;
            });
            reject((copy) => {
                copy.commandError!.code = 'CONFIG_INVALID';
            });
            reject((copy) => {
                copy.commandError!.message = 'unrelated exception';
            });
            reject((copy) => {
                copy.stockRowInsertion!.referenceRow = 2;
            });
            reject((copy) => {
                copy.stockRowInsertion!.slots = [];
            });
            reject((copy) => {
                copy.stockRowInsertion!.map.fill(1);
            });
            reject((copy) => {
                copy.stockRowInsertion!.command.at = 99;
            });
            reject((copy) => {
                copy.stockRowInsertion!.document = { type: 'doc' };
            });
            reject((copy) => {
                copy.stockRowInsertion!.document.content![0]!.content!.push({ type: 'table_row' });
                copy.rawAfter = structuredClone(copy.stockRowInsertion!.document);
                copy.availabilityBoundary!.after.documentJson = copy.rawAfter as Record<
                    string,
                    unknown
                >;
            });
        }
    }
});

test('availability outcome counts keep successful edits and applied history separate', async () => {
    assert.equal(typeof corpus.structuralAvailabilityOutcome, 'function');
    const slots = supplementaryRequirements()
        .filter((slot) => slot.family === 'overlap' && slot.proof === 'structure')
        .slice(0, 1);
    const result = await runSupplementary(slots[0]!);
    assert.equal(corpus.structuralAvailabilityOutcome(result), 'verified-native-refusal');
    assert.ok(result.tracePath);
    const { executeContinuations } = await import('../continuation-runner.js');
    const summary = await executeContinuations(
        slots,
        async () => result,
        async () => {},
    );
    assert.equal(summary.successfulEdits, 0);
    assert.equal(summary.appliedStructuralHistory, 0);
    assert.equal(summary.availabilityOutcomes['verified-native-refusal'], 1);
    const failed = JSON.parse(JSON.stringify(result));
    failed.failures.push('unrelated');
    assert.equal(corpus.structuralAvailabilityOutcome(failed), 'unexplained-failure');
    const missing = JSON.parse(JSON.stringify(result));
    missing.actions = [];
    assert.equal(corpus.structuralAvailabilityOutcome(missing), 'missing-evidence');
    const mismatched = JSON.parse(JSON.stringify(result));
    mismatched.slot.target = 'b';
    assert.throws(
        () => corpus.continuationCoverage(slots, [mismatched]),
        /availability declaration mismatch/,
    );
});

test('stock span traversal advances map index independently from skipped columns', async () => {
    const { observeRowInsertion } = await import('../browser/row-insertion-observer.js');
    const tables = new Schema({
        nodes: schema.spec.nodes.append(
            tableNodes({ tableGroup: 'block', cellContent: 'block+', cellAttributes: {} }),
        ),
        marks: schema.spec.marks,
    });
    const cell = (text: string, attrs = {}) =>
        tables.nodes.table_cell!.create(
            attrs,
            tables.nodes.paragraph!.create(null, tables.text(text)),
        );
    const doc = tables.nodes.doc!.create(
        null,
        tables.nodes.table!.create(null, [
            tables.nodes.table_row!.create(null, [
                cell('a', { colspan: 2, rowspan: 2 }),
                cell('b', { colspan: 2 }),
            ]),
            tables.nodes.table_row!.create(null, cell('c')),
        ]),
    );
    const state = EditorState.create({ doc, selection: TextSelection.near(doc.resolve(8)) });
    const observed = observeRowInsertion(state, {
        type: 'tableCommand',
        name: 'addRowAfter',
        at: 8,
    });
    assert.equal(observed.row, 1);
    assert.deepEqual(
        observed.slots.map(({ column, index }) => ({ column, index })),
        [
            { column: 0, index: 4 },
            { column: 2, index: 5 },
        ],
    );
    assert.ok(observed.map.includes(0));
    assert.ok(observed.slots.every((slot) => slot.branch === 'span' && slot.position !== 0));
});

test('stock observation bounds hidden attribute payload before serializing the live state', async () => {
    const { observeRowInsertion } = await import('../browser/row-insertion-observer.js');
    const tables = new Schema({
        nodes: schema.spec.nodes.append(
            tableNodes({
                tableGroup: 'block',
                cellContent: 'block+',
                cellAttributes: { payload: { default: null } },
            }),
        ),
        marks: schema.spec.marks,
    });
    const cell = tables.nodes.table_cell!.create(
        { payload: 'x'.repeat(3_000_000) },
        tables.nodes.paragraph!.create(null, tables.text('a')),
    );
    const doc = tables.nodes.doc!.create(
        null,
        tables.nodes.table!.create(null, tables.nodes.table_row!.create(null, cell)),
    );
    const state = EditorState.create({ doc, selection: TextSelection.near(doc.resolve(3)) });
    assert.throws(
        () => observeRowInsertion(state, { type: 'tableCommand', name: 'addRowAfter', at: 3 }),
        /observation budget/,
    );
});

test('usable history requires exact original availability and independent companion declarations', async () => {
    const linkage = await import('../availability-history.js');
    assert.equal(typeof linkage.usableHistoryObligations, 'function');
    const slot = supplementaryRequirements().find(
        (slot) =>
            slot.family === 'overlap' && slot.proof === 'history' && slot.actorKind === 'rust',
    )!;
    const companion = corpus.textHistoryRequirements([slot])[0]!;
    const original = await runSupplementary(slot);
    const text = await runSupplementary(companion);
    const accepted = linkage.usableHistoryObligations([slot], [original], [text]);
    assert.equal(accepted[0]!.usableHistoryProven, true);
    assert.equal(accepted[0]!.structuralHistoryProven, false);
    assert.equal(corpus.continuationPassed(original), false);
    for (const [originals, companions] of [
        [[original], []],
        [[original], [text, text]],
        [[], [text]],
        [[original, original], [text]],
    ] as const)
        assert.throws(() => linkage.usableHistoryObligations([slot], originals, companions));
    const copy = (value: typeof original) => JSON.parse(JSON.stringify(value)) as typeof original;
    for (const mutate of [
        (value: typeof text) => {
            value.failures.push('unrelated failure');
        },
        (value: typeof text) => {
            Object.assign(value, { slot: { ...value.slot, preset: 'tiptap' } });
        },
        (value: typeof text) => {
            Object.assign(value, {
                slot: { ...value.slot, textHistoryTarget: { kind: 'cell-text', text: 'b' } },
            });
        },
        (value: typeof text) => {
            value.checkpoints[0]!.textHistoryObservations![0]!.raw = { type: 'doc' };
        },
    ]) {
        const bad = copy(text);
        mutate(bad);
        assert.throws(() => linkage.usableHistoryObligations([slot], [original], [bad]));
    }
    const bad = copy(original);
    bad.failures.push('unrelated');
    assert.throws(() => linkage.usableHistoryObligations([slot], [bad], [text]));
});

test('successful structural and normalization lifetime outcomes retain their own proof predicates', async () => {
    for (const history of ['no-remote', 'remote'] as const) {
        const slot = supplementaryRequirements().find(
            (slot) => slot.family === 'native-owned-normalization' && slot.history === history,
        )!;
        const result = await runSupplementary(slot);
        assert.equal(corpus.continuationPassed(result), true, JSON.stringify(result.failures));
        assert.equal(corpus.structuralAvailabilityOutcome(result), 'successful-edit');
        const linkage = await import('../availability-history.js');
        assert.throws(() => linkage.usableHistoryObligations([slot], [result], []));
    }
});
