import { createElement, type ReactNode } from 'react';
import {
    TABLE_TOOLBAR_ACTIONS,
    TableToolbar,
    placeTableToolbar,
    useTableToolbar,
    type RichTextEditorProps,
    type TableCommand,
    type TableToolbarAction,
    type TableToolbarOptions,
    type TableToolbarRect,
    type TableToolbarState,
} from '../index';

declare const state: TableToolbarState;

const visible: boolean = state.visible;
const frame: TableToolbarRect | null = state.frame;
const compact: boolean = state.compact;
const canMerge: boolean = state.commands.mergeCells;
const pending: Promise<void> = state.run({ type: 'mergeTableCells' });

void [ visible,
    frame,
    compact,
    canMerge,
    pending ];

// @ts-expect-error commands are keyed by toolbar action, not arbitrary strings
const unknownAction: boolean = state.commands.dropTable;
void unknownAction;

// @ts-expect-error run takes a public table command
void state.run({ type: 'setTableColumnWidth', width: 120 });

const renderer: RichTextEditorProps['tableToolbar'] = (current: TableToolbarState): ReactNode =>
    current.visible ? createElement(TableToolbar, { ...current, theme: { buttonColor: '#000' } }) : null;

void renderer;

const hidden: RichTextEditorProps['tableToolbar'] = false;
void hidden;

// @ts-expect-error only false disables the toolbar; true is not a renderer
const enabledFlag: RichTextEditorProps['tableToolbar'] = true;
void enabledFlag;

const deleteAction: TableToolbarAction = 'deleteTable';
const deleteCommand: TableCommand = TABLE_TOOLBAR_ACTIONS[deleteAction].command;
void deleteCommand;

// @ts-expect-error toolbar actions come from the exported action map
const unknownToolbarAction: TableToolbarAction = 'dropTable';
void unknownToolbarAction;

const placed: TableToolbarRect | null = placeTableToolbar(
    { x: 80, y: 100, width: 80, height: 40 },
    { x: 0, y: 0, width: 390, height: 600 },
    { width: 200, height: 44 }
);

void placed;

export function useToolbarContract(options: TableToolbarOptions): TableToolbarState {
    return useTableToolbar(options);
}

export function useIncompleteToolbar(): TableToolbarState {
    // @ts-expect-error a toolbar needs its editor, geometry identity, and measured inputs
    return useTableToolbar({ enabled: true });
}
