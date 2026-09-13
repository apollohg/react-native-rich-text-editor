import { assertConverged } from './assertions.js';
import {
    PeerError,
    call,
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

export const SCHEDULES_PER_TOPOLOGY = 100;
export const CORPUS_PRESETS: readonly SchemaPreset[] = ['prosemirror', 'tiptap'];
export const CORPUS_BASE_SEED = 0x7ab1_c0f5;

const DOC_NODE = 'doc';
const TEXT_NODE = 'text';
const TABLE_START = 0;
const SINGLE_SPAN = 1;
const NO_LOOPS = 0;
const TOP_LEFT_CELL = 0;
const TOP_RIGHT_CELL = 1;
const BOTTOM_LEFT_CELL = 2;
const TYPED_TEXT = 'typed';
const OUT_OF_ORDER_DELIVERY = 2;

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

type ScenarioContext = {
    readonly peers: readonly Peer[];
    readonly anchors: readonly number[];
    readonly preset: SchemaPreset;
    readonly seed: number;
};

export type CorpusScenario = {
    readonly name: string;
    readonly mutatesGeometry: boolean;
    readonly act: (context: ScenarioContext) => Promise<void>;
};

function peerAt(peers: readonly Peer[], index: number): Peer {
    const peer = peers[index];
    if (peer === undefined) {
        throw new Error(`the corpus scenario addressed peer ${index} outside the started set`);
    }
    return peer;
}

function anchorAt(anchors: readonly number[], index: number): number {
    const anchor = anchors[index];
    if (anchor === undefined) {
        throw new Error(`the corpus fixture exposes no cell anchor ${index}`);
    }
    return anchor;
}

export const CORPUS_SCENARIOS: readonly CorpusScenario[] = [
    {
        name: 'concurrent row and column insertion at the same boundary',
        mutatesGeometry: true,
        act: async ({ peers, anchors }) => {
            await addRowAfter(peerAt(peers, 0), anchorAt(anchors, TOP_LEFT_CELL));
            await addColumnAfter(peerAt(peers, 1), anchorAt(anchors, TOP_LEFT_CELL));
        },
    },
    {
        name: 'concurrent merges from opposite corners',
        mutatesGeometry: true,
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
        act: async ({ peers, anchors, seed }) => {
            const editor = peerAt(peers, 1);
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
        act: async ({ peers, anchors }) => {
            await typeInCell(peerAt(peers, 0), anchorAt(anchors, TOP_LEFT_CELL));
        },
    },
];

export interface CorpusSchedule {
    readonly name: string;
    readonly topology: ConvergenceTopology;
    readonly kinds: readonly PeerKind[];
    readonly participants: number;
    readonly preset: SchemaPreset;
    readonly scenario: CorpusScenario;
    readonly seed: number;
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
            const preset = CORPUS_PRESETS[index % CORPUS_PRESETS.length];
            const scenario = CORPUS_SCENARIOS[index % CORPUS_SCENARIOS.length];
            if (preset === undefined || scenario === undefined) {
                throw new Error('the corpus generator produced an incomplete schedule');
            }
            const { kinds, participants } = kindsFor(topology, preset);
            schedules.push({
                name: `${topology} ${preset} ${scenario.name} seed ${seed}`,
                topology,
                kinds,
                participants,
                preset,
                scenario,
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
    readonly webControlLoops: number;
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
    return projectedGeometry(judge, preset, JSON.parse(first));
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

async function authorTable(peer: Peer, preset: SchemaPreset): Promise<void> {
    const table = corpusTable(preset);
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
    try {
        await withPeers(
            schedule.kinds as PeerKind[],
            async (peers) => {
                started = peers;
                const participants = peers.slice(0, schedule.participants);
                const judge = peerAt(peers, peers.length - 1);
                const author = peerAt(participants, 0);
                await authorTable(author, schedule.preset);
                await seedFrom(author, participants.slice(1));
                await exchangeUntilIdle([...participants], schedule.seed);
                const seeded = JSON.stringify(tableOf((await snapshot(author)).documentJson));
                const repairsBefore = await webRepairWrites(participants);

                await schedule.scenario.act({
                    peers: participants,
                    anchors: cellAnchors(corpusTable(schedule.preset), TABLE_START),
                    preset: schedule.preset,
                    seed: schedule.seed,
                });
                await exchangeUntilIdle([...participants], schedule.seed);

                const settledTable = JSON.stringify(
                    tableOf((await snapshot(author)).documentJson),
                );
                if (schedule.scenario.mutatesGeometry && settledTable === seeded) {
                    throw new Error(
                        `the schedule ${schedule.name} left the seeded table unchanged, so it `
                            + 'proves nothing',
                    );
                }
                await assertConverged([...participants]);
                outcome = {
                    peers: participants,
                    geometry: await settledGeometryOf(participants, judge, schedule.preset),
                    webControlLoops: (await webRepairWrites(participants)) - repairsBefore,
                };
            },
            tableFixture(schedule.preset),
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
            webControlLoops: NO_LOOPS,
        };
    }
    if (outcome === null) {
        throw new Error(`the schedule ${schedule.name} produced no outcome`);
    }
    return outcome;
}
