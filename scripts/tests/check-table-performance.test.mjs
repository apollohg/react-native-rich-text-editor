import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import path from 'node:path';
import test from 'node:test';
import { fileURLToPath } from 'node:url';

import {
    checkTablePerformance,
    loadTablePerformanceConfig,
    maxRetainedPresentations,
    percentile,
} from '../check-table-performance.mjs';

const repositoryRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '../..');
const checker = path.join(repositoryRoot, 'scripts/check-table-performance.mjs');
const config = loadTablePerformanceConfig();
const PLAIN_FIXTURES = ['plain-3x3', 'plain-1000x20', 'plain-100x200'];
const COLD_METRICS = ['viewerColdLayout', 'editorColdLayout'];
const RICH_FIXTURES = ['rich-merged-3x3', 'rich-merged-1000x20', 'rich-merged-100x200'];
const UNMOUNTED_CACHE_BUDGET_BYTES = 32 * 1024 * 1024;
const FAST_MS = 0.5;
const SLOW_TAIL_MS = 1000;
const FRAMES_PER_AXIS = 1800;
const SCROLL_FRAME_BUDGET_MS = 16.67;
const DROPPED_FRAME_MS = 33.34;
const SHORT_TRAVERSAL_FRAMES = 1700;
const OVER_LIMIT_DELTA_MS = 0.01;
const EDITS_PER_UNSIZED_CASE = 4;
const TYPICAL_RETAINED_PRESENTATIONS = 12;
const DERIVED_MAX_RETAINED_PRESENTATIONS = 2272;

const iphone13 = {
    platform: 'ios',
    device: 'iPhone14,5',
    os: 'iOS 26.0 (23A341)',
    physicalDevice: true,
    buildType: 'release',
    refreshHz: 60,
    textScale: 1,
    viewportWidth: 390,
    viewportHeight: 844,
    overscanViewports: 1,
};
const pixel7 = {
    platform: 'android',
    device: 'Pixel 7',
    os: 'Android 16 (BP2A.250605.031)',
    physicalDevice: true,
    buildType: 'release',
    refreshHz: 60,
    textScale: 1,
    viewportWidth: 390,
    viewportHeight: 844,
    overscanViewports: 1,
};

function counters(overrides = {}) {
    return {
        maxCellInputInstances: 1,
        nonFiniteLayouts: 0,
        unchangedCellRemeasurements: 0,
        unmountedCacheBytes: 1024,
        pinnedLayoutBytes: 2048,
        authoritativeDocumentBytes: 4096,
        retainedPresentations: TYPICAL_RETAINED_PRESENTATIONS,
        ...overrides,
    };
}

function repeat(count, value) {
    return Array.from({ length: count }, () => value);
}

function samplesAtPercentileBoundaries(count, percentiles, tailMs = SLOW_TAIL_MS) {
    const samples = [];
    for (const { fraction, maxMs } of percentiles) {
        samples.push(...repeat(Math.ceil(fraction * count) - samples.length, maxMs));
    }
    samples.push(...repeat(count - samples.length, tailMs));
    return samples;
}

function scrollSample(fixture, metric) {
    const samplesMs = samplesAtPercentileBoundaries(
        FRAMES_PER_AXIS,
        [{ fraction: 0.99, maxMs: SCROLL_FRAME_BUDGET_MS }],
        DROPPED_FRAME_MS
    );
    return {
        fixture,
        metric,
        run: 1,
        samplesMs,
        tableAttributed: repeat(samplesMs.length, true),
    };
}

function hardCaseSamples(gate, fixture, run) {
    if (gate.minTraversalMs !== undefined) {
        return scrollSample(fixture, gate.metric);
    }
    const count = gate.samplesPerRun ?? EDITS_PER_UNSIZED_CASE;
    const perEditCounters = Object.fromEntries(
        Object.entries(gate.countersPerSample ?? {}).map(([name, perEdit]) => [
            name,
            perEdit * count,
        ])
    );
    return {
        fixture,
        metric: gate.metric,
        run,
        ...(gate.warmupSamples === undefined ? {} : { warmupSamplesDiscarded: gate.warmupSamples }),
        counters: counters(perEditCounters),
        samplesMs:
            gate.percentiles.length === 0
                ? repeat(count, FAST_MS)
                : samplesAtPercentileBoundaries(count, gate.percentiles),
    };
}

function completeExport(device = iphone13) {
    const samples = [];
    for (const gate of config.hardGates) {
        for (const fixture of gate.fixtures) {
            for (let run = 1; run <= gate.runs; run += 1) {
                samples.push(hardCaseSamples(gate, fixture, run));
            }
        }
    }
    for (const { metric, fixtures } of config.baselineMetrics) {
        for (const fixture of fixtures) {
            samples.push({ fixture, metric, run: 1, samplesMs: [SLOW_TAIL_MS] });
        }
    }
    return {
        samples: samples.map((sample) => ({
            ...device,
            counters: counters(),
            ...sample,
        })),
    };
}

function findSample(input, metric, fixture, run = 1) {
    const sample = input.samples.find(
        (candidate) =>
            candidate.metric === metric && candidate.fixture === fixture && candidate.run === run
    );
    assert.ok(sample, `fixture export has ${metric}/${fixture} run ${run}`);
    return sample;
}

