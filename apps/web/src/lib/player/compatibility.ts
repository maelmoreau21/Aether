import { tr } from '../i18n';
import type { Source, Subtitle } from './types';

// Deliberately narrow negative check. A positive capability declaration never
// proves decoding. Unknown codecs and ambiguous mixed-codec originals still
// require an actual load; do not manufacture incomplete AV1/HEVC codec strings.
export function unsupportedAudio(source:Source, canPlay:(mime:string)=>string):string|undefined {
  const tracks=source.tracks?.filter(track=>track.codec_type==='audio') ?? [];
  const selected=/^internal-(\d+)$/.exec(source.audioTrackId);
  const audios=selected ? tracks.filter(track=>track.index===Number(selected[1])) : tracks;
  if (!audios.length) return;
  const rejected=audios.filter(track=>{
    if (track.codec_name!=='dts') return false;
    const codec=track.profile?.includes('MA') ? 'dtsl' : track.profile?.includes('HRA') ? 'dtsh' : 'dtsc';
    return !canPlay(`${source.mime.split(';')[0]}; codecs="${codec}"`);
  });
  if (rejected.length!==audios.length) return;
  const ma=rejected.some(track=>track.profile?.includes('MA'));
  const mime=source.mime.split(';')[0];
  const container=({'video/x-matroska':'MKV','video/matroska':'MKV','video/mp4':'MP4','video/webm':'WebM'} as Record<string,string>)[mime] ?? mime;
  return `Audio ${ma ? 'DTS-HD MA' : 'DTS'} non pris en charge selon les capacités déclarées par ce navigateur dans ce conteneur ${container}. Aucun transcodage serveur : choisissez une autre version ou une piste compatible.`;
}

/**
 * Warns at usage time when a browser explicitly declares no support for AV1
 * video or direct Opus audio in the selected container.
 */
export function mediaCompatibilityWarning(
  source: Source | undefined,
  canPlay: (mime: string) => string
): string | undefined {
  if (!source || source.transcodeOnly) return undefined;
  const container = source.mime.split(';')[0].trim();
  if (!container) return undefined;

  const tracks = source.tracks ?? [];
  const video = tracks.find(track => track.codec_type === 'video');
  if (video?.codec_name === 'av1' && (!canPlay(container) || !canPlay(`${container}; codecs="av01.0.08M.08"`))) {
    return tr('av1_browser_warning');
  }

  // External/prepared audio routes remux or decode independently from the main container.
  if (!source.externalAudioUrl && !source.audioSegmentUrl && !source.prepare) {
    const audioTracks = tracks.filter(track => track.codec_type === 'audio');
    const selected = /^internal-(\d+)$/.exec(source.audioTrackId);
    const activeAudio = selected
      ? audioTracks.filter(track => track.index === Number(selected[1]))
      : audioTracks;
    const hasOpus = activeAudio.some(track => track.codec_name === 'opus');
    if (hasOpus && (!canPlay(container) || !canPlay(`${container}; codecs="opus"`))) {
      return tr('opus_browser_warning');
    }
  }
  return undefined;
}

/**
 * Warns when an ASS/SSA subtitle track is selected, because complex ASS/SSA
 * styles and embedded fonts are not guaranteed across all browsers.
 */
export function subtitleCompatibilityWarning(subtitle: Subtitle | null | undefined): string | undefined {
  if (!subtitle) return undefined;
  const format = subtitle.sourceFormat?.toLowerCase();
  if (subtitle.assUrl || format === 'ass' || format === 'ssa') {
    return tr('ass_style_warning');
  }
  return undefined;
}
