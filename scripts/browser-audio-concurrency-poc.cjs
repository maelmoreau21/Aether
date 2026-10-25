// Diagnostic only: production TS player in four real Chrome contexts. HTTP
// fixture/packet-copy harness, not the authenticated Rust system benchmark.
const fs=require('node:fs'),path=require('node:path'),os=require('node:os'),http=require('node:http');
const {execFileSync,execFile}=require('node:child_process'),{promisify}=require('node:util');
const {createHash}=require('node:crypto');
const {chromium}=require('../apps/web/node_modules/playwright'),ts=require('../apps/web/node_modules/typescript');
const run=promisify(execFile),fixture=path.resolve('tests/fixtures/generated/dual-opus/dual-opus-long.mp4');
const out=path.resolve(process.env.MEDIA_AUDIO_POC_OUT||'target/audio-concurrency-poc'),temp=fs.mkdtempSync(path.join(os.tmpdir(),'media-audio-concurrency-'));
const rounds=Number(process.env.MEDIA_AUDIO_POC_ROUNDS||3),report={scope:'Four real Chrome contexts, production NativePlayer modules, synthetic localhost HTTP harness; no Rust/physical NAS proof',rounds:[],requests:[]},jobs=new Map();
const nativeInput=process.env.MEDIA_AUDIO_POC_NATIVE_DIR?path.resolve(process.env.MEDIA_AUDIO_POC_NATIVE_DIR):undefined;
const waitResume=process.env.MEDIA_AUDIO_POC_AWAIT_CONTEXT==='1';report.audioContextResumeAwaited=waitResume;
const rapid=process.env.MEDIA_AUDIO_POC_RAPID==='1',delay90=rapid?500:0;report.controlledChunk90DelayMs=delay90;
const delay0=Number(process.env.MEDIA_AUDIO_POC_DELAY_CHUNK0_MS||0),saved=Number(process.env.MEDIA_AUDIO_POC_SAVED_POSITION||0),baseline=process.env.MEDIA_AUDIO_POC_BASELINE==='1';
const baselineCommit=execFileSync('git',['rev-parse','HEAD'],{encoding:'utf8'}).trim(),hash=bytes=>createHash('sha256').update(bytes).digest('hex');
report.controlledChunk0DelayMs=delay0;report.savedPosition=saved;report.provenance={baselineExternalAudio:baseline,baselineCommit,startedAt:new Date().toISOString(),modules:{},nativeExportProvenanceSha256:nativeInput?hash(fs.readFileSync(path.join(nativeInput,'provenance.json'))):undefined};
report.audioMode=nativeInput?'absolute native Rust response exports':'rebased FFmpeg packet-copy windows';
const probe=JSON.parse(execFileSync('ffprobe',['-v','error','-show_streams','-show_format','-of','json',fixture]));
const server=http.createServer(async(req,res)=>{
  const route=new URL(req.url,'http://localhost').pathname;
  if(route==='/'){res.writeHead(200,{'Content-Type':'text/html'});res.end('<video playsinline preload="auto"></video>');return;}
  let file=fixture;const match=/^\/audio-(\d+)-chunk-(\d+)$/.exec(route);
  if(match){
    if(Number(match[2])===90&&delay90)await new Promise(resolve=>setTimeout(resolve,delay90));
    if(Number(match[2])===0&&delay0)await new Promise(resolve=>setTimeout(resolve,delay0));
    if(nativeInput){const track=probe.streams.find(s=>s.index===Number(match[1]));file=path.join(nativeInput,`${Number.parseInt(track.id)}-${Number(match[2])}.mp4`);if(!fs.existsSync(file)){res.writeHead(404);res.end();return;}}
    else {
    file=path.join(temp,route.slice(1));
    if(!jobs.has(route))jobs.set(route,run('ffmpeg',['-v','error','-nostdin','-ss',String(Math.max(0,Number(match[2])*10-1)),'-i',fixture,'-ss','0','-map',`0:${match[1]}`,'-vn','-sn','-c:a','copy','-t','12','-avoid_negative_ts','make_zero','-movflags','frag_keyframe+empty_moov+default_base_moof','-frag_duration','1000000','-f','mp4',file]));
    try{await jobs.get(route);}catch{res.writeHead(503);res.end();return;}
    }
  }else if(route!=='/original'){res.writeHead(404);res.end();return;}
  const size=fs.statSync(file).size,range=/^bytes=(\d+)-(\d*)$/.exec(req.headers.range||''),start=range?Number(range[1]):0,end=range?.[2]?Math.min(size-1,Number(range[2])):size-1;
  if(start>=size||start>end){res.writeHead(416);res.end();return;}
  report.requests.push({route,start,end});res.writeHead(range?206:200,{'Content-Type':match?'audio/mp4':'video/mp4','Content-Length':end-start+1,'Accept-Ranges':'bytes',...(range?{'Content-Range':`bytes ${start}-${end}/${size}`}:{})});
  const stream=fs.createReadStream(file,{start,end,highWaterMark:65536});res.on('close',()=>stream.destroy());stream.pipe(res);
});
const modules=['startup-buffer','errors','compatibility','source-selection','external-audio','segmented-audio','mp4-timeline','mp4-fragments','encoded-video-buffer','progressive-video','native'];
(async()=>{
  let browser;
  try{
    await new Promise(resolve=>server.listen(0,'127.0.0.1',resolve));browser=await chromium.launch({channel:'chrome',headless:true,args:['--autoplay-policy=no-user-gesture-required']});report.chrome=browser.version();
    for(let round=0;round<rounds;round++){
      const pages=[];
      for(let i=0;i<4;i++){
        const context=await browser.newContext(),page=await context.newPage();pages.push({context,page});await page.goto(`http://127.0.0.1:${server.address().port}/`);
        await page.evaluate(()=>{
          window.audioHistory=[];window.audioElements=[];window.mseHistory=[];const NativeAudio=window.Audio;
          const snapshot=a=>({position:a.currentTime,ready:a.readyState,network:a.networkState,seeking:a.seeking,paused:a.paused,duration:Number.isFinite(a.duration)?a.duration:null,track:a.dataset.internalAudioTrack,ranges:Array.from({length:a.buffered.length},(_,i)=>[a.buffered.start(i),a.buffered.end(i)])});
          window.Audio=function(...args){const a=new NativeAudio(...args),id=window.audioElements.length;window.audioElements.push(a);
            for(const type of ['loadstart','loadedmetadata','loadeddata','canplay','seeking','seeked','waiting','playing','error','suspend','stalled','emptied','durationchange','abort'])a.addEventListener(type,()=>window.audioHistory.push({id,type,at:performance.now(),...snapshot(a)}));
            const remove=a.remove.bind(a);a.remove=()=>{window.audioHistory.push({id,type:'before-remove',at:performance.now(),...snapshot(a)});remove();};return a;};window.Audio.prototype=NativeAudio.prototype;
          const removeAttribute=Element.prototype.removeAttribute;Element.prototype.removeAttribute=function(name){if(this instanceof HTMLAudioElement&&name==='src')window.audioHistory.push({id:window.audioElements.indexOf(this),type:'before-remove-src',at:performance.now(),...snapshot(this)});return removeAttribute.call(this,name);};
          const add=MediaSource.prototype.addSourceBuffer;
          MediaSource.prototype.addSourceBuffer=function(...args){const sb=add.apply(this,args),id=window.mseHistory.length;
            sb.addEventListener('updateend',()=>window.mseHistory.push({id,type:'updateend',at:performance.now(),ranges:Array.from({length:sb.buffered.length},(_,i)=>[sb.buffered.start(i),sb.buffered.end(i)])}));sb.addEventListener('error',()=>window.mseHistory.push({id,type:'error',at:performance.now()}));return sb;};
          window.snapshot=snapshot;
          let ctx;const graphs=new Map(),context=()=>{ctx??=new AudioContext();window.audioContext=ctx;return ctx;};window.startAudioContext=()=>context().resume();
          new MutationObserver(()=>{for(const a of document.querySelectorAll('audio'))if(!graphs.has(a)){const source=context().createMediaElementSource(a),analyser=context().createAnalyser();source.connect(analyser);analyser.connect(context().destination);graphs.set(a,{source,analyser});}for(const[a,g]of graphs)if(!a.isConnected){g.source.disconnect();g.analyser.disconnect();graphs.delete(a);}}).observe(document.body,{childList:true,subtree:true});
        });
        for(const name of modules){const source=baseline&&name==='external-audio'?execFileSync('git',['show',`${baselineCommit}:apps/web/src/lib/player/external-audio.ts`],{encoding:'utf8'}):fs.readFileSync(path.resolve('apps/web/src/lib/player',name+'.ts'),'utf8');const code=ts.transpileModule(source,{compilerOptions:{target:ts.ScriptTarget.ES2022,module:ts.ModuleKind.CommonJS}}).outputText;
          const sha=hash(source);if(report.provenance.modules[name]&&report.provenance.modules[name].sourceSha256!==sha)throw new Error('Module changed during POC: '+name);report.provenance.modules[name]={sourceSha256:sha,transpiledSha256:hash(code)};
          await page.addScriptTag({content:`window[${JSON.stringify(name)}]={};(function(exports,require){${code}})(window[${JSON.stringify(name)}],name=>window[name.slice(2)]);`});}
        await page.evaluate(({probe,native,waitResume,saved})=>{
          window.errors=[];const video=document.querySelector('video');window.player=new window.native.NativePlayer(video,message=>window.errors.push(message));
          const media={id:'poc',title:'Synthetic',subtitles:[],sources:probe.streams.filter(s=>s.codec_type==='audio').map(s=>({id:`track-${s.index}`,audioTrackId:`internal-${s.index}`,language:s.tags?.language||'und',url:'/original',mime:'video/mp4',externalAudioUrl:'/unused',audioSegmentUrl:`/audio-${s.index}-chunk-`,durationSeconds:Number(probe.format.duration),audioSegmentMode:native?'absolute':'rebased'}))};
          window.act=async(kind,target)=>{const resumed=window.startAudioContext();if(waitResume)await resumed;if(kind==='load'){await window.player.load(media,'internal-1',saved);window.player.setAudioOffset(0);await window.player.play();}else if(kind==='switch')await window.player.selectAudio(target);else{
            if(kind==='rapid-seek'){window.player.seek(900);await new Promise((resolve,reject)=>{const timeout=setTimeout(()=>reject(new Error('Pending seek precondition missing')),3000),poll=setInterval(()=>{const audio=document.querySelector('audio');if(audio?.seeking&&audio.currentTime>=899){clearTimeout(timeout);clearInterval(poll);window.rapidSnapshot={audio:window.snapshot(audio),video:window.snapshot(video)};resolve();}},5);});}
            window.player.seek(target);await new Promise((resolve,reject)=>{const timeout=setTimeout(()=>reject(new Error('Seek oracle timeout')),12000),poll=setInterval(()=>{if(window.errors.length){clearTimeout(timeout);clearInterval(poll);reject(new Error(window.errors.at(-1)));}else{const a=[...document.querySelectorAll('audio')].find(a=>!a.paused);if(a&&!video.paused&&!video.seeking&&!a.seeking&&Math.abs(video.currentTime-target)<1&&Math.abs(a.currentTime-video.currentTime)<.15){clearTimeout(timeout);clearInterval(poll);resolve();}}},20);});}
            if(window.errors.length)throw new Error(window.errors.at(-1));return {kind,target,rapidSnapshot:window.rapidSnapshot,video:window.snapshot(video),audio:[...document.querySelectorAll('audio')].map(window.snapshot)};};
        },{probe,native:!!nativeInput,waitResume,saved});
      }
      const result={round,actions:[]};report.rounds.push(result);
      for(const[kind,target]of [['load',0],['switch','internal-2'],['switch','internal-1'],...(rapid?[['rapid-seek',2]]:[]),['seek',900],['seek',1800],['seek',900],['seek',2]]){
        if(kind==='switch')await Promise.all(pages.map(({page})=>page.waitForTimeout(900)));
        if(kind==='switch'&&target==='internal-1')await Promise.all(pages.map(({page})=>page.waitForFunction(()=>document.querySelector('video').currentTime>=8.7)));
        const actions=await Promise.all(pages.map(async({page},session)=>{try{return {session,value:await page.evaluate(({kind,target})=>window.act(kind,target),{kind,target})};}catch(error){return {session,error:String(error),diagnostic:await page.evaluate(()=>({errors:window.errors,rapidSnapshot:window.rapidSnapshot,audioHistory:window.audioHistory,mseHistory:window.mseHistory,video:window.snapshot(document.querySelector('video')),remaining:[...document.querySelectorAll('audio')].map(window.snapshot)}))};}}));
        result.actions.push({kind,target,sessions:actions});if(actions.some(a=>a.error)){report.failed=true;break;}
      }
      for(const{context,page}of pages){await page.evaluate(()=>{window.player.destroy();window.audioContext.close();});await context.close();}if(report.failed)break;
    }
  }catch(error){report.error=String(error.stack);process.exitCode=1;}
  finally{if(browser)await browser.close();await new Promise(resolve=>server.close(resolve));await Promise.allSettled([...jobs.values()]);report.provenance.finishedAt=new Date().toISOString();fs.mkdirSync(out,{recursive:true});fs.writeFileSync(path.join(out,'report.json'),JSON.stringify(report,null,2));if(path.dirname(temp)===os.tmpdir()&&path.basename(temp).startsWith('media-audio-concurrency-'))fs.rmSync(temp,{recursive:true,force:true});console.log(JSON.stringify({out,failed:report.failed,error:report.error}));if(report.failed)process.exitCode=1;}
})();
