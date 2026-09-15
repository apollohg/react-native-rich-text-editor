import assert from 'node:assert/strict';
import test from 'node:test';
import * as supplementary from '../supplementary-continuity.js';
import * as evidence from '../supplementary-evidence.js';
import type { RecordedAction } from '../scenario-evidence.js';
import type { EffectiveDocument, JsonNode } from '../peer-protocol.js';
import * as Y from 'yjs';
import { yXmlFragmentToProsemirrorJSON } from 'y-prosemirror';
import * as setup from '../supplementary-setup.js';
import { assertConverged, canonicalDocumentShape } from '../assertions.js';
import { call, exchangeUntilIdle, snapshot, tableFixture, withPeers } from '../controller.js';
import { assertEffectivePresentation } from '../presentation-semantics.js';
import { observeEvidence } from '../evidence-observer.js';
import { realCells } from '../scenario-evidence.js';
import { continuationPassed } from '../corpus.js';
import { assertGapContinuation } from '../continuity-evidence.js';

test('named overlap native structural action applies to its declared source', async () => {
    const slot = supplementary
        .supplementaryRequirements()
        .find(
            (slot) =>
                slot.topology === 'native/native' &&
                slot.preset === 'prosemirror' &&
                slot.family === 'overlap' &&
                slot.actor === 0 &&
                slot.proof === 'structure',
        )!;
    const result = await supplementary.runSupplementary(slot);
    assert.equal(
        continuationPassed(result),
        true,
        JSON.stringify({
            key: slot.key,
            failures: result.failures,
            trace: result.tracePath,
        }),
    );
});

test('named overlap stock structural action applies to its declared source', async () => {
    const slot = supplementary
        .supplementaryRequirements()
        .find(
            (slot) =>
                slot.topology === 'two-web-control' &&
                slot.preset === 'prosemirror' &&
                slot.family === 'overlap' &&
                slot.actor === 0 &&
                slot.proof === 'structure',
        )!;
    const result = await supplementary.runSupplementary(slot);
    assert.equal(
        continuationPassed(result),
        true,
        JSON.stringify({
            key: slot.key,
            failures: result.failures,
            trace: result.tracePath,
        }),
    );
});

test('named overlap presentation stays compatible after unrelated stock typing', async () => {
    const slot = supplementary
        .supplementaryRequirements()
        .find(
            (slot) =>
                slot.topology === 'native/web' &&
                slot.preset === 'prosemirror' &&
                slot.family === 'overlap' &&
                slot.proof === 'unrelated-web',
        )!;
    const result = await supplementary.runSupplementary(slot);
    assert.equal(
        continuationPassed(result),
        true,
        JSON.stringify({
            key: slot.key,
            checkpoints: result.checkpoints.map(({ boundary, presentation }) => ({
                boundary,
                presentation,
            })),
            trace: result.tracePath,
        }),
    );
});

