import assert from 'node:assert/strict';
import { canonicalDocumentShape } from './assertions.js';
import {
    assertSourcePreservation,
    outsideShape,
    requireContinuity,
    typedNode,
    type ContinuationObservation,
} from './continuity-evidence.js';
import { realCells, type RecordedAction } from './scenario-evidence.js';
import type { EffectiveDocument, JsonNode, PeerKind } from './peer-protocol.js';

export interface TextHistoryObservation extends ContinuationObservation {
    raw: JsonNode;
}

export interface TextHistoryIntent {
    actor: number;
    kind: PeerKind;
    sourceId: string;
    text: string;
    before: EffectiveDocument;
    rawBefore: JsonNode;
    original: JsonNode;
    typed: JsonNode;
}

function atSource(raw: JsonNode, source: string): JsonNode {
    let node = raw;
    for (const part of source.split('.')) {
        requireContinuity(
            /^\d+$/.test(part) && node.content?.[Number(part)],
            'raw source materialization',
        );
        node = node.content![Number(part)]!;
    }
    return node;
}

function rawView(view: EffectiveDocument, raw: JsonNode): EffectiveDocument {
    const result = structuredClone(view);
    const sources = new Set(realCells(result).map((cell) => cell.source));
    function visit(node: JsonNode, source: string): void {
        if (['table_cell', 'table_header', 'tableCell', 'tableHeader'].includes(node.type))
            requireContinuity(sources.has(source), 'unattributed raw source');
        node.content?.forEach((child, index) =>
            visit(child, source ? `${source}.${index}` : String(index)),
        );
    }
    visit(raw, '');
    for (const table of result.tables) {
        table.node = atSource(raw, table.source);
        for (const cell of table.cells)
            if (cell.source !== null) cell.node = atSource(raw, cell.source);
    }
    return result;
}

export function textHistoryIntent(
    before: EffectiveDocument,
    rawBefore: JsonNode,
    target: Pick<TextHistoryIntent, 'actor' | 'kind' | 'sourceId' | 'text'>,
): TextHistoryIntent {
    const cells = realCells(before).filter((cell) => cell.sourceId === target.sourceId);
    requireContinuity(
        cells.length === 1 && target.text.length > 0,
        'unique text-history source/marker',
    );
    requireContinuity(
        !JSON.stringify(rawBefore).includes(target.text),
        'fresh text-history marker',
    );
    const original = structuredClone(cells[0]!.node);
    const intent = {
        ...target,
        before: structuredClone(before),
        rawBefore: structuredClone(rawBefore),
        original,
        typed: typedNode(original, target.text),
    };
    assertSourcePreservation(before, rawView(before, rawBefore), new Map(), false, {
        before: target.kind,
        after: target.kind,
    });
    return intent;
}

export function assertTextHistoryBoundary(
    action: RecordedAction,
    intent: TextHistoryIntent,
    stage: number,
    settled: readonly TextHistoryObservation[],
): void {
    requireContinuity(
        stage >= 0 && stage < 3 && action.operation === ['insertText', 'undo', 'redo'][stage],
        'text-history operation',
    );
    requireContinuity(
        action.actor === intent.actor && action.kind === intent.kind && !action.observationFailure,
        'text-history action observation',
    );
    requireContinuity(
        action.reply[stage === 0 ? 'documentChanged' : 'applied'] === true,
        'text-history applied',
    );
    requireContinuity(
        action.autonomous === 0 && (action.kind !== 'rust' || action.passes === 0),
        'text-history normalization',
    );
    if (stage === 0)
        requireContinuity(
            action.target?.sourceId === intent.sourceId && action.text === intent.text,
            'text-history intended source',
        );
    requireContinuity(settled.length > 0, 'text-history settled observations');
    assertTextHistoryState(
        intent,
        {
            kind: action.kind,
            document: action.before,
            raw: action.rawBefore as JsonNode,
        },
        stage === 1,
    );
    for (const view of [
        {
            kind: action.kind,
            document: action.after,
            raw: action.rawAfter as JsonNode,
        },
        ...settled,
    ])
        assertTextHistoryState(intent, view, stage !== 1);
    if (action.kind === 'rust') {
        const expected = structuredClone(action.rawBefore) as JsonNode;
        const cell = realCells(action.before).find((cell) => cell.sourceId === intent.sourceId);
        requireContinuity(cell?.source, 'native text-history source');
        atSource(expected, cell.source).content = structuredClone(
            stage === 1 ? intent.original.content : intent.typed.content,
        );
        assert.deepEqual(
            canonicalDocumentShape(action.rawAfter),
            canonicalDocumentShape(expected),
            'TBL21 CONTINUITY native text-history raw geometry/content',
        );
    }
}

export function assertTextHistoryState(
    intent: TextHistoryIntent,
    view: TextHistoryObservation,
    typed: boolean,
): void {
    const original = realCells(intent.before).filter((cell) => cell.sourceId === intent.sourceId);
    requireContinuity(original.length === 1, 'text-history declared source');
    assert.deepEqual(intent.original, original[0]!.node, 'TBL21 CONTINUITY original text intent');
    assert.deepEqual(
        intent.typed,
        typedNode(original[0]!.node, intent.text),
        'TBL21 CONTINUITY derived text intent',
    );
    const changed = new Map([[intent.sourceId, typed ? intent.typed : intent.original]]);
    const coordinates = { before: intent.kind, after: view.kind };
    assertSourcePreservation(intent.before, view.document, changed, true, coordinates);
    assertSourcePreservation(
        intent.before,
        rawView(view.document, view.raw),
        changed,
        true,
        coordinates,
    );
    assert.deepEqual(
        outsideShape(view.raw),
        outsideShape(intent.rawBefore),
        'TBL21 CONTINUITY text-history outside content',
    );
}
