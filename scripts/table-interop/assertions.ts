import * as Y from 'yjs';
import { EMPTY_STATE_VECTOR_BASE64, call, flushDocumentEvents, snapshot } from './controller.js';
import type { PeerSnapshot } from './controller.js';
import { isRecord } from './peer-protocol.js';
import type { Peer } from './peer-protocol.js';

export function assertDrainBound(rounds: number, newUpdates: number): void {
    if (rounds > 100 || newUpdates > 10_000) {
        throw new Error('TBL-21 NON_QUIESCENT');
    }
}

function canonical(value: unknown): unknown {
    if (Array.isArray(value)) {
        return value.map((entry) => canonical(entry));
    }
    if (typeof value !== 'object' || value === null) {
        return value;
    }
    const sorted: Record<string, unknown> = {};
    for (const key of Object.keys(value as Record<string, unknown>).sort()) {
        sorted[key] = canonical((value as Record<string, unknown>)[key]);
    }
    return sorted;
}

function clocksOf(stateVectorBase64: string): [number, number][] {
    const decoded = Y.decodeStateVector(Buffer.from(stateVectorBase64, 'base64'));
    return [...decoded.entries()].sort(([one], [other]) => one - other);
}

function isTextNode(value: unknown): value is Record<string, unknown> {
    return isRecord(value) && value['type'] === 'text' && typeof value['text'] === 'string';
}

function mergedTextRuns(content: unknown[]): unknown[] {
    const merged: unknown[] = [];
    for (const entry of content) {
        const previous = merged[merged.length - 1];
        if (
            isTextNode(entry)
            && isTextNode(previous)
            && JSON.stringify(canonical(previous['marks'] ?? null))
                === JSON.stringify(canonical(entry['marks'] ?? null))
        ) {
            merged[merged.length - 1] = {
                ...previous,
                text: `${String(previous['text'])}${String(entry['text'])}`,
            };
            continue;
        }
        merged.push(documentShape(entry));
    }
    return merged;
}

function documentShape(value: unknown): unknown {
    if (Array.isArray(value)) {
        return value.map((entry) => documentShape(entry));
    }
    if (!isRecord(value)) {
        return value;
    }
    const shaped: Record<string, unknown> = {};
    for (const key of Object.keys(value).sort()) {
        const child = value[key];
        shaped[key] = key === 'content' && Array.isArray(child)
            ? mergedTextRuns(child)
            : documentShape(child);
    }
    return shaped;
}

function comparable(value: PeerSnapshot): string {
    return JSON.stringify({
        documentJson: documentShape(value.documentJson),
        clocks: clocksOf(value.stateVectorBase64),
        mounted: value.mounted,
        pendingDependencies: value.pendingDependencies,
    });
}

function requireBoolean(value: unknown, field: string): boolean {
    if (typeof value !== 'boolean') {
        throw new Error(`TBL-21 DIVERGED: peer reply field ${field} was ${JSON.stringify(value)}`);
    }
    return value;
}

function requireString(value: unknown, field: string): string {
    if (typeof value !== 'string') {
        throw new Error(`TBL-21 DIVERGED: peer reply field ${field} was ${JSON.stringify(value)}`);
    }
    return value;
}

async function assertSnapshotsAgree(peers: Peer[], stage: string): Promise<PeerSnapshot[]> {
    const snapshots: PeerSnapshot[] = [];
    for (const peer of peers) {
        snapshots.push(await snapshot(peer));
    }
    const [first] = snapshots;
    if (first === undefined) {
        throw new Error('TBL-21 DIVERGED: convergence needs at least one peer');
    }
    const expected = comparable(first);
    for (const [index, candidate] of snapshots.entries()) {
        const actual = comparable(candidate);
        if (actual !== expected) {
            throw new Error(
                `TBL-21 DIVERGED: peer ${index} disagrees with peer 0 ${stage}; peer 0 ${expected}; peer ${index} ${actual}`,
            );
        }
    }
    return snapshots;
}

export async function assertConverged(peers: Peer[]): Promise<void> {
    await assertSnapshotsAgree(peers, 'before the full-state exchange');
    for (const peer of peers) {
        await flushDocumentEvents(peer);
    }
    for (const [sourceIndex, source] of peers.entries()) {
        const diff = await call(source, 'stateDiff', {
            stateVectorBase64: EMPTY_STATE_VECTOR_BASE64,
        });
        const updateBase64 = requireString(diff['updateBase64'], 'stateDiff.updateBase64');
        for (const [targetIndex, target] of peers.entries()) {
            if (targetIndex === sourceIndex) {
                continue;
            }
            const applied = await call(target, 'applyUpdate', { updateBase64 });
            if (requireBoolean(applied['changed'], 'applyUpdate.changed')) {
                throw new Error(
                    `TBL-21 DIVERGED: peer ${targetIndex} changed when it replayed the full state of peer ${sourceIndex}`,
                );
            }
        }
    }
    for (const [index, peer] of peers.entries()) {
        const documentEvents = await flushDocumentEvents(peer);
        if (documentEvents.length !== 0) {
            throw new Error(
                `TBL-21 DIVERGED: peer ${index} emitted ${documentEvents.length} document updates after the full-state exchange`,
            );
        }
    }
    await assertSnapshotsAgree(peers, 'after the full-state exchange');
}
