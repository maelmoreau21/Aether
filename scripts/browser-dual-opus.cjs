// Real decoding of a legal dual-5.1 MP4 with the production TS player.
// Isolated HTTP server; Core/Node authorization is covered by browser_scanner.
const fs = require('node:fs');
const path = require('node:path');
const os = require('node:os');
const http = require('node:http');
const assert = require('node:assert/strict');
const {execFileSync} = require('node:child_process');
const root = path.resolve(__dirname, '..');
const {chromium} = require('../apps/web/node_modules/playwright');
const ts = require('../apps/web/node_modules/typescript');
const fixture = path.join(root, 'tests/fixtures/generated/dual-opus/dual-opus.mp4');
const probe = JSON.parse(execFileSync('ffprobe', ['-v','error','-show_streams','-of','json',fixture]));
const temp = fs.mkdtempSync(path.join(os.tmpdir(), 'media-dual-opus-'));
const report = {os:os.release(), scope:'Synthetic 1080p AV1 10-bit + two Opus 5.1; isolated HTTP, no physical surround proof', probe, switches:[]};
const files = {'/original':fixture};
let browser;
const server = http.createServer((req,res) => {
  const name = req.url.split('?')[0], file = files[name];
  if (!file) { res.writeHead(name === '/' ? 200 : 404, {'Content-Type':'text/html'}); res.end(name === '/' ? '<div><button>Fullscreen</button><video></video></div>' : ''); return; }
  const size = fs.statSync(file).size, range = /^bytes=(\d+)-(\d*)$/.exec(req.headers.range || '');
  const start = range ? Number(range[1]) : 0, end = range?.[2] ? Math.min(size-1,Number(range[2])) : size-1;
  if (start >= size || end < start) { res.writeHead(416,{'Content-Range':`bytes */${size}`}); res.end(); return; }
  res.writeHead(range ? 206 : 200, {'Content-Type':name.startsWith('/audio-')?'audio/ogg':'video/mp4',
    'Content-Length':end-start+1,'Accept-Ranges':'bytes',...(range?{'Content-Range':`bytes ${start}-${end}/${size}`}:{})});
  if(req.method === 'HEAD') res.end();
  else {const stream=fs.createReadStream(file,{start,end}); res.on('close',()=>stream.destroy()); stream.pipe(res);}
});
function compile(name) {
  return ts.transpileModule(fs.readFileSync(path.join(root,'apps/web/src/lib/player',name+'.ts'),'utf8'),
    {compilerOptions:{target:ts.ScriptTarget.ES2022,module:ts.ModuleKind.CommonJS}}).outputText;
}
(async()=>{
  try {
    for (const [name,args] of [['video',['-map','0:v:0','-an','-c:v','copy','-movflags','frag_keyframe+empty_moov+default_base_moof','-f','mp4']],
      ...[1,2].map(index=>['audio-'+index,['-map',`0:${index}`,'-vn','-c:a','copy','-f','ogg']])]) {
      files['/'+name] = path.join(temp,name);
      execFileSync('ffmpeg',['-v','error','-nostdin','-i',fixture,...args,files['/'+name]]);
    }
    await new Promise(resolve=>server.listen(0,'127.0.0.1',resolve));
    browser=await chromium.launch({channel:process.env.MEDIA_BROWSER_CHANNEL || 'chrome',executablePath:process.env.MEDIA_BROWSER_EXECUTABLE || undefined,headless:true,args:['--autoplay-policy=no-user-gesture-required']});
    report.browser=browser.version();
    const page=await browser.newPage(); page.setDefaultTimeout(15000);
    await page.goto(`http://127.0.0.1:${server.address().port}/`);
    for(const name of ['source-selection','external-audio','native']) {
      await page.addScriptTag({content:`window[${JSON.stringify(name)}]={};(function(exports,require){${compile(name)}\n})(window[${JSON.stringify(name)}],name=>window[name.slice(2)]);`});
    }
    await page.evaluate(probe=>{
      window.errors=[];window.videoLoads=0;
      const video=document.querySelector('video');video.addEventListener('loadstart',()=>window.videoLoads++);
      document.querySelector('button').onclick=()=>video.parentElement.requestFullscreen();
      window.media={id:'dual',title:'Synthetic',subtitles:[],sources:[
        {id:'original',audioTrackId:'internal-default',language:'und',url:'/original',mime:'video/mp4',tracks:probe.streams},
        ...probe.streams.filter(t=>t.codec_type==='audio').map(t=>({id:'track-'+t.index,audioTrackId:'internal-'+t.index,
          language:t.tags.language,url:'/video',externalAudioUrl:'/audio-'+t.index,mime:'video/mp4',prepare:true,tracks:[t]}))]};
      window.openPlayer=async(id,language='fr')=>{
        window.player=new window.native.NativePlayer(video,message=>window.errors.push(message));
        const selected=window['source-selection'].chooseSource(window.media,id,language,s=>Boolean(video.canPlayType(window['source-selection'].playbackMime(window.media,s))));
        await window.player.load(window.media,selected.audioTrackId);await window.player.play();return selected.audioTrackId;
      };
      window.sample=async()=>{
        const audio=document.querySelector('audio'),c=new AudioContext();await c.resume();
        const source=c.createMediaElementSource(audio),analyser=c.createAnalyser();analyser.fftSize=8192;source.connect(analyser);analyser.connect(c.destination);
        await new Promise(r=>setTimeout(r,250));const bins=new Float32Array(analyser.frequencyBinCount);analyser.getFloatFrequencyData(bins);
        let peak=1;for(let i=2;i<bins.length;i++)if(bins[i]>bins[peak])peak=i;
        const result={hz:peak*c.sampleRate/analyser.fftSize,db:bins[peak],drift:Math.abs(audio.currentTime-video.currentTime),
          time:video.currentTime,url:video.currentSrc,frames:video.getVideoPlaybackQuality().totalVideoFrames,fullscreen:!!document.fullscreenElement,loads:window.videoLoads};
        source.disconnect();await c.close();return result;
      };
    },probe);
    await page.getByRole('button').click();
    report.capabilities=await page.evaluate(()=>['video/mp4; codecs="av01"','video/mp4','audio/ogg; codecs="opus"'].map(type=>({type,result:document.createElement('video').canPlayType(type)})));
    assert.equal(await page.evaluate(()=>window.openPlayer()),'internal-1');
    await page.waitForFunction(()=>document.querySelector('video').currentTime>.3);
    let lastTime=0,videoUrl;
    for(const id of ['internal-1','internal-2','internal-1','internal-2','internal-1']) {
      await page.evaluate(id=>window.player.selectAudio(id),id);
      const sample=await page.evaluate(()=>window.sample());
      assert.ok(sample.db>-80);assert.ok(Math.abs(sample.hz-(id==='internal-1'?440:990))<25,JSON.stringify(sample));
      assert.ok(sample.drift<.2,JSON.stringify(sample));assert.ok(sample.frames>0);assert.equal(sample.fullscreen,true);
      assert.ok(sample.time>=lastTime-.05);lastTime=sample.time;
      if(videoUrl)assert.equal(sample.url,videoUrl);videoUrl=sample.url;assert.equal(sample.loads,1);
      report.switches.push({id,...sample});
    }
    report.pausedSeeks=[];
    for(const target of [8,2]) {
      await page.evaluate(target=>{window.player.pause();window.player.seek(target);},target);
      await page.waitForFunction(target=>{const v=document.querySelector('video'),a=document.querySelector('audio');return !v.seeking&&!a.seeking&&a.paused&&v.paused&&Math.abs(v.currentTime-target)<.05&&Math.abs(a.currentTime-target)<.1;},target);
      await page.evaluate(()=>window.player.selectAudio('internal-2'));
      assert.ok(await page.evaluate(target=>window.player.state().paused&&Math.abs(window.player.state().position-target)<.05,target));
      await page.evaluate(()=>window.player.play());
      await page.waitForFunction(target=>document.querySelector('video').currentTime>target+.2,target);
      report.pausedSeeks.push({target,passed:true});
    }
    // Real failed candidate leaves the previous track recoverable.
    report.failedCandidate=await page.evaluate(async()=>{
      const saved=window.media.sources[1].externalAudioUrl;window.media.sources[1].externalAudioUrl='/missing';window.media.sources[1].prepare=false;
      let failure;try{await window.player.selectAudio('internal-1');}catch(e){failure=e.message;}
      window.media.sources[1].externalAudioUrl=saved;window.media.sources[1].prepare=true;
      await window.player.selectAudio('internal-2');await window.player.play();
      return {failure,url:document.querySelector('audio').currentSrc,count:document.querySelectorAll('audio').length};
    });
    assert.ok(report.failedCandidate.failure);assert.match(report.failedCandidate.url,/audio-2$/);assert.equal(report.failedCandidate.count,1);
    report.reopened=[];
    for(const id of ['internal-2','internal-1']) {
      await page.evaluate(()=>window.player.destroy());
      assert.equal(await page.evaluate(id=>window.openPlayer(id,id==='internal-1'?'fra':'eng'),id),id);
      await page.waitForFunction(()=>document.querySelector('video').currentTime>.2);
      const sample=await page.evaluate(()=>window.sample());assert.ok(Math.abs(sample.hz-(id==='internal-1'?440:990))<25);
      report.reopened.push({id,...sample});
    }
    report.errors=await page.evaluate(()=>window.errors);assert.deepEqual(report.errors,[]);
    await page.evaluate(()=>window.player.destroy());
    report.passed=true;
  } catch(error) {report.failure=String(error.stack);process.exitCode=1;}
  finally {
    if(browser)await browser.close();await new Promise(resolve=>server.close(resolve));
    for(const name of ['video','audio-1','audio-2']){const file=path.join(temp,name);if(fs.existsSync(file))fs.unlinkSync(file);}fs.rmdirSync(temp);
    const out=process.env.MEDIA_REPORT_DIR||'target/dual-opus-browser';fs.mkdirSync(out,{recursive:true});
    fs.writeFileSync(path.join(out,'report.json'),JSON.stringify(report,null,2)+'\n');console.log(JSON.stringify(report,null,2));
  }
})();
