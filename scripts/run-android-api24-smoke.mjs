import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { existsSync, readFileSync, mkdirSync, realpathSync, writeFileSync } from 'node:fs';
import { homedir } from 'node:os';
import { dirname, join } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';

const repoRoot = fileURLToPath(new URL('../', import.meta.url));
const requiredSdk = '24';
const testClass = 'com.apollohg.editor.AndroidApi24SmokeTest';
const testMethod = 'editorAndViewerSmoke';
const testApplicationId = 'com.apollohg.editor.test';
const testRunner = `${testApplicationId}/androidx.test.runner.AndroidJUnitRunner`;
const testRunnerId = 'AndroidJUnitRunner';
const expectedTestCount = 1;
const requiredDeviceCount = 1;
const requiredApkCount = 1;
const testStarted = 1;
const testPassed = 0;
const instrumentationFinished = -1;
const adbOperationTimeoutMs = 240_000;
const statusPrefix = 'INSTRUMENTATION_STATUS: ';
const statusCodePattern = /^INSTRUMENTATION_STATUS_CODE: (-?\d+)$/;
const resultCodePattern = /^INSTRUMENTATION_CODE: (-?\d+)$/;
const adbErrorPattern = /^(?:adb: )?error:/m;

export function requireApi24SmokeSuccess(output) {
    const events = [];
    const resultCodes = [];
    let fields = {};
    const lines = output.replaceAll('\r', '').split('\n');
    for (const line of lines) {
        if (line.startsWith(statusPrefix)) {
            const separator = line.indexOf('=', statusPrefix.length);
            if (separator !== -1) {
                fields[line.slice(statusPrefix.length, separator)] = line.slice(separator + 1);
            }
        }
        const status = line.match(statusCodePattern);
        if (status) {
            events.push({ code: Number(status[1]), fields });
            fields = {};
        }
        const result = line.match(resultCodePattern);
        if (result) resultCodes.push(Number(result[1]));
    }
    assert.deepEqual(fields, {}, 'API 24 instrumentation must not leave an unfinished status record');
    assert.deepEqual(resultCodes, [instrumentationFinished], 'API 24 instrumentation must finish successfully exactly once');
    assert.match(lines.filter(Boolean).at(-1), resultCodePattern, 'API 24 instrumentation output must end with its completion record');
    assert.deepEqual(
        events.map(({ code, fields: event }) => ({
            code,
            class: event.class,
            test: event.test,
            id: event.id,
            numtests: Number(event.numtests),
            current: Number(event.current),
        })),
        [testStarted, testPassed].map((code) => ({
            code,
            class: testClass,
            test: testMethod,
            id: testRunnerId,
            numtests: expectedTestCount,
            current: expectedTestCount,
        })),
        'API 24 smoke must start and pass the existing test, without failures, skips, or unexpected tests'
    );
    const summaries = lines.filter((line) => /^OK \(\d+ tests?\)$/.test(line));
    assert.deepEqual(summaries, [`OK (${expectedTestCount} test)`], 'API 24 smoke must report exactly one passing test');
    assert.doesNotMatch(output, /INSTRUMENTATION_FAILED|INSTRUMENTATION_ABORTED|INSTRUMENTATION_STATUS: Error=|INSTRUMENTATION_RESULT: shortMsg=|FAILURES!!!/,
        'API 24 instrumentation must not report an aborted or failed run');
    assert.doesNotMatch(output, adbErrorPattern, 'API 24 instrumentation must not report an adb transport error');
}

