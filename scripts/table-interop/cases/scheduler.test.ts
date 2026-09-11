import assert from 'node:assert/strict';
import test from 'node:test';
import * as Y from 'yjs';
import { DeliveryScheduler, nextRandom } from '../scheduler.js';
import type { FlushResult, SchedulerAccess } from '../scheduler.js';
import { assertDrainBound, assertConverged } from '../assertions.js';
import {
    call,
    createScheduler,
    exchangeUntilIdle,
    replayTrace,
    seedFrom,
    snapshot,
    withPeers,
} from '../controller.js';
import { TRACE_PROTOCOL_VERSION, lastTrace, reduceTrace } from '../trace.js';
import type { UpdateEvent } from '../peer-protocol.js';

const REMOTE_ORIGIN = 'schedulerCaseRemote';
const TEXT_NAME = 'text';
const FIRST_SEED = 0x5eed_1234;
const SECOND_SEED = 0x0bad_c0de;

class LocalNetwork implements SchedulerAccess {
    readonly documents: Y.Doc[] = [];
    private readonly emitted: Uint8Array[][] = [];

    constructor(count: number, private readonly repairing: readonly number[] = []) {
        for (let index = 0; index < count; index += 1) {
            const document = new Y.Doc();
            const outbox: Uint8Array[] = [];
            document.on('update', (update: Uint8Array, origin: unknown) => {
                if (origin === REMOTE_ORIGIN) {
                    return;
                }
                outbox.push(update);
            });
            this.documents.push(document);
            this.emitted.push(outbox);
        }
    }

    peerCount(): number {
        return this.documents.length;
    }

    documentAt(index: number): Y.Doc {
        const document = this.documents[index];
        if (document === undefined) {
            throw new Error(`the local network has no document ${index}`);
        }
        return document;
    }

    private outboxAt(index: number): Uint8Array[] {
        const outbox = this.emitted[index];
        if (outbox === undefined) {
            throw new Error(`the local network has no outbox ${index}`);
        }
        return outbox;
    }

    type(index: number, text: string): void {
        this.documentAt(index).getText(TEXT_NAME).insert(0, text);
    }

    textAt(index: number): string {
        return this.documentAt(index).getText(TEXT_NAME).toString();
    }

    async deliver(recipient: number, updateBase64: string): Promise<void> {
        const document = this.documentAt(recipient);
        Y.applyUpdate(document, Buffer.from(updateBase64, 'base64'), REMOTE_ORIGIN);
        if (this.repairing.includes(recipient)) {
            document.getText(TEXT_NAME).insert(0, 'r');
        }
        await Promise.resolve();
    }

    async flush(peer: number): Promise<FlushResult> {
        const outbox = this.outboxAt(peer);
        const updates = outbox.splice(0, outbox.length);
        const store = this.documentAt(peer).store;
        await Promise.resolve();
        return {
            events: updates.map((update): UpdateEvent => ({
                kind: 'document',
                origin: 'local',
                bytesBase64: Buffer.from(update).toString('base64'),
            })),
            pendingDependencies: store.pendingStructs !== null || store.pendingDs !== null,
        };
    }
}

function deliveryOrder(scheduler: DeliveryScheduler): string[] {
    return scheduler.deliveries().map((record) => `${record.id}:${record.sender}>${record.recipient}`);
}

test('nextRandom is a pure function of its seed and advances the state', () => {
    assert.equal(nextRandom(FIRST_SEED), nextRandom(FIRST_SEED));
    assert.notEqual(nextRandom(FIRST_SEED), nextRandom(SECOND_SEED));
    assert.notEqual(nextRandom(FIRST_SEED), FIRST_SEED);
    assert.equal(Number.isInteger(nextRandom(FIRST_SEED)), true);
    assert.equal(nextRandom(FIRST_SEED) >= 0, true);
});

test('assertDrainBound admits both caps and rejects one step past either', () => {
    assert.equal(assertDrainBound(100, 10_000), undefined);
    assert.throws(() => assertDrainBound(101, 0), /TBL-21 NON_QUIESCENT/);
    assert.throws(() => assertDrainBound(0, 10_001), /TBL-21 NON_QUIESCENT/);
});

