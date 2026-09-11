import { existsSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { assertReply, isRecord } from './peer-protocol.js';
import type { Peer, PeerKind, Request, UpdateEvent } from './peer-protocol.js';
import { startRustPeer } from './rust-peer.js';
import { startWebPeer } from './web-peer.js';
import { DeliveryScheduler } from './scheduler.js';
import type {
    DeliveryObserver,
    DeliveryRecord,
    FlushResult,
    SchedulerAccess,
    ScheduledMessage,
} from './scheduler.js';
import {
    beginTrace,
    dependencyManifest,
    endTrace,
    failureClassOf,
    recordAction,
    recordDelivery,
    recordDrain,
    recordOutput,
} from './trace.js';
import type { Trace } from './trace.js';

export interface PeerSnapshot {
    documentJson: Record<string, unknown> | null;
    displayJson: Record<string, unknown> | null;
    documentRevision: string;
    stateVectorBase64: string;
    projection: unknown;
    mounted: boolean;
    pendingDependencies: boolean;
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
export const EMPTY_STATE_VECTOR_BASE64 = Buffer.from([0]).toString('base64');
export const DEFAULT_EXCHANGE_SEED = 0x7ab1_e21d;
const NATIVE_NORMALIZATION_IS_NOT_INSTRUMENTED = 0;
const RECORDED_ACTIONS: readonly Request['operation'][] = ['command', 'undo', 'redo', 'applyUpdate'];

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
    index: number;
    events: UpdateEvent[];
};

const records = new WeakMap<Peer, PeerRecord>();
let requestCounter = 0;
let drainCounter = 0;
let activeDrainId: string | null = null;

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

function requireBoolean(value: unknown, field: string): boolean {
    if (typeof value !== 'boolean') {
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

async function performRequest(
    peer: Peer,
    operation: Request['operation'],
    payload: Record<string, unknown>,
    capture: 'action' | 'delivery',
): Promise<Record<string, unknown>> {
    const record = recordFor(peer);
    if (capture === 'action' && RECORDED_ACTIONS.includes(operation)) {
        recordAction(record.index, operation, payload);
    }
    requestCounter += 1;
    const id = `c${requestCounter}`;
    const reply = await peer.request({ id, operation, payload: wirePayload(operation, payload) });
    record.events.push(...reply.events);
    assertReply(reply, id);
    if (reply.error !== null) {
        throw new PeerError(operation, reply.error.code, reply.error.message);
    }
    if (reply.value === null) {
        throw new Error(`peer reply ${id} carried no value`);
    }
    return reply.value;
}

export async function call(
    peer: Peer,
    operation: Request['operation'],
    payload: Record<string, unknown>,
): Promise<Record<string, unknown>> {
    return performRequest(peer, operation, payload, 'action');
}

export async function snapshot(peer: Peer): Promise<PeerSnapshot> {
    const record = recordFor(peer);
    const value = await call(peer, 'snapshot', {});
    const vector = await call(peer, 'stateVector', {});
    const documentJson = requireJsonOrNull(value['json'], 'snapshot.json');
    const displayJson = record.kind === 'rust'
        ? documentJson
        : requireJsonOrNull(value['displayJson'], 'snapshot.displayJson');
    const normalizationPassesAfterLastAction = record.kind === 'rust'
        ? NATIVE_NORMALIZATION_IS_NOT_INSTRUMENTED
        : requireCount(
            value['normalizationPassesAfterLastAction'],
            'snapshot.normalizationPassesAfterLastAction',
        );
    const captured: PeerSnapshot = {
        documentJson,
        displayJson,
        documentRevision: requireString(value['documentRevision'], 'snapshot.documentRevision'),
        stateVectorBase64: requireString(
            vector['stateVectorBase64'],
            'stateVector.stateVectorBase64',
        ),
        projection: null,
        mounted: requireBoolean(value['mounted'], 'snapshot.mounted'),
        pendingDependencies: requireBoolean(
            value['pendingDependencies'],
            'snapshot.pendingDependencies',
        ),
        normalizationPassesAfterLastAction,
        autonomousRepairWrites: requireCount(
            value['autonomousRepairWrites'],
            'snapshot.autonomousRepairWrites',
        ),
    };
    recordOutput({
        peer: record.index,
        documentRevision: captured.documentRevision,
        stateVectorBase64: captured.stateVectorBase64,
        mounted: captured.mounted,
        pendingDependencies: captured.pendingDependencies,
        documentJson: captured.documentJson,
    });
    return captured;
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

class ControllerAccess implements SchedulerAccess {
    private readonly peers: Peer[];

    constructor(peers: Peer[]) {
        this.peers = peers;
    }

    peerCount(): number {
        return this.peers.length;
    }

    private peerAt(index: number): Peer {
        const peer = this.peers[index];
        if (peer === undefined) {
            throw new Error(`the scheduler addressed peer ${index} outside the started set`);
        }
        return peer;
    }

    async deliver(recipient: number, updateBase64: string): Promise<void> {
        await performRequest(this.peerAt(recipient), 'applyUpdate', { updateBase64 }, 'delivery');
    }

    async flush(peer: number): Promise<FlushResult> {
        const target = this.peerAt(peer);
        const drained = await performRequest(target, 'drain', {}, 'delivery');
        return {
            events: takeDocumentEvents(target),
            pendingDependencies: requireBoolean(
                drained['pendingDependencies'],
                'drain.pendingDependencies',
            ),
        };
    }
}

function deliveryObserver(): { onDelivery: (record: DeliveryRecord, message: ScheduledMessage) => void } {
    return {
        onDelivery(record: DeliveryRecord, message: ScheduledMessage): void {
            recordDelivery({
                drainId: activeDrainId,
                id: record.id,
                attempt: record.attempt,
                failed: record.failed,
                sender: record.sender,
                recipient: record.recipient,
                sequence: record.sequence,
                origin: record.origin,
                digest: record.digest,
                bytesBase64: message.event.bytesBase64,
            });
        },
    };
}

export function createScheduler(
    peers: Peer[],
    seed: number = DEFAULT_EXCHANGE_SEED,
    observer: DeliveryObserver | null = null,
): DeliveryScheduler {
    const tracing = deliveryObserver();
    return new DeliveryScheduler(new ControllerAccess(peers), {
        seed,
        observer: {
            onDelivery(record: DeliveryRecord, message: ScheduledMessage): void {
                tracing.onDelivery(record, message);
                observer?.onDelivery(record, message);
            },
        },
    });
}

export async function flushDocumentEvents(peer: Peer): Promise<UpdateEvent[]> {
    await performRequest(peer, 'drain', {}, 'delivery');
    return takeDocumentEvents(peer);
}

export async function exchangeUntilIdle(
    peers: Peer[],
    seed: number = DEFAULT_EXCHANGE_SEED,
): Promise<void> {
    const scheduler = createScheduler(peers, seed);
    drainCounter += 1;
    const drainId = `x${drainCounter}`;
    recordDrain(drainId, peers.map((peer) => recordFor(peer).index), seed);
    const previousDrainId = activeDrainId;
    activeDrainId = drainId;
    try {
        await scheduler.drain();
    } finally {
        activeDrainId = previousDrainId;
    }
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

export type PeerTuple<Kinds extends readonly PeerKind[]> = { [Index in keyof Kinds]: Peer };

function assertPeerCountMatchesKinds<Kinds extends readonly PeerKind[]>(
    peers: Peer[],
    kinds: Kinds,
): asserts peers is PeerTuple<Kinds> & Peer[] {
    if (peers.length !== kinds.length) {
        throw new Error(
            `the controller started ${peers.length} peers for ${kinds.length} requested kinds`,
        );
    }
}

export async function withPeers<const Kinds extends readonly PeerKind[]>(
    kinds: Kinds,
    body: (peers: PeerTuple<Kinds>) => Promise<void>,
    config: Record<string, unknown> = paragraphFixture('prosemirror'),
    seed: number = DEFAULT_EXCHANGE_SEED,
): Promise<void> {
    const started: Peer[] = [];
    let failure: unknown = null;
    const outerTrace = beginTrace(
        { kinds: [...kinds], config, seed },
        dependencyManifest(RUST_PEER_EXECUTABLE),
    );
    try {
        for (const [index, kind] of kinds.entries()) {
            const peer = await startPeer(kind, config);
            started.push(peer);
            records.set(peer, { kind, index, events: [] });
            await call(peer, 'initialize', initializePayload(kind, config, index !== 0));
        }
        assertPeerCountMatchesKinds(started, kinds);
        await body(started);
    } catch (error) {
        failure = error;
    }
    endTrace(outerTrace, failure);
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

function replayPeerAt(peers: Peer[], index: number): Peer {
    const peer = peers[index];
    if (peer === undefined) {
        throw new Error(`the trace addressed peer ${index} outside the recorded set`);
    }
    return peer;
}

export async function replayTrace(trace: Trace): Promise<string | null> {
    let failure: unknown = null;
    try {
        await withPeers(
            trace.initialization.kinds,
            async (peers) => {
                const replayedDrains = new Set<string>();
                for (const record of trace.records) {
                    if (record.kind === 'output') {
                        continue;
                    }
                    if (record.kind === 'action') {
                        await call(
                            replayPeerAt(peers, record.peer),
                            record.operation,
                            record.payload,
                        );
                        continue;
                    }
                    if (record.kind === 'drain') {
                        replayedDrains.add(record.drainId);
                        await exchangeUntilIdle(
                            record.peers.map((index) => replayPeerAt(peers, index)),
                            record.seed,
                        );
                        continue;
                    }
                    if (record.drainId !== null && replayedDrains.has(record.drainId)) {
                        continue;
                    }
                    await call(replayPeerAt(peers, record.recipient), 'applyUpdate', {
                        updateBase64: record.bytesBase64,
                    });
                }
            },
            trace.initialization.config,
            trace.initialization.seed,
        );
    } catch (error) {
        failure = error;
    }
    return failure === null ? null : failureClassOf(failure);
}
