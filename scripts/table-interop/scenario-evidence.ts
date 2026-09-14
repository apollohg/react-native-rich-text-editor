import assert from 'node:assert/strict';
import { canonicalDocumentShape } from './assertions.js';
import type {
    EffectiveCell,
    EffectiveDocument,
    EffectiveTable,
    JsonNode,
    PeerKind,
} from './peer-protocol.js';

export interface ActionEvidence {
    actor: number;
    kind: PeerKind;
    operation: string;
    target: EffectiveCell | null;
    head?: EffectiveCell | null;
    before: EffectiveDocument;
    after: EffectiveDocument;
    reply: Record<string, unknown>;
    passes: number;
    autonomous: number;
    text?: string;
    width?: number;
    targetGridValid?: boolean;
}
export interface ActionIntent {
    actor: number;
    operation: string;
    source: string;
    text?: string;
    width?: number;
}
export type CoverageStatus = 'proven' | 'exercised-unproven' | 'unexercised';

export function coverageStatus(
    exercised: boolean,
    predicate: boolean,
    passed: boolean,
): CoverageStatus {
    return !exercised ? 'unexercised' : predicate && passed ? 'proven' : 'exercised-unproven';
}
export function cellText(node: JsonNode): string {
    return node.text ?? (node.content ?? []).map(cellText).join('');
}
export function realCells(document: EffectiveDocument): EffectiveCell[] {
    return document.tables.flatMap((table) => table.cells.filter((cell) => cell.source !== null));
}
export function contentShape(node: JsonNode): unknown {
    return canonicalDocumentShape(node);
}
function equal(a: unknown, b: unknown, code: string): void {
    assert.deepEqual(a, b, `TBL21 EVIDENCE ${code}`);
}
function requireEvidence(ok: unknown, code: string): asserts ok {
    assert.ok(ok, `TBL21 EVIDENCE ${code}`);
}
function prefixed(node: JsonNode, text: string): JsonNode {
    const result = structuredClone(node);
    const paragraph = result.content?.[0];
    requireEvidence(paragraph?.type === 'paragraph', 'TEXT_TARGET_PARAGRAPH');
    const first = paragraph.content?.[0];
    if (first?.type === 'text') first.text = text + (first.text ?? '');
    else paragraph.content = [{ type: 'text', text }, ...(paragraph.content ?? [])];
    return result;
}
function tableFor(document: EffectiveDocument, source: string): EffectiveTable {
    const found = document.tables.find((table) =>
        table.cells.some((cell) => cell.source === source),
    );
    requireEvidence(found, 'TARGET_TABLE');
    return found;
}
function payloadTokens(cells: EffectiveCell[]): string[] {
    const tokens: string[] = [];
    function visit(node: JsonNode): void {
        if (node.type === 'text' || (!node.content && node.type !== 'paragraph'))
            tokens.push(JSON.stringify(contentShape(node)));
        else for (const child of node.content ?? []) visit(child);
    }
    for (const cell of cells) for (const child of cell.node.content ?? []) visit(child);
    return tokens.sort();
}
function authoredAttributes(node: JsonNode): unknown {
    const {
        colspan: _colspan,
        rowspan: _rowspan,
        colwidth: _colwidth,
        ...attrs
    } = node.attrs ?? {};
    return contentShape({ type: node.type, attrs });
}
function preserveStructuralPayload(
    before: EffectiveTable,
    after: EffectiveTable,
    target: EffectiveCell,
    operation: string,
    head?: EffectiveCell | null,
): void {
    const original = before.cells.filter(
        (cell) => cell.source !== null && (operation !== 'deleteRow' || cell.row !== target.row),
    );
    const surviving = after.cells.filter((cell) => cell.source !== null);
    equal(payloadTokens(surviving), payloadTokens(original), 'PRESERVATION_CONTENT');
    for (const cell of original) {
        const same = surviving.filter((candidate) =>
            cell.sourceId ? candidate.sourceId === cell.sourceId : candidate.source === cell.source,
        );
        if (
            operation !== 'merge' ||
            (cell.sourceId !== target.sourceId && cell.sourceId !== head?.sourceId)
        )
            requireEvidence(same.length === 1, 'PRESERVATION_IDENTITY');
        if (same.length === 1)
            equal(
                authoredAttributes(same[0]!.node),
                authoredAttributes(cell.node),
                'PRESERVATION_ATTRIBUTES',
            );
        if (
            same.length === 1 &&
            (operation !== 'merge' ||
                (cell.sourceId !== target.sourceId && cell.sourceId !== head?.sourceId))
        )
            equal(
                canonicalDocumentShape(same[0]!.node.content),
                canonicalDocumentShape(cell.node.content),
                'PRESERVATION_CELL_CONTENT',
            );
    }
    equal(before.node.attrs ?? {}, after.node.attrs ?? {}, 'PRESERVATION_TABLE_ATTRIBUTES');
}
export function assertActionEvidence(e: ActionEvidence, intent: ActionIntent): void {
    equal([e.actor, e.operation], [intent.actor, intent.operation], 'ACTOR_COMMAND');
    requireEvidence(e.target?.source !== null && e.target?.source === intent.source, 'TARGET');
    const source = e.target.source;
    const sourceCell = realCells(e.before).find((cell) => cell.source === source);
    requireEvidence(
        sourceCell?.sourceId && sourceCell.sourceId === e.target.sourceId,
        'TARGET_IDENTITY',
    );
    const before = tableFor(e.before, source);
    const after = e.after.tables.find((table) => table.source === before.source);
    requireEvidence(after, 'TARGET_TABLE');
    requireEvidence(e.reply['documentChanged'] === true, 'ACTION_APPLIED');
    equal(e.autonomous, 0, 'AUTONOMOUS_REPAIR');
    if (e.kind === 'rust') requireEvidence(e.passes <= 2, 'NORMALIZATION_BOUND');
    if (intent.operation === 'insertText') {
        if (e.kind === 'rust') equal(e.passes, 0, 'NORMALIZATION');
        const cells = realCells(e.after);
        const changed = cells.filter((cell) =>
            e.target?.sourceId ? cell.sourceId === e.target.sourceId : cell.source === source,
        );
        requireEvidence(changed.length === 1, 'TEXT_EFFECT');
        equal(
            contentShape(changed[0]!.node),
            contentShape(prefixed(e.target.node, intent.text ?? '')),
            'TEXT_EFFECT',
        );
        const expected = structuredClone(e.before);
        const target = realCells(expected).find((cell) => cell.source === source)!;
        target.node = prefixed(target.node, intent.text ?? '');
        equal(
            realCells(e.after).map((cell) => [
                cell.sourceId ?? cell.source,
                contentShape(cell.node),
            ]),
            realCells(expected).map((cell) => [
                cell.sourceId ?? cell.source,
                contentShape(cell.node),
            ]),
            'PRESERVATION',
        );
        return;
    }
    const rows = (table: EffectiveTable) => table.node.content?.length ?? table.rows;
    if (e.kind === 'rust') requireEvidence(e.targetGridValid === true, 'VALID_TARGET');
    if (intent.operation === 'addRow') equal(rows(after), rows(before) + 1, 'ROW_EFFECT');
    else if (intent.operation === 'deleteRow')
        equal(rows(after), rows(before) - 1, 'ROW_DELETE_EFFECT');
    else if (intent.operation === 'addColumn')
        equal(after.columns, before.columns + 1, 'COLUMN_EFFECT');
    else if (intent.operation === 'merge')
        requireEvidence(
            after.cells.filter((c) => c.source !== null).length <
                before.cells.filter((c) => c.source !== null).length,
            'MERGE_EFFECT',
        );
    else if (intent.operation === 'split')
        requireEvidence(
            after.cells.filter((c) => c.source !== null).length >
                before.cells.filter((c) => c.source !== null).length,
            'SPLIT_EFFECT',
        );
    else if (intent.operation === 'resize') {
        requireEvidence(
            typeof intent.width === 'number' && e.target.column !== null,
            'WIDTH_TARGET',
        );
        equal(after.widths?.[e.target.column], intent.width, 'WIDTH_EFFECT');
        requireEvidence(
            after.cells.some(
                (cell) =>
                    Array.isArray(cell.node.attrs?.['colwidth']) &&
                    cell.node.attrs!['colwidth'].includes(intent.width),
            ),
            'WIDTH_RAW_CONTRIBUTION',
        );
    } else throw new Error(`TBL21 EVIDENCE UNKNOWN_COMMAND ${intent.operation}`);
    preserveStructuralPayload(before, after, e.target, intent.operation, e.head);
    if (intent.operation === 'addRow' || intent.operation === 'addColumn') {
        const axis = intent.operation === 'addRow' ? 'row' : 'column';
        const span = axis === 'row' ? 'rowspan' : 'colspan';
        requireEvidence(e.target[axis] !== null && e.target[span] !== null, 'INSERTION_TARGET');
        const boundary = e.target[axis] + e.target[span];
        for (const cell of before.cells.filter((cell) => cell.source !== null)) {
            const surviving = after.cells.find((candidate) =>
                cell.sourceId
                    ? candidate.sourceId === cell.sourceId
                    : candidate.source === cell.source,
            );
            requireEvidence(
                surviving && cell[axis] !== null && cell[span] !== null,
                'INSERTION_IDENTITY',
            );
            equal(
                surviving[axis],
                cell[axis] >= boundary ? cell[axis] + 1 : cell[axis],
                'INSERTION_FOOTPRINT',
            );
            equal(
                surviving[span],
                cell[axis] < boundary && cell[axis] + cell[span] > boundary
                    ? cell[span] + 1
                    : cell[span],
                'INSERTION_SPAN',
            );
        }
    }
}
export interface HistoryEvidence {
    before: unknown;
    acted: unknown;
    undone: unknown;
    redone: unknown;
    undo: Record<string, unknown>;
    redo: Record<string, unknown>;
    passes: number[];
}
export function assertHistoryEvidence(e: HistoryEvidence): void {
    requireEvidence(e.undo['applied'] === true && e.redo['applied'] === true, 'HISTORY_APPLIED');
    equal(e.passes, [0, 0], 'HISTORY_NORMALIZATION');
    assert.notDeepEqual(e.before, e.acted, 'TBL21 EVIDENCE ACTION_EFFECT');
    equal(e.undone, e.before, 'UNDO_EFFECT');
    equal(e.redone, e.acted, 'REDO_EFFECT');
}
export interface LifetimeEvidence {
    beforeIds: string[];
    createdIds: string[];
    targetId: string;
    afterRemoteIds: string[];
    remoteOccurrences: number[];
    nativePasses: number;
}
export function assertLifetimeEvidence(e: LifetimeEvidence): void {
    requireEvidence(
        !e.beforeIds.includes(e.targetId) &&
            e.createdIds.includes(e.targetId) &&
            e.nativePasses > 0 &&
            e.nativePasses <= 2,
        'NATIVE_OWNERSHIP',
    );
    requireEvidence(e.afterRemoteIds.includes(e.targetId), 'REMOTE_TARGET');
    equal(e.remoteOccurrences, [1, 1, 1], 'REMOTE_CONTENT');
}
type ActorRole =
    | 'first'
    | 'second'
    | 'native'
    | 'other-native'
    | 'non-author'
    | 'native-non-author'
    | 'web-non-author'
    | 'other-editor';
