import {afterEach,describe,expect,it,vi} from 'vitest';
import {bufferedAhead,waitForStartupBuffer} from './startup-buffer';
class Media extends EventTarget {
  currentTime=0;duration=30;readyState=3;seeking=false;ranges=[[0,.1]];
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
    media.ranges=[[0,.1],[2,20]];media.buffered={length:2,start:i=>media.ranges[i][0],end:i=>media.ranges[i][1]};media.dispatchEvent(new Event('progress'));await Promise.resolve();expect(ready).toBe(false);
    media.ranges=[[0,1]];media.buffered={length:1,start:i=>media.ranges[i][0],end:i=>media.ranges[i][1]};media.dispatchEvent(new Event('progress'));await pending;expect(ready).toBe(true);
    expect(bufferedAhead(element(media))).toBe(1);
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
});
