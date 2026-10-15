import type { Source, CodecTrack } from './types';
export function languageLabel(language: string) {
  try { return new Intl.DisplayNames(['fr'], { type: 'language' }).of(language) ?? language; }
  catch { return language; }
}
export function audioLabel(source: Source) {
  const track = source.tracks?.find(track => track.codec_type === 'audio');
  const channels = track?.channel_layout ?? (track?.channels === 2 ? 'stéréo' : track?.channels === 1 ? 'mono' : track?.channels ? `${track.channels} canaux` : '');
  return [languageLabel(source.language), track?.codec_name?.toUpperCase(), channels].filter(Boolean).join(' · ');
}
export function videoLabel(track: CodecTrack) {
  return [track.codec_name?.toUpperCase(), track.width && track.height ? `${track.width} × ${track.height}` : '', track.pix_fmt?.includes('10') ? '10 bits' : ''].filter(Boolean).join(' · ') || 'Informations indisponibles';
}
