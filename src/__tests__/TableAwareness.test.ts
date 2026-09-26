import './helpers/YjsCollaborationFixture';
import {
    TRANSPORT_URL,
    ALICE,
    remotePeer,
    runtime,
    createRoomHandle,
    setupController,
    synchronize,
    awarenessPayload,
} from './helpers/YjsCollaborationFixture';
import { V2_FAKE_AWARENESS_FRAME } from './helpers/nativeEditorV2Fake';
import { act, renderHook } from '@testing-library/react-native';
import { useYjsCollaboration } from '../YjsCollaboration';
import {
    createNativeEditorLocalAwarenessCellSelection,
    type NativeEditorLocalAwarenessIntent,
} from '../NativeEditorBridge';
import { serializeRemoteSelections } from '../RichTextEditorSerialization';

const ANCHOR_CELL = 2;
const HEAD_CELL = 6;
const U32_OVERFLOW = 0x1_0000_0000;
const INVALID_INTENT = 'invalid local awareness intent';

function nativeAwarenessCalls(): unknown[] {
    return runtime.module.editorV2CollaborationSetAwareness.mock.calls.map(call =>
        JSON.parse(call[1] as string));
}

describe('table cell awareness', () => {
    it('publishes a factory cell selection as the tagged cell wire selection', () => {
        const handle = createRoomHandle({ withSnapshot: true });
        const selection = createNativeEditorLocalAwarenessCellSelection(ANCHOR_CELL, HEAD_CELL);

        expect(selection).toEqual({ anchorCell: ANCHOR_CELL, headCell: HEAD_CELL });
        expect(Object.isFrozen(selection)).toBe(true);
        expect(() => Object.assign(selection, { anchorCell: HEAD_CELL })).toThrow();

        handle.setLocalAwareness({ state: { user: ALICE }, focused: true, selection });

        expect(nativeAwarenessCalls()).toEqual([
            {
                state: { user: ALICE },
                focused: true,
                selection: { type: 'cell', anchorCell: ANCHOR_CELL, headCell: HEAD_CELL },
            },
        ]);
    });

    it('rejects literal, tagged, cloned, and proxied cell selections before native invocation', () => {
        const handle = createRoomHandle({ withSnapshot: true });
        const factorySelection = createNativeEditorLocalAwarenessCellSelection(
            ANCHOR_CELL,
            HEAD_CELL
        );
        let accessorRead = false;
        const accessorProxy = new Proxy(factorySelection, {
            get: () => {
                accessorRead = true;
                throw new Error('selection accessor must not be read');
            },
        });
        const forgedSelections: unknown[] = [
            { anchorCell: ANCHOR_CELL, headCell: HEAD_CELL },
            { type: 'cell', anchorCell: ANCHOR_CELL, headCell: HEAD_CELL },
            { ...factorySelection },
            new Proxy(factorySelection, {}),
            accessorProxy,
        ];

        for (const selection of forgedSelections) {
            expect(() =>
                handle.setLocalAwareness({
                    state: { user: ALICE },
                    focused: true,
                    selection,
                } as unknown as NativeEditorLocalAwarenessIntent)).toThrow(INVALID_INTENT);
        }

        expect(accessorRead).toBe(false);
        expect(runtime.module.editorV2CollaborationSetAwareness).not.toHaveBeenCalled();
    });

    it('rejects non-u32 cell openings at construction', () => {
        const invalidOpenings = [ -1, 1.5, Number.NaN, Number.POSITIVE_INFINITY, U32_OVERFLOW ];

        for (const opening of invalidOpenings) {
            expect(() => createNativeEditorLocalAwarenessCellSelection(opening, HEAD_CELL)).toThrow(
                INVALID_INTENT
            );
            expect(() =>
                createNativeEditorLocalAwarenessCellSelection(ANCHOR_CELL, opening)).toThrow(
                INVALID_INTENT
            );
        }
    });

    it('publishes a controller cell selection change as cell presence', () => {
        const setup = setupController({
            handle: createRoomHandle({ withSnapshot: true }),
            localAwareness: ALICE,
        });

        setup.controller.handleSelectionChange({
            type: 'cell',
            anchorCell: ANCHOR_CELL,
            headCell: HEAD_CELL,
        });

        expect(setup.errors).toEqual([]);
        expect(awarenessPayload()).toEqual({
            state: { user: ALICE },
            focused: false,
            selection: { type: 'cell', anchorCell: ANCHOR_CELL, headCell: HEAD_CELL },
        });
        expect(runtime.session(setup.handle.editorId).localAwarenessCursor).toEqual({
            anchor: ANCHOR_CELL,
            head: HEAD_CELL,
        });
    });

    it('hands a resolved remote rectangle to native beside the cursor fallback', () => {
        const handle = createRoomHandle({ withSnapshot: true });
        const { result } = renderHook(() =>
            useYjsCollaboration({
                documentId: 'doc-1',
                handle,
                transport: { url: TRANSPORT_URL, connect: true },
                localAwareness: ALICE,
            }));

        act(() => {
            synchronize(handle);
        });
        act(() => {
            runtime.pushRemotePeers(handle.editorId, [
                remotePeer({
                    cursor: { anchor: ANCHOR_CELL, head: HEAD_CELL },
                    cellRectangle: { anchorCell: ANCHOR_CELL, headCell: HEAD_CELL },
                }),
                remotePeer({
                    clientId: '43',
                    cursor: { anchor: 4, head: 4 },
                    cellRectangle: null,
                }),
            ]);
            runtime.transportReceive(handle.editorId, V2_FAKE_AWARENESS_FRAME);
        });

        const [ rectanglePeer, cursorPeer ] = result.current.editorBindings.remoteSelections;

        expect(rectanglePeer).toMatchObject({
            clientId: '42',
            anchor: ANCHOR_CELL,
            head: HEAD_CELL,
            cellRectangle: { anchorCell: ANCHOR_CELL, headCell: HEAD_CELL },
        });
        expect(cursorPeer).toMatchObject({ clientId: '43', anchor: 4, head: 4 });
        expect(cursorPeer).not.toHaveProperty('cellRectangle');

        const serialized = JSON.parse(
            serializeRemoteSelections(result.current.editorBindings.remoteSelections) ?? '[]'
        );

        expect(serialized[0].cellRectangle).toEqual({
            anchorCell: ANCHOR_CELL,
            headCell: HEAD_CELL,
        });
        expect(serialized[1]).not.toHaveProperty('cellRectangle');
    });

    it('refuses a non-u32 remote rectangle before it reaches the native prop', () => {
        expect(() =>
            serializeRemoteSelections([
                {
                    clientId: '42',
                    anchor: ANCHOR_CELL,
                    head: HEAD_CELL,
                    color: ALICE.color,
                    cellRectangle: { anchorCell: -1, headCell: HEAD_CELL },
                },
            ])).toThrow('invalid u32 remote selection anchor cell');
    });
});
