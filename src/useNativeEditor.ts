import { useCallback, useEffect, useMemo, useRef, useState } from 'react';

import {
    _assertNativeEditorDocumentHandle,
    _getNativeEditorDocumentHandleDescriptor,
    type DocumentJSON,
    type MarkdownExportOptions,
    type HistoryState,
    type NativeEditorDocumentHandle,
    type NativeEditorDocumentOrigin,
    type NativeEditorDocumentState,
} from './NativeEditorBridge';
import { NativeEditorOperationError } from './NativeEditorBoundaryError';

// Controlled writes use the last rendered revision; conflicts refresh before retrying.

/** A newly observed document revision; content is read separately through getters. */
export interface NativeEditorDocumentChangeEvent {
    documentRevision: string;
    origin: NativeEditorDocumentOrigin;
}

/** Configuration for {@link useNativeEditorDocument}. */
export interface UseNativeEditorDocumentOptions {
    /** The shared document session. The binding never creates or destroys it. */
    handle: NativeEditorDocumentHandle;
    /** Controlled HTML content. */
    value?: string;
    /** Controlled ProseMirror JSON content (already normalized by the caller). Ignored when value is set. */
    valueJSON?: DocumentJSON;
    /** History policy for controlled replacements. Defaults to 'replace'. */
    valueJSONUpdateMode?: 'replace' | 'reset';
    /** External revision signal (e.g. from the collaboration controller) forcing an engine re-read. */
    revisionSignal?: string | null;
    /** Revision notification without content exports, including controlled and remote changes. */
    onDocumentChange?: (event: NativeEditorDocumentChangeEvent) => void;
    onContentChange?: (html: string) => void;
    onContentChangeJSON?: (json: DocumentJSON) => void;
    onHistoryStateChange?: (state: HistoryState) => void;
    /** Application notification after a successful local mutation. Transport does not depend on it. */
    onLocalCommit?: () => void;
}

/** What {@link useNativeEditorDocument} returns. */
export interface UseNativeEditorDocumentReturn {
    /** True once the engine document is ready (LocalReady/RoomReady). */
    isReady: boolean;
    /** Raw engine readiness. Null before the first read. */
    documentState: NativeEditorDocumentState | null;
    /** Decimal-string engine document revision; null while awaiting the server document. */
    documentRevision: string | null;
    /** Trusted origin paired with documentRevision. */
    documentOrigin: NativeEditorDocumentOrigin | null;
    /** Current undo/redo availability. */
    historyState: HistoryState;
    /**
     * Re-read the engine now, emitting content/history callbacks when the
     * engine advanced. The interactive editor calls this when the native
     * view reports an adapter-driven commit (typing, toolbar commands).
     */
    refresh(): void;
    /** Current document as HTML. Empty until the document is ready. */
    getContent(): string;
    /** Current document as ProseMirror JSON. Empty until the document is ready. */
    getContentJson(): DocumentJSON;
    /** Export current content as vanilla CommonMark, or explicit GFM. */
    getContentMarkdown(options?: MarkdownExportOptions): string;
    /** The engine's own answer for whether the document holds no content. */
    getIsEmpty(): boolean;
    /** Concatenated document text, without markup, atom labels, or added block separators. */
    getTextContent(): string;
    /** Replace the document with HTML, as one undoable step. */
    setContent(html: string): void;
    /** Replace the document with JSON, as one undoable step. */
    setContentJson(doc: DocumentJSON): void;
    /** Reset to the schema's empty document and clear undo history. */
    clearContent(): void;
    undo(): void;
    redo(): void;
    canUndo(): boolean;
    canRedo(): boolean;
}

const DEFAULT_V2_HISTORY_STATE: HistoryState = { canUndo: false, canRedo: false };

function isRevisionMismatchError(error: unknown): boolean {
    return error instanceof NativeEditorOperationError && error.code === 'REVISION_MISMATCH';
}

interface V2EngineView {
    editorId: string;
    ready: boolean;
    documentState: NativeEditorDocumentState | null;
    documentRevision: string | null;
    documentOrigin: NativeEditorDocumentOrigin | null;
    historyState: HistoryState;
}

