const mockNativeModule: Record<string, jest.Mock> = {};

jest.mock('expo-modules-core', () => ({
    requireNativeModule: () => mockNativeModule,
    requireNativeViewManager: () => 'NativeEditorView',
}));

import React from 'react';
import { act, render } from '@testing-library/react-native';

import {
    _resetNativeModuleCache,
    createNativeEditorDocumentHandle,
    type NativeEditorDocumentHandle,
} from '../NativeEditorBridge';
import { useNativeEditorDocument } from '../useNativeEditor';
import { createFakeNativeEditorV2Runtime, fakeDocForText } from './helpers/nativeEditorV2Fake';

describe('useNativeEditorDocument', () => {
    beforeEach(() => {
        _resetNativeModuleCache();
        const runtime = createFakeNativeEditorV2Runtime();

        for (const key of Object.keys(mockNativeModule)) {
            delete mockNativeModule[key];
        }

        Object.assign(mockNativeModule, runtime.module);
    });

    it('keeps document revisions and history current without exporting unobserved content', () => {
        const typingBurstLength = 20;

        const handle = createNativeEditorDocumentHandle({
            initialization: { type: 'localJson', json: fakeDocForText('initial') },
        });

        let document: ReturnType<typeof useNativeEditorDocument>;

        function Observer() {
            document = useNativeEditorDocument({ handle });

            return null;
        }

        const observer = render(<Observer />);

        for (let index = 0; index < typingBurstLength; index += 1) {
            act(() => {
                handle.bridge.applyInput({
                    text: 'x',
                    baseDocumentRevision: handle.bridge.getState().documentRevision!,
                });

                document!.refresh();
            });
        }

        expect(document!.documentRevision).toBe(handle.bridge.getState().documentRevision);
        expect(document!.historyState).toEqual({ canUndo: true, canRedo: false });
        expect(document!.getContentJson()).toEqual(fakeDocForText(`initial${'x'.repeat(typingBurstLength)}`));
        expect(mockNativeModule.editorV2GetContentSnapshot).not.toHaveBeenCalled();
        observer.unmount();
        handle.destroy();
    });

    it('exports subscribed content once per document revision and skips unchanged renders', () => {
        const handle = createNativeEditorDocumentHandle({
            initialization: { type: 'localJson', json: fakeDocForText('initial') },
        });

        const onContentChange = jest.fn();
        const onContentChangeJSON = jest.fn();
        const onHistoryStateChange = jest.fn();
        const renders = jest.fn();
        let document: ReturnType<typeof useNativeEditorDocument>;

        function Observer() {
            document = useNativeEditorDocument({
                handle,
                onContentChange,
                onContentChangeJSON,
                onHistoryStateChange,
            });

            renders();

            return null;
        }

        const observer = render(<Observer />);
        mockNativeModule.editorV2GetContentSnapshot.mockClear();
        renders.mockClear();
        onHistoryStateChange.mockClear();

        act(() => document!.refresh());

        expect(mockNativeModule.editorV2GetContentSnapshot).not.toHaveBeenCalled();
        expect(renders).not.toHaveBeenCalled();
        expect(onContentChange).not.toHaveBeenCalled();
        expect(onHistoryStateChange).not.toHaveBeenCalled();

        act(() => document!.setContentJson(fakeDocForText('changed')));

        expect(mockNativeModule.editorV2GetContentSnapshot).toHaveBeenCalledTimes(1);
        expect(onContentChange).toHaveBeenCalledWith('<p>changed</p>');
        expect(onContentChangeJSON).toHaveBeenCalledWith(fakeDocForText('changed'));
        expect(onHistoryStateChange).toHaveBeenCalledWith({ canUndo: true, canRedo: false });
        renders.mockClear();

        act(() => document!.refresh());

        expect(mockNativeModule.editorV2GetContentSnapshot).toHaveBeenCalledTimes(1);
        expect(renders).not.toHaveBeenCalled();
        expect(onContentChange).toHaveBeenCalledTimes(1);
        observer.unmount();
        handle.destroy();
    });

    it('publishes JSON-only changes without generating unobserved HTML and caches unchanged refreshes', () => {
        const typingBurstLength = 20;

        const handle = createNativeEditorDocumentHandle({
            initialization: { type: 'localJson', json: fakeDocForText('initial') },
        });

        const onContentChangeJSON = jest.fn();
        let document: ReturnType<typeof useNativeEditorDocument>;

        function Observer() {
            document = useNativeEditorDocument({ handle, onContentChangeJSON });

            return null;
        }

        const observer = render(<Observer />);
        mockNativeModule.editorV2GetDocumentJson.mockClear();

        for (let index = 0; index < typingBurstLength; index += 1) {
            act(() => {
                handle.bridge.applyInput({
                    text: 'x',
                    baseDocumentRevision: handle.bridge.getState().documentRevision!,
                });

                document!.refresh();
                document!.refresh();
            });

            expect(onContentChangeJSON).toHaveBeenLastCalledWith(
                fakeDocForText(`initial${'x'.repeat(index + 1)}`)
            );
        }

        expect(onContentChangeJSON).toHaveBeenCalledTimes(typingBurstLength);
        expect(mockNativeModule.editorV2GetDocumentJson).toHaveBeenCalledTimes(typingBurstLength);
        expect(mockNativeModule.editorV2GetContentSnapshot).not.toHaveBeenCalled();
        expect(mockNativeModule.editorV2GetDocumentHtml).not.toHaveBeenCalled();
        expect(document!.historyState).toEqual({ canUndo: true, canRedo: false });
        observer.unmount();
        handle.destroy();
    });

    it('upgrades a cached JSON revision when HTML becomes observed without emitting a phantom change', () => {
        const handle = createNativeEditorDocumentHandle({
            initialization: { type: 'localJson', json: fakeDocForText('initial') },
        });

        const onContentChangeJSON = jest.fn();
        const onContentChange = jest.fn();
        let document: ReturnType<typeof useNativeEditorDocument>;

        function Observer({ observeHTML }: { observeHTML: boolean }) {
            document = useNativeEditorDocument({
                handle,
                onContentChangeJSON,
                onContentChange: observeHTML ? onContentChange : undefined,
            });

            return null;
        }

        const observer = render(<Observer observeHTML={false} />);

        expect(mockNativeModule.editorV2GetContentSnapshot).not.toHaveBeenCalled();
        observer.rerender(<Observer observeHTML={true} />);
        act(() => document!.refresh());

        expect(mockNativeModule.editorV2GetContentSnapshot).toHaveBeenCalledTimes(1);
        expect(onContentChange).not.toHaveBeenCalled();
        expect(onContentChangeJSON).not.toHaveBeenCalled();

        act(() => document!.setContentJson(fakeDocForText('changed')));

        expect(onContentChange).toHaveBeenCalledWith('<p>changed</p>');
        expect(onContentChangeJSON).toHaveBeenCalledWith(fakeDocForText('changed'));
        expect(mockNativeModule.editorV2GetContentSnapshot).toHaveBeenCalledTimes(2);
        observer.unmount();
        handle.destroy();
    });

    it('reuses the revision snapshot when reconciling controlled content', () => {
        const initialDocument = fakeDocForText('initial');

        const handle = createNativeEditorDocumentHandle({
            initialization: { type: 'localJson', json: initialDocument },
        });

        function Observer() {
            useNativeEditorDocument({ handle, valueJSON: initialDocument });

            return null;
        }

        const observer = render(<Observer />);

        expect(mockNativeModule.editorV2GetDocumentJson).toHaveBeenCalledTimes(1);
        expect(mockNativeModule.editorV2GetContentSnapshot).not.toHaveBeenCalled();
        expect(mockNativeModule.editorV2ApplyLocalApi).not.toHaveBeenCalled();
        observer.unmount();
        handle.destroy();
    });

    it('changes controlled format from JSON to HTML without replacing equal content', () => {
        const initialDocument = fakeDocForText('initial');

        const handle = createNativeEditorDocumentHandle({
            initialization: { type: 'localJson', json: initialDocument },
        });

        function Observer({ controlledHTML }: { controlledHTML: boolean }) {
            useNativeEditorDocument({
                handle,
                value: controlledHTML ? '<p>initial</p>' : undefined,
                valueJSON: controlledHTML ? undefined : initialDocument,
            });

            return null;
        }

        const observer = render(<Observer controlledHTML={false} />);

        expect(mockNativeModule.editorV2GetDocumentJson).toHaveBeenCalledTimes(1);
        expect(mockNativeModule.editorV2GetContentSnapshot).not.toHaveBeenCalled();
        observer.rerender(<Observer controlledHTML={true} />);

        expect(mockNativeModule.editorV2GetContentSnapshot).toHaveBeenCalledTimes(1);
        expect(mockNativeModule.editorV2ApplyLocalApi).not.toHaveBeenCalled();
        expect(handle.bridge.getDocumentJson()).toEqual(initialDocument);
        observer.unmount();
        handle.destroy();
    });

    it('reconciles controlled content when the engine advances after the cached refresh', () => {
        const initialDocument = fakeDocForText('initial');
        const controlledDocument = fakeDocForText('controlled');
        const racedDocument = fakeDocForText('raced');

        const handle = createNativeEditorDocumentHandle({
            initialization: { type: 'localJson', json: initialDocument },
        });

        let document: ReturnType<typeof useNativeEditorDocument>;

        function Observer({ valueJSON }: { valueJSON: typeof initialDocument }) {
            document = useNativeEditorDocument({ handle, valueJSON });

            return null;
        }

        const observer = render(<Observer valueJSON={initialDocument} />);

        act(() => {
            handle.bridge.replaceDocument({ setJson: controlledDocument, history: 'undoableBoundary' });
            document!.refresh();
            handle.bridge.replaceDocument({ setJson: racedDocument, history: 'undoableBoundary' });
            observer.rerender(<Observer valueJSON={controlledDocument} />);
        });

        expect(handle.bridge.getContentSnapshot().json).toEqual(controlledDocument);
        expect(document!.documentRevision).toBe(handle.bridge.getState().documentRevision);
        observer.unmount();
        handle.destroy();
    });

    it('never exposes A state while rebinding to B and ignores a late A refresh', () => {
        const handleA = createNativeEditorDocumentHandle({
            initialization: { type: 'localJson', json: fakeDocForText('alpha') },
        });

        const handleB = createNativeEditorDocumentHandle({
            initialization: { type: 'localJson', json: fakeDocForText('beta') },
        });

        handleA.bridge.replaceDocument({
            setJson: fakeDocForText('alpha changed'),
            history: 'undoableBoundary',
        });

        const historyChanges = jest.fn();
        const contentChanges = jest.fn();

        const renders: Array<{
            editorId: string;
            isReady: boolean;
            documentRevision: string | null;
            historyState: { canUndo: boolean; canRedo: boolean };
            canUndo: boolean;
            content: string;
        }> = [];

        let currentDocument: ReturnType<typeof useNativeEditorDocument> | null = null;
        let refreshA: (() => void) | null = null;

        function SnapshotObserver({ handle }: { handle: NativeEditorDocumentHandle }) {
            const document = useNativeEditorDocument({
                handle,
                onContentChange: contentChanges,
                onHistoryStateChange: historyChanges,
            });

            currentDocument = document;

            if (handle === handleA) {
                refreshA = document.refresh;
            }

            renders.push({
                editorId: handle.editorId,
                isReady: document.isReady,
                documentRevision: document.documentRevision,
                historyState: document.historyState,
                canUndo: document.canUndo(),
                content: document.getContent(),
            });

            return null;
        }

        const observer = render(<SnapshotObserver handle={handleA} />);

        expect(currentDocument!.isReady).toBe(true);
        expect(currentDocument!.historyState).toEqual({ canUndo: true, canRedo: false });

        observer.rerender(<SnapshotObserver handle={handleB} />);
        const firstBRender = renders.find(rendered => rendered.editorId === handleB.editorId);

        expect(firstBRender).toEqual({
            editorId: handleB.editorId,
            isReady: true,
            documentRevision: handleB.bridge.getState().documentRevision,
            historyState: { canUndo: false, canRedo: false },
            canUndo: false,
            content: '<p>beta</p>',
        });

        handleA.bridge.replaceDocument({
            setJson: fakeDocForText('alpha late'),
            history: 'undoableBoundary',
        });

        act(() => refreshA!());

        expect(contentChanges).not.toHaveBeenCalled();
        expect(historyChanges).not.toHaveBeenLastCalledWith({ canUndo: true, canRedo: false });

        expect(currentDocument).toMatchObject({
            isReady: true,
            documentRevision: handleB.bridge.getState().documentRevision,
            historyState: { canUndo: false, canRedo: false },
        });

        expect(currentDocument!.getContent()).toBe('<p>beta</p>');
        handleA.destroy();
        handleB.destroy();
    });
});

