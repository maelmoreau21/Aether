import { afterEach, describe, expect, it, vi } from 'vitest';
import { get } from 'svelte/store';
import {
  cacheMedia,
  clearDescriptorCache,
  fetchMediaDescriptor,
  fetchMediaDescriptors,
  getCachedMedia,
  playbackIds,
  preloadMediaDescriptor,
  revalidateMediaDescriptor,
  sourceLabel,
  type Work
} from './api';
import { login, logout, readSession, session, type Session } from '../auth/session';
import type { Media } from '../player/types';

const sessionA: Session = {
  user: { id: 'user-a', login: 'alice', role: 'user' },
  csrfToken: 'csrf-a'
};

const sessionB: Session = {
  user: { id: 'user-b', login: 'bob', role: 'user' },
  csrfToken: 'csrf-b'
};

const sampleMedia: Media = {
  id: 'media-1',
  title: 'Épisode 1',
  sources: [{ id: 's1', audioTrackId: 'a1', language: 'fr', url: '/api/v1/assets/media-1/content', mime: 'video/x-matroska' }],
  subtitles: []
};

const sampleMedia2: Media = {
  id: 'media-2',
  title: 'Épisode 2',
  sources: [{ id: 's2', audioTrackId: 'a2', language: 'fr', url: '/api/v1/assets/media-2/content', mime: 'video/mp4' }],
  subtitles: []
};

afterEach(() => {
  vi.unstubAllGlobals();
  session.set(null);
  clearDescriptorCache();
});

