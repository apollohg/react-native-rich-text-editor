import assert from 'node:assert/strict';
import test from 'node:test';
import * as Y from 'yjs';
import { Schema } from 'prosemirror-model';
import { schema as basicSchema } from 'prosemirror-schema-basic';
import { EditorState, TextSelection } from 'prosemirror-state';
import { tableNodes, toggleHeaderCell } from 'prosemirror-tables';
import { prosemirrorJSONToYXmlFragment, updateYFragment, yXmlFragmentToProsemirrorJSON } from 'y-prosemirror';
import { CELL_NODE, HEADER_CELL_NODE, PARAGRAPH_NODE, ROW_NODE, TABLE_NODE } from '../table-schema.js';

const FRAGMENT_NAME = 'prosemirror';
const FIRST_CELL_TEXT_POSITION = 4;
const ORIGINAL_TEXT = 'original';
const REMOTE_TEXT = 'remote';
const CONCURRENT_TEXT = 'concurrent';
const NEIGHBOR_TEXT = 'neighbor';
const schema = new Schema({
    nodes: basicSchema.spec.nodes.append(tableNodes({ tableGroup: 'block', cellContent: 'block+', cellAttributes: {} })),
    marks: basicSchema.spec.marks,
});

for (const gc of [true, false]) {
    test(`stock header replacement converges without preserving deleted text identity (gc=${gc})`, () => {
        const local = new Y.Doc({ gc });
        const remote = new Y.Doc({ gc });
        try {
            const cell = (text: string) => ({ type: CELL_NODE, content: [
                { type: PARAGRAPH_NODE, content: [{ type: 'text', text }] },
            ] });
            const source = { type: 'doc', content: [{ type: TABLE_NODE, content: [
                { type: ROW_NODE, content: [cell(ORIGINAL_TEXT), cell(NEIGHBOR_TEXT)] },
            ] }] };
            const localFragment = local.getXmlFragment(FRAGMENT_NAME);
            prosemirrorJSONToYXmlFragment(schema, source, localFragment);
            Y.applyUpdate(remote, Y.encodeStateAsUpdate(local));
            const remoteFragment = remote.getXmlFragment(FRAGMENT_NAME);
            const firstRow = (localFragment.get(0) as Y.XmlElement).get(0) as Y.XmlElement;
            const originalCell = firstRow.get(0) as Y.XmlElement;
            const originalText = (originalCell.get(0) as Y.XmlElement).get(0) as Y.XmlText;
            const anchor = Y.createRelativePositionFromTypeIndex(originalText, 0, -1);
            assert.equal(Y.createAbsolutePositionFromRelativePosition(anchor, local)?.type, originalText);

            let state = EditorState.create({ schema, doc: schema.nodeFromJSON(source) });
            const mapping = { mapping: new Map(), isOMark: new Map() };
            state = state.apply(state.tr.setSelection(TextSelection.create(state.doc, FIRST_CELL_TEXT_POSITION))
                .insertText(REMOTE_TEXT));
            updateYFragment(remote, remoteFragment, state.doc, mapping);
            assert.equal(toggleHeaderCell(state, transaction => { state = state.apply(transaction); }), true);
            updateYFragment(remote, remoteFragment, state.doc, mapping);

            originalText.insert(0, CONCURRENT_TEXT);
            assert.equal(originalText.toString(), CONCURRENT_TEXT + ORIGINAL_TEXT);
            const localUpdate = Y.encodeStateAsUpdate(local);
            const remoteUpdate = Y.encodeStateAsUpdate(remote);
            Y.applyUpdate(local, remoteUpdate);
            Y.applyUpdate(remote, localUpdate);

            const expected = schema.nodeFromJSON({ ...source, content: [{ type: TABLE_NODE, content: [
                { type: ROW_NODE, content: [
                    { ...cell(REMOTE_TEXT + ORIGINAL_TEXT), type: HEADER_CELL_NODE }, cell(NEIGHBOR_TEXT),
                ] },
            ] }] });
            assert.deepEqual(schema.nodeFromJSON(yXmlFragmentToProsemirrorJSON(localFragment)).toJSON(), expected.toJSON());
            assert.deepEqual(yXmlFragmentToProsemirrorJSON(localFragment), yXmlFragmentToProsemirrorJSON(remoteFragment));
            const replacement = ((localFragment.get(0) as Y.XmlElement).get(0) as Y.XmlElement).get(0) as Y.XmlElement;
            const replacementText = (replacement.get(0) as Y.XmlElement).get(0) as Y.XmlText;
            assert.notEqual(replacement, originalCell);
            assert.notEqual(Y.createAbsolutePositionFromRelativePosition(anchor, local)?.type, replacementText);
        } finally {
            local.destroy();
            remote.destroy();
        }
    });
}
