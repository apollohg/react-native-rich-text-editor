import { BUTTON_HIT } from './EditorToolbarRegistry';

export interface Rect {
    x: number;
    y: number;
    width: number;
    height: number;
}

export interface Size {
    width: number;
    height: number;
}

export interface TableToolbarObstructions {
    safeArea: Rect;
    keyboard: Rect | null;
}

export interface TableToolbarPlacement {
    frame: Rect;
    compact: boolean;
}

export const TABLE_TOOLBAR_GAP = 8;

export const TABLE_TOOLBAR_COMPACT_MIN_WIDTH = BUTTON_HIT * 2;

export function placeTableToolbar(
    anchor: Rect | null,
    safe: Rect,
    size: Size,
    gap = TABLE_TOOLBAR_GAP
): Rect | null {
    if (
        !anchor ||
        anchor.width <= 0 ||
        anchor.height <= 0 ||
        size.width > safe.width ||
        size.height > safe.height
    ) {
        return null;
    }

    const clamp = (n: number, low: number, high: number) => Math.min(high, Math.max(low, n));
    const above = anchor.y - gap - size.height;
    const below = anchor.y + anchor.height + gap;

    return {
        x: clamp(anchor.x + (anchor.width - size.width) / 2, safe.x, safe.x + safe.width - size.width),
        y: clamp(above >= safe.y ? above : below, safe.y, safe.y + safe.height - size.height),
        width: size.width,
        height: size.height,
    };
}

function area(rect: Rect): number {
    return rect.width * rect.height;
}

export function isFiniteRect(rect: Rect): boolean {
    return (
        Number.isFinite(rect.x) &&
        Number.isFinite(rect.y) &&
        Number.isFinite(rect.width) &&
        Number.isFinite(rect.height) &&
        rect.width >= 0 &&
        rect.height >= 0
    );
}

export function isFiniteSize(size: Size): boolean {
    return (
        Number.isFinite(size.width) &&
        Number.isFinite(size.height) &&
        size.width >= 0 &&
        size.height >= 0
    );
}

export function intersectRects(left: Rect, right: Rect): Rect | null {
    const x = Math.max(left.x, right.x);
    const y = Math.max(left.y, right.y);
    const maxX = Math.min(left.x + left.width, right.x + right.width);
    const maxY = Math.min(left.y + left.height, right.y + right.height);

    return maxX > x && maxY > y ? { x, y, width: maxX - x, height: maxY - y } : null;
}

export function unionRects(rects: readonly Rect[]): Rect | null {
    const visible = rects.filter(rect => rect.width > 0 && rect.height > 0);

    if (visible.length === 0) {
        return null;
    }

    const x = Math.min(...visible.map(rect => rect.x));
    const y = Math.min(...visible.map(rect => rect.y));
    const maxX = Math.max(...visible.map(rect => rect.x + rect.width));
    const maxY = Math.max(...visible.map(rect => rect.y + rect.height));

    return { x, y, width: maxX - x, height: maxY - y };
}

function keyboardFreeRegions(safe: Rect, keyboard: Rect): Rect[] {
    const safeMaxX = safe.x + safe.width;
    const safeMaxY = safe.y + safe.height;
    const keyboardMaxX = keyboard.x + keyboard.width;
    const keyboardMaxY = keyboard.y + keyboard.height;

    return [
        { x: safe.x, y: safe.y, width: safe.width, height: keyboard.y - safe.y },
        { x: safe.x, y: keyboardMaxY, width: safe.width, height: safeMaxY - keyboardMaxY },
        { x: safe.x, y: safe.y, width: keyboard.x - safe.x, height: safe.height },
        { x: keyboardMaxX, y: safe.y, width: safeMaxX - keyboardMaxX, height: safe.height },
    ].filter(region => region.width > 0 && region.height > 0);
}

export function keyboardSafeViewport(
    obstructions: TableToolbarObstructions,
    selection: readonly Rect[],
    visibleRegion: Rect
): Rect | null {
    const anchor = unionRects(selection);
    const safeArea = intersectRects(obstructions.safeArea, visibleRegion);

    if (anchor == null || safeArea == null || intersectRects(anchor, safeArea) == null) {
        return null;
    }

    const keyboard =
        obstructions.keyboard == null ? null : intersectRects(obstructions.keyboard, safeArea);

    if (keyboard == null) {
        return safeArea;
    }

    let best: { region: Rect; visible: number } | null = null;

    for (const region of keyboardFreeRegions(safeArea, keyboard)) {
        const visibleSelection = intersectRects(anchor, region);

        if (visibleSelection == null) {
            continue;
        }

        const visible = area(visibleSelection);

        if (
            best == null ||
            visible > best.visible ||
            (visible === best.visible && area(region) > area(best.region))
        ) {
            best = { region, visible };
        }
    }

    return best?.region ?? null;
}

export function resolveTableToolbarPlacement(
    anchor: Rect,
    safe: Rect,
    size: Size
): TableToolbarPlacement | null {
    const frame = placeTableToolbar(anchor, safe, size);

    if (frame != null) {
        return { frame, compact: false };
    }

    if (safe.width < TABLE_TOOLBAR_COMPACT_MIN_WIDTH) {
        return null;
    }

    const compactFrame = placeTableToolbar(anchor, safe, {
        width: Math.min(size.width, safe.width),
        height: size.height,
    });

    return compactFrame == null ? null : { frame: compactFrame, compact: true };
}

export function windowToHostRect(frame: Rect, hostOrigin: Pick<Rect, 'x' | 'y'>): Rect {
    return {
        x: frame.x - hostOrigin.x,
        y: frame.y - hostOrigin.y,
        width: frame.width,
        height: frame.height,
    };
}
