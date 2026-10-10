#!/usr/bin/env node

import path from 'node:path';
import { fileURLToPath } from 'node:url';

import { readJsonFile } from './lib/json-file.mjs';

const repositoryRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const CONFIG_PATH = path.join(repositoryRoot, 'scripts/tests/table-performance-config.json');
const PLATFORMS = ['ios', 'android'];
const RETAINED_PRESENTATIONS = 'retainedPresentations';
const RELEASE_MODE = 'release';
const DIAGNOSTIC_MODE = 'diagnostic';
const DEVICE_FIELDS = [
    'platform',
    'device',
    'os',
    'buildType',
    'refreshHz',
    'textScale',
    'viewportWidth',
    'viewportHeight',
    'overscanViewports',
    'physicalDevice',
];
const RELEASE_ENVIRONMENT_FIELDS = [
    'physicalDevice',
    'buildType',
    'refreshHz',
    'textScale',
    'viewportWidth',
    'viewportHeight',
    'overscanViewports',
];

export function percentile(samples, fraction) {
    if (
        samples.length === 0 ||
        !Number.isFinite(fraction) ||
        fraction <= 0 ||
        fraction > 1 ||
        samples.some((value) => !Number.isFinite(value) || value < 0)
    ) {
        throw new Error('Invalid performance samples');
    }
    const ordered = [...samples].sort((left, right) => left - right);
    return ordered[Math.ceil(fraction * ordered.length) - 1];
}

export function loadTablePerformanceConfig() {
    return readJsonFile(CONFIG_PATH, 'config');
}

function parseArguments(args) {
    let inputPath;
    let mode;

    for (let index = 0; index < args.length; index += 1) {
        const argument = args[index];
        if (argument === '--input') {
            inputPath = args[index + 1];
            index += 1;
            if (!inputPath) {
                throw new Error('--input requires a file path');
            }
        } else if (argument === '--release' || argument === '--diagnostic') {
            const requested = argument === '--release' ? RELEASE_MODE : DIAGNOSTIC_MODE;
            if (mode && mode !== requested) {
                throw new Error('--release and --diagnostic are mutually exclusive');
            }
            mode = requested;
        } else {
            throw new Error(`unknown argument: ${argument}`);
        }
    }

    if (!inputPath) {
        throw new Error('--input <export.json> is required');
    }
    if (!mode) {
        throw new Error('provide either --release or --diagnostic');
    }
    return { inputPath, mode };
}

function percentileLabel(fraction) {
    return `p${Math.round(fraction * 100)}`;
}

function isNonEmptyString(value) {
    return typeof value === 'string' && value.length > 0;
}

function isPositiveFinite(value) {
    return typeof value === 'number' && Number.isFinite(value) && value > 0;
}

function requiredCounters(config, classification) {
    return [
        ...Object.keys(config.counterGates),
        ...config.reportedCounters,
        RETAINED_PRESENTATIONS,
        ...Object.keys(classification.gate?.countersPerSample ?? {}),
    ];
}

function classifySample(sample, config) {
    const matches = (entry) =>
        entry.metric === sample.metric && entry.fixtures.includes(sample.fixture);
    const hardGate = config.hardGates.find(matches);
    if (hardGate) {
        return { kind: 'hard', gate: hardGate };
    }
    if (config.baselineMetrics.some(matches)) {
        return { kind: 'baseline' };
    }
    return undefined;
}

