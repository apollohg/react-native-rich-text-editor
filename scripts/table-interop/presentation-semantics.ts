import { call, peerKindOf, snapshot } from './controller.js';
import type {
    Peer,
    JsonNode,
    EffectiveCell,
    EffectiveTable,
    EffectiveDocument,
} from './peer-protocol.js';
export type {
    JsonNode,
    EffectiveCell,
    EffectiveTable,
    EffectiveDocument,
} from './peer-protocol.js';
import { tableSchemaOf } from './table-schema.js';

export type PresentationFailure =
    | 'PRESENTATION_MISMATCH'
    | 'UNACCOUNTED_CONTENT'
    | 'AMBIGUOUS_SOURCE_MAPPING'
    | 'UNSUPPORTED_OBSERVATION';
export class PresentationError extends Error {
    constructor(
        readonly code: PresentationFailure,
        message: string,
    ) {
        super(`TBL-21-P ${code}: ${message}`);
        this.name = 'PresentationError';
    }
}
const cellTypes = new Set(['table_cell', 'table_header', 'tableCell', 'tableHeader']);
export type AttributeDefaults = Record<string, Record<string, unknown>>;
const defaults: AttributeDefaults = {
    table_cell: { colspan: 1, rowspan: 1, colwidth: null },
    table_header: { colspan: 1, rowspan: 1, colwidth: null },
    tableCell: { colspan: 1, rowspan: 1, colwidth: null, align: null },
    tableHeader: { colspan: 1, rowspan: 1, colwidth: null, align: null },
    image: { alt: null, title: null },
    link: { title: null },
};
function plain(value: unknown): unknown {
    if (Array.isArray(value)) return value.map(plain);
    if (value === null || typeof value !== 'object') return value;
    return Object.fromEntries(
        Object.entries(value)
            .sort(([a], [b]) => a.localeCompare(b))
            .map(([k, v]) => [k, plain(v)]),
    );
}
function equal(a: unknown, b: unknown): boolean {
    return JSON.stringify(plain(a)) === JSON.stringify(plain(b));
}
function semantic(node: JsonNode, declared: AttributeDefaults, geometry = false): unknown {
    const result: Record<string, unknown> = {};
    for (const [key, value] of Object.entries(node)) {
        if (key === 'attrs') {
            const attrs = Object.fromEntries(
                Object.entries(node.attrs ?? {}).filter(
                    ([name, entry]) =>
                        !(geometry && ['colspan', 'rowspan', 'colwidth'].includes(name)) &&
                        !(
                            name in (declared[node.type] ?? {}) &&
                            equal(entry, declared[node.type]![name])
                        ),
                ),
            );
            if (Object.keys(attrs).length) result[key] = attrs;
        } else if (key === 'content') {
            if (node.type === 'table') continue;
            const children = (node.content ?? []).map(
                (child) => semantic(child, declared) as Record<string, unknown>,
            );
            const merged: Record<string, unknown>[] = [];
            for (const child of children) {
                const previous = merged.at(-1);
                if (
                    previous?.type === 'text' &&
                    child.type === 'text' &&
                    equal({ ...previous, text: null }, { ...child, text: null })
                ) {
                    previous.text = String(previous.text) + String(child.text);
                } else merged.push(child);
            }
            if (merged.length) result[key] = merged;
        } else if (key === 'marks') {
            if (node.marks?.length)
                result[key] = node.marks.map((mark) => semantic(mark, declared));
        } else result[key] = value;
    }
    return result;
}
function fail(code: PresentationFailure, message: string): never {
    throw new PresentationError(code, message);
}
function same(a: unknown, b: unknown, reason: string): void {
    if (!equal(a, b))
        fail(
            'PRESENTATION_MISMATCH',
            `${reason}: expected ${JSON.stringify(a)}, observed ${JSON.stringify(b)}`,
        );
}
function resolvedCellWidths(cell: EffectiveCell, table: EffectiveTable): number[] | null {
    const widths = table.widths
        .slice(cell.column, cell.column + cell.colspan)
        .map((width) => width ?? 0);
    return widths.some(Boolean) ? widths : null;
}
function emptyGap(
    cell: EffectiveCell,
    table: EffectiveTable,
    declared: AttributeDefaults,
): boolean {
    if (
        cell.node.attrs?.colwidth != null &&
        !equal(cell.node.attrs.colwidth, resolvedCellWidths(cell, table))
    )
        return false;
    if (
        (cell.node.attrs?.colspan ?? 1) !== cell.colspan ||
        (cell.node.attrs?.rowspan ?? 1) !== cell.rowspan
    )
        return false;
    const value = semantic(cell.node, defaults, true);
    return (
        equal(value, { type: 'table_cell', content: [{ type: 'paragraph' }] }) ||
        equal(value, { type: 'tableCell', content: [{ type: 'paragraph' }] })
    );
}
function geometry(cell: EffectiveCell): number[] {
    return [cell.row, cell.column, cell.rowspan, cell.colspan];
}

