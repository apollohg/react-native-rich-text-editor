import {
    type ListContext,
    type RenderElement,
    type RenderBlocksPatch,
    type Selection,
    type ActiveState,
    type HistoryState,
    type NativeEditorAtomicRenderSnapshot,
    type NativeEditorAtomicRenderPayload,
    type DocumentJSON,
    type ContentSnapshot,
} from './NativeEditorTypes';
import {
    isPlainRecord,
    hasOnlyOwnKeys,
    nativeEditorV2U32,
    RENDER_ELEMENT_TYPES,
    hasExactOwnKeys,
    validRenderMark,
    normalizeNativeEditorV2DecimalId,
    booleanRecord,
    stringArray,
    optionalBoolean,
    parseNativeEditorV2JsonValue,
    normalizeRevisionField,
    normalizeNativeEditorV2Bytes,
} from './NativeEditorResultNormalization';
import { validEditorMentionTheme } from './EditorMentionThemeValidation';

export function validListContext(value: unknown): value is ListContext {
    if (!isPlainRecord(value)) {
        return false;
    }

    return (
        hasOnlyOwnKeys(value, [
            'ordered',
            'index',
            'total',
            'start',
            'isFirst',
            'isLast',
            'kind',
            'checked',
        ]) &&
        typeof value.ordered === 'boolean' &&
        nativeEditorV2U32(value.index) != null &&
        nativeEditorV2U32(value.total) != null &&
        nativeEditorV2U32(value.start) != null &&
        typeof value.isFirst === 'boolean' &&
        typeof value.isLast === 'boolean' &&
        (value.kind == null || typeof value.kind === 'string') &&
        (value.checked == null || typeof value.checked === 'boolean')
    );
}

function validLeafRenderElement(value: unknown): value is RenderElement {
    if (
        !isPlainRecord(value) ||
        !RENDER_ELEMENT_TYPES.has(value.type as RenderElement['type'])
    ) {
        return false;
    }

    switch (value.type) {
        case 'textRun':
            return (
                hasExactOwnKeys(value, ['type', 'text', 'marks']) &&
                typeof value.text === 'string' &&
                Array.isArray(value.marks) &&
                value.marks.every(validRenderMark)
            );
        case 'blockStart':
            return (
                hasOnlyOwnKeys(value, [
                    'type',
                    'nodeType',
                    'depth',
                    'listContext',
                    'language',
                ]) &&
                typeof value.nodeType === 'string' &&
                nativeEditorV2U32(value.depth) != null &&
                (value.language === undefined ||
                    typeof value.language === 'string') &&
                (value.listContext === undefined ||
                    validListContext(value.listContext))
            );
        case 'blockEnd':
            return hasExactOwnKeys(value, ['type']);
        case 'voidInline':
            return (
                hasOnlyOwnKeys(value, [
                    'type',
                    'nodeType',
                    'docPos',
                    'attrs',
                ]) &&
                typeof value.nodeType === 'string' &&
                nativeEditorV2U32(value.docPos) != null &&
                (value.attrs === undefined || isPlainRecord(value.attrs))
            );
        case 'voidBlock':
            return (
                hasOnlyOwnKeys(value, [
                    'type',
                    'nodeType',
                    'docPos',
                    'attrs',
                    'atomId',
                ]) &&
                typeof value.nodeType === 'string' &&
                nativeEditorV2U32(value.docPos) != null &&
                (value.attrs === undefined || isPlainRecord(value.attrs)) &&
                (value.atomId === undefined || typeof value.atomId === 'string')
            );
        case 'opaqueInlineAtom':
            return (
                hasOnlyOwnKeys(value, [
                    'type',
                    'nodeType',
                    'label',
                    'docPos',
                    'attrs',
                    'mentionTheme',
                ]) &&
                typeof value.nodeType === 'string' &&
                typeof value.label === 'string' &&
                nativeEditorV2U32(value.docPos) != null &&
                (value.attrs === undefined || isPlainRecord(value.attrs)) &&
                (value.mentionTheme === undefined ||
                    validEditorMentionTheme(value.mentionTheme))
            );
        case 'opaqueBlockAtom':
            return (
                hasOnlyOwnKeys(value, [
                    'type',
                    'nodeType',
                    'label',
                    'docPos',
                    'attrs',
                ]) &&
                typeof value.nodeType === 'string' &&
                typeof value.label === 'string' &&
                nativeEditorV2U32(value.docPos) != null &&
                (value.attrs === undefined || isPlainRecord(value.attrs))
            );
    }

    return false;
}

