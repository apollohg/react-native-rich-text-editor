import assert from 'node:assert/strict';
import test from 'node:test';
import * as Y from 'yjs';
import { canonicalDocumentShape } from '../assertions.js';
import {
    call,
    exchangeUntilIdle,
    seedFrom,
    snapshot,
    withPeers,
} from '../controller.js';

const CODE_MARK = 'code';
const PLAIN_TEXT = 'plain ';
const MARKED_TEXT = 'code';
const WEB_TEXT = 'X';
const PARAGRAPH_TEXT_START = 1;
const MARKED_RUN_SPLIT = 2;
const INSIDE_MARKED_RUN = PARAGRAPH_TEXT_START + PLAIN_TEXT.length + MARKED_RUN_SPLIT;
const EDITED_MARKED_TEXT = `${MARKED_TEXT.slice(0, MARKED_RUN_SPLIT)}${WEB_TEXT}${MARKED_TEXT.slice(MARKED_RUN_SPLIT)}`;

function formatItemsIn(updateBase64: string): Record<string, unknown>[] {
    return Y.decodeUpdate(Buffer.from(updateBase64, 'base64')).structs.flatMap((struct) =>
        struct instanceof Y.Item && struct.content instanceof Y.ContentFormat
            ? [{ [struct.content.key]: struct.content.value }]
            : []);
}

async function documentOf(peer: Parameters<typeof snapshot>[0]): Promise<string> {
    return JSON.stringify(canonicalDocumentShape((await snapshot(peer)).documentJson));
}

test('a stock web peer types inside a natively marked run without rewriting its formats', async () => {
    await withPeers(['rust', 'prosemirror'] as const, async ([native, web]) => {
        await call(native, 'command', { type: 'insertText', text: PLAIN_TEXT });
        await call(native, 'command', { type: 'toggleMark', markType: CODE_MARK });
        await call(native, 'command', { type: 'insertText', text: MARKED_TEXT });
        await seedFrom(native, [web]);
        await exchangeUntilIdle([native, web]);
        const seeded = await snapshot(web);

        await call(web, 'command', { type: 'insertText', text: WEB_TEXT, at: INSIDE_MARKED_RUN });
        const webEdit = await call(web, 'stateDiff', { stateVectorBase64: seeded.stateVectorBase64 });
        await exchangeUntilIdle([native, web]);

        assert.deepEqual(
            formatItemsIn(String(webEdit['updateBase64'])),
            [],
            'the web peer rewrote mark formats that the native peer wrote for an attribute-less mark',
        );
        const nativeDocument = await documentOf(native);
        assert.equal(nativeDocument, await documentOf(web), 'the peers diverged after the web edit');
        assert.deepEqual(
            canonicalDocumentShape((await snapshot(native)).documentJson),
            {
                type: 'doc',
                content: [{
                    type: 'paragraph',
                    content: [
                        { type: 'text', text: PLAIN_TEXT },
                        { type: 'text', text: EDITED_MARKED_TEXT, marks: [{ type: CODE_MARK }] },
                    ],
                }],
            },
            `the web edit did not land inside the natively marked run: ${nativeDocument}`,
        );
    });
});
