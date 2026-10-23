const mockNativeModule: Record<string, jest.Mock> = {};

jest.mock('expo-modules-core', () => ({
    requireNativeModule: () => mockNativeModule,
    requireNativeViewManager: () => 'NativeEditorView'
}));

import React from 'react';
import { act, render } from '@testing-library/react-native';
import {
    _resetNativeModuleCache,
    createNativeEditorDocumentHandle
} from '../NativeEditorBridge';
import {
    useNativeEditorDocument,
    type UseNativeEditorDocumentOptions
} from '../useNativeEditor';
import {
    createFakeNativeEditorV2Runtime,
    fakeDocForText
} from './helpers/nativeEditorV2Fake';

let runtime: ReturnType<typeof createFakeNativeEditorV2Runtime>;
let document: ReturnType<typeof useNativeEditorDocument>;

function Observer(props: UseNativeEditorDocumentOptions) {
    document = useNativeEditorDocument(props);
    return null;
}

beforeEach(() => {
    _resetNativeModuleCache();
    runtime = createFakeNativeEditorV2Runtime();
    Object.assign(mockNativeModule, runtime.module);
});

function localHandle() {
    return createNativeEditorDocumentHandle({
        initialization: { type: 'localJson', json: fakeDocForText('initial') }
    });
}

it('notifies once per observed revision without exporting content, including history', () => {
    const handle = localHandle();
    const onDocumentChange = jest.fn();
    const observer = render(
        <Observer handle={handle} onDocumentChange={onDocumentChange} />
    );
    expect(onDocumentChange).not.toHaveBeenCalled();
    act(() => document.setContentJson(fakeDocForText('changed')));
    expect(onDocumentChange).toHaveBeenLastCalledWith({
        documentRevision: document.documentRevision,
        origin: document.documentOrigin
    });
    act(() => {
        document.refresh();
        document.refresh();
    });
    expect(onDocumentChange).toHaveBeenCalledTimes(1);
    act(() => document.undo());
    expect(onDocumentChange).toHaveBeenCalledTimes(2);
    expect(onDocumentChange).toHaveBeenLastCalledWith({
        documentRevision: document.documentRevision,
        origin: 'history'
    });
    for (const method of [
        'editorV2GetContentSnapshot',
        'editorV2GetDocumentHtml',
        'editorV2GetDocumentJson'
    ]) {
        expect(mockNativeModule[method]).not.toHaveBeenCalled();
    }
    observer.unmount();
    handle.destroy();
});

it('reports remote revisions through revisionSignal and resets the baseline on handle replacement', () => {
    const handle = localHandle();
    const replacement = localHandle();
    const onDocumentChange = jest.fn();
    const observer = render(
        <Observer handle={handle} onDocumentChange={onDocumentChange} />
    );
    const session = runtime.session(handle.editorId);
    session.documentRevision += 1;
    session.documentOrigin = 'remoteCollaboration';
    observer.rerender(
        <Observer
            handle={handle}
            revisionSignal={String(session.documentRevision)}
            onDocumentChange={onDocumentChange}
        />
    );
    expect(onDocumentChange).toHaveBeenLastCalledWith({
        documentRevision: String(session.documentRevision),
        origin: 'remoteCollaboration'
    });
    observer.rerender(
        <Observer handle={replacement} onDocumentChange={onDocumentChange} />
    );
    expect(onDocumentChange).toHaveBeenCalledTimes(1);
    act(() => document.setContentJson(fakeDocForText('replacement')));
    expect(onDocumentChange).toHaveBeenCalledTimes(2);
    observer.unmount();
    handle.destroy();
    replacement.destroy();
});

it('reports controlled replacements while leaving eager controlled callbacks suppressed', () => {
    const handle = localHandle();
    const onDocumentChange = jest.fn();
    const onContentChangeJSON = jest.fn();
    const observer = render(
        <Observer
            handle={handle}
            onDocumentChange={onDocumentChange}
            onContentChangeJSON={onContentChangeJSON}
        />
    );
    observer.rerender(
        <Observer
            handle={handle}
            valueJSON={fakeDocForText('controlled')}
            onDocumentChange={onDocumentChange}
            onContentChangeJSON={onContentChangeJSON}
        />
    );
    expect(onDocumentChange).toHaveBeenCalledTimes(1);
    expect(onContentChangeJSON).not.toHaveBeenCalled();
    observer.unmount();
    handle.destroy();
});

it('does not publish stale eager content when the lightweight callback mutates the document', () => {
    const handle = localHandle();
    const onContentChangeJSON = jest.fn();
    const onDocumentChange = jest.fn(() => {
        if (onDocumentChange.mock.calls.length === 1) {
            document.setContentJson(fakeDocForText('newest'));
        }
    });
    const observer = render(
        <Observer
            handle={handle}
            onDocumentChange={onDocumentChange}
            onContentChangeJSON={onContentChangeJSON}
        />
    );
    act(() => document.setContentJson(fakeDocForText('intermediate')));
    expect(onDocumentChange).toHaveBeenCalledTimes(2);
    expect(onContentChangeJSON.mock.calls).toEqual([
        [fakeDocForText('newest')]
    ]);
    observer.unmount();
    handle.destroy();
});

it('stops delivery if the lightweight callback destroys the handle', () => {
    const handle = localHandle();
    const onContentChangeJSON = jest.fn();
    const observer = render(
        <Observer
            handle={handle}
            onDocumentChange={() => handle.destroy()}
            onContentChangeJSON={onContentChangeJSON}
        />
    );
    act(() => document.setContentJson(fakeDocForText('changed')));
    expect(handle.isDestroyed).toBe(true);
    expect(onContentChangeJSON).not.toHaveBeenCalled();
    observer.unmount();
});

it('reads raw text and emptiness without HTML or render-state exports', () => {
    const handle = localHandle();
    const observer = render(<Observer handle={handle} />);
    const rawText = '<tag> & café 👩🏽‍💻';
    act(() => document.setContentJson(fakeDocForText(rawText)));
    expect(document.getTextContent()).toBe(rawText);
    expect(document.getIsEmpty()).toBe(false);
    act(() => document.clearContent());
    expect(document.getTextContent()).toBe('');
    expect(document.getIsEmpty()).toBe(true);
    expect(mockNativeModule.editorV2GetDocumentHtml).not.toHaveBeenCalled();
    expect(mockNativeModule.editorV2RenderState).not.toHaveBeenCalled();
    expect(mockNativeModule.editorV2GetContentSnapshot).not.toHaveBeenCalled();
    observer.unmount();
    handle.destroy();
    expect(document.getTextContent()).toBe('');
    expect(document.getIsEmpty()).toBe(true);
});

it('coalesces unobserved revisions even when the final content equals the baseline', () => {
    const handle = localHandle();
    const onDocumentChange = jest.fn();
    const observer = render(<Observer handle={handle} onDocumentChange={onDocumentChange} />);
    handle.bridge.replaceDocument({ setJson: fakeDocForText('intermediate'), history: 'undoableBoundary' });
    handle.bridge.replaceDocument({ setJson: fakeDocForText('initial'), history: 'undoableBoundary' });
    act(() => document.refresh());
    expect(onDocumentChange.mock.calls).toEqual([[{
        documentRevision: handle.bridge.getState().documentRevision,
        origin: handle.bridge.getState().documentOrigin,
    }]]);
    expect(mockNativeModule.editorV2GetContentSnapshot).not.toHaveBeenCalled();
    expect(mockNativeModule.editorV2GetDocumentJson).not.toHaveBeenCalled();
    observer.unmount();
    handle.destroy();
});
