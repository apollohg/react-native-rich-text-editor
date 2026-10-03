import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { createReadStream } from 'node:fs';
import { readFile, readdir } from 'node:fs/promises';
import { createInterface } from 'node:readline';
import { fileURLToPath } from 'node:url';
import { lostSourceCells } from './convergence-report.js';

export const SUITE_CASES: Record<string, readonly string[]> = {
    plumbing: ['plumbing', 'scheduler', 'dependencies'],
    convergence: [
        'convergence',
        'projection-properties',
        'presentation-semantics',
        'scenario-evidence',
        'editing-continuity',
        'core-gate',
        'checkpoint-report',
        'checkpoint-io',
    ],
    differential: ['differential'],
    'checkpoint-regressions': [
        'availability',
        'text-history',
        'continuation-runner',
        'supplementary-continuity',
        'checkpoint-runner',
    ],
};

export async function provenance() {
    const sources: Record<string, string> = {};
    const collect = async (directory: string, recursive = false) => {
        for (const entry of await readdir(new URL(directory, import.meta.url), {
            withFileTypes: true,
        })) {
            const path = `${directory}${entry.name}`;
            if (entry.isDirectory() && recursive)
                await collect(`${path}/`, true);
            else if (
                entry.isFile() &&
                /\.(?:ts|rs|json|html|toml|lock)$/.test(entry.name)
            )
                sources[path.replace(/^\.\//, '')] = createHash('sha256')
                    .update(await readFile(new URL(path, import.meta.url)))
                    .digest('hex');
        }
    };
    for (const directory of [
        './',
        './browser/',
        './cases/',
        '../../rust/editor-core/',
    ])
        await collect(directory);
    await collect('../../rust/editor-core/src/', true);
    await collect('../../rust/editor-core/examples/', true);
    const path =
        process.env['RUST_PEER_EXECUTABLE'] ??
        fileURLToPath(
            new URL(
                '../../rust/editor-core/target/debug/examples/table_interop_peer',
                import.meta.url,
            ),
        );
    const sha256 = createHash('sha256')
        .update(await readFile(path))
        .digest('hex');
    return { node: process.version, sources, nativePeer: { path, sha256 } };
}

export async function verifyProvenance(
    expected: Awaited<ReturnType<typeof provenance>>,
): Promise<void> {
    assert.deepEqual(
        await provenance(),
        expected,
        'checkpoint source/native provenance mismatch',
    );
}

export async function* jsonLines<T>(path: string): AsyncGenerator<T> {
    const lines = createInterface({
        input: createReadStream(path),
        crlfDelay: Infinity,
    });
    try {
        for await (const line of lines) {
            assert.ok(line.length > 0, `empty evidence record in ${path}`);
            yield JSON.parse(line) as T;
        }
    } finally {
        lines.close();
    }
}

export const ADMISSION_CASES = [
    'colspan below the span domain',
    'negative colspan',
    'colspan of the wrong attribute type',
    'fractional colspan',
    'colspan beyond the update budget',
    'colwidth of the wrong attribute type',
    'a paragraph where a cell belongs',
    'loose text where a cell belongs',
    'an overlong rowspan',
    'a missing slot',
    'a span collision',
    'a width disagreement between rows',
    'a table with no rows',
] as const;
export const SOURCE_CASES = [
    'a regular two by two grid',
    'a ragged grid with a missing slot',
    'a grid with a spanning cell',
    'a grid with a rowspan longer than the table',
    'a grid with header cells',
] as const;

type SafetyObservation = {
    kind: string;
    name: string;
    classification?: string;
    admitted?: boolean;
    detail?: string;
    sourceCellAnchors?: number[];
    projectedSlots?: (number | null)[];
};
export function safetyResult(observations: readonly SafetyObservation[]) {
    const expected = [
        ...ADMISSION_CASES.map((name) => `admission:${name}`),
        ...SOURCE_CASES.map((name) => `source-coverage:${name}`),
    ];
    const keys = observations.map((row) => `${row.kind}:${row.name}`);
    let complete =
        keys.length === expected.length &&
        new Set(keys).size === keys.length &&
        expected.every((key) => keys.includes(key));
    let unsafeAdmissions = 0,
        unexpectedSourceCellLosses = 0;
    for (const row of observations) {
        if (row.kind === 'admission') {
            const index = (ADMISSION_CASES as readonly string[]).indexOf(
                row.name,
            );
            const classification = index < 8 ? 'unsafe' : 'admissibleIrregular';
            complete &&=
                index >= 0 &&
                row.classification === classification &&
                typeof row.admitted === 'boolean' &&
                typeof row.detail === 'string';
            if (row.classification === 'unsafe' && row.admitted)
                unsafeAdmissions++;
        } else if (
            row.kind === 'source-coverage' &&
            Array.isArray(row.sourceCellAnchors) &&
            Array.isArray(row.projectedSlots)
        ) {
            complete &&= row.sourceCellAnchors.length > 0;
            unexpectedSourceCellLosses += lostSourceCells({
                name: row.name,
                sourceCellAnchors: row.sourceCellAnchors,
                projectedSlots: row.projectedSlots,
            }).length;
        } else complete = false;
    }
    return { complete, unsafeAdmissions, unexpectedSourceCellLosses };
}

export type SuiteEvent = {
    type: string;
    data: {
        name?: string;
        file?: string;
        skip?: unknown;
        todo?: unknown;
        success?: boolean;
        counts?: Record<string, number>;
        [key: string]: unknown;
    };
};
export type SuiteResult = {
    suite: string;
    exitCode: number;
    files: readonly string[];
    events: SuiteEvent[];
};
export function suiteFilePassed(
    result: SuiteResult | undefined,
    file: string,
): boolean {
    if (!result?.files.includes(file)) return false;
    const events = result.events.filter((event) =>
        event.data.file?.endsWith(`/cases/${file}.test.ts`),
    );
    if (
        events.some(
            (event) =>
                event.type === 'test:fail' ||
                event.data.skip ||
                event.data.todo,
        )
    )
        return false;
    const summary = events
        .filter((event) => event.type === 'test:summary')
        .at(-1)?.data;
    return (
        events.some((event) => event.type === 'test:pass') &&
        summary?.success === true &&
        !!summary.counts &&
        summary.counts.tests! > 0 &&
        ['failed', 'cancelled', 'skipped', 'todo'].every(
            (key) => summary.counts![key] === 0,
        )
    );
}
export function suitePassed(result: SuiteResult): boolean {
    const expected = SUITE_CASES[result.suite];
    if (
        !expected ||
        JSON.stringify(result.files) !== JSON.stringify(expected) ||
        result.exitCode !== 0
    )
        return false;
    const passed = result.events.filter((event) => event.type === 'test:pass');
    if (
        result.events.some(
            (event) =>
                event.type === 'test:fail' ||
                event.data.skip ||
                event.data.todo,
        )
    )
        return false;
    const summary = result.events
        .filter((event) => event.type === 'test:summary')
        .at(-1)?.data;
    return (
        expected.every((file) =>
            passed.some((event) =>
                event.data.file?.endsWith(`/cases/${file}.test.ts`),
            ),
        ) &&
        summary?.success === true &&
        !!summary.counts &&
        summary.counts.tests! > 0 &&
        ['failed', 'cancelled', 'skipped', 'todo'].every(
            (key) => summary.counts![key] === 0,
        )
    );
}
