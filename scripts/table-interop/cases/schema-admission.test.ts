import assert from 'node:assert/strict';
import { test } from 'node:test';
import {
    withPeers,
    call,
    flushDocumentEvents,
    seedFrom,
    exchangeUntilIdle,
    snapshot,
    tableFixture,
} from '../controller.js';
import type { Peer } from '../peer-protocol.js';
import {
    CELL_NODE,
    PARAGRAPH_NODE,
    ROW_NODE,
    TABLE_NODE,
    TABLE_SCHEMA,
    geometryOf,
} from '../table-schema.js';

const TIPTAP_ROW_NODE = 'tableRow';
const TIPTAP_CELL_NODE = 'tableCell';
const FIRST_CELL_POSITION = 3;
const NO_NATIVE_REPAIR_WRITES = 0;
const NO_PENDING_NATIVE_UPDATES = 0;

function cell(cellNode: string, content: unknown[]): Record<string, unknown> {
    return {
        type: cellNode,
        attrs: { colspan: 1, rowspan: 1, colwidth: null },
        content,
    };
}

function paragraphCell(cellNode: string, text: string): Record<string, unknown> {
    return cell(cellNode, [{ type: PARAGRAPH_NODE, content: [{ type: 'text', text }] }]);
}

function row(rowNode: string, cells: unknown[]): Record<string, unknown> {
    return { type: rowNode, content: cells };
}

function regularTable(rowNode: string, cellNode: string): Record<string, unknown> {
    return {
        type: TABLE_NODE,
        content: [
            row(rowNode, [paragraphCell(cellNode, 'a'), paragraphCell(cellNode, 'b')]),
            row(rowNode, [paragraphCell(cellNode, 'c'), paragraphCell(cellNode, 'd')]),
        ],
    };
}

function raggedTable(): Record<string, unknown> {
    return {
        type: TABLE_NODE,
        content: [
            row(ROW_NODE, [paragraphCell(CELL_NODE, 'a'), paragraphCell(CELL_NODE, 'b')]),
            row(ROW_NODE, [paragraphCell(CELL_NODE, 'c')]),
        ],
    };
}

function documentTables(documentJson: Record<string, unknown> | null): Record<string, unknown>[] {
    const content = documentJson?.['content'];
    if (!Array.isArray(content)) {
        throw new Error('the peer document carried no content array');
    }
    return content.filter(
        (node): node is Record<string, unknown> =>
            typeof node === 'object' && node !== null && (node as Record<string, unknown>)['type'] === TABLE_NODE,
    );
}

async function projectionOf(peer: Peer, table: Record<string, unknown>): Promise<Record<string, unknown>> {
    return call(peer, 'projectTable', { schema: TABLE_SCHEMA, table });
}

test('TBL-10 a real tableEditing plugin repairs its own ragged insert before it reaches the CRDT', async () => {
    await withPeers(['prosemirror', 'rust'], async ([web, native]) => {
        await seedFrom(web, [native]);
        await call(web, 'command', { type: 'insertNode', node: raggedTable() });
        const inserted = await snapshot(web);

        assert.ok(
            inserted.normalizationPassesAfterLastAction > 0,
            'the real tableEditing plugin must append at least one fixTables transaction',
        );
        const [repaired] = documentTables(inserted.documentJson);
        assert.ok(repaired !== undefined, 'the web peer holds the inserted table');
        const projection = await projectionOf(web, repaired);
        assert.equal(projection['irregular'], false);
        assert.equal(projection['rows'], 2);
        assert.equal(projection['columns'], 2);

        await exchangeUntilIdle([web, native]);
        const admitted = await snapshot(native);
        assert.deepEqual(admitted.documentJson, inserted.documentJson);
        assert.equal(admitted.autonomousRepairWrites, NO_NATIVE_REPAIR_WRITES);
    }, tableFixture('prosemirror'));
});

