import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import test from 'node:test';
import * as Y from 'yjs';
import { canonicalDocumentShape } from '../assertions.js';
import { call, snapshot, tableFixture, withPeers } from '../controller.js';
import { EXPORTS_VARIABLE, nativeAcceptanceExportPaths } from '../native-acceptance-exports.js';
import { isRecord } from '../peer-protocol.js';

const WEB_EDIT = { type: 'appendParagraph' };
const MARKS_KEY = 'marks';
const ATTRIBUTES_KEY = 'attrs';
const ATTRIBUTELESS_MARK_FORMAT = true;

type NativeAcceptanceExport = {
    readonly path: string;
    readonly platform: string;
    readonly documentJson: Record<string, unknown>;
    readonly encodedStateBase64: string;
};

const EXPORT_PATHS = nativeAcceptanceExportPaths();
const NO_EXPORTS_REASON = `${EXPORTS_VARIABLE} is unset`;

async function readExport(path: string): Promise<NativeAcceptanceExport> {
    const parsed: unknown = JSON.parse(await readFile(path, 'utf8'));
    if (
        !isRecord(parsed) ||
        typeof parsed['platform'] !== 'string' ||
        !isRecord(parsed['documentJson']) ||
        typeof parsed['encodedStateBase64'] !== 'string'
    ) {
        throw new Error(`${path} is not a native table acceptance export`);
    }
    return {
        path,
        platform: parsed['platform'],
        documentJson: parsed['documentJson'],
        encodedStateBase64: parsed['encodedStateBase64'],
    };
}

function clientsOf(stateVectorBase64: string): Set<number> {
    return new Set(Y.decodeStateVector(Buffer.from(stateVectorBase64, 'base64')).keys());
}

function withAttributelessMarkFormats(value: unknown): unknown {
    if (Array.isArray(value)) {
        return value.map(withAttributelessMarkFormats);
    }
    if (!isRecord(value)) {
        return value;
    }
    return Object.fromEntries(
        Object.entries(value).map(([key, child]) => [
            key,
            key === MARKS_KEY && Array.isArray(child)
                ? child.map((mark) =>
                    isRecord(mark) && mark[ATTRIBUTES_KEY] === ATTRIBUTELESS_MARK_FORMAT
                        ? Object.fromEntries(Object.entries(mark).filter(([field]) => field !== ATTRIBUTES_KEY))
                        : mark)
                : withAttributelessMarkFormats(child),
        ]),
    );
}

function shapeOf(document: unknown): string {
    return JSON.stringify(canonicalDocumentShape(document));
}

async function webPeerReadsTheExport(exported: NativeAcceptanceExport): Promise<void> {
    await withPeers(
        ['rust', 'prosemirror'] as const,
        async ([, web]) => {
            await call(web, 'applyUpdate', { updateBase64: exported.encodedStateBase64 });
            const seeded = await snapshot(web);
            assert.equal(seeded.pendingDependencies, false, `${exported.path} left the web peer waiting on updates`);
            assert.equal(seeded.mounted, true, 'the seeded web peer never mounted its editor');
            const nativeShape = shapeOf(exported.documentJson);
            assert.equal(
                shapeOf(seeded.displayJson),
                nativeShape,
                `the web editor state differs from the ${exported.platform} document, so it would repair it`,
            );
            assert.equal(
                shapeOf(withAttributelessMarkFormats(seeded.documentJson)),
                nativeShape,
                `the web peer reads a different raw document than ${exported.platform} wrote`,
            );

            const nativeClients = clientsOf(seeded.stateVectorBase64);
            assert.ok(nativeClients.size > 0, 'the export carries no native client at all');
            await call(web, 'command', WEB_EDIT);
            const edited = await snapshot(web);
            const webClients = [...clientsOf(edited.stateVectorBase64)].filter(
                (client) => !nativeClients.has(client),
            );
            assert.equal(
                webClients.length,
                1,
                `the web peer must write under its own client id, not one of ${[...nativeClients].join(', ')}`,
            );
        },
        tableFixture('prosemirror'),
    );
}

test('a stock web peer reads each native acceptance export exactly as the device wrote it', {
    skip: EXPORT_PATHS === null ? NO_EXPORTS_REASON : false,
}, async (context) => {
    for (const path of EXPORT_PATHS ?? []) {
        const exported = await readExport(path);
        await context.test(`the web peer reads the ${exported.platform} export`, async () => {
            await webPeerReadsTheExport(exported);
        });
    }
});
