import type { MentionSuggestion } from './addons';
import { useMemo, useState } from 'react';
import { Image, StyleSheet, Text, View } from 'react-native';
import { getNativeModule } from './NativeEditorNativeModule';
import {
    type EditorToolbarDefaultIconId,
    type EditorToolbarIcon,
    type EditorToolbarImageIcon,
    type EditorToolbarIconRenderer,
    type ToolbarButton,
} from './EditorToolbarTypes';
import { type EditorToolbarTheme } from './EditorTheme';
import { BUTTON_HIT } from './EditorToolbarRegistry';

export const BUTTON_VISIBLE = 32;

export const TOOLBAR_PADDING_H = 12;

export const TOOLBAR_PADDING_V = 4;

export const MAX_BUTTON_SIZE = 40;

export const BUTTON_HEIGHT_INSET = 4;

export const MENU_MARGIN = 8;

export const MENU_WIDTH = 192;

export const KEYBOARD_FRAME_REMEASURE_DELAYS_MS = [ 50, 150, 300 ] as const;

export const ACTIVE_BG = 'rgba(0, 122, 255, 0.12)';

export const ACTIVE_COLOR = '#007AFF';

export const DEFAULT_COLOR = '#666666';

export const DISABLED_COLOR = '#C7C7CC';

export const SEPARATOR_COLOR = '#E5E5EA';

export const TOOLBAR_BG = '#FFFFFF';

export const TOOLBAR_BORDER = '#E5E5EA';

export const TOOLBAR_RADIUS = 0;

export const BUTTON_RADIUS = 6;

export const MENU_BORDER = '#D1D1D6';

export const MENU_SHADOW = '#000000';

export const DEFAULT_GLYPH_ICONS: Record<EditorToolbarDefaultIconId, string> = {
    alignLeft: '⇤',
    alignCenter: '↔',
    alignRight: '⇥',
    alignReset: '↺',
    bold: 'B',
    italic: 'I',
    underline: 'U',
    strike: 'S',
    link: '🔗',
    image: '🖼',
    blockquote: '❝',
    h1: 'H1',
    h2: 'H2',
    h3: 'H3',
    h4: 'H4',
    h5: 'H5',
    h6: 'H6',
    bulletList: '•≡',
    orderedList: '1.',
    indentList: '→',
    outdentList: '←',
    lineBreak: '↵',
    horizontalRule: ':',
    undo: '↩',
    redo: '↪',
};

export const DEFAULT_MATERIAL_DESIGN_ICONS: Partial<Record<EditorToolbarDefaultIconId, string>> = {
    alignLeft: 'format-align-left',
    alignCenter: 'format-align-center',
    alignRight: 'format-align-right',
    alignReset: 'format-clear',
    bold: 'format-bold',
    italic: 'format-italic',
    underline: 'format-underline',
    strike: 'format-strikethrough-variant',
    link: 'link',
    image: 'image',
    h1: 'format-title',
    h2: 'format-title',
    h3: 'format-title',
    h4: 'format-title',
    h5: 'format-title',
    h6: 'format-title',
    blockquote: 'format-quote-close',
    bulletList: 'format-list-bulleted',
    orderedList: 'format-list-numbered',
    indentList: 'format-indent-increase',
    outdentList: 'format-indent-decrease',
    lineBreak: 'keyboard-return',
    horizontalRule: 'minus',
    undo: 'undo',
    redo: 'redo',
};

export interface ToolbarMetrics {
    toolbarHeight: number;
    buttonHeight: number;
    paddingVertical: number;
}

export function resolveToolbarMetrics(theme: EditorToolbarTheme | undefined): ToolbarMetrics {
    const toolbarHeight = Math.max(theme?.height ?? BUTTON_VISIBLE + TOOLBAR_PADDING_V * 2, 1);

    const buttonHeight =
        theme?.height == null
            ? BUTTON_VISIBLE
            : Math.max(1, Math.min(MAX_BUTTON_SIZE, toolbarHeight - BUTTON_HEIGHT_INSET));

    const paddingVertical =
        theme?.height == null ? TOOLBAR_PADDING_V : Math.max(0, (toolbarHeight - buttonHeight) / 2);

    return { toolbarHeight, buttonHeight, paddingVertical };
}

