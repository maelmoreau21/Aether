/* eslint-disable @typescript-eslint/ban-ts-comment */
// @ts-nocheck -- build tooling parsed with the Svelte and TypeScript compilers, not application code.
// Counts the user-visible sentences still written directly in a source file. Used by the guard test
// (src/lib/i18n/hardcoded.test.ts) and by `npm run i18n:baseline`.
import { findTemplateLiterals } from './literals.mjs';
import { findScriptLiterals, findSvelteScriptLiterals, findSvelteTemplateExpressionLiterals } from './script-literals.mjs';

/** Files whose French text is intentional: stable identifiers, mojibake repair table, SVG paths. */
export const IGNORED = [
  /\/lib\/i18n\//, /\.test\.ts$/, /\.d\.ts$/, /\/lib\/admin\/plugin-text\.ts$/, /\/lib\/ui\/Icon\.svelte$/,
  // Administration sidebar: French labels are the canonical search index, with English keywords beside them.
  /\/lib\/admin\/navigation\.ts$/
];

const technical = text => /^M[\d{]/.test(text) || text === 'Cache invalidated';

/** Number of hardcoded sentences in `source` (a .svelte or .ts file). */
export function countLiterals(source, file) {
  if (file.endsWith('.svelte')) {
    const template = findTemplateLiterals(source).literals.length;
    const script = findSvelteScriptLiterals(source).found.filter(item => !technical(item.text)).length;
    const expressions = findSvelteTemplateExpressionLiterals(source).found.filter(item => !technical(item.text)).length;
    return template + script + expressions;
  }
  return findScriptLiterals(source, 0, file).found.filter(item => !technical(item.text)).length;
}
