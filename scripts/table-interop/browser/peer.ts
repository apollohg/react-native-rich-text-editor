import * as Y from 'yjs';
import { Awareness, applyAwarenessUpdate, encodeAwarenessUpdate } from 'y-protocols/awareness';
import { EditorState, Plugin, PluginKey, TextSelection } from 'prosemirror-state';
import type { Transaction } from 'prosemirror-state';
import { EditorView } from 'prosemirror-view';
import { schema as prosemirrorBasicSchema } from 'prosemirror-schema-basic';
import { Schema as ProsemirrorSchema } from 'prosemirror-model';
import {
    CellSelection,
    TableMap,
    addColumnAfter,
    addColumnBefore,
    addRowAfter,
    addRowBefore,
    deleteColumn,
    deleteRow,
    deleteTable,
    fixTables,
    mergeCells,
    splitCell,
    tableEditing,
    tableNodes,
    toggleHeaderCell,
    toggleHeaderColumn,
    toggleHeaderRow,
} from 'prosemirror-tables';
import type { Command } from 'prosemirror-state';
import type { Node as ProsemirrorNode } from 'prosemirror-model';
import {
    absolutePositionToRelativePosition,
    relativePositionToAbsolutePosition,
    ySyncPlugin,
    ySyncPluginKey,
    yUndoPlugin,
    undo as prosemirrorUndo,
    redo as prosemirrorRedo,
    yXmlFragmentToProsemirrorJSON,
} from 'y-prosemirror';
import { Editor, Extension } from '@tiptap/core';
import Document from '@tiptap/extension-document';
import Paragraph from '@tiptap/extension-paragraph';
import Text from '@tiptap/extension-text';
import Collaboration from '@tiptap/extension-collaboration';
import { Table, TableCell, TableHeader, TableRow } from '@tiptap/extension-table';
import {
    ySyncPluginKey as tiptapSyncPluginKey,
    yXmlFragmentToProsemirrorJSON as tiptapXmlFragmentToProsemirrorJSON,
} from '@tiptap/y-tiptap';
import type { PeerReply, Request, UpdateEvent, WebPeerHandler } from '../peer-protocol.js';

const REMOTE_ORIGIN = 'tableInteropRemoteUpdate';
const EDITOR_ELEMENT_ID = 'editor';
const KIND_QUERY_PARAMETER = 'kind';
const SETTLE_TICKS = 2;
const BASE64_CHUNK_BYTES = 0x8000;
const CONFIG_INVALID = 'CONFIG_INVALID';
const PEER_NOT_INITIALIZED = 'PEER_NOT_INITIALIZED';
const UNSUPPORTED_OPERATION = 'UNSUPPORTED_OPERATION';
const LIMIT_EXCEEDED = 'LIMIT_EXCEEDED';
const INTERNAL_ERROR = 'INTERNAL_ERROR';
const TABLE_GROUP = 'block';
const TABLE_CELL_CONTENT = 'block+';
const COLWIDTH_MISMATCH = 'colwidth mismatch';
const SYNTHETIC_SLOT_POSITION = 0;
const TABLE_CONTENT_OFFSET = 1;
const CELL_COLLISION = 'collision';
const UNSET_COLUMN_WIDTH = 0;
const INSERT_TEXT_COMMAND = 'insertText';
const INSERT_NODE_COMMAND = 'insertNode';
const TABLE_COMMAND = 'tableCommand';
const AWARENESS_CURSOR_KEY = 'cursor';
const AWARENESS_CELL_RECTANGLE_KEY = 'nativeEditorTableSelection';
const AWARENESS_CELL_RECTANGLE_VERSION = 1;
const AWARENESS_CELL_RECTANGLE_FIELDS = 3;
const AWARENESS_MUTATED_DOCUMENT = 'AWARENESS_MUTATED_DOCUMENT';
const TEXT_SELECTION = 'text';
const CELL_SELECTION = 'cell';
const AWARENESS_ORIGIN = 'tableInteropAwareness';
const TABLE_COMMANDS: Record<string, Command> = {
    addColumnAfter,
    addColumnBefore,
    addRowAfter,
    addRowBefore,
    deleteColumn,
    deleteRow,
    deleteTable,
    mergeCells,
    splitCell,
    toggleHeaderCell,
    toggleHeaderColumn,
    toggleHeaderRow,
};
const TABLE_CELL_ROLES = ['cell', 'header_cell'];
const CELL_DEPTH_FLOOR = 1;

