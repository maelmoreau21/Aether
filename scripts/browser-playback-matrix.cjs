#!/usr/bin/env node
// Direct Play matrix in a real Chromium-family browser: AV1 8/10-bit x MP4/MKV/WebM x Opus stereo/5.1.
// Measures the first picture and the first audible sound separately, then pause/resume and seeks.
//
//   Media: scripts/generate-playback-matrix.sh (synthetic test patterns).
//   MEDIA_MATRIX_DIR      media folder (default tests/fixtures/generated/matrix)
//   MEDIA_MATRIX_REPORT   output JSON (default target/playback-matrix/report.json)
//   MEDIA_BROWSER_CHANNEL chrome | msedge (installed browser)   MEDIA_BROWSER_EXECUTABLE explicit binary
//   MEDIA_MATRIX_ROUNDS   reopenings per file (default 3: the first one in a fresh context)
//
// First picture: first requestVideoFrameCallback after play() (frame handed to the compositor).
// First sound: first analyser frame above -70 dBFS peak, polled every animation frame / 8 ms, on the
// element's own audio output. Which track is audible is read from the dominant FFT bin (440 Hz = FR,
// 1050 Hz = EN). This is not speaker onset. Chromium does not expose audioTracks by default, so a
// native element cannot switch language: that limit is recorded, not worked around.
// The server serves the files by HTTP Range in 64 KiB blocks; nothing is transcoded or prepared.
'use strict';
const fs = require('fs');
const http = require('http');
const path = require('path');
const { chromium } = require('../apps/web/node_modules/playwright');

const dir = path.resolve(process.env.MEDIA_MATRIX_DIR || 'tests/fixtures/generated/matrix');
const reportPath = path.resolve(process.env.MEDIA_MATRIX_REPORT || 'target/playback-matrix/report.json');
const rounds = Number(process.env.MEDIA_MATRIX_ROUNDS || 3);
const mime = { mp4: 'video/mp4', mkv: 'video/x-matroska', webm: 'video/webm' };
const files = fs.readdirSync(dir).filter(name => /\.(mp4|mkv|webm)$/.test(name)).sort();
if (!files.length) throw new Error(`No media in ${dir}; run scripts/generate-playback-matrix.sh`);

const requests = [];
const server = http.createServer((req, res) => {
  const name = decodeURIComponent(new URL(req.url, 'http://x').pathname.slice(1));
  if (name === '') { res.writeHead(200, { 'Content-Type': 'text/html' }); res.end('<video playsinline preload="auto"></video>'); return; }
  const file = path.join(dir, path.basename(name));
  if (!fs.existsSync(file)) { res.writeHead(404); res.end(); return; }
  const size = fs.statSync(file).size;
  const range = /^bytes=(\d*)-(\d*)$/.exec(req.headers.range || '');
  let start = 0, end = size - 1, status = 200;
  if (range) {
    start = range[1] === '' ? Math.max(0, size - Number(range[2])) : Number(range[1]);
    end = range[1] !== '' && range[2] !== '' ? Math.min(Number(range[2]), size - 1) : size - 1;
    if (start > end || start >= size) { res.writeHead(416, { 'Content-Range': `bytes */${size}` }); res.end(); return; }
    status = 206;
  }
  const record = { name, range: req.headers.range || null, status, bytes: end - start + 1 };
  requests.push(record);
  const headers = { 'Content-Type': mime[path.extname(name).slice(1)], 'Accept-Ranges': 'bytes', 'Content-Length': end - start + 1, 'Cache-Control': 'no-store' };
  if (status === 206) headers['Content-Range'] = `bytes ${start}-${end}/${size}`;
  res.writeHead(status, headers);
  if (req.method === 'HEAD') { res.end(); return; }
  const stream = fs.createReadStream(file, { start, end, highWaterMark: 64 * 1024 });
  res.on('close', () => stream.destroy());
  stream.pipe(res);
});

