import assert from 'node:assert/strict';
import { mkdtempSync, mkdirSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import path from 'node:path';
import { test } from 'node:test';
import {
    collectRustPackages,
    renderCrate,
    writeNotices,
} from '../generate-third-party-notices.mjs';

function fixture(t) {
    const root = mkdtempSync(path.join(tmpdir(), 'native-editor-notices-'));
    t.after(() => rmSync(root, { recursive: true, force: true }));
    const write = (name, text) => {
        const target = path.join(root, name);
        mkdirSync(path.dirname(target), { recursive: true });
        writeFileSync(target, text);
        return target;
    };
    return { root, write };
}

test('Cargo inventory includes transitive/build/mobile dependencies and excludes dev, CLI, and desktop-only crates', (t) => {
    const { root, write } = fixture(t);
    const crate = (name, dependencies = '') => {
        write(`${name}/src/lib.rs`, '');
        return write(`${name}/Cargo.toml`, `[package]\nname = "${name}"\nversion = "1.0.0"\nedition = "2021"\nlicense = "MIT"\n${dependencies}`);
    };
    for (const name of ['shared', 'build-only', 'test-only', 'cli-only', 'android-only', 'ios-only', 'desktop-only']) {
        crate(name);
    }
    crate('normal', '[dependencies]\nshared = { path = "../shared" }\n');
    const manifest = crate('app', `
[dependencies]
normal = { path = "../normal" }
cli-only = { path = "../cli-only", optional = true }
[features]
cli = ["dep:cli-only"]
[build-dependencies]
build-only = { path = "../build-only" }
[dev-dependencies]
test-only = { path = "../test-only" }
[target.'cfg(target_os = "android")'.dependencies]
android-only = { path = "../android-only" }
[target.'cfg(target_os = "ios")'.dependencies]
ios-only = { path = "../ios-only" }
[target.'cfg(target_os = "windows")'.dependencies]
desktop-only = { path = "../desktop-only" }
`);
    write('app/Cargo.lock', `version = 4
${['android-only', 'build-only', 'cli-only', 'desktop-only', 'ios-only', 'shared', 'test-only'].map(name => `[[package]]\nname = "${name}"\nversion = "1.0.0"\n`).join('\n')}
[[package]]
name = "app"
version = "1.0.0"
dependencies = ["android-only", "build-only", "cli-only", "desktop-only", "ios-only", "normal", "test-only"]
[[package]]
name = "normal"
version = "1.0.0"
dependencies = ["shared"]
`);
    const lockBefore = readFileSync(path.join(root, 'app/Cargo.lock'), 'utf8');
    const packages = collectRustPackages(manifest, { offline: true });
    assert.deepEqual(packages.map(pkg => pkg.name), ['android-only', 'build-only', 'ios-only', 'normal', 'shared']);
    assert.equal(readFileSync(path.join(root, 'app/Cargo.lock'), 'utf8'), lockBefore, 'inventory must not modify the lockfile');
});

test('rendering retains nested notices, declared license files, and Markdown fences verbatim', (t) => {
    const { root, write } = fixture(t);
    const manifest_path = write('crate/Cargo.toml', '');
    write('crate/LICENSE-MIT', 'Copyright Alice\n````\nMIT text\n');
    write('crate/vendor/NOTICE', 'Copyright Bob\r\nNested attribution\r\n');
    write('crate/legal/terms.txt', 'Additional terms\n');
    const result = renderCrate({ name: 'demo', version: '1.2.3', license: 'MIT', license_file: 'legal/terms.txt', source: 'registry+https://github.com/rust-lang/crates.io-index', manifest_path }, root, {});
    assert.match(result, /`````text\nCopyright Alice\n````\nMIT text\n`````/);
    assert.match(result, /vendor\/NOTICE/);
    assert.match(result, /Copyright Bob\nNested attribution/);
    assert.doesNotMatch(result, /\r/, 'license line endings must be stable across platforms');
    assert.match(result, /Additional terms/);
    assert.match(result, /https:\/\/crates.io\/api\/v1\/crates\/demo\/1.2.3\/download/);
});

test('missing license text fails closed and fallback licenses apply only to the reviewed version', (t) => {
    const { root, write } = fixture(t);
    const pkg = { name: 'demo', version: '1.2.3', license: 'MIT', source: 'registry+https://github.com/rust-lang/crates.io-index', manifest_path: write('crate/Cargo.toml', '') };
    assert.throws(() => renderCrate(pkg, root, {}), /demo@1.2.3.*license/i);
    write('fallback.txt', 'Copyright Carol\nMIT terms\n');
    const config = { overrides: { 'demo@1.2.3': { license: 'MIT', files: [{ path: 'fallback.txt', source: 'https://example.org/pinned/LICENSE' }] } } };
    assert.match(renderCrate(pkg, root, config), /Copyright Carol/);
    assert.throws(() => renderCrate({ ...pkg, version: '1.2.4' }, root, config), /demo@1.2.4.*license/i);
    assert.throws(() => renderCrate({ ...pkg, license: 'Apache-2.0' }, root, config), /license.*changed/i);
});

test('patched crates link the distributed source and cannot be mistaken for unmodified registry crates', (t) => {
    const { root, write } = fixture(t);
    const pkg = { name: 'demo', version: '1.2.3', license: 'MIT', source: null, manifest_path: write('rust/vendor/demo/Cargo.toml', '') };
    write('rust/vendor/demo/LICENSE', 'Copyright Dave\nMIT terms\n');
    assert.throws(() => renderCrate(pkg, root, {}), /demo@1.2.3.*local source/i);
    const config = { localSources: { 'demo@1.2.3': { directory: 'rust/vendor/demo', source: 'https://example.org/repo/tree/main/rust/vendor/demo' } } };
    const result = renderCrate(pkg, root, config);
    assert.match(result, /patched/);
    assert.match(result, /https:\/\/example.org\/repo\/tree\/main\/rust\/vendor\/demo/);
    assert.doesNotMatch(result, /\[published crate\]|without source modifications/);
});

test('check mode reports missing and stale files without modifying any outputs', (t) => {
    const { root, write } = fixture(t);
    const first = write('first.md', 'stale');
    const second = path.join(root, 'second.md');
    const outputs = new Map([[first, 'first\n'], [second, 'second\n']]);
    assert.throws(() => writeNotices(outputs, { check: true }), /first.md[\s\S]*second.md/);
    assert.equal(readFileSync(first, 'utf8'), 'stale');
    writeNotices(outputs);
    assert.equal(readFileSync(second, 'utf8'), 'second\n');
    assert.doesNotThrow(() => writeNotices(outputs, { check: true }));
});
