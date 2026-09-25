import './helpers/NativeRichTextEditorFixture';
import {
    mockNativeModule,
    V2_INITIAL_DOC,
    V2_DOC_B,
    createV2LocalHandle,
} from './helpers/NativeRichTextEditorFixture';
import { okRecord, operationError } from './helpers/nativeEditorV2FakeRecords';
import { createRef } from 'react';
import { render, act } from '@testing-library/react-native';
import { NativeRichTextEditor, type NativeRichTextEditorRef } from '../NativeRichTextEditor';
import {
    NativeEditorEngineBoundaryError,
    NativeEditorNonRetryableError,
    NativeEditorOperationError,
} from '../NativeEditorBoundaryError';
import { type NativeEditorDocumentHandle, type Selection } from '../NativeEditorBridge';
import { type TableCommand } from '../TableTypes';

const TABLE_POS = 0;
const RESIZED_COLUMN = 1;
const RESIZED_WIDTH = 180;
const INSERTED_ROWS = 4;
const INSERTED_COLUMNS = 2;
const ANCHOR_CELL = 2;
const HEAD_CELL = 16;
const SCALAR_ANCHOR = 1;
const SCALAR_HEAD = 5;
const U32_OVERFLOW = 0x1_0000_0000;
const FORWARD: 1 = 1;
const BACKWARD: -1 = -1;

type EngineEffect = 'document' | 'selection';

interface DiscriminantCase {
    command: TableCommand;
    wire: Record<string, unknown>;
    effect: EngineEffect;
}

const COMMAND_CASES: DiscriminantCase[] = [
    {
        command: { type: 'insertTable' },
        wire: { type: 'insertTable' },
        effect: 'document',
    },
    {
        command: {
            type: 'insertTable',
            rows: INSERTED_ROWS,
            columns: INSERTED_COLUMNS,
            withHeaderRow: false,
        },
        wire: {
            type: 'insertTable',
            rows: INSERTED_ROWS,
            columns: INSERTED_COLUMNS,
            withHeaderRow: false,
        },
        effect: 'document',
    },
    { command: { type: 'deleteTable' }, wire: { type: 'deleteTable' }, effect: 'document' },
    {
        command: { type: 'addTableRow', side: 'before' },
        wire: { type: 'addTableRow', side: 'before' },
        effect: 'document',
    },
    {
        command: { type: 'addTableRow', side: 'after' },
        wire: { type: 'addTableRow', side: 'after' },
        effect: 'document',
    },
    {
        command: { type: 'deleteTableRows' },
        wire: { type: 'deleteTableRows' },
        effect: 'document',
    },
    {
        command: { type: 'addTableColumn', side: 'before' },
        wire: { type: 'addTableColumn', side: 'before' },
        effect: 'document',
    },
    {
        command: { type: 'addTableColumn', side: 'after' },
        wire: { type: 'addTableColumn', side: 'after' },
        effect: 'document',
    },
    {
        command: { type: 'deleteTableColumns' },
        wire: { type: 'deleteTableColumns' },
        effect: 'document',
    },
    ...([ 'row', 'column', 'cell' ] as const).map(target => ({
        command: { type: 'toggleTableHeader', target } as const,
        wire: { type: 'toggleTableHeader', target },
        effect: 'document' as const,
    })),
    {
        command: { type: 'mergeTableCells' },
        wire: { type: 'mergeTableCells' },
        effect: 'document',
    },
    {
        command: { type: 'splitTableCell' },
        wire: { type: 'splitTableCell' },
        effect: 'document',
    },
    {
        command: {
            type: 'resizeTableColumn',
            tablePos: TABLE_POS,
            column: RESIZED_COLUMN,
            width: RESIZED_WIDTH,
        },
        wire: {
            type: 'setTableColumnWidth',
            width: RESIZED_WIDTH,
            column: RESIZED_COLUMN,
            tablePos: TABLE_POS,
        },
        effect: 'document',
    },
    {
        command: { type: 'selectTableRows' },
        wire: { type: 'selectTableRows' },
        effect: 'selection',
    },
    {
        command: { type: 'selectTableColumns' },
        wire: { type: 'selectTableColumns' },
        effect: 'selection',
    },
    {
        command: { type: 'clearTableCells' },
        wire: { type: 'clearTableCells' },
        effect: 'document',
    },
    {
        command: { type: 'goToTableCell', direction: FORWARD, appendRow: true },
        wire: { type: 'moveToAdjacentCell', step: 'forward', appendRow: true },
        effect: 'selection',
    },
    {
        command: { type: 'goToTableCell', direction: BACKWARD, appendRow: false },
        wire: { type: 'moveToAdjacentCell', step: 'backward', appendRow: false },
        effect: 'selection',
    },
];

