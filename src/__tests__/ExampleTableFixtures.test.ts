import {
    PLAIN_TABLE_SIZES,
    TABLE_DIRECTION_ATTR,
    TABLE_FIXTURE_CELL_TEXT,
    createIrregularTableDocument,
    createPlainTable,
    createRichTableDocument,
    withTableDirection,
    type PlainTableSize,
} from '../../example/tableContent';
import type { DocumentJSON } from '../NativeEditorTypes';
import { DEFAULT_MAX_TABLE_GRID_SLOTS } from '../ResourceLimits';
import { TABLE_NODE_NAMES } from '../TableSchema';
import type { TableNamingPreset, TableNodeNames } from '../TableTypes';

const FIXTURE_TEXT_LENGTH = 12;

const EXPECTED_SLOTS: Readonly<Record<PlainTableSize, number>> = {
    small: 9,
    tall: 20_000,
    wide: 20_000,
};

const PRESETS = Object.keys(TABLE_NODE_NAMES) as TableNamingPreset[];
const SIZES = Object.keys(PLAIN_TABLE_SIZES) as PlainTableSize[];
const RICH_FIXTURE_URL = 'https://example.com/cell.png';
const RICH_FIXTURE_ATOM: DocumentJSON = { type: 'counterCard', attrs: { title: 'Fixture', count: 1 } };

const INVALID_EXTENTS = [ 0,
    -1,
    1.5,
    Number.NaN,
    Number.POSITIVE_INFINITY ];

const SINGLE_SPAN = 1;

function children(node: DocumentJSON): DocumentJSON[] {
    return Array.isArray(node.content) ? (node.content as DocumentJSON[]) : [];
}

function span(cell: DocumentJSON, attr: 'colspan' | 'rowspan'): number {
    const value = (cell.attrs as Record<string, unknown> | undefined)?.[attr];

    return typeof value === 'number' ? value : SINGLE_SPAN;
}

function tablesIn(node: DocumentJSON, names: TableNodeNames): DocumentJSON[] {
    const nested = children(node).flatMap(child => tablesIn(child, names));

    return node.type === names.table ? [ node, ...nested ] : nested;
}

describe('example table fixtures', () => {
    describe.each(PRESETS)('%s naming preset', preset => {
        const names = TABLE_NODE_NAMES[preset];

        test.each(SIZES)('the %s plain table has its exact grid, header row, and cell text', size => {
            const { rows, columns } = PLAIN_TABLE_SIZES[size];
            const document = createPlainTable(rows, columns, names);

            expect(document.type).toBe('doc');
            const [ onlyTable, ...rest ] = children(document);
            expect(rest).toEqual([]);
            expect(onlyTable.type).toBe(names.table);

            const tableRows = children(onlyTable);
            expect(tableRows).toHaveLength(rows);
            let slots = 0;
            const offenders: string[] = [];

            tableRows.forEach((row, rowIndex) => {
                expect(row.type).toBe(names.row);
                const cells = children(row);
                expect(cells).toHaveLength(columns);
                const expectedType = rowIndex === 0 ? names.headerCell : names.cell;

                cells.forEach((cell, columnIndex) => {
                    slots += span(cell, 'colspan') * span(cell, 'rowspan');
                    const [ block, ...extraBlocks ] = children(cell);
                    const [ run, ...extraRuns ] = block === undefined ? [] : children(block);
                    const cellText = run?.text;

                    if (
                        cell.type !== expectedType ||
                        extraBlocks.length > 0 ||
                        block?.type !== 'paragraph' ||
                        extraRuns.length > 0 ||
                        run?.type !== 'text' ||
                        cellText !== TABLE_FIXTURE_CELL_TEXT ||
                        (cellText as string).length !== FIXTURE_TEXT_LENGTH
                    ) {
                        offenders.push(`(${rowIndex},${columnIndex}) ${JSON.stringify(cell)}`);
                    }
                });
            });

            expect(offenders.slice(0, 3)).toEqual([]);
            expect(slots).toBe(EXPECTED_SLOTS[size]);
            expect(slots).toBeLessThanOrEqual(DEFAULT_MAX_TABLE_GRID_SLOTS);
        });

        test('rich fixtures carry spans and an imported nested table under the same names', () => {
            const document = createRichTableDocument({
                names,
                imageUrl: RICH_FIXTURE_URL,
                atom: RICH_FIXTURE_ATOM,
                linkUrl: RICH_FIXTURE_URL,
            });

            const tables = tablesIn(document, names);
            expect(tables).toHaveLength(2);
            const cells = children(tables[0]).flatMap(children);
            expect(cells.some(cell => span(cell, 'colspan') > SINGLE_SPAN)).toBe(true);
            expect(cells.some(cell => span(cell, 'rowspan') > SINGLE_SPAN)).toBe(true);
            expect(cells.every(cell => cell.type === names.cell || cell.type === names.headerCell)).toBe(true);

            const rtl = withTableDirection(document, names, 'rtl');

            expect(tablesIn(rtl, names).map(table => (table.attrs as Record<string, unknown>)[TABLE_DIRECTION_ATTR]))
                .toEqual([ 'rtl', 'rtl' ]);

            expect(tablesIn(document, names).every(table => table.attrs === undefined)).toBe(true);
        });
    });

    test.each(INVALID_EXTENTS)('a plain table refuses the non-positive or fractional extent %p', extent => {
        const names = TABLE_NODE_NAMES.prosemirror;

        expect(() => createPlainTable(extent, 1, names)).toThrow(RangeError);
        expect(() => createPlainTable(1, extent, names)).toThrow(RangeError);
    });

    test('the irregular fixture keeps its raw row widths instead of normalizing them', () => {
        const names = TABLE_NODE_NAMES.prosemirror;
        const [ irregular ] = tablesIn(createIrregularTableDocument(names), names);

        const rowWidths = children(irregular).map(row =>
            children(row).reduce((width, cell) => width + span(cell, 'colspan'), 0));

        const rowspans = children(irregular).flatMap(children).map(cell => span(cell, 'rowspan'));

        expect(rowWidths).toEqual([ 3, 2, 4 ]);
        expect(Math.max(...rowspans)).toBeGreaterThan(children(irregular).length - 1);
    });
});
