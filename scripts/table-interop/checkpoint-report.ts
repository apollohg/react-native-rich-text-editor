import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import {
    CONVERGENCE_CORPUS,
    continuationDeclaration,
    continuationPassed,
    continuationRequirements,
    scenarioCoverage,
    structuralAvailabilityOutcome,
    textHistoryRequirements,
    type ContinuationResult,
    type ContinuationSlot,
    type ScenarioCoverage,
} from './corpus.js';
import { supplementaryRequirements } from './supplementary-continuity.js';
import { availabilityVerified } from './availability-evidence.js';
import { usableHistoryObligations } from './availability-history.js';
import {
    baseProof,
    checkpointProof,
    type BaseResult,
} from './checkpoint-evidence.js';
import { coreGatePassed, type CoreGateResult } from './convergence-report.js';
import type { PeerKind } from './peer-protocol.js';

const serializable = (value: unknown) => JSON.parse(JSON.stringify(value));
const digest = (value: unknown) =>
    createHash('sha256').update(JSON.stringify(value)).digest('hex');
type Coverage = {
    key: string;
    topology: string;
    preset: string;
    baseFamily: string;
    actor: number;
    actorKind: string;
    proof: string;
    required: boolean | null;
    status: string;
    accepted: boolean;
};

export class CheckpointReport {
    private readonly bases = new Map<string, ScenarioCoverage[]>();
    private readonly originals = [
        ...continuationRequirements(),
        ...supplementaryRequirements(),
    ];
    private readonly declarations = new Map(
        this.originals.map((slot) => [slot.key, slot]),
    );
    private readonly results = new Map<string, Coverage>();
    private readonly companions = new Map<string, Coverage>();
    private readonly unavailableHistory = new Set<string>();
    private readonly originalDigests = new Map<string, string>();
    private readonly linkedHistory = new Set<string>();
    private raw = true;
    private presentation = true;
    private effects = true;
    private safe = true;
    private geometry = true;
    private autonomous = 0;
    private nonQuiescent = 0;
    readonly findings: {
        key: string;
        topology: string;
        axis: string;
        reasons: unknown;
    }[] = [];
    readonly presentationKinds: Record<string, number> = {};
    readonly outcomes: Record<string, number> = {};
    private irregular = 0;
    private displayRawDifferences = 0;
    private successfulEdits = 0;
    private appliedStructuralHistory = 0;
    private appliedTextHistory = 0;

    private finding(
        key: string,
        topology: string,
        axis: string,
        reasons: unknown,
    ) {
        this.findings.push({ key, topology, axis, reasons });
    }

    addBase(result: BaseResult): void {
        const schedule = CONVERGENCE_CORPUS.find(
            (entry) => entry.name === result.schedule.name,
        );
        assert.ok(schedule, 'unknown base declaration');
        assert.deepEqual(
            result.schedule,
            serializable(schedule),
            'base declaration mismatch',
        );
        assert.ok(!this.bases.has(schedule.name), 'duplicate base result');
        const proof = baseProof(result);
        this.bases.set(
            schedule.name,
            scenarioCoverage(schedule, { ...result.outcome, peers: [] }),
        );
        this.raw &&= proof.raw;
        this.presentation &&= proof.presentation;
        this.effects &&= proof.effects;
        this.safe &&= proof.safe;
        this.geometry &&= result.outcome.geometry.kind === 'admitted';
        if (!proof.raw)
            this.finding(
                schedule.name,
                schedule.topology,
                'R',
                result.outcome.rawConvergence,
            );
        if (!proof.presentation)
            this.finding(schedule.name, schedule.topology, 'P', proof.failures);
        if (!proof.effects)
            this.finding(
                schedule.name,
                schedule.topology,
                'scenario-effect',
                result.outcome.evidence.failures,
            );
        if (
            result.outcome.geometry.kind === 'admitted' &&
            result.outcome.geometry.irregular
        )
            this.irregular++;
        this.chargeCheckpoints(
            schedule.name,
            schedule.topology,
            result.checkpoint ? [result.checkpoint] : [],
            result.outcome.nativeAutonomousRepairWrites,
            schedule.kinds.slice(0, schedule.participants),
        );
        if (
            result.outcome.geometry.kind === 'convergenceOracleFailed' &&
            /DRAIN_(?:ROUND|UPDATE)_LIMIT|NON_QUIESCENT/.test(
                result.outcome.geometry.message,
            )
        )
            this.nonQuiescent++;
    }

