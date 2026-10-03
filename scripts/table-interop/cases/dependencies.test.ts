import assert from 'node:assert/strict';
import test from 'node:test';
import {
    EMPTY_STATE_VECTOR_BASE64,
    call,
    createScheduler,
    exchangeUntilIdle,
    seedFrom,
    snapshot,
    withPeers,
} from '../controller.js';
import { assertConverged, canonicalDocumentShape } from '../assertions.js';
import type { Peer } from '../peer-protocol.js';
import type { PeerSnapshot } from '../controller.js';

function auditOf(value: PeerSnapshot): string {
    return JSON.stringify({
        documentJson: value.documentJson,
        documentRevision: value.documentRevision,
        stateVectorBase64: value.stateVectorBase64,
        mounted: value.mounted,
    });
}

async function updateFrom(source: Peer, stateVectorBase64: string): Promise<string> {
    const diff = await call(source, 'stateDiff', { stateVectorBase64 });
    const updateBase64 = diff['updateBase64'];
    assert.equal(typeof updateBase64, 'string');
    return updateBase64 as string;
}

async function applyTo(target: Peer, updateBase64: string): Promise<boolean> {
    const applied = await call(target, 'applyUpdate', { updateBase64 });
    assert.equal(typeof applied['changed'], 'boolean');
    return applied['changed'] as boolean;
}

test('TBL-21 out-of-order inserts stay quarantined in Rust until their dependencies complete', async () => {
    await withPeers(['prosemirror', 'rust'], async ([source, target]) => {
        await call(source, 'command', { type: 'insertText', text: 'base' });
        const afterBase = (await snapshot(source)).stateVectorBase64;
        const base = await updateFrom(source, EMPTY_STATE_VECTOR_BASE64);
        await call(source, 'command', { type: 'insertText', text: 'a' });
        const afterA = (await snapshot(source)).stateVectorBase64;
        const deltaA = await updateFrom(source, afterBase);
        await call(source, 'command', { type: 'insertText', text: 'b' });
        const deltaB = await updateFrom(source, afterA);

        const initial = await snapshot(target);
        assert.equal(initial.mounted, false);
        assert.equal(initial.pendingDependencies, false);

        assert.equal(await applyTo(target, deltaB), false);
        const afterDeltaB = await snapshot(target);
        assert.equal(auditOf(afterDeltaB), auditOf(initial));
        assert.equal(afterDeltaB.mounted, false);
        assert.equal(afterDeltaB.pendingDependencies, true);

        assert.equal(await applyTo(target, deltaA), false);
        assert.equal(auditOf(await snapshot(target)), auditOf(initial));

        assert.equal(await applyTo(target, base), true);
        const completed = await snapshot(target);
        assert.equal(completed.mounted, true);
        assert.equal(completed.pendingDependencies, false);
        const expected = await snapshot(source);
        assert.deepEqual(completed.documentJson, expected.documentJson);
        assert.equal(JSON.stringify(completed.documentJson).includes('baseab'), true);

        assert.equal(await applyTo(target, base), false);
        assert.equal(await applyTo(target, deltaB), false);
        assert.deepEqual((await snapshot(target)).documentJson, expected.documentJson);

        await assertConverged([source, target]);
    });
});

test('TBL-21 a delete set that arrives before its insert is quarantined and then converges', async () => {
    await withPeers(['rust', 'rust'], async ([source, target]) => {
        await call(source, 'command', { type: 'insertText', text: 'delete-me' });
        const beforeDeleteVector = (await snapshot(source)).stateVectorBase64;
        const beforeDelete = await updateFrom(source, EMPTY_STATE_VECTOR_BASE64);
        await call(source, 'command', { type: 'deleteBackward' });
        const deleteFirst = await updateFrom(source, beforeDeleteVector);

        const initial = await snapshot(target);
        assert.equal(initial.mounted, false);

        assert.equal(await applyTo(target, deleteFirst), false);
        const quarantined = await snapshot(target);
        assert.equal(auditOf(quarantined), auditOf(initial));
        assert.equal(quarantined.mounted, false);
        assert.equal(quarantined.pendingDependencies, true);

        assert.equal(await applyTo(target, beforeDelete), true);
        const expected = await snapshot(source);
        const completed = await snapshot(target);
        assert.equal(completed.mounted, true);
        assert.equal(completed.pendingDependencies, false);
        assert.deepEqual(completed.documentJson, expected.documentJson);
        assert.equal(JSON.stringify(completed.documentJson).includes('delete-m'), true);
        assert.equal(JSON.stringify(completed.documentJson).includes('delete-me'), false);

        assert.equal(await applyTo(target, deleteFirst), false);
        assert.deepEqual((await snapshot(target)).documentJson, expected.documentJson);

        await assertConverged([source, target]);
    });
});

test('TBL-21 a self-consistent partial seed mounts the web peer over an incomplete document', async () => {
    await withPeers(['prosemirror', 'prosemirror'], async ([seeder, awaiting]) => {
        await call(seeder, 'command', { type: 'insertText', text: 'seed' });
        const firstChunk = await updateFrom(seeder, EMPTY_STATE_VECTOR_BASE64);
        await call(seeder, 'command', { type: 'insertText', text: 'more' });

        assert.equal(await applyTo(awaiting, firstChunk), true);
        const mountedEarly = await snapshot(awaiting);
        assert.equal(mountedEarly.mounted, true);
        assert.equal(mountedEarly.pendingDependencies, false);
        assert.notDeepEqual(mountedEarly.documentJson, (await snapshot(seeder)).documentJson);
        assert.equal(JSON.stringify(mountedEarly.documentJson).includes('seedmore'), false);

        await seedFrom(seeder, [awaiting]);
        await exchangeUntilIdle([seeder, awaiting]);
        await assertConverged([seeder, awaiting]);
        assert.equal(
            JSON.stringify((await snapshot(awaiting)).documentJson).includes('seedmore'),
            true,
        );
    });
});