function validateSample(sample, index, config) {
    const label = `sample ${index}`;
    if (!sample || typeof sample !== 'object' || Array.isArray(sample)) {
        throw new Error(`${label} must be an object`);
    }
    if (!PLATFORMS.includes(sample.platform)) {
        throw new Error(`${label} platform must be one of ${PLATFORMS.join(', ')}`);
    }
    for (const field of ['device', 'os', 'buildType', 'fixture', 'metric']) {
        if (!isNonEmptyString(sample[field])) {
            throw new Error(`${label} must have a non-empty ${field}`);
        }
    }
    if (!Number.isSafeInteger(sample.overscanViewports) || sample.overscanViewports < 0) {
        throw new Error(`${label} overscanViewports must be a non-negative integer`);
    }
    if (typeof sample.physicalDevice !== 'boolean') {
        throw new Error(`${label} physicalDevice must be a boolean`);
    }
    for (const field of ['refreshHz', 'textScale', 'viewportWidth', 'viewportHeight']) {
        if (!isPositiveFinite(sample[field])) {
            throw new Error(`${label} ${field} must be a finite positive number`);
        }
    }
    if (!Number.isSafeInteger(sample.run) || sample.run < 1) {
        throw new Error(`${label} run must be a positive integer`);
    }
    const caseLabel = caseKey(sample.metric, sample.fixture, sample.run);
    const classification = classifySample(sample, config);
    if (!classification) {
        throw new Error(
            `${label} has unexpected metric/fixture: ${sample.metric}/${sample.fixture}`
        );
    }
    if (
        classification.gate?.warmupSamples !== undefined &&
        (!Number.isSafeInteger(sample.warmupSamplesDiscarded) || sample.warmupSamplesDiscarded < 0)
    ) {
        throw new Error(
            `${label} (${caseLabel}) warmupSamplesDiscarded must be a non-negative integer`
        );
    }
    if (
        !Array.isArray(sample.samplesMs) ||
        sample.samplesMs.length === 0 ||
        sample.samplesMs.some(
            (value) => typeof value !== 'number' || !Number.isFinite(value) || value < 0
        )
    ) {
        throw new Error(
            `${label} (${caseLabel}) samplesMs must be a non-empty array of finite non-negative numbers`
        );
    }
    if (!sample.counters || typeof sample.counters !== 'object' || Array.isArray(sample.counters)) {
        throw new Error(`${label} (${caseLabel}) must have a counters object`);
    }
    for (const counter of requiredCounters(config, classification)) {
        const value = sample.counters[counter];
        if (!Number.isSafeInteger(value) || value < 0) {
            throw new Error(
                `${label} (${caseLabel}) counter ${counter} must be a non-negative integer`
            );
        }
    }
}

function deviceKey(sample) {
    return JSON.stringify(DEVICE_FIELDS.map((field) => sample[field]));
}

function caseKey(metric, fixture, run) {
    return `${metric}/${fixture} run ${run}`;
}

function groupByDevice(samples) {
    const devices = new Map();
    for (const sample of samples) {
        const key = deviceKey(sample);
        if (!devices.has(key)) {
            devices.set(key, {
                metadata: Object.fromEntries(DEVICE_FIELDS.map((field) => [field, sample[field]])),
                cases: new Map(),
            });
        }
        const device = devices.get(key);
        const sampleKey = caseKey(sample.metric, sample.fixture, sample.run);
        if (device.cases.has(sampleKey)) {
            throw new Error(`duplicate sample for ${sampleKey} on ${device.metadata.device}`);
        }
        device.cases.set(sampleKey, sample);
    }
    return [...devices.values()];
}

function releaseIneligibility(metadata, releaseEnvironment) {
    const reasons = [];
    if (
        !releaseEnvironment.devices.some(
            ({ platform, device }) => platform === metadata.platform && device === metadata.device
        )
    ) {
        reasons.push(
            `device ${metadata.platform}/${metadata.device} is not an eligible release device`
        );
    }
    for (const field of RELEASE_ENVIRONMENT_FIELDS) {
        if (metadata[field] !== releaseEnvironment[field]) {
            reasons.push(
                `${field} is ${metadata[field]}, release requires ${releaseEnvironment[field]}`
            );
        }
    }
    return reasons;
}

export function maxRetainedPresentations(config) {
    const { viewportWidth, viewportHeight, overscanViewports } = config.releaseEnvironment;
    const { minColumnWidth, cellPadding, borderWidth } = config.defaultTableTheme;
    const minRowHeight = 2 * (cellPadding + borderWidth);
    const windowViewports = 1 + 2 * overscanViewports;
    const straddlingCells = 1;
    const columns = Math.ceil((viewportWidth * windowViewports) / minColumnWidth) + straddlingCells;
    const rows = Math.ceil((viewportHeight * windowViewports) / minRowHeight) + straddlingCells;
    return columns * rows;
}