const tableSchema = new ProsemirrorSchema({
    nodes: prosemirrorBasicSchema.spec.nodes.append(
        tableNodes({
            tableGroup: TABLE_GROUP,
            cellContent: TABLE_CELL_CONTENT,
            cellAttributes: {},
        }),
    ),
    marks: prosemirrorBasicSchema.spec.marks,
});

type WebPeerKind = 'prosemirror' | 'tiptap';

type NormalizationCounter = { passes: number };

class PeerOperationError extends Error {
    readonly code: string;

    constructor(code: string, message: string) {
        super(message);
        this.name = 'PeerOperationError';
        this.code = code;
    }
}

function isRecord(value: unknown): value is Record<string, unknown> {
    return typeof value === 'object' && value !== null && !Array.isArray(value);
}

function requireRecord(value: unknown, field: string): Record<string, unknown> {
    if (!isRecord(value)) {
        throw new PeerOperationError(CONFIG_INVALID, `${field} must be an object`);
    }
    return value;
}

function requireString(value: unknown, field: string): string {
    if (typeof value !== 'string') {
        throw new PeerOperationError(CONFIG_INVALID, `${field} must be a string`);
    }
    return value;
}

function requireBoolean(value: unknown, field: string): boolean {
    if (typeof value !== 'boolean') {
        throw new PeerOperationError(CONFIG_INVALID, `${field} must be a boolean`);
    }
    return value;
}

function requirePositiveInteger(value: unknown, field: string): number {
    if (typeof value !== 'number' || !Number.isInteger(value) || value <= 0) {
        throw new PeerOperationError(CONFIG_INVALID, `${field} must be a positive integer`);
    }
    return value;
}

function toBase64(bytes: Uint8Array): string {
    let binary = '';
    for (let offset = 0; offset < bytes.length; offset += BASE64_CHUNK_BYTES) {
        binary += String.fromCharCode(...bytes.subarray(offset, offset + BASE64_CHUNK_BYTES));
    }
    return btoa(binary);
}

function fromBase64(encoded: string, field: string): Uint8Array {
    let binary: string;
    try {
        binary = atob(encoded);
    } catch {
        throw new PeerOperationError(CONFIG_INVALID, `${field} is not valid base64`);
    }
    const bytes = new Uint8Array(binary.length);
    for (let index = 0; index < binary.length; index += 1) {
        bytes[index] = binary.charCodeAt(index);
    }
    return bytes;
}

const normalizationCounterKey = new PluginKey<null>('tableInteropNormalizationCounter');

function normalizationCounterPlugin(counter: NormalizationCounter): Plugin<null> {
    return new Plugin<null>({
        key: normalizationCounterKey,
        state: {
            init: () => null,
            apply: (transaction) => {
                if (transaction.getMeta('appendedTransaction') !== undefined) {
                    counter.passes += 1;
                }
                return null;
            },
        },
    });
}

interface MountedEditor {
    readonly view: EditorView;
    documentJson(): Record<string, unknown>;
    history(direction: 'undo' | 'redo'): boolean;
    destroy(): void;
}

function mountProsemirror(
    fragment: Y.XmlFragment,
    element: HTMLElement,
    counter: NormalizationCounter,
    tables: boolean,
): MountedEditor {
    const state = EditorState.create({
        schema: tables ? tableSchema : prosemirrorBasicSchema,
        plugins: tables
            ? [
                ySyncPlugin(fragment),
                yUndoPlugin(),
                tableEditing(),
                normalizationCounterPlugin(counter),
            ]
            : [ySyncPlugin(fragment), yUndoPlugin(), normalizationCounterPlugin(counter)],
    });
    const view = new EditorView(element, {
        state,
        dispatchTransaction(this: EditorView, transaction: Transaction) {
            const applied = this.state.applyTransaction(transaction);
            this.updateState(applied.state);
        },
    });
    return {
        view,
        documentJson: () => yXmlFragmentToProsemirrorJSON(fragment),
        history: (direction) =>
            direction === 'undo' ? prosemirrorUndo(view.state) : prosemirrorRedo(view.state),
        destroy: () => view.destroy(),
    };
}

