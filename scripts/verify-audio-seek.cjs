// Narrow verification: reselecting an already active track with margin 0.
const fs=require('node:fs'),path=require('node:path');
const {createHash}=require('node:crypto');
const {firefoxAdapter,playwrightAdapter}=require('./browser-matrix.cjs');
const {initializeComparison}=require('./player-comparison-checks.cjs');
const out=path.resolve(process.env.MEDIA_REPORT_DIR||'artifacts/audio-seek-check');fs.mkdirSync(out,{recursive:true});
const reportPath=path.join(out,'report.json');if(fs.existsSync(reportPath))throw new Error('Refusing overwrite');
const originals=JSON.parse(fs.readFileSync('tests/fixtures/generated/dash/generation.json')).original_sha256_before;
const method=process.env.MEDIA_SEEK_RECOVERY||'reselect',from=process.env.MEDIA_SEEK_FROM||'fr';
if(!['reselect','variant-clear'].includes(method)||!['fr','en'].includes(from))throw new Error('Invalid recovery experiment');
const to=from==='fr'?'en':'fr';
const hashes=()=>Object.fromEntries(Object.keys(originals).map(p=>[p,createHash('sha256').update(fs.readFileSync(p)).digest('hex')]));
(async()=>{
  const report={date:new Date().toISOString(),channel:process.env.MEDIA_BROWSER_CHANNEL||'chrome',from,to,method,scope:'Single switch margin0.5; backward seek; explicit public recovery API; not automatic or a stability benchmark',before:hashes()};let browser;
  try{
    browser=await(report.channel==='firefox'?firefoxAdapter():playwrightAdapter());report.browser=browser.version;
    await browser.goto('http://127.0.0.1:3000/shaka');await browser.click('#start');
    await browser.evaluate(initializeComparison,'shaka');await browser.evaluate(()=>window.shakaBench.seekPolicy.setEnabled(false));await browser.evaluate(()=>window.comparisonHarness.manualSubtitlePreference());
    if(from==='en')await browser.evaluate(()=>window.comparisonHarness.change({language:'en',mode:'warmup',round:0}));
    await browser.evaluate(language=>window.comparisonHarness.seek(1.25,language),from);
    await browser.evaluate(()=>new Promise(resolve=>setTimeout(resolve,500)));
    report.switch=await browser.evaluate(language=>window.comparisonHarness.change({language,mode:'playing',round:1,safeMargin:.5,sampleAfterSignalMs:500}),to);
    report.backwardSeek=await browser.evaluate(arg=>window.comparisonHarness.probeBackwardSeek(arg),{position:.5,expectedLanguage:to,previousLanguage:from});
    report.reselection=await browser.evaluate(async({method,to})=>{
      const b=window.shakaBench,v=document.querySelector('video'),src=v.src,track=b.player.getAudioTracks().find(t=>t.active);
      const qualityBefore=v.getVideoPlaybackQuality(),events=b.events.length,start=performance.now(),position=v.currentTime;
      const correct=()=>{const a=window.comparisonHarness.signal(),hz=to==='en'?[990,1100]:[440,550,660,80,770,880];return hz.every((f,i)=>a[i].rms>.001&&Math.abs(a[i].hz-f)<15)&&(to==='fr'||a.slice(2).every(c=>c.rms<.001));};
      if(method==='variant-clear')b.player.selectVariantTrack(b.player.getVariantTracks().find(t=>t.active),true,0);
      else b.player.selectAudioTrack(track,0);
      let consecutive=0,detected=null;
      while(performance.now()-start<5000){consecutive=correct()?consecutive+1:0;if(consecutive>=3){detected=performance.now();break;}await new Promise(resolve=>setTimeout(resolve,20));}
      await new Promise(resolve=>setTimeout(resolve,700));
      const signal=window.comparisonHarness.signal();
      const qualityAfter=v.getVideoPlaybackQuality();
      return{selectedLanguage:track.language,signal,requestToCorrectSignalMs:detected===null?null:detected-start,sourceUnchanged:v.src===src,restoredSelectedLanguage:correct(),positionAdvance:v.currentTime-position,events:b.events.slice(events),videoFramesDelta:qualityAfter.totalVideoFrames-qualityBefore.totalVideoFrames,droppedVideoFramesDelta:qualityAfter.droppedVideoFrames-qualityBefore.droppedVideoFrames};
    },{method,to});
    await browser.evaluate(switches=>window.comparisonHarness.finish({switches,markers:0,subtitles:0}),from==='en'?2:1);
  }catch(error){report.failure=String(error);process.exitCode=1;}
  finally{if(browser)await browser.close();report.after=hashes();report.originalsUnchanged=JSON.stringify(report.before)===JSON.stringify(report.after);if(!report.originalsUnchanged)process.exitCode=1;fs.writeFileSync(reportPath,JSON.stringify(report,null,2));console.log(JSON.stringify({channel:report.channel,from,to,method,failure:report.failure,seekMatchesSelected:report.backwardSeek?.matchesSelectedLanguage,recovered:report.reselection?.restoredSelectedLanguage,latency:report.reselection?.requestToCorrectSignalMs,sourceUnchanged:report.reselection?.sourceUnchanged,originalsUnchanged:report.originalsUnchanged}));}
})().catch(error=>{console.error(error);process.exitCode=1;});
