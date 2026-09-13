import { spawn } from 'node:child_process';
import { existsSync } from 'node:fs';
import { dirname } from 'node:path';
import { fileURLToPath } from 'node:url';

const SUITE_FLAG = '--suite';
const SUITE_VALUE_OFFSET = 1;
const SUITE_NAME_PATTERN = /^[a-z][a-z-]*$/;
const PACKAGE_DIRECTORY = dirname(fileURLToPath(import.meta.url));
const CARGO_MANIFEST = fileURLToPath(
    new URL('../../rust/editor-core/Cargo.toml', import.meta.url),
);
const SUITE_CASES: Record<string, readonly string[]> = {
    plumbing: ['plumbing', 'scheduler', 'dependencies'],
    convergence: ['convergence', 'projection-properties'],
};

function requestedSuite(argv: string[]): string {
    const flagIndex = argv.indexOf(SUITE_FLAG);
    const suite = flagIndex === -1 ? undefined : argv[flagIndex + 1];
    if (suite === undefined || !SUITE_NAME_PATTERN.test(suite)) {
        throw new Error(`usage: run.ts ${SUITE_FLAG} <suite> [node --test options]`);
    }
    return suite;
}

function runnerOptions(argv: string[]): string[] {
    const flagIndex = argv.indexOf(SUITE_FLAG);
    return argv.filter(
        (_argument, index) => index !== flagIndex && index !== flagIndex + SUITE_VALUE_OFFSET,
    );
}

function runToCompletion(command: string, args: string[]): Promise<number> {
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

const argv = process.argv.slice(2);
const suite = requestedSuite(argv);
const suiteFiles = (SUITE_CASES[suite] ?? [suite]).map((name) =>
    fileURLToPath(new URL(`./cases/${name}.test.ts`, import.meta.url)),
);
for (const suiteFile of suiteFiles) {
    if (!existsSync(suiteFile)) {
        throw new Error(`there is no interop suite at ${suiteFile}`);
    }
}

const buildCode = await runToCompletion('cargo', [
    'build',
    '--manifest-path',
    CARGO_MANIFEST,
    '--features',
    'table-interop',
    '--example',
    'table_interop_peer',
]);
if (buildCode !== 0) {
    process.exitCode = buildCode;
} else {
    process.exitCode = await runToCompletion(process.execPath, [
        '--import',
        'tsx',
        '--test',
        ...runnerOptions(argv),
        ...suiteFiles,
    ]);
}
