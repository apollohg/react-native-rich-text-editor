import './helpers/NativeRichTextEditorFixture';
import './helpers/EditorToolbarFixture';
import { fireEvent } from '@testing-library/react-native';
import { renderToolbar } from './helpers/EditorToolbarFixture';
import { prosemirrorSchema, tiptapCompatibleSchema, resolveDocumentSchema } from '../schemas';
import {
    normalizeRenderActiveState,
    normalizeRenderBlocks,
    normalizeRenderSelection,
} from '../NativeEditorRenderNormalization';
import { parseActiveStateFromUpdate, publicSelection } from '../RichTextEditorSerialization';

const emptyState = {
    marks: {},
    markAttrs: {},
    nodes: {},
    commands: {},
    allowedMarks: [],
    insertableNodes: [],
};

const alignmentItems = [
    {
        type: 'textAlign',
        align: 'left',
        label: 'Align Left',
        icon: { type: 'default', id: 'alignLeft' },
    },
    {
        type: 'textAlign',
        align: 'center',
        label: 'Align Center',
        icon: { type: 'default', id: 'alignCenter' },
    },
    {
        type: 'textAlign',
        align: 'right',
        label: 'Align Right',
        icon: { type: 'default', id: 'alignRight' },
    },
    {
        type: 'textAlign',
        align: null,
        label: 'Reset Alignment',
        icon: { type: 'default', id: 'alignReset' },
    },
] as const;

describe('paragraph text alignment', () => {
    it('preserves whole-node scalar ranges and rejects invalid endpoints', () => {
        const selection = { type: 'node', pos: 0, posScalar: 0, endScalar: 6 };
        expect(normalizeRenderSelection(selection)).toEqual(selection);
        expect(publicSelection({ ...selection, type: 'node' })).toEqual({ type: 'node', pos: 0 });
        expect(normalizeRenderSelection({ ...selection, endScalar: 0 })).toEqual({
            ...selection,
            endScalar: 0,
        });
        for (const endScalar of [-1, 1.5, null, '6']) {
            expect(normalizeRenderSelection({ ...selection, endScalar })).toBeNull();
        }
        expect(normalizeRenderSelection({ ...selection, posScalar: 7 })).toBeNull();
        expect(normalizeRenderSelection({ type: 'node', pos: 0, posScalar: 0 })).toEqual({
            type: 'node',
            pos: 0,
            posScalar: 0,
        });
    });

    it.each([prosemirrorSchema, tiptapCompatibleSchema])(
        'compiles nullable alignment for paragraphs and all native heading variants',
        (schema) => {
            const resolved = resolveDocumentSchema(schema);
            for (const name of ['paragraph', 'h1', 'h2', 'h3', 'h4', 'h5', 'h6']) {
                expect(resolved.nodes.find((node) => node.name === name)?.attrs?.textAlign).toEqual(
                    {
                        default: null,
                        enum: [null, 'left', 'center', 'right'],
                    }
                );
            }
        }
    );

    it.each(['left', 'center', 'right', 'mixed', null])(
        'preserves active alignment %s from snapshots and updates',
        (textAlign) => {
            const state = { ...emptyState, textAlign };
            expect(normalizeRenderActiveState(state)?.textAlign).toBe(textAlign);
            expect(parseActiveStateFromUpdate(state)?.textAlign).toBe(textAlign);
        }
    );

    it('defaults older active state payloads to inherited alignment and rejects invalid new values', () => {
        expect(normalizeRenderActiveState(emptyState)?.textAlign).toBeNull();
        expect(normalizeRenderActiveState({ ...emptyState, textAlign: 'justify' })).toBeNull();
    });

    it('accepts per-block render alignment and rejects unsupported values', () => {
        const blocks = [
            [
                { type: 'blockStart', nodeType: 'paragraph', depth: 0, textAlign: 'center' },
                { type: 'textRun', text: 'Centered', marks: [] },
                { type: 'blockEnd' },
            ],
        ];
        expect(normalizeRenderBlocks(blocks)).toEqual(blocks);
        expect(
            normalizeRenderBlocks([
                [{ ...blocks[0][0], textAlign: 'justify' }, ...blocks[0].slice(1)],
            ])
        ).toBeNull();
    });

    it('uses engine alignment for active controls and dispatches explicit and reset values', () => {
        const onSetTextAlign = jest.fn();
        const view = renderToolbar({
            toolbarItems: alignmentItems,
            onSetTextAlign,
            activeState: {
                ...emptyState,
                textAlign: 'center',
                commands: { setTextAlign: true, unsetTextAlign: true },
            },
        });
        expect(view.getByLabelText('Align Center').props.accessibilityState.selected).toBe(true);
        expect(view.getByLabelText('Align Left').props.accessibilityState.selected).toBe(false);
        fireEvent.press(view.getByLabelText('Align Right'));
        fireEvent.press(view.getByLabelText('Reset Alignment'));
        expect(onSetTextAlign.mock.calls).toEqual([['right'], [null]]);
    });

    it.each(['mixed', null] as const)(
        'does not imply explicit left alignment for %s',
        (textAlign) => {
            const view = renderToolbar({
                toolbarItems: alignmentItems,
                onSetTextAlign: jest.fn(),
                activeState: {
                    ...emptyState,
                    textAlign,
                    commands: { setTextAlign: true, unsetTextAlign: textAlign === 'mixed' },
                },
            });
            for (const label of ['Align Left', 'Align Center', 'Align Right']) {
                expect(view.getByLabelText(label).props.accessibilityState.selected).toBe(false);
            }
            expect(view.getByLabelText('Reset Alignment').props.accessibilityState.selected).toBe(
                textAlign === null
            );
        }
    );

    it('disables controls when the engine reports no eligible block', () => {
        const onSetTextAlign = jest.fn();
        const view = renderToolbar({
            toolbarItems: alignmentItems,
            onSetTextAlign,
            activeState: emptyState,
        });
        for (const item of alignmentItems) {
            expect(view.getByLabelText(item.label).props.accessibilityState.disabled).toBe(true);
            fireEvent.press(view.getByLabelText(item.label));
        }
        expect(onSetTextAlign).not.toHaveBeenCalled();
    });
});