for (const preset of ['prosemirror', 'tiptap'] as const)
    test(`${preset} ordinary native projection compares independently with both web histories`, async () => {
        const slot = supplementary
            .supplementaryRequirements()
            .find(
                (slot) =>
                    slot.preset === preset &&
                    slot.family === 'overlap' &&
                    slot.proof === 'unrelated-web',
            )!;
        await withPeers(
            [preset, preset, 'rust'],
            async ([author, counterpart, native]) => {
                const peers = [author, counterpart, native];
                const updateBase64 = setup.supplementarySeed(slot);
                for (const peer of peers) await call(peer, 'applyUpdate', { updateBase64 });
                await exchangeUntilIdle(peers);
                await assertConverged(peers);
                const initial = await observeEvidence(native);
                const sources = new Map(
                    realCells(initial).map((cell) => [cell.sourceId, cell.node.content]),
                );
                for (const web of [author, counterpart])
                    assert.equal(
                        assertEffectivePresentation(initial, await observeEvidence(web)).tables[0]!
                            .kind,
                        'overlap-fallback',
                    );

                const check = async () => {
                    await exchangeUntilIdle(peers);
                    await assertConverged(peers);
                    const projected = await observeEvidence(native);
                    assert.equal(projected.tables[0]!.overlap, undefined);
                    for (const cell of realCells(projected).filter((cell) =>
                        sources.has(cell.sourceId),
                    ))
                        assert.deepEqual(cell.node.content, sources.get(cell.sourceId));
                    assert.equal(
                        realCells(projected).filter((cell) => sources.has(cell.sourceId)).length,
                        sources.size,
                    );
                    assert.deepEqual(
                        assertEffectivePresentation(projected, await observeEvidence(author))
                            .tables,
                        [
                            {
                                source: '0',
                                kind: 'ordinary-safe-overlap',
                                evidence: 'live-overlap',
                            },
                        ],
                    );
                    const other = await observeEvidence(counterpart);
                    assert.equal(other.tables[0]!.overlap, undefined);
                    assert.deepEqual(assertEffectivePresentation(projected, other).tables, [
                        { source: '0', kind: 'exact' },
                    ]);
                    for (const peer of peers)
                        assert.equal((await snapshot(peer)).autonomousRepairWrites, 0);
                };
                assert.equal(
                    (await call(author, 'command', { type: 'appendParagraph' })).documentChanged,
                    true,
                );
                await check();
                const display = (await snapshot(author)).displayJson as JsonNode;
                const size = (node: JsonNode): number =>
                    node.type === 'text'
                        ? node.text!.length
                        : 2 + (node.content ?? []).reduce((n, child) => n + size(child), 0);
                const at = 1 + display.content!.slice(0, -1).reduce((n, node) => n + size(node), 0);
                assert.equal(
                    (
                        await call(author, 'command', {
                            type: 'insertText',
                            text: 'outside-',
                            at,
                        })
                    ).documentChanged,
                    true,
                );
                await check();
                for (const peer of peers)
                    assert.equal(
                        ((await snapshot(peer)).documentJson as JsonNode).content!.at(-1)!
                            .content![0]!.text,
                        'outside-',
                    );
            },
            tableFixture(preset),
        );
    });

test('overlap moved gap retains command-bound source attribution', async () => {
    const slot = supplementary
        .supplementaryRequirements()
        .find(
            (slot) =>
                slot.topology === 'two-web-control' &&
                slot.preset === 'prosemirror' &&
                slot.family === 'overlap' &&
                slot.actor === 0 &&
                slot.proof === 'web-gap' &&
                slot.gapRow === 1,
        )!;
    const result = await supplementary.runSupplementary(slot);
    const action = result.actions[0]!;
    assert.ok(action.reply['textTarget'], 'actual command-bound mapped target');
    const target = action.reply['textTarget'] as {
        beforeCellPosition: number;
        afterCellPosition: number;
        sourceId: string;
    };
    assert.notEqual(target.beforeCellPosition, target.afterCellPosition);
    assert.equal(result.status, 'proven', JSON.stringify(result.failures));
    const intent = {
        actor: slot.actor,
        tableSource: action.before.tables[0]!.source,
        position: target.beforeCellPosition,
        text: 'gap-',
    };
    for (const mutation of [
        (changed: RecordedAction) => {
            delete changed.reply['textTarget'];
        },
        (changed: RecordedAction) => {
            (changed.reply['textTarget'] as Record<string, unknown>)['requestedPosition'] = 1;
        },
        (changed: RecordedAction) => {
            (changed.reply['textTarget'] as Record<string, unknown>)['beforeCellPosition'] = 1;
        },
        (changed: RecordedAction) => {
            (changed.reply['textTarget'] as Record<string, unknown>)['afterCellPosition'] = 2;
        },
        (changed: RecordedAction) => {
            (changed.reply['textTarget'] as Record<string, unknown>)['sourceId'] = realCells(
                changed.before,
            )[0]!.sourceId;
        },
        (changed: RecordedAction) => {
            const other = realCells(changed.after).find(
                (cell) => cell.sourceId !== target.sourceId,
            )!;
            other.node.content![0]!.content = [{ type: 'text', text: 'gap-' }];
        },
    ]) {
        const changed = structuredClone(action);
        mutation(changed);
        assert.throws(
            () =>
                assertGapContinuation(changed, intent, [
                    { kind: action.kind, document: changed.after },
                ]),
            /CONTINUITY/,
        );
    }
});

