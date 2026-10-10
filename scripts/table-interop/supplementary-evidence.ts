import assert from 'node:assert/strict';
import {
    assertContinuationHistory,
    assertSourcePreservation,
    assertTypingContinuation,
    nodeSize,
    requireContinuity,
    type ContinuationObservation,
} from './continuity-evidence.js';
import { cellText, realCells, type RecordedAction } from './scenario-evidence.js';
import { canonicalDocumentShape } from './assertions.js';
import type { EffectiveDocument, EffectiveTable, JsonNode, PeerKind } from './peer-protocol.js';
import type { SupplementarySlot } from './supplementary-continuity.js';

export function supplementaryGaps(view: EffectiveDocument, slot: SupplementarySlot) {
    const matches = view.tables.filter((table) =>
        slot.family === 'overlap'
            ? table.parentCell === null
            : table.cells.some(
                  (cell) =>
                      cell.sourceId &&
                      cellText(cell.node) ===
                          (slot.target === 'first-nested' ? 'first-a' : 'second-a'),
              ),
    );
    requireContinuity(matches.length === 1, 'designated gap table observed');
    const table = matches[0]!;
    let cells = table.cells;
    if (slot.gapRow !== undefined) {
        const row = table.node.content?.[slot.gapRow];
        requireContinuity(row, 'designated gap row wrapper observed');
        const start =
            table.position +
            1 +
            table.node
                .content!.slice(0, slot.gapRow)
                .reduce((size, node) => size + nodeSize(node, slot.actorKind), 0);
        const end = start + nodeSize(row, slot.actorKind);
        cells = cells.filter((cell) => cell.position > start && cell.position < end);
    }
    return cells.filter((cell) => cell.source === null).map((cell) => ({ table, cell }));
}

function sourceRows(table: EffectiveTable, kind: PeerKind): Map<string, number> {
    const rows = new Map<string, number>();
    let position = table.position + 1;
    for (const [index, row] of (table.node.content ?? []).entries()) {
        let start = position + 1;
        for (const node of row.content ?? []) {
            const cell = table.cells.find((cell) => cell.position === start);
            if (cell?.sourceId) rows.set(cell.sourceId, index);
            start += nodeSize(node, kind);
        }
        position += nodeSize(row, kind);
    }
    requireContinuity(
        table.cells.filter((cell) => cell.sourceId).every((cell) => rows.has(cell.sourceId!)),
        'source row observation',
    );
    return rows;
}

export function assertSourceRowInsertion(
    action: RecordedAction,
    intent: { actor: number; sourceId: string },
    settled: readonly ContinuationObservation[],
): void {
    requireContinuity(
        action.actor === intent.actor &&
            action.operation === 'addRow' &&
            !action.observationFailure,
        'source insertion actor/operation',
    );
    requireContinuity(
        action.target?.sourceId === intent.sourceId && action.reply['documentChanged'] === true,
        'source insertion applied target',
    );
    requireContinuity(
        action.autonomous === 0 &&
            (action.kind !== 'rust' || (action.targetGridValid === true && action.passes <= 2)),
        'source insertion normalization',
    );
    const before = action.before.tables.find((table) =>
        table.cells.some((cell) => cell.sourceId === intent.sourceId),
    );
    requireContinuity(before, 'source insertion original table');
    const rows = sourceRows(before, action.kind);
    const targetRow = rows.get(intent.sourceId);
    requireContinuity(targetRow !== undefined, 'source insertion row target');
    const after = action.after.tables.find((table) =>
        table.cells.some((cell) => cell.sourceId === intent.sourceId),
    );
    requireContinuity(after, 'source insertion resulting table');
    requireContinuity(
        after.node.content!.length === before.node.content!.length + 1,
        'source insertion row count',
    );
    const current = sourceRows(after, action.kind);
    for (const [id, row] of rows)
        requireContinuity(
            current.get(id) === row + (row > targetRow ? 1 : 0),
            'source insertion row footprint',
        );
    const authored = [...current].filter(([id, row]) => !rows.has(id) && row === targetRow + 1);
    requireContinuity(authored.length > 0, 'source insertion new row sources');
    requireContinuity(settled.length > 0, 'source insertion settled observations');
    for (const view of [{ kind: action.kind, document: action.after }, ...settled]) {
        const coordinates = { before: action.kind, after: view.kind };
        assertSourcePreservation(action.before, view.document, new Map(), true, coordinates);
        assertSourcePreservation(action.after, view.document, new Map(), true, coordinates);
    }
}

