import { afterEach, describe, expect, it, vi } from 'vitest';
import { cachedCatalog, catalogCacheStats, invalidateCatalog, loadCatalog, preloadCatalog } from './page-cache';
import { session } from '../auth/session';
const page = (id = 'a', libraryId = 'lib') => ({items:[{id,libraryId,title:id}], total:1,offset:0,limit:24,notice:''});
afterEach(() => { invalidateCatalog(); vi.useRealTimers(); vi.unstubAllGlobals(); });
describe('session catalogue SWR cache', () => {
  it('shares navigation prefetch with the display request including library and sorting', async () => {
    const fetcher = vi.fn().mockImplementation(() => Promise.resolve(Response.json(page())));
    vi.stubGlobal('fetch', fetcher);
    preloadCatalog('films', 'lib & privé');
    const url = `/api/v1/catalog?${new URLSearchParams({library:'lib & privé',kind:'film',q:'',sort:'title-asc',offset:'0',limit:'24',count:'false'})}`;
    await loadCatalog(url, url);
    expect(fetcher).toHaveBeenCalledTimes(1);
    expect(cachedCatalog(url)?.items[0].id).toBe('a');
    preloadCatalog('home');
    await Promise.resolve();
    expect(fetcher).toHaveBeenCalledTimes(3);
  });
  it('deduplicates and serves ten fresh returns with zero extra requests', async () => {
    const fetcher=vi.fn().mockResolvedValue(Response.json(page())); vi.stubGlobal('fetch',fetcher);
    await Promise.all([loadCatalog('home','/api/v1/catalog'),loadCatalog('home','/api/v1/catalog')]);
    for(let i=0;i<10;i++) { expect(cachedCatalog('home')?.items[0].id).toBe('a'); await loadCatalog('home','/api/v1/catalog'); }
    expect(fetcher).toHaveBeenCalledTimes(1);
  });
  it('retains stale content after temporary network failure', async () => {
    vi.useFakeTimers(); const fetcher=vi.fn().mockResolvedValueOnce(Response.json(page())).mockRejectedValue(new TypeError('offline')); vi.stubGlobal('fetch',fetcher);
    await loadCatalog('home','/api/v1/catalog'); vi.advanceTimersByTime(31000);
    const refresh=loadCatalog('home','/api/v1/catalog'); expect(cachedCatalog('home')).toBeDefined();
    await expect(refresh).rejects.toThrow('offline'); expect(cachedCatalog('home')).toBeDefined();
  });
  it('separates query keys and invalidates a library and aggregate pages only', async () => {
    vi.stubGlobal('fetch',vi.fn().mockImplementation(()=>Promise.resolve(Response.json(page()))));
    await loadCatalog('home','/api/v1/catalog'); await loadCatalog('a','/api/v1/catalog?library=a'); await loadCatalog('b','/api/v1/catalog?library=b');
    invalidateCatalog('a'); expect(cachedCatalog('home')).toBeUndefined(); expect(cachedCatalog('a')).toBeUndefined(); expect(cachedCatalog('b')).toBeDefined();
  });
  it('clears on account switch and rejects late replies from the previous session', async () => {
    let finish!: (response:Response)=>void;
    vi.stubGlobal('fetch',vi.fn(()=>new Promise<Response>(resolve=>finish=resolve)));
    const result=loadCatalog('home','/api/v1/catalog');
    session.set({user:{id:'new',login:'new',role:'user'},csrfToken:'new'});
    finish(Response.json(page())); await expect(result).rejects.toThrow(); expect(cachedCatalog('home')).toBeUndefined();
  });
  it('evicts at 48 pages and rejects an oversized entry', async () => {
    const fetcher=vi.fn().mockImplementation(()=>Promise.resolve(Response.json(page()))); vi.stubGlobal('fetch',fetcher);
    for(let i=0;i<55;i++) await loadCatalog(String(i),'/api/v1/catalog');
    expect(catalogCacheStats().pages).toBe(48); expect(cachedCatalog('0')).toBeUndefined();
    fetcher.mockResolvedValueOnce(Response.json(page('x'.repeat(4*1024*1024))));
    await loadCatalog('big','/api/v1/catalog'); expect(cachedCatalog('big')).toBeUndefined(); expect(catalogCacheStats().jsonBytes).toBeLessThan(4*1024*1024);
  });
  it('keeps out-of-order pages separate and prevents resurrection after invalidation', async () => {
    const replies: ((response:Response)=>void)[]=[];
    vi.stubGlobal('fetch',vi.fn(()=>new Promise<Response>(resolve=>replies.push(resolve))));
    const a=loadCatalog('search-a','/api/v1/catalog?q=a'); const b=loadCatalog('search-b','/api/v1/catalog?q=b');
    replies[1](Response.json(page('b'))); await b;
    replies[0](Response.json(page('a'))); await a;
    expect(cachedCatalog('search-a')?.items[0].id).toBe('a');expect(cachedCatalog('search-b')?.items[0].id).toBe('b');
    const old=loadCatalog('removed','/api/v1/catalog?library=removed');
    const rejection=expect(old).rejects.toThrow();invalidateCatalog('removed');
    replies[2](Response.json(page('old')));await rejection;expect(cachedCatalog('removed')).toBeUndefined();
  });
  it('clears on logout and on a new session of the same account', async () => {
    vi.stubGlobal('fetch',vi.fn().mockImplementation(()=>Promise.resolve(Response.json(page()))));
    const user={id:'account',login:'account',role:'user' as const};
    session.set({user,csrfToken:'first'});await loadCatalog('home','/api/v1/catalog');
    session.set(null);expect(cachedCatalog('home')).toBeUndefined();
    session.set({user,csrfToken:'second'});await loadCatalog('home','/api/v1/catalog');
    session.set({user,csrfToken:'third'});expect(cachedCatalog('home')).toBeUndefined();
  });

});
