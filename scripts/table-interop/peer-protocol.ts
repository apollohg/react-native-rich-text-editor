export const NATIVE_PEER_KIND = 'rust';
export interface JsonNode {
    type: string;
    attrs?: Record<string, unknown>;
    content?: JsonNode[];
    marks?: JsonNode[];
    text?: string;
    [key: string]: unknown;
}
export interface EffectiveCell {
    // Raw JSON path in this observation; null denotes a display-only gap.
    source: string | null;
    // Stable Y cell type identity, independent of its content items.
    sourceId?: string;
    rawPosition?: number;
    // Wrapper start on this peer's current surface, not a text cursor.
    position: number;
    row: number | null;
    column: number | null;
    rowspan: number | null;
    colspan: number | null;
    node: JsonNode;
}
export interface EffectiveTable {
    overlap?:
        | { kind: 'native-fallback'; reason: 'overlapping-reference-cells' }
        | {
              kind: 'web-overlap';
              logicalGeometry: 'unavailable';
              boxes: CellBox[];
          };
    source: string;
    parentCell: string | null;
    pathWithinCell: string;
    position: number;
    node: JsonNode;
    rows: number;
    columns: number;
    widths: (number | null)[] | null;
    cells: EffectiveCell[];
}
export interface CellBox {
    source: string;
    position: number;
    tableSource: string;
    left: number;
    top: number;
    right: number;
    bottom: number;
}
export interface EffectiveDocument {
    tables: EffectiveTable[];
}
export const WEB_PEER_KINDS = ['prosemirror', 'tiptap'] as const;
export type PeerKind = typeof NATIVE_PEER_KIND | (typeof WEB_PEER_KINDS)[number];
export type TableCommand =
    | { type: 'insertTable'; rows?: number; columns?: number; withHeaderRow?: boolean }
    | { type: 'deleteTable' }
    | { type: 'addTableRow'; side: 'before' | 'after' }
    | { type: 'deleteTableRows' }
    | { type: 'addTableColumn'; side: 'before' | 'after' }
    | { type: 'deleteTableColumns' }
    | { type: 'toggleTableHeader'; target: 'row' | 'column' | 'cell' }
    | { type: 'selectTableRows' }
    | { type: 'selectTableColumns' }
    | { type: 'clearTableCells' }
    | { type: 'mergeTableCells' }
    | { type: 'splitTableCell' }
    | { type: 'setTableColumnWidth'; width: number };
export type Request = {
    id: string;
    operation: 'initialize' | 'command' | 'undo' | 'redo' | 'applyUpdate'
        | 'drain' | 'snapshot' | 'stateVector' | 'stateDiff' | 'projectTable' | 'normalizeTable'
        | 'repairTableDuringRemoteWindow' | 'observePresentation'
        | 'setAwareness' | 'applyAwareness' | 'shutdown';
    payload: Record<string, unknown>;
};
export type UpdateEvent = {
    kind: 'document' | 'protocol' | 'awareness';
    origin: 'local' | 'history' | 'remote' | 'webRepair';
    bytesBase64: string;
};
export type PeerReply = {
    id: string;
    value: Record<string, unknown> | null;
    error: { code: string; message: string } | null;
    events: UpdateEvent[];
};
export interface WebPeerHandler {
    handle(requestJson: string): Promise<string>;
}
export interface Peer {
    request(request: Request): Promise<PeerReply>;
    close(): Promise<void>;
}
export function assertReply(reply: PeerReply, id: string): void {
    if (reply.id !== id || (reply.value === null) === (reply.error === null)) {
        throw new Error('Invalid peer reply envelope');
    }
}

const EVENT_KINDS = ['document', 'protocol', 'awareness'] as const;
const EVENT_ORIGINS = ['local', 'history', 'remote', 'webRepair'] as const;

export function isRecord(value: unknown): value is Record<string, unknown> {
    return typeof value === 'object' && value !== null && !Array.isArray(value);
}

function parseEvent(value: unknown): UpdateEvent {
    if (!isRecord(value)) {
        throw new Error('peer emitted a non-object update event');
    }
    const kind = value['kind'];
    const origin = value['origin'];
    const bytesBase64 = value['bytesBase64'];
    if (!EVENT_KINDS.includes(kind as UpdateEvent['kind'])) {
        throw new Error(`peer emitted an unknown event kind ${JSON.stringify(kind)}`);
    }
    if (!EVENT_ORIGINS.includes(origin as UpdateEvent['origin'])) {
        throw new Error(`peer emitted an unknown event origin ${JSON.stringify(origin)}`);
    }
    if (typeof bytesBase64 !== 'string') {
        throw new Error('peer emitted an event without base64 bytes');
    }
    return {
        kind: kind as UpdateEvent['kind'],
        origin: origin as UpdateEvent['origin'],
        bytesBase64,
    };
}

export function parseReplyValue(parsed: unknown): PeerReply {
    if (!isRecord(parsed)) {
        throw new Error('peer emitted a non-object reply');
    }
    const id = parsed['id'];
    if (typeof id !== 'string') {
        throw new Error('peer emitted a reply without a string id');
    }
    const value = parsed['value'];
    const error = parsed['error'];
    const events = parsed['events'];
    if (value !== null && !isRecord(value)) {
        throw new Error(`peer reply ${id} carried a non-object value`);
    }
    if (error !== null && !isRecord(error)) {
        throw new Error(`peer reply ${id} carried a non-object error`);
    }
    if (!Array.isArray(events)) {
        throw new Error(`peer reply ${id} carried no events array`);
    }
    let parsedError: PeerReply['error'] = null;
    if (isRecord(error)) {
        const code = error['code'];
        const message = error['message'];
        if (typeof code !== 'string' || typeof message !== 'string') {
            throw new Error(`peer reply ${id} carried a malformed error envelope`);
        }
        parsedError = { code, message };
    }
    return {
        id,
        value: isRecord(value) ? value : null,
        error: parsedError,
        events: events.map((event) => parseEvent(event)),
    };
}
