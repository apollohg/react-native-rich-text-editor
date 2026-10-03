import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { ScrollView, StyleSheet, Text, View } from 'react-native';
import { StatusBar } from 'expo-status-bar';
import * as ImagePicker from 'expo-image-picker';
import { ImageManipulator, SaveFormat } from 'expo-image-manipulator';
import { createCodeHighlightingAddon } from '@apollohg/react-native-rich-text-editor-code-highlighting';
import { SafeAreaProvider, useSafeAreaInsets } from 'react-native-safe-area-context';
import {
    createNativeEditorDocumentHandle,
    DEFAULT_EDITOR_IMAGE_LOADING_POLICY,
    RichTextEditor,
    RichTextViewer,
    TableToolbar,
    defaultSchema,
    withAtomsSchema,
    withImagesSchema,
    withMentionsSchema,
    withTablesSchema,
    type DocumentJSON,
    type EditorAddons,
    createMentionsAddon,
    type EditorTheme,
    type ImageRequestContext,
    type LinkRequestContext,
    type MentionQueryChangeEvent,
    type MentionSuggestion,
    type ReadonlyActiveState,
    type RichTextEditorRef,
    type TableDirection,
    type TableToolbarState,
} from '@apollohg/react-native-rich-text-editor';

import { counterCardAtom } from './components/CounterCard';
import { LinkEditorModal } from './components/LinkEditorModal';
import { OptionGroup } from './components/OptionGroup';
import {
    APP_TITLE,
    buildToolbarItems,
    DOCUMENT_FIXTURE_LABELS,
    DOCUMENT_FIXTURES,
    EDITOR_PLACEHOLDER,
    EDITOR_SURFACE_LABELS,
    EDITOR_SURFACES,
    fixtureDocument,
    INSERT_COUNTER_ACTION_KEY,
    MENTION_SUGGESTIONS,
    MENTION_TRIGGER,
    TABLE_DIRECTION_LABELS,
    TABLE_DIRECTIONS,
    TABLE_NAMES,
    TABLE_NAMING_PRESET,
    TABLE_TOOLBAR_MODE_LABELS,
    TABLE_TOOLBAR_MODES,
    TOGGLE_TASK_LIST_ACTION_KEY,
    viewerDocument,
    VIEWPORT_MODE_LABELS,
    VIEWPORT_MODES,
    type DocumentFixture,
    type EditorSurface,
    type TableToolbarMode,
    type ViewportMode,
} from './content';
import { withTableDirectionAttribute } from './tableContent';
import { TASK_LIST_NODE_NAME, withTaskListSchema } from './taskList';
import {
    customTableToolbarTheme,
    editorTheme,
    FONT_SIZE,
    LINE_HEIGHT,
    mentionTheme,
    NARROW_VIEWPORT_WIDTH,
    PALETTE,
    RADIUS,
    SPACE,
} from './theme';

const PICKED_IMAGE_COMPRESSION = 0.8;
const NEW_COUNTER_TITLE = 'New counter';

const contentSchema = withTaskListSchema(
    withImagesSchema(
        withTableDirectionAttribute(
            withTablesSchema(defaultSchema, { preset: TABLE_NAMING_PRESET }),
            TABLE_NAMES
        )
    )
);

const editorAtoms = [ counterCardAtom ];

const documentSchema = withAtomsSchema(withMentionsSchema(contentSchema), editorAtoms);

const codeHighlightingAddon = createCodeHighlightingAddon({ theme: 'InspiredGitHub' });

const viewerAddons: EditorAddons = [ codeHighlightingAddon ];

const NO_MENTION_SUGGESTIONS: readonly MentionSuggestion[] = [];

const INITIAL_FIXTURE: DocumentFixture = 'showcase';
const INITIAL_DIRECTION: TableDirection = 'ltr';
const INITIAL_SURFACE: EditorSurface = 'editor';
const INITIAL_TABLE_TOOLBAR_MODE: TableToolbarMode = 'default';
const INITIAL_VIEWPORT_MODE: ViewportMode = 'full';

function renderCustomTableToolbar(state: TableToolbarState) {
    return <TableToolbar {...state} theme={customTableToolbarTheme} />;
}

const TABLE_TOOLBAR_PROPS: Readonly<Record<TableToolbarMode, false | typeof renderCustomTableToolbar | undefined>> = {
    custom: renderCustomTableToolbar,
    default: undefined,
    hidden: false,
};

export default function App() {
    return (
        <SafeAreaProvider>
            <StatusBar style={'light'} />
            <EditorScreen />
        </SafeAreaProvider>
    );
}

