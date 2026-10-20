'use strict';
// Whole 12-second fragmented tracks only. No manifest, ABR or long-film scheduler.
const v=document.querySelector('video');
const status=document.querySelector('#status');
const root='/api/v1/experiment/mse/';
const bench=window.mseBench={errors:[],events:[],language:null,busy:false};
for(const event of ['error','waiting','stalled','playing','pause','seeking','seeked','emptied']) {
  v.addEventListener(event,()=>bench.events.push({event,at:performance.now(),time:v.currentTime,code:v.error?.code}));
}
const update=(buffer,action)=>new Promise((resolve,reject)=>{
  const done=error=>{clearTimeout(timer);buffer.removeEventListener('updateend',ok);buffer.removeEventListener('error',bad);error?reject(error):resolve();};
  const ok=()=>done();const bad=()=>done(new Error('SourceBuffer error'));
  const timer=setTimeout(()=>done(new Error('SourceBuffer timeout')),8000);
  buffer.addEventListener('updateend',ok,{once:true});buffer.addEventListener('error',bad,{once:true});
  try {action();}catch(error){done(error);}
});
async function bytes(name) {
  const response=await fetch(root+name);if(!response.ok) throw new Error(`${name}: HTTP ${response.status}`);
  const data=await response.arrayBuffer();if(data.byteLength>4*1024*1024) throw new Error('Fixture exceeds 4 MiB experiment limit');return data;
}
bench.start=async()=>{
  if(bench.source) throw new Error('Reload page to restart experiment');
  bench.source=new MediaSource();
  bench.url=URL.createObjectURL(bench.source);v.src=bench.url;
  await new Promise((resolve,reject)=>{bench.source.addEventListener('sourceopen',resolve,{once:true});setTimeout(()=>reject(new Error('sourceopen timeout')),5000);});
  bench.videoMime='video/mp4; codecs="av01.0.00M.10"';bench.audioMime='audio/mp4; codecs="opus"';
  bench.supported=[bench.videoMime,bench.audioMime].map(m=>MediaSource.isTypeSupported(m));
  bench.video=bench.source.addSourceBuffer(bench.videoMime);bench.audio=bench.source.addSourceBuffer(bench.audioMime);
  bench.cache=Object.fromEntries(await Promise.all(['video','fr','en'].map(async name=>[name,await bytes((name==='video'?'video':'audio-'+name)+'.mp4')])));
  await update(bench.video,()=>bench.video.appendBuffer(bench.cache.video));
  await update(bench.audio,()=>bench.audio.appendBuffer(bench.cache.fr));
  bench.source.duration=Math.max(bench.video.buffered.end(bench.video.buffered.length-1),bench.audio.buffered.end(bench.audio.buffered.length-1));bench.language='fr';bench.srcBefore=v.src;
  status.textContent='MSE prêt — FR';
};
bench.switchLanguage=async language=>{
  if(!['fr','en'].includes(language)||!bench.audio||bench.busy) throw new Error('Invalid or concurrent switch');
  bench.busy=true;
  try {
    const start=performance.now(),position=v.currentTime,paused=v.paused;
    // Retain video SourceBuffer and URL. Replace the bounded audio clip, including
    // its past, so a backward seek still uses the selected language.
    await update(bench.audio,()=>bench.audio.remove(0,13));
    bench.audio.appendWindowStart=0;
    await update(bench.audio,()=>bench.audio.appendBuffer(bench.cache[language]));
    bench.language=language;
    const result={language,position,paused,completedMs:performance.now()-start,sourceUnchanged:v.src===bench.srcBefore,videoBufferUnchanged:bench.source.sourceBuffers[0]===bench.video};
    status.textContent=`MSE — ${language}`;return result;
  } finally {bench.busy=false;}
};
const displayError=error=>{bench.errors.push(String(error));status.textContent=String(error);};
document.querySelector('#start').onclick=()=>bench.start().catch(displayError);
for(const language of ['fr','en']) document.querySelector('#'+language).onclick=()=>bench.switchLanguage(language).catch(displayError);