const INVALID_COMMANDS: Array<[string, unknown]> = [
    [ 'a non-object payload', 'insertTable' ],
    [ 'a missing discriminant', { rows: INSERTED_ROWS } ],
    [ 'an unknown discriminant', { type: 'dropTable' } ],
    [ 'a raw engine discriminant', { type: 'setTableColumnWidth', width: RESIZED_WIDTH } ],
    [ 'zero rows', { type: 'insertTable', rows: 0 } ],
    [ 'negative columns', { type: 'insertTable', columns: -1 } ],
    [ 'fractional rows', { type: 'insertTable', rows: 1.5 } ],
    [ 'non-finite columns', { type: 'insertTable', columns: Number.POSITIVE_INFINITY } ],
    [ 'u32-overflowing rows', { type: 'insertTable', rows: U32_OVERFLOW } ],
    [ 'a string header flag', { type: 'insertTable', withHeaderRow: 'yes' } ],
    [ 'an unknown edge', { type: 'addTableRow', side: 'middle' } ],
    [ 'a missing edge', { type: 'addTableColumn' } ],
    [ 'an unknown header target', { type: 'toggleTableHeader', target: 'table' } ],
    [
        'a zero width',
        { type: 'resizeTableColumn', tablePos: TABLE_POS, column: RESIZED_COLUMN, width: 0 },
    ],
    [
        'a negative table position',
        { type: 'resizeTableColumn', tablePos: -1, column: RESIZED_COLUMN, width: RESIZED_WIDTH },
    ],
    [
        'a decimal-string column',
        { type: 'resizeTableColumn', tablePos: TABLE_POS, column: '1', width: RESIZED_WIDTH },
    ],
    [ 'a missing resize column', { type: 'resizeTableColumn', tablePos: TABLE_POS, width: 1 } ],
    [ 'a negative anchor cell', { type: 'selectTableCells', anchorCell: -1, headCell: HEAD_CELL } ],
    [
        'an overflowing head cell',
        { type: 'selectTableCells', anchorCell: ANCHOR_CELL, headCell: U32_OVERFLOW },
    ],
    [ 'a zero cell step', { type: 'goToTableCell', direction: 0, appendRow: true } ],
    [ 'a two cell step', { type: 'goToTableCell', direction: 2, appendRow: true } ],
    [ 'a missing append flag', { type: 'goToTableCell', direction: FORWARD } ],
    [ 'a caller-supplied origin', { type: 'mergeTableCells', origin: 'localCommand' } ],
    [
        'a caller-supplied origin on a selection',
        { type: 'selectTableCells', anchorCell: ANCHOR_CELL, headCell: HEAD_CELL, origin: 'remote' },
    ],
    [ 'a cell document handle', { type: 'clearTableCells', cellDocument: { editorId: '9' } } ],
];

function parsedRequests(mock: jest.Mock): Array<Record<string, unknown>> {
    return mock.mock.calls.map(call => JSON.parse(call[1] as string) as Record<string, unknown>);
}

function currentRevision(handle: NativeEditorDocumentHandle): string {
    return handle.bridge.getState().documentRevision;
}

