import {
    hasExactOwnKeys,
    hasOnlyOwnKeys,
    invalidV2RequestError,
    isPlainRecord,
    nativeEditorV2U32,
    normalizeRevisionField,
    requireNativeEditorV2U32,
} from './NativeEditorResultNormalization';
import { type NativeEditorSelectionEnvelope } from './NativeEditorTypes';
import {
    type TableCellStep,
    type TableEdge,
    type TableHeaderTarget,
    type TableSelectionGeometry,
} from './TableTypes';
import { type TableToolbarObstructions } from './TableToolbarPlacement';

export type NativeTableSelectionGeometry =
    | { kind: 'cleared'; editorId: string }
    | {
          kind: 'geometry';
          geometry: Omit<TableSelectionGeometry, 'ownerId'>;
          obstructions: TableToolbarObstructions;
          editMenuVisible: boolean;
      };

type TableSelectionRect = TableSelectionGeometry['viewport'];

export type TableCommandRequest =
    | { kind: 'command'; command: Record<string, unknown> }
    | { kind: 'selection'; selection: NativeEditorSelectionEnvelope };

const TABLE_COMMAND_ERROR_PREFIX = 'NativeEditorBridge: invalid table command';
const MIN_TABLE_EXTENT = 1;
const CELL_POSITION_KIND = 'document';
const RESIZE_COLUMN_WIRE_TYPE = 'setTableColumnWidth';
const ADJACENT_CELL_WIRE_TYPE = 'moveToAdjacentCell';
const TABLE_EDGES: readonly TableEdge[] = [ 'before', 'after' ];
export const TABLE_SELECTION_COORDINATE_SPACE = 'window';
const TABLE_SELECTION_RECT_FIELDS = [ 'x', 'y', 'width', 'height' ];
const TABLE_SELECTION_CLEARED_FIELDS = [ 'editorId' ];
const TABLE_SELECTION_GEOMETRY_FIELDS = [
    'editorId',
    'documentRevision',
    'layoutEpoch',
    'tablePos',
    'coordinateSpace',
    'rects',
    'viewport',
    'safeArea',
    'editMenuVisible',
];
const TABLE_SELECTION_KEYBOARD_FIELD = 'keyboard';
const TABLE_HEADER_TARGETS: readonly TableHeaderTarget[] = [ 'row', 'column', 'cell' ];

const TABLE_CELL_STEPS = new Map<unknown, string>([
    [ -1 satisfies TableCellStep, 'backward' ],
    [ 1 satisfies TableCellStep, 'forward' ],
]);

const FIELDLESS_TABLE_COMMANDS = new Set([
    'deleteTable',
    'deleteTableRows',
    'deleteTableColumns',
    'mergeTableCells',
    'splitTableCell',
    'selectTableRows',
    'selectTableColumns',
    'clearTableCells',
]);

function invalidTableCommand(detail: string): never {
    throw invalidV2RequestError(`${TABLE_COMMAND_ERROR_PREFIX} ${detail}`);
}

function requireFields(command: Record<string, unknown>, type: string, fields: string[]): void {
    if (!hasExactOwnKeys(command, [ 'type', ...fields ])) {
        invalidTableCommand(`${type} fields`);
    }
}

function requirePositive(value: unknown, field: string): number {
    const extent = requireNativeEditorV2U32(value, field);

    if (extent < MIN_TABLE_EXTENT) {
        invalidTableCommand(field);
    }

    return extent;
}

function requireBoolean(value: unknown, field: string): boolean {
    if (typeof value !== 'boolean') {
        invalidTableCommand(field);
    }

    return value;
}

function requireMember<T>(value: unknown, members: readonly T[], field: string): T {
    const member = members.find(candidate => candidate === value);

    if (member === undefined) {
        invalidTableCommand(field);
    }

    return member;
}

function cellPosition(value: unknown, field: string) {
    return { offset: requireNativeEditorV2U32(value, field), kind: CELL_POSITION_KIND } as const;
}

function insertTableCommand(command: Record<string, unknown>): Record<string, unknown> {
    if (!hasOnlyOwnKeys(command, [ 'type', 'rows', 'columns', 'withHeaderRow' ])) {
        invalidTableCommand('insertTable fields');
    }

    const wire: Record<string, unknown> = { type: command.type };

    if (command.rows !== undefined) {
        wire.rows = requirePositive(command.rows, 'rows');
    }

    if (command.columns !== undefined) {
        wire.columns = requirePositive(command.columns, 'columns');
    }

    if (command.withHeaderRow !== undefined) {
        wire.withHeaderRow = requireBoolean(command.withHeaderRow, 'withHeaderRow');
    }

    return wire;
}