describe('useNativeEditorDocument Markdown reads', () => {
    beforeEach(() => {
        _resetNativeModuleCache();
        Object.assign(mockNativeModule, createFakeNativeEditorV2Runtime().module);
        mockNativeModule.editorV2GetDocumentMarkdown = jest.fn(() => ({ value: JSON.stringify({ markdown: 'initial' }), error: null }));
    });

    it('reads the current engine content with default/explicit flavor and stops after destruction', () => {
        const handle = createNativeEditorDocumentHandle({ initialization: { type: 'localJson', json: fakeDocForText('initial') } });
        let document: ReturnType<typeof useNativeEditorDocument>;

        function Observer() {
            document = useNativeEditorDocument({ handle });

            return null;
        }

        const observer = render(<Observer />);
        expect(document!.getContentMarkdown()).toBe('initial');
        expect(mockNativeModule.editorV2GetDocumentMarkdown).toHaveBeenLastCalledWith(handle.editorId, 'commonmark');
        mockNativeModule.editorV2GetDocumentMarkdown.mockReturnValue({ value: JSON.stringify({ markdown: 'current **content**' }), error: null });
        expect(document!.getContentMarkdown({ flavor: 'gfm' })).toBe('current **content**');
        expect(mockNativeModule.editorV2GetDocumentMarkdown).toHaveBeenLastCalledWith(handle.editorId, 'gfm');
        handle.destroy();
        expect(document!.getContentMarkdown()).toBe('');
        expect(mockNativeModule.editorV2GetDocumentMarkdown).toHaveBeenCalledTimes(2);
        observer.unmount();
    });

    it('does not request content while awaiting a room document', () => {
        const handle = createNativeEditorDocumentHandle({ initialization: { type: 'room', documentId: 'markdown-room', lineageId: 'markdown-lineage' } });
        let document: ReturnType<typeof useNativeEditorDocument>;

        function Observer() {
            document = useNativeEditorDocument({ handle });

            return null;
        }

        const observer = render(<Observer />);
        expect(document!.getContentMarkdown({ flavor: 'gfm' })).toBe('');
        expect(mockNativeModule.editorV2GetDocumentMarkdown).not.toHaveBeenCalled();
        observer.unmount();
        handle.destroy();
    });
});