test('crossing-rowspan native redo remains unchanged by a full-state exchange', async () => {
    const slot = supplementary
        .supplementaryRequirements()
        .find(
            (slot) =>
                slot.topology === 'native/native' &&
                slot.preset === 'prosemirror' &&
                slot.family === 'crossing-rowspan' &&
                slot.actor === 0 &&
                slot.proof === 'history',
        )!;
    const result = await supplementary.runSupplementary(slot);
    assert.equal(
        continuationPassed(result),
        true,
        JSON.stringify({
            key: slot.key,
            failures: result.failures,
            checkpoints: result.checkpoints.map(({ boundary, raw }) => ({
                boundary,
                raw,
            })),
            trace: result.tracePath,
        }),
    );
});

test('native lifetime executes applied history with and without remote content', async () => {
    for (const history of ['no-remote', 'remote']) {
        const slot = supplementary
            .supplementaryRequirements()
            .find(
                (slot) =>
                    slot.topology === 'native/web' &&
                    slot.family === 'native-owned-normalization' &&
                    slot.history === history,
            )!;
        const result = await supplementary.runSupplementary(slot);
        assert.equal(
            continuationPassed(result),
            true,
            JSON.stringify({
                key: slot.key,
                failures: result.failures,
                trace: result.tracePath,
            }),
        );
    }
});

test('supplementary overlap typing uses the raw setup with fresh live presentation checks', async () => {
    assert.equal(typeof supplementary.runSupplementary, 'function');
    const slot = supplementary
        .supplementaryRequirements()
        .find(
            (slot) =>
                slot.topology === 'native/web' &&
                slot.family === 'overlap' &&
                slot.actorKind === 'rust' &&
                slot.proof === 'typing',
        )!;
    const result = await supplementary.runSupplementary(slot);
    assert.equal(
        continuationPassed(result),
        true,
        JSON.stringify({ failures: result.failures, trace: result.tracePath }),
    );
    assert.ok(
        result.checkpoints[0]!.presentation.comparisons.some((check) =>
            check.tables.some(
                (table) => table.kind === 'overlap-fallback' && table.evidence === 'live-overlap',
            ),
        ),
    );
});

test('raw supplementary setup retains original seed identities and has no native placeholder', async () => {
    assert.equal(typeof setup.withSupplementarySetup, 'function');
    for (const preset of ['prosemirror', 'tiptap']) {
        const slot = supplementary
            .supplementaryRequirements()
            .find(
                (slot) =>
                    slot.preset === preset &&
                    slot.topology === 'native/native' &&
                    slot.family === 'overlap',
            )!;
        await setup.withSupplementarySetup(slot, async (live) => {
            assert.equal(live.participants.length, 2);
            const identities = [];
            for (const peer of live.participants) {
                const state = await snapshot(peer);
                assert.deepEqual(
                    canonicalDocumentShape(state.documentJson),
                    canonicalDocumentShape(
                        supplementary.supplementaryFixture('overlap', slot.preset),
                    ),
                );
                assert.equal(state.autonomousRepairWrites, 0);
                const cells = realCells(await observeEvidence(peer));
                assert.equal(cells.length, 3);
                identities.push(cells.map((cell) => cell.sourceId));
            }
            assert.deepEqual(identities[0], identities[1]);
            assert.ok(live.reference);
        });
    }
});

test('supplementary binary seeds preserve every original raw fixture without normalization', () => {
    for (const slot of supplementary.supplementaryRequirements()) {
        const doc = new Y.Doc();
        try {
            Y.applyUpdate(doc, Buffer.from(setup.supplementarySeed(slot), 'base64'));
            assert.deepEqual(
                canonicalDocumentShape(
                    yXmlFragmentToProsemirrorJSON(doc.getXmlFragment('prosemirror')),
                ),
                canonicalDocumentShape(
                    supplementary.supplementaryFixture(slot.family, slot.preset),
                ),
            );
        } finally {
            doc.destroy();
        }
    }
});

