import assert from 'node:assert/strict';
import { test } from 'node:test';
import {
    applyAwareness,
    awarenessPeers,
    call,
    exchangeUntilIdle,
    seedFrom,
    setAwareness,
    snapshot,
    tableFixture,
    withPeers,
} from '../controller.js';
import type { AwarenessPeerProjection } from '../controller.js';
import type { Peer } from '../peer-protocol.js';
import { CELL_NODE, PARAGRAPH_NODE, ROW_NODE, TABLE_NODE } from '../table-schema.js';

const TEXT_NODE = 'text';
const NODE_OPENING_TOKENS = 2;
const VOID_NODE_SIZE = 1;
const TOP_LEFT = 0;
const TOP_RIGHT = 1;
const BOTTOM_LEFT = 2;
const BOTTOM_RIGHT = 3;
const CELL_TEXTS = ['alpha', 'beta', 'gamma', 'delta'];
const CELL_TEXT_OFFSET = 2;
const INSERTED_TEXT = 'xy';
const UNKNOWN_EXTENSION_VERSION = 2;
const AWARENESS_CELL_RECTANGLE_KEY = 'nativeEditorTableSelection';
const NOT_BASE64 = 'not base64!!';
const ONE_AWARENESS_UPDATE = 1;

function cell(text: string): Record<string, unknown> {
    return {
        type: CELL_NODE,
        attrs: { colspan: 1, rowspan: 1, colwidth: null },
        content: [{ type: PARAGRAPH_NODE, content: [{ type: TEXT_NODE, text }] }],
    };
}

function regularTable(): Record<string, unknown> {
    return {
        type: TABLE_NODE,
        content: [
            {
                type: ROW_NODE,
                content: [cell(CELL_TEXTS[TOP_LEFT] ?? ''), cell(CELL_TEXTS[TOP_RIGHT] ?? '')],
            },
            {
                type: ROW_NODE,
                content: [cell(CELL_TEXTS[BOTTOM_LEFT] ?? ''), cell(CELL_TEXTS[BOTTOM_RIGHT] ?? '')],
            },
        ],
    };
}

function nodeSize(node: Record<string, unknown>): number {
    if (node['type'] === TEXT_NODE) {
        return [...String(node['text'] ?? '')].length;
    }
    const content = node['content'];
    if (!Array.isArray(content)) {
        return VOID_NODE_SIZE;
    }
    return content.reduce<number>(
        (size, child) => size + nodeSize(child as Record<string, unknown>),
        NODE_OPENING_TOKENS,
    );
}

function cellOpenings(documentJson: Record<string, unknown> | null): number[] {
    const content = documentJson?.['content'];
    if (!Array.isArray(content)) {
        throw new Error('the peer document carried no content array');
    }
    const openings: number[] = [];
    let position = 0;
    for (const child of content as Record<string, unknown>[]) {
        if (child['type'] === TABLE_NODE) {
            let rowPosition = position + VOID_NODE_SIZE;
            for (const row of (child['content'] ?? []) as Record<string, unknown>[]) {
                let cellPosition = rowPosition + VOID_NODE_SIZE;
                for (const cellNode of (row['content'] ?? []) as Record<string, unknown>[]) {
                    openings.push(cellPosition);
                    cellPosition += nodeSize(cellNode);
                }
                rowPosition += nodeSize(row);
            }
        }
        position += nodeSize(child);
    }
    if (openings.length === 0) {
        throw new Error('the peer document held no table cells');
    }
    return openings;
}

function remotePeer(peers: AwarenessPeerProjection[]): AwarenessPeerProjection {
    const remote = peers.find((peer) => !peer.isLocal);
    if (remote === undefined) {
        throw new Error(`no remote awareness peer among ${JSON.stringify(peers)}`);
    }
    return remote;
}

async function seedTable(web: Peer, native: Peer): Promise<number[]> {
    await seedFrom(web, [native]);
    await call(web, 'command', { type: 'insertNode', node: regularTable() });
    await exchangeUntilIdle([web, native]);
    const shared = await snapshot(native);
    assert.deepEqual(shared.documentJson, (await snapshot(web)).documentJson);
    return cellOpenings(shared.documentJson);
}

async function publishNativeRectangle(
    native: Peer,
    anchorCell: number,
    headCell: number,
): Promise<string> {
    const events = await setAwareness(native, {
        state: { user: 'native' },
        focused: true,
        selection: { type: 'cell', anchorCell, headCell },
    });
    assert.equal(events.length, ONE_AWARENESS_UPDATE);
    const [update] = events;
    if (update === undefined) {
        throw new Error('the native peer published no awareness update');
    }
    return update.bytesBase64;
}

test('TBL-14 a native real-cell rectangle reaches a stock y-prosemirror peer as the same cells', async () => {
    await withTablePeers(async (web, native) => {
        const cells = await seedTable(web, native);
        const anchorCell = cells[TOP_LEFT] ?? 0;
        const headCell = cells[BOTTOM_RIGHT] ?? 0;

        const update = await publishNativeRectangle(native, anchorCell, headCell);
        const documentBefore = await snapshot(web);
        await applyAwareness(web, update);
        const documentAfter = await snapshot(web);

        assert.equal(
            documentAfter.documentRevision,
            documentBefore.documentRevision,
            'awareness must never produce a document event',
        );
        const remote = remotePeer(await awarenessPeers(web));
        assert.deepEqual(remote.cellRectangle, { anchorCell, headCell });
        assert.notEqual(remote.cursor, null, 'the standard cursor fallback travels with it');
    });
});

