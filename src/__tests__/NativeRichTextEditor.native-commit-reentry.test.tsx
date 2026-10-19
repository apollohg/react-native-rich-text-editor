import './helpers/NativeRichTextEditorFixture';
import {
    createV2LocalHandle,
    renderUpdateValue,
    V2_INITIAL_DOC,
    V2_DOC_B,
    V2_DOC_C,
    counterAtomDefinition,
    atomBlock,
    installAtomRenderSource,
} from './helpers/NativeRichTextEditorFixture';

import { act, within } from '@testing-library/react-native';
import { create } from 'react-test-renderer';
import { NativeRichTextEditor } from '../NativeRichTextEditor';
import { createNativeEditorDocumentHandle, type DocumentJSON } from '../NativeEditorBridge';
import { acceptNativeCommitPayload } from '../RichTextEditorSerialization';
import { withAtomsSchema } from '../atoms';
import { tiptapCompatibleSchema } from '../schemas';
import { useNativeEditorDocument } from '../useNativeEditor';

const NATIVE_VIEW_TEST_ID = 'native-editor-view';
const BOLD_SCALAR_CARET = 1;
const ATOMIC_RENDER_SNAPSHOT_FIELD_COUNT = 9;
const ATOM_DOCUMENT_POSITION = 1;
const ATOM_LAYOUT_WIDTH = 200;
const SOURCE_ATOM_KEY = 'source-atom';
const REBOUND_ATOM_KEY = 'rebound-atom';
const SOURCE_ATOM_INITIAL_TITLE = 'source before';
const SOURCE_ATOM_INTERMEDIATE_TITLE = 'source intermediate';
const SOURCE_ATOM_COMMITTED_TITLE = 'source committed';
const REBOUND_ATOM_TITLE = 'rebound B';

const ACCEPTED_DOCUMENT_A: DocumentJSON = {
    type: 'doc',
    content: [ {
        type: 'paragraph',
        content: [ { type: 'text', text: 'accepted A', marks: [ { type: 'bold' } ] } ],
    } ],
};

type SynchronousRenderer = ReturnType<typeof create> & {
    unstable_flushSync(callback: () => void): void;
};

it.each([ 'history', 'html' ] as const)(
    'keeps newer content final when an accepted native commit reenters through its %s callback on the same handle',
    trigger => {
        const handle = createV2LocalHandle(V2_INITIAL_DOC);
        const onCommit = jest.fn();
        const onHtml = jest.fn();
        const onJson = jest.fn();
        let armed = false;
        let renderer!: SynchronousRenderer;

        function deliverNativeCommit(doc: DocumentJSON) {
            handle.bridge.replaceDocument({ setJson: doc, history: 'undoableBoundary' });

            const payload = {
                editorId: handle.editorId,
                documentRevision: handle.bridge.getState().documentRevision,
                updateJson: renderUpdateValue(handle.editorId),
            };

            expect(Object.keys(JSON.parse(payload.updateJson))).toHaveLength(ATOMIC_RENDER_SNAPSHOT_FIELD_COUNT);
            expect(acceptNativeCommitPayload(payload, handle.editorId, null)).not.toBeNull();
            within(renderer.root).getByTestId(NATIVE_VIEW_TEST_ID).props.onEditorUpdate({ nativeEvent: payload });
        }

        try {
            act(() => {
                renderer = create(
                    <NativeRichTextEditor
                        documentHandle={handle}
                        onLocalCommit={onCommit}
                        onContentChangeJSON={onJson}
                        onHistoryStateChange={history => {
                            if (trigger === 'history' && armed && history.canUndo) {
                                armed = false;
                                deliverNativeCommit(V2_DOC_B);
                            }
                        }}
                        onContentChange={html => {
                            onHtml(html);

                            if (trigger === 'html' && armed) {
                                armed = false;
                                deliverNativeCommit(V2_DOC_B);
                            }
                        }}
                    />
                ) as SynchronousRenderer;
            });

            const sourceHtml = '<p>accepted A</p>';
            const newerHtml = '<p>local b</p>';
            const followingHtml = '<p>local c</p>';
            onCommit.mockClear();
            onHtml.mockClear();
            onJson.mockClear();
            armed = true;

            act(() => deliverNativeCommit(ACCEPTED_DOCUMENT_A));

            expect(armed).toBe(false);
            expect(onCommit).toHaveBeenCalledTimes(2);
            expect(handle.bridge.getDocumentJson()).toEqual(V2_DOC_B);
            const callbacksAfterReentry = { html: onHtml.mock.calls.slice(), json: onJson.mock.calls.slice() };

            act(() => deliverNativeCommit(V2_DOC_C));

            const expectedFirstHtml = trigger === 'html' ? [ [ sourceHtml ] ] : [];

            expect({
                callbacksAfterReentry,
                commitCount: onCommit.mock.calls.length,
                html: onHtml.mock.calls,
                json: onJson.mock.calls,
            }).toEqual({
                callbacksAfterReentry: { html: [ ...expectedFirstHtml, [ newerHtml ] ], json: [ [ V2_DOC_B ] ] },
                commitCount: 3,
                html: [ ...expectedFirstHtml, [ newerHtml ], [ followingHtml ] ],
                json: [ [ V2_DOC_B ], [ V2_DOC_C ] ],
            });

            expect(handle.bridge.getDocumentJson()).toEqual(V2_DOC_C);
        } finally {
            if (renderer !== undefined) {
                act(() => renderer.unmount());
            }

            handle.destroy();
        }
    }
);

