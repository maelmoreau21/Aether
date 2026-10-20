// Identical measurement graph and tolerances for the unchanged MSE bench and Shaka.
async function initializeComparison(engine) {
  const v=document.querySelector('video'),b=engine==='shaka'?window.shakaBench:window.mseBench;
  const report=window.comparisonReport={engine,passed:false,switches:[],markers:[],seeks:[],subtitles:[],errors:[],physicalMultichannel:'NT',physical10bit:'NT'};
  const wait=ms=>new Promise(resolve=>setTimeout(resolve,ms));
  const bounded=async(promise,label)=>{let timer;try{return await Promise.race([promise,new Promise((_,reject)=>{timer=setTimeout(()=>reject(new Error('Timeout '+label)),6000);})]);}finally{clearTimeout(timer);}};
  const until=async(fn,label,timeout=6000)=>{const end=performance.now()+timeout;while(!fn()){if(v.error)throw new Error(`MediaError ${v.error.code}: ${v.error.message}`);if(b.errors.length)throw new Error(JSON.stringify(b.errors));if(performance.now()>end)throw new Error('Timeout '+label);await wait(10);}};
  await until(()=>engine==='shaka'?b.ready:b.language==='fr','prepare');
  await until(()=>v.readyState>=2,'initial data');
  const context=new AudioContext(),source=context.createMediaElementSource(v),splitter=context.createChannelSplitter(6);
  source.connect(context.destination);source.connect(splitter);
  const analysers=Array.from({length:6},(_,i)=>{const a=context.createAnalyser();a.fftSize=8192;a.smoothingTimeConstant=0;splitter.connect(a,i);return a;});
  await bounded(context.resume(),'context resume');
  const signal=()=>analysers.map(a=>{const window=new Float32Array(a.fftSize);a.getFloatTimeDomainData(window);const pcm=window.subarray(window.length-1024);const fft=new Float32Array(a.frequencyBinCount);a.getFloatFrequencyData(fft);let peak=1;for(let i=2;i<fft.length;i++)if(fft[i]>fft[peak])peak=i;return{rms:Math.sqrt(pcm.reduce((sum,x)=>sum+x*x,0)/pcm.length),hz:peak*context.sampleRate/a.fftSize};});
  const matches=language=>{const s=signal(),expected=language==='fr'?[440,550,660,80,770,880]:[990,1100];return expected.every((hz,i)=>s[i].rms>.001&&Math.abs(s[i].hz-hz)<15)&&(language==='fr'||s.slice(2).every(c=>c.rms<.001));};
  const stable=async(language,timeout=6000)=>{let n=0;await until(()=>{n=matches(language)?n+1:0;return n>=3;},'PCM '+language,timeout);};
  const canvas=document.createElement('canvas');canvas.width=32;canvas.height=18;const ctx=canvas.getContext('2d',{willReadFrequently:true});
  const pixels=()=>{ctx.drawImage(v,0,0,32,18);const p=ctx.getImageData(0,0,32,18).data;let sum=0;const colors=new Set();for(let i=0;i<p.length;i+=4){sum+=(p[i]+p[i+1]+p[i+2])/3;colors.add(p[i]+','+p[i+1]+','+p[i+2]);}return{brightness:sum/(p.length/4),colors:colors.size};};
  let frames=[],frameId,marker=null;
  const frame=()=>{frameId=v.requestVideoFrameCallback((at,m)=>{frames.push({at,pts:m.mediaTime,presentedFrames:m.presentedFrames});if(marker&&!marker.video&&Math.abs(m.mediaTime-marker.target)<.2&&pixels().brightness>235)marker.video={at,pts:m.mediaTime};frame();});};frame();
  await bounded(v.play(),'initial play');await stable('fr');await wait(350);
  report.initial={signal:signal(),pixels:pixels(),src:v.src,version:engine==='shaka'?b.version:null,videoDimensions:[v.videoWidth,v.videoHeight]};
  if(report.initial.pixels.colors<10)throw new Error('Video colors not decoded');
  report.defaultFrench=true;report.defaultAudioTracks=engine==='shaka'?b.player.getAudioTracks():null;
  report.defaultTextTracks=engine==='shaka'?b.player.getTextTracks():null;
  report.defaultForcedFrench=engine==='shaka'?report.defaultTextTracks.some(t=>t.active&&t.language==='fr'&&t.forced):null;
  report.actualLoadMode=engine==='shaka'?b.player.getLoadMode():null;
  if(engine==='shaka'&&(!report.defaultForcedFrench||report.actualLoadMode!==shaka.Player.LoadMode.MEDIA_SOURCE))throw new Error('French forced default or MSE load mode not observed');
  report.baselineVideoGapMs=frames.reduce((max,f,i)=>i?Math.max(max,f.at-frames[i-1].at):max,0);
  const api={report,signal};
  api.manualSubtitlePreference=async()=>{
    if(engine==='shaka'){b.selectSubtitle('en-full');await until(()=>b.player.getTextTracks().some(t=>t.active&&t.language==='en'),'manual EN subtitle');}
    else Array.from(v.textTracks).forEach((t,i)=>{t.mode=i===2?'showing':'disabled';});
    report.manualSubtitleRequested='en';
  };
  api.seek=async(position,language)=>{v.pause();v.currentTime=position;await until(()=>!v.seeking&&v.readyState>=2,'seek '+position);await bounded(v.play(),'seek play');await stable(language);const result={requested:position,observed:v.currentTime,language,selectedAudio:engine==='shaka'?b.player.getAudioTracks().find(t=>t.active)?.language:b.language};report.seeks.push(result);if(result.selectedAudio!==language)throw new Error('Manual audio selection lost after seek');return result;};
  api.change=async({language,mode,round,safeMargin=0,sampleAfterSignalMs=0})=>{
    if(mode==='paused')v.pause();else if(v.paused)await bounded(v.play(),'change play');
    const start=performance.now(),position=v.currentTime,paused=v.paused,src=v.src,events=b.events.length;
    const qualityBefore=v.getVideoPlaybackQuality?.();
    const buffersBefore=engine==='shaka'?b.player.getBufferedInfo():null;
    let silence=0,last=start,polls=[],silenceRun=0,maxSilenceRun=0,progress=[];
    const sample=setInterval(()=>{const now=performance.now(),dt=now-last;polls.push(dt);if(signal().every(c=>c.rms<.001)){silence+=dt;silenceRun+=dt;maxSilenceRun=Math.max(maxSilenceRun,silenceRun);}else silenceRun=0;progress.push({at:now,time:v.currentTime});last=now;},10);
    let ui,resumeAt=null,pausePreserved=null,detected;
    try{
      ui=await bounded(b.switchLanguage(language,safeMargin),'switch API');
      if(paused){await wait(100);pausePreserved=v.paused&&Math.abs(v.currentTime-position)<.05;if(!pausePreserved)throw new Error('Pause not preserved');resumeAt=performance.now();await bounded(v.play(),'paused resume');}
      await stable(language,Math.max(6000,safeMargin*1000+5000));
      detected=performance.now();
      if(sampleAfterSignalMs)await wait(sampleAfterSignalMs);
    }finally{clearInterval(sample);}
    const observedEnd=performance.now();
    if(!sampleAfterSignalMs)await wait(120);
    const near=frames.filter(f=>f.at>=start-100&&f.at<=(sampleAfterSignalMs?observedEnd:detected));let maxGap=0;for(let i=1;i<near.length;i++)maxGap=Math.max(maxGap,near[i].at-near[i-1].at);
    const during=near.filter(f=>f.at>=start);const selected=engine==='shaka'?b.player.getAudioTracks().find(t=>t.active)?.language:b.language;
    const result={round,language,mode,ui,requestToSignalMs:detected-start,resumeToSignalMs:resumeAt===null?null:detected-resumeAt,pausePreserved,sourceUnchanged:v.src===src,selectedLanguage:selected,signal:signal(),sampledSilenceTotalMs:silence,maxSampledSilenceRunMs:maxSilenceRun,maxPollMs:Math.max(0,...polls),maxVideoCallbackGapMs:maxGap,videoFramesDuring:during.length,videoPtsProgress:during.length>1?during.at(-1).pts-during[0].pts:0,positionAdvance:v.currentTime-position,events:b.events.slice(events),passed:v.src===src&&selected===language&&(!paused||pausePreserved)};
    const qualityAfter=v.getVideoPlaybackQuality?.();
    result.safeMargin=safeMargin;result.postSignalObservationMs=sampleAfterSignalMs;
    result.buffersBefore=buffersBefore;result.buffersAfter=engine==='shaka'?b.player.getBufferedInfo():null;
    result.videoQuality=qualityBefore&&qualityAfter?{droppedFramesDelta:qualityAfter.droppedVideoFrames-qualityBefore.droppedVideoFrames,totalFramesDelta:qualityAfter.totalVideoFrames-qualityBefore.totalVideoFrames}:null;
    result.callbackPresentedCounterSkips=during.reduce((sum,f,i)=>i?sum+Math.max(0,f.presentedFrames-during[i-1].presentedFrames-1):sum,0);
    result.videoContinuityObserved=paused?null:result.videoFramesDuring>=2&&result.videoPtsProgress>0;
    result.manualSubtitlePreserved=engine==='shaka'?b.player.getTextTracks().some(t=>t.active&&t.language==='en'):Array.from(v.textTracks).some(t=>t.mode==='showing'&&t.language==='en');
    if(!result.manualSubtitlePreserved)result.passed=false;
    if(!paused&&!result.videoContinuityObserved)result.passed=false;
    report.switches.push(result);if(!result.passed)throw new Error('Switch invariants failed');if(paused)v.pause();return result;
  };
  api.marker=async({target,language,round})=>{
    v.pause();v.currentTime=target-.4;await until(()=>!v.seeking,'marker seek');
    marker={target,language,round};await bounded(v.play(),'marker play');
    let last=performance.now(),maxPoll=0;
    await until(()=>{const now=performance.now();maxPoll=Math.max(maxPoll,now-last);last=now;if(!marker.audio&&signal()[0].rms>.03&&Math.abs(v.currentTime-target)<.25)marker.audio={at:now,mediaTime:v.currentTime};return marker.video&&marker.audio;},'A/V marker');
    const result={...marker,mediaClockSkewSeconds:marker.audio.mediaTime-marker.video.pts,wallCallbackSkewMs:marker.audio.at-marker.video.at,maxPollMs:maxPoll};
    result.passed=Math.abs(result.mediaClockSkewSeconds)<=.2;report.markers.push(result);marker=null;if(!result.passed)throw new Error('A/V marker outside 200ms tolerance');return result;
  };
  api.subtitles=async key=>{
    v.pause();v.currentTime=4.25;await until(()=>!v.seeking,'subtitle seek');
    if(engine==='shaka')b.selectSubtitle(key);else{const index=['fr-full','fr-forced','en-full'].indexOf(key);Array.from(v.textTracks).forEach((t,i)=>{t.mode=i===index?'showing':'disabled';});}
    const expected={'fr-full':'FR COMPLET CUE 2','fr-forced':'FR FORCE CUE 2','en-full':'EN FULL CUE 2'}[key];
    const active=()=>Array.from(v.textTracks).filter(t=>t.mode==='showing').flatMap(t=>Array.from(t.activeCues||[],c=>c.text));
    await until(()=>key==='off'?active().length===0:active().includes(expected),'subtitle '+key);
    const result={key,activeCues:active(),selected:engine==='shaka'?b.player.getTextTracks().filter(t=>t.active):null,passed:key==='off'?active().length===0:active().includes(expected)};report.subtitles.push(result);return result;
  };
  api.probeBackwardSeek=async({position,expectedLanguage,previousLanguage})=>{
    v.pause();v.currentTime=position;await until(()=>!v.seeking,'backward probe seek');await bounded(v.play(),'backward probe play');await wait(400);
    const result={position:v.currentTime,expectedLanguage,previousLanguage,selectedLanguage:engine==='shaka'?b.player.getAudioTracks().find(t=>t.active)?.language:b.language,signal:signal(),expectedSignal:matches(expectedLanguage),previousSignal:matches(previousLanguage)};
    result.matchesSelectedLanguage=result.expectedSignal&&result.selectedLanguage===expectedLanguage;
    return result;
  };
  api.finish=async(expected={switches:18,markers:6,subtitles:4})=>{
    v.pause();v.cancelVideoFrameCallback(frameId);report.errors=b.errors;report.events=b.events;
    report.finalConfig=engine==='shaka'?{preferredAudio:b.player.getConfiguration().preferredAudio,preferredText:b.player.getConfiguration().preferredText,accessibility:b.player.getConfiguration().accessibility,abr:b.player.getConfiguration().abr.enabled}:null;
    report.frames=frames.length;report.passed=report.switches.length===expected.switches&&report.switches.every(s=>s.passed)&&report.markers.length===expected.markers&&report.markers.every(m=>m.passed)&&report.subtitles.length>=expected.subtitles&&report.subtitles.every(t=>t.passed)&&!b.errors.length;
    await bounded(context.close(),'context close');return report;
  };
  window.comparisonHarness=api;
  return{initial:report.initial,defaultAudioTracks:report.defaultAudioTracks,defaultTextTracks:report.defaultTextTracks};
}
module.exports={initializeComparison};
