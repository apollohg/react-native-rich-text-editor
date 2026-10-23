import './helpers/NativeRichTextEditorFixture';
import {
    mockNativeModule,
    V2_TRANSPORT_URL,
    v2Runtime,
    V2_INITIAL_DOC,
    V2_DOC_B,
    V2_DOC_C,
    createV2RoomHandle,
    createV2LocalHandle,
    renderUpdateValue,
} from './helpers/NativeRichTextEditorFixture';

import { render, act } from '@testing-library/react-native';
import { NativeRichTextEditor } from '../NativeRichTextEditor';
import { type DocumentJSON } from '../NativeEditorBridge';

import { useYjsCollaboration } from '../YjsCollaboration';
import { V2_FAKE_STEP2_FRAME, V2_FAKE_UPDATE_FRAME } from './helpers/nativeEditorV2Fake';
import { acceptNativeCommitPayload } from '../RichTextEditorSerialization';
import { normalizeNativeEditorV2RenderUpdateValue } from '../NativeEditorRenderNormalization';

const capturedPixelFrame = require('./helpers/nativeCommitPixelFrame.json') as Record<string, unknown>;
const { positionEpoch: _capturedEpoch, ...capturedPixelCommitSnapshot } = capturedPixelFrame;

const capturedQueuedSelection = require('./helpers/nativeQueuedSelectionProof.json') as {
    historicalPublicSnapshot: string;
    newerSelectionStateJson: string;
    newerSelectionPayload: { anchor: number; head: number; editorId: string; documentVersion: string; stateJson: string };
    commitStateRevision: string;
    selectionStateRevision: string;
};

const QUEUED_SELECTION_INITIAL_DOCUMENT: DocumentJSON = { type: 'doc',
    content: [ { type: 'paragraph',
        content: [
            { type: 'text', text: 'Bold', marks: [ { type: 'bold' } ] },
            { type: 'text', text: ' plain' },
        ] } ] };

const QUEUED_SELECTION_COMMITTED_DOCUMENT: DocumentJSON = { type: 'doc',
    content: [ { type: 'paragraph',
        content: [
            { type: 'text', text: 'BoXld', marks: [ { type: 'bold' } ] },
            { type: 'text', text: ' plain' },
        ] } ] };

function prepareQueuedNativeCommit(handle: ReturnType<typeof createV2LocalHandle>) {
    const historicalSelection = (JSON.parse(capturedQueuedSelection.historicalPublicSnapshot) as {
        selection: { anchorScalar: number };
    }).selection;

    const initialNativeScalarCaret = 0;
    const insertionScalarLength = Array.from('X').length;
    const initialScalarCaret = historicalSelection.anchorScalar - insertionScalarLength;

    for (const offset of [ initialNativeScalarCaret, initialScalarCaret ]) {
        handle.bridge.setSelection({ baseDocumentRevision: handle.bridge.getState().documentRevision,
            selection: { type: 'text', anchor: { kind: 'scalar', offset }, head: { kind: 'scalar', offset } } });
    }

    handle.bridge.replaceDocument({ setJson: QUEUED_SELECTION_COMMITTED_DOCUMENT, history: 'undoableBoundary' });
    expect(handle.bridge.getState().documentRevision).toBe(capturedQueuedSelection.newerSelectionPayload.documentVersion);
    expect(handle.bridge.getState().stateRevision).toBe(capturedQueuedSelection.commitStateRevision);
}