export function normalizeRenderBlocks(
    value: unknown,
    tableAttributes: unknown = {},
    tableRecords: unknown = {},
): RenderElement[][] | null {
    if (!Array.isArray(value)) {
        return null;
    }

    if (!value.every(Array.isArray)) return null;
    return validRenderElements(value.flat(), tableAttributes, tableRecords)
        ? (value as RenderElement[][])
        : null;
}

const tableFailures = new Set([
    'gridLimit',
    'workLimit',
    'allocation',
    'invalidStructure',
    'invalidAttributes',
]);
const tableDiagnostics = new Set([
    'virtual-grid-limit',
    'empty-reference-surface',
    'unsupported-row-role',
    'unsupported-cell-role',
    'ambiguous-source-map',
    'unsupported-gap-default',
    'overlapping-reference-cells',
    'unmapped-reference-cell',
    'nonrectangular-reference-cell',
    'zero-span-after-reference-pass',
]);

export function validRenderElement(value: unknown): value is RenderElement {
    return validRenderElements([value]);
}

function utf8Bytes(value: string): number | null {
    let bytes = 0;
    for (let index = 0; index < value.length; index++) {
        const code = value.charCodeAt(index);
        if (code >= 0xd800 && code <= 0xdbff) {
            const low = value.charCodeAt(index + 1);
            if (low < 0xdc00 || low > 0xdfff) return null;
            bytes += 4;
            index++;
        } else if (code >= 0xdc00 && code <= 0xdfff) {
            return null;
        } else if (code <= 0x7f) {
            bytes++;
        } else if (code <= 0x7ff) {
            bytes += 2;
        } else {
            bytes += 3;
        }
    }
    return bytes;
}

export function normalizeTableAttributes(
    value: unknown,
): Record<string, string> | null {
    if (!isPlainRecord(value)) return null;
    let bytes = 0;
    let entryCount = 0;
    const payloads = new Set<string>();
    for (const [key, json] of Object.entries(value)) {
        if (
            !/^[0-9a-f]{64}$/.test(key) ||
            typeof json !== 'string' ||
            payloads.has(json)
        )
            return null;
        payloads.add(json);
        const keyBytes = utf8Bytes(key);
        const jsonBytes = utf8Bytes(json);
        if (keyBytes === null || jsonBytes === null || ++entryCount > 7_000_000)
            return null;
        bytes += jsonBytes;
        if (bytes > 192 * 1024 * 1024) return null;
        const parsed = parseNativeEditorV2JsonValue(json);
        if (!isPlainRecord(parsed)) return null;
        const pending = [{ value: parsed as unknown, depth: 0 }];
        let work = 0;
        while (pending.length) {
            const entry = pending.pop()!;
            if (++work > json.length || entry.depth > 1024) return null;
            if (
                typeof entry.value === 'number' &&
                !Number.isFinite(entry.value)
            )
                return null;
            if (entry.value !== null && typeof entry.value === 'object') {
                for (const child of Object.values(entry.value))
                    pending.push({ value: child, depth: entry.depth + 1 });
            }
        }
    }
    return value as Record<string, string>;
}