async function measure(page, name) {
  return page.evaluate(async ({ name }) => {
    const video = document.querySelector('video');
    const wait = (ms) => new Promise(resolve => setTimeout(resolve, ms));
    const result = { events: [] };
    const ctx = new AudioContext();
    const source = ctx.createMediaElementSource(video);
    const analyser = ctx.createAnalyser(); analyser.fftSize = 2048; analyser.smoothingTimeConstant = 0;
    source.connect(analyser); analyser.connect(ctx.destination);
    const bins = new Float32Array(analyser.frequencyBinCount);
    const hzPerBin = ctx.sampleRate / analyser.fftSize;
    const sound = () => { analyser.getFloatFrequencyData(bins); let peak = -Infinity, at = 0; bins.forEach((v, i) => { if (v > peak) { peak = v; at = i; } }); return { peak, hz: at * hzPerBin }; };
    const track = (label, fromStart) => new Promise(resolve => {
      const out = {}; const t0 = fromStart();
      video.requestVideoFrameCallback(() => { out.frameMs = performance.now() - t0; });
      const poll = () => { const s = sound(); if (s.peak > -70 && out.soundMs === undefined) { out.soundMs = performance.now() - t0; out.hz = Math.round(s.hz); } if (out.soundMs !== undefined && out.frameMs !== undefined) resolve(out); else if (performance.now() - t0 > 5000) resolve({ ...out, timeout: label }); else setTimeout(poll, 8); };
      poll();
    });
    video.muted = false; video.src = `/${name}`;
    const t0 = performance.now();
    const metadata = new Promise((resolve, reject) => { video.onloadedmetadata = () => resolve(performance.now() - t0); video.onerror = () => reject(new Error(`MediaError ${video.error && video.error.code}`)); });
    video.load();
    try { result.metadataMs = await metadata; } catch (error) { result.error = String(error.message); return result; }
    result.duration = video.duration; result.videoWidth = video.videoWidth;
    result.audioTracksApi = Boolean(video.audioTracks);
    await ctx.resume();
    const start = performance.now();
    await video.play();
    result.playResolvedMs = performance.now() - start;
    Object.assign(result, await track('start', () => start));
    await wait(400);
    video.pause(); const pausedAt = video.currentTime; await wait(200); result.pauseHeld = Math.abs(video.currentTime - pausedAt) < 0.05;
    const resumeStart = performance.now(); await video.play(); Object.assign(result, { resume: await track('resume', () => resumeStart) });
    result.seeks = [];
    for (const target of [Math.min(8, video.duration - 2), 2]) {
      const seekStart = performance.now(); video.currentTime = target;
      await new Promise(resolve => { const done = () => resolve(); video.addEventListener('seeked', done, { once: true }); setTimeout(resolve, 5000); });
      const seeked = await track('seek', () => seekStart);
      result.seeks.push({ target, reached: Math.round(video.currentTime * 100) / 100, ...seeked });
    }
    video.pause(); video.removeAttribute('src'); video.load(); await ctx.close();
    return result;
  }, { name });
}

(async () => {
  await new Promise(resolve => server.listen(0, '127.0.0.1', resolve));
  const origin = `http://127.0.0.1:${server.address().port}/`;
  const executablePath = process.env.MEDIA_BROWSER_EXECUTABLE;
  const browser = await chromium.launch({ ...(executablePath ? { executablePath } : { channel: process.env.MEDIA_BROWSER_CHANNEL || 'chrome' }), headless: true, args: ['--autoplay-policy=no-user-gesture-required'] });
  const report = { date: new Date().toISOString(), browser: browser.version(), channel: process.env.MEDIA_BROWSER_CHANNEL || (executablePath ? 'executable' : 'chrome'), rounds, scope: 'Direct Play of synthetic AV1/Opus media over loopback HTTP Range; no Core, Node, NAS or real film', files: [] };
  for (const name of files) {
    const entry = { name, runs: [] };
    for (let round = 0; round < rounds; round++) {
      // Round 0 uses a fresh context; later rounds reuse the browser but not the page.
      const context = await browser.newContext(); const page = await context.newPage();
      await page.goto(origin);
      if (round === 0) entry.canPlayType = await page.evaluate(({ mime }) => document.createElement('video').canPlayType(mime), { mime: mime[path.extname(name).slice(1)] + '; codecs="av01.0.04M.' + (name.includes('10bit') ? '10' : '08') + ', opus"' });
      const before = requests.length;
      entry.runs.push({ round, ...(await measure(page, name)), requests: requests.slice(before).map(r => ({ status: r.status, range: r.range, bytes: r.bytes })).length, bytesServed: requests.slice(before).reduce((sum, r) => sum + r.bytes, 0) });
      await context.close();
    }
    report.files.push(entry);
    const first = entry.runs[0];
    console.log(`${name.padEnd(34)} ${first.error ? 'ERROR ' + first.error : `frame ${first.frameMs?.toFixed(0)} ms  sound ${first.soundMs?.toFixed(0) ?? 'none'} ms  ${first.hz ?? ''} Hz  seeks ${first.seeks?.map(s => s.soundMs?.toFixed(0) ?? 'x').join('/')}`}`);
  }
  fs.mkdirSync(path.dirname(reportPath), { recursive: true });
  fs.writeFileSync(reportPath, JSON.stringify(report, null, 2));
  await browser.close(); server.close();
  const failed = report.files.filter(f => f.runs.some(r => r.error || r.timeout));
  console.log(`${report.files.length} files, ${failed.length} with an error or a missing signal -> ${reportPath}`);
})().catch(error => { console.error(error); process.exit(1); });
