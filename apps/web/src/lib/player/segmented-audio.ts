import { ExternalAudio } from './external-audio';

const MIME = 'audio/mp4; codecs="opus"';
const SECONDS = 10;

// The original MP4 supplies the video index. Only short packet-copy audio
// windows enter MSE; no WebAudio graph, decoding, channel conversion or encoder.
export class SegmentedAudio extends ExternalAudio {
  static supported() { return typeof MediaSource !== 'undefined' && MediaSource.isTypeSupported(MIME); }
  private mediaSource = new MediaSource();
  private buffer?: SourceBuffer;
  private objectUrl = URL.createObjectURL(this.mediaSource);
  private controller?: AbortController;
  private interval?: ReturnType<typeof setInterval>;
  private closed = false;
  private filling = false;
  private seekEpoch = 0;
  private offsetSeconds: number;
  private loaded = new Set<number>();
  private prefix = '';
  private opened?: () => void;
  private rejectOpen?: (error: Error) => void;

  constructor(private element: HTMLVideoElement, offset: number, private reportError: (message: string) => void) {
    super(element, offset, reportError); this.offsetSeconds = offset;
  }
  override async load(prefix: string, activate = true) {
    this.prefix = prefix;
    const index = /audio-(\d+)-chunk-$/.exec(prefix)?.[1];
    if(index)this.audio.dataset.internalAudioTrack=`internal-${index}`;
    const opened = new Promise<void>((resolve, reject) => {
      this.rejectOpen = reject;
      this.opened = () => {
        try {
          this.buffer = this.mediaSource.addSourceBuffer(MIME);
          this.mediaSource.duration = this.element.duration;
          this.rejectOpen = undefined;
          resolve();
        } catch (error) { reject(error); }
      };
      this.mediaSource.addEventListener('sourceopen', this.opened, {once:true});
    });
    // Set src before waiting for sourceopen. The audio metadata wait completes
    // only once the first bounded window has arrived.
    const mediaLoad = super.load(this.objectUrl, activate, MIME);
    try {
      await Promise.all([mediaLoad, (async()=>{await opened;await this.fill(true);})()]);
      if (this.closed) throw new Error('Lecteur fermé.');
      this.element.addEventListener('seeking', this.seekChanged);
      this.interval = setInterval(this.schedule, 250);
    } catch (error) { this.destroy(); throw error; }
  }
  private seekChanged = () => {this.seekEpoch++;this.controller?.abort();this.schedule();};
  private schedule = () => { void this.fill().catch(error => {
    if (!this.closed) {this.element.pause();this.reportError(error instanceof Error ? error.message : String(error));}
  }); };
  private async idle() {
    if (!this.buffer?.updating) return;
    const buffer = this.buffer;
    await new Promise<void>((resolve,reject)=>{
      const done=()=>{cleanup();resolve();};
      const fail=()=>{cleanup();reject(new Error('Segment Opus rejeté par le navigateur.'));};
      const cleanup=()=>{buffer.removeEventListener('updateend',done);buffer.removeEventListener('error',fail);buffer.removeEventListener('abort',fail);};
      buffer.addEventListener('updateend',done,{once:true});buffer.addEventListener('error',fail,{once:true});buffer.addEventListener('abort',fail,{once:true});
    });
  }
  private async fill(onlyCurrent = false) {
    if (this.closed || this.filling || !this.buffer || this.mediaSource.readyState !== 'open') return;
    this.filling = true;
    const epoch = this.seekEpoch;
    try {
      const target = Math.max(0, this.element.currentTime - this.offsetSeconds);
      const current = Math.floor(target / SECONDS);
      // At most the current window and one ahead; never copy the full episode.
      for (const chunk of onlyCurrent ? [current] : [current, current + 1]) {
        if (this.closed || epoch !== this.seekEpoch) return;
        const ranges=this.buffer.buffered;
        const retained=Array.from({length:ranges.length},(_,i)=>i).some(i=>ranges.start(i)<=chunk*SECONDS+.05 && ranges.end(i)>=Math.min(this.element.duration,(chunk+1)*SECONDS)-.05);
        if(!retained)this.loaded.delete(chunk);
        if (chunk * SECONDS >= this.element.duration || this.loaded.has(chunk) || this.closed) continue;
        const controller = new AbortController(); this.controller = controller;
        const timeout = setTimeout(()=>controller.abort(), 20000);
        const started=performance.now();
        let bytes: ArrayBuffer;
        try {
          const response = await fetch(`${this.prefix}${chunk}`, {signal:controller.signal});
          if (!response.ok) throw new Error(`Préparation audio segment ${chunk} : HTTP ${response.status} (${response.headers.get('x-media-preparation-error') ?? 'source indisponible'}).`);
          const length = response.headers.get('content-length');
          const size = length === null ? undefined : Number(length);
          if (size !== undefined && (!Number.isFinite(size) || size <= 0 || size > 16*1024*1024)) throw new Error('Taille de segment audio invalide.');
          const reader=response.body?.getReader();if(!reader)throw new Error('Segment audio vide.');
          const parts:Uint8Array[]=[];let total=0;
          try {
            while(true){const {done,value}=await reader.read();if(done)break;total+=value.byteLength;
              if(total>16*1024*1024){await reader.cancel();throw new Error('Segment audio trop volumineux.');}parts.push(value);}
          } finally {reader.releaseLock();}
          if(total===0 || (size!==undefined && total!==size))throw new Error('Segment audio tronqué.');
          const data=new Uint8Array(total);let cursor=0;for(const part of parts){data.set(part,cursor);cursor+=part.byteLength;}bytes=data.buffer;
          console.info('[media audio]', {stage:'segment-ready',chunk,bytes:total,elapsedMs:Math.round(performance.now()-started)});
        } catch (error) {
          if(this.closed || epoch!==this.seekEpoch)return;
          if(controller.signal.aborted) throw new Error('Préparation du segment audio annulée ou expirée (20 s).');
          throw error;
        } finally {clearTimeout(timeout);if(this.controller===controller)this.controller=undefined;}
        if (this.closed || epoch !== this.seekEpoch) return;
        await this.idle();
        if (this.closed || epoch !== this.seekEpoch) return;
        const start = chunk * SECONDS;
        this.buffer.timestampOffset = Math.max(0, start - 1);
        this.buffer.appendWindowEnd = Infinity;
        this.buffer.appendWindowStart = start;
        this.buffer.appendWindowEnd = Math.min(this.element.duration, start + SECONDS);
        this.buffer.appendBuffer(bytes);await this.idle();this.loaded.add(chunk);
      }
      // Bound browser buffers even on long episodes and repeated seeks.
      for (const chunk of [...this.loaded]) {
        if (Math.abs(chunk-current)>3 && !this.closed) {
          await this.idle();this.buffer.remove(chunk*SECONDS,(chunk+1)*SECONDS);await this.idle();this.loaded.delete(chunk);
        }
      }
    } finally {
      this.filling=false;
      // A seek may have arrived while fetch or SourceBuffer was finishing.
      // Start the latest position immediately instead of waiting for the timer.
      if (!this.closed && epoch !== this.seekEpoch) this.schedule();
    }
  }
  override setOffset(seconds: number) {if(!Number.isFinite(seconds)||Math.abs(seconds)>30)return;this.offsetSeconds=seconds;super.setOffset(seconds);this.schedule();}
  override destroy() {
    if(this.closed)return;
    this.closed=true;clearInterval(this.interval);this.controller?.abort();
    this.element.removeEventListener('seeking',this.seekChanged);
    if(this.opened)this.mediaSource.removeEventListener('sourceopen',this.opened);
    this.rejectOpen?.(new Error('Lecteur fermé.'));this.rejectOpen=undefined;
    if(this.buffer?.updating){try{this.buffer.abort();}catch{/* MSE can already be closed. */}}
    super.destroy();URL.revokeObjectURL(this.objectUrl);
  }
}
