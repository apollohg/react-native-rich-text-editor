import assert from 'node:assert/strict';
import test from 'node:test';
import {
    call,
    exchangeUntilIdle,
    seedFrom,
    snapshot,
    tableFixture,
    withPeers,
} from '../controller.js';
import { canonicalDocumentShape } from '../assertions.js';
import { cell, cellAnchors, row, table } from '../table-schema.js';

const TABLE_START = 0;
const LOWER_LEFT_CELL = 1;
const LOWER_RIGHT_CELL = 2;
const FIRST_COLUMN_WIDTH = 120;
const SECOND_COLUMN_WIDTH = 140;
const RESIZED_COLUMN_WIDTH = 200;
const MERGED_SPAN = 2;

const SPANNING_TABLE = table([
    row([
        cell({
            colspan: MERGED_SPAN,
            colwidth: [FIRST_COLUMN_WIDTH, SECOND_COLUMN_WIDTH],
            text: 'a',
        }),
    ]),
    row([
        cell({ colwidth: [FIRST_COLUMN_WIDTH], text: 'c' }),
        cell({ colwidth: [SECOND_COLUMN_WIDTH], text: 'd' }),
    ]),
]);

type NativeStructuralEdit = {
    readonly name: string;
    readonly command: (anchors: readonly number[]) => Record<string, unknown>;
};

function anchorAt(anchors: readonly number[], index: number): number {
    const anchor = anchors[index];
    if (anchor === undefined) {
        throw new Error(`the spanning fixture exposes no cell anchor ${index}`);
    }
    return anchor;
}

const NATIVE_STRUCTURAL_EDITS: readonly NativeStructuralEdit[] = [
    {
        name: 'mergeTableCells',
        command: (anchors) => ({
            type: 'mergeTableCells',
            at: anchorAt(anchors, LOWER_LEFT_CELL),
            head: anchorAt(anchors, LOWER_RIGHT_CELL),
        }),
    },
    {
        name: 'setTableColumnWidth',
        command: (anchors) => ({
            type: 'setTableColumnWidth',
            width: RESIZED_COLUMN_WIDTH,
            at: anchorAt(anchors, LOWER_LEFT_CELL),
        }),
    },
    {
        name: 'addTableRow',
        command: (anchors) => ({
            type: 'addTableRow',
            side: 'after',
            at: anchorAt(anchors, LOWER_LEFT_CELL),
        }),
    },
    {
        name: 'addTableColumn',
        command: (anchors) => ({
            type: 'addTableColumn',
            side: 'after',
            at: anchorAt(anchors, LOWER_LEFT_CELL),
        }),
    },
];

async function documentOf(peer: Parameters<typeof snapshot>[0]): Promise<string> {
    return JSON.stringify(canonicalDocumentShape((await snapshot(peer)).documentJson));
}

async function nativeEditReachesTheWebPeer(edit: NativeStructuralEdit): Promise<void> {
    await withPeers(
        ['prosemirror', 'rust'] as const,
        async ([web, native]) => {
            await call(web, 'command', { type: 'insertNode', node: SPANNING_TABLE });
            await seedFrom(web, [native]);
            await exchangeUntilIdle([web, native]);
            const seeded = await documentOf(web);

            await call(
                native,
                'command',
                edit.command(cellAnchors(SPANNING_TABLE, TABLE_START)),
            );
            await exchangeUntilIdle([web, native]);

            const webDocument = await documentOf(web);
            assert.notEqual(
                webDocument,
                seeded,
                `native ${edit.name} left the web peer's document unchanged, so it proves nothing`,
            );
            assert.equal(
                webDocument,
                await documentOf(native),
                `the web peer disagrees with the native author after native ${edit.name}`,
            );
        },
        tableFixture('prosemirror'),
    );
}

test(
    'a stock web peer reads native structural edits over a spanning table',
    async (context) => {
        for (const edit of NATIVE_STRUCTURAL_EDITS) {
            await context.test(`the web peer reads native ${edit.name}`, async () => {
                await nativeEditReachesTheWebPeer(edit);
            });
        }
    },
);
