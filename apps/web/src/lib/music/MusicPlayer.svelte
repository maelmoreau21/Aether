<script lang="ts">
  import {onMount} from 'svelte';
  import {get} from 'svelte/store';
  import {musicQueue,nextMusic,removeMusic,shuffleMusic,clearMusic} from './queue';
  import {parseLyrics,type LyricLine} from './lyrics';
  import {onSessionGenerationChange} from '../auth/session';
  import Icon from '../ui/Icon.svelte';
  import MusicArtwork from './MusicArtwork.svelte';
  let audio:HTMLAudioElement;let position=$state(0);let duration=$state(0);let paused=$state(true);let error=$state('');let panel=$state<'queue'|'lyrics'|null>(null);let lyrics=$state<LyricLine[]>([]);let lyricsLoading=$state(false);let volume=$state(1);
  const current=$derived($musicQueue.tracks[$musicQueue.index]);
  let panelDialog=$state<HTMLDialogElement>();
  let playerElement=$state<HTMLElement>();
  $effect(()=>{if(!current){panel=null;return;}if(panel)panelDialog?.showModal();else panelDialog?.close();});
  $effect(()=>{const node=playerElement;if(!node)return;const observer=new ResizeObserver(()=>document.documentElement.style.setProperty('--music-player-height',`${node.getBoundingClientRect().height}px`));observer.observe(node);return()=>{observer.disconnect();document.documentElement.style.removeProperty('--music-player-height');};});
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
{#snippet controls()}
  <div class="buttons"><button onclick={previous} aria-label="Piste précédente"><Icon name="chapter-prev" size={20} /></button><button class="play" onclick={toggle} aria-label={paused?'Lire la musique':'Mettre la musique en pause'}><Icon name={paused?'play':'pause'} size={20} /></button><button onclick={()=>nextMusic()} aria-label="Piste suivante"><Icon name="chapter-next" size={20} /></button></div>
  <label class="timeline"><span>{time(position)}</span><input aria-label="Position dans le titre" aria-valuetext={`${time(position)} sur ${time(duration)}`} type="range" min="0" max={duration||1} step="0.1" value={position} oninput={event=>{audio.currentTime=Number(event.currentTarget.value);position=audio.currentTime;}} /><span>{time(duration)}</span></label>
{/snippet}
{#if current}
  <section class="music-player" aria-label="Lecteur musique" bind:this={playerElement}>
    <div class="bar"><button class="now" aria-label={`À suivre (${$musicQueue.tracks.length})`} aria-haspopup="dialog" onclick={()=>panel='queue'}><span class="art"><MusicArtwork src={current.coverUrl} label={current.album||current.title} /></span><span class="now-copy"><strong>{current.title}</strong><small>À suivre · {current.artist||'Artiste inconnu'}</small></span></button>
      <div class="transport">{@render controls()}</div>
      <div class="extras"><button aria-haspopup="dialog" onclick={()=>panel='lyrics'}>Paroles</button><label class="volume"><Icon name="volume" size={18} /><input aria-label="Volume musique" type="range" min="0" max="1" step="0.01" value={volume} oninput={event=>{volume=Number(event.currentTarget.value);audio.volume=volume;}} /></label><button onclick={clearMusic} aria-label="Arrêter et vider la file"><Icon name="close" size={20} /></button></div>
    </div>
    {#if error}<p class="playback-error" role="alert">{error}</p>{/if}
    <dialog class="music-dialog" bind:this={panelDialog} aria-labelledby="music-panel-title" oncancel={()=>panel=null} onclose={()=>panel=null}>
      <header class="panel-heading"><div><h2 id="music-panel-title">Musique en cours</h2><p>{current.title}</p></div><button onclick={()=>panel=null} aria-label="Fermer le panneau musique"><Icon name="close" size={20} /></button></header>
      <div class="panel-tabs" role="group" aria-label="Panneau musique"><button aria-pressed={panel==='queue'} onclick={()=>panel='queue'}>À suivre ({$musicQueue.tracks.length})</button><button aria-pressed={panel==='lyrics'} onclick={()=>panel='lyrics'}>Paroles</button></div>
      <div class="panel-body">
        {#if panel==='queue'}<ol>{#each $musicQueue.tracks as track,i (`${i}-${track.id}`)}<li class:current={i===$musicQueue.index}><button class="queue-title" aria-current={i===$musicQueue.index?'true':undefined} onclick={()=>musicQueue.update(q=>({...q,index:i,playing:true}))}><strong>{track.title}</strong><small>{track.artist}</small></button><button onclick={()=>removeMusic(i)} aria-label={`Retirer ${track.title} de la file`}><Icon name="close" size={18} /></button></li>{/each}</ol>
        {:else if lyricsLoading}<p role="status">Chargement des paroles…</p>{:else if lyrics.length}<div class="lyrics" bind:this={lyricsBox}>{#each lyrics as line,i (i)}<p data-line={i} class:active={i===activeLine}>{#if line.time!==null}<button onclick={()=>audio.currentTime=line.time!}>{line.text||'♪'}</button>{:else}{line.text}{/if}</p>{/each}</div>{:else}<p>Aucune parole disponible. Ajoutez un fichier .lrc ou .txt du même nom que la piste, puis relancez le scan.</p>{/if}
        <details class="playback-settings"><summary>Options de lecture</summary><div><button onclick={shuffleMusic}>Mélanger la file</button><label>Répétition<select aria-label="Répétition" value={$musicQueue.repeat} onchange={event=>musicQueue.update(q=>({...q,repeat:event.currentTarget.value as 'off'|'all'|'one'}))}><option value="off">Désactivée</option><option value="all">Toute la file</option><option value="one">Ce morceau</option></select></label><label>Volume<input aria-label="Volume musique du panneau" type="range" min="0" max="1" step="0.01" value={volume} oninput={event=>{volume=Number(event.currentTarget.value);audio.volume=volume;}} /></label><button onclick={()=>{panel=null;clearMusic();}}>Arrêter et vider la file</button></div></details>
      </div>
      <div class="panel-transport">{@render controls()}{#if error}<p class="playback-error" role="alert">{error}</p>{/if}</div>
    </dialog>
  </section>
{/if}
<style>
  :global(body.music-playing) { padding-bottom:calc(var(--music-player-height,110px) + 24px); }
  :global(body:has(.music-dialog[open])) { overflow:hidden; }
  .music-player { position:fixed; bottom:0; padding-bottom:env(safe-area-inset-bottom,0px); left:0; right:0; z-index:120; background:var(--color-surface); border-top:1px solid var(--color-line); color:var(--color-text); }
  .bar { display:grid; grid-template-columns:minmax(0,1fr) minmax(280px,1.2fr) minmax(0,1fr); gap:24px; padding:12px 24px; align-items:center; }
  button,select { font:inherit; color:inherit; min-height:44px; border:1px solid var(--color-line); background:transparent; border-radius:var(--radius-md); padding:10px 12px; cursor:pointer; }
  button { display:inline-flex; align-items:center; justify-content:center; } .now { display:flex; gap:12px; min-width:0; padding:0; border:0; text-align:left; justify-content:start; }
  .now-copy { min-width:0; } .now strong,.now small { display:block; overflow:hidden; text-overflow:ellipsis; white-space:nowrap; } .now strong { font-size:14px; font-weight:550; } small { color:var(--color-muted); margin-top:4px; font-size:12px; }
  .art { display:block; width:48px; height:48px; flex:none; }
  .buttons,.extras,.timeline { display:flex; align-items:center; justify-content:center; gap:8px; min-width:0; } .buttons button { width:44px; padding:0; border:0; }
  .buttons .play { background:var(--color-play); color:var(--color-on-play); border-radius:50%; }
  .timeline { font-size:11px; font-variant-numeric:tabular-nums; color:var(--color-muted); margin-top:6px; } .timeline input { flex:1; min-width:0; width:100%; height:24px; margin:0; }
  input { accent-color:var(--color-play); } .extras { justify-content:end; font-size:13px; } .extras > button:last-child { width:44px; padding:0; border:0; } .volume { display:flex; gap:8px; align-items:center; } .volume input { width:64px; }
  .playback-error { margin:0; padding:10px 16px; background:var(--color-danger-soft); font-size:13px; line-height:1.5; overflow-wrap:anywhere; }
  .music-dialog { width:min(720px,calc(100vw - 48px)); height:min(740px,calc(100dvh - 48px)); max-height:calc(100dvh - 48px); max-width:none; padding:0; overflow:hidden; border:1px solid var(--color-line); border-radius:var(--radius-lg); background:var(--color-surface); color:var(--color-text); }
  .music-dialog[open] { display:flex; flex-direction:column; } .music-dialog::backdrop { background:rgb(0 0 0 / .65); }
  .panel-heading { display:flex; align-items:center; justify-content:space-between; gap:16px; padding:20px 24px; flex:none; } .panel-heading > div { min-width:0; } .panel-heading > button { width:44px; flex:none; padding:0; }
  h2 { font-size:22px; font-weight:600; margin:0; } .panel-heading p { margin:6px 0 0; color:var(--color-muted); font-size:13px; overflow:hidden; text-overflow:ellipsis; white-space:nowrap; }
  .panel-tabs { display:flex; gap:20px; padding:0 24px; flex:none; border-bottom:1px solid var(--color-line); } .panel-tabs button { border:0; border-bottom:2px solid transparent; border-radius:0; padding-inline:0; color:var(--color-muted); } .panel-tabs [aria-pressed=true] { color:var(--color-text); border-bottom-color:var(--color-accent); }
  .panel-body { overflow:auto; overscroll-behavior:contain; min-height:0; padding:8px 24px 24px; flex:1; } .panel-body > p { font-size:14px; line-height:1.7; color:var(--color-muted); }
  ol { padding:0; margin:0; list-style:none; } li { display:flex; align-items:center; gap:12px; border-bottom:1px solid var(--color-line); padding:8px 0; min-width:0; } li > button:last-child { flex:none; width:44px; padding:0; border:0; }
  .queue-title { display:block; flex:1; min-width:0; text-align:left; border:0; padding:8px; } .queue-title strong,.queue-title small { display:block; overflow-wrap:anywhere; font-size:14px; line-height:1.5; } .queue-title small { font-size:12px; } .current .queue-title { box-shadow:inset 3px 0 var(--color-accent); }
  .lyrics { font-size:clamp(20px,3vw,28px); line-height:1.7; white-space:pre-wrap; overflow-wrap:anywhere; } .lyrics p { color:var(--color-muted); } .lyrics p.active { color:var(--color-text); } .lyrics button { font:inherit; line-height:inherit; text-align:left; justify-content:start; border:0; width:100%; padding:8px 0; }
  .playback-settings { margin-top:24px; border-top:1px solid var(--color-line); } summary { min-height:44px; padding-block:14px; font-size:14px; cursor:pointer; } .playback-settings > div { display:grid; grid-template-columns:repeat(2,minmax(0,1fr)); gap:16px; padding:12px 0; } .playback-settings label { display:grid; gap:8px; font-size:13px; } select { min-width:0; width:100%; font-size:16px; background:var(--color-surface); } .playback-settings input { width:100%; min-width:0; min-height:44px; }
  .panel-transport { padding:12px 24px; border-top:1px solid var(--color-line); flex:none; } .panel-transport .playback-error { margin-top:12px; }
  @media(max-width:900px) { .bar { grid-template-columns:minmax(0,1fr) auto; gap:6px 12px; padding:10px 16px; } .transport { display:contents; } .transport .buttons { grid-column:2; grid-row:1; gap:0; } .transport .timeline { grid-column:1/-1; grid-row:2; margin:0; } .timeline input { height:44px; } .extras { display:none; } .art { width:40px; height:40px; } .now { gap:10px; } }
  @media(max-width:600px) { .music-dialog { width:100vw; height:100dvh; max-height:100dvh; margin:0; border:0; border-radius:0; } .panel-heading { padding:16px; } .panel-tabs { padding-inline:16px; } .panel-body { padding-inline:16px; } .panel-transport { padding:12px 16px calc(12px + env(safe-area-inset-bottom,0px)); } .playback-settings > div { grid-template-columns:minmax(0,1fr); } }
  @media(max-height:520px) { .panel-heading { padding:8px 16px; } .panel-heading p { display:none; } .panel-tabs { padding-inline:16px; } .music-dialog { height:100dvh; max-height:100dvh; margin:auto; } .panel-transport { padding:8px 16px; } }
</style>
