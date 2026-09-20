import { assertConverged } from './assertions.js';
import assert from 'node:assert/strict';
import {
    PARTICIPANT_COUNT_KEY,
    PeerError,
    exchangeUntilIdle,
    flushDocumentEvents,
    peerKindOf,
    seedFrom,
    snapshot,
    tableFixture,
    withPeers,
    createScheduler,
} from './controller.js';
import type { SchemaPreset } from './controller.js';
import {
    GEOMETRY_ADMITTED,
    GEOMETRY_ORACLE_FAILED,
    GEOMETRY_PROJECTION_FAILED,
    GEOMETRY_RAW_JSON_DISAGREEMENT,
    TOPOLOGY_NATIVE_NATIVE,
    TOPOLOGY_NATIVE_TWO_WEB,
    TOPOLOGY_NATIVE_WEB,
    TOPOLOGY_TWO_WEB_CONTROL,
} from './convergence-report.js';
import type { ConvergenceTopology, SettledGeometry } from './convergence-report.js';
import { NATIVE_PEER_KIND } from './peer-protocol.js';
import type {
    Peer,
    PeerKind,
    EffectiveDocument,
    EffectiveCell,
    EffectiveTable,
    JsonNode,
} from './peer-protocol.js';
import { nextRandom } from './scheduler.js';
import {
    CELL_NODE,
    HEADER_CELL_NODE,
    PARAGRAPH_NODE,
    ROW_NODE,
    TABLE_NODE,
    TIPTAP_CELL_NODE,
    TIPTAP_HEADER_CELL_NODE,
    TIPTAP_ROW_NODE,
    cellAnchors,
    tableOf,
    tableSchemaOf,
} from './table-schema.js';
import { failureClassOf, lastTrace, persistTrace } from './trace.js';
import {
    evidenceCall as call,
    observeEvidence,
    startEvidence,
    stopEvidence,
} from './evidence-observer.js';
import {
    assertActionEvidence,
    assertFamilyEvidence,
    declaredFamilyActors,
    FAMILY_INTENTS,
    cellText,
    realCells,
} from './scenario-evidence.js';
import type { CoverageStatus, FamilyEvidence, RecordedAction } from './scenario-evidence.js';
import { assertUnavailableAction, assertSuccessfulStructuralResult, availabilityVerified, type UnavailableOutcome } from './availability-evidence.js';
import {
    assertTypingContinuation,
    assertSourcePreservation,
    assertStructuralContinuation,
    type ContinuationObservation,
    assertGapContinuation,
    assertGapRefusal,
    assertUnrelatedContinuation,
    assertContinuationHistory,
    requireContinuity,
    typingCursor,
    nodeSize,
    outsideShape,
    continuationTarget,
} from './continuity-evidence.js';
import {
    assertEffectivePresentation,
    observeNativePresentation,
    observeWebPresentation,
} from './presentation-semantics.js';
import type { PresentationCheck } from './presentation-semantics.js';

import {
    assertTextHistoryBoundary,
    assertTextHistoryState,
    textHistoryIntent,
    type TextHistoryIntent,
    type TextHistoryObservation,
} from './text-history-evidence.js';

export const SCHEDULES_PER_TOPOLOGY = 100;
export const CORPUS_PRESETS: readonly SchemaPreset[] = ['prosemirror', 'tiptap'];
export const CORPUS_BASE_SEED = 0x7ab1_c0f5;

const DOC_NODE = 'doc';
const TEXT_NODE = 'text';
const TABLE_START = 0;
const SINGLE_SPAN = 1;
const NO_LOOPS = 0;
const NO_REPAIR_WRITES = 0;
const TOP_LEFT_CELL = 0;
const TOP_RIGHT_CELL = 1;
const BOTTOM_LEFT_CELL = 2;
const TYPED_TEXT = 'typed';
const OUT_OF_ORDER_DELIVERY = 2;
const ONE_CELL = 1;
const RESIZED_WIDTH = 180;
const OTHER_RESIZED_WIDTH = 220;
const NO_ACTORS = 0;
const ONE_ACTOR = 1;
const TWO_ACTORS = 2;

type PresetNodes = {
    readonly row: string;
    readonly cell: string;
    readonly headerCell: string;
};

const PRESET_NODES: Record<SchemaPreset, PresetNodes> = {
    prosemirror: { row: ROW_NODE, cell: CELL_NODE, headerCell: HEADER_CELL_NODE },
    tiptap: { row: TIPTAP_ROW_NODE, cell: TIPTAP_CELL_NODE, headerCell: TIPTAP_HEADER_CELL_NODE },
};

export function tableSchemaForPreset(preset: SchemaPreset): Record<string, unknown> {
    const nodes = PRESET_NODES[preset];
    return tableSchemaOf(nodes.row, nodes.cell, nodes.headerCell);
}

function presetCell(preset: SchemaPreset, text: string): Record<string, unknown> {
    return {
        type: PRESET_NODES[preset].cell,
        attrs: { colspan: SINGLE_SPAN, rowspan: SINGLE_SPAN, colwidth: null },
        content: [{ type: PARAGRAPH_NODE, content: [{ type: TEXT_NODE, text }] }],
    };
}

export function corpusTable(preset: SchemaPreset): Record<string, unknown> {
    const row = PRESET_NODES[preset].row;
    return {
        type: TABLE_NODE,
        content: [
            { type: row, content: [presetCell(preset, 'a'), presetCell(preset, 'b')] },
            { type: row, content: [presetCell(preset, 'c'), presetCell(preset, 'd')] },
        ],
    };
}

async function addRowAfter(peer: Peer, at: number): Promise<void> {
    if (peerKindOf(peer) === NATIVE_PEER_KIND) {
        await call(peer, 'command', { type: 'addTableRow', side: 'after', at });
        return;
    }
    await call(peer, 'command', { type: 'tableCommand', name: 'addRowAfter', at });
}

async function addColumnAfter(peer: Peer, at: number): Promise<void> {
    if (peerKindOf(peer) === NATIVE_PEER_KIND) {
        await call(peer, 'command', { type: 'addTableColumn', side: 'after', at });
        return;
    }
    await call(peer, 'command', { type: 'tableCommand', name: 'addColumnAfter', at });
}

async function mergeCells(peer: Peer, at: number, head: number): Promise<void> {
    if (peerKindOf(peer) === NATIVE_PEER_KIND) {
        await call(peer, 'command', { type: 'mergeTableCells', at, head });
        return;
    }
    await call(peer, 'command', { type: 'tableCommand', name: 'mergeCells', at, head });
}

async function typeInCell(peer: Peer, at: number): Promise<void> {
    await call(peer, 'command', { type: 'insertText', text: TYPED_TEXT, at });
}

async function splitCell(peer: Peer, at: number): Promise<void> {
    if (peerKindOf(peer) === NATIVE_PEER_KIND) {
        await call(peer, 'command', { type: 'splitTableCell', at });
        return;
    }
    await call(peer, 'command', { type: 'tableCommand', name: 'splitCell', at });
}

async function deleteRow(peer: Peer, at: number): Promise<void> {
    if (peerKindOf(peer) === NATIVE_PEER_KIND) {
        await call(peer, 'command', { type: 'deleteTableRows', at });
        return;
    }
    await call(peer, 'command', { type: 'tableCommand', name: 'deleteRow', at });
}

async function resizeColumn(peer: Peer, at: number, width: number): Promise<void> {
    await call(peer, 'command', { type: 'setTableColumnWidth', width, at });
}

export function raggedCorpusTable(preset: SchemaPreset): Record<string, unknown> {
    const row = PRESET_NODES[preset].row;
    return {
        type: TABLE_NODE,
        content: [
            {
                type: row,
                content: [
                    presetCell(preset, 'a'),
                    presetCell(preset, 'b'),
                    presetCell(preset, 'c'),
                ],
            },
            { type: row, content: [presetCell(preset, 'd')] },
        ],
    };
}

function nestedIrregularTable(preset: SchemaPreset): Record<string, unknown> {
    const row = PRESET_NODES[preset].row;
    const cellType = PRESET_NODES[preset].cell;
    const inner = {
        type: TABLE_NODE,
        content: [
            { type: row, content: [presetCell(preset, 'x'), presetCell(preset, 'y')] },
            { type: row, content: [presetCell(preset, 'z')] },
        ],
    };
    const hostCell = {
        type: cellType,
        attrs: { colspan: SINGLE_SPAN, rowspan: SINGLE_SPAN, colwidth: null },
        content: [inner],
    };
    return {
        type: TABLE_NODE,
        content: [
            { type: row, content: [hostCell, presetCell(preset, 'b')] },
            { type: row, content: [presetCell(preset, 'c'), presetCell(preset, 'd')] },
        ],
    };
}

type ScenarioContext = {
    readonly peers: readonly Peer[];
    readonly fixture: Record<string, unknown>;
    readonly liveTable: () => Promise<Record<string, unknown>>;
    readonly author: Peer;
    readonly anchors: readonly number[];
    readonly liveAnchors: () => Promise<number[]>;
    readonly liveRowAnchors: () => Promise<number[][]>;
    readonly preset: SchemaPreset;
    readonly seed: number;
};

