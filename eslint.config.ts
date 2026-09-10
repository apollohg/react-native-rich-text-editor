import eslint from '@eslint/js';
import stylistic from '@stylistic/eslint-plugin';
import { type ESLint } from 'eslint';
import { defineConfig } from 'eslint/config';
import reactHooksPlugin from 'eslint-plugin-react-hooks';
import tseslint from 'typescript-eslint';
// @ts-ignore
import eslintReactNative from 'eslint-plugin-react-native';
import { createSlopConfig } from 'eslint-plugin-slop';

export default defineConfig([
    {
        ignores: [
            '.tmp/**',
            '.worktrees/**',
            '**/dist/**',
            'coverage/**',
            'ios-tests/Pods/**',
            'ios-tests/build/**',
            'ios',
            'android',
            'example',
            'node_modules',
            '**/node_modules/**',
        ],
    },
    ...createSlopConfig({
        inspection: 'full',
    }),
    {
        extends: [
            eslint.configs.recommended,
            ...tseslint.configs.recommendedTypeChecked,
        ],
        plugins: {
            'react-hooks': reactHooksPlugin as ESLint.Plugin,
            'react-native': eslintReactNative,
            '@stylistic': stylistic,
        },
        files: [ '**/*.{js,ts,tsx}', 'eslint.config.ts' ],
        languageOptions: {
            ecmaVersion: 'latest',
            sourceType: 'module',
            parserOptions: {
                parser: tseslint.parser,
                projectService: true,
                ecmaFeatures: {
                    jsx: true,
                },
            },
        },
        rules: {
            indent: [
                'error',
                4,
                {
                    SwitchCase: 1,
                },
            ],
            quotes: [ 'error', 'single' ],
            semi: [ 'error', 'always' ],
            camelcase: 'off',
            curly: [ 'error' ],
            'prefer-arrow-callback': 'warn',
            'prefer-destructuring': 'off',
            'object-shorthand': [ 'warn', 'always' ],
            'no-redeclare': 'off',
            'linebreak-style': [ 'warn', 'unix' ],
            'no-console': 0,
            'no-undef': 0,
            'no-unused-vars': 0,
            'eol-last': 1,
            'arrow-parens': [ 'warn', 'as-needed' ],
            'comma-dangle': [
                'error',
                {
                    arrays: 'always-multiline',
                    objects: 'always-multiline',
                    imports: 'always-multiline',
                    exports: 'always-multiline',
                    functions: 'ignore',
                },
            ],
            'object-curly-spacing': [ 'error', 'always' ],
            'array-bracket-spacing': [ 'error', 'always' ],
            'space-before-function-paren': [
                'error',
                {
                    anonymous: 'never',
                    named: 'never',
                    asyncArrow: 'never',
                },
            ],
            'lines-between-class-members': [
                'error',
                'always',
                {
                    exceptAfterSingleLine: false,
                },
            ],
            'no-multiple-empty-lines': [
                'error',
                {
                    max: 1,
                    maxEOF: 1,
                    maxBOF: 0,
                },
            ],
            '@typescript-eslint/unbound-method': 'off',
            '@typescript-eslint/no-redeclare': 'error',
            '@typescript-eslint/ban-ts-comment': 'off',
            '@typescript-eslint/no-empty-object-type': 'off',
            '@typescript-eslint/no-redundant-type-constituents': 'off',
            '@typescript-eslint/no-misused-promises': 'off',
            '@typescript-eslint/no-require-imports': 'off',
            '@typescript-eslint/no-unsafe-assignment': 'off',
            '@typescript-eslint/no-unsafe-member-access': 'off',
            '@typescript-eslint/consistent-type-imports': [
                'error',
                {
                    prefer: 'type-imports',
                    fixStyle: 'inline-type-imports',
                    disallowTypeAnnotations: false,
                },
            ],
            '@typescript-eslint/only-throw-error': 'error',
            '@typescript-eslint/no-explicit-any': 'off',
            '@typescript-eslint/no-unused-vars': [
                'error',
                {
                    ignoreRestSiblings: true,
                    argsIgnorePattern: '^_',
                },
            ],
            '@typescript-eslint/restrict-template-expressions': [
                'error',
                {
                    allow: [
                        {
                            name: [ 'Error', 'URL', 'URLSearchParams' ],
                            from: 'lib',
                        },
                        {
                            name: [ 'DocumentNode' ],
                            from: 'package',
                            package: 'graphql',
                        },
                        {
                            name: [ 'TypedDocumentNode' ],
                            from: 'package',
                            package: '@graphql-typed-document-node',
                        },
                    ],
                },
            ],
            '@stylistic/comma-spacing': [ 'error' ],
            '@stylistic/member-delimiter-style': [ 'error' ],
            '@stylistic/array-element-newline': [
                'error',
                {
                    ArrayExpression: { minItems: 5, consistent: true },
                    ArrayPattern: { minItems: 5, consistent: true },
                },
            ],
            '@stylistic/object-curly-spacing': [ 'error', 'always' ],
            '@stylistic/object-property-newline': [
                'error',
                {
                    allowAllPropertiesOnSameLine: true,
                },
            ],
            '@stylistic/object-curly-newline': [
                'error',
                {
                    ObjectExpression: {
                        minProperties: 5,
                        consistent: true,
                    },
                    ObjectPattern: {
                        minProperties: 5,
                        consistent: true,
                    },
                },
            ],
            '@stylistic/one-var-declaration-per-line': [ 'error', 'always' ],
            '@stylistic/function-paren-newline': [ 'error', 'consistent' ],
            '@stylistic/function-call-argument-newline': [
                'error',
                'consistent',
            ],
            '@stylistic/function-call-spacing': [ 'error' ],
            '@stylistic/jsx-quotes': [ 'error', 'prefer-single' ],
            '@stylistic/jsx-curly-brace-presence': [
                'error',
                {
                    props: 'always',
                    children: 'ignore',
                },
            ],
            '@stylistic/jsx-closing-bracket-location': [ 'error', 'tag-aligned' ],
            '@stylistic/brace-style': [ 'error', '1tbs' ],
            '@stylistic/curly-newline': [ 'error', 'always' ],
            '@stylistic/nonblock-statement-body-position': [ 'error', 'below' ],
            '@stylistic/padded-blocks': [ 'error', 'never' ],
            '@stylistic/padding-line-between-statements': [ 'error',
                { blankLine: 'always', prev: '*', next: 'block-like' },
                { blankLine: 'always', prev: 'block-like', next: '*' },
                { blankLine: 'always', prev: '*', next: 'return' },
                {
                    blankLine: 'always',
                    prev: '*',
                    next: [ 'multiline-const', 'multiline-let', 'multiline-var', 'multiline-expression' ],
                },
                {
                    blankLine: 'always',
                    prev: [ 'multiline-const', 'multiline-let', 'multiline-var', 'multiline-expression' ],
                    next: '*',
                },
            ],
            'react-hooks/rules-of-hooks': 'error',
            'react-hooks/exhaustive-deps': 'error',
        },
    },
    {
        files: [ '**/*.{js,mjs,cjs}', 'src/__tests__/**/*.{ts,tsx}', 'eslint.config.ts' ],
        extends: [ tseslint.configs.disableTypeChecked ],
    },
    {
        files: [ 'src/__tests__/**/*.{ts,tsx}' ],
        rules: {
            // Fixtures deliberately exercise malformed native and public API values.
            'slop/no-chained-type-assertions': 'off',
        },
    },
]);
