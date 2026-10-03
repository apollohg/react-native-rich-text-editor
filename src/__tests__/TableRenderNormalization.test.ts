jest.mock('expo-modules-core', () => ({
    requireNativeModule: jest.fn(() => ({})),
}));

import {
    normalizeRenderBlocks,
    normalizeRenderPatch,
} from '../NativeEditorRenderNormalization';

const reference = { type: 'table', tableId: 'y17-42' };

test('accepts shallow table references in full snapshots and patches', () => {
    const blocks = [[reference]];
    expect(normalizeRenderBlocks(blocks)).toEqual(blocks);
    const patch = {
        baseDocumentVersion: '1',
        startIndex: 0,
        deleteCount: 1,
        renderBlocks: blocks,
    };
    expect(normalizeRenderPatch(patch)).toEqual(patch);
});

test.each([
    { type: 'table' },
    { type: 'table', tableId: 1 },
    { ...reference, cells: [] },
    { ...reference, rows: [] },
    { ...reference, table: {} },
])('rejects table payloads and malformed references: %j', (element) => {
    expect(normalizeRenderBlocks([[element]])).toBeNull();
});

test('rejects duplicate root table identities across blocks', () => {
    expect(normalizeRenderBlocks([[reference], [reference]])).toBeNull();
    expect(
        normalizeRenderBlocks([
            [reference],
            [{ type: 'table', tableId: 'y17-43' }],
        ]),
    ).not.toBeNull();
});

test('rejects table identity fields on prose elements', () => {
    expect(
        normalizeRenderBlocks([
            [
                {
                    type: 'textRun',
                    text: 'text',
                    marks: [],
                    tableId: reference.tableId,
                },
            ],
        ]),
    ).toBeNull();
});
