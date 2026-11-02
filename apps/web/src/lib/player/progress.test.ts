import { afterEach, describe, expect, it, vi } from 'vitest';
import { session } from '../auth/session';
import { ProgressTracker, writeProgress, preloadProgress, readProgress, isCreditsOrCompleted, isResumable } from './progress';
afterEach(()=>{session.set(null);vi.unstubAllGlobals();vi.useRealTimers();});
const state={position:4,duration:12,paused:true,volume:1,speed:1};
function login() {session.set({user:{id:'user-a',login:'a',role:'user'},csrfToken:'test-csrf'});}
describe('user progress',()=>{
  it('identifies resumable progress adhering to Jellyfin percentage and duration thresholds', () => {
    const item = { workId: 'w1', position: 100, duration: 1000, watched: false, revision: 1 };
    expect(isResumable(item)).toBe(true);
    expect(isResumable({ ...item, watched: true })).toBe(false);
    expect(isResumable({ ...item, position: 0 })).toBe(false);
    // Below minResumePct (e.g. 5%)
    expect(isResumable({ ...item, position: 40 }, { minResumePct: 5 })).toBe(false);
    expect(isResumable({ ...item, position: 60 }, { minResumePct: 5 })).toBe(true);
    // Above maxResumePct (e.g. 90%)
    expect(isResumable({ ...item, position: 910 }, { maxResumePct: 90 })).toBe(false);
    // Below minResumeDurationSeconds (e.g. 300s)
    expect(isResumable({ ...item, duration: 200, position: 50 }, { minResumeDurationSeconds: 300 })).toBe(false);
  });
  it('marks media as watched when end credits are reached',()=>{
    expect(isCreditsOrCompleted(5400, 6000)).toBe(true);
    expect(isCreditsOrCompleted(5300, 6000)).toBe(true);
    expect(isCreditsOrCompleted(4800, 6000, 4750)).toBe(true);
    expect(isCreditsOrCompleted(3000, 6000)).toBe(false);
    expect(isCreditsOrCompleted(5700, 6000, undefined, 98)).toBe(false);
    expect(isCreditsOrCompleted(5900, 6000, undefined, 98)).toBe(true);
    expect(isCreditsOrCompleted(500, 1000, undefined, 50)).toBe(true);
  });
  it('consumes a recent hover prefetch once and otherwise reloads the live revision',async()=>{
    login();const fetch=vi.fn().mockImplementation(async()=>Response.json({workId:'work',position:4,duration:12,watched:false,revision:7}));vi.stubGlobal('fetch',fetch);
    preloadProgress('work');preloadProgress('work');
    expect((await readProgress('work')).revision).toBe(7);expect(fetch).toHaveBeenCalledTimes(1);
    await readProgress('work');expect(fetch).toHaveBeenCalledTimes(2);
  });
  it('expires prefetched progress and discards it when the account changes',async()=>{
    vi.useFakeTimers();login();const fetch=vi.fn().mockImplementation(async()=>Response.json({position:4,watched:false,revision:1}));vi.stubGlobal('fetch',fetch);
    preloadProgress('work');await vi.advanceTimersByTimeAsync(5001);await readProgress('work');expect(fetch).toHaveBeenCalledTimes(2);
    preloadProgress('work');session.set(null);login();await readProgress('work');expect(fetch).toHaveBeenCalledTimes(4);
  });
  it('invalidates a prefetch before writing or retrying a progress update',async()=>{
    login();const fetch=vi.fn().mockImplementation(async()=>Response.json({position:4,watched:false,revision:1}));vi.stubGlobal('fetch',fetch);
    vi.stubGlobal('window',{dispatchEvent:vi.fn()});
    preloadProgress('work');await writeProgress({workId:'work',position:1,duration:12,watched:false,revision:0});await readProgress('work');expect(fetch).toHaveBeenCalledTimes(3);
  });
  it('does not overwrite a newer session revision after a conflict',async()=>{
    login();const error=vi.fn();const fetch=vi.fn().mockResolvedValueOnce(Response.json({workId:'work',position:4,duration:12,watched:false,revision:7})).mockResolvedValue(new Response(null,{status:409}));vi.stubGlobal('fetch',fetch);
    const tracker=new ProgressTracker('work',error);expect(await tracker.load()).toBe(4);tracker.save(state,true);
    await vi.waitFor(()=>expect(error).toHaveBeenCalledOnce());tracker.save(state,true);expect(fetch).toHaveBeenCalledTimes(2);
    expect(JSON.parse(fetch.mock.calls[1][1].body).expectedRevision).toBe(7);
  });
  it('discards pending writes after switching users',async()=>{
    login();const fetch=vi.fn().mockResolvedValue(Response.json({position:4,watched:false,revision:1}));vi.stubGlobal('fetch',fetch);
    const tracker=new ProgressTracker('work',vi.fn());await tracker.load();session.set(null);tracker.save(state,true);await Promise.resolve();expect(fetch).toHaveBeenCalledOnce();
  });
  it('requires an active session to save',async()=>{
    await expect(writeProgress({workId:'work',position:1,duration:12,watched:false,revision:0})).rejects.toThrow('Session requise');
  });
});
