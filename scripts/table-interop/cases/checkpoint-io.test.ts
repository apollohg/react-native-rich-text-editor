import assert from 'node:assert/strict';
import test from 'node:test';

test('TBL-24 saved provenance rejects stale source and native hashes', async () => {
    const path = '../checkpoint-io.js';
    const api = await import(path).catch(() => undefined);
    assert.equal(
        typeof api?.provenance,
        'function',
        'source and binary provenance required',
    );
    const actual = await api.provenance();
    await api.verifyProvenance(actual);
    assert.ok(actual.sources['cases/checkpoint-io.test.ts']);
    assert.ok(actual.sources['../../rust/editor-core/src/lib.rs']);
    await assert.rejects(
        api.verifyProvenance({
            ...actual,
            nativePeer: { ...actual.nativePeer, sha256: 'stale' },
        }),
    );
    const stale = structuredClone(actual);
    stale.sources['cases/checkpoint-io.test.ts'] = 'stale';
    await assert.rejects(api.verifyProvenance(stale));
});

test('TBL-24 safety requires actual complete admission and source observations', async () => {
    const path = '../checkpoint-io.js';
    const api = await import(path).catch(() => undefined);
    assert.equal(
        typeof api?.safetyResult,
        'function',
        'required safety inventory',
    );
    assert.equal(api.safetyResult([]).complete, false);
    const observations = [
        ...api.ADMISSION_CASES.map((name: string, index: number) => ({
            kind: 'admission',
            name,
            classification: index < 8 ? 'unsafe' : 'admissibleIrregular',
            admitted: index >= 8,
            detail: 'captured',
        })),
        ...api.SOURCE_CASES.map((name: string) => ({
            kind: 'source-coverage',
            name,
            sourceCellAnchors: [2, 7],
            projectedSlots: [2, 7],
        })),
    ];
    assert.equal(api.safetyResult(observations).complete, true);
    assert.equal(api.safetyResult(observations.slice(1)).complete, false);
    assert.equal(
        api.safetyResult([...observations, observations[0]]).complete,
        false,
    );
    const unsafe = structuredClone(observations);
    unsafe[0].admitted = true;
    assert.equal(api.safetyResult(unsafe).unsafeAdmissions, 1);
    const lost = structuredClone(observations);
    lost.at(-1).projectedSlots = [2];
    assert.equal(api.safetyResult(lost).unexpectedSourceCellLosses, 1);
});

test('TBL-24 suites require complete unfiltered file execution and zero skipped cases', async () => {
    const path = '../checkpoint-io.js';
    const api = await import(path).catch(() => undefined);
    assert.equal(
        typeof api?.suitePassed,
        'function',
        'suite evidence required',
    );
    const files = ['plumbing', 'scheduler', 'dependencies'];
    const events = files.map((name) => ({
        type: 'test:pass',
        data: { name: 'actual test', file: `/repo/cases/${name}.test.ts` },
    }));
    events.push({
        type: 'test:summary',
        data: {
            success: true,
            counts: {
                tests: 3,
                passed: 3,
                failed: 0,
                cancelled: 0,
                skipped: 0,
                todo: 0,
            },
        },
    } as never);
    assert.equal(
        api.suitePassed({ suite: 'plumbing', exitCode: 0, files, events }),
        true,
    );
    assert.equal(
        api.suitePassed({ suite: 'plumbing', exitCode: 0, files, events: [] }),
        false,
    );
    assert.equal(
        api.suitePassed({ suite: 'plumbing', exitCode: 1, files, events }),
        false,
    );
    assert.equal(
        api.suitePassed({
            suite: 'plumbing',
            exitCode: 0,
            files,
            events: events.slice(1),
        }),
        false,
    );
    const skipped = structuredClone(events);
    skipped[0]!.data = { ...skipped[0]!.data, skip: true } as never;
    assert.equal(
        api.suitePassed({
            suite: 'plumbing',
            exitCode: 0,
            files,
            events: skipped,
        }),
        false,
    );
});
