import assert from 'node:assert/strict';
import { canonicalDocumentShape } from './assertions.js';
import { availabilityVerified, declaredStructuralTarget } from './availability-evidence.js';
import {
    continuationDeclaration,
    continuationPassed,
    textHistoryRequirements,
    type ContinuationResult,
    type ContinuationSlot,
} from './corpus.js';
import { requireContinuity } from './continuity-evidence.js';

export function usableHistoryObligations(
    originals: readonly ContinuationSlot[],
    structural: readonly ContinuationResult[],
    companions: readonly ContinuationResult[],
) {
    const expected = textHistoryRequirements(originals);
    const indexed = (results: readonly ContinuationResult[], keys: readonly string[]) => {
        const found = new Map(results.map((result) => [result.slot.key, result]));
        requireContinuity(
            found.size === results.length &&
                results.length === keys.length &&
                keys.every((key) => found.has(key)),
            'availability history exact result set',
        );
        return found;
    };
    const originalResults = indexed(
        structural,
        originals.map((slot) => slot.key),
    );
    const textResults = indexed(
        companions,
        expected.map((slot) => slot.key),
    );
    return originals.map((slot, index) => {
        const original = originalResults.get(slot.key)!;
        const text = textResults.get(expected[index]!.key)!;
        assert.deepEqual(
            continuationDeclaration(original.slot),
            continuationDeclaration(slot),
            'availability original declaration',
        );
        assert.deepEqual(
            continuationDeclaration(text.slot),
            continuationDeclaration(expected[index]!),
            'availability companion declaration',
        );
        requireContinuity(
            availabilityVerified(original) && continuationPassed(text),
            'availability and independent text history required',
        );
        const baseline = original.checkpoints[0]!.textHistoryObservations![slot.actor]!;
        const textBaseline = text.checkpoints[0]!.textHistoryObservations![slot.actor]!;
        assert.deepEqual(
            canonicalDocumentShape(textBaseline.raw),
            canonicalDocumentShape(baseline.raw),
            'availability companion original raw starting state',
        );
        const originalTarget = declaredStructuralTarget(slot, baseline);
        const companionTarget = declaredStructuralTarget(slot, textBaseline);
        assert.equal(
            companionTarget.source,
            originalTarget.source,
            'availability companion source path',
        );
        assert.equal(
            text.textHistory!.sourceId,
            companionTarget.sourceId,
            'availability companion target policy',
        );
        return {
            originalKey: slot.key,
            companionKey: text.slot.key,
            structuralOutcome: original.availability!,
            structuralHistoryProven: false as const,
            textHistoryProven: true as const,
            usableHistoryProven: true as const,
        };
    });
}