function mountTiptap(
    sharedDocument: Y.Doc,
    fragmentName: string,
    element: HTMLElement,
    counter: NormalizationCounter,
    tables: boolean,
): MountedEditor {
    const fragment = sharedDocument.getXmlFragment(fragmentName);
    const instrumentation = Extension.create({
        name: 'tableInteropInstrumentation',
        addProseMirrorPlugins: () => [normalizationCounterPlugin(counter)],
    });
    const editor = new Editor({
        element,
        extensions: [
            Document,
            Paragraph,
            Text,
            ...(tables ? [Table, TableRow, TableHeader, TableCell] : []),
            Collaboration.configure({ document: sharedDocument, field: fragmentName }),
            instrumentation,
        ],
    });
    return {
        view: editor.view,
        documentJson: () => tiptapXmlFragmentToProsemirrorJSON(fragment),
        history: (direction) =>
            direction === 'undo' ? editor.commands.undo() : editor.commands.redo(),
        destroy: () => editor.destroy(),
    };
}

function bindingOriginFor(kind: WebPeerKind): unknown {
    return kind === 'prosemirror' ? ySyncPluginKey : tiptapSyncPluginKey;
}

function cellPositionAt(doc: ProsemirrorNode, position: number): number {
    const resolved = doc.resolve(position);
    for (let depth = resolved.depth; depth >= CELL_DEPTH_FLOOR; depth -= 1) {
        const role = resolved.node(depth).type.spec['tableRole'];
        if (typeof role === 'string' && TABLE_CELL_ROLES.includes(role)) {
            return resolved.before(depth);
        }
    }
    throw new PeerOperationError(
        CONFIG_INVALID,
        `document position ${position} is not inside a table cell`,
    );
}

function anchorSelection(editor: MountedEditor, command: Record<string, unknown>): void {
    const at = command['at'];
    if (at === undefined) {
        return;
    }
    const anchor = requirePositiveInteger(at, 'command.at');
    const head = command['head'];
    const { state } = editor.view;
    const selection = head === undefined
        ? TextSelection.near(state.doc.resolve(anchor))
        : new CellSelection(
            state.doc.resolve(cellPositionAt(state.doc, anchor)),
            state.doc.resolve(
                cellPositionAt(state.doc, requirePositiveInteger(head, 'command.head')),
            ),
        );
    editor.view.dispatch(state.tr.setSelection(selection));
}

function applyCommand(editor: MountedEditor, command: Record<string, unknown>): void {
    anchorSelection(editor, command);
    const type = command['type'];
    if (type === INSERT_TEXT_COMMAND) {
        const text = requireString(command['text'], 'command.text');
        editor.view.dispatch(editor.view.state.tr.insertText(text));
        return;
    }
    if (type === INSERT_NODE_COMMAND) {
        const json = requireRecord(command['node'], 'command.node');
        const node = editor.view.state.schema.nodeFromJSON(json);
        editor.view.dispatch(editor.view.state.tr.replaceSelectionWith(node));
        return;
    }
    if (type === TABLE_COMMAND) {
        const name = requireString(command['name'], 'command.name');
        const at = requirePositiveInteger(command['at'], 'command.at');
        const tableCommand = TABLE_COMMANDS[name];
        if (tableCommand === undefined) {
            throw new PeerOperationError(
                CONFIG_INVALID,
                `table command ${JSON.stringify(name)} is not served by the web peer`,
            );
        }
        if (!tableCommand(editor.view.state, editor.view.dispatch, editor.view)) {
            throw new PeerOperationError(
                CONFIG_INVALID,
                `table command ${name} did not apply at ${at}`,
            );
        }
        return;
    }
    throw new PeerOperationError(
        CONFIG_INVALID,
        `command ${JSON.stringify(type)} is not served by the web peer`,
    );
}

type ProsemirrorMapping = Map<Y.AbstractType<unknown>, ProsemirrorNode | ProsemirrorNode[]>;
type RelativeSelectionPoints = { anchor: Y.RelativePosition; head: Y.RelativePosition };

function requireU32(value: unknown, field: string): number {
    if (typeof value !== 'number' || !Number.isInteger(value) || value < 0 || value > 0xffff_ffff) {
        throw new PeerOperationError(CONFIG_INVALID, `${field} must be a u32 document position`);
    }
    return value;
}