function observedRows(rows: string[][]): EffectiveDocument {
    let position = 1;
    const cells: EffectiveDocument['tables'][number]['cells'] = [];
    const content = rows.map((row, rowIndex) => {
        position += 1;
        const content = row.map((sourceId, index) => {
            const node: JsonNode = {
                type: 'table_cell',
                content: [{ type: 'paragraph' }],
            };
            cells.push({
                sourceId,
                source: `0.${rowIndex}.${index}`,
                position,
                row: null,
                column: null,
                rowspan: null,
                colspan: null,
                node,
            });
            position += 4;
            return node;
        });
        position += 1;
        return { type: 'table_row', content };
    });
    return {
        tables: [
            {
                source: '0',
                parentCell: null,
                pathWithinCell: '0',
                position: 0,
                rows: rows.length,
                columns: 0,
                widths: null,
                node: { type: 'table', content },
                cells,
            },
        ],
    };
}

function insertion(): RecordedAction {
    const before = observedRows([['a', 'b'], ['c']]);
    const after = observedRows([
        ['a', 'b'],
        ['c', 'repair'],
        ['insert-a', 'insert-b'],
    ]);
    return {
        actor: 0,
        kind: 'rust',
        operation: 'addRow',
        reply: { documentChanged: true },
        before,
        after,
        rawBefore: { type: 'doc', content: [before.tables[0]!.node] },
        rawAfter: { type: 'doc', content: [after.tables[0]!.node] },
        target: before.tables[0]!.cells[2]!,
        head: null,
        passes: 1,
        autonomous: 0,
        targetGridValid: true,
    };
}

test('normalization ownership excludes authored row cells and pre-existing repair sources', () => {
    const action = insertion();
    assert.equal(evidence.normalizationTarget(action, 0, 'c'), 'repair');
    for (const mutate of [
        (a: RecordedAction) => {
            a.passes = 0;
        },
        (a: RecordedAction) => {
            a.reply = { documentChanged: false };
        },
        (a: RecordedAction) => {
            a.after = observedRows([['a', 'b'], ['c'], ['insert-a', 'insert-b']]);
        },
        (a: RecordedAction) => {
            a.before = observedRows([
                ['a', 'b'],
                ['c', 'repair'],
            ]);
        },
        (a: RecordedAction) => {
            a.target = a.before.tables[0]!.cells[0]!;
        },
    ]) {
        const invalid = structuredClone(action);
        mutate(invalid);
        assert.throws(() => evidence.normalizationTarget(invalid, 0, 'c'), /CONTINUITY/);
    }
});

function remoteHistory(): RecordedAction[] {
    const action = insertion();
    const remote = structuredClone(action);
    remote.actor = 1;
    remote.operation = 'insertText';
    remote.passes = 0;
    remote.before = structuredClone(action.after);
    remote.after = structuredClone(action.after);
    remote.target = remote.before.tables[0]!.cells.find((cell) => cell.sourceId === 'repair')!;
    remote.after.tables[0]!.cells.find(
        (cell) => cell.sourceId === 'repair',
    )!.node.content![0]!.content = [{ type: 'text', text: 'remote-' }];
    remote.rawBefore = action.rawAfter;
    remote.rawAfter = { type: 'doc', content: [remote.after.tables[0]!.node] };
    const undo = structuredClone(remote);
    undo.actor = 0;
    undo.operation = 'undo';
    undo.reply = { applied: true };
    undo.before = structuredClone(remote.after);
    undo.after = observedRows([
        ['a', 'b'],
        ['c', 'repair'],
    ]);
    undo.after.tables[0]!.cells.find(
        (cell) => cell.sourceId === 'repair',
    )!.node.content![0]!.content = [{ type: 'text', text: 'remote-' }];
    undo.rawBefore = remote.rawAfter;
    undo.rawAfter = { type: 'doc', content: [undo.after.tables[0]!.node] };
    const redo = structuredClone(undo);
    redo.operation = 'redo';
    redo.before = structuredClone(undo.after);
    redo.after = structuredClone(remote.after);
    redo.rawBefore = undo.rawAfter;
    redo.rawAfter = remote.rawAfter;
    return [action, remote, undo, redo];
}

