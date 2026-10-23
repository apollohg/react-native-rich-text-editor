import { createV2LocalHandle, mockNativeModule } from './helpers/NativeRichTextEditorFixture';
import { render } from '@testing-library/react-native';
import { Text } from 'react-native';
import { NativeRichTextEditor } from '../NativeRichTextEditor';
import type { EditorToolbarItem } from '../EditorToolbar';

it('forwards custom rendering inline and image descriptors to the native keyboard toolbar', () => {
    const handle = createV2LocalHandle();
    const renderToolbarIcon = jest.fn(() => <Text>Consumer icon</Text>);
    const toolbarItems: EditorToolbarItem[] = [
        {
            type: 'action',
            key: 'custom',
            label: 'Custom',
            icon: { type: 'image', source: { uri: 'file:///custom.png' }, tintable: false },
        },
    ];
    const props = {
        documentHandle: handle,
        toolbarItems,
        renderToolbarIcon,
        onToolbarAction: jest.fn(),
    };
    const { getByText, getByTestId, rerender, unmount } = render(
        <NativeRichTextEditor {...props} toolbarPlacement="inline" />
    );
    expect(getByText('Consumer icon')).toBeTruthy();
    expect(renderToolbarIcon).toHaveBeenCalled();
    renderToolbarIcon.mockClear();
    rerender(<NativeRichTextEditor {...props} toolbarPlacement="keyboard" />);
    expect(renderToolbarIcon).not.toHaveBeenCalled();
    expect(JSON.parse(getByTestId('native-editor-view').props.toolbarItemsJson)).toEqual(
        toolbarItems
    );
    unmount();
    handle.destroy();
});

it('reuses bundled glyphs across inline toolbar colour changes', () => {
    const handle = createV2LocalHandle();
    const toolbarItems: EditorToolbarItem[] = [
        { type: 'mark', mark: 'bold', label: 'Bold', icon: { type: 'default', id: 'bold' } },
    ];
    const props = { documentHandle: handle, toolbarItems, toolbarPlacement: 'inline' as const };
    const { rerender, unmount } = render(<NativeRichTextEditor {...props} />);
    expect(mockNativeModule.getToolbarIcon).toHaveBeenCalledWith('format-bold');
    mockNativeModule.getToolbarIcon.mockClear();
    rerender(<NativeRichTextEditor {...props} theme={{ toolbar: { buttonColor: '#abcdef' } }} />);
    expect(mockNativeModule.getToolbarIcon).not.toHaveBeenCalled();
    unmount();
    handle.destroy();
});
