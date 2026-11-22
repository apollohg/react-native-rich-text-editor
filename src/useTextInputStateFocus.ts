import { useCallback, useEffect, useRef, type RefCallback } from 'react';
import { TextInput, type HostInstance } from 'react-native';

export function useTextInputStateFocus(isFocused: boolean): RefCallback<HostInstance> {
    const focusHostRef = useRef<HostInstance | null>(null);

    useEffect(() => {
        const focusHost = focusHostRef.current;

        if (isFocused && focusHost != null) {
            TextInput.State.focusTextInput(focusHost);
        }
    }, [ isFocused ]);

    return useCallback((focusHost: HostInstance | null) => {
        if (focusHost == null) {
            return;
        }

        focusHostRef.current = focusHost;

        return () => {
            focusHostRef.current = null;
            TextInput.State.blurTextInput(focusHost);
        };
    }, []);
}
