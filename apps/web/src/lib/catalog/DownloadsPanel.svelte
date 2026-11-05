<script lang="ts">
  import { onMount } from 'svelte';
  import { SvelteMap } from 'svelte/reactivity';
  import { resolve } from '$app/paths';
  import { offlineItems, offlineKey, storedOfflineKey, unlockDetails, removeOffline, type OfflineItem, type OfflineDetails } from '../offline-vault';
  import { t } from '../i18n';

  /** `embedded` hides the page title and back link, for the settings panel. */
  let { embedded = false }: { embedded?: boolean } = $props();
  let items = $state<OfflineItem[]>([]);
  let details = $state<Record<string, OfflineDetails>>({});
  let phrase = $state(''); let error = $state(''); let selected = $state(''); let busy = $state(false); let loading = $state(true);
  const keys = new SvelteMap<string, CryptoKey>();
  let video = $state<HTMLVideoElement>();
  const legacyItems = $derived(items.filter(item => item.complete && !item.deviceKey));
  function stop() { if (video) { video.pause(); video.removeAttribute('src'); video.load(); } selected = ''; }
  async function load() {
    items = await offlineItems();
    for (const item of items.filter(item => item.complete && item.deviceKey)) {
      const key = await storedOfflineKey(item.id);
      if (key) { keys.set(item.id, key); details[item.id] = await unlockDetails(item, key); }
    }
  }
  onMount(() => {
    void load().catch(() => error = 'Impossible d’ouvrir les téléchargements de cet appareil.').finally(() => loading = false);
    const respond = (event: MessageEvent) => { if (event.data?.type === 'offline-key' && event.ports[0]) event.ports[0].postMessage(keys.get(event.data.id)); };
    navigator.serviceWorker?.addEventListener('message', respond);
    return () => { navigator.serviceWorker?.removeEventListener('message', respond); stop(); keys.clear(); };
  });
  async function unlock(event: SubmitEvent) {
    event.preventDefault(); busy = true; error = ''; let opened = 0;
    try {
      for (const item of legacyItems) {
        try { const key = await offlineKey(phrase, item.salt); const value = await unlockDetails(item, key); keys.set(item.id, key); details[item.id] = value; opened++; }
        catch { /* Legacy copies can have different passphrases. */ }
      }
      if (!opened) error = 'Aucun ancien téléchargement ne correspond à cette phrase.';
    } finally { phrase = ''; busy = false; }
  }
  async function remove(id: string) { if (selected === id) stop(); await removeOffline(id); keys.delete(id); delete details[id]; await load(); }
</script>

