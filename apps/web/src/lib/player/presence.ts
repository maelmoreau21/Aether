import { get } from 'svelte/store';
import { session, getSessionGeneration } from '../auth/session';
import type { PlaybackState } from './types';
export type PlaybackRemoteCommand = 'pause' | 'play' | 'stop';
export class PlaybackPresence {
  private id=crypto.randomUUID();
  private generation=getSessionGeneration();
  private timer:ReturnType<typeof setInterval>;
  private pending=false;
  private stopped=false;
  private request:Promise<unknown>|undefined;
  constructor(private workId:string, private state:()=>PlaybackState|undefined, private onCommand?:(command:PlaybackRemoteCommand)=>void){this.timer=setInterval(()=>void this.send(),5000);void this.send();}
  ping(){void this.send();}
  private async send(){
    const current=get(session),state=this.state();
    if(this.stopped||this.pending||!current||this.generation!==getSessionGeneration()||!state||!Number.isFinite(state.duration)||state.duration<=0)return;
    this.pending=true;
    try{
      const req=fetch(`/api/v1/account/playback/${this.id}`,{method:'PUT',credentials:'same-origin',signal:AbortSignal.timeout(8000),headers:{'content-type':'application/json','x-csrf-token':current.csrfToken},body:JSON.stringify({workId:this.workId,position:Math.max(0,Math.min(state.position,state.duration)),duration:state.duration,paused:state.paused})});
      this.request=req;
      const response=await req;
      if(!this.stopped&&response.status===200&&this.onCommand){
        const payload=await response.json().catch(()=>null) as {command?:unknown}|null;
        this.pending=false;
        if(payload?.command==='pause'||payload?.command==='play'||payload?.command==='stop')this.onCommand(payload.command);
      }
    }catch{/* Presence expires if the client cannot report. */}finally{this.pending=false;}
  }
  stop(){this.stopped=true;clearInterval(this.timer);const current=get(session);if(current&&this.generation===getSessionGeneration())void Promise.resolve(this.request).catch(()=>{}).then(()=>fetch(`/api/v1/account/playback/${this.id}`,{method:'DELETE',credentials:'same-origin',keepalive:true,headers:{'x-csrf-token':current.csrfToken}})).catch(()=>{});}
}
