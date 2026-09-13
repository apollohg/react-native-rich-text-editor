import * as Y from 'yjs';
import { EMPTY_STATE_VECTOR_BASE64, call, flushDocumentEvents, snapshot } from './controller.js';
import type { PeerSnapshot } from './controller.js';
import { isRecord } from './peer-protocol.js';
import type { Peer } from './peer-protocol.js';
import { CELL_ATTRIBUTE_DEFAULTS } from './table-schema.js';

const MINIMUM_CONVERGENCE_PEERS = 2;
const ATTRIBUTES_KEY = 'attrs';
const NO_ATTRIBUTES = 0;

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

function fieldsBesidesText(node: Record<string, unknown>): string[] {
    return Object.keys(node).filter((key) => key !== 'text').sort();
}

function mergeable(previous: unknown, entry: unknown): boolean {
    if (!isTextNode(previous) || !isTextNode(entry)) {
        return false;
    }
    if (String(previous['text']).length === 0 || String(entry['text']).length === 0) {
        return false;
    }
    if (
        JSON.stringify(canonical(previous['marks'] ?? null))
            !== JSON.stringify(canonical(entry['marks'] ?? null))
    ) {
        return false;
    }
    const previousFields = fieldsBesidesText(previous);
    const entryFields = fieldsBesidesText(entry);
    if (JSON.stringify(previousFields) !== JSON.stringify(entryFields)) {
        throw new Error(
            `TBL-21 DIVERGED: adjacent text runs carry different fields ${JSON.stringify(previousFields)} and ${JSON.stringify(entryFields)}`,
        );
    }
    for (const field of previousFields) {
        if (
            JSON.stringify(canonical(previous[field])) !== JSON.stringify(canonical(entry[field]))
        ) {
            throw new Error(
                `TBL-21 DIVERGED: adjacent text runs disagree on ${field}: ${JSON.stringify(previous[field])} and ${JSON.stringify(entry[field])}`,
            );
        }
    }
    return true;
}

function mergedTextRuns(content: unknown[]): unknown[] {
    const merged: unknown[] = [];
    for (const entry of content) {
        const previous = merged[merged.length - 1];
        if (mergeable(previous, entry) && isRecord(previous) && isRecord(entry)) {
            merged[merged.length - 1] = {
                ...previous,
                text: `${String(previous['text'])}${String(entry['text'])}`,
            };
            continue;
        }
        merged.push(canonicalDocumentShape(entry));
    }
    return merged;
}

function isImplicitAttribute(key: string, value: unknown): boolean {
    if (value === null) {
        return true;
    }
    return key in CELL_ATTRIBUTE_DEFAULTS && value === CELL_ATTRIBUTE_DEFAULTS[key];
}

function withoutImplicitAttributes(attrs: Record<string, unknown>): Record<string, unknown> {
    const kept: Record<string, unknown> = {};
    for (const key of Object.keys(attrs).sort()) {
        const value = attrs[key];
        if (isImplicitAttribute(key, value)) {
            continue;
        }
        kept[key] = canonicalDocumentShape(value);
    }
    return kept;
}

export function canonicalDocumentShape(value: unknown): unknown {
    if (Array.isArray(value)) {
        return value.map((entry) => canonicalDocumentShape(entry));
    }
    if (!isRecord(value)) {
        return value;
    }
    const shaped: Record<string, unknown> = {};
    for (const key of Object.keys(value).sort()) {
        const child = value[key];
        if (key === ATTRIBUTES_KEY && isRecord(child)) {
            const kept = withoutImplicitAttributes(child);
            if (Object.keys(kept).length === NO_ATTRIBUTES) {
                continue;
            }
            shaped[key] = kept;
            continue;
        }
        shaped[key] = key === 'content' && Array.isArray(child)
            ? mergedTextRuns(child)
            : canonicalDocumentShape(child);
    }
    return shaped;
}

function comparable(value: PeerSnapshot): string {
    return JSON.stringify({
        documentJson: canonicalDocumentShape(value.documentJson),
        displayJson: canonicalDocumentShape(value.displayJson),
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
    if (peers.length < MINIMUM_CONVERGENCE_PEERS) {
        throw new Error(
            `TBL-21 DIVERGED: convergence needs at least ${MINIMUM_CONVERGENCE_PEERS} peers, not ${peers.length}`,
        );
    }
    const snapshots: PeerSnapshot[] = [];
    for (const peer of peers) {
        snapshots.push(await snapshot(peer));
    }
    for (const [index, candidate] of snapshots.entries()) {
        if (!candidate.mounted) {
            throw new Error(
                `TBL-21 DIVERGED: peer ${index} is not mounted ${stage}`,
            );
        }
        if (candidate.pendingDependencies) {
            throw new Error(
                `TBL-21 DIVERGED: peer ${index} still holds quarantined updates ${stage}`,
            );
        }
        if (
            JSON.stringify(canonicalDocumentShape(candidate.displayJson))
                !== JSON.stringify(canonicalDocumentShape(candidate.documentJson))
        ) {
            throw new Error(
                `TBL-21 DIVERGED: peer ${index} projects a display document that disagrees with its own CRDT document ${stage}`,
            );
        }
    }
    const [first] = snapshots;
    if (first === undefined) {
        throw new Error('TBL-21 DIVERGED: the compared peer set is empty');
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
