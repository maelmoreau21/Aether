import {afterEach,describe,expect,it,vi} from 'vitest';
import {SegmentedAudio} from './segmented-audio';

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
afterEach(()=>vi.unstubAllGlobals());
describe('segmented audio seek cancellation',()=>{
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
  it('keeps packet-copy preparation away from startup and requests the next window with four seconds of lead',async()=>{
    const {audio}=harness();
    const fetcher=vi.fn().mockImplementation(()=>Promise.resolve(new Response(new Uint8Array([1,2,3]))));vi.stubGlobal('fetch',fetcher);
    await audio.fill();expect(fetcher.mock.calls.map(c=>c[0])).toEqual(['/chunks/0']);
    (audio as unknown as {element:{currentTime:number}}).element.currentTime=7;
    await audio.fill();expect(fetcher.mock.calls.map(c=>c[0])).toEqual(['/chunks/0','/chunks/0','/chunks/1']);
  });
  it('loads only the saved-position window on a resumed session',async()=>{
    const {audio}=harness();
    (audio as unknown as {element:{currentTime:number}}).element.currentTime=26;
    const fetcher=vi.fn().mockResolvedValue(new Response(new Uint8Array([1,2,3])));vi.stubGlobal('fetch',fetcher);
    await audio.fill(true);
    expect(fetcher.mock.calls.map(c=>c[0])).toEqual(['/chunks/2']);
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
