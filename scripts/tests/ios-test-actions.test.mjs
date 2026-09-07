import assert from 'node:assert/strict';
import { copyFileSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join, dirname } from 'node:path';
import { spawnSync } from 'node:child_process';

const root = mkdtempSync(join(tmpdir(), 'ios-test-actions-'));
try {
    for (const file of ['scripts/run-ios-tests.sh', 'scripts/require-native-artifacts.sh']) {
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
} finally {
    rmSync(root, { recursive: true, force: true });
}
console.log('iOS build and test actions preserve destinations, test filters, and derived data paths.');
