import { get } from 'svelte/store';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { session } from './auth/session';
import { ensurePreferences, hydratePreferences, preferences, preferenceState, savePreferences } from './preferences';

const account = (id: string) => ({ user: { id, login: id, role: 'user' as const }, csrfToken: `csrf-${id}` });
afterEach(() => { session.set(null); vi.unstubAllGlobals(); });

describe('account preferences', () => {
  it('publishes account appearance after hydration, focus refresh and logout', async () => {
    session.set(account('appearance'));
    const fetch = vi.fn().mockResolvedValueOnce(Response.json({ homeSections: ['series', 'continue'], cardSize: 'small', accentColor: 'blue' }))
      .mockResolvedValueOnce(new Response(null, { status: 204 }))
      .mockResolvedValueOnce(Response.json({ homeSections: [], cardSize: 'large', accentColor: 'green' }));
    vi.stubGlobal('fetch', fetch);
    await hydratePreferences();
    expect(get(preferenceState).homeSections).toEqual(['series', 'continue']);
    expect(await savePreferences({ cardDensity: 'compact' })).toBe(true);
    expect(get(preferenceState).cardDensity).toBe('compact');
    await hydratePreferences();
    expect(get(preferenceState)).toEqual({ homeSections: [], cardSize: 'large', accentColor: 'green' });
    session.set(null);
    expect(get(preferenceState)).toEqual({});
  });
  it('reports a failed server save without claiming successful persistence', async () => {
    session.set(account('failure'));
    vi.stubGlobal('fetch', vi.fn().mockResolvedValueOnce(Response.json({})).mockResolvedValueOnce(new Response(null, { status: 503 })));
    await hydratePreferences();
    expect(await savePreferences({ hidePlayedInLatest: true })).toBe(false);
  });
  it('loads and saves playback defaults with the current account CSRF token', async () => {
    session.set(account('playback'));
    const fetch = vi.fn().mockResolvedValueOnce(Response.json({ autoplayNextEpisode: true, skipBackSeconds: 5, skipForwardSeconds: 30 }))
      .mockResolvedValueOnce(new Response(null, { status: 204 }));
    vi.stubGlobal('fetch', fetch);
    expect(await hydratePreferences()).toEqual({ autoplayNextEpisode: true, skipBackSeconds: 5, skipForwardSeconds: 30 });
    const saving = savePreferences({ autoplayNextEpisode: false, skipForwardSeconds: 60 });
    expect(await saving).toBe(true);
    await vi.waitFor(() => expect(fetch).toHaveBeenCalledTimes(2));
    const request = fetch.mock.calls[1][1];
    expect(JSON.parse(request.body)).toEqual({ autoplayNextEpisode: false, skipForwardSeconds: 60 });
    expect(request.headers['x-csrf-token']).toBe('csrf-playback');
    expect(preferences()).toEqual({ autoplayNextEpisode: false, skipBackSeconds: 5, skipForwardSeconds: 60 });
  });
  it('shares slow startup hydration with playback and reuses loaded settings', async () => {
    session.set(account('a'));
    let respond!: (response: Response) => void;
    const fetch = vi.fn(() => new Promise<Response>(resolve => { respond = resolve; }));
    vi.stubGlobal('fetch', fetch);
    const loading = hydratePreferences();
    expect(ensurePreferences()).toBe(loading);
    expect(hydratePreferences()).toBe(loading);
    respond(Response.json({ audioLanguage: 'en' }));
    await loading;
    expect(await ensurePreferences()).toEqual({ audioLanguage: 'en' });
    expect(fetch).toHaveBeenCalledTimes(1);
  });
  it('hydrates server settings without importing legacy browser preferences', async () => {
    const storage = { getItem: vi.fn((key: string) => key ? 'previous-account-value' : null), setItem: vi.fn(), removeItem: vi.fn() };
    vi.stubGlobal('localStorage', storage);
    session.set(account('a'));
    const fetch = vi.fn().mockResolvedValue(Response.json({ theme: 'dark', audioLanguage: 'fr' }));
    vi.stubGlobal('fetch', fetch);
    expect(await hydratePreferences()).toEqual({ theme: 'dark', audioLanguage: 'fr' });
    expect(storage.getItem.mock.calls.every(([key]) => !['media_theme', 'media_locale', 'media_pref_audio', 'media_pref_subtitles'].includes(key))).toBe(true);
    expect(storage.removeItem).toHaveBeenCalledWith('media_subtitle_a');
  });

  it('ignores a delayed response belonging to a previous account', async () => {
    vi.stubGlobal('localStorage', { getItem: vi.fn(() => JSON.stringify({ audio: 4, subtitle: 1 })), setItem: vi.fn(), removeItem: vi.fn() });
    session.set(account('a'));
    let respond!: (response: Response) => void;
    const fetch = vi.fn(() => new Promise<Response>(resolve => { respond = resolve; }));
    vi.stubGlobal('fetch', fetch);
    const loading = hydratePreferences();
    session.set(account('b'));
    respond(Response.json({ offsets: { audio: 4, subtitle: 1 } }));
    expect(await loading).toBeNull();
    expect(preferences()).toEqual({});
    expect(fetch).toHaveBeenCalledTimes(1);
  });

  it('migrates only validated scoped settings missing on the server', async () => {
    const values: Record<string, string> = { media_audio_a: 'en', media_audio_b: 'de', media_offsets_a: '{"audio":0.5,"subtitle":-1}', media_audio_track_a: '{"mediaId":"m","trackId":"t"}', media_subtitle_a: '{"enabled":true,"language":"fr","forced":true}', media_subtitle_style_a: '{"fontSize":"large","textColor":"#ffff00"}', media_pref_audio: 'es' };
    const storage = { getItem: vi.fn((key: string) => values[key] ?? null), setItem: vi.fn(), removeItem: vi.fn() };
    vi.stubGlobal('localStorage', storage); session.set(account('a'));
    const fetch = vi.fn().mockResolvedValueOnce(Response.json({ audioLanguage: 'fr' })).mockResolvedValueOnce(new Response(null, { status: 204 }));
    vi.stubGlobal('fetch', fetch);
    const loaded = await hydratePreferences();
    expect(loaded?.audioLanguage).toBe('fr');
    expect(loaded?.offsets).toEqual({ audio: 0.5, subtitle: -1 });
    const patch = JSON.parse(fetch.mock.calls[1][1].body);
    expect(patch.audioLanguage).toBeUndefined();
    expect(patch.audioTrack).toEqual({ mediaId: 'm', trackId: 't' });
    expect(patch.subtitlePreference).toEqual({ enabled: true, language: 'fr', forced: true });
    expect(patch.subtitleStyle).toEqual({ fontSize: 'large', textColor: '#ffff00' });
    expect(fetch.mock.calls[1][1].headers['x-csrf-token']).toBe('csrf-a');
    expect(storage.getItem).not.toHaveBeenCalledWith('media_audio_b');
    expect(storage.getItem).not.toHaveBeenCalledWith('media_pref_audio');
  });

  it('preserves scoped cache until migration succeeds and reports failure', async () => {
    const storage = { getItem: vi.fn((key: string) => key === 'media_offsets_a' ? '{"audio":1,"subtitle":0}' : null), setItem: vi.fn(), removeItem: vi.fn() };
    vi.stubGlobal('localStorage', storage); session.set(account('a')); storage.removeItem.mockClear();
    vi.stubGlobal('fetch', vi.fn().mockResolvedValueOnce(Response.json({})).mockResolvedValueOnce(new Response(null, { status: 500 })));
    await expect(hydratePreferences()).rejects.toThrow('conservées');
    expect(storage.removeItem).not.toHaveBeenCalled(); expect(storage.setItem).not.toHaveBeenCalled();
  });

  it('drops queued writes when the account generation changes', async () => {
    session.set(account('a'));
    const fetch = vi.fn().mockResolvedValue(Response.json({}));
    vi.stubGlobal('fetch', fetch);
    await hydratePreferences();
    const saving = savePreferences({ theme: 'light' });
    session.set(account('b'));
    await Promise.resolve(); await Promise.resolve();
    expect(fetch).toHaveBeenCalledTimes(1);
    expect(preferences()).toEqual({});
    expect(await saving).toBe(false);
  });
});
