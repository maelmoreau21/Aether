import { describe, it, expect, vi, afterEach } from 'vitest';
import { NativePlayer } from './native';
import { ExternalAudio } from './external-audio';
import { SegmentedAudio } from './segmented-audio';
import { AudioSelectionRetainedError } from './errors';
import type { Media } from './types';
vi.mock('./external-audio', () => ({ ExternalAudio: class {
  async load() {} async bufferForPlayback() {} async play() {} destroy() {} setOffset() {} activate() {} assertReady() {} pause() {} isBuffering() { return false; }
} }));
class Video extends EventTarget {
  currentTime = 0; duration = 12; paused = true; volume = 0.4; playbackRate = 1.5;
  src = ''; loads = 0; auto = true; readyState = 4; seeking = false;
  buffered = {length:1,start:()=>0,end:()=>12};
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
  it('preloads the saved audio window in parallel with video metadata when the descriptor supplies duration', async()=>{
    const {video,player}=setup();video.auto=false;
    vi.stubGlobal('MediaSource',class {});
    const supported=vi.spyOn(SegmentedAudio,'supported').mockReturnValue(true);
    const objectUrl=vi.spyOn(URL,'createObjectURL').mockReturnValue('blob:test');
    const revoke=vi.spyOn(URL,'revokeObjectURL').mockImplementation(()=>{});
    let ready!:()=>void;
    const preload=vi.spyOn(SegmentedAudio.prototype,'preload').mockImplementationOnce(()=>new Promise<void>(resolve=>{ready=resolve;}));
    const position=Promise.resolve(6);
    const loading=player.load({...media,sources:[{...media.sources[0],externalAudioUrl:'/audio',audioSegmentUrl:'/chunks-',durationSeconds:12,audioSegmentMode:'absolute'}]},'fr',position);
    expect(preload).toHaveBeenCalledWith('/chunks-',position);expect(video.loads).toBe(1);
    video.dispatchEvent(new Event('loadedmetadata'));await Promise.resolve();video.dispatchEvent(new Event('seeked'));
    ready();await loading;expect(video.currentTime).toBe(6);
    player.destroy();preload.mockRestore();supported.mockRestore();objectUrl.mockRestore();revoke.mockRestore();
  });
  it('does not pause a reused video when the destroyed player times out on an unresolved play promise', async()=>{
    vi.useFakeTimers();const {video,player}=setup();await player.load(media);
    const play=vi.spyOn(video,'play').mockImplementationOnce(()=>new Promise<void>(()=>{}));
    const starting=expect(player.play()).rejects.toThrow('expiré');
    await vi.advanceTimersByTimeAsync(0);expect(play).toHaveBeenCalledOnce();
    player.destroy();await video.play();
    const pause=vi.spyOn(video,'pause');
    await vi.advanceTimersByTimeAsync(10000);await starting;
    expect(video.paused).toBe(false);expect(pause).not.toHaveBeenCalled();
    expect(vi.getTimerCount()).toBe(0);pause.mockRestore();play.mockRestore();
  });
  it('rejects a delayed video play promise after destruction without pausing a reused video element', async()=>{
    const {video,player}=setup();await player.load(media);
    let played!:()=>void;
    const play=vi.spyOn(video,'play').mockImplementationOnce(()=>new Promise<void>(resolve=>{played=resolve;}));
    const starting=expect(player.play()).rejects.toThrow('Démarrage annulé');
    await vi.waitFor(()=>expect(play).toHaveBeenCalled());
    player.destroy();expect(video.paused).toBe(true);
    await video.play(); // The next player can already be using the same element.
    played();await starting;
    expect(video.paused).toBe(false);play.mockRestore();
  });
  it('rejects a delayed external audio play promise resolved after a manual pause', async()=>{
    const {video,player}=setup();
    await player.load({...media,sources:[{...media.sources[0],externalAudioUrl:'/audio'}]});
    let played!:()=>void;
    const play=vi.spyOn(ExternalAudio.prototype,'play').mockImplementationOnce(()=>new Promise<void>(resolve=>{played=resolve;}));
    const starting=expect(player.play()).rejects.toThrow('Démarrage annulé');
    await vi.waitFor(()=>expect(play).toHaveBeenCalled());
    player.pause();played();await starting;
    expect(video.paused).toBe(true);play.mockRestore();player.destroy();
  });
  it('validates candidate failures at commit before destroying the retained audio', async () => {
    const {video,player}=setup();
    const prepared={...media,sources:media.sources.map(s=>({...s,url:'/same-video',externalAudioUrl:`/audio-${s.language}`}))};
    await player.load(prepared);await player.play();
    const destroy=vi.spyOn(ExternalAudio.prototype,'destroy');
    let failed=false;
    const buffer=vi.spyOn(ExternalAudio.prototype,'bufferForPlayback').mockImplementationOnce(async()=>{queueMicrotask(()=>{failed=true;});});
    const ready=vi.spyOn(ExternalAudio.prototype,'assertReady').mockImplementationOnce(()=>{if(failed)throw new Error('candidate window HTTP 503');});
    await expect(player.selectAudio('en')).rejects.toBeInstanceOf(AudioSelectionRetainedError);
    expect(destroy).toHaveBeenCalledTimes(1);expect(video.paused).toBe(false);
    await player.selectAudio('fr');expect(video.loads).toBe(1);
    ready.mockRestore();buffer.mockRestore();destroy.mockRestore();player.destroy();
  });
  it('keeps the previous presentation playing while a replacement audio loads, then buffers the current position', async () => {
    const {video,player}=setup();
    const prepared={...media,sources:media.sources.map(s=>({...s,url:'/same-video',externalAudioUrl:`/audio-${s.language}`}))};
    await player.load(prepared);await player.play();video.currentTime=3;
    let ready!:()=>void;
    const load=vi.spyOn(ExternalAudio.prototype,'load').mockImplementationOnce(()=>new Promise<void>(resolve=>{ready=resolve;}));
    const buffer=vi.spyOn(ExternalAudio.prototype,'bufferForPlayback').mockImplementation(async()=>{expect(video.paused).toBe(true);expect(video.currentTime).toBe(4);});
    const paused=vi.spyOn(video,'pause');
    const switching=player.selectAudio('en');
    expect(video.paused).toBe(false);expect(paused).not.toHaveBeenCalled();
    video.currentTime=4;ready();await switching;
    expect(buffer).toHaveBeenCalled();expect(video.paused).toBe(false);expect(video.loads).toBe(1);
    load.mockRestore();buffer.mockRestore();player.destroy();
  });
  it('respects a manual pause while the replacement audio is loading', async () => {
    const {video,player}=setup();
    const prepared={...media,sources:media.sources.map(s=>({...s,url:'/same-video',externalAudioUrl:`/audio-${s.language}`}))};
    await player.load(prepared);await player.play();
    let ready!:()=>void;
    const load=vi.spyOn(ExternalAudio.prototype,'load').mockImplementationOnce(()=>new Promise<void>(resolve=>{ready=resolve;}));
    const switching=player.selectAudio('en');player.pause();ready();await switching;
    expect(video.paused).toBe(true);expect(video.loads).toBe(1);
    load.mockRestore();player.destroy();
  });
  it('keeps committed replacement audio retryable when its play promise rejects', async () => {
    const {video,player}=setup();
    const prepared={...media,sources:media.sources.map(s=>({...s,url:'/same-video',externalAudioUrl:`/audio-${s.language}`}))};
    await player.load(prepared);await player.play();
    const destroy=vi.spyOn(ExternalAudio.prototype,'destroy');
    const play=vi.spyOn(ExternalAudio.prototype,'play').mockRejectedValueOnce(new Error('play rejected'));
    await expect(player.selectAudio('en')).rejects.toThrow('play rejected');
    expect(destroy).toHaveBeenCalledTimes(1); // Only the old, replaced track.
    expect(video.paused).toBe(true);
    await player.play();expect(video.paused).toBe(false);expect(video.loads).toBe(1);
    play.mockRestore();destroy.mockRestore();player.destroy();
  });
  it('uses decoded original Matroska video without preparing the whole file', async () => {
    const {video,player}=setup();const fetcher=vi.fn();vi.stubGlobal('fetch',fetcher);
    await player.load({...media,sources:[{...media.sources[0],url:'/original.mkv',prepare:true,prepareVideo:false,preparedVideoUrl:'/prepared/video'}]});
    expect(video.src).toBe('/original.mkv');expect(fetcher).not.toHaveBeenCalled();player.destroy();
  });
  it('requires actual original video data and falls back to packet-copy preparation after decoding fails', async () => {
    const {video,player,error}=setup();video.auto=false;video.readyState=1;
    const fetcher=vi.fn().mockResolvedValue({ok:true});vi.stubGlobal('fetch',fetcher);
    const loading=player.load({...media,sources:[{...media.sources[0],url:'/original.mkv',prepare:true,prepareVideo:false,preparedVideoUrl:'/prepared/video'}]});
    video.dispatchEvent(new Event('loadedmetadata'));await Promise.resolve();await Promise.resolve();
    expect(fetcher).not.toHaveBeenCalled();
    video.dispatchEvent(new Event('error'));
    await vi.waitFor(()=>expect(video.src).toBe('/prepared/video'));
    video.dispatchEvent(new Event('loadedmetadata'));await loading;
    expect(fetcher).toHaveBeenCalledWith('/prepared/video',expect.objectContaining({method:'HEAD'}));
    expect(error).not.toHaveBeenCalled();player.destroy();
  });
  it('cancels a startup reserve on pause without starting playback after late data', async () => {
    const {video,player}=setup();await player.load(media);
    video.buffered={length:1,start:()=>0,end:()=>.01};
    const starting=expect(player.play()).rejects.toThrow('annulé');
    player.pause();await starting;
    video.buffered={length:1,start:()=>0,end:()=>12};video.dispatchEvent(new Event('progress'));
    await Promise.resolve();expect(video.paused).toBe(true);player.destroy();
  });
  it('resolves saved progress alongside metadata and seeks before loading independent audio', async () => {
    const {video,player}=setup();video.auto=false;
    let resume!:(position:number)=>void;
    const position=new Promise<number>(resolve=>{resume=resolve;});
    const loading=player.load(media,'fr',position);
    expect(video.loads).toBe(1);
    video.dispatchEvent(new Event('loadedmetadata'));await Promise.resolve();
    expect(video.currentTime).toBe(0);
    resume(6);await Promise.resolve();expect(video.currentTime).toBe(6);
    video.dispatchEvent(new Event('seeked'));await loading;
    expect(video.loads).toBe(1);player.destroy();
  });
  it('does not apply late saved progress after a closed player', async () => {
    const {video,player}=setup();video.auto=false;
    let resume!:(position:number)=>void;
    const loading=expect(player.load(media,'fr',new Promise<number>(resolve=>{resume=resolve;}))).rejects.toThrow('Lecteur');
    video.dispatchEvent(new Event('loadedmetadata'));player.destroy();await loading;
    resume(6);await Promise.resolve();expect(video.currentTime).toBe(0);
  });
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
    video.currentTime=0; // A new browser resource resets its timeline before metadata.
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
