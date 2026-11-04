import { afterEach, describe, expect, it, vi } from 'vitest';
import { get } from 'svelte/store';
import { session } from '../auth/session';
import { applicationRequest, errorMessage, features, loadFeatures, parseSelection, reportPayload, requestPayload } from './api';

afterEach(() => { session.set(null); vi.unstubAllGlobals(); });
describe('demandes et signalements', () => {
  it('submits only admission fields, preserving the searched server and explicit scope', () => {
    expect(requestPayload({ kind: 'series', title: 'Synthetic', externalId: '7', externalProvider: 'tvdb', serverId: 'sonarr-2', year: 2026, summary: 'Discovery-only metadata' }, { seasons: [2], monitorFuture: false })).toEqual({ kind: 'series', title: 'Synthetic', externalId: '7', externalProvider: 'tvdb', serverId: 'sonarr-2', seasons: [2], episodes: [], albumIds: [], monitorFuture: false });
    expect(errorMessage('track_not_supported')).toContain('Lidarr');
  });
  it('validates explicit season/episode selections without silently turning empty entries into specials', () => {
    expect(parseSelection('1, 2,1', '1:3, 2x1, 1:3')).toEqual({ seasons: [1, 2], episodes: [{ season: 1, episode: 3 }, { season: 2, episode: 1 }] });
    for (const invalid of ['1,,2', '-1', '1.5', '1001']) expect(() => parseSelection(invalid, '')).toThrow();
    expect(() => parseSelection('', '1:0')).toThrow();
    expect(() => parseSelection('', 'episode 1')).toThrow();
  });
  it('keeps a report attached to its media and sends only supported diagnostic fields', () => {
    const result = reportPayload({ category: 'audio', description: '  Audio décalé  ', workId: 'episode-2', positionSeconds: 10, endSeconds: 20, context: { engine: 'direct', audioTrackId: 'audio-2', rawLog: 'private file path' } } as Parameters<typeof reportPayload>[0]);
    expect(result.description).toBe('Audio décalé');
    expect(result.workId).toBe('episode-2');
    expect(result.context).toEqual({ engine: 'direct', audioTrackId: 'audio-2' });
    expect(() => reportPayload({ category: 'audio', description: 'test' })).toThrow();
    expect(() => reportPayload({ category: 'audio', description: 'test', trackId: 'track', positionSeconds: 20, endSeconds: 10 })).toThrow();
    expect(() => reportPayload({ category: 'audio', description: 'test', trackId: 'track', positionSeconds: NaN })).toThrow();
  });
  it('uses the current CSRF token and reports backend failures', async () => {
    session.set({ user: { id: 'first', login: 'first', role: 'user' }, csrfToken: 'fresh' });
    const fetcher = vi.fn().mockResolvedValue(new Response(null, { status: 204 }));
    vi.stubGlobal('fetch', fetcher);
    await applicationRequest('/api/v1/reports', { method: 'POST', body: '{}' });
    expect(fetcher.mock.calls[0][1]).toMatchObject({ credentials: 'same-origin', headers: { 'x-csrf-token': 'fresh' } });
    fetcher.mockResolvedValueOnce(new Response(JSON.stringify({ error: 'csrf_required' }), { status: 403 }));
    await expect(applicationRequest('/api/v1/reports')).rejects.toThrow('Votre session a changé');
  });
  it('does not restore feature visibility from a request belonging to an old session', async () => {
    session.set({ user: { id: 'first', login: 'first', role: 'user' }, csrfToken: 'one' });
    let finish: (response: Response) => void = () => {};
    vi.stubGlobal('fetch', vi.fn().mockImplementation(() => new Promise<Response>(resolve => { finish = resolve; })));
    const loading = loadFeatures(true);
    session.set(null);
    finish(new Response(JSON.stringify({ requests: true, reports: true })));
    await expect(loading).rejects.toThrow('Votre session a changé');
    expect(get(features)).toEqual({ requests: false, reports: false });
  });
});
