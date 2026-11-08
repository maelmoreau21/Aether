import { derived, get } from 'svelte/store';
import { session } from '../auth/session';
import { locale } from '../i18n';
import { metadataLanguage, metadataScope } from '../i18n/languages';
import { preferenceState } from '../preferences';

// Catalogue data is rendered by the server for one account in one language chain. Everything that
// caches it (pages, descriptors) puts this scope in its key, so a response for another account or
// another language can neither be served nor stored under the wrong key, whatever the timing of
// a logout, a language change or a request that is still in flight.

/** The language chain of the current reader, from the interface language and saved preferences. */
export const languageScope = derived([locale, preferenceState], ([$locale, $preferences]) =>
  metadataScope({ ...$preferences, locale: $locale }));

/** `<account>|<language chain>`. */
export function cacheScope(): string {
  const account = get(session)?.user.id ?? 'anonymous';
  return `${account}|${metadataScope({ ...get(preferenceState), locale: get(locale) })}`;
}

/** Cache key for something that depends on who reads it and in which language. */
export function scopedKey(key: string): string {
  return `${cacheScope()}\n${key}`;
}

/** The part after the scope, for code that needs to inspect the original key. */
export function unscopedKey(key: string): string {
  const at = key.indexOf('\n');
  return at < 0 ? key : key.slice(at + 1);
}

/**
 * Add the metadata language to a catalogue request so the response, and therefore its cache entry,
 * is for exactly the language in the scope, even while a preference save is still travelling.
 */
export function withMetadataLocale(url: string): string {
  if (!url.startsWith('/api/v1/catalog')) return url;
  const parsed = new URL(url, 'http://local');
  if (parsed.searchParams.has('locale')) return url;
  parsed.searchParams.set('locale', metadataLanguage({ ...get(preferenceState), locale: get(locale) }));
  return `${parsed.pathname}${parsed.search}`;
}
