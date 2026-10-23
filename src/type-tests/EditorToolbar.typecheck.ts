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

import type { EditorToolbarIcon, EditorToolbarIconRenderer, RichTextEditorProps } from '../index';

const imageIcon: EditorToolbarIcon = {
    type: 'image',
    source: { uri: 'file:///toolbar.png' },
    tintable: false,
    fallbackText: 'Image',
};
const renderToolbarIcon: EditorToolbarIconRenderer = ({
    icon,
    size,
    color,
    isActive,
    isDisabled,
    key,
}) => {
    void [icon, size, color, isActive, isDisabled, key];
    return undefined;
};
const inlineIcons: Pick<RichTextEditorProps, 'renderToolbarIcon'> = { renderToolbarIcon };
void [imageIcon, inlineIcons];
// @ts-expect-error URI sources must include a URI
const missingUri: EditorToolbarIcon = { type: 'image', source: { width: 24 } };
void missingUri;
