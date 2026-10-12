import { invalidV2RequestError, isPlainRecord } from './NativeEditorResultNormalization';
import { type MarkdownExportOptions, type MarkdownFlavor } from './NativeEditorTypes';

const DEFAULT_MARKDOWN_FLAVOR: MarkdownFlavor = 'commonmark';

export function requireMarkdownFlavor(options?: MarkdownExportOptions): MarkdownFlavor {
    if (
        options !== undefined &&
        (!isPlainRecord(options) || Object.keys(options).some(key => key !== 'flavor'))
    ) {
        throw invalidV2RequestError('NativeEditorBridge: invalid Markdown export options');
    }

    const flavor = options?.flavor === undefined ? DEFAULT_MARKDOWN_FLAVOR : options.flavor;

    if (flavor !== 'commonmark' && flavor !== 'gfm') {
        throw invalidV2RequestError('NativeEditorBridge: Markdown flavor must be commonmark or gfm');
    }

    return flavor;
}
