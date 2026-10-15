const fs=require('node:fs'),path=require('node:path'),os=require('node:os');
const {createHash}=require('node:crypto');
const {spawn}=require('node:child_process');
const {firefoxAdapter,playwrightAdapter}=require('./browser-matrix.cjs');
const {initializeComparison}=require('./player-comparison-checks.cjs');
const out=path.resolve(process.env.MEDIA_REPORT_DIR||'artifacts/player-comparison');fs.mkdirSync(out,{recursive:true});
const channel=process.env.MEDIA_BROWSER_CHANNEL||'chrome',engine=process.env.MEDIA_PLAYER_ENGINE||'shaka';
const root=path.resolve('tests/fixtures/generated');
function hashes(){const files=['representative/representative.mkv','representative/representative.mp4',...['mse/verified','dash'].flatMap(d=>['video.mp4','audio-fr.mp4','audio-en.mp4'].map(n=>d+'/'+n))];return Object.fromEntries(files.map(n=>[n,createHash('sha256').update(fs.readFileSync(path.join(root,n))).digest('hex')]));}
(async()=>{
  if(!['mse','shaka'].includes(engine))throw new Error('Unknown engine');
  const r={date:new Date().toISOString(),channel,engine,os:os.release(),headless:true,hashesBefore:hashes(),method:'common FFT8192/RMS1024; 3 stable samples; 3 rounds x6 switches; A/V marker tolerance200ms; physical output NT'};
  let browser,sampler,samplerLog;
  const stopPath=path.join(out,'resources.stop');
  try{
    browser=await(channel==='firefox'?firefoxAdapter():playwrightAdapter());r.browser=browser.version;r.driver=browser.driver;
    await browser.goto('http://127.0.0.1:3000/'+engine);await browser.click('#start');
    r.initial=await browser.evaluate(initializeComparison,engine);
    if(engine==='shaka'){
      await browser.evaluate(()=>window.comparisonHarness.seek(4.25,'fr'));
      r.defaultSubtitle=await browser.evaluate(()=>{const v=document.querySelector('video');return Array.from(v.textTracks).filter(t=>t.mode==='showing').flatMap(t=>Array.from(t.activeCues||[],c=>c.text));});
      if(!r.defaultSubtitle.includes('FR FORCE CUE 2'))throw new Error('Default forced French cue not displayed');
      await browser.screenshot('default-forced-fr.png');
    }
    await browser.evaluate(()=>window.comparisonHarness.manualSubtitlePreference());
    if(process.env.MEDIA_SAMPLE_RESOURCES==='1'){
      if(!browser.pid||!process.env.MEDIA_SERVER_PID)throw new Error('Resource sampling needs browser and server PID');
      if(fs.existsSync(stopPath)||fs.existsSync(path.join(out,'resources.jsonl')))throw new Error('Use a fresh resource report directory');
      samplerLog=fs.openSync(path.join(out,'sampler.log'),'w');
      sampler=spawn('powershell.exe',['-NoProfile','-File',path.resolve('scripts/sample-process-resources.ps1'),'-BrowserPid',String(browser.pid),'-ServerPid',process.env.MEDIA_SERVER_PID,'-OutputPath',path.join(out,'resources.jsonl'),'-StopPath',stopPath],{windowsHide:true,stdio:['ignore',samplerLog,samplerLog]});
      sampler.on('error',error=>{r.resourceSamplerFailure=String(error);});
      const end=Date.now()+7000;while(!fs.existsSync(path.join(out,'resources.jsonl'))){if(Date.now()>end)throw new Error('Sampler startup timeout');await new Promise(resolve=>setTimeout(resolve,100));}
      r.resources={file:'resources.jsonl',browserRootPid:browser.pid,serverPid:Number(process.env.MEDIA_SERVER_PID),scope:'warm playback/seek/switch; observer concurrent, overhead recorded; sum WorkingSet may double count shared pages'};
    }
    for(let round=1;round<=3;round++){
      await browser.evaluate(()=>window.comparisonHarness.seek(.3,'fr'));
      for(const language of ['en','fr'])await browser.evaluate(arg=>window.comparisonHarness.change(arg),{language,mode:'playing',round});
      for(const language of ['en','fr'])await browser.evaluate(arg=>window.comparisonHarness.change(arg),{language,mode:'paused',round});
      await browser.evaluate(()=>window.comparisonHarness.seek(3.25,'fr'));
      await browser.evaluate(arg=>window.comparisonHarness.change(arg),{language:'en',mode:'after-seek',round});
      await browser.evaluate(arg=>window.comparisonHarness.marker(arg),{target:6,language:'en',round});
      await browser.evaluate(()=>window.comparisonHarness.seek(3.25,'en'));
      await browser.evaluate(arg=>window.comparisonHarness.change(arg),{language:'fr',mode:'after-seek',round});
      await browser.evaluate(arg=>window.comparisonHarness.marker(arg),{target:9,language:'fr',round});
    }
    for(const key of ['fr-full','fr-forced','en-full','off']){await browser.evaluate(key=>window.comparisonHarness.subtitles(key),key);await browser.screenshot('subtitle-'+key+'.png');}
    r.result=await browser.evaluate(()=>window.comparisonHarness.finish());
    if(!r.result.passed)process.exitCode=1;
  }catch(error){r.failure=String(error);if(browser)r.result=await browser.evaluate(()=>window.comparisonReport).catch(()=>null);process.exitCode=1;}
  finally{
    if(sampler){fs.writeFileSync(stopPath,'stop');await Promise.race([new Promise(resolve=>{if(sampler.exitCode!==null)resolve();else sampler.once('exit',resolve);}),new Promise(resolve=>setTimeout(resolve,4000))]);if(sampler.exitCode===null){sampler.kill();r.resourceSamplerFailure='Sampler did not stop promptly';}else if(sampler.exitCode!==0)r.resourceSamplerFailure='Sampler exit '+sampler.exitCode;fs.closeSync(samplerLog);}
    if(browser)await browser.close();r.hashesAfter=hashes();r.originalsUnchanged=JSON.stringify(r.hashesBefore)===JSON.stringify(r.hashesAfter);if(!r.originalsUnchanged)process.exitCode=1;
    fs.writeFileSync(path.join(out,'report.json'),JSON.stringify(r,null,2));console.log(JSON.stringify({engine,channel,browser:r.browser,passed:r.result?.passed,failure:r.failure,switches:r.result?.switches?.length,markers:r.result?.markers,unchanged:r.originalsUnchanged}));
  }
})().catch(error=>{console.error(error);process.exitCode=1;});
