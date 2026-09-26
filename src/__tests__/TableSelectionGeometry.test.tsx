import './helpers/NativeRichTextEditorFixture';
import {
    mockNativeModule,
    V2_INITIAL_DOC,
    V2_DOC_B,
    createV2LocalHandle,
} from './helpers/NativeRichTextEditorFixture';
import { render, act } from '@testing-library/react-native';
import { NativeRichTextEditor } from '../NativeRichTextEditor';
import * as EditorToolbarRegistry from '../EditorToolbarRegistry';
import { type NativeEditorDocumentHandle } from '../NativeEditorBridge';
import { type TableSelectionGeometry } from '../TableTypes';

const DOCUMENT_REVISION = '12';
const LAYOUT_EPOCH = '4';
const TABLE_POS = 7;
const U32_OVERFLOW = 0x1_0000_0000;
const SELECTED_RECT = { x: 24, y: 180, width: 120, height: 44 };
const SCROLLED_RECT = { x: -36, y: 180, width: 120, height: 44 };
const VIEWPORT = { x: 0, y: 96, width: 390, height: 600 };

type GeometryListener = jest.Mock<void, [TableSelectionGeometry | null]>;

function nativeGeometry(editorId: string, overrides: Record<string, unknown> = {}) {
    return {
        editorId,
        documentRevision: DOCUMENT_REVISION,
        layoutEpoch: LAYOUT_EPOCH,
        tablePos: TABLE_POS,
        coordinateSpace: 'window',
        rects: [ SELECTED_RECT ],
        viewport: VIEWPORT,
        ...overrides,
    };
}

function totalNativeModuleCalls(): number {
    return Object.values(mockNativeModule).reduce((sum, mock) => sum + mock.mock.calls.length, 0);
}

function renderEditor(handle: NativeEditorDocumentHandle, listener: GeometryListener) {
    const view = render(
        <NativeRichTextEditor documentHandle={handle} onTableSelectionGeometryChange={listener} />
    );
    const nativeView = () => view.getByTestId('native-editor-view');
    const emit = (nativeEvent: unknown) =>
        act(() => {
            nativeView().props.onTableSelectionGeometry({ nativeEvent });
        });
    const focus = () =>
        act(() => {
            nativeView().props.onFocusChange({
                nativeEvent: { isFocused: true, editorId: handle.editorId },
            });
        });

    return { view, nativeView, emit, focus };
}

