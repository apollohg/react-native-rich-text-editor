import { defineConfig } from 'eslint/config';

import eslintConfig from '../eslint.config';

export default defineConfig([
    {
        files: [
            '**/*.{js,ts,tsx}',
            'eslint.config.ts',
        ],
        ignores: [
            '.expo',
            'ios',
            'android',
            'dist',
            'dist-export-test',
            'node_modules',
            '**/node_modules/**',
            'babel.config.js',
            'metro.config.js',
        ],
        extends: [
            ...eslintConfig,
        ],
    },
]);
