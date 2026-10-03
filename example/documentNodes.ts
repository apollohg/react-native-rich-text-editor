import type { DocumentJSON } from '@apollohg/react-native-rich-text-editor';

export type MarkJSON = { type: string; attrs?: Record<string, unknown> };

export const BOLD: MarkJSON = { type: 'bold' };
export const ITALIC: MarkJSON = { type: 'italic' };
export const UNDERLINE: MarkJSON = { type: 'underline' };
export const STRIKE: MarkJSON = { type: 'strike' };

export function link(href: string): MarkJSON {
    return { type: 'link', attrs: { href } };
}

export function text(value: string, ...marks: readonly MarkJSON[]): DocumentJSON {
    return marks.length === 0
        ? { type: 'text', text: value }
        : { type: 'text', text: value, marks };
}

export function paragraph(...content: readonly DocumentJSON[]): DocumentJSON {
    return content.length === 0 ? { type: 'paragraph' } : { type: 'paragraph', content };
}

export function heading(level: number, title: string): DocumentJSON {
    return { type: 'heading', attrs: { level }, content: [ text(title) ] };
}

export function listItem(...content: readonly DocumentJSON[]): DocumentJSON {
    return { type: 'list_item', content };
}

export function bulletList(...items: readonly DocumentJSON[]): DocumentJSON {
    return { type: 'bullet_list', content: items };
}

export function orderedList(...items: readonly DocumentJSON[]): DocumentJSON {
    return { type: 'ordered_list', content: items };
}

export function blockquote(...content: readonly DocumentJSON[]): DocumentJSON {
    return { type: 'blockquote', content };
}

export function codeBlock(language: string, source: string): DocumentJSON {
    return { type: 'codeBlock', attrs: { language }, content: [ text(source) ] };
}

export function image(src: string, alt: string): DocumentJSON {
    return { type: 'image', attrs: { src, alt } };
}

export function documentOf(...content: readonly DocumentJSON[]): DocumentJSON {
    return { type: 'doc', content };
}
