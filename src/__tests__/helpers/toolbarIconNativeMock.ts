import glyphMap from '../../../android/src/main/assets/editor-icons/MaterialDesignIcons.json';

export function getToolbarIcon(name: string) {
    const codePoint = (glyphMap as Record<string, number>)[name];
    return codePoint == null
        ? null
        : { glyph: String.fromCodePoint(codePoint), fontFamily: 'MaterialDesignIcons' };
}
