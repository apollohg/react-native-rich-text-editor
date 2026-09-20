import assert from 'node:assert/strict';
import { isDeepStrictEqual } from 'node:util';
import { canonicalDocumentShape } from './assertions.js';
import {
    assertContinuationHistory,
    assertSourcePreservation,
    continuationTarget,
    nodeSize,
    outsideShape,
    requireContinuity as requireEvidence,
} from './continuity-evidence.js';
import {
    assertNativeNoRemoteLifetime,
    assertNativeRemoteLifetime,
    assertSourceRowInsertion,
} from './supplementary-evidence.js';
import { cellText, realCells, type RecordedAction } from './scenario-evidence.js';
import { rawView, type TextHistoryObservation } from './text-history-evidence.js';
import type { ContinuationSlot, ContinuationResult, ContinuationCheckpoint } from './corpus.js';
import type { EffectiveCell, EffectiveDocument, JsonNode } from './peer-protocol.js';
import { assertEffectivePresentation } from './presentation-semantics.js';

export type UnavailableOutcome = 'verified-native-refusal' | 'verified-stock-limitation';

function assertCheckpointObservations(checkpoint: ContinuationCheckpoint): void {
    const observations = checkpoint.textHistoryObservations!;
    assert.deepEqual(
        checkpoint.observations,
        observations.map(({ kind, document }) => ({ kind, document })),
        'availability checkpoint observation linkage',
    );
    const raw = canonicalDocumentShape(observations[0]!.raw);
    for (const view of observations)
        assert.deepEqual(
            canonicalDocumentShape(view.raw),
            raw,
            'availability replay raw convergence',
        );
    const views = checkpoint.presentation.views;
    requireEvidence(
        views && views.native.length > 0 && views.web.length > 0,
        'availability presentation inputs',
    );
    const withoutIdentity = (document: EffectiveDocument) => {
        const copy = structuredClone(document);
        for (const cell of copy.tables.flatMap((table) => table.cells)) delete cell.sourceId;
        return copy;
    };
    const native = observations.filter((view) => view.kind === 'rust');
    const web = observations.filter((view) => view.kind !== 'rust');
    requireEvidence(
        views.native.length === Math.max(1, native.length) &&
            views.web.length === Math.max(1, web.length),
        'availability presentation participant count',
    );
    native.forEach((view, index) =>
        assert.deepEqual(
            withoutIdentity(views.native[index]!),
            withoutIdentity(view.document),
            'availability native presentation linkage',
        ),
    );
    web.forEach((view, index) =>
        assert.deepEqual(views.web[index], view.document, 'availability web presentation linkage'),
    );
    const comparisons = views.native
        .slice(1)
        .map((view) => assertEffectivePresentation(views.native[0]!, view));
    for (const web of views.web)
        for (const native of views.native)
            comparisons.push(assertEffectivePresentation(native, web));
    assert.deepEqual(
        checkpoint.presentation.comparisons,
        comparisons,
        'availability replay presentation',
    );
}

export function assertSuccessfulStructuralResult(result: ContinuationResult): void {
    const action = result.actions[0]!;
    const initial = result.checkpoints[0]?.observations[result.slot.actor];
    requireEvidence(initial && action.availabilityBoundary, 'successful structural baseline');
    const target = declaredStructuralTarget(result.slot, {
        ...initial,
        raw: action.rawBefore as JsonNode,
    });
    assert.deepEqual(action.before, initial.document, 'successful structural baseline linkage');
    assert.deepEqual(
        action.rawBefore,
        action.availabilityBoundary.before.documentJson,
        'successful structural raw linkage',
    );
    assertSourceRowInsertion(
        action,
        { actor: result.slot.actor, sourceId: target.sourceId! },
        result.checkpoints[1]!.observations,
    );
    if (result.slot.proof !== 'history') return;
    if ('family' in result.slot && result.slot.family === 'native-owned-normalization') {
        const views = (boundary: string) => {
            const checkpoints = result.checkpoints.filter(
                (checkpoint) => checkpoint.boundary === boundary,
            );
            requireEvidence(checkpoints.length === 1, 'lifetime availability checkpoint');
            return checkpoints[0]!.observations;
        };
        if ('history' in result.slot && result.slot.history === 'remote') {
            requireEvidence(
                'remoteActor' in result.slot && typeof result.slot.remoteActor === 'number',
                'lifetime declared remote actor',
            );
            assertNativeRemoteLifetime(
                result.actions,
                {
                    actor: result.slot.actor,
                    remoteActor: result.slot.remoteActor,
                    sourceId: target.sourceId!,
                    text: 'remote-',
                },
                [views('remote-content'), views('undo'), views('redo')],
            );
        } else {
            requireEvidence(
                'history' in result.slot && result.slot.history === 'no-remote',
                'lifetime declared history',
            );
            assertNativeNoRemoteLifetime(
                result.actions,
                { actor: result.slot.actor, sourceId: target.sourceId! },
                [views('undo'), views('redo')],
            );
        }
    } else assertContinuationHistory(result.actions, result.slot.actor);
}

