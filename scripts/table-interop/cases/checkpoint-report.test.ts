import assert from 'node:assert/strict';
import test from 'node:test';
import * as report from '../convergence-report.js';
import {
    CONVERGENCE_CORPUS,
    continuationRequirements,
    runContinuation,
    runSchedule,
    type ContinuationCheckpoint,
} from '../corpus.js';
import { checkpointProof } from '../checkpoint-evidence.js';
import {
    runSupplementary,
    supplementaryRequirements,
} from '../supplementary-continuity.js';
import { textHistoryRequirements } from '../corpus.js';

test('TBL-24 aggregate retains unavailable history and separately requires its exact companion', async () => {
    const { CheckpointReport } = await import('../checkpoint-report.js');
    const originalSlot = supplementaryRequirements().find(
        (slot) => slot.family === 'overlap' && slot.proof === 'history',
    )!;
    const original = await runSupplementary(originalSlot);
    const companion = await runSupplementary(
        textHistoryRequirements([originalSlot])[0]!,
    );
    const report = new CheckpointReport();
    report.addContinuation(original);
    assert.equal(report.finish().usableHistories.length, 0);
    report.addCompanion(original, companion);
    assert.deepEqual(report.finish().usableHistories, [originalSlot.key]);
    assert.equal(
        report
            .finish()
            .coverage.supplementary.find((row) => row.key === originalSlot.key)!
            .status,
        'exercised-unproven',
    );
    assert.throws(() => report.addCompanion(original, companion), /duplicate/);
    assert.throws(() => report.addContinuation(original), /duplicate/);
    const changed = JSON.parse(JSON.stringify(original));
    changed.failures.push('unobserved original');
    const mismatch = new CheckpointReport();
    mismatch.addContinuation(original);
    assert.throws(
        () => mismatch.addCompanion(changed, companion),
        /original result/,
    );
    const wrongSlot = JSON.parse(JSON.stringify(original));
    wrongSlot.slot = { ...wrongSlot.slot, actor: 99 };
    assert.throws(
        () => new CheckpointReport().addContinuation(wrongSlot),
        /declaration/,
    );
});

test('TBL-24 coverage rejects missing duplicate and mismatched base declarations', async () => {
    const path = '../checkpoint-report.js';
    const api = await import(path).catch(() => undefined);
    assert.equal(
        typeof api?.CheckpointReport,
        'function',
        'real aggregate required',
    );
    const report = new api.CheckpointReport();
    assert.equal(report.finish().gate.coverageComplete, false);
    assert.equal(report.finish().coverage.base.length, 926);
    const result = await (
        await import('../checkpoint-evidence.js')
    ).runBaseSchedule(CONVERGENCE_CORPUS[0]!);
    report.addBase(result);
    assert.throws(() => report.addBase(result), /duplicate/);
    const invalid = structuredClone(result);
    invalid.schedule = { ...invalid.schedule, seed: invalid.schedule.seed + 1 };
    assert.throws(
        () => new api.CheckpointReport().addBase(invalid),
        /declaration/,
    );
    const raw = structuredClone(result);
    raw.outcome.rawConvergence.passed = false;
    const changed = new api.CheckpointReport();
    changed.addBase(raw);
    assert.ok(
        changed
            .finish()
            .findings.some((finding: { axis: string }) => finding.axis === 'R'),
    );
    assert.equal(changed.finish().gate.rawConvergencePassed, false);
    assert.equal(
        changed
            .finish()
            .findings.some(
                (finding: { axis: string }) =>
                    finding.axis === 'scenario-effect',
            ),
        false,
    );
    const badGeometry = structuredClone(result);
    badGeometry.outcome = {
        ...badGeometry.outcome,
        geometry: {
            kind: 'projectionFailed',
            code: 'canary',
            message: 'geometry only',
        },
    };
    const projected = new api.CheckpointReport();
    projected.addBase(badGeometry);
    assert.equal(
        projected.finish({
            plumbingPassed: true,
            differentialPassed: true,
            projectionPassed: true,
            safetyComplete: true,
            unsafeAdmissions: 0,
            unexpectedSourceCellLosses: 0,
        }).gate.projectionPassed,
        false,
    );
    const unsafeDrain = structuredClone(result);
    unsafeDrain.checkpoint!.drain.passed = false;
    unsafeDrain.checkpoint!.drain.failure = 'TBL-21 NON_QUIESCENT';
    const drained = new api.CheckpointReport();
    drained.addBase(unsafeDrain);
    const drainGate = drained.finish({
        plumbingPassed: true,
        differentialPassed: true,
        projectionPassed: true,
        safetyComplete: true,
        unsafeAdmissions: 0,
        unexpectedSourceCellLosses: 0,
    }).gate;
    assert.equal(drainGate.projectionPassed, true);
    assert.equal(drainGate.nonQuiescentRuns, 1);
    const effects = structuredClone(result);
    effects.outcome.evidence.status = 'exercised-unproven';
    const unproven = new api.CheckpointReport();
    unproven.addBase(effects);
    assert.ok(
        unproven
            .finish()
            .coverage.base.some(
                (row: { status: string }) =>
                    row.status === 'exercised-unproven',
            ),
    );
});

