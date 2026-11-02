import { get } from 'svelte/store';
import { session, getSessionGeneration } from '../auth/session';
import type { PlaybackState } from './types';
export class PlaybackPresence {
  private id=crypto.randomUUID();
  private generation=getSessionGeneration();
  private timer:ReturnType<typeof setInterval>;
  private pending=false;
  private stopped=false;
  private request:Promise<unknown>|undefined;
  constructor(private workId:string, private state:()=>PlaybackState|undefined){this.timer=setInterval(()=>void this.send(),15000);void this.send();}
  private async send(){
    const current=get(session),state=this.state();
    if(this.stopped||this.pending||!current||this.generation!==getSessionGeneration()||!state||!Number.isFinite(state.duration)||state.duration<=0)return;
    this.pending=true;
    try{this.request=fetch(`/api/v1/account/playback/${this.id}`,{method:'PUT',credentials:'same-origin',signal:AbortSignal.timeout(8000),headers:{'content-type':'application/json','x-csrf-token':current.csrfToken},body:JSON.stringify({workId:this.workId,position:Math.max(0,Math.min(state.position,state.duration)),duration:state.duration,paused:state.paused})});await this.request;}catch{/* Presence expires if the client cannot report. */}finally{this.pending=false;}
  }
  stop(){this.stopped=true;clearInterval(this.timer);const current=get(session);if(current&&this.generation===getSessionGeneration())void Promise.resolve(this.request).catch(()=>{}).then(()=>fetch(`/api/v1/account/playback/${this.id}`,{method:'DELETE',credentials:'same-origin',keepalive:true,headers:{'x-csrf-token':current.csrfToken}})).catch(()=>{});}
}
