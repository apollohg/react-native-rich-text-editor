import {
    TABLE_TOOLBAR_COMPACT_MIN_WIDTH,
    TABLE_TOOLBAR_GAP,
    keyboardSafeViewport,
    placeTableToolbar,
    resolveTableToolbarPlacement,
    unionRects,
    windowToHostRect,
    type Rect,
} from '../TableToolbarPlacement';

const PHONE_WINDOW: Rect = { x: 0, y: 0, width: 390, height: 600 };
const SELECTION: Rect = { x: 80, y: 100, width: 80, height: 40 };
const TOOLBAR = { width: 200, height: 44 };
const PLACED_ABOVE: Rect = { x: 20, y: 48, width: 200, height: 44 };
const SAFE_AREA: Rect = { x: 0, y: 47, width: 390, height: 719 };
const DOCKED_KEYBOARD: Rect = { x: 0, y: 500, width: 390, height: 300 };
const FLOATING_KEYBOARD: Rect = { x: 120, y: 260, width: 240, height: 200 };
const LEFT_OF_FLOATING_KEYBOARD: Rect = { x: 0, y: 47, width: 120, height: 719 };
const BELOW_FLOATING_KEYBOARD: Rect = { x: 0, y: 460, width: 390, height: 306 };
const ABOVE_DOCKED_KEYBOARD: Rect = { x: 0, y: 47, width: 390, height: 453 };
const TOP_SELECTION: Rect = { x: 150, y: 60, width: 90, height: 30 };
const EDGE_SELECTION: Rect = { x: 340, y: 300, width: 40, height: 30 };
const TALL_SELECTION: Rect = { x: 100, y: 10, width: 100, height: 580 };
const UNDER_DOCKED_KEYBOARD: Rect = { x: 40, y: 560, width: 100, height: 40 };
const LEFT_SELECTION: Rect = { x: 20, y: 300, width: 80, height: 40 };
const BELOW_SELECTION: Rect = { x: 150, y: 520, width: 90, height: 30 };
const HOST_ORIGIN = { x: 24, y: 96 };
const WIDE_TOOLBAR = { width: 520, height: 44 };
const NARROW_WINDOW: Rect = { x: 0, y: 0, width: TABLE_TOOLBAR_COMPACT_MIN_WIDTH - 1, height: 600 };
const SHORT_WINDOW: Rect = { x: 0, y: 0, width: 390, height: 30 };
const EMPTY_RECT: Rect = { x: 10, y: 10, width: 0, height: 20 };
const FULL_WINDOW: Rect = { x: 0, y: 0, width: 390, height: 844 };
const EDITOR_UNDER_HEADER: Rect = { x: 16, y: 90, width: 200, height: 500 };
const OUTSIDE_EDITOR: Rect = { x: 240, y: 300, width: 80, height: 40 };

function bottom(rect: Rect): number {
    return rect.y + rect.height;
}

describe('placeTableToolbar', () => {
    it('centres above the selection when there is room', () => {
        expect(placeTableToolbar(SELECTION, PHONE_WINDOW, TOOLBAR)).toEqual(PLACED_ABOVE);
    });

    it('places nothing without an anchor', () => {
        expect(placeTableToolbar(null, PHONE_WINDOW, TOOLBAR)).toBeNull();
    });

    it('places nothing for an anchor without area', () => {
        expect(placeTableToolbar(EMPTY_RECT, PHONE_WINDOW, TOOLBAR)).toBeNull();
    });

    it('drops below a selection that sits too close to the top of the safe area', () => {
        const frame = placeTableToolbar(TOP_SELECTION, SAFE_AREA, TOOLBAR);

        expect(frame?.y).toBe(bottom(TOP_SELECTION) + TABLE_TOOLBAR_GAP);
    });

    it('clamps against the trailing edge instead of overflowing the window', () => {
        const frame = placeTableToolbar(EDGE_SELECTION, PHONE_WINDOW, TOOLBAR);

        expect(frame?.x).toBe(PHONE_WINDOW.width - TOOLBAR.width);
    });

    it('clamps into the safe area when neither side of the selection has room', () => {
        const frame = placeTableToolbar(TALL_SELECTION, PHONE_WINDOW, TOOLBAR);

        expect(frame?.y).toBe(bottom(PHONE_WINDOW) - TOOLBAR.height);
    });

    it('refuses a toolbar larger than the safe region', () => {
        expect(placeTableToolbar(SELECTION, PHONE_WINDOW, WIDE_TOOLBAR)).toBeNull();
    });
});

