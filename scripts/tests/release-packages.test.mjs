import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { releasePackages } from '../release-packages.mjs';

const core = { name: '@apollohg/react-native-rich-text-editor', version: '2.0.0', tarball: 'core.tgz' };
const addon = { name: `${core.name}-code-highlighting`, version: '2.0.0', tarball: 'addon.tgz', peerDependencies: { [core.name]: '2.0.0' } };
const published = new Map([[core.name, core.version]]);
const calls = [];
const options = {
    lookup: async name => published.get(name),
    publish: async item => { calls.push(item.name); published.set(item.name, item.version); },
};
await releasePackages([core, addon], options);
assert.deepEqual(calls, [addon.name]);
await releasePackages([core, addon], options);
assert.equal(calls.length, 1, 'retry must skip already published versions');
await assert.rejects(releasePackages([core, { ...addon, version: '2.1.0' }], options), /version/);
await assert.rejects(releasePackages([core, { ...addon, peerDependencies: { [core.name]: '1.0.0' } }], options), /peer/);
await assert.rejects(releasePackages([core, core], options), /Duplicate/);
await assert.rejects(releasePackages([core, addon], { ...options, lookup: async () => { throw Error('registry unavailable'); } }), /registry unavailable/);
assert.equal(calls.length, 1, 'failed preflight must never publish');
console.log('Coordinated release preflight and retry tests passed.');

const manifest = JSON.parse(readFileSync(new URL('../../package.json', import.meta.url)));
for (const dependencies of [manifest.dependencies, manifest.optionalDependencies]) {
    assert.equal(dependencies?.[addon.name], undefined, 'Core must not install the highlighting extension');
}
assert.ok(!manifest.files.some(file => file.startsWith('packages/')), 'Core must not bundle extension sources or binaries');