test('TBL-14 a native rectangle keeps the same cells after an unrelated earlier insertion', async () => {
    await withTablePeers(async (web, native) => {
        const cells = await seedTable(web, native);
        const anchorCell = cells[TOP_RIGHT] ?? 0;
        const headCell = cells[BOTTOM_LEFT] ?? 0;
        const update = await publishNativeRectangle(native, anchorCell, headCell);
        await applyAwareness(web, update);

        await call(web, 'command', {
            type: 'tableCommand',
            name: 'addRowAfter',
            at: (cells[BOTTOM_LEFT] ?? 0) + CELL_TEXT_OFFSET,
        });
        await exchangeUntilIdle([web, native]);

        const shifted = cellOpenings((await snapshot(web)).documentJson);
        const remote = remotePeer(await awarenessPeers(web));
        assert.deepEqual(remote.cellRectangle, {
            anchorCell: shifted[TOP_RIGHT] ?? 0,
            headCell: shifted[BOTTOM_LEFT] ?? 0,
        });
    });
});

test('TBL-14 a stock web text cursor reaches the engine with no rectangle', async () => {
    await withTablePeers(async (web, native) => {
        const cells = await seedTable(web, native);
        const anchor = (cells[TOP_LEFT] ?? 0) + CELL_TEXT_OFFSET;
        const head = anchor + INSERTED_TEXT.length;

        const [update] = await setAwareness(web, {
            state: { user: 'web' },
            selection: { type: 'text', anchor, head },
        });
        if (update === undefined) {
            throw new Error('the web peer published no awareness update');
        }
        await applyAwareness(native, update.bytesBase64);

        const remote = remotePeer(await awarenessPeers(native));
        assert.deepEqual(remote.cursor, { anchor, head });
        assert.equal(remote.cellRectangle, null);
    });
});

test('TBL-14 a web-published rectangle reaches the engine as the same real cells', async () => {
    await withTablePeers(async (web, native) => {
        const cells = await seedTable(web, native);
        const anchorCell = cells[TOP_LEFT] ?? 0;
        const headCell = cells[BOTTOM_RIGHT] ?? 0;

        const [update] = await setAwareness(web, {
            state: { user: 'web' },
            selection: { type: 'cell', anchorCell, headCell },
        });
        if (update === undefined) {
            throw new Error('the web peer published no awareness update');
        }
        await applyAwareness(native, update.bytesBase64);

        const remote = remotePeer(await awarenessPeers(native));
        assert.deepEqual(remote.cellRectangle, { anchorCell, headCell });
        assert.notEqual(remote.cursor, null);
    });
});

test('TBL-14 an unknown extension version drops only the rectangle', async () => {
    await withTablePeers(async (web, native) => {
        const cells = await seedTable(web, native);
        const anchor = (cells[TOP_LEFT] ?? 0) + CELL_TEXT_OFFSET;

        const [update] = await setAwareness(web, {
            state: {
                user: 'web',
                [AWARENESS_CELL_RECTANGLE_KEY]: {
                    version: UNKNOWN_EXTENSION_VERSION,
                    anchor: 'AAAA',
                    head: 'AAAA',
                },
            },
            selection: { type: 'text', anchor, head: anchor },
        });
        if (update === undefined) {
            throw new Error('the web peer published no awareness update');
        }
        await applyAwareness(native, update.bytesBase64);

        const remote = remotePeer(await awarenessPeers(native));
        assert.deepEqual(remote.cursor, { anchor, head: anchor });
        assert.equal(remote.cellRectangle, null);
        assert.equal(remote.state['user'], 'web', 'the rest of the peer state survives');
    });
});

test('TBL-14 malformed extension metadata drops only the rectangle', async () => {
    await withTablePeers(async (web, native) => {
        const cells = await seedTable(web, native);
        const anchor = (cells[TOP_LEFT] ?? 0) + CELL_TEXT_OFFSET;

        const [update] = await setAwareness(web, {
            state: {
                user: 'web',
                [AWARENESS_CELL_RECTANGLE_KEY]: {
                    version: 1,
                    anchor: NOT_BASE64,
                    head: NOT_BASE64,
                },
            },
            selection: { type: 'text', anchor, head: anchor },
        });
        if (update === undefined) {
            throw new Error('the web peer published no awareness update');
        }
        await applyAwareness(native, update.bytesBase64);

        const remote = remotePeer(await awarenessPeers(native));
        assert.deepEqual(remote.cursor, { anchor, head: anchor });
        assert.equal(remote.cellRectangle, null);
        assert.equal(remote.state['user'], 'web');
    });
});

async function withTablePeers(
    body: (web: Peer, native: Peer) => Promise<void>,
): Promise<void> {
    await withPeers(
        ['prosemirror', 'rust'] as const,
        async ([web, native]) => {
            await body(web, native);
        },
        tableFixture('prosemirror'),
    );
}
