import { ExternalAudio } from './external-audio';

const MIME = 'audio/mp4; codecs="opus"';
const SECONDS = 10;
export interface AudioTimeline { duration?: number; position?: number; mode?: 'absolute' | 'rebased'; trackId?: string }

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
  private loading = true;
  private rejectLoad?: (error: Error) => void;
  private activated = false;
  private preparationError?: Error;
  private rejectBuffer?: (error: Error) => void;
  private pendingUpdates = new Set<() => void>();

  constructor(private element: HTMLVideoElement, offset: number, private reportError: (message: string) => void, private timeline?: AudioTimeline) {
    super(element, offset, reportError); this.offsetSeconds = offset;
  }
  private position() { return !this.activated && this.timeline?.position !== undefined ? this.timeline.position : this.element.currentTime; }
  private duration() { return this.timeline?.duration ?? this.element.duration; }
  async preload(prefix: string, position: number | Promise<number>) {
    const saved = await position;
    if (this.closed) throw new Error('Lecteur fermé.');
    this.timeline = { ...this.timeline, position: Number.isFinite(saved) ? Math.max(0, Math.min(saved, Math.max(0, this.duration() - .05))) : 0 };
    await this.load(prefix, false);
  }
  override async load(prefix: string, activate = true) {
    this.prefix = prefix;
    const legacyIndex = /audio-(\d+)-chunk-$/.exec(prefix)?.[1];
    // MP4 track IDs in opus-{id} are not ffprobe stream indices. Preserve the
    // descriptor's logical identity instead of inferring it from that URL.
    const trackId = this.timeline?.trackId ?? (legacyIndex ? `internal-${legacyIndex}` : undefined);
    if(trackId)this.audio.dataset.internalAudioTrack=trackId;
    const failed = new Promise<never>((_, reject) => { this.rejectLoad = reject; });
    const opened = new Promise<void>((resolve, reject) => {
      this.rejectOpen = reject;
      this.opened = () => {
        try {
          this.buffer = this.mediaSource.addSourceBuffer(MIME);
          this.mediaSource.duration = this.duration();
          // A replacement track loads while the previous presentation advances.
          // Keep following its current window even before audio metadata/seek
          // completes, otherwise crossing a boundary can deadlock ready().
          this.element.addEventListener('seeking', this.seekChanged);
          this.interval = setInterval(this.schedule, 250);
          this.rejectOpen = undefined;
          resolve();
        } catch (error) { reject(error); }
      };
      this.mediaSource.addEventListener('sourceopen', this.opened, {once:true});
    });
    // Set src before waiting for sourceopen. The audio metadata wait completes
    // only once the first bounded window has arrived.
    const mediaLoad = super.load(this.objectUrl, false, MIME, this.timeline?.position);
    try {
      await Promise.race([Promise.all([mediaLoad, (async()=>{await opened;await this.fill(true);})()]), failed]);
      if (this.closed) throw new Error('Lecteur fermé.');
      this.loading = false; this.rejectLoad = undefined;
      if (activate) this.activate();
    } catch (error) { this.destroy(); throw error; }
  }
  private seekChanged = () => {this.seekEpoch++;this.controller?.abort();this.schedule();};
  private schedule = () => { if (this.preparationError) return; void this.fill(this.loading).catch(error => {
    if (this.closed) return;
    if (!this.activated) {
      // A loaded candidate still belongs to the replacement transaction until
      // activate(). Its next window can fail while commit buffering is pending;
      // never route that error through the current presentation's fatal handler.
      this.preparationError = error instanceof Error ? error : new Error(String(error));
      clearInterval(this.interval);
      this.rejectLoad?.(this.preparationError);
      this.rejectBuffer?.(this.preparationError);
    }
    else {
      // A terminal window failure requires an explicit player retry. Keeping
      // the interval alive would prepare the same failed window four times/s.
      this.preparationError = error instanceof Error ? error : new Error(String(error));
      clearInterval(this.interval);
      this.element.pause();this.reportError(this.preparationError.message);
    }
  }); };
  override activate() {
    this.assertReady();
    super.activate(); this.activated = true;
  }
  override assertReady() {
    super.assertReady();
    if (this.preparationError) throw this.preparationError;
  }
  override async bufferForPlayback() {
    if (this.preparationError) throw this.preparationError;
    const failed = new Promise<never>((_, reject) => { this.rejectBuffer = reject; });
    try { await Promise.race([super.bufferForPlayback(), failed]); }
    catch (error) { super.pause(); throw error; }
    finally { this.rejectBuffer = undefined; }
  }
  private async idle() {
    if (!this.buffer?.updating) return;
    const buffer = this.buffer;
    await new Promise<void>((resolve,reject)=>{
      const done=()=>{cleanup();resolve();};
      const fail=()=>{cleanup();reject(new Error('Segment Opus rejeté par le navigateur.'));};
      const cancel=()=>{cleanup();reject(new Error('Lecteur fermé.'));};
      const timeout=setTimeout(()=>{cleanup();reject(new Error('Mise en tampon du segment Opus expirée après 10 secondes.'));},10000);
      const cleanup=()=>{clearTimeout(timeout);this.pendingUpdates.delete(cancel);buffer.removeEventListener('updateend',done);buffer.removeEventListener('error',fail);buffer.removeEventListener('abort',fail);};
      this.pendingUpdates.add(cancel);
      buffer.addEventListener('updateend',done,{once:true});buffer.addEventListener('error',fail,{once:true});buffer.addEventListener('abort',fail,{once:true});
      if(this.closed)cancel();
    });
  }
  private async fill(onlyCurrent = false) {
    if (this.closed || this.filling || !this.buffer || this.mediaSource.readyState === 'closed') return;
    this.filling = true;
    const epoch = this.seekEpoch;
    const target = Math.max(0, this.position() - this.offsetSeconds);
    const current = Math.floor(target / SECONDS);
    try {
      // At most the current window and one ahead; never copy the full episode.
      // Keep ordinary startup dedicated to the current window. Close to its
      // end, the decoder may need packets from the next window to complete a
      // seek/HAVE_FUTURE_DATA; waiting for load() first would deadlock startup.
      // Give the Node and a mobile connection more time to prepare the next
      // window during playback. Neither path loads more than two windows.
      const remaining = (current + 1) * SECONDS - target;
      const needsFollowingWindow = remaining <= (onlyCurrent ? .5 : 8);
      for (const chunk of needsFollowingWindow ? [current, current + 1] : [current]) {
        if (this.closed || epoch !== this.seekEpoch) return;
        const ranges=this.buffer.buffered;
        const retained=Array.from({length:ranges.length},(_,i)=>i).some(i=>ranges.start(i)<=chunk*SECONDS+.05 && ranges.end(i)>=Math.min(this.duration(),(chunk+1)*SECONDS)-.05);
        if(!retained)this.loaded.delete(chunk);
        if (chunk * SECONDS >= this.duration() || this.loaded.has(chunk) || this.closed) continue;
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
        this.buffer.timestampOffset = this.timeline?.mode === 'absolute' ? 0 : Math.max(0, start - 1);
        this.buffer.appendWindowEnd = Infinity;
        this.buffer.appendWindowStart = start;
        this.buffer.appendWindowEnd = Math.min(this.duration(), start + SECONDS);
        this.buffer.appendBuffer(bytes);await this.idle();this.loaded.add(chunk);
      }
      // Bound browser buffers even on long episodes and repeated seeks.
      for (const chunk of [...this.loaded]) {
        if (Math.abs(chunk-current)>3 && !this.closed) {
          await this.idle();this.buffer.remove(chunk*SECONDS,(chunk+1)*SECONDS);await this.idle();this.loaded.delete(chunk);
        }
      }
      // Without EOS an Opus packet ending just before the video's final frame
      // looks like an infinite network stall. Appending after a backwards seek
      // reopens an ended MediaSource; do not reject that state above.
      const last = Math.ceil(this.duration() / SECONDS) - 1;
      if (!this.closed && epoch === this.seekEpoch && this.loaded.has(last) && this.mediaSource.readyState === 'open') {
        await this.idle();
        if (!this.closed && epoch === this.seekEpoch && this.mediaSource.readyState === 'open') this.mediaSource.endOfStream();
      }
    } catch (error) {
      // Fetch errors already check the epoch. A SourceBuffer update can also
      // fail after a seek superseded this fill; its result must not invalidate
      // the new position or a replacement track transaction.
      if (!this.closed && epoch === this.seekEpoch) throw error;
    } finally {
      this.filling=false;
      // A seek may have arrived while fetch or SourceBuffer was finishing.
      // Start the latest position immediately instead of waiting for the timer.
      const latestWindow = Math.floor(Math.max(0, this.position() - this.offsetSeconds) / SECONDS);
      if (!this.closed && (epoch !== this.seekEpoch || latestWindow !== current)) this.schedule();
    }
  }
  override setOffset(seconds: number) {if(!Number.isFinite(seconds)||Math.abs(seconds)>30)return;this.offsetSeconds=seconds;super.setOffset(seconds);this.schedule();}
  override destroy() {
    if(this.closed)return;
    this.closed=true;clearInterval(this.interval);this.controller?.abort();
    for(const cancel of this.pendingUpdates)cancel();
    this.element.removeEventListener('seeking',this.seekChanged);
    if(this.opened)this.mediaSource.removeEventListener('sourceopen',this.opened);
    this.rejectOpen?.(new Error('Lecteur fermé.'));this.rejectOpen=undefined;
    this.rejectLoad?.(new Error('Lecteur fermé.'));this.rejectLoad=undefined;
    this.rejectBuffer?.(new Error('Lecteur fermé.'));this.rejectBuffer=undefined;
    if(this.buffer?.updating){try{this.buffer.abort();}catch{/* MSE can already be closed. */}}
    super.destroy();URL.revokeObjectURL(this.objectUrl);
  }
}
