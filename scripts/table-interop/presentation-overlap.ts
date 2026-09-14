import type { CellBox, EffectiveTable } from './peer-protocol.js';

const WORK_LIMIT = 1_000_000;
export function overlapWitness(boxes: CellBox[]): [CellBox, CellBox] | null {
    if (boxes.length > WORK_LIMIT / 32) throw new Error('overlap measurement cell budget exceeded');
    const byTop = [...boxes].sort((a, b) => a.top - b.top);
    const indices = new Map(byTop.map((box, i) => [box, i]));
    const byLeft = [...boxes].sort((a, b) => a.left - b.left);
    const byRight = [...boxes].sort((a, b) => a.right - b.right);
    let base = 1;
    while (base < boxes.length) base *= 2;
    const maxima: (CellBox | null)[] = Array(base * 2).fill(null);
    const higher = (a: CellBox | null, b: CellBox | null) =>
        !a ? b : !b || a.bottom >= b.bottom ? a : b;
    const update = (box: CellBox, active: boolean) => {
        let index = base + indices.get(box)!;
        maxima[index] = active ? box : null;
        while (index > 1) {
            index = Math.floor(index / 2);
            maxima[index] = higher(maxima[index * 2]!, maxima[index * 2 + 1]!);
        }
    };
    let expired = 0;
    for (const box of byLeft) {
        while (expired < byRight.length && byRight[expired]!.right <= box.left)
            update(byRight[expired++]!, false);
        let lo = 0,
            hi = byTop.length;
        while (lo < hi) {
            const mid = Math.floor((lo + hi) / 2);
            if (byTop[mid]!.top < box.bottom) lo = mid + 1;
            else hi = mid;
        }
        let left = base,
            right = base + lo;
        let candidate: CellBox | null = null;
        while (left < right) {
            if (left % 2) candidate = higher(candidate, maxima[left++]!);
            if (right % 2) candidate = higher(candidate, maxima[--right]!);
            left = Math.floor(left / 2);
            right = Math.floor(right / 2);
        }
        if (candidate && candidate.bottom > box.top && candidate.source !== box.source)
            return [candidate, box];
        update(box, true);
    }
    return null;
}

export function validateOverlapEvidence(table: EffectiveTable): void {
    if (table.overlap?.kind !== 'web-overlap' || table.overlap.logicalGeometry !== 'unavailable')
        throw new Error(`missing live overlap evidence for ${table.source}`);
    const boxes = table.overlap.boxes;
    if (
        boxes.length !== 2 ||
        boxes[0]!.source === boxes[1]!.source ||
        boxes[0]!.position === boxes[1]!.position
    )
        throw new Error('overlap witnesses must be two distinct real cells');
    for (const box of boxes) {
        const cells = table.cells.filter((c) => c.source === box.source);
        if (
            box.tableSource !== table.source ||
            cells.length !== 1 ||
            cells[0]!.position !== box.position
        )
            throw new Error(`overlap witness attribution for ${box.source}`);
        if (
            ![box.left, box.right, box.top, box.bottom].every(Number.isFinite) ||
            box.left >= box.right ||
            box.top >= box.bottom
        )
            throw new Error('overlap boxes must be finite and positive');
    }
    if (!overlapWitness(boxes)) throw new Error('overlap evidence has no intersection');
}

export function validateFallback(table: EffectiveTable): void {
    if (
        table.overlap?.kind !== 'native-fallback' ||
        table.overlap.reason !== 'overlapping-reference-cells'
    )
        throw new Error('ineligible native fallback diagnostic');
    const rows = table.node.content ?? [];
    const real = table.cells.filter((c) => c.source !== null);
    const occupied = new Set<string>();
    const widths: { value: number; count: number }[] = [];
    let index = 0;
    let columns = 0;
    let irregular = false;
    let work = 0;
    const spend = () => {
        if (++work > WORK_LIMIT) throw new Error('fallback geometry work budget exceeded');
    };
    for (const [r, row] of rows.entries()) {
        let cursor = 0;
        for (const [c, raw] of (row.content ?? []).entries()) {
            const w = raw.attrs?.colspan ?? 1;
            const rawHeight = raw.attrs?.rowspan ?? 1;
            if (
                typeof w !== 'number' ||
                typeof rawHeight !== 'number' ||
                !Number.isSafeInteger(w) ||
                !Number.isSafeInteger(rawHeight) ||
                w < 1 ||
                rawHeight < 1
            )
                throw new Error('fallback geometry invalid raw spans');
            const h = Math.min(rawHeight, rows.length - r);
            if (h !== rawHeight) irregular = true;
            while (occupied.has(`${r},${cursor}`)) {
                spend();
                cursor++;
            }
            const free = () => {
                for (let x = cursor; x < cursor + w; x++) {
                    spend();
                    if (occupied.has(`${r},${x}`)) return false;
                }
                return true;
            };
            while (!free()) {
                spend();
                cursor++;
                irregular = true;
            }
            const cell = real[index++];
            if (
                !cell ||
                cell.source !== `${table.source}.${r}.${c}` ||
                cell.row !== r ||
                cell.column !== cursor ||
                cell.rowspan !== h ||
                cell.colspan !== w
            )
                throw new Error(
                    `fallback geometry/source order differs at ${table.source}.${r}.${c}`,
                );
            for (let y = r; y < r + h; y++)
                for (let x = cursor; x < cursor + w; x++) {
                    spend();
                    occupied.add(`${y},${x}`);
                    const rawWidths = raw.attrs?.colwidth;
                    const value = Array.isArray(rawWidths) ? rawWidths[x - cursor] : null;
                    if (typeof value === 'number' && value > 0) {
                        const old = widths[x];
                        if (!old || (old.value !== value && old.count === 1))
                            widths[x] = { value, count: 1 };
                        else if (old.value === value) old.count++;
                    }
                }
            cursor += w;
            columns = Math.max(columns, cursor);
        }
    }
    if (
        real.length !== index ||
        table.rows !== rows.length ||
        table.columns !== columns ||
        table.widths === null ||
        table.widths.length !== columns ||
        table.widths.some((value, x) => value !== (widths[x]?.value ?? null))
    )
        throw new Error('fallback geometry/widths extent differs');
    if (!irregular && occupied.size === rows.length * columns)
        throw new Error('regular table falsely labelled as overlap fallback');
}
