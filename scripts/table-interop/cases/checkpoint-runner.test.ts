import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { mkdtemp, readFile, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { spawnSync } from 'node:child_process';
import test from 'node:test';
import { executeFile, reportCheckpoint } from '../run-checkpoint.js';
import { provenance, SUITE_CASES } from '../checkpoint-io.js';
import {
    CONVERGENCE_CORPUS,
    continuationRequirements,
    textHistoryRequirements,
} from '../corpus.js';
import { supplementaryRequirements } from '../supplementary-continuity.js';
import { runBaseSchedule } from '../checkpoint-evidence.js';
import { PACKAGE_DIRECTORY } from '../peer-build.js';

test('TBL-24 streaming runner preserves actual partial proof without claiming complete coverage', async () => {
    const directory = await mkdtemp(join(tmpdir(), 'table-checkpoint-smoke-'));
    await writeFile(
        join(directory, 'provenance.json'),
        JSON.stringify(await provenance()),
    );
    const base = await runBaseSchedule(CONVERGENCE_CORPUS[0]!);
    await writeFile(join(directory, 'base.jsonl'), `${JSON.stringify(base)}\n`);
    const nested = continuationRequirements().find(
        (slot) =>
            slot.baseFamily.includes('nested') && slot.proof === 'history',
    )!;
    assert.ok(nested);
    await executeFile(directory, 'continuations', [nested]);
    const originals = supplementaryRequirements()
        .filter((slot) => slot.family === 'overlap' && slot.proof === 'history')
        .slice(0, 1);
    await executeFile(directory, 'supplementary', originals);
    await executeFile(
        directory,
        'companions',
        textHistoryRequirements(originals),
    );
    await writeFile(join(directory, 'safety.jsonl'), '');
    await writeFile(join(directory, 'suites.jsonl'), '');
    const files: Record<string, string> = {};
    for (const name of [
        'base.jsonl',
        'continuations.jsonl',
        'supplementary.jsonl',
        'companions.jsonl',
        'safety.jsonl',
        'suites.jsonl',
    ])
        files[name] = createHash('sha256')
            .update(await readFile(join(directory, name)))
            .digest('hex');
    await writeFile(
        join(directory, 'completion.json'),
        JSON.stringify({ files, typecheck: 0 }),
    );
    const result = await reportCheckpoint(directory);
    assert.equal(result.passed, false);
    assert.equal(result.gate.coverageComplete, false);
    assert.equal(result.usableHistories.length, 1);
    assert.equal(
        result.coverage.continuations.filter(
            (slot) => slot.status === 'unexercised',
        ).length,
        4599,
    );
    console.log(
        JSON.stringify({
            smokeDirectory: directory,
            nestedBytes: (
                await readFile(join(directory, 'continuations.jsonl'))
            ).length,
        }),
    );
    const env = { ...process.env };
    delete env['NODE_TEST_CONTEXT'];
    delete env['TABLE_CHECKPOINT_SAFETY'];
    delete env['TABLE_CHECKPOINT_BASE'];
    const projection = spawnSync(
        process.execPath,
        [
            '--import',
            'tsx',
            'run.ts',
            '--suite',
            'projection-properties',
            '--built-peer',
            join(directory, 'provenance.json'),
            '--test-reporter',
            './checkpoint-reporter.ts',
        ],
        { cwd: PACKAGE_DIRECTORY, encoding: 'utf8', env },
    );
    assert.equal(projection.status, 0, projection.stderr);
    const events = projection.stdout
        .trim()
        .split('\n')
        .map((line) => JSON.parse(line));
    events.push({
        type: 'test:fail',
        data: {
            name: 'R-only canary',
            file: `${PACKAGE_DIRECTORY}/cases/convergence.test.ts`,
        },
    });
    await writeFile(
        join(directory, 'suites.jsonl'),
        JSON.stringify({
            suite: 'convergence',
            exitCode: 1,
            files: SUITE_CASES.convergence,
            events,
        }) + '\n',
    );
    files['suites.jsonl'] = createHash('sha256')
        .update(await readFile(join(directory, 'suites.jsonl')))
        .digest('hex');
    await writeFile(
        join(directory, 'completion.json'),
        JSON.stringify({ files, typecheck: 0 }),
    );
    const independent = await reportCheckpoint(directory);
    assert.equal(independent.gate.projectionPassed, true);
    assert.equal(independent.suites.convergence, false);
    assert.equal(independent.gate.coverageComplete, false);
    assert.equal(independent.passed, false);
    await writeFile(join(directory, 'base.jsonl'), '{}\n');
    await assert.rejects(reportCheckpoint(directory), /artifact digest/);
});

test('TBL-24 built peer reuse rejects stale provenance and records real suite events', async () => {
    const directory = await mkdtemp(join(tmpdir(), 'table-checkpoint-peer-'));
    const path = join(directory, 'provenance.json');
    const actual = await provenance();
    await writeFile(
        path,
        JSON.stringify({
            ...actual,
            nativePeer: { ...actual.nativePeer, sha256: 'stale' },
        }),
    );
    const args = [
        '--import',
        'tsx',
        'run.ts',
        '--suite',
        'core-gate',
        '--built-peer',
        path,
        '--test-reporter',
        './checkpoint-reporter.ts',
    ];
    const env = { ...process.env };
    delete env['NODE_TEST_CONTEXT'];
    const stale = spawnSync(process.execPath, args, {
        cwd: PACKAGE_DIRECTORY,
        encoding: 'utf8',
        env,
    });
    assert.notEqual(stale.status, 0);
    assert.match(stale.stderr, /provenance mismatch/);
    await writeFile(path, JSON.stringify(actual));
    const completed = spawnSync(process.execPath, args, {
        cwd: PACKAGE_DIRECTORY,
        encoding: 'utf8',
        env,
    });
    assert.equal(completed.status, 0, completed.stderr);
    const events = completed.stdout
        .trim()
        .split('\n')
        .map((line) => JSON.parse(line));
    assert.ok(events.some((event) => event.type === 'test:pass'));
    assert.equal(events.at(-1).data.success, true);
});
