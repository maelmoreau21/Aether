<script lang="ts">
  import { onMount } from 'svelte';
  import { SvelteMap } from 'svelte/reactivity';
  import { resolve } from '$app/paths';
  import Navbar from '../../lib/ui/Navbar.svelte';
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
<Navbar section="downloads" />
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
  main { width: 100%; max-width: var(--maxw); margin: 0 auto; padding: 20px var(--gutter) 80px; min-width: 0; color: var(--tx); }
  main > a { display: inline-flex; align-items: center; min-height: 44px; margin-bottom: 8px; padding: 0 12px 0 4px; border-radius: var(--r-sm); color: var(--mu); font-size: 14px; font-weight: 700; transition: color var(--t-fast) ease, background-color var(--t-fast) ease; }
  main > a:hover { color: var(--tx); background: var(--pn); }
  h1 { margin: 0; font-size: clamp(32px, 4vw, 44px); font-weight: 800; letter-spacing: -0.03em; line-height: 1.02; }
  p { margin: 8px 0 0; color: var(--mu); font-size: 15px; line-height: 1.6; }
  h1 + p { margin-bottom: 24px; }
  form, label, article { display: flex; gap: 12px; align-items: center; flex-wrap: wrap; }
  article { margin: 0; min-height: 72px; padding: 12px 16px; border: 1px solid var(--line); border-top-width: 0; background: var(--bg-2); }
  h1 + p + article, [role='status'] + article { border-top-width: 1px; border-radius: var(--r-lg) var(--r-lg) 0 0; }
  article:last-of-type { border-radius: 0 0 var(--r-lg) var(--r-lg); }
  h1 + p + article:last-of-type, [role='status'] + article:last-of-type { border-radius: var(--r-lg); }
  article > div { flex: 1; min-width: 150px; }
  article strong { font-size: 15px; font-weight: 700; overflow-wrap: anywhere; }
  article p { margin: 2px 0 0; font-size: 13px; font-variant-numeric: tabular-nums; }
  label { flex-direction: column; align-items: stretch; gap: 8px; font-size: 14px; font-weight: 700; color: var(--tx); }
  form { align-items: flex-end; margin-top: 16px; }
  input { width: min(360px, 100%); height: 46px; min-height: 46px; padding: 0 14px; border: 1px solid transparent; border-radius: var(--r-sm); background: var(--pn); color: var(--tx); font: inherit; font-size: 15px; font-weight: 500; box-shadow: none; transition: border-color var(--t-fast) ease, background-color var(--t-fast) ease; }
  input:hover { background: var(--pn-2); }
  input:focus { outline: none; border-color: var(--ac); background: var(--bg-3); }
  button { display: inline-flex; align-items: center; justify-content: center; min-height: 44px; padding: 0 18px; border: 0; border-radius: var(--r-md); background: var(--pn-2); color: var(--tx); font: inherit; font-size: 14px; font-weight: 700; cursor: pointer; transition: background-color var(--t-fast) ease, opacity var(--t-fast) ease, transform var(--t-fast) var(--ease); }
  button:hover:not(:disabled) { background: var(--pn-3); }
  button:active:not(:disabled) { transform: scale(0.98); }
  button:disabled { opacity: 0.45; cursor: default; }
  article > button:first-of-type { background: var(--btn); color: var(--btn-tx); }
  article > button:first-of-type:hover:not(:disabled) { background: var(--btn); opacity: 0.9; }
  article > button:last-of-type { background: transparent; color: var(--err); }
  article > button:last-of-type:hover:not(:disabled) { background: var(--err-soft); }
  form button { height: 46px; background: var(--btn); color: var(--btn-tx); }
  form button:hover:not(:disabled) { background: var(--btn); opacity: 0.9; }
  button:focus-visible, a:focus-visible, summary:focus-visible, input:focus-visible { outline: 2px solid var(--ac); outline-offset: 3px; }
  summary { display: flex; align-items: center; min-height: 48px; font-size: 15px; font-weight: 700; color: var(--tx); cursor: pointer; border-radius: var(--r-xs); }
  video { display: block; width: 100%; max-height: 70vh; margin-top: 24px; border-radius: var(--r-lg); background: var(--bg-2); }
  .legacy { margin-block: 28px; padding: 8px 24px 24px; border: 1px solid var(--line); border-radius: var(--r-lg); background: var(--bg-2); color: var(--mu); }
  .legacy p { max-width: 650px; margin-top: 0; font-size: 14px; }
  [role='status'] { margin-bottom: 16px; }
  [role='alert'] { margin-top: 20px; padding: 12px 14px; border-radius: var(--r-sm); background: var(--err-soft); color: var(--err); font-weight: 600; }
  @media (max-width: 640px) {
    main { padding-top: 12px; }
    article { padding: 12px; }
    .legacy { padding: 4px 16px 16px; border-radius: var(--r-md); }
  }
</style>