export type ScenarioEvidence = {
    readonly settledTable: Record<string, unknown>;
    readonly seededTable: Record<string, unknown>;
    readonly fixtureTable: Record<string, unknown>;
    readonly boundaries?: FamilyEvidence;
};

export type CorpusScenario = {
    readonly name: string;
    readonly mutatesGeometry: boolean;
    readonly proves?: (evidence: ScenarioEvidence) => void;
    readonly minimumNativeActors: number;
    readonly minimumWebActors: number;
    readonly table?: (preset: SchemaPreset) => Record<string, unknown>;
    readonly act: (context: ScenarioContext) => Promise<void>;
};

function peerAt(peers: readonly Peer[], index: number): Peer {
    const peer = peers[index];
    if (peer === undefined) {
        throw new Error(`the corpus scenario addressed peer ${index} outside the started set`);
    }
    return peer;
}

function nonAuthoringPeer(peers: readonly Peer[], author: Peer): Peer {
    const editor = peers.find((peer) => peer !== author);
    if (editor === undefined) {
        throw new Error('the corpus scenario needs a peer that did not author the table');
    }
    return editor;
}

function webActor(peers: readonly Peer[]): Peer {
    const web = peers.find((peer) => peerKindOf(peer) !== NATIVE_PEER_KIND);
    if (web === undefined) {
        throw new Error('the corpus scenario needs a web actor');
    }
    return web;
}

function nativeActor(peers: readonly Peer[]): Peer {
    const native = peers.find((peer) => peerKindOf(peer) === NATIVE_PEER_KIND);
    if (native === undefined) {
        throw new Error('the corpus scenario needs a native actor');
    }
    return native;
}

function anchorAt(anchors: readonly number[], index: number): number {
    const anchor = anchors[index];
    if (anchor === undefined) {
        throw new Error(`the corpus fixture exposes no cell anchor ${index}`);
    }
    return anchor;
}

const ONE_OCCURRENCE = 1;
const ONE_ROW = 1;
const NO_SLOTS = 0;
const FIRST_MARKER = 0;
const FIRST_CELL_IN_ROW = 0;

function rowGroupedAnchors(tableJson: Record<string, unknown>): number[][] {
    const flat = cellAnchors(tableJson, TABLE_START);
    const grouped: number[][] = [];
    let cursor = 0;
    for (const rowJson of tableRows(tableJson)) {
        const cells = rowJson['content'];
        const width = Array.isArray(cells) ? cells.length : 0;
        grouped.push(flat.slice(cursor, cursor + width));
        cursor += width;
    }
    return grouped;
}

function tableRows(tableJson: Record<string, unknown>): Record<string, unknown>[] {
    const rows = tableJson['content'];
    return Array.isArray(rows) ? (rows as Record<string, unknown>[]) : [];
}

function structuralText(node: unknown, collected: string[]): void {
    if (Array.isArray(node)) {
        for (const entry of node) {
            structuralText(entry, collected);
        }
        return;
    }
    if (typeof node !== 'object' || node === null) {
        return;
    }
    const record = node as Record<string, unknown>;
    const text = record['text'];
    if (typeof text === 'string') {
        collected.push(text);
    }
    structuralText(record['content'], collected);
}

function cellTexts(cellJson: Record<string, unknown>): string {
    const collected: string[] = [];
    structuralText(cellJson['content'], collected);
    return collected.join('');
}

function rowCells(rowJson: Record<string, unknown>): Record<string, unknown>[] {
    const cells = rowJson['content'];
    return Array.isArray(cells) ? (cells as Record<string, unknown>[]) : [];
}

function rowTexts(rowJson: Record<string, unknown>): string[] {
    return rowCells(rowJson).map((cellJson) => cellTexts(cellJson));
}

function fixtureRowWidths(fixtureTable: Record<string, unknown>): number[] {
    return tableRows(fixtureTable).map((rowJson) => rowCells(rowJson).length);
}

function preExistingRowMarkers(fixtureTable: Record<string, unknown>): string[] {
    return tableRows(fixtureTable).map((rowJson) => rowTexts(rowJson)[FIRST_CELL_IN_ROW] ?? '');
}

function gapFilledSlots(
    fixtureTable: Record<string, unknown>,
    settledTable: Record<string, unknown>,
): { rowIndex: number; columnIndex: number }[] {
    const widths = fixtureRowWidths(fixtureTable);
    const markers = preExistingRowMarkers(fixtureTable);
    const slots: { rowIndex: number; columnIndex: number }[] = [];
    for (const [rowIndex, rowJson] of tableRows(settledTable).entries()) {
        const texts = rowTexts(rowJson);
        const fixtureIndex = markers.findIndex(
            (marker) => marker.length > 0 && texts[FIRST_CELL_IN_ROW] === marker,
        );
        if (fixtureIndex === -1) {
            continue;
        }
        const authored = widths[fixtureIndex] ?? 0;
        for (let columnIndex = authored; columnIndex < texts.length; columnIndex += 1) {
            slots.push({ rowIndex, columnIndex });
        }
    }
    return slots;
}

function scenarioEvidenceFailure(detail: string): Error {
    return new Error(`TBL-21 SCENARIO_EVIDENCE: ${detail}`);
}

function markerSlots(tableJson: Record<string, unknown>): { rowIndex: number; columnIndex: number }[] {
    const found: { rowIndex: number; columnIndex: number }[] = [];
    for (const [rowIndex, rowJson] of tableRows(tableJson).entries()) {
        for (const [columnIndex, text] of rowTexts(rowJson).entries()) {
            if (text.includes(TYPED_TEXT)) {
                found.push({ rowIndex, columnIndex });
            }
        }
    }
    return found;
}

function provesNormalizationHistory(evidence: ScenarioEvidence): void {
    const settledGaps = gapFilledSlots(evidence.fixtureTable, evidence.settledTable);
    if (settledGaps.length === NO_SLOTS) {
        throw scenarioEvidenceFailure(
            'normalization filled no gap in a pre-existing row, so no cell here was created by '
                + 'normalization',
        );
    }
    const settledRows = tableRows(evidence.settledTable);
    const seededRows = tableRows(evidence.seededTable);
    if (settledRows.length !== seededRows.length + ONE_ROW) {
        throw scenarioEvidenceFailure(
            `the redone row insertion must leave ${seededRows.length + ONE_ROW} rows, not `
                + `${settledRows.length}`,
        );
    }
    const carrying = markerSlots(evidence.settledTable);
    if (carrying.length !== ONE_OCCURRENCE) {
        throw scenarioEvidenceFailure(
            `the remote edit must survive in exactly one cell, found ${carrying.length}`,
        );
    }
    const edited = carrying[FIRST_MARKER];
    if (
        edited === undefined
        || !settledGaps.some(
            (slot) => slot.rowIndex === edited.rowIndex && slot.columnIndex === edited.columnIndex,
        )
    ) {
        throw scenarioEvidenceFailure(
            'the remote edit must survive in a cell normalization created, not one an insertion '
                + `created; it is at ${JSON.stringify(edited)} and the gap-filled slots are `
                + JSON.stringify(settledGaps),
        );
    }
}

function requireApplied(result: Record<string, unknown>, operation: string): void {
    if (result['applied'] !== true) {
        throw scenarioEvidenceFailure(
            `${operation} reported applied=${JSON.stringify(result['applied'])}, so the history `
                + 'step this scenario exists to prove never happened',
        );
    }
}

async function normalizationHistory(
    context: ScenarioContext,
    selectEditor: (peers: readonly Peer[]) => Peer,
): Promise<void> {
    const { peers, author, anchors, fixture, liveTable, liveRowAnchors, seed } = context;
    const editor = selectEditor(peers.filter((peer) => peer !== author));
    const typist = peers.find((peer) => peer !== editor);
    if (typist === undefined) {
        throw scenarioEvidenceFailure('the scenario needs a peer other than the editor to type');
    }

    const seededTable = await liveTable();
    const seededRowCount = tableRows(seededTable).length;
    await addRowAfter(editor, anchorAt(anchors, TOP_LEFT_CELL));
    await exchangeUntilIdle([...peers], seed);

    const inserted = await liveTable();
    if (tableRows(inserted).length !== seededRowCount + ONE_ROW) {
        throw scenarioEvidenceFailure(
            `the editor's row insertion must add one row, leaving ${seededRowCount + ONE_ROW}, not `
                + `${tableRows(inserted).length}`,
        );
    }
    const target = gapFilledSlots(fixture, inserted)[FIRST_MARKER];
    if (target === undefined) {
        throw scenarioEvidenceFailure(
            'normalization filled no gap in a pre-existing row, so there is nothing this '
                + 'scenario can remotely edit',
        );
    }
    const rows = await liveRowAnchors();
    const targetRow = rows[target.rowIndex];
    if (targetRow === undefined) {
        throw scenarioEvidenceFailure('the gap-filled row exposes no cell anchors');
    }
    await typeInCell(typist, anchorAt(targetRow, target.columnIndex));
    await exchangeUntilIdle([...peers], seed);

    requireApplied(await call(editor, 'undo', {}), 'undo');
    await exchangeUntilIdle([...peers], seed);
    const undone = await liveTable();
    if (tableRows(undone).length !== seededRowCount) {
        throw scenarioEvidenceFailure(
            `undoing the row insertion must restore ${seededRowCount} rows, not `
                + `${tableRows(undone).length}`,
        );
    }
    if (markerSlots(undone).length !== ONE_OCCURRENCE) {
        throw scenarioEvidenceFailure(
            'the remote edit must survive the undo of an unrelated structural action, found '
                + `${markerSlots(undone).length} occurrences`,
        );
    }

    requireApplied(await call(editor, 'redo', {}), 'redo');
}