function installTableEngine(handle: NativeEditorDocumentHandle, effects: Map<string, EngineEffect>) {
    const applyCommand = mockNativeModule.editorV2ApplyCommand;
    const fakeApplyCommand = applyCommand.getMockImplementation()!;

    applyCommand.mockImplementation((editorId: string, requestJson: string) => {
        const request = JSON.parse(requestJson) as Record<string, unknown>;
        const command = request.command as Record<string, unknown>;
        const effect = effects.get(String(command.type));

        if (effect === undefined) {
            return fakeApplyCommand(editorId, requestJson);
        }

        if (effect === 'document') {
            return fakeApplyCommand(
                editorId,
                JSON.stringify({
                    ...request,
                    command: {
                        type: 'insertContentJson',
                        json: {
                            type: 'doc',
                            content: [
                                {
                                    type: 'paragraph',
                                    content: [ { type: 'text', text: `[${String(command.type)}]` } ],
                                },
                            ],
                        },
                    },
                })
            );
        }

        return selectionOnlyOutcome(handle, request);
    });

    mockNativeModule.editorV2SetSelection.mockImplementation(
        (_editorId: string, requestJson: string) =>
            selectionOnlyOutcome(handle, JSON.parse(requestJson) as Record<string, unknown>)
    );
}

function selectionOnlyOutcome(
    handle: NativeEditorDocumentHandle,
    request: Record<string, unknown>
): Record<string, unknown> {
    const state = handle.bridge.getState();

    if (request.baseDocumentRevision !== state.documentRevision) {
        return operationError('REVISION_MISMATCH', 'base document revision does not match', {
            expectedRevision: request.baseDocumentRevision,
            actualRevision: state.documentRevision,
        });
    }

    return okRecord(
        JSON.stringify({
            type: 'transaction',
            changed: false,
            documentRevision: state.documentRevision,
            stateRevision: state.stateRevision,
            canUndo: state.canUndo,
            canRedo: state.canRedo,
        })
    );
}

async function runTableCommand(
    ref: React.RefObject<NativeRichTextEditorRef | null>,
    command: unknown
): Promise<unknown> {
    let thrown: unknown = null;

    await act(async() => {
        try {
            await ref.current!.runTableCommand(command as TableCommand);
        } catch (error) {
            thrown = error;
        }
    });

    return thrown;
}

function mutationRequestCount(): number {
    return (
        mockNativeModule.editorV2ApplyCommand.mock.calls.length +
        mockNativeModule.editorV2SetSelection.mock.calls.length
    );
}

