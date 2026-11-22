declare module 'react-native/Libraries/Components/TextInput/TextInputState' {
    import type { HostInstance } from 'react-native';

    const TextInputState: {
        focusInput(textField: HostInstance | null): void;
        blurInput(textField: HostInstance | null): void;
    };

    export default TextInputState;
}
