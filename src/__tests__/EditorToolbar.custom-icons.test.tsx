import { renderToolbar } from './helpers/EditorToolbarFixture';
import React from 'react';
import { Image, StyleSheet, Text, View } from 'react-native';
import { fireEvent } from '@testing-library/react-native';
import type { EditorToolbarIconRenderer } from '../EditorToolbar';

const CUSTOM_COLOR = '#123456';
const CUSTOM_SIZE = 24;
const IMAGE_URI = 'file:///toolbar-icon.png';

describe('consumer toolbar icons', () => {
    it('passes resolved appearance and state to a renderer without changing button actions', () => {
        const onToolbarAction = jest.fn();
        const renderToolbarIcon = jest.fn<
            ReturnType<EditorToolbarIconRenderer>,
            Parameters<EditorToolbarIconRenderer>
        >(({ color, size }) => (
            <Text testID="custom-icon" style={{ color, fontSize: size }}>
                Custom
            </Text>
        ));
        const { getByTestId, getByLabelText } = renderToolbar({
            renderToolbarIcon,
            onToolbarAction,
            toolbarItems: [
                {
                    type: 'action',
                    key: 'custom',
                    label: 'Custom action',
                    icon: { type: 'glyph', text: 'C' },
                    isActive: true,
                    buttonStyle: { activeColor: CUSTOM_COLOR, iconSize: CUSTOM_SIZE },
                },
            ],
        });
        expect(renderToolbarIcon).toHaveBeenLastCalledWith({
            icon: { type: 'glyph', text: 'C' },
            key: 'custom',
            color: CUSTOM_COLOR,
            size: CUSTOM_SIZE,
            isActive: true,
            isDisabled: false,
        });
        expect(StyleSheet.flatten(getByTestId('custom-icon').props.style)).toEqual({
            color: CUSTOM_COLOR,
            fontSize: CUSTOM_SIZE,
        });
        fireEvent.press(getByLabelText('Custom action'));
        expect(onToolbarAction).toHaveBeenCalledWith('custom');
    });

    it('delegates undefined results to built-in icons, but preserves an intentional null', () => {
        const { getByText, queryByText } = renderToolbar({
            onToolbarAction: jest.fn(),
            renderToolbarIcon: ({ key }) => (key === 'hidden' ? null : undefined),
            toolbarItems: [
                {
                    type: 'action',
                    key: 'fallback',
                    label: 'Fallback',
                    icon: { type: 'glyph', text: 'Fallback glyph' },
                },
                {
                    type: 'action',
                    key: 'hidden',
                    label: 'Hidden icon',
                    icon: { type: 'glyph', text: 'Hidden glyph' },
                },
            ],
        });
        expect(getByText('Fallback glyph')).toBeTruthy();
        expect(queryByText('Hidden glyph')).toBeNull();
    });

    it('renders group menu children with disabled state and resolved disabled colour', () => {
        const measurement = jest
            .spyOn(View.prototype, 'measureInWindow')
            .mockImplementation((callback) => callback(0, 0, 40, 32));
        const renderToolbarIcon = jest.fn(({ key }) => <Text>{`custom-${key}`}</Text>);
        const { getByLabelText, getByText } = renderToolbar({
            renderToolbarIcon,
            onToolbarAction: jest.fn(),
            theme: { buttonDisabledColor: CUSTOM_COLOR },
            toolbarItems: [
                {
                    type: 'group',
                    key: 'group',
                    label: 'Group',
                    presentation: 'menu',
                    icon: { type: 'glyph', text: 'G' },
                    items: [
                        {
                            type: 'action',
                            key: 'child',
                            label: 'Child',
                            isDisabled: true,
                            icon: { type: 'glyph', text: 'C' },
                        },
                        {
                            type: 'action',
                            key: 'enabled',
                            label: 'Enabled',
                            icon: { type: 'glyph', text: 'E' },
                        },
                    ],
                },
            ],
        });
        fireEvent.press(getByLabelText('Group'));
        expect(getByText('custom-group:child')).toBeTruthy();
        expect(renderToolbarIcon).toHaveBeenCalledWith(
            expect.objectContaining({
                key: 'group:child',
                isDisabled: true,
                color: CUSTOM_COLOR,
            })
        );
        measurement.mockRestore();
    });

    it('tints image icons and falls back on load failure', () => {
        const { UNSAFE_getByType, getByText } = renderToolbar({
            onToolbarAction: jest.fn(),
            theme: { buttonColor: CUSTOM_COLOR, buttonIconSize: CUSTOM_SIZE },
            toolbarItems: [
                {
                    type: 'action',
                    key: 'image',
                    label: 'Image',
                    icon: {
                        type: 'image',
                        source: { uri: IMAGE_URI },
                        fallbackText: 'Failed image',
                    },
                },
            ],
        });
        const image = UNSAFE_getByType(Image);
        expect(image.props.source).toEqual({ uri: IMAGE_URI });
        expect(StyleSheet.flatten(image.props.style)).toEqual(
            expect.objectContaining({
                width: CUSTOM_SIZE,
                height: CUSTOM_SIZE,
                tintColor: CUSTOM_COLOR,
            })
        );
        fireEvent(image, 'error', { nativeEvent: { error: 'Cannot decode image' } });
        expect(getByText('Failed image')).toBeTruthy();
    });
});

it('preserves the default Material glyph and fixed icon size without an icon package', () => {
    const { getByText } = renderToolbar({
        toolbarItems: [
            { type: 'mark', mark: 'bold', label: 'Bold', icon: { type: 'default', id: 'bold' } },
        ],
    });
    const { glyph, fontFamily } = require('./helpers/toolbarIconNativeMock').getToolbarIcon(
        'format-bold'
    );
    const icon = getByText(glyph);
    expect(StyleSheet.flatten(icon.props.style)).toEqual(
        expect.objectContaining({
            fontFamily,
            fontSize: 20,
            fontWeight: 'normal',
            fontStyle: 'normal',
        })
    );
    expect(icon.props.allowFontScaling).toBe(false);
});
