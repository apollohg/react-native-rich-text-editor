import {
    TABLE_NODE_NAMES,
    type DocumentJSON,
    type EditorToolbarItem,
    type MentionSuggestion,
    type TableDirection,
    type TableNamingPreset,
} from '@apollohg/react-native-rich-text-editor';

import { BOLD, ITALIC, STRIKE, UNDERLINE, heading, link, listItem, paragraph, text } from './documentNodes';
import {
    PLAIN_TABLE_SIZES,
    createIrregularTableDocument,
    createPlainTable,
    createRichTableDocument,
    table,
    tableRow,
    textCell,
    withTableDirection,
    type TableCellKind,
} from './tableContent';
import { TASK_ITEM_NODE_NAME, TASK_LIST_NODE_NAME } from './taskList';

export const APP_TITLE = 'React Native Editor';

export const EDITOR_PLACEHOLDER = 'Start writing…';

export const MENTION_TRIGGER = '@';

/** Remote image used by the initial document. */
export const SAMPLE_IMAGE_URL = 'https://picsum.photos/seed/native-editor/1200/800';

export const TABLE_NAMING_PRESET: TableNamingPreset = 'prosemirror';
export const TABLE_NAMES = TABLE_NODE_NAMES[TABLE_NAMING_PRESET];

export const EDITOR_SURFACES = [ 'editor', 'viewer' ] as const;
export type EditorSurface = (typeof EDITOR_SURFACES)[number];
export const EDITOR_SURFACE_LABELS: Readonly<Record<EditorSurface, string>> = {
    editor: 'Editor',
    viewer: 'Viewer',
};

export const TABLE_DIRECTIONS: readonly TableDirection[] = [ 'ltr', 'rtl' ];
export const TABLE_DIRECTION_LABELS: Readonly<Record<TableDirection, string>> = {
    ltr: 'LTR',
    rtl: 'RTL',
};

export const TABLE_TOOLBAR_MODES = [ 'custom', 'default', 'hidden' ] as const;
export type TableToolbarMode = (typeof TABLE_TOOLBAR_MODES)[number];
export const TABLE_TOOLBAR_MODE_LABELS: Readonly<Record<TableToolbarMode, string>> = {
    custom: 'Custom bar',
    default: 'Default bar',
    hidden: 'No bar',
};

export const VIEWPORT_MODES = [ 'full', 'narrow' ] as const;
export type ViewportMode = (typeof VIEWPORT_MODES)[number];
export const VIEWPORT_MODE_LABELS: Readonly<Record<ViewportMode, string>> = {
    full: 'Full width',
    narrow: 'Narrow',
};

const REPOSITORY_URL = 'https://github.com/apollohg/react-native-rich-text-editor';
const MERGED_HEADER_COLSPAN = 2;
const MERGED_GROUP_ROWSPAN = 2;
const WIDE_TABLE_COLUMN_WIDTH = 180;
const COUNTER_CARD_NODE_NAME = 'counterCard';

const REPOSITORY_LINK = link(REPOSITORY_URL);

function taskItem(checked: boolean, label: string): DocumentJSON {
    return { type: TASK_ITEM_NODE_NAME, attrs: { checked }, content: [ paragraph(text(label)) ] };
}

function counterCard(title: string, count: number): DocumentJSON {
    return { type: COUNTER_CARD_NODE_NAME, attrs: { title, count } };
}

function showcaseCell(
    kind: TableCellKind,
    label: string | null,
    attrs?: Record<string, unknown>
): DocumentJSON {
    return textCell(TABLE_NAMES, kind, label, attrs);
}

function wideTableCell(kind: TableCellKind, label: string): DocumentJSON {
    return showcaseCell(kind, label, { colwidth: [ WIDE_TABLE_COLUMN_WIDTH ] });
}

/**
 * The initial document is JSON rather than HTML: the HTML importer maps every
 * `<ul>` to a bullet list, so a checklist can only be seeded this way.
 */