function encodeCellRectangle(points: RelativeSelectionPoints): Record<string, unknown> {
    return {
        version: AWARENESS_CELL_RECTANGLE_VERSION,
        anchor: toBase64(Y.encodeRelativePosition(points.anchor)),
        head: toBase64(Y.encodeRelativePosition(points.head)),
    };
}

function decodeCellRectangle(state: unknown): RelativeSelectionPoints | null {
    if (!isRecord(state)) {
        return null;
    }
    const entry = state[AWARENESS_CELL_RECTANGLE_KEY];
    if (!isRecord(entry) || Object.keys(entry).length !== AWARENESS_CELL_RECTANGLE_FIELDS) {
        return null;
    }
    if (entry['version'] !== AWARENESS_CELL_RECTANGLE_VERSION) {
        return null;
    }
    const anchor = entry['anchor'];
    const head = entry['head'];
    if (typeof anchor !== 'string' || typeof head !== 'string') {
        return null;
    }
    try {
        return {
            anchor: Y.decodeRelativePosition(fromBase64(anchor, 'awareness.anchor')),
            head: Y.decodeRelativePosition(fromBase64(head, 'awareness.head')),
        };
    } catch {
        return null;
    }
}

class WebPeerRuntime {
    private readonly kind: WebPeerKind;
    private readonly element: HTMLElement;
    private readonly fragmentName: string;
    private readonly maxUpdateBytes: number;
    private readonly tables: boolean;
    private readonly bindingOrigin: unknown;
    private readonly document = new Y.Doc();
    private readonly awareness: Awareness;
    private readonly counter: NormalizationCounter = { passes: 0 };
    private readonly pendingEvents: UpdateEvent[] = [];
    private editor: MountedEditor | null = null;
    private insideRequestedOperation = false;
    private autonomousRepairWrites = 0;
    private documentRevision = 0;

    constructor(
        kind: WebPeerKind,
        element: HTMLElement,
        fragmentName: string,
        maxUpdateBytes: number,
        tables: boolean,
    ) {
        this.kind = kind;
        this.element = element;
        this.fragmentName = fragmentName;
        this.maxUpdateBytes = maxUpdateBytes;
        this.tables = tables;
        this.bindingOrigin = bindingOriginFor(kind);
        this.awareness = new Awareness(this.document);
        this.awareness.setLocalState(null);
        this.document.on('update', (update: Uint8Array, origin: unknown) => {
            this.documentRevision += 1;
            const classified = this.classifyOrigin(origin);
            if (classified === 'remote') {
                return;
            }
            if (classified === 'webRepair') {
                this.autonomousRepairWrites += 1;
            }
            this.pendingEvents.push({
                kind: 'document',
                origin: classified,
                bytesBase64: toBase64(update),
            });
        });
    }

    private syncState(): { type: Y.XmlFragment; mapping: ProsemirrorMapping } {
        const editor = this.requireEditor();
        const key = this.kind === 'prosemirror' ? ySyncPluginKey : tiptapSyncPluginKey;
        const state: unknown = key.getState(editor.view.state);
        if (!isRecord(state) || state['binding'] === undefined) {
            throw new PeerOperationError(
                PEER_NOT_INITIALIZED,
                'the web peer has no y-sync binding yet',
            );
        }
        return {
            type: this.document.getXmlFragment(this.fragmentName),
            mapping: (state['binding'] as { mapping: ProsemirrorMapping }).mapping,
        };
    }

    private relativeAt(documentPosition: number): Y.RelativePosition {
        const { type, mapping } = this.syncState();
        return absolutePositionToRelativePosition(documentPosition, type, mapping);
    }

    private absoluteAt(relative: Y.RelativePosition): number | null {
        const { type, mapping } = this.syncState();
        return relativePositionToAbsolutePosition(this.document, type, relative, mapping);
    }

