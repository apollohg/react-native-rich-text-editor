import { useCallback, useMemo, useRef, type RefObject } from 'react';
import { type ReadonlyActiveState } from './NativeEditorBridge';
import { localOperationError } from './NativeEditorResultNormalization';
import { TABLE_SELECTION_COORDINATE_SPACE } from './TableNormalization';
import { type RichTextEditorRef } from './RichTextEditorTypes';
import { type TableCommand, type TableSelectionGeometry } from './TableTypes';
import {
    intersectRects,
    isFiniteRect,
    isFiniteSize,
    resolveTableToolbarPlacement,
    unionRects,
    type Rect,
    type Size,
} from './TableToolbarPlacement';

export interface TableToolbarActionSpec {
    applicability: string;
    command: TableCommand;
}

export const TABLE_TOOLBAR_ACTIONS = {
    addRowBefore: {
        applicability: 'addTableRowBefore',
        command: { type: 'addTableRow', side: 'before' },
    },
    addRowAfter: {
        applicability: 'addTableRowAfter',
        command: { type: 'addTableRow', side: 'after' },
    },
    deleteRows: { applicability: 'deleteTableRows', command: { type: 'deleteTableRows' } },
    selectRows: { applicability: 'selectTableRows', command: { type: 'selectTableRows' } },
    addColumnBefore: {
        applicability: 'addTableColumnBefore',
        command: { type: 'addTableColumn', side: 'before' },
    },
    addColumnAfter: {
        applicability: 'addTableColumnAfter',
        command: { type: 'addTableColumn', side: 'after' },
    },
    deleteColumns: { applicability: 'deleteTableColumns', command: { type: 'deleteTableColumns' } },
    selectColumns: { applicability: 'selectTableColumns', command: { type: 'selectTableColumns' } },
    toggleHeaderRow: {
        applicability: 'toggleTableHeaderRow',
        command: { type: 'toggleTableHeader', target: 'row' },
    },
    toggleHeaderColumn: {
        applicability: 'toggleTableHeaderColumn',
        command: { type: 'toggleTableHeader', target: 'column' },
    },
    toggleHeaderCell: {
        applicability: 'toggleTableHeaderCell',
        command: { type: 'toggleTableHeader', target: 'cell' },
    },
    mergeCells: { applicability: 'mergeTableCells', command: { type: 'mergeTableCells' } },
    splitCell: { applicability: 'splitTableCell', command: { type: 'splitTableCell' } },
    clearCells: { applicability: 'clearTableCells', command: { type: 'clearTableCells' } },
    deleteTable: { applicability: 'deleteTable', command: { type: 'deleteTable' } },
} as const satisfies Record<string, TableToolbarActionSpec>;

export type TableToolbarAction = keyof typeof TABLE_TOOLBAR_ACTIONS;

const TABLE_TOOLBAR_ACTION_NAMES = Object.keys(TABLE_TOOLBAR_ACTIONS) as TableToolbarAction[];

export type TableToolbarIdentity = Pick<
    TableSelectionGeometry,
    'editorId' | 'ownerId' | 'documentRevision' | 'layoutEpoch'
>;

export interface TableToolbarState {
    visible: boolean;
    frame: Rect | null;
    compact: boolean;
    commands: Readonly<Record<TableToolbarAction, boolean>>;
    run: (command: TableCommand) => Promise<void>;
    reportError: (error: unknown) => void;
}

export interface TableToolbarOptions {
    editor: RefObject<RichTextEditorRef | null>;
    geometry: TableSelectionGeometry | null;
    identity: TableToolbarIdentity | null;
    activeState: ReadonlyActiveState;
    safeViewport: Rect | null;
    size: Size | null;
    enabled: boolean;
    onError: (error: unknown) => void;
}

const STALE_TABLE_SELECTION_MESSAGE =
    'NativeRichTextEditor: the table selection changed before the toolbar action ran';

