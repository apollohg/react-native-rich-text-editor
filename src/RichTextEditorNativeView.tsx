import { requireNativeViewManager } from 'expo-modules-core';

import { StyleSheet } from 'react-native';

export const NativeEditorView = requireNativeViewManager('NativeEditor');

export const styles = StyleSheet.create({
    container: {
        position: 'relative',
    },
    inlineToolbar: {
        flexDirection: 'row',
        alignItems: 'center',
    },
    placedTableToolbar: {
        position: 'absolute',
    },
    measuringTableToolbar: {
        position: 'absolute',
        left: 0,
        top: 0,
        opacity: 0,
    },
});
