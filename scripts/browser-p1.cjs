const fs = require('node:fs'), path = require('node:path'), assert = require('node:assert/strict');
const {createHash} = require('node:crypto');
const {playwrightAdapter, firefoxAdapter} = require('./browser-matrix.cjs');
const out = path.resolve(process.env.MEDIA_REPORT_DIR || 'artifacts/p1-chrome');
fs.mkdirSync(out, {recursive:true});
function hashes() {
  const root = 'tests/fixtures/generated';
  const walk = dir => fs.readdirSync(dir, {withFileTypes:true}).flatMap(entry => entry.isDirectory() ? walk(path.join(dir,entry.name)) : [path.join(dir,entry.name)]);
  return Object.fromEntries(walk(root).filter(p => /\.(mkv|mp4|webm|vtt)$/.test(p)).map(p => [p,createHash('sha256').update(fs.readFileSync(p)).digest('hex')]));
}
(async()=>{
  const report = {date:new Date().toISOString(),channel:process.env.MEDIA_BROWSER_CHANNEL||'chrome',os:require('node:os').release(),results:[],hashesBefore:hashes(),limits:'Headless decoded pixels and PCM; no physical speakers/10bit display, NAS, films or human listening proof.'};
  let browser;
  try {
    browser = await (report.channel==='firefox'?firefoxAdapter():playwrightAdapter());
    report.browser=browser.version; report.driver=browser.driver;
    await browser.goto('http://127.0.0.1:3000/app/films/?work=fixture-film');
    await browser.evaluate(async()=>{
      const until = async (fn, label) => {const end=Date.now()+12000;while(!fn()){if(Date.now()>end)throw new Error('Timeout '+label+' '+document.querySelector('[role=alert]')?.textContent);await new Promise(r=>setTimeout(r,20));}};
      const v=document.querySelector('video');
      const select=async(label,value)=>{
        const root=document.querySelector('.player-shell');root.dispatchEvent(new PointerEvent('pointermove',{pointerType:'mouse',bubbles:true}));
        if(label==='Source'&&!document.querySelector('dialog').open){document.querySelector(`[aria-label="Sélectionner la version ${value.endsWith('mkv')?'MKV':'MP4'}"]`).click();return;}
        const map={Audio:['audio','data-audio-id'], 'Sous-titres':['subtitles','data-subtitle-id'], Source:['video','data-source-id'], Vitesse:['speed','data-speed']};
        const [menu,attr]=map[label];document.querySelector(`[data-menu="${menu}"]`).click();await new Promise(r=>setTimeout(r,0));
        const button=document.querySelector(`[${attr}="${value||'off'}"]`);if(!button||button.disabled)throw new Error('Missing usable choice '+label+' '+value);if(label==='Audio'){
          const video=document.querySelector('video');window.p1test.lastAudioPosition=video.currentTime;window.p1test.lastAudioRestore=null;
          video.addEventListener('seeked',()=>{window.p1test.lastAudioRestore=video.currentTime;},{once:true});
        }button.click();
      };
      const ready=async()=>{await new Promise(r=>setTimeout(r,30));await until(()=>document.querySelector('dialog').open&&!document.querySelector('button[aria-label="Audio"]').disabled&&!document.querySelector('.loading'),'ready');};
      const seek=async time=>{const input=document.querySelector('input[aria-label="Position"]');input.value=time;input.dispatchEvent(new Event('input',{bubbles:true}));await until(()=>!v.seeking&&Math.abs(v.currentTime-time)<.15,'seek');};
      window.p1test={until,v,select,ready,seek};
      await until(()=>document.querySelectorAll('.version-choice').length===2,'catalogue hydration');
    });
    for (const container of (process.env.MEDIA_TEST_CONTAINER ? [process.env.MEDIA_TEST_CONTAINER] : ['mkv','mp4'])) {
      await browser.evaluate(container=>window.p1test.select('Source','demo-'+container),container);
      await browser.click('.hero-cta');
      await browser.evaluate(async()=>{await window.p1test.ready();window.p1test.v.pause();await window.p1test.select('Audio','audio-fr');await window.p1test.ready();await window.p1test.seek(0);});
      await browser.evaluate(()=>{const v=document.querySelector('video');
        if(!window.p1test.context){
          const context=new AudioContext(),source=context.createMediaElementSource(v),splitter=context.createChannelSplitter(6);
          const outputAnalyser=context.createAnalyser();source.connect(outputAnalyser);outputAnalyser.connect(context.destination);source.connect(splitter);
          const analysers=Array.from({length:6},(_,i)=>{const a=context.createAnalyser();a.fftSize=8192;a.smoothingTimeConstant=0;splitter.connect(a,i);return a;});
          window.p1test.context=context;
          window.p1test.signal=()=>analysers.map(a=>{const data=new Float32Array(8192);a.getFloatTimeDomainData(data);const pcm=data.subarray(7168);const fft=new Float32Array(a.frequencyBinCount);a.getFloatFrequencyData(fft);let peak=1;for(let i=2;i<fft.length;i++)if(fft[i]>fft[peak])peak=i;return {hz:peak*context.sampleRate/8192,rms:Math.sqrt(pcm.reduce((s,x)=>s+x*x,0)/pcm.length)};});
        }

      });
      // Real user activation before AudioContext resume (Firefox compatible).
      await browser.evaluate(speed=>window.p1test.testSpeed=Number(speed),process.env.MEDIA_TEST_SPEED||'1.5');
      await browser.click('button.primary');
      const result=await browser.evaluate(async container=>{
        const {v,until,select,ready,seek}=window.p1test;
        await window.p1test.context.resume();
        const signal=window.p1test.signal;
        const stable=async language=>{let n=0;await until(()=>{const data=signal(),expected=language==='fr'?[440,550,660,80,770,880]:[990,1100];const okay=expected.every((hz,i)=>data[i].rms>.001&&Math.abs(data[i].hz-hz)<15)&&(language==='fr'||data.slice(2).every(c=>c.rms<.001));n=okay?n+1:0;return n>=3;},'PCM '+language).catch(e=>{throw new Error(e.message+' '+JSON.stringify({signal:signal(),time:v.currentTime,paused:v.paused,context:window.p1test.context.state,src:v.currentSrc,ready:v.readyState,muted:v.muted,volume:v.volume}));});return signal();};
        const fr=await stable('fr');
        const canvas=document.createElement('canvas');canvas.width=32;canvas.height=18;const ctx=canvas.getContext('2d');ctx.drawImage(v,0,0,32,18);const data=ctx.getImageData(0,0,32,18).data,colors=new Set();for(let i=0;i<data.length;i+=4)colors.add(`${data[i]},${data[i+1]},${data[i+2]}`);if(colors.size<10)throw new Error('No decoded pixels');
        v.pause();await seek(4.25);const volume=document.querySelector('input[aria-label="Volume"]');volume.value=.4;volume.dispatchEvent(new Event('input',{bubbles:true}));
        await select('Vitesse',String(window.p1test.testSpeed));await ready();
        await new Promise(r=>setTimeout(r,20));
        await select('Audio','audio-en');await ready();
        const paused={position:v.currentTime,paused:v.paused,volume:v.volume,speed:v.playbackRate};
        if(!paused.paused||Math.abs(paused.position-4.25)>.15||paused.volume!==.4||paused.speed!==window.p1test.testSpeed)throw new Error('Paused state lost '+JSON.stringify(paused));
        await v.play();const en=await stable('en');
        // Keep enough of the 12s fixture for a real menu-driven active switch.
        v.pause();await seek(1);await v.play();await stable('en');
        await select('Audio','audio-fr');const before=window.p1test.lastAudioPosition;await ready();const restored=window.p1test.lastAudioRestore;const afterReadyPosition=v.currentTime;
        if(v.paused||restored===null||Math.abs(restored-before)>.15)throw new Error('Playing state/position lost '+JSON.stringify({before,restored,paused:v.paused,ended:v.ended,src:v.currentSrc}));
        const frAgain=await stable('fr');
        v.pause();await seek(4.25);
        const subtitles=[];
        for(const [id,text] of [['fr-full','FR COMPLET CUE 2'],['fr-forced','FR FORCE CUE 2'],['en-full','EN FULL CUE 2']]){
          await select('Sous-titres',id);await ready();await until(()=>Array.from(v.textTracks).some(t=>t.mode==='showing'&&Array.from(t.activeCues||[]).some(c=>c.text===text)),'cue '+id);subtitles.push({id,text});
        }
        await select('Audio','audio-en');await ready();await until(()=>Array.from(v.textTracks).some(t=>t.mode==='showing'&&Array.from(t.activeCues||[]).some(c=>c.text==='EN FULL CUE 2')),'manual subtitle retained');
        await select('Sous-titres','');await ready();if(Array.from(v.textTracks).some(t=>t.mode==='showing'))throw new Error('Subtitle not off');
        await seek(.5);await v.play();const enAfterBackward=await stable('en');v.pause();await seek(8.7);await v.play();await stable('en');await until(()=>v.ended,'end');
        return {container,fr,en,frAgain,enAfterBackward,pixels:colors.size,paused,playingPositionError:restored-before,afterReadyPosition,subtitles,manualSubtitleRetained:true,ended:v.ended};
      },container);
      report.results.push(result);
      if (container !== (process.env.MEDIA_TEST_CONTAINER || 'mp4')) await browser.evaluate(()=>document.querySelector('.return').click());
    }
    await browser.evaluate(()=>{document.querySelector('video').src='/api/v1/assets/missing/content';document.querySelector('video').load();});
    await browser.evaluate(()=>window.p1test.until(()=>document.querySelector('[role=alert]')?.textContent.includes('MediaError'),'visible source error'));
    report.errorVisible=true;
    await browser.screenshot('desktop-error.png');
    await browser.evaluate(()=>window.p1test.select('Source','demo-mkv'));await browser.evaluate(()=>window.p1test.ready());
    await browser.evaluate(async()=>{const {v,seek,select,ready,until}=window.p1test;v.pause();await seek(4.25);await select('Sous-titres','fr-full');await ready();await until(()=>Array.from(v.textTracks).some(t=>Array.from(t.activeCues||[]).some(c=>c.text==='FR COMPLET CUE 2')),'screenshot cue');});
    await browser.evaluate(()=>document.querySelector('.player-shell').dispatchEvent(new PointerEvent('pointermove',{pointerType:'mouse',bubbles:true})));await browser.screenshot('desktop.png');
    report.hashesAfter=hashes();assert.deepEqual(report.hashesAfter,report.hashesBefore);report.originalsUnchanged=true;report.passed=true;
  } catch(error) {report.failure=String(error.stack||error);process.exitCode=1;}
  finally {if(browser)await browser.close();fs.writeFileSync(path.join(out,'report.json'),JSON.stringify(report,null,2));console.log(JSON.stringify({channel:report.channel,browser:report.browser,passed:report.passed,failure:report.failure,results:report.results.length}));}
})();