describe('media descriptor cache and deferred loading', () => {
  it('caches descriptors within the same session and does not re-fetch from network', async () => {
    const fetchMock = vi.fn().mockResolvedValue(Response.json(sampleMedia));
    vi.stubGlobal('fetch', fetchMock);

    expect(getCachedMedia('media-1')).toBeUndefined();
    const result1 = await fetchMediaDescriptor('media-1');
    expect(result1).toEqual(sampleMedia);
    expect(fetchMock).toHaveBeenCalledTimes(1);

    expect(getCachedMedia('media-1')).toEqual(sampleMedia);

    // Second call in same session comes from cache
    const result2 = await fetchMediaDescriptor('media-1');
    expect(result2).toEqual(sampleMedia);
    expect(fetchMock).toHaveBeenCalledTimes(1);
  });

  it('supports batch prefetching without duplicating cached entries', async () => {
    const fetchMock = vi.fn().mockImplementation((url: string) => {
      if (url.includes('media-1')) return Promise.resolve(Response.json(sampleMedia));
      if (url.includes('media-2')) return Promise.resolve(Response.json(sampleMedia2));
      return Promise.reject(new Error('not found'));
    });
    vi.stubGlobal('fetch', fetchMock);

    const results = await fetchMediaDescriptors(['media-1', 'media-2']);
    expect(results).toHaveLength(2);
    expect(fetchMock).toHaveBeenCalledTimes(2);

    // Pre-cached: zero additional network calls
    const cachedAgain = await fetchMediaDescriptors(['media-1', 'media-2']);
    expect(cachedAgain).toEqual(results);
    expect(fetchMock).toHaveBeenCalledTimes(2);
  });

  it('isolates cache across login user A -> cache filled -> logout -> login user B', async () => {
    const mediaForB: Media = { ...sampleMedia, title: 'Épisode 1 (Session B)' };
    const fetchMock = vi.fn().mockImplementation((url: string) => {
      if (url === '/api/v1/auth/login') {
        const currentLoginCount = fetchMock.mock.calls.filter(c => c[0] === '/api/v1/auth/login').length;
        return Promise.resolve(Response.json(currentLoginCount === 1 ? sessionA : sessionB));
      }
      if (url === '/api/v1/auth/logout') {
        return Promise.resolve(new Response(null, { status: 204 }));
      }
      if (url === '/api/v1/media/media-1') {
        return Promise.resolve(Response.json(get(session)?.user.id === 'user-a' ? sampleMedia : mediaForB));
      }
      return Promise.reject(new Error(`unexpected url: ${url}`));
    });
    vi.stubGlobal('fetch', fetchMock);

    // 1. User A logs in and populates cache
    await login('alice', 'password-a');
    expect(get(session)).toEqual(sessionA);
    const resA = await fetchMediaDescriptor('media-1');
    expect(resA.title).toBe('Épisode 1');
    expect(getCachedMedia('media-1')).toEqual(sampleMedia);

    // 2. User A logs out -> cache must be purged immediately
    await logout(sessionA);
    expect(get(session)).toBeNull();
    expect(getCachedMedia('media-1')).toBeUndefined();

    // 3. User B logs in -> cache must remain empty and fetch fresh data for B
    await login('bob', 'password-b');
    expect(get(session)).toEqual(sessionB);
    expect(getCachedMedia('media-1')).toBeUndefined();

    const resB = await fetchMediaDescriptor('media-1');
    expect(resB.title).toBe('Épisode 1 (Session B)');
    expect(getCachedMedia('media-1')).toEqual(mediaForB);
  });

  it('purges cache when the authenticated user account changes directly', async () => {
    session.set(sessionA);
    cacheMedia(sampleMedia);
    expect(getCachedMedia('media-1')).toEqual(sampleMedia);

    // Account changes directly to User B (e.g. via readSession or store update)
    vi.stubGlobal('fetch', vi.fn().mockResolvedValueOnce(Response.json(sessionB)));
    await readSession();
    expect(get(session)).toEqual(sessionB);
    expect(getCachedMedia('media-1')).toBeUndefined();
  });

  it('prevents an in-flight request from user A from repopulating the cache after user B connects', async () => {
    let resolveInFlightA!: (value: Response) => void;
    const fetchMock = vi.fn().mockImplementation((url: string) => {
      if (url === '/api/v1/media/media-1') {
        return new Promise<Response>(resolve => {
          resolveInFlightA = resolve;
        });
      }
      if (url === '/api/v1/auth/logout') {
        return Promise.resolve(new Response(null, { status: 204 }));
      }
      if (url === '/api/v1/auth/login') {
        return Promise.resolve(Response.json(sessionB));
      }
      if (url === '/api/v1/media/media-2') {
        return Promise.resolve(Response.json(sampleMedia2));
      }
      return Promise.reject(new Error(`unexpected url: ${url}`));
    });
    vi.stubGlobal('fetch', fetchMock);

    // User A is connected and starts a request for media-1
    session.set(sessionA);
    const inFlightA = fetchMediaDescriptor('media-1');

    // Before A's request finishes, A logs out and B connects
    await logout(sessionA);
    await login('bob', 'password-b');
    expect(get(session)).toEqual(sessionB);

    // B caches media-2
    await fetchMediaDescriptor('media-2');
    expect(getCachedMedia('media-2')).toEqual(sampleMedia2);

    // Now A's late response arrives
    resolveInFlightA(Response.json(sampleMedia));
    await inFlightA.catch(() => {});

    // A's late response must NOT have entered B's cache
    expect(getCachedMedia('media-1')).toBeUndefined();
    expect(getCachedMedia('media-2')).toEqual(sampleMedia2);
  });

  it('prevents a stale HTTP 401 from session A from logging out user B or clearing B cache', async () => {
    let resolveStale401!: (value: Response) => void;
    const fetchMock = vi.fn().mockImplementation((url: string) => {
      if (url === '/api/v1/media/media-1') {
        return new Promise<Response>(resolve => {
          resolveStale401 = resolve;
        });
      }
      if (url === '/api/v1/auth/login') {
        return Promise.resolve(Response.json(sessionB));
      }
      if (url === '/api/v1/media/media-2') {
        return Promise.resolve(Response.json(sampleMedia2));
      }
      return Promise.reject(new Error(`unexpected url: ${url}`));
    });
    vi.stubGlobal('fetch', fetchMock);

    session.set(sessionA);
    const staleRequestA = fetchMediaDescriptor('media-1');

    // User B logs in while A's request is still in flight
    await login('bob', 'password-b');
    expect(get(session)).toEqual(sessionB);
    await fetchMediaDescriptor('media-2');
    expect(getCachedMedia('media-2')).toEqual(sampleMedia2);

    // Now A's old request finishes with HTTP 401
    resolveStale401(new Response(null, { status: 401 }));
    await expect(staleRequestA).rejects.toThrow('Votre session a expiré. Connectez-vous à nouveau.');

    // User B must remain logged in and B's cached descriptors must remain intact
    expect(get(session)).toEqual(sessionB);
    expect(getCachedMedia('media-2')).toEqual(sampleMedia2);
  });

  it('invalidates current session and purges cache on a current HTTP 401', async () => {
    session.set(sessionA);
    cacheMedia(sampleMedia);
    expect(getCachedMedia('media-1')).toEqual(sampleMedia);

    vi.stubGlobal('fetch', vi.fn().mockResolvedValue(new Response(null, { status: 401 })));
    await expect(fetchMediaDescriptor('media-2')).rejects.toThrow('Votre session a expiré. Connectez-vous à nouveau.');

    expect(get(session)).toBeNull();
    expect(getCachedMedia('media-1')).toBeUndefined();
    expect(getCachedMedia('media-2')).toBeUndefined();
  });

  it('invalidates cached descriptors on HTTP 403 after library access revocation and allows playback again after rights restoration', async () => {
    let accessAllowed = true;
    const fetchMock = vi.fn().mockImplementation(() => {
      if (!accessAllowed) {
        return Promise.resolve(new Response(null, { status: 403 }));
      }
      return Promise.resolve(Response.json(sampleMedia));
    });
    vi.stubGlobal('fetch', fetchMock);
    session.set(sessionA);

    // 1. Initial prefetch / load succeeds and caches descriptor
    const initial = await preloadMediaDescriptor('media-1');
    expect(initial).toEqual(sampleMedia);
    expect(getCachedMedia('media-1')).toEqual(sampleMedia);
    expect(fetchMock).toHaveBeenCalledTimes(1);

    // 2. Rights are revoked on the library
    accessAllowed = false;

    // 3. Launching playback always revalidates with the server -> catches 403 and evicts cache
    await expect(revalidateMediaDescriptor('media-1')).rejects.toThrow('Votre compte n’a pas accès à cette ressource.');
    expect(fetchMock).toHaveBeenCalledTimes(2);
    expect(getCachedMedia('media-1')).toBeUndefined();

    // Even a standard fetchMediaDescriptor call now hits the server and fails with 403
    await expect(fetchMediaDescriptor('media-1')).rejects.toThrow('Votre compte n’a pas accès à cette ressource.');
    expect(fetchMock).toHaveBeenCalledTimes(3);

    // 4. Rights are restored on the library -> playback revalidation succeeds again
    accessAllowed = true;
    const restored = await revalidateMediaDescriptor('media-1');
    expect(restored).toEqual(sampleMedia);
    expect(getCachedMedia('media-1')).toEqual(sampleMedia);
    expect(fetchMock).toHaveBeenCalledTimes(4);
  });

  it('handles rapid switching between episodes with AbortController', async () => {
    const controller1 = new AbortController();
    const controller2 = new AbortController();

    const fetchMock = vi.fn().mockImplementation((url: string) => {
      if (url.includes('media-1')) {
        return new Promise<Response>(resolve => {
          setTimeout(() => resolve(Response.json(sampleMedia)), 200);
        });
      }
      if (url.includes('media-2')) {
        return Promise.resolve(Response.json(sampleMedia2));
      }
      return Promise.reject(new Error('not found'));
    });
    vi.stubGlobal('fetch', fetchMock);

    // Start launching episode 1, then immediately switch to episode 2 and abort episode 1
    const launch1 = revalidateMediaDescriptor('media-1', controller1.signal);
    controller1.abort();
    const launch2 = revalidateMediaDescriptor('media-2', controller2.signal);

    await expect(launch1).rejects.toThrow();
    const ep2Result = await launch2;
    expect(ep2Result).toEqual(sampleMedia2);
    expect(getCachedMedia('media-1')).toBeUndefined();
    expect(getCachedMedia('media-2')).toEqual(sampleMedia2);
  });

  it('does not repopulate cache when cache is cleared during an in-flight prefetch', async () => {
    let resolvePrefetch!: (value: Response) => void;
    const fetchMock = vi.fn().mockImplementation(() => {
      return new Promise<Response>(resolve => {
        resolvePrefetch = resolve;
      });
    });
    vi.stubGlobal('fetch', fetchMock);

    // Start prefetch
    const prefetchPromise = preloadMediaDescriptor('media-1');
    // Clear cache while prefetch is in flight
    clearDescriptorCache();
    // Resolve network response after clear
    resolvePrefetch(Response.json(sampleMedia));
    await prefetchPromise;

    // Cache must remain empty
    expect(getCachedMedia('media-1')).toBeUndefined();
  });

  it('enforces a bounded cache size by evicting the oldest descriptor', () => {
    for (let i = 1; i <= 129; i++) {
      cacheMedia({ ...sampleMedia, id: `media-${i}`, title: `Épisode ${i}` });
    }
    expect(getCachedMedia('media-1')).toBeUndefined();
    expect(getCachedMedia('media-2')?.title).toBe('Épisode 2');
    expect(getCachedMedia('media-129')?.title).toBe('Épisode 129');
  });

  it('computes correct source labels from media mime type', () => {
    expect(sourceLabel(sampleMedia)).toBe('MKV');
    expect(sourceLabel(sampleMedia2)).toBe('MP4');
  });

  it('extracts playback ids accurately from Work structure without triggering network requests', () => {
    const fetchMock = vi.fn();
    vi.stubGlobal('fetch', fetchMock);
    const work: Work = {
      id: 'ep-1',
      kind: 'episode',
      title: 'Episode 1',
      year: 2026,
      description: '',
      libraryId: 'lib-1',
      synthetic: false,
      editions: [
        {
          id: 'ed-1',
          label: 'Locale',
          versions: [
            { id: 'v1', label: '1080p', sources: [{ id: 's1', assetIds: ['a1'], playbackMediaId: 'a1' }] },
            { id: 'v2', label: '4k', sources: [{ id: 's2', assetIds: ['a2'], playbackMediaId: 'a2' }] }
          ]
        }
      ]
    };
    expect(playbackIds(work)).toEqual(['a1', 'a2']);
    expect(fetchMock).not.toHaveBeenCalled();
  });
});