const LEGACY_SCENARIOS: readonly CorpusScenario[] = [
    {
        name: 'concurrent row and column insertion at the same boundary',
        mutatesGeometry: true,
        minimumNativeActors: NO_ACTORS,
        minimumWebActors: NO_ACTORS,
        act: async ({ peers, anchors }) => {
            await addRowAfter(peerAt(peers, 0), anchorAt(anchors, TOP_LEFT_CELL));
            await addColumnAfter(peerAt(peers, 1), anchorAt(anchors, TOP_LEFT_CELL));
        },
    },
    {
        name: 'concurrent merges from opposite corners',
        mutatesGeometry: true,
        minimumNativeActors: NO_ACTORS,
        minimumWebActors: NO_ACTORS,
        act: async ({ peers, anchors }) => {
            await mergeCells(
                peerAt(peers, 0),
                anchorAt(anchors, TOP_LEFT_CELL),
                anchorAt(anchors, TOP_RIGHT_CELL),
            );
            await mergeCells(
                peerAt(peers, 1),
                anchorAt(anchors, BOTTOM_LEFT_CELL),
                anchorAt(anchors, TOP_LEFT_CELL),
            );
        },
    },
    {
        name: 'a structural action undone and redone by a peer that did not author the table',
        mutatesGeometry: true,
        minimumNativeActors: NO_ACTORS,
        minimumWebActors: NO_ACTORS,
        act: async ({ peers, author, anchors, seed }) => {
            const editor = nonAuthoringPeer(peers, author);
            await addRowAfter(editor, anchorAt(anchors, TOP_LEFT_CELL));
            await exchangeUntilIdle([...peers], seed);
            await call(editor, 'undo', {});
            await exchangeUntilIdle([...peers], seed);
            await call(editor, 'redo', {});
        },
    },
    {
        name: 'a dependent update released before its prerequisite',
        mutatesGeometry: true,
        minimumNativeActors: NO_ACTORS,
        minimumWebActors: NO_ACTORS,
        act: async ({ peers, anchors }) => {
            const author = peerAt(peers, 0);
            await addRowAfter(author, anchorAt(anchors, TOP_LEFT_CELL));
            await addColumnAfter(author, anchorAt(anchors, TOP_LEFT_CELL));
            const produced = await flushDocumentEvents(author);
            if (produced.length < OUT_OF_ORDER_DELIVERY) {
                throw new Error(
                    `the dependency scenario needs ${OUT_OF_ORDER_DELIVERY} updates, got ${produced.length}`,
                );
            }
            for (const [index, peer] of peers.entries()) {
                if (index === 0) {
                    continue;
                }
                for (const event of [...produced].reverse()) {
                    await call(peer, 'applyUpdate', { updateBase64: event.bytesBase64 });
                }
            }
        },
    },
    {
        name: 'typing inside a cell without touching geometry',
        mutatesGeometry: false,
        minimumNativeActors: NO_ACTORS,
        minimumWebActors: NO_ACTORS,
        act: async ({ peers, anchors }) => {
            await typeInCell(peerAt(peers, 0), anchorAt(anchors, TOP_LEFT_CELL));
        },
    },
    {
        name: 'a merged cell split against a concurrent row deletion',
        mutatesGeometry: true,
        minimumNativeActors: NO_ACTORS,
        minimumWebActors: NO_ACTORS,
        act: async ({ peers, anchors, liveAnchors, seed }) => {
            await mergeCells(
                peerAt(peers, 0),
                anchorAt(anchors, TOP_LEFT_CELL),
                anchorAt(anchors, TOP_RIGHT_CELL),
            );
            await exchangeUntilIdle([...peers], seed);
            const settled = await liveAnchors();
            await splitCell(peerAt(peers, 0), anchorAt(settled, TOP_LEFT_CELL));
            await deleteRow(peerAt(peers, 1), anchorAt(settled, settled.length - ONE_CELL));
        },
    },
    {
        name: 'a row inserted across a spanning cell',
        mutatesGeometry: true,
        minimumNativeActors: NO_ACTORS,
        minimumWebActors: NO_ACTORS,
        act: async ({ peers, anchors, liveAnchors, seed }) => {
            await mergeCells(
                peerAt(peers, 0),
                anchorAt(anchors, TOP_LEFT_CELL),
                anchorAt(anchors, TOP_RIGHT_CELL),
            );
            await exchangeUntilIdle([...peers], seed);
            const settled = await liveAnchors();
            await addRowAfter(peerAt(peers, 1), anchorAt(settled, TOP_LEFT_CELL));
            await addColumnAfter(peerAt(peers, 0), anchorAt(settled, TOP_LEFT_CELL));
        },
    },
    {
        name: 'a native column resize concurrent with a remote row insertion',
        mutatesGeometry: true,
        minimumNativeActors: ONE_ACTOR,
        minimumWebActors: NO_ACTORS,
        act: async ({ peers, anchors }) => {
            const native = nativeActor(peers);
            await resizeColumn(native, anchorAt(anchors, TOP_LEFT_CELL), RESIZED_WIDTH);
            await addRowAfter(
                nonAuthoringPeer(peers, native),
                anchorAt(anchors, TOP_LEFT_CELL),
            );
        },
    },
    {
        name: 'a native edit inside a normalization created cell, then undone and redone',
        mutatesGeometry: false,
        proves: provesNormalizationHistory,
        minimumNativeActors: ONE_ACTOR,
        minimumWebActors: NO_ACTORS,
        table: raggedCorpusTable,
        act: async (context) => normalizationHistory(context, nativeActor),
    },
    {
        name: 'a web edit inside a normalization created cell, then undone and redone',
        mutatesGeometry: false,
        proves: provesNormalizationHistory,
        minimumNativeActors: NO_ACTORS,
        minimumWebActors: TWO_ACTORS,
        table: raggedCorpusTable,
        act: async (context) => normalizationHistory(context, webActor),
    },
    {
        name: 'concurrent resizes of the same logical column',
        mutatesGeometry: true,
        minimumNativeActors: TWO_ACTORS,
        minimumWebActors: NO_ACTORS,
        act: async ({ peers, anchors }) => {
            await resizeColumn(peerAt(peers, 0), anchorAt(anchors, TOP_LEFT_CELL), RESIZED_WIDTH);
            await resizeColumn(
                peerAt(peers, 1),
                anchorAt(anchors, BOTTOM_LEFT_CELL),
                OTHER_RESIZED_WIDTH,
            );
        },
    },
    {
        name: 'concurrent resizes of different logical columns',
        mutatesGeometry: true,
        minimumNativeActors: TWO_ACTORS,
        minimumWebActors: NO_ACTORS,
        act: async ({ peers, anchors }) => {
            await resizeColumn(peerAt(peers, 0), anchorAt(anchors, TOP_LEFT_CELL), RESIZED_WIDTH);
            await resizeColumn(
                peerAt(peers, 1),
                anchorAt(anchors, TOP_RIGHT_CELL),
                OTHER_RESIZED_WIDTH,
            );
        },
    },
    {
        name: 'a web repair followed by a native undo',
        mutatesGeometry: false,
        minimumNativeActors: ONE_ACTOR,
        minimumWebActors: ONE_ACTOR,
        table: raggedCorpusTable,
        act: async ({ peers, anchors, seed }) => {
            const native = nativeActor(peers);
            await addRowAfter(native, anchorAt(anchors, TOP_LEFT_CELL));
            await exchangeUntilIdle([...peers], seed);
            await call(native, 'undo', {});
        },
    },
    {
        name: 'a nested irregular table under an outer structural edit',
        mutatesGeometry: true,
        minimumNativeActors: NO_ACTORS,
        minimumWebActors: NO_ACTORS,
        table: nestedIrregularTable,
        act: async ({ peers, liveAnchors }) => {
            const settled = await liveAnchors();
            await addRowAfter(peerAt(peers, 1), anchorAt(settled, settled.length - ONE_CELL));
        },
    },
];

