<script lang="ts">
  import {onMount} from 'svelte';
  import {get} from 'svelte/store';
  import {musicQueue,nextMusic,removeMusic,shuffleMusic,clearMusic} from './queue';
  import {parseLyrics,type LyricLine} from './lyrics';
  import {onSessionGenerationChange} from '../auth/session';
  let audio:HTMLAudioElement;let position=$state(0);let duration=$state(0);let paused=$state(true);let error=$state('');let panel=$state<'queue'|'lyrics'|null>(null);let lyrics=$state<LyricLine[]>([]);let lyricsLoading=$state(false);let volume=$state(1);
  const current=$derived($musicQueue.tracks[$musicQueue.index]);
  const activeLine=$derived(lyrics.findLastIndex((line,i)=>line.time!==null&&line.time<=position&&(lyrics[i+1]?.time??Infinity)>position));
  let lyricsBox=$state<HTMLDivElement>();
  $effect(()=>{const line=activeLine;if(panel==='lyrics'&&line>=0)lyricsBox?.querySelector(`[data-line="${line}"]`)?.scrollIntoView({block:'nearest',behavior:window.matchMedia('(prefers-reduced-motion: reduce)').matches?'auto':'smooth'});});
  function time(value:number){return Number.isFinite(value)?`${Math.floor(value/60)}:${String(Math.floor(value%60)).padStart(2,'0')}`:'0:00';}
  function toggle(){musicQueue.update(q=>({...q,playing:!q.playing}));}
  function previous(){if(audio.currentTime>3){audio.currentTime=0;return;}musicQueue.update(q=>({...q,index:Math.max(0,q.index-1),playing:true}));}
  onMount(()=>{
    let loaded='';let epoch=0;let pending:AbortController|undefined;let starting=false;let priorTracks:unknown;let priorIndex=-1;
    const unsubscribe=musicQueue.subscribe(q=>{
      const track=q.tracks[q.index];
      document.body.classList.toggle('music-playing',!!track);
      if(!track){epoch++;loaded='';pending?.abort();audio.pause();audio.removeAttribute('src');audio.load();lyrics=[];return;}
      const changed=track.id!==loaded||(q.tracks===priorTracks&&q.index!==priorIndex);
      priorTracks=q.tracks;priorIndex=q.index;
      if(changed){
        const generation=++epoch;loaded=track.id;error='';position=0;duration=track.duration??0;lyrics=[];
        pending?.abort();const controller=new AbortController();pending=controller;lyricsLoading=true;
        const timeout=setTimeout(()=>controller.abort(),8000);
        void fetch(`/api/v1/music/tracks/${encodeURIComponent(track.id)}/lyrics`,{signal:controller.signal,cache:'no-store'}).then(async response=>{
          if(!response.ok)throw new Error();const data=await response.json();if(epoch===generation)lyrics=parseLyrics(data.text);
        }).catch(()=>{}).finally(()=>{clearTimeout(timeout);if(epoch===generation)lyricsLoading=false;});
        audio.src=`/api/v1/music/tracks/${encodeURIComponent(track.id)}/content`;audio.load();
        if('mediaSession' in navigator){navigator.mediaSession.metadata=new MediaMetadata({title:track.title,artist:track.artist,album:track.album});}
      }
      if(q.playing&&audio.paused&&!starting){const generation=epoch;starting=true;void audio.play().catch(()=>{if(epoch===generation){error='Lecture indisponible : vérifiez le format, la connexion et vos droits.';musicQueue.update(q=>({...q,playing:false}));}}).finally(()=>{starting=false;if(epoch!==generation&&get(musicQueue).playing){void audio.play().catch(()=>{});}});}
      else if(!q.playing)audio.pause();
    });
    const logout=onSessionGenerationChange(()=>clearMusic());
    const videoPlay=(event:Event)=>{if(event.target instanceof HTMLVideoElement)musicQueue.update(q=>({...q,playing:false}));};
    document.addEventListener('play',videoPlay,true);
    if('mediaSession' in navigator){for(const [action,handler] of Object.entries({play:()=>musicQueue.update(q=>({...q,playing:true})),pause:()=>musicQueue.update(q=>({...q,playing:false})),nexttrack:()=>nextMusic(),previoustrack:previous,seekto:(details:MediaSessionActionDetails)=>{if(details.seekTime!==undefined)audio.currentTime=details.seekTime;}})){try{navigator.mediaSession.setActionHandler(action as MediaSessionAction,handler);}catch{/* Optional platform controls. */}}}
    return()=>{unsubscribe();logout();pending?.abort();epoch++;audio.pause();audio.removeAttribute('src');audio.load();document.body.classList.remove('music-playing');document.removeEventListener('play',videoPlay,true);if('mediaSession' in navigator){navigator.mediaSession.metadata=null;for(const action of ['play','pause','nexttrack','previoustrack','seekto'] as MediaSessionAction[])navigator.mediaSession.setActionHandler(action,null);}};
  });