function raiseOneBoundarySample(sample, boundaryMs) {
    const index = sample.samplesMs.indexOf(boundaryMs);
    assert.notEqual(index, -1, `sample has a value at the ${boundaryMs} ms boundary`);
    sample.samplesMs[index] = boundaryMs + OVER_LIMIT_DELTA_MS;
}

function check(input, mode = 'release') {
    return checkTablePerformance(input, config, mode);
}

function allFailures(report) {
    return report.devices.flatMap(({ failures }) => failures);
}

function assertOnlyFailure(report, pattern) {
    const failures = allFailures(report);
    assert.equal(report.passed, false, 'the report fails');
    assert.equal(failures.length, 1, `exactly one failure:\n${failures.join('\n')}`);
    assert.match(failures[0], pattern);
}

function runWithInputFile(command, args, input) {
    const directory = mkdtempSync(path.join(tmpdir(), 'table-performance-check-'));
    const inputPath = path.join(directory, 'export.json');
    try {
        if (input !== undefined) {
            writeFileSync(inputPath, typeof input === 'string' ? input : JSON.stringify(input));
        }
        return spawnSync(
            command,
            args.map((argument) => (argument === '<input>' ? inputPath : argument)),
            { cwd: repositoryRoot, encoding: 'utf8', maxBuffer: 64 * 1024 * 1024 }
        );
    } finally {
        rmSync(directory, { recursive: true, force: true });
    }
}

function runChecker(args, input) {
    return runWithInputFile(process.execPath, [checker, ...args], input);
}

test('percentile uses nearest rank without interpolation', () => {
    const samples = Array.from({ length: 100 }, (_, index) => 100 - index);

    assert.equal(percentile(samples, 0.95), 95);
    assert.equal(percentile(samples, 0.99), 99);
    assert.equal(percentile(samples, 1), 100);
    assert.equal(percentile([3, 1, 2], 0.5), 2);
    assert.equal(percentile([10, 20], 0.95), 20);
    assert.equal(percentile([10, 20], 0.5), 10);
    assert.deepEqual(samples.slice(0, 2), [100, 99], 'the input order is untouched');
});

test('percentile rejects every value outside its finite domain', async (t) => {
    for (const [name, samples, fraction] of [
        ['empty samples', [], 0.95],
        ['NaN sample', [1, Number.NaN], 0.95],
        ['infinite sample', [1, Number.POSITIVE_INFINITY], 0.95],
        ['negative sample', [1, -0.001], 0.95],
        ['zero fraction', [1], 0],
        ['fraction above one', [1], 1.01],
        ['NaN fraction', [1], Number.NaN],
    ]) {
        await t.test(name, () => {
            assert.throws(() => percentile(samples, fraction), /Invalid performance samples/);
        });
    }
});

test('the configuration carries the TBL-23 sampling protocol and budgets exactly', () => {
    const gate = (metric, fixture) =>
        config.hardGates.find(
            (candidate) => candidate.metric === metric && candidate.fixtures.includes(fixture)
        );

    for (const fixture of PLAIN_FIXTURES) {
        assert.equal(gate('typing', fixture).runs, 5);
        assert.equal(gate('typing', fixture).samplesPerRun, 500);
        assert.equal(gate('typing', fixture).warmupSamples, 20);
        assert.deepEqual(gate('typing', fixture).percentiles, [
            { fraction: 0.95, maxMs: 32 },
            { fraction: 0.99, maxMs: 50 },
        ]);
        for (const metric of COLD_METRICS) assert.equal(gate(metric, fixture).samplesPerRun, 30);
        assert.equal(gate('warmMeasurement', fixture).samplesPerRun, 1000);
        assert.deepEqual(gate('warmMeasurement', fixture).percentiles, [
            { fraction: 0.99, maxMs: 1 },
        ]);
        for (const axis of ['scrollHorizontal', 'scrollVertical']) {
            assert.equal(gate(axis, fixture).minTraversalMs, 30000);
            assert.equal(gate(axis, fixture).maxTableAttributedPauseMs, 50);
            assert.deepEqual(gate(axis, fixture).percentiles, [{ fraction: 0.99, maxMs: 16.67 }]);
        }
    }
    for (const metric of COLD_METRICS) {
        assert.deepEqual(gate(metric, 'plain-3x3').percentiles, [{ fraction: 0.95, maxMs: 16 }]);
        for (const fixture of ['plain-1000x20', 'plain-100x200']) {
            assert.deepEqual(gate(metric, fixture).percentiles, [{ fraction: 0.95, maxMs: 500 }]);
        }
    }
    for (const fixture of [...PLAIN_FIXTURES, ...RICH_FIXTURES]) {
        for (const metric of ['cellChangeStart', 'cellChangeEnd']) {
            assert.deepEqual(gate(metric, fixture).countersPerSample, {
                changedCellRemeasurements: 1,
            });
        }
    }
    assert.equal(config.counterGates.unmountedCacheBytes, UNMOUNTED_CACHE_BUDGET_BYTES);
    assert.equal(config.counterGates.maxCellInputInstances, 1);
    assert.deepEqual(config.releaseEnvironment, {
        devices: [
            { platform: 'ios', device: 'iPhone14,5' },
            { platform: 'android', device: 'Pixel 7' },
        ],
        physicalDevice: true,
        buildType: 'release',
        refreshHz: 60,
        textScale: 1,
        viewportWidth: 390,
        viewportHeight: 844,
        overscanViewports: 1,
    });
    assert.deepEqual(config.defaultTableTheme, {
        minColumnWidth: 80,
        cellPadding: 8,
        borderWidth: 1,
    });
});