function meaningfulGaps(
    table: EffectiveTable,
    declared: AttributeDefaults,
): Map<number, unknown> {
    const result = new Map<number, unknown>();
    for (const cell of table.cells) {
        if (cell.source !== null || emptyGap(cell, table, declared)) continue;
        if (
            (cell.node.attrs?.colspan ?? 1) !== cell.colspan ||
            (cell.node.attrs?.rowspan ?? 1) !== cell.rowspan ||
            (cell.node.attrs?.colwidth != null &&
                !equal(cell.node.attrs.colwidth, resolvedCellWidths(cell, table)))
        )
            fail('UNACCOUNTED_CONTENT', `unexplained synthetic geometry at ${cell.position}`);
        for (let row = cell.row; row < cell.row + cell.rowspan; row++)
            for (let column = cell.column; column < cell.column + cell.colspan; column++)
                result.set(row * table.columns + column, semantic(cell.node, declared, true));
    }
    return result;
}
function validateGrid(table: EffectiveTable): void {
    if (
        !Number.isInteger(table.rows) ||
        !Number.isInteger(table.columns) ||
        table.rows < 0 ||
        table.columns < 0 ||
        table.widths.length !== table.columns
    )
        fail('UNSUPPORTED_OBSERVATION', `invalid extent for ${table.source}`);
    const occupied = new Set<number>();
    for (const cell of table.cells) {
        const [r, c, h, w] = geometry(cell) as [number, number, number, number];
        if (
            ![r, c, h, w].every(Number.isInteger) ||
            r < 0 ||
            c < 0 ||
            h < 1 ||
            w < 1 ||
            r + h > table.rows ||
            c + w > table.columns
        )
            fail(
                'PRESENTATION_MISMATCH',
                `cell ${cell.source} has invalid rectangle ${geometry(cell)}`,
            );
        for (let row = r; row < r + h; row++)
            for (let col = c; col < c + w; col++) {
                const slot = row * table.columns + col;
                if (occupied.has(slot))
                    fail(
                        'PRESENTATION_MISMATCH',
                        `overlapping content regions in ${table.source} at ${row},${col}`,
                    );
                occupied.add(slot);
            }
    }
}

