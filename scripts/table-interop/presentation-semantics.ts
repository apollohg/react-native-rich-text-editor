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
import { validateFallback, validateOverlapEvidence } from './presentation-overlap.js';

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
    if (cell.column === null || cell.colspan === null || table.widths === null)
        fail('UNSUPPORTED_OBSERVATION', 'logical cell geometry unavailable');
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
        equal(value, {
            type: 'table_cell',
            content: [{ type: 'paragraph' }],
        }) || equal(value, { type: 'tableCell', content: [{ type: 'paragraph' }] })
    );
}
function geometry(cell: EffectiveCell): (number | null)[] {
    return [cell.row, cell.column, cell.rowspan, cell.colspan];
}

function meaningfulGaps(table: EffectiveTable, declared: AttributeDefaults): Map<number, unknown> {
    const result = new Map<number, unknown>();
    for (const cell of table.cells) {
        if (cell.source !== null || emptyGap(cell, table, declared)) continue;
        if (
            cell.row === null ||
            cell.column === null ||
            cell.rowspan === null ||
            cell.colspan === null
        )
            fail('UNACCOUNTED_CONTENT', 'meaningful synthetic region has unavailable geometry');
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
        table.widths === null ||
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

export interface PresentationCheck {
    tables: (
        | { source: string; kind: 'exact' }
        | {
              source: string;
              kind: 'ordinary-safe-overlap';
              evidence: 'live-overlap';
          }
        | {
              source: string;
              kind: 'overlap-fallback';
              evidence: 'native-equality' | 'live-overlap';
          }
    )[];
}
// Expected is a native observation; actual is live web or another native observation.
export function assertEffectivePresentation(
    expected: EffectiveDocument,
    actual: EffectiveDocument,
    declared: AttributeDefaults = defaults,
): PresentationCheck {
    const checked: PresentationCheck = { tables: [] };
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
        for (const view of [wanted, observed]) {
            const identities = new Set<string>();
            for (const cell of view.cells) {
                if (cell.sourceId === undefined) continue;
                if (cell.source === null || identities.has(cell.sourceId))
                    fail(
                        'AMBIGUOUS_SOURCE_MAPPING',
                        `duplicated/unattributed source identity ${cell.sourceId}`,
                    );
                identities.add(cell.sourceId);
            }
        }
        const bySource = new Map<string, EffectiveCell>();
        for (const cell of observed.cells) {
            if (cell.source === null) continue;
            if (bySource.has(cell.source))
                fail('UNACCOUNTED_CONTENT', `duplicated cell ${cell.source}`);
            bySource.set(cell.source, cell);
        }
        const exceptional = wanted.overlap !== undefined || observed.overlap !== undefined;
        const ordinarySafe =
            wanted.overlap === undefined && observed.overlap?.kind === 'web-overlap';
        const mixed =
            observed.overlap?.kind === 'web-overlap' &&
            (wanted.overlap === undefined || wanted.overlap.kind === 'native-fallback');
        if (exceptional) {
            if (
                !mixed &&
                !(
                    wanted.overlap?.kind === 'native-fallback' &&
                    observed.overlap?.kind === 'native-fallback'
                )
            )
                fail(
                    'UNSUPPORTED_OBSERVATION',
                    'overlap evidence requires a native projection and live web overlap',
                );
            for (const view of [wanted, observed]) {
                try {
                    if (view.overlap?.kind === 'web-overlap') validateOverlapEvidence(view);
                    else {
                        validateGrid(view);
                        if (view.overlap !== undefined) validateFallback(view);
                        const real = view.cells.filter((cell) => cell.source !== null);
                        const sources: string[] = [];
                        let index = 0;
                        for (const [r, row] of (view.node.content ?? []).entries())
                            for (const [c, raw] of (row.content ?? []).entries()) {
                                const source = `${view.source}.${r}.${c}`;
                                sources.push(source);
                                const projected = real[index++];
                                if (!projected || projected.source !== source)
                                    fail(
                                        'UNACCOUNTED_CONTENT',
                                        `native source order/content ${source}`,
                                    );
                                same(
                                    semantic(raw, declared, view.overlap === undefined),
                                    semantic(projected.node, declared, view.overlap === undefined),
                                    `native source content/header/attributes ${projected.source}`,
                                );
                            }
                        same(
                            sources,
                            real.map((cell) => cell.source),
                            `native source order ${view.source}`,
                        );
                    }
                } catch (error) {
                    fail('PRESENTATION_MISMATCH', String(error));
                }
            }
        } else {
            validateGrid(wanted);
            validateGrid(observed);
        }
        const exceptionalGaps = (view: EffectiveTable) => {
            for (const c of view.cells.filter((c) => c.source === null)) {
                const value = semantic(c.node, declared, true);
                if (
                    !equal(value, {
                        type: 'table_cell',
                        content: [{ type: 'paragraph' }],
                    }) &&
                    !equal(value, {
                        type: 'tableCell',
                        content: [{ type: 'paragraph' }],
                    })
                )
                    fail('UNACCOUNTED_CONTENT', `meaningful synthetic regions in ${view.source}`);
            }
            return new Map<number, unknown>();
        };
        const wantedGaps = mixed ? exceptionalGaps(wanted) : meaningfulGaps(wanted, declared);
        const observedGaps = mixed ? exceptionalGaps(observed) : meaningfulGaps(observed, declared);
        if (
            wantedGaps.size !== observedGaps.size ||
            [...wantedGaps].some(
                ([slot, value]) => !observedGaps.has(slot) || !equal(value, observedGaps.get(slot)),
            )
        )
            fail('UNACCOUNTED_CONTENT', `meaningful synthetic regions differ in ${wanted.source}`);
        const real = wanted.cells.filter((cell) => cell.source !== null);
        if (new Set(real.map((cell) => cell.source)).size !== real.length)
            fail('UNACCOUNTED_CONTENT', `duplicated cell in ${wanted.source}`);
        if (bySource.size !== real.length)
            fail('UNACCOUNTED_CONTENT', `real cell count differs in ${wanted.source}`);
        if (mixed)
            same(
                real.map((cell) => cell.source),
                observed.cells.filter((cell) => cell.source !== null).map((cell) => cell.source),
                `real source order ${wanted.source}`,
            );
        same(
            [wanted.parentCell, wanted.pathWithinCell],
            [observed.parentCell, observed.pathWithinCell],
            `table nesting ${wanted.source}`,
        );
        if (!mixed)
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
            if (cell.sourceId !== undefined && live.sourceId !== undefined)
                same(cell.sourceId, live.sourceId, `source identity ${cell.source}`);
            if (!mixed) same(geometry(cell), geometry(live), `placement/spans ${cell.source}`);
            same(
                semantic(cell.node, declared, true),
                semantic(live.node, declared, true),
                `content/header/attributes ${cell.source}`,
            );
            if (mixed) continue;
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
        checked.tables.push(
            ordinarySafe
                ? {
                      source: wanted.source,
                      kind: 'ordinary-safe-overlap',
                      evidence: 'live-overlap',
                  }
                : exceptional
                  ? {
                        source: wanted.source,
                        kind: 'overlap-fallback',
                        evidence: mixed ? 'live-overlap' : 'native-equality',
                    }
                  : { source: wanted.source, kind: 'exact' },
        );
    }
    return checked;
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
            if (
                projection.compatibilityDiagnostic != null &&
                projection.compatibilityDiagnostic !== 'overlapping-reference-cells'
            )
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
                ...(projection.compatibilityDiagnostic === 'overlapping-reference-cells'
                    ? {
                          overlap: {
                              kind: 'native-fallback' as const,
                              reason: 'overlapping-reference-cells' as const,
                          },
                      }
                    : {}),
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
