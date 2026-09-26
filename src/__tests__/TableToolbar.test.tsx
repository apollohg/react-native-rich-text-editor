import './helpers/NativeRichTextEditorFixture';
import {
    mockNativeModule,
    V2_INITIAL_DOC,
    createV2LocalHandle,
} from './helpers/NativeRichTextEditorFixture';
import { installTableEngine, type EngineEffect } from './helpers/TableEngineFixture';
import { createRef } from 'react';
import { Pressable, ScrollView, StyleSheet, Text, View } from 'react-native';
import {
    act,
    fireEvent,
    render,
    renderHook,
    type RenderResult,
} from '@testing-library/react-native';
import { NativeRichTextEditor, type NativeRichTextEditorRef } from '../NativeRichTextEditor';
import { type NativeEditorDocumentHandle, type ReadonlyActiveState } from '../NativeEditorBridge';
import { NativeEditorOperationError } from '../NativeEditorBoundaryError';
import { type RichTextEditorProps, type RichTextEditorRef } from '../RichTextEditorTypes';
import { type TableSelectionGeometry } from '../TableTypes';
import { TABLE_TOOLBAR_GAP, type Rect } from '../TableToolbarPlacement';
import {
    TABLE_TOOLBAR_ACTIONS,
    useTableToolbar,
    type TableToolbarAction,
    type TableToolbarIdentity,
    type TableToolbarState,
} from '../useTableToolbar';
import {
    TABLE_TOOLBAR_ACTION_LABELS,
    TABLE_TOOLBAR_BACK_LABEL,
    TABLE_TOOLBAR_MENUS,
    TABLE_TOOLBAR_MENU_LABELS,
    type TableToolbarMenu,
} from '../TableToolbar';

const HOST_TEST_ID = 'native-editor-table-toolbar-host';
const WRAPPER_TEST_ID = 'native-editor-table-toolbar';
const DEFAULT_TOOLBAR_TEST_ID = 'table-toolbar';
const CUSTOM_TOOLBAR_TEST_ID = 'custom-table-toolbar';
const CUSTOM_MERGE_TEST_ID = 'custom-merge';
const WINDOW_SPACE = 'window';
const LAYOUT_EPOCH = '3';
const NEXT_LAYOUT_EPOCH = '4';
const TABLE_POS = 1;
const ANCHOR_CELL = 2;
const HEAD_CELL = 9;
const OWNER_ID = 41;
const HOST_ORIGIN = { x: 16, y: 120 };
const SCROLLED_HOST_ORIGIN = { x: 16, y: 40 };
const SAFE_AREA: Rect = { x: 0, y: 47, width: 390, height: 763 };
const VIEWPORT: Rect = { x: 16, y: 120, width: 358, height: 600 };
const LEFT_CELL: Rect = { x: 40, y: 300, width: 120, height: 44 };
const RIGHT_CELL: Rect = { x: 160, y: 300, width: 120, height: 44 };
const SELECTION: Rect = { x: 40, y: 300, width: 240, height: 44 };
const TOOLBAR_SIZE = { width: 300, height: 44 };
const MENU_SIZE = { width: 260, height: 44 };
const WIDE_TOOLBAR_SIZE = { width: 520, height: 44 };
const RESIZE_PREVIEW_SHIFT = 30;
const SCROLL_SHIFT = 80;
const SCROLLED_RECTS: Rect[] = [ LEFT_CELL, RIGHT_CELL ].map(rect => ({ ...rect, y: rect.y - SCROLL_SHIFT }));
const SCROLLED_SELECTION: Rect = { ...SELECTION, y: SELECTION.y - SCROLL_SHIFT };
const FLOATING_KEYBOARD: Rect = { x: 0, y: 150, width: 390, height: 140 };
const DOCKED_KEYBOARD: Rect = { x: 0, y: 520, width: 390, height: 290 };
const CRAMPED_KEYBOARD: Rect = { x: 0, y: 87, width: 390, height: 723 };
const CRAMPED_CELL: Rect = { x: 40, y: 50, width: 120, height: 30 };
const TOP_CELL: Rect = { x: 40, y: 60, width: 240, height: 44 };
const THEME_BACKGROUND = '#102030';
const THEME_RADIUS = 3;
const THEME_BUTTON = '#aa5500';
const THEME_DISABLED = '#556677';

const SELECTION_ACTIONS = new Set<TableToolbarAction>([ 'selectRows', 'selectColumns' ]);

