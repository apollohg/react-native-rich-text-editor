import { assertConverged } from './assertions.js';
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
import type { Peer, PeerKind } from './peer-protocol.js';
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
import { failureClassOf } from './trace.js';
import { evidenceCall as call, observeEvidence, startEvidence, stopEvidence } from './evidence-observer.js';
import { assertFamilyEvidence, declaredFamilyActors, FAMILY_INTENTS } from './scenario-evidence.js';
import type { CoverageStatus, FamilyEvidence, RecordedAction } from './scenario-evidence.js';

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

export async function runSchedule(schedule: CorpusSchedule): Promise<ScheduleOutcome> {
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
            schedule.kinds as PeerKind[],
            async (peers) => {
                started = peers;
                const participants = peers.slice(0, schedule.participants);
                const judge = peerAt(peers, peers.length - 1);
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
