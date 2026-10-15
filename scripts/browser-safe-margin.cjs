const fs=require('node:fs'),path=require('node:path'),os=require('node:os');
const {createHash}=require('node:crypto');
const {firefoxAdapter,playwrightAdapter}=require('./browser-matrix.cjs');
const {initializeComparison}=require('./player-comparison-checks.cjs');
const channel=process.env.MEDIA_BROWSER_CHANNEL||'chrome';
const out=path.resolve(process.env.MEDIA_REPORT_DIR||`artifacts/safe-margin-${channel}`);
fs.mkdirSync(out,{recursive:true});
const streamPath=path.join(out,'trials.jsonl');
if(fs.existsSync(streamPath))throw new Error('Use a new output directory; refusing to overwrite trials');
const append=record=>fs.appendFileSync(streamPath,JSON.stringify(record)+'\n');
function hashes(){const root='tests/fixtures/generated';const source=JSON.parse(fs.readFileSync(root+'/dash/generation.json')).original_sha256_before;const names=[...Object.keys(source),root+'/dash/manifest.mpd',...['video.mp4','audio-fr.mp4','audio-en.mp4','fr-full.vtt','fr-forced.vtt','en-full.vtt'].map(n=>root+'/dash/'+n),root+'/vendor/shaka-5.2.13/shaka-player.compiled.js'];return Object.fromEntries(names.map(n=>[n,createHash('sha256').update(fs.readFileSync(n)).digest('hex')]));}
(async()=>{
  const margins=process.env.MEDIA_SAFE_MARGINS?process.env.MEDIA_SAFE_MARGINS.split(',').map(Number):[0,.5,1,2,4];
  if(!margins.length||margins.some(m=>![0,.5,1,2,4].includes(m)))throw new Error('Invalid margin list');
  const activeRounds=Number(process.env.MEDIA_SAFE_ACTIVE_ROUNDS||10),otherRounds=Number(process.env.MEDIA_SAFE_OTHER_ROUNDS||2);
  const onlyMode=process.env.MEDIA_SAFE_ONLY_MODE,onlyFrom=process.env.MEDIA_SAFE_ONLY_FROM;
  if((onlyMode&&!['playing','paused','after-seek'].includes(onlyMode))||(onlyFrom&&!['fr','en'].includes(onlyFrom)))throw new Error('Invalid supplementary trial filter');
  if(!Number.isInteger(activeRounds)||!Number.isInteger(otherRounds)||activeRounds<1||otherRounds<1||activeRounds>20||otherRounds>10)throw new Error('Invalid rounds');
  const metadata={type:'metadata',date:new Date().toISOString(),channel,os:os.release(),arch:os.arch(),margins,activeRounds,otherRounds,onlyMode:onlyMode||null,onlyFrom:onlyFrom||null,hashesBefore:hashes(),settings:'Shaka5.2.13/default streaming config; no resource sampler; 500ms post-signal; independent 12s presentation per target switch; rotated margin/direction order; failed observations retained'};
  append(metadata);let count=0,failed=0,seekRegressions=0;
  for(const mode of ['playing','paused','after-seek'].filter(m=>!onlyMode||m===onlyMode))for(let round=0;round<(mode==='playing'?activeRounds:otherRounds);round++){
    let browser;
    try{
      browser=await(channel==='firefox'?firefoxAdapter():playwrightAdapter());
      const ordered=margins.map((_,i)=>margins[(i+round)%margins.length]);
      for(const margin of ordered)for(const from of (round%2?['en','fr']:['fr','en']).filter(f=>!onlyFrom||f===onlyFrom)){
        const to=from==='fr'?'en':'fr';
        const trial={type:'trial',index:++count,channel,browser:browser.version,driver:browser.driver,mode,round:round+1,margin,from,to,passed:false};
        try{
          trial.stage='initialize';
          await browser.goto('http://127.0.0.1:3000/shaka');await browser.click('#start');
          await browser.evaluate(initializeComparison,'shaka');await browser.evaluate(()=>window.shakaBench.seekPolicy.setEnabled(false));await browser.evaluate(()=>window.comparisonHarness.manualSubtitlePreference());
          if(from==='en'){trial.stage='warmup';await browser.evaluate(()=>window.comparisonHarness.change({language:'en',mode:'warmup',round:0,safeMargin:0}));}
          trial.stage='precondition-seek';
          await browser.evaluate(language=>window.comparisonHarness.seek(1.25,language),from);
          if(mode==='after-seek')await browser.evaluate(language=>window.comparisonHarness.seek(3.25,language),from);
          else await browser.evaluate(()=>new Promise(resolve=>setTimeout(resolve,500)));
          trial.stage='target-switch';
          trial.switch=await browser.evaluate(arg=>window.comparisonHarness.change(arg),{language:to,mode,round:round+1,safeMargin:margin,sampleAfterSignalMs:500});
          trial.stage='marker';
          trial.marker=await browser.evaluate(arg=>window.comparisonHarness.marker(arg),{target:9,language:to,round:round+1});
          trial.stage='backward-seek';
          trial.backwardSeek=await browser.evaluate(arg=>window.comparisonHarness.probeBackwardSeek(arg),{position:.5,expectedLanguage:to,previousLanguage:from});
          trial.stage='finish';
          trial.report=await browser.evaluate(expected=>window.comparisonHarness.finish(expected),{switches:from==='en'?2:1,markers:1,subtitles:0});
          trial.passed=trial.report.passed&&trial.switch.passed&&trial.marker.passed;
          if(!trial.backwardSeek.matchesSelectedLanguage)seekRegressions++;
        }catch(error){trial.failure=String(error);trial.partial=await browser.evaluate(()=>window.comparisonReport).catch(()=>null);}
        if(!trial.passed)failed++;
        append(trial);fs.writeFileSync(path.join(out,'progress.json'),JSON.stringify({channel,count,failed,seekRegressions,mode,round:round+1,margin,from,to,lastFailure:trial.failure}));
        if(!trial.passed)console.log(JSON.stringify({channel,index:trial.index,mode,margin,from,failure:trial.failure}));
      }
    }finally{if(browser)await browser.close();}
    console.log(JSON.stringify({channel,completedMode:mode,round:round+1,trials:count,failed,seekRegressions}));
  }
  const after=hashes();const result={type:'end',date:new Date().toISOString(),channel,trials:count,failed,seekRegressions,hashesAfter:after,originalsUnchanged:JSON.stringify(metadata.hashesBefore)===JSON.stringify(after)};append(result);fs.writeFileSync(path.join(out,'result.json'),JSON.stringify(result,null,2));if(failed||!result.originalsUnchanged)process.exitCode=1;
})().catch(error=>{append({type:'runner_failure',failure:String(error)});console.error(error);process.exitCode=1;});
