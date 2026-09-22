import {
    hasExactOwnKeys,
    isPlainRecord,
    nativeEditorV2U32,
} from './NativeEditorResultNormalization';
import type {
    TableInputExtent,
    TableInputMappings,
    TableRenderRecord,
} from './TableTypes';

const u32 = (value: unknown): value is number =>
    nativeEditorV2U32(value) !== null;

function validExtent(
    value: unknown,
    scalarLength: number,
): value is TableInputExtent | null {
    return (
        value === null ||
        (isPlainRecord(value) &&
            hasExactOwnKeys(value, ['scalarStart', 'scalarEnd']) &&
            u32(value.scalarStart) &&
            u32(value.scalarEnd) &&
            value.scalarStart <= value.scalarEnd &&
            value.scalarEnd <= scalarLength)
    );
}

function sameExtent(
    a: TableInputExtent | null,
    b: TableInputExtent | null,
): boolean {
    return a === null || b === null
        ? a === b
        : a.scalarStart === b.scalarStart && a.scalarEnd === b.scalarEnd;
}

function appendExtent(
    a: TableInputExtent | null,
    b: TableInputExtent,
): TableInputExtent {
    return {
        scalarStart: a?.scalarStart ?? b.scalarStart,
        scalarEnd: b.scalarEnd,
    };
}

type Part = {
    elementIndex: number;
    docStart: number;
    docEnd: number;
    extent: TableInputExtent | null;
    breakEnd: number | null;
};

