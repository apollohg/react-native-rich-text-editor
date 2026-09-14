import assert from 'node:assert/strict';
import test from 'node:test';
import * as runner from '../continuation-runner.js';
import { continuationRequirements, type ContinuationResult } from '../corpus.js';
import { spawnSync } from 'node:child_process';
import { mkdtempSync, readFileSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';

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
