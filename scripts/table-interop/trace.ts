import { readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { isRecord } from './peer-protocol.js';
import type { PeerKind, Request, UpdateEvent } from './peer-protocol.js';

export const TRACE_PROTOCOL_VERSION = 'table-interop-trace-1';

const PACKAGE_MANIFEST = fileURLToPath(new URL('./package.json', import.meta.url));
const FAILURE_CLASS_PATTERN = /TBL-21 ([A-Z_]+)/;
const PEER_ERROR_PATTERN = /^peer rejected [a-zA-Z]+ with ([A-Z_]+)/;
const ASSERTION_FAILURE_CLASS = 'ASSERTION';
const UNCLASSIFIED_FAILURE_CLASS = 'UNCLASSIFIED';

export interface TraceDependencyManifest {
    node: string;
    packages: Record<string, string>;
    rustPeerExecutable: string;
}

export interface TraceInitialization {
    kinds: PeerKind[];
    config: Record<string, unknown>;
    seed: number;
}

export interface TraceActionRecord {
    kind: 'action';
    step: number;
    peer: number;
    operation: Request['operation'];
    payload: Record<string, unknown>;
}

export interface TraceDeliveryRecord {
    kind: 'delivery';
    step: number;
    drainId: string | null;
    id: string;
    sender: number;
    recipient: number;
    sequence: number;
    origin: UpdateEvent['origin'];
    digest: string;
    bytesBase64: string;
}

export interface TraceDrainRecord {
    kind: 'drain';
    step: number;
    drainId: string;
    peers: number[];
    seed: number;
}

export interface TraceOutputRecord {
    kind: 'output';
    step: number;
    peer: number;
    documentRevision: string;
    stateVectorBase64: string;
    mounted: boolean;
    pendingDependencies: boolean;
    documentJson: Record<string, unknown> | null;
}

export type TraceRecord =
    | TraceActionRecord
    | TraceDeliveryRecord
    | TraceDrainRecord
    | TraceOutputRecord;

export interface Trace {
    protocolVersion: string;
    dependencies: TraceDependencyManifest;
    initialization: TraceInitialization;
    records: TraceRecord[];
    failureClass: string | null;
    failureMessage: string | null;
}

export function dependencyManifest(rustPeerExecutable: string): TraceDependencyManifest {
    const parsed: unknown = JSON.parse(readFileSync(PACKAGE_MANIFEST, 'utf8'));
    if (!isRecord(parsed)) {
        throw new Error('the interop package manifest is not an object');
    }
    const packages: Record<string, string> = {};
    for (const field of ['dependencies', 'devDependencies']) {
        const declared = parsed[field];
        if (!isRecord(declared)) {
            throw new Error(`the interop package manifest carries no ${field}`);
        }
        for (const [name, version] of Object.entries(declared)) {
            if (typeof version !== 'string') {
                throw new Error(`the interop package manifest pins ${name} to a non-string version`);
            }
            packages[name] = version;
        }
    }
    return { node: process.versions.node, packages, rustPeerExecutable };
}

export function failureClassOf(failure: unknown): string {
    if (!(failure instanceof Error)) {
        return UNCLASSIFIED_FAILURE_CLASS;
    }
    const harness = FAILURE_CLASS_PATTERN.exec(failure.message);
    if (harness !== null && harness[1] !== undefined) {
        return harness[1];
    }
    const peer = PEER_ERROR_PATTERN.exec(failure.message);
    if (peer !== null && peer[1] !== undefined) {
        return `PEER_ERROR:${peer[1]}`;
    }
    if (failure.name === 'AssertionError') {
        return ASSERTION_FAILURE_CLASS;
    }
    return UNCLASSIFIED_FAILURE_CLASS;
}

let active: Trace | null = null;
let completed: Trace | null = null;
let steps = 0;

export function beginTrace(
    initialization: TraceInitialization,
    dependencies: TraceDependencyManifest,
): Trace | null {
    const previous = active;
    steps = 0;
    active = {
        protocolVersion: TRACE_PROTOCOL_VERSION,
        dependencies,
        initialization,
        records: [],
        failureClass: null,
        failureMessage: null,
    };
    return previous;
}

export function endTrace(previous: Trace | null, failure: unknown): Trace | null {
    const finished = active;
    if (finished !== null && failure !== null && failure !== undefined) {
        finished.failureClass = failureClassOf(failure);
        finished.failureMessage = failure instanceof Error ? failure.message : String(failure);
    }
    if (finished !== null) {
        completed = finished;
    }
    active = previous;
    return finished;
}

export function lastTrace(): Trace {
    if (completed === null) {
        throw new Error('no interop trace has been captured yet');
    }
    return completed;
}

function push(record: TraceRecord): void {
    if (active === null) {
        return;
    }
    active.records.push(record);
}

function step(): number {
    steps += 1;
    return steps;
}

export function recordAction(
    peer: number,
    operation: Request['operation'],
    payload: Record<string, unknown>,
): void {
    push({ kind: 'action', step: step(), peer, operation, payload });
}

export function recordDelivery(
    record: Omit<TraceDeliveryRecord, 'kind' | 'step'>,
): void {
    push({ kind: 'delivery', step: step(), ...record });
}

export function recordDrain(drainId: string, peers: number[], seed: number): void {
    push({ kind: 'drain', step: step(), drainId, peers, seed });
}

export function recordOutput(record: Omit<TraceOutputRecord, 'kind' | 'step'>): void {
    push({ kind: 'output', step: step(), ...record });
}

function withRecords(trace: Trace, records: TraceRecord[]): Trace {
    return { ...trace, records };
}

function chunksOf(length: number, size: number): number[][] {
    const chunks: number[][] = [];
    for (let start = 0; start < length; start += size) {
        const chunk: number[] = [];
        for (let index = start; index < Math.min(start + size, length); index += 1) {
            chunk.push(index);
        }
        chunks.push(chunk);
    }
    return chunks;
}

export async function reduceTrace(
    trace: Trace,
    classify: (candidate: Trace) => Promise<string | null>,
): Promise<Trace> {
    const target = trace.failureClass;
    if (target === null) {
        throw new Error('a trace without a recorded failure class cannot be reduced');
    }
    let records = [...trace.records];
    let size = Math.max(1, Math.floor(records.length / 2));
    while (size >= 1) {
        let reduced = false;
        for (const chunk of chunksOf(records.length, size)) {
            const removable = new Set(chunk);
            const candidate = records.filter((_record, index) => !removable.has(index));
            if (candidate.length === records.length) {
                continue;
            }
            if (await classify(withRecords(trace, candidate)) !== target) {
                continue;
            }
            records = candidate;
            reduced = true;
            break;
        }
        if (!reduced) {
            size = size === 1 ? 0 : Math.floor(size / 2);
        }
    }
    return withRecords(trace, records);
}
