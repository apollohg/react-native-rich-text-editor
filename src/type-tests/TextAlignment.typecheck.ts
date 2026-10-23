import type { EditorToolbarItem, RichTextEditorRef, TextAlign } from '../index';
import type { NativeEditorSelectionEnvelope } from '../NativeEditorTypes';

declare const editor: RichTextEditorRef;
declare const alignment: TextAlign;
editor.setTextAlign(alignment);
editor.unsetTextAlign();
// @ts-expect-error Justified alignment is not supported.
editor.setTextAlign('justify');
// @ts-expect-error Reset uses unsetTextAlign.
editor.setTextAlign(null);

const resetItem: EditorToolbarItem = {
    type: 'textAlign',
    align: null,
    label: 'Reset Alignment',
    icon: { type: 'default', id: 'alignReset' },
};
void resetItem;

const paragraphSelection: NativeEditorSelectionEnvelope = {
    type: 'node',
    at: { kind: 'document', offset: 0 },
};
const invalidTextSelection: NativeEditorSelectionEnvelope = {
    type: 'text',
    // @ts-expect-error Document openings are only valid for structural selections.
    anchor: { kind: 'document', offset: 0 },
    head: { kind: 'scalar', offset: 1 },
};
void paragraphSelection;
void invalidTextSelection;
