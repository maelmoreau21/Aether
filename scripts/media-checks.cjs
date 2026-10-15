// One browser-side scenario shared by Playwright and stock Firefox/WebDriver.
async function checkMedia({id, mime}) {
  const v = document.querySelector('video');
  const events = [];
  const names = ['loadedmetadata', 'playing', 'pause', 'seeking', 'seeked', 'ended', 'error'];
  const record = e => events.push({event:e.type, time:v.currentTime});
  names.forEach(e => v.addEventListener(e, record));
  const wait = ms => new Promise(resolve => setTimeout(resolve, ms));
  const until = async (test, label) => {
    const end = performance.now() + 12000;
    while (!test()) {
      if (v.error) throw new Error(`MediaError ${v.error.code}: ${v.error.message}`);
      if (performance.now() > end) throw new Error(`Timeout: ${label}`);
      await wait(25);
    }
  };
  const snapshot = () => v.audioTracks ? Array.from(v.audioTracks, (t, index) => ({index, id:t.id, language:t.language, label:t.label, enabled:t.enabled})) : null;
  const r = {id, mime, method:'html5-native-url', canPlayType:v.canPlayType(mime), events,
    mse:{available:typeof MediaSource !== 'undefined', actualAppend:'not-tested'}, audioSelection:{status:'not-tested'}};
  const mseType = `${mime}; codecs="av01.0.00M.08,opus"`;
  r.mse.type = mseType;
  r.mse.isTypeSupported = typeof MediaSource !== 'undefined' && MediaSource.isTypeSupported(mseType);
  let audio;
  try {
    await until(() => v.readyState >= 2, 'metadata/data');
    r.duration = v.duration; r.dimensions = [v.videoWidth, v.videoHeight];
    r.nativeAudioTracks = snapshot();
    if (!window.benchAudio) {
      const context = new AudioContext();
      const analyser = context.createAnalyser(); analyser.fftSize = 8192;
      const source = context.createMediaElementSource(v);
      source.connect(analyser); analyser.connect(context.destination);
      window.benchAudio = {context, analyser};
    }
    audio = window.benchAudio.context;
    const analyser = window.benchAudio.analyser;
    const signal = () => {
      const pcm = new Float32Array(analyser.fftSize); analyser.getFloatTimeDomainData(pcm);
      const spectrum = new Float32Array(analyser.frequencyBinCount); analyser.getFloatFrequencyData(spectrum);
      const rms = Math.sqrt(pcm.reduce((s,x) => s+x*x, 0)/pcm.length);
      let peak = 0;
      for (let i=1; i<spectrum.length; i++) if (spectrum[i] > spectrum[peak]) peak = i;
      return {rms, dominantHz:peak*audio.sampleRate/analyser.fftSize, sampleRate:audio.sampleRate};
    };
    await audio.resume(); await v.play();
    await until(() => v.currentTime > 0.7, 'progress');
    r.initialAudio = signal();
    const canvas = document.createElement('canvas'); canvas.width=320; canvas.height=180;
    const ctx = canvas.getContext('2d'); ctx.drawImage(v,0,0,320,180);
    // Ignore alpha, which would let a black/blank frame look nonzero.
    const pixels = ctx.getImageData(0,0,320,180).data;
    const colors = new Set();
    for (let i=0; i<pixels.length; i+=4) colors.add((pixels[i]<<16)|(pixels[i+1]<<8)|pixels[i+2]);
    r.distinctColors = colors.size;
    v.pause(); const paused=v.currentTime; await wait(250);
    r.pauseStable = Math.abs(v.currentTime-paused)<0.05;
    v.currentTime=4;
    await until(() => !v.seeking && Math.abs(v.currentTime-4)<0.2, 'seek');
    r.seekTime=v.currentTime;
    await v.play(); await until(() => v.currentTime>4.5, 'resume');
    r.afterSeekAudio=signal();
    r.decodedFrames=v.getVideoPlaybackQuality().totalVideoFrames;
    if (!v.audioTracks || v.audioTracks.length<2) {
      r.audioSelection = {status:'unavailable', reason:!v.audioTracks ? 'HTMLMediaElement.audioTracks not exposed with default settings' : 'Fewer than two native audio tracks exposed', tracks:snapshot()};
    } else {
      const switches=[];
      for (const [index, language, hz] of [[0,'fr',440],[1,'en',880],[0,'fr',440]]) {
        const selector=document.querySelector('#audio');
        selector.value=String(index); selector.dispatchEvent(new Event('change', {bubbles:true}));
        await wait(350);
        const observed=signal();
        switches.push({index, language, expectedHz:hz, ...observed, tracks:snapshot(), time:v.currentTime,
          passed:observed.rms>0.001 && Math.abs(observed.dominantHz-hz)<20 && v.audioTracks[index].enabled && !v.paused});
      }
      r.audioSelection={status:switches.every(s=>s.passed)?'passed':'failed', switches};
    }
    await until(() => v.ended, 'end'); r.ended=true;
    r.passed=r.initialAudio.rms>0.001 && Math.abs(r.initialAudio.dominantHz-440)<20 && r.afterSeekAudio.rms>0.001 && r.distinctColors>16 && r.pauseStable && r.decodedFrames>0;
  } catch (error) {
    r.passed=false; r.failure=error.message;
    r.nativeAudioTracks=snapshot();
    r.mediaError=v.error && {code:v.error.code, message:v.error.message};
  } finally {
    v.pause(); if(audio) await audio.suspend();
    names.forEach(e=>v.removeEventListener(e,record));
  }
  return r;
}
module.exports={checkMedia};