    private chargeCheckpoints(
        key: string,
        topology: string,
        checkpoints: ContinuationResult['checkpoints'],
        baselineRepairs = 0,
        kinds?: readonly PeerKind[],
    ) {
        let repairs = baselineRepairs;
        let nonQuiescent = false;
        for (const checkpoint of checkpoints) {
            const proof = checkpointProof(checkpoint, kinds);
            this.displayRawDifferences += checkpoint.displayRawDifferences ?? 0;
            this.raw &&= proof.raw;
            this.presentation &&= proof.presentation;
            this.safe &&= proof.safe;
            repairs = Math.max(
                repairs,
                checkpoint.nativeAutonomousRepairWrites,
            );
            nonQuiescent ||=
                !checkpoint.drain.passed ||
                checkpoint.drain.rounds > 100 ||
                checkpoint.drain.emitted > 10000;
            if (!proof.passed)
                this.finding(
                    key,
                    topology,
                    checkpoint.boundary,
                    proof.failures,
                );
            for (const table of proof.comparisons.flatMap(
                (check) => check.tables,
            )) {
                const kind =
                    table.kind === 'exact'
                        ? 'exact'
                        : `${table.kind}:${table.evidence}`;
                this.presentationKinds[kind] =
                    (this.presentationKinds[kind] ?? 0) + 1;
            }
        }
        this.autonomous += repairs;
        this.nonQuiescent += Number(nonQuiescent);
    }

    addContinuation(result: ContinuationResult): void {
        const slot = this.declarations.get(result.slot.key);
        assert.ok(slot, 'unknown continuation declaration');
        assert.deepEqual(
            continuationDeclaration(result.slot),
            continuationDeclaration(slot),
            'continuation declaration mismatch',
        );
        assert.ok(!this.results.has(slot.key), 'duplicate continuation result');
        if (slot.proof === 'history')
            this.originalDigests.set(slot.key, digest(result));
        const kinds = slot.schedule.kinds.slice(0, slot.schedule.participants);
        let accepted =
            continuationPassed(result) &&
            result.checkpoints.every(
                (checkpoint) => checkpointProof(checkpoint, kinds).passed,
            );
        if (['structure', 'history'].includes(slot.proof)) {
            const outcome = structuralAvailabilityOutcome(result);
            this.outcomes[outcome] = (this.outcomes[outcome] ?? 0) + 1;
            if (slot.proof === 'history' && availabilityVerified(result)) {
                this.unavailableHistory.add(slot.key);
            } else
                accepted &&=
                    outcome === 'successful-edit' ||
                    availabilityVerified(result);
        }
        this.results.set(slot.key, this.coverage(slot, result, accepted));
        if (accepted && result.disposition === 'edited') {
            this.successfulEdits++;
            if (slot.proof === 'history') this.appliedStructuralHistory++;
        }
        if (!accepted)
            this.finding(slot.key, slot.topology, 'C', {
                status: result.status,
                failures: result.failures,
                availability: result.availability,
                tracePath: result.tracePath,
            });
        this.raw &&=
            result.baseline?.rawConvergence.passed === true &&
            result.checkpoints.length > 0;
        this.effects &&=
            result.baseline?.evidence.status === 'proven' &&
            result.baseline.evidence.failures.length === 0;
        this.geometry &&= result.baseline?.geometry.kind === 'admitted';
        this.chargeCheckpoints(
            slot.key,
            slot.topology,
            result.checkpoints,
            result.baseline?.nativeAutonomousRepairWrites,
            kinds,
        );
        if (
            !result.checkpoints.some(
                (checkpoint) => !checkpoint.drain.passed,
            ) &&
            result.failures.some((failure) =>
                /TBL-21 NON_QUIESCENT/.test(failure),
            )
        )
            this.nonQuiescent++;
    }

