import assert from 'node:assert/strict';
import test from 'node:test';
import {
    call,
    exchangeUntilIdle,
    paragraphFixture,
    seedFrom,
    snapshot,
    withPeers,
} from '../controller.js';

const TEXT_STYLE_MARK = 'textStyle';
const CANONICAL_RED = '#ff0000ff';
const RED_TEXT = 'colour';
const INSERT_POSITION = 3;
const WEB_INSERT = 'X';
const COLOURED_NODE = {
    type: 'text',
    text: RED_TEXT,
    marks: [{ type: TEXT_STYLE_MARK, attrs: { color: CANONICAL_RED } }],
};

function textRuns(document: unknown): Record<string, unknown>[] {
    if (document === null || typeof document !== 'object') return [];
    const node = document as Record<string, unknown>;
    if (node['type'] === 'text') return [node];
    return Array.isArray(node['content']) ? node['content'].flatMap(textRuns) : [];
}

test('stock Tiptap TextStyle and Color preserve colour attributes through shared JSON', async () => {
    await withPeers(['tiptap', 'tiptap'], async ([source, target]) => {
        await call(source, 'command', { type: 'setMark', markType: TEXT_STYLE_MARK, attrs: { color: CANONICAL_RED } });
        await call(source, 'command', { type: 'insertText', text: RED_TEXT });
        await seedFrom(source, [target]);
        await exchangeUntilIdle([source, target]);
        for (const peer of [source, target]) {
            const state = await snapshot(peer);
            assert.deepEqual(textRuns(state.displayJson), [COLOURED_NODE]);
        }
    }, paragraphFixture('tiptap'));
});

test('native textStyle colours survive Tiptap typing without format repair writes', async () => {
    await withPeers(['rust', 'tiptap'], async ([native, web]) => {
        await call(native, 'command', {
            type: 'setMark', markType: TEXT_STYLE_MARK, attrs: { color: 'red' },
        });
        await call(native, 'command', { type: 'insertText', text: RED_TEXT });
        await seedFrom(native, [web]);
        await exchangeUntilIdle([native, web]);
        assert.deepEqual(textRuns((await snapshot(web)).displayJson), [COLOURED_NODE]);
        await call(web, 'command', { type: 'insertText', text: WEB_INSERT, at: INSERT_POSITION });
        await exchangeUntilIdle([native, web]);
        const expected = [{ ...COLOURED_NODE, text: 'coXlour' }];
        for (const peer of [native, web]) {
            const state = await snapshot(peer);
            assert.deepEqual(textRuns(state.documentJson), expected);
            assert.equal(state.autonomousRepairWrites, 0);
        }
    }, paragraphFixture('tiptap'));
});

test('native accepts a stock Tiptap colour mark and preserves it through local typing', async () => {
    await withPeers(['tiptap', 'rust'], async ([web, native]) => {
        await call(web, 'command', { type: 'setMark', markType: TEXT_STYLE_MARK, attrs: { color: CANONICAL_RED } });
        await call(web, 'command', { type: 'insertText', text: RED_TEXT });
        await seedFrom(web, [native]);
        await exchangeUntilIdle([web, native]);
        await call(native, 'command', { type: 'insertText', text: WEB_INSERT, at: INSERT_POSITION });
        await exchangeUntilIdle([web, native]);
        const nativeState = await snapshot(native);
        const webState = await snapshot(web);
        assert.deepEqual(textRuns(nativeState.documentJson), [{ ...COLOURED_NODE, text: 'coXlour' }]);
        assert.deepEqual(nativeState.documentJson, webState.documentJson);
        assert.equal(webState.autonomousRepairWrites, 0);
    }, paragraphFixture('tiptap'));
});
