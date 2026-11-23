import { invalidV2RequestError, isPlainRecord } from './NativeEditorResultNormalization';
import { type TextContentOptions } from './NativeEditorTypes';

export function requireBlockSeparator(options?: TextContentOptions): string | null {
    if (
        options !== undefined &&
        (!isPlainRecord(options) || Object.keys(options).some(key => key !== 'blockSeparator'))
    ) {
        throw invalidV2RequestError('NativeEditorBridge: invalid text content options');
    }

    const blockSeparator = options?.blockSeparator;

    if (blockSeparator === undefined) {
        return null;
    }

    if (typeof blockSeparator !== 'string') {
        throw invalidV2RequestError('NativeEditorBridge: blockSeparator must be a string');
    }

    return blockSeparator;
}