export function normalizationTarget(
    action: RecordedAction,
    actor: number,
    sourceId: string,
): string {
    assertSourceRowInsertion(action, { actor, sourceId }, [
        { kind: action.kind, document: action.after },
    ]);
    requireContinuity(
        action.kind === 'rust' && action.passes > 0 && action.passes <= 2,
        'native-owned normalization passes',
    );
    const beforeIds = new Set(realCells(action.before).map((cell) => cell.sourceId));
    const table = action.after.tables.find((table) =>
        table.cells.some((cell) => cell.sourceId === sourceId),
    )!;
    const rows = sourceRows(table, action.kind);
    const row = rows.get(sourceId);
    const created = table.cells.filter(
        (cell) => cell.sourceId && !beforeIds.has(cell.sourceId) && rows.get(cell.sourceId) === row,
    );
    assert.equal(created.length, 1, 'TBL21 CONTINUITY native-owned original-row target');
    return created[0]!.sourceId!;
}

export function assertCrossingRowspan(
    action: RecordedAction,
    intent: { actor: number; sourceId: string; spanningId: string },
    settled: readonly ContinuationObservation[],
): void {
    assertSourceRowInsertion(action, intent, settled);
    const table = action.before.tables.find((table) =>
        table.cells.some((cell) => cell.sourceId === intent.sourceId),
    )!;
    const spanning = table.cells.find((cell) => cell.sourceId === intent.spanningId);
    const rows = sourceRows(table, action.kind);
    const span = spanning?.node.attrs?.['rowspan'];
    const start = rows.get(intent.spanningId);
    const boundary = rows.get(intent.sourceId)! + 1;
    requireContinuity(
        typeof span === 'number' &&
            start !== undefined &&
            start < boundary &&
            start + span > boundary,
        'genuine crossing rowspan',
    );
    for (const view of [{ kind: action.kind, document: action.after }, ...settled]) {
        const current = realCells(view.document).find(
            (cell) => cell.sourceId === intent.spanningId,
        );
        requireContinuity(
            current?.node.attrs?.['rowspan'] === span + 1,
            'same crossing source span increment',
        );
    }
}

function nodeAtSource(document: JsonNode, source: string): JsonNode {
    let node = document;
    for (const part of source.split('.')) {
        requireContinuity(/^\d+$/.test(part), 'lifetime source path');
        const child = node.content?.[Number(part)];
        requireContinuity(child, 'lifetime source node');
        node = child;
    }
    return node;
}

function remoteHistoryEffects(action: RecordedAction, targetId: string, payload: JsonNode) {
    const target = realCells(action.after).find((cell) => cell.sourceId === targetId)!;
    requireContinuity(target.source, 'lifetime normalization source path');
    const path = target.source.split('.');
    const cellIndex = Number(path.pop());
    const rowSource = path.join('.');
    const undone = structuredClone(action.rawBefore) as JsonNode;
    const redone = structuredClone(action.rawAfter) as JsonNode;
    const originalRow = nodeAtSource(undone, rowSource);
    requireContinuity(
        originalRow.content && cellIndex <= originalRow.content.length,
        'lifetime remnant original row',
    );
    originalRow.content.splice(cellIndex, 0, structuredClone(payload));
    nodeAtSource(redone, rowSource).content![cellIndex] = structuredClone(payload);
    return [undone, redone];
}