test('remote lifetime rejects consistently corrupted redo structure and attributes', () => {
    for (const corrupt of [
        (row: JsonNode) => {
            row.attrs = { label: 'wrong-redo-row' };
        },
        (row: JsonNode) => {
            row.content![0]!.attrs = { label: 'wrong-redo-cell' };
        },
        (row: JsonNode) => {
            row.content![0]!.content![0]!.content = [{ type: 'text', text: 'unexpected' }];
        },
        (row: JsonNode) => {
            row.content!.push({ type: 'table_cell', content: [{ type: 'paragraph' }] });
        },
    ]) {
        const actions = remoteHistory();
        const redo = actions[3]!;
        corrupt(redo.after.tables[0]!.node.content![2]!);
        redo.rawAfter = { type: 'doc', content: [redo.after.tables[0]!.node] };
        assert.throws(
            () =>
                evidence.assertNativeRemoteLifetime(
                    actions,
                    { actor: 0, remoteActor: 1, sourceId: 'c', text: 'remote-' },
                    actions
                        .slice(1)
                        .map((action) => [{ kind: action.kind, document: action.after }]),
                ),
            /CONTINUITY/,
        );
    }
});

test('remote lifetime permits fresh authored redo identities with the declared effect', () => {
    const actions = remoteHistory();
    for (const cell of actions[3]!.after.tables[0]!.cells)
        if (cell.sourceId?.startsWith('insert-')) cell.sourceId = `redo-${cell.sourceId}`;
    evidence.assertNativeRemoteLifetime(
        actions,
        { actor: 0, remoteActor: 1, sourceId: 'c', text: 'remote-' },
        actions.slice(1).map((action) => [{ kind: action.kind, document: action.after }]),
    );
});

test('native-owned remote lifetime requires applied changing history and exactly one preserved remote payload', () => {
    const actions = remoteHistory();
    const check = (candidate: RecordedAction[]) =>
        evidence.assertNativeRemoteLifetime(
            candidate,
            { actor: 0, remoteActor: 1, sourceId: 'c', text: 'remote-' },
            candidate.slice(1).map((action) => [{ kind: action.kind, document: action.after }]),
        );
    check(actions);
    for (const mutate of [
        (a: RecordedAction[]) => {
            a.splice(2, 1);
        },
        (a: RecordedAction[]) => {
            a[2]!.reply = { applied: false };
        },
        (a: RecordedAction[]) => {
            a[2]!.rawAfter = a[2]!.rawBefore;
        },
        (a: RecordedAction[]) => {
            a[3]!.rawAfter = a[3]!.rawBefore;
        },
        (a: RecordedAction[]) => {
            a[1]!.target = a[1]!.before.tables[0]!.cells[0]!;
        },
        (a: RecordedAction[]) => {
            a[2]!.after.tables[0]!.cells.find(
                (cell) => cell.sourceId === 'repair',
            )!.node.content![0]!.content = [];
        },
        (a: RecordedAction[]) => {
            a[3]!.after.tables[0]!.cells[0]!.node.content![0]!.content = [
                { type: 'text', text: 'remote-' },
            ];
        },
        (a: RecordedAction[]) => {
            a[2]!.passes = 1;
        },
    ]) {
        const invalid = structuredClone(actions);
        mutate(invalid);
        assert.throws(() => check(invalid), /CONTINUITY|EVIDENCE/);
    }
});

test('lifetime history settlement preserves newly restored source attributes', () => {
    const actions = remoteHistory();
    const observations = actions
        .slice(1)
        .map((action) => [{ kind: action.kind, document: structuredClone(action.after) }]);
    observations[2]![0]!.document.tables[0]!.cells.find(
        (cell) => cell.sourceId === 'insert-a',
    )!.node.attrs = { label: 'corrupt' };
    assert.throws(
        () =>
            evidence.assertNativeRemoteLifetime(
                actions,
                { actor: 0, remoteActor: 1, sourceId: 'c', text: 'remote-' },
                observations,
            ),
        /CONTINUITY/,
    );
});

