<script lang="ts">
  import { onMount } from 'svelte';
  import { session } from '../auth/session';
  import { clearDescriptorCache, type Work } from './api';
  import type { Media } from '../player/types';
  import Icon from '../ui/Icon.svelte';
  import { subtitleErrorText, subtitleMatchText, subtitleRequest, type SubtitleInventory, type SubtitleResult } from './subtitle-download-api';

  let { work, selectedMedia, onclose }: { work: Work; selectedMedia?: Media; onclose: () => void } = $props();
  const copies = $derived.by(() => {
    const seen: string[] = [];
    const items: { id: string; label: string }[] = [];
    if (selectedMedia) {
      seen.push(selectedMedia.id);
      items.push({ id: selectedMedia.id, label: selectedMedia.title || 'Copie sélectionnée' });
    }
    for (const edition of work.editions ?? []) for (const version of edition.versions) for (const source of version.sources) {
      const id = source.playbackMediaId;
      if (!id || seen.includes(id)) continue;
      seen.push(id);
      items.push({ id, label: [edition.label, version.label, source.nodeId].filter(Boolean).join(' · ') || `Copie ${items.length + 1}` });
    }
    return items;
  });
  let assetId = $state('');
  let inventory = $state<SubtitleInventory | null>(null);
  let providerId = $state('');
  let language = $state('fr');
  let forced = $state(false);
  let sdh = $state(false);
  let items = $state<SubtitleResult[]>([]);
  let searchId = $state('');
  let page = $state(1);
  let totalPages = $state(1);
  let searched = $state(false);
  let busy = $state(false);
  let error = $state('');
  let message = $state('');
  let dialog: HTMLDialogElement;
  let controller: AbortController | undefined;
  const base = $derived(`/api/v1/admin/subtitles/${encodeURIComponent(assetId)}`);
  const quota = $derived(inventory?.quotas?.find(entry => entry.providerId === providerId));

  onMount(() => {
    dialog.showModal();
    assetId = copies[0]?.id ?? '';
    if (assetId) void refresh();
    return () => controller?.abort();
  });

  async function run(action: (signal: AbortSignal) => Promise<void>) {
    controller?.abort();
    const current = new AbortController();
    controller = current;
    busy = true;
    error = '';
    message = '';
    try { await action(current.signal); }
    catch (cause) {
      if (!current.signal.aborted) error = cause instanceof Error && cause.name !== 'TimeoutError' ? cause.message : 'Le délai de réponse est dépassé. Réessayez manuellement.';
    } finally { if (controller === current) busy = false; }
  }

  async function loadInventory(signal: AbortSignal) {
    const data = await subtitleRequest<SubtitleInventory>(base, signal, $session?.csrfToken ?? '');
    if (signal.aborted) return;
    inventory = data;
    if (!data.providers.some(provider => provider.id === providerId)) providerId = data.providers[0]?.id ?? '';
  }

  async function refresh() {
    inventory = null;
    items = [];
    searchId = '';
    searched = false;
    await run(loadInventory);
  }

  async function search(nextPage = 1) {
    await run(async signal => {
      const data = await subtitleRequest<{ searchId: string; items: SubtitleResult[]; totalPages: number }>(`${base}/search`, signal, $session?.csrfToken ?? '', 'POST', { providerId, language: language.trim(), forced: forced || undefined, sdh: sdh || undefined, page: nextPage });
      if (signal.aborted) return;
      searchId = data.searchId;
      items = data.items;
      page = nextPage;
      totalPages = Math.max(1, data.totalPages);
      searched = true;
    });
  }

  async function download(item: SubtitleResult) {
    await run(async signal => {
      const result = await subtitleRequest<{ id: string; status: string; remaining?: number | null; resetTimeUtc?: string | null }>(`${base}/download`, signal, $session?.csrfToken ?? '', 'POST', { searchId, fileId: item.id });
      if (signal.aborted) return;
      clearDescriptorCache();
      await loadInventory(signal);
      if (signal.aborted) return;
      message = result.status === 'completed' ? 'Sous-titre téléchargé. Relancez le lecteur puis choisissez sa piste dans Sous-titres.' : 'Téléchargement enregistré. Actualisez la liste pour vérifier son état.';
    });
  }

  async function remove(id: string) {
    await run(async signal => {
      await subtitleRequest<void>(`${base}/${encodeURIComponent(id)}`, signal, $session?.csrfToken ?? '', 'DELETE');
      if (signal.aborted) return;
      clearDescriptorCache();
      await loadInventory(signal);
      if (signal.aborted) return;
      message = 'Sous-titre téléchargé supprimé. Relancez le lecteur pour actualiser ses pistes.';
    });
  }

  function cancel() {
    controller?.abort();
    busy = false;
    message = 'Attente interrompue. Une opération déjà reçue par le serveur peut avoir abouti ; actualisez avant de réessayer.';
  }