export function resolveToolbarButtonVisuals(
    button: Pick<ToolbarButton, 'buttonStyle' | 'isActive' | 'isDisabled'>,
    theme: EditorToolbarTheme | undefined,
    buttonHeight: number
) {
    const activeColor = button.buttonStyle?.activeColor ?? theme?.buttonActiveColor ?? ACTIVE_COLOR;

    const defaultColor = button.buttonStyle?.color ?? theme?.buttonColor ?? DEFAULT_COLOR;

    const disabledColor =
        button.buttonStyle?.disabledColor ?? theme?.buttonDisabledColor ?? DISABLED_COLOR;

    const backgroundColor =
        button.buttonStyle?.backgroundColor ?? theme?.buttonBackgroundColor ?? 'transparent';

    const activeBackgroundColor =
        button.buttonStyle?.activeBackgroundColor ?? theme?.buttonActiveBackgroundColor ?? ACTIVE_BG;

    const disabledBackgroundColor =
        button.buttonStyle?.disabledBackgroundColor ??
        theme?.buttonDisabledBackgroundColor ??
        (button.isActive ? activeBackgroundColor : backgroundColor);

    const requestedIconSize = button.buttonStyle?.iconSize ?? theme?.buttonIconSize;
    const requestedWidth = button.buttonStyle?.width;
    const requestedMinWidth = button.buttonStyle?.minWidth;

    return {
        width:
            requestedWidth != null && Number.isFinite(requestedWidth) && requestedWidth > 0
                ? requestedWidth
                : BUTTON_HIT,
        minWidth:
            requestedMinWidth != null && Number.isFinite(requestedMinWidth) && requestedMinWidth > 0
                ? requestedMinWidth
                : undefined,
        color: button.isDisabled ? disabledColor : button.isActive ? activeColor : defaultColor,
        backgroundColor: button.isDisabled
            ? disabledBackgroundColor
            : button.isActive
                ? activeBackgroundColor
                : backgroundColor,
        iconSize:
            requestedIconSize != null && Number.isFinite(requestedIconSize) && requestedIconSize > 0
                ? Math.min(requestedIconSize, buttonHeight)
                : undefined,
        borderRadius: Math.max(
            0,
            button.buttonStyle?.borderRadius ?? theme?.buttonBorderRadius ?? BUTTON_RADIUS
        ),
    };
}

export function resolveMentionSuggestionDisplayLabel(
    suggestion: MentionSuggestion,
    trigger: string
): string {
    const label = suggestion.label?.trim() || suggestion.title;

    return trigger.length > 0 && !label.startsWith(trigger) ? `${trigger}${label}` : label;
}

export const DEFAULT_TOOLBAR_ICON_SIZE = 20;

function ToolbarImageIcon({
    icon,
    color,
    size,
}: {
    icon: EditorToolbarImageIcon;
    color: string;
    size: number;
}) {
    const [failed, setFailed] = useState(false);
    return failed ? (
        <Text style={[styles.iconText, { color, fontSize: size }]}>{icon.fallbackText ?? '?'}</Text>
    ) : (
        <Image
            source={icon.source}
            resizeMode="contain"
            style={{
                width: size,
                height: size,
                tintColor: icon.tintable === false ? undefined : color,
            }}
            onError={() => setFailed(true)}
        />
    );
}

export function ToolbarIcon({
    icon,
    color,
    size,
    itemKey,
    isActive,
    isDisabled,
    renderToolbarIcon,
}: {
    icon: EditorToolbarIcon;
    color: string;
    size?: number;
    itemKey: string;
    isActive: boolean;
    isDisabled: boolean;
    renderToolbarIcon?: EditorToolbarIconRenderer;
}) {
    const resolvedSize = size ?? DEFAULT_TOOLBAR_ICON_SIZE;
    const customIcon = renderToolbarIcon?.({
        icon,
        color,
        size: resolvedSize,
        key: itemKey,
        isActive,
        isDisabled,
    });
    const materialIconName = customIcon === undefined ? resolveMaterialIconName(icon) : undefined;
    const materialIcon = useMemo(() => {
        if (!materialIconName) return null;
        const nativeModule = getNativeModule();
        if (typeof nativeModule.getToolbarIcon !== 'function') {
            throw new Error(
                'NativeEditor.getToolbarIcon is unavailable. Rebuild the native app after upgrading the editor.'
            );
        }
        return nativeModule.getToolbarIcon(materialIconName);
    }, [materialIconName]);
    if (customIcon !== undefined) {
        return <View style={styles.iconContainer}>{customIcon}</View>;
    }
    if (icon.type === 'image') {
        return (
            <View style={styles.iconContainer}>
                <ToolbarImageIcon
                    key={JSON.stringify(icon.source)}
                    icon={icon}
                    color={color}
                    size={resolvedSize}
                />
            </View>
        );
    }
    if (materialIcon) {
        return (
            <View style={styles.iconContainer}>
                <Text
                    selectable={false}
                    allowFontScaling={false}
                    style={{
                        fontFamily: materialIcon.fontFamily,
                        fontSize: resolvedSize,
                        color,
                        fontWeight: 'normal',
                        fontStyle: 'normal',
                    }}
                >
                    {materialIcon.glyph}
                </Text>
            </View>
        );
    }
    const glyph = resolveGlyphText(icon) ?? '?';
    return (
        <View style={styles.iconContainer}>
            <Text style={[styles.iconText, size == null ? null : { fontSize: size }, { color }]}>
                {glyph}
            </Text>
        </View>
    );
}

