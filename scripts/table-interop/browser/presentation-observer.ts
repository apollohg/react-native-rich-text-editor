import * as Y from 'yjs';
import type { Node as PMNode } from 'prosemirror-model';
import { TableMap } from 'prosemirror-tables';
import type { EditorView } from 'prosemirror-view';
import { overlapWitness } from '../presentation-overlap.js';
import type {
    EffectiveCell,
    EffectiveDocument,
    EffectiveTable,
    JsonNode,
} from '../peer-protocol.js';

type Mapping = Map<Y.AbstractType<unknown>, PMNode | PMNode[]>;
type RawNode = {
    element: Y.XmlElement;
    path: string;
    position: number;
    cellAncestors: RawNode[];
    node: JsonNode;
};
type LiveNode = {
    node: PMNode;
    position: number;
    cellAncestors: LiveNode[];
    table: LiveNode | null;
    path: string;
};
const isCell = (name: string): boolean =>
    ['table_cell', 'table_header', 'tableCell', 'tableHeader'].includes(name);
function fail(code: string, message: string): never {
    throw Object.assign(new Error(`TBL-21-P ${code}: ${message}`), { code });
}
function size(node: JsonNode): number {
    if (node.type === 'text') return node.text!.length;
    if (
        !node.content?.length &&
        !['paragraph', 'table', 'table_row', 'tableRow'].includes(node.type) &&
        !isCell(node.type)
    )
        return 1;
    return 2 + (node.content ?? []).reduce((total, child) => total + size(child), 0);
}
function nodes(mapped: PMNode | PMNode[] | undefined): PMNode[] {
    return mapped === undefined ? [] : Array.isArray(mapped) ? mapped : [mapped];
}
function mappedElement(mapping: Mapping, element: Y.XmlElement): PMNode[] {
    return nodes(mapping.get(element as unknown as Y.AbstractType<unknown>));
}

