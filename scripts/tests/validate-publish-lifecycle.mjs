import assert from 'node:assert/strict';
import { copyFile, mkdir, mkdtemp, readFile, rm, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import path from 'node:path';
import { spawnSync } from 'node:child_process';

const repoRoot = path.resolve(import.meta.dirname, '../..');
const packageJson = JSON.parse(await readFile(path.join(repoRoot, 'package.json'), 'utf8'));
const babelConfigSource = await readFile(path.join(repoRoot, 'babel.config.js'), 'utf8');
const jestConfigSource = await readFile(path.join(repoRoot, 'jest.config.cjs'), 'utf8');
const ciWorkflow = await readFile(path.join(repoRoot, '.github/workflows/ci.yml'), 'utf8');
const publishWorkflow = await readFile(path.join(repoRoot, '.github/workflows/publish.yml'), 'utf8');
const packedFixtureSource = await readFile(
  path.join(repoRoot, 'scripts/tests/validate-packed-package.test.mjs'),
  'utf8',
);
const packedValidatorSource = await readFile(
  path.join(repoRoot, 'scripts/validate-packed-package.sh'),
  'utf8',
);
const rn076ValidatorSource = await readFile(
  path.join(repoRoot, 'scripts/validate-android-rn076-consumer.sh'),
  'utf8',
);
const rn076ConsumerManifest = JSON.parse(
  await readFile(
    path.join(repoRoot, 'scripts/tests/android-rn076-consumer/package.json'),
    'utf8',
  ),
);
const distTagResolver = path.join(repoRoot, 'scripts/resolve-npm-dist-tag.mjs');

const workflowJob = (workflow, jobName, workflowName) => {
  const jobsSource = workflow.slice(workflow.search(/^jobs:\s*$/m));
  const jobIndent = jobsSource.match(/^(\s+)[a-z0-9-]+:\s*$/m)?.[1];
  assert.ok(jobIndent, `${workflowName} workflow must contain jobs`);
  const jobHeaders = [
    ...jobsSource.matchAll(new RegExp(`^${jobIndent}([a-z0-9-]+):\\s*$`, 'gm')),
  ];
  const index = jobHeaders.findIndex((match) => match[1] === jobName);
  assert.notEqual(index, -1, `${workflowName} workflow must define ${jobName}`);
  const start = jobHeaders[index].index;
  const end = jobHeaders[index + 1]?.index ?? jobsSource.length;
  return jobsSource.slice(start, end);
};
const requireJob = (jobName) => workflowJob(publishWorkflow, jobName, 'publish');
const requireCiJob = (jobName) => workflowJob(ciWorkflow, jobName, 'CI');

const ciPushTrigger = ciWorkflow.split('\n  push:\n')[1]?.split('\n  pull_request:')[0];
assert.ok(ciPushTrigger, 'CI must run after changes land on a release branch');
assert.match(ciPushTrigger, /^\s+- v1\s*$/m, 'CI must validate pushes to the default v1 release branch');

const assertRustCache = (job, jobName) => {
  assert.match(
    job,
    /uses: Swatinem\/rust-cache@v2/,
    `${jobName} must use the dependency-aware Rust cache`,
  );
  assert.match(
    job,
    /workspaces:\s*(?:\|\s*)?rust\/editor-core/,
    `${jobName} must cache the editor-core workspace`,
  );
  assert.match(
    job,
    /shared-key:\s*editor-core/,
    `${jobName} must share sanitized editor-core dependencies`,
  );
  assert.match(
    job,
    /cache-on-failure:\s*true/,
    `${jobName} must preserve warmed Rust dependencies after failures`,
  );
};

const assertGradleCache = (job, jobName, readOnlyExpression) => {
  assert.match(
    job,
    /uses: gradle\/actions\/setup-gradle@v5/,
    `${jobName} must use Gradle's rolling commit-aware cache`,
  );
  assert.match(
    job,
    new RegExp(`cache-read-only:\\s*${readOnlyExpression}`),
    `${jobName} must use the expected Gradle cache write policy`,
  );
};

assert.match(
  publishWorkflow,
  /^permissions:\s*\n\s+contents:\s*read\s*$/m,
  'workflow-level permissions must be read-only',
);
assert.doesNotMatch(
  `${ciWorkflow}\n${publishWorkflow}`,
  /actions\/cache(?:\/(?:restore|save))?@v4/,
  'workflow caches must not use the deprecated Node.js 20 action major',
);
assert.doesNotMatch(
  publishWorkflow,
  /actions\/(?:upload-artifact|download-artifact)@v4/,
  'release artifacts must not use deprecated Node.js 20 action majors',
);

const buildJob = requireJob('build-package');
assert.match(
  requireCiJob('ios'),
  /runs-on:\s*group: native-editor-ios\s+labels: \[self-hosted, macOS, ARM64\]/,
  'CI iOS must use the self-hosted Mac runner group',
);
assert.match(
  buildJob,
  /runs-on: &macos-runner\s+group: native-editor-ios\s+labels: \[self-hosted, macOS, ARM64\]/,
  'release builds must define the self-hosted Mac runner group',
);
for (const jobName of [
  'build-code-highlighting',
  'package-contracts',
  'security-ios',
  'ios-consumer-positive',
  'ios-consumer-negative',
]) {
  assert.match(requireJob(jobName), /runs-on: \*macos-runner/, `${jobName} must use the self-hosted Mac runner group`);
}
assert.doesNotMatch(
  `${ciWorkflow}\n${publishWorkflow}`,
  /runs-on:\s*macos-[^\s]+/,
  'macOS jobs must not use GitHub-hosted runners',
);
assert.match(buildJob, /cargo install cargo-ndk --version 4\.1\.2 --locked/);
assert.match(buildJob, /sdkmanager --install ['"]ndk;27\.1\.12297006['"]/);
const nativeRestore = buildJob.split('- name: Restore native build cache')[1]?.split('\n            - name:')[0];
assert.ok(nativeRestore, 'release builds must restore reusable native outputs');
assert.match(nativeRestore, /id: native-build-cache\s+uses: actions\/cache\/restore@v5/);
assert.match(nativeRestore, /key: native-build-v1-.*runner\.os.*runner\.arch.*rust-1\.95\.0.*ndk-4\.1\.2.*android-27\.1\.12297006.*steps\.native-toolchain\.outputs\.xcode.*hashFiles/);
assert.doesNotMatch(nativeRestore, /github\.sha|restore-keys:|release-artifact|\bdist\b/,
  'native cache must neither depend on the commit nor restore stale package outputs');
for (const input of [
  'rust/editor-core/src/**', 'rust/editor-core/*.toml', 'rust/editor-core/Cargo.lock',
  'rust/editor-core/build.rs', 'rust/editor-core/.cargo/**', 'rust/*.sh',
  'rust/vendor/yrs/**', 'rust/vendor/sha2/**',
  'rust/optimize-swift-bindings.py', 'rust/optimize-kotlin-bindings.py',
  '.cargo/**', 'rust/.cargo/**',
]) {
  assert.ok(nativeRestore.includes(`'${input}'`), `native cache must include ${input}`);
}
assert.match(buildJob, /xcodebuild -version/, 'native cache must identify the selected Xcode toolchain');
for (const stepName of [
  'Setup Java', 'Setup Rust', 'Install cargo-ndk',
  'Setup Android SDK', 'Setup Android NDK', 'Build editor-core for all shipping targets',
]) {
  assert.match(buildJob, new RegExp(
    `- name: ${stepName}\\s+if: steps\\.native-build-cache\\.outputs\\.cache-hit != 'true'`,
  ), `${stepName} must be skipped when matching native outputs are restored`);
}
for (const stepName of [
  'Setup Node', 'Install dependencies', 'Verify generated bindings are current',
  'Build package', 'Pack release artifact',
]) {
  const step = buildJob.split(`- name: ${stepName}`)[1]?.split('\n            - name:')[0];
  assert.ok(step, `${stepName} must remain in the release build`);
  assert.doesNotMatch(step, /\bif:/, `${stepName} must run even on a native cache hit`);
}
const nativeSave = buildJob.split('- name: Save native build cache')[1]?.split('\n            - name:')[0];
assert.ok(nativeSave, 'fresh native outputs must be cached');
assert.match(nativeSave, /if: steps\.native-build-cache\.outputs\.cache-hit != 'true'\s+uses: actions\/cache\/save@v5/);
assert.match(nativeSave, /key: \$\{\{ steps\.native-build-cache\.outputs\.cache-primary-key \}\}/);
const cachePaths = (step) => step.match(/path: \|([\s\S]*?)\n\s+key:/)?.[1].trim();
assert.equal(cachePaths(nativeRestore), cachePaths(nativeSave), 'native restore and save paths must match');
assert.ok(buildJob.indexOf('- name: Save native build cache') < buildJob.indexOf('- name: Build package'),
  'native outputs should survive later packaging failures');
assert.match(
  buildJob,
  /- name: Upload release artifact\s+uses: actions\/upload-artifact@v7/,
  'restored and freshly built outputs must both be uploaded for validation',
);
assert.match(buildJob, /release-artifact\/\*\.tgz/);

for (const jobName of ['rust', 'android-native', 'android']) {
  assertRustCache(requireCiJob(jobName), `CI ${jobName}`);
}

const localCargoTarget = /CARGO_TARGET_DIR: \$\{\{ github\.workspace \}\}\/\.\.\/\.cargo-target/;
const ciIosJob = requireCiJob('ios');
assert.match(ciIosJob, localCargoTarget, 'CI iOS must keep Cargo build outputs outside checkout cleanup');
assert.match(buildJob, /env: &macos-cargo-env\s+CARGO_TARGET_DIR: \$\{\{ github\.workspace \}\}\/\.\.\/\.cargo-target/,
  'release builds must define the persistent Mac Cargo target');
for (const jobName of ['build-code-highlighting', 'package-contracts']) {
  assert.match(requireJob(jobName), /env: \*macos-cargo-env/,
    `${jobName} must reuse the persistent Mac Cargo target`);
}
for (const [job, jobName] of [
  [ciIosJob, 'CI iOS'],
  [buildJob, 'build-package'],
  ...['build-code-highlighting', 'package-contracts'].map((name) => [requireJob(name), name]),
]) {
  assert.doesNotMatch(job, /Swatinem\/rust-cache@v2/,
    `${jobName} must not transfer persistent Cargo outputs through GitHub cache`);
}
const iosBuildCacheAction = await readFile(path.join(repoRoot, '.github/actions/ios-build-cache/action.yml'), 'utf8');
assert.match(iosBuildCacheAction, /CCACHE_DIR=\$HOME\/\.cache\/native-editor-ccache/);
assert.doesNotMatch(iosBuildCacheAction, /uses: actions\/cache@v5/,
  'persistent Mac compiler and CocoaPods caches must stay local');

const ciAggregateJob = requireCiJob('js-and-android');
assert.match(ciAggregateJob, /if: always\(\)/, 'The existing required check must report failures even when a dependency is skipped');
const ciDependencies = ciAggregateJob.match(/needs: \[([^\]]+)\]/)?.[1].split(',').map((name) => name.trim());
assert.deepEqual(ciDependencies, ['javascript', 'rust', 'android-native', 'android', 'android-rn076'],
  'The existing required check must cover every split JavaScript, Rust, and Android job');
const aggregateScript = ciAggregateJob.match(/run: \|\n([\s\S]+)/)?.[1];
assert.ok(aggregateScript, 'The aggregate check must validate dependency results');
for (const dependency of ciDependencies) {
  for (const outcome of ['success', 'failure', 'cancelled', 'skipped']) {
    const results = ciDependencies.map((name) => name === dependency ? outcome : 'success').join(',');
    const result = spawnSync('bash', ['-euo', 'pipefail', '-c', aggregateScript], {
      env: { ...process.env, RESULTS: results },
      encoding: 'utf8',
    });
    assert.equal(result.status === 0, outcome === 'success',
      `Aggregate check must propagate ${dependency}=${outcome}; results=${results}\n${result.stderr}`);
  }
}

for (const jobName of [
  'security-rust-typescript',
  'android-release-validation',
]) {
  assertRustCache(requireJob(jobName), jobName);
}

for (const jobName of [
  'package-contracts',
  'security-rust-typescript',
  'security-ios',
  'security-android',
  'ios-consumer-positive',
  'ios-consumer-negative',
  'android-consumer-positive',
  'android-consumer-negative',
  'android-release-validation',
]) {
  const job = requireJob(jobName);
  assert.match(
    job,
    /actions\/download-artifact@v8/,
    `${jobName} must download the release artifact`,
  );
}

const androidReleaseJob = requireJob('android-release-validation');
const ciAndroidApi24Job = requireCiJob('android-api-24');
assert.equal(packageJson.scripts?.['test:android:api24'], 'node scripts/run-android-api24-smoke.mjs',
  'API 24 validation must run the strict file-backed instrumentation gate');
const linuxKvmSetup =
  /- name: Enable KVM\s+(?:if: matrix\.check == 'api24'\s+)?run: \|\s+echo 'KERNEL=="kvm", GROUP="kvm", MODE="0666", OPTIONS\+="static_node=kvm"' \| sudo tee \/etc\/udev\/rules\.d\/99-kvm4all\.rules\s+sudo udevadm control --reload-rules\s+sudo udevadm trigger --name-match=kvm/;
assert.match(androidReleaseJob, /runs-on:\s*linux-runner/);
assert.match(androidReleaseJob, /timeout-minutes:\s*60/);
assert.match(androidReleaseJob, /sdkmanager --install ['"]ndk;27\.1\.12297006['"]/);
assert.match(androidReleaseJob, /npm run test:android/);
assert.match(androidReleaseJob, /npm run lint:android/);
assert.match(androidReleaseJob, /:apollohg_react-native-rich-text-editor:assembleRelease/);
assert.match(androidReleaseJob, /npm run validate:package:android:rn076/);
assert.match(androidReleaseJob, /reactivecircus\/android-emulator-runner@v2\.38\.0/);
assert.match(androidReleaseJob, /api-level:\s*24/);
assert.match(
  androidReleaseJob,
  linuxKvmSetup,
  'publish API 24 validation must enable KVM on Linux',
);
assert.match(
  ciAndroidApi24Job,
  /runs-on:\s*linux-runner/,
  'CI API 24 validation must use Linux',
);
assert.match(
  ciAndroidApi24Job,
  linuxKvmSetup,
  'CI API 24 validation must enable KVM on Linux',
);
for (const [job, jobName] of [
  [androidReleaseJob, 'publish API 24 validation'],
  [ciAndroidApi24Job, 'CI API 24 validation'],
]) {
  assert.match(job, /arch:\s*x86_64/, `${jobName} must use the accelerated x86_64 image`);
  assert.match(
    job,
    /- name: Validate API 24 instrumentation result handling\s+(?:if: matrix\.check == 'api24'\s+)?run: node scripts\/tests\/android-api24-instrumentation\.test\.mjs[ \t]*$/m,
    `${jobName} must run the negative instrumentation protocol fixtures`,
  );
  assert.match(
    job,
    /script: npm run test:android:api24[ \t]*$/m,
    `${jobName} must run the complete API 24 smoke gate`,
  );
}
assert.match(
  ciWorkflow,
  /RELEASE_TARBALL="\$tarball" npm run validate:package:android:rn076/,
  'CI must validate the exact packed artifact against React Native 0.76',
);

for (const jobName of [
  'security-android',
  'android-consumer-positive',
  'android-consumer-negative',
]) {
  const job = requireJob(jobName);
  assert.match(job, /android-actions\/setup-android@v4/);
  assert.match(job, /sdkmanager --install ['"]ndk;27\.1\.12297006['"]/);
  assertGradleCache(job, jobName, String.raw`\$\{\{ github\.event_name == 'pull_request' \}\}`);
}

assertGradleCache(
  requireCiJob('android'),
  'CI android',
  "\\$\\{\\{ github\\.event_name == 'pull_request' \\}\\}",
);
assertGradleCache(
  ciAndroidApi24Job,
  'CI android-api-24',
  "\\$\\{\\{ github\\.event_name == 'pull_request' \\}\\}",
);
assertGradleCache(androidReleaseJob, 'android-release-validation', String.raw`\$\{\{ github\.event_name == 'pull_request' \}\}`);
assert.doesNotMatch(
  `${ciWorkflow}\n${publishWorkflow}`,
  /key: gradle-(?:publish|android-release)-/,
  'manual immutable Gradle cache keys must not return',
);

const publishJob = requireJob('publish');
for (const dependency of [
  'build-package',
  'build-code-highlighting',
  'package-contracts',
  'security-rust-typescript',
  'security-ios',
  'security-android',
  'ios-consumer-positive',
  'ios-consumer-negative',
  'android-consumer-positive',
  'android-consumer-negative',
  'android-release-validation',
]) {
  assert.match(
    publishJob,
    new RegExp(`- ${dependency}\\b`),
    `publish must require ${dependency}`,
  );
}
assert.match(publishJob, /id-token:\s*write/);
assert.match(publishJob, /actions\/download-artifact@v8/);
assert.match(
  publishJob,
  /- name: Publish to npm\s+if: github\.event_name == 'release' && github\.event\.action == 'published'/,
  'real npm publishing must only run for a published GitHub release',
);
assert.match(
  publishJob,
  /- name: Validate npm artifact locally\s+if: github\.event_name != 'release'/,
  'manual workflow dispatches must validate the npm artifact locally',
);
assert.match(publishJob, /node scripts\/release-packages\.mjs release-artifact/);
assert.match(
  publishJob,
  /node scripts\/release-packages\.mjs release-artifact --dry-run/,
  'the manual publish rehearsal must inspect the exact release tarball without publishing it',
);
assert.doesNotMatch(
  publishJob,
  /npm publish "\$tarball" --dry-run/,
  'manual workflow dispatches must never enter npm publish',
);
assert.doesNotMatch(publishJob, /npm run (?:build|validate:package|build:rust)/);

assert.match(packedFixtureSource, /VALIDATE_PACKED_PACKAGE_GROUP/);
assert.match(packedFixtureSource, /ios-consumer/);
assert.match(packedFixtureSource, /android-consumer/);
assert.match(packedValidatorSource, /--validate-packed-tarball/);
assert.match(packedValidatorSource, /--validate-android-tarball/);
assert.match(packedValidatorSource, /validate-android-rn076-consumer\.sh/);
assert.equal(rn076ConsumerManifest.dependencies?.['react-native'], '0.76.9');
assert.equal(rn076ConsumerManifest.dependencies?.react, '18.3.1');
assert.equal(rn076ConsumerManifest.dependencies?.expo, '~52.0.49');
assert.match(rn076ValidatorSource, /npm ci --ignore-scripts/);
assert.match(rn076ValidatorSource, /generate-codegen-artifacts\.js/);
assert.match(rn076ValidatorSource, /:app:assembleRelease/);
assert.match(rn076ValidatorSource, /-PnewArchEnabled=true/);
assert.match(rn076ValidatorSource, /-PreactNativeArchitectures=x86_64/);
for (const script of [
  'validate:package:contracts',
  'validate:package:ios:positive',
  'validate:package:ios:negative',
  'validate:package:android:positive',
  'validate:package:android:negative',
  'validate:package:android:rn076',
]) {
  assert.equal(
    typeof packageJson.scripts?.[script],
    'string',
    `package.json must define ${script}`,
  );
}
assert.match(
  packageJson.scripts['validate:package:android:positive'],
  /--validate-android-tarball \"\$RELEASE_TARBALL\"/,
  'positive Android validation must consume the exact release tarball',
);

for (const dependency of [
  'babel-preset-expo',
  'expo',
  'expo-modules-core',
  'react',
  'react-native',
]) {
  assert.equal(
    typeof packageJson.devDependencies?.[dependency],
    'string',
    `clean package builds must install ${dependency}`,
  );
}
assert.match(babelConfigSource, /require\.resolve\(['"]babel-preset-expo['"]\)/);
assert.doesNotMatch(
  babelConfigSource,
  /example\/node_modules/,
  'root tests must not depend on an example app install',
);
assert.doesNotMatch(
  jestConfigSource,
  /EXAMPLE_MODULES/,
  'Jest must resolve runtime modules from the root install',
);

for (const [version, expectedTag] of [
  ['1.0.0-alpha', 'alpha'],
  ['1.0.0-alpha.4', 'alpha'],
  ['1.0.0-beta.2', 'beta'],
  ['1.0.0-0', 'next'],
  ['1.0.0', 'latest'],
]) {
  const result = spawnSync(process.execPath, [distTagResolver, version], {
    cwd: repoRoot,
    encoding: 'utf8',
  });
  assert.equal(result.status, 0, result.stderr);
  assert.equal(result.stdout.trim(), expectedTag, `${version} must publish with the ${expectedTag} tag`);
}

const artifactOnlyResolver = await mkdtemp(path.join(tmpdir(), 'native-editor-dist-tag-'));
try {
  const artifactScripts = path.join(artifactOnlyResolver, 'scripts');
  const artifactResolver = path.join(artifactScripts, 'resolve-npm-dist-tag.mjs');
  await mkdir(artifactScripts);
  await copyFile(distTagResolver, artifactResolver);
  const result = spawnSync(process.execPath, [artifactResolver, '1.0.0-beta.2'], {
    cwd: artifactOnlyResolver,
    encoding: 'utf8',
  });
  assert.equal(result.status, 0, result.stderr);
  assert.equal(result.stdout.trim(), 'beta');
} finally {
  await rm(artifactOnlyResolver, { recursive: true, force: true });
}

assert.equal(
  packageJson.scripts?.['prepare:example:native'],
  'npm run prebuild:example && npm run install:example:pods',
  'native package validation must generate both example projects before installing pods',
);
assert.equal(
  packageJson.scripts?.['install:example:pods'],
  'cd example/ios && pod install --no-repo-update',
  'native package validation must install the generated example CocoaPods project',
);
assert.equal(
  packageJson.scripts?.['install:ios-test-pods'],
  'cd ios-tests && pod install --no-repo-update',
  'native package validation must install the iOS test workspace without updating spec repositories',
);
assert.match(
  packageJson.scripts?.['validate:package'] ?? '',
  /^npm run prepare:example:native && npm run install:ios-test-pods && /,
  'package validation must prepare generated native projects and the iOS test workspace before consuming them',
);

assert.equal(
  packageJson.scripts?.prepublishOnly,
  'npm run prepare:publish',
  'standard npm publish must invoke the complete release gate through prepublishOnly',
);

const fixture = await mkdtemp(path.join(tmpdir(), 'native-editor-publish-lifecycle-'));
try {
  await writeFile(
    path.join(fixture, 'package.json'),
    JSON.stringify({
      name: 'native-editor-publish-lifecycle-fixture',
      version: '1.0.0',
      scripts: {
        prepublishOnly: 'node fail-gate.mjs',
        postpublish: 'node publish-reached.mjs',
      },
    }),
  );
  await writeFile(path.join(fixture, 'fail-gate.mjs'), 'process.exit(23);\n');
  await writeFile(
    path.join(fixture, 'publish-reached.mjs'),
    "import { writeFileSync } from 'node:fs'; writeFileSync('publish-reached', 'yes');\n",
  );

  const result = spawnSync('npm', ['publish', '--dry-run', '--ignore-scripts=false'], {
    cwd: fixture,
    encoding: 'utf8',
  });
  assert.notEqual(result.status, 0, 'npm publish must fail when prepublishOnly fails');
  const marker = spawnSync('test', ['-e', path.join(fixture, 'publish-reached')]);
  assert.notEqual(marker.status, 0, 'npm publish must not advance beyond a failed release gate');
} finally {
  await rm(fixture, { recursive: true, force: true });
}

const highlightingJob = requireJob('build-code-highlighting');
assert.match(highlightingJob, /key: highlighting-native-v1-/);
assert.match(highlightingJob, /packages\/code-highlighting run test:rust/);
assert.match(publishWorkflow, /GRADLE_OPTS: -Dorg.gradle.caching=true/);
assert.match(androidReleaseJob, /check: \[jvm-lint, rn076, api24\]/);
for (const name of ['security-ios', 'ios-consumer-positive', 'ios-consumer-negative']) {
  assert.match(requireJob(name), /uses: \.\/\.github\/actions\/ios-build-cache/);
}
const androidBuildSource = await readFile(path.join(repoRoot, 'android/build.gradle'), 'utf8');
assert.match(androidBuildSource, /outputs\.cacheIf\s*\{\s*false\s*\}/, 'Fixture-driven JVM tests must execute instead of restoring cached results');
assert.match(androidBuildSource, /outputs\.upToDateWhen\s*\{\s*false\s*\}/, 'JVM tests must re-read external security fixtures');
const iosSecurityJob = requireJob('security-ios');
const iosCiJob = requireCiJob('ios');
const iosNativeSaveIndex = iosCiJob.indexOf('- name: Save iOS native outputs');
assert.ok(iosNativeSaveIndex > iosCiJob.indexOf('- name: Verify generated bindings are current'),
  'CI must save validated native outputs immediately, so later cancellation does not discard the build');
assert.ok(iosNativeSaveIndex < iosCiJob.indexOf('- name: Prepare iOS simulator'),
  'Native outputs must be saved before simulator startup and XCTest');
const iosNativeSave = iosCiJob.split('- name: Save iOS native outputs')[1]?.split(/\n\s+- name:/)[0];
assert.match(iosNativeSave, /uses: actions\/cache\/save@v5/);
assert.match(iosNativeSave, /key: \$\{\{ steps\.native-build-cache\.outputs\.cache-primary-key \}\}/);
assert.match(iosNativeSave, /path: \*ios-native-cache-paths/);
const iosTestHostAction = await readFile(path.join(repoRoot, '.github/actions/ios-test-host/action.yml'), 'utf8');
for (const [job, name] of [[iosCiJob, 'CI'], [iosSecurityJob, 'release']]) {
  assert.match(job, /timeout-minutes:\s*90/, `${name} must retain the full-suite time budget`);
  const prepareStep = job.split('- name: Prepare iOS test host')[1]?.split(/\n\s+- name:/)[0];
  assert.ok(prepareStep, `${name} must prepare the shared iOS test host`);
  assert.match(prepareStep, /uses: \.\/\.github\/actions\/ios-test-host/);
  assert.match(prepareStep, /xcode: \$\{\{ steps\.ios-toolchain\.outputs\.xcode \}\}/);
  assert.doesNotMatch(prepareStep, /\bif:|continue-on-error:/);
  const unitTestStep = job.split('- name: Run iOS unit tests')[1]?.split(/\n\s+- name:/)[0];
  assert.ok(unitTestStep, `${name} validation must run the complete iOS XCTest suite`);
  assert.match(unitTestStep, /^\s+run: npm run test:ios -- -resultBundlePath test-results\/ios\/NativeEditorTests\.xcresult\s*$/m,
    `${name} iOS tests must run without test filters`);
  assert.match(unitTestStep, /NATIVE_EDITOR_IOS_TEST_ACTION: test-without-building/);
  assert.doesNotMatch(unitTestStep, /\bif:|continue-on-error:/,
    `${name} must execute the full iOS suite on every host cache hit`);
  assert.ok(job.indexOf('- name: Prepare iOS test host') < job.indexOf('- name: Run iOS unit tests'));
}
const iosBuildStep = iosTestHostAction.split('- name: Build iOS test host')[1]?.split(/\n\s+- name:/)[0];
assert.ok(iosBuildStep, 'the shared action must build the complete iOS test host');
assert.match(iosBuildStep, /^\s+run: npm run test:ios\s*$/m, 'the cached host must not retain a test filter');
assert.match(iosBuildStep, /NATIVE_EDITOR_IOS_TEST_ACTION: build-for-testing/);
assert.match(iosBuildStep, /if: steps\.ios-test-build\.outputs\.cache-hit != 'true'/);
const iosTestRestore = iosTestHostAction.split('- name: Restore compiled iOS test host')[1]?.split(/\n\s+- name:/)[0];
assert.ok(iosTestRestore);
assert.match(iosTestRestore, /ios-test-host-v1-.*inputs\.xcode.*hashFiles/);
assert.doesNotMatch(iosTestRestore, /restore-keys:|github\.sha/);
for (const input of [
  'ios/**', 'ios-tests/**', 'scripts/tests/*.json', 'common/cpp/**', 'android/src/main/assets/editor-icons/**', 'ReactNativeProseEditor.podspec',
  'src/specs/**', 'react-native.config.js', 'package.json', 'package-lock.json',
  'example/package.json', 'example/package-lock.json', 'scripts/run-ios-tests.sh',
  'scripts/require-native-artifacts.sh', '.github/actions/ios-test-host/**', '.github/actions/ios-build-cache/**',
]) {
  assert.ok(iosTestRestore.includes(`'${input}'`), `iOS test host cache must include ${input}`);
}
assert.ok(iosTestHostAction.indexOf('- name: Restore compiled iOS test host') < iosTestHostAction.indexOf('- name: Install iOS test dependencies'),
  'Hash pristine test-project inputs before CocoaPods mutates the project');
const iosTestSave = iosTestHostAction.split('- name: Save compiled iOS test host')[1];
assert.ok(iosTestSave, 'the compiled host must be saved before tests can fail or be cancelled');
assert.match(iosTestSave, /uses: actions\/cache\/save@v5/);
assert.match(iosTestSave, /key: \$\{\{ steps\.ios-test-build\.outputs\.cache-primary-key \}\}/);
assert.equal(iosTestRestore.match(/path: ([^\n]+)/)?.[1], iosTestSave.match(/path: ([^\n]+)/)?.[1],
  'The compiled host must be saved and restored from the same directory');
assert.ok(iosTestHostAction.indexOf('- name: Save compiled iOS test host') > iosTestHostAction.indexOf('- name: Build iOS test host'));
assert.match(iosTestHostAction, /NATIVE_EDITOR_IOS_DERIVED_DATA=\$GITHUB_WORKSPACE\/\.ci\/ios-tests/);
assert.match(iosTestRestore, /path: \.ci\/ios-tests\/Build\/Products/);
const iosTestStep = iosSecurityJob.split('- name: Validate iOS security behavior')[1];
assert.match(iosTestStep, /NATIVE_EDITOR_IOS_TEST_ACTION: test-without-building/);
assert.doesNotMatch(iosTestStep, /\bif:/, 'iOS security tests must run on every cache hit');

assert.match(requireJob('package-contracts'), /node-version: 22/, 'Full Jest suite must use the supported Node 22 runtime');

console.log('npm publish lifecycle validation passed.');
