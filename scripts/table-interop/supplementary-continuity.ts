import {
    CONVERGENCE_CORPUS,
    continuationRequirements,
    runContinuation,
    type ContinuationSlot,
    type CorpusSchedule,
} from './corpus.js';
import { withSupplementarySetup } from './supplementary-setup.js';
import {
    assertCrossingRowspan,
    assertSourceRowInsertion,
    supplementaryGaps,
} from './supplementary-evidence.js';
import { cellText, realCells } from './scenario-evidence.js';
import { requireContinuity } from './continuity-evidence.js';
import { runNativeLifetime } from './supplementary-lifetime.js';
import type { JsonNode } from './peer-protocol.js';
import type { SchemaPreset } from './controller.js';

export type SupplementaryFamily =
    'overlap' | 'crossing-rowspan' | 'native-owned-normalization' | 'nested-gaps';
export interface SupplementarySlot extends ContinuationSlot {
    family: SupplementaryFamily;
    target: 'a' | 'b' | 'c' | 'first-nested' | 'second-nested';
    history?: 'no-remote' | 'remote';
    remoteActor?: number;
    gapRow?: number;
}

export async function runSupplementary(slot: SupplementarySlot) {
    if (slot.family === 'native-owned-normalization') return runNativeLifetime(slot);
    return runContinuation(slot, {
        setup: (_candidate, body) => withSupplementarySetup(slot, body),
        target: (view) => {
            const cells = realCells(view).filter((cell) => cellText(cell.node) === slot.target);
            requireContinuity(cells.length === 1, 'designated supplementary source target');
            return cells[0]!;
        },
        gaps: (view) => supplementaryGaps(view, slot),
        structuralEvidence: (action, intent, settled) => {
            const sourceId = action.before.tables
                .flatMap((table) => table.cells)
                .find((cell) => cell.source === intent.source)?.sourceId;
            requireContinuity(sourceId, 'supplementary structural source identity');
            if (slot.family === 'crossing-rowspan') {
                const spanning = realCells(action.before).find(
                    (cell) => cellText(cell.node) === 'a',
                );
                requireContinuity(spanning?.sourceId, 'crossing source identity');
                assertCrossingRowspan(
                    action,
                    {
                        actor: intent.actor,
                        sourceId,
                        spanningId: spanning.sourceId,
                    },
                    settled,
                );
            } else assertSourceRowInsertion(action, { actor: intent.actor, sourceId }, settled);
        },
    });
}

export function supplementaryFixture(family: SupplementaryFamily, preset: SchemaPreset): JsonNode {
    const cell = (text: string, rowspan = 1, colspan = 1): JsonNode => ({
        type: preset === 'tiptap' ? 'tableCell' : 'table_cell',
        attrs: { colspan, rowspan },
        content: [
            {
                type: 'paragraph',
                ...(text ? { content: [{ type: 'text', text }] } : {}),
            },
        ],
    });
    const row = (content: JsonNode[]): JsonNode => ({
        type: preset === 'tiptap' ? 'tableRow' : 'table_row',
        ...(content.length ? { content } : {}),
    });
    const table = (content: JsonNode[]): JsonNode => ({
        type: 'table',
        content,
    });
    let grid: JsonNode;
    if (family === 'overlap')
        grid = table([row([cell('a'), cell('b', 2)]), row([cell('c', 3, 2)]), row([])]);
    else if (family === 'crossing-rowspan')
        grid = table([row([cell('a', 2), cell('b')]), row([cell('c')])]);
    else if (family === 'native-owned-normalization')
        grid = table([row([cell('a'), cell('b')]), row([cell('c')])]);
    else {
        const container = cell('outer');
        container.content!.push(
            table([row([cell('first-a'), cell('first-b')]), row([cell('first-c')])]),
            table([row([cell('second-a'), cell('second-b')]), row([cell('second-c')])]),
        );
        grid = table([row([container])]);
    }
    return { type: 'doc', content: [grid] };
}

export function supplementaryRequirements(): SupplementarySlot[] {
    const groups = new Map<string, CorpusSchedule>();
    for (const schedule of CONVERGENCE_CORPUS)
        groups.set(`${schedule.topology} ${schedule.preset}`, schedule);
    const result: SupplementarySlot[] = [];
    for (const sample of groups.values()) {
        for (const family of [
            'overlap',
            'crossing-rowspan',
            'native-owned-normalization',
            'nested-gaps',
        ] as const) {
            const schedule: CorpusSchedule = {
                ...sample,
                actorOffset: 0,
                name: `supplementary :: ${sample.topology} :: ${sample.preset} :: ${family}`,
                scenario: {
                    name: family,
                    mutatesGeometry: false,
                    minimumNativeActors: 0,
                    minimumWebActors: 0,
                    table: (preset) =>
                        supplementaryFixture(family, preset).content![0]! as Record<
                            string,
                            unknown
                        >,
                    act: async () => {},
                    proves: () => {
                        throw new Error('TBL21 CONTINUITY raw supplementary setup required');
                    },
                },
            };
            for (const slot of continuationRequirements([schedule])) {
                const add = (
                    variant: string,
                    target: SupplementarySlot['target'],
                    options: Pick<SupplementarySlot, 'history' | 'remoteActor' | 'gapRow'> = {},
                ) =>
                    result.push({
                        ...slot,
                        family,
                        target,
                        ...options,
                        key: `${slot.key} :: ${variant}`,
                    });
                if (family === 'overlap') {
                    if (slot.proof === 'web-gap') {
                        for (const gapRow of [0, 1, 2])
                            add(`row-wrapper-${gapRow}`, 'a', { gapRow });
                    } else add('real-overlap', 'a');
                } else if (
                    family === 'crossing-rowspan' &&
                    ['structure', 'history'].includes(slot.proof)
                )
                    add('cross-a-after-b', 'b');
                else if (
                    family === 'native-owned-normalization' &&
                    slot.actorKind === 'rust' &&
                    slot.proof === 'history'
                ) {
                    add('no-remote', 'c', { history: 'no-remote' });
                    for (let remoteActor = 0; remoteActor < schedule.participants; remoteActor += 1)
                        if (remoteActor !== slot.actor)
                            add(`remote-${remoteActor}`, 'c', {
                                history: 'remote',
                                remoteActor,
                            });
                } else if (family === 'nested-gaps' && slot.proof === 'web-gap') {
                    add('first-nested', 'first-nested');
                    add('second-nested', 'second-nested');
                }
            }
        }
    }
    return result;
}
