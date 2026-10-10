import assert from 'node:assert/strict';
import { copyFileSync, mkdirSync, mkdtempSync, readFileSync, realpathSync, rmSync, writeFileSync } from 'node:fs';
import { spawnSync } from 'node:child_process';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { requireApi24SmokeSuccess } from '../run-android-api24-smoke.mjs';

const captures = JSON.parse(readFileSync(new URL('./android-api24-instrumentation-results.json', import.meta.url), 'utf8'));
assert.doesNotThrow(() => requireApi24SmokeSuccess(captures.passed.output), 'the real API 24 editor/viewer result must pass');
for (const key of ['zeroTests', 'failedTest', 'abortedRun']) {
    assert.throws(() => requireApi24SmokeSuccess(captures[key].output),
        `${key}: a real unsuccessful instrumentation run must never pass`);
}
assert.equal(captures.zeroTests.adbExit, 0, 'zero-test instrumentation can exit adb successfully');
assert.equal(captures.failedTest.adbExit, 0, 'failed JUnit instrumentation can exit adb successfully');
const mutations = {
    skipped: captures.passed.output.replace('INSTRUMENTATION_STATUS_CODE: 0', 'INSTRUMENTATION_STATUS_CODE: -3'),
    failedExistingTest: captures.passed.output.replace('INSTRUMENTATION_STATUS_CODE: 0', 'INSTRUMENTATION_STATUS_CODE: -2'),
    crashed: captures.passed.output.replace('INSTRUMENTATION_CODE: -1', 'INSTRUMENTATION_CODE: 0'),
    truncated: captures.passed.output.slice(0, captures.passed.output.indexOf('INSTRUMENTATION_CODE:')),
    unexpectedClass: captures.passed.output.replaceAll('com.apollohg.editor.AndroidApi24SmokeTest', 'com.apollohg.editor.OtherTest'),
    missingSummary: captures.passed.output.replace('OK (1 test)', ''),
    duplicateRun: captures.passed.output + captures.passed.output,
    unfinishedStatus: captures.passed.output.replace('INSTRUMENTATION_CODE:', 'INSTRUMENTATION_STATUS: class=OtherTest\nINSTRUMENTATION_CODE:'),
    statusError: captures.passed.output.replace('INSTRUMENTATION_CODE:', 'INSTRUMENTATION_STATUS: Error=Unable to finish\nINSTRUMENTATION_CODE:'),
    adbError: captures.passed.output.replace('INSTRUMENTATION_CODE:', 'adb: error: device offline\nINSTRUMENTATION_CODE:'),
};
for (const [key, output] of Object.entries(mutations)) {
    assert.throws(() => requireApi24SmokeSuccess(output), `${key}: the protocol gate must reject an incomplete or unsuccessful result`);
}