</script>
<audio bind:this={audio} ontimeupdate={()=>position=audio.currentTime} ondurationchange={()=>duration=Number.isFinite(audio.duration)?audio.duration:0} onplay={()=>{if(!$musicQueue.playing){audio.pause();return;}paused=false;document.querySelectorAll('video').forEach(video=>video.pause());}} onpause={()=>paused=true} onended={()=>{if($musicQueue.repeat==='one'){audio.currentTime=0;void audio.play().catch(()=>{error='Reprise du titre impossible.';musicQueue.update(q=>({...q,playing:false}));});}else nextMusic(true);}} onerror={()=>{if(current){error='Lecture indisponible : format non décodable, réseau interrompu ou accès retiré.';musicQueue.update(q=>({...q,playing:false}));}}}></audio>
{#if current}
  <section class="music-player" aria-label="Lecteur musique">
    {#if panel}<div class="expanded"><div class="panel-heading"><h2>{panel==='queue'?'À suivre':'Paroles'}</h2><button onclick={()=>panel=null} aria-label="Fermer le panneau musique">Fermer</button></div>
      {#if panel==='queue'}<ol>{#each $musicQueue.tracks as track,i (`${i}-${track.id}`)}<li class:current={i===$musicQueue.index}><button class="queue-title" onclick={()=>musicQueue.update(q=>({...q,index:i,playing:true}))}><strong>{track.title}</strong><small>{track.artist}</small></button><button onclick={()=>removeMusic(i)} aria-label={`Retirer ${track.title} de la file`}>Retirer</button></li>{/each}</ol>
      {:else if lyricsLoading}<p role="status">Chargement des paroles…</p>{:else if lyrics.length}<div class="lyrics" bind:this={lyricsBox}>{#each lyrics as line,i (i)}<p data-line={i} class:active={i===activeLine}>{#if line.time!==null}<button onclick={()=>audio.currentTime=line.time!}>{line.text||'♪'}</button>{:else}{line.text}{/if}</p>{/each}</div>{:else}<p>Aucune parole disponible. Ajoutez un fichier .lrc ou .txt du même nom que la piste, puis relancez le scan.</p>{/if}
    </div>{/if}
    <div class="bar"><div class="now"><span class="art" aria-hidden="true">♫</span><div><strong>{current.title}</strong><small>{current.artist} · {current.album||'Album inconnu'}</small></div></div>
    <div class="transport"><div class="buttons"><button onclick={shuffleMusic} aria-label="Mélanger la file">Aléatoire</button><button onclick={previous} aria-label="Piste précédente">◀</button><button class="play" onclick={toggle} aria-label={paused?'Lire la musique':'Mettre la musique en pause'}>{paused?'▶':'Ⅱ'}</button><button onclick={()=>nextMusic()} aria-label="Piste suivante">▶</button><button aria-label="Répétition" onclick={()=>musicQueue.update(q=>({...q,repeat:q.repeat==='off'?'all':q.repeat==='all'?'one':'off'}))}>Répéter : {$musicQueue.repeat==='off'?'non':$musicQueue.repeat==='all'?'tout':'1'}</button></div><label class="timeline"><span>{time(position)}</span><input aria-label="Position dans le titre" type="range" min="0" max={duration||1} step="0.1" value={position} oninput={event=>{audio.currentTime=Number(event.currentTarget.value);position=audio.currentTime;}} /><span>{time(duration)}</span></label></div>
    <div class="extras"><button aria-pressed={panel==='lyrics'} onclick={()=>panel=panel==='lyrics'?null:'lyrics'}>Paroles</button><button aria-pressed={panel==='queue'} onclick={()=>panel=panel==='queue'?null:'queue'}>À suivre ({$musicQueue.tracks.length})</button><input aria-label="Volume musique" type="range" min="0" max="1" step="0.01" bind:value={volume} oninput={()=>audio.volume=volume} /><button onclick={clearMusic} aria-label="Arrêter et vider la file">×</button></div></div>
    {#if error}<p role="alert">{error}</p>{/if}
  </section>
{/if}
<style>
  :global(body.music-playing){padding-bottom:130px}.music-player{position:fixed;bottom:0;left:0;right:0;z-index:120;background:var(--color-surface);border-top:1px solid var(--color-line);box-shadow:0 -10px 40px #0003;color:var(--color-text)}.bar{display:grid;grid-template-columns:1fr 1.4fr 1fr;gap:20px;padding:16px 24px;align-items:center}.now{display:flex;gap:12px;min-width:0}.now strong,.now small{display:block;overflow:hidden;text-overflow:ellipsis;white-space:nowrap}.art{display:grid;place-items:center;background:var(--color-accent);color:var(--color-on-accent);width:48px;height:48px;border-radius:12px;font-size:26px;flex-shrink:0}small{color:var(--color-muted);margin-top:4px}.buttons,.extras,.timeline{display:flex;align-items:center;justify-content:center;gap:10px}.timeline{font-size:12px;margin-top:8px}.timeline input{flex:1;min-width:40px}input{accent-color:var(--color-accent)}.extras{justify-content:flex-end;flex-wrap:wrap}.extras input{width:70px}button{border:1px solid var(--color-line);background:transparent;color:inherit;border-radius:8px;padding:8px;cursor:pointer;min-height:36px}.play{background:var(--color-accent);color:var(--color-on-accent);min-width:40px}.expanded{max-width:760px;margin:auto;padding:20px 24px;max-height:60vh;overflow:auto}.panel-heading{display:flex;justify-content:space-between;align-items:center}h2{font-size:21px}ol{padding:0;list-style:none}li{display:flex;justify-content:space-between;gap:12px;border-bottom:1px solid var(--color-line);padding:6px}.queue-title{flex:1;text-align:left;border:0}.queue-title small{display:block}.current,.active{color:var(--color-accent)}.lyrics{font-size:23px;line-height:1.8;white-space:pre-wrap}.lyrics button{font:inherit;text-align:left;border:0;width:100%}.music-player>p{padding:0 24px}.lyrics p{opacity:.55}.lyrics p.active{opacity:1}button[aria-pressed=true]{color:var(--color-accent)}@media(max-width:900px){.bar{grid-template-columns:1fr 1fr;gap:10px;padding:12px}.transport{grid-column:1/-1;grid-row:2}.extras{font-size:12px}.buttons button{font-size:12px}.art{width:38px;height:38px}.extras input{display:none}:global(body.music-playing){padding-bottom:180px}}
</style>
