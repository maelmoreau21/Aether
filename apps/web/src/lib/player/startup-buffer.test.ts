import {afterEach,describe,expect,it,vi} from 'vitest';
import {bufferedAhead,waitForStartupBuffer} from './startup-buffer';
class Media extends EventTarget {
  currentTime=0;duration=30;readyState=3;seeking=false;ranges=[[0,.01]];
  buffered: {readonly length:number;start:(i:number)=>number;end:(i:number)=>number};
  constructor(){super();this.buffered={length:1,start:i=>this.ranges[i][0],end:i=>this.ranges[i][1]};}
}
const element=(media:Media)=>media as unknown as HTMLMediaElement;
afterEach(()=>vi.useRealTimers());
describe('startup buffer',()=>{
  it('waits for a contiguous playable reserve and resolves immediately when cached',async()=>{
    const media=new Media();let ready=false;
    const pending=waitForStartupBuffer(element(media),new AbortController().signal).then(()=>{ready=true;});
    await Promise.resolve();expect(ready).toBe(false);
    media.ranges=[[0,.01],[2,20]];media.buffered={length:2,start:i=>media.ranges[i][0],end:i=>media.ranges[i][1]};media.dispatchEvent(new Event('progress'));await Promise.resolve();expect(ready).toBe(false);
    media.ranges=[[0,1]];media.buffered={length:1,start:i=>media.ranges[i][0],end:i=>media.ranges[i][1]};media.dispatchEvent(new Event('progress'));await pending;expect(ready).toBe(true);
    expect(bufferedAhead(element(media))).toBe(1);
    await waitForStartupBuffer(element(media),new AbortController().signal);
  });
  it('starts immediately on a short decoded reserve instead of waiting for 750 ms',async()=>{
    const media=new Media();media.ranges=[[0,.06]];
    await waitForStartupBuffer(element(media),new AbortController().signal);
  });
  it('honors HAVE_ENOUGH_DATA after a same-URL reload exposes stale buffered ranges, while still waiting for seeks',async()=>{
    const media=new Media();media.duration=3500;media.readyState=4;media.ranges=[[3447,3500]];media.seeking=true;
    let ready=false;
    const pending=waitForStartupBuffer(element(media),new AbortController().signal).then(()=>{ready=true;});
    await Promise.resolve();expect(ready).toBe(false);
    media.seeking=false;media.dispatchEvent(new Event('seeked'));await pending;
    expect(ready).toBe(true);expect(bufferedAhead(element(media))).toBe(0);
    await waitForStartupBuffer(element(media),new AbortController().signal);
    media.buffered={length:0,start:()=>{throw new Error('no ranges');},end:()=>{throw new Error('no ranges');}};
    await waitForStartupBuffer(element(media),new AbortController().signal);
  });
  it('accepts the remaining tail, but never treats seeking or metadata as playable',async()=>{
    const media=new Media();media.currentTime=29.8;media.ranges=[[29,30]];media.seeking=true;
    let ready=false;const pending=waitForStartupBuffer(element(media),new AbortController().signal).then(()=>{ready=true;});
    await Promise.resolve();expect(ready).toBe(false);
    media.seeking=false;media.readyState=1;media.dispatchEvent(new Event('seeked'));await Promise.resolve();expect(ready).toBe(false);
    media.readyState=3;media.dispatchEvent(new Event('canplay'));await pending;
  });
  it('cancels and bounds stalled storage without leaking timers',async()=>{
    vi.useFakeTimers();const media=new Media(),controller=new AbortController();
    const cancelled=expect(waitForStartupBuffer(element(media),controller.signal)).rejects.toThrow('annulé');controller.abort();await cancelled;
    expect(vi.getTimerCount()).toBe(0);
    const timeout=expect(waitForStartupBuffer(element(media),new AbortController().signal)).rejects.toThrow('8 secondes');await vi.advanceTimersByTimeAsync(8000);await timeout;
    expect(vi.getTimerCount()).toBe(0);
  });
  it('holds independent audio until a real reserve is buffered, then accepts less after the soft deadline',async()=>{
    vi.useFakeTimers();const media=new Media();media.readyState=4;let ready=false;
    const pending=waitForStartupBuffer(element(media),new AbortController().signal,{seconds:1.5,softDeadlineMs:3000}).then(()=>{ready=true;});
    await vi.advanceTimersByTimeAsync(200);expect(ready).toBe(false); // HAVE_ENOUGH_DATA alone is not a reserve.
    media.ranges=[[0,1.6]];await vi.advanceTimersByTimeAsync(100);await pending;expect(ready).toBe(true);
    const slow=new Media();slow.readyState=3;slow.ranges=[[0,.2]];let late=false;
    const waiting=waitForStartupBuffer(element(slow),new AbortController().signal,{seconds:1.5,softDeadlineMs:3000}).then(()=>{late=true;});
    await vi.advanceTimersByTimeAsync(2900);expect(late).toBe(false);
    await vi.advanceTimersByTimeAsync(200);await waiting;expect(late).toBe(true); // A slow NAS delays, never fails.
    expect(vi.getTimerCount()).toBe(0);
  });
});
