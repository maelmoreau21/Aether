// Decoding and graph-level measurements only, never physical 5.1/display proof.
async function checkRepresentative(asset) {
  const v=document.querySelector('video');
  const expected=asset.representative;
  const r={id:asset.id,method:expected.method,expected,passed:false,physical51:'not-tested',display10bit:'not-tested',
    scope:'Native decoded pixels, PCM channel graph, coarse marker timing and seek; subtitle capabilities reported separately',
    markerChecks:[],seekChecks:[],subtitleChecks:[]};
  const wait=ms=>new Promise(resolve=>setTimeout(resolve,ms));
  const until=async(test,label)=>{
    const end=performance.now()+14000;
    while(!test()) {
      if(v.error) throw new Error(`MediaError ${v.error.code}: ${v.error.message}`);
      if(performance.now()>end) throw new Error(`Timeout: ${label}`);
      await wait(10);
    }
  };
  let context,splitter,analysers=[],frameCallback;
  let activeMarker=null;
  const canvas=document.createElement('canvas');canvas.width=32;canvas.height=18;
  const ctx=canvas.getContext('2d',{willReadFrequently:true});
  const frame=()=>{
    frameCallback=v.requestVideoFrameCallback((now,meta)=>{
      ctx.drawImage(v,0,0,32,18);
      const pixels=ctx.getImageData(0,0,32,18).data;
      let sum=0;for(let i=0;i<pixels.length;i+=4) sum+=(pixels[i]+pixels[i+1]+pixels[i+2])/3;
      const brightness=sum/(pixels.length/4);
      if(activeMarker && !activeMarker.video && brightness>235 && Math.abs(meta.mediaTime-activeMarker.expectedOnset)<0.2) {
        activeMarker.video={mediaTime:meta.mediaTime,wallMs:now,brightness};
      }
      frame();
    });
  };
  try {
    await until(()=>v.readyState>=2,'initial representative data');
    if(!v.requestVideoFrameCallback) throw new Error('Video frame callback unavailable: sync observation NT');
    r.duration=v.duration;r.dimensions=[v.videoWidth,v.videoHeight];
    r.nativeAudioTracks=v.audioTracks?Array.from(v.audioTracks,t=>({language:t.language,label:t.label,enabled:t.enabled})):null;
    if(!window.benchAudio) {
      const context=new AudioContext();
      const source=context.createMediaElementSource(v);
      const analyser=context.createAnalyser();analyser.fftSize=8192;analyser.smoothingTimeConstant=0;
      source.connect(analyser);analyser.connect(context.destination);
      window.benchAudio={context,source,analyser};
    }
    context=window.benchAudio.context;
    if(!window.benchAudio.source) throw new Error('Reload the bench before graph instrumentation');
    splitter=context.createChannelSplitter(6);
    window.benchAudio.source.connect(splitter);
    for(let i=0;i<6;i++) {const a=context.createAnalyser();a.fftSize=8192;a.smoothingTimeConstant=0;splitter.connect(a,i);analysers.push(a);}
    const pcmRms=analyser=>{
      // Read the last 1024 samples of the analyser's 8192-sample window.
      const data=new Float32Array(8192);analyser.getFloatTimeDomainData(data);
      let sum=0;for(let i=data.length-1024;i<data.length;i++) sum+=data[i]*data[i];
      return Math.sqrt(sum/1024);
    };
    const channelSnapshot=()=>analysers.map((a,index)=>{
      const fft=new Float32Array(a.frequencyBinCount);a.getFloatFrequencyData(fft);
      let peak=0;for(let i=1;i<fft.length;i++) if(fft[i]>fft[peak]) peak=i;
      return {index,rms:pcmRms(a),hz:peak*context.sampleRate/a.fftSize};
    });
    r.audioContext={sampleRate:context.sampleRate,baseLatency:context.baseLatency,outputLatency:context.outputLatency,
      destinationChannels:context.destination.channelCount,destinationMaxChannels:context.destination.maxChannelCount};
    await context.resume();frame();await v.play();await until(()=>v.currentTime>0.8,'initial progress');
    r.channelGraph=channelSnapshot();
    const frequencies=expected.expected_audio_language==='en'?[990,1100]:[440,550,660,80,770,880];
    r.channelGraphMatchesExpected=frequencies.every((hz,i)=>r.channelGraph[i].rms>0.001 && Math.abs(r.channelGraph[i].hz-hz)<20)
      && r.channelGraph.slice(frequencies.length).every(c=>c.rms<0.001);
    r.decodedAudioPresent=r.channelGraph.some(c=>c.rms>0.001);
    // The first graph channel is observed independently of any destination downmix.
    const baseline=pcmRms(analysers[0]);
    if(baseline<0.001) throw new Error('No measurable initial audio PCM');
    r.markerMeasurement={pollTargetMs:10,rmsSamples:1024,windowMs:1024/context.sampleRate*1000,
      audioThreshold:baseline*3,acceptedCoarseSkewSeconds:0.2,physicalSynchronization:'not-tested'};
    const observeMarker=async(onset,label)=>{
      const m={label,expectedOnset:onset,video:null,audio:null,maxPollGapMs:0};activeMarker=m;
      let previous=performance.now();
      await until(()=>{
        const now=performance.now();m.maxPollGapMs=Math.max(m.maxPollGapMs,now-previous);previous=now;
        const rms=pcmRms(analysers[0]);
        if(!m.audio && v.currentTime>onset-0.15 && v.currentTime<onset+0.3 && rms>baseline*3) m.audio={mediaClock:v.currentTime,wallMs:now,rms};
        return !!m.video && !!m.audio;
      },`marker ${label}`);
      m.audioVideoMediaClockSkewSeconds=m.audio.mediaClock-m.video.mediaTime;
      m.audioVideoCallbackSkewMs=m.audio.wallMs-m.video.wallMs;
      m.passed=Math.abs(m.video.mediaTime-onset)<0.1 && Math.abs(m.audioVideoMediaClockSkewSeconds)<=0.2;
      r.markerChecks.push(m);activeMarker=null;
      if(!m.passed) throw new Error(`Coarse A/V marker mismatch: ${label}`);
      await until(()=>v.currentTime>onset+0.65,`after ${label}`);
    };
    const seek=async(target)=>{
      const before=v.currentTime;const paused=v.paused;v.currentTime=target;
      await until(()=>!v.seeking && Math.abs(v.currentTime-target)<0.15,'representative seek');
      const s={before,target,restored:v.currentTime,pausedBefore:paused,pausedAfter:v.paused};
      s.passed=Math.abs(s.restored-target)<0.15 && s.pausedBefore===s.pausedAfter;r.seekChecks.push(s);
      if(!s.passed) throw new Error('Seek did not preserve position/pause state');
    };
    await observeMarker(2,'initial');
    v.pause();const paused=v.currentTime;await wait(220);r.pauseStable=Math.abs(v.currentTime-paused)<0.05;
    await seek(5.7);await v.play();await observeMarker(6,'forward-seek');
    await seek(1.7);await observeMarker(2,'backward-seek');
    await seek(8.7);await observeMarker(9,'late-forward-seek');
    r.afterSeekChannels=channelSnapshot();
    r.decodedFrames=v.getVideoPlaybackQuality().totalVideoFrames;
    await until(()=>v.ended,'representative end');r.ended=true;
    r.passed=r.decodedAudioPresent && r.channelGraphMatchesExpected
      && frequencies.every((hz,i)=>r.afterSeekChannels[i].rms>0.001 && Math.abs(r.afterSeekChannels[i].hz-hz)<20)
      && r.afterSeekChannels.slice(frequencies.length).every(c=>c.rms<0.001)
      && r.pauseStable && r.decodedFrames>0 && r.markerChecks.length===4 && r.seekChecks.every(s=>s.passed);
    // Probe exposed native cues at a known subtitle interval, without injecting VTT.
    await seek(4.25);
    const tracks=Array.from(v.textTracks);
    r.nativeTextTracks=tracks.map((t,index)=>({index,kind:t.kind,language:t.language,label:t.label,mode:t.mode}));
    r.nativeSubtitleSelection=tracks.length?'exposed-needs-cue-render-check':'unavailable';
    r.subtitleUiDisabled=document.querySelector('#subtitles').disabled;
    for(let i=0;i<tracks.length;i++) {
      const control=document.querySelector('#subtitles');control.value=String(i);control.dispatchEvent(new Event('change',{bubbles:true}));
      await wait(150);
      r.subtitleChecks.push({index:i,mode:tracks[i].mode,cueCount:tracks[i].cues?.length??null,
        activeCues:Array.from(tracks[i].activeCues||[],c=>({startTime:c.startTime,endTime:c.endTime,text:c.text})),
        exclusive:tracks.every((t,j)=>t.mode===(i===j?'showing':'disabled')),rendering:'screenshot-and-human-check-required'});
    }
    if(tracks.length) {const control=document.querySelector('#subtitles');control.value=String(Math.min(1,tracks.length-1));control.dispatchEvent(new Event('change',{bubbles:true}));}
  } catch(error) {r.passed=false;r.failure=error.message;r.mediaError=v.error&&{code:v.error.code,message:v.error.message};}
  finally {
    activeMarker=null;if(frameCallback!==undefined) v.cancelVideoFrameCallback(frameCallback);
    if(splitter) {window.benchAudio.source.disconnect(splitter);splitter.disconnect();analysers.forEach(a=>a.disconnect());}
    v.pause();if(context) await context.suspend();
  }
  return r;
}
module.exports={checkRepresentative};