it('does not duplicate content when a public same-version refresh reenters its HTML callback', () => {
    const handle = createV2LocalHandle(V2_INITIAL_DOC);
    const onHtml = jest.fn();
    const onJson = jest.fn();
    let armed = false;
    let document!: ReturnType<typeof useNativeEditorDocument>;
    let renderer: ReturnType<typeof create> | undefined;

    function Subscriber() {
        document = useNativeEditorDocument({
            handle,
            onContentChangeJSON: onJson,
            onContentChange: html => {
                onHtml(html);

                if (armed) {
                    armed = false;
                    document.refresh();
                }
            },
        });

        return null;
    }

    try {
        act(() => {
            renderer = create(<Subscriber />);
        });

        handle.bridge.replaceDocument({ setJson: ACCEPTED_DOCUMENT_A, history: 'undoableBoundary' });
        onHtml.mockClear();
        onJson.mockClear();
        armed = true;

        act(() => document.refresh());

        expect(armed).toBe(false);
        const callbacksAfterReentry = { html: onHtml.mock.calls.slice(), json: onJson.mock.calls.slice() };
        handle.bridge.replaceDocument({ setJson: V2_DOC_C, history: 'undoableBoundary' });

        act(() => document.refresh());

        expect({ callbacksAfterReentry, html: onHtml.mock.calls, json: onJson.mock.calls }).toEqual({
            callbacksAfterReentry: { html: [ [ '<p>accepted A</p>' ] ], json: [ [ ACCEPTED_DOCUMENT_A ] ] },
            html: [ [ '<p>accepted A</p>' ], [ '<p>local c</p>' ] ],
            json: [ [ ACCEPTED_DOCUMENT_A ], [ V2_DOC_C ] ],
        });

        expect(document.getContentJson()).toEqual(V2_DOC_C);
    } finally {
        if (renderer !== undefined) {
            act(() => renderer.unmount());
        }

        handle.destroy();
    }
});

