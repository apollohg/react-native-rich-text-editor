import type { NativeRichTextEditorRef, RichTextEditorRef } from '../index';

declare const editor: RichTextEditorRef;
declare const nativeEditor: NativeRichTextEditorRef;

const results: void[] = [
    editor.setMark('textStyle', { color: '#e53935' }),
    editor.setMark('textStyle', { color: null }),
    editor.unsetMark('textStyle'),
    nativeEditor.setMark('link', { href: 'https://example.com' }),
    nativeEditor.unsetMark('link'),
];
void results;