test('TBL-10 a web-generated irregular merge lands in Rust exactly, with no local update of its own', async () => {
    await withPeers(['prosemirror', 'prosemirror', 'rust'], async ([first, second, native]) => {
        await call(first, 'command', { type: 'insertNode', node: regularTable(ROW_NODE, CELL_NODE) });
        await seedFrom(first, [second, native]);
        await exchangeUntilIdle([first, second, native]);
        assert.deepEqual((await snapshot(native)).documentJson, (await snapshot(first)).documentJson);
        await flushDocumentEvents(native);

        await call(first, 'command', {
            type: 'tableCommand',
            name: 'addRowAfter',
            at: FIRST_CELL_POSITION,
        });
        await call(second, 'command', {
            type: 'tableCommand',
            name: 'addColumnAfter',
            at: FIRST_CELL_POSITION,
        });
        const concurrent = [
            ...(await flushDocumentEvents(first)),
            ...(await flushDocumentEvents(second)),
        ];
        assert.ok(concurrent.length >= 2, 'both web peers produced concurrent table edits');

        for (const event of concurrent) {
            await call(native, 'applyUpdate', { updateBase64: event.bytesBase64 });
        }

        const merged = await snapshot(native);
        const [mergedTable] = documentTables(merged.documentJson);
        assert.ok(mergedTable !== undefined, 'the merged native document still holds the table');
        const projection = await projectionOf(native, mergedTable);
        assert.equal(
            projection['irregular'],
            true,
            'concurrent row and column insertion produces irregular geometry',
        );
        assert.equal(merged.autonomousRepairWrites, NO_NATIVE_REPAIR_WRITES);
        assert.equal(
            (await call(native, 'drain', {}))['count'],
            NO_PENDING_NATIVE_UPDATES,
            'admitting irregular geometry must emit no local document update',
        );

        const rawRows = mergedTable['content'];
        assert.ok(Array.isArray(rawRows));
        const widths = rawRows.map(candidate => {
            const cells = (candidate as Record<string, unknown>)['content'];
            return Array.isArray(cells) ? cells.length : 0;
        });
        assert.ok(
            new Set(widths).size > 1,
            `the raw CRDT rows stay ragged exactly as merged: ${widths.join(', ')}`,
        );
    }, tableFixture('prosemirror'));
});

test('TBL-10 every peer converges on the merged geometry and Rust agrees on its projection', async () => {
    await withPeers(['prosemirror', 'prosemirror', 'rust'], async ([first, second, native]) => {
        await call(first, 'command', { type: 'insertNode', node: regularTable(ROW_NODE, CELL_NODE) });
        await seedFrom(first, [second, native]);
        await exchangeUntilIdle([first, second, native]);

        await call(first, 'command', {
            type: 'tableCommand',
            name: 'addRowAfter',
            at: FIRST_CELL_POSITION,
        });
        await call(second, 'command', {
            type: 'tableCommand',
            name: 'addColumnAfter',
            at: FIRST_CELL_POSITION,
        });
        await exchangeUntilIdle([first, second, native]);

        const settled = await snapshot(native);
        assert.deepEqual(settled.documentJson, (await snapshot(first)).documentJson);
        assert.deepEqual(settled.documentJson, (await snapshot(second)).documentJson);
        assert.equal(
            settled.autonomousRepairWrites,
            NO_NATIVE_REPAIR_WRITES,
            'every repair in the room is web-origin; native writes none of its own',
        );
        const [settledTable] = documentTables(settled.documentJson);
        assert.ok(settledTable !== undefined);
        const nativeGeometry = geometryOf(await projectionOf(native, settledTable));
        assert.deepEqual(
            nativeGeometry,
            geometryOf(await projectionOf(first, settledTable)),
            'Rust and ProseMirror project the settled geometry identically',
        );
        assert.equal(
            nativeGeometry['irregular'],
            true,
            'real web peers settle on irregular geometry that native carries unchanged',
        );
    }, tableFixture('prosemirror'));
});

test('TBL-10 a real Tiptap table peer and Rust share the same table document', async () => {
    await withPeers(['tiptap', 'rust'], async ([web, native]) => {
        await seedFrom(web, [native]);
        await call(web, 'command', {
            type: 'insertNode',
            node: regularTable(TIPTAP_ROW_NODE, TIPTAP_CELL_NODE),
        });
        await exchangeUntilIdle([web, native]);

        const admitted = await snapshot(native);
        assert.deepEqual(admitted.documentJson, (await snapshot(web)).documentJson);
        assert.equal(admitted.autonomousRepairWrites, NO_NATIVE_REPAIR_WRITES);
        assert.equal(documentTables(admitted.documentJson).length, 1);
    }, tableFixture('tiptap'));
});
