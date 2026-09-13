import { existsSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { buildRustPeer, runToCompletion } from './peer-build.js';

const SUITE_FLAG = '--suite';
const SUITE_VALUE_OFFSET = 1;
const SUITE_NAME_PATTERN = /^[a-z][a-z-]*$/;
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

const buildCode = await buildRustPeer();
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
