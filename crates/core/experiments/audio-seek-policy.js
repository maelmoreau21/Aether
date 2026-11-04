'use strict';
// Conservative buffer-history workaround using public Shaka APIs only.
// No fixture names, frequencies or decoded-audio inspection belong here.
(function(root){
  function createAudioSeekPolicy({video,player,onState=()=>{},timeoutMs=6000}){
    let enabled=true,dirty=false,desired=null,operation=null,disposed=false,sequence=0,watchdog=null;
    const history=[];
    const state={phase:'idle',error:null,attempts:0,ignoredInternalSeeks:0};
    const emit=()=>{onState({...state});};
    const active=()=>player.getVariantTracks().find(t=>t.active);
    const covers=(ranges,time)=>ranges?.some(r=>r.start<=time+.02&&r.end>=time+.02);
    const snapshot=()=>({position:video.currentTime,paused:video.paused,rate:video.playbackRate,variantId:desired?.id,language:desired?.language});
    const fail=(error,op)=>{
      if(disposed)return;
      video.pause();state.phase='error';state.error=String(error);state.attempts=op?.attempts||0;emit();
    };
    const api={state,history,
      get enabled(){return enabled;},get dirty(){return dirty;},
      setEnabled(value){if(operation)throw new Error('Recovery in progress');enabled=Boolean(value);},
      assertSelectable(){if(operation)throw new Error('Audio recovery in progress');if(state.phase==='error')throw new Error(state.error);},
      noteSelection(variant,margin){
        api.assertSelectable();if(!variant)throw new Error('Selected variant missing');
        // A real audio change with margin 0 already clears audio alone. Merely
        // reselecting the same audio is a Shaka no-op and retains the risk.
        dirty=margin>0||(dirty&&desired?.audioId===variant.audioId);
        desired={id:variant.id,language:variant.language,audioId:variant.audioId,videoId:variant.videoId};
      },
      dispose(){disposed=true;clearInterval(watchdog);video.removeEventListener('seeking',onSeeking);if(operation)operation.cancelled=true;}
    };
    const recover=async(op)=>{
      const started=performance.now();let appended=false,lastSeek=started;
      const append=e=>{if(e.contentType==='audio'&&e.start<=op.saved.position+.02&&e.end>op.saved.position)appended=true;};
      const seek=()=>{lastSeek=performance.now();};
      player.addEventListener('segmentappended',append);video.addEventListener('seeking',seek);
      const waitFor=async(test,label)=>{
        for(;;){
          if(disposed||op.cancelled)throw new Error('Audio recovery interrupted by another seek');
          if(video.error)throw new Error('Media error during audio recovery');
          if(test())return;
          if(performance.now()-started>timeoutMs)throw new Error('Audio recovery timeout: '+label);
          await new Promise(resolve=>setTimeout(resolve,20));
        }
      };
      try{
        video.pause();
        const variant=player.getVariantTracks().find(t=>t.id===op.saved.variantId);
        if(!variant||variant.language!==op.saved.language)throw new Error('Requested audio variant unavailable');
        op.attempts=1;state.attempts=1;emit();
        player.selectVariantTrack(variant,true,0);
        await waitFor(()=>{
          const b=player.getBufferedInfo();
          return appended&&covers(b.audio,op.saved.position)&&covers(b.video,op.saved.position)&&video.readyState>=2;
        },'fresh selected audio');
        if(op.cancelled)throw new Error('Audio recovery interrupted');
        if(Math.abs(video.currentTime-op.saved.position)>.02){
          op.restores=1;video.currentTime=op.saved.position;
        }
        await waitFor(()=>!video.seeking&&performance.now()-lastSeek>=180,'settled seek');
        const selected=active();
        if(!selected||selected.id!==op.saved.variantId)throw new Error('Manual audio selection changed during recovery');
        if(Math.abs(video.currentTime-op.saved.position)>.04)throw new Error('Playback position not preserved');
        video.playbackRate=op.saved.rate;
        if(!op.saved.paused){let timer;try{await Promise.race([video.play(),new Promise((_,reject)=>{timer=setTimeout(()=>reject(new Error('Playback resume timeout')),2000);})]);}finally{clearTimeout(timer);}}
        if(op.cancelled||disposed)throw new Error('Audio recovery interrupted');
        dirty=false;state.phase='idle';state.error=null;
        op.result={...snapshot(),positionError:video.currentTime-op.saved.position};op.success=true;
        // Public state cannot prove decoded language. Detect a subsequent stall,
        // report it terminally, and never initiate a second purge.
        clearInterval(watchdog);
        let lastPosition=video.currentTime,lastAdvance=performance.now(),observed=0;
        watchdog=setInterval(()=>{
          const now=performance.now();observed+=100;
          if(video.paused||video.seeking||Math.abs(video.currentTime-lastPosition)>.01){lastPosition=video.currentTime;lastAdvance=now;}
          if(video.error||(!video.paused&&now-lastAdvance>2000)){
            clearInterval(watchdog);op.success=false;op.error='Playback stalled after audio recovery';fail(op.error,op);
          }else if(observed>=5000)clearInterval(watchdog);
        },100);
      }catch(error){op.success=false;op.error=String(error);fail(error,op);}
      finally{
        player.removeEventListener('segmentappended',append);video.removeEventListener('seeking',seek);
        op.durationMs=performance.now()-started;
        history.push(op);if(history.length>100)history.shift();operation=null;emit();
      }
    };
    function onSeeking(){
      if(disposed||!enabled)return;
      if(operation){
        // Shaka flush and our single position restoration can emit seeking too.
        if(Math.abs(video.currentTime-operation.saved.position)>.04){operation.cancelled=true;}
        else state.ignoredInternalSeeks++;
        return;
      }
      if(state.phase==='error'||!dirty||!desired)return;
      const op={id:++sequence,saved:snapshot(),attempts:0,restores:0,success:false};
      clearInterval(watchdog);
      operation=op;state.phase='recovering';state.error=null;emit();
      // Publish the lock before any synchronous seeking emitted by Shaka.
      void recover(op);
    }
    video.addEventListener('seeking',onSeeking);
    return api;
  }
  if(typeof module!=='undefined'&&module.exports)module.exports={createAudioSeekPolicy};
  else root.createAudioSeekPolicy=createAudioSeekPolicy;
})(globalThis);
