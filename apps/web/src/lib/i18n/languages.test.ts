import { describe, expect, it } from 'vitest';
import { MAX_FALLBACK_LANGUAGES, artworkLanguages, baseLanguage, cleanFallbacks, metadataLanguage, metadataScope } from './languages';

describe('language preferences', () => {
  it('follows the interface language until a metadata language is chosen', () => {
    expect(metadataLanguage({ locale: 'en' })).toBe('en');
    expect(metadataLanguage({ locale: 'en', metadataLocale: 'fr-CA' })).toBe('fr-CA');
    expect(metadataLanguage({})).toBe('fr');
  });

  it('keeps artwork independent from the interface and from the metadata language', () => {
    expect(artworkLanguages({ locale: 'fr' })).toEqual(['fr']);
    expect(artworkLanguages({ locale: 'fr', metadataLocale: 'ja' })).toEqual(['ja']);
    expect(artworkLanguages({ locale: 'fr', metadataLocale: 'ja', artworkLocale: 'en-GB', metadataFallbackLocales: ['fr', 'en'] })).toEqual(['en', 'fr']);
  });

  it('drops duplicates and the primary language, and caps fallbacks at four', () => {
    expect(cleanFallbacks(['en', 'EN', 'fr', 'de', 'es', 'it', 'pt'], 'fr')).toEqual(['en', 'de', 'es', 'it']);
    expect(cleanFallbacks(['en', 'de', 'es', 'it', 'pt'], 'fr')).toHaveLength(MAX_FALLBACK_LANGUAGES);
    expect(cleanFallbacks([' ', ''], 'fr')).toEqual([]);
  });

  it('builds a cache scope that differs for every language chain', () => {
    expect(metadataScope({ locale: 'fr' })).toBe('fr');
    expect(metadataScope({ locale: 'en', metadataFallbackLocales: ['fr'] })).toBe('en>fr');
    expect(metadataScope({ locale: 'fr' })).not.toBe(metadataScope({ locale: 'en' }));
    expect(metadataScope({ locale: 'fr', metadataLocale: 'en-GB' })).toBe('en-gb');
  });

  it('reduces a tag to its base language', () => {
    expect(baseLanguage('pt_BR')).toBe('pt');
    expect(baseLanguage(' EN-gb ')).toBe('en');
  });
});