const fixtureRoot = mkdtempSync(join(tmpdir(), 'android-api24-gate-'));
try {
    const runnerPath = join(fixtureRoot, 'scripts/run-android-api24-smoke.mjs');
    const callsPath = join(fixtureRoot, 'commands.jsonl');
    const resultPath = join(fixtureRoot, 'android/build/outputs/androidTest-results/api24/instrumentation.txt');
    const apkDirectory = join(fixtureRoot, 'android/build/outputs/apk/androidTest/debug');
    const apkName = 'universal-androidTest.apk';
    for (const directory of ['scripts', 'bin', 'sdk/platform-tools', 'example/android']) {
        mkdirSync(join(fixtureRoot, directory), { recursive: true });
    }
    mkdirSync(apkDirectory, { recursive: true });
    copyFileSync(new URL('../run-android-api24-smoke.mjs', import.meta.url), runnerPath);
    writeFileSync(join(fixtureRoot, 'captures.json'), JSON.stringify(captures));
    writeFileSync(join(apkDirectory, 'output-metadata.json'), JSON.stringify({
        applicationId: 'com.apollohg.editor.test',
        elements: [{ type: 'SINGLE', filters: [], outputFile: apkName }],
    }));
    writeFileSync(join(apkDirectory, apkName), 'universal APK fixture');
    const recordCommand = `import { appendFileSync, readFileSync } from 'node:fs';
const args = process.argv.slice(2);
appendFileSync(process.env.CALLS_PATH, JSON.stringify([process.argv[1], ...args]) + '\\n');
`;
    for (const file of ['bin/npm', 'example/android/gradlew']) {
        writeFileSync(join(fixtureRoot, file), `#!/usr/bin/env node\n${recordCommand}`, { mode: 0o755 });
    }
    writeFileSync(join(fixtureRoot, 'sdk/platform-tools/adb'), `#!/usr/bin/env node
${recordCommand}
if (args[0] === 'devices') {
    process.stdout.write(process.env.DEVICES_OUTPUT);
} else if (args.includes('getprop')) {
    process.stdout.write(process.env.DEVICE_SDK + '\\n');
} else if (args.includes('instrument')) {
    const capture = JSON.parse(readFileSync(process.env.CAPTURES_PATH, 'utf8'))[process.env.CAPTURE_KEY];
    process.stdout.write(capture.output);
    process.stderr.write(process.env.ADB_STDERR ?? '');
    process.exit(Number(process.env.ADB_EXIT ?? capture.adbExit));
} else if (args.includes('install')) {
    process.exit(Number(process.env.INSTALL_EXIT ?? 0));
} else {
    process.exit(1);
}
`, { mode: 0o755 });
    function runFixture(overrides = {}) {
        writeFileSync(callsPath, '');
        rmSync(resultPath, { force: true });
        const env = { ...process.env };
        for (const key of ['ANDROID_SERIAL', 'ANDROID_DEVICE_ID', 'EMULATOR_PORT']) delete env[key];
        return spawnSync(process.execPath, [runnerPath], {
            cwd: fixtureRoot,
            env: {
                ...env,
                PATH: `${fixtureRoot}/bin:${process.env.PATH}`,
                ANDROID_HOME: join(fixtureRoot, 'sdk'),
                CALLS_PATH: callsPath,
                CAPTURES_PATH: join(fixtureRoot, 'captures.json'),
                DEVICES_OUTPUT: 'List of devices attached\nemulator-5580\tdevice\n',
                DEVICE_SDK: '24',
                CAPTURE_KEY: 'passed',
                ...overrides,
            },
            encoding: 'utf8',
        });
    }
    const passed = runFixture();
    assert.equal(passed.status, 0, passed.stderr);
    assert.equal(readFileSync(resultPath, 'utf8'), captures.passed.output);
    const calls = readFileSync(callsPath, 'utf8').trim().split('\n').map((line) => JSON.parse(line));
    const install = calls.find((call) => call.includes('install'));
    assert.deepEqual(install.slice(1), ['-s', 'emulator-5580', 'install', '--no-streaming', '-r', '-t', realpathSync(join(apkDirectory, apkName))]);
    const instrumentation = calls.find((call) => call.includes('instrument'));
    assert.deepEqual(instrumentation.slice(1), ['-s', 'emulator-5580', 'shell', 'am', 'instrument', '-w', '-r', '-e', 'class',
        'com.apollohg.editor.AndroidApi24SmokeTest', 'com.apollohg.editor.test/androidx.test.runner.AndroidJUnitRunner']);
    for (const key of ['zeroTests', 'failedTest', 'abortedRun']) {
        const result = runFixture({ CAPTURE_KEY: key });
        assert.notEqual(result.status, 0, `${key}: the complete runner must propagate the real unsuccessful result`);
        assert.equal(readFileSync(resultPath, 'utf8'), captures[key].output, `${key}: preserve the failed instrumentation output`);
    }
    const transportFailure = runFixture({ ADB_EXIT: '1' });
    assert.notEqual(transportFailure.status, 0, 'successful-looking output must not hide a failed adb process');
    assert.equal(readFileSync(resultPath, 'utf8'), captures.passed.output, 'preserve transport-failure output');
    const stderrFailure = runFixture({ ADB_STDERR: 'adb: error: device offline\n' });
    assert.notEqual(stderrFailure.status, 0, 'an adb transport error on stderr must not pass');
    assert.match(readFileSync(resultPath, 'utf8'), /adb: error: device offline/);
    const wrongSdk = runFixture({ DEVICE_SDK: '25' });
    assert.notEqual(wrongSdk.status, 0, 'a newer Android SDK must not satisfy the API 24 gate');
    assert.doesNotMatch(readFileSync(callsPath, 'utf8'), /gradlew|\/npm/, 'reject the wrong SDK before building or installing');
    const ambiguous = runFixture({ DEVICES_OUTPUT: 'List of devices attached\nemulator-5580\tdevice\nemulator-5582\tdevice\n' });
    assert.notEqual(ambiguous.status, 0, 'ambiguous devices require explicit serial selection');
    assert.doesNotMatch(readFileSync(callsPath, 'utf8'), /getprop|gradlew|\/npm/);
    const selected = runFixture({ ANDROID_SERIAL: 'emulator-5582' });
    assert.equal(selected.status, 0, selected.stderr);
    assert.doesNotMatch(readFileSync(callsPath, 'utf8'), /emulator-5580/, 'all adb operations must target the explicitly selected device');
    const installFailure = runFixture({ INSTALL_EXIT: '1' });
    assert.notEqual(installFailure.status, 0, 'failed APK installation must not run stale installed instrumentation');
    assert.doesNotMatch(readFileSync(callsPath, 'utf8'), /"instrument"/);
} finally {
    rmSync(fixtureRoot, { recursive: true, force: true });
}
console.log('API 24 gate preserves the real test, selects SDK 24, installs without streaming, and rejects unsuccessful protocol or adb results.');
