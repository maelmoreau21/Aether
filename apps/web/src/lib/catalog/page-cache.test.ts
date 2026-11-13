import { afterEach, describe, expect, it, vi } from 'vitest';
import { cachedCatalog, catalogCacheStats, invalidateCatalog, invalidateLatestCatalog, loadCatalog, preloadCatalog } from './page-cache';
import { session } from '../auth/session';
import { locale } from '../i18n';
import { preferenceState } from '../preferences';
const page = (id = 'a', libraryId = 'lib') => ({items:[{id,libraryId,title:id}], total:1,offset:0,limit:24,notice:''});
afterEach(() => { invalidateCatalog(); vi.useRealTimers(); vi.unstubAllGlobals(); });
describe('session catalogue SWR cache', () => {
  it('refreshes watched shelves without evicting normal browsing or resurrecting a late shelf', async () => {
    const latest = '/api/v1/catalog?scope=latest';
    const normal = '/api/v1/catalog?kind=film';
    const fetcher = vi.fn().mockImplementation(() => Promise.resolve(Response.json(page())));
    vi.stubGlobal('fetch', fetcher);
    await loadCatalog(latest, latest);
    await loadCatalog(normal, normal);
    invalidateLatestCatalog();
    expect(cachedCatalog(latest)).toBeUndefined();
    expect(cachedCatalog(normal)).toBeDefined();
    let finish!: (response: Response) => void;
    fetcher.mockImplementationOnce(() => new Promise<Response>(resolve => finish = resolve));
    const pending = loadCatalog(latest, latest);
    const rejected = expect(pending).rejects.toThrow();
    invalidateLatestCatalog();
    finish(Response.json(page('stale')));
    await rejected;
    expect(cachedCatalog(latest)).toBeUndefined();
    expect(cachedCatalog(normal)).toBeDefined();
  });
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
  it('drops pages rendered in the previous language once the new language is stored', async () => {
    const fetcher = vi.fn().mockImplementation(() => Promise.resolve(Response.json(page())));
    vi.stubGlobal('fetch', fetcher);
    // The module registers its listener on `window` at import time: provide one first.
    const browser = new EventTarget();
    vi.stubGlobal('window', browser);
    vi.resetModules();
    const fresh = await import('./page-cache');
    await fresh.loadCatalog('home', '/api/v1/catalog');
    expect(fresh.cachedCatalog('home')).toBeDefined();
    browser.dispatchEvent(new Event('media-locale-saved'));
    expect(fresh.cachedCatalog('home')).toBeUndefined();
    await fresh.loadCatalog('home', '/api/v1/catalog');
    expect(fetcher).toHaveBeenCalledTimes(2);
    fresh.invalidateCatalog();
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
    const rejected=expect(refresh).rejects.toThrow('offline'); await vi.advanceTimersByTimeAsync(1500);
    await rejected; expect(fetcher).toHaveBeenCalledTimes(3); expect(cachedCatalog('home')).toBeDefined();
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

describe('catalogue cache isolation', () => {
  const user = (id: string) => ({ user: { id, login: id, displayName: id, role: 'user' as const }, csrfToken: `csrf-${id}` }) as never;

  it('never serves a page rendered for another account or another language chain', async () => {
    const fetcher = vi.fn().mockImplementation((url: string) => Promise.resolve(Response.json(page(new URL(url, 'http://local').searchParams.get('locale') ?? 'none'))));
    vi.stubGlobal('fetch', fetcher);
    const key = '/api/v1/catalog?kind=film';

    session.set(user('ana'));
    locale.set('fr');
    preferenceState.set({});
    expect((await loadCatalog(key, key)).items[0].id).toBe('fr');
    expect(cachedCatalog(key)?.items[0].id).toBe('fr');

    // Same account, other language: the French page is not found under the English key.
    locale.set('en');
    expect(cachedCatalog(key)).toBeUndefined();
    expect((await loadCatalog(key, key)).items[0].id).toBe('en');

    // A metadata language chosen apart from the interface language is a different scope again.
    preferenceState.set({ metadataLocale: 'ja' });
    expect(cachedCatalog(key)).toBeUndefined();
    expect((await loadCatalog(key, key)).items[0].id).toBe('ja');

    // Another account with the same languages does not see ana's page.
    session.set(user('bob'));
    expect(cachedCatalog(key)).toBeUndefined();
    expect(fetcher).toHaveBeenCalledTimes(3);
    session.set(null);
    locale.set('fr');
    preferenceState.set({});
  });

  it('drops a response that finishes after the language changed', async () => {
    let finish!: (response: Response) => void;
    vi.stubGlobal('fetch', vi.fn().mockImplementation(() => new Promise<Response>(resolve => finish = resolve)));
    const key = '/api/v1/catalog?kind=series';
    session.set(user('ana'));
    locale.set('fr');
    const late = loadCatalog(key, key);
    const rejected = expect(late).rejects.toThrow();
    locale.set('en');
    finish(Response.json(page('fr-page')));
    await rejected;
    expect(cachedCatalog(key)).toBeUndefined();
    session.set(null);
    locale.set('fr');
  });
  it('tries a busy server once more but never retries an access answer', async () => {
    vi.useFakeTimers();
    const url = '/api/v1/catalog?kind=series';
    const fetcher = vi.fn()
      .mockResolvedValueOnce(new Response('{}', { status: 503 }))
      .mockResolvedValueOnce(Response.json(page('ok')));
    vi.stubGlobal('fetch', fetcher);
    const loading = loadCatalog(url, url);
    await vi.advanceTimersByTimeAsync(1500);
    expect((await loading).items[0].id).toBe('ok');
    expect(fetcher).toHaveBeenCalledTimes(2);
    const denied = '/api/v1/catalog?kind=film';
    fetcher.mockResolvedValueOnce(new Response('{}', { status: 403 }));
    await expect(loadCatalog(denied, denied)).rejects.toThrow();
    expect(fetcher).toHaveBeenCalledTimes(3);
  });
});