test('the retained presentation ceiling derives from the release viewport, overscan and minimum cell', () => {
    const windowWidth = 390 * 3;
    const windowHeight = 844 * 3;
    const minRowHeight = 2 * (8 + 1);
    const columns = Math.ceil(windowWidth / 80) + 1;
    const rows = Math.ceil(windowHeight / minRowHeight) + 1;

    assert.equal(columns * rows, DERIVED_MAX_RETAINED_PRESENTATIONS);
    assert.equal(maxRetainedPresentations(config), DERIVED_MAX_RETAINED_PRESENTATIONS);
    assert.equal(
        maxRetainedPresentations({
            ...config,
            releaseEnvironment: {
                ...config.releaseEnvironment,
                overscanViewports: 0,
            },
        }),
        (Math.ceil(390 / 80) + 1) * (Math.ceil(844 / minRowHeight) + 1),
        'overscan widens the window on both sides of each axis'
    );
});

test('complete eligible evidence sitting exactly on every hard boundary passes release', () => {
    const input = completeExport();
    const report = check(input);

    assert.deepEqual(allFailures(report), []);
    assert.equal(report.passed, true);
    assert.equal(report.releaseEvidence, true);
    assert.equal(report.devices.length, 1);
    assert.equal(report.devices[0].releaseEligible, true);

    const measured = (metric, fixture, run = 1) =>
        report.devices[0].hardGates.find(
            (entry) => entry.metric === metric && entry.fixture === fixture && entry.run === run
        ).percentilesMs;
    assert.deepEqual(measured('typing', 'plain-100x200', 5), {
        p95: 32,
        p99: 50,
    });
    for (const metric of COLD_METRICS) {
        for (const fixture of PLAIN_FIXTURES) {
            assert.deepEqual(measured(metric, fixture), {
                p95: fixture === 'plain-3x3' ? 16 : 500,
            });
        }
    }
    assert.deepEqual(measured('warmMeasurement', 'plain-3x3'), { p99: 1 });
    assert.deepEqual(measured('scrollVertical', 'plain-100x200'), { p99: 16.67 });
    assert.ok(
        findSample(input, 'typing', 'plain-3x3').samplesMs.includes(SLOW_TAIL_MS),
        'the boundary fixture keeps a slow tail beyond p99 so nearest rank is what passes it'
    );
});

test('one sample over each hard boundary fails exactly that case', async (t) => {
    for (const [metric, fixture, run, boundaryMs, percentileName] of [
        ['typing', 'plain-1000x20', 3, 32, 'p95'],
        ['typing', 'plain-1000x20', 3, 50, 'p99'],
        ['viewerColdLayout', 'plain-3x3', 1, 16, 'p95'],
        ['viewerColdLayout', 'plain-100x200', 1, 500, 'p95'],
        ['editorColdLayout', 'plain-3x3', 1, 16, 'p95'],
        ['editorColdLayout', 'plain-100x200', 1, 500, 'p95'],
        ['warmMeasurement', 'plain-100x200', 1, 1, 'p99'],
        ['scrollHorizontal', 'plain-1000x20', 1, SCROLL_FRAME_BUDGET_MS, 'p99'],
    ]) {
        await t.test(`${metric}/${fixture} ${percentileName} ${boundaryMs} ms`, () => {
            const input = completeExport();
            raiseOneBoundarySample(findSample(input, metric, fixture, run), boundaryMs);

            assertOnlyFailure(
                check(input),
                new RegExp(
                    `^${metric}/${fixture} run ${run}: ${percentileName}=${
                        boundaryMs + OVER_LIMIT_DELTA_MS
                    } ms exceeds ${boundaryMs} ms$`
                )
            );
        });
    }
});

test('a table-attributed pause is gated at 50 ms while unattributed frames are not', () => {
    const atBoundary = completeExport();
    const boundarySample = findSample(atBoundary, 'scrollVertical', 'plain-3x3');
    boundarySample.samplesMs[boundarySample.samplesMs.length - 1] = 50;
    boundarySample.samplesMs[boundarySample.samplesMs.length - 2] = SLOW_TAIL_MS;
    boundarySample.tableAttributed[boundarySample.samplesMs.length - 2] = false;
    assert.deepEqual(allFailures(check(atBoundary)), []);

    const overBoundary = completeExport();
    const overSample = findSample(overBoundary, 'scrollVertical', 'plain-3x3');
    overSample.samplesMs[overSample.samplesMs.length - 1] = 50 + OVER_LIMIT_DELTA_MS;
    assertOnlyFailure(
        check(overBoundary),
        /^scrollVertical\/plain-3x3 run 1: table-attributed pause of 50\.01 ms exceeds 50 ms$/
    );

    const unmarked = completeExport();
    delete findSample(unmarked, 'scrollHorizontal', 'plain-3x3').tableAttributed;
    assertOnlyFailure(check(unmarked), /tableAttributed must mark every frame sample/);
});

