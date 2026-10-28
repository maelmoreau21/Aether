import {afterEach,describe,expect,it,vi} from 'vitest';
import {SegmentedAudio} from './segmented-audio';
import {ExternalAudio} from './external-audio';

// Exercise races where abort cannot recall an already delivered response, or
// where a seek arrives while a previous SourceBuffer update is still pending.
function harness() {
  const appendBuffer=vi.fn();const schedule=vi.fn();
  const audio=Object.assign(Object.create(SegmentedAudio.prototype),{
    closed:false,filling:false,seekEpoch:0,loaded:new Set<number>(),prefix:'/chunks/',offsetSeconds:0,
    element:{currentTime:0,duration:60},mediaSource:{readyState:'open',endOfStream:vi.fn()},
    buffer:{buffered:{length:0},appendBuffer,remove:vi.fn()},idle:vi.fn().mockResolvedValue(undefined),schedule
  }) as {fill(current?:boolean):Promise<void>;seekEpoch:number;idle:()=>Promise<void>};
  return {audio,appendBuffer,schedule};
}
afterEach(()=>{vi.restoreAllMocks();vi.unstubAllGlobals();vi.useRealTimers();});
describe('segmented audio seek cancellation',()=>{
  it('rejects pending commit buffering after an inactive candidate window fails without reporting a fatal presentation error', async()=>{
    vi.useFakeTimers();
    class AudioElement extends EventTarget {dataset={};pause=vi.fn();}
    class Source extends EventTarget {readyState='open';duration=60;addSourceBuffer(){return {};}}
    vi.stubGlobal('Audio',AudioElement);vi.stubGlobal('MediaSource',Source);
    vi.spyOn(URL,'createObjectURL').mockReturnValue('blob:test');vi.spyOn(URL,'revokeObjectURL').mockImplementation(()=>{});
    const video=Object.assign(new EventTarget(),{currentTime:9.9,duration:60,insertAdjacentElement:vi.fn(),pause:vi.fn()});
    const report=vi.fn();
    vi.spyOn(ExternalAudio.prototype,'load').mockResolvedValue(undefined);
    vi.spyOn(ExternalAudio.prototype,'bufferForPlayback').mockImplementation(()=>new Promise<void>(()=>{}));
    vi.spyOn(ExternalAudio.prototype,'destroy').mockImplementation(()=>{});
    const audio=new SegmentedAudio(video as unknown as HTMLVideoElement,0,report,{trackId:'internal-1'});
    const state=audio as unknown as {fill(current?:boolean):Promise<void>;mediaSource:Source;audio:{dataset:{internalAudioTrack?:string}}};
    const fill=vi.spyOn(state,'fill').mockResolvedValue(undefined);
    const loading=audio.load('/prepared/opus-2-chunk-',false);
    state.mediaSource.dispatchEvent(new Event('sourceopen'));await loading;
    expect(state.audio.dataset.internalAudioTrack).toBe('internal-1');
    fill.mockRejectedValueOnce(new Error('candidate window HTTP 503'));
    const buffering=expect(audio.bufferForPlayback()).rejects.toThrow('candidate window HTTP 503');
    await vi.advanceTimersByTimeAsync(250);await buffering;
    expect(video.pause).not.toHaveBeenCalled();expect(report).not.toHaveBeenCalled();
    expect(()=>audio.assertReady()).toThrow('candidate window HTTP 503');
    expect(()=>audio.activate()).toThrow('candidate window HTTP 503');
    audio.destroy();expect(vi.getTimerCount()).toBe(0);
  });
  it('follows a window crossed during candidate loading before metadata/seek is ready and cancels the scheduler on destroy', async()=>{
    vi.useFakeTimers();
    class AudioElement extends EventTarget {dataset={};}
    class Source extends EventTarget {
      readyState='open';duration=60;
      addSourceBuffer(){return {};}
    }
    vi.stubGlobal('Audio',AudioElement);vi.stubGlobal('MediaSource',Source);
    vi.spyOn(URL,'createObjectURL').mockReturnValue('blob:test');vi.spyOn(URL,'revokeObjectURL').mockImplementation(()=>{});
    const video=Object.assign(new EventTarget(),{currentTime:9.9,duration:60,insertAdjacentElement:vi.fn(),pause:vi.fn()});
    let metadataReady!:()=>void;let firstFilled!:()=>void;
    vi.spyOn(ExternalAudio.prototype,'load').mockImplementation(()=>new Promise<void>(resolve=>{metadataReady=resolve;}));
    vi.spyOn(ExternalAudio.prototype,'destroy').mockImplementation(()=>{});
    const audio=new SegmentedAudio(video as unknown as HTMLVideoElement,0,vi.fn());
    const state=audio as unknown as {fill(current?:boolean):Promise<void>;mediaSource:Source};
    const fill=vi.spyOn(state,'fill').mockImplementationOnce(()=>new Promise<void>(resolve=>{firstFilled=resolve;})).mockImplementation(async()=>{
      expect(video.currentTime).toBe(10.4);metadataReady();
    });
    const loading=audio.load('/audio-1-chunk-',false);
    state.mediaSource.dispatchEvent(new Event('sourceopen'));await Promise.resolve();
    expect(fill).toHaveBeenCalledWith(true);
    video.currentTime=10.4;firstFilled();await vi.advanceTimersByTimeAsync(250);await loading;
    expect(fill).toHaveBeenCalledTimes(2);expect(fill).toHaveBeenLastCalledWith(true);
    audio.destroy();expect(vi.getTimerCount()).toBe(0);
    video.dispatchEvent(new Event('seeking'));await vi.advanceTimersByTimeAsync(1000);
    expect(fill).toHaveBeenCalledTimes(2);
  });
  it('ends a final short window and can refill an evicted window after a backwards seek',async()=>{
    const {audio,appendBuffer}=harness();
    const state=audio as unknown as {element:{currentTime:number;duration:number};mediaSource:{readyState:string;endOfStream:()=>void};loaded:Set<number>};
    state.element={currentTime:51,duration:52};
    state.mediaSource.endOfStream=vi.fn(()=>{state.mediaSource.readyState='ended';});
    const fetcher=vi.fn().mockImplementation(()=>Promise.resolve(new Response(new Uint8Array([1,2,3]))));vi.stubGlobal('fetch',fetcher);
    await audio.fill(true);
    expect(state.mediaSource.endOfStream).toHaveBeenCalledOnce();
    expect(fetcher.mock.calls.map(call=>call[0])).toEqual(['/chunks/5']);
    state.element.currentTime=1;
    await audio.fill(true);
    expect(fetcher.mock.calls.map(call=>call[0])).toEqual(['/chunks/5','/chunks/0']);
    expect(appendBuffer).toHaveBeenCalledTimes(2);
  });
  it('keeps preparation away from startup and requests the next window eight seconds before the boundary',async()=>{
    const {audio}=harness();
    const fetcher=vi.fn().mockImplementation(()=>Promise.resolve(new Response(new Uint8Array([1,2,3]))));vi.stubGlobal('fetch',fetcher);
    await audio.fill();expect(fetcher.mock.calls.map(c=>c[0])).toEqual(['/chunks/0']);
    (audio as unknown as {element:{currentTime:number}}).element.currentTime=2;
    await audio.fill();expect(fetcher.mock.calls.map(c=>c[0])).toEqual(['/chunks/0','/chunks/0','/chunks/1']);
  });
  it('loads only the saved-position window on a resumed session',async()=>{
    const {audio}=harness();
    (audio as unknown as {element:{currentTime:number}}).element.currentTime=26;
    const fetcher=vi.fn().mockResolvedValue(new Response(new Uint8Array([1,2,3])));vi.stubGlobal('fetch',fetcher);
    await audio.fill(true);
    expect(fetcher.mock.calls.map(c=>c[0])).toEqual(['/chunks/2']);
  });
  it('prepares an explicit saved position before video metadata and preserves absolute Rust fragment timestamps',async()=>{
    const {audio}=harness();
    const state=audio as unknown as {element:{currentTime:number;duration:number};timeline:{duration:number;position:number;mode:string};buffer:{timestampOffset:number;appendWindowStart:number}};
    state.element={currentTime:0,duration:NaN};state.timeline={duration:60,position:26,mode:'absolute'};
    const fetcher=vi.fn().mockResolvedValue(new Response(new Uint8Array([1,2,3])));vi.stubGlobal('fetch',fetcher);
    await audio.fill(true);
    expect(fetcher.mock.calls.map(c=>c[0])).toEqual(['/chunks/2']);
    expect(state.buffer.timestampOffset).toBe(0);expect(state.buffer.appendWindowStart).toBe(20);
  });
  it('immediately schedules the latest window when preparation outlives a boundary',async()=>{
    const {audio,schedule}=harness();
    const state=audio as unknown as {element:{currentTime:number}};
    state.element.currentTime=9.9;
    vi.stubGlobal('fetch',vi.fn().mockImplementation(async()=>{
      state.element.currentTime=10.4;
      return new Response(new Uint8Array([1,2,3]));
    }));
    await audio.fill(true);expect(schedule).toHaveBeenCalledOnce();
  });
  it('discards an obsolete response even when fetch completes after cancellation',async()=>{
    const {audio,appendBuffer,schedule}=harness();
    let respond!:(response:Response)=>void;
    vi.stubGlobal('fetch',vi.fn(()=>new Promise<Response>(resolve=>{respond=resolve;})));
    const filling=audio.fill(true);audio.seekEpoch++;
    respond(new Response(new Uint8Array([1,2,3])));await filling;
    expect(appendBuffer).not.toHaveBeenCalled();expect(schedule).toHaveBeenCalledOnce();
  });
  it('rechecks the seek after waiting for SourceBuffer before appending',async()=>{
    const {audio,appendBuffer,schedule}=harness();
    let release!:()=>void;
    audio.idle=vi.fn(()=>new Promise<void>(resolve=>{release=resolve;}));
    vi.stubGlobal('fetch',vi.fn().mockResolvedValue(new Response(new Uint8Array([1,2,3]))));
    const filling=audio.fill(true);await vi.waitFor(()=>expect(audio.idle).toHaveBeenCalled());
    audio.seekEpoch++;release();await filling;
    expect(appendBuffer).not.toHaveBeenCalled();expect(schedule).toHaveBeenCalledOnce();
  });
});
