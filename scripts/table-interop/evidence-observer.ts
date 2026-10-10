import * as Y from 'yjs';
import { call, EMPTY_STATE_VECTOR_BASE64, peerKindOf, snapshot, PeerError } from './controller.js';
import type { RowInsertionObservation } from './browser/row-insertion-observer.js';
import { dependencyManifest } from './trace.js';
import { observeNativePresentation, observeWebPresentation } from './presentation-semantics.js';
import type { EffectiveCell, EffectiveDocument, JsonNode, Peer, Request } from './peer-protocol.js';
import type { RecordedAction, RecordedDelivery } from './scenario-evidence.js';
import { realCells } from './scenario-evidence.js';
import { tableSchemaOf } from './table-schema.js';

export async function observeEvidence(peer: Peer): Promise<EffectiveDocument> {
    if (peerKindOf(peer) !== 'rust') return observeWebPresentation(peer);
    const observed = await observeNativePresentation(peer);
    const binary = await call(peer, 'stateDiff', { stateVectorBase64: EMPTY_STATE_VECTOR_BASE64 });
    const document = new Y.Doc();
    try {
        Y.applyUpdate(document, Buffer.from(binary['updateBase64'] as string, 'base64'));
        const identities = new Map<string, string>();
        function visit(nodes: unknown[], parent: string): void {
            for (const [index, node] of nodes.entries()) {
                const source = parent ? `${parent}.${index}` : String(index);
                if (!(node instanceof Y.XmlElement)) continue;
                if (
                    ['table_cell', 'table_header', 'tableCell', 'tableHeader'].includes(
                        node.nodeName,
                    )
                ) {
                    const identity = Y.createRelativePositionFromTypeIndex(node, 0).type;
                    if (identity) identities.set(source, JSON.stringify({ type: identity }));
                }
                visit(node.toArray(), source);
            }
        }
        visit(document.getXmlFragment('prosemirror').toArray(), '');
        for (const cell of realCells(observed)) {
            const identity = identities.get(cell.source!);
            if (!identity) throw new Error(`TBL21 EVIDENCE SOURCE_ID ${cell.source}`);
            cell.sourceId = identity;
        }
        return observed;
    } finally {
        document.destroy();
    }
}

