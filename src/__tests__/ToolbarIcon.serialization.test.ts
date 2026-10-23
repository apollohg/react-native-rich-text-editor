import './helpers/NativeRichTextEditorFixture';
import { Image } from 'react-native';
import { mapToolbarItemsForNative } from '../RichTextEditorSerialization';
import type { EditorToolbarItem } from '../EditorToolbar';

const state = {
    marks: {},
    markAttrs: {},
    nodes: {},
    commands: {},
    allowedMarks: [],
    insertableNodes: [],
};
const ASSET_ID = 17;
const SOURCE = { uri: 'file:///bundle/icon.png', width: 24, height: 24, scale: 2 };

it('resolves bundled image assets on groups and children before crossing the native bridge', () => {
    const resolve = jest.spyOn(Image, 'resolveAssetSource').mockReturnValue(SOURCE);
    const icon = { type: 'image' as const, source: ASSET_ID, tintable: false, fallbackText: 'I' };
    const items: EditorToolbarItem[] = [
        {
            type: 'group',
            key: 'images',
            label: 'Images',
            icon,
            items: [
                { type: 'link', label: 'Link', icon },
                { type: 'action', key: 'custom', label: 'Custom', icon },
            ],
        },
    ];
    const result = mapToolbarItemsForNative(items, state, true, jest.fn());
    const expectedIcon = { ...icon, source: SOURCE };
    expect(result[0]).toEqual(
        expect.objectContaining({
            icon: expectedIcon,
            items: [
                expect.objectContaining({ type: 'action', icon: expectedIcon }),
                expect.objectContaining({ type: 'action', icon: expectedIcon }),
            ],
        })
    );
    expect(items[0]).toEqual(expect.objectContaining({ icon }));
    resolve.mockRestore();
});