test('TBL-21 a known seed repeats the same delivery order and another seed reorders it', async () => {
    const orderFor = async (seed: number): Promise<string[]> => {
        const network = new LocalNetwork(3);
        network.type(0, 'zero');
        network.type(1, 'one');
        network.type(2, 'two');
        const scheduler = new DeliveryScheduler(network, { seed });
        await scheduler.drain();
        assert.equal(network.textAt(0), network.textAt(1));
        assert.equal(network.textAt(1), network.textAt(2));
        return deliveryOrder(scheduler);
    };

    const first = await orderFor(FIRST_SEED);
    const repeated = await orderFor(FIRST_SEED);
    const other = await orderFor(SECOND_SEED);
    assert.equal(first.length > 3, true);
    assert.deepEqual(first, repeated);
    assert.notDeepEqual(first, other);
    assert.deepEqual([...first].sort(), [...other].sort());
});

test('TBL-21 a duplicated message keeps identical bytes under a distinct delivery id', async () => {
    const network = new LocalNetwork(2);
    network.type(0, 'duplicate');
    const scheduler = new DeliveryScheduler(network, { seed: FIRST_SEED });
    await scheduler.collect();
    const [original] = scheduler.pending();
    assert.notEqual(original, undefined);
    if (original === undefined) {
        return;
    }
    const copy = scheduler.duplicate(original.id);
    assert.notEqual(copy.id, original.id);
    assert.equal(copy.event.bytesBase64, original.event.bytesBase64);
    assert.equal(copy.sender, original.sender);
    assert.equal(copy.recipient, original.recipient);
    assert.equal(copy.sequence, original.sequence);
    assert.equal(scheduler.pending().length, 2);

    await scheduler.drain();
    assert.equal(network.textAt(1), 'duplicate');
    const delivered = scheduler.deliveries().filter((record) => record.id === copy.id);
    assert.equal(delivered.length, 1);
    assert.equal(delivered[0]?.digest, scheduler.deliveries()[0]?.digest);
});

test('TBL-21 a partition holds queued bytes and refuses delivery until it heals', async () => {
    const network = new LocalNetwork(2);
    network.type(0, 'partitioned');
    const scheduler = new DeliveryScheduler(network, { seed: FIRST_SEED });
    await scheduler.collect();
    const [held] = scheduler.pending();
    assert.notEqual(held, undefined);
    if (held === undefined) {
        return;
    }
    const bytesBeforePartition = held.event.bytesBase64;

    scheduler.partition(0, 1, true);
    await assert.rejects(scheduler.deliver(held), /TBL-21 PARTITIONED/);
    await assert.rejects(scheduler.drain(), /TBL-21 PARTITIONED/);
    assert.equal(scheduler.pending().length, 1);
    assert.equal(scheduler.pending()[0]?.event.bytesBase64, bytesBeforePartition);
    assert.equal(network.textAt(1), '');

    scheduler.partition(0, 1, false);
    await scheduler.drain();
    assert.equal(network.textAt(1), 'partitioned');
    assert.equal(scheduler.pending().length, 0);
});

test('TBL-21 a drain that never quiesces fails as NON_QUIESCENT and reports its trace', async () => {
    const network = new LocalNetwork(2, [0, 1]);
    network.type(0, 'seed');
    const scheduler = new DeliveryScheduler(network, { seed: FIRST_SEED });
    await assert.rejects(scheduler.drain(), (error: unknown) => {
        assert.ok(error instanceof Error);
        assert.match(error.message, /TBL-21 NON_QUIESCENT/);
        assert.match(error.message, /"rounds"/);
        assert.match(error.message, /"digest"/);
        assert.match(error.message, /"sequence"/);
        return true;
    });
    assert.equal(scheduler.deliveries().length > 0, true);
});

