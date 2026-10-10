import { NativeEditorEngineBoundaryError } from './NativeEditorBoundaryError';
import { invalidV2RequestError, nativeEditorV2U32 } from './NativeEditorResultNormalization';
import {
    type NativeEditorLocalAwarenessSelection,
    NativeEditorLocalAwarenessCellSelectionValue,
    NativeEditorLocalAwarenessSelectionValue,
} from './NativeEditorTypes';
import { normalizeV2JsonValue, serializeV2CreateEnvelope } from './NativeEditorCreateJson';

export const LOCAL_AWARENESS_INTENT_KEYS = new Set([ 'state', 'focused', 'selection' ]);

export const LOCAL_AWARENESS_TEXT_SELECTION = 'text';
export const LOCAL_AWARENESS_CELL_SELECTION = 'cell';

export type NativeEditorLocalAwarenessWireSelection =
    | { type: typeof LOCAL_AWARENESS_TEXT_SELECTION; anchor: number; head: number }
    | { type: typeof LOCAL_AWARENESS_CELL_SELECTION; anchorCell: number; headCell: number };

export const LOCAL_AWARENESS_SELECTION_VALUES = new WeakMap<
    object,
    Readonly<NativeEditorLocalAwarenessWireSelection>
>();

export function invalidLocalAwarenessIntent(message = 'invalid local awareness intent'): never {
    throw invalidV2RequestError(`NativeEditorBridge: ${message}`);
}

function acceptedLocalAwarenessPositions(first: number, second: number): [number, number] {
    const acceptedFirst = nativeEditorV2U32(first);
    const acceptedSecond = nativeEditorV2U32(second);

    if (acceptedFirst == null || acceptedSecond == null) {
        invalidLocalAwarenessIntent();
    }

    return [ acceptedFirst, acceptedSecond ];
}

function registerLocalAwarenessSelection<Selection extends NativeEditorLocalAwarenessSelection>(
    selection: Selection,
    wire: NativeEditorLocalAwarenessWireSelection
): Selection {
    Object.freeze(selection);
    LOCAL_AWARENESS_SELECTION_VALUES.set(selection, Object.freeze(wire));

    return selection;
}

/**
 * Create the only caller-owned local-awareness selection accepted at the
 * JavaScript-to-native boundary. The private WeakMap makes provenance an API
 * capability instead of a structural object-shape check.
 */
export function createNativeEditorLocalAwarenessSelection(
    anchor: number,
    head: number
): NativeEditorLocalAwarenessSelection {
    const [ acceptedAnchor, acceptedHead ] = acceptedLocalAwarenessPositions(anchor, head);

    return registerLocalAwarenessSelection(
        new NativeEditorLocalAwarenessSelectionValue(acceptedAnchor, acceptedHead),
        { type: LOCAL_AWARENESS_TEXT_SELECTION, anchor: acceptedAnchor, head: acceptedHead }
    );
}

export function createNativeEditorLocalAwarenessCellSelection(
    anchorCell: number,
    headCell: number
): NativeEditorLocalAwarenessSelection {
    const [ acceptedAnchorCell, acceptedHeadCell ] = acceptedLocalAwarenessPositions(
        anchorCell,
        headCell
    );

    return registerLocalAwarenessSelection(
        new NativeEditorLocalAwarenessCellSelectionValue(acceptedAnchorCell, acceptedHeadCell),
        {
            type: LOCAL_AWARENESS_CELL_SELECTION,
            anchorCell: acceptedAnchorCell,
            headCell: acceptedHeadCell,
        }
    );
}

export function isLocalAwarenessRecord(value: unknown): value is Record<string, unknown> {
    if (value == null || typeof value !== 'object' || Array.isArray(value)) {
        return false;
    }

    const prototype = Object.getPrototypeOf(value);

    return prototype === Object.prototype || prototype === null;
}

export function localAwarenessOwnDataValue(record: Record<string, unknown>, key: string): unknown {
    const descriptor = Object.getOwnPropertyDescriptor(record, key);

    if (descriptor === undefined || !('value' in descriptor) || descriptor.enumerable !== true) {
        invalidLocalAwarenessIntent();
    }

    return descriptor.value;
}

