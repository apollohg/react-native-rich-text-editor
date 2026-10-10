import assert from 'node:assert/strict';
import test from 'node:test';
import * as reports from '../convergence-report.js';

const passing = {
    plumbingPassed: true,
    differentialPassed: true,
    projectionPassed: true,
    rawConvergencePassed: true,
    presentationPassed: true,
    continuityPassed: true,
    coverageComplete: true,
    nativeAutonomousRepairWrites: 0,
    unsafeAdmissions: 0,
    nonQuiescentRuns: 0,
    unexpectedSourceCellLosses: 0,
};

test('TBL-24 aggregate rejects each independently missing proof and safety violation', () => {
    const gate = (
        reports as unknown as { coreGatePassed?: (value: unknown) => boolean }
    ).coreGatePassed;
    assert.equal(typeof gate, 'function', 'the executable gate is required');
    assert.equal(gate!(passing), true);
    for (const [key, value] of Object.entries(passing)) {
        assert.equal(
            gate!({
                ...passing,
                [key]: typeof value === 'boolean' ? false : 1,
            }),
            false,
            key,
        );
        const missing: Record<string, unknown> = { ...passing };
        delete missing[key];
        assert.equal(Boolean(gate!(missing)), false, `missing ${key}`);
    }
    for (const key of Object.keys(passing).filter(
        (key) => typeof passing[key as keyof typeof passing] === 'number',
    ))
        for (const value of [-1, NaN, Infinity])
            assert.equal(
                gate!({ ...passing, [key]: value }),
                false,
                `${key}: ${value}`,
            );
});