test('scroll traversal shorter than 30 seconds per axis fails', () => {
    const input = completeExport();
    const sample = findSample(input, 'scrollHorizontal', 'plain-100x200');
    sample.samplesMs = sample.samplesMs.slice(0, SHORT_TRAVERSAL_FRAMES);
    sample.tableAttributed = sample.tableAttributed.slice(0, SHORT_TRAVERSAL_FRAMES);

    assertOnlyFailure(
        check(input),
        /^scrollHorizontal\/plain-100x200 run 1: traversed \d+(\.\d+)? ms, protocol requires 30000 ms$/
    );
});

test('each run is evaluated alone so one slow run cannot hide in the pooled samples', () => {
    const input = completeExport();
    for (let run = 1; run <= 5; run += 1) {
        findSample(input, 'typing', 'plain-100x200', run).samplesMs = repeat(500, FAST_MS);
    }
    const slowRun = findSample(input, 'typing', 'plain-100x200', 4);
    slowRun.samplesMs = [...repeat(470, FAST_MS), ...repeat(30, 40)];
    const pooled = Array.from(
        { length: 5 },
        (_, index) => findSample(input, 'typing', 'plain-100x200', index + 1).samplesMs
    ).flat();
    assert.ok(percentile(pooled, 0.95) <= 32, 'pooling would have passed p95');
    assert.ok(percentile(pooled, 0.99) <= 50, 'pooling would have passed p99');

    assertOnlyFailure(check(input), /^typing\/plain-100x200 run 4: p95=40 ms exceeds 32 ms$/);
});

test('each device is evaluated alone so a fast device cannot carry a slow one', () => {
    const fast = completeExport(pixel7);
    const slow = completeExport(iphone13);
    raiseOneBoundarySample(findSample(slow, 'viewerColdLayout', 'plain-3x3'), 16);

    const report = check({ samples: [...fast.samples, ...slow.samples] });

    assert.equal(report.passed, false);
    assert.deepEqual(
        report.devices.map(({ device, passed }) => [device, passed]),
        [
            ['Pixel 7', true],
            ['iPhone14,5', false],
        ]
    );
});

test('insufficient runs, short runs and missing cases fail instead of being skipped', async (t) => {
    const cases = [
        [
            'a missing typing run',
            (input) => {
                input.samples = input.samples.filter(
                    (sample) =>
                        !(
                            sample.metric === 'typing' &&
                            sample.fixture === 'plain-1000x20' &&
                            sample.run === 5
                        )
                );
            },
            /^missing samples for typing\/plain-1000x20 run 5$/,
        ],
        [
            'a typing run with 499 measured edits',
            (input) => findSample(input, 'typing', 'plain-3x3', 2).samplesMs.pop(),
            /^typing\/plain-3x3 run 2: has 499 samples, protocol requires 500$/,
        ],
        [
            'a cold layout with 29 generations',
            (input) => findSample(input, 'viewerColdLayout', 'plain-1000x20').samplesMs.pop(),
            /^viewerColdLayout\/plain-1000x20 run 1: has 29 samples, protocol requires 30$/,
        ],
        [
            'a missing single-cell change fixture',
            (input) => {
                input.samples = input.samples.filter(
                    (sample) =>
                        !(sample.metric === 'cellChangeEnd' && sample.fixture === 'rich-merged-3x3')
                );
            },
            /^missing samples for cellChangeEnd\/rich-merged-3x3 run 1$/,
        ],
        [
            'a missing baseline metric',
            (input) => {
                input.samples = input.samples.filter(
                    (sample) =>
                        !(sample.metric === 'remoteUpdate' && sample.fixture === 'plain-100x200')
                );
            },
            /^missing samples for remoteUpdate\/plain-100x200 run 1$/,
        ],
        [
            'a sixth typing run',
            (input) =>
                input.samples.push({
                    ...findSample(input, 'typing', 'plain-3x3', 5),
                    run: 6,
                }),
            /^unexpected extra run typing\/plain-3x3 run 6$/,
        ],
    ];
    for (const [name, mutate, message] of cases) {
        await t.test(name, () => {
            const input = completeExport();
            mutate(input);
            assertOnlyFailure(check(input), message);
            assertOnlyFailure(check(input, 'diagnostic'), message);
        });
    }
});

