import { createServer } from 'node:http';
import type { IncomingMessage, Server, ServerResponse } from 'node:http';
import { readFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { fileURLToPath } from 'node:url';
import { build } from 'esbuild';
import { chromium } from 'playwright';
import type { Browser, ConsoleMessage, Page } from 'playwright';
import { isRecord, parseReplyValue } from './peer-protocol.js';
import type { Peer, PeerReply, Request, WebPeerHandler } from './peer-protocol.js';

declare const window: { __tableInteropPeer?: WebPeerHandler };

const STARTUP_TIMEOUT_MILLIS = 30_000;
const CLOSE_TIMEOUT_MILLIS = 5_000;
const MAX_BROWSER_LOG_BYTES = 64 * 1024;
const SHUTDOWN_REQUEST_ID = '__close__';
const BUNDLE_PATH = '/peer.js';
const ENTRY_POINT = fileURLToPath(new URL('./browser/peer.ts', import.meta.url));
const PAGE_DOCUMENT = fileURLToPath(new URL('./browser/index.html', import.meta.url));
const PACKAGE_MANAGER_NEUTRAL_DIRECTORY = tmpdir();

let bundle: Promise<string> | null = null;

async function browserBundle(): Promise<string> {
    bundle ??= build({
        entryPoints: [ENTRY_POINT],
        bundle: true,
        platform: 'browser',
        format: 'esm',
        write: false,
        absWorkingDir: PACKAGE_MANAGER_NEUTRAL_DIRECTORY,
    }).then((result) => {
        const output = result.outputFiles?.[0];
        if (output === undefined) {
            throw new Error('esbuild produced no browser peer bundle');
        }
        return output.text;
    });
    return bundle;
}

function requestTimeoutFrom(config: Record<string, unknown>): number {
    const limits = config['limits'];
    const configured = isRecord(limits) ? limits['requestTimeoutMillis'] : undefined;
    if (typeof configured !== 'number' || !Number.isInteger(configured) || configured <= 0) {
        throw new Error('the peer config must carry a positive integer limits.requestTimeoutMillis');
    }
    return configured;
}

async function startPageServer(bundleSource: string): Promise<{ server: Server; origin: string }> {
    const page = await readFile(PAGE_DOCUMENT, 'utf8');
    const server = createServer((request: IncomingMessage, response: ServerResponse) => {
        const path = (request.url ?? '/').split('?')[0];
        if (path === BUNDLE_PATH) {
            response.writeHead(200, { 'content-type': 'text/javascript; charset=utf-8' });
            response.end(bundleSource);
            return;
        }
        if (path === '/') {
            response.writeHead(200, { 'content-type': 'text/html; charset=utf-8' });
            response.end(page);
            return;
        }
        response.writeHead(404, { 'content-type': 'text/plain; charset=utf-8' });
        response.end('not found');
    });
    await new Promise<void>((resolve, reject) => {
        server.once('error', reject);
        server.listen(0, '127.0.0.1', resolve);
    });
    const address = server.address();
    if (address === null || typeof address === 'string') {
        server.close();
        throw new Error('the peer page server did not bind a loopback port');
    }
    return { server, origin: `http://127.0.0.1:${address.port}` };
}

function closeServer(server: Server): Promise<void> {
    return new Promise<void>((resolve, reject) => {
        server.close((error) => {
            if (error) {
                reject(error);
                return;
            }
            resolve();
        });
    });
}

async function withTimeout<T>(work: Promise<T>, millis: number, description: string): Promise<T> {
    let timer: NodeJS.Timeout | undefined;
    try {
        return await Promise.race([
            work,
            new Promise<never>((_resolve, reject) => {
                timer = setTimeout(() => {
                    reject(new Error(`${description} timed out after ${millis}ms`));
                }, millis);
            }),
        ]);
    } finally {
        clearTimeout(timer);
    }
}

class WebPeer implements Peer {
    private readonly browser: Browser;
    private readonly page: Page;
    private readonly server: Server;
    private readonly requestTimeoutMillis: number;
    private readonly log: string[] = [];
    private logBytes = 0;
    private readonly inFlight = new Set<string>();
    private failure: Error | null = null;
    private closing: Promise<void> | null = null;

    constructor(browser: Browser, page: Page, server: Server, requestTimeoutMillis: number) {
        this.browser = browser;
        this.page = page;
        this.server = server;
        this.requestTimeoutMillis = requestTimeoutMillis;
    }

    record(entry: string): void {
        const room = MAX_BROWSER_LOG_BYTES - this.logBytes;
        if (room <= 0) {
            return;
        }
        const retained = entry.slice(0, room);
        this.log.push(retained);
        this.logBytes += retained.length;
    }

    logSuffix(): string {
        if (this.log.length === 0) {
            return '';
        }
        return `; browser log: ${this.log.join('\n')}`;
    }

    private fail(error: Error): void {
        this.failure ??= error;
    }

    async request(request: Request): Promise<PeerReply> {
        if (this.failure !== null) {
            throw this.failure;
        }
        if (this.inFlight.has(request.id)) {
            throw new Error(`duplicate in-flight request id ${request.id}`);
        }
        this.inFlight.add(request.id);
        try {
            const replyJson = await withTimeout(
                this.page.evaluate((json: string) => {
                    const peer = window.__tableInteropPeer;
                    if (peer === undefined) {
                        throw new Error('the peer page exposed no request handler');
                    }
                    return peer.handle(json);
                }, JSON.stringify(request)),
                this.requestTimeoutMillis,
                `web peer request ${request.id}`,
            );
            return parseReplyValue(JSON.parse(replyJson));
        } catch (error) {
            const raised = error instanceof Error ? error : new Error(String(error));
            const reported = new Error(`${raised.message}${this.logSuffix()}`);
            this.fail(reported);
            throw reported;
        } finally {
            this.inFlight.delete(request.id);
        }
    }

    close(): Promise<void> {
        this.closing ??= this.shutdown();
        return this.closing;
    }

    private async shutdown(): Promise<void> {
        try {
            if (this.failure === null) {
                await this.request({
                    id: SHUTDOWN_REQUEST_ID,
                    operation: 'shutdown',
                    payload: {},
                }).catch(() => undefined);
            }
        } finally {
            await this.release();
        }
    }

    private async release(): Promise<void> {
        this.fail(new Error('peer is closed'));
        let browserError: Error | null = null;
        try {
            await withTimeout(this.browser.close(), CLOSE_TIMEOUT_MILLIS, 'browser close');
        } catch (error) {
            browserError = error instanceof Error ? error : new Error(String(error));
        }
        await closeServer(this.server);
        if (browserError !== null) {
            throw browserError;
        }
    }
}

export async function startWebPeer(
    kind: 'prosemirror' | 'tiptap',
    config: Record<string, unknown>,
): Promise<Peer> {
    const requestTimeoutMillis = requestTimeoutFrom(config);
    const bundleSource = await browserBundle();
    const { server, origin } = await startPageServer(bundleSource);
    let browser: Browser | null = null;
    try {
        browser = await chromium.launch();
        const page = await browser.newPage();
        const peer = new WebPeer(browser, page, server, requestTimeoutMillis);
        page.on('pageerror', (error: Error) => peer.record(`pageerror: ${error.message}`));
        page.on('crash', () => peer.record('the peer page crashed'));
        page.on('console', (message: ConsoleMessage) => {
            if (message.type() === 'error') {
                peer.record(`console.error: ${message.text()}`);
            }
        });
        const response = await page.goto(`${origin}/?kind=${kind}`);
        if (response === null || !response.ok()) {
            throw new Error(`the peer page did not load from ${origin}`);
        }
        await page.waitForFunction(
            () => window.__tableInteropPeer !== undefined,
            undefined,
            { timeout: STARTUP_TIMEOUT_MILLIS },
        );
        return peer;
    } catch (error) {
        if (browser !== null) {
            await browser.close().catch(() => undefined);
        }
        await closeServer(server).catch(() => undefined);
        throw error;
    }
}
