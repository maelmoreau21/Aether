import {bufferedAhead} from './startup-buffer';
import {mp4TimelineOffset} from './mp4-timeline';
import {Mp4Fragments,mp4Timescale,mp4RandomAccess} from './mp4-fragments';
import {EncodedVideoBuffer} from './encoded-video-buffer';

// A packet-copy fallback inside NativePlayer. One continuous FFmpeg response
// avoids replacing a playing GOP with overlapping independently muxed windows.
export class ProgressiveVideo {
  static supported(mime: string) {return typeof MediaSource !== 'undefined' && MediaSource.isTypeSupported(mime);}
  private source = new MediaSource();
  readonly url = URL.createObjectURL(this.source);
  private buffer?: SourceBuffer;
  private controller?: AbortController;
  private epoch = 0;
  private closed = false;
  private active = false;
  private restarting = false;
  private retained = new EncodedVideoBuffer();
  private operation: Promise<void> = Promise.resolve();
  private rejectOpen?: (error: Error) => void;
  private opened: Promise<void>;
  private rejectFailure!: (error: Error) => void;
  readonly failure = new Promise<never>((_,reject)=>{this.rejectFailure=reject;});
  private failed?: Error;
  constructor(private video: HTMLVideoElement,private prefix: string,private duration: number,private mime: string,private onError: (message: string)=>void) {
    void this.failure.catch(()=>{});
    this.opened = new Promise<void>((resolve,reject)=>{
      this.rejectOpen=reject;
      this.source.addEventListener('sourceopen',()=>{
        if(this.closed)return;
        try {this.buffer=this.source.addSourceBuffer(this.mime);this.source.duration=this.duration;resolve();}
        catch(error){reject(error);}
      },{once:true});
    });
    void this.opened.catch(()=>{});
  }
  async load(position: number | Promise<number>) {
    const [saved]=await Promise.race([Promise.all([position,this.opened]),this.failure]);
    if(this.closed)throw new Error('Lecteur fermé.');
    const target=Number.isFinite(saved)?Math.max(0,Math.min(saved,Math.max(0,this.duration-.05))):0;
    await this.start(target,false);
  }
  activate() {
    if(this.failed)throw this.failed;
    this.active=true;this.video.addEventListener('seeking',this.seeked);
  }
  private seeked = () => {
    if(this.closed || !this.buffer)return;
    const position=this.video.currentTime;
    if(!this.restarting)for(let i=0;i<this.buffer.buffered.length;i++)if(this.buffer.buffered.start(i)<=position && this.buffer.buffered.end(i)>position+.05)return;
    void this.start(position,true).catch(()=>{});
  };
  private exclusive(work:()=>Promise<void>) {
    const next=this.operation.catch(()=>{}).then(work);this.operation=next;return next;
  }
  private idle(signal: AbortSignal) {
    if(signal.aborted)return Promise.reject(new Error('Lecture vidéo annulée.'));
    if(!this.buffer?.updating)return Promise.resolve();
    const buffer=this.buffer;
    return new Promise<void>((resolve,reject)=>{
      const finish=(error?:Error)=>{buffer.removeEventListener('updateend',done);buffer.removeEventListener('error',fail);buffer.removeEventListener('abort',fail);signal.removeEventListener('abort',abort);if(error)reject(error);else resolve();};
      const done=()=>finish(),fail=()=>finish(new Error('Fragment AV1 rejeté par le navigateur.')),abort=()=>finish(new Error('Lecture vidéo annulée.'));
      buffer.addEventListener('updateend',done,{once:true});buffer.addEventListener('error',fail,{once:true});buffer.addEventListener('abort',fail,{once:true});signal.addEventListener('abort',abort,{once:true});
    });
  }
  private capacity(signal: AbortSignal) {
    if(bufferedAhead(this.video)<=12)return Promise.resolve();
    return new Promise<void>((resolve,reject)=>{
      const finish=(error?:Error)=>{clearInterval(timer);this.video.removeEventListener('timeupdate',check);this.video.removeEventListener('seeking',check);signal.removeEventListener('abort',abort);if(error)reject(error);else resolve();};
      const check=()=>{if(bufferedAhead(this.video)<=12)finish();},abort=()=>finish(new Error('Lecture vidéo annulée.'));
      const timer=setInterval(check,200);this.video.addEventListener('timeupdate',check);this.video.addEventListener('seeking',check);signal.addEventListener('abort',abort,{once:true});if(signal.aborted)abort();else check();
    });
  }
  private async start(position: number, clear: boolean) {
    this.restarting=clear;
    this.controller?.abort();const controller=new AbortController();this.controller=controller;const signal=controller.signal,epoch=++this.epoch;
    let ready!:()=>void, rejectFirst!:(error:Error)=>void;
    const first=new Promise<void>((resolve,reject)=>{ready=resolve;rejectFirst=reject;});
    const cancelled=()=>rejectFirst(new Error('Lecture vidéo annulée.'));
    signal.addEventListener('abort',cancelled,{once:true});
    const current=()=>!this.closed && epoch===this.epoch && !signal.aborted;
    const run=async()=>{
      if(clear)await this.exclusive(async()=>{
        if(!current()||!this.buffer)return;await this.idle(signal);if(this.source.readyState==='open')this.buffer.abort();
        if(this.buffer.buffered.length){this.buffer.remove(0,this.duration);await this.idle(signal);}
        // remove() reopens an ended MediaSource. abort() itself is illegal in
        // that state, which a short seek stream can reach before the next seek.
        if(current()&&this.source.readyState==='open')this.buffer.abort();
        if(current())this.retained.clear();
      });
      if(!current())return;
      const response=await fetch(`${this.prefix}${Math.min(86399999,Math.max(0,Math.floor(position*1000)))}`,{signal});
      if(!response.ok) {
        let detail = response.headers.get('x-media-preparation-error');
        if (!detail) {
          try {
            const body = await response.clone().json();
            if (body && typeof body.error === 'string') detail = body.error;
          } catch {}
        }
        const messages: Record<string, string> = {
          node_unavailable: 'Media Node indisponible ou déconnecté',
          node_revoked: 'Media Node révoqué',
          asset_changed_rescan_required: 'Fichier source introuvable ou modifié sur disque (réanalyse requise)',
          source_changed_rescan_required: 'Fichier source modifié sur disque (réanalyse requise)',
          preparation_tool_or_source_unavailable: 'FFmpeg ou fichier source introuvable sur le Media Node',
          preparation_busy: 'Media Node occupé',
          preparation_cache_limit_reached: 'Limite du cache de préparation atteinte',
          preparation_timeout: 'Délai de préparation dépassé',
          unsupported_preparation: 'Format non supporté sans réencodage',
          video_stream_capacity_reached: 'Capacité de flux vidéo simultanés atteinte'
        };
        const hint = messages[detail ?? ''] ?? detail ?? 'Media Node indisponible';
        throw new Error(`Préparation vidéo progressive : HTTP ${response.status} (${hint}).`);
      }
      const reader=response.body?.getReader();if(!reader)throw new Error('Flux vidéo progressif vide.');
      let offset: number|undefined,scale=0,prefix=new Uint8Array(),appended=false,received=0,complete=false,raps:{time:number;start:number}[]=[];
      const index=new Mp4Fragments((type,bytes,start)=>{
        if(type==='moov'){offset=mp4TimelineOffset(bytes);scale=mp4Timescale(bytes);}
        else if(type==='moof' && scale){const time=mp4RandomAccess(bytes,scale);if(time!==undefined)raps.push({time:time+(offset??0),start});}
      });
      const append=async(bytes:Uint8Array)=>this.exclusive(async()=>{
        if(!current()||!this.buffer)return;await this.idle(signal);if(!current())return;
        const past=raps.filter(rap=>rap.time>0 && rap.time<=this.video.currentTime-20).at(-1);
        if(past!==undefined && this.buffer.buffered.length && this.buffer.buffered.start(0)<past.time){this.buffer.remove(0,past.time);await this.idle(signal);if(!current())return;this.retained.evictedBefore(past.start);raps=raps.filter(rap=>rap.time>=past.time);}
        this.retained.append(bytes.byteLength,received);
        this.buffer.appendBuffer(bytes as Uint8Array<ArrayBuffer>);await this.idle(signal);
      });
      try {
        while(current()){
          await this.capacity(signal);if(!current())return;
          const {done,value}=await reader.read();if(done)break;
          // Fetch implementations can coalesce network blocks. Feed MSE at most
          // 64 KiB at a time; only moov/moof indexes retain bounded extra bytes.
          for(let at=0;at<value.byteLength;at+=65536){
            if(!current())return;const part=value.subarray(at,Math.min(value.byteLength,at+65536));received+=part.byteLength;index.push(part);
            if(!appended){
              if(prefix.byteLength+part.byteLength>1024*1024)throw new Error('Initialisation vidéo supérieure à 1 Mio.');
              const joined=new Uint8Array(prefix.byteLength+part.byteLength);joined.set(prefix);joined.set(part,prefix.byteLength);prefix=joined;
              if(offset===undefined)continue;
              await this.exclusive(async()=>{if(current()&&this.buffer)this.buffer.timestampOffset=offset!;});
              await append(prefix);prefix=new Uint8Array();appended=true;if(current())this.restarting=false;ready();
            }else await append(part);
            await this.capacity(signal);
          }
        }
        if(!current())return;index.finishStream();
        if(!appended || !this.buffer?.buffered.length || this.buffer.buffered.end(this.buffer.buffered.length-1)<this.duration-.25)throw new Error('Flux vidéo interrompu avant la fin du média.');
        await this.exclusive(async()=>{if(current()){await this.idle(signal);if(this.source.readyState==='open')this.source.endOfStream();}});
        complete=true;
      }finally{if(!complete||!current())await reader.cancel().catch(()=>{});reader.releaseLock();}
    };
    void run().catch(error=>{
      if(!current())return;const failure=error instanceof Error?error:new Error(String(error));this.failed=failure;this.rejectFailure(failure);controller.abort();if(this.active)this.onError(failure.message);
    });
    try { await Promise.race([first,this.failure]); }
    finally { signal.removeEventListener('abort',cancelled); }
  }
  destroy() {
    if(this.closed)return;this.closed=true;this.epoch++;this.controller?.abort();this.video.removeEventListener('seeking',this.seeked);
    this.rejectOpen?.(new Error('Lecteur fermé.'));this.rejectFailure(new Error('Lecteur fermé.'));
    if(this.buffer?.updating){try{this.buffer.abort();}catch{/* The media element may already be detached. */}}
    URL.revokeObjectURL(this.url);
    this.retained.clear();
  }
}
