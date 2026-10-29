// Production NativePlayer + genuine browser click, rVFC and decoded PCM.
// Synthetic HTTP harness only: excludes Svelte, Core authorization and NAS I/O.
const fs = require('node:fs');
const path = require('node:path');
const os = require('node:os');
const http = require('node:http');
const { execFileSync, execFile } = require('node:child_process');
const { promisify } = require('node:util');
const { createHash } = require('node:crypto');
const { chromium } = require('../apps/web/node_modules/playwright');
const ts = require('../apps/web/node_modules/typescript');
const run = promisify(execFile);
const root = path.resolve(__dirname, '..');
const out = process.env.MEDIA_REPORT_DIR || 'target/streaming-performance';
const fixture = path.resolve(process.env.MEDIA_TEST_FIXTURE || 'tests/fixtures/generated/representative/representative.mp4');
const rounds = Number(process.env.MEDIA_PERFORMANCE_ROUNDS || 5);
const baseline = process.env.MEDIA_PERFORMANCE_BASELINE === '1';
const baselineRef = process.env.MEDIA_PERFORMANCE_BASE_REF || 'HEAD';
const baselineCommit = execFileSync('git', ['rev-parse', '--verify', `${baselineRef}^{commit}`], {cwd: root, encoding: 'utf8'}).trim();
const audioDelayMs = Number(process.env.MEDIA_PERFORMANCE_AUDIO_DELAY_MS || 0);
const englishHz = Number(process.env.MEDIA_PERFORMANCE_ENGLISH_HZ || 1050);
if (!Number.isFinite(englishHz) || englishHz < 100 || englishHz > 10000) throw new Error('English audio frequency must be 100..10000 Hz');
if (!Number.isFinite(audioDelayMs) || audioDelayMs < 0 || audioDelayMs > 2000) throw new Error('audio delay must be 0..2000 ms');
if (!Number.isInteger(rounds) || rounds < 1 || rounds > 30) throw new Error('rounds must be 1..30');
const container = path.extname(fixture) === '.mkv' ? 'x-matroska' : 'mp4';
const probe = JSON.parse(execFileSync('ffprobe', ['-v', 'error', '-show_streams', '-show_format', '-of', 'json', fixture]));
const temp = fs.mkdtempSync(path.join(os.tmpdir(), 'media-performance-'));
const preparations = [], requests = [], jobs = new Map(), files = new Map([['/original', fixture]]);
const report = {
  schemaVersion: 1, date: new Date().toISOString(), baselineNativeFromHead: baseline, os: `${os.platform()} ${os.release()}`,
  scope: 'Production NativePlayer in synthetic click harness; localhost Range server; no Svelte/Core/Node/NAS proof',
  cacheContract: 'First trial: fresh browser context + empty preparation cache. OS disk cache is uncontrolled and not called cold. Warm trials reuse context and explicitly listed prepared chunks.',
  audioOracle: 'WebAudio FFT decoded PCM, 8 ms polling; not physical speaker onset or proof of discrete surround output',
  switchEnglishFrequencyHz: englishHz,
  originalAudioSelection: 'Browser default multi-audio track; language cannot be asserted from metadata. Measured PCM signal is reported.',
  fixture: { path: path.relative(root, fixture), bytes: fs.statSync(fixture).size, probe },
  baselineCommit, playerModuleSha256: {},
  simulatedAudioResponseDelayMs: audioDelayMs, strategies: [], preparations, requests,
};
const server = http.createServer(async (req, res) => {
  const name = new URL(req.url, 'http://localhost').pathname;
  if (name === '/') {
    res.writeHead(200, { 'Content-Type': 'text/html' });
    res.end('<button id="start">Lecture</button><button id="switch">VF/VO</button><button id="seek">Seek</button><video playsinline preload="auto"></video>'); return;
  }
  const started = performance.now();
  const chunk = /^\/audio-(\d+)-chunk-(\d+)$/.exec(name);
  if (chunk && audioDelayMs) await new Promise(resolve => setTimeout(resolve, audioDelayMs));
  if (chunk && !files.has(name)) {
    if (!jobs.has(name)) {
      const file = path.join(temp, name.slice(1));
      const index = Number(chunk[1]), offset = Math.max(0, Number(chunk[2]) * 10 - 1);
      jobs.set(name, run('ffmpeg', ['-v', 'error', '-nostdin', '-ss', String(offset), '-i', fixture, '-ss', '0', '-map', `0:${index}`, '-vn', '-sn', '-c:a', 'copy', '-t', '12', '-avoid_negative_ts', 'make_zero', '-movflags', 'frag_keyframe+empty_moov+default_base_moof', '-frag_duration', '1000000', '-f', 'mp4', file]).then(() => {
        files.set(name, file); preparations.push({ path: name, elapsedMs: performance.now() - started, bytes: fs.statSync(file).size });
      }));
    }
    try { await jobs.get(name); } catch { res.writeHead(503); res.end(); return; }
  }
  const file = files.get(name);
  if (!file) { res.writeHead(404); res.end(); return; }
  const size = fs.statSync(file).size, range = /^bytes=(\d+)-(\d*)$/.exec(req.headers.range || '');
  const start = range ? Number(range[1]) : 0, end = range?.[2] ? Math.min(size - 1, Number(range[2])) : size - 1;
  if (start >= size || end < start) { res.writeHead(416, { 'Content-Range': `bytes */${size}` }); res.end(); return; }
  const status = range ? 206 : 200;
  const record = { path: name, method: req.method, status, start, end, headerMs: performance.now() - started, bytesSent: 0 };
  requests.push(record);
  res.writeHead(status, { 'Content-Type': chunk ? 'audio/mp4' : `video/${container}`, 'Content-Length': end - start + 1, 'Accept-Ranges': 'bytes', ...(range ? { 'Content-Range': `bytes ${start}-${end}/${size}` } : {}) });
  if (req.method === 'HEAD') { res.end(); return; }
  const stream = fs.createReadStream(file, { start, end, highWaterMark: 65536 });
  stream.on('data', data => { record.bytesSent += data.length; });
  res.on('close', () => { record.cancelled = !res.writableEnded; stream.destroy(); });
  stream.pipe(res);
});
function compile(name) {
  const source = baseline && name === 'native' ? execFileSync('git', ['show', `${baselineCommit}:apps/web/src/lib/player/native.ts`], {cwd: root, encoding: 'utf8'}) : fs.readFileSync(path.join(root, 'apps/web/src/lib/player', `${name}.ts`), 'utf8');
  const hash = createHash('sha256').update(source).digest('hex');
  if (report.playerModuleSha256[name] && report.playerModuleSha256[name] !== hash) throw new Error(`Player module changed during benchmark: ${name}`);
  report.playerModuleSha256[name] = hash;
  return ts.transpileModule(source, { compilerOptions: { target: ts.ScriptTarget.ES2022, module: ts.ModuleKind.CommonJS } }).outputText;
}
async function instrument(page, strategy) {
  for (const name of ['startup-buffer', 'errors', 'compatibility', 'source-selection', 'external-audio', 'segmented-audio', 'native']) {
    await page.addScriptTag({ content: `window[${JSON.stringify(name)}]={};(function(exports,require){${compile(name)}\n})(window[${JSON.stringify(name)}],name=>window[name.slice(2)]);` });
  }
  await page.evaluate(({ probe, container, strategy, englishHz }) => {
    const video = document.querySelector('video'), ctx = new AudioContext(), attached = new WeakMap();
    window.videoLoadCount = 0; video.addEventListener('loadstart',()=>window.videoLoadCount++);
    for (const name of ['pause','playing','waiting','seeking','seeked']) video.addEventListener(name,()=>{if(active){active.videoEvents??=[];active.videoEvents.push({name,atMs:performance.now()-active.start,time:video.currentTime});}});
    let player, active, nextTrack = 'internal-2';
    window.trials = []; window.errors = [];
    const audioTracks = probe.streams.filter(t => t.codec_type === 'audio');
    const media = { id: 'performance', title: 'Synthetic', subtitles: [], sources: strategy === 'original' ? [{ id: 'original', audioTrackId: 'internal-default', language: 'und', url: '/original', mime: `video/${container}`, tracks: probe.streams }] : audioTracks.map(t => ({ id: `track-${t.index}`, audioTrackId: `internal-${t.index}`, language: t.tags?.language || 'und', url: '/original', mime: `video/${container}`, externalAudioUrl: `/unused-audio-${t.index}`, prepare: true, prepareVideo: false, audioSegmentUrl: `/audio-${t.index}-chunk-`, tracks: [t] })) };
    function analyser(element) {
      if (!attached.has(element)) { const source = ctx.createMediaElementSource(element), node = ctx.createAnalyser(); node.fftSize = 2048; node.smoothingTimeConstant = 0; source.connect(node); node.connect(ctx.destination); attached.set(element, {source,node}); }
      return attached.get(element).node;
    }
    const frequencies = new Float32Array(1024);
    function signal(element) {
      const node = analyser(element); node.getFloatFrequencyData(frequencies);
      let peak = 1; for (let i = 2; i < frequencies.length; i++) if (frequencies[i] > frequencies[peak]) peak = i;
      return { hz: peak * ctx.sampleRate / node.fftSize, db: frequencies[peak] };
    }
    new MutationObserver(records => {
      for(const record of records)for(const removed of record.removedNodes){
        if(!(removed instanceof Element))continue;
        for(const a of [ ...(removed.matches('audio')?[removed]:[]), ...removed.querySelectorAll('audio')]){const graph=attached.get(a);if(graph){graph.source.disconnect();graph.node.disconnect();attached.delete(a);}}
      }
      for (const a of document.querySelectorAll('audio')) analyser(a);
    }).observe(document.body, { childList: true, subtree: true });
    // PCM output is observed only in this test harness, never production code.
    analyser(video);
    let frameId, previousFrameAt;
    function frame() {
      frameId = video.requestVideoFrameCallback((at, meta) => {
        if (active && active.kind === 'switch' && previousFrameAt) active.maxVideoFrameGapMs = Math.max(active.maxVideoFrameGapMs || 0, at - Math.max(previousFrameAt,active.start));
        previousFrameAt = at;
        if (active && active.firstFrameMs === undefined && (active.kind !== 'seek' || Math.abs(meta.mediaTime - active.target) < .3)) { active.firstFrameMs = performance.now() - active.start; active.frameMediaTime = meta.mediaTime; }
        frame();
      });
    }
    frame();
    let previousPoll = performance.now();
    const poll = setInterval(() => {
      const now = performance.now(), delta = now - previousPoll; previousPoll = now;
      if (!active) return;
      const audios = [...document.querySelectorAll('audio[data-external-audio]')];
      // A pending candidate is inserted before the retained playing audio.
      // Observe audible presentation across ALL elements, not DOM order.
      const element = audios.find(a=>!a.paused&&!a.muted) || audios[0] || video;
      const sample = signal(element);
      // Preserve bounded timeline evidence even before the strict PCM oracle
      // accepts a sample. In particular, distinguish a stalled media clock
      // or suspended AudioContext from silence observed on a running clock.
      const elapsedMs = now - active.start;
      if (sample.db > -80 && active.firstNonSilentMs === undefined) active.firstNonSilentMs = elapsedMs;
      if ((active.clockSamples?.length ?? 0) < 45 && elapsedMs >= (active.nextClockSampleMs ?? 0)) {
        (active.clockSamples ??= []).push({ elapsedMs, mediaTime: video.currentTime, audioTime: element.currentTime,
          contextTime: ctx.currentTime, contextState: ctx.state, paused: element.paused, muted: element.muted,
          readyState: element.readyState, seeking: element.seeking, sample });
        active.nextClockSampleMs = elapsedMs + 500;
      }
      if (active.kind === 'switch') { active.pcmPolls = (active.pcmPolls || 0) + 1; active.maxPollGapMs = Math.max(active.maxPollGapMs || 0, delta); if (sample.db < -80 || element.paused || element.muted) { active.silentPolls = (active.silentPolls || 0) + 1; active.observedSilenceMs = (active.observedSilenceMs || 0) + delta; } }
      if (active.firstAudioMs !== undefined) return;
      active.lastSignal = sample;
      const matches = active.kind !== 'switch' || (element.dataset.internalAudioTrack===active.track&&(active.track === 'internal-2' ? Math.abs(sample.hz - englishHz) < 100 : sample.hz > 400 && sample.hz < 900));
      if (!element.paused && !element.muted && sample.db > -80 && matches && Math.abs(element.currentTime-video.currentTime)<.15 && (active.kind !== 'seek' || (!element.seeking && Math.abs(video.currentTime - active.target) < .4))) {
        active.firstAudioMs = performance.now() - active.start; active.audioSignal = sample; active.driftSeconds = Math.abs(element.currentTime - video.currentTime);
      }
    }, 8);
    function begin(kind) { active = { kind, start: performance.now(), videoSourceBefore: video.currentSrc, positionBefore: video.currentTime, videoLoadsBefore: window.videoLoadCount }; window.trials.push(active); ctx.resume(); return active; }
    document.querySelector('#start').onclick = async () => {
      const trial = begin('open');
      try {
        player?.destroy(); player = new window.native.NativePlayer(video, message => window.errors.push(message));
        window.player = player;
        const load = player.load(media, strategy === 'original' ? undefined : 'internal-1');
        await load; trial.loadReadyMs = performance.now() - trial.start;
        await player.play(); trial.playReadyMs = performance.now() - trial.start;
      } catch (error) { trial.error = String(error); trial.mediaDiagnostic=[...document.querySelectorAll('video,audio')].map(v=>({tag:v.tagName,position:v.currentTime,duration:v.duration,paused:v.paused,seeking:v.seeking,readyState:v.readyState,networkState:v.networkState,buffered:Array.from({length:v.buffered.length},(_,i)=>[v.buffered.start(i),v.buffered.end(i)]),error:v.error?.message})); }
    };
    document.querySelector('#switch').onclick = async () => {
      const trial = begin('switch'); trial.track = nextTrack; trial.observedSilenceMs=0; trial.maxVideoFrameGapMs=0;
      try { await player.selectAudio(nextTrack); trial.readyMs = performance.now() - trial.start; trial.videoSourceUnchanged = video.currentSrc === trial.videoSourceBefore; trial.videoLoadsAfter = window.videoLoadCount; nextTrack = nextTrack === 'internal-1' ? 'internal-2' : 'internal-1'; }
      catch (error) { trial.error = String(error); }
    };
    let seekIndex=0;
    document.querySelector('#seek').onclick = () => { const trial = begin('seek'); const duration=Number(probe.format.duration); const targets=duration>100?[900,1800,duration-20,2]:[6,1]; trial.target=targets[seekIndex++%targets.length]; player.seek(trial.target); };
    window.stopBenchmark = async () => { clearInterval(poll); video.cancelVideoFrameCallback(frameId); player?.destroy(); await ctx.close(); };
  }, { probe, container, strategy, englishHz });
}
function summarize(trials, field) {
  const numbers = trials.map(t => t[field]).filter(Number.isFinite).sort((a, b) => a - b);
  return numbers.length ? { n: numbers.length, min: numbers[0], median: numbers[Math.floor(numbers.length / 2)], p95: numbers[Math.ceil(numbers.length * .95) - 1], max: numbers.at(-1) } : null;
}
(async () => {
  let browser;
  try {
    await new Promise(resolve => server.listen(0, '127.0.0.1', resolve));
    const executablePath = process.env.MEDIA_BROWSER_EXECUTABLE;
    browser = await chromium.launch({ ...(executablePath ? { executablePath } : { channel: process.env.MEDIA_BROWSER_CHANNEL || 'chrome' }), headless: true, args: ['--autoplay-policy=no-user-gesture-required'] });
    report.browser = browser.version(); report.ffmpeg = execFileSync('ffmpeg', ['-version'], { encoding: 'utf8' }).split('\n')[0];
    for (const strategy of ['original', 'segmented']) {
      const context = await browser.newContext(), page = await context.newPage();
      await page.goto(`http://127.0.0.1:${server.address().port}/`); await instrument(page, strategy);
      const result = { strategy, trials: [] }; report.strategies.push(result);
      for (let i = 0; i < rounds; i++) {
        await page.click('#start');
        await page.waitForFunction(() => { const t = window.trials.at(-1); return t.error || (t.firstFrameMs !== undefined && t.firstAudioMs !== undefined && t.playReadyMs !== undefined); }, null, { timeout: 20000 }).catch(error => { result.timeout = String(error); });
        const open = await page.evaluate(() => window.trials.at(-1)); open.cache = i === 0 ? 'fresh-browser-context' : 'warm-browser-and-preparations'; result.trials.push(open);
        if (open.error || result.timeout) break;
        if (strategy === 'segmented') {
          for (let n = 0; n < 2; n++) {
            await page.waitForTimeout(600); // Explicit stable-playback precondition, outside switch timing.
            await page.click('#switch');
            await page.waitForFunction(() => { const t = window.trials.at(-1); return t.error || (t.readyMs !== undefined && t.firstAudioMs !== undefined); }, null, { timeout: 15000 });
            await page.waitForTimeout(180); // Observation window; excluded from click→PCM and ready timings.
            result.trials.push(await page.evaluate(() => window.trials.at(-1)));
          }
        }
        await page.click('#seek');
        await page.waitForFunction(() => { const t = window.trials.at(-1); return t.error || (t.firstFrameMs !== undefined && t.firstAudioMs !== undefined); }, null, { timeout: 15000 });
        result.trials.push(await page.evaluate(() => window.trials.at(-1)));
      }
      result.errors = await page.evaluate(() => window.errors);
      result.passed = !result.timeout && result.errors.length===0 && !result.trials.some(t=>t.error);
      result.summary = {};
      for (const kind of ['open', 'switch', 'seek']) {
        const trials = result.trials.filter(t => !t.error && t.kind === kind && (kind !== 'open' || t.cache.startsWith('warm')));
        result.summary[kind] = Object.fromEntries(['firstFrameMs', 'firstAudioMs', 'playReadyMs', 'readyMs', 'maxVideoFrameGapMs', 'observedSilenceMs', 'maxPollGapMs'].map(field => [field, summarize(trials, field)]));
      }
      await page.evaluate(() => window.stopBenchmark()); await context.close();
    }
    if (process.env.MEDIA_PERFORMANCE_CONCURRENT === '1') {
      const contexts = [], pages = [];
      for(let i=0;i<4;i++) { const context=await browser.newContext(); contexts.push(context); const page=await context.newPage(); await page.goto(`http://127.0.0.1:${server.address().port}/`); await instrument(page,'segmented'); pages.push(page); }
      const cpuBefore=process.cpuUsage(),memoryBefore=process.memoryUsage();
      await Promise.all(pages.map(async page=>{await page.click('#start'); await page.waitForFunction(()=>{const t=window.trials.at(-1);return t.error||(t.firstFrameMs!==undefined&&t.firstAudioMs!==undefined&&t.playReadyMs!==undefined);},null,{timeout:20000});}));
      await Promise.all(pages.map(page=>page.waitForTimeout(1000)));
      report.concurrent={sessions:4,cache:'fresh browser contexts, audio chunks explicitly warmed by previous serial trials',results:await Promise.all(pages.map(page=>page.evaluate(()=>({trial:window.trials.at(-1),frames:document.querySelector('video').getVideoPlaybackQuality().totalVideoFrames,dropped:document.querySelector('video').getVideoPlaybackQuality().droppedVideoFrames,errors:window.errors})))),httpHarnessOnly:{cpuMicroseconds:process.cpuUsage(cpuBefore),rssBefore:memoryBefore.rss,rssAfter:process.memoryUsage().rss,scope:'Node.js fixture HTTP process only, excludes Chrome, Rust Core/Node and FFmpeg children'}};
      await Promise.all(pages.map(page=>page.evaluate(()=>window.stopBenchmark()))); await Promise.all(contexts.map(context=>context.close()));
    }
    if(report.strategies.some(strategy=>!strategy.passed))process.exitCode=1;
  } catch (error) { report.failure = String(error.stack); const page = browser?.contexts().at(-1)?.pages().at(-1); if (page) report.diagnostic = await page.evaluate(() => ({ trials: window.trials, errors: window.errors, videos: [...document.querySelectorAll('video,audio')].map(v => ({ tag: v.tagName, time: v.currentTime, paused: v.paused, muted: v.muted, ready: v.readyState, error: v.error?.message, src: v.currentSrc })) })); process.exitCode = 1; }
  finally {
    if (browser) await browser.close();
    await new Promise(resolve => server.close(resolve)); await Promise.allSettled([...jobs.values()]);
    // Hash after timing: provenance must not secretly prime the file cache.
    report.fixture.sha256 = await new Promise((resolve, reject) => { const hash = createHash('sha256'); fs.createReadStream(fixture).on('data', bytes => hash.update(bytes)).on('error', reject).on('end', () => resolve(hash.digest('hex'))); });
    fs.rmSync(temp, { recursive: true, force: true });
    fs.mkdirSync(out, { recursive: true }); fs.writeFileSync(path.join(out, 'report.json'), JSON.stringify(report, null, 2) + '\n');
    console.log(JSON.stringify({ out, failure: report.failure, strategies: report.strategies.map(s => ({ strategy: s.strategy, timeout: s.timeout, summary: s.summary })) }, null, 2));
  }
})();
