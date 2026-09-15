import assert from 'node:assert/strict';
import test from 'node:test';
import * as runner from '../continuation-runner.js';
import { continuationRequirements, type ContinuationResult } from '../corpus.js';
import { spawnSync } from 'node:child_process';
import { mkdtempSync, readFileSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { beginTrace, endTrace, recordAction } from '../trace.js';
import { runContinuation, runSchedule, continuationPassed, CORPUS_SCENARIOS } from '../corpus.js';

test('observed gap absence cannot hide an escaped lifecycle failure', async () => {
    const slot = continuationRequirements().find(
        (slot) =>
            slot.topology === 'native/web' &&
            slot.proof === 'web-gap' &&
            slot.schedule.scenario === CORPUS_SCENARIOS[4],
    )!;
    const valid = await runContinuation(slot);
    assert.equal(valid.required, false);
    assert.equal(continuationPassed(valid), true);
    let written: ContinuationResult | undefined;
    const summary = await runner.executeContinuations(
        [slot],
        (candidate) =>
            runContinuation(candidate, {
                setup: async (candidate, body) => {
                    await runSchedule(candidate.schedule, body);
                    throw new Error('lifecycle failure after observed gap absence');
                },
            }),
        async (result) => {
            written = result;
        },
    );
    assert.equal(written!.required, false, 'independently observed absence remains known');
    assert.equal(written!.disposition, 'no-gap');
    assert.deepEqual(written!.gap?.positions, []);
    assert.ok(written!.checkpoints.length > 0);
    assert.ok(written!.tracePath);
    assert.equal(continuationPassed(written!), false);
    assert.equal(summary.passed, 0);
});

test('escaped lifecycle failure retains already captured continuation evidence', async () => {
    const slot = continuationRequirements().find(
        (slot) => slot.topology === 'native/native' && slot.proof === 'typing',
    )!;
    let written: ContinuationResult | undefined;
    await runner.executeContinuations(
        [slot],
        (candidate) =>
            runContinuation(candidate, {
                setup: async (candidate, body) => {
                    await runSchedule(candidate.schedule, body, { webReference: true });
                    throw new Error('failure after captured continuation');
                },
            }),
        async (result) => {
            written = result;
        },
    );
    assert.ok(written!.actions.length > 0, 'recorded action is retained');
    assert.ok(written!.checkpoints.length >= 2, 'recorded checkpoints are retained');
    assert.ok(written!.baseline, 'baseline is retained');
    assert.equal(written!.status, 'exercised-unproven');
    assert.match(written!.failures.join(), /failure after captured continuation/);
    assert.ok(written!.tracePath);
});

test('escaped execution uses only its own completed lifecycle trace', async () => {
    const slots = continuationRequirements().slice(0, 3);
    const written: ContinuationResult[] = [];
    await runner.executeContinuations(
        slots,
        async (slot) => {
            if (slot === slots[1]) throw new Error('before current lifecycle');
            const previous = beginTrace(
                { kinds: ['rust', 'rust'], config: {}, seed: slot.schedule.seed },
                { node: process.version, packages: {}, rustPeerExecutable: 'test-only' },
            );
            recordAction(0, 'snapshot', { key: slot.key });
            endTrace(previous, null);
            throw new Error(`inside lifecycle ${slot.key}`);
        },
        async (result) => {
            written.push(result);
        },
    );
    for (const index of [0, 2]) {
        assert.ok(written[index]!.tracePath, 'associated current trace retained');
        const trace = JSON.parse(readFileSync(written[index]!.tracePath!, 'utf8'));
        assert.equal(trace.records[0].payload.key, slots[index]!.key);
        assert.equal(JSON.parse(trace.failureMessage).key, slots[index]!.key);
    }
    assert.equal(written[1]!.tracePath, undefined, 'no stale trace before lifecycle starts');
});

test('scoped runner declares all186 supplementary keys separately', () => {
    const result = spawnSync(
        process.execPath,
        [
            '--import',
            './node_modules/tsx/dist/loader.mjs',
            'run-continuations.ts',
            '--supplementary',
            '--manifest',
        ],
        { encoding: 'utf8' },
    );
    assert.equal(result.status, 0, result.stderr);
    const slots = JSON.parse(result.stdout);
    assert.equal(slots.length, 186);
    assert.equal(new Set(slots.map((slot: { key: string }) => slot.key)).size, 186);
});

test('scoped runner persists a declared subset and refuses to overwrite prior evidence', () => {
    const directory = mkdtempSync(join(tmpdir(), 'continuation-runner-'));
    const keys = join(directory, 'keys.json');
    const output = join(directory, 'results.jsonl');
    writeFileSync(keys, '[]');
    const args = [
        '--import',
        './node_modules/tsx/dist/loader.mjs',
        'run-continuations.ts',
        '--output',
        output,
        '--keys',
        keys,
    ];
    const result = spawnSync(process.execPath, args, { encoding: 'utf8' });
    assert.equal(result.status, 0, result.stderr);
    const summary = JSON.parse(readFileSync(`${output}.summary.json`, 'utf8'));
    assert.equal(summary.fullCandidates, 4600);
    assert.equal(summary.declared, 0);
    assert.equal(summary.executed, 0);
    assert.equal(summary.passed, 0);
    const provenance = JSON.parse(readFileSync(`${output}.provenance.json`, 'utf8'));
    assert.match(provenance.sources['corpus.ts'], /^[a-f0-9]{64}$/);
    assert.match(provenance.nativePeer.sha256, /^[a-f0-9]{64}$/);
    assert.equal(readFileSync(output, 'utf8'), '');
    const again = spawnSync(process.execPath, args, { encoding: 'utf8' });
    assert.notEqual(again.status, 0);
    assert.match(again.stderr, /EEXIST/);
});

test('scoped runner lists all4600 candidate keys without launching peer groups', () => {
    const result = spawnSync(
        process.execPath,
        ['--import', './node_modules/tsx/dist/loader.mjs', 'run-continuations.ts', '--manifest'],
        { encoding: 'utf8', maxBuffer: 8 * 1024 * 1024 },
    );
    assert.equal(result.status, 0, result.stderr);
    const manifest = JSON.parse(result.stdout);
    assert.equal(manifest.length, 4600);
    assert.deepEqual(
        manifest.map((slot: { key: string }) => slot.key),
        continuationRequirements().map((slot) => slot.key),
    );
});

test('serial runner retains every declared slot after an execution failure', async () => {
    const slots = continuationRequirements().slice(0, 3);
    let active = false;
    const events: string[] = [];
    const written: ContinuationResult[] = [];
    const summary = await runner.executeContinuations(
        slots,
        async (slot) => {
            assert.equal(active, false);
            active = true;
            events.push(slot.key);
            await Promise.resolve();
            active = false;
            if (slot === slots[1]) throw new Error('retained runner failure');
            return {
                slot,
                required: slot.required,
                status: 'unexercised',
                disposition: 'unreached',
                checkpoints: [],
                actions: [],
                failures: [],
                dependencies: [],
            };
        },
        async (result) => {
            written.push(result);
        },
    );
    assert.deepEqual(
        events,
        slots.map((slot) => slot.key),
    );
    assert.equal(written.length, 3);
    assert.match(written[1]!.failures.join(), /retained runner failure/);
    assert.equal(summary.declared, 3);
    assert.equal(summary.executed, 3);
    assert.equal(summary.passed, 0);
    assert.equal(summary.coverage.filter((slot) => slot.status === 'unexercised').length, 3);
});

test('serial coverage refuses duplicate declared keys before any execution', async () => {
    const slot = continuationRequirements()[0]!;
    let calls = 0;
    await assert.rejects(
        () =>
            runner.executeContinuations(
                [slot, slot],
                async () => {
                    calls += 1;
                    throw new Error('must not execute');
                },
                async () => {},
            ),
        /duplicate/,
    );
    assert.equal(calls, 0);
});
