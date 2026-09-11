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
export interface Peer {
    request(request: Request): Promise<PeerReply>;
    close(): Promise<void>;
}
export function assertReply(reply: PeerReply, id: string): void {
    if (reply.id !== id || (reply.value === null) === (reply.error === null)) {
        throw new Error('Invalid peer reply envelope');
    }
}
