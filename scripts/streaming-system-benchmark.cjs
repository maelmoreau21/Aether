// Real Core/SQLite + remote Rust Node processes + built Svelte UI over TCP.
// The launcher serves no HTTP and never intercepts an API/media response.
const fs=require('node:fs'),fsp=fs.promises,path=require('node:path'),net=require('node:net'),os=require('node:os');
const {spawn,execFileSync}=require('node:child_process');
const {createHash,randomBytes}=require('node:crypto');
const {parseArgs}=require('node:util');
const {createInterface}=require('node:readline');
const {chromium,request}=require('../apps/web/node_modules/playwright');
const {installStreamingObserver}=require('./streaming-system-observer.cjs');
const {createTcpProfile}=require('./streaming-tcp-profile.cjs');
const {latencyOutcomes}=require('./streaming-report-summary.cjs');
const root=path.resolve(__dirname,'..');
const {values}=parseArgs({options:{'binary-dir':{type:'string',default:'target/streaming-system-artifacts/baseline-bd08e05'},fixture:{type:'string',default:'tests/fixtures/generated/dual-opus/dual-opus-long.mp4'},sessions:{type:'string',default:'1,4'},rounds:{type:'string',default:'5'},out:{type:'string',default:'target/streaming-system-baseline'},revision:{type:'string',default:'bd08e05839963afa8c84dbcd5aca067dea7863ad'},'skip-actions':{type:'boolean',default:false},'http-samples':{type:'string',default:'0'},'tcp-delay-ms':{type:'string',default:'0'},'tcp-rate-mib':{type:'string',default:'0'},'seek-targets':{type:'string',default:'900,1800,900,2'},robustness:{type:'boolean',default:false},'prepared-reopen':{type:'boolean',default:false},'build-profile':{type:'string',default:'debug'},'fault-mode':{type:'string',default:'node-crash'},'close-during-preparation':{type:'boolean',default:false},'truncate-after-scan-bytes':{type:'string',default:'0'},'export-native-input':{type:'string'}}});
if(process.platform!=='win32')throw new Error('This collector requires Windows Job Objects; no substitute CPU/RSS is claimed');
const artifactDir=path.resolve(root,values['binary-dir']),fixture=path.resolve(root,values.fixture),out=path.resolve(root,values.out);
const groups=values.sessions.split(',').map(Number),rounds=Number(values.rounds);
const truncateBytes=Number(values['truncate-after-scan-bytes']);if(!Number.isSafeInteger(truncateBytes)||truncateBytes<0)throw new Error('truncate-after-scan-bytes must be nonnegative integer');
const httpSamples=Number(values['http-samples']);if(!Number.isInteger(httpSamples)||httpSamples<0||httpSamples>1000)throw new Error('http-samples 0..1000');
const tcpDelay=Number(values['tcp-delay-ms']),tcpRate=Number(values['tcp-rate-mib'])*1024*1024;
const seekTargets=values['seek-targets'].split(',').map(Number);if(!seekTargets.length||!seekTargets.every(n=>Number.isFinite(n)&&n>=0))throw new Error('invalid seek-targets');
if(!Number.isFinite(tcpDelay)||tcpDelay<0||tcpDelay>2000||!Number.isFinite(tcpRate)||tcpRate<0)throw new Error('invalid TCP profile');
if(!groups.every(n=>Number.isInteger(n)&&n>=1&&n<=16)||!Number.isInteger(rounds)||rounds<1||rounds>1000)throw new Error('sessions 1..16, rounds 1..1000');
if(!['debug','release'].includes(values['build-profile']))throw new Error('build-profile debug|release');
if(!['node-crash','network-cut'].includes(values['fault-mode']))throw new Error('fault-mode node-crash|network-cut');
if(fs.existsSync(out))throw new Error('Refusing to overwrite benchmark output');
fs.mkdirSync(out,{recursive:true});const work=path.join(out,'private-runtime');fs.mkdirSync(work);
const report={schemaVersion:2,date:new Date().toISOString(),scope:'Real Rust Core/SQLite and separate Rust Node on loopback TCP; actual built Svelte UI; no route mocks; not two physical machines',os:`${os.platform()} ${os.release()}`,hardware:{cpuModel:os.cpus()[0]?.model,logicalCpus:os.cpus().length,totalMemoryBytes:os.totalmem()},buildProfile:values['build-profile']+' binaries supplied by caller; executable SHA-256 authoritative',cells:[],resourceMethod:'Win32 Job Object inclusive CPU/logical IO, root-process counters, sampled inclusive working set; cache file byte snapshots. Logical I/O is not physical disk I/O.',cacheContract:'cold-app-preparation restarts both Rust processes and uses an empty preparation root, with persisted DB/Node inventory; OS disk cache not purged. Warm retains processes and prepared windows. No media content request is made before the timed click.'};
report.observationMethod={revision:2,firstRawPcmMs:'First non-muted playing selected element PCM above 1e-4 RMS, without video-playing, seek-position or drift gates; known fixture frequency identifies switched target',firstAudioMs:'Legacy aligned PCM: video playing, drift <150ms and target/restore position gates',firstAnyPcmMs:'Any playing external audio PCM, may belong to previous track during switching',initialPcm:'Signed audio clock minus video clock at first raw PCM; sampled clock difference is not physical speaker/display synchrony'};
report.statisticalMethod={median:'Arithmetic mean of central pair for even n',percentiles:'Nearest rank',p99MinimumSamples:100,failures:'All attempts remain in raw cells; successful early measurements are retained when a later action fails'};
const password=randomBytes(24).toString('hex'),pairToken=randomBytes(32).toString('hex'),bootstrapToken=randomBytes(32).toString('hex');
const secrets=new Set([password,pairToken,bootstrapToken]);
let core,node,browser,api,metrics,metricsStderr='',metricExitError,metricReady,proxy;
let corePort,nodePort,origin,nodeOrigin,processIndex=0,csrf,cookie;
const userStates=new Map();
let loginBatchCount=0;
async function paceSetupLogin(){if(loginBatchCount>=9){report.authSetupPacing??={scope:'Setup only; real login throttle unchanged, no spoofed client IP',waitsMs:[]};const start=performance.now();console.log('setup: respecting real authentication limit; waiting61s before next login batch');await new Promise(resolve=>setTimeout(resolve,61000));report.authSetupPacing.waitsMs.push(performance.now()-start);loginBatchCount=0;}loginBatchCount++;}
const metricReplies=[];let metricPending;
function scrub(text){let value=String(text);for(const secret of secrets)if(secret)value=value.replaceAll(secret,'[redacted]');return value.replace(/([?&]token=)[^\s"']+/g,'$1[redacted]').replace(/((?:x-csrf-token|cookie|authorization):\s*)[^\r\n]+/gi,'$1[redacted]');}
const hash=async file=>new Promise((resolve,reject)=>{const h=createHash('sha256');fs.createReadStream(file).on('data',b=>h.update(b)).on('error',reject).on('end',()=>resolve(h.digest('hex')));});
async function fileManifest(dir,prefix=''){const rows={};for(const ent of await fsp.readdir(dir,{withFileTypes:true})){const relative=path.join(prefix,ent.name),absolute=path.join(dir,ent.name);if(ent.isDirectory())Object.assign(rows,await fileManifest(absolute,relative));else if(ent.isFile())rows[relative.replaceAll('\\','/')]={bytes:(await fsp.stat(absolute)).size,sha256:await hash(absolute)};}return rows;}
async function cacheFiles(dir){const rows={};if(!fs.existsSync(dir))return rows;for(const ent of await fsp.readdir(dir,{withFileTypes:true})){const p=path.join(dir,ent.name);if(ent.isDirectory()){const child=await cacheFiles(p);for(const [key,size]of Object.entries(child))rows[`${ent.name}/${key}`]=size;}else if(ent.isFile())rows[ent.name]=(await fsp.stat(p)).size;}return rows;}
async function port(){return new Promise((resolve,reject)=>{const s=net.createServer();s.once('error',reject);s.listen(0,'127.0.0.1',()=>{const n=s.address().port;s.close(()=>resolve(n));});});}
async function metricCommand(command){if(metricPending)throw new Error('Concurrent metric command');if(metricExitError)throw metricExitError;return new Promise((resolve,reject)=>{const timer=setTimeout(()=>{metricPending=null;reject(new Error(`Metric collector timeout: ${command}`));},15000);metricPending={resolve:value=>{clearTimeout(timer);metricPending=null;if(value.error)reject(new Error(value.error));else resolve(value);}};metrics.stdin.write(command+'\n');});}
async function startMetrics(){metrics=spawn('pwsh',['-NoProfile','-File',path.join(root,'scripts/streaming-process-metrics.ps1')],{windowsHide:true,stdio:['pipe','pipe','pipe']});metrics.stderr.on('data',data=>metricsStderr+=data);const ready=new Promise((resolve,reject)=>{metricReady={resolve,reject};});createInterface({input:metrics.stdout}).on('line',line=>{try{const data=JSON.parse(line);if(data.ready)metricReady.resolve(data);else if(metricPending)metricPending.resolve(data);else metricReplies.push(data);}catch(error){metricExitError=error;}});metrics.on('exit',code=>{if(code&&code!==0){metricExitError=new Error(`metrics exit ${code}: ${metricsStderr}`);metricReady.reject(metricExitError);}});await ready;}
async function waitHealth(base,child){const until=Date.now()+30000;while(Date.now()<until){if(child.exitCode!==null)throw new Error('Rust process exited before health');try{const r=await fetch(base+'/internal/v1/health');if(r.ok)return;}catch{}await new Promise(r=>setTimeout(r,50));}throw new Error(`Health timeout ${base}`);}
async function startRust(cacheDir,{nodeOnly=false}={}){
  const index=processIndex++,nodeCwd=path.join(work,'node'),coreCwd=path.join(work,'core');
  const cleanEnv={...process.env};for(const key of Object.keys(cleanEnv))if(key.startsWith('MEDIA_'))delete cleanEnv[key];
  const spawnOne=async(label,exe,cwd,env)=>{const child=spawn(path.join(artifactDir,exe),[],{cwd,env:{...cleanEnv,...env},windowsHide:true,stdio:['ignore','pipe','pipe']});const log=fs.createWriteStream(path.join(out,`${label}-${index}.log`));child.stdout.on('data',b=>log.write(scrub(b)));child.stderr.on('data',b=>log.write(scrub(b)));child.on('exit',()=>log.end());child.on('error',e=>log.write(scrub(e)));await metricCommand(`attach ${label}${index} ${child.pid}`);return child;};
  if(!nodeOnly)core=await spawnOne('core','media-poc.exe',coreCwd,{MEDIA_DB_PATH:path.join(work,'core.sqlite3'),MEDIA_LIBRARY_ROOT:path.join(work,'empty-local'),MEDIA_PORT:String(corePort),MEDIA_PUBLIC_URL:origin,MEDIA_BOOTSTRAP_TOKEN:bootstrapToken,MEDIA_SILENT_BOOTSTRAP_BANNER:'true',MEDIA_PREPARATION_CACHE_DIR:path.join(work,'unused-core-cache')});
  node=await spawnOne('node','aether-node.exe',nodeCwd,{MEDIA_NODE_BIND:`127.0.0.1:${nodePort}`,MEDIA_NODE_DATA_DIR:path.join(nodeCwd,'data'),MEDIA_NODE_ROOTS:'media',MEDIA_NODE_BOOTSTRAP_TOKEN:pairToken,MEDIA_NODE_PUBLIC_URL:nodeOrigin,MEDIA_PREPARATION_CACHE_DIR:cacheDir});
  await Promise.all([waitHealth(origin,core),waitHealth(nodeOrigin,node)]);return {coreLabel:`core${index}`,nodeLabel:`node${index}`};
}
async function stopChild(child){if(!child||child.exitCode!==null)return;await new Promise(resolve=>{child.once('exit',resolve);child.kill();});}
async function stopRust(){await stopChild(core);await stopChild(node);core=null;node=null;}
async function apiCall(method,url,data){const response=await api.fetch(origin+url,{method,data,headers:{Origin:origin,...(csrf?{'x-csrf-token':csrf}:{})}});const text=await response.text();if(!response.ok())throw new Error(`API ${method} ${url} HTTP ${response.status()}: ${scrub(text).slice(0,1000)}`);return text?JSON.parse(text):null;}
function resourceDelta(before,after){const fields=['cpuUser100ns','cpuKernel100ns','parentCpuUser100ns','parentCpuKernel100ns','readTransferBytes','writeTransferBytes','otherTransferBytes','parentReadTransferBytes','parentWriteTransferBytes','processesCreated'];const result={};for(const field of fields)result[field]=after[field]-before[field];result.cpuMs=(result.cpuUser100ns+result.cpuKernel100ns)/10000;result.parentCpuMs=(result.parentCpuUser100ns+result.parentCpuKernel100ns)/10000;result.descendantCpuMs=result.cpuMs-result.parentCpuMs;result.rssBeforeBytes=before.rssBytes;result.rssAfterBytes=after.rssBytes;result.peakObservedRssBytes=after.peakObservedRssBytes;result.rssSamples=after.rssSamples;result.maxRssSampleGapMs=after.maxRssSampleGapMs;return result;}
function stats(numbers){numbers=numbers.filter(Number.isFinite).sort((a,b)=>a-b);if(!numbers.length)return null;const middle=Math.floor(numbers.length/2),median=numbers.length%2?numbers[middle]:(numbers[middle-1]+numbers[middle])/2;return {n:numbers.length,min:numbers[0],median,p95:numbers[Math.ceil(numbers.length*.95)-1],p99:numbers.length>=100?numbers[Math.ceil(numbers.length*.99)-1]:null,p99Scope:numbers.length>=100?'empirical nearest rank; repeat runs still share one machine':'not estimated: fewer than 100 samples',max:numbers.at(-1)};}
async function finished(page,kind){await page.waitForFunction(kind=>{const s=window.__streamingSystem?.sample();return s&&(s.error||s.trial?.notices?.some(n=>/précédente.*conservée/i.test(n))||(s.trial?.kind===kind&&s.trial.firstFrameMs!==undefined&&s.trial.firstAudioMs!==undefined&&(s.trial.expectedResumeTime===undefined||s.trial.firstRestoredFrameMs!==undefined)&&!s.video.paused&&!s.loading));},kind,{timeout:30000});const s=await page.evaluate(()=>window.__streamingSystem.sample());if(s.error)throw new Error(s.error);if(s.trial?.notices?.some(n=>/précédente.*conservée/i.test(n)))throw new Error('Candidate failed; previous presentation retained (captured DOM notice)');return s;}
async function preparePage(storageState){
  const context=await browser.newContext({storageState,viewport:{width:1440,height:900},locale:'fr-FR'});
  await context.addInitScript(installStreamingObserver);
  const page=await context.newPage();page.setDefaultTimeout(30000);
  const network=[],mediaRequests=[],endpoints={},errors=[];
  page.on('request',req=>{const u=new URL(req.url());if(/\/stream\/v1\/|\/prepared\/|\/assets\/.*content/.test(u.pathname))mediaRequests.push({origin:u.origin,path:u.pathname,method:req.method()});});
  page.on('response',response=>{
    const u=new URL(response.url());
    if(!/\/stream\/v1\/|\/prepared\/|\/assets\/.*content/.test(u.pathname))return;
    const h=response.headers();
    network.push({timing:response.request().timing(),origin:u.origin,path:u.pathname,status:response.status(),method:response.request().method(),range:response.request().headers().range,contentRange:h['content-range'],contentLength:h['content-length']});
    if(u.origin===nodeOrigin&&u.pathname.endsWith('/content'))endpoints.original=response.url();
    if(!endpoints.audio&&/\/prepared\/(?:audio|opus)-\d+-chunk-0$/.test(u.pathname))endpoints.audio=response.url();
    if(u.pathname.includes('/prepared/video-stream-'))endpoints.video=response.url();
  });
  page.on('pageerror',e=>errors.push(String(e)));
  await page.goto(origin+'/app/films/');
  await page.locator('.media-card').filter({hasText:'Streaming Benchmark'}).first().click();
  await page.getByRole('heading',{name:'Streaming Benchmark',exact:true}).waitFor();
  return {context,page,network,mediaRequests,endpoints,errors};
}
async function measureHttp(endpoints){
  const auth=(await api.storageState()).cookies.map(c=>`${c.name}=${c.value}`).join(';');report.http=[];
  for(const [component,url]of Object.entries(endpoints)){
    const rows=[];await metricCommand('reset');const before=(await metricCommand('sample')).jobs;
    for(let i=0;i<httpSamples;i++){
      const started=performance.now();try{
        const response=await fetch(url,{headers:{...(component==='video'?{}:{Range:'bytes=0-65535'}),...(url.startsWith(origin)?{Cookie:auth}:{})}});
        const headersMs=performance.now()-started,reader=response.body.getReader();
        let bytes=0,done=false;
        while(bytes<65536){const next=await reader.read();done=next.done;if(done)break;bytes+=next.value.byteLength;}
        if(!done&&response.status===206&&Number(response.headers.get('content-length'))<=65536){const end=await reader.read();if(!end.done)throw new Error('Range exceeded bounded response');done=true;}
        if(!done)await reader.cancel();
        rows.push({headersMs,totalMs:performance.now()-started,status:response.status,bytesReceived:bytes,rangeHonored:response.status===206});
      }catch(error){rows.push({error:scrub(error.message)});}
    }
    const after=(await metricCommand('sample')).jobs;const resourceRows={};for(const b of before.filter(j=>j.activeProcesses)){const a=after.find(j=>j.label===b.label);resourceRows[b.label]=resourceDelta(b,a);}
    const success=rows.filter(r=>r.status===(component==='video'?200:206));
    report.http.push({component,path:new URL(url).pathname,scope:component==='video'?'Authorized progressive GET, no Range; headers time, then bounded first 64 KiB and cancellation; no UI/decoder':'Authorized warm Range 0..65535; time to fetch response headers (not wire first byte), then complete 64 KiB; no UI/decoder; connection pool reused',attempts:rows.length,failures:rows.length-success.length,samples:rows,headersMs:stats(success.map(r=>r.headersMs)),totalMs:stats(success.map(r=>r.totalMs)),resources:resourceRows});
  }
}
async function runActions(page,results=[]){const initial=await page.evaluate(()=>window.__streamingSystem.sample().audio?.track);if(!['internal-1','internal-2'].includes(initial))throw new Error('Fixture audio identity unavailable');const targets=initial==='internal-1'?['internal-2','internal-1']:['internal-1','internal-2'];for(const track of targets){await page.waitForTimeout(600);await page.keyboard.press('Shift');await page.locator('[data-menu="audio"]').click();await page.locator(`[data-audio-id="${track}"]`).click();results.push(await finished(page,'switch'));}for(const target of seekTargets){await page.keyboard.press('Shift');await page.getByRole('slider',{name:'Position',exact:true}).evaluate((slider,target)=>{slider.value=String(target);slider.dispatchEvent(new Event('input',{bubbles:true}));},target);results.push(await finished(page,'seek'));}return results;}
async function runPreparedReopen(view,results){
  const before=await view.page.evaluate(()=>window.__streamingSystem.sample()),networkStart=view.network.length;
  const saved=view.page.waitForResponse(response=>{try{return response.request().method()==='PUT'&&new URL(response.url()).pathname.startsWith('/api/v1/progress/')&&Math.abs(response.request().postDataJSON().position-before.video.position)<.5;}catch{return false;}},{timeout:10000});
  await view.page.keyboard.press('Shift');await view.page.locator('button.return').click();
  await view.page.waitForFunction(()=>!document.querySelector('dialog#player')?.hasAttribute('open'));
  const response=await saved;if(!response.ok())throw new Error(`Prepared reopen progress save HTTP ${response.status()}`);
  const progress=await response.json(),expected=progress.watched?0:progress.position;
  await view.page.evaluate(target=>{window.__streamingSystem.nextExpectedResumeTime=target;},expected);
  await view.page.locator('.hero-cta').click();const reopened=await finished(view.page,'open');
  reopened.trial.kind='reopen';reopened.precondition={positionBeforeClose:before.video.position,savedPosition:progress.position,watched:progress.watched,expectedResumeTime:expected,progressSaveAwaitedBeforeClick:true,mediaPreparationOutsidePriorPlayback:false};
  reopened.videoLoadDelta=reopened.video.loads-before.video.loads;reopened.mediaResponses=view.network.length-networkStart;results.push(reopened);
}
async function exportNativeInput(){
  const dir=path.resolve(root,values['export-native-input']);if(fs.existsSync(dir))throw new Error('Refusing to overwrite native export directory');await fsp.mkdir(dir,{recursive:true});
  report.mode='export-only; setup and authenticated requests warm index/cache; no timing claims';
  const catalog=await apiCall('GET','/api/v1/catalog');const detail=await apiCall('GET',`/api/v1/catalog/${catalog.items[0].id}`);
  const mediaId=detail.editions[0].versions[0].sources[0].playbackMediaId;
  const descriptor=await apiCall('GET',`/api/v1/media/${mediaId}`);
  const source=descriptor.sources.find(s=>s.audioSegmentUrl?.includes('/opus-'));
  if(!source)throw new Error('Real descriptor did not advertise native Opus');
  const sourceUrl=new URL(source.audioSegmentUrl,origin);
  const cookieHeader=(await api.storageState()).cookies.map(c=>`${c.name}=${c.value}`).join(';');
  const exported=[];
  for(const id of [2,3])for(const n of [0,1,2,90,91,180]){
    const pathname=sourceUrl.pathname.replace(/opus-\d+-chunk-$/,`opus-${id}-chunk-${n}`);
    const response=await fetch(origin+pathname,{headers:{Cookie:cookieHeader}});if(!response.ok)throw new Error(`Native export HTTP ${response.status} ${pathname}`);
    const chunks=[];let bytes=0;for await(const chunk of response.body){bytes+=chunk.length;if(bytes>32*1024*1024)throw new Error('Native window exceeds export bound32MiB');chunks.push(chunk);}
    const name=`${id}-${n}.mp4`,file=path.join(dir,name);await fsp.writeFile(file,Buffer.concat(chunks));exported.push({file:name,bytes,sha256:await hash(file),requestPath:pathname,status:response.status,contentType:response.headers.get('content-type')});
  }
  report.export={scope:'Exact authorized Core relay responses from genuine Rust Node; no repackaging, no timing; fixture index/cache warmed',files:exported};report.passed=true;
  await fsp.writeFile(path.join(dir,'provenance.json'),JSON.stringify({fixture:report.fixture.path,fixtureSha256:await hash(fixture),exportedAt:new Date().toISOString(),artifacts:report.artifacts,mode:report.mode,...report.export},null,2)+'\n');
}
async function runCloseDuringPreparation(){
  const result={scope:'Fresh preparation directory; close actual dialog after first real media request, without waiting for playback readiness'};report.closeDuringPreparation=result;
  await startRust(path.join(work,'close-during-preparation'));
  const view=await preparePage(userStates.get('bench-robustness'));
  try{
    const requestStarted=view.page.waitForRequest(r=>/\/stream\/v1\/|\/prepared\//.test(new URL(r.url()).pathname),{timeout:15000});
    await view.page.locator('.hero-cta').click();const requested=await requestStarted;result.firstRequestPath=new URL(requested.url()).pathname;
    result.beforeClose=await view.page.evaluate(()=>window.__streamingSystem.sample());
    result.closedBeforePresentationReady=result.beforeClose.trial?.firstFrameMs===undefined||result.beforeClose.trial?.firstAudioMs===undefined;
    await view.page.keyboard.press('Shift');await view.page.locator('button.return').click();
    await view.page.waitForFunction(()=>!document.querySelector('dialog#player')?.hasAttribute('open'));
    await view.page.waitForTimeout(1000);
    result.afterClose=await view.page.evaluate(()=>({dialogOpen:!!document.querySelector('dialog[open]'),playingAudio:[...document.querySelectorAll('audio')].filter(a=>!a.paused&&!a.muted).length,videoElements:document.querySelectorAll('dialog video').length}));
    await view.page.locator('.hero-cta').click();result.reopened=await finished(view.page,'open');
    result.passed=result.closedBeforePresentationReady&&!result.afterClose.dialogOpen&&result.afterClose.playingAudio===0&&!result.reopened.error;
  }catch(error){result.error=scrub(error.stack);result.diagnostic=await view.page.evaluate(()=>window.__streamingSystem.sample()).catch(()=>null);result.passed=false;}
  finally{await view.context.close();await stopRust();}
}
async function runRobustness(){
  const result={faultMode:values['fault-mode'],scope:'Separate genuine Node-process outage, outside natural timing. Buffered playback is temporary, never an indefinite offline guarantee.'};report.robustness=result;
  const cacheDir=path.join(work,'robustness-preparation');await startRust(cacheDir);
  const view=await preparePage(userStates.get('bench-robustness'));
  try{
    await view.page.locator('.hero-cta').click();await finished(view.page,'open');
    await view.page.waitForFunction(()=>{const v=document.querySelector('video'),a=document.querySelector('audio[data-external-audio]');const ahead=e=>e&&Array.from({length:e.buffered.length},(_,i)=>[e.buffered.start(i),e.buffered.end(i)]).some(([s,t])=>s<=e.currentTime+.025&&t-e.currentTime>=5);return v&&a&&!v.paused&&!a.paused&&ahead(v)&&ahead(a);},null,{timeout:15000});
    const english=report.fixture.probe.streams.find(s=>s.codec_type==='audio'&&s.tags?.language==='eng');
    if(!english)throw new Error('Known English fixture track required for outage oracle');
    const englishPattern=new RegExp(`/prepared/(?:audio-${english.index}|opus-${Number.parseInt(english.id)})-chunk-`);
    if(view.network.some(n=>englishPattern.test(n.path)))throw new Error('English candidate was unexpectedly prefetched before outage');
    result.before=await view.page.evaluate(()=>window.__streamingSystem.sample());if(values['fault-mode']==='network-cut'){proxy.setDisconnected(true);result.networkCut=true;}else{await stopChild(node);node=null;result.nodeStopped=true;}
    await view.page.keyboard.press('Shift');await view.page.locator('[data-menu="audio"]').click();await view.page.locator('[data-audio-id="internal-2"]').click();
    await view.page.waitForFunction(()=>/précédente.*conservée/i.test(document.querySelector('.fullscreen-notice')?.textContent||''),null,{timeout:15000});
    await view.page.waitForTimeout(500);result.retained=await view.page.evaluate(()=>window.__streamingSystem.sample());
    const retained=result.retained;await view.page.keyboard.press('Shift');await view.page.locator('[data-menu="audio"]').click();result.menuRetainedFrench=await view.page.locator('[data-audio-id="internal-1"]').getAttribute('aria-checked')==='true';await view.page.locator('[data-menu="audio"]').click();
    result.retentionPassed=result.menuRetainedFrench&&!retained.error&&!retained.video.paused&&!retained.audio.paused&&retained.audio.track==='internal-1'&&retained.audio.signal.hz>400&&retained.audio.signal.hz<900&&retained.video.position>result.before.video.position+.1&&retained.video.loads===result.before.video.loads;
    if(result.networkCut){proxy.setDisconnected(false);result.networkRestored=true;}else{await startRust(cacheDir,{nodeOnly:true});result.nodeRestarted=true;}
    await view.page.keyboard.press('Shift');await view.page.locator('[data-menu="audio"]').click();await view.page.locator('[data-audio-id="internal-2"]').click();result.recovered=await finished(view.page,'switch');
    result.passed=result.retentionPassed&&result.recovered.audio.track==='internal-2'&&result.recovered.video.loads===result.before.video.loads;
  }catch(error){result.error=scrub(error.stack);result.diagnostic=await view.page.evaluate(()=>window.__streamingSystem.sample()).catch(()=>null);result.passed=false;}
  finally{if(result.networkCut)proxy.setDisconnected(false);await view.context.close();await stopRust();}
}
(async()=>{
  let sealed;
  try{
    report.harnessSha256=Object.fromEntries(await Promise.all(['streaming-system-benchmark.cjs','streaming-system-observer.cjs','streaming-process-metrics.ps1','streaming-tcp-profile.cjs','streaming-report-summary.cjs'].map(async name=>[name,await hash(path.join(root,'scripts',name))])));
    const commit=execFileSync('git',['rev-parse',values.revision],{cwd:root,encoding:'utf8'}).trim();
    const manifestPath=path.join(artifactDir,'manifest.json');
    const artifactFiles=await fileManifest(artifactDir);
    delete artifactFiles['manifest.json'];
    if(fs.existsSync(manifestPath)){sealed=JSON.parse(await fsp.readFile(manifestPath,'utf8'));if(JSON.stringify(sealed.files)!==JSON.stringify(artifactFiles))throw new Error('Immutable artifact manifest mismatch');}
    else{const sourcePaths=execFileSync('git',['ls-files','--cached','--others','--exclude-standard','crates','apps/web/src','Cargo.toml','Cargo.lock','apps/web/package.json','apps/web/package-lock.json'],{cwd:root,encoding:'utf8'}).trim().split(/\r?\n/).filter(Boolean);const sourceHashes=Object.fromEntries(await Promise.all(sourcePaths.map(async name=>[name,await hash(path.join(root,name))])));sealed={revision:commit,sealedAt:new Date().toISOString(),sourceAssociation:'Artifacts supplied by caller; revision and observed source hashes are not an embedded build attestation. Executable/Web hashes identify the tested artifacts.',observedCheckoutHead:execFileSync('git',['rev-parse','HEAD'],{cwd:root,encoding:'utf8'}).trim(),observedDirtyPatchSha256:createHash('sha256').update(execFileSync('git',['diff','--binary','HEAD'],{cwd:root})).digest('hex'),observedSourceSha256:sourceHashes,files:artifactFiles};await fsp.writeFile(manifestPath,JSON.stringify(sealed,null,2)+'\n');}
    report.artifacts=sealed;await fsp.writeFile(path.join(out,'artifact-manifest.json'),JSON.stringify(sealed,null,2)+'\n');
    await fsp.cp(path.join(artifactDir,'web'),path.join(work,'core','apps','web','build'),{recursive:true});
    for(const dir of ['empty-local','node/media/Streaming Benchmark (2026)','node/data'])await fsp.mkdir(path.join(work,dir),{recursive:true});
    const mediaFile=path.join(work,'node/media/Streaming Benchmark (2026)',`Streaming Benchmark (2026)${path.extname(fixture)}`);
    if(truncateBytes)await fsp.copyFile(fixture,mediaFile);else await fsp.link(fixture,mediaFile);
    await fsp.writeFile(path.join(path.dirname(mediaFile),'movie.nfo'),'<movie><title>Streaming Benchmark</title><year>2026</year><plot>Legal synthetic streaming benchmark.</plot></movie>');
    report.fixture={path:path.relative(root,fixture),bytes:(await fsp.stat(fixture)).size,hardlink:!truncateBytes,probe:JSON.parse(execFileSync('ffprobe',['-v','error','-show_streams','-show_format','-of','json',fixture]))};
    if(seekTargets.some(n=>n>=Number(report.fixture.probe.format.duration)))throw new Error('seek target exceeds fixture duration');report.seekTargets=seekTargets;
    report.ffmpeg=execFileSync('ffmpeg',['-version'],{encoding:'utf8'}).split('\n')[0];
    corePort=await port();nodePort=await port();origin=`http://127.0.0.1:${corePort}`;nodeOrigin=`http://127.0.0.1:${nodePort}`;
    if(tcpDelay||tcpRate||(values.robustness&&values['fault-mode']==='network-cut')){proxy=createTcpProfile({targetPort:nodePort,delayMs:tcpDelay,bytesPerSecond:tcpRate});nodeOrigin=`http://127.0.0.1:${await proxy.start()}`;}
    report.origins={core:origin,node:nodeOrigin};await startMetrics();
    await startRust(path.join(work,'setup-preparation'));
    api=await request.newContext();const identity=await apiCall('POST','/api/v1/auth/bootstrap',{login:'streaming-bench',password,bootstrap_token:bootstrapToken});csrf=identity.csrfToken;secrets.add(csrf);for(const c of (await api.storageState()).cookies)secrets.add(c.value);
    const paired=await apiCall('POST','/api/v1/admin/nodes/pair',{name:'Streaming Benchmark Node',baseUrl:nodeOrigin,publicUrl:nodeOrigin,pairingToken:pairToken});
    const library=await apiCall('POST','/api/v1/admin/libraries',{name:'Streaming Benchmark',kind:'films',nodeId:paired.id,root:paired.roots[0],enabled:true,userIds:[]});
    for(const sessions of groups)for(let index=0;index<sessions;index++){
      const login=`bench-${sessions}-${index}`;await apiCall('POST','/api/v1/admin/users',{login,password,role:'user',displayName:login,libraryIds:[library.id]});
      await paceSetupLogin();const user=await request.newContext();const response=await user.post(origin+'/api/v1/auth/login',{headers:{Origin:origin},data:{login,password}});if(!response.ok())throw new Error(`User login failed ${response.status()}`);const userIdentity=await response.json();secrets.add(userIdentity.csrfToken);const storage=await user.storageState();for(const c of storage.cookies)secrets.add(c.value);userStates.set(login,storage);await user.dispose();
    }
    if(values.robustness||values['close-during-preparation']){const login='bench-robustness';await apiCall('POST','/api/v1/admin/users',{login,password,role:'user',displayName:login,libraryIds:[library.id]});await paceSetupLogin();const user=await request.newContext();const loginResponse=await user.post(origin+'/api/v1/auth/login',{headers:{Origin:origin},data:{login,password}});if(!loginResponse.ok())throw new Error('Robustness user login failed');secrets.add((await loginResponse.json()).csrfToken);const storage=await user.storageState();for(const c of storage.cookies)secrets.add(c.value);userStates.set(login,storage);await user.dispose();}
    report.sessionIdentity='Distinct ordinary user account and server progress/preferences per simultaneous session; administrator used only for setup and separate HTTP probes.';
    await apiCall('POST',`/api/v1/admin/libraries/${library.id}/scan`);
    const end=Date.now()+120000;let job;while(Date.now()<end){job=await apiCall('GET',`/api/v1/admin/libraries/${library.id}/scan`);if(job.status==='completed')break;if(job.status==='error')throw new Error('Scan failed');await new Promise(r=>setTimeout(r,100));}if(job?.status!=='completed'||job.errorsCount)throw new Error(`Scan not completed: ${JSON.stringify(job)}`);report.scan=job;
    if(values['export-native-input']){await exportNativeInput();return;}
    await stopRust();if(truncateBytes){const size=(await fsp.stat(mediaFile)).size;if(truncateBytes>=size)throw new Error('Truncation must shorten test copy');await fsp.truncate(mediaFile,truncateBytes);report.corruption={kind:'Private fixture copy truncated after completed real scan',originalBytes:size,remainingBytes:truncateBytes,originalFixtureUnmodified:true};}
    browser=await chromium.launch({channel:'chrome',headless:true,args:['--autoplay-policy=no-user-gesture-required']});report.browser=browser.version();
    for(const sessions of groups){let labels,lastEndpoints;const cacheDir=path.join(work,`preparation-${sessions}`);
      for(let round=0;round<=rounds;round++){
        if(round===0)labels=await startRust(cacheDir);
        const pages=await Promise.all(Array.from({length:sessions},(_,index)=>preparePage(userStates.get(`bench-${sessions}-${index}`))));
        const cell={sessions,round,phase:round===0?'cold-app-preparation':'warm-server-new-browser',trials:[],network:[],errors:[]};report.cells.push(cell);
        cell.resourceScope=values['prepared-reopen']?'Initial open, optional track/seeks, explicit same-context close/reopen and observation':'Initial open, optional track/seeks and observation';
        cell.mediaRequestsBeforeClick=pages.map(view=>view.mediaRequests.length);
        cell.mediaResponsesBeforeClick=pages.map(view=>view.network.length);
        if(cell.mediaRequestsBeforeClick.some(count=>count>0))cell.errors.push('Media request observed before timed click; this attempt violates media-cold precondition');
        if(cell.mediaResponsesBeforeClick.some(count=>count>0))cell.errors.push('Media response observed before timed click; cache state is not a media-cold claim');
        const cacheBefore=await cacheFiles(cacheDir);await metricCommand('reset');const before=(await metricCommand('sample')).jobs;const started=performance.now();
        await Promise.all(pages.map(async(view,index)=>{
          const trial={session:index,actions:[],deadlineMs:30000};cell.trials.push(trial);
          try{await view.page.locator('.hero-cta').click();trial.open=await finished(view.page,'open');if(!values['skip-actions'])await runActions(view.page,trial.actions);if(values['prepared-reopen'])await runPreparedReopen(view,trial.actions);}
          catch(error){trial.error=String(error);trial.timeout=/timeout/i.test(trial.error);cell.errors.push(trial.error);trial.diagnostic=await view.page.evaluate(()=>window.__streamingSystem?.sample()).catch(()=>null);trial.observerHistory=await view.page.evaluate(()=>window.__streamingSystem?.trials).catch(()=>null);}
        }));
        await Promise.all(pages.map(v=>v.page.waitForTimeout(200)));const after=(await metricCommand('sample')).jobs;cell.observedWallMs=performance.now()-started;cell.resources={};for(const [name,label]of Object.entries(labels)){const b=before.find(j=>j.label===label),a=after.find(j=>j.label===label);cell.resources[name.replace('Label','')]=resourceDelta(b,a);}
        const cacheAfter=await cacheFiles(cacheDir);cell.cache={beforeBytes:Object.values(cacheBefore).reduce((a,b)=>a+b,0),afterBytes:Object.values(cacheAfter).reduce((a,b)=>a+b,0),newFiles:Object.entries(cacheAfter).filter(([key])=>!(key in cacheBefore)).map(([path,bytes])=>({path,bytes}))};
        for(const view of pages){cell.network.push(...view.network);cell.errors.push(...view.errors);lastEndpoints={...lastEndpoints,...view.endpoints};await view.context.close();}
        cell.passed=cell.errors.length===0&&cell.trials.length===sessions&&cell.network.some(n=>n.origin===nodeOrigin&&n.path.startsWith('/stream/v1/'));
        await fsp.writeFile(path.join(out,'report.json'),JSON.stringify(report,null,2)+'\n');
      }
      if(httpSamples&&sessions===groups.at(-1))await measureHttp(lastEndpoints||{});
      await stopRust();
    }
    if(values.robustness)await runRobustness();
    if(values['close-during-preparation'])await runCloseDuringPreparation();
    report.summary={};
    for(const sessions of groups)for(const phase of ['cold-app-preparation','warm-server-new-browser']){
      const cells=report.cells.filter(c=>c.sessions===sessions&&c.phase===phase),trials=cells.flatMap(c=>c.trials.filter(t=>t.open)),actions=trials.flatMap(t=>t.actions);
      report.summary[`${sessions}-${phase}`]={
        runs:cells.length,sessionAttempts:cells.reduce((n,c)=>n+c.sessions,0),sessionObservations:trials.length,failures:cells.filter(c=>!c.passed).length,
        openingOutcomes:latencyOutcomes(cells.flatMap(c=>c.trials)),
        clickToRawPcmMs:stats(cells.flatMap(c=>c.trials).map(t=>(t.open?.trial||t.diagnostic?.trial)?.firstRawPcmMs)),
        initialSignedDriftSeconds:stats(cells.flatMap(c=>c.trials).map(t=>(t.open?.trial||t.diagnostic?.trial)?.initialPcm?.signedDriftSeconds)),
        clickToFrameMs:stats(trials.map(t=>t.open.trial.firstFrameMs)),clickToPcmMs:stats(trials.map(t=>t.open.trial.firstAudioMs)),
        switchPcmMs:stats(actions.filter(a=>a.trial.kind==='switch').map(a=>a.trial.firstAudioMs)),
        seekFrameMs:stats(actions.filter(a=>a.trial.kind==='seek').map(a=>a.trial.firstFrameMs)),seekPcmMs:stats(actions.filter(a=>a.trial.kind==='seek').map(a=>a.trial.firstAudioMs)),
        preparedReopenFrameMs:stats(actions.filter(a=>a.trial.kind==='reopen').map(a=>a.trial.firstRestoredFrameMs)),preparedReopenPcmMs:stats(actions.filter(a=>a.trial.kind==='reopen').map(a=>a.trial.firstAudioMs)),
        preparedReopenPolicy:actions.filter(a=>a.trial.kind==='reopen').map(a=>({watched:a.precondition.watched,expectedTime:a.precondition.expectedResumeTime,frames:a.videoLoadDelta,mediaResponses:a.mediaResponses}))
      };
    }
    report.passed=report.cells.every(c=>c.passed)&&(!report.robustness||report.robustness.passed)&&(!report.closeDuringPreparation||report.closeDuringPreparation.passed)&&(!report.http||report.http.every(h=>h.failures===0));if(!report.passed)process.exitCode=1;
  }catch(error){report.failure=scrub(error.stack);process.exitCode=1;}
  finally{
    if(browser)await browser.close();if(api)await api.dispose();await stopRust();if(metrics){metrics.stdin.end('stop\n');await new Promise(resolve=>{if(metrics.exitCode!==null)resolve();else metrics.once('exit',resolve);});}if(proxy){report.tcpProfile=proxy.snapshot();await proxy.close();}
    report.fixture??={path:path.relative(root,fixture)};report.fixture.sha256=await hash(fixture);
    if(sealed){const files=await fileManifest(artifactDir);delete files['manifest.json'];report.artifactsUnchanged=JSON.stringify(files)===JSON.stringify(sealed.files);if(!report.artifactsUnchanged)process.exitCode=1;}
    report.metricsCollectorStderr=scrub(metricsStderr);const serialized=JSON.stringify(report,null,2);for(const secret of secrets)if(secret&&serialized.includes(secret))throw new Error('Refusing report containing a runtime secret');await fsp.writeFile(path.join(out,'report.json'),serialized+'\n');console.log(JSON.stringify({out,passed:report.passed,failure:report.failure,summary:report.summary},null,2));
  }
})();
