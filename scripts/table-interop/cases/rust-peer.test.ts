import test from 'node:test';
import assert from 'node:assert/strict';
import { existsSync } from 'node:fs';
import { chmod, mkdtemp, readFile, rm, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { assertReply } from '../peer-protocol.js';
import type { PeerReply } from '../peer-protocol.js';
import { startRustPeer } from '../rust-peer.js';

const DEFAULT_EXECUTABLE = fileURLToPath(
    new URL('../../../rust/editor-core/target/debug/examples/table_interop_peer', import.meta.url),
);
const EXECUTABLE = process.env['RUST_PEER_EXECUTABLE'] ?? DEFAULT_EXECUTABLE;

function requireValue(reply: PeerReply, id: string): Record<string, unknown> {
    assertReply(reply, id);
    assert.equal(reply.error, null, `reply ${id} carried error ${JSON.stringify(reply.error)}`);
    assert.notEqual(reply.value, null, `reply ${id} carried no value`);
    return reply.value as Record<string, unknown>;
}

function documentText(json: unknown): string {
    if (typeof json !== 'object' || json === null) {
        return '';
    }
    const node = json as { text?: unknown; content?: unknown };
    if (typeof node.text === 'string') {
        return node.text;
    }
    if (!Array.isArray(node.content)) {
        return '';
    }
    return node.content.map((child) => documentText(child)).join('');
}

function revisionOf(snapshot: Record<string, unknown>): number {
    const revision = snapshot['documentRevision'];
    assert.equal(typeof revision, 'string', `documentRevision was ${JSON.stringify(revision)}`);
    return Number(revision as string);
}

test('the spawned Rust peer edits, reports one local update, and exits cleanly', async () => {
    assert.ok(
        existsSync(EXECUTABLE),
        `build the peer first: cargo build --manifest-path rust/editor-core/Cargo.toml --features table-interop --example table_interop_peer (looked for ${EXECUTABLE})`,
    );

    const peer = await startRustPeer(EXECUTABLE);
    try {
        requireValue(
            await peer.request({ id: 'r1', operation: 'initialize', payload: {} }),
            'r1',
        );

        const before = requireValue(
            await peer.request({ id: 'r2', operation: 'snapshot', payload: {} }),
            'r2',
        );
        assert.equal(documentText(before['json']), '');

        const applied = await peer.request({
            id: 'r3',
            operation: 'command',
            payload: { kind: 'command', command: { type: 'insertText', text: 'a' } },
        });
        const appliedValue = requireValue(applied, 'r3');
        assert.equal(appliedValue['type'], 'transaction');
        assert.deepEqual(applied.events, []);

        const after = requireValue(
            await peer.request({ id: 'r4', operation: 'snapshot', payload: {} }),
            'r4',
        );
        assert.equal(documentText(after['json']), 'a');
        assert.ok(
            revisionOf(after) > revisionOf(before),
            `document revision did not advance: ${revisionOf(before)} -> ${revisionOf(after)}`,
        );

        const firstDrain = await peer.request({ id: 'r5', operation: 'drain', payload: {} });
        requireValue(firstDrain, 'r5');
        assert.equal(firstDrain.events.length, 1, JSON.stringify(firstDrain.events));
        const event = firstDrain.events[0];
        assert.ok(event !== undefined);
        assert.equal(event.kind, 'document');
        assert.equal(event.origin, 'local');
        assert.ok(event.bytesBase64.length > 0, 'local update carried no bytes');

        const secondDrain = await peer.request({ id: 'r6', operation: 'drain', payload: {} });
        requireValue(secondDrain, 'r6');
        assert.deepEqual(secondDrain.events, []);

        const rejected = await peer.request({
            id: 'r7',
            operation: 'command',
            payload: { kind: 'command', command: { type: 'notACommand' } },
        });
        assertReply(rejected, 'r7');
        assert.equal(rejected.value, null);
        assert.notEqual(rejected.error, null);
        assert.equal(rejected.error?.code, 'CONFIG_INVALID');

        const survived = requireValue(
            await peer.request({ id: 'r8', operation: 'snapshot', payload: {} }),
            'r8',
        );
        assert.equal(documentText(survived['json']), 'a');

        requireValue(
            await peer.request({ id: 'r9', operation: 'shutdown', payload: {} }),
            'r9',
        );
    } finally {
        await peer.close();
    }
});

const DELAYED_EXIT_MILLIS = 100;

test('close awaits an explicit shutdown without writing another request', async () => {
    const directory = await mkdtemp(join(tmpdir(), 'table-peer-shutdown-'));
    const executable = join(directory, 'peer.cjs');
    const requests = join(directory, 'requests.jsonl');
    await writeFile(executable, String.raw`#!/usr/bin/env node
const { appendFileSync } = require('node:fs');
const { createInterface } = require('node:readline');
const reader = createInterface({ input: process.stdin });
let shutdowns = 0;
reader.on('line', line => {
    const request = JSON.parse(line);
    appendFileSync(${JSON.stringify(requests)}, line + '\n');
    if (request.operation !== 'shutdown' || ++shutdowns !== 1) process.exit(1);
    process.stdout.write(JSON.stringify({ id: request.id, value: {}, error: null, events: [] }) + '\n');
    setTimeout(() => process.exit(0), ${DELAYED_EXIT_MILLIS});
});
`);
    await chmod(executable, 0o755);
    const peer = await startRustPeer(executable);
    try {
        const request = { id: 'explicit-shutdown', operation: 'shutdown', payload: {} } as const;
        requireValue(await peer.request(request), request.id);
        await Promise.all([peer.close(), peer.close()]);
        assert.deepEqual((await readFile(requests, 'utf8')).trim().split('\n').map(line => JSON.parse(line)), [request]);
    } finally {
        try {
            await peer.close();
        } finally {
            await rm(directory, { recursive: true, force: true });
        }
    }
});