const ALL_ACTIONS = Object.keys(TABLE_TOOLBAR_ACTIONS) as TableToolbarAction[];

const ALL_APPLICABLE: Record<string, boolean> = Object.fromEntries(
    ALL_ACTIONS.map(action => [ TABLE_TOOLBAR_ACTIONS[action].applicability, true ])
);

const ACTION_MENUS = new Map<TableToolbarAction, TableToolbarMenu>(
    (Object.keys(TABLE_TOOLBAR_MENUS) as TableToolbarMenu[]).flatMap(menu =>
        TABLE_TOOLBAR_MENUS[menu].map(action => [ action, menu ] as const))
);

const ACTIVE_STATE_BASE = {
    marks: {},
    markAttrs: {},
    nodes: {},
    allowedMarks: [],
    insertableNodes: [],
};

type MeasureCallback = (x: number, y: number, width: number, height: number) => void;

let deferHostMeasurements = false;
let pendingHostMeasurements: MeasureCallback[] = [];
let measureInWindow: jest.SpyInstance;

beforeEach(() => {
    deferHostMeasurements = false;
    pendingHostMeasurements = [];

    const viewPrototype = (View as unknown as { prototype: { measureInWindow: (callback: MeasureCallback) => void } })
        .prototype;

    measureInWindow = jest.spyOn(viewPrototype, 'measureInWindow').mockImplementation(function(
        this: { props?: { testID?: string } },
        callback: MeasureCallback
    ) {
        if (this.props?.testID !== HOST_TEST_ID) {
            return;
        }

        if (deferHostMeasurements) {
            pendingHostMeasurements.push(callback);

            return;
        }

        callback(HOST_ORIGIN.x, HOST_ORIGIN.y, VIEWPORT.width, VIEWPORT.height);
    });
});

afterEach(() => {
    measureInWindow.mockRestore();
});

function engineEffects(): Map<string, EngineEffect> {
    return new Map(
        ALL_ACTIONS.map(action => [
            TABLE_TOOLBAR_ACTIONS[action].command.type,
            SELECTION_ACTIONS.has(action) ? 'selection' : 'document',
        ])
    );
}

function tableRequestCount(): number {
    return (
        mockNativeModule.editorV2ApplyCommand.mock.calls.length +
        mockNativeModule.editorV2SetSelection.mock.calls.length
    );
}

function lastAppliedCommand(): Record<string, unknown> {
    const calls = mockNativeModule.editorV2ApplyCommand.mock.calls;

    const request = JSON.parse(calls[calls.length - 1][1] as string) as {
        command: Record<string, unknown>;
    };

    return request.command;
}

function flatStyle(element: { props: { style?: unknown } }): Record<string, unknown> {
    return (StyleSheet.flatten(element.props.style as never) ?? {}) as Record<string, unknown>;
}

function editorDriver(view: RenderResult, handle: NativeEditorDocumentHandle, index = 0) {
    const nativeView = () => view.getAllByTestId('native-editor-view')[index];

    return {
        nativeView,
        focus(isFocused = true) {
            act(() => {
                nativeView().props.onFocusChange({
                    nativeEvent: { isFocused, editorId: handle.editorId },
                });
            });
        },
        selectCells(commands: Record<string, boolean> = ALL_APPLICABLE) {
            act(() => {
                nativeView().props.onSelectionChange({
                    nativeEvent: {
                        editorId: handle.editorId,
                        anchor: 0,
                        head: 0,
                        stateJson: JSON.stringify({
                            selection: { type: 'cell', anchorCell: ANCHOR_CELL, headCell: HEAD_CELL },
                            activeState: { ...ACTIVE_STATE_BASE, commands },
                        }),
                    },
                });
            });
        },
        emitGeometry(overrides: Record<string, unknown> = {}) {
            act(() => {
                nativeView().props.onTableSelectionGeometry({
                    nativeEvent: {
                        editorId: handle.editorId,
                        documentRevision: handle.bridge.getState().documentRevision,
                        layoutEpoch: LAYOUT_EPOCH,
                        tablePos: TABLE_POS,
                        coordinateSpace: WINDOW_SPACE,
                        rects: [ LEFT_CELL, RIGHT_CELL ],
                        viewport: VIEWPORT,
                        safeArea: SAFE_AREA,
                        ...overrides,
                    },
                });
            });
        },
        clearGeometry() {
            act(() => {
                nativeView().props.onTableSelectionGeometry({
                    nativeEvent: { editorId: handle.editorId },
                });
            });
        },
        registeredFrames(): Rect[] {
            const raw = nativeView().props.toolbarFrameJson as string | undefined;

            if (raw == null) {
                return [];
            }

            const parsed = JSON.parse(raw) as Rect | { frames: Rect[] };

            return 'frames' in parsed ? parsed.frames : [ parsed ];
        },
    };
}

