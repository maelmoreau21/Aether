import { describe, expect, it } from 'vitest';
import { domains, messages, type TranslationKey } from './messages';
import { translateMessage } from './index';

// `{name}` and the plural form `{name#one|other}` both reference the parameter `name`.
const placeholders = (text: string) => [...new Set([...text.matchAll(/\{([A-Za-z0-9_]+)(?:#[^|{}]*\|[^{}]*)?\}/g)].map(match => match[1]))].sort().join(',');
const frKeys = Object.keys(messages.fr) as TranslationKey[];

describe('translation catalogue', () => {
  it('has the same keys in French and English', () => {
    const en = new Set(Object.keys(messages.en));
    const fr = new Set(frKeys);
    expect(frKeys.filter(key => !en.has(key)), 'keys missing in English').toEqual([]);
    expect([...en].filter(key => !fr.has(key as TranslationKey)), 'keys missing in French').toEqual([]);
  });

  it('declares each key in exactly one domain', () => {
    for (const language of ['fr', 'en'] as const) {
      const owner = new Map<string, string>();
      const duplicated: string[] = [];
      for (const [domain, table] of Object.entries(domains[language])) {
        for (const key of Object.keys(table)) {
          if (owner.has(key)) duplicated.push(`${key} (${owner.get(key)}, ${domain})`);
          owner.set(key, domain);
        }
      }
      expect(duplicated, `${language} duplicates`).toEqual([]);
    }
  });

  it('keeps French and English domains aligned', () => {
    for (const [domain, table] of Object.entries(domains.fr)) {
      expect(Object.keys(domains.en[domain as keyof typeof domains.en]).sort(), domain).toEqual(Object.keys(table).sort());
    }
  });

  it('uses the same placeholders in both languages', () => {
    const mismatched = frKeys.filter(key => placeholders(messages.fr[key]) !== placeholders(messages.en[key]));
    expect(mismatched.map(key => `${key}: ${messages.fr[key]} / ${messages.en[key]}`)).toEqual([]);
  });

  it('has no empty or unbalanced messages', () => {
    const bad: string[] = [];
    for (const language of ['fr', 'en'] as const) {
      for (const key of frKeys) {
        const text = messages[language][key];
        if (typeof text !== 'string' || text.trim() === '') bad.push(`${language}.${key}: empty`);
        else if ((text.match(/\{/g) ?? []).length !== (text.match(/\}/g) ?? []).length) bad.push(`${language}.${key}: unbalanced braces`);
      }
    }
    expect(bad).toEqual([]);
  });

  it('does not leave French sentences in the English catalogue', () => {
    const frenchOnly = /[àâçèéêëîïôùûœ]/i;
    // Proper nouns and brand names that legitimately keep their French spelling.
    const allowed = new Set<string>(['language_fr']); // endonym: the language name is written in that language
    const suspicious = frKeys.filter(key => !allowed.has(key) && frenchOnly.test(messages.en[key]));
    expect(suspicious.map(key => `${key}: ${messages.en[key]}`)).toEqual([]);
  });

  it('falls back to French, then to the key, instead of throwing', () => {
    expect(translateMessage('en', 'home')).toBe(messages.en.home);
    expect(translateMessage('fr', 'definitely_not_a_key' as TranslationKey)).toBe('definitely_not_a_key');
  });

  it('interpolates every placeholder', () => {
    const key = frKeys.find(candidate => /\{count\}/.test(messages.fr[candidate]) && /\{count\}/.test(messages.en[candidate]));
    expect(key).toBeDefined();
    for (const language of ['fr', 'en'] as const) {
      expect(translateMessage(language, key!, { count: 42 })).toContain('42');
      expect(translateMessage(language, key!, { count: 42 })).not.toContain('{count}');
    }
  });
});

// Source files are read as text so a key used in a component but absent from the catalogue fails here
// instead of rendering the raw identifier in production.
const sources = import.meta.glob('/src/**/*.{svelte,ts}', { query: '?raw', import: 'default', eager: true }) as Record<string, string>;
const usable = Object.entries(sources).filter(([path]) => !path.includes('/src/lib/i18n/') && !path.endsWith('/statistics/i18n.ts') && !path.endsWith('.test.ts'));

describe('translation keys used by the application', () => {
  const direct = /(?<![A-Za-z0-9_.])\$?(t|tr)\(\s*['"]([A-Za-z0-9_]+)['"]/g;
  const stat = /(?<![A-Za-z0-9_.])\$?st\(\s*['"]([A-Za-z0-9_]+)['"]/g;

  it('only references keys that exist', () => {
    const known = new Set<string>(frKeys);
    const missing: string[] = [];
    for (const [path, text] of usable) {
      for (const match of text.matchAll(direct)) if (!known.has(match[2])) missing.push(`${path}: ${match[2]}`);
      for (const match of text.matchAll(stat)) if (!known.has(`stats_${match[1]}`)) missing.push(`${path}: stats_${match[1]}`);
    }
    expect(missing).toEqual([]);
  });

  it('has no unused keys', () => {
    const corpus = usable.map(([, text]) => text).join('\n');
    // Dynamic lookups (`sched_day_${n}`) are matched by their literal prefix.
    const prefixes = [...corpus.matchAll(/['"`]([A-Za-z0-9_]+_)\$\{/g)].map(match => match[1]);
    const unused = frKeys.filter(key => {
      if (key.startsWith('stats_')) {
        const name = key.slice('stats_'.length);
        return !new RegExp(`['"\`]${name}['"\`]`).test(corpus) && !prefixes.some(prefix => name.startsWith(prefix));
      }
      return !corpus.includes(`'${key}'`) && !corpus.includes(`"${key}"`) && !corpus.includes(`\`${key}\``) && !prefixes.some(prefix => key.startsWith(prefix));
    });
    expect(unused).toEqual([]);
  });
});
