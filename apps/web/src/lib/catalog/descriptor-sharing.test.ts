import { afterEach, expect, it, vi } from 'vitest';
import { clearDescriptorCache, fetchMediaDescriptor, revalidateMediaDescriptor } from './api';
afterEach(()=>{clearDescriptorCache();vi.unstubAllGlobals();});
it('shares in-flight descriptors and isolates consumer cancellation', async()=>{
 let finish!: (r:Response)=>void;
 const fetcher=vi.fn(()=>new Promise<Response>(r=>finish=r));vi.stubGlobal('fetch',fetcher);
 const a=new AbortController(); const first=fetchMediaDescriptor('shared',a.signal);const second=revalidateMediaDescriptor('shared');
 const rejection=expect(first).rejects.toThrow(); a.abort();
 finish(Response.json({id:'shared',sources:[],subtitles:[],title:'Test'}));await rejection;expect((await second).id).toBe('shared');expect(fetcher).toHaveBeenCalledTimes(1);
 // A completed prefetch cannot authorize a later playback.
 fetcher.mockImplementation(()=>Promise.resolve(Response.json({id:'shared',sources:[],subtitles:[],title:'Fresh'})));
 expect((await revalidateMediaDescriptor('shared')).title).toBe('Fresh');expect(fetcher).toHaveBeenCalledTimes(2);
});
