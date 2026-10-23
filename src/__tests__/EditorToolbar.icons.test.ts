import './helpers/NativeRichTextEditorFixture';
import glyphMap from '../../android/src/main/assets/editor-icons/MaterialDesignIcons.json';
import { buildToolbarItems } from '../../example/content';
import type { EditorToolbarItem } from '../EditorToolbarTypes';
import { DEFAULT_MATERIAL_DESIGN_ICONS, resolveMaterialIconName } from '../EditorToolbarVisuals';

function collectIconNames(items: readonly EditorToolbarItem[]): string[] {
    return items.flatMap(item => {
        const name = 'icon' in item && item.icon ? resolveMaterialIconName(item.icon) : undefined;
        const names = name ? [name] : [];
        return item.type === 'group' ? [...names, ...collectIconNames(item.items)] : names;
    });
}

describe('toolbar icon assets', () => {
    const exampleIconNames = collectIconNames(buildToolbarItems({
        taskListActive: false,
        taskListAvailable: true,
    }));

    it.each([...new Set([...Object.values(DEFAULT_MATERIAL_DESIGN_ICONS), ...exampleIconNames])])(
        'resolves toolbar icon %s in the bundled font map',
        name => {
            expect((glyphMap as Record<string, number>)[name]).toEqual(expect.any(Number));
        }
    );

});