/**
 * Headless binding to a document handle: the same controlled content,
 * getters, and history the editor exposes, with no view attached. Use it to
 * read or drive a document that is not currently mounted : a preview, a
 * background save, a test.
 *
 * `RichTextEditor` uses this internally, so mounting an editor on the
 * same handle does not conflict with it.
 */
export function useNativeEditorDocument(
    options: UseNativeEditorDocumentOptions
): UseNativeEditorDocumentReturn {
    const {
        handle,
        value,
        valueJSON,
        valueJSONUpdateMode = 'replace',
        revisionSignal,
        onLocalCommit,
    } = options;

    _assertNativeEditorDocumentHandle(handle);
    const editorId = handle.editorId;

    const onDocumentChangeRef = useRef(options.onDocumentChange);
    onDocumentChangeRef.current = options.onDocumentChange;
    const onContentChangeRef = useRef(options.onContentChange);
    onContentChangeRef.current = options.onContentChange;
    const onContentChangeJSONRef = useRef(options.onContentChangeJSON);
    onContentChangeJSONRef.current = options.onContentChangeJSON;
    const onHistoryStateChangeRef = useRef(options.onHistoryStateChange);
    onHistoryStateChangeRef.current = options.onHistoryStateChange;
    const onLocalCommitRef = useRef(onLocalCommit);
    onLocalCommitRef.current = onLocalCommit;

    const [ engineView, setEngineView ] = useState<V2EngineView>({
        editorId,
        ready: false,
        documentState: null,
        documentRevision: null,
        documentOrigin: null,
        historyState: DEFAULT_V2_HISTORY_STATE,
    });

    const engineViewRef = useRef(engineView);

    const currentEditorIdRef = useRef(editorId);
    currentEditorIdRef.current = editorId;
    const readyRef = useRef({ editorId, ready: false });

    const contentSnapshotRef = useRef<{
        editorId: string;
        documentRevision: string;
        snapshot: { json: DocumentJSON; html?: string };
        contentKey: string;
    } | null>(null);

    const emittedRef = useRef({
        editorId,
        lastRevision: null as string | null,
        lastContentKey: null as string | null,
        lastHistoryKey: null as string | null,
    });

    const pendingControlledEchoesRef = useRef({
        editorId,
        keys: [] as string[],
    });

    const controlledKindRef = useRef<'html' | 'json' | null>(null);
    controlledKindRef.current = value != null ? 'html' : valueJSON != null ? 'json' : null;
    const ignoredControlledEchoRef = useRef<string | null>(null);
    const lastControlledContentKeyRef = useRef<string | null>(null);

    if (readyRef.current.editorId !== editorId) {
        readyRef.current = { editorId, ready: false };
    }

    if (emittedRef.current.editorId !== editorId) {
        emittedRef.current = {
            editorId,
            lastRevision: null,
            lastContentKey: null,
            lastHistoryKey: null,
        };

        pendingControlledEchoesRef.current = { editorId, keys: [] };
        ignoredControlledEchoRef.current = null;
        lastControlledContentKeyRef.current = null;
        contentSnapshotRef.current = null;
    }

    const readContentSnapshot = useCallback((documentRevision: string) => {
        const cached = contentSnapshotRef.current;
        const needsHtml = controlledKindRef.current === 'html' || onContentChangeRef.current != null;

        if (cached?.editorId === handle.editorId && cached.documentRevision === documentRevision &&
            (!needsHtml || cached.snapshot.html != null)) {
            return cached;
        }

        const snapshot = needsHtml
            ? handle.bridge.getContentSnapshot()
            : { json: handle.bridge.getDocumentJson(), html: undefined };

        const nextSnapshot = {
            editorId: handle.editorId,
            documentRevision,
            snapshot,
            contentKey: JSON.stringify(snapshot.json),
        };

        contentSnapshotRef.current = nextSnapshot;

        return nextSnapshot;
    }, [ handle ]);

    const refresh = useCallback(
        (emitContentCallbacks: boolean) => {
            const refreshedEditorId = handle.editorId;

            if (handle.isDestroyed || currentEditorIdRef.current !== refreshedEditorId) {
                return;
            }

            const state = handle.bridge.getState();
            const ready = state.documentState !== 'AwaitRemote';
            let contentKey: string | null = null;
            let snapshotHtml: string | null = null;
            let snapshotJson: DocumentJSON | null = null;

            const needsContent = controlledKindRef.current != null ||
                onContentChangeRef.current != null || onContentChangeJSONRef.current != null;

            if (ready && state.documentRevision != null && needsContent) {
                const { snapshot, contentKey: snapshotKey } = readContentSnapshot(state.documentRevision);
                snapshotHtml = snapshot.html ?? null;
                snapshotJson = snapshot.json;
                contentKey = snapshotKey;
            } else {
                contentSnapshotRef.current = null;
            }

            if (currentEditorIdRef.current !== refreshedEditorId) {
                return;
            }

            const historyState: HistoryState = {
                canUndo: state.canUndo,
                canRedo: state.canRedo,
            };

            readyRef.current = { editorId: refreshedEditorId, ready };

            const previous = engineViewRef.current;

            if (previous.editorId !== refreshedEditorId || previous.ready !== ready ||
                previous.documentState !== state.documentState ||
                previous.documentRevision !== state.documentRevision ||
                previous.documentOrigin !== state.documentOrigin ||
                previous.historyState.canUndo !== historyState.canUndo ||
                previous.historyState.canRedo !== historyState.canRedo) {
                const nextView = {
                    editorId: refreshedEditorId,
                    ready,
                    documentState: state.documentState,
                    documentRevision: state.documentRevision,
                    documentOrigin: state.documentOrigin,
                    historyState,
                };

                engineViewRef.current = nextView;
                setEngineView(nextView);
            }

            const historyKey = `${state.canUndo}/${state.canRedo}`;
            const emitted = emittedRef.current;

            if (handle.isDestroyed || currentEditorIdRef.current !== refreshedEditorId ||
                emitted.editorId !== refreshedEditorId) {
                return;
            }

            const lastRevision = emitted.lastRevision;
            const lastContentKey = emitted.lastContentKey;

            if (ready) {
                emitted.lastRevision = state.documentRevision;
                emitted.lastContentKey = contentKey;
            }

            const admittedRevision = emitted.lastRevision;

            const isCurrentRefresh = () => !handle.isDestroyed &&
                currentEditorIdRef.current === refreshedEditorId && emittedRef.current === emitted &&
                emitted.lastRevision === admittedRevision;

            if (historyKey !== emitted.lastHistoryKey) {
                emitted.lastHistoryKey = historyKey;
                onHistoryStateChangeRef.current?.(historyState);
            }

            if (!isCurrentRefresh()) {
                return;
            }

            if (ready && lastRevision != null && lastRevision !== state.documentRevision) {
                onDocumentChangeRef.current?.({
                    documentRevision: state.documentRevision,
                    origin: state.documentOrigin,
                });
            }

            if (!isCurrentRefresh()) {
                return;
            }

            if (ready) {
                if (
                    emitContentCallbacks &&
                    lastRevision != null &&
                    lastRevision !== state.documentRevision &&
                    lastContentKey !== contentKey
                ) {
                    if (snapshotHtml != null) {
                        if (
                            controlledKindRef.current === 'html' &&
                            onContentChangeRef.current != null
                        ) {
                            const echoes = pendingControlledEchoesRef.current.keys;
                            echoes.push(`html:${snapshotHtml}`);

                            if (echoes.length > 32) {
                                echoes.shift();
                            }
                        }

                        onContentChangeRef.current?.(snapshotHtml);
                    }

                    if (!isCurrentRefresh()) {
                        return;
                    }

                    if (snapshotJson != null) {
                        if (
                            controlledKindRef.current === 'json' &&
                            onContentChangeJSONRef.current != null
                        ) {
                            const echoes = pendingControlledEchoesRef.current.keys;
                            echoes.push(`json:${contentKey}`);

                            if (echoes.length > 32) {
                                echoes.shift();
                            }
                        }

                        onContentChangeJSONRef.current?.(snapshotJson);
                    }
                }
            }
        },
        [ handle, readContentSnapshot ]
    );

    useEffect(() => {
        refresh(true);
    }, [ refresh, revisionSignal ]);

    const refreshFromEngine = useCallback(() => {
        refresh(true);
    }, [ refresh ]);

    const serializedControlledJson = useMemo(
        () => (value == null && valueJSON != null ? JSON.stringify(valueJSON) : null),
        [ value, valueJSON ]
    );

    // Controlled document flow: external value/valueJSON changes lower to one
    // local-API replacement carrying the rendered engine revision as its
    // base. Content callbacks stay suppressed (controlled sync parity).
    useEffect(() => {
        if (engineView.editorId !== editorId || !engineView.ready || handle.isDestroyed) {
            return;
        }

        if (engineView.documentRevision == null) {
            return;
        }

        const wantsHtml = value != null;
        const wantsJson = value == null && serializedControlledJson != null;

        if (!wantsHtml && !wantsJson) {
            return;
        }

        const controlledContentKey = wantsHtml
            ? `html:${value}`
            : `json:${serializedControlledJson}`;

        const controlledContentChanged =
            lastControlledContentKeyRef.current !== controlledContentKey;

        if (controlledContentChanged) {
            lastControlledContentKeyRef.current = controlledContentKey;
            ignoredControlledEchoRef.current = null;
        }

        if (ignoredControlledEchoRef.current === controlledContentKey) {
            return;
        }

        if (!controlledContentChanged && pendingControlledEchoesRef.current.keys.length > 0) {
            return;
        }

        const snapshotRevision = handle.bridge.getState().documentRevision ?? engineView.documentRevision;
        const { snapshot, contentKey: currentJsonKey } = readContentSnapshot(snapshotRevision);

        if (currentEditorIdRef.current !== editorId || emittedRef.current.editorId !== editorId) {
            return;
        }

        if (wantsHtml && snapshot.html === value) {
            const echoIndex = pendingControlledEchoesRef.current.keys.indexOf(controlledContentKey);

            if (echoIndex >= 0) {
                pendingControlledEchoesRef.current.keys.splice(0, echoIndex + 1);
            }

            emittedRef.current.lastRevision = engineView.documentRevision;
            emittedRef.current.lastContentKey = currentJsonKey;

            return;
        }

        if (wantsJson && currentJsonKey === serializedControlledJson) {
            const echoIndex = pendingControlledEchoesRef.current.keys.indexOf(controlledContentKey);

            if (echoIndex >= 0) {
                pendingControlledEchoesRef.current.keys.splice(0, echoIndex + 1);
            }

            emittedRef.current.lastRevision = engineView.documentRevision;
            emittedRef.current.lastContentKey = currentJsonKey;

            return;
        }

        const echoIndex = pendingControlledEchoesRef.current.keys.indexOf(controlledContentKey);

        if (echoIndex >= 0) {
            pendingControlledEchoesRef.current.keys.splice(0, echoIndex + 1);
            ignoredControlledEchoRef.current = controlledContentKey;

            return;
        }

        try {
            pendingControlledEchoesRef.current.keys = [];

            handle.bridge.applyLocalApi({
                ...(wantsHtml
                    ? { setHtml: value }
                    : { setJson: JSON.parse(serializedControlledJson!) as DocumentJSON }),
                history: valueJSONUpdateMode === 'reset' ? 'resetAndClear' : 'undoableBoundary',
                baseDocumentRevision: engineView.documentRevision,
            });

            if (currentEditorIdRef.current !== editorId) {
                return;
            }

            onLocalCommitRef.current?.();
            refresh(false);
        } catch (error) {
            if (isRevisionMismatchError(error)) {
                // Refresh from the engine; the effect re-applies against the
                // fresh revision (never a guessed one).
                refresh(false);

                return;
            }

            throw error;
        }
    }, [
        handle,
        value,
        serializedControlledJson,
        valueJSONUpdateMode,
        editorId,
        engineView.editorId,
        engineView.ready,
        engineView.documentRevision,
        refresh,
        readContentSnapshot,
    ]);

    const mutate = useCallback(
        (operation: () => unknown) => {
            if (handle.isDestroyed || currentEditorIdRef.current !== handle.editorId) {
                if (__DEV__) {
                    console.error(
                        'useNativeEditorDocument: dropped a mutation because the handle is not the current binding',
                        {
                            destroyed: handle.isDestroyed,
                            boundEditorId: currentEditorIdRef.current,
                            handleEditorId: handle.editorId,
                        }
                    );
                }

                return;
            }

            operation();

            if (currentEditorIdRef.current !== handle.editorId) {
                return;
            }

            onLocalCommitRef.current?.();
            refresh(true);
        },
        [ handle, refresh ]
    );

    const setContent = useCallback(
        (html: string) => {
            mutate(() =>
                handle.bridge.replaceDocument({ setHtml: html, history: 'undoableBoundary' }));
        },
        [ handle, mutate ]
    );

    const setContentJson = useCallback(
        (doc: DocumentJSON) => {
            mutate(() =>
                handle.bridge.replaceDocument({ setJson: doc, history: 'undoableBoundary' }));
        },
        [ handle, mutate ]
    );

    const clearContent = useCallback(() => {
        mutate(() => {
            return handle.bridge.replaceDocument({
                setJson: _getNativeEditorDocumentHandleDescriptor(handle).emptyDocument,
                history: 'resetAndClear',
            });
        });
    }, [ handle, mutate ]);

    const undo = useCallback(() => {
        mutate(() => handle.bridge.undo());
    }, [ handle, mutate ]);

    const redo = useCallback(() => {
        mutate(() => handle.bridge.redo());
    }, [ handle, mutate ]);

    const getContent = useCallback((): string => {
        if (
            handle.isDestroyed ||
            readyRef.current.editorId !== handle.editorId ||
            !readyRef.current.ready
        ) {
            return '';
        }

        return handle.bridge.getDocumentHtml();
    }, [ handle ]);

    const getContentJson = useCallback((): DocumentJSON => {
        if (
            handle.isDestroyed ||
            readyRef.current.editorId !== handle.editorId ||
            !readyRef.current.ready
        ) {
            return {};
        }

        return handle.bridge.getDocumentJson();
    }, [ handle ]);

    const getContentMarkdown = useCallback((options?: MarkdownExportOptions): string => {
        if (
            handle.isDestroyed ||
            readyRef.current.editorId !== handle.editorId ||
            !readyRef.current.ready
        ) {
            return '';
        }

        return handle.bridge.getDocumentMarkdown(options);
    }, [ handle ]);

    const getIsEmpty = useCallback((): boolean => {
        if (
            handle.isDestroyed ||
            readyRef.current.editorId !== handle.editorId ||
            !readyRef.current.ready
        ) {
            return true;
        }

        return handle.bridge.getDocumentIsEmpty();
    }, [ handle ]);

    const getTextContent = useCallback((): string => {
        if (
            handle.isDestroyed ||
            readyRef.current.editorId !== handle.editorId ||
            !readyRef.current.ready
        ) {
            return '';
        }

        return handle.bridge.getDocumentText();
    }, [ handle ]);

    const canUndo = useCallback((): boolean => {
        if (
            handle.isDestroyed ||
            readyRef.current.editorId !== handle.editorId ||
            !readyRef.current.ready
        ) {
            return false;
        }

        return handle.bridge.getState().canUndo;
    }, [ handle ]);

    const canRedo = useCallback((): boolean => {
        if (
            handle.isDestroyed ||
            readyRef.current.editorId !== handle.editorId ||
            !readyRef.current.ready
        ) {
            return false;
        }

        return handle.bridge.getState().canRedo;
    }, [ handle ]);

    let renderedEngineView = engineView;

    if (engineView.editorId !== editorId) {
        if (handle.isDestroyed) {
            renderedEngineView = {
                editorId,
                ready: false,
                documentState: null,
                documentRevision: null,
                documentOrigin: null,
                historyState: DEFAULT_V2_HISTORY_STATE,
            };
        } else {
            const state = handle.bridge.getState();

            renderedEngineView = {
                editorId,
                ready: state.documentState !== 'AwaitRemote',
                documentState: state.documentState,
                documentRevision: state.documentRevision,
                documentOrigin: state.documentOrigin,
                historyState: { canUndo: state.canUndo, canRedo: state.canRedo },
            };
        }

        readyRef.current = { editorId, ready: renderedEngineView.ready };
    }

    return {
        isReady: renderedEngineView.ready,
        documentState: renderedEngineView.documentState,
        documentRevision: renderedEngineView.documentRevision,
        documentOrigin: renderedEngineView.documentOrigin,
        historyState: renderedEngineView.historyState,
        refresh: refreshFromEngine,
        getContent,
        getContentJson,
        getContentMarkdown,
        getIsEmpty,
        getTextContent,
        setContent,
        setContentJson,
        clearContent,
        undo,
        redo,
        canUndo,
        canRedo,
    };
}
