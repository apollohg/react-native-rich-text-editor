import assert from 'node:assert/strict';
import { copyFileSync, existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join, dirname } from 'node:path';
import { spawnSync } from 'node:child_process';

const root = mkdtempSync(join(tmpdir(), 'ios-test-actions-'));
try {
    for (const file of ['scripts/run-ios-tests.sh', 'scripts/run-ios-on-device.sh', 'scripts/require-native-artifacts.sh', 'scripts/prepare-ios-simulator.sh']) {
        mkdirSync(dirname(join(root, file)), { recursive: true });
        copyFileSync(new URL(`../../${file}`, import.meta.url), join(root, file));
    }
    for (const file of ['ios/EditorCore.xcframework/ios-arm64/libeditor_core.a', 'ios/EditorCore.xcframework/ios-arm64_x86_64-simulator/libeditor_core.a', 'ios-tests/NativeEditorTests.xcodeproj/project.pbxproj']) {
        mkdirSync(dirname(join(root, file)), { recursive: true });
        writeFileSync(join(root, file), 'fixture');
    }
    mkdirSync(join(root, 'ios-tests/NativeEditorTests.xcworkspace'), { recursive: true });
    mkdirSync(join(root, 'bin'));
    writeFileSync(join(root, 'bin/xcodebuild'), '#!/bin/bash\nprintf "%s\\n" "$@" > "$CAPTURE"\n', { mode: 0o755 });
    for (const action of ['test', 'build-for-testing', 'test-without-building']) {
        const result = spawnSync('bash', [join(root, 'scripts/run-ios-tests.sh'), '-only-testing:NativeEditorTests/RenderBridgeTests'], {
            env: { ...process.env, PATH: `${root}/bin:${process.env.PATH}`, CAPTURE: join(root, 'args'), IOS_DESTINATION: 'platform=iOS Simulator,name=fixture', NATIVE_EDITOR_IOS_TEST_ACTION: action, NATIVE_EDITOR_IOS_DERIVED_DATA: join(root, 'derived') },
            encoding: 'utf8',
        });
        assert.equal(result.status, 0, result.stderr);
        const args = readFileSync(join(root, 'args'), 'utf8').trim().split('\n');
        assert.equal(args[0], action);
        assert.equal(args[args.indexOf('-derivedDataPath') + 1], join(root, 'derived'));
        assert.ok(args.includes('-only-testing:NativeEditorTests/RenderBridgeTests'));
    }
    const deviceRun = spawnSync(
        'bash',
        [
            join(root, 'scripts/run-ios-on-device.sh'),
            '--class',
            'TablePerformanceTests',
            '--configuration',
            'Release',
        ],
        {
            cwd: root,
            env: {
                ...process.env,
                PATH: `${root}/bin:${process.env.PATH}`,
                CAPTURE: join(root, 'args'),
                IOS_DEVICE_ID: 'fixture-device',
                IOS_DEVELOPMENT_TEAM: 'fixture-team',
                IOS_DESTINATION: 'platform=iOS,id=fixture-device',
            },
            encoding: 'utf8',
        }
    );
    assert.equal(deviceRun.status, 0, deviceRun.stderr);
    const deviceArgs = readFileSync(join(root, 'args'), 'utf8').trim().split('\n');
    assert.ok(deviceArgs.includes('-only-testing:NativeEditorTests/TablePerformanceTests'));
    assert.equal(deviceArgs[deviceArgs.indexOf('-configuration') + 1], 'Release');
    assert.equal(
        deviceArgs[deviceArgs.indexOf('-scheme') + 1],
        'NativeEditorPreparedProsePerformance'
    );

    const simulatorId = 'A3615395-EDA9-4A60-982A-D428BCC5719B';
    const otherSimulatorId = 'B548F91A-98D1-4F57-B9A6-42D802DC911F';
    writeFileSync(join(root, 'bin/xcrun'), `#!/bin/bash
case "$*" in
    'simctl list devices available') printf '%s\\n' "$SIMCTL_DEVICES" ;;
    'simctl boot '*|'simctl bootstatus '*) printf '%s\\n' "$*" >> "$SIMCTL_CALLS" ;;
    *) exit 1 ;;
esac
`, { mode: 0o755 });
    writeFileSync(join(root, 'bin/xcodebuild'), `#!/bin/bash
if [[ "$1" == '-showdestinations' ]]; then
    printf '%s\\n' "$XCODE_DESTINATIONS"
    printf '%s\\n' "$XCODE_DIAGNOSTICS" >&2
    exit "$DISCOVERY_STATUS"
fi
printf '%s\\n' "$@" > "$CAPTURE"
`, { mode: 0o755 });

    for (const cacheHit of [false, true]) {
        const githubEnv = join(root, `github-env-${cacheHit}`);
        const simctlCalls = join(root, `simctl-calls-${cacheHit}`);
        const actions = cacheHit
            ? ['test-without-building', 'test-without-building']
            : ['build-for-testing', 'test-without-building', 'test-without-building'];
        let selectedDestination = '';
        for (const [index, action] of actions.entries()) {
            const run = spawnSync('bash', [join(root, 'scripts/run-ios-tests.sh')], {
                env: {
                    ...process.env,
                    PATH: `${root}/bin:${process.env.PATH}`,
                    CAPTURE: join(root, 'args'),
                    GITHUB_ENV: githubEnv,
                    IOS_DESTINATION: selectedDestination,
                    IOS_SIMULATOR_NAME: '',
                    IOS_DEVICE_ID: '',
                    NATIVE_EDITOR_IOS_TEST_ACTION: action,
                    SIMCTL_CALLS: simctlCalls,
                    SIMCTL_DEVICES: `== Devices ==\n-- iOS 26.2 --\n    iPhone 16e (${simulatorId}) (${index === 0 ? 'Booted' : 'Shutdown'})`,
                    XCODE_DESTINATIONS: 'No eligible simulator destinations are currently reported.',
                    DISCOVERY_STATUS: '0',
                },
                encoding: 'utf8',
            });
            const context = `cacheHit=${cacheHit}, step=${index}, action=${action}`;
            assert.equal(run.status, 0, `${context}\n${run.stdout}\n${run.stderr}`);
            selectedDestination = existsSync(githubEnv) ? readFileSync(githubEnv, 'utf8')
                .trim().split('\n').findLast((line) => line.startsWith('IOS_DESTINATION='))
                ?.slice('IOS_DESTINATION='.length) : '';
            const args = readFileSync(join(root, 'args'), 'utf8').trim().split('\n');
            assert.equal(args[0], action, context);
            assert.equal(args[args.indexOf('-destination') + 1], `id=${simulatorId}`, context);
        }
        assert.equal(selectedDestination, `id=${simulatorId}`, 'Simulator destination must persist for subsequent GitHub Actions steps');
        const bootChecks = readFileSync(simctlCalls, 'utf8').trim().split('\n')
            .filter((call) => call === `simctl bootstatus ${simulatorId} -b`);
        assert.equal(bootChecks.length, actions.length, 'Each step must wait for the same simulator to boot');
    }
    const discoveryDiagnostic = 'Simulator runtime is unavailable for NativeEditorTests.';
    for (const discoveryStatus of ['0', '65']) {
        const run = spawnSync('bash', [join(root, 'scripts/run-ios-tests.sh')], {
            env: {
                ...process.env,
                PATH: `${root}/bin:${process.env.PATH}`,
                IOS_DESTINATION: '',
                IOS_SIMULATOR_NAME: '',
                IOS_DEVICE_ID: '',
                GITHUB_ENV: '',
                SIMCTL_DEVICES: '== Devices ==',
                XCODE_DESTINATIONS: 'No eligible simulator destinations are currently reported.',
                XCODE_DIAGNOSTICS: discoveryDiagnostic,
                DISCOVERY_STATUS: discoveryStatus,
            },
            encoding: 'utf8',
        });
        assert.notEqual(run.status, 0, 'Unavailable simulators must fail without running tests');
        assert.ok(run.stderr.includes(discoveryDiagnostic), `Discovery exit ${discoveryStatus} must retain Xcode diagnostics: ${run.stderr}`);
    }

    const ciWorkflow = readFileSync(new URL('../../.github/workflows/ci.yml', import.meta.url), 'utf8');
    const publishWorkflow = readFileSync(new URL('../../.github/workflows/publish.yml', import.meta.url), 'utf8');
    const simulatorStep = /- name: Prepare iOS simulator\s+id: ios-simulator\s+shell: bash\s+run: bash scripts\/prepare-ios-simulator\.sh/;
    assert.match(ciWorkflow, simulatorStep, 'CI must prepare an iOS simulator');
    const securityIosJob = publishWorkflow.split('    security-ios:\n')[1]?.split('\n    security-android:')[0];
    assert.ok(securityIosJob, 'Release validation must define the iOS security job');
    assert.match(securityIosJob, simulatorStep, 'Release validation must reuse the CI simulator selection');
    assert.match(securityIosJob, /IOS_DESTINATION: \$\{\{ steps\.ios-simulator\.outputs\.destination \}\}/);
    writeFileSync(join(root, 'bin/xcrun'), `#!/bin/bash
case "$*" in
    'simctl list runtimes available') printf '%s\\n' "$SIMCTL_RUNTIMES" ;;
    'simctl list devicetypes') printf '%s\\n' "$SIMCTL_DEVICE_TYPES" ;;
    'simctl list devices available') printf '%s\\n' "$SIMCTL_DEVICES" ;;
    'simctl create '*)
        printf '%s\\n' "$*" >> "$SIMCTL_CALLS"
        if [ "$#" -ne 4 ]; then
            printf 'Incompatible device\\n' >&2
            exit 147
        fi
        printf '%s\\n' "$SIMULATOR_ID"
        ;;
    'simctl boot '*|'simctl bootstatus '*) printf '%s\\n' "$*" >> "$SIMCTL_CALLS" ;;
    *) exit 1 ;;
esac
`, { mode: 0o755 });
    for (const [scenario, devices, expectedCreate] of [
        ['reuse an existing iPhone despite trailing whitespace', `== Devices ==\n-- iOS 27.0 --\n    iPhone 16 (${simulatorId}) (Shutdown) `, false],
        ['reuse the first available iPhone across workflows', `== Devices ==\n-- iOS 27.0 --\n    iPhone 17 Pro (${simulatorId}) (Shutdown)\n    iPhone 17 (${otherSimulatorId}) (Shutdown)`, false],
        ['create an iPhone using a compatible installed runtime', '== Devices ==', true],
    ]) {
        const fixtureName = scenario.replaceAll(' ', '-');
        const simctlCalls = join(root, `ci-simctl-calls-${fixtureName}`);
        const githubOutput = join(root, `ci-github-output-${fixtureName}`);
        const run = spawnSync('bash', [join(root, 'scripts/prepare-ios-simulator.sh')], {
            cwd: root,
            env: {
                ...process.env,
                PATH: `${root}/bin:${process.env.PATH}`,
                CAPTURE: join(root, 'args'),
                GITHUB_OUTPUT: githubOutput,
                SIMCTL_CALLS: simctlCalls,
                SIMULATOR_ID: simulatorId,
                SIMCTL_DEVICES: devices,
                SIMCTL_RUNTIMES: '== Runtimes ==\niOS 26.5 (26.5 - 23F77) - com.apple.CoreSimulator.SimRuntime.iOS-26-5\niOS 27.0 (27.0 - 24A434) - com.apple.CoreSimulator.SimRuntime.iOS-27-0\niOS 27.1 (27.1 - 24A94425) - com.apple.CoreSimulator.SimRuntime.iOS-27-1',
                SIMCTL_DEVICE_TYPES: 'iPhone 16 (com.apple.CoreSimulator.SimDeviceType.iPhone-16)',
            },
            encoding: 'utf8',
        });
        assert.equal(run.status, 0, `${scenario}\n${run.stdout}\n${run.stderr}`);
        assert.equal(readFileSync(githubOutput, 'utf8').trim(), `destination=id=${simulatorId}`, scenario);
        const calls = readFileSync(simctlCalls, 'utf8').trim().split('\n');
        assert.equal(calls.some((call) => call.startsWith('simctl create ')), expectedCreate, scenario);
        assert.ok(calls.includes(`simctl bootstatus ${simulatorId} -b`), scenario);
    }
} finally {
    rmSync(root, { recursive: true, force: true });
}
console.log('iOS build and test actions preserve destinations, test filters, and derived data paths.');
