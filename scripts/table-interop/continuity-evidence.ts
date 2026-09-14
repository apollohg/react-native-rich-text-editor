import assert from 'node:assert/strict';
import { canonicalDocumentShape } from './assertions.js';
import type { EffectiveCell, EffectiveDocument, JsonNode, PeerKind } from './peer-protocol.js';
import { realCells, assertHistoryEvidence } from './scenario-evidence.js';
import type { RecordedAction } from './scenario-evidence.js';

export function requireContinuity(value: unknown, detail: string): asserts value {
    assert.ok(value, `TBL21 CONTINUITY ${detail}`);
}

export function assertContinuationHistory(actions: readonly RecordedAction[], actor: number): void {
    requireContinuity(
        actions.length === 3 &&
            actions.every((action) => action.actor === actor && !action.observationFailure),
        'history action boundaries',
    );
    const [action, undo, redo] = actions;
    requireContinuity(
        action!.operation === 'addRow' && undo!.operation === 'undo' && redo!.operation === 'redo',
        'history operations',
    );
    assertHistoryEvidence({
        before: canonicalDocumentShape(action!.rawBefore),
        acted: canonicalDocumentShape(action!.rawAfter),
        undone: canonicalDocumentShape(undo!.rawAfter),
        redone: canonicalDocumentShape(redo!.rawAfter),
        undo: undo!.reply,
        redo: redo!.reply,
        passes: [undo!.passes, redo!.passes],
    });
}

export function nodeSize(node: JsonNode, kind: PeerKind): number {
    if (node.type === 'text')
        return kind === 'rust' ? [...(node.text ?? '')].length : (node.text ?? '').length;
    if (node.content)
        return 2 + node.content.reduce((size, child) => size + nodeSize(child, kind), 0);
    return node.type === 'paragraph' ? 2 : 1;
}

function paragraphAt(
    node: JsonNode,
    kind: PeerKind,
    start = 0,
): { node: JsonNode; offset: number } | null {
    if (node.type === 'paragraph') return { node, offset: start + 1 };
    let offset = start + 1;
    for (const child of node.content ?? []) {
        if (child.type !== 'table') {
            const found = paragraphAt(child, kind, offset);
            if (found) return found;
        }
        offset += nodeSize(child, kind);
    }
    return null;
}

export function typingCursor(cell: EffectiveCell, kind: PeerKind): number {
    const paragraph = paragraphAt(cell.node, kind);
    requireContinuity(paragraph, 'editable paragraph unavailable');
    return cell.position + paragraph.offset;
}

function typedNode(node: JsonNode, text: string): JsonNode {
    const result = structuredClone(node);
    const paragraph = paragraphAt(result, 'rust');
    requireContinuity(paragraph, 'intended paragraph unavailable');
    const first = paragraph.node.content?.[0];
    if (first?.type === 'text') first.text = text + (first.text ?? '');
    else paragraph.node.content = [{ type: 'text', text }, ...(paragraph.node.content ?? [])];
    return result;
}

function payload(node: JsonNode): unknown {
    const result = structuredClone(node);
    function visit(child: JsonNode): void {
        if (['table_cell', 'table_header', 'tableCell', 'tableHeader'].includes(child.type)) {
            const { colspan: _c, rowspan: _r, colwidth: _w, ...attrs } = child.attrs ?? {};
            child.attrs = attrs;
        }
        if (child.type === 'table') delete child.content;
        else for (const nested of child.content ?? []) visit(nested);
    }
    visit(result);
    return canonicalDocumentShape(result);
}

export function assertSourcePreservation(
    before: EffectiveDocument,
    after: EffectiveDocument,
    changed: ReadonlyMap<string, JsonNode> = new Map(),
    allowEmptyAdditions = false,
): void {
    const original = realCells(before);
    const current = realCells(after);
    const ids = current.map((cell) => cell.sourceId);
    requireContinuity(
        ids.every(Boolean) && new Set(ids).size === ids.length,
        'unique source identities',
    );
    for (const table of before.tables) {
        const anchor = table.cells.find((cell) => cell.sourceId)?.sourceId;
        const found = after.tables.find((candidate) =>
            anchor
                ? candidate.cells.some((cell) => cell.sourceId === anchor)
                : candidate.source === table.source,
        );
        requireContinuity(found, 'table survival');
        assert.deepEqual(
            canonicalDocumentShape(found.node.attrs ?? {}),
            canonicalDocumentShape(table.node.attrs ?? {}),
            'TBL21 CONTINUITY table attributes',
        );
        const rowAttributes = (node: JsonNode) =>
            (node.content ?? [])
                .map((row) => canonicalDocumentShape(row.attrs ?? {}))
                .filter((attrs) => JSON.stringify(attrs) !== '{}')
                .map((attrs) => JSON.stringify(attrs))
                .sort();
        assert.deepEqual(
            rowAttributes(found.node),
            rowAttributes(table.node),
            'TBL21 CONTINUITY row attributes',
        );
    }
    for (const cell of original) {
        requireContinuity(cell.sourceId, 'original source identity');
        const found = current.filter((candidate) => candidate.sourceId === cell.sourceId);
        requireContinuity(found.length === 1, 'source survival');
        assert.deepEqual(
            payload(found[0]!.node),
            payload(changed.get(cell.sourceId) ?? cell.node),
            'TBL21 CONTINUITY source content/attributes',
        );
    }
    for (const cell of current.filter(
        (cell) => !original.some((old) => old.sourceId === cell.sourceId),
    )) {
        if (cell.sourceId && changed.has(cell.sourceId)) {
            assert.deepEqual(
                payload(cell.node),
                payload(changed.get(cell.sourceId)!),
                'TBL21 CONTINUITY new source content',
            );
            continue;
        }
        requireContinuity(allowEmptyAdditions, 'unexpected new source');
        const empty = (node: JsonNode): boolean =>
            node.type === 'paragraph'
                ? (node.content ?? []).every(empty)
                : ['table_cell', 'table_header', 'tableCell', 'tableHeader'].includes(node.type)
                  ? (node.content ?? []).every(empty)
                  : false;
        requireContinuity(empty(cell.node), 'new source contains undeclared content');
    }
}

