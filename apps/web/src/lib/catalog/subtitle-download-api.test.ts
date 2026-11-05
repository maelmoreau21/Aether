import { afterEach, describe, expect, it, vi } from 'vitest';
import { subtitleErrorText, subtitleMatchText, subtitleRequest } from './subtitle-download-api';

afterEach(() => vi.unstubAllGlobals());

describe('subtitle provider client', () => {
  it('sends explicit mutations with CSRF and no-store, forwarding cancellation', async () => {
    const fetchMock = vi.fn().mockResolvedValue(new Response(JSON.stringify({ id: 'download', status: 'completed' })));
    vi.stubGlobal('fetch', fetchMock);
    const controller = new AbortController();
    await subtitleRequest('/api/v1/admin/subtitles/asset/download', controller.signal, 'test-csrf', 'POST', { searchId: 'search', fileId: 'file' });
    const options = fetchMock.mock.calls[0][1];
    expect(options.method).toBe('POST');
    expect(options.headers['x-csrf-token']).toBe('test-csrf');
    expect(JSON.parse(options.body)).toEqual({ searchId: 'search', fileId: 'file' });
    expect(options.credentials).toBe('same-origin');
    expect(options.cache).toBe('no-store');
    controller.abort();
    expect(options.signal.aborted).toBe(true);
  });

  it('returns a quota message without exposing provider payloads or retrying', async () => {
    const fetchMock = vi.fn().mockResolvedValue(new Response(JSON.stringify({ error: 'provider_quota', message: 'private-provider-detail' }), { status: 429 }));
    vi.stubGlobal('fetch', fetchMock);
    await expect(subtitleRequest('/subtitles', new AbortController().signal, 'test')).rejects.toThrow('Quota ou limite');
    expect(fetchMock).toHaveBeenCalledTimes(1);
  });

  it('does not expose unknown server errors', async () => {
    vi.stubGlobal('fetch', vi.fn().mockResolvedValue(new Response(JSON.stringify({ error: 'private-server-detail' }), { status: 500 })));
    await expect(subtitleRequest('/subtitles', new AbortController().signal, '')).rejects.toThrow('Les sous-titres sont indisponibles.');
  });

  it('accepts an empty successful deletion response', async () => {
    vi.stubGlobal('fetch', vi.fn().mockResolvedValue(new Response(null, { status: 204 })));
    await expect(subtitleRequest('/subtitles/download', new AbortController().signal, 'test', 'DELETE')).resolves.toBeUndefined();
  });

  it('distinguishes unsupported hash policy and unverified matches', () => {
    expect(subtitleErrorText('subtitle_hash_required')).toContain('indisponible');
    expect(subtitleErrorText('subtitle_already_downloaded')).toContain('déjà téléchargé');
    expect(subtitleErrorText('subtitle_attempt_exists')).toContain('supprimez une tentative');
    expect(subtitleErrorText('subtitle_source_changed')).toContain('média a changé');
    expect(subtitleMatchText('identity')).toContain('synchronisation non garantie');
    expect(subtitleMatchText('title')).toContain('à vérifier');
    expect(subtitleMatchText('unknown')).toBe('Correspondance non vérifiée.');
  });
});
