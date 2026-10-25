// Test-only observation of the real Svelte DOM. No API/media routes are mocked.
function installStreamingObserver() {
  const graphs = new Map(), videos = new Map();
  let context, active, previousPoll = performance.now();
  const trials = [], events = [];
  function begin(kind, details = {}) {
    active = {kind, start: performance.now(), startEpochMs:performance.timeOrigin+performance.now(), observedSilenceMs: 0, maxVideoFrameGapMs: 0, ...details};
    const v = document.querySelector('dialog[open] video');
    active.positionBefore = v?.currentTime;
    active.audioTrackBefore = [...document.querySelectorAll('audio[data-external-audio]')].find(a=>!a.paused)?.dataset.internalAudioTrack;
    active.videoSourceBefore = v?.currentSrc?.split('?')[0];
    active.videoLoadsBefore = window.__streamingSystem.videoLoads;
    trials.push(active); context ??= new AudioContext(); context.resume(); return active;
  }
  function attachAudio(a) {
    if(graphs.has(a))return;
    context ??= new AudioContext();
    const source=context.createMediaElementSource(a), analyser=context.createAnalyser();
    analyser.fftSize=2048;analyser.smoothingTimeConstant=0;source.connect(analyser);analyser.connect(context.destination);
    graphs.set(a,{source,analyser,bins:new Float32Array(1024),pcm:new Float32Array(2048)});
  }
  function signal(a) {
    attachAudio(a);const g=graphs.get(a);g.analyser.getFloatFrequencyData(g.bins);g.analyser.getFloatTimeDomainData(g.pcm);
    let peak=1,sum=0;for(let i=2;i<g.bins.length;i++)if(g.bins[i]>g.bins[peak])peak=i;
    for(let i=g.pcm.length-256;i<g.pcm.length;i++)sum+=g.pcm[i]*g.pcm[i];
    return {hz:peak*context.sampleRate/g.analyser.fftSize,db:g.bins[peak],rms:Math.sqrt(sum/256)};
  }
  function attachVideo(v) {
    if(videos.has(v))return;
    // Needed only for the PCM oracle if a future source uses embedded audio.
    // Selected segmented audio normally comes from same-origin blob URLs.
    v.crossOrigin='anonymous';
    const state={lastFrame:undefined,frameId:undefined};videos.set(v,state);
    for(const name of ['loadstart','loadedmetadata','playing','pause','waiting','seeking','seeked','error'])v.addEventListener(name,()=>{
      if(name==='loadstart')window.__streamingSystem.videoLoads++;
      if(active){active.videoEvents??=[];if(active.videoEvents.length<128)active.videoEvents.push({name,atMs:performance.now()-active.start,time:v.currentTime});}
    });
    const frame=()=>{state.frameId=v.requestVideoFrameCallback((at,meta)=>{
      if(active){
        active.maxVideoFrameGapMs=Math.max(active.maxVideoFrameGapMs,at-Math.max(active.start,state.lastFrame||active.start));
        if(active.firstFrameMs===undefined&&(active.kind!=='seek'||Math.abs(meta.mediaTime-active.target)<.2)){
          active.firstFrameMs=performance.now()-active.start;active.frameMediaTime=meta.mediaTime;
        }
        if(active.expectedResumeTime!==undefined&&active.firstRestoredFrameMs===undefined&&Math.abs(meta.mediaTime-active.expectedResumeTime)<.25)active.firstRestoredFrameMs=performance.now()-active.start;
      }
      state.lastFrame=at;frame();
    });};frame();
  }
  function sample() {
    const v=document.querySelector('dialog[open] video'),audios=[...document.querySelectorAll('audio[data-external-audio]')];
    const audible=audios.filter(a=>!a.paused&&!a.muted),a=audible[0]||audios[0]||v;
    const ranges=e=>Array.from({length:e.buffered.length},(_,i)=>[e.buffered.start(i),e.buffered.end(i)]);
    return {trial:active?JSON.parse(JSON.stringify(active)):null,video:v?{position:v.currentTime,paused:v.paused,seeking:v.seeking,readyState:v.readyState,networkState:v.networkState,buffered:ranges(v),source:v.currentSrc.split('?')[0],loads:window.__streamingSystem.videoLoads,frames:v.getVideoPlaybackQuality().totalVideoFrames,dropped:v.getVideoPlaybackQuality().droppedVideoFrames}:null,audio:a?{position:a.currentTime,paused:a.paused,track:a.dataset.internalAudioTrack,readyState:a.readyState,buffered:ranges(a),signal:context?signal(a):undefined}:null,error:document.querySelector('.player-error')?.textContent,notice:document.querySelector('.fullscreen-notice')?.textContent,loading:!!document.querySelector('dialog[open] .loading')};
  }
  window.__streamingSystem={begin,sample,trials,events,videoLoads:0};
  document.addEventListener('click',event=>{
    const target=event.target instanceof Element?event.target:null;if(!target)return;
    if(target.closest('.hero-cta')){const expected=window.__streamingSystem.nextExpectedResumeTime;delete window.__streamingSystem.nextExpectedResumeTime;begin('open',expected===undefined?{}:{expectedResumeTime:expected});}
    const choice=target.closest('[data-audio-id]');if(choice)begin('switch',{track:choice.dataset.audioId});
  },true);
  document.addEventListener('input',event=>{
    const target=event.target;if(target instanceof HTMLInputElement&&target.getAttribute('aria-label')==='Position')begin('seek',{target:Number(target.value)});
  },true);
  const observer=new MutationObserver(()=>{
    for(const [a,g] of graphs)if(!a.isConnected){g.source.disconnect();g.analyser.disconnect();graphs.delete(a);}
    for(const [v,s] of videos)if(!v.isConnected){v.cancelVideoFrameCallback(s.frameId);videos.delete(v);}
    for(const v of document.querySelectorAll('video'))attachVideo(v);
    for(const a of document.querySelectorAll('audio[data-external-audio]'))attachAudio(a);
    if(active?.kind==='open'&&active.dialogOpenMs===undefined&&document.querySelector('dialog[open]'))active.dialogOpenMs=performance.now()-active.start;
    if(active){const notice=document.querySelector('.fullscreen-notice')?.textContent?.trim();if(notice){active.notices??=[];if(active.notices.at(-1)!==notice&&active.notices.length<32)active.notices.push(notice);}}
  });
  observer.observe(document,{subtree:true,attributes:true,childList:true});
  setInterval(()=>{
    const now=performance.now(),delta=now-previousPoll;previousPoll=now;
    if(!active||!context)return;
    active.maxPcmPollGapMs=Math.max(active.maxPcmPollGapMs||0,delta);
    const v=document.querySelector('dialog[open] video');if(!v)return;
    const audios=[...document.querySelectorAll('audio[data-external-audio]')];
    const playing=audios.filter(a=>!a.paused&&!a.muted),selected=playing.find(a=>!active.track||a.dataset.internalAudioTrack===active.track);
    const samples=playing.map(a=>({element:a,...signal(a)}));
    const audible=samples.some(s=>s.rms>1e-4);
    if(audible&&active.firstAnyPcmMs===undefined)active.firstAnyPcmMs=now-active.start;
    if(active.kind==='switch'&&!audible)active.observedSilenceMs+=delta;
    let candidate=selected?{element:selected,...signal(selected)}:null;
    if(!audios.length&&!v.muted&&!v.paused)candidate={element:v,...signal(v)};
    if(!candidate||candidate.rms<=1e-4)return;
    const e=candidate.element;
    const targetSignal=active.kind!=='switch'||(active.track==='internal-2'?candidate.hz>950&&candidate.hz<1150:candidate.hz>400&&candidate.hz<900);
    if(targetSignal&&active.firstRawPcmMs===undefined){
      active.firstRawPcmMs=now-active.start;
      active.initialPcm={signedDriftSeconds:e.currentTime-v.currentTime,videoPosition:v.currentTime,audioPosition:e.currentTime,videoPaused:v.paused,videoSeeking:v.seeking,hz:candidate.hz,rms:candidate.rms};
    }
    if(targetSignal&&active.firstRawPcmMs!==undefined&&now-active.start-active.firstRawPcmMs<1000){active.initialDriftSamples??=[];if(active.initialDriftSamples.length<128)active.initialDriftSamples.push({atMs:now-active.start,signedSeconds:e.currentTime-v.currentTime,videoPaused:v.paused,seeking:v.seeking});}
    if(active.firstAudioMs!==undefined)return;
    if(Math.abs(e.currentTime-v.currentTime)>.15||v.paused||v.seeking)return;
    if(active.kind==='seek'&&Math.abs(v.currentTime-active.target)>.3)return;
    if(active.expectedResumeTime!==undefined&&Math.abs(v.currentTime-active.expectedResumeTime)>.5)return;
    if(!targetSignal)return;
    active.firstAudioMs=now-active.start;active.audioSignal={hz:candidate.hz,db:candidate.db,rms:candidate.rms};active.driftSeconds=Math.abs(e.currentTime-v.currentTime);
  },8);
}
module.exports={installStreamingObserver};