function counterFailures(sample, config) {
    const limits = {
        ...config.counterGates,
        [RETAINED_PRESENTATIONS]: maxRetainedPresentations(config),
    };
    return Object.entries(limits)
        .filter(([counter, allowed]) => sample.counters[counter] > allowed)
        .map(([counter, allowed]) => `${counter}=${sample.counters[counter]} exceeds ${allowed}`);
}

function reportedCounters(sample, config) {
    return Object.fromEntries(
        config.reportedCounters.map((counter) => [counter, sample.counters[counter]])
    );
}

function evaluateHardCase(sample, gate, config) {
    const failures = counterFailures(sample, config);
    if (gate.warmupSamples !== undefined && sample.warmupSamplesDiscarded !== gate.warmupSamples) {
        failures.push(
            `discarded ${sample.warmupSamplesDiscarded} warm-up samples, protocol requires ${gate.warmupSamples}`
        );
    }
    for (const [counter, perSample] of Object.entries(gate.countersPerSample ?? {})) {
        const expected = perSample * sample.samplesMs.length;
        if (sample.counters[counter] !== expected) {
            failures.push(
                `${counter}=${sample.counters[counter]}, protocol requires ${expected} (${perSample} per sample)`
            );
        }
    }
    if (gate.samplesPerRun !== undefined && sample.samplesMs.length !== gate.samplesPerRun) {
        failures.push(
            `has ${sample.samplesMs.length} samples, protocol requires ${gate.samplesPerRun}`
        );
    }
    if (gate.minTraversalMs !== undefined) {
        const traversalMs = sample.samplesMs.reduce((total, value) => total + value, 0);
        if (traversalMs < gate.minTraversalMs) {
            failures.push(
                `traversed ${traversalMs} ms, protocol requires ${gate.minTraversalMs} ms`
            );
        }
    }
    if (gate.maxTableAttributedPauseMs !== undefined) {
        const attribution = sample.tableAttributed;
        if (
            !Array.isArray(attribution) ||
            attribution.length !== sample.samplesMs.length ||
            attribution.some((marker) => typeof marker !== 'boolean')
        ) {
            failures.push('tableAttributed must mark every frame sample with a boolean');
        } else {
            const pauses = sample.samplesMs.filter(
                (value, index) => attribution[index] && value > gate.maxTableAttributedPauseMs
            );
            if (pauses.length > 0) {
                failures.push(
                    `table-attributed pause of ${Math.max(...pauses)} ms exceeds ${gate.maxTableAttributedPauseMs} ms`
                );
            }
        }
    }
    const percentilesMs = {};
    for (const { fraction, maxMs } of gate.percentiles) {
        const value = percentile(sample.samplesMs, fraction);
        percentilesMs[percentileLabel(fraction)] = value;
        if (value > maxMs) {
            failures.push(`${percentileLabel(fraction)}=${value} ms exceeds ${maxMs} ms`);
        }
    }
    return {
        metric: sample.metric,
        fixture: sample.fixture,
        run: sample.run,
        sampleCount: sample.samplesMs.length,
        percentilesMs,
        reportedCounters: reportedCounters(sample, config),
        passed: failures.length === 0,
        failures,
    };
}

function evaluateBaselineCase(sample, config) {
    return {
        metric: sample.metric,
        fixture: sample.fixture,
        run: sample.run,
        sampleCount: sample.samplesMs.length,
        percentilesMs: Object.fromEntries(
            config.baselinePercentiles.map((fraction) => [
                percentileLabel(fraction),
                percentile(sample.samplesMs, fraction),
            ])
        ),
        reportedCounters: reportedCounters(sample, config),
        hardCounterFailures: counterFailures(sample, config),
    };
}

