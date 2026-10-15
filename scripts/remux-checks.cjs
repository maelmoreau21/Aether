// Browser-side P0C measurement. No codec/track result inferred from a label.
async function checkRemux({id}) {
  const v=document.querySelector('video');
  const selector=document.querySelector('#remux-language');
  const bench=window.mediaBench;
  const r={id,method:'prepared-remux-source-reload',switches:[],pausedSwitches:[],passed:false,
    measurement:{pollTargetMs:10,pcmWindowSamples:1024,fftSamples:8192,silenceRmsThreshold:0.001,
      limits:'Main-thread sampling and compositor callbacks, not physical speaker/display timing. FFT includes a rolling analysis window; no seamless playback claim.'}};
  const wait=ms=>new Promise(resolve=>setTimeout(resolve,ms));
  const until=async(test,label)=>{
    const end=performance.now()+12000;
    while(!test()) {
      if(v.error) throw new Error(`MediaError ${v.error.code}: ${v.error.message}`);
      if(performance.now()>end) throw new Error(`Timeout: ${label}`);
      await wait(10);
    }
  };
  let audio,small,videoCallback;
  let frames=[];
  const arm=()=>{
    if(videoCallback!==undefined) v.cancelVideoFrameCallback(videoCallback);
    videoCallback=v.requestVideoFrameCallback((now,meta)=>{
      frames.push({wall:now,mediaTime:meta.mediaTime,presentedFrames:meta.presentedFrames});
      arm();
    });
  };
  try {
    if(!['fr','en'].every(language=>bench.inventory.some(a=>a.language_remux?.source_asset===id && a.language_remux.language===language))) throw new Error('Prepare both language remuxes before this scenario');
    // readyState can change before the queued loadedmetadata UI handler runs.
    await until(()=>v.readyState>=2 && !selector.disabled,'initial data and enabled remux control');
    if(typeof v.requestVideoFrameCallback!=='function') throw new Error('Video frame callback unavailable: interruption measurement NT');
    if(!window.benchAudio) {
      const context=new AudioContext();
      const analyser=context.createAnalyser();analyser.fftSize=8192;analyser.smoothingTimeConstant=0;
      const source=context.createMediaElementSource(v);source.connect(analyser);analyser.connect(context.destination);
      window.benchAudio={context,analyser};
    }
    audio=window.benchAudio.context;
    const analyser=window.benchAudio.analyser;analyser.smoothingTimeConstant=0;
    small=audio.createAnalyser();small.fftSize=1024;analyser.connect(small);
    const signal=()=>{
      const pcm=new Float32Array(1024);small.getFloatTimeDomainData(pcm);
      const fft=new Float32Array(analyser.frequencyBinCount);analyser.getFloatFrequencyData(fft);
      let peak=0;for(let i=1;i<fft.length;i++) if(fft[i]>fft[peak]) peak=i;
      return {rms:Math.sqrt(pcm.reduce((s,x)=>s+x*x,0)/pcm.length),hz:peak*audio.sampleRate/analyser.fftSize};
    };
    const request=language=>{
      selector.value=language;selector.dispatchEvent(new Event('change',{bubbles:true}));
      return bench.pendingSwitch;
    };
    await audio.resume();v.addEventListener('loadedmetadata',arm);arm();
    await v.play();await until(()=>v.currentTime>0.8 && signal().rms>0.001,'initial FR signal');
    r.initialAudio=signal();r.sampleRate=audio.sampleRate;r.nativeAudioTracksExposed=!!v.audioTracks;
    if(Math.abs(r.initialAudio.hz-440)>20) throw new Error('Initial source is not the FR 440 Hz signal');
    for(const language of ['en','fr','en','fr','en','fr']) {
      await wait(160);
      const expectedHz=language==='fr'?440:880;
      const lastPre=frames.at(-1);
      const baseline=frames.filter(f=>f.wall>performance.now()-300);
      if(!lastPre) throw new Error('No baseline video frame observed');
      const start=performance.now();
      const trace=[];
      const sample=()=>trace.push({ms:performance.now()-start,...signal(),position:v.currentTime,readyState:v.readyState,paused:v.paused});
      sample();const timer=setInterval(sample,10);
      let ui,stableAt;
      try {
        ui=await request(language);
        if(!ui?.ok) throw new Error(ui?.error || 'Missing switch result');
        // Wait for three correct observations, not just buffered samples from the old source.
        let stable=0;
        await until(()=>{const s=signal();stable=(s.rms>0.001 && Math.abs(s.hz-expectedHz)<20)?stable+1:0;return stable>=3;},'new language PCM');
        stableAt=performance.now();
        await until(()=>frames.some(f=>f.wall>=start+ui.seekReadyMs && f.mediaTime>=ui.requestedPosition-0.1),'restored video frame after new-source seek');
        await wait(120);sample();
      } finally {clearInterval(timer);}
      const firstFrame=frames.find(f=>f.wall>=start+ui.seekReadyMs && f.mediaTime>=ui.requestedPosition-0.1);
      const silentIntervals=[];let silentStart=null;
      for(const s of trace) {
        if(s.rms<0.001 && silentStart===null) silentStart=s.ms;
        if(s.rms>=0.001 && silentStart!==null) {silentIntervals.push({fromMs:silentStart,toMs:s.ms,durationMs:s.ms-silentStart});silentStart=null;}
      }
      if(silentStart!==null) silentIntervals.push({fromMs:silentStart,toMs:trace.at(-1).ms,durationMs:trace.at(-1).ms-silentStart});
      const gaps=baseline.slice(1).map((f,i)=>f.wall-baseline[i].wall);
      const restoredFrames=frames.filter(f=>f.wall>=start+ui.seekReadyMs && f.mediaTime>=ui.requestedPosition-0.1);
      const wrongStartFrames=frames.filter(f=>f.wall>=start && f.mediaTime<ui.requestedPosition-0.1);
      const measured={language,expectedHz,ui,observedAudio:signal(),
        requestToStableAudioMs:stableAt-start,requestToRestoredVideoFrameMs:firstFrame.wall-start,
        firstRestoredFrameMediaTime:firstFrame.mediaTime,restoredFrameThresholdMs:ui.seekReadyMs,
        crossSourceFrameGapMs:firstFrame.wall-lastPre.wall,baselineMaxFrameGapMs:gaps.length?Math.max(...gaps):null,
        sampledSilenceTotalMs:silentIntervals.reduce((s,x)=>s+x.durationMs,0),silentIntervals,
        maxSampleGapMs:Math.max(...trace.slice(1).map((s,i)=>s.ms-trace[i].ms)),
        framesBeforeRestoredPosition:wrongStartFrames.length,restoredVideoFrames:restoredFrames.length,
        trace,passed:ui.positionErrorSeconds<=0.15 && !v.paused && signal().rms>0.001 && Math.abs(signal().hz-expectedHz)<20 && restoredFrames.length>0};
      r.switches.push(measured);
      if(!measured.passed) throw new Error('Switch signal/position/video check failed');
    }
    v.pause();v.currentTime=4.25;await until(()=>!v.seeking,'paused initial seek');
    for(const language of ['en','fr']) {
      const ui=await request(language);await wait(220);
      const record={language,ui,paused:v.paused,position:v.currentTime,passed:!!ui?.ok && !ui.wasPlaying && v.paused && Math.abs(v.currentTime-4.25)<0.15};
      r.pausedSwitches.push(record);if(!record.passed) throw new Error('Paused switch changed pause state or position');
    }
    await v.play();await until(()=>v.currentTime>4.6 && Math.abs(signal().hz-440)<20,'resume FR after paused switches');
    const canvas=document.createElement('canvas');canvas.width=320;canvas.height=180;
    const ctx=canvas.getContext('2d');ctx.drawImage(v,0,0,320,180);
    const pixels=ctx.getImageData(0,0,320,180).data;const colors=new Set();
    for(let i=0;i<pixels.length;i+=4) colors.add((pixels[i]<<16)|(pixels[i+1]<<8)|pixels[i+2]);
    r.finalDistinctColors=colors.size;
    await until(()=>v.ended,'end after switching');
    r.ended=true;r.passed=colors.size>16 && r.switches.length===6 && r.pausedSwitches.length===2;
  } catch(error) {r.failure=error.message;r.mediaError=v.error&&{code:v.error.code,message:v.error.message};}
  finally {
    v.removeEventListener('loadedmetadata',arm);
    if(videoCallback!==undefined) v.cancelVideoFrameCallback(videoCallback);
    if(small) {window.benchAudio.analyser.disconnect(small);small.disconnect();}
    v.pause();if(audio) await audio.suspend();
  }
  return r;
}
module.exports={checkRemux};