export interface EvidenceCapture {
    actions: RecordedAction[];
    deliveries: RecordedDelivery[];
}
const captures = new WeakMap<Peer, { capture: EvidenceCapture; actor: number }>();
export function startEvidence(peers: readonly Peer[]): EvidenceCapture {
    const capture: EvidenceCapture = { actions: [], deliveries: [] };
    peers.forEach((peer, actor) => captures.set(peer, { capture, actor }));
    return capture;
}
export function stopEvidence(peers: readonly Peer[]): void {
    peers.forEach((peer) => captures.delete(peer));
}
function nodeSize(node: JsonNode, native: boolean): number {
    return node.type === 'text'
        ? native
            ? [...(node.text ?? '')].length
            : (node.text ?? '').length
        : node.content
          ? 2 + node.content.reduce((total, child) => total + nodeSize(child, native), 0)
          : node.type === 'paragraph'
            ? 2
            : 1;
}
function targetAt(document: EffectiveDocument, at: unknown, native: boolean): EffectiveCell | null {
    if (typeof at !== 'number') return null;
    return (
        realCells(document)
            .filter(
                (cell) => at > cell.position && at < cell.position + nodeSize(cell.node, native),
            )
            .sort((a, b) => b.position - a.position)[0] ?? null
    );
}
function commandName(operation: Request['operation'], payload: Record<string, unknown>): string {
    if (operation !== 'command') return operation;
    const type = payload['type'] === 'tableCommand' ? payload['name'] : payload['type'];
    const names: Record<string, string> = {
        addTableRow: 'addRow',
        addRowAfter: 'addRow',
        addTableColumn: 'addColumn',
        addColumnAfter: 'addColumn',
        mergeTableCells: 'merge',
        mergeCells: 'merge',
        splitTableCell: 'split',
        splitCell: 'split',
        deleteTableRows: 'deleteRow',
        deleteRow: 'deleteRow',
        setTableColumnWidth: 'resize',
    };
    return names[String(type)] ?? String(type);
}
export async function evidenceCall(
    peer: Peer,
    operation: Request['operation'],
    payload: Record<string, unknown>,
): Promise<Record<string, unknown>> {
    const active = captures.get(peer);
    if (!active || !['command', 'undo', 'redo', 'applyUpdate'].includes(operation))
        return call(peer, operation, payload);
    const beforeState = await snapshot(peer);
    if (operation === 'applyUpdate') {
        const reply = await call(peer, operation, payload);
        const after = await snapshot(peer);
        active.capture.deliveries.push({
            actor: active.actor,
            bytes: String(payload['updateBase64']),
            pendingBefore: beforeState.pendingDependencies,
            pendingAfter: after.pendingDependencies,
        });
        return reply;
    }
    let failure: string | undefined;
    const observe = async () => {
        try {
            return await observeEvidence(peer);
        } catch (error) {
            failure = String(error);
            return { tables: [] };
        }
    };
    const before = await observe();
    const structural = operation === 'command' && ['addTableRow', 'tableCommand'].includes(String(payload['type']));
    const encoded = async () => String((await call(peer, 'stateDiff', { stateVectorBase64: EMPTY_STATE_VECTOR_BASE64 }))['updateBase64']);
    const encodedBefore = structural ? await encoded() : '';
    let reply: Record<string, unknown> = {};
    let rejected: unknown;
    try {
        reply = await call(peer, operation, payload);
    } catch (error) {
        rejected = error;
    }
    const afterState = await snapshot(peer);
    const encodedAfter = structural ? await encoded() : '';
    const after = await observe();
    let targetGridValid: boolean | undefined;
    const target = targetAt(before, payload['at'], peerKindOf(peer) === 'rust');
    if (
        peerKindOf(peer) === 'rust' &&
        operation === 'command' &&
        payload['type'] !== 'insertText'
    ) {
        const originalTable = before.tables.find((table) =>
            table.cells.some((cell) => cell.source === target?.source),
        );
        const table = after.tables.find((table) => table.source === originalTable?.source);
        if (table) {
            const camel = table.node.content?.[0]?.type === 'tableRow';
            const projection = await call(peer, 'projectTable', {
                table: table.node,
                schema: tableSchemaOf(
                    camel ? 'tableRow' : 'table_row',
                    camel ? 'tableCell' : 'table_cell',
                    camel ? 'tableHeader' : 'table_header',
                ),
            });
            targetGridValid = projection['irregular'] === false;
        }
    }
    active.capture.actions.push({
        request: { operation, payload: structuredClone(payload) },
        ...(structural ? { availabilityBoundary: { before: beforeState, after: afterState, encodedBefore, encodedAfter, versions: dependencyManifest('').packages } } : {}),
        ...(rejected !== undefined ? { commandError: { code: rejected instanceof PeerError ? rejected.code : 'UNSUPPORTED_ERROR', message: rejected instanceof PeerError ? rejected.detail : String(rejected) } } : {}),
        ...(afterState.rowInsertion ? { stockRowInsertion: afterState.rowInsertion as RowInsertionObservation } : {}),
        actor: active.actor,
        kind: peerKindOf(peer),
        operation: commandName(operation, payload),
        target,
        head: targetAt(before, payload['head'], peerKindOf(peer) === 'rust'),
        before,
        after,
        rawBefore: beforeState.documentJson,
        rawAfter: afterState.documentJson,
        reply,
        passes: afterState.normalizationPassesAfterLastAction,
        autonomous: afterState.autonomousRepairWrites,
        ...(typeof payload['text'] === 'string' ? { text: payload['text'] } : {}),
        ...(typeof payload['width'] === 'number' ? { width: payload['width'] } : {}),
        ...(failure ? { observationFailure: failure } : {}),
        ...(targetGridValid !== undefined ? { targetGridValid } : {}),
    });
    if (rejected !== undefined) throw rejected;
    return reply;
}
