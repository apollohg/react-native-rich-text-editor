import assert from 'node:assert/strict';
import { copyFileSync, existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join, dirname } from 'node:path';
import { spawnSync } from 'node:child_process';

const root = mkdtempSync(join(tmpdir(), 'ios-test-actions-'));
try {
    for (const file of ['scripts/run-ios-tests.sh', 'scripts/run-ios-on-device.sh', 'scripts/require-native-artifacts.sh']) {
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
} finally {
    rmSync(root, { recursive: true, force: true });
}
console.log('iOS build and test actions preserve destinations, test filters, and derived data paths.');
