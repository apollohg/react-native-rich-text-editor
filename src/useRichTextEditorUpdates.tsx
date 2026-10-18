import { useCallback, useEffect, useLayoutEffect } from 'react';
import {
    type NativeEditorAtomicRenderSnapshot,
    type NativeEditorAtomicRenderState,
    type NativeEditorPositionAffinity,
    type Selection,
} from './NativeEditorBridge';
import { applyRenderPatch, collectAtomInstanceBlocks, retainVoidBlockElements } from './atomInstances';
import { allocateEditorUpdateRevision } from './EditorUpdateRevision';
import { normalizeNativeEditorV2U64 } from './NativeEditorV2Decimal';
import { setActiveEditorToolbarFrameOwnerForEditor } from './EditorToolbar';
import { type useRichTextEditorState } from './useRichTextEditorState';
import {
    selectedAtomKeys,
    equalStringSets,
    equalAtomInstances,
    stringifyCachedJson,
    serializeRenderNotice,
    isRecord,
    parseSelectionFromUpdate,
    parseActiveStateFromUpdate,
    isPositionInvalidError,
} from './RichTextEditorSerialization';

export function useRichTextEditorUpdates(
    context: Pick<
        ReturnType<typeof useRichTextEditorState>,
        | 'atomStateRef'
        | 'setSelectedKeys'
        | 'bridge'
        | 'registeredAtomTypes'
        | 'setAtomState'
        | 'selectionRef'
        | 'document'
        | 'documentHandle'
        | 'renderSeedEditorIdRef'
        | 'editorId'
        | 'scalarSelectionRef'
        | 'activeStateRef'
        | 'setActiveState'
        | 'activeStateKeyRef'
        | 'selectionEventActiveStateRef'
        | 'onActiveStateChangeRef'
        | 'pushedUpdateBindingGenerationRef'
        | 'currentPushedUpdateEditorIdRef'
        | 'pushRevisionRef'
        | 'lastPushedEngineRevisionRef'
        | 'setPushedUpdate'
        | 'pendingFocusedRebindRef'
        | 'toolbarFrameOwnerId'
        | 'onFocusRef'
        | 'didRebindRevisionScope'
        | 'didObserveInitialRevisionRef'
        | 'lastNativeDrivenRevisionRef'
        | 'latestPresentedStateRevisionRef'
        | 'onLocalCommitRef'
    >
) {
    const {
        atomStateRef,
        setSelectedKeys,
        bridge,
        registeredAtomTypes,
        setAtomState,
        selectionRef,
        document,
        documentHandle,
        renderSeedEditorIdRef,
        editorId,
        scalarSelectionRef,
        activeStateRef,
        setActiveState,
        activeStateKeyRef,
        selectionEventActiveStateRef,
        onActiveStateChangeRef,
        pushedUpdateBindingGenerationRef,
        currentPushedUpdateEditorIdRef,
        pushRevisionRef,
        lastPushedEngineRevisionRef,
        setPushedUpdate,
        pendingFocusedRebindRef,
        toolbarFrameOwnerId,
        onFocusRef,
        didRebindRevisionScope,
        didObserveInitialRevisionRef,
        lastNativeDrivenRevisionRef,
        latestPresentedStateRevisionRef,
        onLocalCommitRef,
    } = context;

    const stateBindingGeneration = pushedUpdateBindingGenerationRef.current;

    const isCurrentStateBinding = useCallback(
        () => currentPushedUpdateEditorIdRef.current === editorId &&
            pushedUpdateBindingGenerationRef.current === stateBindingGeneration,
        [ currentPushedUpdateEditorIdRef, editorId, pushedUpdateBindingGenerationRef, stateBindingGeneration ]
    );

    const updateAtomSelection = useCallback(
        (selection: Selection, instances = atomStateRef.current.instances) => {
            const next = selectedAtomKeys(selection, instances);
            setSelectedKeys(current => (equalStringSets(current, next) ? current : next));
        },
        [ atomStateRef, setSelectedKeys ]
    );

    const refreshAtomsFromUpdate = useCallback(
        (update: NativeEditorAtomicRenderSnapshot | NativeEditorAtomicRenderState) => {
            if (!isCurrentStateBinding()) {
                return;
            }

            const previous = atomStateRef.current;

            const source =
                'renderPatch' in update &&
                update.renderPatch != null &&
                update.renderPatch.baseDocumentVersion !== previous.documentVersion
                    ? bridge.renderState()
                    : update;

            if (!isCurrentStateBinding()) {
                return;
            }

            const renderPatch = 'renderPatch' in source ? source.renderPatch : null;

            const blocks =
                'voidBlocks' in source
                    ? source.voidBlocks
                    : source.renderBlocks != null
                        ? retainVoidBlockElements(source.renderBlocks)
                        : applyRenderPatch(previous.blocks, {
                            startIndex: source.renderPatch.startIndex,
                            deleteCount: source.renderPatch.deleteCount,
                            renderBlocks: retainVoidBlockElements(source.renderPatch.renderBlocks),
                        });

            const patchedCollection =
                renderPatch != null && previous.hasOnlyStableAtomKeys
                    ? collectAtomInstanceBlocks(renderPatch.renderBlocks, registeredAtomTypes)
                    : null;

            const collection =
                renderPatch != null && patchedCollection?.hasOnlyStableKeys
                    ? {
                        instanceBlocks: applyRenderPatch(previous.instanceBlocks, {
                            startIndex: renderPatch.startIndex,
                            deleteCount: renderPatch.deleteCount,
                            renderBlocks: patchedCollection.instanceBlocks,
                        }),
                        hasOnlyStableKeys: true,
                    }
                    : collectAtomInstanceBlocks(blocks, registeredAtomTypes);

            const instances = collection.instanceBlocks.flat();

            const next = {
                blocks,
                instanceBlocks: collection.instanceBlocks,
                instances,
                documentVersion: source.documentVersion,
                hasOnlyStableAtomKeys: collection.hasOnlyStableKeys,
            };

            atomStateRef.current = next;

            if (!equalAtomInstances(previous.instances, next.instances)) {
                setAtomState(next);
            }

            if (isCurrentStateBinding()) {
                updateAtomSelection(selectionRef.current, next.instances);
            }
        },
        [ atomStateRef,
            bridge,
            isCurrentStateBinding,
            registeredAtomTypes,
            selectionRef,
            setAtomState,
            updateAtomSelection ]
    );

    useEffect(() => {
        const current = atomStateRef.current;
        const collection = collectAtomInstanceBlocks(current.blocks, registeredAtomTypes);
        const instances = collection.instances;

        const next = {
            blocks: current.blocks,
            instanceBlocks: collection.instanceBlocks,
            instances,
            documentVersion: current.documentVersion,
            hasOnlyStableAtomKeys: collection.hasOnlyStableKeys,
        };

        atomStateRef.current = next;

        if (!equalAtomInstances(current.instances, instances)) {
            setAtomState(next);
        }

        updateAtomSelection(selectionRef.current, instances);
    }, [ atomStateRef,
        registeredAtomTypes,
        selectionRef,
        setAtomState,
        updateAtomSelection ]);

    const admitStateRevision = useCallback(
        (value: unknown) => {
            if (!isCurrentStateBinding()) {
                return false;
            }

            if (value === undefined) {
                return true;
            }

            const revision = normalizeNativeEditorV2U64(value);
            const latest = latestPresentedStateRevisionRef.current;

            if (revision == null || (latest != null && BigInt(revision) < BigInt(latest))) {
                return false;
            }

            latestPresentedStateRevisionRef.current = revision;

            return true;
        },
        [ isCurrentStateBinding, latestPresentedStateRevisionRef ]
    );

    const isCurrentStateRevision = useCallback(
        (value: string | null) => isCurrentStateBinding() && value === latestPresentedStateRevisionRef.current,
        [ isCurrentStateBinding, latestPresentedStateRevisionRef ]
    );

    const applyTypedUpdateState = useCallback(
        (
            update: Pick<NativeEditorAtomicRenderSnapshot, 'selection' | 'activeState'> & { stateRevision?: unknown },
            isCurrent: () => boolean = () => true
        ) => {
            if (!isCurrent() || !admitStateRevision(update.stateRevision)) {
                return false;
            }

            const admittedRevision = latestPresentedStateRevisionRef.current;
            const isCurrentState = () => isCurrent() && isCurrentStateRevision(admittedRevision);

            selectionRef.current = update.selection;
            updateAtomSelection(update.selection);

            if (!isCurrentState()) {
                return false;
            }

            if (
                update.selection.type === 'text' &&
                update.selection.anchorScalar != null &&
                update.selection.headScalar != null
            ) {
                scalarSelectionRef.current = {
                    anchor: update.selection.anchorScalar,
                    head: update.selection.headScalar,
                };
            }

            const nextActiveState = update.activeState;
            activeStateRef.current = nextActiveState;

            if (!isCurrentState()) {
                return false;
            }

            const key = stringifyCachedJson(nextActiveState);

            if (key !== activeStateKeyRef.current) {
                setActiveState(nextActiveState);

                if (!isCurrentState()) {
                    return false;
                }

                activeStateKeyRef.current = key;

                if (!isCurrentState()) {
                    return false;
                }

                onActiveStateChangeRef.current?.(nextActiveState);
            }

            return isCurrentState();
        },
        [ activeStateKeyRef,
            activeStateRef,
            admitStateRevision,
            isCurrentStateRevision,
            latestPresentedStateRevisionRef,
            onActiveStateChangeRef,
            scalarSelectionRef,
            selectionRef,
            setActiveState,
            updateAtomSelection ]
    );

    useLayoutEffect(() => {
        if (
            !document.isReady ||
            documentHandle.isDestroyed ||
            renderSeedEditorIdRef.current === editorId
        ) {
            return;
        }

        renderSeedEditorIdRef.current = editorId;
        const snapshot = bridge.renderState();
        refreshAtomsFromUpdate(snapshot);
        applyTypedUpdateState(snapshot);
    }, [ renderSeedEditorIdRef,
        applyTypedUpdateState,
        bridge,
        document.isReady,
        documentHandle,
        editorId,
        refreshAtomsFromUpdate ]);

    const applyParsedUpdateState = useCallback(
        (parsed: Record<string, unknown>) => {
            const nextSelection = parseSelectionFromUpdate(parsed.selection);
            const nextActiveState = parseActiveStateFromUpdate(parsed.activeState);

            if ((parsed.stateRevision !== undefined && !nextSelection && !nextActiveState) ||
                !admitStateRevision(parsed.stateRevision)) {
                return null;
            }

            const admittedRevision = latestPresentedStateRevisionRef.current;

            if (nextSelection && nextActiveState) {
                if (!applyTypedUpdateState({
                    selection: nextSelection,
                    activeState: nextActiveState,
                    stateRevision: parsed.stateRevision,
                })) {
                    return null;
                }
            } else {
                if (nextSelection) {
                    selectionRef.current = nextSelection;
                } else if (nextActiveState) {
                    activeStateRef.current = nextActiveState;
                    const key = stringifyCachedJson(nextActiveState);

                    if (key !== activeStateKeyRef.current) {
                        setActiveState(nextActiveState);

                        if (!isCurrentStateRevision(admittedRevision)) {
                            return null;
                        }

                        activeStateKeyRef.current = key;
                        onActiveStateChangeRef.current?.(nextActiveState);
                    }
                }
            }

            return isCurrentStateRevision(admittedRevision) ? parsed : null;
        },
        [ activeStateKeyRef,
            activeStateRef,
            admitStateRevision,
            applyTypedUpdateState,
            isCurrentStateRevision,
            latestPresentedStateRevisionRef,
            onActiveStateChangeRef,
            selectionRef,
            setActiveState ]
    );

    const applyUpdateState = useCallback(
        (updateJson: string | null | undefined) => {
            const parsed = parseUpdateStateJson(updateJson);

            return parsed == null ? null : applyParsedUpdateState(parsed);
        },
        [ applyParsedUpdateState ]
    );

    const applySelectionEventState = useCallback(
        (stateJson: string | null | undefined) => {
            const parsed = parseUpdateStateJson(stateJson);

            if (parsed == null) {
                return null;
            }

            const { activeStateRevision, ...state } = parsed;

            if (state.activeState !== undefined) {
                selectionEventActiveStateRef.current = {
                    stateRevision: state.stateRevision,
                    activeState: state.activeState,
                };
            } else if (typeof activeStateRevision === 'string') {
                const emitted = selectionEventActiveStateRef.current;

                state.activeState = emitted?.stateRevision === activeStateRevision
                    ? emitted.activeState
                    : bridge.renderState().activeState;
            }

            return applyParsedUpdateState(state);
        },
        [ applyParsedUpdateState, bridge, selectionEventActiveStateRef ]
    );

    const pushEngineUpdateToView = useCallback(() => {
        if (documentHandle.isDestroyed) {
            return;
        }

        const sourceEditorId = documentHandle.editorId;
        const sourceBindingGeneration = pushedUpdateBindingGenerationRef.current;

        const isCurrentSource = () => {
            const current =
                sourceEditorId === currentPushedUpdateEditorIdRef.current &&
                sourceBindingGeneration === pushedUpdateBindingGenerationRef.current;

            return current;
        };

        const allocation = allocateEditorUpdateRevision(pushRevisionRef.current);

        if ('error' in allocation) {
            bridge._emitAutonomousError(allocation.error);

            return;
        }

        const reset = bridge._pendingViewReset;
        selectionEventActiveStateRef.current = null;
        const snapshot = bridge.renderState();

        // renderState can synchronously re-enter React and bind this
        // component to another handle. Do not let any side effect from A
        // reach B once that happens.
        if (!isCurrentSource()) {
            return;
        }

        refreshAtomsFromUpdate(snapshot);

        if (!isCurrentSource()) {
            return;
        }

        const updateJson = serializeRenderNotice(snapshot);

        if (!isCurrentSource()) {
            return;
        }

        if (!applyTypedUpdateState(snapshot, isCurrentSource)) {
            return;
        }

        if (!isCurrentSource()) {
            return;
        }

        lastPushedEngineRevisionRef.current = snapshot.documentVersion;

        if (!isCurrentSource()) {
            return;
        }

        pushRevisionRef.current = allocation.revision;

        if (!isCurrentSource()) {
            return;
        }

        setPushedUpdate({
            json: updateJson,
            resetJson: reset?.json,
            revision: allocation.revision,
            editorId: sourceEditorId,
        });

        if (bridge._pendingViewReset === reset) {
            bridge._pendingViewReset = null;
        }
    }, [ applyTypedUpdateState,
        bridge,
        currentPushedUpdateEditorIdRef,
        documentHandle.editorId,
        documentHandle.isDestroyed,
        lastPushedEngineRevisionRef,
        pushRevisionRef,
        pushedUpdateBindingGenerationRef,
        refreshAtomsFromUpdate,
        selectionEventActiveStateRef,
        setPushedUpdate ]);

    useLayoutEffect(() => {
        const pending = pendingFocusedRebindRef.current;

        if (pending?.editorId !== editorId || !document.isReady) {
            return;
        }

        pendingFocusedRebindRef.current = null;

        const snapshot = bridge.renderState();
        const anchor = Math.min(pending.anchor, snapshot.scalarLength);
        const head = Math.min(pending.head, snapshot.scalarLength);
        const collapsed = anchor === head;

        const setSelection = (affinity: NativeEditorPositionAffinity) =>
            bridge.setSelection({
                baseDocumentRevision: snapshot.documentVersion,
                selection: {
                    type: 'text',
                    anchor: { offset: anchor, kind: 'scalar', affinity },
                    head: { offset: head, kind: 'scalar', affinity },
                },
            });

        try {
            try {
                setSelection(collapsed ? 'after' : 'before');
            } catch (error) {
                if (!collapsed || !isPositionInvalidError(error)) {
                    throw error;
                }

                setSelection('before');
            }

            pushEngineUpdateToView();
        } catch (error) {
            if (!isPositionInvalidError(error)) {
                throw error;
            }
        }

        setActiveEditorToolbarFrameOwnerForEditor(toolbarFrameOwnerId, true);
        onFocusRef.current?.();
    }, [ bridge,
        document.isReady,
        editorId,
        onFocusRef,
        pendingFocusedRebindRef,
        pushEngineUpdateToView,
        toolbarFrameOwnerId ]);

    // A pending update is owned by the handle that produced its snapshot.
    // Drop it when this component rebinds, so no old session state reaches
    // the next native view binding.
    useEffect(() => {
        setPushedUpdate(current => {
            if (current == null || current.editorId === editorId) {
                return current;
            }

            return null;
        });
    }, [ editorId, setPushedUpdate ]);

    const pendingViewReset = bridge._pendingViewReset;

    // Native commits already update the view; echo only subsequent JS-driven revisions.
    useEffect(() => {
        // The document hook refreshes its state after the rebind commit. Its
        // first render can therefore still contain A's revision; do not let
        // that stale snapshot establish B's initial-observation state.
        if (didRebindRevisionScope) {
            return;
        }

        if (!document.isReady || document.documentRevision == null) {
            return;
        }

        const revision = document.documentRevision;

        if (!pendingViewReset && !didObserveInitialRevisionRef.current) {
            didObserveInitialRevisionRef.current = true;
            lastPushedEngineRevisionRef.current = revision;

            return;
        }

        if (
            !pendingViewReset &&
            lastPushedEngineRevisionRef.current != null &&
            BigInt(revision) <= BigInt(lastPushedEngineRevisionRef.current)
        ) {
            return;
        }

        if (
            !pendingViewReset &&
            (revision === lastNativeDrivenRevisionRef.current ||
                document.documentOrigin === 'nativeView' ||
                document.documentOrigin === 'remoteCollaboration')
        ) {
            if (document.documentOrigin === 'remoteCollaboration' && !documentHandle.isDestroyed) {
                refreshAtomsFromUpdate(bridge.renderState());
            }

            lastPushedEngineRevisionRef.current = revision;

            return;
        }

        lastNativeDrivenRevisionRef.current = null;
        pushEngineUpdateToView();
    }, [ pendingViewReset,
        didRebindRevisionScope,
        document.isReady,
        document.documentRevision,
        document.documentOrigin,
        bridge,
        documentHandle,
        pushEngineUpdateToView,
        refreshAtomsFromUpdate,
        didObserveInitialRevisionRef,
        lastPushedEngineRevisionRef,
        lastNativeDrivenRevisionRef ]);

    const afterLocalEngineMutation = useCallback(() => {
        onLocalCommitRef.current?.();
        pushEngineUpdateToView();
        document.refresh();
    }, [ document, onLocalCommitRef, pushEngineUpdateToView ]);

    return {
        refreshAtomsFromUpdate,
        afterLocalEngineMutation,
        pushEngineUpdateToView,
        applyTypedUpdateState,
        applyUpdateState,
        applySelectionEventState,
        updateAtomSelection,
    };
}

function parseUpdateStateJson(updateJson: string | null | undefined): Record<string, unknown> | null {
    if (typeof updateJson !== 'string' || updateJson.length === 0) {
        return null;
    }

    try {
        const candidate = JSON.parse(updateJson) as unknown;

        return isRecord(candidate) ? candidate : null;
    } catch {
        return null;
    }
}
