import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { session } from '../auth/session';
import { PlaybackCollector } from './collector';

class MediaFixture extends EventTarget {
  currentTime = 0; duration = 100; paused = false; playbackRate = 1; ended = false;
  error: { code: number } | null = null;
}
let clock = 0;
let collectors: PlaybackCollector[] = [];
const batches: { path: string; body: { events: Record<string, unknown>[] } }[] = [];
let enabled = true;
function create(media = new MediaFixture(), music = false) {
  const collector = new PlaybackCollector(media as unknown as HTMLMediaElement, { mediaId: music ? 'song' : 'asset', music,
    info: () => ({ method: 'direct', audio: { id: 'audio-fr', language: 'fr', codec: 'opus' } }) });
  collectors.push(collector); return { collector, media };
}
async function settle() { await vi.advanceTimersByTimeAsync(0); }
beforeEach(() => {
  clock = 0; enabled = true; batches.length = 0;
  vi.useFakeTimers({ toFake: ['setInterval', 'clearInterval', 'setTimeout', 'clearTimeout'] });
  vi.spyOn(performance, 'now').mockImplementation(() => clock);
  vi.stubGlobal('window', new EventTarget());
  const doc = Object.assign(new EventTarget(), { visibilityState: 'visible' });
  vi.stubGlobal('document', doc);
  session.set({ user: { id: 'reader', login: 'reader', role: 'user' }, csrfToken: 'fixture-csrf' });
  vi.stubGlobal('fetch', vi.fn(async (path: string, init?: RequestInit) => {
    if (path.endsWith('/statistics')) return Response.json({ enabled });
    if (path.endsWith('/events')) { batches.push({ path, body: JSON.parse(String(init?.body)) }); return Response.json({ acceptedThrough: 100 }); }
    return Response.json({ id: 'session' });
  }));
});
afterEach(async () => {
  collectors.forEach(c => c.stop()); collectors = []; await settle();
  session.set(null); vi.restoreAllMocks(); vi.unstubAllGlobals(); vi.useRealTimers();
});
describe('optional playback collection', () => {
  it('does not create sessions or send events when collection is disabled', async () => {
    enabled = false; const { media, collector } = create(); await settle();
    media.dispatchEvent(new Event('playing')); collector.seek(80, 2); collector.stop(); await settle();
    expect(vi.mocked(fetch).mock.calls.map(c => c[0])).toEqual(['/api/v1/playback/statistics']);
    expect(batches).toHaveLength(0);
  });
  it('sends consecutive deduplicable samples and successful track changes without raw errors or URLs', async () => {
    const { collector, media } = create(); await settle();
    media.dispatchEvent(new Event('playing')); await settle();
    clock = 10000; media.currentTime = 10;
    collector.track('audio', { id: 'audio-en', language: 'en', codec: 'opus' }, 'manual'); await settle();
    collector.track('audio', { id: 'audio-en', language: 'en', codec: 'opus' }, 'manual');
    collector.seek(80, 10, 'skip_intro'); media.currentTime = 80; await settle();
    collector.stop(); await settle();
    const events = batches.flatMap(b => b.body.events);
    expect(events.map(e => e.sequence)).toEqual(events.map((_, i) => i + 1));
    const changes = events.filter(e => e.kind === 'audio_change'); expect(changes).toHaveLength(2);
    expect(changes[1]).toMatchObject({ origin: 'manual', previousTrack: { id: 'audio-fr' }, track: { id: 'audio-en' } });
    expect(events.find(e => e.kind === 'skip_intro')).toMatchObject({ position: 80, fromPosition: 10 });
    expect(events.at(-1)).toMatchObject({ kind: 'stop', playing: false });
    expect(JSON.stringify(events)).not.toContain('fixture-csrf');
  });
  it('abandons pending data on account changes, including an in-flight creation', async () => {
    let release: (response: Response) => void = () => {};
    vi.stubGlobal('fetch', vi.fn((path: string) => path.endsWith('/statistics') ? Promise.resolve(Response.json({ enabled: true })) : new Promise<Response>(resolve => release = resolve)));
    const { media } = create(); await settle();
    session.set({ user: { id: 'other', login: 'other', role: 'user' }, csrfToken: 'other-csrf' });
    release(Response.json({ id: 'late' })); await settle(); media.dispatchEvent(new Event('playing'));
    await vi.advanceTimersByTimeAsync(60000);
    expect(vi.mocked(fetch).mock.calls).toHaveLength(2);
  });
  it('keeps playback usable on network failures and bounds retries', async () => {
    vi.stubGlobal('fetch', vi.fn().mockRejectedValue(new Error('offline')));
    const { collector, media } = create(); await settle();
    for (let i = 0; i < 300; i++) collector.event('heartbeat');
    media.currentTime = 20; collector.seek(20, 0); collector.stop(); await settle();
    expect(media.currentTime).toBe(20); expect(fetch).toHaveBeenCalledTimes(1);
  });
  it('closes a music replay and gives the next play a separate session', async () => {
    const { collector, media } = create(new MediaFixture(), true); await settle();
    media.dispatchEvent(new Event('playing')); await settle();
    clock = 10000; media.currentTime = 10; collector.stop(true); await settle();
    create(new MediaFixture(), true); await settle();
    const starts = vi.mocked(fetch).mock.calls.filter(c => String(c[0]).endsWith('/sessions')).map(c => JSON.parse(String(c[1]?.body)));
    expect(starts).toHaveLength(2); expect(starts[0].id).not.toBe(starts[1].id); expect(starts.every(s => s.music)).toBe(true);
    expect(batches.flatMap(b => b.body.events).some(e => e.kind === 'ended')).toBe(true);
  });
});
