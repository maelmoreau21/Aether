<script lang="ts">
  import { onMount } from 'svelte';
  import { session } from '../auth/session';
  let { visible = true }: { visible?: boolean } = $props();
  interface Archive { id: string; size: number; createdAt: number }
  interface Schedule { enabled: boolean; intervalMinutes: number; nextDueAt: number; lastStartedAt: number | null; lastCompletedAt: number | null; lastOutcome: string }
  interface Listing { available: boolean; reason: string | null; running: boolean; archives: Archive[]; schedule: Schedule }
  
  interface MemoryReport {
    core: {
      pid: number;
      name: string;
      rssBytes: number;
      anonBytes: number;
      fileBytes: number;
      peakRssBytes: number;
      virtBytes: number;
    };
    children: Array<{
      pid: number;
      name: string;
      rssBytes: number;
    }>;
    childrenTotalRssBytes: number;
    host: {
      memTotalBytes: number;
      memAvailableBytes: number;
      memFreeBytes: number;
      linuxCachedBytes: number;
      linuxBuffersBytes: number;
      linuxSreclaimableBytes: number;
      linuxReclaimableCacheBytes: number;
    } | null;
    summary: {
      coreRssBytes: number;
      childrenRssBytes: number;
      totalAetherRssBytes: number;
      linuxPageCacheBytes: number;
      diagnosis: string;
      reclaimableAdvice: string;
    };
  }

  let listing = $state<Listing | null>(null);
  let memoryReport = $state<MemoryReport | null>(null);
  let loading = $state(true);
  let memoryLoading = $state(false);
  let memoryTrimming = $state(false);
  let creating = $state(false);
  let error = $state('');
  let success = $state('');
  let scheduleEnabled = $state(false);
  let intervalMinutes = $state(1440);
  let saving = $state(false);
  const outcomeLabels: Record<string, string> = { never: 'Aucune exécution', running: 'En cours', succeeded: 'Réussie', failed: 'Échec — vérifiez le dossier serveur et son espace libre', interrupted: 'Interrompue au redémarrage' };
  let active = true;

  async function refresh() {
    loading = true;
    error = '';
    try {
      const response = await fetch('/api/v1/admin/backups', { credentials: 'same-origin', cache: 'no-store', signal: AbortSignal.timeout(15000) });
      if (!response.ok) throw new Error(`Chargement impossible (HTTP ${response.status}).`);
      const data: Listing = await response.json();
      if (active) { listing = data; scheduleEnabled = data.schedule.enabled; intervalMinutes = data.schedule.intervalMinutes; }
    } catch (cause) {
      if (active) error = cause instanceof Error ? cause.message : 'Chargement impossible.';
    } finally { if (active) loading = false; }
    void refreshMemory();
  }

  async function refreshMemory() {
    memoryLoading = true;
    try {
      const response = await fetch('/api/v1/admin/system/memory', { credentials: 'same-origin', cache: 'no-store', signal: AbortSignal.timeout(10000) });
      if (response.ok && active) {
        memoryReport = await response.json();
      }
    } catch {
      // Non-fatal if memory metrics unavailable
    } finally {
      if (active) memoryLoading = false;
    }
  }

  async function trimMemory() {
    if (!$session || memoryTrimming) return;
    memoryTrimming = true;
    try {
      const response = await fetch('/api/v1/admin/system/memory/trim', {
        method: 'POST',
        credentials: 'same-origin',
        cache: 'no-store',
        headers: { 'x-csrf-token': $session.csrfToken },
        signal: AbortSignal.timeout(15000),
      });
      if (response.ok && active) {
        memoryReport = await response.json();
        success = 'Caches purgés et mémoire restituée au système d’exploitation.';
      }
    } catch {
      error = 'Impossible de libérer la mémoire.';
    } finally {
      if (active) memoryTrimming = false;
    }
  }

  async function create() {
    if (!$session || creating) return;
    creating = true;
    error = '';
    success = '';
    try {
      const response = await fetch('/api/v1/admin/backups', { method: 'POST', credentials: 'same-origin', cache: 'no-store', headers: { 'x-csrf-token': $session.csrfToken }, signal: AbortSignal.timeout(300000) });
      if (!response.ok) {
        const detail = await response.json().catch(() => null);
        throw new Error(typeof detail?.code === 'string' && detail.code.startsWith('backup_') && typeof detail.message === 'string'
          ? detail.message
          : response.status === 409 ? 'Une sauvegarde est déjà en cours ou le dossier serveur est refusé. Actualisez son état.' : `Sauvegarde impossible (HTTP ${response.status}).`);
      }
      if (active) { success = 'Sauvegarde complète créée. Archive non chiffrée à protéger.'; await refresh(); }
    } catch (cause) {
      if (active) error = cause instanceof Error && cause.name === 'TimeoutError' ? 'Délai dépassé. La sauvegarde peut continuer côté serveur : actualisez la liste avant de réessayer.' : cause instanceof Error ? cause.message : 'Sauvegarde impossible.';
    } finally { if (active) creating = false; }
  }

  async function saveSchedule(event: SubmitEvent) {
    event.preventDefault();
    if (!$session || saving) return;
    saving = true;
    error = '';
    success = '';
    try {
      const response = await fetch('/api/v1/admin/backups/schedule', { method: 'PUT', credentials: 'same-origin', cache: 'no-store', headers: { 'content-type': 'application/json', 'x-csrf-token': $session.csrfToken }, body: JSON.stringify({ enabled: scheduleEnabled, intervalMinutes }), signal: AbortSignal.timeout(15000) });
      if (!response.ok) throw new Error(`Planification impossible (HTTP ${response.status}).`);
      if (active) { await refresh(); success = 'Planification enregistrée.'; }
    } catch (cause) {
      if (active) error = cause instanceof Error ? cause.message : 'Planification impossible.';
    } finally { if (active) saving = false; }
  }

  const mb = (bytes: number) => (bytes / 1024 / 1024).toFixed(1);

  onMount(() => { active = true; return () => { active = false; }; });
  $effect(() => { if (visible) void refresh(); });
