import assert from 'node:assert/strict';
import { spawn } from 'node:child_process';
import { createHash } from 'node:crypto';
import { createReadStream, createWriteStream } from 'node:fs';
import { mkdir, open, readFile, writeFile } from 'node:fs/promises';
import { resolve, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { finished } from 'node:stream/promises';
import {
    CONVERGENCE_CORPUS,
    continuationRequirements,
    runContinuation,
    textHistoryRequirements,
    type ContinuationResult,
    type ContinuationSlot,
} from './corpus.js';
import {
    runSupplementary,
    supplementaryRequirements,
    type SupplementarySlot,
} from './supplementary-continuity.js';
import { availabilityVerified } from './availability-evidence.js';
import { executeContinuations } from './continuation-runner.js';
import { CheckpointReport } from './checkpoint-report.js';
import { appendEvidence, type BaseResult } from './checkpoint-evidence.js';
import {
    jsonLines,
    provenance,
    safetyResult,
    SUITE_CASES,
    suitePassed,
    suiteFilePassed,
    verifyProvenance,
    type SuiteEvent,
    type SuiteResult,
} from './checkpoint-io.js';
import { buildRustPeer, PACKAGE_DIRECTORY } from './peer-build.js';

const baseManifest = CONVERGENCE_CORPUS.map(({ scenario, ...schedule }) => ({
    ...schedule,
    scenario: scenario.name,
}));
const frozenManifest = CONVERGENCE_CORPUS.map(
    ({
        name,
        topology,
        kinds,
        participants,
        preset,
        scenario,
        actorOffset,
        seed,
    }) => ({
        name,
        topology,
        kinds,
        participants,
        preset,
        scenario: scenario.name,
        actorOffset,
        seed,
    }),
);
assert.equal(
    createHash('sha256').update(JSON.stringify(frozenManifest)).digest('hex'),
    '603770fda8be1800bc64a2680d92ab380f27980a3f45f6bbeec65b8b14300b0d',
    'frozen base manifest',
);

async function immutable(path: string, value: unknown) {
    await writeFile(path, JSON.stringify(value), { flag: 'wx' });
}

async function command(
    command: string,
    args: string[],
    log: string,
    env: NodeJS.ProcessEnv = process.env,
) {
    const stream = createWriteStream(log, { flags: 'wx' });
    const child = spawn(command, args, {
        cwd: PACKAGE_DIRECTORY,
        env,
        stdio: ['ignore', 'pipe', 'pipe'],
    });
    child.stdout.pipe(stream, { end: false });
    child.stderr.pipe(stream, { end: false });
    const code = await new Promise<number>((resolve, reject) => {
        child.on('error', reject);
        child.on('close', (code) => resolve(code ?? 1));
    });
    stream.end();
    await finished(stream);
    return code;
}

async function hashFile(path: string) {
    const hash = createHash('sha256');
    for await (const chunk of createReadStream(path)) hash.update(chunk);
    return hash.digest('hex');
}

async function executeFile(
    directory: string,
    name: string,
    slots: readonly ContinuationSlot[],
) {
    const path = join(directory, `${name}.jsonl`);
    const file = await open(path, 'wx');
    await immutable(`${path}.manifest.json`, slots);
    let executed = 0;
    try {
        const summary = await executeContinuations(
            slots,
            (slot) =>
                'family' in slot
                    ? runSupplementary(slot as SupplementarySlot)
                    : runContinuation(slot),
            async (result) => {
                await file.write(`${JSON.stringify(result)}\n`);
                if (++executed % 100 === 0)
                    console.log(
                        JSON.stringify({
                            matrix: name,
                            executed,
                            declared: slots.length,
                        }),
                    );
            },
        );
        await immutable(`${path}.summary.json`, summary);
    } finally {
        await file.close();
    }
}

type Location = {
    file: string;
    offset: number;
    length: number;
    slot: ContinuationSlot;
};
async function indexOriginals(
    directory: string,
    consume?: (result: ContinuationResult) => void,
) {
    const origins = new Map<string, Location>();
    const named = new Set(
        supplementaryRequirements()
            .filter(
                (slot) => slot.family === 'overlap' && slot.proof === 'history',
            )
            .map((slot) => slot.key),
    );
    for (const name of ['continuations', 'supplementary']) {
        const file = join(directory, `${name}.jsonl`);
        let offset = 0;
        for await (const result of jsonLines<ContinuationResult>(file)) {
            const length = Buffer.byteLength(`${JSON.stringify(result)}\n`);
            consume?.(result);
            if (
                result.slot.proof === 'history' &&
                (availabilityVerified(result) || named.has(result.slot.key))
            )
                origins.set(result.slot.key, {
                    file,
                    offset,
                    length,
                    slot: result.slot,
                });
            offset += length;
        }
    }
    return origins;
}

async function readOriginal(location: Location) {
    const file = await open(location.file, 'r');
    try {
        const bytes = Buffer.alloc(location.length);
        const result = await file.read(bytes, 0, bytes.length, location.offset);
        assert.equal(
            result.bytesRead,
            bytes.length,
            'complete original evidence record',
        );
        return JSON.parse(bytes.toString('utf8')) as ContinuationResult;
    } finally {
        await file.close();
    }
}

async function report(directory: string) {
    await verifyProvenance(
        JSON.parse(await readFile(join(directory, 'provenance.json'), 'utf8')),
    );
    const completion = JSON.parse(
        await readFile(join(directory, 'completion.json'), 'utf8'),
    ) as { files: Record<string, string>; typecheck: number };
    const required = [
        'base.jsonl',
        'safety.jsonl',
        'continuations.jsonl',
        'supplementary.jsonl',
        'companions.jsonl',
        'suites.jsonl',
    ];
    assert.ok(
        required.every((name) => name in completion.files),
        'missing required execution artifacts',
    );
    for (const [name, expected] of Object.entries(completion.files))
        assert.equal(
            await hashFile(join(directory, name)),
            expected,
            `artifact digest ${name}`,
        );
    const aggregate = new CheckpointReport();
    for await (const result of jsonLines<BaseResult>(
        join(directory, 'base.jsonl'),
    ))
        aggregate.addBase(result);
    const origins = await indexOriginals(directory, (result) =>
        aggregate.addContinuation(result),
    );
    for await (const result of jsonLines<ContinuationResult>(
        join(directory, 'companions.jsonl'),
    )) {
        const original = origins.get(result.slot.companionOf ?? '');
        assert.ok(
            original,
            `missing unavailable original ${result.slot.companionOf}`,
        );
        aggregate.addCompanion(await readOriginal(original), result);
    }
    const suites = new Map<string, SuiteResult>();
    for await (const suite of jsonLines<SuiteResult>(
        join(directory, 'suites.jsonl'),
    )) {
        assert.ok(!suites.has(suite.suite), 'duplicate suite evidence');
        suites.set(suite.suite, suite);
    }
    const safety = [];
    for await (const observation of jsonLines<
        Parameters<typeof safetyResult>[0][number]
    >(join(directory, 'safety.jsonl')))
        safety.push(observation);
    const observed = safetyResult(safety);
    const passed = (name: string) =>
        suites.has(name) && suitePassed(suites.get(name)!);
    const result = aggregate.finish({
        plumbingPassed: passed('plumbing'),
        differentialPassed: passed('differential'),
        projectionPassed: suiteFilePassed(
            suites.get('convergence'),
            'projection-properties',
        ),
        safetyComplete:
            observed.complete &&
            passed('convergence') &&
            passed('checkpoint-regressions') &&
            completion.typecheck === 0,
        unsafeAdmissions: observed.unsafeAdmissions,
        unexpectedSourceCellLosses: observed.unexpectedSourceCellLosses,
    });
    return {
        ...result,
        suites: Object.fromEntries(
            [...suites].map(([name, suite]) => [name, suitePassed(suite)]),
        ),
        safety: observed,
        typecheck: completion.typecheck,
    };
}

async function execute(directory: string) {
    await mkdir(directory);
    console.log(
        'Building the single prerequisite native peer before provenance freeze',
    );
    assert.equal(await buildRustPeer(), 0, 'native peer prerequisite build');
    const frozen = await provenance();
    await immutable(join(directory, 'provenance.json'), frozen);
    await immutable(join(directory, 'base.manifest.json'), baseManifest);
    for (const name of ['base', 'safety', 'suites'])
        await writeFile(join(directory, `${name}.jsonl`), '', { flag: 'wx' });
    const env = {
        ...process.env,
        TABLE_CHECKPOINT_BASE: join(directory, 'base.jsonl'),
        TABLE_CHECKPOINT_SAFETY: join(directory, 'safety.jsonl'),
    };
    for (const suite of Object.keys(SUITE_CASES)) {
        await verifyProvenance(frozen);
        const eventsPath = join(directory, `${suite}.events.jsonl`);
        const args = [
            '--prefix',
            PACKAGE_DIRECTORY,
            'run',
            suite === 'checkpoint-regressions' ? 'test:unit' : `test:${suite}`,
        ];
        const actualArgs =
            suite === 'checkpoint-regressions'
                ? ['--import', 'tsx', 'run.ts', '--suite', suite]
                : [...args, '--'];
        actualArgs.push(
            '--built-peer',
            join(directory, 'provenance.json'),
            '--test-concurrency=1',
            '--test-reporter',
            './checkpoint-reporter.ts',
            '--test-reporter-destination',
            eventsPath,
        );
        const exitCode = await command(
            suite === 'checkpoint-regressions' ? process.execPath : 'npm',
            actualArgs,
            join(directory, `${suite}.log`),
            env,
        );
        const events: SuiteEvent[] = [];
        for await (const event of jsonLines<SuiteEvent>(eventsPath))
            events.push(event);
        const result = { suite, exitCode, files: SUITE_CASES[suite]!, events };
        appendEvidence(join(directory, 'suites.jsonl'), result);
        console.log(
            JSON.stringify({
                suite,
                exitCode,
                evidencePassed: suitePassed(result),
            }),
        );
    }
    const typecheck = await command(
        'npm',
        ['run', 'typecheck'],
        join(directory, 'typecheck.log'),
    );
    await verifyProvenance(frozen);
    await executeFile(directory, 'continuations', continuationRequirements());
    await verifyProvenance(frozen);
    await executeFile(directory, 'supplementary', supplementaryRequirements());
    const origins = await indexOriginals(directory);
    const declared = [
        ...continuationRequirements(),
        ...supplementaryRequirements(),
    ];
    const required = declared.filter((slot) => origins.has(slot.key));
    await executeFile(
        directory,
        'companions',
        textHistoryRequirements(required),
    );
    await verifyProvenance(frozen);
    const files: Record<string, string> = {};
    for (const name of [
        'base.jsonl',
        'safety.jsonl',
        'continuations.jsonl',
        'supplementary.jsonl',
        'companions.jsonl',
        'suites.jsonl',
    ])
        files[name] = await hashFile(join(directory, name));
    await immutable(join(directory, 'completion.json'), { files, typecheck });
    const result = await report(directory);
    await immutable(join(directory, 'report.json'), result);
    console.log(
        JSON.stringify({
            runnerCompleted: true,
            passed: result.passed,
            gate: result.gate,
            report: join(directory, 'report.json'),
        }),
    );
    process.exitCode = result.passed ? 0 : 1;
}

if (
    process.argv[1] &&
    resolve(process.argv[1]) === fileURLToPath(import.meta.url)
) {
    const [mode, path] = process.argv.slice(2);
    assert.ok(
        path && ['--output', '--report'].includes(mode!),
        'usage: run-checkpoint.ts (--output NEW_DIRECTORY | --report DIRECTORY)',
    );
    if (mode === '--output') await execute(resolve(path));
    else {
        const result = await report(resolve(path));
        console.log(JSON.stringify(result));
        process.exitCode = result.passed ? 0 : 1;
    }
}

export {
    executeFile,
    indexOriginals,
    readOriginal,
    report as reportCheckpoint,
};
