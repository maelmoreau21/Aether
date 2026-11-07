// Statistics and Wrapped strings live in the shared catalogue (`i18n/locales/*/stats.ts`, exposed as
// `stats_<key>`). This module keeps the short, typed `st('key')` helper used by the statistics pages.
import { derived } from 'svelte/store';
import { locale, translateMessage, type SupportedLocale, type TranslationKey } from '../i18n';
import { intlTag } from '../i18n/format';
import type { stats } from '../i18n/locales/fr/stats';

export type StatKey = keyof typeof stats;
export type StatParams = Record<string, string | number>;

export function translate(language: string, key: StatKey, params: StatParams = {}): string {
  return translateMessage(language === 'en' ? 'en' : 'fr', `stats_${key}` as TranslationKey, params);
}
export const st = derived(locale, $locale => (key: StatKey, params: StatParams = {}) => translateMessage($locale, `stats_${key}` as TranslationKey, params));
/** BCP 47 tag for Intl formatting. */
export const statLocale = derived(locale, ($locale: SupportedLocale) => intlTag($locale));