function layoutToolbar(view: RenderResult, size: { width: number; height: number }, index = 0) {
    act(() => {
        fireEvent(view.getAllByTestId(WRAPPER_TEST_ID)[index], 'layout', {
            nativeEvent: { layout: { x: 0, y: 0, ...size } },
        });
    });
}

function placedHostFrame(view: RenderResult, index = 0) {
    const style = flatStyle(view.getAllByTestId(WRAPPER_TEST_ID)[index]);

    return { left: style.left, top: style.top, opacity: style.opacity };
}

function aboveSelection(size: { width: number; height: number }, selection = SELECTION): Rect {
    return {
        x: selection.x + (selection.width - size.width) / 2,
        y: selection.y - TABLE_TOOLBAR_GAP - size.height,
        width: size.width,
        height: size.height,
    };
}

function hostOffset(frame: Rect, origin = HOST_ORIGIN) {
    return { left: frame.x - origin.x, top: frame.y - origin.y, opacity: undefined };
}

function renderTableEditor(props: Partial<RichTextEditorProps> = {}) {
    const handle = createV2LocalHandle(V2_INITIAL_DOC);
    const ref = createRef<NativeRichTextEditorRef>();
    const onContentChangeJSON = jest.fn();
    const onHistoryStateChange = jest.fn();
    const onSelectionChange = jest.fn();

    const view = render(
        <NativeRichTextEditor
            ref={ref}
            documentHandle={handle}
            onContentChangeJSON={onContentChangeJSON}
            onHistoryStateChange={onHistoryStateChange}
            onSelectionChange={onSelectionChange}
            {...props}
        />
    );

    installTableEngine(handle, engineEffects());
    const driver = editorDriver(view, handle);

    return {
        handle,
        ref,
        view,
        driver,
        onContentChangeJSON,
        onHistoryStateChange,
        onSelectionChange,
    };
}

function showToolbar(
    editor: ReturnType<typeof renderTableEditor>,
    geometry: Record<string, unknown> = {},
    size = TOOLBAR_SIZE
) {
    editor.driver.focus();
    editor.driver.selectCells();
    editor.driver.emitGeometry(geometry);
    layoutToolbar(editor.view, size);
}

async function pressAsync(view: RenderResult, label: string) {
    await act(async() => {
        fireEvent.press(view.getByLabelText(label));
    });
}

function press(view: RenderResult, label: string) {
    act(() => {
        fireEvent.press(view.getByLabelText(label));
    });
}

