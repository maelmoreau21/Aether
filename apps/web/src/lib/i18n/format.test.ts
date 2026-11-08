import { describe, expect, it, vi } from 'vitest';
import { compareText, formatDate, formatDateTime, formatDecimal, formatNumber, formatPercent, formatTime, languageName } from './format';
import { interpolate, translateMessage } from './index';

// 2026-11-07 13:45:00 UTC. Time zone pinned so the expectations do not depend on the machine.
const instant = Date.UTC(2026, 10, 7, 13, 45) / 1000;
vi.stubEnv('TZ', 'UTC');

describe('formats follow the interface language, not the machine', () => {
  it('writes dates in each language', () => {
    expect(formatDate('fr', instant)).toBe('7 nov. 2026');
    expect(formatDate('en', instant)).toBe('7 Nov 2026');
    expect(formatDate('fr', instant, 'long')).toBe('7 novembre 2026');
    expect(formatDate('en', instant, 'long')).toBe('7 November 2026');
    expect(formatDateTime('fr', instant, 'short')).toMatch(/07\/11\/2026.*13:45/);
    expect(formatDateTime('en', instant, 'short')).toMatch(/07\/11\/2026.*13:45/);
    expect(formatTime('fr', instant)).toBe('13:45');
  });

  it('writes numbers and ratings with the right separators', () => {
    expect(formatDecimal('fr', 7.5)).toBe('7,5');
    expect(formatDecimal('en', 7.5)).toBe('7.5');
    expect(formatNumber('fr', 1234567.5)).toMatch(/^1\s234\s567,5$/u);
    expect(formatNumber('en', 1234567.5)).toBe('1,234,567.5');
    expect(formatPercent('fr', 0.42)).toMatch(/^42\s%$/u);
    expect(formatPercent('en', 0.42)).toBe('42%');
  });

  it('shows a dash for missing or invalid values instead of throwing', () => {
    expect(formatDate('fr', null)).toBe('—');
    expect(formatDateTime('en', 'not a date')).toBe('—');
    expect(formatNumber('fr', Number.NaN)).toBe('—');
    expect(formatTime('en', undefined)).toBe('—');
  });

  it('sorts naturally and ignores case and accents', () => {
    const titles = ['Épisode 10', 'Episode 2', 'zèbre', 'Zebre', 'alpha'];
    expect([...titles].sort((a, b) => compareText('fr', a, b))).toEqual(['alpha', 'Episode 2', 'Épisode 10', 'zèbre', 'Zebre']);
    expect(compareText('en', 'Éclair', 'eclair')).toBe(0);
  });

  it('names languages in the interface language', () => {
    expect(languageName('fr', 'en')).toBe('anglais');
    expect(languageName('en', 'fr')).toBe('French');
    expect(languageName('en', 'not a language!')).toBe('not a language!');
  });
});

describe('plural forms and parameters', () => {
  it('picks singular or plural with the language rules (French counts 0 as singular)', () => {
    const text = '{count} {count#library|libraries}';
    expect(interpolate('en', text, { count: 1 })).toBe('1 library');
    expect(interpolate('en', text, { count: 0 })).toBe('0 libraries');
    expect(interpolate('en', text, { count: 2 })).toBe('2 libraries');
    const french = '{count} {count#bibliothèque|bibliothèques}';
    expect(interpolate('fr', french, { count: 0 })).toBe('0 bibliothèque');
    expect(interpolate('fr', french, { count: 1 })).toBe('1 bibliothèque');
    expect(interpolate('fr', french, { count: 5 })).toBe('5 bibliothèques');
  });

  it('renders a missing parameter as nothing, never as "undefined"', () => {
    expect(interpolate('en', 'Hello {name}!', { name: undefined })).toBe('Hello !');
    expect(translateMessage('en', 'home', { unused: 1 })).toBe('Home');
  });
});
