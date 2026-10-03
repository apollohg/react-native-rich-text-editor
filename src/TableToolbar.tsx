import { useCallback, useState } from 'react';
import { Pressable, ScrollView, StyleSheet, Text, View } from 'react-native';
import { type EditorToolbarTheme } from './EditorTheme';
import {
    COMMAND_NOT_APPLICABLE_ERROR_CODE,
    NativeEditorOperationError,
} from './NativeEditorBoundaryError';
import {
    MENU_SHADOW,
    TOOLBAR_BG,
    TOOLBAR_BORDER,
    resolveToolbarButtonVisuals,
    resolveToolbarMetrics,
} from './EditorToolbarVisuals';
import { BUTTON_HIT } from './EditorToolbarRegistry';
import {
    TABLE_TOOLBAR_ACTIONS,
    type TableToolbarAction,
    type TableToolbarState,
} from './useTableToolbar';

export interface TableToolbarProps extends TableToolbarState {
    theme?: EditorToolbarTheme;
}

export type TableToolbarMenu = 'row' | 'column' | 'header' | 'more';

type TableToolbarRootItem =
    | { kind: 'menu'; menu: TableToolbarMenu }
    | { kind: 'action'; action: TableToolbarAction };

export const TABLE_TOOLBAR_MENUS: Readonly<Record<TableToolbarMenu, readonly TableToolbarAction[]>> = {
    row: [ 'addRowBefore', 'addRowAfter', 'deleteRows', 'selectRows' ],
    column: [ 'addColumnBefore', 'addColumnAfter', 'deleteColumns', 'selectColumns' ],
    header: [ 'toggleHeaderRow', 'toggleHeaderColumn', 'toggleHeaderCell' ],
    more: [ 'clearCells', 'deleteTable' ],
};

const TABLE_TOOLBAR_ROOT_ITEMS: readonly TableToolbarRootItem[] = [
    { kind: 'menu', menu: 'row' },
    { kind: 'menu', menu: 'column' },
    { kind: 'menu', menu: 'header' },
    { kind: 'action', action: 'mergeCells' },
    { kind: 'action', action: 'splitCell' },
    { kind: 'menu', menu: 'more' },
];

export const TABLE_TOOLBAR_MENU_LABELS: Readonly<Record<TableToolbarMenu, string>> = {
    row: 'Row actions',
    column: 'Column actions',
    header: 'Header actions',
    more: 'More table actions',
};

const TABLE_TOOLBAR_MENU_TITLES: Readonly<Record<TableToolbarMenu, string>> = {
    row: 'Row',
    column: 'Column',
    header: 'Header',
    more: '⋯',
};

export const TABLE_TOOLBAR_ACTION_LABELS: Readonly<Record<TableToolbarAction, string>> = {
    addRowBefore: 'Insert row above',
    addRowAfter: 'Insert row below',
    deleteRows: 'Delete rows',
    selectRows: 'Select rows',
    addColumnBefore: 'Insert column before',
    addColumnAfter: 'Insert column after',
    deleteColumns: 'Delete columns',
    selectColumns: 'Select columns',
    toggleHeaderRow: 'Toggle header row',
    toggleHeaderColumn: 'Toggle header column',
    toggleHeaderCell: 'Toggle header cell',
    mergeCells: 'Merge cells',
    splitCell: 'Split cell',
    clearCells: 'Clear cells',
    deleteTable: 'Delete table',
};

const TABLE_TOOLBAR_ACTION_TITLES: Readonly<Record<TableToolbarAction, string>> = {
    addRowBefore: 'Above',
    addRowAfter: 'Below',
    deleteRows: 'Delete',
    selectRows: 'Select',
    addColumnBefore: 'Before',
    addColumnAfter: 'After',
    deleteColumns: 'Delete',
    selectColumns: 'Select',
    toggleHeaderRow: 'Row',
    toggleHeaderColumn: 'Column',
    toggleHeaderCell: 'Cell',
    mergeCells: 'Merge',
    splitCell: 'Split',
    clearCells: 'Clear',
    deleteTable: 'Delete table',
};

export const TABLE_TOOLBAR_BACK_LABEL = 'Back to table actions';

const TABLE_TOOLBAR_BACK_TITLE = '‹';

const TABLE_TOOLBAR_RADIUS = 10;

const TABLE_TOOLBAR_BUTTON_PADDING = 6;

const TABLE_TOOLBAR_LABEL_SIZE = 14;

const TABLE_TOOLBAR_SHADOW_OPACITY = 0.16;

const TABLE_TOOLBAR_SHADOW_RADIUS = 12;

const TABLE_TOOLBAR_SHADOW_OFFSET = { width: 0, height: 4 };

const TABLE_TOOLBAR_ELEVATION = 8;