test('TBL-24 normalization lifetime retains replayable checkpoint observations', async () => {
    const slot = supplementaryRequirements().find(
        (candidate) => candidate.family === 'native-owned-normalization',
    )!;
    const result = await runSupplementary(slot);
    assert.deepEqual(result.failures, []);
    for (const checkpoint of result.checkpoints)
        assert.equal(
            checkpointProof(checkpoint).passed,
            true,
            JSON.stringify(checkpointProof(checkpoint)),
        );
});

test('TBL-24 ordinary typing retains replayable checkpoint observations', async () => {
    const slot = continuationRequirements().find(
        (candidate) => candidate.proof === 'typing',
    )!;
    const result = await runContinuation(slot);
    assert.deepEqual(result.failures, []);
    for (const checkpoint of result.checkpoints)
        assert.equal(
            checkpointProof(checkpoint).passed,
            true,
            JSON.stringify(checkpointProof(checkpoint)),
        );
    const { CheckpointReport } = await import('../checkpoint-report.js');
    const invalid = JSON.parse(JSON.stringify(result));
    invalid.checkpoints[0].raw.passed = false;
    const report = new CheckpointReport();
    report.addContinuation(invalid);
    assert.equal(
        report
            .finish()
            .coverage.continuations.find((row) => row.key === slot.key)!.status,
        'exercised-unproven',
    );
});

test('TBL-24 base report captures live presentation and rejects absent or corrupted proof', async () => {
    const path = '../checkpoint-evidence.js';
    const api = await import(path).catch(() => undefined);
    assert.equal(
        typeof api?.runBaseSchedule,
        'function',
        'live base capture required',
    );
    let observed = 0;
    const result = await api.runBaseSchedule(
        CONVERGENCE_CORPUS[0]!,
        () => observed++,
    );
    assert.equal(
        observed,
        1,
        'existing live settled observer remains connected',
    );
    assert.equal(api.baseProof(result).passed, true);
    assert.ok(result.checkpoint.presentation.views.web.length);
    assert.equal('peers' in result.outcome, false);
    assert.equal(typeof result.checkpoint.displayRawDifferences, 'number');
    assert.equal(
        api.baseProof({ ...result, checkpoint: undefined }).passed,
        false,
    );
    const raw = structuredClone(result);
    raw.outcome.rawConvergence.passed = false;
    assert.equal(api.baseProof(raw).raw, false);
    assert.equal(api.baseProof(raw).presentation, true);
    assert.equal(api.baseProof(raw).effects, true);
    const evidence = structuredClone(result);
    evidence.outcome.evidence.status = 'exercised-unproven';
    assert.equal(api.baseProof(evidence).passed, false);
    assert.equal(api.baseProof(evidence).raw, true);
    assert.equal(api.baseProof(evidence).effects, false);
    const geometry = structuredClone(result);
    geometry.outcome.geometry = {
        kind: 'projectionFailed',
        code: 'canary',
        message: 'geometry only',
    };
    assert.equal(api.baseProof(geometry).effects, true);
    assert.equal(api.baseProof(geometry).passed, false);
    const repair = structuredClone(result);
    repair.outcome.nativeAutonomousRepairWrites = 1;
    assert.equal(api.baseProof(repair).effects, true);
    assert.equal(api.baseProof(repair).passed, false);
    const presentation = structuredClone(result);
    const checkpoint: ContinuationCheckpoint = presentation.checkpoint;
    checkpoint.presentation.views!.web[0]!.tables[0]!.cells[0]!.node.attrs = {
        corrupted: 'opaque',
    };
    assert.equal(api.baseProof(presentation).presentation, false);
    const missingParticipant = structuredClone(result);
    missingParticipant.checkpoint.textHistoryObservations.pop();
    assert.equal(api.baseProof(missingParticipant).raw, false);
    const detached = structuredClone(result);
    detached.checkpoint.observations[0].document.tables[0].cells[0].node.attrs =
        { detached: true };
    assert.equal(api.baseProof(detached).presentation, false);
});

test('TBL-24 report caller independently rejects raw and scenario evidence failures', async () => {
    const check = (
        report as unknown as {
            scheduleProofPassed?: (value: unknown) => boolean;
        }
    ).scheduleProofPassed;
    assert.equal(
        typeof check,
        'function',
        'the caller must consume both observed axes',
    );
    const outcome = await runSchedule(CONVERGENCE_CORPUS[0]!);
    assert.equal(check!(outcome), true);
    assert.equal(
        check!({
            ...outcome,
            rawConvergence: { passed: false, failure: 'raw canary' },
        }),
        false,
    );
    assert.equal(
        check!({
            ...outcome,
            evidence: { ...outcome.evidence, status: 'exercised-unproven' },
        }),
        false,
    );
    assert.equal(
        check!({
            ...outcome,
            evidence: { ...outcome.evidence, failures: ['effect canary'] },
        }),
        false,
    );
});
