import type { TextStyle } from 'react-native';
import type { EditorToolbarTheme, EditorFontWeight } from './EditorTheme';

export type EditorStyleProp<T> = T | false | null | undefined | readonly EditorStyleProp<T>[];

export interface EditorTypographyStyle {
    fontFamily?: string;
    fontSize?: number;
    fontWeight?: EditorFontWeight | 100 | 200 | 300 | 400 | 500 | 600 | 700 | 800 | 900;
    fontStyle?: 'normal' | 'italic';
    color?: string;
    lineHeight?: number;
    letterSpacing?: number;
    textDecorationLine?: TextStyle['textDecorationLine'];
    textDecorationColor?: string;
    textDecorationStyle?: 'solid' | 'double' | 'dotted' | 'dashed';
}

export interface EditorBorderStyle {
    borderWidth?: number;
    borderColor?: string;
    borderStyle?: 'solid' | 'dashed' | 'dotted';
    borderTopWidth?: number;
    borderRightWidth?: number;
    borderBottomWidth?: number;
    borderLeftWidth?: number;
    borderTopColor?: string;
    borderRightColor?: string;
    borderBottomColor?: string;
    borderLeftColor?: string;
    borderRadius?: number;
    borderTopLeftRadius?: number;
    borderTopRightRadius?: number;
    borderBottomLeftRadius?: number;
    borderBottomRightRadius?: number;
}

export interface EditorPaddingStyle {
    padding?: number;
    paddingHorizontal?: number;
    paddingVertical?: number;
    paddingTop?: number;
    paddingRight?: number;
    paddingBottom?: number;
    paddingLeft?: number;
}

export interface EditorMarginStyle {
    margin?: number;
    marginHorizontal?: number;
    marginVertical?: number;
    marginTop?: number;
    marginRight?: number;
    marginBottom?: number;
    marginLeft?: number;
}

export interface EditorSurfaceStyle extends EditorBorderStyle, EditorPaddingStyle {
    backgroundColor?: string;
}

export interface EditorBoxStyle extends EditorSurfaceStyle, EditorMarginStyle {}

export interface EditorTextStyle extends EditorTypographyStyle, EditorBoxStyle {
    textAlign?: TextStyle['textAlign'];
}

export interface EditorInlineStyle extends EditorTypographyStyle {
    backgroundColor?: string;
}

export interface EditorImageStyle extends EditorBoxStyle {
    resizeMode?: 'contain' | 'cover' | 'stretch';
}

export interface EditorListStyle extends EditorTextStyle {
    indent?: number;
    baseIndentMultiplier?: number;
}

export type EditorOrderedListNumberingScheme =
    | 'decimal'
    | 'lowerAlpha'
    | 'upperAlpha'
    | 'lowerRoman'
    | 'upperRoman';

export interface EditorOrderedListMarkerTheme {
    schemes?: readonly EditorOrderedListNumberingScheme[];
    suffix?: '.' | ')';
}

export interface EditorListMarkerStyle {
    color?: string;
    scale?: number;
    gap?: number;
    ordered?: EditorOrderedListMarkerTheme;
}

export interface EditorCheckboxAppearance extends EditorBorderStyle {
    backgroundColor?: string;
    checkColor?: string;
}

export interface EditorTaskCheckboxStyle extends EditorCheckboxAppearance {
    size?: number;
    gap?: number;
    checked?: EditorStyleProp<EditorCheckboxAppearance>;
}

export interface EditorHorizontalRuleStyle extends EditorBorderStyle, EditorMarginStyle {
    backgroundColor?: string;
    height?: number;
}

export interface EditorMentionStyle extends EditorInlineStyle, EditorBorderStyle, EditorPaddingStyle {}