    addCompanion(
        original: ContinuationResult,
        companion: ContinuationResult,
    ): void {
        const declared = this.declarations.get(original.slot.key);
        assert.ok(
            declared && this.results.has(declared.key),
            'companion requires observed original',
        );
        assert.equal(
            digest(original),
            this.originalDigests.get(declared.key),
            'companion original result mismatch',
        );
        const slot = textHistoryRequirements([declared])[0]!;
        assert.deepEqual(
            continuationDeclaration(companion.slot),
            continuationDeclaration(slot),
            'companion declaration mismatch',
        );
        assert.ok(!this.companions.has(slot.key), 'duplicate companion result');
        let accepted = false;
        try {
            usableHistoryObligations([declared], [original], [companion]);
            accepted = companion.checkpoints.every(
                (checkpoint) =>
                    checkpointProof(
                        checkpoint,
                        slot.schedule.kinds.slice(
                            0,
                            slot.schedule.participants,
                        ),
                    ).passed,
            );
            if (accepted) this.linkedHistory.add(declared.key);
        } catch (error) {
            this.finding(
                slot.key,
                slot.topology,
                'usable-history',
                String(error),
            );
        }
        this.companions.set(slot.key, this.coverage(slot, companion, accepted));
        if (accepted) this.appliedTextHistory++;
        this.chargeCheckpoints(
            slot.key,
            slot.topology,
            companion.checkpoints,
            companion.baseline?.nativeAutonomousRepairWrites,
            slot.schedule.kinds.slice(0, slot.schedule.participants),
        );
    }

    private coverage(
        slot: ContinuationSlot,
        result?: ContinuationResult,
        accepted = false,
    ): Coverage {
        return {
            key: slot.key,
            topology: slot.topology,
            preset: slot.preset,
            baseFamily: slot.baseFamily,
            actor: slot.actor,
            actorKind: slot.actorKind,
            proof: slot.proof,
            required: result?.required ?? slot.required,
            status:
                result?.status === 'proven' && !accepted
                    ? 'exercised-unproven'
                    : (result?.status ?? 'unexercised'),
            accepted,
        };
    }

    finish(prerequisites?: {
        plumbingPassed: boolean;
        differentialPassed: boolean;
        projectionPassed: boolean;
        safetyComplete: boolean;
        unsafeAdmissions: number;
        unexpectedSourceCellLosses: number;
    }) {
        const base = CONVERGENCE_CORPUS.flatMap(
            (schedule) =>
                this.bases.get(schedule.name) ?? scenarioCoverage(schedule),
        );
        const original = this.originals.map(
            (slot) => this.results.get(slot.key) ?? this.coverage(slot),
        );
        const histories = new Set([
            ...supplementaryRequirements()
                .filter(
                    (slot) =>
                        slot.family === 'overlap' && slot.proof === 'history',
                )
                .map((slot) => slot.key),
            ...this.unavailableHistory,
        ]);
        const companions = textHistoryRequirements(
            [...histories].map((key) => this.declarations.get(key)!),
        ).map((slot) => this.companions.get(slot.key) ?? this.coverage(slot));
        const exercised =
            this.bases.size === CONVERGENCE_CORPUS.length &&
            this.results.size === this.originals.length &&
            companions.every((row) => row.status !== 'unexercised');
        const complete =
            exercised &&
            base.every((row) => row.status === 'proven') &&
            original.every(
                (row) => row.accepted || this.linkedHistory.has(row.key),
            ) &&
            companions.every((row) => row.accepted);
        const gate: CoreGateResult = {
            plumbingPassed: prerequisites?.plumbingPassed === true,
            differentialPassed: prerequisites?.differentialPassed === true,
            projectionPassed:
                prerequisites?.projectionPassed === true && this.geometry,
            rawConvergencePassed: exercised && this.raw,
            presentationPassed: exercised && this.presentation,
            continuityPassed: complete && this.effects,
            coverageComplete:
                complete && this.safe && prerequisites?.safetyComplete === true,
            nativeAutonomousRepairWrites: this.autonomous,
            unsafeAdmissions: prerequisites?.unsafeAdmissions ?? NaN,
            nonQuiescentRuns: this.nonQuiescent,
            unexpectedSourceCellLosses:
                prerequisites?.unexpectedSourceCellLosses ?? NaN,
        };
        return {
            gate,
            passed: coreGatePassed(gate),
            findings: this.findings,
            diagnostics: {
                irregularBaseRuns: this.irregular,
                displayRawDifferences: this.displayRawDifferences,
                presentationKinds: this.presentationKinds,
            },
            outcomes: this.outcomes,
            usableHistories: [...this.linkedHistory],
            successfulEdits: this.successfulEdits,
            appliedStructuralHistory: this.appliedStructuralHistory,
            appliedTextHistory: this.appliedTextHistory,
            coverage: {
                base,
                continuations: original.slice(0, 4600),
                supplementary: original.slice(4600),
                companions,
            },
        };
    }
}
