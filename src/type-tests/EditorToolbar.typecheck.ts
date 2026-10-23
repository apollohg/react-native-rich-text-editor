import type { EditorToolbarButtonStyle, EditorToolbarItem } from '../index';

const textButtonStyle: EditorToolbarButtonStyle = { width: 120, minWidth: 100 };

const textStyleMenu: EditorToolbarItem = {
    type: 'group',
    key: 'text-style',
    label: 'Text style',
    icon: { type: 'glyph', text: 'Normal' },
    presentation: 'menu',
    buttonStyle: textButtonStyle,
    items: [
        {
            type: 'heading',
            level: 1,
            label: 'Heading 1',
            icon: { type: 'glyph', text: 'Heading 1' },
            buttonStyle: { minWidth: 100 },
        },
    ],
};

void textStyleMenu;

// @ts-expect-error toolbar button dimensions use numeric points
const percentageWidth: EditorToolbarButtonStyle = { width: '100%' };
void percentageWidth;
