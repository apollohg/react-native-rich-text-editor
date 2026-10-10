import { execFileSync } from 'node:child_process';
import { existsSync, readFileSync, readdirSync, writeFileSync } from 'node:fs';
import path from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';
import { parseJson, readJsonFile } from './lib/json-file.mjs';

const repoRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const cargoWrapper = path.join(repoRoot, 'rust/toolchain-cargo.sh');
const configPath = 'scripts/notices/config.json';
const cargoOutputLimit = 32 * 1024 * 1024;
const minimumFenceLength = 4;
const cratesIoSource = 'registry+https://github.com/rust-lang/crates.io-index';
const noticeFilePattern = /^(?:licen[sc]e|copying|copyright|notice|unlicense)(?:$|[._-])/i;
const excludedDirectories = new Set(['.git', 'target', 'node_modules']);
const shippedTargets = [
    'aarch64-apple-ios',
    'aarch64-apple-ios-sim',
    'x86_64-apple-ios',
    'aarch64-linux-android',
    'armv7-linux-androideabi',
    'i686-linux-android',
    'x86_64-linux-android',
];

function compare(left, right) {
    return left < right ? -1 : left > right ? 1 : 0;
}

function packageKey(pkg) {
    return `${pkg.name}@${pkg.version}`;
}

function cargo(args, manifestPath, offline) {
    return execFileSync('bash', [
        cargoWrapper, ...args, '--manifest-path', manifestPath, '--locked',
        ...(offline ? ['--offline'] : []),
    ], {
        cwd: path.dirname(manifestPath),
        encoding: 'utf8',
        maxBuffer: cargoOutputLimit,
        stdio: ['ignore', 'pipe', 'inherit'],
    });
}

export function collectRustPackages(manifestPath, { offline = false } = {}) {
    const metadata = parseJson(cargo(['metadata', '--format-version', '1'], manifestPath, offline), 'Cargo metadata');
    const packagesByKey = new Map();
    for (const pkg of metadata.packages) {
        const key = packageKey(pkg);
        const matches = packagesByKey.get(key) ?? [];
        matches.push(pkg);
        packagesByKey.set(key, matches);
    }
    const selected = new Map();
    for (const target of shippedTargets) {
        const tree = cargo([
            'tree', '--target', target, '--edges', 'normal,build',
            '--prefix', 'none', '--format', '{p}', '--color', 'never',
        ], manifestPath, offline);
        for (const line of tree.trim().split('\n')) {
            const match = /^(\S+) v(\S+)(?: \((.*?)\))?(?: \(\*\))?$/.exec(line);
            if (!match) throw new Error(`Unrecognised Cargo tree entry for ${target}: ${line}`);
            const [, name, version, location] = match;
            const candidates = packagesByKey.get(`${name}@${version}`) ?? [];
            const matches = candidates.filter(pkg => pkg.source !== null || location === path.dirname(pkg.manifest_path));
            if (matches.length !== 1) {
                throw new Error(`Cannot uniquely resolve Cargo tree entry: ${line}`);
            }
            const pkg = matches[0];
            if (pkg.id !== metadata.resolve.root) selected.set(pkg.id, pkg);
        }
    }
    return [...selected.values()].sort((left, right) => compare(left.name, right.name) || compare(left.version, right.version));
}

function noticeFiles(directory) {
    const files = [];
    function visit(relativeDirectory, inLicenseDirectory = false) {
        for (const entry of readdirSync(path.join(directory, relativeDirectory), { withFileTypes: true })) {
            const relativePath = path.posix.join(relativeDirectory, entry.name);
            if (entry.isDirectory() && !excludedDirectories.has(entry.name)) {
                visit(relativePath, inLicenseDirectory || /^(?:licenses|licences)$/i.test(entry.name));
            } else if (entry.isFile() && (inLicenseDirectory || noticeFilePattern.test(entry.name))) {
                files.push(relativePath);
            }
        }
    }
    visit('');
    return files;
}