function EditorScreen() {
    const insets = useSafeAreaInsets();
    const editorRef = useRef<RichTextEditorRef>(null);

    const [ mentionSuggestions, setMentionSuggestions ] =
        useState<readonly MentionSuggestion[]>(NO_MENTION_SUGGESTIONS);

    const [ linkRequest, setLinkRequest ] = useState<LinkRequestContext | null>(null);
    const [ taskListActive, setTaskListActive ] = useState(false);
    const [ taskListAvailable, setTaskListAvailable ] = useState(false);
    const [ fixture, setFixture ] = useState<DocumentFixture>(INITIAL_FIXTURE);
    const [ direction, setDirection ] = useState<TableDirection>(INITIAL_DIRECTION);
    const [ surface, setSurface ] = useState<EditorSurface>(INITIAL_SURFACE);

    const [ tableToolbarMode, setTableToolbarMode ] =
        useState<TableToolbarMode>(INITIAL_TABLE_TOOLBAR_MODE);

    const [ viewportMode, setViewportMode ] = useState<ViewportMode>(INITIAL_VIEWPORT_MODE);
    const [ editedDocument, setEditedDocument ] = useState<DocumentJSON | null>(null);

    const seededDocument = useMemo(() => fixtureDocument(fixture), [ fixture ]);

    const viewerContent = useMemo(
        () => viewerDocument(editedDocument ?? seededDocument, direction),
        [ direction, editedDocument, seededDocument ]
    );

    const documentHandle = useMemo(
        () =>
            createNativeEditorDocumentHandle({
                schema: documentSchema,
                initialization: { type: 'localJson', json: seededDocument },
            }),
        [ seededDocument ]
    );

    useEffect(() => () => documentHandle.destroy(), [ documentHandle ]);

    /** Keeps the previous list when the filter result is unchanged, so the addons prop is stable across keystrokes. */
    const handleMentionQueryChange = useCallback((event: MentionQueryChangeEvent) => {
        const next = filterMentionSuggestions(event.isActive ? event.query : null);
        setMentionSuggestions(current => (sameSuggestions(current, next) ? current : next));
    }, []);

    const addons = useMemo<EditorAddons>(
        () => [
            codeHighlightingAddon,
            createMentionsAddon({
                trigger: MENTION_TRIGGER,
                suggestions: mentionSuggestions,
                theme: mentionTheme,
                onQueryChange: handleMentionQueryChange,
            }),
        ],
        [ handleMentionQueryChange, mentionSuggestions ]
    );

    const handleActiveStateChange = useCallback((state: ReadonlyActiveState) => {
        setTaskListActive(state.nodes[TASK_LIST_NODE_NAME] === true);
        setTaskListAvailable(state.commands.wrapTaskList === true);
    }, []);

    const toolbarItems = useMemo(
        () => buildToolbarItems({ taskListActive, taskListAvailable }),
        [ taskListActive, taskListAvailable ]
    );

    const handleToolbarAction = useCallback((key: string) => {
        switch (key) {
            case INSERT_COUNTER_ACTION_KEY:
                editorRef.current?.insertContentJson(
                    counterCardAtom.buildFragmentJson({ title: NEW_COUNTER_TITLE, count: 0 })
                );

                break;
            case TOGGLE_TASK_LIST_ACTION_KEY:
                editorRef.current?.toggleList(TASK_LIST_NODE_NAME);
                break;
        }
    }, []);

    const handleRequestImage = useCallback((context: ImageRequestContext) => {
        void pickImageUri().then(uri => {
            if (uri != null) {
                context.insertImage(uri);
            }
        });
    }, []);

    const closeLinkRequest = useCallback(() => setLinkRequest(null), []);

    const selectFixture = useCallback((next: DocumentFixture) => {
        setEditedDocument(null);
        setFixture(next);
    }, []);

    const selectSurface = useCallback((next: EditorSurface) => {
        if (next === 'viewer') {
            setEditedDocument(editorRef.current?.getContentJson() ?? null);
        }

        setSurface(next);
    }, []);

    const theme = useMemo<EditorTheme>(() => {
        const content = editorTheme.content;

        return {
            ...editorTheme,
            content: { ...content, paddingBottom: content.paddingBottom + insets.bottom },
        };
    }, [ insets.bottom ]);

    return (
        <View style={styles.screen}>
            <View style={[ styles.header, { paddingTop: insets.top + SPACE.lg } ]}>
                <Text accessibilityRole={'header'} style={styles.title}>
                    {APP_TITLE}
                </Text>
            </View>

            <ScrollView
                horizontal
                showsHorizontalScrollIndicator={false}
                keyboardShouldPersistTaps={'always'}
                style={styles.controlRow}
                contentContainerStyle={styles.controls}
            >
                <OptionGroup
                    label={'Document fixture'}
                    options={DOCUMENT_FIXTURES}
                    labels={DOCUMENT_FIXTURE_LABELS}
                    value={fixture}
                    onChange={selectFixture}
                />
            </ScrollView>
            <ScrollView
                horizontal
                showsHorizontalScrollIndicator={false}
                keyboardShouldPersistTaps={'always'}
                style={styles.controlRow}
                contentContainerStyle={styles.controls}
            >
                <OptionGroup
                    label={'Surface'}
                    options={EDITOR_SURFACES}
                    labels={EDITOR_SURFACE_LABELS}
                    value={surface}
                    onChange={selectSurface}
                />
                <OptionGroup
                    label={'Table direction'}
                    options={TABLE_DIRECTIONS}
                    labels={TABLE_DIRECTION_LABELS}
                    value={direction}
                    onChange={setDirection}
                />
                <OptionGroup
                    label={'Table toolbar'}
                    options={TABLE_TOOLBAR_MODES}
                    labels={TABLE_TOOLBAR_MODE_LABELS}
                    value={tableToolbarMode}
                    onChange={setTableToolbarMode}
                />
                <OptionGroup
                    label={'Viewport'}
                    options={VIEWPORT_MODES}
                    labels={VIEWPORT_MODE_LABELS}
                    value={viewportMode}
                    onChange={setViewportMode}
                />
            </ScrollView>

            <View style={[ styles.sheet, viewportMode === 'narrow' && styles.narrowSheet ]}>
                {surface === 'viewer' ? (
                    <ScrollView style={styles.editor}>
                        <RichTextViewer
                            contentJSON={viewerContent}
                            schema={contentSchema}
                            atoms={editorAtoms}
                            addons={viewerAddons}
                            theme={theme}
                            accessibilityLabel={'Document preview'}
                        />
                    </ScrollView>
                ) : (
                    <RichTextEditor
                        ref={editorRef}
                        documentHandle={documentHandle}
                        atoms={editorAtoms}
                        addons={addons}
                        theme={theme}
                        toolbarItems={toolbarItems}
                        toolbarPlacement={'keyboard'}
                        heightBehavior={'fixed'}
                        placeholder={EDITOR_PLACEHOLDER}
                        accessibilityLabel={'Document'}
                        accessibilityHint={'Formatting is available from the toolbar above the keyboard.'}
                        autoCapitalize={'sentences'}
                        autoCorrect
                        allowImageResizing
                        onActiveStateChange={handleActiveStateChange}
                        onToolbarAction={handleToolbarAction}
                        onRequestLink={setLinkRequest}
                        onRequestImage={handleRequestImage}
                        containerStyle={styles.editorContainer}
                        style={styles.editor}
                        tableToolbar={TABLE_TOOLBAR_PROPS[tableToolbarMode]}
                        tableDirection={direction}
                    />
                )}
            </View>

            <LinkEditorModal request={linkRequest} onClose={closeLinkRequest} />
        </View>
    );
}

