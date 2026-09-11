import { existsSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { assertReply, isRecord } from './peer-protocol.js';
import type { Peer, PeerKind, Request, UpdateEvent } from './peer-protocol.js';
import { startRustPeer } from './rust-peer.js';
import { startWebPeer } from './web-peer.js';

export interface PeerSnapshot {
    documentJson: Record<string, unknown> | null;
    displayJson: Record<string, unknown> | null;
    documentRevision: string;
    stateVectorBase64: string;
    projection: unknown;
    normalizationPassesAfterLastAction: number;
    autonomousRepairWrites: number;
}

export type SchemaPreset = 'prosemirror' | 'tiptap';

const RUST_PEER_EXECUTABLE = process.env['RUST_PEER_EXECUTABLE'] ?? fileURLToPath(
    new URL('../../rust/editor-core/target/debug/examples/table_interop_peer', import.meta.url),
);
const COLLABORATION_FRAGMENT_NAME = 'prosemirror';
const MAX_UPDATE_BYTES = 8 * 1024 * 1024;
const REQUEST_TIMEOUT_MILLIS = 30_000;
const EMPTY_STATE_VECTOR_BASE64 = Buffer.from([0]).toString('base64');
const MAX_EXCHANGE_ROUNDS = 100;
const MAX_EXCHANGE_UPDATES = 10_000;
const MUTATING_OPERATIONS = new Set(['command', 'undo', 'redo', 'applyUpdate']);

export class PeerError extends Error {
    readonly code: string;

    constructor(operation: string, code: string, message: string) {
        super(`peer rejected ${operation} with ${code}: ${message}`);
        this.name = 'PeerError';
        this.code = code;
    }
}

type PeerRecord = {
    kind: PeerKind;
    events: UpdateEvent[];
    autonomousRepairWrites: number;
    autonomousRepairWritesAfterLastAction: number;
};

const records = new WeakMap<Peer, PeerRecord>();
let requestCounter = 0;

function recordFor(peer: Peer): PeerRecord {
    const record = records.get(peer);
    if (record === undefined) {
        throw new Error('the controller does not own this peer; start it through withPeers');
    }
    return record;
}

function requireString(value: unknown, field: string): string {
    if (typeof value !== 'string') {
        throw new Error(`peer reply field ${field} was ${JSON.stringify(value)}`);
    }
    return value;
}

function requireCount(value: unknown, field: string): number {
    if (typeof value !== 'number' || !Number.isInteger(value) || value < 0) {
        throw new Error(`peer reply field ${field} was ${JSON.stringify(value)}`);
    }
    return value;
}

function requireJsonOrNull(value: unknown, field: string): Record<string, unknown> | null {
    if (value === null) {
        return null;
    }
    if (!isRecord(value)) {
        throw new Error(`peer reply field ${field} was ${JSON.stringify(value)}`);
    }
    return value;
}

function wirePayload(
    operation: string,
    payload: Record<string, unknown>,
): Record<string, unknown> {
    if (operation === 'command') {
        return { kind: 'command', command: payload };
    }
    return payload;
}

export async function call(
    peer: Peer,
    operation: Request['operation'],
    payload: Record<string, unknown>,
): Promise<Record<string, unknown>> {
    const record = recordFor(peer);
    if (MUTATING_OPERATIONS.has(operation)) {
        record.autonomousRepairWritesAfterLastAction = 0;
    }
    requestCounter += 1;
    const id = `c${requestCounter}`;
    const reply = await peer.request({ id, operation, payload: wirePayload(operation, payload) });
    for (const event of reply.events) {
        record.events.push(event);
        if (event.origin === 'webRepair') {
            record.autonomousRepairWrites += 1;
            record.autonomousRepairWritesAfterLastAction += 1;
        }
    }
    assertReply(reply, id);
    if (reply.error !== null) {
        throw new PeerError(operation, reply.error.code, reply.error.message);
    }
    if (reply.value === null) {
        throw new Error(`peer reply ${id} carried no value`);
    }
    return reply.value;
}

export async function snapshot(peer: Peer): Promise<PeerSnapshot> {
    const record = recordFor(peer);
    const value = await call(peer, 'snapshot', {});
    const vector = await call(peer, 'stateVector', {});
    const documentJson = requireJsonOrNull(value['json'], 'snapshot.json');
    const displayJson = record.kind === 'rust'
        ? documentJson
        : requireJsonOrNull(value['displayJson'], 'snapshot.displayJson');
    const counters = record.kind === 'rust'
        ? {
            normalizationPassesAfterLastAction: record.autonomousRepairWritesAfterLastAction,
            autonomousRepairWrites: record.autonomousRepairWrites,
        }
        : {
            normalizationPassesAfterLastAction: requireCount(
                value['normalizationPassesAfterLastAction'],
                'snapshot.normalizationPassesAfterLastAction',
            ),
            autonomousRepairWrites: requireCount(
                value['autonomousRepairWrites'],
                'snapshot.autonomousRepairWrites',
            ),
        };
    return {
        documentJson,
        displayJson,
        documentRevision: requireString(value['documentRevision'], 'snapshot.documentRevision'),
        stateVectorBase64: requireString(
            vector['stateVectorBase64'],
            'stateVector.stateVectorBase64',
        ),
        projection: null,
        ...counters,
    };
}

export async function seedFrom(source: Peer, targets: Peer[]): Promise<void> {
    const diff = await call(source, 'stateDiff', {
        stateVectorBase64: EMPTY_STATE_VECTOR_BASE64,
    });
    const updateBase64 = requireString(diff['updateBase64'], 'stateDiff.updateBase64');
    for (const target of targets) {
        await call(target, 'applyUpdate', { updateBase64 });
    }
}

function takeDocumentEvents(peer: Peer): UpdateEvent[] {
    const record = recordFor(peer);
    const taken: UpdateEvent[] = [];
    const retained: UpdateEvent[] = [];
    for (const event of record.events) {
        if (event.kind === 'document') {
            taken.push(event);
        } else {
            retained.push(event);
        }
    }
    record.events.length = 0;
    record.events.push(...retained);
    return taken;
}

export async function exchangeUntilIdle(peers: Peer[]): Promise<void> {
    let generated = 0;
    for (let round = 0; round < MAX_EXCHANGE_ROUNDS; round += 1) {
        for (const peer of peers) {
            await call(peer, 'drain', {});
        }
        const batch: { source: Peer; updateBase64: string }[] = [];
        for (const peer of peers) {
            for (const event of takeDocumentEvents(peer)) {
                batch.push({ source: peer, updateBase64: event.bytesBase64 });
            }
        }
        if (batch.length === 0) {
            return;
        }
        generated += batch.length;
        if (generated > MAX_EXCHANGE_UPDATES) {
            throw new Error(
                `peer exchange generated more than ${MAX_EXCHANGE_UPDATES} document updates`,
            );
        }
        for (const { source, updateBase64 } of batch) {
            for (const target of peers) {
                if (target === source) {
                    continue;
                }
                await call(target, 'applyUpdate', { updateBase64 });
            }
        }
    }
    throw new Error(`peer exchange did not settle within ${MAX_EXCHANGE_ROUNDS} rounds`);
}

export function paragraphFixture(schema: SchemaPreset): Record<string, unknown> {
    return {
        schema,
        tables: false,
        fragmentName: COLLABORATION_FRAGMENT_NAME,
        limits: {
            maxUpdateBytes: MAX_UPDATE_BYTES,
            requestTimeoutMillis: REQUEST_TIMEOUT_MILLIS,
        },
    };
}

function requireSchemaPreset(config: Record<string, unknown>): SchemaPreset {
    const schema = config['schema'];
    if (schema !== 'prosemirror' && schema !== 'tiptap') {
        throw new Error(`the peer config carried an unknown schema preset ${JSON.stringify(schema)}`);
    }
    return schema;
}

function initializePayload(
    kind: PeerKind,
    config: Record<string, unknown>,
    awaitSeed: boolean,
): Record<string, unknown> {
    if (kind === 'rust') {
        return { schema: requireSchemaPreset(config), awaitSeed };
    }
    return {
        tables: config['tables'],
        fragmentName: config['fragmentName'],
        limits: config['limits'],
        awaitSeed,
    };
}

async function startPeer(kind: PeerKind, config: Record<string, unknown>): Promise<Peer> {
    if (kind === 'rust') {
        if (!existsSync(RUST_PEER_EXECUTABLE)) {
            throw new Error(
                `the Rust peer executable is missing at ${RUST_PEER_EXECUTABLE}; run npm run test:plumbing so the harness builds it`,
            );
        }
        return startRustPeer(RUST_PEER_EXECUTABLE);
    }
    return startWebPeer(kind, config);
}

export async function withPeers(
    kinds: PeerKind[],
    body: (peers: Peer[]) => Promise<void>,
    config: Record<string, unknown> = paragraphFixture('prosemirror'),
): Promise<void> {
    const started: Peer[] = [];
    let failure: unknown = null;
    try {
        for (const [index, kind] of kinds.entries()) {
            const peer = await startPeer(kind, config);
            started.push(peer);
            records.set(peer, {
                kind,
                events: [],
                autonomousRepairWrites: 0,
                autonomousRepairWritesAfterLastAction: 0,
            });
            await call(peer, 'initialize', initializePayload(kind, config, index !== 0));
        }
        await body(started);
    } catch (error) {
        failure = error;
    }
    const closeFailures: unknown[] = [];
    for (const peer of started) {
        try {
            await peer.close();
        } catch (error) {
            closeFailures.push(error);
        }
    }
    if (failure !== null) {
        throw failure;
    }
    if (closeFailures.length > 0) {
        throw closeFailures[0];
    }
}
