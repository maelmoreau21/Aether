import { afterEach, describe, expect, it, vi } from 'vitest';
import { session } from '../auth/session';
import { ProgressTracker, writeProgress } from './progress';
afterEach(()=>{session.set(null);vi.unstubAllGlobals();vi.useRealTimers();});
const state={position:4,duration:12,paused:true,volume:1,speed:1};
function login() {session.set({user:{id:'user-a',login:'a',role:'user'},csrfToken:'test-csrf'});}
describe('user progress',()=>{
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