describe('RichTextEditorRef.runTableCommand', () => {
    it('lowers every public discriminant to one engine command at the current revision', async() => {
        const handle = createV2LocalHandle(V2_INITIAL_DOC);
        const ref = createRef<NativeRichTextEditorRef>();
        const onLocalCommit = jest.fn();
        const onContentChangeJSON = jest.fn();

        render(
            <NativeRichTextEditor
                ref={ref}
                documentHandle={handle}
                onLocalCommit={onLocalCommit}
                onContentChangeJSON={onContentChangeJSON}
            />
        );

        installTableEngine(
            handle,
            new Map(COMMAND_CASES.map(({ wire, effect }) => [ String(wire.type), effect ]))
        );

        for (const { command, wire, effect } of COMMAND_CASES) {
            const commandCalls = mockNativeModule.editorV2ApplyCommand.mock.calls.length;
            const requestsBefore = mutationRequestCount();
            const commitsBefore = onLocalCommit.mock.calls.length;
            const base = currentRevision(handle);

            const thrown = await runTableCommand(ref, command);

            expect({ command, thrown }).toEqual({ command, thrown: null });
            expect({ command, requests: mutationRequestCount() - requestsBefore }).toEqual({
                command,
                requests: 1,
            });

            const request = parsedRequests(mockNativeModule.editorV2ApplyCommand)[commandCalls];
            expect({ command, wire: request.command, base: request.baseDocumentRevision }).toEqual({
                command,
                wire,
                base,
            });
            expect(Object.keys(request).sort()).toEqual([
                'baseDocumentRevision',
                'command',
                'requestId',
                'version',
            ]);

            const committed = onLocalCommit.mock.calls.length - commitsBefore;
            expect({ command, committed }).toEqual({
                command,
                committed: effect === 'document' ? 1 : 0,
            });

            if (effect === 'document') {
                expect(currentRevision(handle)).not.toBe(base);
                expect(JSON.stringify(onContentChangeJSON.mock.calls.at(-1)?.[0])).toContain(
                    `[${String(wire.type)}]`
                );
            } else {
                expect(currentRevision(handle)).toBe(base);
            }
        }

        handle.destroy();
    });

    it('selects an exact cell rectangle through the selection entrance, not a command', async() => {
        const handle = createV2LocalHandle(V2_INITIAL_DOC);
        const ref = createRef<NativeRichTextEditorRef>();
        const onLocalCommit = jest.fn();
        render(<NativeRichTextEditor ref={ref} documentHandle={handle} onLocalCommit={onLocalCommit} />);
        installTableEngine(handle, new Map());
        const base = currentRevision(handle);
        mockNativeModule.editorV2RenderUpdate.mockClear();

        const thrown = await runTableCommand(ref, {
            type: 'selectTableCells',
            anchorCell: HEAD_CELL,
            headCell: ANCHOR_CELL,
        });

        expect(thrown).toBeNull();
        expect(mockNativeModule.editorV2ApplyCommand).not.toHaveBeenCalled();
        const requests = parsedRequests(mockNativeModule.editorV2SetSelection);
        expect(requests).toHaveLength(1);
        expect(requests[0]!.baseDocumentRevision).toBe(base);
        expect(requests[0]!.selection).toEqual({
            type: 'cell',
            anchorCell: { offset: HEAD_CELL, kind: 'document' },
            headCell: { offset: ANCHOR_CELL, kind: 'document' },
        });
        expect(onLocalCommit).not.toHaveBeenCalled();
        expect(mockNativeModule.editorV2RenderUpdate).toHaveBeenCalled();
        handle.destroy();
    });

    it.each(INVALID_COMMANDS)(
        'rejects %s with the structured request error and sends nothing',
        async(_label, command) => {
            const handle = createV2LocalHandle(V2_INITIAL_DOC);
            const ref = createRef<NativeRichTextEditorRef>();
            render(<NativeRichTextEditor ref={ref} documentHandle={handle} />);

            const thrown = await runTableCommand(ref, command);

            expect(thrown).toBeInstanceOf(NativeEditorEngineBoundaryError);
            expect((thrown as NativeEditorEngineBoundaryError).code).toBe('CONFIG_INVALID');
            expect((thrown as NativeEditorEngineBoundaryError).domain).toBe('boundary');
            expect(mutationRequestCount()).toBe(0);
            handle.destroy();
        }
    );

    it('rejects a command the engine does not apply with COMMAND_NOT_APPLICABLE and publishes nothing', async() => {
        const handle = createV2LocalHandle(V2_INITIAL_DOC);
        const ref = createRef<NativeRichTextEditorRef>();
        const onLocalCommit = jest.fn();
        render(<NativeRichTextEditor ref={ref} documentHandle={handle} onLocalCommit={onLocalCommit} />);
        const base = currentRevision(handle);

        const thrown = await runTableCommand(ref, { type: 'mergeTableCells' });

        expect(thrown).toBeInstanceOf(NativeEditorOperationError);
        expect((thrown as NativeEditorOperationError).code).toBe('COMMAND_NOT_APPLICABLE');
        expect(mockNativeModule.editorV2ApplyCommand).toHaveBeenCalledTimes(1);
        expect(parsedRequests(mockNativeModule.editorV2ApplyCommand)[0]!.command).toEqual({
            type: 'mergeTableCells',
        });
        expect(onLocalCommit).not.toHaveBeenCalled();
        expect(currentRevision(handle)).toBe(base);
        handle.destroy();
    });

    it('rejects a stale revision, refreshes from the engine, and never retries', async() => {
        const handle = createV2LocalHandle(V2_INITIAL_DOC);
        const ref = createRef<NativeRichTextEditorRef>();
        const onLocalCommit = jest.fn();
        const onContentChangeJSON = jest.fn();
        render(
            <NativeRichTextEditor
                ref={ref}
                documentHandle={handle}
                onLocalCommit={onLocalCommit}
                onContentChangeJSON={onContentChangeJSON}
            />
        );
        installTableEngine(handle, new Map([ [ 'deleteTableRows', 'document' ] ]));
        const staleBase = currentRevision(handle);
        handle.bridge.replaceDocument({ setJson: V2_DOC_B, history: 'undoableBoundary' });

        const thrown = await runTableCommand(ref, { type: 'deleteTableRows' });

        expect(thrown).toBeInstanceOf(NativeEditorOperationError);
        expect((thrown as NativeEditorOperationError).code).toBe('REVISION_MISMATCH');
        expect((thrown as NativeEditorOperationError).details).toEqual({
            expectedRevision: staleBase,
            actualRevision: currentRevision(handle),
        });
        expect(mockNativeModule.editorV2ApplyCommand).toHaveBeenCalledTimes(1);
        expect(onLocalCommit).not.toHaveBeenCalled();
        expect(onContentChangeJSON).toHaveBeenLastCalledWith(V2_DOC_B);
        handle.destroy();
    });

    it('rejects a stale cell selection the same way', async() => {
        const handle = createV2LocalHandle(V2_INITIAL_DOC);
        const ref = createRef<NativeRichTextEditorRef>();
        render(<NativeRichTextEditor ref={ref} documentHandle={handle} />);
        installTableEngine(handle, new Map());
        handle.bridge.replaceDocument({ setJson: V2_DOC_B, history: 'undoableBoundary' });

        const thrown = await runTableCommand(ref, {
            type: 'selectTableCells',
            anchorCell: ANCHOR_CELL,
            headCell: HEAD_CELL,
        });

        expect((thrown as NativeEditorOperationError).code).toBe('REVISION_MISMATCH');
        expect(mockNativeModule.editorV2SetSelection).toHaveBeenCalledTimes(1);
        handle.destroy();
    });

    it('rejects a destroyed handle as non-retryable without a native request', async() => {
        const handle = createV2LocalHandle(V2_INITIAL_DOC);
        const ref = createRef<NativeRichTextEditorRef>();
        render(<NativeRichTextEditor ref={ref} documentHandle={handle} />);
        handle.destroy();

        const thrown = await runTableCommand(ref, { type: 'insertTable' });

        expect(thrown).toBeInstanceOf(NativeEditorNonRetryableError);
        expect((thrown as NativeEditorNonRetryableError).code).toBe('ENGINE_DESTROYED');
        expect(mutationRequestCount()).toBe(0);
    });

    it('rejects every table command in a read-only view without a native request', async() => {
        const handle = createV2LocalHandle(V2_INITIAL_DOC);
        const ref = createRef<NativeRichTextEditorRef>();
        render(<NativeRichTextEditor ref={ref} documentHandle={handle} editable={false} />);

        const commands: TableCommand[] = [
            ...COMMAND_CASES.map(({ command }) => command),
            { type: 'selectTableCells', anchorCell: ANCHOR_CELL, headCell: HEAD_CELL },
        ];

        for (const command of commands) {
            const thrown = await runTableCommand(ref, command);

            expect({ command, thrown }).toEqual({ command, thrown: expect.any(NativeEditorOperationError) });
            expect({ command, code: (thrown as NativeEditorOperationError).code }).toEqual({
                command,
                code: 'MUTATION_REJECTED',
            });
        }

        expect(mutationRequestCount()).toBe(0);
        handle.destroy();
    });

    it('propagates structured engine failures unchanged', async() => {
        const handle = createV2LocalHandle(V2_INITIAL_DOC);
        const ref = createRef<NativeRichTextEditorRef>();
        render(<NativeRichTextEditor ref={ref} documentHandle={handle} />);
        const refusal = {
            domain: 'boundary',
            code: 'CONFIG_INVALID',
            message: 'table dimension 5000 is outside 1..=1000',
            requestId: null,
            operationIndex: null,
            limit: null,
            actual: null,
            details: null,
        };
        mockNativeModule.editorV2ApplyCommand.mockImplementationOnce(() => ({
            value: null,
            error: refusal,
        }));

        const thrown = await runTableCommand(ref, { type: 'insertTable', rows: 5000 });

        expect(thrown).toBeInstanceOf(NativeEditorEngineBoundaryError);
        expect((thrown as NativeEditorEngineBoundaryError).message).toBe(refusal.message);
        expect(mockNativeModule.editorV2ApplyCommand).toHaveBeenCalledTimes(1);
        handle.destroy();
    });
});