    private selectionPoints(selection: Record<string, unknown>): RelativeSelectionPoints {
        const type = selection['type'];
        if (type === TEXT_SELECTION) {
            return {
                anchor: this.relativeAt(requireU32(selection['anchor'], 'selection.anchor')),
                head: this.relativeAt(requireU32(selection['head'], 'selection.head')),
            };
        }
        if (type === CELL_SELECTION) {
            return {
                anchor: this.relativeAt(requireU32(selection['anchorCell'], 'selection.anchorCell')),
                head: this.relativeAt(requireU32(selection['headCell'], 'selection.headCell')),
            };
        }
        throw new PeerOperationError(
            CONFIG_INVALID,
            `selection type ${JSON.stringify(type)} is not served by the web peer`,
        );
    }

    setAwareness(payload: Record<string, unknown>): Record<string, unknown> {
        const before = this.documentRevision;
        const intent = payload['intent'];
        if (intent === null) {
            this.awareness.setLocalState(null);
            return this.awarenessResult(before);
        }
        const record = requireRecord(intent, 'payload.intent');
        const state = requireRecord(record['state'], 'payload.intent.state');
        const published: Record<string, unknown> = { ...state };
        const selection = record['selection'];
        if (selection !== null && selection !== undefined) {
            const requested = requireRecord(selection, 'payload.intent.selection');
            const points = this.selectionPoints(requested);
            published[AWARENESS_CURSOR_KEY] = {
                anchor: Y.relativePositionToJSON(points.anchor),
                head: Y.relativePositionToJSON(points.head),
            };
            if (requested['type'] === CELL_SELECTION) {
                published[AWARENESS_CELL_RECTANGLE_KEY] = encodeCellRectangle(points);
            }
        }
        this.awareness.setLocalState(published);
        return this.awarenessResult(before);
    }

    applyAwareness(payload: Record<string, unknown>): Record<string, unknown> {
        const before = this.documentRevision;
        const update = fromBase64(
            requireString(payload['updateBase64'], 'payload.updateBase64'),
            'updateBase64',
        );
        if (update.length > this.maxUpdateBytes) {
            throw new PeerOperationError(
                LIMIT_EXCEEDED,
                `an awareness update of ${update.length} bytes exceeds the ${this.maxUpdateBytes} byte ceiling`,
            );
        }
        applyAwarenessUpdate(this.awareness, update, AWARENESS_ORIGIN);
        return this.awarenessResult(before);
    }

    private awarenessResult(documentRevisionBefore: number): Record<string, unknown> {
        if (this.documentRevision !== documentRevisionBefore) {
            throw new PeerOperationError(
                AWARENESS_MUTATED_DOCUMENT,
                'awareness must never change the document',
            );
        }
        this.pendingEvents.push({
            kind: 'awareness',
            origin: 'local',
            bytesBase64: toBase64(
                encodeAwarenessUpdate(this.awareness, [this.awareness.clientID]),
            ),
        });
        return { documentRevision: this.documentRevision.toString() };
    }

    awarenessPeers(): Record<string, unknown>[] {
        const peers: Record<string, unknown>[] = [];
        for (const [clientId, state] of this.awareness.getStates()) {
            const record = isRecord(state) ? state : {};
            peers.push({
                clientId: clientId.toString(),
                isLocal: clientId === this.awareness.clientID,
                state: record,
                cursor: this.projectedCursor(record),
                cellRectangle: this.projectedCellRectangle(record),
            });
        }
        peers.sort((left, right) => Number(left['clientId']) - Number(right['clientId']));
        return peers;
    }

    private projectedCursor(state: Record<string, unknown>): Record<string, unknown> | null {
        const cursor = state[AWARENESS_CURSOR_KEY];
        if (!isRecord(cursor)) {
            return null;
        }
        const anchor = this.absoluteAt(Y.createRelativePositionFromJSON(cursor['anchor']));
        const head = this.absoluteAt(Y.createRelativePositionFromJSON(cursor['head']));
        if (anchor === null || head === null) {
            return null;
        }
        return { anchor, head };
    }

    private projectedCellRectangle(
        state: Record<string, unknown>,
    ): Record<string, unknown> | null {
        const points = decodeCellRectangle(state);
        if (points === null) {
            return null;
        }
        const anchorCell = this.absoluteAt(points.anchor);
        const headCell = this.absoluteAt(points.head);
        if (anchorCell === null || headCell === null) {
            return null;
        }
        return { anchorCell, headCell };
    }

