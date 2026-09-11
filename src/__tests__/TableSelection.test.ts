jest.mock('expo-modules-core', () => ({
    requireNativeModule: () => ({}),
}));

import { assertCellPosition, type TableCellSelection } from '../TableTypes';
import { normalizeRenderSelection } from '../NativeEditorRenderNormalization';
import { normalizeNativeEditorV2PeersValue } from '../NativeEditorResultNormalization';
import { parseSelectionFromUpdate } from '../RichTextEditorSerialization';
import { atomSelected } from '../atomInstances';

const ANCHOR_CELL = 2;
const HEAD_CELL = 16;
const MAX_CELL_POSITION = 0xffff_ffff;
const INVALID_CELL_POSITION_MESSAGE = 'Invalid table cell position';
const CLIENT_ID = '7';
const CLOCK = 3;

const CELL_SELECTION: TableCellSelection = {
    type: 'cell',
    anchorCell: ANCHOR_CELL,
    headCell: HEAD_CELL,
};

function peerPayload(cellRectangle: unknown): unknown {
    return {
        peers: [
            {
                clientId: CLIENT_ID,
                clock: CLOCK,
                isLocal: false,
                state: { focused: true },
                cursor: { anchor: ANCHOR_CELL, head: HEAD_CELL },
                cellRectangle,
            },
        ],
    };
}

describe('assertCellPosition', () => {
    it('accepts every representable cell opening position', () => {
        expect(() => {
            assertCellPosition(0);
        }).not.toThrow();
        expect(() => {
            assertCellPosition(MAX_CELL_POSITION);
        }).not.toThrow();
    });

    it('rejects negative, fractional, oversized and non-finite positions', () => {
        for (const value of [ -1, 0.5, MAX_CELL_POSITION + 1, Number.NaN, Number.POSITIVE_INFINITY ]) {
            expect(() => {
                assertCellPosition(value);
            }).toThrow(INVALID_CELL_POSITION_MESSAGE);
        }
    });
});

describe('normalizeRenderSelection', () => {
    it('normalizes a cell selection into document cell openings', () => {
        expect(
            normalizeRenderSelection({
                type: 'cell',
                anchorCell: ANCHOR_CELL,
                headCell: HEAD_CELL,
            })
        ).toEqual(CELL_SELECTION);
    });

    it('preserves anchor and head direction for a reversed rectangle', () => {
        expect(
            normalizeRenderSelection({
                type: 'cell',
                anchorCell: HEAD_CELL,
                headCell: ANCHOR_CELL,
            })
        ).toEqual({ type: 'cell', anchorCell: HEAD_CELL, headCell: ANCHOR_CELL });
    });

    it('rejects a cell selection carrying text or scalar keys', () => {
        expect(
            normalizeRenderSelection({
                type: 'cell',
                anchorCell: ANCHOR_CELL,
                headCell: HEAD_CELL,
                anchorScalar: 0,
            })
        ).toBeNull();
        expect(
            normalizeRenderSelection({ type: 'cell', anchor: ANCHOR_CELL, head: HEAD_CELL })
        ).toBeNull();
    });

    it('rejects a cell selection whose positions are not u32 values', () => {
        expect(
            normalizeRenderSelection({ type: 'cell', anchorCell: -1, headCell: HEAD_CELL })
        ).toBeNull();
        expect(
            normalizeRenderSelection({ type: 'cell', anchorCell: ANCHOR_CELL, headCell: '16' })
        ).toBeNull();
    });

    it('leaves text, node and all selections unchanged', () => {
        expect(
            normalizeRenderSelection({
                type: 'text',
                anchor: 1,
                head: 4,
                anchorScalar: 0,
                headScalar: 3,
            })
        ).toEqual({ type: 'text', anchor: 1, head: 4, anchorScalar: 0, headScalar: 3 });
        expect(normalizeRenderSelection({ type: 'node', pos: 5, posScalar: 4 })).toEqual({
            type: 'node',
            pos: 5,
            posScalar: 4,
        });
        expect(normalizeRenderSelection({ type: 'all' })).toEqual({ type: 'all' });
    });
});

describe('parseSelectionFromUpdate', () => {
    it('parses a cell selection from an update payload', () => {
        expect(parseSelectionFromUpdate(CELL_SELECTION)).toEqual(CELL_SELECTION);
    });

    it('rejects a cell selection with a missing head cell', () => {
        expect(parseSelectionFromUpdate({ type: 'cell', anchorCell: ANCHOR_CELL })).toBeNull();
    });
});

describe('atomSelected', () => {
    it('never reports an atom as selected by a cell rectangle', () => {
        expect(atomSelected(CELL_SELECTION, ANCHOR_CELL)).toBe(false);
        expect(atomSelected(CELL_SELECTION, HEAD_CELL)).toBe(false);
    });

    it('still reports an atom selected by a node selection', () => {
        expect(atomSelected({ type: 'node', pos: ANCHOR_CELL }, ANCHOR_CELL)).toBe(true);
    });
});

describe('normalizeNativeEditorV2PeersValue', () => {
    it('projects a peer cell rectangle beside its standard cursor', () => {
        const peers = normalizeNativeEditorV2PeersValue(
            peerPayload({ anchorCell: ANCHOR_CELL, headCell: HEAD_CELL })
        );

        expect(peers).toHaveLength(1);
        expect(peers?.[0]?.cursor).toEqual({ anchor: ANCHOR_CELL, head: HEAD_CELL });
        expect(peers?.[0]?.cellRectangle).toEqual({
            anchorCell: ANCHOR_CELL,
            headCell: HEAD_CELL,
        });
    });

    it('keeps the standard cursor when the engine dropped the rectangle', () => {
        const peers = normalizeNativeEditorV2PeersValue(peerPayload(null));

        expect(peers?.[0]?.cursor).toEqual({ anchor: ANCHOR_CELL, head: HEAD_CELL });
        expect(peers?.[0]?.cellRectangle).toBeNull();
    });

    it('rejects a peer whose rectangle is not a pair of u32 cell openings', () => {
        expect(normalizeNativeEditorV2PeersValue(peerPayload({ anchorCell: -1 }))).toBeNull();
        expect(normalizeNativeEditorV2PeersValue(peerPayload('cell'))).toBeNull();
    });
});