export function assertNativeRemoteLifetime(
    actions: readonly RecordedAction[],
    intent: {
        actor: number;
        remoteActor: number;
        sourceId: string;
        text: string;
    },
    settled: readonly (readonly ContinuationObservation[])[],
): void {
    requireContinuity(
        actions.length === 4 && settled.length === 3 && settled.every((views) => views.length > 0),
        'lifetime action/checkpoint boundaries',
    );
    const [action, remote, undo, redo] = actions as [
        RecordedAction,
        RecordedAction,
        RecordedAction,
        RecordedAction,
    ];
    const targetId = normalizationTarget(action, intent.actor, intent.sourceId);
    assertTypingContinuation(
        remote,
        { actor: intent.remoteActor, sourceId: targetId, text: intent.text },
        settled[0]!,
    );
    requireContinuity(intent.remoteActor !== intent.actor, 'lifetime distinct remote actor');
    const payload = realCells(remote.after).find((cell) => cell.sourceId === targetId)!.node;
    const expectedEffects = remoteHistoryEffects(action, targetId, payload);
    for (const [index, history] of [undo, redo].entries()) {
        requireContinuity(
            history.actor === intent.actor &&
                history.kind === 'rust' &&
                !history.observationFailure &&
                history.operation === (index === 0 ? 'undo' : 'redo'),
            'lifetime history actor/operation',
        );
        requireContinuity(
            history.reply['applied'] === true && history.passes === 0 && history.autonomous === 0,
            'lifetime applied history without normalization',
        );
        assert.notDeepEqual(
            canonicalDocumentShape(history.rawBefore),
            canonicalDocumentShape(history.rawAfter),
            'TBL21 CONTINUITY lifetime history effect',
        );
        const expected = expectedEffects[index]!;
        assert.deepEqual(
            canonicalDocumentShape(history.rawAfter),
            canonicalDocumentShape(expected),
            'TBL21 CONTINUITY lifetime declared history effect',
        );
        const targetTable = history.after.tables.find((table) =>
            table.cells.some((cell) => cell.sourceId === intent.sourceId),
        );
        const expectedTable = (index === 0 ? action.before : action.after).tables.find((table) =>
            table.cells.some((cell) => cell.sourceId === intent.sourceId),
        );
        requireContinuity(
            targetTable?.node.content?.length === expectedTable?.node.content?.length,
            'lifetime independent inserted row reversal',
        );
        for (const view of [
            { kind: history.kind, document: history.after },
            ...settled[index + 1]!,
        ]) {
            for (const original of action.before.tables) {
                const current = view.document.tables.find(
                    (table) => table.source === original.source,
                );
                requireContinuity(current, 'lifetime expected history table');
                assert.deepEqual(
                    canonicalDocumentShape(current.node),
                    canonicalDocumentShape(nodeAtSource(expected, original.source)),
                    'TBL21 CONTINUITY lifetime settled history effect',
                );
            }
            assertSourcePreservation(
                action.before,
                view.document,
                new Map([[targetId, payload]]),
                true,
                { before: action.kind, after: view.kind },
            );
            assertSourcePreservation(history.after, view.document, new Map(), true, {
                before: history.kind,
                after: view.kind,
            });
            const target = realCells(view.document).filter((cell) => cell.sourceId === targetId);
            requireContinuity(target.length === 1, 'lifetime remote source survives');
            assert.deepEqual(
                canonicalDocumentShape(target[0]!.node),
                canonicalDocumentShape(payload),
                'TBL21 CONTINUITY lifetime remote payload',
            );
            const occurrences = realCells(view.document).reduce(
                (count, cell) => count + cellText(cell.node).split(intent.text).length - 1,
                0,
            );
            requireContinuity(occurrences === 1, 'lifetime remote content exactly once');
        }
    }
}

export function assertNativeNoRemoteLifetime(
    actions: readonly RecordedAction[],
    intent: { actor: number; sourceId: string },
    settled: readonly (readonly ContinuationObservation[])[],
): void {
    assertContinuationHistory(actions, intent.actor);
    requireContinuity(
        actions.every((action) => action.kind === 'rust' && action.autonomous === 0),
        'native lifetime history kinds',
    );
    requireContinuity(
        settled.length === 2 && settled.every((views) => views.length > 0),
        'no-remote lifetime checkpoints',
    );
    const [action, undo, redo] = actions as [RecordedAction, RecordedAction, RecordedAction];
    const targetId = normalizationTarget(action, intent.actor, intent.sourceId);
    requireContinuity(
        !realCells(undo.after).some((cell) => cell.sourceId === targetId),
        'no-remote normalization target removed',
    );
    for (const [index, history] of [undo, redo].entries()) {
        for (const view of [{ kind: history.kind, document: history.after }, ...settled[index]!]) {
            assertSourcePreservation(action.before, view.document, new Map(), true, {
                before: action.kind,
                after: view.kind,
            });
            assertSourcePreservation(history.after, view.document, new Map(), true, {
                before: history.kind,
                after: view.kind,
            });
        }
    }
}
