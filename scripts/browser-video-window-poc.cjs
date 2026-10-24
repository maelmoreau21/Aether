// Research only: preprepared AV1-copy video windows with GOPs longer than a
// nominal window, plus production SegmentedAudio. No cold-start claim.
const fs = require('node:fs');
const path = require('node:path');
const http = require('node:http');
const {execFileSync,spawn} = require('node:child_process');
const {chromium} = require('../apps/web/node_modules/playwright');
const ts = require('../apps/web/node_modules/typescript');
const out = path.resolve('target/streaming-video-window-poc');
const streaming = process.env.MEDIA_VIDEO_POC_STREAM === '1';
fs.mkdirSync(out,{recursive:true});
const mkv=path.join(out,'gop15.mkv'), mp4=path.join(out,'gop15.mp4');
const ffmpeg=args=>execFileSync('ffmpeg',['-v','error','-nostdin','-y',...args]);
if(!fs.existsSync(mkv))ffmpeg(['-f','lavfi','-i','testsrc2=size=320x180:rate=12','-f','lavfi','-i','sine=frequency=440:sample_rate=48000','-t','34','-c:v','libaom-av1','-cpu-used','8','-crf','40','-b:v','0','-pix_fmt','yuv420p10le','-g','180','-keyint_min','180','-c:a','libopus',mkv]);
ffmpeg(['-i',mkv,'-map','0','-c','copy',mp4]);
const report={scope:streaming?'Continuous FFmpeg stdout over localhost HTTP, preprepared audio; not Rust Node or cold startup proof':'Preprepared window boundary/seek experiment only, localhost, not Node or cold startup proof',streaming,formats:[]};
const files=new Map();
const offsets=new Map();
for(const [format,file] of [['mkv',mkv],['mp4',mp4]]){
  const result={format,windows:[]};report.formats.push(result);
  for(let n=0;n<4;n++){
    const video=path.join(out,`${format}-video-${n}.mp4`);
    ffmpeg(['-copyts','-start_at_zero','-ss',String(n*10),'-i',file,'-map','0:v:0','-an','-sn','-c:v','copy','-to',String((n+1)*10),'-movflags','frag_keyframe+empty_moov+default_base_moof+delay_moov','-frag_duration','1000000','-f','mp4',video]);
    const packets=JSON.parse(execFileSync('ffprobe',['-v','error','-show_packets','-select_streams','v:0','-show_entries','packet=pts_time,dts_time,flags','-of','json',video])).packets;
    result.windows.push({n,first:packets[0],last:packets.at(-1),bytes:fs.statSync(video).size});
    files.set(`/${format}/video-${n}`,video);
    offsets.set(`/${format}/video-${n}`,Number(packets[0].pts_time));
    const audio=path.join(out,`${format}-audio-${n}.mp4`);
    ffmpeg(['-ss',String(Math.max(0,n*10-1)),'-i',file,'-ss','0','-map','0:a:0','-vn','-sn','-c:a','copy','-t','12','-avoid_negative_ts','make_zero','-movflags','frag_keyframe+empty_moov+default_base_moof','-frag_duration','1000000','-f','mp4',audio]);
    files.set(`/${format}/audio-1-chunk-${n}`,audio);
  }
}
if(process.env.MEDIA_VIDEO_POC_PREPARE_ONLY==='1'){console.log(JSON.stringify({scope:'Synthetic fixture and explicit audio/video preparation only',directory:out}));process.exit(0);}
const server=http.createServer((req,res)=>{
  const stream=/^\/(mkv|mp4)\/stream-(\d+)$/.exec(new URL(req.url,'http://localhost').pathname);
  if(stream){
    const process=spawn('ffmpeg',['-v','error','-nostdin','-copyts','-start_at_zero','-ss',stream[2],'-i',stream[1]==='mkv'?mkv:mp4,'-map','0:v:0','-an','-sn','-c:v','copy','-movflags','frag_keyframe+empty_moov+default_base_moof+delay_moov','-frag_duration','1000000','-flush_packets','1','-f','mp4','pipe:1'],{stdio:['ignore','pipe','pipe']});
    res.writeHead(200,{'Content-Type':'video/mp4'});process.stdout.pipe(res);res.on('close',()=>process.kill());return;
  }
  const file=files.get(new URL(req.url,'http://localhost').pathname);
  if(!file){res.writeHead(200,{'Content-Type':'text/html'});res.end('<video playsinline></video>');return;}
  res.writeHead(200,{'Content-Type':'video/mp4','Content-Length':fs.statSync(file).size,'X-Media-Timeline-Offset':String(offsets.get(new URL(req.url,'http://localhost').pathname)??0)});fs.createReadStream(file).pipe(res);
});
(async()=>{
  let browser;
  try{
    await new Promise(resolve=>server.listen(0,'127.0.0.1',resolve));
    browser=await chromium.launch({channel:'chrome',headless:true,args:['--autoplay-policy=no-user-gesture-required']});
    for(const result of report.formats){
      const page=await browser.newPage();await page.goto(`http://127.0.0.1:${server.address().port}/`);
      for(const name of ['startup-buffer','external-audio','segmented-audio','mp4-timeline']){
        const code=ts.transpileModule(fs.readFileSync(path.resolve('apps/web/src/lib/player',`${name}.ts`),'utf8'),{compilerOptions:{target:ts.ScriptTarget.ES2022,module:ts.ModuleKind.CommonJS}}).outputText;
        await page.addScriptTag({content:`window[${JSON.stringify(name)}]={};(function(exports,require){${code}})(window[${JSON.stringify(name)}],name=>window[name.slice(2)]);`});
      }
      result.browser=await page.evaluate(async ({format,streaming})=>{
        const video=document.querySelector('video'),source=new MediaSource(),events=[],frames=[],appends=[],errors=[];
        let priorFrame=0;
        function frame(){video.requestVideoFrameCallback((now,meta)=>{frames.push({time:meta.mediaTime,gap:priorFrame?now-priorFrame:0});priorFrame=now;frame();});}frame();
        for(const type of ['waiting','stalled','playing','seeking','seeked'])video.addEventListener(type,()=>events.push({type,time:video.currentTime,at:performance.now()}));
        video.src=URL.createObjectURL(source);
        await new Promise(resolve=>source.addEventListener('sourceopen',resolve,{once:true}));
        const buffer=source.addSourceBuffer('video/mp4; codecs="av01.0.00M.10"');source.duration=34;
        const loaded=new Set();let appending=false;
        async function append(n){
          if(loaded.has(n)||appending||n>3)return;appending=true;
          try{
            const response=await fetch(`/${format}/video-${n}`),data=await response.arrayBuffer();
            if(n>0)buffer.abort();
            buffer.timestampOffset=Number(response.headers.get('x-media-timeline-offset'));
            await new Promise((resolve,reject)=>{buffer.addEventListener('updateend',resolve,{once:true});buffer.addEventListener('error',reject,{once:true});buffer.appendBuffer(data);});
            loaded.add(n);appends.push({n,time:video.currentTime,ranges:Array.from({length:buffer.buffered.length},(_,i)=>[buffer.buffered.start(i),buffer.buffered.end(i)])});
          }finally{appending=false;}
        }
        let pump;
        if(streaming){
          const response=await fetch(`/${format}/stream-0`),reader=response.body.getReader();
          let initialized=false,prefix=new Uint8Array();
          const appended=bytes=>new Promise((resolve,reject)=>{buffer.addEventListener('updateend',resolve,{once:true});buffer.addEventListener('error',reject,{once:true});buffer.appendBuffer(bytes);});
          let ready;const first=new Promise(resolve=>{ready=resolve;});
          pump=(async()=>{while(true){const {done,value}=await reader.read();if(done)break;
            if(!initialized){const bytes=new Uint8Array(prefix.length+value.length);bytes.set(prefix);bytes.set(value,prefix.length);prefix=bytes;if(prefix.length>1024*1024)throw new Error('Init MP4 exceeds 1 MiB');
              let cursor=0,end=0;const view=new DataView(prefix.buffer);while(cursor+8<=prefix.length){const size=view.getUint32(cursor),type=String.fromCharCode(...prefix.subarray(cursor+4,cursor+8));if(size<8||cursor+size>prefix.length)break;if(type==='moov'){end=cursor+size;break;}cursor+=size;}
              if(!end)continue;buffer.timestampOffset=window['mp4-timeline'].mp4TimelineOffset(prefix.buffer.slice(0,end));initialized=true;await appended(prefix);ready();prefix=new Uint8Array();
            }else await appended(value);
          }source.endOfStream();})();
          void pump.catch(e=>errors.push(String(e)));await first;
        }else await append(0);
        const audio=new window['segmented-audio'].SegmentedAudio(video,0,message=>errors.push(message));
        await audio.load(`/${format}/audio-1-chunk-`);await audio.bufferForPlayback();await video.play();await audio.play();
        const interval=setInterval(()=>{if(streaming)return;const next=Math.floor(video.currentTime/10)+1;if(next<=3&&video.currentTime>=next*10-4)void append(next).catch(e=>errors.push(String(e)));},30);
        const ctx=new AudioContext(),node=ctx.createMediaElementSource(document.querySelector('audio')),analyser=ctx.createAnalyser();node.connect(analyser);analyser.connect(ctx.destination);await ctx.resume();
        const samples=new Float32Array(1024),sync=[];
        const pcm=setInterval(()=>{analyser.getFloatTimeDomainData(samples);sync.push({time:video.currentTime,audio:document.querySelector('audio').currentTime,peak:Math.max(...samples.map(Math.abs))});},20);
        let playbackError;
        try{await new Promise((resolve,reject)=>{const timeout=setTimeout(()=>reject(new Error('Boundary playback timed out')),35000);const check=setInterval(()=>{if(video.currentTime>=31){clearTimeout(timeout);clearInterval(check);resolve();}},50);});}catch(error){playbackError=String(error);}
        clearInterval(interval);clearInterval(pcm);video.pause();audio.pause();
        if(playbackError){const a=document.querySelector('audio');const result={playbackError,errors,events,appends,video:{time:video.currentTime,ready:video.readyState,seeking:video.seeking,error:video.error?.message,ranges:Array.from({length:video.buffered.length},(_,i)=>[video.buffered.start(i),video.buffered.end(i)])},audio:{time:a.currentTime,ready:a.readyState,seeking:a.seeking,error:a.error?.message,ranges:Array.from({length:a.buffered.length},(_,i)=>[a.buffered.start(i),a.buffered.end(i)])},lastFrames:frames.slice(-10)};audio.destroy();await ctx.close();return result;}
        const seeks=[];
        for(const target of [24,2,31]){
          const start=performance.now();video.currentTime=target;
          await new Promise(resolve=>video.addEventListener('seeked',resolve,{once:true}));
          await audio.bufferForPlayback();await video.play();await audio.play();
          await new Promise(resolve=>video.requestVideoFrameCallback(()=>resolve()));
          seeks.push({target,elapsedMs:performance.now()-start,position:video.currentTime,audio:document.querySelector('audio').currentTime});video.pause();audio.pause();
        }
        const boundaries=[10,20,30].map(time=>({time,frames:frames.filter(f=>Math.abs(f.time-time)<.3),pcm:sync.filter(s=>Math.abs(s.time-time)<.15),events:events.filter(e=>Math.abs(e.time-time)<.3)}));
        audio.destroy();await ctx.close();
        return {errors,boundaries,appends,seeks,frames:frames.length,maxFrameGapMs:Math.max(...frames.filter(f=>f.time>1&&f.time<31).map(f=>f.gap)),events};
      },{format:result.format,streaming});
      if(result.browser.playbackError||result.browser.errors.length||result.browser.boundaries.some(b=>!b.frames.length||b.frames.some(f=>f.gap>350)||!b.pcm.some(s=>s.peak>.01&&Math.abs(s.time-s.audio)<.15)||b.events.some(e=>e.type==='waiting'||e.type==='stalled')))throw new Error(`Boundary proof failed: ${result.format}`);
      await page.close();
    }
  }catch(error){report.error=String(error.stack);process.exitCode=1;}
  finally{if(browser)await browser.close();await new Promise(resolve=>server.close(resolve));fs.writeFileSync(path.join(out,'report.json'),JSON.stringify(report,null,2));console.log(JSON.stringify(report,null,2));}
})();