describe('keyboardSafeViewport', () => {
    it('is the whole safe area without a keyboard', () => {
        expect(keyboardSafeViewport({ safeArea: SAFE_AREA, keyboard: null }, [ SELECTION ], FULL_WINDOW)).toEqual(
            SAFE_AREA
        );
    });

    it('excludes a docked keyboard from the region above it', () => {
        expect(
            keyboardSafeViewport({ safeArea: SAFE_AREA, keyboard: DOCKED_KEYBOARD }, [ SELECTION ], FULL_WINDOW)
        ).toEqual(ABOVE_DOCKED_KEYBOARD);
    });

    it('keeps a floating keyboard as a rectangle and picks the free region holding the selection', () => {
        expect({
            left: keyboardSafeViewport({ safeArea: SAFE_AREA, keyboard: FLOATING_KEYBOARD }, [
                LEFT_SELECTION,
            ], FULL_WINDOW),
            below: keyboardSafeViewport({ safeArea: SAFE_AREA, keyboard: FLOATING_KEYBOARD }, [
                BELOW_SELECTION,
            ], FULL_WINDOW),
        }).toEqual({ left: LEFT_OF_FLOATING_KEYBOARD, below: BELOW_FLOATING_KEYBOARD });
    });

    it('has no region for a selection the keyboard covers completely', () => {
        expect(
            keyboardSafeViewport({ safeArea: SAFE_AREA, keyboard: DOCKED_KEYBOARD }, [
                UNDER_DOCKED_KEYBOARD,
            ], FULL_WINDOW)
        ).toBeNull();
    });

    it('has no region for a selection without a visible rectangle', () => {
        expect(keyboardSafeViewport({ safeArea: SAFE_AREA, keyboard: null }, [ EMPTY_RECT ], FULL_WINDOW)).toBeNull();
    });

    it('is bounded by the visible editor region, not just the window safe area', () => {
        expect({
            inside: keyboardSafeViewport(
                { safeArea: SAFE_AREA, keyboard: DOCKED_KEYBOARD },
                [ SELECTION ],
                EDITOR_UNDER_HEADER
            ),
            outside: keyboardSafeViewport(
                { safeArea: SAFE_AREA, keyboard: null },
                [ OUTSIDE_EDITOR ],
                EDITOR_UNDER_HEADER
            ),
        }).toEqual({
            inside: {
                x: EDITOR_UNDER_HEADER.x,
                y: EDITOR_UNDER_HEADER.y,
                width: EDITOR_UNDER_HEADER.width,
                height: DOCKED_KEYBOARD.y - EDITOR_UNDER_HEADER.y,
            },
            outside: null,
        });
    });

    it('places a toolbar inside the floating keyboard gap rather than above a bottom inset', () => {
        const safe = keyboardSafeViewport({ safeArea: SAFE_AREA, keyboard: FLOATING_KEYBOARD }, [
            LEFT_SELECTION,
        ], FULL_WINDOW);

        const placement =
            safe == null ? null : resolveTableToolbarPlacement(LEFT_SELECTION, safe, TOOLBAR);

        expect(placement).toEqual({
            compact: true,
            frame: {
                x: LEFT_OF_FLOATING_KEYBOARD.x,
                y: LEFT_SELECTION.y - TABLE_TOOLBAR_GAP - TOOLBAR.height,
                width: LEFT_OF_FLOATING_KEYBOARD.width,
                height: TOOLBAR.height,
            },
        });
    });
});

describe('resolveTableToolbarPlacement', () => {
    it('uses the full toolbar when it fits', () => {
        expect(resolveTableToolbarPlacement(SELECTION, PHONE_WINDOW, TOOLBAR)).toEqual({
            frame: PLACED_ABOVE,
            compact: false,
        });
    });

    it('falls back to a compact overflow strip the width of the safe region', () => {
        expect(resolveTableToolbarPlacement(SELECTION, PHONE_WINDOW, WIDE_TOOLBAR)).toEqual({
            compact: true,
            frame: { ...PLACED_ABOVE, x: PHONE_WINDOW.x, width: PHONE_WINDOW.width },
        });
    });

    it('hides when even the compact strip cannot fit', () => {
        expect({
            narrow: resolveTableToolbarPlacement(SELECTION, NARROW_WINDOW, WIDE_TOOLBAR),
            short: resolveTableToolbarPlacement(SELECTION, SHORT_WINDOW, TOOLBAR),
        }).toEqual({ narrow: null, short: null });
    });
});

describe('selection and host geometry', () => {
    it('unions only the rectangles that have area', () => {
        expect({
            union: unionRects([ SELECTION, EMPTY_RECT, TOP_SELECTION ]),
            none: unionRects([ EMPTY_RECT ]),
        }).toEqual({
            union: {
                x: SELECTION.x,
                y: TOP_SELECTION.y,
                width: TOP_SELECTION.x + TOP_SELECTION.width - SELECTION.x,
                height: bottom(SELECTION) - TOP_SELECTION.y,
            },
            none: null,
        });
    });

    it('converts a window frame into host coordinates once', () => {
        expect(windowToHostRect(PLACED_ABOVE, HOST_ORIGIN)).toEqual({
            x: PLACED_ABOVE.x - HOST_ORIGIN.x,
            y: PLACED_ABOVE.y - HOST_ORIGIN.y,
            width: PLACED_ABOVE.width,
            height: PLACED_ABOVE.height,
        });
    });
});