function isSupersededTableActionError(error: unknown): boolean {
    return (
        error instanceof NativeEditorOperationError &&
        (error.code === 'REVISION_MISMATCH' || error.code === COMMAND_NOT_APPLICABLE_ERROR_CODE)
    );
}

export function TableToolbar({
    frame, compact, commands, run, reportError, theme,
}: TableToolbarProps) {
    const [ menu, setMenu ] = useState<TableToolbarMenu | null>(null);
    const { toolbarHeight, buttonHeight, paddingVertical } = resolveToolbarMetrics(theme);

    const runAction = useCallback(
        (action: TableToolbarAction) => {
            setMenu(null);

            void run(TABLE_TOOLBAR_ACTIONS[action].command).catch((error: unknown) => {
                if (!isSupersededTableActionError(error)) {
                    reportError(error);
                }
            });
        },
        [ reportError, run ]
    );

    const renderButton = (
        key: string,
        title: string,
        label: string,
        isDisabled: boolean,
        onPress: () => void,
        expanded?: boolean
    ) => {
        const visuals = resolveToolbarButtonVisuals({ isDisabled }, theme, buttonHeight);

        return (
            <Pressable
                key={key}
                onPress={onPress}
                disabled={isDisabled}
                accessibilityRole={'button'}
                accessibilityLabel={label}
                accessibilityState={{ disabled: isDisabled, expanded }}
                style={[
                    styles.button,
                    {
                        height: buttonHeight,
                        borderRadius: visuals.borderRadius,
                        backgroundColor: visuals.backgroundColor,
                    },
                ]}
            >
                <Text numberOfLines={1} style={[ styles.title, { color: visuals.color } ]}>
                    {title}
                </Text>
            </Pressable>
        );
    };

    const renderAction = (action: TableToolbarAction) =>
        renderButton(
            action,
            TABLE_TOOLBAR_ACTION_TITLES[action],
            TABLE_TOOLBAR_ACTION_LABELS[action],
            !commands[action],
            () => runAction(action)
        );

    const buttons =
        menu == null
            ? TABLE_TOOLBAR_ROOT_ITEMS.map(item =>
                item.kind === 'action'
                    ? renderAction(item.action)
                    : renderButton(
                        item.menu,
                        TABLE_TOOLBAR_MENU_TITLES[item.menu],
                        TABLE_TOOLBAR_MENU_LABELS[item.menu],
                        !TABLE_TOOLBAR_MENUS[item.menu].some(action => commands[action]),
                        () => setMenu(item.menu),
                        false
                    ))
            : [
                renderButton(
                    TABLE_TOOLBAR_BACK_LABEL,
                    TABLE_TOOLBAR_BACK_TITLE,
                    TABLE_TOOLBAR_BACK_LABEL,
                    false,
                    () => setMenu(null),
                    true
                ),
                ...TABLE_TOOLBAR_MENUS[menu].map(renderAction),
            ];

    const row = <View style={styles.row}>{buttons}</View>;

    return (
        <View
            testID={'table-toolbar'}
            style={[
                styles.container,
                {
                    minHeight: toolbarHeight,
                    paddingVertical,
                    backgroundColor: theme?.backgroundColor ?? TOOLBAR_BG,
                    borderColor: theme?.borderColor ?? TOOLBAR_BORDER,
                    borderWidth: theme?.borderWidth ?? StyleSheet.hairlineWidth,
                    borderRadius: theme?.borderRadius ?? TABLE_TOOLBAR_RADIUS,
                },
                compact && frame != null ? { width: frame.width } : null,
            ]}
        >
            {compact ? (
                <ScrollView
                    horizontal
                    showsHorizontalScrollIndicator={false}
                    keyboardShouldPersistTaps={'always'}
                >
                    {row}
                </ScrollView>
            ) : (
                row
            )}
        </View>
    );
}

const styles = StyleSheet.create({
    container: {
        overflow: 'hidden',
        shadowColor: MENU_SHADOW,
        shadowOpacity: TABLE_TOOLBAR_SHADOW_OPACITY,
        shadowRadius: TABLE_TOOLBAR_SHADOW_RADIUS,
        shadowOffset: TABLE_TOOLBAR_SHADOW_OFFSET,
        elevation: TABLE_TOOLBAR_ELEVATION,
    },
    row: {
        flexDirection: 'row',
        alignItems: 'center',
        paddingHorizontal: TABLE_TOOLBAR_BUTTON_PADDING,
    },
    button: {
        minWidth: BUTTON_HIT,
        paddingHorizontal: TABLE_TOOLBAR_BUTTON_PADDING,
        justifyContent: 'center',
        alignItems: 'center',
    },
    title: {
        fontSize: TABLE_TOOLBAR_LABEL_SIZE,
        fontWeight: '600',
    },
});