describe('native table selection geometry events', () => {
    let consoleError: jest.SpyInstance;

    beforeEach(() => {
        consoleError = jest.spyOn(console, 'error').mockImplementation(() => undefined);
    });

    afterEach(() => {
        consoleError.mockRestore();
    });

    it('delivers window geometry stamped with the mounted toolbar owner, not the document id', () => {
        const handle = createV2LocalHandle(V2_INITIAL_DOC);
        const listener: GeometryListener = jest.fn();
        const { emit, focus } = renderEditor(handle, listener);
        focus();
        const toolbarOwner = EditorToolbarRegistry.activeEditorToolbarFrameOwnerId;

        emit(nativeGeometry(handle.editorId));

        expect(typeof toolbarOwner).toBe('number');
        expect(listener.mock.calls).toEqual([ [ {
            editorId: handle.editorId,
            ownerId: toolbarOwner,
            documentRevision: DOCUMENT_REVISION,
            layoutEpoch: LAYOUT_EPOCH,
            tablePos: TABLE_POS,
            coordinateSpace: 'window',
            rects: [ SELECTED_RECT ],
            viewport: VIEWPORT,
        } ] ]);
        handle.destroy();
    });

    it('gives two views of one document the same editorId but distinct owners', () => {
        const handle = createV2LocalHandle(V2_INITIAL_DOC);
        const first: GeometryListener = jest.fn();
        const second: GeometryListener = jest.fn();
        const firstEditor = renderEditor(handle, first);
        const secondEditor = renderEditor(handle, second);

        firstEditor.emit(nativeGeometry(handle.editorId));
        secondEditor.emit(nativeGeometry(handle.editorId));

        const firstGeometry = first.mock.calls[0]?.[0];
        const secondGeometry = second.mock.calls[0]?.[0];
        expect({
            editorIds: [ firstGeometry?.editorId, secondGeometry?.editorId ],
            ownersDiffer: firstGeometry?.ownerId !== secondGeometry?.ownerId,
        }).toEqual({ editorIds: [ handle.editorId, handle.editorId ], ownersDiffer: true });
        handle.destroy();
    });

    it('lets native report each scroll position, with rects outside the viewport kept as-is', () => {
        const handle = createV2LocalHandle(V2_INITIAL_DOC);
        const listener: GeometryListener = jest.fn();
        const { emit } = renderEditor(handle, listener);

        emit(nativeGeometry(handle.editorId));
        emit(nativeGeometry(handle.editorId, { rects: [ SCROLLED_RECT ] }));
        emit(nativeGeometry(handle.editorId, { rects: [] }));

        expect(listener.mock.calls.map(([ geometry ]) => geometry?.rects)).toEqual([
            [ SELECTED_RECT ],
            [ SCROLLED_RECT ],
            [],
        ]);
        handle.destroy();
    });

    it.each([
        [ 'a numeric editorId', { editorId: 1 } ],
        [ 'a non-canonical editorId', { editorId: '01' } ],
        [ 'a native-supplied ownerId', { ownerId: 3 } ],
        [ 'a screen coordinate space', { coordinateSpace: 'screen' } ],
        [ 'a numeric documentRevision', { documentRevision: 12 } ],
        [ 'a fractional layoutEpoch', { layoutEpoch: '4.0' } ],
        [ 'a negative tablePos', { tablePos: -1 } ],
        [ 'a fractional tablePos', { tablePos: 1.5 } ],
        [ 'an overflowing tablePos', { tablePos: U32_OVERFLOW } ],
        [ 'rects that are not an array', { rects: SELECTED_RECT } ],
        [ 'a rect with an extra field', { rects: [ { ...SELECTED_RECT, radius: 2 } ] } ],
        [ 'a rect with a negative width', { rects: [ { ...SELECTED_RECT, width: -1 } ] } ],
        [ 'a rect with a NaN origin', { rects: [ { ...SELECTED_RECT, x: Number.NaN } ] } ],
        [ 'an infinite viewport', { viewport: { ...VIEWPORT, height: Number.POSITIVE_INFINITY } } ],
        [ 'a viewport missing its origin', { viewport: { width: 390, height: 600 } } ],
    ])('rejects %s without delivering anything', (_name, overrides) => {
        const handle = createV2LocalHandle(V2_INITIAL_DOC);
        const listener: GeometryListener = jest.fn();
        const { emit } = renderEditor(handle, listener);
        const payload = { ...nativeGeometry(handle.editorId), ...overrides };

        emit(payload);

        expect({
            delivered: listener.mock.calls,
            reported: consoleError.mock.calls.map(([ message ]) => message),
        }).toEqual({
            delivered: [],
            reported: [ 'NativeEditorBridge: native table selection geometry was rejected' ],
        });
        handle.destroy();
    });

    it('rejects a payload missing a required field', () => {
        const handle = createV2LocalHandle(V2_INITIAL_DOC);
        const listener: GeometryListener = jest.fn();
        const { emit } = renderEditor(handle, listener);
        const { layoutEpoch: _omitted, ...withoutEpoch } = nativeGeometry(handle.editorId);

        emit(withoutEpoch);

        expect(listener).not.toHaveBeenCalled();
        expect(consoleError).toHaveBeenCalledTimes(1);
        handle.destroy();
    });

    it('delivers null when native clears geometry it previously delivered', () => {
        const handle = createV2LocalHandle(V2_INITIAL_DOC);
        const listener: GeometryListener = jest.fn();
        const { emit } = renderEditor(handle, listener);

        emit({ editorId: handle.editorId });
        emit(nativeGeometry(handle.editorId));
        emit({ editorId: handle.editorId });
        emit({ editorId: handle.editorId });

        expect(listener.mock.calls.map(([ geometry ]) => geometry?.tablePos ?? null)).toEqual([
            TABLE_POS,
            null,
        ]);
        handle.destroy();
    });

    it('ignores geometry from another editor and a clear for geometry it never delivered', () => {
        const handle = createV2LocalHandle(V2_INITIAL_DOC);
        const other = createV2LocalHandle(V2_DOC_B);
        const listener: GeometryListener = jest.fn();
        const { emit } = renderEditor(handle, listener);

        emit(nativeGeometry(other.editorId));
        emit(nativeGeometry(handle.editorId));
        emit({ editorId: other.editorId });

        expect(listener.mock.calls.map(([ geometry ]) => geometry?.editorId ?? null)).toEqual([
            handle.editorId,
        ]);
        handle.destroy();
        other.destroy();
    });

    it('still clears geometry of the previous binding after the view rebinds', () => {
        const first = createV2LocalHandle(V2_INITIAL_DOC);
        const second = createV2LocalHandle(V2_DOC_B);
        const listener: GeometryListener = jest.fn();
        const { view, emit } = renderEditor(first, listener);

        emit(nativeGeometry(first.editorId));
        view.rerender(
            <NativeRichTextEditor documentHandle={second} onTableSelectionGeometryChange={listener} />
        );
        emit(nativeGeometry(first.editorId, { tablePos: TABLE_POS + 1 }));
        emit({ editorId: first.editorId });

        expect(listener.mock.calls.map(([ geometry ]) => geometry?.editorId ?? null)).toEqual([
            first.editorId,
            null,
        ]);
        first.destroy();
        second.destroy();
    });

    it('clears after the document is destroyed but accepts no new geometry', () => {
        const handle = createV2LocalHandle(V2_INITIAL_DOC);
        const listener: GeometryListener = jest.fn();
        const { emit } = renderEditor(handle, listener);

        emit(nativeGeometry(handle.editorId));
        handle.destroy();
        emit(nativeGeometry(handle.editorId, { tablePos: TABLE_POS + 1 }));
        emit({ editorId: handle.editorId });

        expect(listener.mock.calls.map(([ geometry ]) => geometry?.tablePos ?? null)).toEqual([
            TABLE_POS,
            null,
        ]);
    });

    it('never creates or changes an engine selection', () => {
        const handle = createV2LocalHandle(V2_INITIAL_DOC);
        const listener: GeometryListener = jest.fn();
        const onSelectionChange = jest.fn();
        const onActiveStateChange = jest.fn();
        const view = render(
            <NativeRichTextEditor
                documentHandle={handle}
                onTableSelectionGeometryChange={listener}
                onSelectionChange={onSelectionChange}
                onActiveStateChange={onActiveStateChange}
            />
        );
        const nativeView = () => view.getByTestId('native-editor-view');
        const pushedBefore = {
            json: nativeView().props.editorUpdateJson,
            revision: nativeView().props.editorUpdateRevision,
        };
        onActiveStateChange.mockClear();
        const callsBefore = totalNativeModuleCalls();

        act(() => {
            nativeView().props.onTableSelectionGeometry({
                nativeEvent: nativeGeometry(handle.editorId),
            });
            nativeView().props.onTableSelectionGeometry({
                nativeEvent: { editorId: handle.editorId },
            });
        });

        expect({
            delivered: listener.mock.calls.length,
            nativeModuleCalls: totalNativeModuleCalls() - callsBefore,
            selectionEvents: onSelectionChange.mock.calls,
            activeStateEvents: onActiveStateChange.mock.calls,
            pushed: {
                json: nativeView().props.editorUpdateJson,
                revision: nativeView().props.editorUpdateRevision,
            },
        }).toEqual({
            delivered: 2,
            nativeModuleCalls: 0,
            selectionEvents: [],
            activeStateEvents: [],
            pushed: pushedBefore,
        });
        handle.destroy();
    });
});