/** Records must first pass complete semantic-pool validation, including on patches. */
export function normalizeTableInputMappings(
    value: unknown,
    records: Record<string, TableRenderRecord>,
    scalarLength: number,
): TableInputMappings | null {
    if (
        !isPlainRecord(value) ||
        !hasExactOwnKeys(value, ['version', 'tables']) ||
        value.version !== 1 ||
        !isPlainRecord(value.tables) ||
        !hasExactOwnKeys(value.tables, Object.keys(records))
    )
        return null;
    const tables = value.tables;
    for (const [id, raw] of Object.entries(tables)) {
        if (
            !isPlainRecord(raw) ||
            !hasExactOwnKeys(raw, ['extent', 'cells']) ||
            !validExtent(raw.extent, scalarLength) ||
            !Array.isArray(raw.cells) ||
            raw.cells.length !== records[id].cells.length
        )
            return null;
    }
    let work = 0;
    for (const [id, raw] of Object.entries(tables)) {
        const mapping = raw as {
            extent: TableInputExtent | null;
            cells: unknown[];
        };
        const record = records[id];
        let tableExtent: TableInputExtent | null = null;
        for (let cellIndex = 0; cellIndex < mapping.cells.length; cellIndex++) {
            const cell = mapping.cells[cellIndex];
            const source = record.cells[cellIndex];
            if (
                !isPlainRecord(cell) ||
                !hasExactOwnKeys(cell, [
                    'cellIndex',
                    'sourcePos',
                    'sourceEnd',
                    'blocks',
                    'excluded',
                ]) ||
                cell.cellIndex !== cellIndex ||
                cell.sourcePos !== source.sourcePos ||
                cell.sourceEnd !== source.sourceEnd ||
                !Array.isArray(cell.blocks) ||
                !Array.isArray(cell.excluded)
            )
                return null;
            work += 1 + cell.blocks.length + cell.excluded.length;
            if (work > 7_000_000) return null;
            const blocks: Part[] = [];
            let previousIndex = -1;
            for (const block of cell.blocks) {
                if (
                    !isPlainRecord(block) ||
                    !hasExactOwnKeys(block, [
                        'elementIndex',
                        'docStart',
                        'docEnd',
                        'scalarStart',
                        'contentScalarStart',
                        'scalarEnd',
                        'breakScalarEnd',
                        'void',
                    ]) ||
                    !u32(block.elementIndex) ||
                    block.elementIndex <= previousIndex ||
                    !u32(block.docStart) ||
                    !u32(block.docEnd) ||
                    block.docStart <= source.sourcePos ||
                    block.docEnd >= source.sourceEnd ||
                    block.docStart > block.docEnd ||
                    !u32(block.scalarStart) ||
                    !u32(block.contentScalarStart) ||
                    !u32(block.scalarEnd) ||
                    !u32(block.breakScalarEnd) ||
                    block.scalarStart > block.contentScalarStart ||
                    block.contentScalarStart > block.scalarEnd ||
                    block.scalarEnd > block.breakScalarEnd ||
                    block.breakScalarEnd > scalarLength ||
                    block.breakScalarEnd - block.scalarEnd > 1 ||
                    typeof block.void !== 'boolean'
                )
                    return null;
                const element = source.elements[block.elementIndex];
                if (
                    !element ||
                    (block.void
                        ? !['voidBlock', 'opaqueBlockAtom'].includes(
                              element.type,
                          ) ||
                          block.docStart !== block.docEnd ||
                          !('docPos' in element) ||
                          element.docPos !== block.docStart
                        : element.type !== 'blockStart')
                )
                    return null;
                previousIndex = block.elementIndex;
                blocks.push({
                    elementIndex: block.elementIndex,
                    docStart: block.docStart,
                    docEnd: block.docEnd,
                    extent: {
                        scalarStart: block.scalarStart,
                        scalarEnd: block.scalarEnd,
                    },
                    breakEnd: block.breakScalarEnd,
                });
            }
            const excluded: Part[] = [];
            previousIndex = -1;
            const nested = source.elements.flatMap((element, index) =>
                element.type === 'table' ? [index] : [],
            );
            if (nested.length !== cell.excluded.length) return null;
            for (let index = 0; index < cell.excluded.length; index++) {
                const item = cell.excluded[index];
                if (
                    !isPlainRecord(item) ||
                    !hasExactOwnKeys(item, [
                        'elementIndex',
                        'tableId',
                        'extent',
                    ]) ||
                    !u32(item.elementIndex) ||
                    item.elementIndex <= previousIndex ||
                    item.elementIndex !== nested[index] ||
                    typeof item.tableId !== 'string' ||
                    !Object.prototype.hasOwnProperty.call(
                        tables,
                        item.tableId,
                    ) ||
                    !validExtent(item.extent, scalarLength)
                )
                    return null;
                const element = source.elements[item.elementIndex];
                const child = tables[item.tableId] as {
                    extent: TableInputExtent | null;
                };
                if (
                    element.type !== 'table' ||
                    element.tableId !== item.tableId ||
                    !sameExtent(item.extent, child.extent)
                )
                    return null;
                previousIndex = item.elementIndex;
                excluded.push({
                    elementIndex: item.elementIndex,
                    docStart: records[item.tableId].tablePos,
                    docEnd: records[item.tableId].sourceEnd,
                    extent: item.extent,
                    breakEnd: item.extent?.scalarEnd ?? null,
                });
            }
            let blockIndex = 0;
            let excludedIndex = 0;
            let docEnd = source.sourcePos;
            let breakEnd: number | null = null;
            let cellExtent: TableInputExtent | null = null;
            while (
                blockIndex < blocks.length ||
                excludedIndex < excluded.length
            ) {
                const a = blocks[blockIndex];
                const b = excluded[excludedIndex];
                if (a && b && a.elementIndex === b.elementIndex) return null;
                const part =
                    a && (!b || a.elementIndex < b.elementIndex)
                        ? blocks[blockIndex++]
                        : excluded[excludedIndex++];
                if (part.docStart < docEnd) return null;
                docEnd = part.docEnd;
                if (part.extent !== null) {
                    if (breakEnd !== null && part.extent.scalarStart < breakEnd)
                        return null;
                    cellExtent = appendExtent(cellExtent, part.extent);
                    breakEnd = part.breakEnd;
                }
            }
            if (cellExtent !== null) {
                if (
                    (breakEnd ?? 0) > cellExtent.scalarEnd ||
                    (tableExtent !== null &&
                        cellExtent.scalarStart < tableExtent.scalarEnd)
                )
                    return null;
                tableExtent = appendExtent(tableExtent, cellExtent);
            }
        }
        if (record.failure === null && !sameExtent(tableExtent, mapping.extent))
            return null;
    }
    let previousEnd = 0;
    const roots = Object.entries(records)
        .filter(([, record]) => !record.readOnlyDescendants)
        .sort(([, a], [, b]) => a.tablePos - b.tablePos);
    for (const [id] of roots) {
        const { extent } = tables[id] as { extent: TableInputExtent | null };
        if (extent === null) continue;
        if (extent.scalarStart < previousEnd) return null;
        previousEnd = extent.scalarEnd;
    }
    return value as unknown as TableInputMappings;
}
