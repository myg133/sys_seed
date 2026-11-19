import js from '@eslint/js';
import ts from 'typescript-eslint';
import svelte from 'eslint-plugin-svelte';
import prettier from 'eslint-config-prettier';
import globals from 'globals';

export default [
  {
    ignores: [
      '**/node_modules/**',
      '**/dist/**',
      '**/target/**',
      '**/coverage/**',
      '**/.svelte-kit/**',
      '**/playwright-report/**',
    ],
  },
  js.configs.recommended,
  ...ts.configs.recommended,
  ...svelte.configs['flat/recommended'],
  prettier,
  {
    // `.svelte` 的 `<script lang="ts">` 块要交给 **TS parser** 解析 ——
    // 否则 eslint 拿默认的 JS parser 读 TS 语法（类型标注、`as`、泛型）会直接报错。
    //
    // 配了这条之后，组件里就能写 `const props: SomeProps = $props()`，
    // props **有类型**（否则退化成 `Record<string, any>`，类型安全形同虚设）。
    files: ['**/*.svelte'],
    languageOptions: {
      parserOptions: {
        parser: ts.parser,
      },
    },
  },
  {
    languageOptions: {
      globals: {
        ...globals.browser,
        ...globals.node,
      },
    },
    rules: {
      'no-console': 'warn',
    },
  },
];
