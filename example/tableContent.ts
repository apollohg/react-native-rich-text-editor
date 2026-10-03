import type {
    DocumentJSON,
    SchemaDefinition,
    TableDirection,
    TableNodeNames,
} from '@apollohg/react-native-rich-text-editor';

import {
    BOLD,
    ITALIC,
    blockquote,
    bulletList,
    codeBlock,
    documentOf,
    heading,
    image,
    link,
    listItem,
    orderedList,
    paragraph,
    text,
} from './documentNodes';

export const TABLE_FIXTURE_CELL_TEXT = 'abcdefghijkl';
export const TABLE_DIRECTION_ATTR = 'dir';

export const PLAIN_TABLE_SIZES = {
    small: { rows: 3, columns: 3 },
    tall: { rows: 1000, columns: 20 },
    wide: { rows: 100, columns: 200 },
} as const;

export type PlainTableSize = keyof typeof PLAIN_TABLE_SIZES;

export type TableCellKind = 'cell' | 'headerCell';

export interface RichTableFixtureOptions {
    names: TableNodeNames;
    imageUrl: string;
    atom: DocumentJSON;
    linkUrl: string;
}

const MIN_TABLE_EXTENT = 1;
const HEADER_ROW_INDEX = 0;
const WIDE_SPAN = 2;
const TALL_SPAN = 2;
const OVERHANGING_ROWSPAN = 3;
const RICH_CODE_LANGUAGE = 'typescript';
const RICH_CODE_SOURCE = 'const cell = table.at(row, column);';

export function tableCell(
    names: TableNodeNames,
    kind: TableCellKind,
    content: readonly DocumentJSON[],
    attrs?: Record<string, unknown>
): DocumentJSON {
    return {
        type: names[kind],
        ...(attrs === undefined ? {} : { attrs }),
        content: content.length === 0 ? [ paragraph() ] : content,
    };
}

export function textCell(
    names: TableNodeNames,
    kind: TableCellKind,
    label: string | null,
    attrs?: Record<string, unknown>
): DocumentJSON {
    return tableCell(names, kind, [ label === null ? paragraph() : paragraph(text(label)) ], attrs);
}

export function tableRow(names: TableNodeNames, ...cells: readonly DocumentJSON[]): DocumentJSON {
    return { type: names.row, content: cells };
}

export function table(names: TableNodeNames, ...rows: readonly DocumentJSON[]): DocumentJSON {
    return { type: names.table, content: rows };
}

function requireTableExtent(value: number, dimension: string): number {
    if (!Number.isSafeInteger(value) || value < MIN_TABLE_EXTENT) {
        throw new RangeError(`a plain table needs a positive integer ${dimension} count, not ${value}`);
    }

    return value;
}

export function createPlainTable(rows: number, columns: number, names: TableNodeNames): DocumentJSON {
    const rowCount = requireTableExtent(rows, 'row');
    const columnCount = requireTableExtent(columns, 'column');

    const tableRows = Array.from({ length: rowCount }, (_, rowIndex) => {
        const kind: TableCellKind = rowIndex === HEADER_ROW_INDEX ? 'headerCell' : 'cell';

        return tableRow(
            names,
            ...Array.from({ length: columnCount }, () =>
                textCell(names, kind, TABLE_FIXTURE_CELL_TEXT))
        );
    });

    return documentOf(table(names, ...tableRows));
}

export function createRichTableDocument({
    names,
    imageUrl,
    atom,
    linkUrl,
}: RichTableFixtureOptions): DocumentJSON {
    const nested = table(
        names,
        tableRow(names, textCell(names, 'headerCell', 'Nested'), textCell(names, 'headerCell', 'Import')),
        tableRow(names, textCell(names, 'cell', 'kept'), textCell(names, 'cell', 'read-only'))
    );

    return documentOf(
        heading(1, 'Rich table'),
        paragraph(text('Cells hold any block the document admits.')),
        table(
            names,
            tableRow(
                names,
                tableCell(names, 'headerCell', [ heading(3, 'Merged heading') ], { colspan: WIDE_SPAN }),
                textCell(names, 'headerCell', 'Notes')
            ),
            tableRow(
                names,
                tableCell(
                    names,
                    'cell',
                    [
                        paragraph(
                            text('Bold', BOLD),
                            text(' and '),
                            text('italic', ITALIC),
                            text(' with a '),
                            text('link', link(linkUrl))
                        ),
                    ],
                    { rowspan: TALL_SPAN }
                ),
                tableCell(names, 'cell', [
                    bulletList(
                        listItem(paragraph(text('First point'))),
                        listItem(paragraph(text('Second point')))
                    ),
                ]),
                tableCell(names, 'cell', [ codeBlock(RICH_CODE_LANGUAGE, RICH_CODE_SOURCE) ])
            ),
            tableRow(
                names,
                tableCell(names, 'cell', [ blockquote(paragraph(text('Quoted inside a cell'))) ]),
                tableCell(names, 'cell', [
                    orderedList(listItem(paragraph(text('Step one'))), listItem(paragraph(text('Step two')))),
                ])
            ),
            tableRow(
                names,
                tableCell(names, 'cell', [ image(imageUrl, 'Cell image') ]),
                tableCell(names, 'cell', [ atom ]),
                tableCell(names, 'cell', [ nested ])
            )
        ),
        paragraph(text('After the rich table.'))
    );
}

export function createIrregularTableDocument(names: TableNodeNames): DocumentJSON {
    return documentOf(
        paragraph(text('Raw irregular import: rows differ in width and a rowspan overhangs the table.')),
        table(
            names,
            tableRow(
                names,
                textCell(names, 'headerCell', 'Wide header', { colspan: WIDE_SPAN }),
                textCell(names, 'headerCell', 'Status')
            ),
            tableRow(
                names,
                textCell(names, 'cell', 'Overhang', { rowspan: OVERHANGING_ROWSPAN }),
                textCell(names, 'cell', 'Short row')
            ),
            tableRow(
                names,
                textCell(names, 'cell', 'One'),
                textCell(names, 'cell', 'Two'),
                textCell(names, 'cell', 'Three'),
                textCell(names, 'cell', 'Four')
            )
        ),
        paragraph(text('After the irregular table.'))
    );
}

export function withTableDirection(
    node: DocumentJSON,
    names: TableNodeNames,
    direction: TableDirection
): DocumentJSON {
    const content = Array.isArray(node.content)
        ? (node.content as DocumentJSON[]).map(child => withTableDirection(child, names, direction))
        : undefined;

    const attrs =
        node.type === names.table
            ? { ...(node.attrs as Record<string, unknown> | undefined), [TABLE_DIRECTION_ATTR]: direction }
            : node.attrs;

    return {
        ...node,
        ...(attrs === undefined ? {} : { attrs }),
        ...(content === undefined ? {} : { content }),
    };
}

export function withTableDirectionAttribute(
    schema: SchemaDefinition,
    names: TableNodeNames
): SchemaDefinition {
    return {
        ...schema,
        nodes: schema.nodes.map(node =>
            node.name === names.table
                ? { ...node, attrs: { ...node.attrs, [TABLE_DIRECTION_ATTR]: { default: null } } }
                : node),
    };
}
