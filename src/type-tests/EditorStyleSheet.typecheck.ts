import { EditorStyleSheet } from '../EditorStyleSheet';
import type { EditorTheme } from '../EditorTheme';
import type { EditorStyleRule } from '../index';

const styles = EditorStyleSheet.create({
    paragraph: [ { fontWeight: 600, padding: 12 }, false, [ undefined, { marginBottom: 0 } ] ],
    image: { resizeMode: 'contain', borderTopLeftRadius: 8 },
    link: { color: 'rebeccapurple', textDecorationLine: 'underline' },
    mention: {
        padding: 8,
        paddingHorizontal: 6,
        paddingVertical: 4,
        paddingTop: 0,
        paddingRight: 1,
        paddingBottom: 2,
        paddingLeft: 3,
    },
});

const theme: EditorTheme = styles;
void theme;

const paragraphRule: EditorStyleRule = {
    path: [ 'listItem', 'paragraph' ],
    style: { marginBottom: 0 },
};
const linkRule: EditorStyleRule = {
    path: [ 'link' ],
    style: { color: 'rebeccapurple' },
};
void paragraphRule;
void linkRule;

const rules = EditorStyleSheet.create({
    rules: [
        paragraphRule,
        {
            path: [ 'link' ],
            style: { color: 'rebeccapurple' },
        },
        {
            path: [ 'taskList', 'listItem', 'paragraph' ],
            style: [ { margin: 8 }, false, [ undefined, { marginBottom: 0 } ] ],
        },
    ],
});

const rulesTheme: EditorTheme = rules;
void rulesTheme;
const literalRulePathElement: 'link' = rules.rules[1].path[0];
void literalRulePathElement;

EditorStyleSheet.create({ rules: undefined });

// @ts-expect-error Mention padding is numeric.
EditorStyleSheet.create({ mention: { padding: true } });
// @ts-expect-error Mention margins are unsupported.
EditorStyleSheet.create({ mention: { margin: 4 } });

// @ts-expect-error Unknown element.
EditorStyleSheet.create({ paragraphs: { color: 'red' } });
// @ts-expect-error Inline links have no box margins.
EditorStyleSheet.create({ link: { color: 'red', marginBottom: 12 } });
// @ts-expect-error Unsupported image fit.
EditorStyleSheet.create({ image: { resizeMode: 'repeat' } });
// @ts-expect-error Unsupported layout property, even alongside valid properties.
EditorStyleSheet.create({ paragraph: [ { fontSize: 16, flex: 1 } ] });
// @ts-expect-error Legacy link spelling.
EditorStyleSheet.create({ links: { color: 'red' } });
// @ts-expect-error Unsupported nested checkbox property.
EditorStyleSheet.create({ taskCheckbox: { checked: { backgroundColor: 'red', flex: 1 } } });
// @ts-expect-error Unsupported nested ordered marker property.
EditorStyleSheet.create({ listMarker: { ordered: { suffix: '.', extra: true } } });
// @ts-expect-error Use a supported numeric or string weight.
EditorStyleSheet.create({ paragraph: { fontWeight: 'semibold' } });

// @ts-expect-error Link rule styles have no box margins.
EditorStyleSheet.create({ rules: [ { path: [ 'link' ], style: { marginBottom: 12 } } ] });
EditorStyleSheet.create({
    rules: [
        {
            path: [ 'listItem', 'paragraph' ],
            // @ts-expect-error Composed rule styles reject unsupported properties.
            style: [ { marginBottom: 12 }, { color: 'red', flex: 1 } ],
        },
    ],
});
// @ts-expect-error Rules require an own style property.
EditorStyleSheet.create({ rules: [ { path: [ 'paragraph' ] } ] });
// @ts-expect-error Rules accept only path and style properties.
EditorStyleSheet.create({ rules: [ { path: [ 'paragraph' ], style: {}, extra: true } ] });
