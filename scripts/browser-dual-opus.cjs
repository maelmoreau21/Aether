// Real decoding of a legal dual-5.1 MP4 with the production TS player.
// Isolated HTTP server; Core/Node authorization is covered by browser_scanner.
const fs = require('node:fs');
const path = require('node:path');
const os = require('node:os');
const http = require('node:http');
const assert = require('node:assert/strict');
const {execFileSync, execFile, spawnSync} = require('node:child_process');
const segmented = process.env.MEDIA_TEST_SEGMENTED === '1';
const docker = process.env.MEDIA_TEST_DOCKER === '1';
const videoContainer = process.env.MEDIA_TEST_VIDEO_CONTAINER || 'mp4';
assert.ok(['mp4','webm'].includes(videoContainer));

const root = path.resolve(__dirname, '..');
const {chromium} = require('../apps/web/node_modules/playwright');
const ts = require('../apps/web/node_modules/typescript');
const fixture = process.env.MEDIA_TEST_FIXTURE || path.join(root, 'tests/fixtures/generated/dual-opus/dual-opus.mp4');
const originalContainer = path.extname(fixture).toLowerCase()==='.webm'?'webm':'mp4';
const probe = JSON.parse(execFileSync('ffprobe', ['-v','error','-show_streams','-of','json',fixture]));
const temp = fs.mkdtempSync(path.join(os.tmpdir(), 'media-dual-opus-'));
const report = {os:os.release(), segmented, docker, videoContainer, scope:'Synthetic AV1 10-bit + two Opus tracks; isolated HTTP, no physical surround proof', audioChannels:probe.streams.filter(t=>t.codec_type==='audio').map(t=>t.channels), probe, switches:[], preparations:[], delayedRequests:0, cancelledHttpRequests:0};
const files = {'/original':fixture};
let browser;
let proxyId;
const jobs = new Map();
const server = http.createServer(async (req,res) => {
  const name = req.url.split('?')[0];
  if(name==='/test-state'){res.setHeader('Content-Type','application/json');res.setHeader('Cache-Control','no-store');res.end(JSON.stringify({delayedRequests:report.delayedRequests}));return;}
  if(name.startsWith('/delayed-chunk-')){report.delayedRequests++;res.on('close',()=>{if(!res.writableEnded)report.cancelledHttpRequests++;});await new Promise(resolve=>setTimeout(resolve,2000));if(!res.destroyed){res.writeHead(503);res.end();}return;}
  const chunkMatch = /^\/audio-([12])-chunk-(\d+)$/.exec(name);
  if (segmented && chunkMatch && !files[name]) {
    if(!jobs.has(name)) jobs.set(name, new Promise((resolve,reject)=>{
      const index=Number(chunkMatch[1]),chunk=Number(chunkMatch[2]),file=path.join(temp,name.slice(1)),started=performance.now();
      const args=['-v','error','-nostdin','-ss',String(Math.max(0,chunk*10-1)),'-i',fixture,'-ss','0','-map',`0:${index}`,'-vn','-sn','-c:a','copy','-t','12','-avoid_negative_ts','make_zero','-movflags','frag_keyframe+empty_moov+default_base_moof','-frag_duration','1000000','-f','mp4',file];
      const dockerArgs=['run','--rm','--cpus=2','--memory=256m','--pids-limit=64','--network=none','--cap-drop=ALL','--security-opt=no-new-privileges','--user=10001:10001',
        '--mount',`type=bind,source=${path.dirname(fixture)},target=/media,readonly`,'--mount',`type=bind,source=${temp},target=/cache`,'media-preparation-runtime:test','bash','-c',
        `exec /probe/bin/ffmpeg -v error -nostdin -ss ${Math.max(0,chunk*10-1)} -protocol_whitelist fd,pipe -format_whitelist matroska,webm,mov,mp4 -max_alloc 67108864 -fd 0 -i fd: -ss 0 -map 0:${index} -vn -sn -c:a copy -t 12 -avoid_negative_ts make_zero -movflags frag_keyframe+empty_moov+default_base_moof -frag_duration 1000000 -f mp4 pipe:1 < /media/fixture.mp4 > /cache/${name.slice(1)}`];
      // Use a fixed fixture name in the read-only mount for shell-safe input.
      if(docker){dockerArgs[dockerArgs.length-1]=dockerArgs.at(-1).replace('/media/fixture.mp4', '/media/'+path.basename(fixture));assert.match(path.basename(fixture),/^[a-zA-Z0-9_.-]+$/);}
      execFile(docker?'docker':'ffmpeg',docker?dockerArgs:args,error=>{
        if(error){reject(error);return;}files[name]=file;report.preparations.push({index,chunk,elapsedMs:performance.now()-started,bytes:fs.statSync(file).size});resolve();
      });
    }));
    try {await jobs.get(name);}catch {res.writeHead(503);res.end();return;}
  }
  const file = files[name];
  if (!file) { res.writeHead(name === '/' ? 200 : 404, {'Content-Type':'text/html'}); res.end(name === '/' ? '<div><button>Fullscreen</button><video></video></div>' : ''); return; }
  const size = fs.statSync(file).size, range = /^bytes=(\d+)-(\d*)$/.exec(req.headers.range || '');
  const start = range ? Number(range[1]) : 0, end = range?.[2] ? Math.min(size-1,Number(range[2])) : size-1;
  if (start >= size || end < start) { res.writeHead(416,{'Content-Range':`bytes */${size}`}); res.end(); return; }
  res.writeHead(range ? 206 : 200, {'Content-Type':chunkMatch?'audio/mp4':name.startsWith('/audio-')?'audio/ogg':name==='/video'?`video/${videoContainer}`:`video/${originalContainer}`,
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
    for (const [name,args] of segmented ? [] : [['video',['-map','0:v:0','-an','-c:v','copy',...(videoContainer==='mp4'?['-movflags','frag_keyframe+empty_moov+default_base_moof']:[]),'-f',videoContainer]],
      ...[1,2].map(index=>['audio-'+index,['-map',`0:${index}`,'-vn','-c:a','copy','-f','ogg']])]) {
      files['/'+name] = path.join(temp,name);
      execFileSync('ffmpeg',['-v','error','-nostdin','-i',fixture,...args,files['/'+name]]);
    }
    if(!segmented){
      const hashes=(file,selector)=>JSON.parse(execFileSync('ffprobe',['-v','error','-select_streams',selector,'-show_packets','-show_data_hash','sha256','-show_entries','packet=data_hash','-of','json',file])).packets.map(packet=>packet.data_hash);
      assert.deepEqual(hashes(files['/video'],'v:0'),hashes(fixture,'v:0'));
      for(const index of [1,2])assert.deepEqual(hashes(files['/audio-'+index],'a:0'),hashes(fixture,`a:${index-1}`));
      report.packetCopyVerified=true;
    }
    await new Promise(resolve=>server.listen(0,'127.0.0.1',resolve));
    let origin=`http://127.0.0.1:${server.address().port}`;
    if(process.env.MEDIA_TEST_PROXY==='1'){
      const config=path.join(temp,'nginx.conf');
      fs.writeFileSync(config,`pid /tmp/nginx.pid; error_log /dev/stderr warn; events { worker_connections 128; } http { access_log off; client_body_temp_path /tmp/body; proxy_temp_path /tmp/proxy; fastcgi_temp_path /tmp/fastcgi; uwsgi_temp_path /tmp/uwsgi; scgi_temp_path /tmp/scgi; server { listen 8080; location / { proxy_pass http://host.docker.internal:${server.address().port}; proxy_read_timeout 10s; proxy_buffering on; } } }`);
      proxyId=execFileSync('docker',['run','-d','--user=10001:10001','--cpus=.25','--memory=64m','--pids-limit=64','--cap-drop=ALL','--security-opt=no-new-privileges','-p','127.0.0.1::8080','--mount',`type=bind,source=${config},target=/config.conf,readonly`,'--entrypoint','nginx','nginx:stable-alpine','-c','/config.conf','-g','daemon off;'],{encoding:'utf8'}).trim();
      const address=execFileSync('docker',['port',proxyId,'8080/tcp'],{encoding:'utf8'}).trim();
      origin='http://'+address;report.proxy={kind:'nginx',buffering:true,readTimeoutSeconds:10,image:execFileSync('docker',['image','inspect','nginx:stable-alpine','--format','{{.Id}}'],{encoding:'utf8'}).trim()};
    }
    browser=await chromium.launch({channel:process.env.MEDIA_BROWSER_CHANNEL || 'chrome',executablePath:process.env.MEDIA_BROWSER_EXECUTABLE || undefined,headless:true,args:['--autoplay-policy=no-user-gesture-required']});
    report.browser=browser.version();
    const page=await browser.newPage(); page.setDefaultTimeout(15000);
    report.console=[];page.on('console',message=>report.console.push(message.text()));
    await page.goto(origin);
    for(const name of ['errors','compatibility','source-selection','external-audio','segmented-audio','native']) {
      await page.addScriptTag({content:`window[${JSON.stringify(name)}]={};(function(exports,require){${compile(name)}\n})(window[${JSON.stringify(name)}],name=>window[name.slice(2)]);`});
    }
    await page.evaluate(({probe,videoContainer,originalContainer})=>{
      window.errors=[];window.videoLoads=0;
      const video=document.querySelector('video');video.addEventListener('loadstart',()=>window.videoLoads++);
      document.querySelector('button').onclick=()=>video.parentElement.requestFullscreen();
      window.media={id:'dual',title:'Synthetic',subtitles:[],sources:[
        {id:'original',audioTrackId:'internal-default',language:'und',url:'/original',mime:`video/${originalContainer}`,tracks:probe.streams},
        ...probe.streams.filter(t=>t.codec_type==='audio').map(t=>({id:'track-'+t.index,audioTrackId:'internal-'+t.index,
          language:t.tags.language,url:'/video',externalAudioUrl:'/audio-'+t.index,mime:`video/${videoContainer}`,prepare:false,tracks:[t]}))]};
      window.openPlayer=async(id,language='fr')=>{
        window.player=new window.native.NativePlayer(video,message=>window.errors.push(message));
        const selected=window['source-selection'].chooseSource(window.media,id,language,s=>Boolean(video.canPlayType(window['source-selection'].playbackMime(window.media,s))));
        await window.player.load(window.media,selected.audioTrackId);await window.player.play();return selected.audioTrackId;
      };
      window.sample=async()=>{
        const audio=document.querySelector('audio'),c=new AudioContext();await c.resume();
        const source=c.createMediaElementSource(audio),analyser=c.createAnalyser();analyser.fftSize=8192;source.connect(analyser);analyser.connect(c.destination);
        await new Promise(r=>setTimeout(r,600));const bins=new Float32Array(analyser.frequencyBinCount);analyser.getFloatFrequencyData(bins);
        let peak=1;for(let i=2;i<bins.length;i++)if(bins[i]>bins[peak])peak=i;
        const result={hz:peak*c.sampleRate/analyser.fftSize,db:bins[peak],drift:Math.abs(audio.currentTime-video.currentTime),
          time:video.currentTime,audioTime:audio.currentTime,audioPaused:audio.paused,audioMuted:audio.muted,audioVolume:audio.volume,audioReady:audio.readyState,audioError:audio.error?.message,buffered:Array.from({length:audio.buffered.length},(_,i)=>[audio.buffered.start(i),audio.buffered.end(i)]),url:video.currentSrc,frames:video.getVideoPlaybackQuality().totalVideoFrames,fullscreen:!!document.fullscreenElement,loads:window.videoLoads};
        source.disconnect();await c.close();return result;
      };
    },{probe,videoContainer,originalContainer});
    if(segmented)await page.evaluate(()=>{
      for(const source of window.media.sources.slice(1)){
        source.url='/original';source.mime=window.media.sources[0].mime;source.prepareVideo=false;source.audioSegmentUrl='/audio-'+source.audioTrackId.split('-')[1]+'-chunk-';
      }
    });
    await page.getByRole('button').click();
    report.capabilities=await page.evaluate(()=>['video/mp4; codecs="av01"','video/mp4','audio/ogg; codecs="opus"','video/x-matroska; codecs="avc1.640028"',...['dtsc','dtsh','dtsl'].flatMap(codec=>['video/x-matroska','audio/mp4'].map(mime=>`${mime}; codecs="${codec}"`))].map(type=>({type,result:document.createElement('video').canPlayType(type)})));
    report.dtsRejection=await page.evaluate(async()=>{
      const video=document.createElement('video'),player=new window.native.NativePlayer(video,()=>{}),started=performance.now();
      let loads=0;video.addEventListener('loadstart',()=>loads++);
      const media={id:'synthetic-dts-descriptor',title:'Capability check only',subtitles:[],sources:[{id:'original',audioTrackId:'internal-default',language:'dut',url:'/must-not-be-requested',mime:'video/x-matroska',tracks:[{index:0,codec_type:'video',codec_name:'h264'},{index:1,codec_type:'audio',codec_name:'dts',profile:'DTS-HD MA',channels:6}]}]};
      let failure;try{await player.load(media);}catch(error){failure=error.message;}player.destroy();
      return {failure,elapsedMs:performance.now()-started,loads,scope:'Descriptor and live browser capability check; not a DTS-HD decoded fixture'};
    });
    assert.match(report.dtsRejection.failure,/DTS-HD MA/);assert.equal(report.dtsRejection.loads,0);assert.ok(report.dtsRejection.elapsedMs<1000);
    report.initial=await page.evaluate(async()=>{
      const started=performance.now(),video=document.querySelector('video');
      const frame=new Promise((resolve,reject)=>{const timeout=setTimeout(()=>reject(new Error('No decoded video frame within 10 seconds')),10000);video.requestVideoFrameCallback(()=>{clearTimeout(timeout);resolve(performance.now()-started);});});
      const id=await window.openPlayer();return {id,firstImageMs:await frame,playReadyMs:performance.now()-started};
    });
    assert.equal(report.initial.id,'internal-1');
    await page.waitForFunction(()=>document.querySelector('video').currentTime>.3);
    let lastTime=0,videoUrl;
    for(const id of ['internal-1','internal-2','internal-1','internal-2','internal-1']) {
      const switchReadyMs=await page.evaluate(async id=>{const started=performance.now();await window.player.selectAudio(id);return performance.now()-started;},id);
      const sample=await page.evaluate(()=>window.sample());
      assert.ok(sample.db>-80,JSON.stringify(sample));assert.ok(Math.abs(sample.hz-(id==='internal-1'?440:990))<25,JSON.stringify(sample));
      assert.ok(sample.drift<.2,JSON.stringify(sample));assert.ok(sample.frames>0);assert.equal(sample.fullscreen,true);
      assert.ok(sample.time>=lastTime-.05);lastTime=sample.time;
      if(videoUrl)assert.equal(sample.url,videoUrl);videoUrl=sample.url;assert.equal(sample.loads,1);
      report.switches.push({id,switchReadyMs,...sample});
    }
    report.pausedSeeks=[];
    if(segmented){
      await page.evaluate(async()=>{window.player.pause();window.player.seek(9.5);await window.player.selectAudio('internal-2');await window.player.play();});
      await page.waitForFunction(()=>{const v=document.querySelector('video'),a=document.querySelector('audio');return v.currentTime>11.2&&!a.paused&&Math.abs(a.currentTime-v.currentTime)<.2;});
      report.boundary=await page.evaluate(()=>window.sample());assert.ok(Math.abs(report.boundary.hz-990)<25);
    }
    for(const target of probe.streams[0].duration>100 ? [8,900,1800,Number(probe.streams[0].duration)-20,2] : [8,2]) {
      await page.evaluate(target=>{window.player.pause();window.player.seek(target);},target);
      await page.waitForFunction(target=>{const v=document.querySelector('video'),a=document.querySelector('audio');return !v.seeking&&!a.seeking&&a.paused&&v.paused&&Math.abs(v.currentTime-target)<.05&&Math.abs(a.currentTime-target)<.1;},target);
      await page.evaluate(()=>window.player.selectAudio('internal-2'));
      assert.ok(await page.evaluate(target=>window.player.state().paused&&Math.abs(window.player.state().position-target)<.05,target));
      await page.evaluate(()=>window.player.play());
      await page.waitForFunction(target=>document.querySelector('video').currentTime>target+.2,target);
      report.pausedSeeks.push({target,passed:true});
    }
    // Real failed candidate leaves the previous track recoverable.
    report.failedCandidate=await page.evaluate(async segmented=>{
      const saved=window.media.sources[1].externalAudioUrl;window.media.sources[1].externalAudioUrl='/missing';window.media.sources[1].prepare=false;
      const savedSegment=window.media.sources[1].audioSegmentUrl;if(segmented)window.media.sources[1].audioSegmentUrl='/missing-chunk-';
      let failure;try{await window.player.selectAudio('internal-1');}catch(e){failure=e.message;}
      window.media.sources[1].externalAudioUrl=saved;window.media.sources[1].prepare=true;
      window.media.sources[1].audioSegmentUrl=savedSegment;
      const resumed=!document.querySelector('video').paused&&!document.querySelector('audio').paused;
      await window.player.selectAudio('internal-2');
      return {failure,resumed,url:document.querySelector('audio').currentSrc,count:document.querySelectorAll('audio').length};
    },segmented);
    assert.ok(report.failedCandidate.failure);assert.match(report.failedCandidate.url,segmented?/^blob:/:/audio-2$/);assert.equal(report.failedCandidate.count,1);
    assert.equal(report.failedCandidate.resumed,true,'Retained audio must resume without another Play command');
    report.reopened=[];
    for(const id of ['internal-2','internal-1']) {
      await page.evaluate(()=>window.player.destroy());
      const warm=await page.evaluate(async id=>{const started=performance.now();const selected=await window.openPlayer(id,id==='internal-1'?'fra':'eng');return {id:selected,playReadyMs:performance.now()-started};},id);
      assert.equal(warm.id,id);
      await page.waitForFunction(()=>document.querySelector('video').currentTime>.2);
      const sample=await page.evaluate(()=>window.sample());assert.ok(Math.abs(sample.hz-(id==='internal-1'?440:990))<25);
      report.reopened.push({...warm,...sample});
    }
    report.errors=await page.evaluate(()=>window.errors);assert.deepEqual(report.errors,[]);
    await page.evaluate(()=>window.player.destroy());
    if(segmented){
      report.cancellation=await page.evaluate(async()=>{
        const video=document.querySelector('video'),player=new window.native.NativePlayer(video,()=>{});
        const media={...window.media,sources:[{...window.media.sources[1],audioSegmentUrl:'/delayed-chunk-'}]};
        const start=performance.now();const loading=player.load(media,'internal-1').then(()=>false,()=>true);
        while(!(await(await fetch('/test-state')).json()).delayedRequests){if(performance.now()-start>5000)throw new Error('Delayed test fetch was not started');await new Promise(resolve=>setTimeout(resolve,20));}
        const cancelling=performance.now();player.destroy();
        return {rejected:await loading,loadBeforeCancelMs:cancelling-start,elapsedMs:performance.now()-cancelling,audios:document.querySelectorAll('audio').length};
      });
      assert.equal(report.cancellation.rejected,true);assert.equal(report.cancellation.audios,0);assert.ok(report.cancellation.elapsedMs<1500);
    }
    report.passed=true;
  } catch(error) {report.failure=String(error.stack);process.exitCode=1;}
  finally {
    if(browser)await browser.close();await new Promise(resolve=>server.close(resolve));
    if(proxyId){try{const logs=spawnSync('docker',['logs',proxyId],{encoding:'utf8'});report.proxyLogs=(logs.stdout||'')+(logs.stderr||'');}catch{}try{execFileSync('docker',['stop','-t','1',proxyId],{stdio:'ignore'});}catch{}try{execFileSync('docker',['rm',proxyId],{stdio:'ignore'});}catch{}}
    await Promise.allSettled([...jobs.values()]);for(const name of fs.readdirSync(temp))fs.unlinkSync(path.join(temp,name));fs.rmdirSync(temp);
    const out=process.env.MEDIA_REPORT_DIR||'target/dual-opus-browser';fs.mkdirSync(out,{recursive:true});
    fs.writeFileSync(path.join(out,'report.json'),JSON.stringify(report,null,2)+'\n');console.log(JSON.stringify(report,null,2));
  }
})();
