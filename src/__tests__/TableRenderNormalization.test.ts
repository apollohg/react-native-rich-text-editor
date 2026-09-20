jest.mock('expo-modules-core', () => ({
    requireNativeModule: jest.fn(() => ({})),
}));

import { normalizeRenderBlocks as normalizeWithPool } from '../NativeEditorRenderNormalization';

const attrsKey =
    'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa';
const normalizeRenderBlocks = (
    value: unknown,
    pool: unknown = { [attrsKey]: '{}' },
    records: unknown = {},
) => normalizeWithPool(value, pool, records);

const table = () => ({
    tablePos: 0,
    sourceEnd: 10,
    rows: 1,
    columns: 2,
    columnWidths: [null, 120],
    direction: null,
    irregular: false,
    readOnlyDescendants: false,
    attrsKey,
    sourceRows: [{ sourcePos: 1, sourceEnd: 9, attrsKey }],
    syntheticRegions: [],
    failure: null,
    compatibilityDiagnostic: null,
    cells: [
        {
            sourcePos: 2,
            sourceEnd: 8,
            row: 0,
            column: 0,
            rowspan: 1,
            colspan: 2,
            header: true,
            attrsKey,
            contentKey: 'stable',
            elements: [
                { type: 'blockStart', nodeType: 'paragraph', depth: 0 },
                { type: 'textRun', text: 'Hi', marks: [] },
                { type: 'blockEnd' },
            ],
        },
    ],
});

const blocks = (_record: unknown) => [[{ type: 'table', tableId: 't0' }]];
const withRecord = (record: unknown) =>
    normalizeRenderBlocks(blocks(record), { [attrsKey]: '{}' }, { t0: record });
const flatBlocks = () => [[{ type: 'table', tableId: 't0' }]];
const flatRecords = () => ({ t0: table() });

test('accepts flat table records and shallow table references', () => {
    expect(
        (normalizeRenderBlocks as Function)(
            flatBlocks(),
            { [attrsKey]: '{}' },
            flatRecords(),
        ),
    ).toEqual(flatBlocks());
});

test('rejects dangling, aliased, and unreachable flat table records', () => {
    expect(
        (normalizeRenderBlocks as Function)(
            [[{ type: 'table', tableId: 't1' }]],
            { [attrsKey]: '{}' },
            flatRecords(),
        ),
    ).toBeNull();
    expect(
        (normalizeRenderBlocks as Function)(
            [
                [
                    { type: 'table', tableId: 't0' },
                    { type: 'table', tableId: 't0' },
                ],
            ],
            { [attrsKey]: '{}' },
            flatRecords(),
        ),
    ).toBeNull();
    expect(
        (normalizeRenderBlocks as Function)(
            flatBlocks(),
            { [attrsKey]: '{}' },
            { ...flatRecords(), t1: table() },
        ),
    ).toBeNull();
});

test('resolves one snapshot pool without expanding attributes into cells', () => {
    const record = table() as unknown as Record<string, any>;
    delete record.attrsJson;
    const shared =
        'bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb';
    record.attrsKey = shared;
    for (const entry of [...record.sourceRows, ...record.cells]) {
        delete entry.attrsJson;
        entry.attrsKey = shared;
    }
    const pool = { [shared]: '{"opaque":{"enabled":true}}' };
    expect(
        (normalizeRenderBlocks as Function)(blocks(record), pool, {
            t0: record,
        }),
    ).toEqual(blocks(record));
    expect(
        (normalizeRenderBlocks as Function)(blocks(record), {}, { t0: record }),
    ).toBeNull();
});

test('retains a bounded semantic table as one outer element', () => {
    const value = blocks(table());
    expect(withRecord(table())).toEqual(value);
});

test.each([NaN, Infinity, -Infinity, -1, 0])(
    'rejects invalid width %s',
    (width) => {
        expect(
            withRecord({ ...table(), columnWidths: [null, width] }),
        ).toBeNull();
    },
);

test('rejects unknown status, impossible rectangles and invented synthetic anchors', () => {
    expect(withRecord({ ...table(), failure: 'unknown' })).toBeNull();
    expect(
        withRecord({ ...table(), compatibilityDiagnostic: 'fallback' }),
    ).toBeNull();
    const outOfRange = table();
    outOfRange.cells[0].column = 1;
    expect(withRecord(outOfRange)).toBeNull();
    expect(
        withRecord({
            ...table(),
            syntheticRegions: [
                {
                    row: 0,
                    column: 0,
                    rowspan: 1,
                    colspan: 1,
                    header: false,
                    attrsJson: '{}',
                    sourcePos: 4,
                },
            ],
        }),
    ).toBeNull();
});

test('rejects cyclic recursive records without overflowing the stack', () => {
    const value = table();
    (value.cells[0].elements as unknown[]).push({
        type: 'table',
        tableId: 't0',
    });
    expect(
        normalizeRenderBlocks(
            blocks(value),
            { [attrsKey]: '{}' },
            { t0: value },
        ),
    ).toBeNull();
});

test('requires a table payload only for the table discriminator', () => {
    expect(normalizeRenderBlocks([[{ type: 'table' }]])).toBeNull();
    expect(
        normalizeRenderBlocks(
            [[{ type: 'blockEnd', tableId: 't0' }]],
            { [attrsKey]: '{}' },
            { t0: table() },
        ),
    ).toBeNull();
});

test('rejects overflowing JSON attribute numbers and overlapping raw cell ranges', () => {
    expect(
        normalizeRenderBlocks(
            blocks(table()),
            { [attrsKey]: '{"size":1e309}' },
            { t0: table() },
        ),
    ).toBeNull();
    const value = table();
    value.cells[0].colspan = 1;
    value.cells.push({
        ...value.cells[0],
        column: 1,
        sourcePos: 3,
        sourceEnd: 7,
    });
    expect(withRecord(value)).toBeNull();
});

test('rejects noncanonical ids and unused attribute pool entries', () => {
    expect(
        normalizeRenderBlocks(
            [[{ type: 'table', tableId: 'table0' }]],
            { [attrsKey]: '{}' },
            { table0: table() },
        ),
    ).toBeNull();
    expect(
        normalizeRenderBlocks(
            flatBlocks(),
            {
                [attrsKey]: '{}',
                bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb:
                    '{"unused":true}',
            },
            flatRecords(),
        ),
    ).toBeNull();
});

test('counts UTF-8 attributes without accepting lone surrogates', () => {
    const key =
        'cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc';
    const record = table() as any;
    record.attrsKey = key;
    for (const entry of [...record.sourceRows, ...record.cells])
        entry.attrsKey = key;
    expect(
        normalizeRenderBlocks(
            blocks(record),
            { [key]: '{"label":"é"}' },
            { t0: record },
        ),
    ).toEqual(blocks(record));
    expect(
        normalizeRenderBlocks(
            blocks(record),
            { [key]: '{"label":"\ud800"}' },
            { t0: record },
        ),
    ).toBeNull();
});