describe('NativeRichTextEditor (v2 document mode)', () => {
    it('keeps a newer native selection active while delivering its queued historical commit', () => {
        const handle = createV2LocalHandle(QUEUED_SELECTION_INITIAL_DOCUMENT);
        const onLocalCommit = jest.fn();
        const onActiveStateChange = jest.fn();
        const onSelectionChange = jest.fn();

        const { getByTestId, getByLabelText } = render(
            <NativeRichTextEditor documentHandle={handle} toolbarPlacement={'inline'}
                onLocalCommit={onLocalCommit} onActiveStateChange={onActiveStateChange}
                onSelectionChange={onSelectionChange}
            />
        );

        const historical = normalizeNativeEditorV2RenderUpdateValue(capturedQueuedSelection.historicalPublicSnapshot);

        const newerSelection = (JSON.parse(capturedQueuedSelection.newerSelectionStateJson) as {
            selection: { anchorScalar: number; headScalar: number };
        }).selection;

        expect(historical).not.toBeNull();
        prepareQueuedNativeCommit(handle);
        expect(handle.bridge.getState().documentRevision).toBe(historical!.documentVersion);

        handle.bridge.setSelection({ baseDocumentRevision: historical!.documentVersion,
            selection: { type: 'text',
                anchor: { kind: 'scalar', offset: newerSelection.anchorScalar },
                head: { kind: 'scalar', offset: newerSelection.headScalar } } });

        expect(handle.bridge.getState().stateRevision).toBe(capturedQueuedSelection.selectionStateRevision);
        expect(BigInt(capturedQueuedSelection.selectionStateRevision)).toBeGreaterThan(BigInt(capturedQueuedSelection.commitStateRevision));

        act(() => {
            getByTestId('native-editor-view').props.onSelectionChange({ nativeEvent: {
                ...capturedQueuedSelection.newerSelectionPayload, editorId: handle.editorId,
            } });
        });

        expect(getByLabelText('Bold').props.accessibilityState.selected).toBe(false);

        expect(onSelectionChange).toHaveBeenLastCalledWith(expect.objectContaining({
            anchor: capturedQueuedSelection.newerSelectionPayload.anchor,
            head: capturedQueuedSelection.newerSelectionPayload.head,
        }));

        onLocalCommit.mockClear();
        onActiveStateChange.mockClear();

        const payload = { editorId: handle.editorId,
            documentRevision: historical!.documentVersion,
            updateJson: capturedQueuedSelection.historicalPublicSnapshot };

        expect(acceptNativeCommitPayload(payload, handle.editorId, null)?.snapshot).toEqual(historical);
        act(() => getByTestId('native-editor-view').props.onEditorUpdate({ nativeEvent: payload }));
        expect(onLocalCommit).toHaveBeenCalledTimes(1);
        expect(handle.bridge.getDocumentJson()).toEqual(QUEUED_SELECTION_COMMITTED_DOCUMENT);
        expect(getByLabelText('Bold').props.accessibilityState.selected).toBe(false);
        expect(onActiveStateChange).not.toHaveBeenCalled();
        act(() => getByTestId('native-editor-view').props.onEditorUpdate({ nativeEvent: payload }));
        expect(onLocalCommit).toHaveBeenCalledTimes(1);
        handle.destroy();
    });

    it('notifies the accepted source commit when its active-state callback rebinds to another handle', () => {
        const handleA = createV2LocalHandle(QUEUED_SELECTION_INITIAL_DOCUMENT);
        const handleB = createV2LocalHandle(V2_DOC_B);
        const onCommitA = jest.fn();
        const onCommitB = jest.fn();
        let armed = false;

        const view = render(
            <NativeRichTextEditor
                documentHandle={handleA}
                onLocalCommit={onCommitA}
                onActiveStateChange={state => {
                    if (armed && state.marks.bold) {
                        armed = false;
                        view.rerender(<NativeRichTextEditor documentHandle={handleB} onLocalCommit={onCommitB} />);
                    }
                }}
            />
        );

        prepareQueuedNativeCommit(handleA);
        onCommitA.mockClear();
        onCommitB.mockClear();
        armed = true;

        act(() => view.getByTestId('native-editor-view').props.onEditorUpdate({ nativeEvent: {
            editorId: handleA.editorId,
            documentRevision: capturedQueuedSelection.newerSelectionPayload.documentVersion,
            updateJson: capturedQueuedSelection.historicalPublicSnapshot,
        } }));

        expect(view.getByTestId('native-editor-view').props.editorId).toBe(handleB.editorId);
        expect(onCommitA).toHaveBeenCalledTimes(1);
        expect(onCommitB).not.toHaveBeenCalled();
        expect(handleA.bridge.getDocumentJson()).toEqual(QUEUED_SELECTION_COMMITTED_DOCUMENT);
        expect(handleB.bridge.getDocumentJson()).toEqual(V2_DOC_B);
        handleA.destroy();
        handleB.destroy();
    });

    it('stops an unversioned selection after its active-state callback delivers a newer versioned selection', () => {
        const handle = createV2LocalHandle(QUEUED_SELECTION_INITIAL_DOCUMENT);
        const onSelectionChange = jest.fn();
        let armed = false;

        const view = render(
            <NativeRichTextEditor
                documentHandle={handle}
                onSelectionChange={onSelectionChange}
                onActiveStateChange={state => {
                    if (armed && state.marks.bold) {
                        armed = false;

                        view.getByTestId('native-editor-view').props.onSelectionChange({ nativeEvent: {
                            ...capturedQueuedSelection.newerSelectionPayload, editorId: handle.editorId,
                        } });
                    }
                }}
            />
        );

        prepareQueuedNativeCommit(handle);
        const historical = normalizeNativeEditorV2RenderUpdateValue(capturedQueuedSelection.historicalPublicSnapshot)!;

        const olderProjection = { documentVersion: historical.documentVersion,
            selection: historical.selection,
            activeState: historical.activeState,
            historyState: historical.historyState };

        expect(historical.selection.type).toBe('text');
        const olderSelection = historical.selection as { anchor: number; head: number };
        armed = true;
        onSelectionChange.mockClear();

        act(() => view.getByTestId('native-editor-view').props.onSelectionChange({ nativeEvent: {
            editorId: handle.editorId,
            anchor: olderSelection.anchor,
            head: olderSelection.head,
            stateJson: JSON.stringify(olderProjection),
        } }));

        expect(onSelectionChange).toHaveBeenCalledTimes(1);

        expect(onSelectionChange).toHaveBeenLastCalledWith(expect.objectContaining({
            anchor: capturedQueuedSelection.newerSelectionPayload.anchor,
            head: capturedQueuedSelection.newerSelectionPayload.head,
        }));

        handle.destroy();
    });

    it('carries parsed scalar coordinates from a selection-only projection through a focused rebind', () => {
        const handleA = createV2LocalHandle(QUEUED_SELECTION_INITIAL_DOCUMENT);
        const handleB = createV2LocalHandle(QUEUED_SELECTION_COMMITTED_DOCUMENT);
        const view = render(<NativeRichTextEditor documentHandle={handleA} />);
        const nativeView = view.getByTestId('native-editor-view');
        prepareQueuedNativeCommit(handleA);

        const newer = JSON.parse(capturedQueuedSelection.newerSelectionStateJson) as {
            selection: { anchor: number; head: number; anchorScalar: number; headScalar: number };
        };

        act(() => {
            nativeView.props.onFocusChange({ nativeEvent: { editorId: handleA.editorId, isFocused: true } });

            nativeView.props.onSelectionChange({ nativeEvent: {
                ...capturedQueuedSelection.newerSelectionPayload,
                editorId: handleA.editorId,
                stateJson: JSON.stringify({ selection: newer.selection }),
            } });
        });

        mockNativeModule.editorV2SetSelection.mockClear();
        view.rerender(<NativeRichTextEditor documentHandle={handleB} />);
        expect(view.getByTestId('native-editor-view')).toBe(nativeView);
        const request = JSON.parse(mockNativeModule.editorV2SetSelection.mock.calls.at(-1)![1] as string);

        expect(request).toMatchObject({ baseDocumentRevision: handleB.bridge.getState().documentRevision,
            selection: { type: 'text',
                anchor: { kind: 'scalar', offset: newer.selection.anchorScalar },
                head: { kind: 'scalar', offset: newer.selection.headScalar } } });

        handleA.destroy();
        handleB.destroy();
    });

    it.each([ 'commit', 'selection' ] as const)(
        'rejects a saved source %s callback after rebinding while accepting the current handle', kind => {
            const handleA = createV2LocalHandle(QUEUED_SELECTION_INITIAL_DOCUMENT);
            const handleB = createV2LocalHandle(QUEUED_SELECTION_INITIAL_DOCUMENT);
            const onCommitA = jest.fn();
            const onCommitB = jest.fn();
            const onSelectionB = jest.fn();
            const view = render(<NativeRichTextEditor documentHandle={handleA} onLocalCommit={onCommitA} />);
            const original = view.getByTestId('native-editor-view');
            const previousCommit = original.props.onEditorUpdate;
            const previousSelection = original.props.onSelectionChange;
            prepareQueuedNativeCommit(handleA);

            view.rerender(
                <NativeRichTextEditor
                    documentHandle={handleB}
                    onLocalCommit={onCommitB}
                    onSelectionChange={onSelectionB}
                />
            );

            onCommitA.mockClear();
            onCommitB.mockClear();
            onSelectionB.mockClear();

            const commit = { documentRevision: capturedQueuedSelection.newerSelectionPayload.documentVersion,
                updateJson: capturedQueuedSelection.historicalPublicSnapshot };

            const initialNativeScalarCaret = 0;

            act(() => {
                if (kind === 'commit') {
                    previousCommit({ nativeEvent: { ...commit, editorId: handleA.editorId } });
                } else {
                    previousSelection({ nativeEvent: { editorId: handleA.editorId,
                        anchor: initialNativeScalarCaret,
                        head: initialNativeScalarCaret } });
                }
            });

            expect(view.getByTestId('native-editor-view').props.editorId).toBe(handleB.editorId);
            expect(onCommitA).not.toHaveBeenCalled();
            expect(onCommitB).not.toHaveBeenCalled();
            expect(onSelectionB).not.toHaveBeenCalled();

            act(() => {
                if (kind === 'commit') {
                    prepareQueuedNativeCommit(handleB);

                    view.getByTestId('native-editor-view').props.onEditorUpdate({ nativeEvent: {
                        ...commit, editorId: handleB.editorId,
                    } });
                } else {
                    view.getByTestId('native-editor-view').props.onSelectionChange({ nativeEvent: {
                        editorId: handleB.editorId, anchor: initialNativeScalarCaret, head: initialNativeScalarCaret,
                    } });
                }
            });

            expect(kind === 'commit' ? onCommitB : onSelectionB).toHaveBeenCalledTimes(1);
            handleA.destroy();
            handleB.destroy();
        }
    );

    it.each([ null, 4, '04', '18446744073709551616' ])(
        'rejects a malformed selection state revision %p before changing presentation or notifying', stateRevision => {
            const handle = createV2LocalHandle(V2_INITIAL_DOC);
            const onSelectionChange = jest.fn();
            const onActiveStateChange = jest.fn();

            const { getByTestId, getByLabelText } = render(
                <NativeRichTextEditor documentHandle={handle} toolbarPlacement={'inline'}
                    onSelectionChange={onSelectionChange} onActiveStateChange={onActiveStateChange}
                />
            );

            const historical = normalizeNativeEditorV2RenderUpdateValue(capturedQueuedSelection.historicalPublicSnapshot);
            expect(historical).not.toBeNull();
            expect(getByLabelText('Bold').props.accessibilityState.selected).toBe(false);
            onActiveStateChange.mockClear();
            const projection = JSON.parse(capturedQueuedSelection.newerSelectionStateJson) as Record<string, unknown>;

            act(() => {
                getByTestId('native-editor-view').props.onSelectionChange({ nativeEvent: {
                    ...capturedQueuedSelection.newerSelectionPayload,
                    editorId: handle.editorId,
                    stateJson: JSON.stringify({ ...projection, stateRevision, activeState: historical!.activeState }),
                } });
            });

            expect(getByLabelText('Bold').props.accessibilityState.selected).toBe(false);
            expect(onSelectionChange).not.toHaveBeenCalled();
            expect(onActiveStateChange).not.toHaveBeenCalled();

            act(() => {
                getByTestId('native-editor-view').props.onSelectionChange({ nativeEvent: {
                    ...capturedQueuedSelection.newerSelectionPayload, editorId: handle.editorId,
                } });
            });

            expect(onSelectionChange).toHaveBeenCalledTimes(1);
            handle.destroy();
        }
    );

    it('accepts the corrected physical Pixel commit snapshot with the exact public fields', () => {
        const updateJson = JSON.stringify(capturedPixelCommitSnapshot);
        const payload = { editorId: '1', documentRevision: capturedPixelFrame.documentVersion as string, updateJson };
        expect(normalizeNativeEditorV2RenderUpdateValue(updateJson)).toEqual(capturedPixelCommitSnapshot);
        const accepted = acceptNativeCommitPayload(payload, payload.editorId, null);
        expect(accepted).toEqual({ documentRevision: payload.documentRevision, snapshot: capturedPixelCommitSnapshot });

        expect(accepted?.snapshot.selection).toEqual({
            type: 'text', anchor: 770, head: 770, anchorScalar: 765, headScalar: 765,
        });

        expect(accepted?.snapshot.renderPatch?.renderBlocks[0]).toContainEqual({ type: 'textRun', text: 'bold e!mphasis', marks: [ 'bold' ] });
        expect(accepted?.snapshot.activeState.marks.bold).toBe(true);
        expect(accepted?.snapshot.historyState).toEqual({ canUndo: true, canRedo: false });
        expect(Object.isFrozen(accepted?.snapshot)).toBe(true);
        expect(Object.isFrozen(accepted?.snapshot.renderPatch?.renderBlocks)).toBe(true);
        expect(capturedPixelFrame.positionEpoch).toBe('5');
    });

    it.each([ null,
        '',
        '05',
        '+5',
        '5.0',
        '-1',
        '18446744073709551616',
        '5',
        '0',
        '18446744073709551615',
        5,
        true,
        {} ])(
        'rejects leaked private positionEpoch %p without accepting the committed revision', epoch => {
            const payload = { editorId: '1',
                documentRevision: capturedPixelFrame.documentVersion as string,
                updateJson: JSON.stringify({ ...capturedPixelFrame, positionEpoch: epoch }) };

            expect(acceptNativeCommitPayload(payload, payload.editorId, null)).toBeNull();
        }
    );

    it('retains unknown-field, duplicate and stale commit snapshot rejection', () => {
        const payload = { editorId: '1',
            documentRevision: capturedPixelFrame.documentVersion as string,
            updateJson: JSON.stringify(capturedPixelCommitSnapshot) };

        expect(acceptNativeCommitPayload({ ...payload,
            updateJson: JSON.stringify({ ...capturedPixelCommitSnapshot, unexpectedNativeField: true }) }, payload.editorId, null)).toBeNull();

        expect(acceptNativeCommitPayload(payload, payload.editorId, payload.documentRevision)).toBeNull();
        expect(acceptNativeCommitPayload(payload, payload.editorId, '3')).toBeNull();
    });

    it('does not roll back a newer native commit when a prior controlled echo renders late', () => {
        const handle = createV2LocalHandle(V2_INITIAL_DOC);
        const onContentChange = jest.fn();

        const { getByTestId, rerender } = render(
            <NativeRichTextEditor
                documentHandle={handle}
                value={'<p>hello</p>'}
                onContentChange={onContentChange}
            />
        );

        mockNativeModule.editorV2ApplyLocalApi.mockClear();

        const commit = (html: string) => {
            handle.bridge.replaceDocument({ setHtml: html, history: 'undoableBoundary' });

            getByTestId('native-editor-view').props.onEditorUpdate({
                nativeEvent: {
                    editorId: handle.editorId,
                    updateJson: renderUpdateValue(handle.editorId),
                    documentRevision: handle.bridge.getState().documentRevision,
                },
            });
        };

        act(() => commit('<p>hello!</p>'));
        expect(onContentChange).toHaveBeenLastCalledWith('<p>hello!</p>');
        act(() => commit('<p>hello! </p>'));
        expect(onContentChange).toHaveBeenLastCalledWith('<p>hello! </p>');
        mockNativeModule.editorV2ApplyLocalApi.mockClear();

        rerender(
            <NativeRichTextEditor
                documentHandle={handle}
                value={'<p>hello!</p>'}
                onContentChange={onContentChange}
            />
        );

        expect(mockNativeModule.editorV2ApplyLocalApi).not.toHaveBeenCalled();
        expect(handle.bridge.getContentSnapshot().html).toBe('<p>hello! </p>');
        expect(getByTestId('native-editor-view').props.editorUpdateJson).toBeUndefined();

        rerender(
            <NativeRichTextEditor
                documentHandle={handle}
                value={'<p>hello! </p>'}
                onContentChange={onContentChange}
            />
        );

        expect(mockNativeModule.editorV2ApplyLocalApi).not.toHaveBeenCalled();

        rerender(
            <NativeRichTextEditor
                documentHandle={handle}
                value={'<p>hello!</p>'}
                onContentChange={onContentChange}
            />
        );

        expect(mockNativeModule.editorV2ApplyLocalApi).toHaveBeenCalledTimes(1);
        expect(handle.bridge.getContentSnapshot().html).toBe('<p>hello!</p>');
        handle.destroy();
    });

    it('does not roll back a newer native commit when a prior controlled JSON echo renders late', () => {
        const handle = createV2LocalHandle(V2_INITIAL_DOC);
        const onContentChangeJSON = jest.fn();

        const { getByTestId, rerender } = render(
            <NativeRichTextEditor
                documentHandle={handle}
                valueJSON={V2_INITIAL_DOC}
                onContentChangeJSON={onContentChangeJSON}
            />
        );

        mockNativeModule.editorV2ApplyLocalApi.mockClear();

        const commit = (json: DocumentJSON) => {
            handle.bridge.replaceDocument({ setJson: json, history: 'undoableBoundary' });

            getByTestId('native-editor-view').props.onEditorUpdate({
                nativeEvent: {
                    editorId: handle.editorId,
                    updateJson: renderUpdateValue(handle.editorId),
                    documentRevision: handle.bridge.getState().documentRevision,
                },
            });
        };

        act(() => commit(V2_DOC_B));
        act(() => commit(V2_DOC_C));

        rerender(
            <NativeRichTextEditor
                documentHandle={handle}
                valueJSON={V2_DOC_B}
                onContentChangeJSON={onContentChangeJSON}
            />
        );

        expect(mockNativeModule.editorV2ApplyLocalApi).not.toHaveBeenCalled();
        expect(handle.bridge.getContentSnapshot().json).toEqual(V2_DOC_C);

        rerender(
            <NativeRichTextEditor
                documentHandle={handle}
                valueJSON={V2_DOC_C}
                onContentChangeJSON={onContentChangeJSON}
            />
        );

        rerender(
            <NativeRichTextEditor
                documentHandle={handle}
                valueJSON={V2_DOC_B}
                onContentChangeJSON={onContentChangeJSON}
            />
        );

        expect(mockNativeModule.editorV2ApplyLocalApi).toHaveBeenCalledTimes(1);
        expect(handle.bridge.getContentSnapshot().json).toEqual(V2_DOC_B);
        handle.destroy();
    });

    it('emits content changes for incremental native commit snapshots', () => {
        const handle = createV2LocalHandle(V2_INITIAL_DOC);
        const onContentChange = jest.fn();
        const onContentChangeJSON = jest.fn();
        const onActiveStateChange = jest.fn();
        const onHistoryStateChange = jest.fn();
        const onLocalCommit = jest.fn();

        const { getByTestId } = render(
            <NativeRichTextEditor documentHandle={handle} onContentChange={onContentChange}
                onContentChangeJSON={onContentChangeJSON} onActiveStateChange={onActiveStateChange}
                onHistoryStateChange={onHistoryStateChange} onLocalCommit={onLocalCommit}
            />
        );

        onHistoryStateChange.mockClear();
        onLocalCommit.mockClear();

        const baseDocumentVersion = handle.bridge.getState().documentRevision;

        v2Runtime.module.editorV2ApplyInput(
            handle.editorId,
            JSON.stringify({
                version: 1,
                requestId: '1',
                baseDocumentRevision: baseDocumentVersion,
                text: '!',
            })
        );

        const atomicUpdate = JSON.parse(renderUpdateValue(handle.editorId)) as Record<
            string,
            unknown
        >;

        const renderBlocks = atomicUpdate.renderBlocks as unknown[];
        atomicUpdate.renderBlocks = null;

        atomicUpdate.renderPatch = {
            baseDocumentVersion,
            startIndex: 0,
            deleteCount: renderBlocks.length,
            renderBlocks,
        };

        act(() => {
            getByTestId('native-editor-view').props.onEditorUpdate({
                nativeEvent: {
                    editorId: handle.editorId,
                    updateJson: JSON.stringify(atomicUpdate),
                    documentRevision: handle.bridge.getState().documentRevision,
                },
            });
        });

        expect(onContentChange).toHaveBeenCalledWith('<p>hello!</p>');
        expect(onContentChangeJSON).toHaveBeenCalledWith({ type: 'doc', content: [ { type: 'paragraph', content: [ { type: 'text', text: 'hello!' } ] } ] });
        expect(onHistoryStateChange).toHaveBeenCalledWith({ canUndo: true, canRedo: false });
        expect(onActiveStateChange).toHaveBeenLastCalledWith(atomicUpdate.activeState);
        expect(onLocalCommit).toHaveBeenCalledTimes(1);
        expect(mockNativeModule.editorV2GetContentSnapshot).toHaveBeenCalled();
        expect(getByTestId('native-editor-view').props.editorUpdateJson).toBeUndefined();
        handle.destroy();
    });

    it.each([ 'iOS', 'Android' ])(
        'accepts only an authentic canonical %s native commit payload and suppresses its exact echo',
        platform => {
            const handle = createV2LocalHandle(V2_INITIAL_DOC);
            const onContentChange = jest.fn();

            const { getByTestId, rerender } = render(
                <NativeRichTextEditor documentHandle={handle} onContentChange={onContentChange} />
            );

            const view = getByTestId('native-editor-view');

            const commit = (text: string) => {
                v2Runtime.module.editorV2ApplyInput(
                    handle.editorId,
                    JSON.stringify({
                        version: 1,
                        requestId: text === '!' ? '1' : '2',
                        baseDocumentRevision: handle.bridge.getState().documentRevision,
                        text,
                    })
                );

                const documentRevision = handle.bridge.getState().documentRevision;

                return {
                    editorId: handle.editorId,
                    documentRevision,
                    updateJson: renderUpdateValue(handle.editorId),
                };
            };

            const first = commit('!');

            const emit = (payload: Record<string, unknown>) =>
                view.props.onEditorUpdate({
                    // Keep both native property orderings covered: iOS emits
                    // identity first while Android builds its map update-first.
                    nativeEvent:
                        platform === 'iOS'
                            ? payload
                            : {
                                updateJson: payload.updateJson,
                                documentRevision: payload.documentRevision,
                                editorId: payload.editorId,
                            },
                });

            const mismatchedSnapshot = JSON.parse(first.updateJson) as Record<string, unknown>;
            mismatchedSnapshot.documentVersion = '0';

            const rejectedPayloads: Record<string, unknown>[] = [
                { ...first, editorId: '999' },
                { ...first, documentRevision: '01' },
                { ...first, documentRevision: '18446744073709551616' },
                { ...first, updateJson: '{}' },
                { ...first, updateJson: JSON.stringify(mismatchedSnapshot) },
            ];

            for (const rejected of rejectedPayloads) {
                act(() => emit(rejected));
            }

            expect(onContentChange).not.toHaveBeenCalled();

            act(() => emit(first));
            expect(onContentChange).toHaveBeenCalledTimes(1);

            // Duplicate native delivery must not refresh, notify, or replace
            // the one-shot echo token.
            act(() => emit(first));
            expect(onContentChange).toHaveBeenCalledTimes(1);

            // The identical revision signal is the sole suppressed echo.
            rerender(
                <NativeRichTextEditor
                    documentHandle={handle}
                    documentRevision={first.documentRevision}
                    onContentChange={onContentChange}
                />
            );

            expect(getByTestId('native-editor-view').props.editorUpdateJson).toBeUndefined();

            // A different/newer external revision clears that token and is
            // pushed, rather than being hidden behind native revision N.
            handle.bridge.replaceDocument({ setJson: V2_DOC_B, history: 'undoableBoundary' });
            const externalRevision = handle.bridge.getState().documentRevision;

            rerender(
                <NativeRichTextEditor
                    documentHandle={handle}
                    documentRevision={externalRevision}
                    onContentChange={onContentChange}
                />
            );

            expect(
                JSON.parse(getByTestId('native-editor-view').props.editorUpdateJson as string)
                    .documentVersion
            ).toBe(externalRevision);

            // A stale native commit after N+1 is deterministically rejected:
            // it produces no further content notification of its own.
            const notificationsBeforeStaleCommit = onContentChange.mock.calls.length;
            act(() => emit(first));
            expect(onContentChange).toHaveBeenCalledTimes(notificationsBeforeStaleCommit);
            handle.destroy();
        }
    );

    it('forwards native selection changes with the engine selection and fires focus/blur edges', () => {
        const handle = createV2LocalHandle(V2_INITIAL_DOC);
        const onSelectionChange = jest.fn();
        const onFocus = jest.fn();
        const onBlur = jest.fn();

        const { getByTestId } = render(
            <NativeRichTextEditor
                documentHandle={handle}
                onSelectionChange={onSelectionChange}
                onFocus={onFocus}
                onBlur={onBlur}
            />
        );

        act(() => {
            getByTestId('native-editor-view').props.onSelectionChange({
                nativeEvent: {
                    anchor: 1,
                    head: 3,
                    editorId: handle.editorId,
                    stateJson: renderUpdateValue(handle.editorId, 1, 3),
                },
            });
        });

        expect(onSelectionChange).toHaveBeenCalledTimes(1);

        // The native mirror offsets are scalar coordinates; the render
        // payload resolves them to the engine's document coordinates.
        expect(onSelectionChange).toHaveBeenCalledWith({ type: 'text', anchor: 2, head: 4 });

        // Without a state payload the raw scalar range is forwarded.
        act(() => {
            getByTestId('native-editor-view').props.onSelectionChange({
                nativeEvent: { anchor: 2, head: 4, editorId: handle.editorId },
            });
        });

        expect(onSelectionChange).toHaveBeenLastCalledWith({ type: 'text', anchor: 2, head: 4 });

        act(() => {
            getByTestId('native-editor-view').props.onFocusChange({
                nativeEvent: { isFocused: true, editorId: handle.editorId },
            });
        });

        expect(onFocus).toHaveBeenCalledTimes(1);
        expect(onBlur).not.toHaveBeenCalled();

        act(() => {
            getByTestId('native-editor-view').props.onFocusChange({
                nativeEvent: { isFocused: true, editorId: handle.editorId },
            });
        });

        expect(onFocus).toHaveBeenCalledTimes(1);

        act(() => {
            getByTestId('native-editor-view').props.onFocusChange({
                nativeEvent: { isFocused: false, editorId: handle.editorId },
            });
        });

        expect(onBlur).toHaveBeenCalledTimes(1);
        handle.destroy();
    });

    it('discards a stale auto-grow height before returning from fixed mode', () => {
        const handle = createV2LocalHandle(V2_INITIAL_DOC);

        const { getByTestId, rerender } = render(
            <NativeRichTextEditor documentHandle={handle} heightBehavior={'autoGrow'} />
        );

        act(() => {
            getByTestId('native-editor-view').props.onContentHeightChange({
                nativeEvent: { contentHeight: 420, editorId: handle.editorId },
            });
        });

        expect(getByTestId('native-editor-view').props.style).toEqual(
            expect.objectContaining({ height: 420 })
        );

        rerender(<NativeRichTextEditor documentHandle={handle} heightBehavior={'fixed'} />);
        rerender(<NativeRichTextEditor documentHandle={handle} heightBehavior={'autoGrow'} />);

        expect(getByTestId('native-editor-view').props.style).not.toEqual(
            expect.objectContaining({ height: 420 })
        );

        handle.destroy();
    });

    it('ignores missing and late editor-scoped interaction events after rebinding to B', () => {
        const handleA = createV2LocalHandle(V2_INITIAL_DOC);
        const handleB = createV2LocalHandle(V2_DOC_B);
        const onSelectionChange = jest.fn();
        const onFocus = jest.fn();
        const onBlur = jest.fn();
        const onToolbarAction = jest.fn();

        const { getByTestId, rerender } = render(
            <NativeRichTextEditor
                documentHandle={handleA}
                heightBehavior={'autoGrow'}
                onSelectionChange={onSelectionChange}
                onFocus={onFocus}
                onBlur={onBlur}
                onToolbarAction={onToolbarAction}
            />
        );

        rerender(
            <NativeRichTextEditor
                documentHandle={handleB}
                heightBehavior={'autoGrow'}
                onSelectionChange={onSelectionChange}
                onFocus={onFocus}
                onBlur={onBlur}
                onToolbarAction={onToolbarAction}
            />
        );

        const view = getByTestId('native-editor-view');

        act(() => {
            view.props.onSelectionChange({ nativeEvent: { anchor: 9, head: 9 } });
            view.props.onFocusChange({ nativeEvent: { isFocused: true } });
            view.props.onContentHeightChange({ nativeEvent: { contentHeight: 240 } });
            view.props.onToolbarAction({ nativeEvent: { key: 'late-action' } });

            view.props.onSelectionChange({
                nativeEvent: { anchor: 9, head: 9, editorId: handleA.editorId },
            });

            view.props.onFocusChange({
                nativeEvent: { isFocused: true, editorId: handleA.editorId },
            });

            view.props.onFocusChange({
                nativeEvent: { isFocused: false, editorId: handleA.editorId },
            });

            view.props.onContentHeightChange({
                nativeEvent: { contentHeight: 240, editorId: handleA.editorId },
            });

            view.props.onToolbarAction({
                nativeEvent: { key: 'late-action', editorId: handleA.editorId },
            });
        });

        expect(onSelectionChange).not.toHaveBeenCalled();
        expect(onFocus).not.toHaveBeenCalled();
        expect(onBlur).not.toHaveBeenCalled();
        expect(onToolbarAction).not.toHaveBeenCalled();
        expect(view.props.style).not.toEqual(expect.objectContaining({ height: 240 }));

        act(() => {
            view.props.onSelectionChange({
                nativeEvent: { anchor: 2, head: 2, editorId: handleB.editorId },
            });

            view.props.onFocusChange({
                nativeEvent: { isFocused: true, editorId: handleB.editorId },
            });

            view.props.onContentHeightChange({
                nativeEvent: { contentHeight: 240, editorId: handleB.editorId },
            });

            view.props.onToolbarAction({
                nativeEvent: { key: 'B-action', editorId: handleB.editorId },
            });
        });

        expect(onSelectionChange).toHaveBeenCalledWith({ type: 'text', anchor: 2, head: 2 });
        expect(onFocus).toHaveBeenCalledTimes(1);
        expect(onToolbarAction).toHaveBeenCalledWith('B-action');

        expect(getByTestId('native-editor-view').props.style).toEqual(
            expect.objectContaining({ height: 240 })
        );

        handleA.destroy();
        handleB.destroy();
    });

    it('keeps editor-bound awareness hooks inert after localAwareness is removed', () => {
        const handle = createV2RoomHandle({ withSnapshot: true });

        let localAwareness: { userId: string; name: string; color: string } | undefined = {
            userId: '1',
            name: 'Alice',
            color: '#f00',
        };

        let collaboration: ReturnType<typeof useYjsCollaboration> | null = null;

        function CollaborationBoundEditor() {
            collaboration = useYjsCollaboration({
                documentId: 'doc-1',
                handle,
                localAwareness,
                transport: { url: V2_TRANSPORT_URL, connect: true },
            });

            return <NativeRichTextEditor {...collaboration.editorBindings} />;
        }

        const { getByTestId, rerender } = render(<CollaborationBoundEditor />);

        act(() => {
            v2Runtime.transportOpen(handle.editorId);
            v2Runtime.transportReceive(handle.editorId, V2_FAKE_STEP2_FRAME);
        });

        localAwareness = undefined;
        rerender(<CollaborationBoundEditor />);

        expect(v2Runtime.session(handle.editorId).desiredAwareness).toBeNull();

        const awarenessCallsAfterClear =
            mockNativeModule.editorV2CollaborationSetAwareness.mock.calls.length;

        act(() => {
            getByTestId('native-editor-view').props.onSelectionChange({
                nativeEvent: { anchor: 1, head: 3, editorId: handle.editorId },
            });

            getByTestId('native-editor-view').props.onFocusChange({
                nativeEvent: { isFocused: true, editorId: handle.editorId },
            });
        });

        expect(mockNativeModule.editorV2CollaborationSetAwareness).toHaveBeenCalledTimes(
            awarenessCallsAfterClear
        );

        expect(v2Runtime.session(handle.editorId).desiredAwareness).toBeNull();
        expect(collaboration).not.toBeNull();
        handle.destroy();
    });
});