const INITIAL_DOCUMENT: DocumentJSON = {
    type: 'doc',
    content: [
        heading(1, 'Field notes'),
        paragraph(
            text('A native editor with a '),
            text('Rust core', BOLD),
            text('. Everything below is editable: headings, '),
            text('emphasis', ITALIC),
            text(', '),
            text('underline', UNDERLINE),
            text(', '),
            text('strikethrough', STRIKE),
            text(', and '),
            text('links', REPOSITORY_LINK),
            text('.')
        ),
        heading(2, 'Tables'),
        heading(3, 'Basic'),
        table(
            TABLE_NAMES,
            tableRow(
                TABLE_NAMES,
                showcaseCell('headerCell', 'Task'),
                showcaseCell('headerCell', 'Owner'),
                showcaseCell('headerCell', 'Status')
            ),
            tableRow(
                TABLE_NAMES,
                showcaseCell('cell', 'Outline'),
                showcaseCell('cell', 'Alice'),
                showcaseCell('cell', 'In progress')
            ),
            tableRow(
                TABLE_NAMES,
                showcaseCell('cell', 'Review'),
                showcaseCell('cell', null),
                showcaseCell('cell', 'Ready')
            )
        ),
        heading(3, 'Merged cells'),
        table(
            TABLE_NAMES,
            tableRow(
                TABLE_NAMES,
                showcaseCell('headerCell', 'Release plan', { colspan: MERGED_HEADER_COLSPAN }),
                showcaseCell('headerCell', 'Status')
            ),
            tableRow(
                TABLE_NAMES,
                showcaseCell('cell', 'Core editor', { rowspan: MERGED_GROUP_ROWSPAN }),
                showcaseCell('cell', 'iOS'),
                showcaseCell('cell', 'Ready')
            ),
            tableRow(TABLE_NAMES, showcaseCell('cell', 'Android'), showcaseCell('cell', 'In review'))
        ),
        heading(3, 'Wide table'),
        table(
            TABLE_NAMES,
            tableRow(
                TABLE_NAMES,
                wideTableCell('headerCell', 'Phase'),
                wideTableCell('headerCell', 'Owner'),
                wideTableCell('headerCell', 'Platform'),
                wideTableCell('headerCell', 'Priority'),
                wideTableCell('headerCell', 'Due date'),
                wideTableCell('headerCell', 'Status')
            ),
            tableRow(
                TABLE_NAMES,
                wideTableCell('cell', 'Design'),
                wideTableCell('cell', 'Chloe'),
                wideTableCell('cell', 'iOS + Android'),
                wideTableCell('cell', 'High'),
                wideTableCell('cell', 'Friday'),
                wideTableCell('cell', 'In progress')
            )
        ),
        {
            type: 'blockquote',
            content: [ paragraph(text('Type @ anywhere to mention someone on the team.')) ],
        },
        {
            type: 'codeBlock',
            attrs: { language: 'typescript' },
            content: [ text('const greet = (name: string) => {\n    return `Hello, ${name}!`;\n};') ],
        },
        heading(2, 'Today'),
        {
            type: TASK_LIST_NODE_NAME,
            content: [
                taskItem(true, 'Review the toolbar above the keyboard'),
                taskItem(false, 'Tap a checkbox to toggle it'),
                taskItem(false, 'Turn any paragraph into a task from the list menu'),
            ],
        },
        heading(2, 'Lists'),
        {
            type: 'bullet_list',
            content: [
                listItem(paragraph(text('Try nested lists')), {
                    type: 'bullet_list',
                    content: [ listItem(paragraph(text('Indent and outdent from the toolbar'))) ],
                }),
                listItem(paragraph(text('Tap the image to resize it'))),
            ],
        },
        { type: 'image', attrs: { src: SAMPLE_IMAGE_URL, alt: 'Sample' } },
        heading(2, 'Counters'),
        paragraph(text('Custom blocks are React components living inside the document.')),
        counterCard('Cups of coffee', 2),
        {
            type: 'ordered_list',
            content: [
                listItem(paragraph(text('Insert another with the + button'))),
                listItem(
                    paragraph(text('Tap a counter to select it, then delete it like any block'))
                ),
            ],
        },
        { type: 'horizontal_rule' },
        paragraph(),
    ],
};