test('malformed samples are rejected before any gate is evaluated', async (t) => {
    const cases = [
        [
            'NaN sample',
            (sample) => sample.samplesMs.splice(0, 1, Number.NaN),
            /samplesMs must be a non-empty array of finite non-negative numbers/,
        ],
        [
            'null sample from a JSON NaN',
            (sample) => sample.samplesMs.splice(0, 1, null),
            /samplesMs must be a non-empty array of finite non-negative numbers/,
        ],
        [
            'negative sample',
            (sample) => sample.samplesMs.splice(0, 1, -1),
            /samplesMs must be a non-empty array of finite non-negative numbers/,
        ],
        [
            'empty samples',
            (sample) => (sample.samplesMs = []),
            /samplesMs must be a non-empty array/,
        ],
        [
            'missing samples array',
            (sample) => delete sample.samplesMs,
            /samplesMs must be a non-empty array/,
        ],
        [
            'missing counter',
            (sample) => delete sample.counters.unmountedCacheBytes,
            /counter unmountedCacheBytes must be a non-negative integer/,
        ],
        [
            'fractional counter',
            (sample) => (sample.counters.retainedPresentations = 1.5),
            /counter retainedPresentations must be a non-negative integer/,
        ],
        ['missing os', (sample) => delete sample.os, /must have a non-empty os/],
        [
            'missing viewport height',
            (sample) => delete sample.viewportHeight,
            /viewportHeight must be a finite positive number/,
        ],
        [
            'fractional overscan',
            (sample) => (sample.overscanViewports = 0.5),
            /overscanViewports must be a non-negative integer/,
        ],
        [
            'non-finite refresh rate',
            (sample) => (sample.refreshHz = null),
            /refreshHz must be a finite positive number/,
        ],
        ['zero run', (sample) => (sample.run = 0), /run must be a positive integer/],
        [
            'unknown metric',
            (sample) => (sample.metric = 'typingFast'),
            /unexpected metric\/fixture: typingFast\/plain-3x3/,
        ],
        ['unknown platform', (sample) => (sample.platform = 'web'), /platform must be one of/],
    ];
    for (const [name, mutate, message] of cases) {
        await t.test(name, () => {
            const input = completeExport();
            mutate(findSample(input, 'typing', 'plain-3x3'));
            assert.throws(() => check(input), message);
        });
    }

    await t.test('duplicate case', () => {
        const input = completeExport();
        input.samples.push(findSample(input, 'warmMeasurement', 'plain-3x3'));
        assert.throws(() => check(input), /duplicate sample for warmMeasurement\/plain-3x3 run 1/);
    });

    await t.test('empty export', () => {
        assert.throws(() => check({ samples: [] }), /non-empty samples array/);
    });
});

test('release rejects ineligible device and build metadata that diagnostic mode reports', async (t) => {
    for (const [name, overrides, reason] of [
        ['a simulator', { device: 'arm64' }, /device ios\/arm64 is not an eligible release device/],
        [
            'a simulator reporting the iPhone 13 model',
            { physicalDevice: false },
            /physicalDevice is false, release requires true/,
        ],
        ['a different phone', { device: 'iPhone17,1' }, /not an eligible release device/],
        ['a debug build', { buildType: 'debug' }, /buildType is debug, release requires release/],
        ['a 120 Hz display', { refreshHz: 120 }, /refreshHz is 120, release requires 60/],
        ['a scaled font', { textScale: 1.3 }, /textScale is 1\.3, release requires 1/],
        ['a wider viewport', { viewportWidth: 428 }, /viewportWidth is 428, release requires 390/],
        [
            'a taller viewport',
            { viewportHeight: 915 },
            /viewportHeight is 915, release requires 844/,
        ],
        [
            'a wider overscan',
            { overscanViewports: 2 },
            /overscanViewports is 2, release requires 1/,
        ],
        [
            'an android device on the ios platform',
            { device: 'Pixel 7' },
            /device ios\/Pixel 7 is not an eligible release device/,
        ],
    ]) {
        await t.test(name, () => {
            const input = completeExport({ ...iphone13, ...overrides });

            const release = check(input, 'release');
            assert.equal(release.passed, false);
            assert.equal(allFailures(release).length, 1);
            assert.match(allFailures(release)[0], /^ineligible release evidence: /);
            assert.match(allFailures(release)[0], reason);

            const diagnostic = check(input, 'diagnostic');
            assert.equal(diagnostic.passed, true);
            assert.equal(diagnostic.releaseEvidence, false);
            assert.equal(diagnostic.devices[0].releaseEligible, false);
            assert.match(diagnostic.devices[0].releaseIneligibility.join('\n'), reason);
        });
    }
});

test('diagnostic mode still enforces every hard gate', () => {
    const input = completeExport({
        ...iphone13,
        device: 'arm64',
        buildType: 'debug',
        physicalDevice: false,
    });
    raiseOneBoundarySample(findSample(input, 'typing', 'plain-3x3', 1), 50);

    assertOnlyFailure(
        check(input, 'diagnostic'),
        /^typing\/plain-3x3 run 1: p99=50\.01 ms exceeds 50 ms$/
    );
});

