import assert from 'node:assert/strict';
import { isDeepStrictEqual } from 'node:util';
import { canonicalDocumentShape } from './assertions.js';
import type {
    EffectiveCell,
    EffectiveDocument,
    EffectiveTable,
    JsonNode,
    PeerKind,
} from './peer-protocol.js';
import { realCells, assertActionEvidence, assertHistoryEvidence } from './scenario-evidence.js';
import type { RecordedAction } from './scenario-evidence.js';

export interface ContinuationObservation {
    kind: PeerKind;
    document: EffectiveDocument;
}

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

const EMPTY_CONTAINER_TYPES = new Set([
    'doc',
    'blockquote',
    'paragraph',
    'heading',
    'code_block',
    'table',
    'table_row',
    'tableRow',
    'table_cell',
    'table_header',
    'tableCell',
    'tableHeader',
]);

const LEAF_TYPES = new Set(['horizontal_rule', 'image', 'hard_break']);

export function nodeSize(node: JsonNode, kind: PeerKind): number {
    if (node.type === 'text')
        return kind === 'rust' ? [...(node.text ?? '')].length : (node.text ?? '').length;
    if (LEAF_TYPES.has(node.type)) return 1;
    if (node.content?.length)
        return 2 + node.content.reduce((size, child) => size + nodeSize(child, kind), 0);
    requireContinuity(EMPTY_CONTAINER_TYPES.has(node.type), `unsupported empty node ${node.type}`);
    return 2;
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

function attributes(node: JsonNode): unknown {
    return canonicalDocumentShape({ type: node.type, attrs: node.attrs ?? {} });
}

function hasAttributes(node: JsonNode): boolean {
    return !isDeepStrictEqual(attributes(node), attributes({ type: node.type }));
}

function rowOwners(table: EffectiveTable, kind: PeerKind | undefined): Map<string, number> {
    requireContinuity(
        kind === 'rust' || kind === 'prosemirror' || kind === 'tiptap',
        'row source attribution requires peer coordinates',
    );
    const cells = table.cells.filter((cell) => cell.source !== null);
    const positions = new Map<number, number>();
    let rowPosition = table.position + 1;
    for (const [index, row] of (table.node.content ?? []).entries()) {
        let position = rowPosition + 1;
        for (const cell of row.content ?? []) {
            positions.set(position, index);
            position += nodeSize(cell, kind);
        }
        rowPosition += nodeSize(row, kind);
    }
    requireContinuity(
        cells.every((cell) => cell.sourceId && positions.has(cell.position)),
        'row source attribution unavailable',
    );
    return new Map(cells.map((cell) => [cell.sourceId!, positions.get(cell.position)!]));
}

function assertRowAttributes(
    before: EffectiveTable,
    after: EffectiveTable,
    coordinates: { before?: PeerKind; after?: PeerKind },
): void {
    const original = before.node.content ?? [];
    const current = after.node.content ?? [];
    if (![...original, ...current].some(hasAttributes)) return;
    const originalOwners = rowOwners(before, coordinates.before);
    const currentOwners = rowOwners(after, coordinates.after);
    const matched = new Set<number>();
    for (const [index, row] of original.entries()) {
        const anchors = [...originalOwners]
            .filter(([, owner]) => owner === index)
            .map(([id]) => id);
        if (!anchors.length) {
            requireContinuity(!hasAttributes(row), 'row attributes have no source identity');
            continue;
        }
        const owners = anchors.map((id) => currentOwners.get(id));
        const owner = owners[0];
        requireContinuity(
            owner !== undefined && owners.every((value) => value === owner) && !matched.has(owner),
            'row source attribution changed',
        );
        matched.add(owner);
        assert.deepEqual(
            attributes(current[owner]!),
            attributes(row),
            'TBL21 CONTINUITY row attributes',
        );
    }
    for (const [index, row] of current.entries())
        if (!matched.has(index))
            requireContinuity(
                !hasAttributes(row),
                'row attributes have no original source identity',
            );
}

export function assertStructuralContinuation(
    action: RecordedAction,
    intent: { actor: number; source: string },
    settled: readonly ContinuationObservation[],
): void {
    requireContinuity(!action.observationFailure, 'structural action observations');
    assertActionEvidence(action, { ...intent, operation: 'addRow' });
    requireContinuity(settled.length > 0, 'settled structural observations');
    assertSourcePreservation(action.before, action.after, new Map(), true, {
        before: action.kind,
        after: action.kind,
    });
    for (const view of settled) {
        const coordinates = { before: action.kind, after: view.kind };
        assertSourcePreservation(action.before, view.document, new Map(), true, coordinates);
        assertSourcePreservation(action.after, view.document, new Map(), true, coordinates);
    }
}

export function assertSourcePreservation(
    before: EffectiveDocument,
    after: EffectiveDocument,
    changed: ReadonlyMap<string, JsonNode> = new Map(),
    allowEmptyAdditions = false,
    coordinates: { before?: PeerKind; after?: PeerKind } = {},
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
            attributes(found.node),
            attributes(table.node),
            'TBL21 CONTINUITY table attributes',
        );
        assertRowAttributes(table, found, coordinates);
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
    settled: readonly ContinuationObservation[],
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
    for (const view of [{ kind: action.kind, document: action.after }, ...settled]) {
        requireContinuity(
            realCells(view.document).some((cell) => cell.sourceId === materialized.sourceId),
            'gap source survives',
        );
        assertSourcePreservation(action.before, view.document, expected, true, {
            before: action.kind,
            after: view.kind,
        });
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
    settledViews: readonly ContinuationObservation[],
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
    for (const view of [{ kind: action.kind, document: action.after }, ...settledViews])
        assertSourcePreservation(action.before, view.document, new Map(), true, {
            before: action.kind,
            after: view.kind,
        });
}

export interface TypingIntent {
    actor: number;
    sourceId: string;
    text: string;
}
export function assertTypingContinuation(
    action: RecordedAction,
    intent: TypingIntent,
    settled: readonly ContinuationObservation[],
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
    for (const view of [{ kind: action.kind, document: action.after }, ...settled])
        assertSourcePreservation(action.before, view.document, changed, true, {
            before: action.kind,
            after: view.kind,
        });
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
