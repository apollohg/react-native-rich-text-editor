import assert from 'node:assert/strict';
import { appendFileSync } from 'node:fs';
import { canonicalDocumentShape } from './assertions.js';
import { assertEffectivePresentation } from './presentation-semantics.js';
import { scheduleProofPassed } from './convergence-report.js';
import {
    continuationCheckpoint,
    runSchedule,
    type ContinuationCheckpoint,
    type CorpusSchedule,
    type ScheduleOutcome,
} from './corpus.js';
import type { PresentationCheck } from './presentation-semantics.js';
import type { EffectiveDocument, PeerKind } from './peer-protocol.js';

export interface BaseResult {
    schedule: CorpusSchedule;
    outcome: Omit<ScheduleOutcome, 'peers'>;
    checkpoint?: ContinuationCheckpoint;
}

export function appendEvidence(path: string | undefined, value: unknown): void {
    if (path) appendFileSync(path, `${JSON.stringify(value)}\n`);
}

export async function runBaseSchedule(
    schedule: CorpusSchedule,
    observe?: (outcome: ScheduleOutcome) => void,
): Promise<BaseResult> {
    let checkpoint: ContinuationCheckpoint | undefined;
    const { peers: _peers, ...outcome } = await runSchedule(
        schedule,
        async (setup) => {
            observe?.(setup.baseline);
            checkpoint = await continuationCheckpoint(
                setup,
                'base-settled',
                schedule.seed,
                true,
                true,
            );
        },
        { webReference: schedule.topology === 'native/native' },
    );
    return {
        schedule: JSON.parse(JSON.stringify(schedule)) as CorpusSchedule,
        outcome,
        checkpoint,
    };
}

export function checkpointProof(
    checkpoint: ContinuationCheckpoint | undefined,
    kinds?: readonly PeerKind[],
) {
    const failures: string[] = [];
    let raw = false,
        presentation = false;
    let comparisons: PresentationCheck[] = [];
    try {
        assert.ok(
            checkpoint?.raw.passed,
            checkpoint?.raw.failure ?? 'missing raw proof',
        );
        const observations = checkpoint.textHistoryObservations;
        assert.ok(
            observations && observations.length >= 2,
            'missing captured participant raw observations',
        );
        if (kinds)
            assert.deepEqual(
                observations.map((observation) => observation.kind),
                kinds,
                'declared participant observations',
            );
        const first = canonicalDocumentShape(observations[0]!.raw);
        for (const observed of observations)
            assert.deepEqual(
                canonicalDocumentShape(observed.raw),
                first,
                'captured raw disagreement',
            );
        raw = true;
    } catch (error) {
        failures.push(`R: ${String(error)}`);
    }
    try {
        assert.ok(
            checkpoint?.presentation.passed,
            'missing or failed live presentation',
        );
        assert.deepEqual(checkpoint.presentation.failures, []);
        const views = checkpoint.presentation.views;
        assert.ok(
            views?.native.length && views.web.length,
            'missing independent presentation views',
        );
        const observations = checkpoint.textHistoryObservations;
        assert.ok(
            observations && observations.length >= 2,
            'missing presentation source observations',
        );
        if (kinds)
            assert.deepEqual(
                observations.map((observation) => observation.kind),
                kinds,
                'declared presentation participants',
            );
        assert.deepEqual(
            checkpoint.observations,
            observations.map(({ kind, document }) => ({ kind, document })),
            'source observation linkage',
        );
        const native = observations.filter((view) => view.kind === 'rust');
        const web = observations.filter((view) => view.kind !== 'rust');
        assert.equal(views.native.length, Math.max(1, native.length));
        assert.equal(views.web.length, Math.max(1, web.length));
        const withoutIdentity = (view: EffectiveDocument) => {
            const copy = structuredClone(view);
            for (const cell of copy.tables.flatMap((table) => table.cells))
                delete cell.sourceId;
            return copy;
        };
        native.forEach((view, index) =>
            assert.deepEqual(
                withoutIdentity(views.native[index]!),
                withoutIdentity(view.document),
                'native source linkage',
            ),
        );
        web.forEach((view, index) =>
            assert.deepEqual(
                views.web[index],
                view.document,
                'web source linkage',
            ),
        );
        comparisons = views.native
            .slice(1)
            .map((view) => assertEffectivePresentation(views.native[0]!, view));
        for (const web of views.web)
            for (const native of views.native)
                comparisons.push(assertEffectivePresentation(native, web));
        assert.deepEqual(
            comparisons,
            checkpoint.presentation.comparisons,
            'presentation replay',
        );
        for (const table of comparisons.flatMap((check) => check.tables))
            if (table.kind === 'overlap-fallback')
                assert.ok(
                    comparisons.some((check) =>
                        check.tables.some(
                            (other) =>
                                other.source === table.source &&
                                other.kind === 'overlap-fallback' &&
                                other.evidence === 'live-overlap',
                        ),
                    ),
                    'fresh overlap evidence',
                );
        presentation = true;
    } catch (error) {
        failures.push(`P: ${String(error)}`);
    }
    const safe =
        !!checkpoint &&
        checkpoint.drain.passed &&
        Number.isInteger(checkpoint.drain.rounds) &&
        checkpoint.drain.rounds >= 0 &&
        checkpoint.drain.rounds <= 100 &&
        Number.isInteger(checkpoint.drain.emitted) &&
        checkpoint.drain.emitted >= 0 &&
        checkpoint.drain.emitted <= 10000 &&
        checkpoint.nativeAutonomousRepairWrites === 0 &&
        checkpoint.observationFailures.length === 0 &&
        checkpoint.remoteBoundaries.every(
            (boundary) =>
                boundary.kind !== 'rust' ||
                (boundary.passes === 0 && boundary.autonomous === 0),
        );
    if (!safe)
        failures.push('safety: missing or failed checkpoint observations');
    return {
        raw,
        presentation,
        safe,
        comparisons,
        failures,
        passed: raw && presentation && safe,
    };
}

export function baseProof(result: BaseResult) {
    const proof = checkpointProof(
        result.checkpoint,
        result.schedule.kinds.slice(0, result.schedule.participants),
    );
    const raw = result.outcome.rawConvergence?.passed === true && proof.raw;
    const effects =
        result.outcome.evidence?.status === 'proven' &&
        result.outcome.evidence.failures.length === 0 &&
        result.outcome.evidence.actions.length > 0;
    return {
        ...proof,
        raw,
        effects,
        passed: proof.passed && scheduleProofPassed(result.outcome),
    };
}
