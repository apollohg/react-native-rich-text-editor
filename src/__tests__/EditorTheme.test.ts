import { EditorStyleSheet } from '../EditorStyleSheet';
import { serializeEditorTheme } from '../EditorTheme';

describe('EditorTheme serialization', () => {
    it('routes table margins to native block styles alongside table presentation', () => {
        const table = Object.freeze({ minColumnWidth: 96, cellPadding: 10, borderColor: '#123456', marginBottom: 16 });
        const theme = Object.freeze({ table, paragraph: Object.freeze({ marginBottom: 8 }) });
        expect(EditorStyleSheet.create(theme)).toBe(theme);

        expect(JSON.parse(serializeEditorTheme(theme)!)).toEqual({
            version: 1,
            table: {
                minColumnWidth: 96,
                cellPadding: 10,
                borderWidth: 1,
                borderColor: '#123456ff',
                headerBackgroundColor: '#f3f4f6ff',
                selectionColor: '#3b82f633',
                resizeHandleColor: '#3b82f6ff',
            },
            styles: { table: { marginBottom: 16 }, paragraph: { marginBottom: 8 } },
        });

        expect(table).toEqual({ minColumnWidth: 96, cellPadding: 10, borderColor: '#123456', marginBottom: 16 });
    });

    it('normalizes table margins with side and axis precedence and explicit zero', () => {
        const theme = { table: { margin: 12, marginVertical: 8, marginBottom: 0, marginRight: 4 } };

        expect(JSON.parse(serializeEditorTheme(theme)!).styles.table).toEqual({
            marginTop: 8, marginRight: 4, marginBottom: 0, marginLeft: 12,
        });
    });

    it('preserves finite negative table margins using the existing block spacing rules', () => {
        expect(JSON.parse(serializeEditorTheme({ table: { marginBottom: -4.5 } })!).styles.table).toEqual({ marginBottom: -4.5 });
    });

    it('does not emit an empty table block style when spacing is omitted', () => {
        expect(JSON.parse(serializeEditorTheme({ table: { marginBottom: undefined } })!)).not.toHaveProperty('styles');
    });

    it.each([ NaN,
        Infinity,
        -Infinity,
        '16',
        true,
        null ])('rejects invalid table margin %s with its property path', marginBottom => {
        expect(() => serializeEditorTheme({ table: { marginBottom } } as never)).toThrow('table.marginBottom: expected a finite number');
    });

    it('normalizes table presentation defaults and overrides', () => {
        expect(JSON.parse(serializeEditorTheme({ table: { minColumnWidth: 96, borderColor: '#123456' } })!)).toEqual({
            version: 1,
            table: {
                minColumnWidth: 96,
                cellPadding: 8,
                borderWidth: 1,
                borderColor: '#123456ff',
                headerBackgroundColor: '#f3f4f6ff',
                selectionColor: '#3b82f633',
                resizeHandleColor: '#3b82f6ff',
            },
        });
    });

    it('serializes flat per-level heading styles', () => {
        expect(
            JSON.parse(
                serializeEditorTheme({
                    text: { color: '#112233', fontSize: 16 },
                    h1: { fontSize: 32, fontWeight: '700', marginBottom: 14 },
                    h3: { color: '#445566', lineHeight: 28 },
                    h5: undefined,
                })!
            )
        ).toEqual({
            version: 1,
            styles: {
                text: { color: '#112233ff', fontSize: 16 },
                h1: { fontSize: 32, fontWeight: '700', marginBottom: 14 },
                h3: { color: '#445566ff', lineHeight: 28 },
            },
        });
    });

    it('serializes link decoration resets', () => {
        expect(
            JSON.parse(
                serializeEditorTheme({
                    link: {
                        color: '#445566',
                        backgroundColor: '#eef6ff',
                        textDecorationLine: 'none',
                    },
                })!
            )
        ).toEqual({
            version: 1,
            styles: {
                link: {
                    color: '#445566ff',
                    backgroundColor: '#eef6ffff',
                    textDecorationLine: 'none',
                },
            },
        });
    });

    it('carries the addon mention wire theme alongside element styles', () => {
        expect(
            JSON.parse(
                serializeEditorTheme(
                    { link: { color: '#445566' } },
                    { node: { textColor: '#112233', borderRadius: undefined } }
                )!
            )
        ).toEqual({
            version: 1,
            styles: { link: { color: '#445566ff' } },
            mentions: { node: { style: { color: '#112233ff' } } },
        });
    });

    it('serializes mention-only themes', () => {
        expect(
            JSON.parse(serializeEditorTheme(undefined, { node: { textColor: '#112233' } })!)
        ).toEqual({ version: 1, mentions: { node: { style: { color: '#112233ff' } } } });
    });

    it('omits missing mention themes', () => {
        expect(JSON.parse(serializeEditorTheme({ text: { fontSize: 16 } })!)).toEqual({
            version: 1,
            styles: { text: { fontSize: 16 } },
        });
    });

    it('retains toolbar configuration separately from content styles', () => {
        expect(
            JSON.parse(serializeEditorTheme({ toolbar: { appearance: 'native', height: 44 } })!)
        ).toEqual({ version: 1, toolbar: { appearance: 'native', height: 44 } });
    });
});

it('normalizes rich mention overrides using the same style rules', () => {
    const result = JSON.parse(
        serializeEditorTheme(undefined, {
            node: {
                color: 'red',
                fontSize: 18,
                borderWidth: 2,
                borderLeftWidth: 5,
                borderTopRightRadius: 9,
            },
        })!
    );

    expect(result.mentions.node.style).toEqual({
        color: '#ff0000ff',
        fontSize: 18,
        borderTopWidth: 2,
        borderRightWidth: 2,
        borderBottomWidth: 2,
        borderLeftWidth: 5,
        borderTopRightRadius: 9,
    });
});

it('normalizes legacy mention color aliases consistently', () => {
    const result = JSON.parse(
        serializeEditorTheme(undefined, {
            node: { textColor: 'rebeccapurple', backgroundColor: 'red' },
        })!
    );

    expect(result.mentions.node.style).toMatchObject({
        color: '#663399ff',
        backgroundColor: '#ff0000ff',
    });
});
