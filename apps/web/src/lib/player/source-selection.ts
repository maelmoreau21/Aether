import type { Media, Source } from './types';

export function audioLanguage(language: string): string {
  const primary = language.trim().toLowerCase().replaceAll('_', '-').split('-')[0];
  return ({ fra: 'fr', fre: 'fr', eng: 'en', deu: 'de', ger: 'de' } as Record<string, string>)[primary] ?? primary;
}

// An original multi-audio file does not expose a selectable track through HTMLMediaElement.
// Prefer a concrete track over that ambiguous source, without overriding a manual choice.
export function chooseSource(media: Media, trackId?: string, language = 'fr', playable: (source: Source) => boolean = () => true): Source | undefined {
  const manual = media.sources.find(source => source.audioTrackId === trackId);
  // Migrate old preferences that saved the browser-selected multi-audio original
  // as though it identified a language. Explicit indexed choices remain authoritative.
  const ambiguous = (source: Source) => source.audioTrackId === 'internal-default' &&
    (source.tracks?.filter(track => track.codec_type === 'audio').length ?? 0) > 1;
  if (manual && !ambiguous(manual)) return manual;
  // A capability hint must not discard a language: only a real load/decode can
  // establish failure. Prefer a declared playable packaging within that language.
  const candidates = [...media.sources.filter(playable), ...media.sources.filter(source => !playable(source))];
  const concrete = (source: Source) => source.audioTrackId !== 'internal-default' || (source.tracks?.filter(track => track.codec_type === 'audio').length ?? 0) === 1;
  for (const wanted of new Set([audioLanguage(language), 'fr'])) {
    const matches = candidates.filter(source => audioLanguage(source.language) === wanted);
    const source = matches.find(source => concrete(source) && !source.tracks?.some(track => track.disposition?.comment === 1 || track.disposition?.hearing_impaired === 1))
      ?? matches.find(concrete);
    if (source) return source;
  }
  return candidates.find(concrete) ?? candidates[0] ?? media.sources[0];
}

export function audioSelectionNotice(media: Media, source: Source, language: string): string {
  const wanted = audioLanguage(language);
  if (audioLanguage(source.language) === wanted || media.sources.some(item => audioLanguage(item.language) === wanted)) return '';
  if (audioLanguage(source.language) === 'und') return 'La langue de la piste choisie par le navigateur ne peut pas être confirmée.';
  return 'Votre langue précédente est indisponible. Première source disponible sélectionnée.';
}

// This is only a capability hint. Successful metadata, decoding and seek remain mandatory.
export function playbackMime(_media: Media, source: Source): string {
  // AV1 requires profile, level/tier and bit depth in a codecs parameter.
  // The descriptor lacks the complete codec configuration; bare "av01" is
  // rejected by Chrome even when the actual 10-bit file decodes successfully.
  return source.mime;
}