export const CORPUS_SCENARIOS: readonly CorpusScenario[] = LEGACY_SCENARIOS.map((scenario, index) => ({
    ...scenario,
    proves: (evidence: ScenarioEvidence) => {
        if (!evidence.boundaries) throw scenarioEvidenceFailure('MISSING_ACTION boundaries');
        assertFamilyEvidence(FAMILY_INTENTS[index]!, evidence.boundaries);
        scenario.proves?.(evidence);
    },
}));

export interface CorpusSchedule {
    readonly name: string;
    readonly topology: ConvergenceTopology;
    readonly kinds: readonly PeerKind[];
    readonly participants: number;
    readonly preset: SchemaPreset;
    readonly scenario: CorpusScenario;
    readonly actorOffset: number;
    readonly seed: number;
}

function rotate(peers: readonly Peer[], offset: number): readonly Peer[] {
    return peers.map((_peer, index) => peerAt(peers, (index + offset) % peers.length));
}

export function scenariosFor(topology: ConvergenceTopology): readonly CorpusScenario[] {
    const { kinds, participants } = kindsFor(topology, CORPUS_PRESETS[0] ?? 'prosemirror');
    const actors = kinds.slice(0, participants);
    const native = actors.filter((kind) => kind === NATIVE_PEER_KIND).length;
    const web = actors.length - native;
    return CORPUS_SCENARIOS.filter(
        (scenario) => native >= scenario.minimumNativeActors && web >= scenario.minimumWebActors,
    );
}

function kindsFor(topology: ConvergenceTopology, preset: SchemaPreset): {
    kinds: readonly PeerKind[];
    participants: number;
} {
    if (topology === TOPOLOGY_NATIVE_NATIVE) {
        return { kinds: [NATIVE_PEER_KIND, NATIVE_PEER_KIND], participants: 2 };
    }
    if (topology === TOPOLOGY_NATIVE_WEB) {
        return { kinds: [preset, NATIVE_PEER_KIND], participants: 2 };
    }
    if (topology === TOPOLOGY_NATIVE_TWO_WEB) {
        return { kinds: [preset, preset, NATIVE_PEER_KIND], participants: 3 };
    }
    return { kinds: [preset, preset, NATIVE_PEER_KIND], participants: 2 };
}

const CORPUS_TOPOLOGY_ORDER: readonly ConvergenceTopology[] = [
    TOPOLOGY_NATIVE_NATIVE,
    TOPOLOGY_NATIVE_WEB,
    TOPOLOGY_NATIVE_TWO_WEB,
    TOPOLOGY_TWO_WEB_CONTROL,
];

function buildCorpus(): readonly CorpusSchedule[] {
    const schedules: CorpusSchedule[] = [];
    let seed = CORPUS_BASE_SEED;
    for (const topology of CORPUS_TOPOLOGY_ORDER) {
        for (let index = 0; index < SCHEDULES_PER_TOPOLOGY; index += 1) {
            seed = nextRandom(seed);
            const applicable = scenariosFor(topology);
            const scenario = applicable[index % applicable.length];
            const presetIndex = Math.floor(index / applicable.length) % CORPUS_PRESETS.length;
            const preset = CORPUS_PRESETS[presetIndex];
            if (preset === undefined || scenario === undefined) {
                throw new Error('the corpus generator produced an incomplete schedule');
            }
            const { kinds, participants } = kindsFor(topology, preset);
            const actorOffset = index % participants;
            schedules.push({
                name: `${topology} ${preset} ${scenario.name} actor ${actorOffset} seed ${seed}`,
                topology,
                kinds,
                participants,
                preset,
                scenario,
                actorOffset,
                seed,
            });
        }
    }
    return schedules;
}

export const CONVERGENCE_CORPUS: readonly CorpusSchedule[] = buildCorpus();

export type ContinuationProof =
    | 'typing'
    | 'structure'
    | 'history'
    | 'text-history'
    | 'partition'
    | 'unrelated-web'
    | 'web-gap';
export interface ContinuationSlot {
    readonly key: string;
    readonly schedule: CorpusSchedule;
    readonly topology: ConvergenceTopology;
    readonly preset: SchemaPreset;
    readonly baseFamily: string;
    readonly actor: number;
    readonly actorKind: PeerKind;
    readonly proof: ContinuationProof;
    readonly required: boolean | null;
    readonly status: CoverageStatus;
    readonly companionOf?: string;
    readonly textHistoryTarget?:
        | { readonly kind: 'cell-text'; readonly text: string }
        | { readonly kind: 'first-typable-source' };
}

export function textHistoryRequirements<T extends ContinuationSlot>(origins: readonly T[]): T[] {
    requireContinuity(
        new Set(origins.map((slot) => slot.key)).size === origins.length,
        'duplicate history declaration',
    );
    return origins.map((slot) => {
        requireContinuity(
            slot.proof === 'history' && !slot.companionOf,
            'original history obligation required',
        );
        return {
            ...slot,
            key: `${slot.key} :: text-history`,
            companionOf: slot.key,
            textHistoryTarget:
                'target' in slot
                    ? { kind: 'cell-text' as const, text: String(slot.target) }
                    : { kind: 'first-typable-source' as const },
            proof: 'text-history',
            required: true,
            status: 'unexercised',
        };
    });
}

function declaredTextHistoryTarget(
    slot: ContinuationSlot,
    before: EffectiveDocument,
): EffectiveCell {
    const policy = slot.textHistoryTarget;
    requireContinuity(policy, 'text-history target policy');
    requireContinuity(
        !('target' in slot) || (policy.kind === 'cell-text' && policy.text === slot.target),
        'text-history supplementary target policy',
    );
    let target: EffectiveCell | undefined;
    if (policy.kind === 'cell-text') {
        const matches = realCells(before).filter((cell) => cellText(cell.node) === policy.text);
        requireContinuity(matches.length === 1, 'unique declared text-history target');
        target = matches[0];
    } else {
        requireContinuity(policy.kind === 'first-typable-source', 'text-history target policy');
        target = continuationTarget(before, slot.actorKind, 'text-history');
    }
    requireContinuity(
        target?.sourceId &&
            realCells(before).filter((cell) => cell.sourceId === target.sourceId).length === 1,
        'unique declared text-history source',
    );
    return target;
}

export function continuationRequirements(
    schedules: readonly CorpusSchedule[] = CONVERGENCE_CORPUS,
): ContinuationSlot[] {
    return schedules.flatMap((schedule) =>
        schedule.kinds.slice(0, schedule.participants).flatMap((actorKind, actor) => {
            const proofs: ContinuationProof[] = ['typing', 'structure', 'history', 'partition'];
            if (actorKind !== NATIVE_PEER_KIND) proofs.push('unrelated-web', 'web-gap');
            return proofs.map((proof) => ({
                key: `${schedule.name} :: ${actor} :: ${proof}`,
                schedule,
                topology: schedule.topology,
                preset: schedule.preset,
                baseFamily: schedule.scenario.name,
                actor,
                actorKind,
                proof,
                required: proof === 'web-gap' ? null : true,
                status: 'unexercised' as const,
            }));
        }),
    );
}

export interface ScheduleOutcome {
    readonly peers: readonly Peer[];
    readonly geometry: SettledGeometry;
    readonly nativeAutonomousRepairWrites: number;
    readonly rawConvergence: { passed: boolean; failure?: string };
    readonly evidence: { status: CoverageStatus; actions: RecordedAction[]; failures: string[] };
}

export interface ScenarioCoverage {
    readonly schedule: string;
    readonly topology: ConvergenceTopology;
    readonly preset: SchemaPreset;
    readonly baseFamily: string;
    readonly actor: number;
    readonly actorKind: PeerKind;
    readonly proof: string;
    readonly required: true;
    readonly status: CoverageStatus;
}

export function scenarioCoverage(
    schedule: CorpusSchedule,
    outcome?: ScheduleOutcome,
): ScenarioCoverage[] {
    const intent = FAMILY_INTENTS[CORPUS_SCENARIOS.indexOf(schedule.scenario)];
    if (!intent) throw scenarioEvidenceFailure('unknown family coverage');
    const actors = Array.from(
        { length: schedule.participants },
        (_, index) => (index + schedule.actorOffset) % schedule.participants,
    );
    const declared = declaredFamilyActors(intent, actors, [...schedule.kinds]);
    return intent.steps.map((step, index) => ({
        schedule: schedule.name,
        topology: schedule.topology,
        preset: schedule.preset,
        baseFamily: schedule.scenario.name,
        actor: declared[index]!,
        actorKind: schedule.kinds[declared[index]!]!,
        proof: `scenario-effect:${index}:${step.operation}`,
        required: true,
        status: outcome?.evidence.status ?? 'unexercised',
    }));
}

async function projectedGeometry(
    judge: Peer,
    preset: SchemaPreset,
    tableJson: unknown,
): Promise<SettledGeometry> {
    if (peerKindOf(judge) !== NATIVE_PEER_KIND) {
        throw new Error(
            `TBL-11 admissibility is the Rust projectTable result; a ${peerKindOf(judge)} peer `
                + 'cannot judge a settled table',
        );
    }
    try {
        const projection = await call(judge, 'projectTable', {
            schema: tableSchemaForPreset(preset),
            table: tableJson,
        });
        return { kind: GEOMETRY_ADMITTED, irregular: projection['irregular'] === true };
    } catch (error) {
        if (error instanceof PeerError) {
            return { kind: GEOMETRY_PROJECTION_FAILED, code: error.code, message: error.message };
        }
        throw error;
    }
}