it('delivers an accepted commit to its source callback after a synchronous active-state rebind', () => {
    const handleA = createV2LocalHandle(V2_INITIAL_DOC);
    const handleB = createV2LocalHandle(V2_DOC_B);
    const onCommitA = jest.fn();
    const onCommitB = jest.fn();
    const order: string[] = [];
    let armed = false;
    let renderer!: SynchronousRenderer;

    const componentB = (
        <NativeRichTextEditor
            documentHandle={handleB}
            onLocalCommit={() => {
                order.push('commit B');
                onCommitB();
            }}
        />
    );

    try {
        act(() => {
            renderer = create(
                <NativeRichTextEditor
                    documentHandle={handleA}
                    onLocalCommit={() => {
                        order.push('commit A');
                        onCommitA();
                    }}
                    onActiveStateChange={state => {
                        if (!armed || !state.marks.bold) {
                            return;
                        }

                        armed = false;
                        order.push('active A');
                        renderer.unstable_flushSync(() => renderer.update(componentB));
                        expect(within(renderer.root).getByTestId(NATIVE_VIEW_TEST_ID).props.editorId).toBe(handleB.editorId);
                        expect(onCommitA).not.toHaveBeenCalled();
                        expect(onCommitB).not.toHaveBeenCalled();
                        order.push('bound B');
                    }}
                />
            ) as SynchronousRenderer;
        });

        handleA.bridge.replaceDocument({ setJson: ACCEPTED_DOCUMENT_A, history: 'undoableBoundary' });

        handleA.bridge.setSelection({
            baseDocumentRevision: handleA.bridge.getState().documentRevision,
            selection: { type: 'text',
                anchor: { kind: 'scalar', offset: BOLD_SCALAR_CARET },
                head: { kind: 'scalar', offset: BOLD_SCALAR_CARET } },
        });

        const payload = {
            editorId: handleA.editorId,
            documentRevision: handleA.bridge.getState().documentRevision,
            updateJson: renderUpdateValue(handleA.editorId),
        };

        const accepted = acceptNativeCommitPayload(payload, handleA.editorId, null);
        expect(accepted?.snapshot.activeState.marks.bold).toBe(true);
        expect(handleA.bridge.getDocumentJson()).toEqual(ACCEPTED_DOCUMENT_A);
        onCommitA.mockClear();
        onCommitB.mockClear();
        order.length = 0;
        armed = true;
        const sourceCommitHandler = within(renderer.root).getByTestId(NATIVE_VIEW_TEST_ID).props.onEditorUpdate;

        act(() => sourceCommitHandler({ nativeEvent: payload }));

        expect(order.slice(0, 2)).toEqual([ 'active A', 'bound B' ]);
        expect(within(renderer.root).getByTestId(NATIVE_VIEW_TEST_ID).props.editorId).toBe(handleB.editorId);

        expect({
            sourceCommitCount: onCommitA.mock.calls.length,
            reboundCommitCount: onCommitB.mock.calls.length,
            callbackOrder: order,
        }).toEqual({
            sourceCommitCount: 1,
            reboundCommitCount: 0,
            callbackOrder: [ 'active A', 'bound B', 'commit A' ],
        });

        expect(handleA.bridge.getDocumentJson()).toEqual(ACCEPTED_DOCUMENT_A);
        expect(handleB.bridge.getDocumentJson()).toEqual(V2_DOC_B);
    } finally {
        if (renderer !== undefined) {
            act(() => renderer.unmount());
        }

        handleA.destroy();
        handleB.destroy();
    }
});

