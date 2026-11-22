import './helpers/NativeRichTextEditorFixture';
import { V2_INITIAL_DOC, createV2LocalHandle } from './helpers/NativeRichTextEditorFixture';
import { render, act } from '@testing-library/react-native';
import { Keyboard, TextInput, type HostInstance } from 'react-native';
import { dispatchCommand } from 'react-native/Libraries/ReactNative/RendererProxy';
import type { ReactTestInstance } from 'react-test-renderer';
import { NativeRichTextEditor } from '../NativeRichTextEditor';
import NativeEditorFocusHost from '../specs/NativeEditorFocusHostNativeComponent';

jest.mock('react-native/Libraries/ReactNative/RendererProxy', () => ({
    ...jest.requireActual('react-native/Libraries/ReactNative/RendererProxy'),
    dispatchCommand: jest.fn(),
}));

const mockedDispatchCommand = jest.mocked(dispatchCommand);

const currentlyFocusedInput = (): HostInstance | null => TextInput.State.currentlyFocusedInput();

function nearestHostParent(instance: ReactTestInstance): ReactTestInstance | null {
    let parent = instance.parent;
    while (parent != null && typeof parent.type !== 'string') {
        parent = parent.parent;
    }
    return parent;
}

function renderFocusedEditor() {
    const handle = createV2LocalHandle(V2_INITIAL_DOC);
    const rendered = render(<NativeRichTextEditor documentHandle={handle} />);
    const emitFocus = (isFocused: boolean) =>
        act(() => rendered.getByTestId('native-editor-view').props.onFocusChange({
            nativeEvent: { editorId: handle.editorId, isFocused },
        }));
    emitFocus(true);
    const focusHost = rendered.UNSAFE_getByType(NativeEditorFocusHost).instance as HostInstance;
    return { handle, rendered, emitFocus, focusHost };
}

describe('NativeRichTextEditor React Native keyboard focus tracking', () => {
    beforeEach(() => {
        mockedDispatchCommand.mockClear();
    });

    afterEach(() => {
        TextInput.State.blurTextInput(currentlyFocusedInput());
    });

    it('mounts the focus host beside the editor view so native blur can find its sibling', () => {
        const { handle, rendered } = renderFocusedEditor();
        const hostParent = nearestHostParent(rendered.UNSAFE_getByType(NativeEditorFocusHost));
        const editorParent = nearestHostParent(rendered.getByTestId('native-editor-view'));

        expect(hostParent === editorParent).toBe(true);
        expect(hostParent?.props.collapsable).toBe(false);
        handle.destroy();
    });

    it('registers the focused editor host with React Native focus tracking', () => {
        const { handle, focusHost } = renderFocusedEditor();

        expect(currentlyFocusedInput()).toBe(focusHost);
        expect(mockedDispatchCommand.mock.calls).toEqual([ [ focusHost, 'focus', [] ] ]);
        handle.destroy();
    });

    it('routes Keyboard.dismiss() to a blur command on the focused editor host', () => {
        const { handle, focusHost } = renderFocusedEditor();
        mockedDispatchCommand.mockClear();

        Keyboard.dismiss();

        expect(mockedDispatchCommand.mock.calls).toEqual([ [ focusHost, 'blur', [] ] ]);
        expect(currentlyFocusedInput()).toBeNull();
        handle.destroy();
    });

    it('keeps another input registered when the editor blur arrives after it took focus', () => {
        const { handle, emitFocus } = renderFocusedEditor();
        const otherInput = {} as HostInstance;

        TextInput.State.focusTextInput(otherInput);
        emitFocus(false);

        expect(currentlyFocusedInput()).toBe(otherInput);
        handle.destroy();
    });

    it('blurs and releases the focused editor host when the editor unmounts', () => {
        const { handle, rendered, focusHost } = renderFocusedEditor();
        mockedDispatchCommand.mockClear();

        rendered.unmount();

        expect(mockedDispatchCommand.mock.calls).toEqual([ [ focusHost, 'blur', [] ] ]);
        expect(currentlyFocusedInput()).toBeNull();
        handle.destroy();
    });
});