function structuralTables(node: unknown, found: Record<string, unknown>[]): void {
    if (Array.isArray(node)) {
        for (const entry of node) {
            structuralTables(entry, found);
        }
        return;
    }
    if (typeof node !== 'object' || node === null) {
        return;
    }
    const record = node as Record<string, unknown>;
    if (record['type'] === TABLE_NODE) {
        found.push(record);
    }
    structuralTables(record['content'], found);
}

export function nestedTablesOf(tableJson: unknown): Record<string, unknown>[] {
    const found: Record<string, unknown>[] = [];
    structuralTables(tableJson, found);
    return found;
}

async function projectedNestedGeometry(
    judge: Peer,
    preset: SchemaPreset,
    tableJson: unknown,
): Promise<SettledGeometry> {
    let irregular = false;
    for (const table of nestedTablesOf(tableJson)) {
        const projected = await projectedGeometry(judge, preset, table);
        if (projected.kind !== GEOMETRY_ADMITTED) {
            return projected;
        }
        irregular = irregular || projected.irregular;
    }
    return { kind: GEOMETRY_ADMITTED, irregular };
}

export async function settledGeometryOf(
    peers: readonly Peer[],
    judge: Peer,
    preset: SchemaPreset,
): Promise<SettledGeometry> {
    const tables: string[] = [];
    for (const peer of peers) {
        tables.push(JSON.stringify(tableOf((await snapshot(peer)).documentJson)));
    }
    const [first] = tables;
    if (first === undefined) {
        throw new Error('a settled run compared an empty peer set');
    }
    for (const [index, candidate] of tables.entries()) {
        if (candidate !== first) {
            return {
                kind: GEOMETRY_RAW_JSON_DISAGREEMENT,
                detail: `peer ${index} holds ${candidate}; peer 0 holds ${first}`,
            };
        }
    }
    return projectedNestedGeometry(judge, preset, JSON.parse(first));
}

export async function nativeRepairWrites(peers: readonly Peer[]): Promise<number> {
    let total = NO_REPAIR_WRITES;
    for (const peer of peers) {
        if (peerKindOf(peer) !== NATIVE_PEER_KIND) {
            continue;
        }
        total += (await snapshot(peer)).autonomousRepairWrites;
    }
    return total;
}

export async function webRepairWrites(peers: readonly Peer[]): Promise<number> {
    let total = NO_LOOPS;
    for (const peer of peers) {
        if (peerKindOf(peer) === NATIVE_PEER_KIND) {
            continue;
        }
        total += (await snapshot(peer)).autonomousRepairWrites;
    }
    return total;
}

async function authorTable(peer: Peer, table: Record<string, unknown>): Promise<void> {
    if (peerKindOf(peer) === NATIVE_PEER_KIND) {
        await call(peer, 'command', {
            type: 'insertContentJson',
            json: { type: DOC_NODE, content: [table] },
        });
        return;
    }
    await call(peer, 'command', { type: 'insertNode', node: table });
}

export interface SettledSetup {
    readonly participants: readonly Peer[];
    readonly judge: Peer;
    readonly reference?: Peer;
    readonly baseline: ScheduleOutcome;
}

export async function runSchedule(
    schedule: CorpusSchedule,
    settled?: (setup: SettledSetup) => Promise<void>,
    options: { readonly webReference?: boolean } = {},
): Promise<ScheduleOutcome> {
    let outcome: ScheduleOutcome | null = null;
    let started: readonly Peer[] = [];
    let nativeRepairs = NO_REPAIR_WRITES;
    let capturedActions: RecordedAction[] = [];
    let rawConvergence: { passed: boolean; failure?: string } = {
        passed: false,
        failure: 'raw convergence was not reached',
    };
    try {
        await withPeers(
            [...schedule.kinds, ...(options.webReference ? [schedule.preset] : [])],
            async (peers) => {
                started = peers;
                const participants = peers.slice(0, schedule.participants);
                const judge = peerAt(peers, schedule.kinds.length - 1);
                const author = peerAt(participants, 0);
                const fixture = (schedule.scenario.table ?? corpusTable)(schedule.preset);
                await authorTable(author, fixture);
                await seedFrom(author, participants.slice(1));
                await exchangeUntilIdle([...participants], schedule.seed);
                const seeded = JSON.stringify(tableOf((await snapshot(author)).documentJson));
                const capture = startEvidence(participants);
                capturedActions = capture.actions;
                const evidenceFailures: string[] = [];
                try {
                    await schedule.scenario.act({
                        peers: rotate(participants, schedule.actorOffset),
                        author,
                        anchors: cellAnchors(fixture, TABLE_START),
                        liveAnchors: async () =>
                            cellAnchors(
                                tableOf((await snapshot(author)).documentJson) as Record<
                                    string,
                                    unknown
                                >,
                                TABLE_START,
                            ),
                        fixture,
                        liveTable: async () =>
                            tableOf((await snapshot(author)).documentJson) as Record<
                                string,
                                unknown
                            >,
                        liveRowAnchors: async () =>
                            rowGroupedAnchors(
                                tableOf((await snapshot(author)).documentJson) as Record<
                                    string,
                                    unknown
                                >,
                            ),
                        preset: schedule.preset,
                        seed: schedule.seed,
                    });
                } catch (error) {
                    evidenceFailures.push(String(error));
                } finally {
                    stopEvidence(participants);
                }
                await exchangeUntilIdle([...participants], schedule.seed);

                const settledJson = tableOf((await snapshot(author)).documentJson);
                try {
                    if (!schedule.scenario.proves)
                        throw scenarioEvidenceFailure('missing predicate');
                    schedule.scenario.proves({
                        settledTable: settledJson as Record<string, unknown>,
                        seededTable: JSON.parse(seeded) as Record<string, unknown>,
                        fixtureTable: fixture,
                        boundaries: {
                            ...capture,
                            settled: await observeEvidence(author),
                            author: 0,
                            actors: rotate(participants, schedule.actorOffset).map((peer) =>
                                participants.indexOf(peer),
                            ),
                            kinds: participants.map(peerKindOf),
                        },
                    });
                } catch (error) {
                    evidenceFailures.push(String(error));
                }
                nativeRepairs = await nativeRepairWrites(participants);
                if (nativeRepairs !== 0)
                    evidenceFailures.push(`TBL10 AUTONOMOUS_REPAIR: ${nativeRepairs}`);
                rawConvergence = { passed: true };
                try {
                    await assertConverged([...participants]);
                } catch (error) {
                    rawConvergence = { passed: false, failure: String(error) };
                }
                outcome = {
                    peers: participants,
                    geometry: await settledGeometryOf(participants, judge, schedule.preset),
                    nativeAutonomousRepairWrites: nativeRepairs,
                    rawConvergence,
                    evidence: {
                        status: evidenceFailures.length ? 'exercised-unproven' : 'proven',
                        actions: capture.actions,
                        failures: evidenceFailures,
                    },
                };
                if (settled)
                    await settled({
                        participants,
                        judge,
                        baseline: outcome,
                        ...(options.webReference
                            ? { reference: peerAt(peers, peers.length - 1) }
                            : {}),
                    });
            },
            { ...tableFixture(schedule.preset), [PARTICIPANT_COUNT_KEY]: schedule.participants },
            schedule.seed,
        );
    } catch (error) {
        if (!(error instanceof Error)) {
            throw error;
        }
        return {
            peers: started.slice(0, schedule.participants),
            geometry: {
                kind: GEOMETRY_ORACLE_FAILED,
                failureClass: failureClassOf(error),
                message: error.message,
            },
            nativeAutonomousRepairWrites: nativeRepairs,
            rawConvergence,
            evidence: {
                status: capturedActions.length ? 'exercised-unproven' : 'unexercised',
                actions: capturedActions,
                failures: [String(error)],
            },
        };
    }
    if (outcome === null) {
        throw new Error(`the schedule ${schedule.name} produced no outcome`);
    }
    return outcome;
}

export interface ContinuationCheckpoint {
    textHistoryObservations?: TextHistoryObservation[];
    readonly boundary: string;
    readonly raw: { passed: boolean; failure?: string };
    readonly presentation: {
        passed: boolean;
        comparisons: PresentationCheck[];
        failures: string[];
        views?: { native: EffectiveDocument[]; web: EffectiveDocument[] };
    };
    readonly nativeAutonomousRepairWrites: number;
    readonly observations: ContinuationObservation[];
    readonly observationFailures: string[];
    readonly drain: {
        passed: boolean;
        failure?: string;
        rounds: number;
        emitted: number;
    };
    readonly remoteBoundaries: ContinuationRemoteBoundary[];
}

