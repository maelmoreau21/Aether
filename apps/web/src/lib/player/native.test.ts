import { describe, it, expect, vi, afterEach } from 'vitest';
import { NativePlayer } from './native';
import { ExternalAudio } from './external-audio';
import { AudioSelectionRetainedError } from './errors';
import type { Media } from './types';
vi.mock('./external-audio', () => ({ ExternalAudio: class {
  async load() {} async play() {} destroy() {} setOffset() {} activate() {} pause() {} isBuffering() { return false; }
} }));
class Video extends EventTarget {
  currentTime = 0; duration = 12; paused = true; volume = 0.4; playbackRate = 1.5;
  src = ''; loads = 0; auto = true;
  pause() { this.paused = true; }
  play() { this.paused = false; return Promise.resolve(); }
  querySelectorAll() { return []; }
  removeAttribute() { this.src = ''; }
  load() {
    this.loads++;
    if (this.auto) queueMicrotask(() => { this.dispatchEvent(new Event('loadedmetadata')); queueMicrotask(() => this.dispatchEvent(new Event('seeked'))); });
  }
}
const media: Media = { id: 'demo', title: 'Test', subtitles: [], sources: ['fr', 'en'].map(language => ({ id: language, audioTrackId: language, language, mime: 'video/mp4', url: `/${language}` })) };
function setup() { const video = new Video(); const error = vi.fn(); return { video, error, player: new NativePlayer(video as unknown as HTMLVideoElement, error) }; }
afterEach(() => { vi.useRealTimers(); vi.unstubAllGlobals(); });
describe('native source replacement', () => {
  it('overlaps independent audio and video loading without activating audio early', async () => {
    const {video,player}=setup();video.auto=false;
    let ready!:()=>void;
    const load=vi.spyOn(ExternalAudio.prototype,'load').mockImplementationOnce(()=>new Promise<void>(resolve=>{ready=resolve;}));
    const activate=vi.spyOn(ExternalAudio.prototype,'activate');
    const loading=player.load({...media,sources:[{...media.sources[0],externalAudioUrl:'/audio'}]});
    expect(load).toHaveBeenCalledWith('/audio',false);
    expect(video.loads).toBe(1);expect(activate).not.toHaveBeenCalled();
    video.dispatchEvent(new Event('loadedmetadata'));
    await Promise.resolve();expect(activate).not.toHaveBeenCalled();
    ready();await loading;expect(activate).toHaveBeenCalledTimes(1);
    player.destroy();load.mockRestore();activate.mockRestore();
  });
  it('rejects declared unsupported DTS-HD MA before HTTP preparation or video loading',async()=>{
    const {video,player}=setup();const fetcher=vi.fn();vi.stubGlobal('fetch',fetcher);
    Object.assign(video,{canPlayType:()=>''});
    const source={...media.sources[0],language:'dut',audioTrackId:'internal-default',prepare:true,mime:'video/x-matroska',tracks:[{index:1,codec_type:'audio',codec_name:'dts',profile:'DTS-HD MA',channels:6}]};
    await expect(player.load({...media,sources:[source]})).rejects.toThrow('DTS-HD MA');
    expect(fetcher).not.toHaveBeenCalled();expect(video.loads).toBe(0);player.destroy();
  });
  it('resumes the retained audio after a rejected candidate without reloading video', async () => {
    const {video,player}=setup();
    const prepared={...media,sources:media.sources.map(s=>({...s,url:'/same-video',externalAudioUrl:`/audio-${s.language}`}))};
    await player.load(prepared); await player.play(); video.currentTime=6;
    const destroy=vi.spyOn(ExternalAudio.prototype,'destroy');
    const load=vi.spyOn(ExternalAudio.prototype,'load').mockRejectedValueOnce(new Error('HTTP 404'));
    await expect(player.selectAudio('en')).rejects.toBeInstanceOf(AudioSelectionRetainedError);
    expect(video.paused).toBe(false);expect(video.currentTime).toBe(6);expect(video.loads).toBe(1);
    expect(destroy).toHaveBeenCalledTimes(1);
    await player.selectAudio('fr');expect(load).toHaveBeenCalledTimes(1);
    load.mockRestore();destroy.mockRestore();player.destroy();
  });
  it('distinguishes metadata timeout from seek timeout without inventing a codec diagnosis', async () => {
    vi.useFakeTimers();const {video,player}=setup();video.auto=false;
    const metadata=expect(player.load(media)).rejects.toThrow('métadonnées');
    await vi.advanceTimersByTimeAsync(10000);await metadata;
    video.auto=true;await player.load(media);video.auto=false;video.currentTime=5;
    const seek=expect(player.selectAudio('en')).rejects.toThrow('de seek');
    video.dispatchEvent(new Event('loadedmetadata'));
    await vi.advanceTimersByTimeAsync(10000);await seek;player.destroy();
  });
  it('reads indexed MP4 video directly and prepares only audio on the legacy fallback',async()=>{
    const fetcher=vi.fn().mockResolvedValue({ok:true});vi.stubGlobal('fetch',fetcher);
    const {video,player}=setup();
    const direct={...media,sources:media.sources.map(s=>({...s,url:'/original.mp4',prepare:true,prepareVideo:false,externalAudioUrl:`/audio-${s.language}`}))};
    await player.load(direct);await player.selectAudio('en');await player.selectAudio('fr');
    expect(video.loads).toBe(1);expect(video.src).toBe('/original.mp4');
    expect(fetcher.mock.calls.every(call=>call[0]!=='/original.mp4')).toBe(true);
    player.destroy();
  });
  it('loads the preferred track directly without loading French first', async () => {
    const { video, player } = setup();
    await player.load(media, 'en');
    await player.selectAudio('en');
    expect(video.src).toBe('/en'); expect(video.loads).toBe(1);
    player.destroy();
  });
  it('switches prepared Opus while keeping the loaded video, position and paused state', async () => {
    const fetcher = vi.fn().mockResolvedValue({ok:true}); vi.stubGlobal('fetch', fetcher);
    const {video,player}=setup();
    const prepared = {...media, sources:media.sources.map(source=>({...source,url:'/prepared/video',externalAudioUrl:`/prepared/${source.language}`,prepare:true}))};
    await player.load(prepared); video.currentTime=8.5;
    await player.selectAudio('en'); expect(video.loads).toBe(1); expect(video.currentTime).toBe(8.5); expect(video.paused).toBe(true);
    await player.play(); await player.selectAudio('fr'); expect(video.paused).toBe(false); expect(video.loads).toBe(1);
    expect(fetcher.mock.calls.filter(call=>call[0]==='/prepared/video')).toHaveLength(1);
    player.destroy();
  });
  it('fails preparation explicitly without replacing the loaded video', async () => {
    const {video,player}=setup(); await player.load(media);
    vi.stubGlobal('fetch', vi.fn().mockResolvedValue({ok:false}));
    await expect(player.load({...media,sources:[{...media.sources[0],prepare:true,url:'/prepared/video'}]})).rejects.toThrow('Préparation impossible');
    expect(video.loads).toBe(1); player.destroy();
  });
  it('preserves paused position, speed and volume, and never silently substitutes a missing language', async () => {
    const { video, player } = setup();
    await player.load(media); video.currentTime = 4.25;
    await player.selectAudio('en');
    expect(video.src).toBe('/en'); expect(video.paused).toBe(true); expect(video.currentTime).toBe(4.25);
    expect(video.volume).toBe(0.4); expect(video.playbackRate).toBe(1.5);
    await expect(player.selectAudio('de')).rejects.toThrow('absente'); expect(video.src).toBe('/en'); expect(video.loads).toBe(2);
  });
  it('opens an indexed source with unknown language without claiming French audio', async () => {
    const { video, player } = setup();
    await player.load({ ...media, sources: [{ ...media.sources[0], language: 'und', url: '/indexed' }] });
    expect(video.src).toBe('/indexed');
  });
  it('resumes only an active presentation and refuses concurrent replacements', async () => {
    const { video, player } = setup(); await player.load(media); await player.play(); video.currentTime = 5;
    const change = player.selectAudio('en');
    await expect(player.selectAudio('fr')).rejects.toThrow('indisponible'); await change;
    expect(video.paused).toBe(false); expect(video.currentTime).toBe(5);
  });
  it('terminates a stalled load after 10s without retry or fallback', async () => {
    vi.useFakeTimers(); const { video, player } = setup(); video.auto = false;
    const result = expect(player.load(media)).rejects.toThrow('expiré');
    await vi.advanceTimersByTimeAsync(10000); await result;
    expect(video.loads).toBe(1); expect(video.paused).toBe(true);
    await vi.advanceTimersByTimeAsync(60000); expect(video.loads).toBe(1);
  });
  it('turns a playback stall into one terminal error without reloading', async () => {
    vi.useFakeTimers(); const { video, player, error } = setup();
    await player.load(media); await player.play(); video.dispatchEvent(new Event('waiting'));
    await vi.advanceTimersByTimeAsync(10000);
    expect(video.paused).toBe(true); expect(error).toHaveBeenCalledTimes(1); expect(video.loads).toBe(1);
    await vi.advanceTimersByTimeAsync(60000); expect(error).toHaveBeenCalledTimes(1);
  });
  it('cancels pending work on teardown', async () => {
    const { video, player } = setup(); video.auto = false;
    const result = expect(player.load(media)).rejects.toThrow('fermé'); player.destroy(); await result;
  });
  it('falls back from an unreachable direct Node after 2.5s without a premature terminal error', async()=>{
    vi.useFakeTimers();const {video,player,error}=setup();video.auto=false;
    const direct={...media,sources:[{...media.sources[0],directUrl:'https://node.invalid/content',relayUrl:'/relay'}]};
    const loading=player.load(direct);
    await vi.advanceTimersByTimeAsync(2499);expect(video.src).toBe('https://node.invalid/content');
    await vi.advanceTimersByTimeAsync(1);expect(video.src).toBe('/relay');
    video.dispatchEvent(new Event('loadedmetadata'));await loading;
    expect(error).not.toHaveBeenCalled();expect(video.loads).toBe(2);
  });
  it('serializes preparation to respect the single Node worker',async()=>{
    const replies:((value:Response)=>void)[]=[];
    const fetcher=vi.fn(()=>new Promise<Response>(resolve=>replies.push(resolve)));vi.stubGlobal('fetch',fetcher);
    const {player}=setup();const loading=player.load({...media,sources:[{...media.sources[0],prepare:true,url:'/prepared/video',externalAudioUrl:'/prepared/audio'}]});
    expect(fetcher).toHaveBeenCalledTimes(1);
    replies[0](new Response(null,{status:200}));
    await vi.waitFor(()=>expect(fetcher).toHaveBeenCalledTimes(2));
    replies[1](new Response(null,{status:200})); await loading;
    player.destroy();
  });

});
