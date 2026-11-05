// Production-bundled libass, real Chrome/AV1 fixture, same Wasm CSP as Core.
// Synthetic ASS styles only; no provider, NAS, embedded-font or device proof.
const { chromium } = require('../apps/web/node_modules/playwright');
const fs = require('node:fs');
const path = require('node:path');
const http = require('node:http');
const assert = require('node:assert/strict');
const build = path.resolve('apps/web/build');
const manifest = JSON.parse(fs.readFileSync('apps/web/.svelte-kit/output/client/.vite/manifest.json'));
const moduleUrl = '/app/' + manifest['src/lib/player/ass-renderer.ts'].file;
const fixture = path.resolve(process.env.MEDIA_ASS_FIXTURE || 'target/test_stream.mp4');
const out = path.resolve(process.env.MEDIA_REPORT_DIR || 'target/ass-renderer');
const ass = `[Script Info]
ScriptType: v4.00+
PlayResX: 640
PlayResY: 360
[V4+ Styles]
Format: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding
Style: Default,Liberation Sans,32,&H0000FFFF,&H000000FF,&H00000000,&H00000000,-1,0,0,0,100,100,0,0,1,2,0,2,10,10,20,1
[Events]
Format: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text
Dialogue: 0,0:00:00.00,0:00:10.00,Default,,0,0,0,,{\\pos(320,180)\\frz-10}ASS styles fidèles
`;
const csp = "default-src 'none'; script-src 'self' 'wasm-unsafe-eval'; worker-src 'self'; connect-src 'self'; media-src 'self'; style-src 'self'";
const server = http.createServer((req, res) => {
  res.setHeader('Content-Security-Policy', csp);
  res.setHeader('Cache-Control', 'no-store');
  const pathname = new URL(req.url, 'http://localhost').pathname;
  if (pathname === '/') { res.setHeader('Content-Type','text/html'); return res.end('<!doctype html><title>ASS fixture</title><link rel="stylesheet" href="/fixture.css"><video muted width="640" height="360" src="/fixture.mp4"></video>'); }
  if (pathname === '/fixture.css') { res.setHeader('Content-Type','text/css'); return res.end('html,body{margin:0;background:#101820}video{display:block;width:640px;height:360px}'); }
  if (pathname === '/fixture.ass') { res.setHeader('Content-Type','text/plain'); return res.end(ass); }
  let filename = pathname === '/fixture.mp4' ? fixture : path.resolve(build, '.' + pathname.replace(/^\/app/,''));
  if (filename !== fixture && !filename.startsWith(build + path.sep)) { res.writeHead(403); return res.end(); }
  if (!fs.existsSync(filename)) { res.writeHead(404); return res.end(); }
  const mime = {'.js':'text/javascript','.wasm':'application/wasm','.woff2':'font/woff2','.mp4':'video/mp4'}[path.extname(filename)] || 'application/octet-stream';
  res.setHeader('Content-Type',mime); fs.createReadStream(filename).pipe(res);
});
async function run() {
  fs.mkdirSync(out,{recursive:true});
  await new Promise(resolve=>server.listen(0,'127.0.0.1',resolve));
  const browser = await chromium.launch({channel:'chrome',headless:true});
  try {
    const page = await browser.newPage({viewport:{width:640,height:360}});
    const errors=[]; page.on('pageerror',error=>errors.push(error.message));
    page.on('console',msg=>{if(['error','warning'].includes(msg.type()))console.log(msg.type(),msg.text());});
    await page.goto(`http://127.0.0.1:${server.address().port}/`);
    await page.waitForFunction(()=>document.querySelector('video').readyState >= 2);
    const before=await page.screenshot();
    await page.evaluate(async url=>{
      const {createAssRenderer}=await import(url);
      window.ass=await createAssRenderer(document.querySelector('video'),'/fixture.ass',0);
    },moduleUrl);
    await page.waitForFunction(()=>document.querySelector('canvas')?.getBoundingClientRect().width > 0);
    await page.waitForTimeout(500);
    const rendered=await page.screenshot({path:path.join(out,'styled.png')});
    assert(!before.equals(rendered),'ASS canvas must draw visible styled text');
    await page.evaluate(async()=>{window.ass.timeOffset=-30;document.querySelector('video').dispatchEvent(new Event('timeupdate'));});
    await page.waitForTimeout(300);
    const shifted=await page.screenshot();
    assert(before.equals(shifted),'subtitle timing correction must also repaint a paused video');
    // Renderer cleanup must remove the overlay and release the worker.
    await page.evaluate(async()=>{await window.ass.destroy();});
    assert.equal(await page.locator('canvas').count(),0);
    assert.deepEqual(errors,[]);
    fs.writeFileSync(path.join(out,'report.json'),JSON.stringify({browser:await browser.version(),scope:'local Chrome, synthetic ASS and AV1 MP4, production assets/CSP',styledRender:true,cleanup:true,errors},null,2));
    console.log('ASS production render and cleanup passed');
  } finally { await browser.close(); server.close(); }
}
run().catch(error=>{console.error(error);server.close();process.exitCode=1;});
