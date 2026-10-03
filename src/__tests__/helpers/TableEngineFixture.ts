import { mockNativeModule } from './NativeRichTextEditorFixture';
import { okRecord, operationError } from './nativeEditorV2FakeRecords';
import { type NativeEditorDocumentHandle } from '../../NativeEditorBridge';

export type EngineEffect = 'document' | 'selection';

export function installTableEngine(handle: NativeEditorDocumentHandle, effects: Map<string, EngineEffect>) {
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