export function normalizeTableCommand(command: unknown): TableCommandRequest {
    if (!isPlainRecord(command) || typeof command.type !== 'string') {
        return invalidTableCommand('type');
    }

    const { type } = command;

    if (FIELDLESS_TABLE_COMMANDS.has(type)) {
        requireFields(command, type, []);

        return { kind: 'command', command: { type } };
    }

    switch (type) {
        case 'insertTable':
            return { kind: 'command', command: insertTableCommand(command) };
        case 'addTableRow':
        case 'addTableColumn':
            requireFields(command, type, [ 'side' ]);

            return {
                kind: 'command',
                command: { type, side: requireMember(command.side, TABLE_EDGES, 'side') },
            };
        case 'toggleTableHeader':
            requireFields(command, type, [ 'target' ]);

            return {
                kind: 'command',
                command: {
                    type,
                    target: requireMember(command.target, TABLE_HEADER_TARGETS, 'target'),
                },
            };
        case 'resizeTableColumn':
            requireFields(command, type, [ 'tablePos', 'column', 'width' ]);

            return {
                kind: 'command',
                command: {
                    type: RESIZE_COLUMN_WIRE_TYPE,
                    width: requirePositive(command.width, 'width'),
                    column: requireNativeEditorV2U32(command.column, 'column'),
                    tablePos: requireNativeEditorV2U32(command.tablePos, 'tablePos'),
                },
            };
        case 'selectTableCells':
            requireFields(command, type, [ 'anchorCell', 'headCell' ]);

            return {
                kind: 'selection',
                selection: {
                    type: 'cell',
                    anchorCell: cellPosition(command.anchorCell, 'anchorCell'),
                    headCell: cellPosition(command.headCell, 'headCell'),
                },
            };
        case 'goToTableCell': {
            requireFields(command, type, [ 'direction', 'appendRow' ]);
            const step = TABLE_CELL_STEPS.get(command.direction);

            if (step === undefined) {
                return invalidTableCommand('direction');
            }

            return {
                kind: 'command',
                command: {
                    type: ADJACENT_CELL_WIRE_TYPE,
                    step,
                    appendRow: requireBoolean(command.appendRow, 'appendRow'),
                },
            };
        }
        default:
            return invalidTableCommand('type');
    }
}

function isFiniteExtent(value: unknown): value is number {
    return typeof value === 'number' && Number.isFinite(value) && value >= 0;
}

function tableSelectionRect(value: unknown): TableSelectionRect | null {
    if (!isPlainRecord(value) || !hasExactOwnKeys(value, TABLE_SELECTION_RECT_FIELDS)) {
        return null;
    }

    const { x, y, width, height } = value;

    if (
        typeof x !== 'number' ||
        !Number.isFinite(x) ||
        typeof y !== 'number' ||
        !Number.isFinite(y) ||
        !isFiniteExtent(width) ||
        !isFiniteExtent(height)
    ) {
        return null;
    }

    return { x, y, width, height };
}

function isSameTableSelectionRect(left: TableSelectionRect, right: TableSelectionRect): boolean {
    return (
        left.x === right.x &&
        left.y === right.y &&
        left.width === right.width &&
        left.height === right.height
    );
}

export function isSameTableSelectionGeometry(
    left: TableSelectionGeometry,
    right: TableSelectionGeometry
): boolean {
    return (
        left.editorId === right.editorId &&
        left.ownerId === right.ownerId &&
        left.documentRevision === right.documentRevision &&
        left.layoutEpoch === right.layoutEpoch &&
        left.tablePos === right.tablePos &&
        left.coordinateSpace === right.coordinateSpace &&
        isSameTableSelectionRect(left.viewport, right.viewport) &&
        left.rects.length === right.rects.length &&
        left.rects.every((rect, index) => isSameTableSelectionRect(rect, right.rects[index]))
    );
}

function tableSelectionRects(value: unknown): TableSelectionRect[] | null {
    if (!Array.isArray(value)) {
        return null;
    }

    const rects: TableSelectionRect[] = [];

    for (const candidate of value) {
        const rect = tableSelectionRect(candidate);

        if (rect == null) {
            return null;
        }

        rects.push(rect);
    }

    return rects;
}

export function normalizeNativeTableSelectionGeometry(
    payload: unknown
): NativeTableSelectionGeometry | null {
    if (!isPlainRecord(payload)) {
        return null;
    }

    const editorId = normalizeRevisionField(payload, 'editorId');

    if (editorId == null) {
        return null;
    }

    if (hasExactOwnKeys(payload, TABLE_SELECTION_CLEARED_FIELDS)) {
        return { kind: 'cleared', editorId };
    }

    const reportsKeyboard = Object.prototype.hasOwnProperty.call(
        payload,
        TABLE_SELECTION_KEYBOARD_FIELD
    );

    const fields = reportsKeyboard
        ? [ ...TABLE_SELECTION_GEOMETRY_FIELDS, TABLE_SELECTION_KEYBOARD_FIELD ]
        : TABLE_SELECTION_GEOMETRY_FIELDS;

    if (
        !hasExactOwnKeys(payload, fields) ||
        payload.coordinateSpace !== TABLE_SELECTION_COORDINATE_SPACE
    ) {
        return null;
    }

    const documentRevision = normalizeRevisionField(payload, 'documentRevision');
    const layoutEpoch = normalizeRevisionField(payload, 'layoutEpoch');
    const tablePos = nativeEditorV2U32(payload.tablePos);
    const rects = tableSelectionRects(payload.rects);
    const viewport = tableSelectionRect(payload.viewport);
    const safeArea = tableSelectionRect(payload.safeArea);
    const keyboard = reportsKeyboard ? tableSelectionRect(payload.keyboard) : null;

    if (
        documentRevision == null ||
        layoutEpoch == null ||
        tablePos == null ||
        rects == null ||
        viewport == null ||
        safeArea == null ||
        typeof payload.editMenuVisible !== 'boolean' ||
        (reportsKeyboard && keyboard == null)
    ) {
        return null;
    }

    return {
        kind: 'geometry',
        geometry: {
            editorId,
            documentRevision,
            layoutEpoch,
            tablePos,
            coordinateSpace: TABLE_SELECTION_COORDINATE_SPACE,
            rects,
            viewport,
        },
        obstructions: { safeArea, keyboard },
        editMenuVisible: payload.editMenuVisible,
    };
}
