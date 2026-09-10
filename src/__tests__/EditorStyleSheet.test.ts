import { EditorStyleSheet } from '../EditorStyleSheet';
import { serializeEditorTheme } from '../EditorTheme';

function styles(theme: Parameters<typeof serializeEditorTheme>[0]) {
    return JSON.parse(serializeEditorTheme(theme)!).styles;
}

describe('EditorStyleSheet', () => {
    it('preserves exact version one wire output without rules', () => {
        expect(serializeEditorTheme({ paragraph: { marginBottom: 8 } })).toBe(
            '{"version":1,"styles":{"paragraph":{"marginBottom":8}}}'
        );
        expect(
            serializeEditorTheme({
                text: { lineHeight: 24, color: 'rgba(10, 20, 30, 0.5)' },
                paragraph: [
                    { margin: 8, padding: 6, fontSize: 16 },
                    { marginVertical: 0, paddingLeft: 2 },
                ],
                toolbar: { height: 44 },
            })
        ).toBe(
            '{"version":1,"toolbar":{"height":44},"styles":{"text":{"lineHeight":24,"color":"#0a141e80"},"paragraph":{"fontSize":16,"paddingLeft":2,"marginTop":0,"marginRight":8,"marginBottom":0,"marginLeft":8,"paddingTop":6,"paddingRight":6,"paddingBottom":6}}}'
        );
        expect(serializeEditorTheme({})).toBeUndefined();
    });

    it('serializes ordered ancestry rules alongside named styles', () => {
        expect(
            JSON.parse(
                serializeEditorTheme({
                    paragraph: { marginBottom: 8 },
                    rules: [
                        {
                            path: [ 'listItem', 'paragraph' ],
                            style: { marginBottom: 0 },
                        },
                    ],
                })!
            )
        ).toEqual({
            version: 1,
            styles: { paragraph: { marginBottom: 8 } },
            rules: [ { path: [ 'listItem', 'paragraph' ], style: { marginBottom: 0 } } ],
        });
    });

    it('distinguishes omitted rules from an explicit empty rule list', () => {
        expect(serializeEditorTheme({ rules: undefined })).toBeUndefined();
        expect(JSON.parse(serializeEditorTheme({ rules: [] })!)).toEqual({
            version: 1,
            rules: [],
        });
    });

    it('retains the required style field for an explicitly empty rule style', () => {
        expect(
            JSON.parse(
                serializeEditorTheme({ rules: [ { path: [ 'paragraph' ], style: {} } ] })!
            )
        ).toEqual({
            version: 1,
            rules: [ { path: [ 'paragraph' ], style: {} } ],
        });
    });

    it('preserves rule order', () => {
        expect(
            JSON.parse(
                serializeEditorTheme({
                    rules: [
                        { path: [ 'blockquote', 'paragraph' ], style: { marginBottom: 1 } },
                        { path: [ 'listItem', 'paragraph' ], style: { marginBottom: 2 } },
                    ],
                })!
            ).rules
        ).toEqual([
            { path: [ 'blockquote', 'paragraph' ], style: { marginBottom: 1 } },
            { path: [ 'listItem', 'paragraph' ], style: { marginBottom: 2 } },
        ]);
    });

    it('normalizes rule colors and spacing shorthands for the path target', () => {
        expect(
            JSON.parse(
                serializeEditorTheme({
                    rules: [
                        {
                            path: [ 'listItem', 'paragraph' ],
                            style: { color: 'rgba(10, 20, 30, 0.5)', margin: 8, marginLeft: 0 },
                        },
                    ],
                })!
            ).rules
        ).toEqual([
            {
                path: [ 'listItem', 'paragraph' ],
                style: {
                    color: '#0a141e80',
                    marginTop: 8,
                    marginRight: 8,
                    marginBottom: 8,
                    marginLeft: 0,
                },
            },
        ]);
    });

    it('does not mutate frozen rule inputs and returns the original create input', () => {
        const path = Object.freeze([ 'listItem', 'paragraph' ] as const);
        const style = Object.freeze({ marginBottom: 0 });
        const rule = Object.freeze({ path, style });
        const rules = Object.freeze([ rule ]);
        const theme = Object.freeze({ rules });

        expect(EditorStyleSheet.create(theme)).toBe(theme);
        expect(JSON.parse(serializeEditorTheme(theme)!)).toEqual({
            version: 1,
            rules: [ { path: [ 'listItem', 'paragraph' ], style: { marginBottom: 0 } } ],
        });
        expect(path).toEqual([ 'listItem', 'paragraph' ]);
        expect(style).toEqual({ marginBottom: 0 });
    });

    it.each([
        [ { rules: {} }, 'rules' ],
        [ { rules: [ null ] }, 'rules[0]' ],
        [ { rules: [ { path: [], style: {} } ] }, 'rules[0].path' ],
        [ { rules: [ { path: 'paragraph', style: {} } ] }, 'rules[0].path' ],
        [ { rules: [ { path: [ 'paragraphs' ], style: {} } ] }, 'rules[0].path[0]' ],
        [ { rules: [ { path: [ 'paragraph' ], style: {}, extra: true } ] }, 'rules[0].extra' ],
        [ { rules: [ { path: [ 'paragraph' ] } ] }, 'rules[0].style' ],
        [
            { rules: [ { path: [ 'link' ], style: { marginBottom: 4 } } ] },
            'rules[0].style.marginBottom',
        ],
    ])('rejects invalid rules with their indexed property path', (theme, path) => {
        expect(() => serializeEditorTheme(theme as never)).toThrow(path);
    });

    it('normalizes mention padding with side and axis precedence', () => {
        expect(
            styles({
                mention: [
                    { padding: 8, paddingHorizontal: 6, paddingVertical: 4 },
                    { paddingTop: 0, paddingRight: 2, paddingBottom: 3, paddingLeft: 1 },
                ],
            }).mention
        ).toEqual({
            paddingTop: 0,
            paddingRight: 2,
            paddingBottom: 3,
            paddingLeft: 1,
        });
        expect(styles({ mention: { padding: 8, paddingHorizontal: 6 } }).mention).toEqual({
            paddingTop: 8,
            paddingRight: 6,
            paddingBottom: 8,
            paddingLeft: 6,
        });
        expect(styles({ mention: { paddingVertical: 0 } }).mention).toEqual({
            paddingTop: 0,
            paddingBottom: 0,
        });
    });

    it('leaves omitted mention padding for native defaults', () => {
        expect(styles({ mention: { color: 'red' } }).mention).toEqual({ color: '#ff0000ff' });
    });

    it.each([ -1, Infinity, -Infinity, NaN, true ])('rejects invalid mention padding %s', value => {
        for (const key of [ 'padding',
            'paddingHorizontal',
            'paddingVertical',
            'paddingTop',
            'paddingRight',
            'paddingBottom',
            'paddingLeft' ]) {
            expect(() => serializeEditorTheme({ mention: { [key]: value } } as never)).toThrow(
                `mention.${key}: expected a ${value === -1 ? 'nonnegative' : 'finite'} number`
            );
        }
    });

    it('resolves side and corner overrides after composing styles', () => {
        expect(
            styles({
                blockquote: [
                    { borderLeftWidth: 4, borderTopLeftRadius: 0 },
                    { borderWidth: 1, borderRightWidth: 0, borderRadius: 8 },
                ],
            }).blockquote
        ).toEqual({
            borderTopWidth: 1,
            borderRightWidth: 0,
            borderBottomWidth: 1,
            borderLeftWidth: 4,
            borderTopLeftRadius: 0,
            borderTopRightRadius: 8,
            borderBottomLeftRadius: 8,
            borderBottomRightRadius: 8,
        });
    });

    it('supports nested conditional styles without mutating inputs', () => {
        const base = Object.freeze({ padding: 12, marginVertical: 8, color: 'red' });

        const theme = EditorStyleSheet.create({
            paragraph: [ base, false, [ null, { paddingLeft: 0 } ] ],
        });

        expect(styles(theme).paragraph).toEqual({
            paddingTop: 12,
            paddingRight: 12,
            paddingBottom: 12,
            paddingLeft: 0,
            marginTop: 8,
            marginBottom: 8,
            color: '#ff0000ff',
        });

        expect(base).toEqual({ padding: 12, marginVertical: 8, color: 'red' });
    });

    it('lets explicit undefined remove a composed override', () => {
        expect(
            styles({ paragraph: [ { color: 'red', marginBottom: 12 }, { color: undefined } ] })
        ).toEqual({ paragraph: { marginBottom: 12 } });
    });

    it('preserves inherited values for native resolution', () => {
        expect(styles({ text: { lineHeight: 24 }, paragraph: { fontSize: 16 } })).toEqual({
            text: { lineHeight: 24 },
            paragraph: { fontSize: 16 },
        });
    });

    it('normalizes alpha colors and numeric font weights', () => {
        expect(
            styles({ codeBlock: { color: 'rgba(10, 20, 30, 0.5)', fontWeight: 600 } }).codeBlock
        ).toEqual({ color: '#0a141e80', fontWeight: '600' });
    });

    it('normalizes marker and checkbox state styles', () => {
        expect(
            styles({
                listMarker: { ordered: { schemes: [ 'decimal', 'lowerRoman' ], suffix: ')' } },
                taskCheckbox: { borderWidth: 1, checked: { backgroundColor: 'blue' } },
            })
        ).toEqual({
            listMarker: { ordered: { schemes: [ 'decimal', 'lowerRoman' ], suffix: ')' } },
            taskCheckbox: {
                borderTopWidth: 1,
                borderRightWidth: 1,
                borderBottomWidth: 1,
                borderLeftWidth: 1,
                checked: { backgroundColor: '#0000ffff' },
            },
        });
    });

    it.each([
        [ { paragraph: { padding: -1 } }, 'paragraph.padding' ],
        [ { image: { resizeMode: 'repeat' } }, 'image.resizeMode' ],
        [ { link: { marginBottom: 4 } }, 'link.marginBottom' ],
        [ { paragraph: { color: 'not-a-color' } }, 'paragraph.color' ],
        [ { paragraph: { fontSize: Infinity } }, 'paragraph.fontSize' ],
        [ { paragraph: { flex: 1 } }, 'paragraph.flex' ],
        [ { paragraphs: {} }, 'paragraphs' ],
        [ { paragraph: { fontWeight: 650 } }, 'paragraph.fontWeight' ],
    ])('rejects invalid styles with their property path', (theme, path) => {
        expect(() => serializeEditorTheme(theme as never)).toThrow(path);
    });

    it('rejects invalid entries hidden behind later array overrides', () => {
        expect(() =>
            serializeEditorTheme({ paragraph: [ { padding: -1 }, { padding: 1 } ] })).toThrow('paragraph.padding');
    });

    it('omits an empty theme and preserves separate toolbar settings', () => {
        expect(serializeEditorTheme(undefined)).toBeUndefined();
        expect(serializeEditorTheme({ paragraph: [ false, undefined ] })).toBeUndefined();

        expect(JSON.parse(serializeEditorTheme({ toolbar: { height: 44 } })!)).toEqual({
            version: 1,
            toolbar: { height: 44 },
        });
    });
});