export interface ContinuationRemoteBoundary {
    actor: number;
    kind: PeerKind;
    bytes: string;
    passes: number;
    autonomous: number;
    pending: boolean;
}

export interface ContinuationResult {
    availability?: UnavailableOutcome;
    textHistory?: TextHistoryIntent;
    readonly slot: ContinuationSlot;
    required: boolean | null;
    status: CoverageStatus;
    disposition: 'edited' | 'refused' | 'limited' | 'no-gap' | 'unreached';
    baseline?: Omit<ScheduleOutcome, 'peers'>;
    checkpoints: ContinuationCheckpoint[];
    actions: RecordedAction[];
    failures: string[];
    dependencies: {
        prerequisite: string;
        dependent: string;
        recipient: number;
        pending: boolean;
    }[];
    gap?: { observed: true; positions: number[] };
    partition?: {
        rounds: number;
        emitted: number;
        remoteBoundaries: ContinuationRemoteBoundary[];
    };
    tracePath?: string;
    refusalEmitted?: number;
}

export class ContinuationExecutionError extends Error {
    constructor(
        readonly result: ContinuationResult,
        cause: unknown,
    ) {
        super(String(cause), { cause });
    }
}

function continuationScheduler(
    peers: readonly Peer[],
    seed: number,
    boundaries: ContinuationRemoteBoundary[],
) {
    return createScheduler([...peers], seed, null, async (peer, bytes) => {
        const state = await snapshot(peer);
        boundaries.push({
            actor: peers.indexOf(peer),
            kind: peerKindOf(peer),
            bytes,
            passes: state.normalizationPassesAfterLastAction,
            autonomous: state.autonomousRepairWrites,
            pending: state.pendingDependencies,
        });
    });
}

export async function continuationCheckpoint(
    setup: SettledSetup,
    boundary: string,
    seed: number,
    drain = true,
    captureRaw = false,
): Promise<ContinuationCheckpoint> {
    const participants = [...setup.participants];
    const checked: ContinuationCheckpoint = {
        boundary,
        raw: { passed: false },
        presentation: { passed: false, comparisons: [], failures: [] },
        nativeAutonomousRepairWrites: 0,
        observations: [],
        observationFailures: [],
        drain: { passed: true, rounds: 0, emitted: 0 },
        remoteBoundaries: [],
    };
    if (drain) {
        const scheduler = continuationScheduler(participants, seed, checked.remoteBoundaries);
        try {
            await scheduler.drain();
        } catch (error) {
            checked.drain.passed = false;
            checked.drain.failure = String(error);
        }
        Object.assign(checked.drain, scheduler.drainStats());
    }
    try {
        await assertConverged(participants);
        checked.raw.passed = true;
    } catch (error) {
        checked.raw.failure = String(error);
    }
    const autonomous = await nativeRepairWrites(participants);
    for (const peer of participants) {
        try {
            const observed = {
                kind: peerKindOf(peer),
                document: await observeEvidence(peer),
            };
            checked.observations.push(observed);
            if (captureRaw) {
                checked.textHistoryObservations ??= [];
                checked.textHistoryObservations.push({
                    ...observed,
                    raw: (await snapshot(peer)).documentJson as JsonNode,
                });
            }
        } catch (error) {
            checked.observationFailures.push(String(error));
        }
    }
    try {
        const native = participants.filter((peer) => peerKindOf(peer) === NATIVE_PEER_KIND);
        const web = participants.filter((peer) => peerKindOf(peer) !== NATIVE_PEER_KIND);
        if (native.length === 0) {
            await seedFrom(participants[0]!, [setup.judge]);
            native.push(setup.judge);
        }
        if (setup.reference) {
            await seedFrom(participants[0]!, [setup.reference]);
            web.push(setup.reference);
        }
        const nativeViews: EffectiveDocument[] = [];
        for (const peer of native) nativeViews.push(await observeNativePresentation(peer));
        if (captureRaw) checked.presentation.views = { native: nativeViews, web: [] };
        for (const view of nativeViews.slice(1))
            checked.presentation.comparisons.push(
                assertEffectivePresentation(nativeViews[0]!, view),
            );
        for (const peer of web) {
            const view = await observeWebPresentation(peer);
            checked.presentation.views?.web.push(view);
            for (const expected of nativeViews)
                checked.presentation.comparisons.push(assertEffectivePresentation(expected, view));
        }
        requireContinuity(
            web.length > 0 || nativeViews.length > 1,
            'independent presentation observation',
        );
        const fallback = checked.presentation.comparisons
            .flatMap((check) => check.tables)
            .filter((table) => table.kind === 'overlap-fallback');
        for (const table of fallback)
            requireContinuity(
                checked.presentation.comparisons.some((check) =>
                    check.tables.some(
                        (other) =>
                            other.source === table.source &&
                            other.kind === 'overlap-fallback' &&
                            other.evidence === 'live-overlap',
                    ),
                ),
                'fresh live overlap evidence',
            );
        checked.presentation.passed = true;
    } catch (error) {
        checked.presentation.failures.push(String(error));
    }
    return { ...checked, nativeAutonomousRepairWrites: autonomous };
}

export function continuationPassed(result: ContinuationResult): boolean {
    if (result.failures.length > 0) return false;
    if (result.availability || result.disposition === 'limited')
        return result.slot.proof === 'structure' && result.status === 'proven' && availabilityVerified(result);
    if (result.slot.proof === 'text-history') {
        try {
            requireContinuity(
                result.slot.companionOf &&
                    result.slot.key === `${result.slot.companionOf} :: text-history`,
                'text-history companion key',
            );
            requireContinuity(
                result.required === true &&
                    result.disposition === 'edited' &&
                    result.actions.length === 3 &&
                    result.textHistory,
                'text-history complete evidence',
            );
            requireContinuity(
                result.textHistory.actor === result.slot.actor &&
                    result.textHistory.kind === result.slot.actorKind,
                'text-history intended actor',
            );
            requireContinuity(
                result.checkpoints.map((checkpoint) => checkpoint.boundary).join(',') ===
                    'baseline,typed,undo,redo',
                'text-history checkpoints',
            );
            for (const checkpoint of result.checkpoints)
                requireContinuity(
                    checkpoint.textHistoryObservations?.length ===
                        result.slot.schedule.participants &&
                        checkpoint.textHistoryObservations.every(
                            (view, index) => view.kind === result.slot.schedule.kinds[index],
                        ),
                    'text-history participant observations',
                );
            for (const observed of result.checkpoints[0]!.textHistoryObservations!)
                assertTextHistoryState(result.textHistory, observed, false);
            const initial = result.checkpoints[0]!.textHistoryObservations![result.slot.actor];
            requireContinuity(
                initial &&
                    initial.kind === result.slot.actorKind &&
                    result.slot.preset === result.slot.schedule.preset &&
                    result.slot.topology === result.slot.schedule.topology,
                'text-history declared actor/setup',
            );
            const target = declaredTextHistoryTarget(result.slot, initial.document);
            requireContinuity(
                target.sourceId === result.textHistory.sourceId,
                'text-history declared target',
            );
            assert.deepEqual(
                result.textHistory.before,
                initial.document,
                'text-history actor baseline',
            );
            assert.deepEqual(
                result.textHistory.rawBefore,
                initial.raw,
                'text-history actor raw baseline',
            );
            result.actions.forEach((action, index) =>
                assertTextHistoryBoundary(
                    action,
                    result.textHistory!,
                    index,
                    result.checkpoints[index + 1]!.textHistoryObservations ?? [],
                ),
            );
        } catch {
            return false;
        }
    }
    const minimumActions: Record<ContinuationProof, number> = {
        typing: 1,
        structure: 1,
        history: 3,
        'text-history': 3,
        partition: 2,
        'unrelated-web': 1,
        'web-gap': 1,
    };
    if (
        result.required === false &&
        (result.slot.proof !== 'web-gap' ||
            result.gap?.positions.length !== 0 ||
            result.disposition !== 'no-gap')
    )
        return false;
    if (result.required === true && result.actions.length < minimumActions[result.slot.proof])
        return false;
    if (
        result.slot.proof === 'web-gap' &&
        (!result.gap || result.required !== result.gap.positions.length > 0)
    )
        return false;
    if (
        result.disposition === 'refused' &&
        (result.slot.proof !== 'web-gap' || result.refusalEmitted !== 0)
    )
        return false;
    const safeRemote = (boundary: ContinuationRemoteBoundary) =>
        boundary.kind !== NATIVE_PEER_KIND || (boundary.passes === 0 && boundary.autonomous === 0);
    if (
        result.slot.proof === 'partition' &&
        result.required &&
        (!result.dependencies.some((entry) => entry.pending) ||
            !result.partition ||
            result.partition.rounds > 100 ||
            result.partition.emitted > 10000 ||
            !result.partition.remoteBoundaries.every(safeRemote))
    )
        return false;
    return (
        result.required !== null &&
        (result.required === false || result.status === 'proven') &&
        result.baseline?.evidence.status === 'proven' &&
        result.baseline.rawConvergence.passed &&
        result.checkpoints.length >= (result.required === false ? 1 : 2) &&
        result.checkpoints.every(
            (checkpoint) =>
                checkpoint.raw.passed &&
                checkpoint.presentation.passed &&
                checkpoint.drain.passed &&
                checkpoint.nativeAutonomousRepairWrites === 0 &&
                checkpoint.drain.rounds <= 100 &&
                checkpoint.drain.emitted <= 10000 &&
                checkpoint.remoteBoundaries.every(safeRemote) &&
                checkpoint.observationFailures.length === 0,
        )
    );
}

