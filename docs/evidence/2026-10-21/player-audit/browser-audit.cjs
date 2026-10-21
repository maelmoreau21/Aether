// Real Chrome + current TS player; isolated HTTP fixture server, not Core/RBAC evidence.
const fs = require('node:fs');
const path = require('node:path');
const os = require('node:os');
const http = require('node:http');
const assert = require('node:assert/strict');
const { execFileSync } = require('node:child_process');
const root = path.resolve(__dirname, '../../../..');
const { chromium } = require(path.join(root, 'apps/web/node_modules/playwright'));
const ts = require(path.join(root, 'apps/web/node_modules/typescript'));
const temp = fs.mkdtempSync(path.join(os.tmpdir(), 'media-player-audit-'));
const fixture = path.join(root, 'tests/fixtures/generated/representative/representative.mp4');
const opus = path.join(temp, 'fr.ogg');
const defaultEn = path.join(temp, 'default-en.mp4');
const report = { scope: 'Chrome, synthetic fixtures, current NativePlayer/ExternalAudio, isolated test HTTP server' };
let browser;
const server = http.createServer((req, res) => {
  const name = req.url.split('?')[0];
  const file = { '/video': fixture, '/fr': opus, '/default-en': defaultEn }[name];
  if (!file) {
    if (name === '/') { res.setHeader('Content-Type', 'text/html'); res.end('<video></video>'); }
    else { res.writeHead(404); res.end(); }
    return;
  }
  const size = fs.statSync(file).size;
  const range = /^bytes=(\d+)-(\d*)$/.exec(req.headers.range || '');
  const start = range ? Number(range[1]) : 0;
  const end = range && range[2] ? Math.min(size - 1, Number(range[2])) : size - 1;
  if (start >= size || end < start) { res.writeHead(416); res.end(); return; }
  res.writeHead(range ? 206 : 200, {
    'Content-Type': name === '/fr' ? 'audio/ogg' : 'video/mp4',
    'Content-Length': end - start + 1, 'Accept-Ranges': 'bytes',
    ...(range ? { 'Content-Range': `bytes ${start}-${end}/${size}` } : {})
  });
  if (req.method === 'HEAD') res.end();
  else { const stream = fs.createReadStream(file, { start, end }); res.on('close', () => stream.destroy()); stream.pipe(res); }
});
function compile(name) {
  return ts.transpileModule(fs.readFileSync(path.join(root, 'apps/web/src/lib/player', name), 'utf8'),
    { compilerOptions: { target: ts.ScriptTarget.ES2022, module: ts.ModuleKind.CommonJS } }).outputText;
}
(async () => {
  try {
    execFileSync('ffmpeg', ['-v','error','-nostdin','-i',fixture,'-map','0:1','-c:a','copy',opus]);
    execFileSync('ffmpeg', ['-v','error','-nostdin','-i',fixture,'-map','0','-c','copy','-disposition:a:0','0','-disposition:a:1','default',defaultEn]);
    await new Promise(resolve => server.listen(0, '127.0.0.1', resolve));
    browser = await chromium.launch({ channel: 'chrome', headless: true, args: ['--autoplay-policy=no-user-gesture-required'] });
    report.browser = browser.version(); report.os = os.release();
    const page = await browser.newPage();
    await page.goto(`http://127.0.0.1:${server.address().port}/`);
    await page.addScriptTag({ content: `const ext={}; (function(exports){${compile('external-audio.ts')}\n})(ext);
      const selection={}; (function(exports){${compile('source-selection.ts')}\n})(selection);
      const nat={}; (function(exports,require){${compile('native.ts')}\n})(nat,name=>name==='./external-audio'?ext:selection); window.NativePlayer=nat.NativePlayer;` });
    report.failedSwitch = await page.evaluate(async () => {
      const video = document.querySelector('video'); const errors=[];
      const player = new window.NativePlayer(video, message=>errors.push(message));
      const sources = ['fr','en'].map(language=>({id:language,audioTrackId:language,language,mime:'video/mp4',url:'/video',externalAudioUrl:`/${language}`}));
      await player.load({id:'audit',title:'Audit',subtitles:[],sources});
      let failure;
      try { await player.selectAudio('en'); } catch(e) { failure=e.message; }
      const before=document.querySelector('audio'); await player.selectAudio('fr');
      const result={failure,errors,retainedPreviousAudio:before===document.querySelector('audio'),audioUrl:document.querySelector('audio').currentSrc,mediaError:document.querySelector('audio').error?.code};
      player.destroy(); return result;
    });
    assert.ok(report.failedSwitch.failure);
    assert.equal(report.failedSwitch.retainedPreviousAudio,true);
    assert.match(report.failedSwitch.audioUrl,/\/fr$/);
    assert.equal(report.failedSwitch.mediaError,undefined);
    // Original-source metadata says first audio FR, but the container explicitly defaults EN.
    report.originalDefault = await page.evaluate(async () => {
      const video=document.querySelector('video');
      const player=new window.NativePlayer(video,()=>{});
      await player.load({id:'original',title:'Audit',subtitles:[],sources:[
        {id:'original',audioTrackId:'internal-default',language:'fr',mime:'video/mp4',url:'/default-en'},
        {id:'fr',audioTrackId:'internal-1',language:'fr',mime:'video/mp4',url:'/video',externalAudioUrl:'/fr'}
      ]});
      const context=new AudioContext(); await context.resume();
      const source=context.createMediaElementSource(document.querySelector('audio') ?? video), analyser=context.createAnalyser(); analyser.fftSize=8192;
      source.connect(analyser); analyser.connect(context.destination); await player.play();
      await new Promise(resolve=>setTimeout(resolve,600));
      const bins=new Float32Array(analyser.frequencyBinCount); analyser.getFloatFrequencyData(bins);
      let peak=0; for(let i=1;i<bins.length;i++) if(bins[i]>bins[peak]) peak=i;
      const result={url:video.currentSrc,peakHz:peak*context.sampleRate/analyser.fftSize,peakDb:bins[peak]};
      player.destroy(); await context.close(); return result;
    });
    assert.ok(report.originalDefault.peakDb > -80);
    assert.ok(report.originalDefault.peakHz > 400 && report.originalDefault.peakHz < 900,
      'The concrete FR signal must be decoded instead of the original default EN');
    report.passed = true;
  } catch(error) { report.failure=String(error.stack); process.exitCode=1; }
  finally {
    if(browser) await browser.close();
    await new Promise(resolve=>server.close(resolve));
    for(const file of [opus,defaultEn]) if(fs.existsSync(file)) fs.unlinkSync(file);
    fs.rmdirSync(temp);
    fs.writeFileSync(path.join(__dirname,'browser-fixed.json'),JSON.stringify(report,null,2)+'\n');
    console.log(JSON.stringify(report,null,2));
  }
})();
