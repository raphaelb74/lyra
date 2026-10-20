import js from '@eslint/js';
import { defineConfig } from 'eslint/config';
import svelte from 'eslint-plugin-svelte';
import globals from 'globals';
import ts from 'typescript-eslint';

import antiSlop from './tools/anti-slop/index.ts';
import svelteConfig from './svelte.config.js';

export default defineConfig(
  { ignores: ['dist/', 'node_modules/', 'src-tauri/', 'tools/'] },

  js.configs.recommended,
  ts.configs.strictTypeChecked,
  ts.configs.stylisticTypeChecked,
  svelte.configs.recommended,
  svelte.configs.prettier,

  {
    languageOptions: {
      globals: globals.browser,
      parserOptions: {
        projectService: true,
        extraFileExtensions: ['.svelte'],
        tsconfigRootDir: import.meta.dirname,
      },
    },
  },
  {
    files: ['**/*.svelte', '**/*.svelte.ts'],
    languageOptions: {
      parserOptions: { parser: ts.parser, svelteConfig },
    },
  },

  {
    plugins: { 'anti-slop': antiSlop },
    rules: {
      'anti-slop/no-array-filter-map': 'error',
      'anti-slop/no-reduce-accumulator-copy': 'error',
      'anti-slop/no-chained-type-assertions': 'error',
      'anti-slop/no-conditional-empty-object-spread': 'error',
      'anti-slop/no-known-value-widening': 'error',
      'anti-slop/no-module-mocking': 'error',
      'anti-slop/no-object-parameters': 'error',
      'anti-slop/no-reflect-apply': 'error',
      'anti-slop/no-reflect-get': 'error',
      // `typeof` only inside dedicated type guards that parse IPC data at the boundary.
      'anti-slop/no-runtime-typeof': ['error', { allowInTypeGuards: true }],
      'anti-slop/no-shape-in-symbol-names': 'error',
      'anti-slop/no-unknown-parameters': 'error',
      'anti-slop/no-unknown-returns': 'error',
      'anti-slop/no-unknown-type-aliases': 'error',
      'anti-slop/no-unsafe-dictionary-type': 'error',
      'anti-slop/no-widen-then-assert': 'error',
      'anti-slop/require-readable-spacing': 'error',
      'anti-slop/require-safety-comment-for-type-assertion': 'error',
    },
  },

  // Build configuration files are outside the app's tsconfig.
  {
    files: ['*.config.js', '*.config.ts'],
    extends: [ts.configs.disableTypeChecked],
    languageOptions: { globals: globals.node },
  },
);
