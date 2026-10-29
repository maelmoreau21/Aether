import {afterEach,describe,expect,it,vi} from 'vitest';
import {ProgressiveVideo} from './progressive-video';
import {EncodedVideoBuffer} from './encoded-video-buffer';
function box(type:string,...parts:Uint8Array[]) {
  const data=new Uint8Array(8+parts.reduce((sum,part)=>sum+part.length,0));new DataView(data.buffer).setUint32(0,data.length);data.set([...type].map(c=>c.charCodeAt(0)),4);
  let at=8;for(const part of parts){data.set(part,at);at+=part.length;}return data;
}
function initialization() {
  const movie=new Uint8Array(100),track=new Uint8Array(24);new DataView(movie.buffer).setUint32(12,1000);new DataView(track.buffer).setUint32(12,12000);
  return box('moov',box('mvhd',movie),box('trak',box('mdia',box('mdhd',track))));
}
function setup() {
  class Buffer extends EventTarget {
    updating=false;ranges:number[][]=[];
    buffered={get length(){return buffer.ranges.length;},start:(i:number)=>this.ranges[i][0],end:(i:number)=>this.ranges[i][1]};
    appendBuffer=vi.fn(()=>{this.ranges=[[0,34]];this.update();});
    remove=vi.fn(()=>{source.readyState='open';this.ranges=[];this.update();});
    abort=vi.fn(()=>{if(source.readyState!=='open')throw new Error('abort on ended MediaSource');});
    private update(){this.updating=true;queueMicrotask(()=>{this.updating=false;this.dispatchEvent(new Event('updateend'));});}
  }
  class Source extends EventTarget {readyState='open';duration=34;addSourceBuffer(){return buffer;}endOfStream(){this.readyState='ended';}}
  const buffer=new Buffer(),source=new Source(),aborted=vi.fn();let streamController!:ReadableStreamDefaultController<Uint8Array>;
  vi.stubGlobal('MediaSource',class {constructor(){return source;}});
  vi.spyOn(URL,'createObjectURL').mockReturnValue('blob:test');vi.spyOn(URL,'revokeObjectURL').mockImplementation(()=>{});
  const fetcher=vi.fn((_url:string,options:RequestInit)=>Promise.resolve(new Response(new ReadableStream<Uint8Array>({start(controller){streamController=controller;controller.enqueue(initialization());options.signal?.addEventListener('abort',()=>{aborted();controller.error(new Error('aborted'));},{once:true});}}))));
  vi.stubGlobal('fetch',fetcher);
  const video=Object.assign(new EventTarget(),{currentTime:0,buffered:buffer.buffered});
  const error=vi.fn(),player=new ProgressiveVideo(video as unknown as HTMLVideoElement,'/video-stream-1-',34,'video/mp4',error);
  return {video,buffer,source,fetcher,aborted,error,player,deliver:(bytes:Uint8Array)=>streamController.enqueue(bytes)};
}
afterEach(()=>{vi.restoreAllMocks();vi.unstubAllGlobals();vi.useRealTimers();});
describe('progressive packet-copy video',()=>{
  it('cancels a load waiting for saved progress when the player closes',async()=>{
    const {player,source,fetcher}=setup();
    const loading=expect(player.load(new Promise<number>(()=>{}))).rejects.toThrow('Lecteur fermé');
    source.dispatchEvent(new Event('sourceopen'));player.destroy();
    await loading;expect(fetcher).not.toHaveBeenCalled();
  });
  it('settles an obsolete first-fragment wait when a newer seek supersedes it',async()=>{
    const {player,source,fetcher}=setup();
    fetcher.mockImplementation((_url,options)=>new Promise((_resolve,reject)=>{
      options.signal?.addEventListener('abort',()=>reject(new Error('aborted')),{once:true});
    }));
    const loading=expect(player.load(0)).rejects.toThrow('annulée');
    source.dispatchEvent(new Event('sourceopen'));
    await vi.waitFor(()=>expect(fetcher).toHaveBeenCalledOnce());
    player.activate();
    const video=(player as unknown as {video:HTMLVideoElement}).video;
    video.currentTime=20;video.dispatchEvent(new Event('seeking'));
    await loading;player.destroy();
  });
  it('aborts the reader and reports a technical error when a long GOP prevents encoded buffer release',async()=>{
    vi.useFakeTimers();const {player,source,buffer,deliver,aborted,error}=setup();
    const state=player as unknown as {retained:EncodedVideoBuffer};state.retained=new EncodedVideoBuffer(initialization().byteLength+10);
    const loading=player.load(0);source.dispatchEvent(new Event('sourceopen'));await loading;player.activate();
    const failed=expect(player.failure).rejects.toThrow('128 Mio');
    buffer.ranges=[];deliver(box('mdat',new Uint8Array(100)));await vi.advanceTimersByTimeAsync(200);await failed;
    expect(aborted).toHaveBeenCalledOnce();expect(error).toHaveBeenCalledWith(expect.stringContaining('tampon vidéo encodé'));
    player.destroy();await vi.advanceTimersByTimeAsync(0);expect(vi.getTimerCount()).toBe(0);
  });
  it('applies backpressure while paused and aborts the reader without a late fatal callback on destruction',async()=>{
    vi.useFakeTimers();const {player,source,fetcher,aborted,error}=setup();
    const loading=player.load(0);source.dispatchEvent(new Event('sourceopen'));await loading;player.activate();
    expect(fetcher).toHaveBeenCalledOnce();player.destroy();await vi.advanceTimersByTimeAsync(0);
    expect(aborted).toHaveBeenCalledOnce();expect(error).not.toHaveBeenCalled();expect(vi.getTimerCount()).toBe(0);
  });
  it('reopens an ended SourceBuffer before aborting it for an uncached seek',async()=>{
    vi.useFakeTimers();const {player,source,video,buffer,fetcher,error}=setup();
    const loading=player.load(0);source.dispatchEvent(new Event('sourceopen'));await loading;player.activate();
    source.readyState='ended';buffer.ranges=[[30,34]];video.currentTime=2;video.dispatchEvent(new Event('seeking'));
    await vi.advanceTimersByTimeAsync(0);
    expect(fetcher).toHaveBeenLastCalledWith('/video-stream-1-2000',expect.anything());
    expect(buffer.remove).toHaveBeenCalledWith(0,34);expect(error).not.toHaveBeenCalled();
    expect((player as unknown as {retained:EncodedVideoBuffer}).retained.bytes).toBe(initialization().byteLength);
    player.destroy();await vi.advanceTimersByTimeAsync(0);expect(vi.getTimerCount()).toBe(0);
  });
  it('keeps the latest seek when it returns to an old cached range before a pending restart clears it',async()=>{
    vi.useFakeTimers();const {player,source,video,buffer,fetcher,error}=setup();
    const loading=player.load(0);source.dispatchEvent(new Event('sourceopen'));await loading;player.activate();
    buffer.ranges=[[0,5]];
    video.currentTime=31;video.dispatchEvent(new Event('seeking'));
    video.currentTime=2;video.dispatchEvent(new Event('seeking'));
    await vi.advanceTimersByTimeAsync(0);
    expect(fetcher.mock.calls.map(call=>call[0])).toEqual(['/video-stream-1-0','/video-stream-1-2000']);
    expect(error).not.toHaveBeenCalled();player.destroy();await vi.advanceTimersByTimeAsync(0);expect(vi.getTimerCount()).toBe(0);
  });
});