describe('cell selections in selection events', () => {
    function renderWithSelectionListener() {
        const handle = createV2LocalHandle(V2_INITIAL_DOC);
        const onSelectionChange = jest.fn<void, [Selection]>();
        const view = render(
            <NativeRichTextEditor documentHandle={handle} onSelectionChange={onSelectionChange} />
        );

        const emit = (selection: unknown) =>
            act(() => {
                view.getByTestId('native-editor-view').props.onSelectionChange({
                    nativeEvent: {
                        anchor: SCALAR_ANCHOR,
                        head: SCALAR_HEAD,
                        editorId: handle.editorId,
                        stateJson: JSON.stringify({ selection }),
                    },
                });
            });

        return { handle, onSelectionChange, emit };
    }

    it('reports a native cell rectangle with its direction preserved', () => {
        const { handle, onSelectionChange, emit } = renderWithSelectionListener();

        emit({ type: 'cell', anchorCell: HEAD_CELL, headCell: ANCHOR_CELL });

        expect(onSelectionChange).toHaveBeenLastCalledWith({
            type: 'cell',
            anchorCell: HEAD_CELL,
            headCell: ANCHOR_CELL,
        });
        handle.destroy();
    });

    it('never reports a malformed cell rectangle as a cell selection', () => {
        const { handle, onSelectionChange, emit } = renderWithSelectionListener();

        for (const malformed of [
            { type: 'cell', anchorCell: -1, headCell: HEAD_CELL },
            { type: 'cell', anchorCell: ANCHOR_CELL, headCell: 1.5 },
            { type: 'cell', anchorCell: ANCHOR_CELL, headCell: U32_OVERFLOW },
            { type: 'cell', anchorCell: String(ANCHOR_CELL), headCell: HEAD_CELL },
        ]) {
            emit(malformed);

            expect({ malformed, reported: onSelectionChange.mock.calls.at(-1)?.[0] }).toEqual({
                malformed,
                reported: { type: 'text', anchor: SCALAR_ANCHOR, head: SCALAR_HEAD },
            });
        }

        handle.destroy();
    });

    it('keeps the text, node and all shapes unchanged', () => {
        const { handle, onSelectionChange, emit } = renderWithSelectionListener();

        emit({ type: 'text', anchor: 3, head: 7, anchorScalar: 2, headScalar: 6 });
        expect(onSelectionChange).toHaveBeenLastCalledWith({ type: 'text', anchor: 3, head: 7 });
        emit({ type: 'node', pos: 4, posScalar: 3 });
        expect(onSelectionChange).toHaveBeenLastCalledWith({ type: 'node', pos: 4 });
        emit({ type: 'all' });
        expect(onSelectionChange).toHaveBeenLastCalledWith({ type: 'all' });
        handle.destroy();
    });
});
