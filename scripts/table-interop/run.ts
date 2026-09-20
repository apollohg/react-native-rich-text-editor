import { existsSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { buildRustPeer, runToCompletion } from './peer-build.js';
import { readFile } from 'node:fs/promises';
import { SUITE_CASES, verifyProvenance } from './checkpoint-io.js';

const SUITE_FLAG = '--suite';
const SUITE_VALUE_OFFSET = 1;
const SUITE_NAME_PATTERN = /^[a-z][a-z-]*$/;

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
const builtPeer = argv.indexOf('--built-peer');
if (builtPeer !== -1) {
    const path = argv[builtPeer + 1];
    if (!path) throw new Error('--built-peer requires source/native provenance');
    await verifyProvenance(JSON.parse(await readFile(path, 'utf8')));
    argv.splice(builtPeer, 2);
}
const suite = requestedSuite(argv);
const suiteFiles = (SUITE_CASES[suite] ?? [suite]).map((name) =>
    fileURLToPath(new URL(`./cases/${name}.test.ts`, import.meta.url)),
);
for (const suiteFile of suiteFiles) {
    if (!existsSync(suiteFile)) {
        throw new Error(`there is no interop suite at ${suiteFile}`);
    }
}

const buildCode = builtPeer === -1 ? await buildRustPeer() : 0;
if (buildCode !== 0) {
    process.exitCode = buildCode;
} else {
    process.exitCode = await runToCompletion(process.execPath, [
        '--import',
        'tsx',
        '--test',
        '--test-concurrency=1',
        ...runnerOptions(argv),
        ...suiteFiles,
    ]);
}