export function availabilityVerified(result: ContinuationResult): boolean {
    try {
        requireEvidence(
            result.failures.length === 0 &&
                result.required === true &&
                result.actions.length === 1 &&
                result.baseline?.evidence.status === 'proven' &&
                result.baseline.rawConvergence.passed &&
                result.baseline.evidence.failures.length === 0,
            'availability complete result',
        );
        requireEvidence(
            result.checkpoints.map((checkpoint) => checkpoint.boundary).join(',') ===
                'baseline,unavailable-action',
            'availability checkpoints',
        );
        for (const checkpoint of result.checkpoints) {
            requireEvidence(
                checkpoint.raw.passed &&
                    checkpoint.presentation.passed &&
                    checkpoint.presentation.failures.length === 0 &&
                    checkpoint.presentation.comparisons.length > 0 &&
                    checkpoint.drain.passed &&
                    checkpoint.nativeAutonomousRepairWrites === 0 &&
                    checkpoint.observationFailures.length === 0,
                'availability checkpoint invariants',
            );
            count(checkpoint.drain.rounds, 100);
            count(checkpoint.drain.emitted, 10_000);
            requireEvidence(
                checkpoint.remoteBoundaries.every(
                    (boundary) =>
                        boundary.kind !== 'rust' ||
                        (boundary.passes === 0 && boundary.autonomous === 0),
                ),
                'availability remote repair',
            );
            requireEvidence(
                checkpoint.textHistoryObservations?.length === result.slot.schedule.participants &&
                    checkpoint.textHistoryObservations.every(
                        (view, index) => view.kind === result.slot.schedule.kinds[index],
                    ),
                'availability participant observations',
            );
            for (const view of checkpoint.textHistoryObservations)
                assertAvailabilityPreservation(result.actions[0]!, view);
            assertCheckpointObservations(checkpoint);
        }
        const outcome = assertUnavailableAction(
            result.actions[0]!,
            result.slot,
            result.checkpoints[0]!.textHistoryObservations![result.slot.actor]!,
        );
        requireEvidence(
            result.availability === outcome &&
                result.disposition ===
                    (outcome === 'verified-native-refusal' ? 'refused' : 'limited'),
            'availability result linkage',
        );
        return true;
    } catch {
        return false;
    }
}

export function declaredStructuralTarget(
    slot: ContinuationSlot,
    initial: TextHistoryObservation,
): EffectiveCell {
    const cells = realCells(initial.document);
    const candidates =
        'target' in slot
            ? cells.filter((cell) => cellText(cell.node) === slot.target)
            : [continuationTarget(initial.document, slot.actorKind, slot.proof)];
    requireEvidence(
        candidates.length === 1 && candidates[0]?.sourceId,
        'availability unique declared target',
    );
    const target = candidates[0]!;
    requireEvidence(
        cells.filter((cell) => cell.sourceId === target.sourceId).length === 1,
        'availability unique source identity',
    );
    return target;
}

function atSource(raw: JsonNode, source: string): JsonNode {
    let node = raw;
    for (const part of source.split('.')) {
        requireEvidence(
            /^\d+$/.test(part) && node.content?.[Number(part)],
            'availability raw source',
        );
        node = node.content![Number(part)]!;
    }
    return node;
}

