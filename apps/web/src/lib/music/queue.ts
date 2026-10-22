import {get,writable} from 'svelte/store';
export interface MusicTrack {id:string;title:string;artist:string;album:string;albumArtist?:string;mime:string;codec:string;duration:number|null}
export const musicQueue=writable<{tracks:MusicTrack[];index:number;playing:boolean;repeat:'off'|'all'|'one'}>({tracks:[],index:0,playing:false,repeat:'off'});
export function playMusic(tracks:MusicTrack[],index=0){if(!tracks.length)return;const start=Math.max(0,Math.min(index,tracks.length-1));const first=Math.max(0,start-100);musicQueue.set({tracks:tracks.slice(first,first+500),index:start-first,playing:true,repeat:get(musicQueue).repeat});}
export function enqueueMusic(track:MusicTrack){musicQueue.update(q=>q.tracks.length>=500?q:{...q,tracks:[...q.tracks,track]});}
export function nextMusic(ended=false){musicQueue.update(q=>{
  if(ended&&q.repeat==='one')return {...q,playing:true};
  if(q.index+1<q.tracks.length)return {...q,index:q.index+1,playing:true};
  return q.repeat==='all'?{...q,index:0,playing:true}:{...q,playing:false};
});}
export function removeMusic(index:number){musicQueue.update(q=>{const tracks=q.tracks.filter((_,i)=>i!==index);return {...q,tracks,index:Math.max(0,Math.min(q.index-(index<q.index?1:0),tracks.length-1)),playing:tracks.length>0&&q.playing};});}
export function shuffleMusic(){musicQueue.update(q=>{if(!q.tracks.length)return q;const current=q.tracks[q.index],rest=q.tracks.filter((_,i)=>i!==q.index);for(let i=rest.length-1;i>0;i--){const j=Math.floor(Math.random()*(i+1));[rest[i],rest[j]]=[rest[j],rest[i]];}return {...q,tracks:[current,...rest],index:0};});}
export function clearMusic(){musicQueue.set({tracks:[],index:0,playing:false,repeat:'off'});}