</script>

<section id="backups-section" class="maintenance" aria-labelledby="maintenance-heading">
  <header><p class="eyebrow">MAINTENANCE</p><h2 id="maintenance-heading">Sauvegardes et base de données</h2><p>Sauvegardez la base complète et la configuration du serveur dans une archive non chiffrée. Réservez son accès à l'administrateur. Sur un serveur Windows, vérifiez les ACL du dossier de sauvegarde et de ses copies : Aether hérite des autorisations du dossier et ne les contrôle pas.</p></header>
  <div class="actions"><button class="primary" onclick={create} disabled={loading || creating || !listing?.available || listing?.running}>{creating || listing?.running ? 'Sauvegarde en cours…' : 'Créer une sauvegarde'}</button><button onclick={refresh} disabled={loading}>Actualiser</button></div>
  {#if error}<p class="error" role="alert">{error}</p>{/if}
  {#if success}<p role="status">{success}</p>{/if}

  {#if memoryReport}
    <article class="memory-card">
      <div class="memory-header">
        <div>
          <h3>Consommation mémoire réelle & cache Linux</h3>
          <p>Distingue la RAM allouée par le Core, les processus enfants (Wasm) et le cache disque du noyau Linux.</p>
        </div>
        <button onclick={trimMemory} disabled={memoryTrimming || memoryLoading}>
          {memoryTrimming ? 'Libération en cours…' : 'Libérer la mémoire (Trim)'}
        </button>
      </div>

      <div class="memory-grid">
        <div class="metric">
          <small>Core Aether (RSS)</small>
          <strong>{mb(memoryReport.core.rssBytes)} Mo</strong>
          <span>Pic historique : {mb(memoryReport.core.peakRssBytes)} Mo</span>
        </div>
        <div class="metric">
          <small>Tas anonyme (Heap)</small>
          <strong>{mb(memoryReport.core.anonBytes)} Mo</strong>
          <span>Données actives en RAM</span>
        </div>
        <div class="metric">
          <small>Fichiers & mmap</small>
          <strong>{mb(memoryReport.core.fileBytes)} Mo</strong>
          <span>SQLite mmap & binaires</span>
        </div>
        <div class="metric">
          <small>Processus enfants (Wasm)</small>
          <strong>{mb(memoryReport.childrenTotalRssBytes)} Mo</strong>
          <span>{memoryReport.children.length} worker(s) actif(s)</span>
        </div>
        {#if memoryReport.host}
          <div class="metric highlight">
            <small>Cache disque Linux (OS)</small>
            <strong>{mb(memoryReport.host.linuxReclaimableCacheBytes)} Mo</strong>
            <span>100% restituable par l’OS</span>
          </div>
        {/if}
      </div>

      <div class="notice">
        <strong>Diagnostic :</strong>
        <p>{memoryReport.summary.diagnosis}</p>
        <p class="advice">{memoryReport.summary.reclaimableAdvice}</p>
      </div>
    </article>
  {/if}

  {#if loading}<p role="status">Chargement des archives…</p>{:else if listing}
    {#if !listing.available}<div class="notice"><strong>Sauvegarde indisponible</strong><p>{listing.reason ?? "Le service de sauvegarde n'est pas disponible actuellement."}</p></div>{:else}
      <article><h3>Planification automatique</h3>
        <form onsubmit={saveSchedule}>
          <label class="check-row"><input type="checkbox" bind:checked={scheduleEnabled} disabled={saving} /> Activer les sauvegardes automatiques</label>
          <label class="interval-field">Intervalle en minutes <input type="number" min="60" max="10080" step="1" required bind:value={intervalMinutes} disabled={saving} /></label>
          <button type="submit" disabled={saving}>{saving ? 'Enregistrement…' : 'Enregistrer la planification'}</button>
        </form>
        <p>De 1 heure à 7 jours. Le Core doit être démarré. Aucune suppression automatique des archives.</p>
        {#if listing.schedule.enabled}<p>Prochaine tentative : {new Date(listing.schedule.nextDueAt * 1000).toLocaleString()}</p>{/if}
        <p>Dernier résultat : {outcomeLabels[listing.schedule.lastOutcome] ?? listing.schedule.lastOutcome}</p>
        {#if listing.schedule.lastCompletedAt}<p>Dernière fin : {new Date(listing.schedule.lastCompletedAt * 1000).toLocaleString()}</p>{/if}
      </article>
      <article><h3>Archives</h3><p>Les archives sont conservées dans le dossier du serveur. Téléchargez une copie et conservez-la en lieu sûr.</p>
      {#if listing.archives.length === 0}<p>Aucune sauvegarde disponible.</p>{:else}<ul class="archives">{#each listing.archives as archive (archive.id)}<li><div><strong>{new Date(archive.createdAt * 1000).toLocaleString()}</strong><small>{(archive.size / 1024 / 1024).toFixed(2)} Mo · {archive.id}</small></div><!-- eslint-disable-next-line svelte/no-navigation-without-resolve -- Core API archive endpoint, not a SvelteKit page -->
<a href={`/api/v1/admin/backups/${archive.id}/download`} download>Télécharger</a></li>{/each}</ul>{/if}</article>
    {/if}
  {/if}
</section>

<style>
  .maintenance { display: grid; gap: 20px; min-width: 0; color: var(--tx); font-family: var(--font); }

  /* En-tête */
  header { min-width: 0; }
  header p, article p { max-width: 80ch; margin: 0 0 8px; font-size: 14px; line-height: 1.6; color: var(--mu); }
  .eyebrow { margin: 0 0 6px; font-size: 12px; font-weight: 700; letter-spacing: 0.07em; text-transform: uppercase; color: var(--mu); }
  h2 { margin: 0 0 8px; font-size: 24px; font-weight: 800; letter-spacing: -0.02em; color: var(--tx); }
  h3 { margin: 0 0 10px; font-size: 17px; font-weight: 700; color: var(--tx); }

  /* Actions */
  .actions { display: flex; flex-wrap: wrap; gap: 8px; }
  button, a {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    gap: 8px;
    min-height: 44px;
    padding: 0 16px;
    border: 0;
    border-radius: var(--r-md);
    background: var(--pn-2);
    color: var(--tx);
    font-family: var(--font);
    font-size: 14px;
    font-weight: 700;
    text-decoration: none;
    white-space: nowrap;
    cursor: pointer;
    transition: background-color var(--t-fast) ease, opacity var(--t-fast) ease, transform var(--t-fast) var(--ease);
  }
  button:hover:not(:disabled), a:hover { background: var(--pn-3); }
  button:active:not(:disabled), a:active { transform: scale(0.98); }
  button:focus-visible, a:focus-visible { outline: 2px solid var(--ac); outline-offset: 3px; }
  button:disabled { opacity: 0.45; cursor: not-allowed; }
  .primary { height: 48px; padding: 0 22px; background: var(--btn); color: var(--btn-tx); font-size: 15px; box-shadow: none; }
  .primary:hover:not(:disabled) { background: var(--btn); opacity: 0.9; }

  /* Retours et avis */
  [role='status'] { margin: 0; padding: 14px 16px; border-radius: var(--r-md); background: var(--pn); color: var(--tx); font-size: 14px; line-height: 1.5; }
  .error { margin: 0; padding: 14px 16px; border-radius: var(--r-md); background: var(--err-soft); color: var(--tx); font-size: 14px; line-height: 1.5; }
  .notice { min-width: 0; padding: 14px 16px; border-radius: var(--r-md); background: var(--info-soft); color: var(--tx); font-size: 14px; }
  .notice strong { font-weight: 700; color: var(--tx); }
  .notice p { margin: 4px 0 0; color: var(--tx); line-height: 1.55; }
  .notice .advice { margin-top: 8px; font-size: 13px; color: var(--mu); }

  /* Panneaux */
  article { min-width: 0; padding: 24px; background: var(--bg-2); border: 1px solid var(--line); border-radius: var(--r-lg); }

  /* Planification */
  form { display: grid; justify-items: start; gap: 16px; margin: 4px 0 16px; }
  form label { color: var(--tx); font-size: 14px; font-weight: 700; }
  .check-row { display: flex; align-items: center; gap: 12px; min-height: 44px; font-weight: 600; }
  .check-row input { width: 20px; height: 20px; margin: 0; accent-color: var(--ac); }
  .interval-field { display: grid; gap: 8px; }
  input[type='number'] {
    box-sizing: border-box;
    width: 100%;
    max-width: 200px;
    height: 46px;
    padding: 0 14px;
    border: 1px solid transparent;
    border-radius: var(--r-sm);
    background-color: var(--pn);
    color: var(--tx);
    font-family: var(--font);
    font-size: 14px;
    font-variant-numeric: tabular-nums;
    box-shadow: none;
    transition: border-color var(--t-fast) ease, background-color var(--t-fast) ease;
  }
  input[type='number']:hover { background-color: var(--pn-2); }
  input[type='number']:focus { outline: none; border-color: var(--ac); background-color: var(--bg-3); }
  input:focus-visible { outline: 2px solid var(--ac); outline-offset: 3px; }

  /* Archives : lignes séparées comme un tableau */
  .archives { margin: 12px 0 0; padding: 0; list-style: none; border-top: 1px solid var(--line); }
  li { display: flex; align-items: center; justify-content: space-between; gap: 16px; padding: 12px 8px; border-bottom: 1px solid var(--line); transition: background-color var(--t-fast) ease; }
  li:last-child { border-bottom: 0; }
  li:hover { background: var(--pn); }
  li > div { min-width: 0; }
  li strong { font-size: 14px; font-weight: 700; font-variant-numeric: tabular-nums; color: var(--tx); }
  small { display: block; margin-top: 4px; font-size: 13px; color: var(--mu); overflow-wrap: anywhere; }
  li small { font-family: var(--font-mono); font-size: 12px; }

  /* Mémoire */
  .memory-card { display: grid; gap: 20px; }
  .memory-header { display: flex; align-items: flex-start; justify-content: space-between; gap: 16px; flex-wrap: wrap; }
  .memory-header > div { min-width: 0; flex: 1 1 320px; }
  .memory-header h3 { margin: 0 0 4px; }
  .memory-header p { margin: 0; }
  .memory-grid { display: grid; grid-template-columns: repeat(auto-fit, minmax(160px, 1fr)); gap: 12px; }
  .metric { display: grid; gap: 6px; padding: 16px; border-radius: var(--r-md); background: var(--pn); }
  .metric.highlight { background: var(--ok-soft); }
  .metric small { margin: 0; font-size: 12px; font-weight: 700; letter-spacing: 0.06em; text-transform: uppercase; color: var(--mu); }
  .metric strong { font-size: 24px; font-weight: 800; letter-spacing: -0.02em; font-variant-numeric: tabular-nums; color: var(--tx); }
  .metric span { font-size: 13px; color: var(--mu); }

  @media (max-width: 640px) {
    article { padding: 16px; }
    li { flex-direction: column; align-items: stretch; gap: 10px; padding: 14px 4px; }
    li a { width: 100%; }
    .actions button { flex: 1 1 auto; }
    .memory-header button { width: 100%; }
    .memory-grid { grid-template-columns: minmax(0, 1fr) minmax(0, 1fr); }
    input[type='number'] { max-width: none; font-size: 16px; }
  }

  @media (prefers-reduced-motion: reduce) {
    button, a, li, input { transition: none; }
  }
</style>
