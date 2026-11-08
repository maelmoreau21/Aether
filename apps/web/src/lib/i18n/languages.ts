// Language choices offered for titles, artwork, audio and subtitles. They are plain tags the Core
// already understands (`language[-REGION]`, see docs/METADATA_LOCALIZATION.md): choosing one never
// implies that a translation exists, only which language is asked for first.

/** Languages offered in the pickers, most common first. Regions are kept only where they matter. */
export const METADATA_LANGUAGES = ['fr', 'fr-CA', 'en', 'en-GB', 'es', 'de', 'it', 'pt', 'pt-BR', 'nl', 'ru', 'ja', 'ko', 'zh', 'ar'] as const;

/** The Core keeps four fallbacks at most. */
export const MAX_FALLBACK_LANGUAGES = 4;

export function baseLanguage(tag: string): string {
  return tag.trim().toLowerCase().replaceAll('_', '-').split('-')[0];
}

/** Fallbacks without duplicates, without the language they back up, capped at the Core's limit. */
export function cleanFallbacks(list: readonly string[], primary: string): string[] {
  const seen = new Set<string>([primary.toLowerCase()]);
  const out: string[] = [];
  for (const tag of list) {
    const key = tag.trim().toLowerCase();
    if (!key || seen.has(key)) continue;
    seen.add(key);
    out.push(tag.trim());
    if (out.length === MAX_FALLBACK_LANGUAGES) break;
  }
  return out;
}

type LanguagePreferences = {
  locale?: string; metadataLocale?: string; metadataFallbackLocales?: string[]; artworkLocale?: string;
};

/** The language titles are asked in: the explicit choice, else the interface language. */
export function metadataLanguage(prefs: LanguagePreferences): string {
  return prefs.metadataLocale || prefs.locale || 'fr';
}

/**
 * Languages tried for artwork, best first: the explicit artwork choice, else the metadata language,
 * then its fallbacks. Same order of ideas as the Core's metadata chain, minus the server default.
 */
export function artworkLanguages(prefs: LanguagePreferences): string[] {
  const first = prefs.artworkLocale || metadataLanguage(prefs);
  return [first, ...(prefs.metadataFallbackLocales ?? [])].map(baseLanguage).filter((code, index, all) => code && all.indexOf(code) === index);
}

/** A label for caches: two different language chains never share an entry. */
export function metadataScope(prefs: LanguagePreferences): string {
  return [metadataLanguage(prefs), ...(prefs.metadataFallbackLocales ?? [])].map(tag => tag.toLowerCase()).join('>');
}
