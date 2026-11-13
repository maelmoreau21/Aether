import { describe, expect, it, vi } from 'vitest';
import {
  PlaybackFailure, classifyHttp, classifyMediaError, failoverHelps, failureKindOf, probeFailure, refineFailure, withContext
} from './diagnostics';

const response = (status: number, headers: Record<string, string> = {}, body?: unknown) => ({
  ok: status >= 200 && status < 300,
  status,
  headers: { get: (name: string) => headers[name.toLowerCase()] ?? null },
  body: { cancel: vi.fn(async () => {}) },
  clone() { return { json: async () => { if (body === undefined) throw new Error('no body'); return body; } }; }
});

describe('HTTP classification', () => {
  it('separates Node loss, permission, preparation and container refusals', () => {
    expect(classifyHttp(503, 'node_unavailable').kind).toBe('node');
    expect(classifyHttp(403, 'node_revoked').kind).toBe('node');
    expect(classifyHttp(504, 'stream_timeout').kind).toBe('node');
    expect(classifyHttp(403).kind).toBe('permission');
    expect(classifyHttp(401).kind).toBe('permission');
    expect(classifyHttp(503, 'preparation_busy').kind).toBe('preparation');
    expect(classifyHttp(503, 'preparation_cache_limit_reached').kind).toBe('preparation');
    expect(classifyHttp(409, 'source_changed_rescan_required').kind).toBe('preparation');
    expect(classifyHttp(503, 'unsupported_preparation').kind).toBe('container');
    expect(classifyHttp(503, 'source_permission_denied').kind).toBe('storage');
    expect(classifyHttp(503, 'source_io_error').kind).toBe('storage');
    expect(classifyHttp(502).kind).toBe('network');
    expect(classifyHttp(418).kind).toBe('unknown');
  });
  it('keeps status and code for support without exposing them as a different kind', () => {
    const failure = classifyHttp(503, 'preparation_busy');
    expect(failure.detail).toEqual({ status: 503, code: 'preparation_busy' });
    expect(failure.message).toContain('HTTP 503');
    expect(failure.message).toContain('preparation_busy');
  });
});

describe('MediaError classification', () => {
  it('maps the decoder codes', () => {
    expect(classifyMediaError(2).kind).toBe('network');
    expect(classifyMediaError(3).kind).toBe('codec');
    expect(classifyMediaError(1).kind).toBe('unknown');
    expect(classifyMediaError(undefined).kind).toBe('unknown');
  });
  it('tells a refused container from a refused stream inside a known container', () => {
    const mp4Only = (mime: string) => (mime === 'video/mp4' ? 'maybe' : '');
    expect(classifyMediaError(4, { mime: 'video/mp4; codecs="av01"', canPlayType: mp4Only }).kind).toBe('codec');
    expect(classifyMediaError(4, { mime: 'video/x-matroska', canPlayType: mp4Only }).kind).toBe('container');
    expect(classifyMediaError(4, {}).kind).toBe('codec');
  });
});

describe('failover policy', () => {
  it('does not detour through a copy that would fail identically', () => {
    expect(failoverHelps('codec')).toBe(false);
    expect(failoverHelps('container')).toBe(false);
    expect(failoverHelps('permission')).toBe(false);
  });
  it('still fails over for Node, network, preparation and undetermined causes', () => {
    for (const kind of ['node', 'network', 'preparation', 'storage', 'unknown'] as const) expect(failoverHelps(kind)).toBe(true);
    expect(failoverHelps(undefined)).toBe(true);
  });
});