function fencedText(text) {
    const normalized = text.replace(/\r\n?/g, '\n').trimEnd();
    const runs = normalized.match(/`+/g) ?? [];
    const fence = '`'.repeat(Math.max(minimumFenceLength, ...runs.map(run => run.length + 1)));
    return `${fence}text\n${normalized}\n${fence}`;
}

function crateSource(pkg, root, config) {
    const key = packageKey(pkg);
    if (pkg.source === cratesIoSource) {
        return {
            url: `https://crates.io/api/v1/crates/${pkg.name}/${pkg.version}/download`,
            label: 'published crate',
            fileUrl: file => `https://docs.rs/crate/${pkg.name}/${pkg.version}/source/${file.split('/').map(encodeURIComponent).join('/')}`,
            note: pkg.license === 'MPL-2.0'
                ? ' The corresponding source is available under MPL-2.0 at the published-crate link. This project uses the upstream crate without source modifications.'
                : '',
        };
    }
    const local = config.localSources?.[key];
    if (pkg.source !== null || !local || path.resolve(root, local.directory) !== path.dirname(pkg.manifest_path)) {
        throw new Error(`${key}: configure the corresponding local source or unsupported Cargo source ${pkg.source}`);
    }
    return {
        url: local.source,
        label: 'patched source in this repository',
        fileUrl: file => `${local.source}/${file.split('/').map(encodeURIComponent).join('/')}`,
        note: ' This project distributes a locally patched copy; the upstream published crate does not contain these changes.',
    };
}

export function renderCrate(pkg, root, config) {
    const key = packageKey(pkg);
    const source = crateSource(pkg, root, config);
    const override = config.overrides?.[key];
    if (override && override.license !== pkg.license) {
        throw new Error(`${key}: license expression changed; review the notice override`);
    }
    if (!pkg.license && !pkg.license_file) throw new Error(`${key}: missing license metadata`);
    const directory = path.dirname(pkg.manifest_path);
    const files = new Set(noticeFiles(directory));
    if (pkg.license_file) files.add(pkg.license_file);
    const notices = [...files].sort(compare).map(file => ({
        label: file,
        source: source.fileUrl(file),
        text: readFileSync(path.resolve(directory, file), 'utf8'),
    }));
    for (const file of override?.files ?? []) {
        notices.push({
            label: file.label ?? path.basename(file.path),
            source: file.source,
            text: readFileSync(path.resolve(root, file.path), 'utf8'),
        });
    }
    if (notices.length === 0 || notices.some(notice => !notice.text.trim())) {
        throw new Error(`${key}: missing or empty license text; add a version-specific override in ${configPath}`);
    }
    const repository = pkg.repository ? `; [upstream repository](${pkg.repository})` : '';
    return [
        `### ${pkg.name} ${pkg.version}`,
        `License expression: \`${pkg.license ?? `LicenseRef-${pkg.name}`}\`.`,
        `Source: [${source.label}](${source.url})${repository}.${source.note}${override?.note ? ` ${override.note}` : ''}`,
        ...notices.map(notice => `[${notice.label}](${notice.source})\n\n${fencedText(notice.text)}`),
    ].join('\n\n');
}

export function writeNotices(outputs, { check = false } = {}) {
    const stale = [...outputs].filter(([file, text]) => !existsSync(file) || readFileSync(file, 'utf8') !== text);
    if (check && stale.length > 0) {
        throw new Error(`Third-party notices are stale or missing:\n${stale.map(([file]) => `  ${file}`).join('\n')}\nRun npm run notices:generate.`);
    }
    if (!check) {
        for (const [file, text] of stale) writeFileSync(file, text);
    }
}

function generateNotices({ check, offline }) {
    const config = readJsonFile(path.join(repoRoot, configPath), 'notices config');
    const preamble = readFileSync(path.join(repoRoot, 'scripts/notices/preamble.md'), 'utf8').trimEnd();
    const toolchain = readFileSync(path.join(repoRoot, 'rust/toolchain.sh'), 'utf8');
    const rustVersion = /^RUST_TOOLCHAIN_VERSION="([^"]+)"$/m.exec(toolchain)?.[1];
    if (rustVersion !== config.standardLibraryVersion) {
        throw new Error('Rust toolchain changed; refresh the standard-library notices and standardLibraryVersion in scripts/notices/config.json');
    }
    const outputs = new Map();
    for (const scope of config.packages) {
        console.log(`Reading locked Rust dependencies for ${scope.output}...`);
        const packages = collectRustPackages(path.join(repoRoot, scope.manifest), { offline });
        for (const [name, version] of Object.entries(scope.reviewedAssetVersions ?? {})) {
            if (!packages.some(pkg => pkg.name === name && pkg.version === version)) {
                throw new Error(`${name}: bundled assets changed; review ${scope.supplement} and reviewedAssetVersions`);
            }
        }
        const inventory = packages.map(pkg => {
            const source = crateSource(pkg, repoRoot, config);
            return `| \`${pkg.name}\` | \`${pkg.version}\` | ${pkg.license ?? `LicenseRef-${pkg.name}`} | [Source](${source.url}) |`;
        });
        const sections = packages.map(pkg => renderCrate(pkg, repoRoot, config));
        outputs.set(path.join(repoRoot, scope.output), [
            '# Third-party notices',
            '<!-- Generated by npm run notices:generate. Edit scripts/notices/ inputs, not this file. -->',
            preamble,
            '## Package scope',
            scope.description,
            '## Rust dependencies',
            'This inventory is the union of the locked normal and build dependency graphs for all seven shipped iOS/Android Rust targets. Build dependencies and procedural macros are included conservatively; test-only and optional CLI-only tools are not represented as shipped runtime code. Upstream alternative license texts are retained as supplied; their presence does not convert an OR choice into an AND requirement.',
            `The Rust standard library and compiler runtime notices are supplied separately in [RUST-STANDARD-LIBRARY-NOTICES.html](RUST-STANDARD-LIBRARY-NOTICES.html), copied from the pinned Rust ${rustVersion} distribution.`,
            ['| Crate | Version | Upstream license expression | Corresponding source |', '| --- | --- | --- | --- |', ...inventory].join('\n'),
            ...sections,
            readFileSync(path.join(repoRoot, scope.supplement), 'utf8').trimEnd(),
        ].join('\n\n') + '\n');
    }
    writeNotices(outputs, { check });
    console.log(check ? 'Third-party notices are up to date.' : 'Generated both third-party notices files.');
}

if (process.argv[1] && import.meta.url === pathToFileURL(path.resolve(process.argv[1])).href) {
    const args = process.argv.slice(2);
    const unknown = args.filter(arg => !['--check', '--offline'].includes(arg));
    if (unknown.length > 0) throw new Error(`Unknown arguments: ${unknown.join(' ')}. Usage: node scripts/generate-third-party-notices.mjs [--check] [--offline]`);
    generateNotices({ check: args.includes('--check'), offline: args.includes('--offline') });
}
