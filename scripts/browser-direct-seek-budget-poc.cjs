// Real Chrome and production player; delayed Range server models a slow Node.
const fs=require('node:fs'),path=require('node:path'),http=require('node:http');
const {execFileSync}=require('node:child_process');const {createHash}=require('node:crypto');
const {chromium}=require('../apps/web/node_modules/playwright');const ts=require('../apps/web/node_modules/typescript');
const root=path.resolve(__dirname,'..'),fixture=path.join(root,'tests/fixtures/generated/dual-opus/dual-opus-long.mp4'),out=path.resolve(process.env.MEDIA_PLAYER_REPORT_DIR||path.join(root,'target/player-direct-seek-budget'));fs.mkdirSync(out,{recursive:true});
const names=['startup-buffer','errors','compatibility','source-selection','external-audio','segmented-audio','mp4-timeline','mp4-fragments','encoded-video-buffer','progressive-video','native'];
const current=Object.fromEntries(names.map(n=>[n,fs.readFileSync(path.join(root,'apps/web/src/lib/player',`${n}.ts`),'utf8')]));
const baseline=execFileSync('git',['show','HEAD:apps/web/src/lib/player/native.ts'],{cwd:root,encoding:'utf8'}),size=fs.statSync(fixture).size,sha=x=>createHash('sha256').update(x).digest('hex');
let armed=false,requests=[];
const server=http.createServer((req,res)=>{const u=new URL(req.url,'http://localhost').pathname;
  if(u==='/'){res.writeHead(200,{'content-type':'text/html'});res.end('<video playsinline></video>');return;}
  if(u==='/arm'){armed=true;res.writeHead(204);res.end();return;}
  if(u!=='/direct'&&u!=='/relay'){res.writeHead(404);res.end();return;}
  const range=/^bytes=(\d+)-(\d*)$/.exec(req.headers.range||''),start=range?Number(range[1]):0,end=range&&range[2]?Math.min(Number(range[2]),size-1):size-1;
  const delayed=armed&&u==='/direct'&&start>0;requests.push({url:u,start,delayed});
  const send=()=>{if(res.destroyed)return;res.writeHead(range?206:200,{'content-type':'video/mp4','accept-ranges':'bytes','content-length':end-start+1,...(range?{'content-range':`bytes ${start}-${end}/${size}`}:{})});if(req.method==='HEAD')res.end();else {const stream=fs.createReadStream(fixture,{start,end});res.on('close',()=>stream.destroy());stream.pipe(res);}};
  if(delayed)setTimeout(send,3500);else send();
});
(async()=>{let browser;const report={scope:'Real Chrome; synthetic local Range server delaying direct seek after metadata by 3.5 s. Not a real Node, NAS or network.',fixtureBytes:size,cases:[]};try{
  await new Promise(resolve=>server.listen(0,'127.0.0.1',resolve));browser=await chromium.launch({channel:'chrome',headless:true});report.browser=browser.version();
  for(const [label,native] of [['baseline',baseline],['current',current.native]]){
    armed=false;requests=[];const page=await browser.newPage();await page.goto(`http://127.0.0.1:${server.address().port}/`);
    for(const n of names){const code=ts.transpileModule(n==='native'?native:current[n],{compilerOptions:{target:ts.ScriptTarget.ES2022,module:ts.ModuleKind.CommonJS}}).outputText;await page.addScriptTag({content:`window[${JSON.stringify(n)}]={};(function(exports,require){${code}})(window[${JSON.stringify(n)}],n=>window[n.slice(2)]);`});}
    const result=await page.evaluate(async()=>{const video=document.querySelector('video'),errors=[],player=new window.native.NativePlayer(video,m=>errors.push(m));
      const position=new Promise(resolve=>video.addEventListener('loadedmetadata',()=>{fetch('/arm').then(()=>resolve(1800));},{once:true}));
      const source={id:'original',audioTrackId:'default',language:'und',url:'/relay',directUrl:location.origin+'/direct',relayUrl:'/relay',mime:'video/mp4'};
      const start=performance.now();await player.load({id:'poc',title:'Synthetic',sources:[source],subtitles:[]},'default',position);
      const result={elapsedMs:performance.now()-start,position:video.currentTime,source:video.currentSrc,errors};player.destroy();return result;});
    report.cases.push({label,nativeSha256:sha(native),...result,requests});await page.close();
  }
  for(const c of report.cases)if(!c.requests.some(r=>r.delayed)||Math.abs(c.position-1800)>1||c.errors.length)throw new Error(`Invalid delayed seek observation: ${c.label}`);
  report.passed=report.cases[0].source.endsWith('/relay')&&report.cases[1].source.endsWith('/direct');if(!report.passed)throw new Error('Expected baseline relay and current direct route');
}catch(e){report.error=String(e.stack);process.exitCode=1;}finally{if(browser)await browser.close();server.closeAllConnections();await new Promise(resolve=>server.close(resolve));fs.writeFileSync(path.join(out,'report.json'),JSON.stringify(report,null,2));console.log(JSON.stringify(report,null,2));}})();