export interface GapIntent {
    actor: number;
    tableSource: string;
    position: number;
    text: string;
}
export function assertGapContinuation(
    action: RecordedAction,
    intent: GapIntent,
    settled: readonly EffectiveDocument[],
): void {
    requireContinuity(
        action.actor === intent.actor && action.operation === 'insertText',
        'gap actor/operation',
    );
    requireContinuity(
        action.reply['documentChanged'] === true && !action.observationFailure,
        'gap action applied',
    );
    const gap = action.before.tables
        .find((table) => table.source === intent.tableSource)
        ?.cells.find((cell) => cell.position === intent.position && cell.source === null);
    requireContinuity(gap, 'declared display-only gap');
    const materialized = action.after.tables
        .find((table) => table.source === intent.tableSource)
        ?.cells.find((cell) => cell.position === intent.position && cell.sourceId);
    requireContinuity(
        materialized?.sourceId &&
            !realCells(action.before).some((cell) => cell.sourceId === materialized.sourceId),
        'new attributable gap source',
    );
    const expected = new Map([[materialized.sourceId, typedNode(gap.node, intent.text)]]);
    requireContinuity(settled.length > 0, 'settled gap observations');
    for (const view of [action.after, ...settled]) {
        requireContinuity(
            realCells(view).some((cell) => cell.sourceId === materialized.sourceId),
            'gap source survives',
        );
        assertSourcePreservation(action.before, view, expected, true);
    }
}

export function assertGapRefusal(action: RecordedAction, emittedUpdates: number): void {
    requireContinuity(
        action.operation === 'insertText' &&
            action.target === null &&
            action.reply['documentChanged'] === false &&
            !action.observationFailure,
        'explicit gap refusal',
    );
    requireContinuity(action.autonomous === 0 && emittedUpdates === 0, 'refusal writes');
    assert.deepEqual(
        canonicalDocumentShape(action.rawAfter),
        canonicalDocumentShape(action.rawBefore),
        'TBL21 CONTINUITY refusal raw mutation',
    );
    assert.deepEqual(action.after, action.before, 'TBL21 CONTINUITY refusal surface mutation');
}

export function outsideShape(node: JsonNode): unknown {
    return canonicalDocumentShape({
        ...node,
        content: (node.content ?? []).map((child) =>
            child.type === 'table' ? { type: 'table' } : child,
        ),
    });
}

export function assertUnrelatedContinuation(
    action: RecordedAction,
    intent: { actor: number; index: number; text: string },
    settledRaw: readonly JsonNode[],
    settledViews: readonly EffectiveDocument[],
): void {
    requireContinuity(
        action.actor === intent.actor &&
            action.operation === 'insertText' &&
            action.target === null,
        'outside actor/source target',
    );
    requireContinuity(
        action.reply['documentChanged'] === true && !action.observationFailure,
        'outside typing applied',
    );
    const expected = structuredClone(action.rawBefore) as JsonNode;
    const paragraph = expected.content?.[intent.index];
    requireContinuity(paragraph?.type === 'paragraph', 'top-level paragraph target');
    expected.content![intent.index] = typedNode(paragraph, intent.text);
    requireContinuity(
        settledRaw.length > 0 && settledViews.length === settledRaw.length,
        'outside settled observations',
    );
    for (const raw of [action.rawAfter as JsonNode, ...settledRaw])
        assert.deepEqual(
            outsideShape(raw),
            outsideShape(expected),
            'TBL21 CONTINUITY outside text effect',
        );
    for (const view of [action.after, ...settledViews])
        assertSourcePreservation(action.before, view, new Map(), true);
}

export interface TypingIntent {
    actor: number;
    sourceId: string;
    text: string;
}
export function assertTypingContinuation(
    action: RecordedAction,
    intent: TypingIntent,
    settled: readonly EffectiveDocument[],
): void {
    requireContinuity(
        action.actor === intent.actor && action.operation === 'insertText',
        'typing actor/operation',
    );
    requireContinuity(action.target?.sourceId === intent.sourceId, 'typing source target');
    requireContinuity(!action.observationFailure, 'typing observation');
    requireContinuity(action.reply['documentChanged'] === true, 'typing applied');
    requireContinuity(action.autonomous === 0, 'autonomous repair');
    if (action.kind === 'rust') requireContinuity(action.passes === 0, 'typing normalization');
    const target = realCells(action.before).find((cell) => cell.sourceId === intent.sourceId);
    requireContinuity(target, 'declared typing source');
    const changed = new Map([[intent.sourceId, typedNode(target.node, intent.text)]]);
    requireContinuity(settled.length > 0, 'settled typing observations');
    for (const view of [action.after, ...settled])
        assertSourcePreservation(action.before, view, changed, true);
    if (action.kind === 'rust') {
        const geometry = (view: EffectiveDocument) =>
            view.tables.map((table) => [
                table.source,
                table.rows,
                table.columns,
                table.cells.map((cell) => [
                    cell.sourceId,
                    cell.row,
                    cell.column,
                    cell.rowspan,
                    cell.colspan,
                ]),
            ]);
        assert.deepEqual(
            geometry(action.after),
            geometry(action.before),
            'TBL21 CONTINUITY native typing geometry',
        );
    }
}