export interface FamilyStep {
    operation: string;
    actor: ActorRole;
    target?: string;
    head?: string;
    width?: number;
}
export interface FamilyIntent {
    steps: FamilyStep[];
    preserve: string[];
    deleted?: string[];
    history?: 'no-remote' | 'remote-repair';
    dependency?: boolean;
    typed?: boolean;
}
const step = (operation: string, actor: ActorRole, target?: string, head?: string): FamilyStep => ({
    operation,
    actor,
    target,
    head,
});
const markers = ['a', 'b', 'c', 'd'];
export const FAMILY_INTENTS: readonly FamilyIntent[] = [
    { steps: [step('addRow', 'first', 'a'), step('addColumn', 'second', 'a')], preserve: markers },
    {
        steps: [step('merge', 'first', 'a', 'b'), step('merge', 'second', 'c', 'a')],
        preserve: markers,
    },
    {
        steps: [
            step('addRow', 'non-author', 'a'),
            step('undo', 'non-author'),
            step('redo', 'non-author'),
        ],
        preserve: markers,
        history: 'no-remote',
    },
    {
        steps: [step('addRow', 'first', 'a'), step('addColumn', 'first', 'a')],
        preserve: markers,
        dependency: true,
    },
    { steps: [step('insertText', 'first', 'a')], preserve: markers, typed: true },
    {
        steps: [
            step('merge', 'first', 'a', 'b'),
            step('split', 'first', 'ab'),
            step('deleteRow', 'second', 'd'),
        ],
        preserve: ['a', 'b'],
        deleted: ['c', 'd'],
    },
    {
        steps: [
            step('merge', 'first', 'a', 'b'),
            step('addRow', 'second', 'ab'),
            step('addColumn', 'first', 'ab'),
        ],
        preserve: markers,
    },
    {
        steps: [
            { ...step('resize', 'native', 'a'), width: 180 },
            step('addRow', 'other-native', 'a'),
        ],
        preserve: markers,
    },
    {
        steps: [
            step('addRow', 'native-non-author', 'a'),
            step('insertText', 'other-editor', ''),
            step('undo', 'native-non-author'),
            step('redo', 'native-non-author'),
        ],
        preserve: markers,
        history: 'remote-repair',
        typed: true,
    },
    {
        steps: [
            step('addRow', 'web-non-author', 'a'),
            step('insertText', 'other-editor', ''),
            step('undo', 'web-non-author'),
            step('redo', 'web-non-author'),
        ],
        preserve: markers,
        history: 'remote-repair',
        typed: true,
    },
    {
        steps: [
            { ...step('resize', 'first', 'a'), width: 180 },
            { ...step('resize', 'second', 'c'), width: 220 },
        ],
        preserve: markers,
    },
    {
        steps: [
            { ...step('resize', 'first', 'a'), width: 180 },
            { ...step('resize', 'second', 'b'), width: 220 },
        ],
        preserve: markers,
    },
    {
        steps: [step('addRow', 'native', 'a'), step('undo', 'native')],
        preserve: markers,
        history: 'remote-repair',
    },
    { steps: [step('addRow', 'second', 'd')], preserve: ['x', 'y', 'z', 'b', 'c', 'd'] },
];
export interface RecordedAction extends ActionEvidence {
    head: EffectiveCell | null;
    rawBefore: unknown;
    rawAfter: unknown;
    observationFailure?: string;
}
export interface RecordedDelivery {
    actor: number;
    bytes: string;
    pendingBefore: boolean;
    pendingAfter: boolean;
}
export interface FamilyEvidence {
    actions: RecordedAction[];
    deliveries: RecordedDelivery[];
    settled: EffectiveDocument;
    author: number;
    actors: number[];
    kinds: PeerKind[];
}
function actorFor(role: ActorRole, e: FamilyEvidence, editor?: number): number {
    const native = e.actors.find((index) => e.kinds[index] === 'rust');
    const found =
        role === 'first'
            ? e.actors[0]
            : role === 'second'
              ? e.actors[1]
              : role === 'native'
                ? native
                : role === 'other-native'
                  ? e.actors.find((index) => index !== native)
                  : role === 'non-author'
                    ? e.actors.find((index) => index !== e.author)
                    : role === 'native-non-author'
                      ? e.actors.find((index) => index !== e.author && e.kinds[index] === 'rust')
                      : role === 'web-non-author'
                        ? e.actors.find((index) => index !== e.author && e.kinds[index] !== 'rust')
                        : e.actors.find((index) => index !== editor);
    requireEvidence(found !== undefined, 'ACTOR_AVAILABLE');
    return found;
}
export function declaredFamilyActors(
    intent: FamilyIntent,
    actors: number[],
    kinds: PeerKind[],
): number[] {
    const evidence: FamilyEvidence = {
        actions: [],
        deliveries: [],
        settled: { tables: [] },
        author: 0,
        actors,
        kinds,
    };
    const editor = actorFor(intent.steps[0]!.actor, evidence);
    return intent.steps.map((step) => actorFor(step.actor, evidence, editor));
}
export function assertFamilyEvidence(intent: FamilyIntent, e: FamilyEvidence): void {
    equal(e.actions.length, intent.steps.length, 'MISSING_ACTION');
    const editor = actorFor(intent.steps[0]!.actor, e);
    for (const [index, declared] of intent.steps.entries()) {
        const action = e.actions[index]!;
        requireEvidence(
            !action.observationFailure,
            `OBSERVATION ${action.observationFailure ?? ''}`,
        );
        equal(
            [action.actor, action.operation],
            [actorFor(declared.actor, e, editor), declared.operation],
            'ACTOR_COMMAND',
        );
        if (declared.operation === 'undo' || declared.operation === 'redo') {
            requireEvidence(action.reply['applied'] === true, 'HISTORY_APPLIED');
            if (action.kind === 'rust') equal(action.passes, 0, 'HISTORY_NORMALIZATION');
            assert.notDeepEqual(
                contentShape(action.rawBefore as JsonNode),
                contentShape(action.rawAfter as JsonNode),
                'TBL21 EVIDENCE HISTORY_EFFECT',
            );
            const intermediate = cellText(action.rawAfter as JsonNode).replaceAll('typed', '');
            for (const marker of intent.preserve)
                equal(intermediate.split(marker).length - 1, 1, 'HISTORY_PRESERVATION');
            continue;
        }
        const targets = realCells(action.before).filter(
            (cell) => cellText(cell.node) === declared.target,
        );
        requireEvidence(
            targets.length === 1 ||
                (declared.target === '' &&
                    action.target?.source !== null &&
                    cellText(action.target!.node) === ''),
            'DECLARED_TARGET',
        );
        const target = declared.target === '' ? action.target! : targets[0]!;
        if (declared.target === '') {
            const rowMarker = realCells(action.before).find((cell) => cellText(cell.node) === 'd');
            requireEvidence(
                rowMarker?.column !== null &&
                    rowMarker?.row === target.row &&
                    target.column === rowMarker.column + 1,
                'DECLARED_GAP_TARGET',
            );
        }
        if (declared.head !== undefined)
            equal(cellText(action.head?.node ?? { type: 'missing' }), declared.head, 'HEAD_TARGET');
        assertActionEvidence(action, {
            actor: actorFor(declared.actor, e, editor),
            operation: declared.operation,
            source: target.source!,
            text: 'typed',
            width: declared.width,
        });
    }
    if (intent.history === 'no-remote') {
        const [acted, undone, redone] = e.actions;
        assertHistoryEvidence({
            before: contentShape(acted!.rawBefore as JsonNode),
            acted: contentShape(acted!.rawAfter as JsonNode),
            undone: contentShape(undone!.rawAfter as JsonNode),
            redone: contentShape(redone!.rawAfter as JsonNode),
            undo: undone!.reply,
            redo: redone!.reply,
            passes: [
                undone!.kind === 'rust' ? undone!.passes : 0,
                redone!.kind === 'rust' ? redone!.passes : 0,
            ],
        });
    }
    if (intent.dependency) {
        requireEvidence(
            e.deliveries.some((delivery) => delivery.pendingAfter),
            'DEPENDENCY_WITHHELD',
        );
        requireEvidence(
            new Set(e.deliveries.map((delivery) => delivery.bytes)).size >= 2,
            'DEPENDENCY_UPDATES',
        );
    }
    const text = e.settled.tables
        .filter((table) => table.parentCell === null)
        .map((table) => cellText(table.node))
        .join('');
    const withoutTyping = text.replaceAll('typed', '');
    for (const marker of intent.preserve)
        equal(withoutTyping.split(marker).length - 1, 1, `PRESERVATION ${marker}`);
    for (const marker of intent.deleted ?? [])
        equal(withoutTyping.split(marker).length - 1, 0, `DELETION ${marker}`);
    if (intent.typed) equal(text.split('typed').length - 1, 1, 'REMOTE_CONTENT');
    if (intent.history === 'remote-repair' && intent.typed) {
        for (const action of e.actions.filter(
            (action) => action.operation === 'undo' || action.operation === 'redo',
        ))
            equal(
                cellText(action.rawAfter as JsonNode).split('typed').length - 1,
                1,
                'REMOTE_CONTENT',
            );
    }
    const original = e.actions[0]!.before.tables.filter(
        (table) => table.parentCell === null,
    ).flatMap((table) => table.cells.filter((cell) => cell.source !== null));
    const expected = structuredClone(original).filter(
        (cell) => !(intent.deleted ?? []).includes(cellText(cell.node)),
    );
    for (const action of e.actions.filter((action) => action.operation === 'insertText')) {
        const target = expected.find((cell) => cell.sourceId === action.target?.sourceId);
        if (target) target.node = prefixed(target.node, 'typed');
        else expected.push({ ...action.target!, node: prefixed(action.target!.node, 'typed') });
    }
    const finalCells = e.settled.tables
        .filter((table) => table.parentCell === null)
        .flatMap((table) => table.cells.filter((cell) => cell.source !== null));
    equal(payloadTokens(finalCells), payloadTokens(expected), 'PRESERVATION_SETTLED_CONTENT');
    for (const cell of original) {
        const surviving = finalCells.filter((candidate) => candidate.sourceId === cell.sourceId);
        const mergeTarget = e.actions.some(
            (action) =>
                action.operation === 'merge' &&
                (action.target?.sourceId === cell.sourceId ||
                    action.head?.sourceId === cell.sourceId),
        );
        const deleted = (intent.deleted ?? []).includes(cellText(cell.node));
        if (!mergeTarget && !deleted) {
            requireEvidence(surviving.length === 1, 'PRESERVATION_SETTLED_IDENTITY');
            const intended = expected.find((candidate) => candidate.sourceId === cell.sourceId)!;
            equal(
                canonicalDocumentShape(surviving[0]!.node.content),
                canonicalDocumentShape(intended.node.content),
                'PRESERVATION_SETTLED_CELL_CONTENT',
            );
        }
        requireEvidence(surviving.length <= 1, 'PRESERVATION_SETTLED_IDENTITY');
        if (surviving.length === 1)
            equal(
                authoredAttributes(surviving[0]!.node),
                authoredAttributes(cell.node),
                'PRESERVATION_SETTLED_ATTRIBUTES',
            );
    }
    if (intent.steps.some((step) => step.operation === 'resize')) {
        for (const table of e.settled.tables) {
            requireEvidence(table.widths !== null, 'WIDTH_OBSERVATION');
            const resolved: (number | null)[] = [];
            for (let column = 0; column < table.columns; column++) {
                const contributions: (number | null)[] = [];
                for (let row = 0; row < table.rows; row++) {
                    for (const cell of table.cells) {
                        requireEvidence(
                            cell.row !== null &&
                                cell.column !== null &&
                                cell.rowspan !== null &&
                                cell.colspan !== null,
                            'WIDTH_GEOMETRY',
                        );
                        if (
                            row >= cell.row &&
                            row < cell.row + cell.rowspan &&
                            column >= cell.column &&
                            column < cell.column + cell.colspan
                        ) {
                            const widths = cell.node.attrs?.['colwidth'];
                            contributions.push(
                                Array.isArray(widths)
                                    ? (widths[column - cell.column] as number)
                                    : null,
                            );
                        }
                    }
                }
                resolved.push(resolveWidthContributions(contributions));
            }
            equal(table.widths, resolved, 'WIDTH_SURVIVING_RESOLVER');
        }
    }
}
export function resolveWidthContributions(widths: (number | null)[]): number | null {
    let candidate: number | null = null;
    let count = 0;
    for (const width of widths) {
        if (!width) continue;
        if (candidate === width) count++;
        else if (count <= 1) {
            candidate = width;
            count = 1;
        }
    }
    return candidate;
}