export function malformedRawTable(table: JsonNode): boolean {
    const rows = table.content ?? [];
    requireEvidence(
        table.type === 'table' && rows.length <= 65_536,
        'availability raw table budget',
    );
    if (rows.length === 0) return true;
    const occupied = new Set<string>();
    let malformed = false,
        width = 0,
        work = 0;
    for (const [row, node] of rows.entries()) {
        requireEvidence(['table_row', 'tableRow'].includes(node.type), 'availability raw row');
        let column = 0;
        for (const cell of node.content ?? []) {
            requireEvidence(
                ['table_cell', 'table_header', 'tableCell', 'tableHeader'].includes(cell.type),
                'availability raw cell',
            );
            const colspan = Number(cell.attrs?.colspan ?? 1),
                rowspan = Number(cell.attrs?.rowspan ?? 1);
            requireEvidence(
                Number.isSafeInteger(colspan) &&
                    colspan > 0 &&
                    Number.isSafeInteger(rowspan) &&
                    rowspan > 0 &&
                    colspan * rowspan <= 65_536,
                'availability span budget',
            );
            while (occupied.has(`${row}:${column}`)) column++;
            if (row + rowspan > rows.length) malformed = true;
            for (let r = row; r < Math.min(row + rowspan, rows.length); r++)
                for (let c = column; c < column + colspan; c++) {
                    requireEvidence(++work <= 65_536, 'availability raw grid budget');
                    const key = `${r}:${c}`;
                    if (occupied.has(key)) malformed = true;
                    occupied.add(key);
                }
            column += colspan;
            width = Math.max(width, column);
        }
    }
    requireEvidence(width * rows.length <= 65_536, 'availability raw dimensions');
    return malformed || width === 0 || occupied.size !== width * rows.length;
}

export function assertAvailabilityPreservation(
    action: RecordedAction,
    view: TextHistoryObservation,
): void {
    const coordinates = { before: action.kind, after: view.kind };
    assertSourcePreservation(action.before, view.document, new Map(), true, coordinates);
    assertSourcePreservation(
        action.before,
        rawView(view.document, view.raw),
        new Map(),
        true,
        coordinates,
    );
    assert.deepEqual(
        outsideShape(view.raw),
        outsideShape(action.rawBefore as JsonNode),
        'availability outside content',
    );
}

function count(value: unknown, max: number): void {
    requireEvidence(
        typeof value === 'number' && Number.isSafeInteger(value) && value >= 0 && value <= max,
        'availability bounded count',
    );
}
function revision(value: unknown): void {
    requireEvidence(typeof value === 'string' && /^\d+$/.test(value), 'availability revision');
}

function nativeRefusal(action: RecordedAction): void {
    const boundary = action.availabilityBoundary!;
    requireEvidence(
        !action.commandError &&
            action.reply.type === 'notApplicable' &&
            action.reply.positionFallback === false &&
            action.passes === 1,
        'native explicit bounded refusal',
    );
    const audit = action.reply.availability as Record<string, any>;
    requireEvidence(
        audit?.observed === true &&
            audit.auditStatus === 'complete' &&
            audit.reason === 'irregular-prepared-grid',
        'native witnessed guard',
    );
    assert.deepEqual(
        audit.auditLimits,
        { maxComponentBytes: 16_777_216, maxItems: 65_536 },
        'native audit limits',
    );
    for (const key of [
        'historyUnchanged',
        'historyMetadataUnchanged',
        'outboxUnchanged',
        'encodedStateUnchanged',
        'contentUnchanged',
        'revisionUnchanged',
    ])
        requireEvidence(audit[key] === true, `native ${key}`);
    revision(audit.requestId);
    const envelope = JSON.parse(audit.command);
    assert.deepEqual(
        envelope,
        {
            version: 1,
            requestId: audit.requestId,
            baseDocumentRevision: boundary.before.documentRevision,
            command: { type: 'addTableRow', side: 'after' },
        },
        'native request envelope',
    );
    requireEvidence(audit.before && audit.after, 'native complete audit boundaries');
    for (const state of [audit.before, audit.after]) {
        for (const key of ['documentRevision', 'stateRevision', 'yrsStateEpoch'])
            revision(state[key]);
        requireEvidence(
            Array.isArray(state.historyCounts) && state.historyCounts.length === 3,
            'native history counts',
        );
        state.historyCounts.forEach((n: unknown) => count(n, 65_536));
        count(state.historyMetadataItems, 65_536);
        requireEvidence(
            Array.isArray(state.outboxCountAndBytes) && state.outboxCountAndBytes.length === 2,
            'native outbox counts',
        );
        count(state.outboxCountAndBytes[0], 65_536);
        count(state.outboxCountAndBytes[1], 16_777_216);
        requireEvidence(
            typeof state.encodedState === 'string' &&
                state.encodedState.length > 0 &&
                Buffer.from(state.encodedState, 'base64').toString('base64') ===
                    state.encodedState &&
                Buffer.from(state.encodedState, 'base64').length <= 16_777_216,
            'native complete encoded bytes',
        );
    }
    assert.deepEqual(audit.after, audit.before, 'native atomic audit values');
    assert.equal(
        audit.before.encodedState,
        boundary.encodedBefore,
        'native independent before encoding',
    );
    assert.equal(
        audit.after.encodedState,
        boundary.encodedAfter,
        'native independent after encoding',
    );
    assert.equal(
        audit.after.documentRevision,
        boundary.after.documentRevision,
        'native after revision',
    );
    assert.equal(
        audit.after.stateRevision,
        boundary.after.stateRevision,
        'native after state revision',
    );
    const setup = audit.selectionSetup;
    requireEvidence(setup && Array.isArray(setup.events), 'native selection setup observation');
    assert.deepEqual(
        setup,
        {
            documentRevisionBefore: boundary.before.documentRevision,
            documentRevisionAfter: audit.before.documentRevision,
            stateRevisionBefore: boundary.before.stateRevision,
            stateRevisionAfter: audit.before.stateRevision,
            emittedEvents: 0,
            events: [],
        },
        'native selection setup linkage',
    );
    assert.deepEqual(
        boundary.before.queuedEvents,
        boundary.after.queuedEvents,
        'native independent queued events',
    );
    requireEvidence(
        Array.isArray(boundary.before.queuedEvents) && Array.isArray(boundary.after.queuedEvents),
        'native observed queued events',
    );
    assert.deepEqual(action.rawAfter, action.rawBefore, 'native unchanged raw content');
}

