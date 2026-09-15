import {
    continuationCheckpoint,
    continuationPassed,
    ContinuationExecutionError,
    type ContinuationResult,
} from './corpus.js';
import { withSupplementarySetup } from './supplementary-setup.js';
import type { SupplementarySlot } from './supplementary-continuity.js';
import {
    evidenceCall as call,
    observeEvidence,
    startEvidence,
    stopEvidence,
} from './evidence-observer.js';
import { cellText, realCells } from './scenario-evidence.js';
import { requireContinuity, typingCursor } from './continuity-evidence.js';
import {
    assertNativeNoRemoteLifetime,
    assertNativeRemoteLifetime,
    assertSourceRowInsertion,
    normalizationTarget,
} from './supplementary-evidence.js';
import { lastTrace, persistTrace } from './trace.js';

export async function runNativeLifetime(slot: SupplementarySlot): Promise<ContinuationResult> {
    const result: ContinuationResult = {
        slot,
        required: true,
        status: 'unexercised',
        disposition: 'unreached',
        checkpoints: [],
        actions: [],
        failures: [],
        dependencies: [],
    };
    await withSupplementarySetup(slot, async (setup) => {
        const { peers: _peers, ...baseline } = setup.baseline;
        result.baseline = baseline;
        const checkpoint = async (boundary: string, drain = true) => {
            const checked = await continuationCheckpoint(
                setup,
                boundary,
                slot.schedule.seed,
                drain,
            );
            result.checkpoints.push(checked);
            return checked;
        };
        const initial = await checkpoint('baseline', false);
        const capture = startEvidence(setup.participants);
        result.actions = capture.actions;
        try {
            requireContinuity(initial.raw.passed, 'lifetime raw setup convergence');
            requireContinuity(
                slot.actorKind === 'rust' && slot.proof === 'history',
                'native lifetime actor/proof',
            );
            const actor = setup.participants[slot.actor]!;
            const target = realCells(await observeEvidence(actor)).find(
                (cell) => cellText(cell.node) === slot.target,
            );
            requireContinuity(target?.sourceId, 'lifetime original source target');
            await call(actor, 'command', {
                type: 'addTableRow',
                side: 'after',
                at: target.position + 1,
            });
            const action = capture.actions.at(-1)!;
            const normalizedId = normalizationTarget(action, slot.actor, target.sourceId);
            const acted = await checkpoint('structural-action');
            assertSourceRowInsertion(
                action,
                { actor: slot.actor, sourceId: target.sourceId },
                acted.observations,
            );
            let remoteState;
            if (slot.history === 'remote') {
                requireContinuity(
                    slot.remoteActor !== undefined && slot.remoteActor !== slot.actor,
                    'lifetime remote actor',
                );
                const remote = setup.participants[slot.remoteActor]!;
                const cell = realCells(await observeEvidence(remote)).find(
                    (cell) => cell.sourceId === normalizedId,
                );
                requireContinuity(cell, 'lifetime fresh remote normalization source');
                await call(remote, 'command', {
                    type: 'insertText',
                    text: 'remote-',
                    at: typingCursor(cell, slot.schedule.kinds[slot.remoteActor]!),
                });
                remoteState = await checkpoint('remote-content');
            }
            await call(actor, 'undo', {});
            const undone = await checkpoint('undo');
            await call(actor, 'redo', {});
            const redone = await checkpoint('redo');
            if (slot.history === 'remote') {
                assertNativeRemoteLifetime(
                    capture.actions,
                    {
                        actor: slot.actor,
                        remoteActor: slot.remoteActor!,
                        sourceId: target.sourceId,
                        text: 'remote-',
                    },
                    [remoteState!.observations, undone.observations, redone.observations],
                );
            } else {
                requireContinuity(slot.history === 'no-remote', 'declared lifetime history');
                assertNativeNoRemoteLifetime(
                    capture.actions,
                    { actor: slot.actor, sourceId: target.sourceId },
                    [undone.observations, redone.observations],
                );
            }
            result.status = 'proven';
            result.disposition = 'edited';
        } catch (error) {
            result.failures.push(String(error));
            result.status = capture.actions.length ? 'exercised-unproven' : 'unexercised';
            await checkpoint('failure-drain');
        } finally {
            stopEvidence(setup.participants);
        }
    }).catch((error) => {
        throw new ContinuationExecutionError(result, error);
    });
    if (!continuationPassed(result))
        result.tracePath = persistTrace({
            ...lastTrace(),
            failureClass: 'CONTINUITY',
            failureMessage: JSON.stringify({
                key: slot.key,
                failures: result.failures,
            }),
        });
    return result;
}