export function assertEffectivePresentation(
    expected: EffectiveDocument,
    actual: EffectiveDocument,
    declared: AttributeDefaults = defaults,
): void {
    const actualTables = new Map<string, EffectiveTable>();
    for (const table of actual.tables) {
        if (actualTables.has(table.source))
            fail('AMBIGUOUS_SOURCE_MAPPING', `multiple live tables map to ${table.source}`);
        actualTables.set(table.source, table);
    }
    if (actualTables.size !== expected.tables.length)
        fail('UNACCOUNTED_CONTENT', 'table count differs');
    for (const wanted of expected.tables) {
        const observed = actualTables.get(wanted.source);
        if (!observed) fail('UNACCOUNTED_CONTENT', `missing table ${wanted.source}`);
        const bySource = new Map<string, EffectiveCell>();
        for (const cell of observed.cells) {
            if (cell.source === null) continue;
            if (bySource.has(cell.source))
                fail('UNACCOUNTED_CONTENT', `duplicated cell ${cell.source}`);
            bySource.set(cell.source, cell);
        }
        validateGrid(wanted);
        validateGrid(observed);
        const wantedGaps = meaningfulGaps(wanted, declared);
        const observedGaps = meaningfulGaps(observed, declared);
        if (
            wantedGaps.size !== observedGaps.size ||
            [...wantedGaps].some(
                ([slot, value]) => !observedGaps.has(slot) || !equal(value, observedGaps.get(slot)),
            )
        )
            fail('UNACCOUNTED_CONTENT', `meaningful synthetic regions differ in ${wanted.source}`);
        const real = wanted.cells.filter((cell) => cell.source !== null);
        if (bySource.size !== real.length)
            fail('UNACCOUNTED_CONTENT', `real cell count differs in ${wanted.source}`);
        same(
            [wanted.parentCell, wanted.pathWithinCell],
            [observed.parentCell, observed.pathWithinCell],
            `table nesting ${wanted.source}`,
        );
        same(
            [wanted.rows, wanted.columns, wanted.widths],
            [observed.rows, observed.columns, observed.widths],
            `table geometry ${wanted.source}`,
        );
        same(
            semantic(wanted.node, declared),
            semantic(observed.node, declared),
            `table attributes ${wanted.source}`,
        );
        same(
            (wanted.node.content ?? []).map((row) => semantic({ ...row, content: [] }, declared)),
            (observed.node.content ?? []).map((row) => semantic({ ...row, content: [] }, declared)),
            `row attributes ${wanted.source}`,
        );
        for (const cell of real) {
            const live = bySource.get(cell.source!);
            if (!live) fail('UNACCOUNTED_CONTENT', `missing source ${cell.source}`);
            same(geometry(cell), geometry(live), `placement/spans ${cell.source}`);
            same(
                semantic(cell.node, declared, true),
                semantic(live.node, declared, true),
                `content/header/attributes ${cell.source}`,
            );
            for (const span of ['colspan', 'rowspan'] as const) {
                const actualSpan = live.node.attrs?.[span] ?? 1;
                if (actualSpan !== (cell.node.attrs?.[span] ?? 1) && actualSpan !== live[span])
                    fail(
                        'PRESENTATION_MISMATCH',
                        `unexplained ${span} attribute for ${cell.source}`,
                    );
            }
            const rawWidths = cell.node.attrs?.colwidth ?? null;
            const liveWidths = live.node.attrs?.colwidth ?? null;
            if (
                !equal(rawWidths, liveWidths) &&
                !equal(liveWidths, resolvedCellWidths(live, observed))
            )
                fail('PRESENTATION_MISMATCH', `unexplained colwidth attributes for ${cell.source}`);
        }
    }
}