</script>

<dialog bind:this={dialog} onclose={onclose} aria-labelledby="subtitle-download-title">
  <div class="modal-header"><h3 id="subtitle-download-title">Sous-titres · {work.title}</h3><button type="button" aria-label="Fermer" onclick={() => dialog.close()}><Icon name="close" size={18} /></button></div>
  <div class="modal-body" aria-busy={busy}>
    <p>Recherche et téléchargement manuels. Les fichiers sont conservés dans les données applicatives. Les médias originaux restent en lecture seule.</p>
    {#if copies.length > 1}<label>Version et copie<select bind:value={assetId} disabled={busy} onchange={() => void refresh()}>{#each copies as copy (copy.id)}<option value={copy.id}>{copy.label}</option>{/each}</select></label>{/if}
    {#if !assetId}<p role="status">Aucune copie disponible pour ce média.</p>{/if}
    {#if error}<p class="error" role="alert">{error}</p>{/if}
    {#if message}<p role="status">{message}</p>{/if}
    {#if busy}<p role="status">Opération en cours…</p><button type="button" onclick={cancel}>Interrompre l’attente</button>{/if}
    {#if inventory}
      {#if !inventory.enabled}<p role="status">{subtitleErrorText(inventory.unavailableCode ?? '')}</p>{/if}
      {#if inventory.providers.length === 0}<p role="status">Aucun fournisseur de sous-titres activé et autorisé. Configurez une extension dans les réglages administrateur.</p>{/if}
      {#if quota}
        <p role="status">Quota du fournisseur : {quota.remaining === null ? 'solde inconnu' : `${quota.remaining} téléchargement(s) restant(s)`}.{quota.resetTimeUtc ? ` Réinitialisation annoncée : ${quota.resetTimeUtc}.` : ''}{quota.nextAllowedAt > Date.now() / 1000 ? ` Prochaine tentative au plus tôt : ${new Date(quota.nextAllowedAt * 1000).toLocaleString('fr-FR')}.` : ''}</p>
      {/if}
      <form onchange={() => { searched = false; items = []; searchId = ''; }} onsubmit={event => { event.preventDefault(); void search(); }}>
        <label>Fournisseur<select bind:value={providerId} disabled={busy || !inventory.enabled}>{#each inventory.providers as provider (provider.id)}<option value={provider.id}>{provider.name}</option>{/each}</select></label>
        <label>Langue<input bind:value={language} required maxlength="35" pattern={'[A-Za-z]{2,8}(-[A-Za-z0-9]{1,8})*'} placeholder="fr" disabled={busy || !inventory.enabled} /></label>
        <label class="check"><input type="checkbox" bind:checked={forced} disabled={busy || !inventory.enabled} /> Uniquement forcés, si renseigné par le fournisseur</label>
        <label class="check"><input type="checkbox" bind:checked={sdh} disabled={busy || !inventory.enabled} /> Uniquement SDH, si renseigné par le fournisseur</label>
        <p>Aucune vérification par hash n’est effectuée dans ce parcours. Un résultat par identifiant ou titre ne garantit pas la synchronisation. Les quotas du compte fournisseur s’appliquent.</p>
        <button type="submit" disabled={busy || !inventory.enabled || !providerId}>Rechercher</button>
      </form>
      {#if searched}
        <h4>Résultats</h4>
        {#each items as item (item.id)}
          <div class="subtitle-row"><div><strong>{item.label}</strong><p>{item.language}{item.forced ? ' · Forcés' : ''}{item.sdh ? ' · SDH' : ''}</p><p>{subtitleMatchText(item.match)}</p></div><button type="button" disabled={busy || !inventory.enabled || inventory.downloads.some(entry => entry.providerId === providerId && entry.fileId === item.id && entry.status === 'completed')} onclick={() => void download(item)}>Télécharger</button></div>
        {:else}<p>Aucun résultat pour cette recherche.</p>{/each}
        {#if totalPages > 1}<nav aria-label="Pages des résultats"><button type="button" disabled={busy || page <= 1} onclick={() => void search(page - 1)}>Précédents</button><span>{page} / {totalPages}</span><button type="button" disabled={busy || page >= totalPages} onclick={() => void search(page + 1)}>Suivants</button></nav>{/if}
      {/if}
      <h4>Sous-titres téléchargés</h4>
      {#each inventory.downloads as entry (entry.id)}
        <div class="subtitle-row"><div><strong>{entry.label}</strong><p>{entry.language}{entry.forced ? ' · Forcés' : ''}{entry.sdh ? ' · SDH' : ''} · {entry.status === 'completed' ? 'Disponible dans le lecteur' : entry.status === 'failed' ? 'Échec' : entry.status === 'interrupted' ? 'Interrompu — supprimer avant de réessayer' : 'En attente ou en cours'}</p>{#if entry.errorCode}<p class="error">{subtitleErrorText(entry.errorCode)}</p>{/if}</div><button type="button" disabled={busy} onclick={() => void remove(entry.id)}>Supprimer le téléchargement</button></div>
      {:else}<p>Aucun sous-titre téléchargé pour cette copie.</p>{/each}
    {/if}
    {#if assetId}<button type="button" disabled={busy} onclick={() => void refresh()}>Actualiser la liste</button>{/if}
  </div>
</dialog>

<style>
  dialog { width:min(720px,calc(100vw - 32px)); max-height:90dvh; padding:0; border:1px solid var(--color-line); border-radius:var(--radius-sm); background:var(--color-surface); color:var(--color-text); }
  dialog::backdrop { background:rgba(0,0,0,.75); }
  .modal-header { display:flex; align-items:center; justify-content:space-between; gap:16px; padding:20px; border-bottom:1px solid var(--color-line); }
  h3 { margin:0; font-size:18px; } .modal-body { padding:20px; } p { font-size:13px; line-height:1.6; color:var(--color-muted); }
  label { display:block; margin:16px 0; } input:not([type=checkbox]),select { display:block; width:100%; margin-top:8px; padding:10px; background:var(--color-bg); color:var(--color-text); border:1px solid var(--color-line); border-radius:var(--radius-sm); }
  .check { display:flex; gap:10px; align-items:center; } button { padding:10px 14px; border:1px solid var(--color-line); border-radius:var(--radius-sm); background:var(--color-bg); color:var(--color-text); cursor:pointer; } button:disabled { opacity:.5; cursor:default; }
  .subtitle-row { display:flex; justify-content:space-between; gap:16px; align-items:center; padding:16px 0; border-bottom:1px solid var(--color-line); overflow-wrap:anywhere; } .subtitle-row p { margin:4px 0; } .error { color:var(--color-error,#ef7878); } nav { display:flex; gap:12px; align-items:center; margin-top:16px; }
  @media(max-width:600px) { .subtitle-row { align-items:flex-start; flex-direction:column; } }
</style>
