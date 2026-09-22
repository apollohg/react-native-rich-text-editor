import './helpers/NativeEditorBridgeV2Fixture';
import { MOCK_ATOMIC_RENDER_SNAPSHOT } from './helpers/NativeEditorBridgeV2Fixture';
import {
    normalizeNativeEditorV2RenderUpdateValue,
    normalizeRenderBlocks,
} from '../NativeEditorRenderNormalization';

const normalize = (value: unknown) =>
    normalizeNativeEditorV2RenderUpdateValue(JSON.stringify(value));

const attrsKey = 'a'.repeat(64);
const paragraph = (text: string) => [
    { type: 'blockStart', nodeType: 'paragraph', depth: 0 },
    { type: 'textRun', text, marks: [] },
    { type: 'blockEnd' },
];
const region = {
    row: 0,
    column: 0,
    rowspan: 1,
    colspan: 1,
    header: false,
    attrsKey,
};
const block = (
    elementIndex: number,
    docStart: number,
    docEnd: number,
    start: number,
    end: number,
    breakEnd = end,
) => ({
    elementIndex,
    docStart,
    docEnd,
    scalarStart: start,
    contentScalarStart: start,
    scalarEnd: end,
    breakScalarEnd: breakEnd,
    void: false,
});

function fixture() {
    const table = (tablePos: number, sourceEnd: number, nested: boolean) => ({
        tablePos,
        sourceEnd,
        rows: 1,
        columns: 1,
        columnWidths: [null],
        direction: null,
        irregular: false,
        readOnlyDescendants: nested,
        attrsKey,
        sourceRows: [
            { sourcePos: tablePos + 1, sourceEnd: sourceEnd - 1, attrsKey },
        ],
        syntheticRegions: [],
        failure: null,
        compatibilityDiagnostic: null,
    });
    return {
        ...MOCK_ATOMIC_RENDER_SNAPSHOT,
        renderBlocks: [[{ type: 'table', tableId: 't0' }]],
        scalarLength: 6,
        tableAttributes: { [attrsKey]: '{}' },
        tableRecords: {
            t0: {
                ...table(0, 22, false),
                cells: [
                    {
                        ...region,
                        sourcePos: 2,
                        sourceEnd: 20,
                        contentKey: 'outer',
                        elements: [
                            ...paragraph('a'),
                            { type: 'table', tableId: 't6' },
                            ...paragraph('b'),
                        ],
                    },
                ],
            },
            t6: {
                ...table(6, 16, true),
                cells: [
                    {
                        ...region,
                        sourcePos: 8,
                        sourceEnd: 14,
                        contentKey: 'inner',
                        elements: paragraph('XY'),
                    },
                ],
            },
        },
        tableInputMappings: {
            version: 1,
            tables: {
                t0: {
                    extent: { scalarStart: 0, scalarEnd: 6 },
                    cells: [
                        {
                            cellIndex: 0,
                            sourcePos: 2,
                            sourceEnd: 20,
                            blocks: [
                                block(0, 4, 5, 0, 1, 2),
                                block(4, 17, 18, 5, 6),
                            ],
                            excluded: [
                                {
                                    elementIndex: 3,
                                    tableId: 't6',
                                    extent: { scalarStart: 2, scalarEnd: 4 },
                                },
                            ],
                        },
                    ],
                },
                t6: {
                    extent: { scalarStart: 2, scalarEnd: 4 },
                    cells: [
                        {
                            cellIndex: 0,
                            sourcePos: 8,
                            sourceEnd: 14,
                            blocks: [block(0, 10, 12, 2, 4)],
                            excluded: [],
                        },
                    ],
                },
            },
        },
    };
}

test('retains and deeply freezes snapshot-bound mappings and nested exclusions', () => {
    const value = fixture();
    const result = normalize(value);
    expect(result).toEqual(value);
    expect(
        Object.isFrozen(
            (result as any).tableInputMappings.tables.t0.cells[0].blocks[0],
        ),
    ).toBe(true);
});

test('accepts the complete mapping pool with an empty render patch', () => {
    const value = {
        ...fixture(),
        renderBlocks: null,
        renderPatch: {
            baseDocumentVersion: '4',
            startIndex: 0,
            deleteCount: 0,
            renderBlocks: [],
        },
    };
    expect(normalize(value)).toEqual(value);
});

test('keeps legacy table snapshots without input mappings valid', () => {
    const { tableInputMappings: _mapping, ...value } = fixture();
    expect(
        normalizeRenderBlocks(
            value.renderBlocks,
            value.tableAttributes,
            value.tableRecords,
        ),
    ).not.toBeNull();
    expect(normalize(value)).toEqual(value);
});

test('rejects an empty mapping sidecar on a table-free snapshot', () => {
    const value = {
        ...MOCK_ATOMIC_RENDER_SNAPSHOT,
        tableAttributes: {},
        tableRecords: {},
        tableInputMappings: { version: 1, tables: {} },
    };
    expect(normalize(value)).toBeNull();
});

test('maps only leaf blocks under list containers and retains prefix coordinates', () => {
    const value: any = fixture();
    delete value.tableRecords.t6;
    delete value.tableInputMappings.tables.t6;
    value.scalarLength = 3;
    value.tableRecords.t0.sourceEnd = 13;
    value.tableRecords.t0.sourceRows[0].sourceEnd = 12;
    const cell = value.tableRecords.t0.cells[0];
    cell.sourceEnd = 11;
    cell.elements = [
        { type: 'blockStart', nodeType: 'customList', depth: 0 },
        { type: 'blockStart', nodeType: 'customItem', depth: 1 },
        ...paragraph('a'),
        { type: 'blockEnd' },
        { type: 'blockEnd' },
    ];
    value.tableInputMappings.tables.t0 = {
        extent: { scalarStart: 0, scalarEnd: 3 },
        cells: [
            {
                cellIndex: 0,
                sourcePos: 2,
                sourceEnd: 11,
                blocks: [{ ...block(2, 6, 7, 0, 3), contentScalarStart: 2 }],
                excluded: [],
            },
        ],
    };
    expect(normalize(value)).toEqual(value);
});