function validRenderElements(
    roots: unknown[],
    tableAttributes: unknown = {},
    tableRecords: unknown = {},
    requireAllRecords = true,
): boolean {
    const pool = normalizeTableAttributes(tableAttributes);
    if (pool === null) return false;
    if (!isPlainRecord(tableRecords)) return false;
    const records = tableRecords as Record<string, unknown>;
    if (
        !Object.entries(records).every(
            ([id, record]) => id.length > 0 && isPlainRecord(record),
        )
    )
        return false;
    const pending = roots.map((value) => ({
        value,
        depth: 0,
        start: 0,
        end: 0xffff_ffff,
    }));
    let nodes = 0;
    let gridSlots = 0;
    const referencedTableIds = new Set<string>();
    const referencedAttributeKeys = new Set<string>();
    const attrs = (value: unknown): boolean => {
        if (
            typeof value !== 'string' ||
            !Object.prototype.hasOwnProperty.call(pool, value)
        )
            return false;
        referencedAttributeKeys.add(value);
        return true;
    };
    const u32 = (value: unknown): value is number =>
        nativeEditorV2U32(value) != null;
    while (pending.length) {
        if (++nodes + pending.length > 7_000_000) return false;
        const { value, depth, start, end } = pending.pop()!;
        if (!isPlainRecord(value) || depth > 1024) return false;
        if (value.type !== 'table') {
            if (!validLeafRenderElement(value)) return false;
            if (
                value.docPos !== undefined &&
                (!u32(value.docPos) ||
                    value.docPos < start ||
                    value.docPos >= end)
            )
                return false;
            continue;
        }
        if (
            !hasExactOwnKeys(value, ['type', 'tableId']) ||
            typeof value.tableId !== 'string' ||
            !/^t(?:0|[1-9][0-9]*)$/.test(value.tableId)
        )
            return false;
        if (referencedTableIds.has(value.tableId)) return false;
        const t = records[value.tableId];
        if (!isPlainRecord(t)) return false;
        referencedTableIds.add(value.tableId);
        if (
            !hasExactOwnKeys(t, [
                'tablePos',
                'sourceEnd',
                'rows',
                'columns',
                'columnWidths',
                'direction',
                'irregular',
                'readOnlyDescendants',
                'attrsKey',
                'sourceRows',
                'cells',
                'syntheticRegions',
                'failure',
                'compatibilityDiagnostic',
            ]) ||
            !u32(t.tablePos) ||
            value.tableId !== `t${t.tablePos}` ||
            !u32(t.sourceEnd) ||
            t.tablePos < start ||
            t.sourceEnd > end ||
            t.sourceEnd <= t.tablePos ||
            !u32(t.rows) ||
            !u32(t.columns) ||
            !Array.isArray(t.columnWidths) ||
            t.columnWidths.length !== t.columns ||
            !t.columnWidths.every(
                (width) => width === null || (u32(width) && width > 0),
            ) ||
            ![null, 'ltr', 'rtl'].includes(t.direction as string | null) ||
            typeof t.irregular !== 'boolean' ||
            t.readOnlyDescendants !== depth > 0 ||
            !attrs(t.attrsKey) ||
            !Array.isArray(t.sourceRows) ||
            !Array.isArray(t.cells) ||
            !Array.isArray(t.syntheticRegions) ||
            (t.failure !== null && !tableFailures.has(t.failure as string)) ||
            (t.compatibilityDiagnostic !== null &&
                !tableDiagnostics.has(t.compatibilityDiagnostic as string))
        )
            return false;
        gridSlots += t.rows * t.columns;
        if (
            t.rows > 4_000_000 ||
            t.columns > 4_000_000 ||
            gridSlots > 4_000_000
        )
            return false;
        if (t.failure !== null) {
            if (
                t.rows !== 0 ||
                t.columns !== 0 ||
                t.cells.length ||
                t.sourceRows.length ||
                t.syntheticRegions.length ||
                t.compatibilityDiagnostic !== null
            )
                return false;
            continue;
        }
        nodes +=
            t.cells.length + t.sourceRows.length + t.syntheticRegions.length;
        if (nodes > 7_000_000) return false;
        let priorRowEnd = t.tablePos + 1;
        for (const row of t.sourceRows) {
            if (
                !isPlainRecord(row) ||
                !hasExactOwnKeys(row, ['sourcePos', 'sourceEnd', 'attrsKey']) ||
                !u32(row.sourcePos) ||
                !u32(row.sourceEnd) ||
                row.sourcePos < priorRowEnd ||
                row.sourceEnd <= row.sourcePos ||
                row.sourceEnd >= t.sourceEnd ||
                !attrs(row.attrsKey)
            )
                return false;
            priorRowEnd = row.sourceEnd;
        }
        const occupied = new Set<number>();
        const sources = new Set<number>();
        let previousCellEnd = t.tablePos + 1;
        let sourceRowIndex = 0;
        for (const [synthetic, regions] of [
            [false, t.cells],
            [true, t.syntheticRegions],
        ] as const) {
            for (const region of regions) {
                if (
                    !isPlainRecord(region) ||
                    !hasExactOwnKeys(
                        region,
                        synthetic
                            ? [
                                  'row',
                                  'column',
                                  'rowspan',
                                  'colspan',
                                  'header',
                                  'attrsKey',
                              ]
                            : [
                                  'sourcePos',
                                  'sourceEnd',
                                  'row',
                                  'column',
                                  'rowspan',
                                  'colspan',
                                  'header',
                                  'attrsKey',
                                  'contentKey',
                                  'elements',
                              ],
                    ) ||
                    !u32(region.row) ||
                    !u32(region.column) ||
                    !u32(region.rowspan) ||
                    !u32(region.colspan) ||
                    region.rowspan === 0 ||
                    region.colspan === 0 ||
                    region.row + region.rowspan > t.rows ||
                    region.column + region.colspan > t.columns ||
                    typeof region.header !== 'boolean' ||
                    !attrs(region.attrsKey)
                )
                    return false;
                for (let r = region.row; r < region.row + region.rowspan; r++) {
                    for (
                        let c = region.column;
                        c < region.column + region.colspan;
                        c++
                    ) {
                        const slot = r * t.columns + c;
                        if (occupied.has(slot)) return false;
                        occupied.add(slot);
                    }
                }
                if (synthetic) continue;
                if (
                    !u32(region.sourcePos) ||
                    !u32(region.sourceEnd) ||
                    region.sourceEnd <= region.sourcePos ||
                    region.sourcePos < previousCellEnd ||
                    sources.has(region.sourcePos) ||
                    typeof region.contentKey !== 'string' ||
                    !region.contentKey.length ||
                    !Array.isArray(region.elements)
                )
                    return false;
                while (
                    sourceRowIndex < t.sourceRows.length &&
                    t.sourceRows[sourceRowIndex].sourceEnd <= region.sourcePos
                )
                    sourceRowIndex++;
                const sourceRow = t.sourceRows[sourceRowIndex];
                if (
                    !sourceRow ||
                    sourceRow.sourcePos >= region.sourcePos ||
                    sourceRow.sourceEnd <= region.sourceEnd
                )
                    return false;
                previousCellEnd = region.sourceEnd;
                sources.add(region.sourcePos);
                for (const element of region.elements)
                    pending.push({
                        value: element,
                        depth: depth + 1,
                        start: region.sourcePos + 1,
                        end: region.sourceEnd - 1,
                    });
            }
        }
    }
    return (
        !requireAllRecords ||
        (referencedTableIds.size === Object.keys(records).length &&
            referencedAttributeKeys.size === Object.keys(pool).length)
    );
}

