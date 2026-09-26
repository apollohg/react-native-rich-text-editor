import { type ComponentRef, useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { serializeEditorTheme } from './EditorTheme';
import { serializeNormalizedEditorAddons } from './addons';
import { serializeEditorImageLoadingPolicy } from './ImageLoadingPolicy';
import { serializeEditorAtoms, type AtomAttrsUpdate } from './atoms';
import { AtomUpdateAttrsError, DEFAULT_ATOM_CHIP_HEIGHT, type AtomInstance } from './atomInstances';
import { type NativeEditorDocumentHandle } from './NativeEditorBridge';
import { DefaultAtomChip } from './DefaultAtomChip';
import {
    Platform,
    StyleSheet,
    View,
    type LayoutChangeEvent,
    type StyleProp,
    type ViewStyle,
} from 'react-native';
import { ATOM_CONTENT_NATIVE_ID_PREFIX, ATOM_NATIVE_ID_PREFIX } from './atomConstants';
import { AtomHost, atomIsVisible } from './AtomHost';
import { IMAGE_NODE_NAME } from './schemas';
import {
    EditorToolbar,
    EditorToolbarFrameOwnerProvider,
    type EditorToolbarCommand,
    type EditorToolbarListType,
} from './EditorToolbar';
import { useRegisteredEditorToolbarFrame } from './EditorToolbarRegistry';
import { TableToolbar } from './TableToolbar';
import { tableToolbarSelectionKey, useTableToolbar } from './useTableToolbar';
import {
    isFiniteSize,
    keyboardSafeViewport,
    windowToHostRect,
    type Rect,
    type Size,
} from './TableToolbarPlacement';
import { type useRichTextEditorState } from './useRichTextEditorState';
import { type useRichTextEditorMentions } from './useRichTextEditorMentions';
import { type useRichTextEditorCommands } from './useRichTextEditorCommands';
import { type useRichTextEditorEvents } from './useRichTextEditorEvents';
import {
    useSerializedValue,
    serializeRemoteSelections,
    mapToolbarItemsForNative,
    stringifyCachedJson,
    serializeToolbarFrames,
    externalCompositionErrorPayload,
} from './RichTextEditorSerialization';
import { styles, NativeEditorView } from './RichTextEditorNativeView';

export function useRichTextEditorPresentation(
    context: Pick<
        ReturnType<typeof useRichTextEditorState>,
        | 'theme'
        | 'addons'
        | 'imageLoadingPolicy'
        | 'androidInputOptions'
        | 'remoteSelections'
        | 'atoms'
        | 'atomState'
        | 'atomComponents'
        | 'warnedUnknownAtomTypesRef'
        | 'atomContentWidth'
        | 'atomPositions'
        | 'atomViewport'
        | 'virtualizeAtoms'
        | 'nativeAtomViewport'
        | 'selectedKeys'
        | 'editable'
        | 'atomsInteractive'
        | 'documentHandle'
        | 'activeState'
        | 'onRequestLink'
        | 'onRequestImage'
        | 'toolbarItemsSerializationCacheRef'
        | 'toolbarItems'
        | 'document'
        | 'toolbarPlacement'
        | 'showToolbar'
        | 'containerStyle'
        | 'style'
        | 'heightBehavior'
        | 'autoGrowHeight'
        | 'pushedUpdate'
        | 'editorId'
        | 'registeredToolbarFrames'
        | 'suppliedFocusPreservingFrames'
        | 'isFocused'
        | 'nativeViewRef'
        | 'refreshFocusPreservingFrames'
        | 'accessibilityLabel'
        | 'accessibilityHint'
        | 'placeholder'
        | 'autoFocus'
        | 'pasteMode'
        | 'autoCapitalize'
        | 'autoCorrect'
        | 'keyboardType'
        | 'allowImageResizing'
        | 'tableDirection'
        | 'tableToolbar'
        | 'tableSelection'
        | 'toolbarFrameOwnerId'
        | 'onToolbarAction'
    > &
        Pick<
            ReturnType<typeof useRichTextEditorMentions>,
            'mentionSuggestionTheme' | 'handleAddonEvent'
        > &
        Pick<
            ReturnType<typeof useRichTextEditorCommands>,
            | 'runAtomAction'
            | 'updateAtomAttrs'
            | 'atomOwnerRef'
            | 'commandToggleMark'
            | 'commandToggleList'
            | 'commandToggleHeading'
            | 'commandToggleBlockquote'
            | 'commandInsertNode'
            | 'commandIndentListItem'
            | 'commandOutdentListItem'
            | 'openLinkRequest'
            | 'openImageRequest'
            | 'editorHandleRef'
        > &
        Pick<
            ReturnType<typeof useRichTextEditorEvents>,
            | 'handleEditorUpdate'
            | 'handleEditorError'
            | 'handleExternalTextCompositionEnd'
            | 'handleSelectionChange'
            | 'handleTableSelectionGeometry'
            | 'handleFocusChange'
            | 'handleContentHeightChange'
            | 'handleAtomLayout'
            | 'handleToolbarAction'
        >
) {
    const {
        theme,
        mentionSuggestionTheme,
        addons,
        imageLoadingPolicy,
        androidInputOptions,
        remoteSelections,
        atoms,
        atomState,
        atomComponents,
        warnedUnknownAtomTypesRef,
        runAtomAction,
        updateAtomAttrs,
        atomOwnerRef,
        atomContentWidth,
        atomPositions,
        atomViewport,
        virtualizeAtoms,
        nativeAtomViewport,
        selectedKeys,
        editable,
        atomsInteractive,
        documentHandle,
        activeState,
        onRequestLink,
        onRequestImage,
        toolbarItemsSerializationCacheRef,
        toolbarItems,
        document,
        toolbarPlacement,
        showToolbar,
        containerStyle,
        style,
        heightBehavior,
        autoGrowHeight,
        pushedUpdate,
        editorId,
        registeredToolbarFrames,
        suppliedFocusPreservingFrames,
        isFocused,
        nativeViewRef,
        refreshFocusPreservingFrames,
        accessibilityLabel,
        accessibilityHint,
        placeholder,
        autoFocus,
        pasteMode,
        autoCapitalize,
        autoCorrect,
        keyboardType,
        allowImageResizing,
        tableDirection,
        tableToolbar,
        tableSelection,
        editorHandleRef,
        handleEditorUpdate,
        handleEditorError,
        handleExternalTextCompositionEnd,
        handleSelectionChange,
        handleTableSelectionGeometry,
        handleFocusChange,
        handleContentHeightChange,
        handleAtomLayout,
        handleToolbarAction,
        handleAddonEvent,
        toolbarFrameOwnerId,
        commandToggleMark,
        commandToggleList,
        commandToggleHeading,
        commandToggleBlockquote,
        commandInsertNode,
        commandIndentListItem,
        commandOutdentListItem,
        openLinkRequest,
        openImageRequest,
        onToolbarAction,
    } = context;

    const themeJson = useMemo(
        () => serializeEditorTheme(theme, mentionSuggestionTheme),
        [ mentionSuggestionTheme, theme ]
    );

    const addonsJson = useSerializedValue(addons, value =>
        serializeNormalizedEditorAddons(value));

    const imageLoadingPolicyJson = useSerializedValue(imageLoadingPolicy, value =>
        serializeEditorImageLoadingPolicy(value));

    const androidInputOptionsJson = useSerializedValue(androidInputOptions, value =>
        JSON.stringify(value));

    const remoteSelectionsJson = useSerializedValue(remoteSelections, selections =>
        serializeRemoteSelections(selections));

    const atomsJson = useMemo(() => {
        const supplied = serializeEditorAtoms(atoms);

        const serialized =
            supplied == null
                ? { nodeTypes: [] as string[], estimatedHeights: {} as Record<string, number> }
                : (JSON.parse(supplied) as {
                      nodeTypes: string[];
                      estimatedHeights: Record<string, number>;
                  });

        for (const instance of atomState.instances) {
            if (
                Object.prototype.hasOwnProperty.call(serialized.estimatedHeights, instance.nodeType)
            ) {
                continue;
            }

            serialized.nodeTypes.push(instance.nodeType);
            serialized.estimatedHeights[instance.nodeType] = DEFAULT_ATOM_CHIP_HEIGHT;
        }

        return serialized.nodeTypes.length === 0 ? undefined : JSON.stringify(serialized);
    }, [ atomState.instances, atoms ]);

    useEffect(() => {
        if (!__DEV__) {
            return;
        }

        for (const instance of atomState.instances) {
            if (
                atomComponents.has(instance.nodeType) ||
                warnedUnknownAtomTypesRef.current.has(instance.nodeType)
            ) {
                continue;
            }

            warnedUnknownAtomTypesRef.current.add(instance.nodeType);

            console.warn(
                `NativeRichTextEditor: rendering unknown atom type '${instance.nodeType}' as a chip`
            );
        }
    }, [ atomComponents, atomState.instances, warnedUnknownAtomTypesRef ]);

    const runAtomActionRef = useRef(runAtomAction);

    runAtomActionRef.current = runAtomAction;

    const invokeAtomAction = useCallback(
        (...args: Parameters<typeof runAtomAction>) => runAtomActionRef.current(...args),
        []
    );

    const updateAtomAttrsRef = useRef(updateAtomAttrs);

    updateAtomAttrsRef.current = updateAtomAttrs;

    const invokeAtomAttrsUpdate = useCallback(
        (
            owner: NativeEditorDocumentHandle,
            instance: AtomInstance,
            documentVersion: string | null,
            attrs: AtomAttrsUpdate
        ) => {
            if (owner !== atomOwnerRef.current) {
                return Promise.reject(
                    new AtomUpdateAttrsError('not-ready', 'The editor has rebound.')
                );
            }

            return updateAtomAttrsRef.current(
                instance.key,
                instance.nodeType,
                instance.docPos,
                documentVersion,
                instance.hasStableKey,
                attrs
            );
        },
        [ atomOwnerRef ]
    );

    const atomChildren = useMemo(
        () =>
            atomContentWidth == null
                ? null
                : atomState.instances.map(instance => {
                    const Component = atomComponents.get(instance.nodeType) ?? DefaultAtomChip;
                    const position = atomPositions.get(instance.key);
                    const width = position?.width ?? atomContentWidth;

                    return (
                        <View
                            key={instance.key}
                            nativeID={`${ATOM_NATIVE_ID_PREFIX}${instance.key}`}
                            collapsable={false}
                            style={{
                                position: 'absolute',
                                top: Platform.OS === 'android' ? (position?.hostY ?? 0) : 0,
                                left: Platform.OS === 'android' ? (position?.hostX ?? 0) : 0,
                                width,
                            }}
                        >
                            <AtomHost
                                nativeID={`${ATOM_CONTENT_NATIVE_ID_PREFIX}${instance.key}`}
                                component={Component}
                                width={width}
                                estimatedHeight={
                                    (atoms ?? []).find(atom => atom.name === instance.nodeType)
                                        ?.estimatedHeight ?? DEFAULT_ATOM_CHIP_HEIGHT
                                }
                                visible={
                                    !position ||
                                      atomIsVisible(
                                          position.y,
                                          position.height ??
                                              (atoms ?? []).find(
                                                  atom => atom.name === instance.nodeType
                                              )?.estimatedHeight ??
                                              DEFAULT_ATOM_CHIP_HEIGHT,
                                          atomViewport ??
                                              (virtualizeAtoms ? nativeAtomViewport : undefined)
                                      )
                                }
                                atomProps={{
                                    attrs: instance.attrs,
                                    selected: selectedKeys.has(instance.key),
                                    readOnly: !editable,
                                    interactive: atomsInteractive,
                                    isViewer: false,
                                    nodeType: instance.nodeType,
                                    updateAttrs: attrs =>
                                        invokeAtomAttrsUpdate(
                                            documentHandle,
                                            instance,
                                            atomState.documentVersion,
                                            attrs
                                        ),
                                    editor: {
                                        select: () =>
                                            invokeAtomAction(
                                                documentHandle,
                                                instance,
                                                atomState.documentVersion,
                                                'select'
                                            ),
                                        delete: () =>
                                            invokeAtomAction(
                                                documentHandle,
                                                instance,
                                                atomState.documentVersion,
                                                'delete'
                                            ),
                                        focusBefore: () =>
                                            invokeAtomAction(
                                                documentHandle,
                                                instance,
                                                atomState.documentVersion,
                                                'before'
                                            ),
                                        focusAfter: () =>
                                            invokeAtomAction(
                                                documentHandle,
                                                instance,
                                                atomState.documentVersion,
                                                'after'
                                            ),
                                    },
                                }}
                            />
                        </View>
                    );
                }),
        [ atomContentWidth,
            atomState.instances,
            atomState.documentVersion,
            atomComponents,
            atomPositions,
            atoms,
            atomViewport,
            virtualizeAtoms,
            nativeAtomViewport,
            selectedKeys,
            editable,
            atomsInteractive,
            invokeAtomAttrsUpdate,
            documentHandle,
            invokeAtomAction ]
    );

    const tableToolbarHostRef = useRef<ComponentRef<typeof View> | null>(null);

    const tableToolbarHostMeasurementRef = useRef(0);

    const [ tableToolbarHostOrigin, setTableToolbarHostOrigin ] = useState<Pick<
        Rect,
        'x' | 'y'
    > | null>(null);

    const [ tableToolbarSize, setTableToolbarSize ] = useState<Size | null>(null);

    const tableToolbarEnabled = tableToolbar !== false && editable && isFocused;

    const measureTableToolbarHost = useCallback(() => {
        const measurement = ++tableToolbarHostMeasurementRef.current;
        const host = tableToolbarHostRef.current;

        if (host == null) {
            return;
        }

        host.measureInWindow((x, y) => {
            if (
                tableToolbarHostMeasurementRef.current !== measurement ||
                !Number.isFinite(x) ||
                !Number.isFinite(y)
            ) {
                return;
            }

            setTableToolbarHostOrigin(current =>
                current?.x === x && current.y === y ? current : { x, y });
        });
    }, []);

    useEffect(() => {
        if (tableSelection == null) {
            tableToolbarHostMeasurementRef.current += 1;
            setTableToolbarHostOrigin(null);

            return;
        }

        measureTableToolbarHost();
    }, [ measureTableToolbarHost, tableSelection ]);

    useEffect(
        () => () => {
            tableToolbarHostMeasurementRef.current += 1;
        },
        []
    );

    const tableToolbarSafeViewport = useMemo(
        () =>
            tableSelection == null
                ? null
                : keyboardSafeViewport(
                    tableSelection.obstructions,
                    tableSelection.geometry.rects,
                    tableSelection.geometry.viewport
                ),
        [ tableSelection ]
    );

    const documentRevision = document.documentRevision;

    const tableToolbarIdentity = useMemo(
        () =>
            tableSelection == null || documentRevision == null
                ? null
                : {
                    editorId,
                    ownerId: toolbarFrameOwnerId,
                    documentRevision,
                    layoutEpoch: tableSelection.geometry.layoutEpoch,
                },
        [ documentRevision, editorId, tableSelection, toolbarFrameOwnerId ]
    );

    const reportTableToolbarError = useCallback(
        (error: unknown) => {
            documentHandle.bridge._emitAutonomousError(externalCompositionErrorPayload(error));
        },
        [ documentHandle ]
    );

    const tableToolbarState = useTableToolbar({
        editor: editorHandleRef,
        geometry: tableSelection?.geometry ?? null,
        identity: tableToolbarIdentity,
        activeState,
        safeViewport: tableToolbarSafeViewport,
        size: tableToolbarSize,
        enabled: tableToolbarEnabled && tableSelection?.editMenuVisible !== true,
        onError: reportTableToolbarError,
    });

    const tableToolbarHostFrame =
        tableToolbarState.frame != null && tableToolbarHostOrigin != null
            ? windowToHostRect(tableToolbarState.frame, tableToolbarHostOrigin)
            : null;

    useRegisteredEditorToolbarFrame(
        tableToolbarHostFrame == null ? null : tableToolbarState.frame,
        toolbarFrameOwnerId
    );

    const tableToolbarCompact = tableToolbarState.compact;

    const handleTableToolbarLayout = useCallback(
        (event: LayoutChangeEvent) => {
            if (tableToolbarCompact) {
                return;
            }

            const { width, height } = event.nativeEvent.layout;
            const size = { width, height };

            if (!isFiniteSize(size)) {
                return;
            }

            setTableToolbarSize(current =>
                current?.width === width && current.height === height ? current : size);
        },
        [ tableToolbarCompact ]
    );

    const isLinkActive = activeState.marks.link === true;

    const allowsLink = activeState.allowedMarks.includes('link');

    const canInsertImage = activeState.insertableNodes.includes(IMAGE_NODE_NAME);

    const canRequestLink = typeof onRequestLink === 'function';

    const canRequestImage = typeof onRequestImage === 'function';

    const cachedToolbarItems = toolbarItemsSerializationCacheRef.current;

    let toolbarItemsJson: string;

    if (
        cachedToolbarItems &&
        cachedToolbarItems.toolbarItems === toolbarItems &&
        cachedToolbarItems.editable === editable &&
        cachedToolbarItems.isLinkActive === isLinkActive &&
        cachedToolbarItems.allowsLink === allowsLink &&
        cachedToolbarItems.canRequestLink === canRequestLink &&
        cachedToolbarItems.canRequestImage === canRequestImage &&
        cachedToolbarItems.canInsertImage === canInsertImage
    ) {
        toolbarItemsJson = cachedToolbarItems.serialized;
    } else {
        const mappedItems = mapToolbarItemsForNative(
            toolbarItems,
            activeState,
            editable,
            onRequestLink,
            onRequestImage
        );

        toolbarItemsJson = stringifyCachedJson(mappedItems);

        toolbarItemsSerializationCacheRef.current = {
            toolbarItems,
            editable,
            isLinkActive,
            allowsLink,
            canRequestLink,
            canRequestImage,
            canInsertImage,
            serialized: toolbarItemsJson,
        };
    }

    // A room document awaiting the server renders nothing (loading), never an
    // unshared fallback paragraph.
    if (!document.isReady) {
        return null;
    }

    const usesNativeKeyboardToolbar =
        toolbarPlacement === 'keyboard' && (Platform.OS === 'ios' || Platform.OS === 'android');

    const shouldRenderJsToolbar = showToolbar && !usesNativeKeyboardToolbar && editable;

    const inlineToolbarMarginTop = theme?.toolbar?.marginTop ?? 8;

    const containerMinHeight = StyleSheet.flatten(containerStyle)?.minHeight;

    const nativeViewStyleParts: StyleProp<ViewStyle>[] = [];

    if (containerMinHeight != null) {
        nativeViewStyleParts.push({ minHeight: containerMinHeight });
    }

    if (style != null) {
        nativeViewStyleParts.push(style);
    }

    if (heightBehavior === 'autoGrow' && autoGrowHeight != null) {
        nativeViewStyleParts.push({ height: autoGrowHeight });
    }

    const nativeViewStyle =
        nativeViewStyleParts.length <= 1 ? nativeViewStyleParts[0] : nativeViewStyleParts;

    const currentPushedUpdate = pushedUpdate?.editorId === editorId ? pushedUpdate : null;

    const focusPreservingFrames = [ ...registeredToolbarFrames, ...suppliedFocusPreservingFrames ];

    const toolbarFrameJson = serializeToolbarFrames(
        editable && isFocused ? focusPreservingFrames : undefined
    );

    return (
        <View style={[ styles.container, containerStyle ]}>
            <NativeEditorView
                ref={nativeViewRef}
                style={nativeViewStyle}
                onLayout={refreshFocusPreservingFrames}
                accessibilityLabel={accessibilityLabel}
                accessibilityHint={accessibilityHint}
                editorId={editorId}
                placeholder={placeholder}
                editable={editable}
                autoFocus={autoFocus}
                pasteMode={pasteMode}
                autoCapitalize={autoCapitalize}
                autoCorrect={autoCorrect}
                keyboardType={keyboardType}
                {...(Platform.OS === 'android' ? { androidInputOptionsJson } : {})}
                showToolbar={showToolbar}
                toolbarPlacement={toolbarPlacement}
                heightBehavior={heightBehavior}
                allowImageResizing={allowImageResizing}
                imageLoadingPolicyJson={imageLoadingPolicyJson}
                themeJson={themeJson}
                addonsJson={addonsJson}
                atomsJson={atomsJson}
                toolbarItemsJson={toolbarItemsJson}
                toolbarFrameJson={toolbarFrameJson}
                remoteSelectionsJson={remoteSelectionsJson}
                tableDirection={tableDirection}
                editorUpdateJson={currentPushedUpdate?.json}
                editorUpdateResetJson={currentPushedUpdate?.resetJson}
                editorUpdateEditorId={currentPushedUpdate?.editorId}
                editorUpdateRevision={currentPushedUpdate?.revision ?? 0}
                onEditorUpdate={handleEditorUpdate}
                onEditorError={handleEditorError}
                onExternalTextCompositionEnd={handleExternalTextCompositionEnd}
                onSelectionChange={handleSelectionChange}
                onFocusChange={handleFocusChange}
                onContentHeightChange={handleContentHeightChange}
                onAtomLayout={handleAtomLayout}
                onTableSelectionGeometry={handleTableSelectionGeometry}
                onToolbarAction={handleToolbarAction}
                onAddonEvent={handleAddonEvent}
            >
                {atomChildren}
            </NativeEditorView>
            {shouldRenderJsToolbar ? (
                <View
                    testID={'native-editor-js-toolbar'}
                    style={[ styles.inlineToolbar, { marginTop: inlineToolbarMarginTop } ]}
                >
                    <EditorToolbarFrameOwnerProvider ownerId={toolbarFrameOwnerId}>
                        <EditorToolbar
                            activeState={activeState}
                            historyState={document.historyState}
                            toolbarItems={toolbarItems}
                            theme={theme?.toolbar}
                            showTopBorder={theme?.toolbar?.showTopBorder ?? false}
                            preserveEditorFocus={false}
                            onToggleMark={commandToggleMark}
                            onToggleListType={(listType: EditorToolbarListType) =>
                                commandToggleList(listType)
                            }
                            onToggleHeading={commandToggleHeading}
                            onToggleBlockquote={commandToggleBlockquote}
                            onInsertNodeType={commandInsertNode}
                            onRunCommand={(command: EditorToolbarCommand) => {
                                switch (command) {
                                    case 'indentList':
                                        commandIndentListItem();
                                        break;
                                    case 'outdentList':
                                        commandOutdentListItem();
                                        break;
                                    case 'undo':
                                        document.undo();
                                        break;
                                    case 'redo':
                                        document.redo();
                                        break;
                                }
                            }}
                            onRequestLink={onRequestLink ? openLinkRequest : undefined}
                            onRequestImage={onRequestImage ? openImageRequest : undefined}
                            onToolbarAction={onToolbarAction}
                            onToggleBold={() => commandToggleMark('bold')}
                            onToggleItalic={() => commandToggleMark('italic')}
                            onToggleUnderline={() => commandToggleMark('underline')}
                            onToggleStrike={() => commandToggleMark('strike')}
                            onUndo={document.undo}
                            onRedo={document.redo}
                        />
                    </EditorToolbarFrameOwnerProvider>
                </View>
            ) : null}
            {tableSelection != null && tableToolbarEnabled ? (
                <View
                    ref={tableToolbarHostRef}
                    testID={'native-editor-table-toolbar-host'}
                    pointerEvents={'box-none'}
                    style={StyleSheet.absoluteFill}
                    onLayout={measureTableToolbarHost}
                >
                    {tableToolbarHostFrame != null || tableToolbarSize == null ? (
                        <View
                            key={tableToolbarSelectionKey(tableSelection.geometry)}
                            testID={'native-editor-table-toolbar'}
                            pointerEvents={tableToolbarHostFrame == null ? 'none' : 'box-none'}
                            onLayout={handleTableToolbarLayout}
                            style={
                                tableToolbarHostFrame == null
                                    ? styles.measuringTableToolbar
                                    : [
                                        styles.placedTableToolbar,
                                        {
                                            left: tableToolbarHostFrame.x,
                                            top: tableToolbarHostFrame.y,
                                        },
                                    ]
                            }
                        >
                            <EditorToolbarFrameOwnerProvider ownerId={toolbarFrameOwnerId}>
                                {tableToolbar === undefined ? (
                                    <TableToolbar {...tableToolbarState} theme={theme?.toolbar} />
                                ) : (
                                    tableToolbar(tableToolbarState)
                                )}
                            </EditorToolbarFrameOwnerProvider>
                        </View>
                    ) : null}
                </View>
            ) : null}
        </View>
    );
}
