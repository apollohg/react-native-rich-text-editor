import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import test from 'node:test';
import { checkTablePerformance, loadTablePerformanceConfig } from '../check-table-performance.mjs';

test('the captured Android run includes complete diagnostic and first-frame evidence', () => {
    const input = JSON.parse(
        readFileSync(
            new URL('./fixtures/table-performance-android-sample.json', import.meta.url),
            'utf8'
        )
    );
    const report = checkTablePerformance(input, loadTablePerformanceConfig(), 'diagnostic');
    assert.equal(report.releaseEvidence, false);
    assert.equal(report.devices.length, 1);
    const device = report.devices[0];
    assert.equal(device.platform, 'android');
    assert.equal(device.physicalDevice, false);
    for (const failure of device.failures) {
        assert.match(
            failure,
            /(?: ms exceeds |(?:unmountedCacheBytes|retainedPresentations|unchangedCellRemeasurements|changedCellRemeasurements|nonFiniteLayouts|maxCellInputInstances)=)/
        );
    }
    for (const sample of input.samples.filter(({ metric }) => metric === 'typing')) {
        assert.equal(sample.wrapCount + sample.nonWrapCount, sample.samplesMs.length);
        assert.ok(sample.wrapCount > 0 && sample.nonWrapCount > 0);
        for (const name of [
            'nativeInputAndFFI',
            'nativeFrameAndFFI',
            'adapterAdoption',
            'frameTotal',
        ]) {
            const values = sample.stageSamplesMs[name];
            assert.equal(values.length, sample.samplesMs.length, `${sample.fixture}/${name}`);
            assert.ok(values.every((value) => Number.isFinite(value) && value > 0));
        }
    }
});
