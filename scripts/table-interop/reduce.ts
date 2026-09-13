import { readFileSync } from 'node:fs';
import { isAbsolute, resolve } from 'node:path';
import { replayTrace } from './controller.js';
import { isRecord } from './peer-protocol.js';
import { buildRustPeer, PACKAGE_DIRECTORY } from './peer-build.js';
import {
    lastMinimizedTracePath,
    reduceTrace,
    traceFailureSignature,
    TRACE_PROTOCOL_VERSION,
} from './trace.js';
import type { Trace } from './trace.js';

const TRACE_FLAG = '--trace';
const TRACE_VALUE_OFFSET = 1;
const ACTION_RECORD = 'action';
const DELIVERY_RECORD = 'delivery';
const DRAIN_RECORD = 'drain';
const PAYLOAD_PREVIEW_BYTES = 96;

function requestedTracePath(argv: string[]): string {
    const flagIndex = argv.indexOf(TRACE_FLAG);
    const path = flagIndex === -1 ? undefined : argv[flagIndex + TRACE_VALUE_OFFSET];
    if (path === undefined || path.length === 0) {
        throw new Error(`usage: reduce.ts ${TRACE_FLAG} <trace.json>`);
    }
    return isAbsolute(path) ? path : resolve(PACKAGE_DIRECTORY, path);
}

function requireTrace(parsed: unknown, path: string): Trace {
    if (!isRecord(parsed)) {
        throw new Error(`${path} does not hold a trace object`);
    }
    if (parsed['protocolVersion'] !== TRACE_PROTOCOL_VERSION) {
        throw new Error(
            `${path} carries protocol ${JSON.stringify(parsed['protocolVersion'])}, not `
                + TRACE_PROTOCOL_VERSION,
        );
    }
    if (!Array.isArray(parsed['records'])) {
        throw new Error(`${path} carries no records array`);
    }
    if (typeof parsed['failureClass'] !== 'string') {
        throw new Error(`${path} carries no recorded failure class, so it cannot be reduced`);
    }
    if (!isRecord(parsed['initialization']) || !isRecord(parsed['dependencies'])) {
        throw new Error(`${path} carries no initialization or dependency manifest`);
    }
    return parsed as unknown as Trace;
}

function describeRecords(trace: Trace): string[] {
    const lines: string[] = [];
    for (const record of trace.records) {
        if (record.kind === ACTION_RECORD) {
            const payload = JSON.stringify(record.payload).slice(0, PAYLOAD_PREVIEW_BYTES);
            lines.push(`  peer${record.peer} ${record.operation} ${payload}`);
            continue;
        }
        if (record.kind === DRAIN_RECORD) {
            lines.push(`  drain peers=[${record.peers.join(', ')}] seed=${record.seed}`);
            continue;
        }
        if (record.kind === DELIVERY_RECORD) {
            lines.push(
                `  deliver ${record.sender}->${record.recipient} sequence=${record.sequence} `
                    + `digest=${record.digest}`,
            );
        }
    }
    return lines;
}

const path = requestedTracePath(process.argv.slice(2));
const buildCode = await buildRustPeer();
if (buildCode !== 0) {
    process.exitCode = buildCode;
} else {
    const trace = requireTrace(JSON.parse(readFileSync(path, 'utf8')), path);
    process.stdout.write(
        `trace ${path}\n`
            + `  peers ${trace.initialization.kinds.join('+')} seed ${trace.initialization.seed}\n`
            + `  recorded failure ${String(trace.failureClass)}: ${String(trace.failureMessage)}\n`
            + `  records ${trace.records.length}\n`,
    );
    const expected = traceFailureSignature(trace);
    const observed = await replayTrace(trace);
    process.stdout.write(`  recorded signature ${expected}\n  replayed signature ${String(observed)}\n`);
    if (observed !== expected) {
        process.stdout.write(
            '  the recorded failure no longer reproduces, so there is nothing to reduce\n',
        );
    } else {
        const reduced = await reduceTrace(trace, replayTrace);
        process.stdout.write(
            `  reduced to ${reduced.records.length} records at ${lastMinimizedTracePath()}\n`
                + `${describeRecords(reduced).join('\n')}\n`,
        );
    }
}
