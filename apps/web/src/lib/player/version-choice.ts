import type { Media } from './types';
// Browser declarations are a selection hint; actual decoding errors stay explicit in the player.
export function bestVersion(media: Media[], canPlay: (mime:string)=>string): Media | undefined {
  const candidates=media.filter(item=>item.available!==false&&item.nodeStatus!=='disconnected'&&item.nodeStatus!=='revoked');
  function score(item:Media){
    let best=-1;
    for(const source of item.sources){
      const video=source.tracks?.find(track=>track.codec_type==='video');
      const codec=video?.codec_name;
      const declared=codec==='av1'?'av01.0.08M.08':codec==='h264'?'avc1.640028':codec==='hevc'?'hvc1.1.6.L120.B0':undefined;
      if(!canPlay(source.mime)||declared&&!canPlay(`${source.mime}; codecs="${declared}"`))continue;
      const pixels=(video?.width??0)*(video?.height??0);
      const transport=(source.nodeId==='local'||item.nodeId==='local'?3:source.directUrl?2:1);
      const nodeBonus=((source.score??item.score??0)/10000);
      best=Math.max(best,pixels*10+transport+nodeBonus-(source.prepare?0.5:0));
    }
    return best;
  }
  return candidates.map(item=>({item,score:score(item)})).filter(entry=>entry.score>=0).sort((a,b)=>b.score-a.score||a.item.id.localeCompare(b.item.id))[0]?.item;
}
