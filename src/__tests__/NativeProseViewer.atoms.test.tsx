jest.mock('../specs/PreparedProseViewerNativeComponent', () => {
    const React = require('react');
    const { View } = require('react-native');

    return React.forwardRef((props: Record<string, unknown>, _ref: React.Ref<unknown>) => (
        <View testID={'prepared-prose-viewer'} {...props} />
    ));
});

import React from 'react';
import { View } from 'react-native';
import { act, fireEvent, render } from '@testing-library/react-native';
import { NativeProseViewer } from '../NativeProseViewer';
import { AtomHost } from '../AtomHost';
import { defineAtomNode, type AtomComponentProps } from '../atoms';

const Counter = (props: AtomComponentProps) => <View testID={'counter'} atomProps={props} />;

const atom = defineAtomNode({
    name: 'counterCard',
    attrs: { count: { default: 0 } },
    html: { tag: 'div', staticAttrs: { 'data-counter': '' } },
    component: Counter,
    estimatedHeight: 80,
});

const atoms = [ atom ];
const content = { type: 'doc', content: [ { type: 'counterCard', attrs: { count: 2 } } ] };

const position = {
    nodeType: atom.name,
    docPos: 0,
    attrsJson: '{"count":2}',
    x: 12,
    y: 24,
    width: 276,
    height: 80,
};

function publish(view: ReturnType<typeof render>, positions = [ position ]) {
    const native = view.getByTestId('prepared-prose-viewer');
    const configuration = JSON.parse(native.props.themeJson).viewerAtoms;

    fireEvent(native, 'atomLayout', {
        nativeEvent: {
            generation: configuration.generation,
            revision: configuration.revision,
            layoutWidth: 300,
            atomsJson: JSON.stringify(positions),
        },
    });
}

function publishPresentation(
    view: ReturnType<typeof render>,
    presentationSequence: string,
    positions: readonly Record<string, unknown>[]
) {
    const native = view.getByTestId('prepared-prose-viewer');
    const configuration = JSON.parse(native.props.themeJson).viewerAtoms;

    fireEvent(native, 'atomLayout', {
        nativeEvent: {
            generation: configuration.generation,
            revision: configuration.revision,
            layoutWidth: 300,
            atomsJson: JSON.stringify({
                format: 'viewer-atoms-v2',
                presentationSequence,
                atoms: positions,
            }),
        },
    });
}