    private classifyOrigin(origin: unknown): UpdateEvent['origin'] {
        if (origin === REMOTE_ORIGIN) {
            return 'remote';
        }
        if (origin instanceof Y.UndoManager) {
            return 'history';
        }
        if (origin === this.bindingOrigin) {
            return this.insideRequestedOperation ? 'local' : 'webRepair';
        }
        throw new PeerOperationError(
            INTERNAL_ERROR,
            `a Y.Doc update carried the unclassifiable origin ${String(origin)}`,
        );
    }

    private createEditor(): void {
        this.editor = this.kind === 'prosemirror'
            ? mountProsemirror(
                this.document.getXmlFragment(this.fragmentName),
                this.element,
                this.counter,
                this.tables,
            )
            : mountTiptap(
                this.document,
                this.fragmentName,
                this.element,
                this.counter,
                this.tables,
            );
    }

    mountForLocalInitialization(): void {
        this.insideRequestedOperation = true;
        try {
            this.createEditor();
        } finally {
            this.insideRequestedOperation = false;
        }
    }

    private mountOverSeed(): void {
        this.createEditor();
    }

    hasPendingDependencies(): boolean {
        const store = this.document.store;
        return store.pendingStructs !== null || store.pendingDs !== null;
    }

    private seedHasArrived(): boolean {
        return !this.hasPendingDependencies()
            && this.document.getXmlFragment(this.fragmentName).length > 0;
    }

    private requireEditor(): MountedEditor {
        if (this.editor === null) {
            throw new PeerOperationError(
                PEER_NOT_INITIALIZED,
                'the web peer is still awaiting its seed update',
            );
        }
        return this.editor;
    }

    async settle(): Promise<void> {
        for (let tick = 0; tick < SETTLE_TICKS; tick += 1) {
            await Promise.resolve();
            await new Promise<void>((resolve) => {
                setTimeout(resolve, 0);
            });
        }
    }

    private async runRequestedOperation(action: () => void): Promise<boolean> {
        this.counter.passes = 0;
        const before = this.documentRevision;
        this.insideRequestedOperation = true;
        try {
            action();
        } finally {
            this.insideRequestedOperation = false;
        }
        await this.settle();
        return this.documentRevision !== before;
    }

    revision(): string {
        return this.documentRevision.toString();
    }

    async command(payload: Record<string, unknown>): Promise<Record<string, unknown>> {
        const mutationKind = requireString(payload['kind'], 'payload.kind');
        if (mutationKind !== 'command') {
            throw new PeerOperationError(
                CONFIG_INVALID,
                `mutation kind ${JSON.stringify(mutationKind)} is not served by the web peer`,
            );
        }
        const anchor = payload['at'];
        const head = payload['head'];
        const command = {
            ...requireRecord(payload['command'], 'payload.command'),
            ...(anchor === undefined ? {} : { at: anchor }),
            ...(head === undefined ? {} : { head }),
        };
        const editor = this.requireEditor();
        const changed = await this.runRequestedOperation(() => {
            applyCommand(editor, command);
        });
        return { type: 'transaction', documentChanged: changed };
    }

    async history(direction: 'undo' | 'redo'): Promise<Record<string, unknown>> {
        const editor = this.requireEditor();
        let applied = false;
        await this.runRequestedOperation(() => {
            applied = editor.history(direction);
        });
        return { applied, documentRevision: this.documentRevision.toString() };
    }

    async applyUpdate(payload: Record<string, unknown>): Promise<Record<string, unknown>> {
        const updateBase64 = requireString(payload['updateBase64'], 'payload.updateBase64');
        const update = fromBase64(updateBase64, 'updateBase64');
        if (update.length > this.maxUpdateBytes) {
            throw new PeerOperationError(
                LIMIT_EXCEEDED,
                `a remote update of ${update.length} bytes exceeds the ${this.maxUpdateBytes} byte ceiling`,
            );
        }
        this.counter.passes = 0;
        const before = this.documentRevision;
        Y.applyUpdate(this.document, update, REMOTE_ORIGIN);
        await this.settle();
        if (this.editor === null && this.seedHasArrived()) {
            this.mountOverSeed();
            await this.settle();
        }
        return { changed: this.documentRevision !== before };
    }

    stateVector(): Record<string, unknown> {
        return { stateVectorBase64: toBase64(Y.encodeStateVector(this.document)) };
    }

