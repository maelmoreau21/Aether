import {afterEach,describe,expect,it,vi} from 'vitest';
import {SegmentedAudio} from './segmented-audio';

// Exercise races where abort cannot recall an already delivered response, or
// where a seek arrives while a previous SourceBuffer update is still pending.
function harness() {
  const appendBuffer=vi.fn();const schedule=vi.fn();
  const audio=Object.assign(Object.create(SegmentedAudio.prototype),{
    closed:false,filling:false,seekEpoch:0,loaded:new Set<number>(),prefix:'/chunks/',offsetSeconds:0,
    element:{currentTime:0,duration:60},mediaSource:{readyState:'open'},
    buffer:{buffered:{length:0},appendBuffer},idle:vi.fn().mockResolvedValue(undefined),schedule
  }) as {fill(current?:boolean):Promise<void>;seekEpoch:number;idle:()=>Promise<void>};
  return {audio,appendBuffer,schedule};
}
afterEach(()=>vi.unstubAllGlobals());
describe('segmented audio seek cancellation',()=>{
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