test('TBL-21 a drain with an unsatisfiable dependency fails instead of reporting convergence', async () => {
    const network = new LocalNetwork(2);
    const source = new Y.Doc();
    source.getText(TEXT_NAME).insert(0, 'first');
    const afterFirst = Y.encodeStateVector(source);
    source.getText(TEXT_NAME).insert(0, 'second');
    const dependent = Y.encodeStateAsUpdate(source, afterFirst);
    Y.applyUpdate(network.documentAt(1), dependent, REMOTE_ORIGIN);

    const scheduler = new DeliveryScheduler(network, { seed: FIRST_SEED });
    await assert.rejects(scheduler.drain(), /TBL-21 UNRESOLVED_DEPENDENCIES/);
});

test('TBL-21 the same seed reproduces an identical delivery manifest across real peer runs', async () => {
    const manifestFor = async (): Promise<string[]> => {
        const manifest: string[] = [];
        await withPeers(['rust', 'prosemirror'], async ([native, web]) => {
            await seedFrom(native, [web]);
            await call(native, 'command', { type: 'insertText', text: 'native' });
            await call(web, 'command', { type: 'insertText', text: 'web' });
            const scheduler = createScheduler([native, web], FIRST_SEED);
            await scheduler.drain();
            for (const record of scheduler.deliveries()) {
                manifest.push(
                    `${record.id}:${record.sender}>${record.recipient}:${record.sequence}:${record.byteLength}`,
                );
            }
            await assertConverged([native, web]);
        });
        return manifest;
    };

    const first = await manifestFor();
    const repeated = await manifestFor();
    assert.equal(first.length > 1, true);
    assert.deepEqual(first, repeated);
});

test('TBL-21 a captured failing trace replays and reduces to the same failure class', async () => {
    const tailBytes: string[] = [];
    await assert.rejects(
        withPeers(['rust', 'rust'], async ([source, stranded]) => {
            await call(source, 'command', { type: 'insertText', text: 'alpha' });
            const afterAlpha = (await snapshot(source)).stateVectorBase64;
            await call(source, 'command', { type: 'insertText', text: 'beta' });
            const tail = await call(source, 'stateDiff', { stateVectorBase64: afterAlpha });
            const updateBase64 = tail['updateBase64'];
            assert.equal(typeof updateBase64, 'string');
            tailBytes.push(updateBase64 as string);
            await call(stranded, 'applyUpdate', { updateBase64 });
            await exchangeUntilIdle([stranded]);
        }),
        /TBL-21 UNRESOLVED_DEPENDENCIES/,
    );

    const trace = lastTrace();
    assert.equal(trace.protocolVersion, TRACE_PROTOCOL_VERSION);
    assert.equal(trace.failureClass, 'UNRESOLVED_DEPENDENCIES');
    assert.deepEqual(trace.initialization.kinds, ['rust', 'rust']);
    assert.equal(trace.dependencies.node, process.versions.node);
    assert.equal(typeof trace.dependencies.packages['yjs'], 'string');
    assert.match(trace.dependencies.rustPeerExecutable, /table_interop_peer$/);
    const carried = trace.records.filter(
        (record) => record.kind === 'action' && record.operation === 'applyUpdate',
    );
    assert.equal(carried.length, 1);
    assert.equal(carried[0]?.kind === 'action' && carried[0].payload['updateBase64'], tailBytes[0]);
    assert.equal(trace.records.some((record) => record.kind === 'drain'), true);
    assert.equal(trace.records.some((record) => record.kind === 'output'), true);

    assert.equal(await replayTrace(trace), 'UNRESOLVED_DEPENDENCIES');

    const reduced = await reduceTrace(trace, replayTrace);
    assert.equal(reduced.records.length < trace.records.length, true);
    assert.equal(await replayTrace(reduced), 'UNRESOLVED_DEPENDENCIES');
    assert.equal(
        reduced.records.some(
            (record) => record.kind === 'action' && record.operation === 'applyUpdate',
        ),
        true,
    );
    assert.equal(reduced.records.some((record) => record.kind === 'drain'), true);
    assert.equal(reduced.records.some((record) => record.kind === 'output'), false);
});