export function validateLocalAwarenessSelection(
    selection: unknown
): Readonly<NativeEditorLocalAwarenessWireSelection> {
    if (selection == null || typeof selection !== 'object') {
        invalidLocalAwarenessIntent();
    }

    // Do not inspect caller-held values before provenance succeeds: a Proxy
    // can imitate every structural and own-data check, but it cannot inherit
    // this module's WeakMap identity.
    const factoryValue = LOCAL_AWARENESS_SELECTION_VALUES.get(selection);

    if (factoryValue === undefined) {
        invalidLocalAwarenessIntent();
    }

    return factoryValue;
}

/** Reject caller-owned sticky cursor data before a native call can occur. */
export function rejectReservedAwarenessCursor(value: unknown): void {
    const pending: unknown[] = [ value ];
    const seen = new WeakSet<object>();

    while (pending.length > 0) {
        const current = pending.pop();

        if (current == null || typeof current !== 'object') {
            continue;
        }

        if (seen.has(current)) {
            continue;
        }

        seen.add(current);

        for (const key of Reflect.ownKeys(current)) {
            if (key === 'cursor') {
                invalidLocalAwarenessIntent('reserved cursor key is not allowed');
            }

            const descriptor = Object.getOwnPropertyDescriptor(current, key);

            if (descriptor == null || !('value' in descriptor)) {
                invalidLocalAwarenessIntent();
            }

            pending.push(descriptor.value);
        }
    }
}

export function normalizeLocalAwarenessState(value: unknown): Record<string, unknown> {
    try {
        const normalized = normalizeV2JsonValue(value, 'local awareness state', {
            seen: new WeakSet<object>(),
            work: 0,
        });

        if (!isLocalAwarenessRecord(normalized) || Object.getPrototypeOf(normalized) !== null) {
            invalidLocalAwarenessIntent();
        }

        rejectReservedAwarenessCursor(normalized);

        return normalized;
    } catch (error) {
        if (error instanceof NativeEditorEngineBoundaryError) {
            throw error;
        }

        invalidLocalAwarenessIntent();
    }
}

export interface NativeEditorLocalAwarenessWireIntent {
    state: Record<string, unknown>;
    focused: boolean;
    /** Absent retains the Rust-owned cursor; `null` clears it. */
    selection?: Readonly<NativeEditorLocalAwarenessWireSelection> | null;
}

export function validateLocalAwarenessIntent(
    intent: unknown
): NativeEditorLocalAwarenessWireIntent {
    try {
        if (
            !isLocalAwarenessRecord(intent) ||
            Reflect.ownKeys(intent).some(
                key => typeof key !== 'string' || !LOCAL_AWARENESS_INTENT_KEYS.has(key)
            ) ||
            !Object.prototype.hasOwnProperty.call(intent, 'state') ||
            !Object.prototype.hasOwnProperty.call(intent, 'focused')
        ) {
            invalidLocalAwarenessIntent();
        }

        const state = normalizeLocalAwarenessState(localAwarenessOwnDataValue(intent, 'state'));
        const focused = localAwarenessOwnDataValue(intent, 'focused');

        if (typeof focused !== 'boolean') {
            invalidLocalAwarenessIntent();
        }

        if (!Object.prototype.hasOwnProperty.call(intent, 'selection')) {
            // Absent: retain whatever cursor Rust already holds.
            return { state, focused };
        }

        const rawSelection = localAwarenessOwnDataValue(intent, 'selection');

        if (rawSelection === null) {
            return { state, focused, selection: null };
        }

        return { state, focused, selection: validateLocalAwarenessSelection(rawSelection) };
    } catch (error) {
        if (error instanceof NativeEditorEngineBoundaryError) {
            throw error;
        }

        invalidLocalAwarenessIntent();
    }
}

export function serializeLocalAwarenessIntent(
    intent: NativeEditorLocalAwarenessWireIntent
): string {
    try {
        const wire = Object.create(null) as Record<string, unknown>;
        wire.state = intent.state;
        wire.focused = intent.focused;

        if (intent.selection === null) {
            wire.selection = null;
        } else if (intent.selection !== undefined) {
            wire.selection = Object.assign(Object.create(null), intent.selection);
        }

        return serializeV2CreateEnvelope(wire);
    } catch {
        invalidLocalAwarenessIntent();
    }
}