export function normalizeRenderPatch(
    value: unknown,
    tableAttributes: unknown = {},
    tableRecords: unknown = {},
): RenderBlocksPatch | null | undefined {
    if (value === null) {
        return null;
    }

    if (
        !isPlainRecord(value) ||
        !hasExactOwnKeys(value, [
            'baseDocumentVersion',
            'startIndex',
            'deleteCount',
            'renderBlocks',
        ])
    ) {
        return undefined;
    }

    if (
        !Array.isArray(value.renderBlocks) ||
        !value.renderBlocks.every(Array.isArray) ||
        !validRenderElements(
            value.renderBlocks.flat(),
            tableAttributes,
            tableRecords,
            false,
        )
    )
        return undefined;
    const renderBlocks = value.renderBlocks as RenderElement[][];
    const baseDocumentVersion = normalizeNativeEditorV2DecimalId(
        value.baseDocumentVersion,
    );
    const startIndex = nativeEditorV2U32(value.startIndex);
    const deleteCount = nativeEditorV2U32(value.deleteCount);

    if (
        renderBlocks == null ||
        baseDocumentVersion == null ||
        startIndex == null ||
        deleteCount == null
    ) {
        return undefined;
    }

    return { baseDocumentVersion, startIndex, deleteCount, renderBlocks };
}