export type StructuralAvailabilityOutcome = 'successful-edit' | UnavailableOutcome | 'unexplained-failure' | 'missing-evidence';

export function structuralAvailabilityOutcome(result: ContinuationResult): StructuralAvailabilityOutcome {
    requireContinuity(['structure', 'history'].includes(result.slot.proof), 'structural availability outcome scope');
    if (result.failures.length) return 'unexplained-failure';
    if (availabilityVerified(result)) return result.availability!;
    if (!result.actions.length || !result.checkpoints.length) return 'missing-evidence';
    if (result.disposition === 'edited' && continuationPassed(result)) {
        try {
            assertSuccessfulStructuralResult(result);
            return 'successful-edit';
        } catch { return 'unexplained-failure'; }
    }
    return result.checkpoints.some(checkpoint => !checkpoint.raw.passed || !checkpoint.presentation.passed || !checkpoint.drain.passed || checkpoint.nativeAutonomousRepairWrites > 0) ? 'unexplained-failure' : 'missing-evidence';
}

export function continuationCoverage(
    required: readonly ContinuationSlot[],
    results: readonly ContinuationResult[],
): ContinuationSlot[] {
    const found = new Map(results.map((result) => [result.slot.key, result]));
    requireContinuity(found.size === results.length, 'duplicate continuation result');
    const keys = new Set(required.map((slot) => slot.key));
    requireContinuity(keys.size === required.length, 'duplicate continuation declaration');
    requireContinuity(
        results.every((result) => keys.has(result.slot.key)),
        'unknown continuation result',
    );
    return required.map((slot) => {
        const result = found.get(slot.key);
        if (result && (slot.proof === 'text-history' || result.availability)) {
            assert.deepEqual(
                continuationDeclaration(result.slot),
                continuationDeclaration(slot),
                slot.proof === 'text-history' ? 'text-history declaration mismatch' : 'availability declaration mismatch',
            );
        }
        return {
            ...slot,
            required: result ? result.required : slot.required,
            status:
                result?.status === 'proven' &&
                (slot.proof === 'text-history' || result.availability) &&
                !continuationPassed(result)
                    ? 'exercised-unproven'
                    : result?.status ?? 'unexercised',
        };
    });
}

export function continuationDeclaration({ status: _status, ...fields }: ContinuationSlot): unknown {
    return JSON.parse(JSON.stringify(fields));
}

export interface ContinuationOptions {
    setup?: (
        slot: ContinuationSlot,
        body: (setup: SettledSetup) => Promise<void>,
    ) => Promise<ScheduleOutcome>;
    target?: (view: EffectiveDocument, slot: ContinuationSlot) => EffectiveCell;
    gaps?: (
        view: EffectiveDocument,
        slot: ContinuationSlot,
    ) => { table: EffectiveTable; cell: EffectiveCell }[];
    structuralEvidence?: (
        action: RecordedAction,
        intent: { actor: number; source: string },
        settled: readonly ContinuationObservation[],
    ) => void;
}

