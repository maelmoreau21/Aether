// Integration experiment of the production NativePlayer fallback. Its HTTP
// harness is not the Rust Node. Audio windows are explicitly prepared by the
// video-window POC; first image/audio timings here are diagnostic, not cold NAS.
const fs=require('node:fs'),path=require('node:path'),http=require('node:http');
const {spawn,execFileSync}=require('node:child_process');
const {createHash}=require('node:crypto');
const os=require('node:os');
const {chromium}=require('../apps/web/node_modules/playwright');
const ts=require('../apps/web/node_modules/typescript');
const out=path.resolve('target/streaming-video-window-poc');
if(!['gop15.mkv','gop15.mp4','mkv-audio-3.mp4','mp4-audio-3.mp4'].every(file=>fs.existsSync(path.join(out,file))))execFileSync(process.execPath,[path.resolve('scripts/browser-video-window-poc.cjs')],{env:{...process.env,MEDIA_VIDEO_POC_PREPARE_ONLY:'1'},stdio:'inherit'});
const hash=bytes=>createHash('sha256').update(bytes).digest('hex');
const report={scope:'Production NativePlayer + ProgressiveVideo, synthetic GOP15, localhost HTTP harness, preprepared audio',purpose:'Decode correctness, buffering, cancellation and seek; not a startup speed comparison',cacheContract:'Audio windows prepared before playback; OS cache uncontrolled; fixture hashes/probes collected after playback',provenance:{startedAt:new Date().toISOString(),platform:`${os.platform()} ${os.release()}`,nodeVersion:process.version,ffmpegVersion:execFileSync('ffmpeg',['-version'],{encoding:'utf8'}).split('\n')[0],scriptSha256:hash(fs.readFileSync(__filename)),modules:{},fixtures:[]},streams:[],formats:[]};
const server=http.createServer((req,res)=>{
  const url=new URL(req.url,'http://localhost').pathname;
  if(url==='/'){res.writeHead(200,{'Content-Type':'text/html'});res.end('<video playsinline></video>');return;}
  const stream=/^\/(mkv|mp4)\/stream-(\d+)$/.exec(url);
  if(stream){
    const record={format:stream[1],milliseconds:Number(stream[2]),bytes:0};report.streams.push(record);
    const child=spawn('ffmpeg',['-v','error','-nostdin','-copyts','-start_at_zero','-ss',String(Number(stream[2])/1000),'-i',path.join(out,`gop15.${stream[1]}`),'-map','0:v:0','-an','-sn','-c:v','copy','-movflags','frag_keyframe+empty_moov+default_base_moof+delay_moov','-frag_duration','1000000','-flush_packets','1','-f','mp4','pipe:1'],{stdio:['ignore','pipe','pipe']});
    child.stdout.on('data',chunk=>{record.bytes+=chunk.length;});res.writeHead(200,{'Content-Type':'video/mp4'});child.stdout.pipe(res);res.on('close',()=>{record.cancelled=!res.writableEnded;child.kill();});return;
  }
  const audio=/^\/(mkv|mp4)\/audio-1-chunk-(\d+)$/.exec(url);
  if(audio){const file=path.join(out,`${audio[1]}-audio-${audio[2]}.mp4`);if(fs.existsSync(file)){res.writeHead(200,{'Content-Type':'audio/mp4','Content-Length':fs.statSync(file).size});fs.createReadStream(file).pipe(res);return;}}
  res.writeHead(404);res.end();
});
(async()=>{
  let browser;
  try{
    await new Promise(resolve=>server.listen(0,'127.0.0.1',resolve));browser=await chromium.launch({channel:'chrome',headless:true,args:['--autoplay-policy=no-user-gesture-required']});report.provenance.browserVersion=browser.version();
    for(const format of ['mkv','mp4']){
      const page=await browser.newPage();await page.goto(`http://127.0.0.1:${server.address().port}/`);
      for(const name of ['startup-buffer','errors','compatibility','source-selection','external-audio','segmented-audio','mp4-timeline','mp4-fragments','encoded-video-buffer','progressive-video','native']){
        const source=fs.readFileSync(path.resolve('apps/web/src/lib/player',`${name}.ts`),'utf8');
        const code=ts.transpileModule(source,{compilerOptions:{target:ts.ScriptTarget.ES2022,module:ts.ModuleKind.CommonJS}}).outputText;
        const sourceSha256=hash(source),transpiledSha256=hash(code);
        if(report.provenance.modules[name]&&report.provenance.modules[name].sourceSha256!==sourceSha256)throw new Error(`Player module changed during POC: ${name}`);
        report.provenance.modules[name]={sourceSha256,transpiledSha256};
        await page.addScriptTag({content:`window[${JSON.stringify(name)}]={};(function(exports,require){${code}})(window[${JSON.stringify(name)}],name=>window[name.slice(2)]);`});
      }
      const result={format};report.formats.push(result);
      result.browser=await page.evaluate(async format=>{
        const video=document.querySelector('video'),errors=[],events=[],frames=[],pcm=[];
        window.pocEvents=events;window.pocErrors=errors;
        let lastFrame=0;function frame(){video.requestVideoFrameCallback((now,meta)=>{frames.push({time:meta.mediaTime,gap:lastFrame?now-lastFrame:0});lastFrame=now;frame();});}frame();
        for(const type of ['waiting','stalled','playing','seeking','seeked'])video.addEventListener(type,()=>events.push({type,time:video.currentTime,at:performance.now()}));
        const player=new window.native.NativePlayer(video,message=>errors.push(message));window.player=player;
        const source={id:'fr',audioTrackId:'fr',language:'fr',url:'/unsupported-original',mime:'video/x-matroska',prepare:true,prepareVideo:false,videoStreamUrl:`/${format}/stream-`,videoMime:'video/mp4; codecs="av01.0.00M.10"',durationSeconds:34,externalAudioUrl:'/unused',audioSegmentUrl:`/${format}/audio-1-chunk-`};
        await player.load({id:'poc',title:'GOP15',sources:[source],subtitles:[]});await player.play();
        const audio=document.querySelector('audio'),ctx=new AudioContext(),node=ctx.createMediaElementSource(audio),analyser=ctx.createAnalyser();node.connect(analyser);analyser.connect(ctx.destination);await ctx.resume();
        const samples=new Float32Array(1024),poll=setInterval(()=>{analyser.getFloatTimeDomainData(samples);pcm.push({time:video.currentTime,audio:audio.currentTime,peak:Math.max(...samples.map(Math.abs))});},20);
        const seek=async target=>{const began=performance.now();player.seek(target);await new Promise((resolve,reject)=>{const timer=setTimeout(()=>reject(new Error(`Seek ${target} timeout`)),10000);video.addEventListener('seeked',()=>{clearTimeout(timer);resolve();},{once:true});});return {target,elapsedMs:performance.now()-began,position:video.currentTime};};
        const seeks=[];seeks.push(await seek(31));seeks.push(await seek(2));
        const playbackStart=performance.now();
        await new Promise((resolve,reject)=>{const timer=setTimeout(()=>reject(new Error('Progressive playback timeout')),35000),check=setInterval(()=>{if(video.currentTime>=31){clearInterval(check);clearTimeout(timer);resolve();}},30);});
        clearInterval(poll);const boundaries=[10,20,30].map(time=>({time,frames:frames.filter(f=>Math.abs(f.time-time)<.3),pcm:pcm.filter(s=>Math.abs(s.time-time)<.15),events:events.filter(e=>e.at>=playbackStart&&Math.abs(e.time-time)<.3)}));
        const result={errors,seeks,boundaries,frames:frames.length,videoSourceScheme:video.currentSrc.startsWith('blob:')?'blob':'other',audioTrackId:audio.dataset.internalAudioTrack,allEvents:events};player.destroy();await ctx.close();return result;
      },format);
      if(result.browser.errors.length||result.browser.boundaries.some(b=>!b.frames.length||b.frames.some(f=>f.gap>350)||!b.pcm.some(s=>s.peak>.01&&Math.abs(s.time-s.audio)<.15)||b.events.some(e=>e.type==='waiting'||e.type==='stalled')))throw new Error(`Progressive boundary proof failed: ${format}`);
      await page.close();
    }
  }catch(error){report.error=String(error.stack);const page=browser?.contexts().at(-1)?.pages().at(-1);if(page)report.diagnostic=await page.evaluate(()=>({events:window.pocEvents,errors:window.pocErrors,video:{time:document.querySelector('video')?.currentTime,paused:document.querySelector('video')?.paused,seeking:document.querySelector('video')?.seeking,ready:document.querySelector('video')?.readyState,error:document.querySelector('video')?.error?.message,ranges:Array.from({length:document.querySelector('video')?.buffered.length??0},(_,i)=>[document.querySelector('video').buffered.start(i),document.querySelector('video').buffered.end(i)])},progressive:window.player?.progressiveVideo&&{closed:window.player.progressiveVideo.closed,active:window.player.progressiveVideo.active,failed:window.player.progressiveVideo.failed?.message,epoch:window.player.progressiveVideo.epoch}}));process.exitCode=1;}
  finally{
    if(browser)await browser.close();await new Promise(resolve=>server.close(resolve));
    for(const format of ['mkv','mp4']){const file=path.join(out,`gop15.${format}`);report.provenance.fixtures.push({path:`target/streaming-video-window-poc/gop15.${format}`,sha256:hash(fs.readFileSync(file)),probe:JSON.parse(execFileSync('ffprobe',['-v','error','-show_entries','format=duration,size:stream=index,codec_name,profile,width,height,pix_fmt,r_frame_rate,sample_rate,channels','-of','json',file]))});}
    report.provenance.finishedAt=new Date().toISOString();fs.writeFileSync(path.join(out,'native-progressive-report.json'),JSON.stringify(report,null,2));console.log(JSON.stringify(report,null,2));
  }
})();