export function normalizeRenderSelection(value: unknown): Selection | null {
    if (!isPlainRecord(value)) {
        return null;
    }

    if (value.type === 'all') {
        return hasExactOwnKeys(value, ['type']) ? { type: 'all' } : null;
    }

    if (value.type === 'text') {
        if (
            !hasExactOwnKeys(value, [
                'type',
                'anchor',
                'head',
                'anchorScalar',
                'headScalar',
            ])
        ) {
            return null;
        }

        const anchor = nativeEditorV2U32(value.anchor);
        const head = nativeEditorV2U32(value.head);
        const anchorScalar = nativeEditorV2U32(value.anchorScalar);
        const headScalar = nativeEditorV2U32(value.headScalar);

        if (
            anchor == null ||
            head == null ||
            anchorScalar == null ||
            headScalar == null
        ) {
            return null;
        }

        return {
            type: 'text',
            anchor,
            head,
            anchorScalar,
            headScalar,
        };
    }

    if (value.type === 'cell') {
        if (!hasExactOwnKeys(value, ['type', 'anchorCell', 'headCell'])) {
            return null;
        }

        const anchorCell = nativeEditorV2U32(value.anchorCell);
        const headCell = nativeEditorV2U32(value.headCell);

        if (anchorCell == null || headCell == null) {
            return null;
        }

        return { type: 'cell', anchorCell, headCell };
    }

    if (value.type === 'node') {
        if (!hasExactOwnKeys(value, ['type', 'pos', 'posScalar'])) {
            return null;
        }

        const pos = nativeEditorV2U32(value.pos);
        const posScalar = nativeEditorV2U32(value.posScalar);

        if (pos == null || posScalar == null) {
            return null;
        }

        return { type: 'node', pos, posScalar };
    }

    return null;
}

export function normalizeRenderActiveState(value: unknown): ActiveState | null {
    if (!isPlainRecord(value)) {
        return null;
    }

    if (
        !hasExactOwnKeys(value, [
            'marks',
            'markAttrs',
            'nodes',
            'commands',
            'allowedMarks',
            'insertableNodes',
        ]) ||
        !booleanRecord(value.marks) ||
        !isPlainRecord(value.markAttrs) ||
        !Object.values(value.markAttrs).every(isPlainRecord) ||
        !booleanRecord(value.nodes) ||
        !booleanRecord(value.commands) ||
        !stringArray(value.allowedMarks) ||
        !stringArray(value.insertableNodes)
    ) {
        return null;
    }

    return {
        marks: value.marks,
        markAttrs: value.markAttrs as ActiveState['markAttrs'],
        nodes: value.nodes,
        commands: value.commands,
        allowedMarks: value.allowedMarks,
        insertableNodes: value.insertableNodes,
    };
}

export function normalizeRenderHistoryState(
    value: unknown,
): HistoryState | null {
    if (
        !isPlainRecord(value) ||
        !hasExactOwnKeys(value, ['canUndo', 'canRedo'])
    ) {
        return null;
    }

    const canUndo = optionalBoolean(value.canUndo);
    const canRedo = optionalBoolean(value.canRedo);

    return canUndo == null || canRedo == null ? null : { canUndo, canRedo };
}

export function deepFreezeV2Value<T>(value: T): T {
    if (value != null && typeof value === 'object' && !Object.isFrozen(value)) {
        for (const child of Object.values(value as Record<string, unknown>)) {
            deepFreezeV2Value(child);
        }

        Object.freeze(value);
    }

    return value;
}

