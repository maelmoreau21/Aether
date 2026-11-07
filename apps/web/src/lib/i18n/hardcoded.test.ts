import { describe, expect, it } from 'vitest';
import { IGNORED, countLiterals } from '../../../scripts/i18n/scan.mjs';
import baseline from './hardcoded-baseline.json';

const sources = import.meta.glob('/src/**/*.{svelte,ts}', { query: '?raw', import: 'default', eager: true }) as Record<string, string>;

// New user-visible text must go through the catalogue (`$t('key')`, `tr('key')`). A file may keep the
// sentences already recorded in hardcoded-baseline.json, never more: translating one lowers the number
// (`npm run i18n:baseline`), adding a literal fails this test.
describe('text written directly in the source', () => {
  it('does not grow beyond the recorded baseline', () => {
    const recorded = baseline as Record<string, number>;
    const grown: string[] = [];
    for (const [path, text] of Object.entries(sources)) {
      if ((IGNORED as RegExp[]).some(pattern => pattern.test(path))) continue;
      const count = countLiterals(text, path) as number;
      const allowed = recorded[path.replace(/^\/src\//, '')] ?? 0;
      if (count > allowed) grown.push(`${path}: ${count} hardcoded sentences, ${allowed} allowed`);
    }
    expect(grown, 'Use $t(...) / tr(...) with a key in src/lib/i18n/locales/{fr,en}').toEqual([]);
  });

  it('has a baseline without stale entries', () => {
    const stale = Object.keys(baseline).filter(file => !(`/src/${file}` in sources));
    expect(stale).toEqual([]);
  });
});
