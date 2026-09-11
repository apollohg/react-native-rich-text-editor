import { spawnSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import { chmod, mkdir, readFile, rename, rm, stat, writeFile } from 'node:fs/promises';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';

const version = '0.65.1';
const checksum = 'c1e429b0599cf1b516f369a2d9ec04eaf0e436f3c12b637df8851fa52ff694d0';
const root = fileURLToPath(new URL('../', import.meta.url));
const home = join(root, '.tmp', 'swiftlint');
const archive = join(home, `portable_swiftlint-${version}.zip`);
const binary = join(home, `swiftlint-${version}`);
const url = `https://github.com/realm/SwiftLint/releases/download/${version}/portable_swiftlint.zip`;

function verify(bytes) {
    if (createHash('sha256').update(bytes).digest('hex') !== checksum) {
        throw new Error(`SwiftLint ${version} checksum mismatch; remove ${archive} and retry.`);
    }
}

async function download() {
    console.error(`Downloading SwiftLint ${version}...`);
    const response = await fetch(url, { signal: AbortSignal.timeout(120_000) });
    if (!response.ok) throw new Error(`SwiftLint download failed: HTTP ${response.status}`);
    const bytes = Buffer.from(await response.arrayBuffer());
    verify(bytes);
    await mkdir(home, { recursive: true });
    const temporaryArchive = `${archive}.${process.pid}.tmp`;
    try {
        await writeFile(temporaryArchive, bytes);
        await rename(temporaryArchive, archive);
    } finally {
        await rm(temporaryArchive, { force: true });
    }
}

async function extract() {
    const staging = `${binary}.${process.pid}.tmp`;
    await rm(staging, { recursive: true, force: true });
    try {
        const result = spawnSync('unzip', ['-o', '-q', '-d', staging, archive, 'swiftlint'], {
            stdio: 'inherit',
        });
        if (result.error) throw new Error(`Cannot unpack SwiftLint: ${result.error.message}`);
        if (result.status !== 0) throw new Error(`Cannot unpack SwiftLint: unzip exited ${result.status}`);
        await chmod(join(staging, 'swiftlint'), 0o755);
        await rename(join(staging, 'swiftlint'), binary);
    } finally {
        await rm(staging, { recursive: true, force: true });
    }
}

try {
    // The pinned release ships a macOS-only portable build.
    if (process.platform !== 'darwin') {
        throw new Error(`SwiftLint ${version} requires macOS; run this lint on a macOS host.`);
    }

    try {
        verify(await readFile(archive));
    } catch (error) {
        if (error.code !== 'ENOENT') throw error;
        await download();
    }

    try {
        await stat(binary);
    } catch (error) {
        if (error.code !== 'ENOENT') throw error;
        await extract();
    }

    const result = spawnSync(binary, process.argv.slice(2), { cwd: root, stdio: 'inherit' });
    if (result.error) throw new Error(`Cannot run SwiftLint; ${result.error.message}`);
    process.exitCode = result.status ?? 1;
} catch (error) {
    console.error(error.message);
    process.exitCode = 1;
}
