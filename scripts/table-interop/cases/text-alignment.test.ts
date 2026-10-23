import assert from 'node:assert/strict';
import test from 'node:test';
import { fileURLToPath } from 'node:url';
import { Schema } from 'prosemirror-model';
import { schema as basic } from 'prosemirror-schema-basic';
import { getSchema, Node as TiptapNode } from '@tiptap/core';
import Document from '@tiptap/extension-document';
import Paragraph from '@tiptap/extension-paragraph';
import Text from '@tiptap/extension-text';
import { prosemirrorJSONToYDoc, yXmlFragmentToProsemirrorJSON } from 'y-prosemirror';
import * as Y from 'yjs';
import { startRustPeer } from '../rust-peer.js';
import type { Request } from '../peer-protocol.js';

const FRAGMENT_NAME = 'prosemirror';
const PARAGRAPH_TEXT_START = 1;
const DEFAULT_EXECUTABLE = fileURLToPath(
    new URL('../../../rust/editor-core/target/debug/examples/table_interop_peer', import.meta.url)
);
const PEER_EXECUTABLE = process.env['RUST_PEER_EXECUTABLE'] ?? DEFAULT_EXECUTABLE;
const alignmentAttribute = { default: null };
const prosemirror = new Schema({
    nodes: basic.spec.nodes
        .update('paragraph', {
            ...basic.spec.nodes.get('paragraph')!,
            attrs: { textAlign: alignmentAttribute },
        })
        .update('heading', {
            ...basic.spec.nodes.get('heading')!,
            attrs: { ...basic.spec.nodes.get('heading')!.attrs, textAlign: alignmentAttribute },
        }),
    marks: basic.spec.marks,
});
const tiptap = getSchema([
    Document,
    Paragraph.extend({ addAttributes: () => ({ textAlign: alignmentAttribute }) }),
    TiptapNode.create({
        name: 'heading',
        group: 'block',
        content: 'inline*',
        addAttributes: () => ({ level: { default: 1 }, textAlign: alignmentAttribute }),
        renderHTML: ({ node }) => [`h${node.attrs.level}`, 0],
    }),
    Text,
]);

for (const [preset, schema] of [
    ['prosemirror', prosemirror],
    ['tiptap', tiptap],
] as const) {
    test(`${preset} alignment attributes converge with Rust without replacing text`, async () => {
        const authored = schema.nodeFromJSON({
            type: 'doc',
            content: [
                {
                    type: 'paragraph',
                    attrs: { textAlign: 'left' },
                    content: [{ type: 'text', text: 'alpha' }],
                },
                {
                    type: 'heading',
                    attrs: { level: 2, textAlign: 'right' },
                    content: [{ type: 'text', text: 'title' }],
                },
            ],
        });
        const web = prosemirrorJSONToYDoc(schema, authored.toJSON(), FRAGMENT_NAME);
        const native = await startRustPeer(PEER_EXECUTABLE);
        let sequence = 0;
        const request = async (
            operation: Request['operation'],
            payload: Record<string, unknown> = {}
        ) => {
            const id = `alignment-${++sequence}`;
            const reply = await native.request({ id, operation, payload });
            assert.equal(reply.id, id);
            assert.equal(reply.error, null, `${operation}: ${JSON.stringify(reply.error)}`);
            assert.ok(reply.value);
            return reply.value;
        };
        const sendWeb = () =>
            request('applyUpdate', {
                updateBase64: Buffer.from(Y.encodeStateAsUpdate(web)).toString('base64'),
            });
        const receiveNative = async () => {
            const diff = await request('stateDiff', {
                stateVectorBase64: Buffer.from(Y.encodeStateVector(web)).toString('base64'),
            });
            Y.applyUpdate(web, Buffer.from(String(diff.updateBase64), 'base64'));
        };
        const command = (value: Record<string, unknown>) =>
            request('command', { kind: 'command', at: PARAGRAPH_TEXT_START, command: value });
        const normalizedJson = (value: unknown) =>
            JSON.parse(JSON.stringify(schema.nodeFromJSON(value).toJSON()));
        const webJson = () =>
            normalizedJson(yXmlFragmentToProsemirrorJSON(web.getXmlFragment(FRAGMENT_NAME)));
        try {
            await request('initialize', { schema: preset, tables: false, awaitSeed: true });
            await sendWeb();
            assert.deepEqual(
                normalizedJson((await request('snapshot')).json),
                normalizedJson(authored.toJSON()),
                'native ingestion preserves web paragraph/heading attributes'
            );
            const paragraph = web.getXmlFragment(FRAGMENT_NAME).get(0) as Y.XmlElement;
            const text = paragraph.get(0) as Y.XmlText;
            await command({ type: 'setTextAlign', align: 'center' });
            // Edit web text concurrently, before receiving native alignment.
            text.insert(0, 'web ');
            await sendWeb();
            await receiveNative();
            assert.ok(
                paragraph.get(0) === text,
                `formatting replaced shared text: ${JSON.stringify(webJson())}`
            );
            assert.equal(paragraph.getAttribute('textAlign'), 'center');
            assert.equal(text.toString(), 'web alpha');
            assert.deepEqual(normalizedJson((await request('snapshot')).json), webJson());
            await command({ type: 'unsetTextAlign' });
            await receiveNative();
            assert.equal(
                webJson().content[0].attrs.textAlign,
                null,
                'reset is the nullable web schema default'
            );
            assert.equal(
                webJson().content[1].attrs.textAlign,
                'right',
                'unselected heading is unchanged'
            );
            await request('undo');
            await receiveNative();
            assert.equal(webJson().content[0].attrs.textAlign, 'center');
            assert.equal(
                text.toString(),
                'web alpha',
                'format undo preserves concurrent web typing'
            );
            assert.deepEqual(normalizedJson((await request('snapshot')).json), webJson());
        } finally {
            await native.close();
            web.destroy();
        }
    });
}
