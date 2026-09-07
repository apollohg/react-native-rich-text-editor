import { forwardRef } from 'react';
import { type RichTextEditorRef, type RichTextEditorProps } from './RichTextEditorTypes';
import { useRichTextEditorState } from './useRichTextEditorState';
import { useRichTextEditorUpdates } from './useRichTextEditorUpdates';
import { useRichTextEditorCommands } from './useRichTextEditorCommands';
import { useRichTextEditorEvents } from './useRichTextEditorEvents';
import { useRichTextEditorMentions } from './useRichTextEditorMentions';
import { useRichTextEditorPresentation } from './useRichTextEditorPresentation';

export const RichTextEditor = forwardRef<RichTextEditorRef, RichTextEditorProps>(
    (props, ref) => {
        const state = useRichTextEditorState(props, ref);
        const updates = useRichTextEditorUpdates(state);
        const commands = useRichTextEditorCommands({ ...state, ...updates });
        const events = useRichTextEditorEvents({ ...state, ...updates, ...commands });

        const mentions = useRichTextEditorMentions({
            ...state,
            ...commands,
            ...updates,
            ...events,
        });

        return useRichTextEditorPresentation({ ...state, ...mentions, ...commands, ...events });
    }
);

/** @deprecated Use RichTextEditor instead. */
export const NativeRichTextEditor = RichTextEditor;
