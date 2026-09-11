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
        assert.equal((await snapshot(native)).documentJson, null);
        await seedFrom(web, [native]);
        assert.equal((await call(native, 'drain', {}))['count'], 0);
        await call(web, 'command', { type: 'insertText', text: 'web' });
        await exchangeUntilIdle([web, native]);
        assert.equal((await call(native, 'drain', {}))['count'], 0);
        const seeded = await snapshot(native);
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
        assert.ok(webSnapshot.stateVectorBase64.length > 0);
        assert.equal((await snapshot(native)).stateVectorBase64.length > 0, true);
    });
});
