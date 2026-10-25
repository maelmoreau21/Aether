import {afterEach,describe,it,expect,vi} from 'vitest';
import {session} from '../auth/session';
import {PlaybackPresence} from './presence';
afterEach(()=>{session.set(null);vi.unstubAllGlobals();vi.useRealTimers();});
const state={position:4,duration:120,paused:false,volume:1,speed:1};
const login=()=>session.set({user:{id:'user-a',login:'a',role:'user'},csrfToken:'test-csrf'});
describe('playback presence',()=>{
 it('reports a paused reader and expires reporting after account changes',async()=>{vi.useFakeTimers();login();const fetch=vi.fn().mockResolvedValue(new Response(null,{status:204}));vi.stubGlobal('fetch',fetch);const presence=new PlaybackPresence('film',()=>({...state,paused:true}));await vi.advanceTimersByTimeAsync(15000);expect(JSON.parse(fetch.mock.calls[1][1].body).paused).toBe(true);session.set(null);await vi.advanceTimersByTimeAsync(15000);expect(fetch).toHaveBeenCalledTimes(2);presence.stop();});
 it('orders the final stop after an in-flight heartbeat and never reports again',async()=>{vi.useFakeTimers();login();let resolve!:(value:Response)=>void;const fetch=vi.fn().mockImplementationOnce(()=>new Promise<Response>(done=>resolve=done)).mockResolvedValue(new Response(null,{status:204}));vi.stubGlobal('fetch',fetch);const presence=new PlaybackPresence('film',()=>state);presence.stop();expect(fetch).toHaveBeenCalledTimes(1);resolve(new Response(null,{status:204}));await vi.advanceTimersByTimeAsync(0);expect(fetch.mock.calls[1][1].method).toBe('DELETE');await vi.advanceTimersByTimeAsync(30000);expect(fetch).toHaveBeenCalledTimes(2);});
});
