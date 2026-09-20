import {
    continuationCoverage,
    continuationPassed,
    structuralAvailabilityOutcome,
    type StructuralAvailabilityOutcome,
    runContinuation,
    ContinuationExecutionError,
    type ContinuationResult,
    type ContinuationSlot,
} from './corpus.js';
import { lastTrace, persistTrace } from './trace.js';

function completedTrace() {
    try {
        return lastTrace();
    } catch {
        return undefined;
    }
}

export async function executeContinuations(
    slots: readonly ContinuationSlot[],
    execute: (slot: ContinuationSlot) => Promise<ContinuationResult> = runContinuation,
    write: (result: ContinuationResult) => Promise<void>,
) {
    if (new Set(slots.map((slot) => slot.key)).size !== slots.length)
        throw new Error('duplicate continuation declaration');
    const coverage: ContinuationSlot[] = [];
    let passed = 0;
    let successfulEdits = 0, appliedStructuralHistory = 0, appliedTextHistory = 0;
    const availabilityOutcomes: Record<StructuralAvailabilityOutcome, number> = { 'successful-edit': 0, 'verified-native-refusal': 0, 'verified-stock-limitation': 0, 'unexplained-failure': 0, 'missing-evidence': 0 };
    for (const slot of slots) {
        let result: ContinuationResult;
        const previousTrace = completedTrace();
        try {
            result = await execute(slot);
        } catch (error) {
            result =
                error instanceof ContinuationExecutionError && error.result.slot.key === slot.key
                    ? error.result
                    : {
                          slot,
                          required: slot.required,
                          status: 'unexercised',
                          disposition: 'unreached',
                          checkpoints: [],
                          actions: [],
                          failures: [],
                          dependencies: [],
                      };
            result.status = result.actions.length ? 'exercised-unproven' : 'unexercised';
            result.failures.push(`runner execution: ${String(error)}`);
            const trace = completedTrace();
            if (trace && trace !== previousTrace)
                result.tracePath = persistTrace({
                    ...trace,
                    failureClass: 'CONTINUITY',
                    failureMessage: JSON.stringify({ key: slot.key, failures: result.failures }),
                });
        }
        coverage.push(...continuationCoverage([slot], [result]));
        if (continuationPassed(result)) passed += 1;
        if (['structure', 'history'].includes(slot.proof)) availabilityOutcomes[structuralAvailabilityOutcome(result)]++;
        if (continuationPassed(result) && result.disposition === 'edited') {
            successfulEdits++;
            if (slot.proof === 'history') appliedStructuralHistory++;
            if (slot.proof === 'text-history') appliedTextHistory++;
        }
        await write(result);
    }
    return {
        declared: slots.length,
        executed: coverage.length,
        passed,
        successfulEdits,
        appliedStructuralHistory,
        appliedTextHistory,
        availabilityOutcomes,
        coverage,
    };
}
