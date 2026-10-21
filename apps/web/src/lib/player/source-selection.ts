import type { Media, Source } from './types';

export function audioLanguage(language: string): string {
  const primary = language.trim().toLowerCase().replaceAll('_', '-').split('-')[0];
  return ({ fra: 'fr', fre: 'fr', eng: 'en', deu: 'de', ger: 'de' } as Record<string, string>)[primary] ?? primary;
}

// An original multi-audio file does not expose a selectable track through HTMLMediaElement.
// Prefer a concrete track over that ambiguous source, without overriding a manual choice.
export function chooseSource(media: Media, trackId?: string, language = 'fr', playable: (source: Source) => boolean = () => true): Source | undefined {
  const manual = media.sources.find(source => source.audioTrackId === trackId);
  if (manual) return manual;
  const candidates = media.sources.filter(playable);
  const concrete = (source: Source) => source.audioTrackId !== 'internal-default' || (source.tracks?.filter(track => track.codec_type === 'audio').length ?? 0) === 1;
  for (const wanted of new Set([audioLanguage(language), 'fr'])) {
    const matches = candidates.filter(source => audioLanguage(source.language) === wanted);
    const source = matches.find(source => concrete(source) && !source.tracks?.some(track => track.disposition?.comment === 1 || track.disposition?.hearing_impaired === 1))
      ?? matches.find(concrete);
    if (source) return source;
  }
  return candidates.find(concrete) ?? candidates[0] ?? media.sources[0];
}

// This is only a capability hint. Successful metadata, decoding and seek remain mandatory.
export function playbackMime(media: Media, source: Source): string {
  const tracks = source.tracks?.some(track => track.codec_type === 'video') ? source.tracks : media.sources.find(item => item.tracks?.some(track => track.codec_type === 'video'))?.tracks;
  const video = tracks?.find(track => track.codec_type === 'video');
  const audio = source.tracks?.find(track => track.codec_type === 'audio');
  if (source.mime !== 'video/mp4' || video?.codec_name !== 'av1') return source.mime;
  return `video/mp4; codecs="av01${!source.externalAudioUrl && audio?.codec_name === 'opus' ? ',opus' : ''}"`;
}
