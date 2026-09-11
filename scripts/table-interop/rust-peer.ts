import { spawn } from 'node:child_process';
import type { ChildProcessWithoutNullStreams } from 'node:child_process';
import { createInterface } from 'node:readline';
import type { Interface } from 'node:readline';
import { parseReplyValue } from './peer-protocol.js';
import type { Peer, PeerReply, Request } from './peer-protocol.js';

const REQUEST_TIMEOUT_MILLIS = 30_000;
const SHUTDOWN_TIMEOUT_MILLIS = 5_000;
const MAX_STDERR_BYTES = 64 * 1024;
const SHUTDOWN_REQUEST_ID = '__close__';

type Pending = {
    resolve: (reply: PeerReply) => void;
    reject: (reason: Error) => void;
    timer: NodeJS.Timeout;
};

function parseReply(line: string): PeerReply {
    return parseReplyValue(JSON.parse(line));
}

class RustPeer implements Peer {
    private readonly child: ChildProcessWithoutNullStreams;
    private readonly reader: Interface;
    private readonly pending = new Map<string, Pending>();
    private readonly stderrChunks: Buffer[] = [];
    private stderrBytes = 0;
    private exit: { code: number | null; signal: NodeJS.Signals | null } | null = null;
    private spawnError: Error | null = null;
    private failure: Error | null = null;
    private closing: Promise<void> | null = null;

    constructor(executable: string) {
        this.child = spawn(executable, [], { stdio: ['pipe', 'pipe', 'pipe'] });
        this.child.stderr.on('data', (chunk: Buffer) => {
            const room = MAX_STDERR_BYTES - this.stderrBytes;
            if (room > 0) {
                const retained = chunk.subarray(0, room);
                this.stderrChunks.push(retained);
                this.stderrBytes += retained.length;
            }
        });
        this.reader = createInterface({ input: this.child.stdout, crlfDelay: Infinity });
        this.reader.on('line', (line: string) => this.acceptLine(line));
        this.child.on('error', (error: Error) => {
            this.spawnError = error;
            this.exit ??= { code: null, signal: null };
            this.fail(error);
        });
        this.child.on('exit', (code, signal) => {
            this.exit = { code, signal };
            this.fail(
                new Error(
                    `peer exited before replying (code ${String(code)}, signal ${String(signal)})${this.stderrSuffix()}`,
                ),
            );
        });
    }

    private stderrSuffix(): string {
        if (this.stderrBytes === 0) {
            return '';
        }
        return `; stderr: ${Buffer.concat(this.stderrChunks).toString('utf8')}`;
    }

    private acceptLine(line: string): void {
        if (line.trim().length === 0) {
            return;
        }
        let reply: PeerReply;
        try {
            reply = parseReply(line);
        } catch (error) {
            this.fail(error instanceof Error ? error : new Error(String(error)));
            return;
        }
        const pending = this.pending.get(reply.id);
        if (pending === undefined) {
            this.fail(new Error(`peer replied to unknown request id ${reply.id}`));
            return;
        }
        this.pending.delete(reply.id);
        clearTimeout(pending.timer);
        pending.resolve(reply);
    }

    private fail(error: Error): void {
        this.failure ??= error;
        for (const [, pending] of this.pending) {
            clearTimeout(pending.timer);
            pending.reject(error);
        }
        this.pending.clear();
    }

    request(request: Request): Promise<PeerReply> {
        return this.send(request.id, JSON.stringify(request));
    }

    private send(id: string, line: string): Promise<PeerReply> {
        if (this.failure !== null) {
            return Promise.reject(this.failure);
        }
        if (this.pending.has(id)) {
            return Promise.reject(new Error(`duplicate in-flight request id ${id}`));
        }
        return new Promise<PeerReply>((resolve, reject) => {
            const timer = setTimeout(() => {
                this.pending.delete(id);
                reject(new Error(`peer request ${id} timed out after ${REQUEST_TIMEOUT_MILLIS}ms`));
            }, REQUEST_TIMEOUT_MILLIS);
            this.pending.set(id, { resolve, reject, timer });
            this.child.stdin.write(`${line}\n`, (error) => {
                if (error) {
                    this.pending.delete(id);
                    clearTimeout(timer);
                    reject(error);
                }
            });
        });
    }

    close(): Promise<void> {
        this.closing ??= this.shutdown();
        return this.closing;
    }

    private async shutdown(): Promise<void> {
        try {
            if (this.exit === null && this.child.stdin.writable) {
                await this.send(SHUTDOWN_REQUEST_ID, JSON.stringify({
                    id: SHUTDOWN_REQUEST_ID,
                    operation: 'shutdown',
                    payload: {},
                })).catch(() => undefined);
            }
            const exit = await this.awaitExit();
            if (this.spawnError !== null) {
                throw this.spawnError;
            }
            if (exit.code !== 0) {
                throw new Error(
                    `peer exited with code ${String(exit.code)} signal ${String(exit.signal)}${this.stderrSuffix()}`,
                );
            }
        } finally {
            this.release();
        }
    }

    private awaitExit(): Promise<{ code: number | null; signal: NodeJS.Signals | null }> {
        if (this.exit !== null) {
            return Promise.resolve(this.exit);
        }
        return new Promise((resolve) => {
            const timer = setTimeout(() => {
                this.child.kill('SIGKILL');
            }, SHUTDOWN_TIMEOUT_MILLIS);
            this.child.once('exit', (code, signal) => {
                clearTimeout(timer);
                resolve({ code, signal });
            });
        });
    }

    private release(): void {
        this.fail(new Error('peer is closed'));
        this.reader.close();
        this.child.stdin.destroy();
        this.child.stdout.destroy();
        this.child.stderr.destroy();
    }
}

export async function startRustPeer(executable: string): Promise<Peer> {
    return new RustPeer(executable);
}
