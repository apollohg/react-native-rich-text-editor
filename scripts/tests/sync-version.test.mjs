import assert from 'node:assert/strict';
import { mkdir, mkdtemp, readFile, rm, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import path from 'node:path';
import { synchronizeVersion } from '../sync-version.mjs';

const fixture = await mkdtemp(path.join(tmpdir(), 'native-editor-sync-version-'));

async function write(relativePath, contents) {
  const target = path.join(fixture, relativePath);
  await mkdir(path.dirname(target), { recursive: true });
  await writeFile(target, contents);
}

try {
  await write('package-lock.json', '{"version":"1.0.0","packages":{"":{"version":"1.0.0"}}}\n');
  await write('example/package.json', '{"version":"1.0.0"}\n');
  await write('example/package-lock.json', '{"version":"1.0.0","packages":{"":{"version":"1.0.0"},"..":{"version":"1.0.0"},"../packages/code-highlighting":{"version":"1.0.0","peerDependencies":{"@apollohg/react-native-rich-text-editor":"1.0.0"}}}}\n');
  await write('rust/editor-core/Cargo.toml', '[package]\nname = "editor-core"\nversion = "1.0.0"\n');
  await write('rust/editor-core/Cargo.lock', '[[package]]\nname = "editor-core"\nversion = "1.0.0"\n');

  await write('packages/code-highlighting/package.json', JSON.stringify({ name: '@apollohg/react-native-rich-text-editor-code-highlighting', version: '2.0.0', peerDependencies: { '@apollohg/react-native-rich-text-editor': '>=2.0.0 <3' } }));
  synchronizeVersion(fixture, '3.0.0');
  const addon = JSON.parse(await readFile(path.join(fixture, 'packages/code-highlighting/package.json'), 'utf8'));
  assert.equal(addon.version, '3.0.0');
  assert.equal(addon.peerDependencies['@apollohg/react-native-rich-text-editor'], '3.0.0');

  const packageLock = JSON.parse(await readFile(path.join(fixture, 'package-lock.json'), 'utf8'));
  const examplePackage = JSON.parse(await readFile(path.join(fixture, 'example/package.json'), 'utf8'));
  const exampleLock = JSON.parse(await readFile(path.join(fixture, 'example/package-lock.json'), 'utf8'));
  const cargoToml = await readFile(path.join(fixture, 'rust/editor-core/Cargo.toml'), 'utf8');
  const cargoLock = await readFile(path.join(fixture, 'rust/editor-core/Cargo.lock'), 'utf8');

  assert.equal(packageLock.version, '3.0.0');
  assert.equal(packageLock.packages[''].version, '3.0.0');
  assert.equal(examplePackage.version, '3.0.0');
  assert.equal(exampleLock.version, '3.0.0');
  assert.equal(exampleLock.packages[''].version, '3.0.0');
  assert.equal(exampleLock.packages['..'].version, '3.0.0');
  assert.equal(exampleLock.packages['../packages/code-highlighting'].version, '3.0.0');
  assert.equal(exampleLock.packages['../packages/code-highlighting'].peerDependencies['@apollohg/react-native-rich-text-editor'], '3.0.0');
  assert.match(cargoToml, /^version = "3\.0\.0"$/m);
  assert.match(cargoLock, /name = "editor-core"\nversion = "3\.0\.0"/);
} finally {
  await rm(fixture, { recursive: true, force: true });
}

console.log('Version synchronization updates tracked package and Rust sources.');