function stockLimitation(action: RecordedAction, target: EffectiveCell): void {
    const observed = action.stockRowInsertion;
    requireEvidence(observed && !('observationFailure' in observed), 'stock live observation');
    const rowType = observed.table.content?.[0]?.type;
    requireEvidence(
        rowType === 'table_row' || rowType === 'tableRow',
        'stock observed schema row type',
    );
    assert.deepEqual(
        action.commandError,
        { code: 'INTERNAL_ERROR', message: `TransformError: Invalid content for node ${rowType}` },
        'stock exact exception',
    );
    assert.deepEqual(action.request!.payload, observed.command, 'stock observed command linkage');
    assert.deepEqual(
        canonicalDocumentShape(observed.document),
        canonicalDocumentShape(action.availabilityBoundary!.before.displayJson),
        'stock independent live display boundary',
    );
    const versions = action.availabilityBoundary!.versions;
    for (const [name, version] of Object.entries({
        'prosemirror-tables': '1.8.5',
        'prosemirror-model': '1.25.11',
        'prosemirror-state': '1.4.4',
        'prosemirror-transform': '1.12.1',
        'y-prosemirror': '1.3.7',
        '@tiptap/core': '3.31.3',
        '@tiptap/extension-table': '3.31.3',
    }))
        assert.equal(versions[name], version, `stock pinned ${name}`);
    const table = action.before.tables.find((table) =>
        table.cells.some((cell) => cell.sourceId === target.sourceId),
    );
    requireEvidence(table, 'stock target table');
    assert.deepEqual(
        canonicalDocumentShape(observed.table),
        canonicalDocumentShape(table.node),
        'stock live table baseline',
    );
    assert.equal(observed.tableStart, table.position + 1, 'stock table position');
    requireEvidence(
        observed.selection.anchor === observed.selection.head &&
            observed.selection.anchor > target.position &&
            observed.selection.anchor < target.position + nodeSize(target.node, action.kind),
        'stock selected intended cell',
    );
    count(observed.width, 65_536);
    count(observed.height, 65_536);
    requireEvidence(
        observed.width > 0 &&
            observed.height > 0 &&
            observed.width * observed.height <= 65_536 &&
            observed.map.length === observed.width * observed.height,
        'stock map dimensions',
    );
    const nodes = new Map<number, JsonNode>();
    let offset = 0;
    for (const row of observed.table.content ?? []) {
        nodes.set(offset, row);
        let cellOffset = offset + 1;
        for (const cell of row.content ?? []) {
            nodes.set(cellOffset, cell);
            cellOffset += nodeSize(cell, action.kind);
        }
        offset += nodeSize(row, action.kind);
    }
    observed.map.forEach((position) =>
        requireEvidence(
            Number.isSafeInteger(position) && nodes.has(position),
            'stock map node reference',
        ),
    );
    const targetOffset = target.position - observed.tableStart;
    const targetSlots = observed.map.flatMap((position, index) =>
        position === targetOffset ? [index] : [],
    );
    requireEvidence(
        targetSlots.length > 0 &&
            observed.row === Math.floor(Math.max(...targetSlots) / observed.width) + 1,
        'stock insertion boundary',
    );
    let referenceRow: number | null = observed.row > 0 ? observed.row - 1 : 0;
    if (
        observed.map
            .slice(referenceRow * observed.width, (referenceRow + 1) * observed.width)
            .every((position) =>
                ['table_header', 'tableHeader'].includes(nodes.get(position)!.type),
            )
    )
        referenceRow = observed.row === 0 || observed.row === observed.height ? null : observed.row;
    assert.equal(observed.referenceRow, referenceRow, 'stock actual reference row');
    const slots = [];
    for (
        let column = 0, index = observed.width * observed.row;
        column < observed.width;
        column++, index++
    ) {
        const span =
            observed.row > 0 &&
            observed.row < observed.height &&
            observed.map[index] === observed.map[index - observed.width];
        const position = span
            ? observed.map[index]!
            : referenceRow === null
              ? null
              : observed.map[index + (referenceRow - observed.row) * observed.width]!;
        const node = position === null ? undefined : nodes.get(position);
        const role = !node
            ? 'cell'
            : ['table_row', 'tableRow'].includes(node.type)
              ? 'row'
              : ['table_header', 'tableHeader'].includes(node.type)
                ? 'header_cell'
                : 'cell';
        const colspan = span ? Number(node?.attrs?.colspan) : 1;
        slots.push({
            column,
            index,
            branch: span ? 'span' : 'create',
            position,
            role,
            colspan: Number.isFinite(colspan) ? colspan : null,
        });
        if (span) {
            if (!Number.isInteger(colspan) || colspan < 1) break;
            column += colspan - 1;
        }
    }
    assert.deepEqual(observed.slots, slots, 'stock visited reference slots');
    requireEvidence(
        slots.some(
            (slot) => slot.branch === 'create' && slot.position === 0 && slot.role === 'row',
        ),
        'stock witnessed hole row construction',
    );
    const afterTable = action.after.tables.find((other) => other.source === table.source);
    assert.deepEqual(
        canonicalDocumentShape(afterTable?.node),
        canonicalDocumentShape(observed.table),
        'stock no partial display insertion',
    );
    const rawAfter = canonicalDocumentShape(action.rawAfter);
    requireEvidence(
        isDeepStrictEqual(rawAfter, canonicalDocumentShape(action.rawBefore)) ||
            isDeepStrictEqual(rawAfter, canonicalDocumentShape(observed.document)),
        'stock no partial raw insertion',
    );
}

