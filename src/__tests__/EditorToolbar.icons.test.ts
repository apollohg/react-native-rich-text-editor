import './helpers/NativeRichTextEditorFixture';
import { readFileSync } from 'node:fs';
import { join } from 'node:path';
import glyphMap from '@react-native-vector-icons/material-design-icons/glyphmaps/MaterialDesignIcons.json';
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
        'resolves toolbar icon %s in the installed font',
        name => {
            expect((glyphMap as Record<string, number>)[name]).toEqual(expect.any(Number));
        }
    );

    it.each([
        ['fonts', 'MaterialDesignIcons.ttf'],
        ['glyphmaps', 'MaterialDesignIcons.json'],
    ])('ships the same %s/%s for native and React toolbars', (directory, filename) => {
        const installed = require.resolve(`@react-native-vector-icons/material-design-icons/${directory}/${filename}`);
        const bundled = join(__dirname, '../../android/src/main/assets/editor-icons', filename);
        expect(readFileSync(bundled).equals(readFileSync(installed))).toBe(true);
    });
});
