import type * as React from 'react';
import {
    codegenNativeCommands,
    codegenNativeComponent,
    type HostComponent,
    type ViewProps,
} from 'react-native';

export interface NativeProps extends ViewProps {}

type NativeEditorFocusHostComponentType = HostComponent<NativeProps>;

interface NativeCommands {
    focus: (viewRef: React.ComponentRef<NativeEditorFocusHostComponentType>) => void;
    blur: (viewRef: React.ComponentRef<NativeEditorFocusHostComponentType>) => void;
}

export const Commands = codegenNativeCommands<NativeCommands>({
    supportedCommands: [ 'focus', 'blur' ],
});

export default codegenNativeComponent<NativeProps>('NativeEditorFocusHost');