/** Validate and freeze the one complete render/state snapshot. */
export function normalizeNativeEditorV2RenderUpdateValue(
    value: unknown,
): NativeEditorAtomicRenderSnapshot | null {
    const parsed = parseNativeEditorV2JsonValue(value);

    if (!isPlainRecord(parsed)) {
        return null;
    }

    if (
        !hasExactOwnKeys(parsed, [
            'renderBlocks',
            'renderPatch',
            'selection',
            'activeState',
            'historyState',
            'documentVersion',
            'stateRevision',
            'scalarLength',
            'documentIsEmpty',
            ...(Object.prototype.hasOwnProperty.call(parsed, 'tableAttributes')
                ? ['tableAttributes']
                : []),
            ...(Object.prototype.hasOwnProperty.call(parsed, 'tableRecords')
                ? ['tableRecords']
                : []),
        ])
    ) {
        return null;
    }

    const tableAttributes = normalizeTableAttributes(
        parsed.tableAttributes ?? {},
    );
    if (tableAttributes === null) return null;
    const tableRecords = parsed.tableRecords ?? {};
    const normalizedTableRecords = tableRecords as Record<
        string,
        import('./TableTypes').TableRenderRecord
    >;
    const renderBlocks =
        parsed.renderBlocks === null
            ? null
            : normalizeRenderBlocks(
                  parsed.renderBlocks,
                  tableAttributes,
                  tableRecords,
              );

    const renderPatch = normalizeRenderPatch(
        parsed.renderPatch,
        tableAttributes,
        tableRecords,
    );
    const selection = normalizeRenderSelection(parsed.selection);
    const activeState = normalizeRenderActiveState(parsed.activeState);
    const historyState = normalizeRenderHistoryState(parsed.historyState);
    const documentVersion = normalizeRevisionField(parsed, 'documentVersion');
    const stateRevision = normalizeRevisionField(parsed, 'stateRevision');
    const scalarLength = nativeEditorV2U32(parsed.scalarLength);
    const documentIsEmpty = parsed.documentIsEmpty;

    if (
        renderPatch === undefined ||
        selection == null ||
        activeState == null ||
        historyState == null ||
        documentVersion == null ||
        stateRevision == null ||
        scalarLength == null ||
        typeof documentIsEmpty !== 'boolean'
    ) {
        return null;
    }

    let renderPayload: NativeEditorAtomicRenderPayload;

    if (renderBlocks == null) {
        if (parsed.renderBlocks !== null || renderPatch == null) {
            return null;
        }

        renderPayload = { renderBlocks: null, renderPatch };
    } else {
        if (renderPatch !== null) {
            return null;
        }

        renderPayload = { renderBlocks, renderPatch: null };
    }

    return deepFreezeV2Value({
        ...renderPayload,
        ...(parsed.tableAttributes === undefined ? {} : { tableAttributes }),
        ...(parsed.tableRecords === undefined
            ? {}
            : { tableRecords: normalizedTableRecords }),
        selection,
        activeState,
        historyState,
        documentVersion,
        stateRevision,
        scalarLength,
        documentIsEmpty,
    });
}

export function normalizeNativeEditorV2DocumentJsonValue(
    value: unknown,
): DocumentJSON | null {
    const parsed = parseNativeEditorV2JsonValue(value);

    return isPlainRecord(parsed) ? parsed : null;
}

export function normalizeNativeEditorV2ContentSnapshotValue(
    value: unknown,
): ContentSnapshot | null {
    const parsed = parseNativeEditorV2JsonValue(value);

    if (
        !isPlainRecord(parsed) ||
        typeof parsed.html !== 'string' ||
        !isPlainRecord(parsed.json)
    ) {
        return null;
    }

    return { html: parsed.html, json: parsed.json };
}

export interface NativeEditorSnapshotExport {
    metadataJson: string;
    encodedState: Uint8Array;
}

/** The snapshot export record arrives as direct fields (JSON + bytes), not a JSON string. */
export function normalizeNativeEditorV2SnapshotExportValue(
    value: unknown,
): NativeEditorSnapshotExport | null {
    if (!isPlainRecord(value) || typeof value.metadataJson !== 'string') {
        return null;
    }

    const encodedState = normalizeNativeEditorV2Bytes(value.encodedState);

    if (encodedState == null) {
        return null;
    }

    return { metadataJson: value.metadataJson, encodedState };
}

export function normalizeNativeEditorV2CreateValue(
    value: unknown,
): { editorId: string } | null {
    const parsed = parseNativeEditorV2JsonValue(value);

    if (!isPlainRecord(parsed)) {
        return null;
    }

    const editorId = normalizeNativeEditorV2DecimalId(parsed.editorId);

    return editorId == null ? null : { editorId };
}