    stateDiff(payload: Record<string, unknown>): Record<string, unknown> {
        const encoded = requireString(payload['stateVectorBase64'], 'payload.stateVectorBase64');
        const stateVector = fromBase64(encoded, 'stateVectorBase64');
        return {
            updateBase64: toBase64(Y.encodeStateAsUpdate(this.document, stateVector)),
        };
    }

    snapshot(): Record<string, unknown> {
        const fragment = this.document.getXmlFragment(this.fragmentName);
        const documentJson = this.editor === null
            ? yXmlFragmentToProsemirrorJSON(fragment)
            : this.editor.documentJson();
        return {
            json: documentJson,
            awarenessPeers: this.editor === null ? [] : this.awarenessPeers(),
            mounted: this.editor !== null,
            pendingDependencies: this.hasPendingDependencies(),
            displayJson: this.editor === null ? null : this.editor.view.state.doc.toJSON(),
            documentRevision: this.documentRevision.toString(),
            normalizationPassesAfterLastAction: this.counter.passes,
            autonomousRepairWrites: this.autonomousRepairWrites,
        };
    }

    flushEvents(): UpdateEvent[] {
        return this.pendingEvents.splice(0, this.pendingEvents.length);
    }

    pendingEventCount(): number {
        return this.pendingEvents.length;
    }

    teardown(): void {
        this.awareness.destroy();
        if (this.editor !== null) {
            this.editor.destroy();
            this.editor = null;
        }
        this.document.destroy();
    }
}

function resolvedColumnWidths(table: ProsemirrorNode, map: TableMap): (number | null)[] {
    const repaired = new Map<number, number[]>();
    for (const problem of map.problems ?? []) {
        if (problem.type === COLWIDTH_MISMATCH) {
            repaired.set(problem.pos, problem.colwidth);
        }
    }
    const widths: (number | null)[] = new Array<number | null>(map.width).fill(null);
    const resolved = new Array<boolean>(map.width).fill(false);
    for (const [index, pos] of map.map.entries()) {
        const column = index % map.width;
        if (resolved[column] || pos === SYNTHETIC_SLOT_POSITION) {
            continue;
        }
        const cell = table.nodeAt(pos);
        if (cell === null) {
            throw new PeerOperationError(INTERNAL_ERROR, `the table map addressed no cell at ${pos}`);
        }
        resolved[column] = true;
        const attributeWidths: unknown = repaired.get(pos) ?? cell.attrs['colwidth'];
        const width = Array.isArray(attributeWidths)
            ? attributeWidths[column - map.colCount(pos)]
            : UNSET_COLUMN_WIDTH;
        widths[column] = typeof width === 'number' && width !== UNSET_COLUMN_WIDTH ? width : null;
    }
    return widths;
}

function projectTable(payload: Record<string, unknown>): Record<string, unknown> {
    const table = tableSchema.nodeFromJSON(requireRecord(payload['table'], 'payload.table'));
    const map = TableMap.get(table);
    const problems = map.problems ?? [];
    const structural = problems.filter((problem) => problem.type !== COLWIDTH_MISMATCH);
    return {
        rows: map.height,
        columns: map.width,
        widths: resolvedColumnWidths(table, map),
        irregular: structural.length > 0,
        slots: map.map.map((pos) =>
            pos === SYNTHETIC_SLOT_POSITION ? null : pos + TABLE_CONTENT_OFFSET,
        ),
        collisions: problems.filter((problem) => problem.type === CELL_COLLISION).length,
    };
}

const DOC_NODE = 'doc';
const FIRST_TABLE_INDEX = 0;

function fixTable(payload: Record<string, unknown>): Record<string, unknown> {
    const table = requireRecord(payload['table'], 'payload.table');
    const doc = tableSchema.nodeFromJSON({ type: DOC_NODE, content: [table] });
    const state = EditorState.create({ schema: tableSchema, doc });
    const transaction = fixTables(state);
    if (transaction === undefined) {
        return { table, changed: false };
    }
    const fixed = state.apply(transaction).doc.maybeChild(FIRST_TABLE_INDEX);
    return { table: fixed === null || fixed === undefined ? null : fixed.toJSON(), changed: true };
}