test('resource, ownership and reuse counters are hard gates on every sample', async (t) => {
    for (const [name, metric, fixture, overrides, message] of [
        [
            'a second cell input',
            'typing',
            'plain-100x200',
            { maxCellInputInstances: 2 },
            /maxCellInputInstances=2 exceeds 1/,
        ],
        [
            'a nonfinite layout',
            'viewerColdLayout',
            'plain-1000x20',
            { nonFiniteLayouts: 1 },
            /nonFiniteLayouts=1 exceeds 0/,
        ],
        [
            'an unchanged cell remeasured by a warm query',
            'warmMeasurement',
            'plain-3x3',
            { unchangedCellRemeasurements: 1 },
            /unchangedCellRemeasurements=1 exceeds 0/,
        ],
        [
            'an unchanged cell remeasured by a single-cell change',
            'cellChangeEnd',
            'plain-1000x20',
            { unchangedCellRemeasurements: 3 },
            /unchangedCellRemeasurements=3 exceeds 0/,
        ],
        [
            'an unmounted cache one byte over 32 MiB',
            'scrollVertical',
            'plain-1000x20',
            { unmountedCacheBytes: UNMOUNTED_CACHE_BUDGET_BYTES + 1 },
            /unmountedCacheBytes=33554433 exceeds 33554432/,
        ],
        [
            'retained presentations beyond the visible window plus overscan',
            'scrollHorizontal',
            'plain-100x200',
            { retainedPresentations: DERIVED_MAX_RETAINED_PRESENTATIONS + 1 },
            /^scrollHorizontal\/plain-100x200 run 1: retainedPresentations=2273 exceeds 2272$/,
        ],
        [
            'a second cell input on a baseline-only rich fixture',
            'typing',
            'rich-merged-1000x20',
            { maxCellInputInstances: 2 },
            /^typing\/rich-merged-1000x20 run 1: maxCellInputInstances=2 exceeds 1$/,
        ],
        [
            'a nonfinite layout on a baseline-only rich cold layout',
            'viewerColdLayout',
            'rich-merged-100x200',
            { nonFiniteLayouts: 1 },
            /^viewerColdLayout\/rich-merged-100x200 run 1: nonFiniteLayouts=1 exceeds 0$/,
        ],
    ]) {
        await t.test(name, () => {
            const input = completeExport();
            const sample = findSample(input, metric, fixture);
            Object.assign(sample.counters, overrides);
            assertOnlyFailure(check(input), message);
        });
    }

    await t.test('an unmounted cache exactly at 32 MiB passes', () => {
        const input = completeExport();
        Object.assign(findSample(input, 'scrollVertical', 'plain-1000x20').counters, {
            unmountedCacheBytes: UNMOUNTED_CACHE_BUDGET_BYTES,
        });
        assert.deepEqual(allFailures(check(input)), []);
    });
});

test('baseline-only latency is reported apart from hard gates and never fails them', () => {
    const input = completeExport();
    findSample(input, 'typing', 'rich-merged-100x200').samplesMs = [5000, 9000, 12000];
    findSample(input, 'remoteUpdate', 'plain-1000x20').samplesMs = [2500];
    Object.assign(findSample(input, 'structuralCommand', 'plain-3x3').counters, {
        pinnedLayoutBytes: 777,
        authoritativeDocumentBytes: 888,
    });

    const report = check(input);
    const [device] = report.devices;

    assert.equal(report.passed, true, allFailures(report).join('\n'));
    const hardKeys = new Set(device.hardGates.map(({ metric, fixture }) => `${metric}/${fixture}`));
    const baselineKeys = device.baselines.map(({ metric, fixture }) => `${metric}/${fixture}`);
    assert.deepEqual(
        new Set(baselineKeys),
        new Set([
            ...['typing', ...COLD_METRICS].flatMap((metric) =>
                RICH_FIXTURES.map((fixture) => `${metric}/${fixture}`)
            ),
            ...['structuralCommand', 'remoteUpdate'].flatMap((metric) =>
                PLAIN_FIXTURES.map((fixture) => `${metric}/${fixture}`)
            ),
        ])
    );
    assert.ok(
        baselineKeys.every((key) => !hardKeys.has(key)),
        'no metric is both hard and baseline'
    );
    assert.ok(hardKeys.has('typing/plain-3x3'));
    assert.ok(!hardKeys.has('typing/rich-merged-3x3'));
    assert.ok(!hardKeys.has('structuralCommand/plain-3x3'));

    const baseline = (metric, fixture) =>
        device.baselines.find((entry) => entry.metric === metric && entry.fixture === fixture);
    assert.deepEqual(baseline('typing', 'rich-merged-100x200').percentilesMs, {
        p50: 9000,
        p95: 12000,
        p99: 12000,
    });
    assert.deepEqual(baseline('remoteUpdate', 'plain-1000x20').percentilesMs, {
        p50: 2500,
        p95: 2500,
        p99: 2500,
    });
    assert.deepEqual(baseline('structuralCommand', 'plain-3x3').reportedCounters, {
        pinnedLayoutBytes: 777,
        authoritativeDocumentBytes: 888,
    });
    assert.equal(baseline('typing', 'rich-merged-100x200').passed, undefined);
});