function run(command, args, { cwd = repoRoot, capture = false, timeout, outputFile } = {}) {
    const result = spawnSync(command, args, {
        cwd,
        encoding: 'utf8',
        stdio: capture ? ['ignore', 'pipe', 'pipe'] : 'inherit',
        timeout,
    });
    if (outputFile) {
        mkdirSync(dirname(outputFile), { recursive: true });
        writeFileSync(outputFile, (result.stdout ?? '') + (result.stderr ?? ''));
    }
    if (result.error || result.status !== 0) {
        if (result.stdout) process.stdout.write(result.stdout);
        if (result.stderr) process.stderr.write(result.stderr);
        if (result.error) throw new Error(`${command} could not complete: ${result.error.message}`, { cause: result.error });
        throw new Error(`${command} failed with ${result.signal ? `signal ${result.signal}` : `exit ${result.status}`}`);
    }
    if (result.stderr) {
        process.stderr.write(result.stderr);
        assert.doesNotMatch(result.stderr, adbErrorPattern, `${command} must not report an adb transport error`);
    }
    return result.stdout;
}

function runSmoke() {
    const sdkRoot = process.env.ANDROID_HOME ?? process.env.ANDROID_SDK_ROOT ?? join(homedir(), 'Library/Android/sdk');
    const sdkAdb = join(sdkRoot, 'platform-tools', process.platform === 'win32' ? 'adb.exe' : 'adb');
    const adb = existsSync(sdkAdb) ? sdkAdb : 'adb';
    const requestedSerial = process.env.ANDROID_SERIAL ?? process.env.ANDROID_DEVICE_ID
        ?? (process.env.EMULATOR_PORT ? `emulator-${process.env.EMULATOR_PORT}` : undefined);
    let serial = requestedSerial;
    if (!serial) {
        const devices = run(adb, ['devices'], { capture: true, timeout: adbOperationTimeoutMs })
            .split('\n').map((line) => line.match(/^(\S+)\s+device\s*$/)?.[1]).filter(Boolean);
        assert.equal(devices.length, requiredDeviceCount, 'API 24 smoke requires exactly one connected device, or ANDROID_SERIAL');
        [serial] = devices;
    }
    const adbArgs = ['-s', serial];
    const sdk = run(adb, [...adbArgs, 'shell', 'getprop', 'ro.build.version.sdk'], {
        capture: true,
        timeout: adbOperationTimeoutMs,
    }).trim();
    assert.equal(sdk, requiredSdk, `API 24 smoke must run on SDK ${requiredSdk}; device ${serial} reports ${sdk}`);
    console.log(`Running API 24 smoke on ${serial} (SDK ${sdk}).`);

    run('npm', ['run', 'prebuild:example:android']);
    run('./gradlew', [':apollohg_react-native-rich-text-editor:assembleDebugAndroidTest'], {
        cwd: join(repoRoot, 'example/android'),
    });
    const apkDirectory = join(repoRoot, 'android/build/outputs/apk/androidTest/debug');
    const metadata = JSON.parse(readFileSync(join(apkDirectory, 'output-metadata.json'), 'utf8'));
    assert.equal(metadata.applicationId, testApplicationId, 'API 24 smoke must install the current instrumentation package');
    assert.equal(metadata.elements.length, requiredApkCount, 'API 24 smoke expects the universal instrumentation APK');
    assert.equal(metadata.elements[0].type, 'SINGLE', 'API 24 smoke must install the complete instrumentation APK');
    assert.deepEqual(metadata.elements[0].filters, [], 'API 24 smoke must not narrow the APK ABIs');
    run(adb, [...adbArgs, 'install', '--no-streaming', '-r', '-t', join(apkDirectory, metadata.elements[0].outputFile)], {
        timeout: adbOperationTimeoutMs,
    });
    const output = run(adb, [...adbArgs, 'shell', 'am', 'instrument', '-w', '-r', '-e', 'class', testClass, testRunner], {
        capture: true,
        timeout: adbOperationTimeoutMs,
        outputFile: join(repoRoot, 'android/build/outputs/androidTest-results/api24/instrumentation.txt'),
    });
    process.stdout.write(output);
    requireApi24SmokeSuccess(output);
    console.log('API 24 editor and viewer smoke passed: 1 test, 0 failures, 0 skips.');
}

if (process.argv[1] && import.meta.url === pathToFileURL(realpathSync(process.argv[1])).href) runSmoke();