export function resolveMaterialIconName(icon: EditorToolbarIcon): string | undefined {
    switch (icon.type) {
        case 'default':
            return DEFAULT_MATERIAL_DESIGN_ICONS[icon.id];
        case 'platform':
            return icon.android?.type === 'material' ? icon.android.name : undefined;
        case 'glyph':
        case 'image':
            return undefined;
    }
}

export function resolveGlyphText(icon: EditorToolbarIcon): string | undefined {
    switch (icon.type) {
        case 'default':
            return DEFAULT_GLYPH_ICONS[icon.id];
        case 'glyph':
            return icon.text;
        case 'platform':
        case 'image':
            return icon.fallbackText;
    }
}

export const styles = StyleSheet.create({
    container: {
        backgroundColor: TOOLBAR_BG,
        borderTopWidth: StyleSheet.hairlineWidth,
        borderTopColor: TOOLBAR_BORDER,
        paddingVertical: TOOLBAR_PADDING_V,
        overflow: 'hidden',
    },
    containerWithoutTopBorder: {
        borderTopWidth: 0,
    },
    scrollContent: {
        flexDirection: 'row',
        alignItems: 'center',
        paddingHorizontal: TOOLBAR_PADDING_H,
        minWidth: '100%',
    },
    mentionSuggestionsContent: {
        paddingHorizontal: 12,
        paddingVertical: 4,
        alignItems: 'center',
        minWidth: '100%',
    },
    mentionSuggestionsScroll: {
        overflow: 'hidden',
    },
    toolbarRow: {
        flexDirection: 'row',
        alignItems: 'center',
    },
    fixedSection: {
        flexDirection: 'row',
        alignItems: 'center',
        flexShrink: 0,
    },
    startFixedSection: {
        paddingStart: TOOLBAR_PADDING_H,
    },
    endFixedSection: {
        paddingEnd: TOOLBAR_PADDING_H,
    },
    scrollSection: {
        flex: 1,
        minWidth: 0,
    },
    mentionSuggestion: {
        minWidth: 88,
        minHeight: 40,
        marginRight: 8,
        paddingHorizontal: 12,
        paddingVertical: 8,
        justifyContent: 'center',
    },
    mentionSuggestionTitle: {
        fontSize: 14,
        fontWeight: '600',
    },
    mentionSuggestionSubtitle: {
        marginTop: 1,
        fontSize: 12,
    },
    buttonAnchor: {
        position: 'relative',
    },
    button: {
        width: BUTTON_HIT,
        height: BUTTON_VISIBLE,
        justifyContent: 'center',
        alignItems: 'center',
        borderRadius: BUTTON_RADIUS,
    },
    groupDisclosure: {
        position: 'absolute',
        right: 5,
        bottom: 2,
        fontSize: 9,
        fontWeight: '700',
    },
    separator: {
        width: StyleSheet.hairlineWidth,
        height: 20,
        marginHorizontal: 4,
        backgroundColor: SEPARATOR_COLOR,
    },
    iconContainer: {
        justifyContent: 'center',
        alignItems: 'center',
    },
    iconText: {
        fontSize: 16,
        fontWeight: '600',
    },
    menuBackdrop: {
        flex: 1,
    },
    menuCard: {
        position: 'absolute',
        width: MENU_WIDTH,
        borderRadius: 14,
        borderWidth: StyleSheet.hairlineWidth,
        paddingVertical: 8,
        shadowColor: MENU_SHADOW,
        shadowOpacity: 0.16,
        shadowRadius: 18,
        shadowOffset: { width: 0, height: 8 },
        elevation: 10,
    },
    menuItem: {
        minHeight: 40,
        paddingHorizontal: 12,
        flexDirection: 'row',
        alignItems: 'center',
    },
    menuLabel: {
        flex: 1,
        marginLeft: 10,
        fontSize: 14,
        fontWeight: '500',
    },
});