function readKind(): WebPeerKind {
    const requested = new URLSearchParams(window.location.search).get(KIND_QUERY_PARAMETER);
    if (requested !== 'prosemirror' && requested !== 'tiptap') {
        throw new Error(`the peer page was opened with an unknown kind ${JSON.stringify(requested)}`);
    }
    return requested;
}

function readElement(): HTMLElement {
    const element = window.document.getElementById(EDITOR_ELEMENT_ID);
    if (element === null) {
        throw new Error(`the peer page is missing the #${EDITOR_ELEMENT_ID} host element`);
    }
    return element;
}

let runtime: WebPeerRuntime | null = null;

function requireRuntime(): WebPeerRuntime {
    if (runtime === null) {
        throw new PeerOperationError(
            PEER_NOT_INITIALIZED,
            'the peer has no session; send initialize first',
        );
    }
    return runtime;
}

function initialize(payload: Record<string, unknown>): Record<string, unknown> {
    if (runtime !== null) {
        throw new PeerOperationError(CONFIG_INVALID, 'the peer session is already initialized');
    }
    const tables = requireBoolean(payload['tables'], 'payload.tables');
    const fragmentName = requireString(payload['fragmentName'], 'payload.fragmentName');
    const limits = requireRecord(payload['limits'], 'payload.limits');
    const maxUpdateBytes = requirePositiveInteger(
        limits['maxUpdateBytes'],
        'payload.limits.maxUpdateBytes',
    );
    const awaitSeed = requireBoolean(payload['awaitSeed'], 'payload.awaitSeed');
    const created = new WebPeerRuntime(
        readKind(),
        readElement(),
        fragmentName,
        maxUpdateBytes,
        tables,
    );
    runtime = created;
    if (!awaitSeed) {
        created.mountForLocalInitialization();
    }
    return { documentRevision: created.revision() };
}

async function dispatch(
    operation: Request['operation'],
    payload: Record<string, unknown>,
): Promise<Record<string, unknown>> {
    switch (operation) {
        case 'initialize':
            return initialize(payload);
        case 'command':
            return requireRuntime().command(payload);
        case 'undo':
            return requireRuntime().history('undo');
        case 'redo':
            return requireRuntime().history('redo');
        case 'applyUpdate':
            return requireRuntime().applyUpdate(payload);
        case 'drain': {
            const peer = requireRuntime();
            await peer.settle();
            return {
                count: peer.pendingEventCount(),
                pendingDependencies: peer.hasPendingDependencies(),
            };
        }
        case 'snapshot':
            return requireRuntime().snapshot();
        case 'stateVector':
            return requireRuntime().stateVector();
        case 'stateDiff':
            return requireRuntime().stateDiff(payload);
        case 'projectTable':
            return projectTable(payload);
        case 'normalizeTable':
            return fixTable(payload);
        case 'setAwareness':
            return requireRuntime().setAwareness(payload);
        case 'applyAwareness':
            return requireRuntime().applyAwareness(payload);
        case 'shutdown': {
            requireRuntime().teardown();
            return {};
        }
        default:
            throw new PeerOperationError(
                UNSUPPORTED_OPERATION,
                `operation ${operation} is not served by the web peer`,
            );
    }
}

function errorEnvelope(error: unknown): { code: string; message: string } {
    if (error instanceof PeerOperationError) {
        return { code: error.code, message: error.message };
    }
    if (error instanceof Error) {
        return { code: INTERNAL_ERROR, message: `${error.name}: ${error.message}` };
    }
    return { code: INTERNAL_ERROR, message: String(error) };
}

async function handle(requestJson: string): Promise<string> {
    let id = '';
    let value: Record<string, unknown> | null = null;
    let error: { code: string; message: string } | null = null;
    try {
        const parsed: unknown = JSON.parse(requestJson);
        const request = requireRecord(parsed, 'request');
        id = requireString(request['id'], 'request.id');
        const operation = requireString(request['operation'], 'request.operation');
        const payload = requireRecord(request['payload'], 'request.payload');
        value = await dispatch(operation as Request['operation'], payload);
    } catch (caught) {
        error = errorEnvelope(caught);
    }
    const events = runtime === null ? [] : runtime.flushEvents();
    const reply: PeerReply = { id, value, error, events };
    return JSON.stringify(reply);
}

declare global {
    interface Window {
        __tableInteropPeer?: WebPeerHandler;
    }
}

window.__tableInteropPeer = { handle };
