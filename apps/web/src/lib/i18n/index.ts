import { derived, fromStore, writable } from 'svelte/store';
import { messages, type SupportedLocale, type TranslationKey } from './messages';
import { compareText, formatDate, formatDateTime, formatDecimal, formatNumber, formatPercent, formatTime, intlTag, languageName, type DateInput } from './format';

export type { SupportedLocale, TranslationKey } from './messages';
/** Missing values (`undefined`/`null`) render as an empty string instead of the word "undefined". */
export type TranslationParams = Record<string, string | number | null | undefined>;
const supportedLocales = Object.keys(messages) as SupportedLocale[];
export const localeOptions = supportedLocales.map(code => ({ code, label: code.toUpperCase() }));

function isSupportedLocale(value: string): value is SupportedLocale {
  return supportedLocales.includes(value as SupportedLocale);
}

export function detectLocale(value: string | null | undefined): SupportedLocale {
  const normalized = value?.toLowerCase() ?? '';
  return supportedLocales.find(candidate => normalized.startsWith(candidate)) ?? 'fr';
}

export function getInitialLocale(): SupportedLocale {
  if (typeof window === 'undefined') return 'fr';
  let saved: string | null = null;
  try { saved = localStorage.getItem('media_locale'); } catch { /* Optional cache. */ }
  if (saved && isSupportedLocale(saved)) return saved;
  return detectLocale(navigator.language);
}

export const locale = writable<SupportedLocale>(getInitialLocale());

/**
 * The account preference store registers itself here (see `preferences.ts`). Keeping the dependency
 * inverted lets low-level modules such as the session import `tr` without an import cycle.
 */
let persistLocale: ((value: SupportedLocale) => Promise<unknown>) | undefined;
export function registerLocalePersistence(save: (value: SupportedLocale) => Promise<unknown>): void {
  persistLocale = save;
}

export function setLocale(newLocale: SupportedLocale, persist = true) {
  locale.set(newLocale);
  // The server renders titles and summaries in the account's saved language: caches filled
  // in the previous language are dropped only once that preference is stored.
  if (persist && persistLocale) void persistLocale(newLocale).finally(() => {
    if (typeof window !== 'undefined') window.dispatchEvent(new Event('media-locale-saved'));
  });
  if (typeof window !== 'undefined') {
    try { localStorage.setItem('media_locale', newLocale); } catch { /* Optional cache. */ }
    document.documentElement.lang = newLocale;
  }
}

const reportedMissing = new Set<string>();

/** Pure lookup: English falls back to French, then to the key itself so a gap stays visible. */
export function translateMessage(language: SupportedLocale, key: TranslationKey, params: TranslationParams = {}): string {
  const found = messages[language]?.[key] ?? messages.fr[key];
  if (found === undefined && import.meta.env?.DEV && !reportedMissing.has(key)) {
    reportedMissing.add(key);
    console.warn(`[i18n] missing translation key: ${key}`);
  }
  const text: string = found ?? key;
  return interpolate(language, text, params);
}

const pluralRules = new Map<SupportedLocale, Intl.PluralRules>();

/**
 * `{name}` is replaced by the parameter; `{count#singular|plural}` picks a form with the language's
 * plural rules (French treats 0 and 1 as singular, English only 1), so “1 library” / “2 libraries”
 * need no `(s)` workaround.
 */
export function interpolate(language: SupportedLocale, text: string, params: TranslationParams = {}): string {
  if (!text.includes('{')) return text;
  const withPlurals = text.replace(/\{([A-Za-z0-9_]+)#([^|{}]*)\|([^{}]*)\}/g, (whole, name: string, one: string, other: string) => {
    if (!(name in params)) return whole;
    let rules = pluralRules.get(language);
    if (!rules) { rules = new Intl.PluralRules(intlTag(language)); pluralRules.set(language, rules); }
    return rules.select(Number(params[name])) === 'one' ? one : other;
  });
  return Object.entries(params).reduce((result, [name, value]) => result.replaceAll(`{${name}}`, value == null ? '' : String(value)), withPlurals);
}

export const t = derived(locale, ($locale) => (key: TranslationKey, params: TranslationParams = {}): string =>
  translateMessage($locale, key, params));

// Reading the language through `fromStore` makes `tr()` reactive wherever Svelte tracks reads (a template
// expression, a `$derived`, an `$effect`), so a label computed by a plain module follows a language change
// without the component naming `$t`. Outside a reactive context it is just the current value.
const reactiveLocale = fromStore(locale);

/** Translation for plain modules (errors, labels): reads the current language, reactively in templates. */
export function tr(key: TranslationKey, params: TranslationParams = {}): string {
  return translateMessage(reactiveLocale.current, key, params);
}

/**
 * An object whose values are translated each time they are read, for module-level label tables:
 * `statusLabels[status]` keeps working in a template and follows the language.
 */
export function translatedLabels<K extends string>(keys: Readonly<Record<K, TranslationKey>>): Readonly<Record<K, string>> {
  return new Proxy({} as Record<K, string>, {
    get: (_target, property) => (typeof property === 'string' && Object.hasOwn(keys, property) ? tr(keys[property as K]) : undefined),
    has: (_target, property) => typeof property === 'string' && Object.hasOwn(keys, property),
    ownKeys: () => Object.keys(keys),
    getOwnPropertyDescriptor: (_target, property) =>
      typeof property === 'string' && Object.hasOwn(keys, property)
        ? { enumerable: true, configurable: true, value: tr(keys[property as K]) }
        : undefined
  });
}

/** The interface language, for plain modules. */
export function currentLocale(): SupportedLocale {
  return reactiveLocale.current;
}

/** Formatters bound to the interface language. `$fmt.date(ts)` re-renders when the language changes. */
export const fmt = derived(locale, ($locale) => ({
  locale: $locale,
  tag: intlTag($locale),
  date: (value: DateInput | null | undefined, style?: 'short' | 'medium' | 'long') => formatDate($locale, value, style),
  dateTime: (value: DateInput | null | undefined, style?: 'short' | 'medium') => formatDateTime($locale, value, style),
  time: (value: DateInput | null | undefined) => formatTime($locale, value),
  number: (value: number | null | undefined, options?: Intl.NumberFormatOptions) => formatNumber($locale, value, options),
  decimal: (value: number | null | undefined, digits?: number) => formatDecimal($locale, value, digits),
  percent: (ratio: number | null | undefined, digits?: number) => formatPercent($locale, ratio, digits),
  compare: (a: string, b: string) => compareText($locale, a, b),
  language: (code: string) => languageName($locale, code)
}));

/** Non-reactive counterpart of `fmt` for plain modules. */
const reactiveFormat = fromStore(fmt);
export function currentFormat() {
  return reactiveFormat.current;
}