it('stops obsolete content callbacks after a history callback synchronously rebinds while preserving later B updates', () => {
    const handleA = createV2LocalHandle(V2_INITIAL_DOC);
    const handleB = createV2LocalHandle(V2_DOC_B);
    const onCommitA = jest.fn();
    const onCommitB = jest.fn();
    const onHtmlA = jest.fn();
    const onJsonA = jest.fn();
    const onHtmlB = jest.fn();
    const onJsonB = jest.fn();
    const onHistoryA = jest.fn();
    let armed = false;
    let renderer!: SynchronousRenderer;

    const componentB = (
        <NativeRichTextEditor
            documentHandle={handleB}
            onLocalCommit={onCommitB}
            onContentChange={onHtmlB}
            onContentChangeJSON={onJsonB}
        />
    );

    try {
        act(() => {
            renderer = create(
                <NativeRichTextEditor
                    documentHandle={handleA}
                    onLocalCommit={onCommitA}
                    onContentChange={onHtmlA}
                    onContentChangeJSON={onJsonA}
                    onHistoryStateChange={history => {
                        onHistoryA(history);

                        if (!armed || !history.canUndo) {
                            return;
                        }

                        armed = false;
                        renderer.unstable_flushSync(() => renderer.update(componentB));
                        expect(within(renderer.root).getByTestId(NATIVE_VIEW_TEST_ID).props.editorId).toBe(handleB.editorId);
                        expect(onCommitA).not.toHaveBeenCalled();
                        expect(onHtmlA).not.toHaveBeenCalled();
                        expect(onJsonA).not.toHaveBeenCalled();
                        expect(onHtmlB).not.toHaveBeenCalled();
                        expect(onJsonB).not.toHaveBeenCalled();
                    }}
                />
            ) as SynchronousRenderer;
        });

        handleA.bridge.replaceDocument({ setJson: ACCEPTED_DOCUMENT_A, history: 'undoableBoundary' });

        const payloadA = {
            editorId: handleA.editorId,
            documentRevision: handleA.bridge.getState().documentRevision,
            updateJson: renderUpdateValue(handleA.editorId),
        };

        expect(acceptNativeCommitPayload(payloadA, handleA.editorId, null)).not.toBeNull();
        onHistoryA.mockClear();
        onCommitA.mockClear();
        onCommitB.mockClear();
        onHtmlA.mockClear();
        onJsonA.mockClear();
        onHtmlB.mockClear();
        onJsonB.mockClear();
        armed = true;
        const sourceCommitHandler = within(renderer.root).getByTestId(NATIVE_VIEW_TEST_ID).props.onEditorUpdate;

        act(() => sourceCommitHandler({ nativeEvent: payloadA }));

        expect(armed).toBe(false);
        expect(onHistoryA).toHaveBeenCalledTimes(1);
        expect(onHistoryA).toHaveBeenCalledWith({ canUndo: true, canRedo: false });
        expect(onCommitA).toHaveBeenCalledTimes(1);
        expect(onCommitB).not.toHaveBeenCalled();

        const callbacksAfterRebind = {
            sourceHtml: onHtmlA.mock.calls.slice(),
            sourceJson: onJsonA.mock.calls.slice(),
            reboundHtml: onHtmlB.mock.calls.slice(),
            reboundJson: onJsonB.mock.calls.slice(),
        };

        handleB.bridge.replaceDocument({ setJson: V2_DOC_C, history: 'undoableBoundary' });
        const expectedHtmlB = handleB.bridge.getDocumentHtml();

        const payloadB = {
            editorId: handleB.editorId,
            documentRevision: handleB.bridge.getState().documentRevision,
            updateJson: renderUpdateValue(handleB.editorId),
        };

        act(() => within(renderer.root).getByTestId(NATIVE_VIEW_TEST_ID).props.onEditorUpdate({ nativeEvent: payloadB }));

        expect({
            callbacksAfterRebind,
            sourceCommitCount: onCommitA.mock.calls.length,
            reboundCommitCount: onCommitB.mock.calls.length,
            genuineReboundHtml: onHtmlB.mock.calls,
            genuineReboundJson: onJsonB.mock.calls,
        }).toEqual({
            callbacksAfterRebind: { sourceHtml: [], sourceJson: [], reboundHtml: [], reboundJson: [] },
            sourceCommitCount: 1,
            reboundCommitCount: 1,
            genuineReboundHtml: [ [ expectedHtmlB ] ],
            genuineReboundJson: [ [ V2_DOC_C ] ],
        });

        expect(handleA.bridge.getDocumentJson()).toEqual(ACCEPTED_DOCUMENT_A);
        expect(handleB.bridge.getDocumentJson()).toEqual(V2_DOC_C);
    } finally {
        if (renderer !== undefined) {
            act(() => renderer.unmount());
        }

        handleA.destroy();
        handleB.destroy();
    }
});

