import type {Source} from './types';

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