describe('URL probe', () => {
  it('reports nothing when the URL answers, and releases the body', async () => {
    const ok = response(206);
    const fetcher = vi.fn().mockResolvedValue(ok);
    await expect(probeFailure('/media', { fetcher })).resolves.toBeUndefined();
    expect(fetcher).toHaveBeenCalledWith('/media', expect.objectContaining({ method: 'GET', headers: { Range: 'bytes=0-0' } }));
    expect(ok.body.cancel).toHaveBeenCalled();
  });
  it('reads the Node code from the header or the JSON body', async () => {
    const header = await probeFailure('/m', { fetcher: vi.fn().mockResolvedValue(response(503, { 'x-media-preparation-error': 'preparation_busy' })) });
    expect(header?.kind).toBe('preparation');
    const body = await probeFailure('/m', { fetcher: vi.fn().mockResolvedValue(response(503, {}, { error: 'node_unavailable' })) });
    expect(body?.kind).toBe('node');
    expect((await probeFailure('/m', { fetcher: vi.fn().mockResolvedValue(response(403)) }))?.kind).toBe('permission');
  });
  it('reports a network failure when no response is obtained', async () => {
    const failure = await probeFailure('/m', { fetcher: vi.fn().mockRejectedValue(new TypeError('Failed to fetch')) });
    expect(failure?.kind).toBe('network');
  });
  it('skips probing revoked or MSE blob URLs so codec verdicts are preserved', async () => {
    const fetcher = vi.fn();
    await expect(probeFailure('blob:https://aether.local/1234', { fetcher })).resolves.toBeUndefined();
    expect(fetcher).not.toHaveBeenCalled();
  });
  it('is bounded and silent when the caller cancels', async () => {
    vi.useFakeTimers();
    const hang = vi.fn((_url: string, init: RequestInit) => new Promise((_, reject) => init.signal?.addEventListener('abort', () => reject(new DOMException('aborted', 'AbortError')))));
    const pending = probeFailure('/m', { fetcher: hang as never, timeoutMs: 3000 });
    await vi.advanceTimersByTimeAsync(3000);
    const timedOut = await pending;
    expect(timedOut?.kind).toBe('network');
    expect(timedOut?.message).toContain('timeout');
    const controller = new AbortController();
    const cancelled = probeFailure('/m', { fetcher: hang as never, signal: controller.signal });
    controller.abort();
    await expect(cancelled).resolves.toBeUndefined();
    vi.useRealTimers();
  });
});

describe('refinement', () => {
  it('replaces a decoding hypothesis with a Node loss that the probe establishes', async () => {
    const guess = classifyMediaError(4, { mime: 'video/mp4', canPlayType: () => 'maybe' });
    const refined = await refineFailure(guess, '/m', { fetcher: vi.fn().mockResolvedValue(response(503, {}, { error: 'node_unavailable' })) });
    expect(failureKindOf(refined)).toBe('node');
  });
  it('keeps the decoding diagnosis when the bytes are reachable', async () => {
    const guess = classifyMediaError(3);
    const refined = await refineFailure(guess, '/m', { fetcher: vi.fn().mockResolvedValue(response(206)) });
    expect(refined).toBe(guess);
  });
  it('does not weaken a precise verdict and skips the probe', async () => {
    const precise = classifyHttp(503, 'preparation_busy');
    const fetcher = vi.fn();
    expect(await refineFailure(precise, '/m', { fetcher })).toBe(precise);
    expect(fetcher).not.toHaveBeenCalled();
  });
  it('keeps a plain timeout Error when the URL answers', async () => {
    const timeout = new Error('Délai expiré');
    expect(await refineFailure(timeout, '/m', { fetcher: vi.fn().mockResolvedValue(response(200)) })).toBe(timeout);
  });
});

describe('context labels', () => {
  it('keeps the kind when a track label is added', () => {
    const labelled = withContext(classifyHttp(403), message => `FR / ${message}`);
    expect(labelled).toBeInstanceOf(PlaybackFailure);
    expect(failureKindOf(labelled)).toBe('permission');
    expect(labelled.message.startsWith('FR / ')).toBe(true);
    expect(failureKindOf(withContext(new Error('x'), message => message))).toBeUndefined();
  });
});
