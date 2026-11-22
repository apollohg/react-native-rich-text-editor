import { useEffect, type RefObject } from 'react';
import type { HostInstance } from 'react-native';
import TextInputState from 'react-native/Libraries/Components/TextInput/TextInputState';

export function useTextInputStateFocus(
    focusHostRef: RefObject<HostInstance | null>,
    isFocused: boolean
): void {
    useEffect(() => {
        const focusHost = focusHostRef.current;

        if (!isFocused || focusHost == null) {
            return;
        }

        TextInputState.focusInput(focusHost);

        return () => TextInputState.blurInput(focusHost);
    }, [ focusHostRef, isFocused ]);
}
