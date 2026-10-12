import {
    type MarkdownExportOptions,
    type MarkdownFlavor,
    type NativeEditorDocumentHandle,
    type RichTextEditorRef,
    type UseNativeEditorDocumentReturn,
} from '../index';

declare const editor: RichTextEditorRef;
declare const handle: NativeEditorDocumentHandle;
declare const document: UseNativeEditorDocumentReturn;

const flavor: MarkdownFlavor = 'gfm';
const options: MarkdownExportOptions = { flavor };
const content: string[] = [
    editor.getContentMarkdown(),
    editor.getContentMarkdown(options),
    handle.bridge.getDocumentMarkdown({ flavor: 'commonmark' }),
    document.getContentMarkdown({}),
];
void content;

// @ts-expect-error only CommonMark and GFM are supported
editor.getContentMarkdown({ flavor: 'html' });
// @ts-expect-error output options are an object
handle.bridge.getDocumentMarkdown('gfm');
// @ts-expect-error explicitly null is not a flavor
const invalid: MarkdownExportOptions = { flavor: null };
void invalid;