function nodeSize(node: JsonNode): number {
    // Native positions count Unicode scalars; live web positions count UTF-16 units.
    if (node.type === 'text') return Array.from(node.text ?? '').length;
    if (
        !node.content?.length &&
        !['doc', 'paragraph', 'table', 'table_row', 'tableRow', ...cellTypes].includes(node.type)
    )
        return 1;
    return 2 + (node.content ?? []).reduce((size, child) => size + nodeSize(child), 0);
}
export async function observeNativePresentation(peer: Peer): Promise<EffectiveDocument> {
    if (peerKindOf(peer) !== 'rust')
        fail('UNSUPPORTED_OBSERVATION', 'native observation requires a Rust peer');
    const state = await snapshot(peer);
    if (!state.mounted || state.pendingDependencies || !state.documentJson)
        fail('UNSUPPORTED_OBSERVATION', 'native peer is not mounted and settled');
    const tables: EffectiveTable[] = [];
    async function visit(
        node: JsonNode,
        source: string,
        position: number,
        parentCell: string | null,
        within: string,
    ): Promise<void> {
        if (node.type === 'table') {
            const camel = node.content?.[0]?.type === 'tableRow';
            const projection = await call(peer, 'projectTable', {
                schema: tableSchemaOf(
                    camel ? 'tableRow' : 'table_row',
                    camel ? 'tableCell' : 'table_cell',
                    camel ? 'tableHeader' : 'table_header',
                ),
                table: node,
            });
            const { rows, columns, widths, slots } = projection;
            if (projection.compatibilityDiagnostic != null)
                fail(
                    'UNSUPPORTED_OBSERVATION',
                    `native compatibility fallback for ${source}: ${projection.compatibilityDiagnostic}`,
                );
            if (
                typeof rows !== 'number' ||
                typeof columns !== 'number' ||
                !Array.isArray(widths) ||
                !Array.isArray(slots) ||
                slots.length !== rows * columns
            )
                fail('UNSUPPORTED_OBSERVATION', `invalid native projection for ${source}`);
            const cells: EffectiveCell[] = [];
            let rowPosition = 1;
            for (const [r, row] of (node.content ?? []).entries()) {
                let offset = rowPosition + 1;
                for (const [c, cell] of (row.content ?? []).entries()) {
                    const indices = slots.flatMap((slot, index) =>
                        slot === offset ? [index] : [],
                    );
                    if (!indices.length)
                        fail(
                            'UNACCOUNTED_CONTENT',
                            `native projection dropped ${source}.${r}.${c}`,
                        );
                    const first = indices[0]!;
                    const last = indices.at(-1)!;
                    cells.push({
                        source: `${source}.${r}.${c}`,
                        position: position + offset,
                        rawPosition: position + offset,
                        row: Math.floor(first / columns),
                        column: first % columns,
                        rowspan: Math.floor(last / columns) - Math.floor(first / columns) + 1,
                        colspan: (last % columns) - (first % columns) + 1,
                        node: cell,
                    });
                    offset += nodeSize(cell);
                }
                rowPosition += nodeSize(row);
            }
            if (!Array.isArray(projection.synthetic))
                fail(
                    'UNSUPPORTED_OBSERVATION',
                    `native projection lacks synthetic semantics for ${source}`,
                );
            for (const region of projection.synthetic as Record<string, unknown>[]) {
                if (!region.node || typeof region.node !== 'object')
                    fail('UNSUPPORTED_OBSERVATION', `invalid synthetic semantics for ${source}`);
                cells.push({
                    source: null,
                    position: -1,
                    row: region.row as number,
                    column: region.column as number,
                    rowspan: region.rowspan as number,
                    colspan: region.colspan as number,
                    node: region.node as JsonNode,
                });
            }
            tables.push({
                source,
                parentCell,
                pathWithinCell: within,
                position,
                node,
                rows,
                columns,
                widths: widths as (number | null)[],
                cells,
            });
        }
        let offset = node.type === 'doc' ? 0 : position + 1;
        for (const [index, child] of (node.content ?? []).entries()) {
            const path = source ? `${source}.${index}` : String(index);
            await visit(
                child,
                path,
                offset,
                cellTypes.has(node.type) ? source : parentCell,
                cellTypes.has(node.type)
                    ? String(index)
                    : within
                      ? `${within}.${index}`
                      : String(index),
            );
            offset += nodeSize(child);
        }
    }
    await visit(state.documentJson as JsonNode, '', -1, null, '');
    return { tables };
}

export async function observeWebPresentation(peer: Peer): Promise<EffectiveDocument> {
    if (peerKindOf(peer) === 'rust')
        fail('UNSUPPORTED_OBSERVATION', 'live web observation requires a web peer');
    let value: Record<string, unknown>;
    try {
        value = await call(peer, 'observePresentation', {});
    } catch (error) {
        const code = (error as { code?: string }).code;
        if (code === 'AMBIGUOUS_SOURCE_MAPPING' || code === 'UNACCOUNTED_CONTENT')
            fail(code, String(error));
        fail('UNSUPPORTED_OBSERVATION', String(error));
    }
    if (!Array.isArray(value.tables))
        fail('UNSUPPORTED_OBSERVATION', 'web observation has no tables');
    return value as unknown as EffectiveDocument;
}
export async function assertPeerPresentation(native: Peer, web: Peer): Promise<void> {
    assertEffectivePresentation(
        await observeNativePresentation(native),
        await observeWebPresentation(web),
    );
}