it('stops the old JSON callback after its HTML callback synchronously rebinds A to B and back to A', () => {
    const handleA = createV2LocalHandle(V2_INITIAL_DOC);
    const handleB = createV2LocalHandle(V2_DOC_B);
    const onCommitA = jest.fn();
    const onCommitB = jest.fn();
    const onCommitRestoredA = jest.fn();
    const onHtmlA = jest.fn();
    const onJsonA = jest.fn();
    const onHtmlB = jest.fn();
    const onJsonB = jest.fn();
    const onHtmlRestoredA = jest.fn();
    const onJsonRestoredA = jest.fn();
    let armed = false;
    let renderer!: SynchronousRenderer;

    const componentB = (
        <NativeRichTextEditor documentHandle={handleB} onLocalCommit={onCommitB}
            onContentChange={onHtmlB} onContentChangeJSON={onJsonB}
        />
    );

    const restoredComponentA = (
        <NativeRichTextEditor documentHandle={handleA} onLocalCommit={onCommitRestoredA}
            onContentChange={onHtmlRestoredA} onContentChangeJSON={onJsonRestoredA}
        />
    );

    try {
        act(() => {
            renderer = create(
                <NativeRichTextEditor
                    documentHandle={handleA}
                    onLocalCommit={onCommitA}
                    onContentChangeJSON={onJsonA}
                    onContentChange={html => {
                        onHtmlA(html);

                        if (!armed) {
                            return;
                        }

                        armed = false;
                        renderer.unstable_flushSync(() => renderer.update(componentB));
                        expect(within(renderer.root).getByTestId(NATIVE_VIEW_TEST_ID).props.editorId).toBe(handleB.editorId);
                        renderer.unstable_flushSync(() => renderer.update(restoredComponentA));
                        expect(within(renderer.root).getByTestId(NATIVE_VIEW_TEST_ID).props.editorId).toBe(handleA.editorId);
                        expect(onCommitA).not.toHaveBeenCalled();
                        expect(onJsonA).not.toHaveBeenCalled();
                        expect(onHtmlRestoredA).not.toHaveBeenCalled();
                        expect(onJsonRestoredA).not.toHaveBeenCalled();
                    }}
                />
            ) as SynchronousRenderer;
        });

        handleA.bridge.replaceDocument({ setJson: ACCEPTED_DOCUMENT_A, history: 'undoableBoundary' });
        const expectedSourceHtml = handleA.bridge.getDocumentHtml();

        const payloadA = {
            editorId: handleA.editorId,
            documentRevision: handleA.bridge.getState().documentRevision,
            updateJson: renderUpdateValue(handleA.editorId),
        };

        expect(acceptNativeCommitPayload(payloadA, handleA.editorId, null)).not.toBeNull();
        armed = true;
        const sourceCommitHandler = within(renderer.root).getByTestId(NATIVE_VIEW_TEST_ID).props.onEditorUpdate;

        act(() => sourceCommitHandler({ nativeEvent: payloadA }));

        expect(armed).toBe(false);
        expect(onCommitA).toHaveBeenCalledTimes(1);
        expect(onCommitB).not.toHaveBeenCalled();
        expect(onCommitRestoredA).not.toHaveBeenCalled();
        expect(onHtmlA).toHaveBeenCalledTimes(1);
        expect(onHtmlA).toHaveBeenCalledWith(expectedSourceHtml);

        const callbacksAfterRebind = {
            sourceJson: onJsonA.mock.calls.slice(),
            intermediateHtml: onHtmlB.mock.calls.slice(),
            intermediateJson: onJsonB.mock.calls.slice(),
            restoredHtml: onHtmlRestoredA.mock.calls.slice(),
            restoredJson: onJsonRestoredA.mock.calls.slice(),
        };

        handleA.bridge.replaceDocument({ setJson: V2_DOC_C, history: 'undoableBoundary' });
        const expectedRestoredHtml = handleA.bridge.getDocumentHtml();

        const payloadRestoredA = {
            editorId: handleA.editorId,
            documentRevision: handleA.bridge.getState().documentRevision,
            updateJson: renderUpdateValue(handleA.editorId),
        };

        act(() => within(renderer.root).getByTestId(NATIVE_VIEW_TEST_ID).props.onEditorUpdate({ nativeEvent: payloadRestoredA }));

        expect({
            callbacksAfterRebind,
            sourceCommitCount: onCommitA.mock.calls.length,
            restoredCommitCount: onCommitRestoredA.mock.calls.length,
            restoredHtml: onHtmlRestoredA.mock.calls,
            restoredJson: onJsonRestoredA.mock.calls,
        }).toEqual({
            callbacksAfterRebind: {
                sourceJson: [], intermediateHtml: [], intermediateJson: [], restoredHtml: [], restoredJson: [], 
            },
            sourceCommitCount: 1,
            restoredCommitCount: 1,
            restoredHtml: [ [ expectedRestoredHtml ] ],
            restoredJson: [ [ V2_DOC_C ] ],
        });

        expect(handleA.bridge.getDocumentJson()).toEqual(V2_DOC_C);
        expect(handleB.bridge.getDocumentJson()).toEqual(V2_DOC_B);
    } finally {
        if (renderer !== undefined) {
            act(() => renderer.unmount());
        }

        handleA.destroy();
        handleB.destroy();
    }
});