it('forwards lightweight document notifications for native commits without content exports', () => {
    const handle = createV2LocalHandle(V2_INITIAL_DOC);
    const onDocumentChange = jest.fn();
    const editor = render(<NativeRichTextEditor documentHandle={handle} onDocumentChange={onDocumentChange} />);
    expect(onDocumentChange).not.toHaveBeenCalled();
    handle.bridge.replaceDocument({ setJson: V2_DOC_B, history: 'undoableBoundary' });
    v2Runtime.session(handle.editorId).documentOrigin = 'nativeView';
    const payload = {
        editorId: handle.editorId,
        updateJson: renderUpdateValue(handle.editorId),
        documentRevision: handle.bridge.getState().documentRevision,
    };
    act(() => {
        editor.getByTestId('native-editor-view').props.onEditorUpdate({ nativeEvent: payload });
        editor.getByTestId('native-editor-view').props.onEditorUpdate({ nativeEvent: payload });
    });
    expect(onDocumentChange.mock.calls).toEqual([[{
        documentRevision: payload.documentRevision, origin: 'nativeView',
    }]]);
    expect(mockNativeModule.editorV2GetContentSnapshot).not.toHaveBeenCalled();
    expect(mockNativeModule.editorV2GetDocumentJson).not.toHaveBeenCalled();
    expect(mockNativeModule.editorV2GetDocumentHtml).not.toHaveBeenCalled();
    editor.unmount();
    handle.destroy();
});

