import type { EditorState } from 'prosemirror-state';
import type { Node } from 'prosemirror-model';
import { selectedRect, tableNodeTypes } from 'prosemirror-tables';
import type { JsonNode } from '../peer-protocol.js';

export interface RowInsertionObservation {
    command: Record<string, unknown>;
    document: JsonNode;
    table: JsonNode;
    tableStart: number;
    selection: { anchor: number; head: number };
    width: number;
    height: number;
    map: number[];
    row: number;
    referenceRow: number | null;
    slots: {
        column: number;
        index: number;
        branch: 'span' | 'create';
        position: number | null;
        role: string | null;
        colspan: number | null;
    }[];
}

export function observeRowInsertion(
    state: EditorState,
    command: Record<string, unknown>,
): RowInsertionObservation {
    let bytes = 0,
        items = 0;
    const charge = (size: number, depth: number) => {
        bytes += size;
        if (++items > 65_536 || bytes > 16_777_216 || depth > 64)
            throw new Error('row insertion observation budget');
    };
    const value = (input: unknown, depth: number): void => {
        charge(typeof input === 'string' ? 128 + input.length * 12 : 128, depth);
        if (input && typeof input === 'object')
            for (const key in input) {
                charge(key.length * 12, depth);
                value((input as Record<string, unknown>)[key], depth + 1);
            }
    };
    const visit = (node: Node, depth: number): void => {
        charge(512 + (node.text?.length ?? 0) * 12, depth);
        value(node.attrs, depth);
        node.marks.forEach((mark) => value(mark.attrs, depth));
        if (node.type.spec.tableRole === 'table') {
            let columns = 0;
            node.forEach((row) =>
                row.forEach((cell) => {
                    charge(0, depth);
                    const colspan = Number(cell.attrs['colspan']),
                        rowspan = Number(cell.attrs['rowspan']);
                    if (
                        !Number.isSafeInteger(colspan) ||
                        colspan < 1 ||
                        !Number.isSafeInteger(rowspan) ||
                        rowspan < 1 ||
                        rowspan > 65_536
                    )
                        throw new Error('row insertion observation budget');
                    columns += colspan;
                }),
            );
            if (columns * node.childCount > 65_536)
                throw new Error('row insertion observation budget');
        }
        node.forEach((child) => visit(child, depth + 1));
    };
    visit(state.doc, 0);
    value(command, 0);
    const { map, table, tableStart, bottom: row } = selectedRect(state);
    if (map.map.length > 65_536) throw new Error('row insertion map budget');
    let referenceRow: number | null = row > 0 ? row - 1 : 0;
    const header = Array.from(
        { length: map.width },
        (_, column) =>
            table.nodeAt(map.map[column + referenceRow! * map.width]!)?.type ===
            tableNodeTypes(table.type.schema).header_cell,
    ).every(Boolean);
    if (header) referenceRow = row === 0 || row === map.height ? null : row;
    const slots: RowInsertionObservation['slots'] = [];
    for (let column = 0, index = map.width * row; column < map.width; column++, index++) {
        const span = row > 0 && row < map.height && map.map[index] === map.map[index - map.width];
        const position = span
            ? map.map[index]!
            : referenceRow === null
              ? null
              : map.map[index + (referenceRow - row) * map.width]!;
        const node = position === null ? null : table.nodeAt(position);
        const colspan = span ? Number(node?.attrs['colspan']) : 1;
        slots.push({
            column,
            index,
            branch: span ? 'span' : 'create',
            position,
            role: node?.type.spec.tableRole ?? (referenceRow === null ? 'cell' : null),
            colspan: Number.isFinite(colspan) ? colspan : null,
        });
        if (span) {
            if (!Number.isInteger(colspan) || colspan < 1) break;
            column += colspan - 1;
        }
    }
    return {
        command: structuredClone(command),
        document: state.doc.toJSON(),
        table: table.toJSON(),
        tableStart,
        selection: { anchor: state.selection.anchor, head: state.selection.head },
        width: map.width,
        height: map.height,
        map: [...map.map],
        row,
        referenceRow,
        slots,
    };
}
