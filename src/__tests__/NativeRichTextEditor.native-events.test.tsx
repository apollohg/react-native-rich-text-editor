import './helpers/NativeRichTextEditorFixture';
import {
    mockNativeModule,
    v2Runtime,
    V2_INITIAL_DOC,
    V2_DOC_B,
    createV2LocalHandle,
    renderUpdateValue,
} from './helpers/NativeRichTextEditorFixture';
import { createRef, Profiler } from 'react';

import { render, act } from '@testing-library/react-native';
import { NativeRichTextEditor, type NativeRichTextEditorRef } from '../NativeRichTextEditor';

import { NativeEditorNonRetryableError } from '../NativeEditorBoundaryError';

const capturedSelection = require('./helpers/nativeQueuedSelectionProof.json') as {
    newerSelectionStateJson: string;
    newerSelectionPayload: { anchor: number; head: number; editorId: string; documentVersion: string; stateJson: string };
    selectionStateRevision: string;
};

describe('NativeRichTextEditor (v2 document mode)', () => {
    it('binds the native view to the session editor id and is editable by default', () => {
        const handle = createV2LocalHandle(V2_INITIAL_DOC);
        const { getByTestId } = render(<NativeRichTextEditor documentHandle={handle} />);
        const view = getByTestId('native-editor-view');
        expect(view.props.editorId).toBe(handle.editorId);
        expect(view.props.editable).toBe(true);
        expect(view.props.showToolbar).toBe(true);
        expect(view.props.autoFocus).toBe(false);
        expect(typeof view.props.onEditorUpdate).toBe('function');
        expect(typeof view.props.onEditorError).toBe('function');
        // The native side marked the session live for view binding at create.
        expect(v2Runtime.liveEditorIds()).toContain(handle.editorId);
        handle.destroy();
    });

    it.each([ 'iOS', 'Android' ])(
        'routes each valid %s autonomous native error once through the bound handle',
        platform => {
            const handle = createV2LocalHandle(V2_INITIAL_DOC);
            const primaryListener = jest.fn();
            const secondaryListener = jest.fn();
            handle.addErrorListener(primaryListener);
            handle.addErrorListener(secondaryListener);
            const { getByTestId } = render(<NativeRichTextEditor documentHandle={handle} />);
            const view = getByTestId('native-editor-view');

            const error = {
                domain: 'operation',
                code: 'POSITION_INVALID',
                message: 'native selection is invalid',
                requestId: '7',
            };

            const emit = () =>
                view.props.onEditorError({
                    // iOS constructs the map identity-first while Android
                    // supplies the error field first.
                    nativeEvent:
                        platform === 'iOS'
                            ? { editorId: handle.editorId, error }
                            : { error, editorId: handle.editorId },
                });

            act(() => emit());
            act(() => emit());

            // Equal native failures are separate emissions, never value-deduped.
            expect(primaryListener).toHaveBeenCalledTimes(2);
            expect(secondaryListener).toHaveBeenCalledTimes(2);

            expect(primaryListener).toHaveBeenNthCalledWith(
                1,
                expect.objectContaining({ code: 'POSITION_INVALID', requestId: '7' })
            );

            handle.destroy();
        }
    );

    it('accepts a queued native commit after the engine advances before event delivery', () => {
        const handle = createV2LocalHandle(V2_INITIAL_DOC);
        const onContentChange = jest.fn();

        const { getByTestId } = render(
            <NativeRichTextEditor documentHandle={handle} onContentChange={onContentChange} />
        );

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

        const queuedRevision = handle.bridge.getState().documentRevision;
        const queuedUpdateJson = renderUpdateValue(handle.editorId);

        handle.bridge.replaceDocument({ setJson: V2_DOC_B, history: 'undoableBoundary' });

        act(() => {
            getByTestId('native-editor-view').props.onEditorUpdate({
                nativeEvent: {
                    editorId: handle.editorId,
                    documentRevision: queuedRevision,
                    updateJson: queuedUpdateJson,
                },
            });
        });

        expect(onContentChange).toHaveBeenCalledTimes(1);
        expect(handle.bridge.getContentSnapshot().json).toEqual(V2_DOC_B);
        handle.destroy();
    });

    it('suppresses a native-origin revision even when collaboration reports it before the native event', () => {
        const handle = createV2LocalHandle(V2_INITIAL_DOC);
        const { getByTestId, rerender } = render(<NativeRichTextEditor documentHandle={handle} />);

        v2Runtime.module.editorV2ApplyInput(
            handle.editorId,
            JSON.stringify({
                version: 1,
                requestId: '1',
                baseDocumentRevision: handle.bridge.getState().documentRevision,
                text: '!',
            })
        );

        v2Runtime.session(handle.editorId).documentOrigin = 'nativeView';
        const revision = handle.bridge.getState().documentRevision;

        rerender(<NativeRichTextEditor documentHandle={handle} documentRevision={revision} />);

        expect(getByTestId('native-editor-view').props.editorUpdateJson).toBeUndefined();
        handle.destroy();
    });

    it('rejects invalid error identities and turns one malformed current error into FFI_RESULT_INVALID', () => {
        const handle = createV2LocalHandle(V2_INITIAL_DOC);
        const ref = createRef<NativeRichTextEditorRef>();
        const received: unknown[] = [];
        handle.addErrorListener(error => received.push(error));
        const { getByTestId } = render(<NativeRichTextEditor ref={ref} documentHandle={handle} />);
        const view = getByTestId('native-editor-view');

        const validError = {
            domain: 'operation',
            code: 'POSITION_INVALID',
            message: 'native selection is invalid',
        };

        for (const editorId of [ undefined,
            1,
            '01',
            '18446744073709551616',
            '999' ]) {
            act(() => {
                view.props.onEditorError({ nativeEvent: { editorId, error: validError } });
            });
        }

        expect(received).toHaveLength(0);

        act(() => {
            view.props.onEditorError({
                nativeEvent: { editorId: handle.editorId, error: { code: 42 } },
            });
        });

        expect(received).toHaveLength(1);
        expect(received[0]).toBeInstanceOf(NativeEditorNonRetryableError);
        expect((received[0] as NativeEditorNonRetryableError).code).toBe('FFI_RESULT_INVALID');

        // The boundary event is non-terminal: a normal interaction remains usable.
        act(() => ref.current!.toggleMark('bold'));
        expect(mockNativeModule.editorV2ApplyCommand).toHaveBeenCalledTimes(1);
        handle.destroy();
    });

    it('drops late autonomous errors after unsubscribe, rebind, unmount, and destroy', () => {
        const handleA = createV2LocalHandle(V2_INITIAL_DOC);
        const handleB = createV2LocalHandle(V2_DOC_B);
        const receivedA: unknown[] = [];
        const receivedB: unknown[] = [];
        const unsubscribeA = handleA.addErrorListener(error => receivedA.push(error));
        handleB.addErrorListener(error => receivedB.push(error));

        const { getByTestId, rerender, unmount } = render(
            <NativeRichTextEditor documentHandle={handleA} />
        );

        const firstABinding = getByTestId('native-editor-view').props.onEditorError;

        const errorA = {
            nativeEvent: {
                editorId: handleA.editorId,
                error: { domain: 'operation', code: 'POSITION_INVALID', message: 'A error' },
            },
        };

        act(() => firstABinding(errorA));
        expect(receivedA).toHaveLength(1);
        unsubscribeA();
        act(() => firstABinding(errorA));
        expect(receivedA).toHaveLength(1);

        rerender(<NativeRichTextEditor documentHandle={handleB} />);
        const bBinding = getByTestId('native-editor-view').props.onEditorError;
        rerender(<NativeRichTextEditor documentHandle={handleA} />);
        const reboundABinding = getByTestId('native-editor-view').props.onEditorError;
        handleA.addErrorListener(error => receivedA.push(error));

        act(() => firstABinding(errorA));

        act(() =>
            bBinding({
                nativeEvent: {
                    editorId: handleB.editorId,
                    error: { domain: 'operation', code: 'POSITION_INVALID', message: 'B error' },
                },
            }));

        expect(receivedA).toHaveLength(1);
        expect(receivedB).toHaveLength(0);

        act(() => reboundABinding(errorA));
        expect(receivedA).toHaveLength(2);

        unmount();
        act(() => reboundABinding(errorA));
        expect(receivedA).toHaveLength(2);

        handleA.destroy();
        act(() => reboundABinding(errorA));
        expect(receivedA).toHaveLength(2);
        handleB.destroy();
    });

    it('moves the caret without re-rendering the editor until the active formatting changes', () => {
        const handle = createV2LocalHandle(V2_INITIAL_DOC);
        const onSelectionChange = jest.fn();
        const onActiveStateChange = jest.fn();
        const committedRenders: string[] = [];

        const { getByTestId, getByLabelText } = render(
            <Profiler id={'editor'} onRender={(_id, phase) => committedRenders.push(phase)}>
                <NativeRichTextEditor documentHandle={handle} toolbarPlacement={'inline'}
                    onSelectionChange={onSelectionChange} onActiveStateChange={onActiveStateChange}
                />
            </Profiler>
        );

        const projection = JSON.parse(capturedSelection.newerSelectionStateJson) as {
            selection: Record<string, unknown>;
            activeState: { marks: Record<string, boolean> };
        };

        let stateRevision = BigInt(capturedSelection.selectionStateRevision);

        const moveCaret = (offset: number, activeState = projection.activeState) => {
            stateRevision += 1n;
            act(() => {
                getByTestId('native-editor-view').props.onSelectionChange({ nativeEvent: {
                    ...capturedSelection.newerSelectionPayload,
                    editorId: handle.editorId,
                    anchor: offset,
                    head: offset,
                    stateJson: JSON.stringify({ ...projection, stateRevision: String(stateRevision), activeState,
                        selection: { ...projection.selection, anchor: offset, head: offset,
                            anchorScalar: offset - 1, headScalar: offset - 1 } }),
                } });
            });
        };

        moveCaret(2);
        const rendersAfterFirstPlainCaret = committedRenders.length;
        onActiveStateChange.mockClear();

        for (const offset of [ 3, 4, 5 ]) {
            moveCaret(offset);
            expect(onSelectionChange).toHaveBeenLastCalledWith(expect.objectContaining({ anchor: offset, head: offset }));
        }

        expect({ plainCaretMoveRenders: committedRenders.slice(rendersAfterFirstPlainCaret) }).toEqual({ plainCaretMoveRenders: [] });
        expect(onActiveStateChange).not.toHaveBeenCalled();
        expect(getByLabelText('Bold').props.accessibilityState.selected).toBe(false);

        moveCaret(6, { ...projection.activeState, marks: { ...projection.activeState.marks, bold: true } });
        expect(committedRenders.length).toBeGreaterThan(rendersAfterFirstPlainCaret);
        expect(onActiveStateChange).toHaveBeenCalledTimes(1);
        expect(getByLabelText('Bold').props.accessibilityState.selected).toBe(true);

        moveCaret(7);
        expect(onActiveStateChange).toHaveBeenCalledTimes(2);
        expect(getByLabelText('Bold').props.accessibilityState.selected).toBe(false);
        handle.destroy();
    });

    describe('selection events that omit an unchanged active state', () => {
        const projection = JSON.parse(capturedSelection.newerSelectionStateJson) as {
            selection: Record<string, unknown>;
            activeState: { marks: Record<string, boolean> };
        };
        const boldState = { ...projection.activeState, marks: { ...projection.activeState.marks, bold: true } };

        const renderEditor = () => {
            const handle = createV2LocalHandle(V2_INITIAL_DOC);
            const ref = createRef<NativeRichTextEditorRef>();
            const onSelectionChange = jest.fn();
            const onActiveStateChange = jest.fn();
            const rendered = render(
                <NativeRichTextEditor ref={ref} documentHandle={handle} toolbarPlacement={'inline'}
                    onSelectionChange={onSelectionChange} onActiveStateChange={onActiveStateChange}
                />
            );
            let stateRevision = BigInt(capturedSelection.selectionStateRevision);
            const moveCaret = (offset: number, active: { activeState: unknown } | { activeStateRevision: string }) => {
                stateRevision += 1n;
                const revision = String(stateRevision);
                act(() => {
                    rendered.getByTestId('native-editor-view').props.onSelectionChange({ nativeEvent: {
                        ...capturedSelection.newerSelectionPayload,
                        editorId: handle.editorId,
                        anchor: offset,
                        head: offset,
                        stateJson: JSON.stringify({ ...projection, activeState: undefined, ...active,
                            stateRevision: revision,
                            selection: { ...projection.selection, anchor: offset, head: offset,
                                anchorScalar: offset - 1, headScalar: offset - 1 } }),
                    } });
                });
                return revision;
            };
            const boldSelected = () => rendered.getByLabelText('Bold').props.accessibilityState.selected as boolean;
            return { handle, ref, rendered, onSelectionChange, onActiveStateChange, moveCaret, boldSelected };
        };

        it('keeps the last emitted active state when a caret move omits it', () => {
            const editor = renderEditor();
            const renderState = jest.spyOn(editor.handle.bridge, 'renderState');
            const boldRevision = editor.moveCaret(2, { activeState: boldState });
            expect(editor.boldSelected()).toBe(true);
            editor.onActiveStateChange.mockClear();
            renderState.mockClear();

            for (const offset of [ 3, 4, 5 ]) {
                editor.moveCaret(offset, { activeStateRevision: boldRevision });
                expect(editor.onSelectionChange).toHaveBeenLastCalledWith(
                    expect.objectContaining({ anchor: offset, head: offset }));
            }

            expect({ refetches: renderState.mock.calls.length, activeStateChanges: editor.onActiveStateChange.mock.calls })
                .toEqual({ refetches: 0, activeStateChanges: [] });
            expect(editor.boldSelected()).toBe(true);
            editor.moveCaret(6, { activeState: projection.activeState });
            expect(editor.boldSelected()).toBe(false);
            editor.handle.destroy();
        });

        it('refetches the engine active state when the omitted revision was never received', () => {
            const editor = renderEditor();
            const renderState = jest.spyOn(editor.handle.bridge, 'renderState');

            editor.moveCaret(3, { activeStateRevision: capturedSelection.selectionStateRevision });

            expect(renderState).toHaveBeenCalledTimes(1);
            expect(editor.onSelectionChange).toHaveBeenLastCalledWith(expect.objectContaining({ anchor: 3, head: 3 }));
            expect(editor.boldSelected()).toBe(
                editor.handle.bridge.renderState().activeState.marks.bold === true);
            editor.handle.destroy();
        });

        it.each([
            [ 'toggleMark', (ref: NativeRichTextEditorRef) => ref.toggleMark('bold') ],
            [ 'setContent', (ref: NativeRichTextEditorRef) => ref.setContent('<p>Replaced</p>') ],
        ])('forgets the emitted active state once %s pushes an update to the view', (_, update) => {
            const editor = renderEditor();
            const boldRevision = editor.moveCaret(2, { activeState: boldState });
            act(() => update(editor.ref.current!));
            const renderState = jest.spyOn(editor.handle.bridge, 'renderState');

            editor.moveCaret(3, { activeStateRevision: boldRevision });

            expect(renderState).toHaveBeenCalledTimes(1);
            expect(editor.boldSelected()).toBe(
                editor.handle.bridge.renderState().activeState.marks.bold === true);
            editor.handle.destroy();
        });

        it('forgets the emitted active state when the view rebinds to another document', () => {
            const editor = renderEditor();
            const other = createV2LocalHandle(V2_INITIAL_DOC);
            const boldRevision = editor.moveCaret(2, { activeState: boldState });
            editor.rendered.rerender(
                <NativeRichTextEditor ref={editor.ref} documentHandle={other} toolbarPlacement={'inline'} />
            );
            editor.rendered.rerender(
                <NativeRichTextEditor ref={editor.ref} documentHandle={editor.handle} toolbarPlacement={'inline'}
                    onSelectionChange={editor.onSelectionChange}
                />
            );
            const renderState = jest.spyOn(editor.handle.bridge, 'renderState');

            editor.moveCaret(3, { activeStateRevision: boldRevision });

            expect(renderState).toHaveBeenCalledTimes(1);
            expect(editor.onSelectionChange).toHaveBeenLastCalledWith(expect.objectContaining({ anchor: 3, head: 3 }));
            editor.handle.destroy();
            other.destroy();
        });
    });

    it('enables the image button when a localEmpty editor is focused without any native state event', () => {
        const handle = createV2LocalHandle();
        const engineState = handle.bridge.renderState();

        const { getByTestId, getByLabelText } = render(
            <NativeRichTextEditor documentHandle={handle} toolbarPlacement={'inline'}
                toolbarItems={[ { type: 'image', label: 'Image', icon: { type: 'default', id: 'image' } } ]}
                onRequestImage={jest.fn()}
            />
        );

        act(() => getByTestId('native-editor-view').props.onFocusChange({ nativeEvent: {
            editorId: handle.editorId, isFocused: true,
        } }));

        expect(engineState.activeState.insertableNodes).toContain('image');
        expect(getByLabelText('Image').props.accessibilityState).toEqual(
            expect.objectContaining({ disabled: false })
        );
        handle.destroy();
    });

    it('respects editable={false} on the native view', () => {
        const handle = createV2LocalHandle(V2_INITIAL_DOC);

        const { getByTestId } = render(
            <NativeRichTextEditor documentHandle={handle} editable={false} />
        );

        expect(getByTestId('native-editor-view').props.editable).toBe(false);
        handle.destroy();
    });

    it('routes a native typing commit through the engine exactly once and never echoes it back', () => {
        const handle = createV2LocalHandle(V2_INITIAL_DOC);
        const ref = createRef<NativeRichTextEditorRef>();
        const onContentChange = jest.fn();

        const { getByTestId } = render(
            <NativeRichTextEditor
                ref={ref}
                documentHandle={handle}
                onContentChange={onContentChange}
            />
        );

        mockNativeModule.editorV2ApplyInput.mockClear();
        mockNativeModule.editorV2ApplyCommand.mockClear();
        mockNativeModule.editorV2ApplyLocalApi.mockClear();
        mockNativeModule.editorV2ReplaceDocument.mockClear();

        // While the IME is composing there is no native event and no engine
        // traffic at all : transient composing text never crosses JS.
        expect(mockNativeModule.editorV2ApplyInput).not.toHaveBeenCalled();

        // The native adapter commits the final text as ONE typed transaction,
        // then the view emits the resulting update.
        const commitRequest = JSON.stringify({
            version: 1,
            requestId: '1',
            baseDocumentRevision: handle.bridge.getState().documentRevision,
            text: '!',
        });

        let commitOutcome: { value: string } | undefined;

        act(() => {
            commitOutcome = v2Runtime.module.editorV2ApplyInput(handle.editorId, commitRequest) as {
                value: string;
            };
        });

        expect(JSON.parse(commitOutcome!.value)).toMatchObject({ type: 'transaction' });

        act(() => {
            getByTestId('native-editor-view').props.onEditorUpdate({
                nativeEvent: {
                    editorId: handle.editorId,
                    updateJson: renderUpdateValue(handle.editorId),
                    documentRevision: handle.bridge.getState().documentRevision,
                },
            });
        });

        // The component issued no mutation of its own: exactly one typed
        // transaction reached the engine (the adapter's).
        expect(mockNativeModule.editorV2ApplyInput).toHaveBeenCalledTimes(1);
        expect(mockNativeModule.editorV2ApplyCommand).not.toHaveBeenCalled();
        expect(mockNativeModule.editorV2ApplyLocalApi).not.toHaveBeenCalled();
        expect(mockNativeModule.editorV2ReplaceDocument).not.toHaveBeenCalled();
        // The content callback fired exactly once for the one commit.
        expect(onContentChange).toHaveBeenCalledTimes(1);
        expect(onContentChange).toHaveBeenCalledWith('<p>hello!</p>');
        // No echo: the view already applied the adapter's update natively.
        expect(getByTestId('native-editor-view').props.editorUpdateJson).toBeUndefined();
        expect(ref.current!.getContent()).toBe('<p>hello!</p>');
        handle.destroy();
    });
});
