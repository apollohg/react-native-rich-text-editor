import {
    continuationCoverage,
    continuationPassed,
    runContinuation,
    type ContinuationResult,
    type ContinuationSlot,
} from './corpus.js';

export async function executeContinuations(
    slots: readonly ContinuationSlot[],
    execute: (slot: ContinuationSlot) => Promise<ContinuationResult> = runContinuation,
    write: (result: ContinuationResult) => Promise<void>,
) {
    if (new Set(slots.map((slot) => slot.key)).size !== slots.length)
        throw new Error('duplicate continuation declaration');
    const coverage: ContinuationSlot[] = [];
    let passed = 0;
    for (const slot of slots) {
        let result: ContinuationResult;
        try {
            result = await execute(slot);
        } catch (error) {
            result = {
                slot,
                required: slot.required,
                status: 'unexercised',
                disposition: 'unreached',
                checkpoints: [],
                actions: [],
                failures: [`runner execution: ${String(error)}`],
                dependencies: [],
            };
        }
        coverage.push(...continuationCoverage([slot], [result]));
        if (continuationPassed(result)) passed += 1;
        await write(result);
    }
    return {
        declared: slots.length,
        executed: coverage.length,
        passed,
        coverage,
    };
}
