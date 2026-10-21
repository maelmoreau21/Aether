<script lang="ts">
  import { onMount } from 'svelte';
  import { SvelteMap } from 'svelte/reactivity';
  import { resolve } from '$app/paths';
  import { offlineItems,offlineKey,storedOfflineKey,unlockDetails,removeOffline,type OfflineItem,type OfflineDetails } from '../../lib/offline-vault';
  let items = $state<OfflineItem[]>([]);
  let details = $state<Record<string,OfflineDetails>>({});
  let phrase = $state(''); let error = $state(''); let selected = $state(''); let busy = $state(false); let loading = $state(true);
  const keys = new SvelteMap<string,CryptoKey>();
  let video = $state<HTMLVideoElement>();
  const legacyItems = $derived(items.filter(item=>item.complete&&!item.deviceKey));
  function stop(){if(video){video.pause();video.removeAttribute('src');video.load();}selected='';}
  async function load(){
    items = await offlineItems();
    for(const item of items.filter(item=>item.complete&&item.deviceKey)) {
      const key = await storedOfflineKey(item.id);
      if(key){keys.set(item.id,key);details[item.id]=await unlockDetails(item,key);}
    }
  }
  onMount(()=>{
    void load().catch(()=>error='Impossible d’ouvrir les téléchargements de cet appareil.').finally(()=>loading=false);
    const respond=(event:MessageEvent)=>{if(event.data?.type==='offline-key'&&event.ports[0])event.ports[0].postMessage(keys.get(event.data.id));};
    navigator.serviceWorker?.addEventListener('message',respond);
    return()=>{navigator.serviceWorker?.removeEventListener('message',respond);stop();keys.clear();};
  });
  async function unlock(event:SubmitEvent){
    event.preventDefault();busy=true;error='';let opened=0;
    try{for(const item of legacyItems){try{const key=await offlineKey(phrase,item.salt);const value=await unlockDetails(item,key);keys.set(item.id,key);details[item.id]=value;opened++;}catch{/* Legacy copies can have different passphrases. */}}if(!opened)error='Aucun ancien téléchargement ne correspond à cette phrase.';}
    finally{phrase='';busy=false;}
  }
  async function remove(id:string){if(selected===id)stop();await removeOffline(id);keys.delete(id);delete details[id];await load();}
</script>
<main>
  <a href={resolve('/')}>Retour à la médiathèque</a>
  <h1>Téléchargements</h1><p>Vos médias disponibles sur cet appareil, même hors connexion.</p>
  {#if loading}<p role="status">Chargement…</p>{/if}
  {#each items as item(item.id)}
    <article>
      <div><strong>{details[item.id]?.title??(item.complete?(item.deviceKey?'Copie locale indisponible':'Ancien téléchargement verrouillé'):'Téléchargement incomplet')}</strong><p>{Math.round(item.size/1024**2)} Mio</p></div>
      <button disabled={!details[item.id]} onclick={()=>{stop();selected=item.id;}}>Lire</button>
      <button onclick={()=>remove(item.id).catch(()=>error='Suppression impossible.')}>Supprimer</button>
    </article>
  {:else}{#if !loading}<p>Aucun téléchargement enregistré.</p>{/if}{/each}
  {#if legacyItems.length}<details class="legacy"><summary>Récupérer mes anciens téléchargements</summary><p>Les copies créées avec l’ancien système conservent leur phrase. Les nouveaux téléchargements s’ouvrent directement.</p><form onsubmit={unlock}><label>Ancienne phrase de déverrouillage<input type="password" autocomplete="current-password" required bind:value={phrase} /></label><button disabled={busy}>Ouvrir les anciennes copies</button></form></details>{/if}
  {#if selected&&details[selected]}<video bind:this={video} controls autoplay playsinline src={`/app/offline-media/${selected}`} onerror={()=>error='Ce fichier ne peut pas être lu dans ce navigateur.'}><track kind="captions" /></video>{/if}
  {#if error}<p role="alert">{error}</p>{/if}
</main>
<style>
main { max-width:1000px;margin:auto;padding:24px; } form,label,article { display:flex;gap:12px;align-items:center;flex-wrap:wrap;margin:16px 0; } article { border-bottom:1px solid var(--color-line);padding:16px 0; } article>div { flex:1;min-width:150px; } input,button { padding:10px;background:var(--color-surface);color:var(--color-text);border:1px solid var(--color-line);border-radius:var(--radius-sm);font:inherit; } button,summary { cursor:pointer; } button:disabled { opacity:.5;cursor:default; } a { color:var(--color-accent); } video { width:100%;max-height:70vh; } .legacy { margin-block:30px;color:var(--color-muted); } .legacy p { max-width:650px; } [role="alert"] { color:var(--status-error); }
</style>
