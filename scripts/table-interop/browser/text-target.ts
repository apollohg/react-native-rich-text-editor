import * as Y from 'yjs';
import type { Node as PMNode, ResolvedPos } from 'prosemirror-model';
import type { Selection } from 'prosemirror-state';
import type { TextTargetEvidence } from '../peer-protocol.js';

function cellAt(position: ResolvedPos) {
    for (let depth = position.depth; depth > 0; depth -= 1) {
        const node = position.node(depth);
        if (['cell', 'header_cell'].includes(String(node.type.spec['tableRole'])))
            return { node, position: position.before(depth) };
    }
    return undefined;
}

export function observeTextTarget(
    before: PMNode,
    requestedPosition: unknown,
    selection: Selection,
    mapping: Map<Y.AbstractType<unknown>, PMNode | PMNode[]>,
): TextTargetEvidence | undefined {
    if (
        typeof requestedPosition !== 'number' ||
        !Number.isInteger(requestedPosition) ||
        requestedPosition < 0 ||
        requestedPosition > before.content.size ||
        !selection.empty
    )
        return undefined;
    const original = cellAt(before.resolve(requestedPosition));
    const current = cellAt(selection.$from);
    if (!original || !current) return undefined;
    const matches = [...mapping].filter(
        ([type, nodes]) =>
            type instanceof Y.XmlElement &&
            (Array.isArray(nodes) ? nodes.includes(current.node) : nodes === current.node),
    );
    if (matches.length !== 1) return undefined;
    const identity = Y.createRelativePositionFromTypeIndex(matches[0]![0], 0).type;
    if (!identity) return undefined;
    return {
        requestedPosition,
        beforeCellPosition: original.position,
        afterCellPosition: current.position,
        sourceId: JSON.stringify({ type: identity }),
    };
}
