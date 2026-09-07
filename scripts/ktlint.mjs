import { spawnSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import { mkdir, readFile, rename, rm, writeFile } from 'node:fs/promises';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const version = '1.8.0';
const checksum = '369ad2b789f95a011f807e1fcb690ccef80bd7cd014fd139e73ae82dcc0baeab';
const root = fileURLToPath(new URL('../', import.meta.url));
const jar = join(root, '.tmp', 'ktlint', `ktlint-${version}.jar`);
const url = `https://repo.maven.apache.org/maven2/com/pinterest/ktlint/ktlint-cli/${version}/ktlint-cli-${version}-all.jar`;
const patterns = [
    '**/*.kt',
    '**/*.kts',
    '!**/node_modules/**',
    '!**/build/**',
    '!**/generated/**',
    '!**/uniffi/**',
    '!rust/bindings/**',
    '!rust/target/**',
    '!example/android/**',
    '!example/ios/**',
];

function verify(bytes) {
    if (createHash('sha256').update(bytes).digest('hex') !== checksum) {
        throw new Error(`ktlint ${version} checksum mismatch; remove ${jar} and retry.`);
    }
}

try {
    try {
        verify(await readFile(jar));
    } catch (error) {
        if (error.code !== 'ENOENT') throw error;
        console.error(`Downloading ktlint ${version}...`);
        const response = await fetch(url, { signal: AbortSignal.timeout(120_000) });
        if (!response.ok) throw new Error(`ktlint download failed: HTTP ${response.status}`);
        const bytes = Buffer.from(await response.arrayBuffer());
        verify(bytes);
        await mkdir(dirname(jar), { recursive: true });
        const temporaryJar = `${jar}.${process.pid}.tmp`;
        try {
            await writeFile(temporaryJar, bytes);
            await rename(temporaryJar, jar);
        } finally {
            await rm(temporaryJar, { force: true });
        }
    }

    const java = process.env.JAVA_HOME
        ? join(process.env.JAVA_HOME, 'bin', process.platform === 'win32' ? 'java.exe' : 'java')
        : 'java';
    const result = spawnSync(
        java,
        ['-jar', jar, '--relative', ...process.argv.slice(2), ...patterns],
        {
            cwd: root,
            stdio: 'inherit',
        }
    );
    if (result.error)
        throw new Error(
            `Cannot run ktlint; install Java 17+ or set JAVA_HOME. ${result.error.message}`
        );
    process.exitCode = result.status ?? 1;
} catch (error) {
    console.error(error.message);
    process.exitCode = 1;
}