describe('RichTextEditor table toolbar', () => {
    it('anchors the default toolbar above the selection in host space and preserves focus over it', () => {
        const editor = renderTableEditor();

        showToolbar(editor);

        const frame = aboveSelection(TOOLBAR_SIZE);

        expect({
            placed: placedHostFrame(editor.view),
            focusPreserving: editor.driver.registeredFrames(),
            defaultToolbar: editor.view.queryByTestId(DEFAULT_TOOLBAR_TEST_ID) != null,
        }).toEqual({
            placed: hostOffset(frame),
            focusPreserving: [ frame ],
            defaultToolbar: true,
        });

        editor.handle.destroy();
    });

    it('measures an unplaced toolbar invisibly without registering a frame', () => {
        const editor = renderTableEditor();

        editor.driver.focus();
        editor.driver.selectCells();
        editor.driver.emitGeometry();

        const wrapper = editor.view.getByTestId(WRAPPER_TEST_ID);

        expect({
            opacity: flatStyle(wrapper).opacity,
            pointerEvents: wrapper.props.pointerEvents,
            focusPreserving: editor.driver.registeredFrames(),
        }).toEqual({ opacity: 0, pointerEvents: 'none', focusPreserving: [] });

        editor.handle.destroy();
    });

    it('disables exactly the actions whose published applicability is false', () => {
        const editor = renderTableEditor();
        const available = new Set<TableToolbarAction>([ 'mergeCells', 'addRowAfter', 'deleteTable' ]);
        showToolbar(editor);

        editor.driver.selectCells(
            Object.fromEntries(
                ALL_ACTIONS.map(action => [
                    TABLE_TOOLBAR_ACTIONS[action].applicability,
                    available.has(action),
                ])
            )
        );

        const disabled = (label: string) =>
            editor.view.getByLabelText(label).props.accessibilityState?.disabled === true;

        const observed: Record<string, boolean> = {
            [TABLE_TOOLBAR_MENU_LABELS.column]: disabled(TABLE_TOOLBAR_MENU_LABELS.column),
            [TABLE_TOOLBAR_MENU_LABELS.header]: disabled(TABLE_TOOLBAR_MENU_LABELS.header),
            [TABLE_TOOLBAR_ACTION_LABELS.mergeCells]: disabled(TABLE_TOOLBAR_ACTION_LABELS.mergeCells),
            [TABLE_TOOLBAR_ACTION_LABELS.splitCell]: disabled(TABLE_TOOLBAR_ACTION_LABELS.splitCell),
        };

        for (const menu of [ 'row', 'more' ] as const) {
            press(editor.view, TABLE_TOOLBAR_MENU_LABELS[menu]);

            for (const action of TABLE_TOOLBAR_MENUS[menu]) {
                observed[TABLE_TOOLBAR_ACTION_LABELS[action]] = disabled(
                    TABLE_TOOLBAR_ACTION_LABELS[action]
                );
            }

            press(editor.view, TABLE_TOOLBAR_BACK_LABEL);
        }

        expect(observed).toEqual({
            [TABLE_TOOLBAR_MENU_LABELS.column]: true,
            [TABLE_TOOLBAR_MENU_LABELS.header]: true,
            [TABLE_TOOLBAR_ACTION_LABELS.mergeCells]: false,
            [TABLE_TOOLBAR_ACTION_LABELS.splitCell]: true,
            [TABLE_TOOLBAR_ACTION_LABELS.addRowBefore]: true,
            [TABLE_TOOLBAR_ACTION_LABELS.addRowAfter]: false,
            [TABLE_TOOLBAR_ACTION_LABELS.deleteRows]: true,
            [TABLE_TOOLBAR_ACTION_LABELS.selectRows]: true,
            [TABLE_TOOLBAR_ACTION_LABELS.clearCells]: true,
            [TABLE_TOOLBAR_ACTION_LABELS.deleteTable]: false,
        });

        editor.handle.destroy();
    });

    it('opens and closes every menu without a document, history, selection, or engine event', () => {
        const editor = renderTableEditor();
        showToolbar(editor);
        editor.onContentChangeJSON.mockClear();
        editor.onHistoryStateChange.mockClear();
        editor.onSelectionChange.mockClear();
        const requestsBefore = tableRequestCount();
        const pushedBefore = editor.driver.nativeView().props.editorUpdateRevision;
        const visited: string[] = [];

        for (const menu of Object.keys(TABLE_TOOLBAR_MENUS) as TableToolbarMenu[]) {
            press(editor.view, TABLE_TOOLBAR_MENU_LABELS[menu]);
            layoutToolbar(editor.view, MENU_SIZE);

            visited.push(
                ...TABLE_TOOLBAR_MENUS[menu].filter(
                    action => editor.view.queryByLabelText(TABLE_TOOLBAR_ACTION_LABELS[action]) != null
                )
            );

            press(editor.view, TABLE_TOOLBAR_BACK_LABEL);
        }

        expect({
            visited,
            requests: tableRequestCount() - requestsBefore,
            content: editor.onContentChangeJSON.mock.calls.length,
            history: editor.onHistoryStateChange.mock.calls.length,
            selection: editor.onSelectionChange.mock.calls.length,
            pushed: editor.driver.nativeView().props.editorUpdateRevision,
        }).toEqual({
            visited: Object.values(TABLE_TOOLBAR_MENUS).flat(),
            requests: 0,
            content: 0,
            history: 0,
            selection: 0,
            pushed: pushedBefore,
        });

        editor.handle.destroy();
    });

    it('runs exactly one engine command, lowered from the action map, per action press', async() => {
        const editor = renderTableEditor();
        editor.driver.focus();
        const outcomes: Array<{ action: TableToolbarAction; requests: number; wire: unknown }> = [];

        for (const action of ALL_ACTIONS) {
            editor.driver.selectCells();
            editor.driver.emitGeometry();

            if (outcomes.length === 0) {
                layoutToolbar(editor.view, TOOLBAR_SIZE);
            }

            const menu = ACTION_MENUS.get(action);

            if (menu != null) {
                press(editor.view, TABLE_TOOLBAR_MENU_LABELS[menu]);
            }

            const requestsBefore = tableRequestCount();
            await pressAsync(editor.view, TABLE_TOOLBAR_ACTION_LABELS[action]);
            const selectionOnly = SELECTION_ACTIONS.has(action);

            outcomes.push({
                action,
                requests: tableRequestCount() - requestsBefore,
                wire: selectionOnly ? TABLE_TOOLBAR_ACTIONS[action].command : lastAppliedCommand(),
            });
        }

        expect(outcomes).toEqual(
            ALL_ACTIONS.map(action => ({
                action,
                requests: 1,
                wire: TABLE_TOOLBAR_ACTIONS[action].command,
            }))
        );

        editor.handle.destroy();
    });

    it('hands a custom renderer the toolbar state and positions what it returns', async() => {
        const received: TableToolbarState[] = [];

        const editor = renderTableEditor({
            tableToolbar: state => {
                received.push(state);

                return (
                    <Pressable
                        testID={CUSTOM_MERGE_TEST_ID}
                        onPress={() => void state.run(TABLE_TOOLBAR_ACTIONS.mergeCells.command)}
                    >
                        <Text testID={CUSTOM_TOOLBAR_TEST_ID}>{String(state.commands.mergeCells)}</Text>
                    </Pressable>
                );
            },
        });

        showToolbar(editor);
        const frame = aboveSelection(TOOLBAR_SIZE);

        const shown = {
            defaultToolbar: editor.view.queryByTestId(DEFAULT_TOOLBAR_TEST_ID),
            customRendered: editor.view.getByTestId(CUSTOM_TOOLBAR_TEST_ID).props.children,
            placed: placedHostFrame(editor.view),
            latest: received[received.length - 1],
        };

        const requestsBefore = tableRequestCount();

        await act(async() => {
            fireEvent.press(editor.view.getByTestId(CUSTOM_MERGE_TEST_ID));
        });

        expect({
            defaultToolbar: shown.defaultToolbar,
            customRendered: shown.customRendered,
            placed: shown.placed,
            latest: { visible: shown.latest.visible, frame: shown.latest.frame },
            measuredBeforePlacement: received[0].frame,
            requests: tableRequestCount() - requestsBefore,
            wire: lastAppliedCommand(),
        }).toEqual({
            defaultToolbar: null,
            customRendered: 'true',
            placed: hostOffset(frame),
            latest: { visible: true, frame },
            measuredBeforePlacement: null,
            requests: 1,
            wire: TABLE_TOOLBAR_ACTIONS.mergeCells.command,
        });

        editor.handle.destroy();
    });

    it('renders and registers nothing when the table toolbar is disabled', () => {
        const editor = renderTableEditor({ tableToolbar: false });

        editor.driver.focus();
        editor.driver.selectCells();
        editor.driver.emitGeometry();

        expect({
            host: editor.view.queryByTestId(HOST_TEST_ID),
            focusPreserving: editor.driver.registeredFrames(),
        }).toEqual({ host: null, focusPreserving: [] });

        editor.handle.destroy();
    });

    it('applies toolbar theme overrides to the default toolbar', () => {
        const editor = renderTableEditor({
            theme: {
                toolbar: {
                    backgroundColor: THEME_BACKGROUND,
                    borderRadius: THEME_RADIUS,
                    buttonColor: THEME_BUTTON,
                    buttonDisabledColor: THEME_DISABLED,
                },
            },
        });

        showToolbar(editor);
        editor.driver.selectCells({ [TABLE_TOOLBAR_ACTIONS.mergeCells.applicability]: true });

        const container = flatStyle(editor.view.getByTestId(DEFAULT_TOOLBAR_TEST_ID));

        const titleColor = (label: string) =>
            flatStyle(editor.view.getByLabelText(label).findByType(Text)).color;

        expect({
            backgroundColor: container.backgroundColor,
            borderRadius: container.borderRadius,
            enabled: titleColor(TABLE_TOOLBAR_ACTION_LABELS.mergeCells),
            disabled: titleColor(TABLE_TOOLBAR_ACTION_LABELS.splitCell),
        }).toEqual({
            backgroundColor: THEME_BACKGROUND,
            borderRadius: THEME_RADIUS,
            enabled: THEME_BUTTON,
            disabled: THEME_DISABLED,
        });

        editor.handle.destroy();
    });

    it('moves clear of a floating keyboard and of the safe-area inset', () => {
        const editor = renderTableEditor();
        showToolbar(editor);
        const unobstructed = placedHostFrame(editor.view);

        editor.driver.emitGeometry({ keyboard: FLOATING_KEYBOARD });
        const besideKeyboard = placedHostFrame(editor.view);

        editor.driver.emitGeometry({ rects: [ TOP_CELL ] });
        const belowInset = placedHostFrame(editor.view);

        const belowSelection = { ...aboveSelection(TOOLBAR_SIZE), y: SELECTION.y + SELECTION.height + TABLE_TOOLBAR_GAP };

        const belowTopCell = {
            ...aboveSelection(TOOLBAR_SIZE, TOP_CELL),
            y: TOP_CELL.y + TOP_CELL.height + TABLE_TOOLBAR_GAP,
        };

        expect({ unobstructed, besideKeyboard, belowInset }).toEqual({
            unobstructed: hostOffset(aboveSelection(TOOLBAR_SIZE)),
            besideKeyboard: hostOffset(belowSelection),
            belowInset: hostOffset(belowTopCell),
        });

        editor.handle.destroy();
    });

    it('overflows into a compact strip when too wide, and hides when even that cannot fit', () => {
        const editor = renderTableEditor();
        showToolbar(editor, {}, WIDE_TOOLBAR_SIZE);
        layoutToolbar(editor.view, { width: SAFE_AREA.width, height: WIDE_TOOLBAR_SIZE.height });

        const compactWidth = flatStyle(editor.view.getByTestId(DEFAULT_TOOLBAR_TEST_ID)).width;
        const scrolls = editor.view.UNSAFE_queryAllByType(ScrollView).length;
        const compactFrames = editor.driver.registeredFrames();

        editor.driver.emitGeometry({ rects: [ CRAMPED_CELL ], keyboard: CRAMPED_KEYBOARD });

        expect({
            compactWidth,
            scrolls,
            compactFrames,
            hidden: editor.view.queryByTestId(WRAPPER_TEST_ID),
            hiddenFrames: editor.driver.registeredFrames(),
            selectionEvents: editor.onSelectionChange.mock.calls.length,
        }).toEqual({
            compactWidth: SAFE_AREA.width,
            scrolls: 1,
            compactFrames: [
                {
                    x: SAFE_AREA.x,
                    y: SELECTION.y - TABLE_TOOLBAR_GAP - WIDE_TOOLBAR_SIZE.height,
                    width: SAFE_AREA.width,
                    height: WIDE_TOOLBAR_SIZE.height,
                },
            ],
            hidden: null,
            hiddenFrames: [],
            selectionEvents: 1,
        });

        editor.handle.destroy();
    });

    it('hides an offscreen selection and shows it again when it scrolls back', () => {
        const editor = renderTableEditor();
        showToolbar(editor);
        editor.onSelectionChange.mockClear();
        const requestsBefore = tableRequestCount();

        editor.driver.emitGeometry({ rects: [] });

        const offscreen = {
            wrapper: editor.view.queryByTestId(WRAPPER_TEST_ID),
            frames: editor.driver.registeredFrames(),
        };

        editor.driver.emitGeometry();

        expect({
            offscreen,
            restored: placedHostFrame(editor.view),
            selectionEvents: editor.onSelectionChange.mock.calls.length,
            requests: tableRequestCount() - requestsBefore,
        }).toEqual({
            offscreen: { wrapper: null, frames: [] },
            restored: hostOffset(aboveSelection(TOOLBAR_SIZE)),
            selectionEvents: 0,
            requests: 0,
        });

        editor.handle.destroy();
    });

    it('follows rect-only moves at an unchanged revision and layout epoch', () => {
        const editor = renderTableEditor();
        showToolbar(editor);

        editor.driver.emitGeometry({
            rects: [ LEFT_CELL, { ...RIGHT_CELL, width: RIGHT_CELL.width + RESIZE_PREVIEW_SHIFT } ],
        });

        const widened = { ...SELECTION, width: SELECTION.width + RESIZE_PREVIEW_SHIFT };
        expect(placedHostFrame(editor.view)).toEqual(hostOffset(aboveSelection(TOOLBAR_SIZE, widened)));
        editor.handle.destroy();
    });

    it('discards a host measurement that resolves after a newer scroll', () => {
        const editor = renderTableEditor();
        deferHostMeasurements = true;
        editor.driver.focus();
        editor.driver.selectCells();
        editor.driver.emitGeometry();
        layoutToolbar(editor.view, TOOLBAR_SIZE);
        const stale = pendingHostMeasurements.splice(0);

        editor.driver.emitGeometry({ rects: SCROLLED_RECTS });

        act(() => {
            pendingHostMeasurements.forEach(resolve =>
                resolve(SCROLLED_HOST_ORIGIN.x, SCROLLED_HOST_ORIGIN.y, VIEWPORT.width, VIEWPORT.height));

            stale.forEach(resolve => resolve(HOST_ORIGIN.x, HOST_ORIGIN.y, VIEWPORT.width, VIEWPORT.height));
        });

        expect(placedHostFrame(editor.view)).toEqual(
            hostOffset(aboveSelection(TOOLBAR_SIZE, SCROLLED_SELECTION), SCROLLED_HOST_ORIGIN)
        );

        editor.handle.destroy();
    });

    it('keeps each mounted editor on its own geometry and focus frames', () => {
        const handle = createV2LocalHandle(V2_INITIAL_DOC);

        const view = render(
            <>
                <NativeRichTextEditor documentHandle={handle} />
                <NativeRichTextEditor documentHandle={handle} />
            </>
        );

        const first = editorDriver(view, handle, 0);
        const second = editorDriver(view, handle, 1);

        first.focus();
        first.selectCells();
        first.emitGeometry();
        layoutToolbar(view, TOOLBAR_SIZE);

        const firstShown = {
            toolbars: view.getAllByTestId(WRAPPER_TEST_ID).length,
            first: first.registeredFrames(),
        };

        first.focus(false);
        first.clearGeometry();
        second.focus();
        second.selectCells();
        second.emitGeometry({ rects: SCROLLED_RECTS });
        layoutToolbar(view, TOOLBAR_SIZE);

        const secondFrame = aboveSelection(TOOLBAR_SIZE, SCROLLED_SELECTION);

        expect({
            firstShown,
            toolbars: view.getAllByTestId(WRAPPER_TEST_ID).length,
            hosts: view.getAllByTestId(HOST_TEST_ID).length,
            first: first.registeredFrames(),
            second: second.registeredFrames(),
        }).toEqual({
            firstShown: { toolbars: 1, first: [ aboveSelection(TOOLBAR_SIZE) ] },
            toolbars: 1,
            hosts: 1,
            first: [],
            second: [ secondFrame ],
        });

        handle.destroy();
    });

    it('cancels a menu opened over the keyboard once the revision advances beneath it', async() => {
        const editor = renderTableEditor();
        showToolbar(editor, { keyboard: DOCKED_KEYBOARD });
        press(editor.view, TABLE_TOOLBAR_MENU_LABELS.row);
        layoutToolbar(editor.view, MENU_SIZE);

        const menuOpen = {
            back: editor.view.queryByLabelText(TABLE_TOOLBAR_BACK_LABEL) != null,
            frames: editor.driver.registeredFrames(),
        };

        const tableRequestsBefore = tableRequestCount();
        const inputsBefore = mockNativeModule.editorV2ApplyInput.mock.calls.length;

        await act(async() => {
            editor.ref.current!.insertText('x');
        });

        const staleWhileMenuOpen = editor.view.queryByTestId(WRAPPER_TEST_ID);
        editor.driver.emitGeometry({ keyboard: DOCKED_KEYBOARD, layoutEpoch: NEXT_LAYOUT_EPOCH });

        expect({
            menuOpen,
            staleWhileMenuOpen,
            back: editor.view.queryByLabelText(TABLE_TOOLBAR_BACK_LABEL),
            rowMenu: editor.view.queryByLabelText(TABLE_TOOLBAR_MENU_LABELS.row) != null,
            inputs: mockNativeModule.editorV2ApplyInput.mock.calls.length - inputsBefore,
            tableRequests: tableRequestCount() - tableRequestsBefore,
        }).toEqual({
            menuOpen: { back: true, frames: [ aboveSelection(MENU_SIZE) ] },
            staleWhileMenuOpen: null,
            back: null,
            rowMenu: true,
            inputs: 1,
            tableRequests: 0,
        });

        editor.handle.destroy();
    });

    it('dismisses the toolbar and its menu on an outside tap without running a command', () => {
        const editor = renderTableEditor();
        showToolbar(editor);
        press(editor.view, TABLE_TOOLBAR_MENU_LABELS.column);
        const requestsBefore = tableRequestCount();

        editor.driver.focus(false);
        editor.driver.clearGeometry();

        const dismissed = {
            host: editor.view.queryByTestId(HOST_TEST_ID),
            frames: editor.driver.registeredFrames(),
        };

        editor.driver.focus();
        editor.driver.emitGeometry();

        expect({
            dismissed,
            reopenedAtRoot: editor.view.queryByLabelText(TABLE_TOOLBAR_BACK_LABEL) == null,
            requests: tableRequestCount() - requestsBefore,
        }).toEqual({
            dismissed: { host: null, frames: [] },
            reopenedAtRoot: true,
            requests: 0,
        });

        editor.handle.destroy();
    });
});