<div class="downloads" class:embedded>
  {#if embedded}
    <h2>{$t('downloads')}</h2>
  {:else}
    <a class="back" href={resolve('/')}>Retour à la médiathèque</a>
    <h1>{$t('downloads')}</h1>
  {/if}
  <p class="intro">Vos médias disponibles sur cet appareil, même hors connexion.</p>
  {#if loading}<p role="status">{$t('loading')}</p>{/if}
  {#if items.length}
    <ul class="items">
      {#each items as item (item.id)}
        <li>
          <div><strong>{details[item.id]?.title ?? (item.complete ? (item.deviceKey ? 'Copie locale indisponible' : 'Ancien téléchargement verrouillé') : 'Téléchargement incomplet')}</strong><p>{Math.round(item.size / 1024 ** 2)} Mio</p></div>
          <button class="play" disabled={!details[item.id]} onclick={() => { stop(); selected = item.id; }}>Lire</button>
          <button class="remove" onclick={() => remove(item.id).catch(() => error = 'Suppression impossible.')}>{$t('delete')}</button>
        </li>
      {/each}
    </ul>
  {:else if !loading}<p class="empty">Aucun téléchargement enregistré.</p>{/if}
  {#if legacyItems.length}<details class="legacy"><summary>Récupérer mes anciens téléchargements</summary><p>Les copies créées avec l’ancien système conservent leur phrase. Les nouveaux téléchargements s’ouvrent directement.</p><form onsubmit={unlock}><label>Ancienne phrase de déverrouillage<input type="password" autocomplete="current-password" required bind:value={phrase} /></label><button disabled={busy}>Ouvrir les anciennes copies</button></form></details>{/if}
  {#if selected && details[selected]}<video bind:this={video} controls autoplay playsinline src={`/app/offline-media/${selected}`} onerror={() => error = 'Ce fichier ne peut pas être lu dans ce navigateur.'}><track kind="captions" /></video>{/if}
  {#if error}<p role="alert">{error}</p>{/if}
</div>

<style>
  .downloads { min-width: 0; color: var(--tx); }
  .back { display: inline-flex; align-items: center; min-height: 44px; margin-bottom: 8px; padding: 0 12px 0 4px; border-radius: var(--r-sm); color: var(--mu); font-size: 14px; font-weight: 700; transition: color var(--t-fast) ease, background-color var(--t-fast) ease; }
  .back:hover { color: var(--tx); background: var(--pn); }
  h1 { margin: 0; font-size: clamp(32px, 4vw, 44px); font-weight: 800; letter-spacing: -0.03em; line-height: 1.02; }
  h2 { margin: 0; font-size: 24px; font-weight: 800; letter-spacing: -0.02em; }
  p { margin: 8px 0 0; color: var(--mu); font-size: 15px; line-height: 1.6; }
  .intro { margin-bottom: 24px; }
  .embedded .intro { font-size: 14px; margin-bottom: 18px; }
  .items { display: grid; margin: 0; padding: 0; list-style: none; border: 1px solid var(--line); border-radius: var(--r-lg); background: var(--bg-2); overflow: hidden; }
  .items li { display: flex; gap: 12px; align-items: center; flex-wrap: wrap; min-height: 72px; padding: 12px 16px; border-top: 1px solid var(--line); }
  .items li:first-child { border-top: 0; }
  .items li > div { flex: 1; min-width: 150px; }
  .items strong { font-size: 15px; font-weight: 700; overflow-wrap: anywhere; }
  .items p { margin: 2px 0 0; font-size: 13px; font-variant-numeric: tabular-nums; }
  .empty { padding: 20px; border: 1px dashed var(--line-2); border-radius: var(--r-lg); text-align: center; font-size: 14px; }
  form, label { display: flex; gap: 12px; align-items: center; flex-wrap: wrap; }
  label { flex-direction: column; align-items: stretch; gap: 8px; font-size: 14px; font-weight: 700; color: var(--tx); }
  form { align-items: flex-end; margin-top: 16px; }
  input { width: min(360px, 100%); height: 46px; min-height: 46px; padding: 0 14px; border: 1px solid transparent; border-radius: var(--r-sm); background: var(--pn); color: var(--tx); font: inherit; font-size: 15px; font-weight: 500; box-shadow: none; transition: border-color var(--t-fast) ease, background-color var(--t-fast) ease; }
  input:hover { background: var(--pn-2); }
  input:focus { outline: none; border-color: var(--ac); background: var(--bg-3); }
  button { display: inline-flex; align-items: center; justify-content: center; min-height: 44px; padding: 0 18px; border: 0; border-radius: var(--r-md); background: var(--pn-2); color: var(--tx); font: inherit; font-size: 14px; font-weight: 700; cursor: pointer; transition: background-color var(--t-fast) ease, opacity var(--t-fast) ease, transform var(--t-fast) var(--ease); }
  button:hover:not(:disabled) { background: var(--pn-3); }
  button:active:not(:disabled) { transform: scale(0.98); }
  button:disabled { opacity: 0.45; cursor: default; }
  .play, form button { background: var(--btn); color: var(--btn-tx); }
  .play:hover:not(:disabled), form button:hover:not(:disabled) { background: var(--btn); opacity: 0.9; }
  .remove { background: transparent; color: var(--err); }
  .remove:hover:not(:disabled) { background: var(--err-soft); }
  form button { height: 46px; }
  button:focus-visible, a:focus-visible, summary:focus-visible, input:focus-visible { outline: 2px solid var(--ac); outline-offset: 3px; }
  summary { display: flex; align-items: center; min-height: 48px; font-size: 15px; font-weight: 700; color: var(--tx); cursor: pointer; border-radius: var(--r-xs); }
  video { display: block; width: 100%; max-height: 70vh; margin-top: 24px; border-radius: var(--r-lg); background: var(--bg-2); }
  .legacy { margin-block: 28px; padding: 8px 24px 24px; border: 1px solid var(--line); border-radius: var(--r-lg); background: var(--bg-2); color: var(--mu); }
  .legacy p { max-width: 650px; margin-top: 0; font-size: 14px; }
  [role='status'] { margin-bottom: 16px; }
  [role='alert'] { margin-top: 20px; padding: 12px 14px; border-radius: var(--r-sm); background: var(--err-soft); color: var(--err); font-weight: 600; }
  @media (max-width: 640px) {
    .items li { padding: 12px; }
    .legacy { padding: 4px 16px 16px; border-radius: var(--r-md); }
  }
</style>