test('remote lifetime accepts declared cell defaults across peer observations', () => {
    const actions = remoteHistory();
    const observations = actions
        .slice(1)
        .map((action) => [{ kind: action.kind, document: structuredClone(action.after) }]);
    for (const group of observations)
        group[0]!.document.tables[0]!.cells.find((cell) => cell.sourceId === 'repair')!.node.attrs =
            { colspan: 1, rowspan: 1, colwidth: null };
    evidence.assertNativeRemoteLifetime(
        actions,
        { actor: 0, remoteActor: 1, sourceId: 'c', text: 'remote-' },
        observations,
    );
});

test('native-owned no-remote history restores complete irregular content and removes its repair target', () => {
    const action = insertion();
    const undo: RecordedAction = {
        ...structuredClone(action),
        operation: 'undo',
        before: action.after,
        after: action.before,
        rawBefore: action.rawAfter,
        rawAfter: action.rawBefore,
        reply: { applied: true },
        passes: 0,
    };
    const redo: RecordedAction = {
        ...structuredClone(action),
        operation: 'redo',
        reply: { applied: true },
        passes: 0,
    };
    const check = (actions: RecordedAction[]) =>
        evidence.assertNativeNoRemoteLifetime(
            actions,
            { actor: 0, sourceId: 'c' },
            actions.slice(1).map((a) => [{ kind: a.kind, document: a.after }]),
        );
    check([action, undo, redo]);
    for (const mutate of [
        (a: RecordedAction[]) => {
            a[1]!.rawAfter = a[1]!.rawBefore;
        },
        (a: RecordedAction[]) => {
            a[2]!.reply = { applied: false };
        },
        (a: RecordedAction[]) => {
            a[1]!.passes = 1;
        },
        (a: RecordedAction[]) => {
            a[1]!.after = a[0]!.after;
        },
    ]) {
        const invalid = structuredClone([action, undo, redo]);
        mutate(invalid);
        assert.throws(() => check(invalid), /CONTINUITY|EVIDENCE/);
    }
});

test('safe source-row insertion rejects skipped and wrong-footprint overlap actions without logical geometry', () => {
    const action = insertion();
    evidence.assertSourceRowInsertion(action, { actor: 0, sourceId: 'c' }, [
        { kind: 'rust', document: action.after },
    ]);
    for (const mutate of [
        (a: RecordedAction) => {
            a.after = a.before;
        },
        (a: RecordedAction) => {
            a.after = observedRows([
                ['a', 'b'],
                ['insert-a', 'insert-b'],
                ['c', 'repair'],
            ]);
        },
        (a: RecordedAction) => {
            a.reply = { documentChanged: false };
        },
        (a: RecordedAction) => {
            a.target = a.before.tables[0]!.cells[0]!;
        },
    ]) {
        const invalid = structuredClone(action);
        mutate(invalid);
        assert.throws(
            () =>
                evidence.assertSourceRowInsertion(invalid, { actor: 0, sourceId: 'c' }, [
                    { kind: 'rust', document: invalid.after },
                ]),
            /CONTINUITY/,
        );
    }
});

test('crossing-rowspan proof requires the same real spanning source to cross the actual inserted boundary', () => {
    const action = insertion();
    action.target = action.before.tables[0]!.cells[1]!;
    action.before.tables[0]!.cells[0]!.node.attrs = { rowspan: 2 };
    action.after = observedRows([['a', 'b'], ['insert'], ['c']]);
    action.after.tables[0]!.cells[0]!.node.attrs = { rowspan: 3 };
    const check = (candidate: RecordedAction) =>
        evidence.assertCrossingRowspan(candidate, { actor: 0, sourceId: 'b', spanningId: 'a' }, [
            { kind: 'rust', document: candidate.after },
        ]);
    check(action);
    for (const mutate of [
        (a: RecordedAction) => {
            a.before.tables[0]!.cells[0]!.node.attrs!.rowspan = 1;
        },
        (a: RecordedAction) => {
            a.after.tables[0]!.cells[0]!.node.attrs!.rowspan = 2;
        },
        (a: RecordedAction) => {
            a.target = a.before.tables[0]!.cells[2]!;
        },
    ]) {
        const invalid = structuredClone(action);
        mutate(invalid);
        assert.throws(() => check(invalid), /CONTINUITY/);
    }
});

