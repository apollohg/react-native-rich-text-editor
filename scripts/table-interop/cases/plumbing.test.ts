import assert from 'node:assert/strict';
import { test } from 'node:test';
import {
    withPeers,
    call,
    seedFrom,
    exchangeUntilIdle,
    snapshot,
    paragraphFixture,
} from '../controller.js';

test('TBL-21 Rust and real web peers share text and undo', async () => {
    await withPeers(['rust', 'prosemirror'], async ([native, web]) => {
        await seedFrom(native, [web]);
        await call(native, 'command', { type: 'insertText', text: 'native' });
        await exchangeUntilIdle([native, web]);
        assert.deepEqual((await snapshot(native)).documentJson, (await snapshot(web)).documentJson);
        await call(web, 'command', { type: 'insertText', text: 'web' });
        await exchangeUntilIdle([native, web]);
        await call(native, 'undo', {});
        await exchangeUntilIdle([native, web]);
        const result = await snapshot(native);
        assert.equal(JSON.stringify(result.documentJson).includes('web'), true);
        assert.deepEqual(result.documentJson, (await snapshot(web)).documentJson);
    });
});

test('TBL-21 Rust and a real Tiptap peer share text and undo', async () => {
    await withPeers(['rust', 'tiptap'], async ([native, web]) => {
        await seedFrom(native, [web]);
        await call(native, 'command', { type: 'insertText', text: 'native' });
        await exchangeUntilIdle([native, web]);
        assert.deepEqual((await snapshot(native)).documentJson, (await snapshot(web)).documentJson);
        await call(web, 'command', { type: 'insertText', text: 'web' });
        await exchangeUntilIdle([native, web]);
        await call(native, 'undo', {});
        await exchangeUntilIdle([native, web]);
        const result = await snapshot(native);
        assert.equal(JSON.stringify(result.documentJson).includes('web'), true);
        assert.equal(JSON.stringify(result.documentJson).includes('native'), false);
        assert.deepEqual(result.documentJson, (await snapshot(web)).documentJson);
    }, paragraphFixture('tiptap'));
});

test('TBL-21 a web-initialized seed carries the document to the Rust peer', async () => {
    await withPeers(['prosemirror', 'rust'], async ([web, native]) => {
        const unseeded = await snapshot(native);
        assert.equal(unseeded.documentJson, null);
        assert.equal(unseeded.mounted, false);
        await seedFrom(web, [native]);
        assert.equal((await snapshot(native)).mounted, false);
        assert.equal((await call(native, 'drain', {}))['count'], 0);
        await call(web, 'command', { type: 'insertText', text: 'web' });
        await exchangeUntilIdle([web, native]);
        assert.equal((await call(native, 'drain', {}))['count'], 0);
        const seeded = await snapshot(native);
        assert.equal(seeded.mounted, true);
        assert.equal(JSON.stringify(seeded.documentJson).includes('web'), true);
        assert.deepEqual(seeded.documentJson, (await snapshot(web)).documentJson);

        await call(native, 'command', { type: 'insertText', text: 'native' });
        await exchangeUntilIdle([web, native]);
        assert.equal((await call(web, 'drain', {}))['count'], 0);
        const webSnapshot = await snapshot(web);
        assert.equal(JSON.stringify(webSnapshot.documentJson).includes('native'), true);
        assert.deepEqual(webSnapshot.documentJson, webSnapshot.displayJson);
        assert.equal(webSnapshot.projection, null);
        assert.equal(webSnapshot.normalizationPassesAfterLastAction, 0);
        assert.equal(webSnapshot.autonomousRepairWrites, 0);
        assert.equal((await snapshot(native)).autonomousRepairWrites, 0);
        assert.ok(webSnapshot.stateVectorBase64.length > 0);
        assert.equal((await snapshot(native)).stateVectorBase64.length > 0, true);
    });
});

test('TBL-21 an awaiting web peer defers mounting until the complete seed arrives', async () => {
    await withPeers(['prosemirror', 'prosemirror'], async ([seeder, awaiting]) => {
        await call(seeder, 'command', { type: 'insertText', text: 'seed' });
        const beforeSecondEdit = (await snapshot(seeder)).stateVectorBase64;
        await call(seeder, 'command', { type: 'insertText', text: 'more' });
        const dependentUpdate = await call(seeder, 'stateDiff', {
            stateVectorBase64: beforeSecondEdit,
        });

        await call(awaiting, 'applyUpdate', {
            updateBase64: dependentUpdate['updateBase64'],
        });
        const unmounted = await snapshot(awaiting);
        assert.equal(unmounted.mounted, false);
        assert.equal(unmounted.displayJson, null);
        assert.deepEqual(unmounted.documentJson, { type: 'doc', content: [] });

        await seedFrom(seeder, [awaiting]);
        const mounted = await snapshot(awaiting);
        assert.equal(mounted.mounted, true);
        assert.notEqual(mounted.displayJson, null);
        assert.deepEqual(mounted.documentJson, (await snapshot(seeder)).documentJson);
        assert.equal(JSON.stringify(mounted.documentJson).includes('seedmore'), true);
        assert.equal(mounted.autonomousRepairWrites, 0);

        await exchangeUntilIdle([seeder, awaiting]);
        assert.deepEqual(
            (await snapshot(awaiting)).documentJson,
            (await snapshot(seeder)).documentJson,
        );
    });
});
