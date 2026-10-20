<script lang="ts">
  import { onMount } from 'svelte';
  import Navbar from '../../lib/ui/Navbar.svelte';
  import { DEFAULT_NAME } from '../../lib/config/brand';
  import OfflineDownload from '../../lib/catalog/OfflineDownload.svelte';
  interface Track { id:string; title:string; artist:string; album:string; albumArtist:string; mime:string; codec:string; duration:number|null }
  let tracks=$state<Track[]>([]);let query=$state('');let error=$state('');let busy=$state(false);let offset=$state(0);let total=$state(0);let selected=$state<Track|null>(null);let audio=$state<HTMLAudioElement>();
  async function load(next=0){busy=true;error='';try{const response=await fetch('/api/v1/music?'+new URLSearchParams({q:query,offset:String(next),limit:'100'}));if(!response.ok)throw new Error('Impossible de charger votre musique.');const data=await response.json();tracks=data.items;total=data.total;offset=next;}catch(cause){error=cause instanceof Error?cause.message:'Chargement impossible.';}finally{busy=false;}}
  function play(track:Track){if(audio&&!audio.canPlayType(track.mime)){error=`Ce navigateur ne déclare pas pouvoir lire ${track.mime} (${track.codec}).`;return;}selected=track;error='';}
  function next(){const index=tracks.findIndex(t=>t.id===selected?.id);if(index>=0&&index+1<tracks.length)play(tracks[index+1]);}
  onMount(()=>{void load();});
</script>
<svelte:head><title>Musique · {DEFAULT_NAME}</title></svelte:head>
<Navbar section="music" />
<main><h1>Musique</h1><form onsubmit={event=>{event.preventDefault();void load();}}><label>Rechercher un titre, artiste ou album<input type="search" bind:value={query} maxlength="256" /></label><button disabled={busy}>Rechercher</button></form>
{#if error}<p role="alert">{error}</p>{/if}
{#if busy}<p role="status">Chargement…</p>{/if}
<p>{total} piste(s)</p><div class="tracks">{#each tracks as track (track.id)}<article><div><strong>{track.title}</strong><p>{track.artist} · {track.album || 'Album inconnu'} · {track.codec}</p></div><button onclick={()=>play(track)} aria-label={`Lire ${track.title}`}>Lire</button></article>{:else}{#if !busy}<p>Aucune piste musicale. Ajoutez une bibliothèque Musique dans l’administration puis lancez son scan.</p>{/if}{/each}</div>
<nav aria-label="Pagination musique"><button disabled={busy||offset===0} onclick={()=>load(Math.max(0,offset-100))}>Précédent</button><button disabled={busy||offset+tracks.length>=total} onclick={()=>load(offset+100)}>Suivant</button></nav>
{#if selected}<section class="player"><strong>{selected.title}</strong><p>{selected.artist} · {selected.album}</p><audio bind:this={audio} controls autoplay src={`/api/v1/music/tracks/${encodeURIComponent(selected.id)}/content`} onended={next} onerror={()=>error='Lecture impossible : format non décodable, connexion interrompue ou droits révoqués.'}></audio><button onclick={next}>Piste suivante</button><OfflineDownload url={`/api/v1/music/tracks/${selected.id}/content`} title={`${selected.artist} — ${selected.title}`} mime={selected.mime} /></section>{/if}
</main>
<style>main { max-width:1100px;margin:auto;padding:24px; } form,label { display:flex;gap:12px;flex-wrap:wrap;align-items:center; } label { flex:1; } input { flex:1;min-width:160px; } button,input { padding:10px 14px;background:var(--color-surface);color:var(--color-text);border:1px solid var(--color-line);border-radius:8px; } article { display:flex;justify-content:space-between;gap:16px;align-items:center;padding:14px 0;border-bottom:1px solid var(--color-line); } p { color:var(--color-muted); } nav { display:flex;gap:12px;margin:20px 0; } .player { position:sticky;bottom:0;background:var(--color-surface);padding:16px;border:1px solid var(--color-line);border-radius:12px; } audio { width:100%; } </style>