export function assertUnavailableAction(
    action: RecordedAction,
    slot: ContinuationSlot,
    initial: TextHistoryObservation,
): UnavailableOutcome {
    requireEvidence(
        ['structure', 'history'].includes(slot.proof) &&
            slot.required === true &&
            slot.actorKind === slot.schedule.kinds[slot.actor] &&
            slot.preset === slot.schedule.preset &&
            slot.topology === slot.schedule.topology,
        'availability declared structural obligation',
    );
    requireEvidence(
        action.actor === slot.actor &&
            action.kind === slot.actorKind &&
            initial.kind === slot.actorKind &&
            action.operation === 'addRow' &&
            !action.observationFailure &&
            action.autonomous === 0,
        'availability actual action',
    );
    const target = declaredStructuralTarget(slot, initial);
    assert.deepEqual(action.before, initial.document, 'availability effective baseline');
    assert.deepEqual(action.rawBefore, initial.raw, 'availability raw baseline');
    assert.deepEqual(action.target, target, 'availability requested source');
    assert.deepEqual(
        action.request,
        {
            operation: 'command',
            payload:
                action.kind === 'rust'
                    ? { type: 'addTableRow', side: 'after', at: target.position + 1 }
                    : { type: 'tableCommand', name: 'addRowAfter', at: target.position + 1 },
        },
        'availability exact command',
    );
    const table = initial.document.tables.find((table) =>
        table.cells.some((cell) => cell.sourceId === target.sourceId),
    );
    requireEvidence(
        table && malformedRawTable(atSource(initial.raw, table.source)),
        'availability independently malformed raw target',
    );
    const boundary = action.availabilityBoundary;
    requireEvidence(
        boundary && !boundary.before.pendingDependencies && !boundary.after.pendingDependencies,
        'availability observed settled action boundaries',
    );
    assert.deepEqual(
        boundary.before.documentJson,
        action.rawBefore,
        'availability before snapshot linkage',
    );
    assert.deepEqual(
        boundary.after.documentJson,
        action.rawAfter,
        'availability after snapshot linkage',
    );
    assertAvailabilityPreservation(action, initial);
    assertAvailabilityPreservation(action, {
        kind: action.kind,
        document: action.after,
        raw: action.rawAfter as JsonNode,
    });
    if (action.kind === 'rust') {
        nativeRefusal(action);
        return 'verified-native-refusal';
    }
    stockLimitation(action, target);
    return 'verified-stock-limitation';
}
