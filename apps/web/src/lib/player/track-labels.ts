import type { Source, CodecTrack } from './types';

const displayNames = typeof Intl !== 'undefined' && Intl.DisplayNames ? new Intl.DisplayNames(['fr'], { type: 'language' }) : null;

export function languageLabel(language: string) {
  try { return displayNames?.of(language) ?? language; }
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

export function videoResolutionBadge(width?: number, height?: number): string | null {
  if (!width && !height) return null;
  const h = height ?? 0;
  const w = width ?? 0;
  if (w >= 3800 || h >= 2100) return '4K Ultra HD';
  if (w >= 1900 || h >= 1050) return '1080p Full HD';
  if (w >= 1200 || h >= 700) return '720p HD';
  return 'SD';
}

export function containerLabel(mime?: string): string {
  if (mime === 'video/mp4') return 'MPEG-4 (.mp4)';
  if (mime === 'video/x-matroska') return 'Matroska (.mkv)';
  if (mime === 'video/webm') return 'WebM (.webm)';
  return mime || 'Conteneur vidéo';
}

export function colorProfileLabel(pix_fmt?: string): string | null {
  if (!pix_fmt) return null;
  if (pix_fmt.includes('10')) return '10 bits (Plage étendue)';
  if (pix_fmt.includes('12')) return '12 bits (Plage étendue)';
  return '8 bits standard';
}
