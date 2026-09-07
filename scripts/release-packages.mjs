import assert from 'node:assert/strict';
import { readdirSync } from 'node:fs';
import { resolve } from 'node:path';
import { pathToFileURL } from 'node:url';
import { execFileSync, spawnSync } from 'node:child_process';

export async function releasePackages(packages, { lookup, publish }) {
    const core = packages.find(item => item.name === '@apollohg/react-native-rich-text-editor');
    assert.ok(core, 'Release must include the core package');
    assert.equal(new Set(packages.map(item => item.name)).size, packages.length, 'Duplicate release package');
    for (const item of packages) {
        assert.equal(item.version, core.version, `Release version mismatch: ${item.name}`);
        if (item !== core) {
            assert.equal(item.peerDependencies?.[core.name], core.version, `Core peer version mismatch: ${item.name}`);
        }
    }
    const existing = await Promise.all(packages.map(item => lookup(item.name, item.version)));
    for (const [index, item] of packages.entries()) {
        if (existing[index] === item.version) {
            console.log(`Already published: ${item.name}@${item.version}`);
        } else {
            await publish(item);
        }
    }
}

if (process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) {
    const directory = resolve(process.argv[2] ?? 'release-artifact');
    const dryRun = process.argv.includes('--dry-run');
    const packages = readdirSync(directory).filter(name => name.endsWith('.tgz')).map(name => {
        const tarball = resolve(directory, name);
        return { ...JSON.parse(execFileSync('tar', ['-xOf', tarball, 'package/package.json'], { encoding: 'utf8' })), tarball };
    });
    assert.ok(packages.length >= 2, 'Release must contain core and extension artifacts');
    const tag = execFileSync(process.execPath, [new URL('./resolve-npm-dist-tag.mjs', import.meta.url).pathname, packages[0].version], { encoding: 'utf8' }).trim();
    await releasePackages(packages, {
        lookup: async (name, version) => {
            if (dryRun) return undefined;
            const result = spawnSync('npm', ['view', `${name}@${version}`, 'version', '--json'], { encoding: 'utf8' });
            if (result.status === 0) return JSON.parse(result.stdout);
            let error;
            try { error = JSON.parse(result.stdout).error; } catch {}
            if (error?.code === 'E404') return undefined;
            throw new Error(`Registry preflight failed for ${name}: ${result.stderr}`);
        },
        publish: async item => {
            const args = dryRun
                ? ['pack', item.tarball, '--dry-run', '--ignore-scripts', '--offline']
                : ['publish', item.tarball, '--ignore-scripts', '--access', 'public', '--tag', tag];
            const result = spawnSync('npm', args, { stdio: 'inherit' });
            assert.equal(result.status, 0, `npm ${args[0]} failed for ${item.name}`);
        },
    });
}