test('TBL-21 a dropped seed chunk leaves an unmountable peer that fails the drain', async () => {
    await withPeers(['prosemirror', 'prosemirror'], async ([seeder, awaiting]) => {
        await call(seeder, 'command', { type: 'insertText', text: 'seed' });
        await call(seeder, 'command', { type: 'insertText', text: 'more' });

        const scheduler = createScheduler([seeder, awaiting]);
        await scheduler.collect();
        const [seedChunk] = scheduler.pending();
        assert.notEqual(seedChunk, undefined);
        if (seedChunk === undefined) {
            return;
        }
        const dropped = scheduler.drop(seedChunk.id);
        assert.equal(dropped.event.bytesBase64, seedChunk.event.bytesBase64);
        await assert.rejects(scheduler.drain(), /TBL-21 UNRESOLVED_DEPENDENCIES/);

        const stranded = await snapshot(awaiting);
        assert.equal(stranded.mounted, false);
        assert.equal(stranded.pendingDependencies, true);
        assert.equal(stranded.displayJson, null);

        await assert.rejects(
            call(awaiting, 'command', { type: 'insertText', text: 'blocked' }),
            /PEER_NOT_INITIALIZED/,
        );
        await seedFrom(seeder, [awaiting]);
        await exchangeUntilIdle([seeder, awaiting]);
        const healed = await snapshot(awaiting);
        assert.equal(healed.mounted, true);
        assert.equal(healed.pendingDependencies, false);
        await assertConverged([seeder, awaiting]);
    });
});

test('TBL-21 a symmetrically stranded pair is rejected by the convergence oracle', async () => {
    await withPeers(['rust', 'rust', 'rust'], async ([source, first, second]) => {
        await call(source, 'command', { type: 'insertText', text: 'alpha' });
        const afterAlpha = (await snapshot(source)).stateVectorBase64;
        await call(source, 'command', { type: 'insertText', text: 'beta' });
        const tail = await updateFrom(source, afterAlpha);

        assert.equal(await applyTo(first, tail), false);
        assert.equal(await applyTo(second, tail), false);
        const stranded = await snapshot(first);
        assert.equal(stranded.mounted, false);
        assert.equal(stranded.pendingDependencies, true);
        assert.equal(
            JSON.stringify(stranded.documentJson),
            JSON.stringify((await snapshot(second)).documentJson),
        );
        assert.equal(
            stranded.stateVectorBase64,
            (await snapshot(second)).stateVectorBase64,
        );

        await assert.rejects(
            assertConverged([first, second]),
            /TBL-21 DIVERGED: peer 0 still holds quarantined updates before the full-state exchange/,
        );

        const complete = await updateFrom(source, EMPTY_STATE_VECTOR_BASE64);
        assert.equal(await applyTo(first, complete), true);
        assert.equal(await applyTo(second, complete), true);
        await assertConverged([first, second]);
    });
});

test('TBL-21 the convergence oracle refuses a call that compares fewer than two peers', async () => {
    await withPeers(['rust'], async ([only]) => {
        await call(only, 'command', { type: 'insertText', text: 'lonely' });
        await assert.rejects(
            assertConverged([only]),
            /TBL-21 DIVERGED: convergence needs at least 2 peers, not 1/,
        );
    });
});

test('the canonical document shape merges only genuinely equal adjacent text runs', () => {
    const merged = canonicalDocumentShape({
        type: 'doc',
        content: [{ type: 'paragraph', content: [
            { type: 'text', text: 'a' },
            { type: 'text', text: 'b' },
        ] }],
    });
    assert.deepEqual(merged, canonicalDocumentShape({
        type: 'doc',
        content: [{ type: 'paragraph', content: [{ type: 'text', text: 'ab' }] }],
    }));

    const withEmptyRun = canonicalDocumentShape({
        type: 'doc',
        content: [{ type: 'paragraph', content: [
            { type: 'text', text: 'a' },
            { type: 'text', text: '' },
            { type: 'text', text: 'b' },
        ] }],
    });
    assert.notDeepEqual(withEmptyRun, merged);

    const markedApart = canonicalDocumentShape({
        type: 'doc',
        content: [{ type: 'paragraph', content: [
            { type: 'text', text: 'a', marks: [{ type: 'em' }] },
            { type: 'text', text: 'b' },
        ] }],
    });
    assert.notDeepEqual(markedApart, merged);

    assert.throws(
        () => canonicalDocumentShape({
            type: 'doc',
            content: [{ type: 'paragraph', content: [
                { type: 'text', text: 'a', attrs: { cell: 1 } },
                { type: 'text', text: 'b' },
            ] }],
        }),
        /TBL-21 DIVERGED: adjacent text runs carry different fields/,
    );

    assert.throws(
        () => canonicalDocumentShape({
            type: 'doc',
            content: [{ type: 'paragraph', content: [
                { type: 'text', text: 'a', attrs: { cell: 1 } },
                { type: 'text', text: 'b', attrs: { cell: 2 } },
            ] }],
        }),
        /TBL-21 DIVERGED: adjacent text runs disagree on attrs/,
    );
});
