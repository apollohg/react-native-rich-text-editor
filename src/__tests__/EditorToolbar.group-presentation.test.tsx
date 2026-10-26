import { renderToolbar } from './helpers/EditorToolbarFixture';
import React from 'react';
import { StyleSheet } from 'react-native';
import { fireEvent, within } from '@testing-library/react-native';
import { EditorToolbar, type EditorToolbarGroupItem } from '../EditorToolbar';

const group: EditorToolbarGroupItem = {
    type: 'group',
    key: 'format',
    label: 'Format',
    icon: { type: 'glyph', text: 'Fallback' },
    iconSource: 'activeChild',
    items: [
        {
            type: 'mark',
            mark: 'bold',
            label: 'Bold',
            icon: { type: 'glyph', text: 'B' },
        },
        {
            type: 'mark',
            mark: 'italic',
            label: 'Italic',
            icon: { type: 'glyph', text: 'I' },
        },
    ],
};

it('updates the group icon with selection and falls back for zero or multiple active children', () => {
    const view = renderToolbar({ toolbarItems: [group] });
    for (const [marks, expected] of [
        [{ bold: true }, 'B'],
        [{ italic: true }, 'I'],
        [{ bold: true, italic: true }, 'Fallback'],
        [{}, 'Fallback'],
    ] as const) {
        view.rerender(
            <EditorToolbar
                {...view.props}
                activeState={{ ...view.props.activeState, marks }}
            />
        );
        expect(
            within(view.getByLabelText('Format')).getByText(expected)
        ).toBeTruthy();
    }
});

it('keeps the default icon static even with an active child', () => {
    const view = renderToolbar({
        toolbarItems: [{ ...group, iconSource: undefined }],
        activeState: { marks: { bold: true } },
    });
    expect(
        within(view.getByLabelText('Format')).getByText('Fallback')
    ).toBeTruthy();
});

it('renders disclosure inside the group button and retains expansion and child actions when hidden', () => {
    const onToggleBold = jest.fn();
    const view = renderToolbar({
        toolbarItems: [group],
        onToggleBold,
        activeState: { allowedMarks: ['bold', 'italic'] },
    });
    expect(
        within(view.getByLabelText('Format')).getByTestId('toolbar-disclosure')
    ).toBeTruthy();
    view.rerender(
        <EditorToolbar
            {...view.props}
            toolbarItems={[{ ...group, disclosure: 'none' }]}
        />
    );
    expect(view.queryByTestId('toolbar-disclosure')).toBeNull();
    fireEvent.press(view.getByLabelText('Format'));
    expect(
        view.getByLabelText('Format').props.accessibilityState.expanded
    ).toBe(true);
    fireEvent.press(view.getByLabelText('Bold'));
    expect(onToggleBold).toHaveBeenCalledTimes(1);
    expect(
        view.getByLabelText('Format').props.accessibilityState.expanded
    ).toBe(false);
});

it('allows wide glyph content to size the group while preserving explicit widths', () => {
    const view = renderToolbar({ toolbarItems: [group] });
    const buttonStyle = () =>
        StyleSheet.flatten(view.getByLabelText('Format').props.style);
    expect(buttonStyle().width).toBeUndefined();
    expect(buttonStyle().minWidth).toBeGreaterThan(0);
    view.rerender(
        <EditorToolbar
            {...view.props}
            toolbarItems={[{ ...group, buttonStyle: { width: 120 } }]}
        />
    );
    expect(buttonStyle().width).toBe(120);
});

it.each(['up', 'down'] as const)('renders %s disclosure without changing expansion behaviour', disclosure => {
    const view = renderToolbar({
        toolbarItems: [{ ...group, disclosure }],
        activeState: { allowedMarks: ['bold'] },
    });
    const indicator = within(view.getByLabelText('Format')).getByTestId('toolbar-disclosure');
    const chevronStyle = StyleSheet.flatten(indicator.children[0].props.style);
    expect(chevronStyle.transform).toEqual([{ rotate: disclosure === 'up' ? '225deg' : '45deg' }]);
    fireEvent.press(view.getByLabelText('Format'));
    expect(view.getByLabelText('Format').props.accessibilityState.expanded).toBe(true);
    expect(StyleSheet.flatten(view.getByTestId('toolbar-disclosure').children[0].props.style).transform)
        .toEqual(chevronStyle.transform);
});
