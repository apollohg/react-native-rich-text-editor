import { continuationPassed, continuationRequirements } from './corpus.js';
import { executeContinuations } from './continuation-runner.js';
import { open, readFile, readdir, writeFile } from 'node:fs/promises';
import { createHash } from 'node:crypto';
import { fileURLToPath } from 'node:url';
import {
    supplementaryRequirements,
    runSupplementary,
    type SupplementarySlot,
} from './supplementary-continuity.js';

const args = process.argv.slice(2);
const supplementary = args[0] === '--supplementary';
if (supplementary) args.shift();
const slots = supplementary ? supplementaryRequirements() : continuationRequirements();
if (args[0] === '--manifest') {
    console.log(
        JSON.stringify(
            slots.map(
                ({
                    key,
                    topology,
                    preset,
                    baseFamily,
                    actor,
                    actorKind,
                    proof,
                    required,
                    status,
                }) => ({
                    key,
                    topology,
                    preset,
                    baseFamily,
                    actor,
                    actorKind,
                    proof,
                    required,
                    status,
                }),
            ),
        ),
    );
} else {
    const output = args[0] === '--output' ? args[1] : undefined;
    if (!output || (args[2] !== '--all' && args[2] !== '--keys'))
        throw new Error(
            'usage: run-continuations.ts --manifest | --output PATH (--all | --keys JSON_KEYS_PATH)',
        );
    let selected = slots;
    if (args[2] === '--keys') {
        const keys: unknown = JSON.parse(await readFile(args[3]!, 'utf8'));
        if (
            !Array.isArray(keys) ||
            keys.some((key) => typeof key !== 'string') ||
            new Set(keys).size !== keys.length
        )
            throw new Error('continuation keys must be unique strings');
        const found = new Set(keys);
        selected = slots.filter((slot) => found.has(slot.key));
        if (selected.length !== keys.length) throw new Error('unknown continuation keys');
    }
    const file = await open(output, 'wx');
    try {
        const sources: Record<string, string> = {};
        for (const directory of ['', 'browser/']) {
            for (const name of await readdir(new URL(`./${directory}`, import.meta.url))) {
                if (!name.endsWith('.ts') && !['package.json', 'package-lock.json'].includes(name))
                    continue;
                const path = `${directory}${name}`;
                sources[path] = createHash('sha256')
                    .update(await readFile(new URL(`./${path}`, import.meta.url)))
                    .digest('hex');
            }
        }
        const nativePath =
            process.env['RUST_PEER_EXECUTABLE'] ??
            fileURLToPath(
                new URL(
                    '../../rust/editor-core/target/debug/examples/table_interop_peer',
                    import.meta.url,
                ),
            );
        const sha256 = createHash('sha256')
            .update(await readFile(nativePath))
            .digest('hex');
        await writeFile(
            `${output}.provenance.json`,
            JSON.stringify({
                node: process.version,
                sources,
                nativePeer: { path: nativePath, sha256 },
            }),
            { flag: 'wx' },
        );
        await writeFile(`${output}.manifest.json`, JSON.stringify(selected), {
            flag: 'wx',
        });
        let executed = 0;
        let failed = 0;
        const summary = await executeContinuations(
            selected,
            supplementary ? (slot) => runSupplementary(slot as SupplementarySlot) : undefined,
            async (result) => {
                await file.write(JSON.stringify(result) + '\n');
                executed += 1;
                if (!continuationPassed(result)) {
                    failed += 1;
                    console.log(
                        JSON.stringify({
                            key: result.slot.key,
                            status: result.status,
                            failures: result.failures,
                            tracePath: result.tracePath,
                        }),
                    );
                }
                if (executed % 25 === 0)
                    console.log(
                        JSON.stringify({
                            executed,
                            failed,
                            declared: selected.length,
                        }),
                    );
            },
        );
        await writeFile(
            `${output}.summary.json`,
            JSON.stringify({ fullCandidates: slots.length, ...summary }),
            { flag: 'wx' },
        );
        console.log(JSON.stringify({ executed, failed, declared: selected.length }));
    } finally {
        await file.close();
    }
}