export interface EditorStyleMap {
    content: EditorSurfaceStyle;
    text: EditorTypographyStyle;
    paragraph: EditorTextStyle;
    h1: EditorTextStyle;
    h2: EditorTextStyle;
    h3: EditorTextStyle;
    h4: EditorTextStyle;
    h5: EditorTextStyle;
    h6: EditorTextStyle;
    blockquote: EditorTextStyle;
    codeBlock: EditorTextStyle;
    bulletList: EditorListStyle;
    orderedList: EditorListStyle;
    taskList: EditorListStyle;
    listItem: EditorTextStyle;
    taskItem: EditorTextStyle;
    listMarker: EditorListMarkerStyle;
    taskCheckbox: EditorTaskCheckboxStyle;
    horizontalRule: EditorHorizontalRuleStyle;
    image: EditorImageStyle;
    link: EditorInlineStyle;
    inlineCode: EditorInlineStyle;
    bold: EditorInlineStyle;
    italic: EditorInlineStyle;
    underline: EditorInlineStyle;
    strike: EditorInlineStyle;
    mention: EditorMentionStyle;
    placeholder: EditorTypographyStyle;
}

export type EditorStyleRule = {
    [K in keyof EditorStyleMap]: {
        path: readonly [ ...(keyof EditorStyleMap)[], K ];
        style: EditorStyleProp<EditorStyleMap[K]>;
    };
}[keyof EditorStyleMap];

export type EditorTheme = {
    [K in keyof EditorStyleMap]?: EditorStyleProp<EditorStyleMap[K]>;
} & {
    rules?: readonly EditorStyleRule[];
    toolbar?: EditorToolbarTheme;
};

export type EditorLinkTheme = EditorInlineStyle;
export type EditorHeadingTheme = Partial<
    Record<'h1' | 'h2' | 'h3' | 'h4' | 'h5' | 'h6', EditorStyleProp<EditorTextStyle>>
>;
export type EditorListTheme = EditorListStyle;
export type EditorHorizontalRuleTheme = EditorHorizontalRuleStyle;
export type EditorBlockquoteTheme = EditorTextStyle;
export type EditorCodeBlockTheme = EditorTextStyle;

export type NormalizedEditorStyle = Readonly<Record<string, unknown>>;
export interface NormalizedEditorTheme {
    version: 1;
    styles?: Partial<Record<keyof EditorStyleMap, NormalizedEditorStyle>>;
    rules?: readonly {
        path: readonly (keyof EditorStyleMap)[];
        style: NormalizedEditorStyle;
    }[];
    toolbar?: EditorToolbarTheme;
}

type ExactStyle<T, Shape> = T extends false | null | undefined
    ? T
    : T extends readonly unknown[]
      ? { [K in keyof T]: ExactStyle<T[K], Shape> }
      : Shape & {
            [K in keyof T]: K extends keyof Shape
                ? K extends 'checked'
                    ? ExactStyle<T[K], EditorCheckboxAppearance>
                    : K extends 'ordered'
                      ? ExactStyle<T[K], EditorOrderedListMarkerTheme>
                      : Shape[K]
                : never;
        };

type ExactRuleStyle<T, Shape> = [ EditorStyleProp<Shape> ] extends [ T ]
    ? [ T ] extends [ EditorStyleProp<Shape> ]
        ? T
        : ExactStyle<T, Shape>
    : ExactStyle<T, Shape>;

type ExactEditorStyleRule<T> = T extends {
    readonly path: infer Path extends readonly (keyof EditorStyleMap)[];
    readonly style: unknown;
}
    ? Path extends readonly [ ...(keyof EditorStyleMap)[], infer Target ]
        ? Target extends keyof EditorStyleMap
            ? {
                [K in keyof T]: K extends 'path'
                    ? Path
                    : K extends 'style'
                        ? ExactRuleStyle<T[K], EditorStyleMap[Target]>
                        : never;
            }
            : never
        : never
    : never;

type ExactEditorStyleRules<T> = T extends undefined
    ? T
    : T extends readonly unknown[]
        ? { [K in keyof T]: ExactEditorStyleRule<T[K]> }
        : never;

export type ExactEditorTheme<T> = {
    [K in keyof T]: K extends 'rules'
        ? ExactEditorStyleRules<T[K]>
        : K extends keyof EditorStyleMap
            ? ExactStyle<T[K], EditorStyleMap[K]>
            : K extends 'toolbar'
                ? EditorToolbarTheme
                : never;
};
