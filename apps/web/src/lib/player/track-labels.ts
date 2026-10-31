import type { Source, CodecTrack, Media } from './types';

const displayNames = typeof Intl !== 'undefined' && Intl.DisplayNames ? new Intl.DisplayNames(['fr'], { type: 'language' }) : null;

export function languageLabel(language: string) {
  const normalized = ({ fra:'fr', fre:'fr', eng:'en', spa:'es', ger:'de', deu:'de', ita:'it', jpn:'ja', zho:'zh', chi:'zh', nld:'nl', dut:'nl', hin:'hi', por:'pt', rus:'ru', und:'' } as Record<string,string>)[language?.toLowerCase()] ?? language;
  if (!normalized) return 'Langue non renseignée';
  try { return displayNames?.of(normalized) ?? normalized; }
  catch { return language; }
}

export function audioLabel(source: Source) {
  if (source.audioTrackId === 'internal-default' && (source.tracks?.filter(track => track.codec_type === 'audio').length ?? 0) > 1) return 'Original · piste choisie par le navigateur';
  const track = source.tracks?.find(track => track.codec_type === 'audio');
  const channels = track?.channel_layout ?? (track?.channels === 2 ? 'stéréo' : track?.channels === 1 ? 'mono' : track?.channels ? `${track.channels} canaux` : '');
  return [languageLabel(source.language), track?.tags?.title, track?.codec_name?.toUpperCase(), channels, source.externalAudioUrl && !source.prepare ? 'externe' : source.label].filter(Boolean).join(' · ');
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
  if (pix_fmt.includes('10')) return '10 bits';
  if (pix_fmt.includes('12')) return '12 bits';
  return '8 bits standard';
}

export function audioChannelBadge(track?: CodecTrack): string | null {
  if (!track) return null;
  const layout = track.channel_layout?.toLowerCase().trim() ?? '';
  if (layout.startsWith('7.2')) return '7.2';
  if (layout.startsWith('7.1')) return '7.1';
  if (layout.startsWith('6.1')) return '6.1';
  if (layout.startsWith('5.2')) return '5.2';
  if (layout.startsWith('5.1')) return '5.1';
  if (layout.startsWith('5.0')) return '5.0';
  if (layout.startsWith('4.1')) return '4.1';
  if (layout.startsWith('4.0') || layout === 'quad') return '4.0';
  if (layout.startsWith('3.1')) return '3.1';
  if (layout.startsWith('3.0')) return '3.0';
  if (layout.startsWith('2.1')) return '2.1';
  if (layout === 'stereo' || layout.startsWith('2.0')) return '2.0';
  if (layout === 'mono' || layout.startsWith('1.0')) return '1.0';
  switch (track.channels) {
    case 1: return '1.0';
    case 2: return '2.0';
    case 3: return '2.1';
    case 4: return '4.0';
    case 5: return '5.0';
    case 6: return '5.1';
    case 7: return '6.1';
    case 8: return '7.1';
    default: return track.channels ? `${track.channels}.0` : (track.channel_layout || null);
  }
}

/** Inventory, not a claim that every track is playable by this browser. */
export function inspectedAudioTracks(media?: Media): (CodecTrack & { language:string })[] {
  const unique = new Map<string, CodecTrack & {language:string}>();
  for (const source of media?.sources ?? []) {
    const external = source.audioTrackId.startsWith('external-');
    for (const [ordinal, track] of (source.tracks ?? []).entries()) {
      if (track.codec_type !== 'audio') continue;
      const key = `${external ? source.audioTrackId : 'internal'}:${track.index ?? `${source.id}:${ordinal}`}`;
      unique.set(key, {...track, language:track.tags?.language || (external ? source.language : 'und')});
    }
  }
  return [...unique.values()];
}