it('preserves rebound atoms and selection when an accepted native commit synchronously rebinds during render fallback', () => {
    const { definition } = counterAtomDefinition();
    const schema = withAtomsSchema(tiptapCompatibleSchema, [ definition ]);
    const documentA = definition.buildFragmentJson({ title: SOURCE_ATOM_INITIAL_TITLE });
    const documentB = definition.buildFragmentJson({ title: REBOUND_ATOM_TITLE });

    const handleA = createNativeEditorDocumentHandle({
        schema,
        initialization: { type: 'localJson', json: documentA },
    });

    const handleB = createNativeEditorDocumentHandle({
        schema,
        initialization: { type: 'localJson', json: documentB },
    });

    const onCommitA = jest.fn();
    const onCommitB = jest.fn();
    const onSelectionB = jest.fn();
    const order: string[] = [];
    const initialBlocksA = atomBlock(definition.name, ATOM_DOCUMENT_POSITION, SOURCE_ATOM_KEY);
    initialBlocksA[0][0].attrs = { title: SOURCE_ATOM_INITIAL_TITLE };
    const committedBlocksA = atomBlock(definition.name, ATOM_DOCUMENT_POSITION, SOURCE_ATOM_KEY);
    committedBlocksA[0][0].attrs = { title: SOURCE_ATOM_COMMITTED_TITLE };
    const blocksB = atomBlock(definition.name, ATOM_DOCUMENT_POSITION, REBOUND_ATOM_KEY);
    blocksB[0][0].attrs = { title: REBOUND_ATOM_TITLE };
    let renderSource = { renderBlocks: initialBlocksA, renderPatch: null };
    let armed = false;
    let renderer!: SynchronousRenderer;
    const selectionB = { type: 'node', pos: ATOM_DOCUMENT_POSITION } as const;

    const componentB = (
        <NativeRichTextEditor
            documentHandle={handleB}
            atoms={[ definition ]}
            onLocalCommit={onCommitB}
            onSelectionChange={onSelectionB}
        />
    );

    installAtomRenderSource(() => {
        const source = renderSource;

        if (armed) {
            armed = false;
            order.push('fallback A');
            renderSource = { renderBlocks: blocksB, renderPatch: null };
            renderer.unstable_flushSync(() => renderer.update(componentB));

            renderer.unstable_flushSync(() => {
                const viewB = within(renderer.root).getByTestId(NATIVE_VIEW_TEST_ID);
                expect(viewB.props.editorId).toBe(handleB.editorId);

                viewB.props.onAtomLayout({ nativeEvent: {
                    editorId: handleB.editorId,
                    width: ATOM_LAYOUT_WIDTH,
                    positions: [ { key: REBOUND_ATOM_KEY, x: 0, y: 0 } ],
                } });

                viewB.props.onSelectionChange({ nativeEvent: {
                    editorId: handleB.editorId,
                    anchor: 0,
                    head: ATOM_DOCUMENT_POSITION,
                    stateJson: JSON.stringify({
                        documentVersion: handleB.bridge.getState().documentRevision,
                        stateRevision: handleB.bridge.getState().stateRevision,
                        selection: selectionB,
                    }),
                } });
            });

            expect(within(renderer.root).getByTestId('counter-atom').props.atomProps).toMatchObject({
                attrs: { title: REBOUND_ATOM_TITLE },
                selected: true,
            });

            expect(onCommitA).not.toHaveBeenCalled();
            expect(onCommitB).not.toHaveBeenCalled();
            order.push('bound B');
        }

        return source;
    });

    try {
        act(() => {
            renderer = create(
                <NativeRichTextEditor
                    documentHandle={handleA}
                    atoms={[ definition ]}
                    onLocalCommit={() => {
                        order.push('commit A');
                        onCommitA();
                    }}
                />
            ) as SynchronousRenderer;
        });

        const seededRevision = handleA.bridge.getState().documentRevision;

        handleA.bridge.replaceDocument({
            setJson: definition.buildFragmentJson({ title: SOURCE_ATOM_INTERMEDIATE_TITLE }),
            history: 'undoableBoundary',
        });

        const missedRenderBase = handleA.bridge.getState().documentRevision;
        const committedDocumentA = definition.buildFragmentJson({ title: SOURCE_ATOM_COMMITTED_TITLE });
        handleA.bridge.replaceDocument({ setJson: committedDocumentA, history: 'undoableBoundary' });
        renderSource = { renderBlocks: committedBlocksA, renderPatch: null };
        const snapshot = JSON.parse(renderUpdateValue(handleA.editorId)) as Record<string, unknown>;
        snapshot.renderBlocks = null;

        snapshot.renderPatch = {
            baseDocumentVersion: missedRenderBase,
            startIndex: 0,
            deleteCount: initialBlocksA.length,
            renderBlocks: committedBlocksA,
        };

        const payload = {
            editorId: handleA.editorId,
            documentRevision: handleA.bridge.getState().documentRevision,
            updateJson: JSON.stringify(snapshot),
        };

        expect(Object.keys(snapshot)).toHaveLength(ATOMIC_RENDER_SNAPSHOT_FIELD_COUNT);
        expect(acceptNativeCommitPayload(payload, handleA.editorId, null)).not.toBeNull();
        expect(missedRenderBase).not.toBe(seededRevision);
        onCommitA.mockClear();
        onCommitB.mockClear();
        onSelectionB.mockClear();
        order.length = 0;
        armed = true;
        const sourceCommitHandler = within(renderer.root).getByTestId(NATIVE_VIEW_TEST_ID).props.onEditorUpdate;

        act(() => sourceCommitHandler({ nativeEvent: payload }));

        expect(order).toEqual([ 'fallback A', 'bound B', 'commit A' ]);
        expect(onCommitA).toHaveBeenCalledTimes(1);
        expect(onCommitB).not.toHaveBeenCalled();
        expect(onSelectionB).toHaveBeenCalledTimes(1);
        expect(onSelectionB).toHaveBeenLastCalledWith(selectionB);
        expect(within(renderer.root).getByTestId(NATIVE_VIEW_TEST_ID).props.editorId).toBe(handleB.editorId);

        expect(within(renderer.root).getAllByTestId('counter-atom').map(atom => ({
            attrs: atom.props.atomProps.attrs,
            selected: atom.props.atomProps.selected,
        }))).toEqual([ { attrs: { title: REBOUND_ATOM_TITLE }, selected: true } ]);

        expect(handleA.bridge.getDocumentJson()).toEqual(committedDocumentA);
        expect(handleB.bridge.getDocumentJson()).toEqual(documentB);
    } finally {
        if (renderer !== undefined) {
            act(() => renderer.unmount());
        }

        handleA.destroy();
        handleB.destroy();
    }
});