export function observePresentation(
    fragment: Y.XmlFragment,
    mapping: Mapping,
    document: PMNode,
    rawJson: JsonNode,
    view?: EditorView,
): EffectiveDocument {
    const raw: RawNode[] = [];
    function readRaw(
        parent: Y.XmlFragment,
        json: JsonNode,
        prefix: string,
        start: number,
        ancestors: RawNode[],
    ): void {
        let index = 0;
        let position = start;
        for (const element of parent.toArray()) {
            if (element instanceof Y.XmlText) {
                let remaining = element.length;
                while (remaining > 0) {
                    const text = json.content?.[index++];
                    if (text?.type !== 'text' || !text.text?.length)
                        fail('UNSUPPORTED_OBSERVATION', 'raw text structure cannot be attributed');
                    remaining -= text.text.length;
                    position += text.text.length;
                }
                if (remaining !== 0) fail('UNSUPPORTED_OBSERVATION', 'raw text extent differs');
                continue;
            }
            if (!(element instanceof Y.XmlElement))
                fail('UNSUPPORTED_OBSERVATION', 'unsupported shared node');
            const node = json.content?.[index];
            if (!node || node.type !== element.nodeName)
                fail('UNSUPPORTED_OBSERVATION', `raw node structure differs at ${prefix}.${index}`);
            const entry: RawNode = {
                element,
                path: prefix ? `${prefix}.${index}` : String(index),
                position,
                cellAncestors: ancestors,
                node,
            };
            raw.push(entry);
            readRaw(
                element,
                node,
                entry.path,
                position + 1,
                isCell(node.type) ? [...ancestors, entry] : ancestors,
            );
            position += size(node);
            index++;
        }
        if (index !== (json.content?.length ?? 0))
            fail('UNSUPPORTED_OBSERVATION', `unattributed raw children at ${prefix}`);
    }
    readRaw(fragment, rawJson, '', 0, []);
    const live: LiveNode[] = [];
    function readLive(
        parent: PMNode,
        prefix: string,
        start: number,
        ancestors: LiveNode[],
        table: LiveNode | null,
    ): void {
        parent.forEach((node, offset, index) => {
            const entry: LiveNode = {
                node,
                position: start + offset,
                cellAncestors: ancestors,
                table,
                path: prefix ? `${prefix}.${index}` : String(index),
            };
            live.push(entry);
            readLive(
                node,
                entry.path,
                entry.position + 1,
                isCell(node.type.name) ? [...ancestors, entry] : ancestors,
                node.type.spec.tableRole === 'table' ? entry : table,
            );
        });
    }
    readLive(document, '', 0, [], null);
    const rawCells = raw.filter((entry) => isCell(entry.node.type));
    const liveCells = live.filter((entry) => isCell(entry.node.type.name));
    const cellMap = new Map<RawNode, LiveNode>();
    const claimed = new Set<LiveNode>();
    for (const source of rawCells) {
        const mapped = mappedElement(mapping, source.element);
        const candidates = liveCells.filter(
            (entry) =>
                entry.cellAncestors.length === source.cellAncestors.length &&
                entry.cellAncestors.every(
                    (parent, index) => cellMap.get(source.cellAncestors[index]!) === parent,
                ),
        );
        let matches = candidates.filter((entry) => mapped.includes(entry.node));
        if (!matches.length) {
            matches = candidates.filter((entry) =>
                mapped.some((node) => node.content.size > 0 && node.content === entry.node.content),
            );
        }
        if (!matches.length) {
            const witnesses = raw.filter((entry) => entry.cellAncestors.includes(source));
            matches = candidates.filter((candidate) =>
                witnesses.some((witness) => {
                    const mappedNodes = mappedElement(mapping, witness.element);
                    const depth = witness.cellAncestors.length - source.cellAncestors.length;
                    return live.some(
                        (entry) =>
                            entry.cellAncestors.includes(candidate) &&
                            entry.cellAncestors.length - candidate.cellAncestors.length === depth &&
                            mappedNodes.includes(entry.node),
                    );
                }),
            );
        }
        if (matches.length !== 1)
            fail(
                'AMBIGUOUS_SOURCE_MAPPING',
                `${source.path} has ${matches.length} identity-backed live owners`,
            );
        const owner = matches[0]!;
        if (claimed.has(owner))
            fail('AMBIGUOUS_SOURCE_MAPPING', `live cell at ${owner.position} has multiple sources`);
        claimed.add(owner);
        cellMap.set(source, owner);
    }
    const reverseCells = new Map([...cellMap].map(([a, b]) => [b, a]));
    const tables: EffectiveTable[] = [];
    const usedTables = new Set<LiveNode>();
    for (const source of raw.filter((entry) => entry.node.type === 'table')) {
        const directSources = rawCells.filter(
            (cell) =>
                cell.path.startsWith(`${source.path}.`) &&
                cell.path.split('.').length === source.path.split('.').length + 2,
        );
        const owners = new Set(directSources.map((cell) => cellMap.get(cell)!.table));
        let candidates = live.filter(
            (entry) => entry.node.type.spec.tableRole === 'table' && owners.has(entry),
        );
        if (!directSources.length)
            candidates = live.filter((entry) =>
                mappedElement(mapping, source.element).includes(entry.node),
            );
        if (candidates.length !== 1 || owners.size > 1)
            fail('AMBIGUOUS_SOURCE_MAPPING', `table ${source.path} has inconsistent owners`);
        const owner = candidates[0]!;
        if (usedTables.has(owner))
            fail('AMBIGUOUS_SOURCE_MAPPING', `table ${source.path} repeats a live owner`);
        usedTables.add(owner);
        const map = TableMap.get(owner.node);
        const collision = map.problems?.some((problem) => problem.type === 'collision') ?? false;
        if (
            map.problems?.some(
                (problem) => !['colwidth mismatch', 'missing', 'collision'].includes(problem.type),
            )
        )
            fail(
                'UNSUPPORTED_OBSERVATION',
                `live table ${source.path} has unsupported map problems: ${JSON.stringify(map.problems)}`,
            );
        const repairedWidths = new Map<number, number[]>();
        for (const problem of map.problems ?? [])
            if (problem.type === 'colwidth mismatch')
                repairedWidths.set(problem.pos, problem.colwidth);
        const widths: (number | null)[] = Array(map.width).fill(null);
        const cells: EffectiveCell[] = [];
        const boxes = [];
        const ownedCells = liveCells.filter((entry) => entry.table === owner);
        if (collision && ownedCells.length > 31_250)
            fail('UNSUPPORTED_OBSERVATION', 'overlap measurement cell budget exceeded');
        for (const liveCell of ownedCells) {
            const offset = liveCell.position - owner.position - 1;
            const rectangle = collision ? null : map.findCell(offset);
            const sourceCell = reverseCells.get(liveCell);
            if (collision && sourceCell) {
                const element = view?.nodeDOM(liveCell.position) as EditorView['dom'] | null;
                const tableElement = view?.nodeDOM(owner.position) as EditorView['dom'] | null;
                if (
                    element?.nodeType !== 1 ||
                    !['TD', 'TH'].includes(element.tagName) ||
                    tableElement?.nodeType !== 1 ||
                    element.closest('table') !==
                        (tableElement.matches('table')
                            ? tableElement
                            : tableElement.querySelector('table'))
                )
                    fail(
                        'UNSUPPORTED_OBSERVATION',
                        `overlap DOM attribution unavailable for ${sourceCell.path}`,
                    );
                const { left, top, right, bottom } = element.getBoundingClientRect();
                if (
                    ![left, top, right, bottom].every(Number.isFinite) ||
                    right <= left ||
                    bottom <= top
                )
                    fail(
                        'UNSUPPORTED_OBSERVATION',
                        `overlap DOM box must be finite and positive for ${sourceCell.path}`,
                    );
                boxes.push({
                    source: sourceCell.path,
                    position: liveCell.position,
                    tableSource: source.path,
                    left,
                    top,
                    right,
                    bottom,
                });
            }
            const colwidth = repairedWidths.get(offset) ?? liveCell.node.attrs.colwidth;
            for (
                let column = rectangle?.left ?? 0;
                rectangle && column < rectangle.right;
                column++
            ) {
                const width = colwidth?.[column - rectangle.left];
                if (typeof width === 'number' && width > 0) widths[column] = width;
            }
            cells.push({
                source: sourceCell?.path ?? null,
                ...(sourceCell
                    ? {
                          sourceId: JSON.stringify({
                              type: Y.relativePositionToJSON(
                                  Y.createRelativePositionFromTypeIndex(sourceCell.element, 0),
                              ).type,
                          }),
                          rawPosition: sourceCell.position,
                      }
                    : {}),
                position: liveCell.position,
                row: rectangle?.top ?? null,
                column: rectangle?.left ?? null,
                rowspan: rectangle ? rectangle.bottom - rectangle.top : null,
                colspan: rectangle ? rectangle.right - rectangle.left : null,
                node: liveCell.node.toJSON() as JsonNode,
            });
        }
        const witness = collision ? overlapWitness(boxes) : null;
        if (collision && !witness)
            fail(
                'UNSUPPORTED_OBSERVATION',
                `live table ${source.path} collision has no measured overlap intersection`,
            );
        const parent = source.cellAncestors.at(-1);
        const liveParent = owner.cellAncestors.at(-1);
        if ((parent && cellMap.get(parent)) !== liveParent)
            fail('AMBIGUOUS_SOURCE_MAPPING', `table ${source.path} changed cell ancestry`);
        tables.push({
            ...(witness
                ? {
                      overlap: {
                          kind: 'web-overlap' as const,
                          logicalGeometry: 'unavailable' as const,
                          boxes: witness,
                      },
                  }
                : {}),
            source: source.path,
            parentCell: parent?.path ?? null,
            pathWithinCell: liveParent ? owner.path.slice(liveParent.path.length + 1) : owner.path,
            position: owner.position,
            node: owner.node.toJSON() as JsonNode,
            rows: map.height,
            columns: map.width,
            widths: collision ? null : widths,
            cells,
        });
    }
    if (
        usedTables.size !==
        live.filter((entry) => entry.node.type.spec.tableRole === 'table').length
    )
        fail('UNACCOUNTED_CONTENT', 'unattributed live table');
    return { tables };
}
