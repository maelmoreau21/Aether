import { createServer } from 'node:http';
import { readFile, mkdtemp, mkdir, rm } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { spawnSync } from 'node:child_process';
import { resolve, extname } from 'node:path';
import assert from 'node:assert/strict';
import { chromium } from 'playwright';

const root = resolve('build');
const evidence=resolve('../../docs/evidence/2026-10-20/offline');await mkdir(evidence,{recursive:true});
const temporary=await mkdtemp(resolve(tmpdir(),'media-pwa-test-'));
const fixture=resolve(temporary,'sample.opus');
assert.equal(spawnSync('ffmpeg',['-v','error','-f','lavfi','-i','sine=frequency=440:duration=3','-c:a','libopus','-y',fixture]).status,0);
const audio=await readFile(fixture);
const server = createServer(async (req, res) => {
  const path = new URL(req.url, 'http://localhost').pathname;
  if (path === '/api/v1/auth/me') {
    res.writeHead(200, { 'Content-Type': 'application/json' }); res.end(JSON.stringify({user:{id:'test-user',login:'test',role:'user'},csrfToken:'fixture'})); return;
  }
  if(path==='/api/v1/preferences'){res.writeHead(200,{'Content-Type':'application/json'});res.end('{}');return;}
  if(path==='/api/v1/music'){res.writeHead(200,{'Content-Type':'application/json'});res.end(JSON.stringify({items:[{id:'sample',title:'Piste hors ligne',artist:'Artiste test',album:'Album test',mime:'audio/ogg',codec:'opus'}],total:1}));return;}
  if(path==='/api/v1/music/tracks/sample/content'){res.writeHead(200,{'Content-Type':'audio/ogg','Content-Length':audio.length,'Accept-Ranges':'bytes'});res.end(audio);return;}
  let file = resolve(root, '.' + path.replace(/^\/app/, ''));
  if (!file.startsWith(root)) { res.writeHead(403); res.end(); return; }
  if (path.endsWith('/')) file = resolve(file, 'index.html');
  try {
    const body = await readFile(file);
    const type = { '.html': 'text/html', '.js': 'application/javascript', '.css': 'text/css', '.webmanifest': 'application/manifest+json', '.png': 'image/png' }[extname(file)] ?? 'application/octet-stream';
    res.writeHead(200, { 'Content-Type': type, 'Cache-Control': 'no-store', 'Content-Security-Policy': "default-src 'none'; script-src 'self'; worker-src 'self'; manifest-src 'self'; img-src 'self'; media-src 'self' blob:; connect-src 'self'; style-src 'self'" });
    res.end(body);
  } catch { res.writeHead(404); res.end(); }
});
await new Promise(done => server.listen(0, '127.0.0.1', done));
const origin = `http://127.0.0.1:${server.address().port}`;
const browser = await chromium.launch({ headless: true, args:['--mute-audio'] });
try {
  const context = await browser.newContext();
  const page = await context.newPage();
  const errors = [];
  page.on('pageerror', error => errors.push(error.message));
  await page.goto(origin + '/app/music/');
  await page.evaluate(() => navigator.serviceWorker.ready.then(() => true));
  await page.reload();
  await page.waitForFunction(() => !!navigator.serviceWorker.controller);
  const manifest = await (await context.request.get(origin + '/app/manifest.webmanifest')).json();
  assert.equal(manifest.scope, '/app/');
  assert.equal(manifest.display, 'standalone');
  for (const icon of manifest.icons) {
    const result = await page.evaluate(async src => {
      const image = new Image(); image.src = src; await image.decode();
      return `${image.naturalWidth}x${image.naturalHeight}`;
    }, '/app/' + icon.src);
    assert.equal(result, icon.sizes);
  }
  const cached=await page.evaluate(async()=>{const names=await caches.keys();return (await Promise.all(names.map(async name=>(await(await caches.open(name)).keys()).map(request=>request.url)))).flat();});
  assert(cached.length>0);assert(cached.every(url=>url.includes('/app/_app/')||url.includes('/app/downloads/')));
  await page.getByRole('button',{name:'Lire Piste hors ligne',exact:true}).click();
  await page.getByText('Télécharger pour une lecture hors ligne',{exact:true}).click();
  await page.getByLabel('Phrase de déverrouillage (à conserver)').fill('phrase integration offline test');
  await page.getByRole('button',{name:'Télécharger cette version originale'}).click();
  await page.getByText('Disponible dans Téléchargements.').waitFor();
  await page.setViewportSize({width:390,height:844});assert.equal(await page.evaluate(()=>document.documentElement.scrollWidth>innerWidth),false);await page.screenshot({path:resolve(evidence,'music-mobile.png'),fullPage:true});
  await context.setOffline(true);
  const response = await page.goto(origin + '/app/films/');
  assert.equal(response.status(), 503);
  await page.getByRole('heading', { name: 'Connexion indisponible' }).waitFor();
  await page.getByRole('link',{name:'Mes téléchargements hors ligne'}).click();
  await page.getByRole('heading',{name:'Téléchargements hors ligne'}).waitFor();
  await page.getByLabel('Phrase de déverrouillage',{exact:true}).fill('wrong phrase');
  await page.getByRole('button',{name:'Déverrouiller',exact:true}).click();
  await page.getByText('Aucun téléchargement ne correspond à cette phrase.').waitFor();
  await page.getByLabel('Phrase de déverrouillage',{exact:true}).fill('phrase integration offline test');
  await page.getByRole('button',{name:'Déverrouiller',exact:true}).click();
  await page.getByRole('button',{name:'Lire',exact:true}).click();
  await page.waitForFunction(()=>document.querySelector('video')?.currentTime>0.2);
  assert.equal(await page.evaluate(()=>document.documentElement.scrollWidth>innerWidth),false);await page.screenshot({path:resolve(evidence,'offline-mobile.png'),fullPage:true});
  const decoded=await page.evaluate(async()=>{
    const context=new AudioContext();const source=context.createMediaElementSource(document.querySelector('video'));const analyser=context.createAnalyser();analyser.fftSize=8192;source.connect(analyser);analyser.connect(context.destination);await context.resume();await new Promise(resolve=>setTimeout(resolve,200));
    const time=new Float32Array(analyser.fftSize);analyser.getFloatTimeDomainData(time);const spectrum=new Float32Array(analyser.frequencyBinCount);analyser.getFloatFrequencyData(spectrum);let peak=1;for(let index=2;index<spectrum.length;index++)if(spectrum[index]>spectrum[peak])peak=index;
    return {rms:Math.sqrt(time.reduce((sum,value)=>sum+value*value,0)/time.length),frequency:peak*context.sampleRate/analyser.fftSize};
  });assert(decoded.rms>0.001);assert(Math.abs(decoded.frequency-440)<20,`decoded Opus frequency ${decoded.frequency}`);
  await page.evaluate(()=>{const video=document.querySelector('video');video.pause();video.currentTime=1.5;});
  await page.waitForFunction(()=>Math.abs(document.querySelector('video').currentTime-1.5)<0.1);
  const range=await page.evaluate(async()=>{const url=document.querySelector('video').src;const response=await fetch(url,{headers:{range:'bytes=0-15'}});return{status:response.status,bytes:Array.from(new Uint8Array(await response.arrayBuffer()))};});assert.equal(range.status,206);assert.deepEqual(range.bytes,Array.from(audio.subarray(0,16)));
  await page.getByRole('button',{name:'Verrouiller',exact:true}).click();
  await context.setOffline(false);
  await page.getByRole('link', { name: 'Retour à la médiathèque' }).click();
  assert.deepEqual(errors, []);
  console.log('PWA: manifest/icons, public-only shell cache, encrypted download, offline unlock, decoded Opus playback, pause/seek, exact Range, lock and reconnect passed.');
} finally { await browser.close(); await new Promise(done => server.close(done)); await rm(temporary,{recursive:true,force:true}); }