export const DOCUMENT_FIXTURES = [
    'showcase',
    'rich',
    'small',
    'tall',
    'wide',
    'irregular',
] as const;

export type DocumentFixture = (typeof DOCUMENT_FIXTURES)[number];

export const DOCUMENT_FIXTURE_LABELS: Readonly<Record<DocumentFixture, string>> = {
    showcase: 'Showcase',
    rich: 'Rich',
    small: '3×3',
    tall: '1000×20',
    wide: '100×200',
    irregular: 'Irregular',
};

export function fixtureDocument(fixture: DocumentFixture): DocumentJSON {
    switch (fixture) {
        case 'showcase':
            return INITIAL_DOCUMENT;
        case 'rich':
            return createRichTableDocument({
                names: TABLE_NAMES,
                imageUrl: SAMPLE_IMAGE_URL,
                atom: counterCard('Cells per row', 3),
                linkUrl: REPOSITORY_URL,
            });
        case 'irregular':
            return createIrregularTableDocument(TABLE_NAMES);

        default: {
            const { rows, columns } = PLAIN_TABLE_SIZES[fixture];

            return createPlainTable(rows, columns, TABLE_NAMES);
        }
    }
}

export function viewerDocument(document: DocumentJSON, direction: TableDirection): DocumentJSON {
    return withTableDirection(document, TABLE_NAMES, direction);
}

export const MENTION_SUGGESTIONS: readonly MentionSuggestion[] = [
    {
        key: 'alice',
        title: 'Alice Chen',
        subtitle: 'Design',
        label: 'alice',
        attrs: { id: 'user_alice', entityType: 'user' },
    },
    {
        key: 'ben',
        title: 'Ben Ortiz',
        subtitle: 'Engineering',
        label: 'ben',
        attrs: { id: 'user_ben', entityType: 'user' },
    },
    {
        key: 'chloe',
        title: 'Chloe Park',
        subtitle: 'Product',
        label: 'chloe',
        attrs: { id: 'user_chloe', entityType: 'user' },
    },
    {
        key: 'apollo-team',
        title: 'Apollo Team',
        subtitle: 'Everyone',
        label: 'apollo-team',
        attrs: { id: 'group_apollo', entityType: 'group' },
    },
];

/** Custom toolbar action that inserts a counter card at the caret. */
export const INSERT_COUNTER_ACTION_KEY = 'insertCounter';

/**
 * Custom toolbar action that wraps the selection in a task list, or unwraps
 * it when already inside one. The built-in `list` item only covers bullet and
 * numbered lists, so active and disabled states come from `onActiveStateChange`.
 */
export const TOGGLE_TASK_LIST_ACTION_KEY = 'toggleTaskList';

