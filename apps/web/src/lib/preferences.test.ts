import { afterEach, describe, expect, it, vi } from 'vitest';
import { session } from './auth/session';
import { hydratePreferences, preferences, savePreferences } from './preferences';

const account = (id: string) => ({ user: { id, login: id, role: 'user' as const }, csrfToken: `csrf-${id}` });
afterEach(() => { session.set(null); vi.unstubAllGlobals(); });

describe('account preferences', () => {
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
    const values: Record<string, string> = { media_audio_a: 'en', media_audio_b: 'de', media_offsets_a: '{"audio":0.5,"subtitle":-1}', media_audio_track_a: '{"mediaId":"m","trackId":"t"}', media_subtitle_a: '{"enabled":true,"language":"fr","forced":true}', media_pref_audio: 'es' };
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
    savePreferences({ theme: 'light' });
    session.set(account('b'));
    await Promise.resolve(); await Promise.resolve();
    expect(fetch).toHaveBeenCalledTimes(1);
    expect(preferences()).toEqual({});
  });
});
