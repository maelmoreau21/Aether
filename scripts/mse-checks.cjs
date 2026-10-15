async function checkMse() {
  const v=document.querySelector('video'), b=window.mseBench;
  const r={passed:false,switches:[],subtitles:[],physical51:'NT',physical10bit:'NT'};
  const wait=ms=>new Promise(resolve=>setTimeout(resolve,ms));
  const bounded=async(promise,label)=>{let timer;try{return await Promise.race([promise,new Promise((_,reject)=>{timer=setTimeout(()=>reject(new Error('Timeout '+label)),5000);})]);}finally{clearTimeout(timer);}};
  const stage=value=>{r.stage=value;window.mseCheckProgress=r;};
  const until=async(fn,label,timeout=5000)=>{const end=performance.now()+timeout;while(!fn()){if(v.error)throw new Error(`MediaError ${v.error.code}: ${v.error.message}`);if(performance.now()>end)throw new Error(`Timeout ${label}`);await wait(10);}};
  let frameId,context;
  try {
    stage('start');await until(()=>b.language==='fr'||b.errors.length,'MSE preparation');if(b.errors.length)throw new Error(b.errors.join('; ')); await until(()=>v.readyState>=2,'initial data');
    context=new AudioContext();const source=context.createMediaElementSource(v),splitter=context.createChannelSplitter(6);
    source.connect(context.destination);source.connect(splitter);
    const analysers=Array.from({length:6},(_,i)=>{const a=context.createAnalyser();a.fftSize=8192;a.smoothingTimeConstant=0;splitter.connect(a,i);return a;});
    stage('audio context');await bounded(context.resume(),'audio context resume');
    const signal=()=>analysers.map(a=>{
      const pcm=new Float32Array(1024);a.getFloatTimeDomainData(pcm);const rms=Math.sqrt(pcm.reduce((s,x)=>s+x*x,0)/pcm.length);
      const fft=new Float32Array(a.frequencyBinCount);a.getFloatFrequencyData(fft);let peak=1;for(let i=2;i<fft.length;i++)if(fft[i]>fft[peak])peak=i;
      return {rms,hz:peak*context.sampleRate/a.fftSize};
    });
    const matches=language=>{const s=signal(),expected=language==='fr'?[440,550,660,80,770,880]:[990,1100];return expected.every((hz,i)=>s[i].rms>.001&&Math.abs(s[i].hz-hz)<15)&&(language==='fr'||s.slice(2).every(c=>c.rms<.001));};
    let frames=[];const frame=()=>{frameId=v.requestVideoFrameCallback((now,m)=>{frames.push({at:now,pts:m.mediaTime});frame();});};frame();
    stage('initial play');await bounded(v.play(),'play '+r.stage); await until(()=>matches('fr'),'FR signal');r.initialSignal=signal();await wait(300);r.baselineVideoGapMs=frames.reduce((max,f,i)=>i?Math.max(max,f.at-frames[i-1].at):max,0);
    async function change(language,mode) {
      stage(mode+' '+language);
      const start=performance.now(),position=v.currentTime,wasPaused=v.paused,src=v.src;
      const events=b.events.length;let polls=[],silence=0,last=start;
      const operation=b.switchLanguage(language);
      const sample=setInterval(()=>{const now=performance.now(),s=signal();polls.push(now-last);if(s.every(c=>c.rms<.001))silence+=now-last;last=now;},10);
      let ui;try{ui=await operation;if(wasPaused){await wait(100);if(!v.paused||Math.abs(v.currentTime-position)>.05)throw new Error('Pause state changed');await bounded(v.play(),'play '+r.stage);}let consecutive=0;await until(()=>{consecutive=matches(language)?consecutive+1:0;return consecutive>=3;},'selected language');}finally{clearInterval(sample);}
      const detected=performance.now(),s=signal();await wait(100);
      const near=frames.filter(f=>f.at>=start-100&&f.at<=performance.now());let gap=0;for(let i=1;i<near.length;i++)gap=Math.max(gap,near[i].at-near[i-1].at);
      r.switches.push({language,mode,ui,requestToSignalMs:detected-start,includesPausedObservationAndResume:wasPaused,sampledSilenceMs:silence,maxPollMs:Math.max(0,...polls),maxVideoCallbackGapMs:gap,signal:s,events:b.events.slice(events),sourceUnchanged:v.src===src,positionAdvance:v.currentTime-position,passed:ui.sourceUnchanged&&ui.videoBufferUnchanged&&v.src===src});
      if(wasPaused)v.pause();
    }
    await change('en','playing');await change('fr','playing');
    v.pause();await change('en','paused');await change('fr','paused');
    v.currentTime=7;await until(()=>!v.seeking,'seek');await bounded(v.play(),'play '+r.stage);
    await change('en','after-seek');
    v.currentTime=3.25;await until(()=>!v.seeking,'backward seek with EN');
    await until(()=>matches('en'),'EN retained after backward seek');r.selectedLanguageAfterBackwardSeek='en';
    v.currentTime=7;await until(()=>!v.seeking,'forward seek with EN');
    await change('fr','after-seek');
    stage('subtitles');v.pause();v.currentTime=4.25;await until(()=>!v.seeking,'subtitle seek');
    await until(()=>v.textTracks.length===3,'three text tracks');
    for(let i=0;i<3;i++){
      for(const t of v.textTracks)t.mode='disabled';v.textTracks[i].mode='showing';
      await until(()=>v.textTracks[i].activeCues?.length>0,'external cue');
      const track=v.textTracks[i];r.subtitles.push({label:track.label,language:track.language,mode:track.mode,text:Array.from(track.activeCues,c=>c.text),method:'external WebVTT; active cue, screenshot separately; physical rendering NT'});
    }
    r.bufferedVideo=Array.from({length:b.video.buffered.length},(_,i)=>[b.video.buffered.start(i),b.video.buffered.end(i)]);
    r.missingStatus=(await fetch('/api/v1/experiment/mse/missing.mp4')).status;
    const canvas=document.createElement('canvas');canvas.width=32;canvas.height=18;const ctx=canvas.getContext('2d');ctx.drawImage(v,0,0,32,18);const pixels=ctx.getImageData(0,0,32,18).data;const rgb=[];const colors=new Set();for(let i=0;i<pixels.length;i+=4){rgb.push(pixels[i],pixels[i+1],pixels[i+2]);colors.add(pixels[i]+','+pixels[i+1]+','+pixels[i+2]);}r.pixelRange=Math.max(...rgb)-Math.min(...rgb);r.uniqueRgbColors=colors.size;
    try{await b.switchLanguage('invalid');r.invalidLanguageRejected=false;}catch{r.invalidLanguageRejected=true;}
    r.frames=frames.length;r.videoDimensions=[v.videoWidth,v.videoHeight];r.supported=b.supported;r.errors=b.errors;r.events=b.events;
    r.passed=r.switches.length===6&&r.switches.every(s=>s.passed)&&r.subtitles.length===3&&r.frames>0&&r.pixelRange>30&&r.uniqueRgbColors>10&&r.invalidLanguageRejected&&r.missingStatus===404&&!v.error;
  }catch(error){r.failure=String(error);r.events=b.events;r.errors=b.errors;}
  finally{if(frameId)v.cancelVideoFrameCallback(frameId);v.pause();if(context)await bounded(context.close(),'context close').catch(error=>{r.closeError=String(error);});}
  return r;
}
module.exports={checkMse};