test('supplementary requirements declare actors, independent histories and distinct gap targets', () => {
    const slots = supplementary.supplementaryRequirements();
    assert.equal(new Set(slots.map((slot) => slot.key)).size, slots.length);
    const counts = Object.fromEntries(
        ['overlap', 'crossing-rowspan', 'native-owned-normalization', 'nested-gaps'].map(
            (family) => [family, slots.filter((slot) => slot.family === family).length],
        ),
    );
    assert.deepEqual(counts, {
        overlap: 112,
        'crossing-rowspan': 36,
        'native-owned-normalization': 18,
        'nested-gaps': 20,
    });
    assert.equal(
        slots.filter((slot) => slot.family === 'overlap' && slot.proof === 'web-gap').length,
        30,
    );
    for (const slot of slots) {
        assert.ok(slot.key.startsWith('supplementary :: '));
        assert.ok(slot.actor < slot.schedule.participants);
        if (slot.family === 'native-owned-normalization') {
            assert.equal(slot.actorKind, 'rust');
            assert.ok(slot.history === 'no-remote' || slot.remoteActor !== undefined);
            if (slot.remoteActor !== undefined) assert.notEqual(slot.remoteActor, slot.actor);
        }
        if (slot.family === 'nested-gaps')
            assert.ok(['first-nested', 'second-nested'].includes(slot.target));
    }
});

test('named fixtures retain real overlap and an actual crossing rowspan independently of actions', () => {
    const overlap = supplementary.supplementaryFixture('overlap', 'prosemirror');
    assert.deepEqual(
        overlap.content![0]!.content!.map((row) => (row.content ?? []).map((cell) => cell.attrs)),
        [
            [
                { colspan: 1, rowspan: 1 },
                { colspan: 1, rowspan: 2 },
            ],
            [{ colspan: 2, rowspan: 3 }],
            [],
        ],
    );
    const crossing = supplementary.supplementaryFixture('crossing-rowspan', 'prosemirror');
    assert.equal(crossing.content![0]!.content![0]!.content![0]!.attrs!.rowspan, 2);
    assert.equal(crossing.content![0]!.content![1]!.content!.length, 1);
});

test('supplementary registrations cannot claim evidence through the ordinary authored-table setup', () => {
    const slot = supplementary.supplementaryRequirements()[0]!;
    assert.throws(
        () =>
            slot.schedule.scenario.proves!({
                seededTable: {},
                settledTable: {},
                fixtureTable: {},
            }),
        /raw supplementary setup/,
    );
});

test('gap applicability observes the designated display row and rejects missing wrappers', () => {
    const view = observedRows([['a'], ['gap-one'], ['gap-two']]);
    for (const cell of view.tables[0]!.cells.slice(1)) {
        cell.source = null;
        delete cell.sourceId;
    }
    const slots = supplementary
        .supplementaryRequirements()
        .filter((slot) => slot.family === 'overlap' && slot.proof === 'web-gap')
        .slice(0, 3);
    assert.deepEqual(evidence.supplementaryGaps(view, slots[0]!), []);
    assert.equal(
        evidence.supplementaryGaps(view, slots[1]!)[0]!.cell.position,
        view.tables[0]!.cells[1]!.position,
    );
    assert.equal(
        evidence.supplementaryGaps(view, slots[2]!)[0]!.cell.position,
        view.tables[0]!.cells[2]!.position,
    );
    assert.throws(() => evidence.supplementaryGaps(observedRows([['a']]), slots[2]!), /CONTINUITY/);
});
