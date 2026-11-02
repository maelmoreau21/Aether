import ts from 'typescript-eslint';
import svelte from 'eslint-plugin-svelte';
export default [...ts.configs.recommended, ...svelte.configs['flat/recommended'], { files: ['**/*.svelte'], languageOptions: { parserOptions: { parser: ts.parser } } }, { files: ['**/*.svelte.ts'], languageOptions: { parser: ts.parser } }];
