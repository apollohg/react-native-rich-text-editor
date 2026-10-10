import { spawn } from 'node:child_process';
import { dirname } from 'node:path';
import { fileURLToPath } from 'node:url';

export const PACKAGE_DIRECTORY = dirname(fileURLToPath(import.meta.url));

const CARGO_MANIFEST = fileURLToPath(
    new URL('../../rust/editor-core/Cargo.toml', import.meta.url),
);
const TABLE_INTEROP_FEATURE = 'table-interop';
const PEER_EXAMPLE = 'table_interop_peer';

export function runToCompletion(command: string, args: string[]): Promise<number> {
    return new Promise<number>((resolve, reject) => {
        const child = spawn(command, args, { cwd: PACKAGE_DIRECTORY, stdio: 'inherit' });
        child.on('error', reject);
        child.on('exit', (code, signal) => {
            if (code === null) {
                reject(new Error(`${command} terminated with signal ${String(signal)}`));
                return;
            }
            resolve(code);
        });
    });
}

export function buildRustPeer(): Promise<number> {
    return runToCompletion('cargo', [
        'build',
        '--manifest-path',
        CARGO_MANIFEST,
        '--features',
        TABLE_INTEROP_FEATURE,
        '--example',
        PEER_EXAMPLE,
    ]);
}