export function buildToolbarItems({
    taskListActive,
    taskListAvailable,
}: {
    taskListActive: boolean;
    taskListAvailable: boolean;
}): readonly EditorToolbarItem[] {
    return [
        {
            type: 'group',
            key: 'headings',
            label: 'Heading',
            icon: {
                type: 'platform',
                ios: { type: 'sfSymbol', name: 'textformat.size' },
                android: { type: 'material', name: 'format-size' },
                fallbackText: 'H',
            },
            presentation: 'menu',
            items: [
                {
                    type: 'heading',
                    level: 1,
                    label: 'Heading 1',
                    icon: { type: 'default', id: 'h1' },
                },
                {
                    type: 'heading',
                    level: 2,
                    label: 'Heading 2',
                    icon: { type: 'default', id: 'h2' },
                },
                {
                    type: 'heading',
                    level: 3,
                    label: 'Heading 3',
                    icon: { type: 'default', id: 'h3' },
                },
                {
                    type: 'heading',
                    level: 4,
                    label: 'Heading 4',
                    icon: { type: 'default', id: 'h4' },
                },
                {
                    type: 'heading',
                    level: 5,
                    label: 'Heading 5',
                    icon: { type: 'default', id: 'h5' },
                },
                {
                    type: 'heading',
                    level: 6,
                    label: 'Heading 6',
                    icon: { type: 'default', id: 'h6' },
                },
            ],
        },
        { type: 'separator' },
        { type: 'mark', mark: 'bold', label: 'Bold', icon: { type: 'default', id: 'bold' } },
        { type: 'mark', mark: 'italic', label: 'Italic', icon: { type: 'default', id: 'italic' } },
        {
            type: 'mark',
            mark: 'underline',
            label: 'Underline',
            icon: { type: 'default', id: 'underline' },
        },
        {
            type: 'mark',
            mark: 'strike',
            label: 'Strikethrough',
            icon: { type: 'default', id: 'strike' },
        },
        { type: 'separator' },
        { type: 'link', label: 'Link', icon: { type: 'default', id: 'link' } },
        { type: 'image', label: 'Image', icon: { type: 'default', id: 'image' } },
        { type: 'separator' },
        {
            type: 'group',
            key: 'lists',
            label: 'Lists',
            icon: { type: 'default', id: 'bulletList' },
            presentation: 'menu',
            items: [
                {
                    type: 'list',
                    listType: 'bullet_list',
                    label: 'Bullet list',
                    icon: { type: 'default', id: 'bulletList' },
                },
                {
                    type: 'list',
                    listType: 'ordered_list',
                    label: 'Numbered list',
                    icon: { type: 'default', id: 'orderedList' },
                },
                {
                    type: 'action',
                    key: TOGGLE_TASK_LIST_ACTION_KEY,
                    label: 'Task list',
                    isActive: taskListActive,
                    isDisabled: !taskListAvailable,
                    icon: {
                        type: 'platform',
                        ios: { type: 'sfSymbol', name: 'checklist' },
                        android: { type: 'material', name: 'format-list-checks' },
                        fallbackText: '☑',
                    },
                },
                {
                    type: 'command',
                    command: 'indentList',
                    label: 'Indent',
                    icon: { type: 'default', id: 'indentList' },
                },
                {
                    type: 'command',
                    command: 'outdentList',
                    label: 'Outdent',
                    icon: { type: 'default', id: 'outdentList' },
                },
            ],
        },
        { type: 'blockquote', label: 'Quote', icon: { type: 'default', id: 'blockquote' } },
        {
            type: 'group',
            key: 'insert',
            label: 'Insert',
            icon: {
                type: 'platform',
                ios: { type: 'sfSymbol', name: 'plus.square' },
                android: { type: 'material', name: 'plus-box' },
                fallbackText: '+',
            },
            presentation: 'menu',
            items: [
                {
                    type: 'action',
                    key: INSERT_COUNTER_ACTION_KEY,
                    label: 'Counter',
                    icon: {
                        type: 'platform',
                        ios: { type: 'sfSymbol', name: 'number.square' },
                        android: { type: 'material', name: 'numeric' },
                        fallbackText: '#',
                    },
                },
                {
                    type: 'node',
                    nodeType: 'horizontal_rule',
                    label: 'Divider',
                    icon: { type: 'default', id: 'horizontalRule' },
                },
                {
                    type: 'node',
                    nodeType: 'hard_break',
                    label: 'Line break',
                    icon: { type: 'default', id: 'lineBreak' },
                },
            ],
        },
        { type: 'separator' },
        { type: 'command', command: 'undo', label: 'Undo', icon: { type: 'default', id: 'undo' } },
        { type: 'command', command: 'redo', label: 'Redo', icon: { type: 'default', id: 'redo' } },
    ];
}
