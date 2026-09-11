export type PeerKind = 'rust' | 'prosemirror' | 'tiptap';
export type Request = {
    id: string;
    operation: 'initialize' | 'command' | 'undo' | 'redo' | 'applyUpdate'
        | 'drain' | 'snapshot' | 'stateVector' | 'stateDiff'
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