test('the CLI requires --input and a mode and fails on unreadable input', async (t) => {
    for (const [name, args, input, message] of [
        ['no input flag', ['--release'], undefined, /--input <export\.json> is required/],
        [
            'input flag without a path',
            ['--release', '--input'],
            undefined,
            /--input requires a file path/,
        ],
        [
            'absent input file',
            ['--release', '--input', '<input>'],
            undefined,
            /failed to read input file .*export\.json/,
        ],
        [
            'malformed input file',
            ['--release', '--input', '<input>'],
            '{',
            /failed to parse input JSON/,
        ],
        [
            'no mode',
            ['--input', '<input>'],
            completeExport(),
            /provide either --release or --diagnostic/,
        ],
        [
            'both modes',
            ['--release', '--diagnostic', '--input', '<input>'],
            completeExport(),
            /--release and --diagnostic are mutually exclusive/,
        ],
        ['unknown argument', ['--release', '--pool'], undefined, /unknown argument: --pool/],
    ]) {
        await t.test(name, () => {
            const result = runChecker(args, input);
            assert.notEqual(result.status, 0);
            assert.match(result.stderr, /^Table performance check failed: /m);
            assert.match(result.stderr, message);
            assert.doesNotMatch(result.stderr, /at .*\.mjs:\d+/);
        });
    }
});

test('the CLI separates release verification from diagnostic evidence', () => {
    const eligible = runChecker(['--release', '--input', '<input>'], completeExport());
    assert.equal(eligible.status, 0, eligible.stderr);
    assert.match(eligible.stdout, /1 device evidence set\(s\) passed all release table gates/);
    assert.match(eligible.stdout, /"releaseEvidence": true/);

    const simulator = completeExport({
        ...iphone13,
        device: 'arm64',
        buildType: 'debug',
        physicalDevice: false,
    });
    const release = runChecker(['--release', '--input', '<input>'], simulator);
    assert.notEqual(release.status, 0);
    assert.match(release.stderr, /ios\/arm64: ineligible release evidence: device ios\/arm64/);
    assert.match(release.stderr, /Table performance check failed: release gates failed/);

    const diagnostic = runChecker(['--diagnostic', '--input', '<input>'], simulator);
    assert.equal(diagnostic.status, 0, diagnostic.stderr);
    assert.match(diagnostic.stdout, /passed all diagnostic \(not release evidence\) table gates/);
    assert.doesNotMatch(diagnostic.stdout, /passed all release/);
    assert.match(diagnostic.stdout, /"releaseEvidence": false/);
});

test('the npm release script passes eligible evidence and rejects a simulator export', () => {
    const npmArguments = [
        'run',
        '--silent',
        'test:tables:performance:check',
        '--',
        '--input',
        '<input>',
    ];

    const eligible = runWithInputFile('npm', npmArguments, completeExport());
    assert.equal(eligible.status, 0, eligible.stderr);
    assert.match(eligible.stdout, /passed all release table gates/);

    const simulator = runWithInputFile(
        'npm',
        npmArguments,
        completeExport({
            ...iphone13,
            device: 'arm64',
            buildType: 'debug',
            physicalDevice: false,
        })
    );
    assert.notEqual(simulator.status, 0);
    assert.match(simulator.stderr, /ineligible release evidence: physicalDevice is false/);
    assert.match(simulator.stderr, /Table performance check failed: release gates failed/);
});

test('retained presentations are gated by the derived window, not by a self-reported bound', async (t) => {
    await t.test('honest fixtures may retain different counts under the ceiling', () => {
        const input = completeExport();
        for (const sample of input.samples) {
            sample.counters.retainedPresentations = sample.fixture.startsWith('rich-merged')
                ? TYPICAL_RETAINED_PRESENTATIONS / 2
                : DERIVED_MAX_RETAINED_PRESENTATIONS;
        }
        assert.deepEqual(allFailures(check(input)), []);
    });

    await t.test('a large fixture retaining beyond the ceiling fails', () => {
        const input = completeExport();
        findSample(input, 'scrollVertical', 'plain-1000x20').counters.retainedPresentations =
            DERIVED_MAX_RETAINED_PRESENTATIONS * 50;
        assertOnlyFailure(
            check(input),
            /^scrollVertical\/plain-1000x20 run 1: retainedPresentations=113600 exceeds 2272$/
        );
    });

    await t.test('an inflation shared by every fixture still fails each sample', () => {
        const input = completeExport();
        for (const sample of input.samples) {
            sample.counters.retainedPresentations = DERIVED_MAX_RETAINED_PRESENTATIONS + 1;
        }
        const failures = allFailures(check(input));
        assert.equal(failures.length, input.samples.length);
        assert.ok(
            failures.every((failure) => failure.endsWith('retainedPresentations=2273 exceeds 2272'))
        );
    });
});

test('a single-cell change must remeasure exactly the edited cell for every edit', async (t) => {
    for (const [name, changed, message] of [
        [
            'nothing remeasured',
            0,
            /^cellChangeStart\/plain-100x200 run 1: changedCellRemeasurements=0, protocol requires 4 \(1 per sample\)$/,
        ],
        [
            'one edit remeasured twice',
            EDITS_PER_UNSIZED_CASE + 1,
            /^cellChangeStart\/plain-100x200 run 1: changedCellRemeasurements=5, protocol requires 4 \(1 per sample\)$/,
        ],
    ]) {
        await t.test(name, () => {
            const input = completeExport();
            findSample(
                input,
                'cellChangeStart',
                'plain-100x200'
            ).counters.changedCellRemeasurements = changed;
            assertOnlyFailure(check(input), message);
        });
    }

    await t.test('the counter is required on single-cell change samples', () => {
        const input = completeExport();
        delete findSample(input, 'cellChangeEnd', 'rich-merged-3x3').counters
            .changedCellRemeasurements;
        assert.throws(
            () => check(input),
            /cellChangeEnd\/rich-merged-3x3 run 1\) counter changedCellRemeasurements must be a non-negative integer/
        );
    });
});