it('observes remote changes after initial room readiness without eager content callbacks', () => {
    const handle = createV2RoomHandle({ withSnapshot: false });
    const onDocumentChange = jest.fn();
    function CollaborationBoundEditor() {
        const collaboration = useYjsCollaboration({
            documentId: 'doc-1', handle,
            transport: { url: V2_TRANSPORT_URL, connect: true },
        });
        return <NativeRichTextEditor {...collaboration.editorBindings} onDocumentChange={onDocumentChange} />;
    }
    const editor = render(<CollaborationBoundEditor />);
    act(() => {
        v2Runtime.pushRemoteDoc(handle.editorId, V2_INITIAL_DOC);
        v2Runtime.transportOpen(handle.editorId);
        v2Runtime.transportReceive(handle.editorId, V2_FAKE_STEP2_FRAME);
    });
    expect(onDocumentChange).not.toHaveBeenCalled();
    act(() => {
        v2Runtime.pushRemoteDoc(handle.editorId, V2_DOC_B);
        v2Runtime.transportReceive(handle.editorId, V2_FAKE_UPDATE_FRAME);
    });
    expect(onDocumentChange.mock.calls).toEqual([[{
        documentRevision: handle.bridge.getState().documentRevision,
        origin: 'remoteCollaboration',
    }]]);
    expect(mockNativeModule.editorV2GetContentSnapshot).not.toHaveBeenCalled();
    expect(mockNativeModule.editorV2GetDocumentHtml).not.toHaveBeenCalled();
    editor.unmount();
    handle.destroy();
});
