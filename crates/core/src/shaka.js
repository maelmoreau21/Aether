'use strict';
const v=document.querySelector('video'),status=document.querySelector('#status');
const bench=window.shakaBench={errors:[],events:[],requests:[],ready:false,busy:false};
for(const event of ['error','waiting','stalled','playing','pause','seeking','seeked','emptied','loadedmetadata'])v.addEventListener(event,()=>bench.events.push({event,at:performance.now(),time:v.currentTime,code:v.error?.code}));
const fail=error=>{bench.errors.push({code:error.code,message:String(error),data:error.data});status.textContent='Échec : '+String(error);};
bench.start=async()=>{
  if(bench.player)throw new Error('Recharger la page pour un nouvel essai');
  shaka.polyfill.installAll();bench.version=shaka.Player.version;
  if(!shaka.Player.isBrowserSupported())throw new Error('Shaka browser unsupported');
  const p=bench.player=new shaka.Player();await p.attach(v);
  p.getNetworkingEngine().registerRequestFilter((type,request)=>{
    bench.requests.push({at:performance.now(),type,uris:request.uris,range:request.headers.Range||request.headers.range||null});
    if(bench.requests.length>2000)bench.requests.shift();
  });
  for(const event of ['error','buffering','variantchanged','audiotrackchanged','textchanged','boundarycrossed','mediasourcerecovered'])p.addEventListener(event,e=>{
    bench.events.push({event,at:performance.now(),time:v.currentTime,buffering:e.buffering,detail:e.detail?{code:e.detail.code,data:e.detail.data}:undefined});if(event==='error')fail(e.detail);
  });
  // Fixed local clip, one video representation; no network/codec fallback.
  const config={abr:{enabled:false},preferredAudio:[{language:'fr'}],preferredText:[{language:'fr',forced:true}],
    accessibility:{handleForcedSubtitlesAutomatically:false},
    textDisplayFactory:player=>new shaka.text.NativeTextDisplayer(player)};
  if(!p.configure(config))throw new Error('Shaka configuration rejected');
  await p.load('/api/v1/experiment/shaka/manifest.mpd');
  bench.config=p.getConfiguration();bench.srcBefore=v.src;bench.ready=true;
  bench.seekPolicy=createAudioSeekPolicy({video:v,player:p,onState:s=>{
    if(s.phase==='error')status.textContent='Échec récupération audio : '+s.error;
    else if(s.phase==='recovering')status.textContent='Récupération audio en cours';
    else status.textContent='Shaka '+bench.version+' prêt';
  }});
  bench.seekPolicy.setEnabled(document.querySelector('#seek-recovery').checked);
  const text=p.getTextTracks().find(t=>t.active);
  document.querySelector('#subtitles').value=text?(text.language==='en'?'en-full':text.forced?'fr-forced':'fr-full'):'off';
  status.textContent='Shaka '+bench.version+' prêt';
};
bench.switchLanguage=async (language,safeMargin=0)=>{
  if(!['fr','en'].includes(language)||!bench.ready||bench.busy)throw new Error('Invalid or concurrent switch');
  if(![0,.5,1,2,4].includes(safeMargin))throw new Error('Invalid experimental safeMargin');
  bench.seekPolicy.assertSelectable();
  bench.busy=true;
  try{
    const track=bench.player.getAudioTracks().find(t=>t.language===language);
    if(!track)throw new Error('Audio language absent: '+language);
    const start=performance.now();bench.player.selectAudioTrack(track,safeMargin);
    bench.seekPolicy.noteSelection(bench.player.getVariantTracks().find(t=>t.active),safeMargin);
    return {language,apiMs:performance.now()-start,sourceUnchanged:v.src===bench.srcBefore,trackId:track.id,safeMargin};
  }finally{bench.busy=false;}
};
bench.selectSubtitle=key=>{
  if(!bench.ready)throw new Error('Player not ready');
  const track=key==='off'?null:bench.player.getTextTracks().find(t=>key==='fr-forced'?t.language==='fr'&&t.forced:key==='fr-full'?t.language==='fr'&&!t.forced:t.language==='en');
  if(key!=='off'&&!track)throw new Error('Subtitle absent: '+key);
  bench.player.selectTextTrack(track);document.querySelector('#subtitles').value=key;return track;
};
document.querySelector('#start').onclick=()=>bench.start().catch(fail);
document.querySelector('#seek-recovery').onchange=e=>{try{bench.seekPolicy?.setEnabled(e.target.checked);}catch(error){fail(error);}};
for(const language of ['fr','en'])document.querySelector('#'+language).onclick=()=>bench.switchLanguage(language).catch(fail);
document.querySelector('#subtitles').onchange=e=>{try{bench.selectSubtitle(e.target.value);}catch(error){fail(error);}};
