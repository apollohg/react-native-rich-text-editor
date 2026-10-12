import './helpers/NativeEditorBridgeV2Fixture';
import { createHandle, mockNativeModule, okRecord, errRecord, mockV2Error, catchRejectedNativeRecord } from './helpers/NativeEditorBridgeV2Fixture';
import { NativeEditorEngineBoundaryError, NativeEditorDocumentError, NativeEditorNonRetryableError } from '../NativeEditorBoundaryError';

const MARKDOWN_METHOD = 'editorV2GetDocumentMarkdown';

describe('Markdown export boundary', () => {
    beforeEach(() => {
        mockNativeModule[MARKDOWN_METHOD] = jest.fn(() => okRecord(JSON.stringify({ markdown: '# Exported' })));
    });

    it('defaults to vanilla CommonMark and forwards explicit flavors', () => {
        const handle = createHandle();
        expect(handle.bridge.getDocumentMarkdown()).toBe('# Exported');
        expect(mockNativeModule[MARKDOWN_METHOD]).toHaveBeenLastCalledWith(handle.editorId, 'commonmark');
        expect(handle.bridge.getDocumentMarkdown({ flavor: 'gfm' })).toBe('# Exported');
        expect(mockNativeModule[MARKDOWN_METHOD]).toHaveBeenLastCalledWith(handle.editorId, 'gfm');
        handle.destroy();
    });

    it.each([ null,
        '',
        'markdown',
        1,
        { flavor: null },
        { flavor: 'invalid' } ])('rejects malformed options %p before invoking native', options => {
        const handle = createHandle();
        // @ts-expect-error Deliberately exercise the runtime boundary.
        expect(() => handle.bridge.getDocumentMarkdown(options)).toThrow(NativeEditorEngineBoundaryError);
        expect(mockNativeModule[MARKDOWN_METHOD]).not.toHaveBeenCalled();
        handle.destroy();
    });

    it.each([ null,
        {},
        { markdown: null },
        { markdown: 1 },
        { html: 'wrong format' } ])('rejects malformed native content %p', result => {
        const handle = createHandle();
        mockNativeModule[MARKDOWN_METHOD].mockReturnValue(okRecord(JSON.stringify(result)));
        expect(catchRejectedNativeRecord(() => handle.bridge.getDocumentMarkdown())).toBeInstanceOf(NativeEditorNonRetryableError);
        handle.destroy();
    });

    it('returns empty exports and preserves native typed errors', () => {
        const handle = createHandle();
        mockNativeModule[MARKDOWN_METHOD].mockReturnValue(okRecord(JSON.stringify({ markdown: '' })));
        expect(handle.bridge.getDocumentMarkdown()).toBe('');
        mockNativeModule[MARKDOWN_METHOD].mockReturnValue(errRecord(mockV2Error({ domain: 'document', code: 'DOCUMENT_LIMIT_EXCEEDED', limit: '100', actual: '200' })));
        expect(() => handle.bridge.getDocumentMarkdown()).toThrow(NativeEditorDocumentError);
        handle.destroy();
        expect(() => handle.bridge.getDocumentMarkdown()).toThrow();
        expect(mockNativeModule[MARKDOWN_METHOD]).toHaveBeenCalledTimes(2);
    });
});
