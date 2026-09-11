import * as Y from 'yjs';
import { EditorState, Plugin, PluginKey } from 'prosemirror-state';
import type { Transaction } from 'prosemirror-state';
import { EditorView } from 'prosemirror-view';
import { schema as prosemirrorBasicSchema } from 'prosemirror-schema-basic';
import {
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
): MountedEditor {
    const state = EditorState.create({
        schema: prosemirrorBasicSchema,
        plugins: [ySyncPlugin(fragment), yUndoPlugin(), normalizationCounterPlugin(counter)],
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

function applyCommand(editor: MountedEditor, command: Record<string, unknown>): void {
    const type = command['type'];
    if (type !== 'insertText') {
        throw new PeerOperationError(
            CONFIG_INVALID,
            `command ${JSON.stringify(type)} is not served by the web peer`,
        );
    }
    const text = requireString(command['text'], 'command.text');
    editor.view.dispatch(editor.view.state.tr.insertText(text));
}

class WebPeerRuntime {
    private readonly kind: WebPeerKind;
    private readonly element: HTMLElement;
    private readonly fragmentName: string;
    private readonly maxUpdateBytes: number;
    private readonly bindingOrigin: unknown;
    private readonly document = new Y.Doc();
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
    ) {
        this.kind = kind;
        this.element = element;
        this.fragmentName = fragmentName;
        this.maxUpdateBytes = maxUpdateBytes;
        this.bindingOrigin = bindingOriginFor(kind);
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
            )
            : mountTiptap(this.document, this.fragmentName, this.element, this.counter);
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
        const command = requireRecord(payload['command'], 'payload.command');
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
        if (this.editor !== null) {
            this.editor.destroy();
            this.editor = null;
        }
        this.document.destroy();
    }
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
    if (tables) {
        throw new PeerOperationError(
            CONFIG_INVALID,
            'the web peer has no table schema yet; table enablement is not served',
        );
    }
    const fragmentName = requireString(payload['fragmentName'], 'payload.fragmentName');
    const limits = requireRecord(payload['limits'], 'payload.limits');
    const maxUpdateBytes = requirePositiveInteger(
        limits['maxUpdateBytes'],
        'payload.limits.maxUpdateBytes',
    );
    const awaitSeed = requireBoolean(payload['awaitSeed'], 'payload.awaitSeed');
    const created = new WebPeerRuntime(readKind(), readElement(), fragmentName, maxUpdateBytes);
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
