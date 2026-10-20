// Paired raw-control / guarded seeks. FFT is exclusively a test oracle.
const fs=require('node:fs'),path=require('node:path'),{createHash}=require('node:crypto');
const {firefoxAdapter,playwrightAdapter}=require('./browser-matrix.cjs');
const {initializeComparison}=require('./player-comparison-checks.cjs');
const channel=process.env.MEDIA_BROWSER_CHANNEL||'chrome',out=process.env.MEDIA_REPORT_DIR||`artifacts/seek-policy-${channel}`;
fs.mkdirSync(out,{recursive:true});const file=path.join(out,'trials.jsonl');if(fs.existsSync(file))throw new Error('Refusing overwrite');
const append=x=>fs.appendFileSync(file,JSON.stringify(x)+'\n');
const originals=JSON.parse(fs.readFileSync('tests/fixtures/generated/dash/generation.json')).original_sha256_before;
const hashes=()=>Object.fromEntries(Object.keys(originals).map(p=>[p,createHash('sha256').update(fs.readFileSync(p)).digest('hex')]));
(async()=>{const before=hashes();let browser,failed=0;
 try{browser=await(channel==='firefox'?firefoxAdapter():playwrightAdapter());append({type:'metadata',channel,browser:browser.version,before});
 for(const from of ['fr','en'].filter(x=>!process.env.MEDIA_SEEK_ONLY_FROM||x===process.env.MEDIA_SEEK_ONLY_FROM))for(const position of [.5,1.99,2.01].filter(x=>!process.env.MEDIA_SEEK_ONLY_POSITION||x===Number(process.env.MEDIA_SEEK_ONLY_POSITION)))for(const guarded of [false,true]){
  const to=from==='fr'?'en':'fr',paused=position===1.99,rate=position===2.01?1.25:1,trial={type:'trial',from,to,position,paused,rate,guarded};
  try{
   await browser.goto('http://127.0.0.1:3000/shaka');await browser.click('#start');await browser.evaluate(initializeComparison,'shaka');
   await browser.evaluate(()=>window.shakaBench.seekPolicy.setEnabled(false));await browser.evaluate(()=>window.comparisonHarness.manualSubtitlePreference());
   if(from==='en')await browser.evaluate(()=>window.comparisonHarness.change({language:'en',mode:'warmup',round:0}));
   const preparationPosition=Number(process.env.MEDIA_SEEK_PREPARATION_POSITION||3.25);
   if(![1.25,3.25].includes(preparationPosition))throw new Error('Invalid preparation position');
   trial.preparationPosition=preparationPosition;
   await browser.evaluate(arg=>window.comparisonHarness.seek(arg.position,arg.language),{position:preparationPosition,language:from});
   await browser.evaluate(()=>new Promise(r=>setTimeout(r,500)));
   trial.switch=await browser.evaluate(async language=>{
    try{return await window.comparisonHarness.change({language,mode:'playing',round:1,safeMargin:.5,sampleAfterSignalMs:500});}
    catch(error){const r=window.comparisonReport.switches.at(-1);if(r?.sourceUnchanged&&r.selectedLanguage===language&&r.manualSubtitlePreserved&&!r.videoContinuityObserved)return {...r,preparationWarning:String(error)};throw error;}
   },to);
   trial.seek=await browser.evaluate(async arg=>{
    const b=window.shakaBench,v=document.querySelector('video'),h=window.comparisonHarness,wait=ms=>new Promise(r=>setTimeout(r,ms));
    const matches=()=>{const s=h.signal(),hz=arg.to==='fr'?[440,550,660,80,770,880]:[990,1100];return hz.every((f,i)=>s[i].rms>.001&&Math.abs(s[i].hz-f)<15)&&(arg.to==='fr'||s.slice(2).every(c=>c.rms<.001));};
    v.playbackRate=arg.rate;if(arg.paused)v.pause();b.seekPolicy.setEnabled(arg.guarded);
    const src=v.src,quality=v.getVideoPlaybackQuality(),eventStart=b.events.length,requestStart=b.requests.length,start=performance.now();
    let frameId,frames=[],samples=[],stable=0,detected=null;
    const frame=()=>{frameId=v.requestVideoFrameCallback((at,m)=>{frames.push({at,pts:m.mediaTime});frame();});};frame();
    v.currentTime=arg.position;
    while(performance.now()-start<6500){
     const now=performance.now();samples.push({at:now,silent:h.signal().every(c=>c.rms<.001)});
     if(!v.seeking&&b.seekPolicy.state.phase!=='recovering')break;await wait(10);
    }
    const settled={position:v.currentTime,paused:v.paused,rate:v.playbackRate,phase:b.seekPolicy.state.phase};
    if(arg.paused)await v.play();
    const resumed=performance.now();
    while(performance.now()-resumed<3500){
     const now=performance.now();samples.push({at:now,silent:h.signal().every(c=>c.rms<.001)});stable=matches()?stable+1:0;
     if(stable>=3&&detected===null)detected=now;await wait(10);
    }
    v.cancelVideoFrameCallback(frameId);
    let silence=0,run=0,maxRun=0,maxPoll=0;for(let i=1;i<samples.length;i++){const dt=samples[i].at-samples[i-1].at;maxPoll=Math.max(maxPoll,dt);if(samples[i].silent){silence+=dt;run+=dt;maxRun=Math.max(maxRun,run);}else run=0;}
    const q=v.getVideoPlaybackQuality();return{settled,selected:b.player.getVariantTracks().filter(t=>t.active),audioTracks:b.player.getAudioTracks(),signal:h.signal(),correct:matches(),requestToSignalMs:detected===null?null:detected-start,resumeToSignalMs:detected===null?null:detected-resumed,sourceUnchanged:src===v.src,sampledSilenceMs:silence,maxSilenceRunMs:maxRun,maxPollMs:maxPoll,maxVideoGapMs:frames.reduce((m,f,i)=>i?Math.max(m,f.at-frames[i-1].at):m,0),frames:frames.length,droppedFrames:q.droppedVideoFrames-quality.droppedVideoFrames,history:b.seekPolicy.history,state:b.seekPolicy.state,events:b.events.slice(eventStart),requestsBefore:b.requests.slice(0,requestStart),requests:b.requests.slice(requestStart),errors:b.errors};
   },{position,paused,rate,guarded,to});
   const s=trial.seek;
   trial.passed=s.sourceUnchanged&&s.settled.paused===paused&&s.settled.rate===rate&&s.errors.length===0&&(guarded?s.correct&&s.state.phase==='idle'&&s.history.length===1&&s.history[0].success&&s.history[0].attempts===1&&Math.abs(s.history[0].result.positionError)<=.04:!s.correct);
   if(guarded&&trial.passed)trial.marker=await browser.evaluate(arg=>window.comparisonHarness.marker(arg),{target:9,language:to,round:1});
  }catch(error){trial.failure=String(error);trial.partial=await browser.evaluate(()=>({state:window.shakaBench?.seekPolicy?.state,history:window.shakaBench?.seekPolicy?.history})).catch(()=>null);trial.passed=false;}
  if(!trial.passed)failed++;append(trial);console.log(JSON.stringify({channel,from,position,guarded,passed:trial.passed,failure:trial.failure,recovery:trial.seek?.history,correct:trial.seek?.correct}));
 }
 }finally{if(browser)await browser.close();const after=hashes(),unchanged=JSON.stringify(before)===JSON.stringify(after);append({type:'end',failed,unchanged,after});if(failed||!unchanged)process.exitCode=1;}
})().catch(e=>{console.error(e);process.exitCode=1;});