test('typing runs must report exactly the 20 discarded warm-up edits', async (t) => {
    await t.test('19 discarded', () => {
        const input = completeExport();
        findSample(input, 'typing', 'plain-3x3', 3).warmupSamplesDiscarded = 19;
        assertOnlyFailure(
            check(input),
            /^typing\/plain-3x3 run 3: discarded 19 warm-up samples, protocol requires 20$/
        );
    });

    await t.test('unreported', () => {
        const input = completeExport();
        delete findSample(input, 'typing', 'plain-3x3', 3).warmupSamplesDiscarded;
        assert.throws(
            () => check(input),
            /typing\/plain-3x3 run 3\) warmupSamplesDiscarded must be a non-negative integer/
        );
    });
});

test('every sample must state whether it ran on physical hardware', () => {
    const input = completeExport();
    delete findSample(input, 'viewerColdLayout', 'plain-3x3').physicalDevice;

    assert.throws(() => check(input), /physicalDevice must be a boolean/);
});

test('editor and viewer cold metrics are each required for every fixture', async (t) => {
    for (const metric of COLD_METRICS) {
        for (const fixture of [...PLAIN_FIXTURES, ...RICH_FIXTURES]) {
            await t.test(`${metric}/${fixture}`, () => {
                const input = completeExport();
                findSample(input, metric, fixture);
                input.samples = input.samples.filter(
                    (sample) => sample.metric !== metric || sample.fixture !== fixture
                );
                assertOnlyFailure(
                    check(input),
                    new RegExp(`^missing samples for ${metric}/${fixture} run 1$`)
                );
            });
        }
    }
});

test('an editor cold miss fails while viewer cold remains within its budget', () => {
    const input = completeExport();
    raiseOneBoundarySample(findSample(input, 'editorColdLayout', 'plain-1000x20'), 500);
    const report = check(input);
    assertOnlyFailure(
        report,
        /^editorColdLayout\/plain-1000x20 run 1: p95=500.01 ms exceeds 500 ms$/
    );
    const viewer = report.devices[0].hardGates.find(
        (entry) => entry.metric === 'viewerColdLayout' && entry.fixture === 'plain-1000x20'
    );
    assert.deepEqual(viewer.percentilesMs, { p95: 500 });
});

test('the legacy coldLayout metric is rejected as unknown', () => {
    const input = completeExport();
    input.samples.push({
        ...findSample(input, 'typing', 'plain-3x3'),
        metric: 'coldLayout',
    });
    assert.throws(() => check(input), /unexpected metric\/fixture: coldLayout\/plain-3x3/);
});

test('the captured iOS table run satisfies the diagnostic export protocol', () => {
    const input = JSON.parse(
        readFileSync(
            path.join(repositoryRoot, 'scripts/tests/fixtures/table-performance-ios-sample.json'),
            'utf8'
        )
    );
    const report = check(input, 'diagnostic');
    assert.equal(report.releaseEvidence, false);
    assert.equal(report.devices.length, 1);
    assert.equal(report.devices[0].platform, 'ios');
    assert.equal(report.devices[0].physicalDevice, false);
    for (const failure of allFailures(report)) {
        assert.match(
            failure,
            /(?: ms exceeds |(?:unmountedCacheBytes|retainedPresentations|unchangedCellRemeasurements|changedCellRemeasurements|nonFiniteLayouts|maxCellInputInstances)=)/
        );
    }
    for (const sample of input.samples) {
        assert.equal(Object.hasOwn(sample.counters, 'presentationWindowBound'), false);
        for (const stage of Object.values(sample.stageSamplesMs)) {
            assert.equal(stage.length, sample.samplesMs.length);
            assert.ok(stage.every((value) => Number.isFinite(value) && value >= 0));
        }
        const requiredStages = {
            typing: ['nativeInputAndFFI', 'nativeFrameAndFFI', 'adapterAdoption'],
            editorColdLayout: ['replacementAndFFI', 'nativeFrameAndFFI', 'adapterAdoption'],
            viewerColdLayout: ['viewerCompileAndLift'],
        }[sample.metric];
        if (requiredStages) {
            for (const stage of [
                ...requiredStages,
                'tablePreparationAndGeometry',
                'drawingAndLayerRecording',
            ]) {
                assert.ok(
                    sample.stageSamplesMs[stage]?.every((value) => value > 0),
                    `${sample.fixture}/${sample.metric} missing ${stage}`
                );
            }
        }
        if (sample.metric === 'typing') {
            assert.equal(sample.wrapCount + sample.nonWrapCount, sample.samplesMs.length);
            assert.ok(sample.wrapCount > 0);
            assert.ok(sample.nonWrapCount > 0);
        }
    }
});
