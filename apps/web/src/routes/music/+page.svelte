<script lang="ts">
  import { page } from '$app/state';
  import { onDestroy, untrack } from 'svelte';
  import { accessibleLibraries } from '../../lib/catalog/libraries';
  import Navbar from '../../lib/ui/Navbar.svelte';
  import { DEFAULT_NAME } from '../../lib/config/brand';
  import {playMusic,enqueueMusic} from '../../lib/music/queue';
  import AddToPlaylist from '../../lib/music/AddToPlaylist.svelte';
  interface Track { id:string; title:string; artist:string; album:string; albumArtist:string; mime:string; codec:string; duration:number|null }
  let tracks=$state<Track[]>([]);let query=$state('');let error=$state('');let busy=$state(false);let offset=$state(0);let total=$state(0);
  let pending: AbortController | undefined;
  async function load(next=0){
    pending?.abort();const controller=new AbortController();pending=controller;
    const timer=setTimeout(()=>controller.abort(),8000);busy=true;error='';
    try{
      const response=await fetch('/api/v1/music?'+new URLSearchParams({q:query,offset:String(next),limit:'100',library:page.url.searchParams.get('library')??''}),{signal:controller.signal});
      if(!response.ok)throw new Error('Impossible de charger votre musique.');
      const data=await response.json();if(pending!==controller)return;
      tracks=data.items;total=data.total;offset=next;
    }catch(cause){if(pending===controller)error=controller.signal.aborted?'Le chargement prend trop de temps. Réessayez.':cause instanceof Error?cause.message:'Chargement impossible.';}
    finally{clearTimeout(timer);if(pending===controller)busy=false;}
  }
  onDestroy(()=>{pending?.abort();pending=undefined;});
  function play(track:Track){playMusic(tracks,tracks.findIndex(item=>item.id===track.id));error='';}
  $effect(()=>{const id=page.url.searchParams.get('library');void id;untrack(()=>void load());});
</script>
<svelte:head><title>Musique · {DEFAULT_NAME}</title></svelte:head>
<Navbar section="music" />
<main><h1>{$accessibleLibraries.find(item=>item.id===page.url.searchParams.get('library'))?.name ?? 'Musique'}</h1><form onsubmit={event=>{event.preventDefault();void load();}}><label>Rechercher un titre, artiste ou album<input type="search" bind:value={query} maxlength="256" /></label><button disabled={busy}>Rechercher</button></form>
{#if error}<p role="alert">{error}</p>{/if}
{#if busy}<p role="status">Chargement…</p>{/if}
<div class="music-heading"><p>{total} piste(s)</p><button disabled={!tracks.length} onclick={()=>playMusic(tracks)}>Lire cette sélection</button></div><div class="tracks">{#each tracks as track (track.id)}<article><div><strong>{track.title}</strong><p>{track.artist} · {track.album || 'Album inconnu'} · {track.codec}</p></div><button onclick={()=>play(track)} aria-label={`Lire ${track.title}`}>Lire</button><button onclick={()=>enqueueMusic(track)} aria-label={`Ajouter ${track.title} à la file`}>À suivre</button><AddToPlaylist id={track.id} /></article>{:else}{#if !busy}<p>Aucune piste musicale. Ajoutez une bibliothèque Musique dans l’administration puis lancez son scan.</p>{/if}{/each}</div>
<nav aria-label="Pagination musique"><button disabled={busy||offset===0} onclick={()=>load(Math.max(0,offset-100))}>Précédent</button><button disabled={busy||offset+tracks.length>=total} onclick={()=>load(offset+100)}>Suivant</button></nav>
</main>
<style>main { max-width:1100px;margin:auto;padding:24px; } form,label { display:flex;gap:12px;flex-wrap:wrap;align-items:center; } label { flex:1; } input { flex:1;min-width:160px; } button,input { padding:10px 14px;background:var(--color-surface);color:var(--color-text);border:1px solid var(--color-line);border-radius:8px; } .music-heading{display:flex;justify-content:space-between;align-items:center} article { display:flex;flex-wrap:wrap;justify-content:space-between;gap:16px;align-items:center;padding:14px 0;border-bottom:1px solid var(--color-line); } p { color:var(--color-muted); } nav { display:flex;gap:12px;margin:20px 0; } </style>
