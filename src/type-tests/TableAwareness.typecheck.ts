import {
    createNativeEditorLocalAwarenessCellSelection,
    type NativeEditorLocalAwarenessIntent,
    type RemoteSelectionDecoration,
} from '../index';

type LocalAwarenessSelection = NonNullable<NativeEditorLocalAwarenessIntent['selection']>;

const cells: LocalAwarenessSelection = createNativeEditorLocalAwarenessCellSelection(2, 6);
const intent: NativeEditorLocalAwarenessIntent = { state: {}, focused: true, selection: cells };

// @ts-expect-error only the factory can create caller cell-awareness selections.
const literal: LocalAwarenessSelection = { anchorCell: 2, headCell: 6 };
// @ts-expect-error the Rust discriminator is bridge-owned wire data.
const tagged: LocalAwarenessSelection = { type: 'cell', anchorCell: 2, headCell: 6 };

const remote: RemoteSelectionDecoration = {
    clientId: '42',
    anchor: 2,
    head: 6,
    color: '#00f',
    cellRectangle: { anchorCell: 2, headCell: 6 },
};

void intent;
void literal;
void tagged;
void remote;