const DETACHED_TABLE_TOOLBAR_MESSAGE =
    'NativeRichTextEditor: the table toolbar has no mounted editor';

function isSameIdentity(left: TableToolbarIdentity, right: TableToolbarIdentity): boolean {
    return (
        left.editorId === right.editorId &&
        left.ownerId === right.ownerId &&
        left.documentRevision === right.documentRevision &&
        left.layoutEpoch === right.layoutEpoch
    );
}

export function tableToolbarSelectionKey(geometry: TableSelectionGeometry): string {
    return [
        geometry.editorId,
        geometry.ownerId,
        geometry.documentRevision,
        geometry.layoutEpoch,
        geometry.tablePos,
    ].join(':');
}

function currentGeometry({
    geometry,
    identity,
    enabled,
}: Pick<TableToolbarOptions, 'geometry' | 'identity' | 'enabled'>): TableSelectionGeometry | null {
    if (
        !enabled ||
        geometry == null ||
        identity == null ||
        geometry.coordinateSpace !== TABLE_SELECTION_COORDINATE_SPACE ||
        !isSameIdentity(geometry, identity) ||
        !geometry.rects.every(isFiniteRect)
    ) {
        return null;
    }

    return geometry;
}

export function useTableToolbar({
    editor,
    geometry,
    identity,
    activeState,
    safeViewport,
    size,
    enabled,
    onError,
}: TableToolbarOptions): TableToolbarState {
    const current = currentGeometry({ geometry, identity, enabled });

    const placement = useMemo(() => {
        if (
            current == null ||
            safeViewport == null ||
            size == null ||
            !isFiniteRect(safeViewport) ||
            !isFiniteSize(size)
        ) {
            return null;
        }

        const selection = unionRects(current.rects);
        const anchor = selection == null ? null : intersectRects(selection, safeViewport);

        return anchor == null ? null : resolveTableToolbarPlacement(anchor, safeViewport, size);
    }, [ current, safeViewport, size ]);

    const applicability = activeState.commands;

    const commands = useMemo(() => {
        const available = {} as Record<TableToolbarAction, boolean>;

        for (const action of TABLE_TOOLBAR_ACTION_NAMES) {
            available[action] = applicability[TABLE_TOOLBAR_ACTIONS[action].applicability] === true;
        }

        return available;
    }, [ applicability ]);

    const latestIdentityRef = useRef<TableToolbarIdentity | null>(null);

    latestIdentityRef.current = current;

    const capturedEditorId = current?.editorId;
    const capturedOwnerId = current?.ownerId;
    const capturedDocumentRevision = current?.documentRevision;
    const capturedLayoutEpoch = current?.layoutEpoch;

    const run = useCallback(
        async(command: TableCommand): Promise<void> => {
            const latest = latestIdentityRef.current;

            if (
                capturedEditorId === undefined ||
                capturedOwnerId === undefined ||
                capturedDocumentRevision === undefined ||
                capturedLayoutEpoch === undefined ||
                latest == null ||
                !isSameIdentity(latest, {
                    editorId: capturedEditorId,
                    ownerId: capturedOwnerId,
                    documentRevision: capturedDocumentRevision,
                    layoutEpoch: capturedLayoutEpoch,
                })
            ) {
                throw localOperationError('REVISION_MISMATCH', STALE_TABLE_SELECTION_MESSAGE);
            }

            const target = editor.current;

            if (target == null) {
                throw localOperationError('ENGINE_NOT_READY', DETACHED_TABLE_TOOLBAR_MESSAGE);
            }

            await target.runTableCommand(command);
        },
        [ capturedDocumentRevision,
            capturedEditorId,
            capturedLayoutEpoch,
            capturedOwnerId,
            editor ]
    );

    return useMemo(
        () => ({
            visible: placement != null,
            frame: placement?.frame ?? null,
            compact: placement?.compact ?? false,
            commands,
            run,
            reportError: onError,
        }),
        [ commands, onError, placement, run ]
    );
}
