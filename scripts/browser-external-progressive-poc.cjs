// Real Chrome and production player, synthetic HTTP/FFmpeg transport, no Core/Node.
const fs=require('node:fs'),path=require('node:path'),http=require('node:http');
const {spawn,execFileSync}=require('node:child_process');
const {createHash}=require('node:crypto');
const {chromium}=require('../apps/web/node_modules/playwright');
const ts=require('../apps/web/node_modules/typescript');
const root=path.resolve(__dirname,'..'),fixtures=path.join(root,'target/streaming-video-window-poc');
const out=path.resolve(process.env.MEDIA_PLAYER_REPORT_DIR||path.join(root,'target/player-external-progressive'));fs.mkdirSync(out,{recursive:true});
const video=path.join(fixtures,'gop15.mp4'),audio=path.join(out,'external.opus');
if(!fs.existsSync(video))throw new Error('Run scripts/browser-video-window-poc.cjs with MEDIA_VIDEO_POC_PREPARE_ONLY=1 first');
if(!fs.existsSync(audio))execFileSync('ffmpeg',['-v','error','-nostdin','-n','-i',video,'-map','0:a:0','-c:a','copy',audio]);
const modules=['startup-buffer','errors','compatibility','source-selection','external-audio','segmented-audio','mp4-timeline','mp4-fragments','encoded-video-buffer','progressive-video','native'];
const sources=Object.fromEntries(modules.map(name=>[name,fs.readFileSync(path.join(root,'apps/web/src/lib/player',name+'.ts'),'utf8')]));
const sha=data=>createHash('sha256').update(data).digest('hex');
const report={scope:'Real Chrome production player with synthetic HTTP and FFmpeg; no Rust Node, Core, NAS or physical output',modules:Object.fromEntries(modules.map(n=>[n,sha(sources[n])])),requests:[],console:[]};
const children=new Set();
const server=http.createServer((req,res)=>{
  const u=new URL(req.url,'http://localhost').pathname;report.requests.push({path:u,method:req.method,range:req.headers.range});
  if(u==='/'){res.writeHead(200,{'content-type':'text/html'});res.end('<video playsinline></video>');return;}
  const m=/^\/stream-(\d+)$/.exec(u);
  if(m){const child=spawn('ffmpeg',['-v','error','-nostdin','-copyts','-start_at_zero','-ss',String(Number(m[1])/1000),'-i',video,'-map','0:v:0','-an','-sn','-c:v','copy','-movflags','frag_keyframe+empty_moov+default_base_moof+delay_moov','-frag_duration','1000000','-flush_packets','1','-f','mp4','pipe:1']);children.add(child);child.once('exit',()=>children.delete(child));res.writeHead(200,{'content-type':'video/mp4'});child.stdout.pipe(res);res.on('close',()=>child.kill());return;}
  if(u==='/external.opus'){const size=fs.statSync(audio).size,range=/^bytes=(\d+)-(\d*)$/.exec(req.headers.range||''),start=range?Number(range[1]):0,end=range&&range[2]?Math.min(Number(range[2]),size-1):size-1;if(start>=size){res.writeHead(416,{'content-range':`bytes */${size}`});res.end();return;}res.writeHead(range?206:200,{'content-type':'audio/ogg','accept-ranges':'bytes','content-length':end-start+1,...(range?{'content-range':`bytes ${start}-${end}/${size}`}:{})});if(req.method==='HEAD')res.end();else fs.createReadStream(audio,{start,end}).pipe(res);return;}
  res.writeHead(404);res.end();
});
(async()=>{let browser;try{
  await new Promise(resolve=>server.listen(0,'127.0.0.1',resolve));browser=await chromium.launch({channel:'chrome',headless:true,args:['--autoplay-policy=no-user-gesture-required']});report.browser=browser.version();
  const page=await browser.newPage();page.on('console',message=>{if(report.console.length<100)report.console.push({type:message.type(),text:message.text()});});await page.goto(`http://127.0.0.1:${server.address().port}/`);
  for(const name of modules){const code=ts.transpileModule(sources[name],{compilerOptions:{target:ts.ScriptTarget.ES2022,module:ts.ModuleKind.CommonJS}}).outputText;await page.addScriptTag({content:`window[${JSON.stringify(name)}]={};(function(exports,require){${code}})(window[${JSON.stringify(name)}],name=>window[name.slice(2)]);`});}
  let deadline;
  try {report.result=await Promise.race([page.evaluate(async()=>{
    const video=document.querySelector('video'),errors=[],player=new window.native.NativePlayer(video,message=>errors.push(message));
    const source={id:'external',audioTrackId:'external-0',language:'nl',url:'/unavailable-original',mime:'video/mp4',videoStreamUrl:'/stream-',videoMime:'video/mp4; codecs="av01.0.00M.10"',durationSeconds:34,externalAudioUrl:'/external.opus'};
    const start=performance.now();await player.load({id:'poc',title:'Synthetic',sources:[source],subtitles:[]},'external-0',2);await player.play();
    const audio=document.querySelector('audio'),ctx=new AudioContext(),node=ctx.createMediaElementSource(audio),analyser=ctx.createAnalyser();node.connect(analyser);analyser.connect(ctx.destination);await ctx.resume();
    await new Promise((resolve,reject)=>{const t=setTimeout(()=>reject(new Error('No decoded video frame')),10000);video.requestVideoFrameCallback(()=>{clearTimeout(t);resolve();});});
    const firstFrameMs=performance.now()-start,samples=new Float32Array(2048);let peak=0;for(let i=0;i<15;i++){analyser.getFloatTimeDomainData(samples);peak=Math.max(peak,...samples.map(Math.abs));await new Promise(resolve=>setTimeout(resolve,20));}
    const seeks=[];for(const target of [20,2,31]){const began=performance.now();player.seek(target);await new Promise((resolve,reject)=>{const timeout=setTimeout(()=>reject(new Error(`Seek ${target} timeout`)),12000),poll=setInterval(()=>{if(errors.length){clearTimeout(timeout);clearInterval(poll);reject(new Error(errors.at(-1)));}else if(!video.seeking&&!audio.seeking&&!video.paused&&!audio.paused&&Math.abs(video.currentTime-target)<1&&Math.abs(audio.currentTime-video.currentTime)<.3){clearTimeout(timeout);clearInterval(poll);resolve();}},25);});seeks.push({target,ms:performance.now()-began,video:video.currentTime,audio:audio.currentTime});}
    const result={firstFrameMs,peak,seeks,errors,scheme:video.currentSrc.startsWith('blob:')?'blob':'other'};player.destroy();await ctx.close();return result;
  }),new Promise((_,reject)=>{deadline=setTimeout(()=>reject(new Error('Browser scenario exceeded 60 seconds')),60000);})]);} finally {clearTimeout(deadline);}
  if(report.result.errors.length||report.result.peak<.001||report.result.scheme!=='blob'||!report.requests.some(r=>r.path.startsWith('/stream-')))throw new Error('Progressive external audio assertions failed');report.passed=true;
}catch(error){report.error=String(error.stack);process.exitCode=1;}finally{if(browser)await browser.close();server.closeAllConnections();for(const child of children)child.kill();await new Promise(resolve=>server.close(resolve));fs.writeFileSync(path.join(out,'report.json'),JSON.stringify(report,null,2));console.log(JSON.stringify(report,null,2));}})();
