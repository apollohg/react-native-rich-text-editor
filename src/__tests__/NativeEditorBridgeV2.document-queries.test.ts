import './helpers/NativeEditorBridgeV2Fixture';
import {
    createHandle,
    mockNativeModule,
    okRecord,
    errRecord,
    mockV2Error,
    catchRejectedNativeRecord
} from './helpers/NativeEditorBridgeV2Fixture';
import {
    NativeEditorDocumentError,
    NativeEditorNonRetryableError
} from '../NativeEditorBoundaryError';

const TEXT_METHOD = 'editorV2GetDocumentText';
const EMPTY_METHOD = 'editorV2GetDocumentIsEmpty';

beforeEach(() => {
    mockNativeModule[TEXT_METHOD] = jest.fn(() =>
        okRecord(JSON.stringify({ text: '' }))
    );
    mockNativeModule[EMPTY_METHOD] = jest.fn(() =>
        okRecord(JSON.stringify({ isEmpty: false }))
    );
});

it.each([null, {}, { text: null }, { text: 1 }, { html: 'wrong format' }])(
    'rejects malformed text result %p',
    (result) => {
        const handle = createHandle();
        mockNativeModule[TEXT_METHOD].mockReturnValue(
            okRecord(JSON.stringify(result))
        );
        expect(
            catchRejectedNativeRecord(() => handle.bridge.getDocumentText())
        ).toBeInstanceOf(NativeEditorNonRetryableError);
        handle.destroy();
    }
);

it.each([null, {}, { isEmpty: null }, { isEmpty: 'false' }, { isEmpty: 0 }])(
    'rejects malformed emptiness result %p',
    (result) => {
        const handle = createHandle();
        mockNativeModule[EMPTY_METHOD].mockReturnValue(
            okRecord(JSON.stringify(result))
        );
        expect(
            catchRejectedNativeRecord(() => handle.bridge.getDocumentIsEmpty())
        ).toBeInstanceOf(NativeEditorNonRetryableError);
        handle.destroy();
    }
);

it('accepts empty text and false emptiness, forwards the handle, and preserves typed errors', () => {
    const handle = createHandle();
    expect(handle.bridge.getDocumentText()).toBe('');
    expect(handle.bridge.getDocumentIsEmpty()).toBe(false);
    expect(mockNativeModule[TEXT_METHOD]).toHaveBeenLastCalledWith(
        handle.editorId
    );
    expect(mockNativeModule[EMPTY_METHOD]).toHaveBeenLastCalledWith(
        handle.editorId
    );
    mockNativeModule[TEXT_METHOD].mockReturnValue(
        errRecord(
            mockV2Error({
                domain: 'document',
                code: 'DOCUMENT_LIMIT_EXCEEDED',
                limit: '100',
                actual: '200'
            })
        )
    );
    expect(() => handle.bridge.getDocumentText()).toThrow(
        NativeEditorDocumentError
    );
    handle.destroy();
    expect(() => handle.bridge.getDocumentText()).toThrow();
    expect(() => handle.bridge.getDocumentIsEmpty()).toThrow();
    expect(mockNativeModule[TEXT_METHOD]).toHaveBeenCalledTimes(2);
    expect(mockNativeModule[EMPTY_METHOD]).toHaveBeenCalledTimes(1);
});