test('retains a zero-leaf nested exclusion without inventing a scalar span', () => {
    const value: any = fixture();
    value.tableRecords.t6.cells[0].elements = [];
    value.tableInputMappings.tables.t6.extent = null;
    value.tableInputMappings.tables.t6.cells[0].blocks = [];
    value.tableInputMappings.tables.t0.cells[0].excluded[0].extent = null;
    value.tableInputMappings.tables.t0.cells[0].blocks[1] = block(
        4,
        17,
        18,
        2,
        3,
    );
    value.tableInputMappings.tables.t0.extent.scalarEnd = 3;
    value.scalarLength = 3;
    expect(normalize(value)).toEqual(value);
});

test.each<[string, (value: any) => void]>([
    [
        'unknown version',
        (v) => {
            v.tableInputMappings.version = 2;
        },
    ],
    [
        'unknown mapping field',
        (v) => {
            v.tableInputMappings.epoch = '1';
        },
    ],
    [
        'null mapping',
        (v) => {
            v.tableInputMappings = null;
        },
    ],
    [
        'missing table',
        (v) => {
            delete v.tableInputMappings.tables.t6;
        },
    ],
    [
        'orphan table',
        (v) => {
            v.tableInputMappings.tables.t99 = v.tableInputMappings.tables.t6;
        },
    ],
    [
        'missing cell',
        (v) => {
            v.tableInputMappings.tables.t0.cells = [];
        },
    ],
    [
        'wrong cell identity',
        (v) => {
            v.tableInputMappings.tables.t0.cells[0].sourcePos = 3;
        },
    ],
    [
        'wrong cell index',
        (v) => {
            v.tableInputMappings.tables.t0.cells[0].cellIndex = 1;
        },
    ],
    [
        'fractional coordinate',
        (v) => {
            v.tableInputMappings.tables.t0.cells[0].blocks[0].docStart = 4.5;
        },
    ],
    [
        'out-of-range scalar',
        (v) => {
            v.tableInputMappings.tables.t0.extent.scalarEnd = 7;
        },
    ],
    [
        'text run as block anchor',
        (v) => {
            v.tableInputMappings.tables.t0.cells[0].blocks[0].elementIndex = 1;
        },
    ],
    [
        'void flag on paragraph',
        (v) => {
            v.tableInputMappings.tables.t0.cells[0].blocks[0].void = true;
        },
    ],
    [
        'doc boundary outside cell',
        (v) => {
            v.tableInputMappings.tables.t0.cells[0].blocks[0].docStart = 2;
        },
    ],
    [
        'prefix after content',
        (v) => {
            v.tableInputMappings.tables.t0.cells[0].blocks[0].contentScalarStart = 2;
        },
    ],
    [
        'break beyond one separator',
        (v) => {
            v.tableInputMappings.tables.t0.cells[0].blocks[0].breakScalarEnd = 3;
        },
    ],
    [
        'duplicate block',
        (v) => {
            v.tableInputMappings.tables.t0.cells[0].blocks.push(
                v.tableInputMappings.tables.t0.cells[0].blocks[1],
            );
        },
    ],
    [
        'overlapping nested extent',
        (v) => {
            v.tableInputMappings.tables.t0.cells[0].blocks[0].scalarEnd = 3;
            v.tableInputMappings.tables.t0.cells[0].blocks[0].breakScalarEnd = 3;
        },
    ],
    [
        'missing exclusion',
        (v) => {
            v.tableInputMappings.tables.t0.cells[0].excluded = [];
        },
    ],
    [
        'mismatched exclusion',
        (v) => {
            v.tableInputMappings.tables.t0.cells[0].excluded[0].extent.scalarStart = 3;
        },
    ],
    [
        'null extent with leaves',
        (v) => {
            v.tableInputMappings.tables.t0.extent = null;
        },
    ],
])('rejects %s', (_name, mutate) => {
    const value = fixture();
    mutate(value);
    expect(normalize(value)).toBeNull();
});

test('rejects unchecked malformed table records even on an empty patch', () => {
    const value: any = fixture();
    value.renderBlocks = null;
    value.renderPatch = {
        baseDocumentVersion: '4',
        startIndex: 0,
        deleteCount: 0,
        renderBlocks: [],
    };
    value.tableRecords.t6.cells = null;
    expect(() => normalize(value)).not.toThrow();
    expect(normalize(value)).toBeNull();
});

test('rejects overlapping scalar extents for separate root tables', () => {
    const value: any = fixture();
    value.renderBlocks.push([{ type: 'table', tableId: 't22' }]);
    value.tableRecords.t22 = {
        ...value.tableRecords.t6,
        tablePos: 22,
        sourceEnd: 32,
        readOnlyDescendants: false,
        sourceRows: [{ sourcePos: 23, sourceEnd: 31, attrsKey }],
        cells: [
            { ...value.tableRecords.t6.cells[0], sourcePos: 24, sourceEnd: 30 },
        ],
    };
    value.tableInputMappings.tables.t22 = {
        extent: { scalarStart: 0, scalarEnd: 2 },
        cells: [
            {
                cellIndex: 0,
                sourcePos: 24,
                sourceEnd: 30,
                blocks: [block(0, 26, 28, 0, 2)],
                excluded: [],
            },
        ],
    };
    expect(normalize(value)).toBeNull();
});