describe('useTableToolbar', () => {
    const identity: TableToolbarIdentity = {
        editorId: '7',
        ownerId: OWNER_ID,
        documentRevision: '12',
        layoutEpoch: LAYOUT_EPOCH,
    };

    const geometry: TableSelectionGeometry = {
        ...identity,
        tablePos: TABLE_POS,
        coordinateSpace: WINDOW_SPACE,
        rects: [ SELECTION ],
        viewport: VIEWPORT,
    };

    function activeState(commands: Record<string, boolean>): ReadonlyActiveState {
        return { ...ACTIVE_STATE_BASE, commands };
    }

    function renderToolbarHook(initial: Parameters<typeof useTableToolbar>[0]) {
        return renderHook((options: Parameters<typeof useTableToolbar>[0]) => useTableToolbar(options), {
            initialProps: initial,
        });
    }

    function editorRef() {
        const runTableCommand = jest.fn(async() => undefined);
        const editor = { current: { runTableCommand } as Pick<RichTextEditorRef, 'runTableCommand'> };

        return { runTableCommand, editor: editor as { current: RichTextEditorRef } };
    }

    it('derives every action from its own published applicability key only', () => {
        const { editor } = editorRef();

        const enabledBy = ALL_ACTIONS.map(action => {
            const { result } = renderToolbarHook({
                editor,
                geometry,
                identity,
                activeState: activeState({ [TABLE_TOOLBAR_ACTIONS[action].applicability]: true }),
                safeViewport: SAFE_AREA,
                size: TOOLBAR_SIZE,
                enabled: true,
            });

            return ALL_ACTIONS.filter(candidate => result.current.commands[candidate]);
        });

        expect(enabledBy).toEqual(ALL_ACTIONS.map(action => [ action ]));
    });

    it('hides geometry from another owner, a stale revision, or a disabled toolbar', () => {
        const { editor } = editorRef();

        const base = {
            editor,
            geometry,
            identity,
            activeState: activeState(ALL_APPLICABLE),
            safeViewport: SAFE_AREA,
            size: TOOLBAR_SIZE,
            enabled: true,
        };

        const visibility = [
            base,
            { ...base, geometry: { ...geometry, ownerId: OWNER_ID + 1 } },
            { ...base, identity: { ...identity, documentRevision: '13' } },
            { ...base, identity: { ...identity, layoutEpoch: NEXT_LAYOUT_EPOCH } },
            { ...base, enabled: false },
            { ...base, safeViewport: { ...SAFE_AREA, width: Number.NaN } },
        ].map(options => {
            const { result } = renderToolbarHook(options);

            return { visible: result.current.visible, frame: result.current.frame };
        });

        expect(visibility).toEqual([
            { visible: true, frame: aboveSelection(TOOLBAR_SIZE) },
            { visible: false, frame: null },
            { visible: false, frame: null },
            { visible: false, frame: null },
            { visible: false, frame: null },
            { visible: false, frame: null },
        ]);
    });

    it('refuses to run a command captured before the owner or revision changed', async() => {
        const { editor, runTableCommand } = editorRef();

        const options = {
            editor,
            geometry,
            identity,
            activeState: activeState(ALL_APPLICABLE),
            safeViewport: SAFE_AREA,
            size: TOOLBAR_SIZE,
            enabled: true,
        };

        const { result, rerender } = renderToolbarHook(options);
        const captured = result.current;
        const advanced = { ...identity, documentRevision: '13' };

        rerender({ ...options, identity: advanced, geometry: { ...geometry, ...advanced } });

        const stale = await captured.run(TABLE_TOOLBAR_ACTIONS.mergeCells.command).then(
            () => null,
            (error: unknown) => error
        );

        await result.current.run(TABLE_TOOLBAR_ACTIONS.splitCell.command);

        expect({
            staleCode: stale instanceof NativeEditorOperationError ? stale.code : stale,
            ran: runTableCommand.mock.calls,
        }).toEqual({
            staleCode: 'REVISION_MISMATCH',
            ran: [ [ TABLE_TOOLBAR_ACTIONS.splitCell.command ] ],
        });
    });
});