describe('NativeProseViewer custom atoms', () => {
    it('allows read-only interactions while rejecting mutations', async() => {
        const view = render(<NativeProseViewer contentJSON={content} atoms={atoms} />);
        publish(view);
        const props = view.getByTestId('counter').props.atomProps;
        expect(props.interactive).toBe(true);

        await act(async() => {
            await expect(props.updateAttrs({ count: 3 })).rejects.toMatchObject({
                code: 'not-applicable',
            });
        });
    });

    it('preserves explicit identity when content before an atom moves', () => {
        const mounted = jest.fn();

        const Card = () => {
            React.useEffect(mounted, []);

            return <View testID={'identified'} />;
        };

        const definitions = [
            {
                ...atom,
                idAttribute: 'id',
                attrs: { ...atom.attrs, id: { default: '' } },
                component: Card,
            },
        ];

        const view = render(<NativeProseViewer contentJSON={content} atoms={definitions} />);
        publish(view, [ { ...position, attrsJson: '{"id":"one","count":2}' } ]);
        view.rerender(<NativeProseViewer contentJSON={{ ...content }} atoms={definitions} />);
        publish(view, [ { ...position, docPos: 10, attrsJson: '{"id":"one","count":2}' } ]);
        expect(mounted).toHaveBeenCalledTimes(1);
    });

    it('rejects duplicate identities and reports the invalid layout', () => {
        const definitions = [ { ...atom, idAttribute: 'id' } ];
        const onError = jest.fn();

        const view = render(
            <NativeProseViewer contentJSON={content} atoms={definitions} onError={onError} />
        );

        publish(
            view,
            [ 0, 1 ].map(docPos => ({ ...position, docPos, attrsJson: '{"id":"same","count":2}' }))
        );

        expect(onError).toHaveBeenCalledWith(
            expect.objectContaining({ code: 'INVALID_ATOM_LAYOUT' })
        );

        expect(view.queryByTestId('counter')).toBeNull();
    });

    it('composes queued functional updates from acknowledged attrs', async() => {
        const onUpdateAtomAttrs = jest.fn();

        const view = render(
            <NativeProseViewer
                contentJSON={content}
                atoms={atoms}
                readOnly={false}
                onUpdateAtomAttrs={onUpdateAtomAttrs}
            />
        );

        publish(view);
        const update = view.getByTestId('counter').props.atomProps.updateAttrs;

        await act(async() => {
            await Promise.all([
                update((a: any) => ({ count: a.count + 1 })),
                update((a: any) => ({ count: a.count + 1 })),
            ]);
        });

        expect(onUpdateAtomAttrs.mock.calls.map(([ event ]) => event.partial.count)).toEqual([ 3, 4 ]);
    });

    it('composes schemas and mounts native HTML atom snapshots as read-only viewer components', () => {
        const view = render(
            <NativeProseViewer
                contentHTML={'<div data-counter data-count="2"></div>'}
                atoms={atoms}
            />
        );

        const native = view.getByTestId('prepared-prose-viewer');
        expect(JSON.parse(native.props.configJson).schema.nodes).toContainEqual(atom.nodeSpec);
        publish(view);

        expect(view.getByTestId('counter').props.atomProps).toMatchObject({
            attrs: { count: 2 },
            nodeType: 'counterCard',
            selected: false,
            readOnly: true,
            isViewer: true,
        });
    });

    it('preserves component state and measured height through controlled attribute updates', async() => {
        const mounted = jest.fn();
        const unmounted = jest.fn();

        const StatefulCounter = (props: AtomComponentProps) => {
            const [ draft, setDraft ] = React.useState('');

            React.useEffect(() => {
                mounted();

                return unmounted;
            }, []);

            return (
                <View
                    testID={'stateful-counter'}
                    atomProps={props}
                    draft={draft}
                    onChange={setDraft}
                />
            );
        };

        const definitions = [
            {
                ...atom,
                idAttribute: 'id',
                attrs: { ...atom.attrs, id: { default: 'one' } },
                component: StatefulCounter,
            },
        ];

        const onUpdateAtomAttrs = jest.fn();

        const view = render(
            <NativeProseViewer
                contentJSON={content}
                atoms={definitions}
                readOnly={false}
                onUpdateAtomAttrs={onUpdateAtomAttrs}
            />
        );

        publish(view, [ { ...position, attrsJson: '{"count":2,"id":"one"}' } ]);
        fireEvent(view.getByTestId('stateful-counter'), 'change', 'local draft');

        fireEvent(view.getByTestId('stateful-counter').parent!, 'layout', {
            nativeEvent: { layout: { width: 276, height: 123, x: 0, y: 0 } },
        });

        const oldUpdate = view.getByTestId('stateful-counter').props.atomProps.updateAttrs;

        view.rerender(
            <NativeProseViewer
                contentJSON={{
                    type: 'doc',
                    content: [ { type: 'counterCard', attrs: { count: 3 } } ],
                }}
                atoms={definitions}
                readOnly={false}
                onUpdateAtomAttrs={onUpdateAtomAttrs}
            />
        );

        expect(unmounted).not.toHaveBeenCalled();

        await act(async() => {
            await expect(oldUpdate({ count: 4 })).rejects.toMatchObject({ code: 'stale-revision' });
        });

        publish(view, [ { ...position, attrsJson: '{"count":3,"id":"one"}' } ]);
        expect(view.getByTestId('stateful-counter').props.draft).toBe('local draft');
        expect(view.getByTestId('stateful-counter').props.atomProps.attrs.count).toBe(3);
        expect(mounted).toHaveBeenCalledTimes(1);

        expect(
            JSON.parse(view.getByTestId('prepared-prose-viewer').props.themeJson).viewerAtoms
                .measurements['0']
        ).toEqual({ width: 276, height: 123 });

        await act(async() => {
            await expect(oldUpdate({ count: 4 })).rejects.toMatchObject({ code: 'stale-revision' });
        });

        await act(async() => {
            await view.getByTestId('stateful-counter').props.atomProps.updateAttrs({ count: 4 });
        });

        expect(onUpdateAtomAttrs).toHaveBeenCalledWith(
            expect.objectContaining({ attrs: { count: 3, id: 'one' } })
        );

        publish(view, []);
        expect(unmounted).toHaveBeenCalledTimes(1);
    });

    it('retains measurements from a replacement component while native layout is pending', () => {
        const Replacement = (props: AtomComponentProps) => (
            <View testID={'replacement'} atomProps={props} />
        );

        const view = render(
            <NativeProseViewer
                contentJSON={content}
                atoms={[
                    {
                        ...atom,
                        idAttribute: 'id',
                        attrs: { ...atom.attrs, id: { default: 'one' } },
                    },
                ]}
            />
        );

        publish(view, [ { ...position, attrsJson: '{"count":2,"id":"one"}' } ]);

        fireEvent(view.getByTestId('counter').parent!, 'layout', {
            nativeEvent: { layout: { width: 276, height: 123, x: 0, y: 0 } },
        });

        const oldMeasure = view
            .UNSAFE_getAllByType(View)
            .find(node => node.props.collapsable === false)!.props.onLayout;

        view.rerender(
            <NativeProseViewer
                contentJSON={{
                    type: 'doc',
                    content: [ { type: 'counterCard', attrs: { count: 3 } } ],
                }}
                atoms={[
                    {
                        ...atom,
                        idAttribute: 'id',
                        attrs: { ...atom.attrs, id: { default: 'one' } },
                        component: Replacement,
                    },
                ]}
            />
        );

        fireEvent(
            view.getByTestId('replacement', { includeHiddenElements: true }).parent!,
            'layout',
            {
                nativeEvent: { layout: { width: 276, height: 200, x: 0, y: 0 } },
            }
        );

        act(() => oldMeasure({ nativeEvent: { layout: { width: 276, height: 999, x: 0, y: 0 } } }));

        expect(
            JSON.parse(view.getByTestId('prepared-prose-viewer').props.themeJson).viewerAtoms
                .measurements
        ).toEqual({});

        publish(view, [ { ...position, attrsJson: '{"count":3,"id":"one"}' } ]);

        expect(
            JSON.parse(view.getByTestId('prepared-prose-viewer').props.themeJson).viewerAtoms
                .measurements['0']
        ).toEqual({ width: 276, height: 200 });
    });

    it('preserves the wrapper measurement when a replacement renderer has the same size', () => {
        const Replacement = (props: AtomComponentProps) => (
            <View testID={'replacement'} atomProps={props} />
        );

        const view = render(
            <NativeProseViewer
                contentJSON={content}
                atoms={[
                    {
                        ...atom,
                        idAttribute: 'id',
                        attrs: { ...atom.attrs, id: { default: 'one' } },
                    },
                ]}
            />
        );

        publish(view, [ { ...position, attrsJson: '{"count":2,"id":"one"}' } ]);

        fireEvent(view.getByTestId('counter').parent!, 'layout', {
            nativeEvent: { layout: { width: 276, height: 123, x: 0, y: 0 } },
        });

        view.rerender(
            <NativeProseViewer
                contentJSON={{
                    type: 'doc',
                    content: [ { type: 'counterCard', attrs: { count: 3 } } ],
                }}
                atoms={[
                    {
                        ...atom,
                        idAttribute: 'id',
                        attrs: { ...atom.attrs, id: { default: 'one' } },
                        component: Replacement,
                    },
                ]}
            />
        );

        publish(view, [ { ...position, attrsJson: '{"count":3,"id":"one"}' } ]);

        expect(
            JSON.parse(view.getByTestId('prepared-prose-viewer').props.themeJson).viewerAtoms
                .measurements['0']
        ).toEqual({ width: 276, height: 123 });
    });

    it('discards mounted measurements when a renderer is unregistered', () => {
        const view = render(<NativeProseViewer contentJSON={content} atoms={atoms} />);
        publish(view);

        fireEvent(view.getByTestId('counter').parent!, 'layout', {
            nativeEvent: { layout: { width: 276, height: 123, x: 0, y: 0 } },
        });

        view.rerender(<NativeProseViewer contentJSON={content} atoms={[]} />);
        view.rerender(<NativeProseViewer contentJSON={content} atoms={atoms} />);
        publish(view);

        expect(
            JSON.parse(view.getByTestId('prepared-prose-viewer').props.themeJson).viewerAtoms
                .measurements
        ).toEqual({});
    });

    it('retains mounted components across resize without reusing measurements at a different width', () => {
        const unmounted = jest.fn();

        const StatefulCounter = (props: AtomComponentProps) => {
            React.useEffect(() => unmounted, []);

            return <View testID={'stateful-counter'} atomProps={props} />;
        };

        const definitions = [ { ...atom, component: StatefulCounter } ];
        const view = render(<NativeProseViewer contentJSON={content} atoms={definitions} />);
        publish(view);
        const atomHost = view.getByTestId('stateful-counter').parent!;

        fireEvent(atomHost, 'layout', {
            nativeEvent: { layout: { width: 276, height: 123, x: 0, y: 0 } },
        });

        const oldMeasure = view
            .UNSAFE_getAllByType(View)
            .find(node => node.props.collapsable === false)!.props.onLayout;

        const container = view
            .UNSAFE_getAllByType(View)
            .find(
                node =>
                    typeof node.props.onLayout === 'function' && node.props.collapsable !== false
            )!;

        fireEvent(container, 'layout', {
            nativeEvent: { layout: { width: 200, height: 100, x: 0, y: 0 } },
        });

        const native = view.getByTestId('prepared-prose-viewer');
        const configuration = JSON.parse(native.props.themeJson).viewerAtoms;

        fireEvent(native, 'atomLayout', {
            nativeEvent: {
                ...configuration,
                layoutWidth: 200,
                atomsJson: JSON.stringify([ { ...position, width: 176 } ]),
            },
        });

        expect(unmounted).not.toHaveBeenCalled();
        act(() => oldMeasure({ nativeEvent: { layout: { width: 276, height: 999, x: 0, y: 0 } } }));

        expect(
            JSON.parse(view.getByTestId('prepared-prose-viewer').props.themeJson).viewerAtoms
                .measurements
        ).toEqual({});

        fireEvent(view.getByTestId('stateful-counter').parent!, 'layout', {
            nativeEvent: { layout: { width: 176, height: 150, x: 0, y: 0 } },
        });

        expect(
            JSON.parse(view.getByTestId('prepared-prose-viewer').props.themeJson).viewerAtoms
                .measurements['0']
        ).toEqual({ width: 176, height: 150 });
    });

    it('delegates updates to the app without changing the rendered attributes', async() => {
        const onUpdateAtomAttrs = jest.fn();

        const view = render(
            <NativeProseViewer
                contentJSON={content}
                atoms={atoms}
                readOnly={false}
                onUpdateAtomAttrs={onUpdateAtomAttrs}
            />
        );

        publish(view);

        await act(async() => {
            await view.getByTestId('counter').props.atomProps.updateAttrs({ count: 3 });
        });

        expect(onUpdateAtomAttrs).toHaveBeenCalledWith({
            nodeType: 'counterCard',
            docPos: 0,
            attrs: { count: 2 },
            partial: { count: 3 },
        });

        expect(view.getByTestId('counter').props.atomProps.attrs.count).toBe(2);
    });

    it('rejects updates when read-only, missing a handler, or given undeclared attrs', async() => {
        const view = render(<NativeProseViewer contentJSON={content} atoms={atoms} />);
        publish(view);

        await act(async() => {
            await expect(
                view.getByTestId('counter').props.atomProps.updateAttrs({ count: 3 })
            ).rejects.toMatchObject({ code: 'not-applicable' });
        });

        view.rerender(<NativeProseViewer contentJSON={content} atoms={atoms} readOnly={false} />);

        await act(async() => {
            await expect(
                view.getByTestId('counter').props.atomProps.updateAttrs({ count: 3 })
            ).rejects.toMatchObject({ code: 'not-applicable' });
        });

        const onUpdateAtomAttrs = jest.fn();

        view.rerender(
            <NativeProseViewer
                contentJSON={content}
                atoms={atoms}
                readOnly={false}
                onUpdateAtomAttrs={onUpdateAtomAttrs}
            />
        );

        await act(async() => {
            await expect(
                view.getByTestId('counter').props.atomProps.updateAttrs({ other: 3 })
            ).rejects.toMatchObject({ code: 'not-applicable' });
        });

        expect(onUpdateAtomAttrs).not.toHaveBeenCalled();
    });

    it('discards stale native layouts and update callbacks after content replacement', async() => {
        const onUpdateAtomAttrs = jest.fn();

        const view = render(
            <NativeProseViewer
                contentJSON={content}
                atoms={atoms}
                readOnly={false}
                onUpdateAtomAttrs={onUpdateAtomAttrs}
            />
        );

        publish(view);
        const update = view.getByTestId('counter').props.atomProps.updateAttrs;
        const oldNative = view.getByTestId('prepared-prose-viewer').props;
        const oldConfiguration = JSON.parse(oldNative.themeJson).viewerAtoms;

        view.rerender(
            <NativeProseViewer
                contentHTML={'<p>Replaced</p>'}
                atoms={atoms}
                readOnly={false}
                onUpdateAtomAttrs={onUpdateAtomAttrs}
            />
        );

        act(() =>
            oldNative.onAtomLayout({
                nativeEvent: {
                    ...oldConfiguration,
                    layoutWidth: 300,
                    atomsJson: JSON.stringify([ position ]),
                },
            }));

        expect(view.queryByTestId('counter')).toBeNull();

        await act(async() => {
            await expect(update({ count: 3 })).rejects.toMatchObject({ code: 'stale-revision' });
        });

        expect(onUpdateAtomAttrs).not.toHaveBeenCalled();
    });

    it('feeds component measurements back into prepared layout without remounting', () => {
        const view = render(<NativeProseViewer contentJSON={content} atoms={atoms} />);
        publish(view);
        const wrapper = view.getByTestId('counter').parent!;

        fireEvent(wrapper, 'layout', {
            nativeEvent: { layout: { width: 276, height: 123, x: 0, y: 0 } },
        });

        const config = JSON.parse(
            view.getByTestId('prepared-prose-viewer').props.themeJson
        ).viewerAtoms;

        expect(config.measurements['0']).toEqual({ width: 276, height: 123 });
        expect(view.getByTestId('counter')).toBeTruthy();
    });

    it('propagates app update failures and rejects callbacks after unmount', async() => {
        const error = new Error('Storage unavailable');
        const onUpdateAtomAttrs = jest.fn().mockRejectedValue(error);

        const view = render(
            <NativeProseViewer
                contentJSON={content}
                atoms={atoms}
                readOnly={false}
                onUpdateAtomAttrs={onUpdateAtomAttrs}
            />
        );

        publish(view);
        const update = view.getByTestId('counter').props.atomProps.updateAttrs;

        await act(async() => {
            await expect(update({ count: 3 })).rejects.toBe(error);
        });

        view.unmount();

        await act(async() => {
            await expect(update({ count: 4 })).rejects.toMatchObject({ code: 'not-ready' });
        });

        expect(onUpdateAtomAttrs).toHaveBeenCalledTimes(1);
    });

    it('clears removed atoms and rejects malformed native attributes', () => {
        const onError = jest.fn();

        const view = render(
            <NativeProseViewer contentJSON={content} atoms={atoms} onError={onError} />
        );

        publish(view);
        publish(view, []);
        expect(view.queryByTestId('counter')).toBeNull();
        publish(view, [ { ...position, attrsJson: '[]' } ]);
        expect(view.queryByTestId('counter')).toBeNull();

        expect(onError).toHaveBeenCalledWith(
            expect.objectContaining({ code: 'INVALID_ATOM_LAYOUT', fatal: false })
        );
    });

    it('clips candidate table hosts while retaining complete offscreen metadata and measurements', () => {
        const second = {
            ...position,
            docPos: 1,
            attrsJson: '{"count":3}',
            x: 190,
            presentation: {
                clip: { x: 200, y: 20, width: 20, height: 40 },
                candidate: false,
            },
        };
        const first = {
            ...position,
            presentation: {
                clip: { x: 20, y: 30, width: 40, height: 50 },
                candidate: true,
            },
        };
        const view = render(<NativeProseViewer contentJSON={content} atoms={atoms} />);

        publishPresentation(view, '1', [ first, second ]);
        expect(view.getAllByTestId('counter')).toHaveLength(1);
        const measurementLayer = view.UNSAFE_getByType(AtomHost).parent!;
        expect(measurementLayer.props.style).toEqual({
            position: 'absolute', left: -8, top: -6, width: 276,
        });
        expect(measurementLayer.parent!.parent!.props.style).toEqual(expect.objectContaining({
            overflow: 'hidden', left: 20, top: 30, width: 40, height: 50,
        }));

        fireEvent(view.getByTestId('counter').parent!, 'layout', {
            nativeEvent: { layout: { width: 276, height: 123, x: 0, y: 0 } },
        });
        publishPresentation(view, '2', [
            { ...first, presentation: { ...first.presentation, candidate: false } },
            second,
        ]);

        expect(view.queryByTestId('counter')).toBeNull();
        expect(
            JSON.parse(view.getByTestId('prepared-prose-viewer').props.themeJson).viewerAtoms
                .measurements['0']
        ).toEqual({ width: 276, height: 123 });

        publishPresentation(view, '3', [ first, second ]);
        expect(view.getAllByTestId('counter')).toHaveLength(1);
        expect(
            JSON.parse(view.getByTestId('prepared-prose-viewer').props.themeJson).viewerAtoms
                .measurements['0']
        ).toEqual({ width: 276, height: 123 });
    });

    it('keeps the newest valid presentation envelope after stale and malformed events', () => {
        const onError = jest.fn();
        const view = render(
            <NativeProseViewer contentJSON={content} atoms={atoms} onError={onError} />
        );
        const newest = { ...position, y: 91, presentation: {
            clip: { x: 0, y: 90, width: 276, height: 80 }, candidate: true,
        } };

        publishPresentation(view, '2', [ newest ]);
        publishPresentation(view, '1', [ { ...newest, attrsJson: '[]' } ]);
        publishPresentation(view, '1', []);
        expect(
            view.UNSAFE_getAllByType(View).some(node => node.props.style?.top === 91)
        ).toBe(true);
        expect(onError).not.toHaveBeenCalled();

        publishPresentation(view, '3', [ { ...newest, presentation: { clip: { x: 0 }, candidate: true } } ]);
        expect(onError).toHaveBeenCalledWith(expect.objectContaining({ code: 'INVALID_ATOM_LAYOUT' }));
        publishPresentation(view, '3', [ newest ]);
        expect(view.getByTestId('counter')).toBeTruthy();
    });

    it('pins an active table host until its liveness callback releases it', () => {
        let setActive!: (active: boolean) => void;
        const ActiveCounter = (props: AtomComponentProps) => {
            setActive = props.setActive;
            return <View testID={'active-counter'} atomProps={props} />;
        };
        const definition = { ...atom, component: ActiveCounter };
        const tableAtom = {
            ...position,
            presentation: {
                clip: { x: 0, y: 0, width: 276, height: 80 },
                candidate: true,
            },
        };
        const view = render(<NativeProseViewer contentJSON={content} atoms={[ definition ]} />);

        publishPresentation(view, '1', [ tableAtom ]);
        act(() => setActive(true));
        publishPresentation(view, '2', [
            { ...tableAtom, presentation: { ...tableAtom.presentation, candidate: false } },
        ]);
        expect(view.getByTestId('active-counter')).toBeTruthy();

        act(() => setActive(false));
        expect(view.queryByTestId('active-counter')).toBeNull();
    });

    it('pins a focused table host until blur after leaving candidates', () => {
        const tableAtom = { ...position, presentation: {
            clip: { x: 12, y: 24, width: 276, height: 80 }, candidate: true,
        } };
        const view = render(<NativeProseViewer contentJSON={content} atoms={atoms} />);
        publishPresentation(view, '1', [ tableAtom ]);
        fireEvent(view.getByTestId('atom-host'), 'focus');
        publishPresentation(view, '2', [ {
            ...tableAtom, presentation: { ...tableAtom.presentation, candidate: false },
        } ]);
        expect(view.getByTestId('counter')).toBeTruthy();
        fireEvent(view.getByTestId('atom-host'), 'blur');
        expect(view.queryByTestId('counter')).toBeNull();
    });

    it('retains a pending atom update across a geometry-only candidate exit', async() => {
        let finish!: () => void;
        const deferred = new Promise<void>(resolve => { finish = resolve; });
        const onUpdateAtomAttrs = jest.fn(() => deferred);
        const tableAtom = { ...position, presentation: {
            clip: { x: 12, y: 24, width: 276, height: 80 }, candidate: true,
        } };
        const view = render(<NativeProseViewer contentJSON={content} atoms={atoms}
            readOnly={false} onUpdateAtomAttrs={onUpdateAtomAttrs} />);
        publishPresentation(view, '1', [ tableAtom ]);
        let pending!: Promise<void>;
        act(() => { pending = view.getByTestId('counter').props.atomProps.updateAttrs({ count: 3 }); });
        expect(onUpdateAtomAttrs).toHaveBeenCalledTimes(1);
        expect(view.getByTestId('counter').props.atomProps.updatePending).toBe(true);
        publishPresentation(view, '2', [ {
            ...tableAtom, x: -400,
            presentation: { clip: { x: 12, y: 24, width: 0, height: 0 }, candidate: false },
        } ]);
        expect(view.getByTestId('counter')).toBeTruthy();
        await act(async() => { finish(); await pending; });
        expect(view.queryByTestId('counter')).toBeNull();
        expect(onUpdateAtomAttrs).toHaveBeenCalledWith(expect.objectContaining({
            docPos: 0, attrs: { count: 2 }, partial: { count: 3 },
        }));
    });

    it('keeps a replacement pinned when a retired host finishes its update', async() => {
        let finish!: () => void;
        const deferred = new Promise<void>(resolve => { finish = resolve; });
        const onUpdateAtomAttrs = jest.fn(() => deferred);
        const tableAtom = { ...position, presentation: {
            clip: { x: 12, y: 24, width: 276, height: 80 }, candidate: true,
        } };
        const view = render(<NativeProseViewer contentJSON={content} atoms={atoms}
            readOnly={false} onUpdateAtomAttrs={onUpdateAtomAttrs} />);
        publishPresentation(view, '1', [ tableAtom ]);
        const retiredHost = view.UNSAFE_getByType(AtomHost);
        let pending!: Promise<void>;
        act(() => { pending = view.getByTestId('counter').props.atomProps.updateAttrs({ count: 3 }); });
        expect(view.getByTestId('counter').props.atomProps.updatePending).toBe(true);
        publishPresentation(view, '2', []);
        expect(view.queryByTestId('counter')).toBeNull();
        publishPresentation(view, '3', [ tableAtom ]);
        expect(view.UNSAFE_getByType(AtomHost)).not.toBe(retiredHost);
        expect(view.getByTestId('counter').props.atomProps.updatePending).toBe(false);
        fireEvent(view.getByTestId('atom-host'), 'focus');
        publishPresentation(view, '4', [ {
            ...tableAtom,
            presentation: { clip: { x: 12, y: 24, width: 0, height: 0 }, candidate: false },
        } ]);
        await act(async() => { finish(); await pending; });
        expect(view.getByTestId('counter')).toBeTruthy();
        const measurementLayer = view.UNSAFE_getByType(AtomHost).parent!;
        expect(measurementLayer.props.style.width).toBe(276);
        expect(measurementLayer.parent!.parent!.props.style).toEqual(expect.objectContaining({
            width: 0, height: 0, overflow: 'hidden',
        }));
        fireEvent(view.getByTestId('atom-host'), 'blur');
        expect(view.queryByTestId('counter')).toBeNull();
        publishPresentation(view, '5', [ tableAtom ]);
        expect(view.getByTestId('counter')).toBeTruthy();
        publishPresentation(view, '6', [ {
            ...tableAtom, presentation: { ...tableAtom.presentation, candidate: false },
        } ]);
        expect(view.queryByTestId('counter')).toBeNull();
    });

    it('orders empty envelopes and prevents a same-revision legacy overwrite', () => {
        const view = render(<NativeProseViewer contentJSON={content} atoms={atoms} />);
        publish(view);
        expect(view.getByTestId('counter')).toBeTruthy();
        publishPresentation(view, '9', [ position ]);
        publish(view, []);
        expect(view.getByTestId('counter')).toBeTruthy();
        publishPresentation(view, '10', []);
        expect(view.queryByTestId('counter')).toBeNull();
        publishPresentation(view, '9', [ position ]);
        publishPresentation(view, '10', [ position ]);
        publish(view);
        expect(view.queryByTestId('counter')).toBeNull();
        publishPresentation(view, '11', [ position ]);
        expect(view.getByTestId('counter')).toBeTruthy();
    });

    it.each([ '0', '01', '-1', '+1', '1.0', '1e2', '18446744073709551616' ])(
        'rejects invalid presentation sequence %s without poisoning freshness', sequence => {
            const onError = jest.fn();
            const view = render(<NativeProseViewer contentJSON={content} atoms={atoms} onError={onError} />);
            publishPresentation(view, sequence, [ position ]);
            expect(onError).toHaveBeenCalledWith(expect.objectContaining({ code: 'INVALID_ATOM_LAYOUT' }));
            publishPresentation(view, '1', [ position ]);
            expect(view.getByTestId('counter')).toBeTruthy();
        }
    );

    it('compares full UInt64 sequences without floating-point rounding', () => {
        const view = render(<NativeProseViewer contentJSON={content} atoms={atoms} />);
        publishPresentation(view, '18446744073709551614', [ position ]);
        expect(view.getByTestId('counter')).toBeTruthy();
        publishPresentation(view, '18446744073709551615', []);
        expect(view.queryByTestId('counter')).toBeNull();
        publishPresentation(view, '18446744073709551614', [ position ]);
        expect(view.queryByTestId('counter')).toBeNull();
    });

    it('rejects obsolete measurements after container width changes', () => {
        const view = render(<NativeProseViewer contentJSON={content} atoms={atoms} />);
        publish(view);

        const container = view
            .UNSAFE_getAllByType(View)
            .find(
                node =>
                    typeof node.props.onLayout === 'function' && node.props.collapsable !== false
            )!;

        const oldMeasure = view
            .UNSAFE_getAllByType(View)
            .find(node => node.props.collapsable === false)!.props.onLayout;

        fireEvent(container, 'layout', {
            nativeEvent: { layout: { width: 200, height: 100, x: 0, y: 0 } },
        });

        act(() => oldMeasure({ nativeEvent: { layout: { width: 276, height: 200, x: 0, y: 0 } } }));

        expect(
            JSON.parse(view.getByTestId('prepared-prose-viewer').props.themeJson).viewerAtoms
                .measurements
        ).toEqual({});

        publish(view);
        expect(view.queryByTestId('counter')).toBeNull();
    });

    it('ignores obsolete native layout revisions and normalizes Yoga width rounding', () => {
        const view = render(<NativeProseViewer contentJSON={content} atoms={atoms} />);
        publish(view);
        const oldNative = view.getByTestId('prepared-prose-viewer').props;
        const old = JSON.parse(oldNative.themeJson).viewerAtoms;

        fireEvent(view.getByTestId('counter').parent!, 'layout', {
            nativeEvent: { layout: { width: 276.333, height: 100, x: 0, y: 0 } },
        });

        act(() =>
            oldNative.onAtomLayout({ nativeEvent: { ...old, layoutWidth: 300, atomsJson: '[]' } }));

        expect(view.getByTestId('counter')).toBeTruthy();

        expect(
            JSON.parse(view.getByTestId('prepared-prose-viewer').props.themeJson).viewerAtoms
                .measurements['0']
        ).toEqual({ width: 276, height: 100 });
    });
});