export async function runContinuation(
    slot: ContinuationSlot,
    options: ContinuationOptions = {},
): Promise<ContinuationResult> {
    const result: ContinuationResult = {
        slot,
        required: slot.required,
        status: 'unexercised',
        disposition: 'unreached',
        checkpoints: [],
        actions: [],
        failures: [],
        dependencies: [],
    };
    const schedule = slot.schedule;
    const setupRunner = options.setup ?? ((candidate, body) =>
        runSchedule(candidate.schedule, body, {
            webReference: candidate.topology === TOPOLOGY_NATIVE_NATIVE,
        }));
    const structuralEvidence = options.structuralEvidence ?? assertStructuralContinuation;
    const baseline = await setupRunner(
        slot,
        async (setup) => {
            const { peers: _closedLater, ...baseline } = setup.baseline;
            result.baseline = baseline;
            const actor = peerAt(setup.participants, slot.actor);
            const checkpoint = async (boundary: string, drain = true) => {
                const checked = await continuationCheckpoint(
                    setup,
                    boundary,
                    schedule.seed,
                    drain,
                    ['text-history', 'structure', 'history'].includes(slot.proof),
                );
                result.checkpoints.push(checked);
                return checked;
            };
            const initial = await checkpoint('baseline', false);
            if (!initial.raw.passed) {
                result.failures.push('TBL21 CONTINUITY setup raw convergence');
                return;
            }
            const capture = startEvidence(setup.participants);
            result.actions = capture.actions;
            try {
                const sourceView = await observeEvidence(actor);
                if (slot.proof === 'web-gap') {
                    const gaps = options.gaps?.(sourceView, slot) ??
                        sourceView.tables.flatMap((table) =>
                            table.cells
                                .filter((cell) => cell.source === null)
                                .map((cell) => ({ table, cell })),
                        );
                    result.gap = {
                        observed: true,
                        positions: gaps.map(({ cell }) => cell.position),
                    };
                    result.required = gaps.length > 0;
                    if (!gaps.length) {
                        result.status = 'proven';
                        result.disposition = 'no-gap';
                        return;
                    }
                    const { table, cell } = gaps[0]!;
                    await call(actor, 'command', {
                        type: 'insertText',
                        text: 'gap-',
                        at: typingCursor(cell, slot.actorKind),
                    });
                    const action = capture.actions.at(-1)!;
                    if (action.reply['documentChanged'] === false) {
                        const emitted = await flushDocumentEvents(actor);
                        result.refusalEmitted = emitted.length;
                        if (emitted.length) {
                            const scheduler = continuationScheduler(
                                setup.participants,
                                schedule.seed,
                                [],
                            );
                            for (const event of emitted) scheduler.enqueue(slot.actor, event);
                            await scheduler.drain();
                        }
                    }
                    const final = await checkpoint('gap-edit');
                    if (action.reply['documentChanged'] === false) {
                        assertGapRefusal(action, result.refusalEmitted!);
                        result.disposition = 'refused';
                    } else {
                        assertGapContinuation(
                            action,
                            {
                                actor: slot.actor,
                                tableSource: table.source,
                                position: cell.position,
                                text: 'gap-',
                            },
                            final.observations,
                        );
                        result.disposition = 'edited';
                    }
                    result.status = 'proven';
                    return;
                }
                if (slot.proof === 'unrelated-web') {
                    let state = await snapshot(actor);
                    let display = state.displayJson as JsonNode;
                    let index =
                        display.content?.findIndex((node) => node.type === 'paragraph') ?? -1;
                    if (index < 0) {
                        await call(actor, 'command', {
                            type: 'appendParagraph',
                        });
                        const insertion = capture.actions.at(-1)!;
                        requireContinuity(
                            insertion.reply['documentChanged'] === true,
                            'outside paragraph insertion applied',
                        );
                        const expected = structuredClone(insertion.rawBefore) as JsonNode;
                        expected.content = [...(expected.content ?? []), { type: 'paragraph' }];
                        requireContinuity(
                            JSON.stringify(outsideShape(insertion.rawAfter as JsonNode)) ===
                                JSON.stringify(outsideShape(expected)),
                            'outside paragraph is a top-level sibling',
                        );
                        assertSourcePreservation(
                            insertion.before,
                            insertion.after,
                            new Map(),
                            true,
                            { before: insertion.kind, after: insertion.kind },
                        );
                        const inserted = await checkpoint('outside-paragraph');
                        for (const view of inserted.observations)
                            assertSourcePreservation(
                                insertion.before,
                                view.document,
                                new Map(),
                                true,
                                { before: insertion.kind, after: view.kind },
                            );
                        state = await snapshot(actor);
                        display = state.displayJson as JsonNode;
                        index = (display.content?.length ?? 0) - 1;
                    }
                    requireContinuity(
                        display.content?.[index]?.type === 'paragraph',
                        'actual outside paragraph',
                    );
                    const at =
                        1 +
                        display
                            .content!.slice(0, index)
                            .reduce(
                                (position, node) => position + nodeSize(node, slot.actorKind),
                                0,
                            );
                    await call(actor, 'command', {
                        type: 'insertText',
                        text: 'outside-',
                        at,
                    });
                    const action = capture.actions.at(-1)!;
                    const final = await checkpoint('outside-typed');
                    const raw: JsonNode[] = [];
                    for (const peer of setup.participants)
                        raw.push((await snapshot(peer)).documentJson as JsonNode);
                    assertUnrelatedContinuation(
                        action,
                        { actor: slot.actor, index, text: 'outside-' },
                        raw,
                        final.observations,
                    );
                    result.status = 'proven';
                    result.disposition = 'edited';
                    return;
                }
                const target =
                    slot.proof === 'text-history'
                        ? declaredTextHistoryTarget(
                              slot,
                              initial.textHistoryObservations![slot.actor]!.document,
                          )
                        : options.target?.(sourceView, slot) ??
                          continuationTarget(sourceView, slot.actorKind, slot.proof);
                requireContinuity(target?.sourceId, 'real source target unavailable');
                const sourceId = target.sourceId;
                const freshTarget = async () => {
                    const view = await observeEvidence(actor);
                    const cell = realCells(view).find((cell) => cell.sourceId === sourceId);
                    requireContinuity(cell, 'fresh source target unavailable');
                    return cell;
                };
                const type = async (text: string) => {
                    const cell = await freshTarget();
                    await call(actor, 'command', {
                        type: 'insertText',
                        text,
                        at: typingCursor(cell, slot.actorKind),
                    });
                    const action = capture.actions.at(-1)!;
                    assertTypingContinuation(action, { actor: slot.actor, sourceId, text }, [
                        { kind: action.kind, document: action.after },
                    ]);
                    return action;
                };
                if (slot.proof === 'text-history') {
                    const intent = textHistoryIntent(
                        sourceView,
                        (await snapshot(actor)).documentJson as JsonNode,
                        {
                            actor: slot.actor,
                            kind: slot.actorKind,
                            sourceId,
                            text: `text-history-${schedule.seed}-${slot.actor}-`,
                        },
                    );
                    result.textHistory = intent;
                    if (slot.actorKind !== NATIVE_PEER_KIND)
                        await new Promise((resolve) => setTimeout(resolve, 510));
                    for (const [index, boundary] of ['typed', 'undo', 'redo'].entries()) {
                        if (index === 0) await type(intent.text);
                        else await call(actor, index === 1 ? 'undo' : 'redo', {});
                        const action = capture.actions.at(-1)!;
                        assertTextHistoryBoundary(action, intent, index, [
                            {
                                kind: action.kind,
                                document: action.after,
                                raw: action.rawAfter as JsonNode,
                            },
                        ]);
                        const settled = await checkpoint(boundary);
                        assertTextHistoryBoundary(
                            action,
                            intent,
                            index,
                            settled.textHistoryObservations ?? [],
                        );
                    }
                } else if (slot.proof === 'typing') {
                    const action = await type('continuation-');
                    const final = await checkpoint('typed');
                    assertTypingContinuation(
                        action,
                        { actor: slot.actor, sourceId, text: 'continuation-' },
                        final.observations,
                    );
                } else if (slot.proof === 'structure' || slot.proof === 'history') {
                    // Stock Yjs history uses a 500ms capture interval; expire it without changing its configuration.
                    if (slot.proof === 'history' && slot.actorKind !== NATIVE_PEER_KIND)
                        await new Promise((resolve) => setTimeout(resolve, 510));
                    const cell = await freshTarget();
                    let commandFailure: unknown;
                    try {
                        await addRowAfter(actor, cell.position + 1);
                    } catch (error) {
                        commandFailure = error;
                    }
                    const action = capture.actions.at(-1)!;
                    if (commandFailure !== undefined || action?.reply['type'] === 'notApplicable') {
                        await checkpoint('unavailable-action');
                        try {
                            result.availability = assertUnavailableAction(action, slot, initial.textHistoryObservations![slot.actor]!);
                            result.disposition = result.availability === 'verified-native-refusal' ? 'refused' : 'limited';
                            requireContinuity(availabilityVerified(result), 'availability settled result');
                        } catch (evidenceError) {
                            if (commandFailure !== undefined) result.failures.push(String(commandFailure));
                            throw evidenceError;
                        }
                        result.status = slot.proof === 'structure' ? 'proven' : 'exercised-unproven';
                        return;
                    }
                    const intent = { actor: slot.actor, source: cell.source! };
                    structuralEvidence(action, intent, [
                        { kind: action.kind, document: action.after },
                    ]);
                    const acted = await checkpoint('structural-action');
                    structuralEvidence(action, intent, acted.observations);
                    if (slot.proof === 'history') {
                        await call(actor, 'undo', {});
                        const undone = capture.actions.at(-1)!;
                        const undoState = await checkpoint('undo');
                        for (const view of [
                            { kind: undone.kind, document: undone.after },
                            ...undoState.observations,
                        ])
                            assertSourcePreservation(
                                action.before,
                                view.document,
                                new Map(),
                                true,
                                { before: action.kind, after: view.kind },
                            );
                        await call(actor, 'redo', {});
                        const redone = capture.actions.at(-1)!;
                        const final = await checkpoint('redo');
                        assertContinuationHistory(capture.actions, slot.actor);
                        for (const view of [
                            { kind: redone.kind, document: redone.after },
                            ...final.observations,
                        ]) {
                            const coordinates = {
                                before: action.kind,
                                after: view.kind,
                            };
                            assertSourcePreservation(
                                action.before,
                                view.document,
                                new Map(),
                                true,
                                coordinates,
                            );
                            assertSourcePreservation(
                                redone.after,
                                view.document,
                                new Map(),
                                true,
                                coordinates,
                            );
                        }
                    }
                } else if (slot.proof === 'partition') {
                    const boundaries: ContinuationRemoteBoundary[] = [];
                    const scheduler = continuationScheduler(
                        setup.participants,
                        schedule.seed,
                        boundaries,
                    );
                    for (let other = 0; other < setup.participants.length; other += 1)
                        if (other !== slot.actor) scheduler.partition(slot.actor, other, true);
                    await type('first-');
                    let emitted = (await scheduler.collect()).emitted;
                    const prerequisite = [...scheduler.pending()];
                    requireContinuity(prerequisite.length > 0, 'partition prerequisite update');
                    const second = await type('second-');
                    emitted += (await scheduler.collect()).emitted;
                    const dependent = scheduler
                        .pending()
                        .filter((message) => !prerequisite.includes(message));
                    requireContinuity(dependent.length > 0, 'partition dependent update');
                    for (const message of dependent) {
                        requireContinuity(
                            scheduler.isPartitioned(slot.actor, message.recipient),
                            'partition retained',
                        );
                        scheduler.partition(slot.actor, message.recipient, false);
                        await scheduler.deliver(message);
                        const state = await snapshot(peerAt(setup.participants, message.recipient));
                        const first = prerequisite.find(
                            (queued) => queued.recipient === message.recipient,
                        );
                        requireContinuity(first, 'withheld prerequisite recipient');
                        result.dependencies.push({
                            prerequisite: first.event.bytesBase64,
                            dependent: message.event.bytesBase64,
                            recipient: message.recipient,
                            pending: state.pendingDependencies,
                        });
                        if (
                            peerKindOf(peerAt(setup.participants, message.recipient)) ===
                            NATIVE_PEER_KIND
                        )
                            requireContinuity(
                                state.normalizationPassesAfterLastAction === 0 &&
                                    state.autonomousRepairWrites === 0,
                                'remote boundary normalization',
                            );
                    }
                    for (const message of prerequisite) {
                        scheduler.partition(slot.actor, message.recipient, false);
                        await scheduler.deliver(message);
                    }
                    await scheduler.drain();
                    const stats = scheduler.drainStats();
                    result.partition = {
                        ...stats,
                        emitted: stats.emitted + emitted,
                        remoteBoundaries: boundaries,
                    };
                    const final = await checkpoint('reconnected');
                    requireContinuity(
                        result.dependencies.some((entry) => entry.pending),
                        'withheld dependency was exercised',
                    );
                    assertTypingContinuation(
                        second,
                        { actor: slot.actor, sourceId, text: 'second-' },
                        final.observations,
                    );
                } else throw new Error(`TBL21 CONTINUITY unimplemented ${slot.proof}`);
                result.status = 'proven';
                result.disposition = 'edited';
            } catch (error) {
                result.failures.push(String(error));
                result.status = capture.actions.length ? 'exercised-unproven' : 'unexercised';
                await checkpoint('failure-drain');
            } finally {
                stopEvidence(setup.participants);
            }
        },
    ).catch((error) => {
        throw new ContinuationExecutionError(result, error);
    });
    if (!result.baseline) {
        const { peers: _closed, ...failed } = baseline;
        result.baseline = failed;
        result.failures.push(...failed.evidence.failures);
    }
    if (!continuationPassed(result) || result.availability)
        result.tracePath = persistTrace({
            ...lastTrace(),
            failureClass: 'CONTINUITY',
            failureMessage: JSON.stringify({
                key: slot.key,
                availability: result.availability,
                attempts: result.actions.filter(action => action.commandError || action.reply['availability']).map(action => ({ request: action.request, error: action.commandError, nativeAudit: action.reply['availability'], stockObservation: action.stockRowInsertion })),
                failures: result.failures,
                checkpoints: result.checkpoints.map(({ boundary, raw, presentation, drain }) => ({
                    boundary,
                    raw,
                    presentation,
                    drain,
                })),
            }),
        });
    return result;
}