function filterMentionSuggestions(query: string | null): readonly MentionSuggestion[] {
    if (query == null) {
        return NO_MENTION_SUGGESTIONS;
    }

    const needle = query.trim().toLowerCase();

    if (needle.length === 0) {
        return MENTION_SUGGESTIONS;
    }

    return MENTION_SUGGESTIONS.filter(
        suggestion =>
            suggestion.title.toLowerCase().includes(needle) ||
            (suggestion.label ?? '').toLowerCase().includes(needle)
    );
}

function sameSuggestions(
    a: readonly MentionSuggestion[],
    b: readonly MentionSuggestion[]
): boolean {
    return a.length === b.length && a.every((suggestion, index) => suggestion.key === b[index].key);
}

/** Opens the photo library and downsizes the pick to the editor's decode limit. */
async function pickImageUri(): Promise<string | null> {
    const permission = await ImagePicker.requestMediaLibraryPermissionsAsync();

    if (!permission.granted) {
        return null;
    }

    const result = await ImagePicker.launchImageLibraryAsync({ quality: 1 });

    if (result.canceled || result.assets.length === 0) {
        return null;
    }

    const asset = result.assets[0];
    const decodeLimit = DEFAULT_EDITOR_IMAGE_LOADING_POLICY.maxDecodeDimensionPx;
    const context = ImageManipulator.manipulate(asset.uri);

    if (asset.width > decodeLimit) {
        context.resize({ width: decodeLimit });
    }

    const rendered = await context.renderAsync();

    const saved = await rendered.saveAsync({
        format: SaveFormat.JPEG,
        compress: PICKED_IMAGE_COMPRESSION,
    });

    return saved.uri;
}

const styles = StyleSheet.create({
    screen: {
        flex: 1,
        backgroundColor: PALETTE.spruceDeep,
    },
    header: {
        paddingHorizontal: SPACE.xl,
        paddingBottom: SPACE.lg,
    },
    title: {
        color: PALETTE.paper,
        fontSize: FONT_SIZE.title,
        lineHeight: LINE_HEIGHT.title,
        fontWeight: '700',
    },
    controlRow: {
        flexGrow: 0,
    },
    controls: {
        gap: SPACE.md,
        paddingHorizontal: SPACE.xl,
        paddingBottom: SPACE.md,
    },
    sheet: {
        flex: 1,
        overflow: 'hidden',
        backgroundColor: PALETTE.paper,
        borderTopLeftRadius: RADIUS.sheet,
        borderTopRightRadius: RADIUS.sheet,
    },
    narrowSheet: {
        width: NARROW_VIEWPORT_WIDTH,
        alignSelf: 'center',
    },
    editorContainer: {
        flex: 1,
    },
    editor: {
        flex: 1,
    },
});