function missingEvidence(device, config) {
    const missing = [];
    for (const gate of config.hardGates) {
        for (const fixture of gate.fixtures) {
            for (let run = 1; run <= gate.runs; run += 1) {
                if (!device.cases.has(caseKey(gate.metric, fixture, run))) {
                    missing.push(caseKey(gate.metric, fixture, run));
                }
            }
        }
    }
    for (const { metric, fixtures } of config.baselineMetrics) {
        for (const fixture of fixtures) {
            if (!device.cases.has(caseKey(metric, fixture, 1))) {
                missing.push(caseKey(metric, fixture, 1));
            }
        }
    }
    return missing;
}

function unexpectedRuns(device, config) {
    return [...device.cases.values()]
        .filter((sample) => {
            const classification = classifySample(sample, config);
            return classification.kind === 'hard' && sample.run > classification.gate.runs;
        })
        .map((sample) => caseKey(sample.metric, sample.fixture, sample.run));
}

function evaluateDevice(device, config, mode) {
    const ineligibility = releaseIneligibility(device.metadata, config.releaseEnvironment);
    const hardGates = [];
    const baselines = [];
    for (const sample of device.cases.values()) {
        const classification = classifySample(sample, config);
        if (classification.kind === 'hard') {
            hardGates.push(evaluateHardCase(sample, classification.gate, config));
        } else {
            baselines.push(evaluateBaselineCase(sample, config));
        }
    }
    const failures = [
        ...missingEvidence(device, config).map((key) => `missing samples for ${key}`),
        ...unexpectedRuns(device, config).map((key) => `unexpected extra run ${key}`),
        ...hardGates.flatMap(({ metric, fixture, run, failures: caseFailures }) =>
            caseFailures.map((failure) => `${caseKey(metric, fixture, run)}: ${failure}`)
        ),
        ...baselines.flatMap(({ metric, fixture, run, hardCounterFailures }) =>
            hardCounterFailures.map((failure) => `${caseKey(metric, fixture, run)}: ${failure}`)
        ),
    ];
    if (mode === RELEASE_MODE) {
        failures.push(...ineligibility.map((reason) => `ineligible release evidence: ${reason}`));
    }
    return {
        ...device.metadata,
        releaseEligible: ineligibility.length === 0,
        releaseIneligibility: ineligibility,
        passed: failures.length === 0,
        failures,
        hardGates,
        baselines,
    };
}

export function checkTablePerformance(input, config, mode) {
    if (mode !== RELEASE_MODE && mode !== DIAGNOSTIC_MODE) {
        throw new Error(`mode must be ${RELEASE_MODE} or ${DIAGNOSTIC_MODE}`);
    }
    if (!input || typeof input !== 'object' || Array.isArray(input)) {
        throw new Error('input must be a JSON object');
    }
    if (!Array.isArray(input.samples) || input.samples.length === 0) {
        throw new Error('input must contain a non-empty samples array');
    }
    input.samples.forEach((sample, index) => validateSample(sample, index, config));
    const devices = groupByDevice(input.samples).map((device) =>
        evaluateDevice(device, config, mode)
    );
    return {
        mode,
        releaseEvidence: mode === RELEASE_MODE,
        passed: devices.every(({ passed }) => passed),
        devices,
    };
}

function main() {
    try {
        const { inputPath, mode } = parseArguments(process.argv.slice(2));
        const report = checkTablePerformance(
            readJsonFile(inputPath, 'input'),
            loadTablePerformanceConfig(),
            mode
        );
        console.log(JSON.stringify(report, null, 4));
        const label = mode === RELEASE_MODE ? 'release' : 'diagnostic (not release evidence)';
        if (!report.passed) {
            for (const device of report.devices) {
                for (const failure of device.failures) {
                    console.error(`${device.platform}/${device.device}: ${failure}`);
                }
            }
            throw new Error(`${label} gates failed`);
        }
        console.log(
            `${report.devices.length} device evidence set(s) passed all ${label} table gates`
        );
    } catch (error) {
        console.error(`Table performance check failed: ${error.message}`);
        process.exitCode = 1;
    }
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
    main();
}
