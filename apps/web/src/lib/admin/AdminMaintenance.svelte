<script lang="ts">
  import { onMount } from 'svelte';
  import { session } from '../auth/session';
  import { t } from '../i18n';
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
        success = $t('memory_released');
      }
    } catch {
      error = $t('memory_release_failed');
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
      if (active) { success = $t('backup_created'); await refresh(); }
    } catch (cause) {
      if (active) error = cause instanceof Error && cause.name === 'TimeoutError' ? $t('timeout_backup') : cause instanceof Error ? cause.message : $t('backup_failed');
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
      if (active) { await refresh(); success = $t('schedule_saved'); }
    } catch (cause) {
      if (active) error = cause instanceof Error ? cause.message : $t('schedule_failed');
    } finally { if (active) saving = false; }
  }

  const mb = (bytes: number) => (bytes / 1024 / 1024).toFixed(1);

  onMount(() => { active = true; return () => { active = false; }; });
  $effect(() => { if (visible) void refresh(); });
</script>

<section id="backups-section" class="maintenance" aria-labelledby="maintenance-heading">
  <header><p class="eyebrow">{$t('maintenance')}</p><h2 id="maintenance-heading">{$t('backups_database')}</h2><p>{$t('backup_intro')}</p></header>
  <div class="actions"><button class="primary" onclick={create} disabled={loading || creating || !listing?.available || listing?.running}>{creating || listing?.running ? $t('backup_in_progress') : $t('create_backup')}</button><button onclick={refresh} disabled={loading}>{$t('refresh')}</button></div>
  {#if error}<p class="error" role="alert">{error}</p>{/if}
  {#if success}<p role="status">{success}</p>{/if}

  {#if memoryReport}
    <article class="memory-card">
      <div class="memory-header">
        <div>
          <h3>{$t('memory_cache')}</h3>
          <p>{$t('memory_cache_note')}</p>
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

    </article>
  {/if}

  {#if loading}<p role="status">{$t('loading_archives')}</p>{:else if listing}
    {#if !listing.available}<div class="notice"><strong>{$t('backup_unavailable')}</strong><p>{listing.reason ?? $t('backup_service_unavailable')}</p></div>{:else}
      <article><h3>{$t('automatic_schedule')}</h3>
        <form onsubmit={saveSchedule}>
          <label class="check-row"><input type="checkbox" bind:checked={scheduleEnabled} disabled={saving} /> Activer les sauvegardes automatiques</label>
          <label class="interval-field">{$t('interval_minutes')} <input type="number" min="60" max="10080" step="1" required bind:value={intervalMinutes} disabled={saving} /></label>
          <button type="submit" disabled={saving}>{saving ? 'Enregistrement…' : 'Enregistrer la planification'}</button>
        </form>
        <p>{$t('schedule_range_note')}</p>
        {#if listing.schedule.enabled}<p>Prochaine tentative : {new Date(listing.schedule.nextDueAt * 1000).toLocaleString()}</p>{/if}
        <p>Dernier résultat : {outcomeLabels[listing.schedule.lastOutcome] ?? listing.schedule.lastOutcome}</p>
        {#if listing.schedule.lastCompletedAt}<p>Dernière fin : {new Date(listing.schedule.lastCompletedAt * 1000).toLocaleString()}</p>{/if}
      </article>
      <article><h3>{$t('archives')}</h3><p>{$t('archives_note')}</p>
      {#if listing.archives.length === 0}<p>{$t('no_backup_available')}</p>{:else}<ul class="archives">{#each listing.archives as archive (archive.id)}<li><div><strong>{new Date(archive.createdAt * 1000).toLocaleString()}</strong><small>{(archive.size / 1024 / 1024).toFixed(2)} Mo · {archive.id}</small></div><!-- eslint-disable-next-line svelte/no-navigation-without-resolve -- Core API archive endpoint, not a SvelteKit page -->
    <a href={`/api/v1/admin/backups/${archive.id}/download`} download>{$t('download')}</a></li>{/each}</ul>{/if}</article>
    {/if}
  {/if}
</section>

<style>
  .maintenance { display: grid; gap: 20px; min-width: 0; max-width: 860px; font-family: var(--font-sans); color: var(--color-text); }
  header { display: grid; gap: 6px; min-width: 0; }
  header p, article p { max-width: 80ch; margin: 0; font-size: 13px; line-height: 1.65; color: var(--color-muted); }
  .eyebrow { font-size: 11px; font-weight: 600; letter-spacing: 0.12em; text-transform: uppercase; color: var(--color-accent); }
  h2, h3 { margin: 0; color: var(--color-text); }
  h2 { font-size: 26px; font-weight: 700; letter-spacing: -0.02em; }
  h3 { font-size: 16px; font-weight: 600; }

  /* Actions : un primaire, le reste en ghost */
  .actions { display: flex; flex-wrap: wrap; gap: 10px; }
  button, a { display: inline-flex; align-items: center; justify-content: center; gap: 8px; min-height: 40px; box-sizing: border-box; padding: 8px 18px; border: 1px solid var(--color-line); border-radius: var(--radius-sm); background: transparent; color: var(--color-text); font: inherit; font-size: 14px; font-weight: 500; text-decoration: none; white-space: nowrap; cursor: pointer; transition: background-color var(--transition-fast); }
  button:hover:not(:disabled), a:hover { background: var(--color-raised); }
  button:focus-visible, a:focus-visible { outline: 2px solid var(--color-accent); outline-offset: 2px; }
  button:disabled { opacity: 0.55; cursor: not-allowed; }
  .primary { border: 0; background: var(--color-play); color: var(--color-on-play); font-weight: 600; }
  .primary:hover:not(:disabled) { background: var(--color-play-hover); }

  /* Retours et avis */
  [role='status'] { margin: 0; font-size: 14px; line-height: 1.5; color: var(--color-muted); }
  .error { margin: 0; padding: 12px 16px; border-radius: var(--radius-sm); background: var(--color-danger-soft); color: var(--color-text); font-size: 14px; line-height: 1.5; }
  .notice { min-width: 0; padding: 14px 16px; border-radius: var(--radius-sm); background: var(--color-warning-soft); font-size: 14px; }
  .notice strong { font-weight: 600; color: var(--color-text); }
  .notice p { margin: 4px 0 0; font-size: 13px; line-height: 1.55; color: var(--color-muted); }

  /* Panneaux plats */
  article { display: grid; gap: 10px; min-width: 0; padding: 20px; border-radius: var(--radius-md); background: var(--color-surface); }

  /* Planification : lignes séparées */
  form { display: grid; justify-items: start; gap: 0; margin: 0; }
  form > * { width: 100%; padding: 12px 0; border-top: 1px solid var(--color-line-subtle); }
  form > :first-child { border-top: 0; padding-top: 4px; }
  form > button { width: fit-content; margin-top: 4px; border-top: 1px solid var(--color-line); }
  form label { color: var(--color-text); font-size: 14px; font-weight: 600; }
  .check-row { display: flex; align-items: center; gap: 12px; min-height: 44px; }
  .check-row input { width: 20px; height: 20px; margin: 0; accent-color: var(--color-accent); }
  .interval-field { display: flex; align-items: center; flex-wrap: wrap; justify-content: space-between; gap: 10px; }
  input[type='number'] { box-sizing: border-box; width: 100%; max-width: 160px; height: 42px; padding: 0 12px; border: 1px solid var(--color-line-subtle); border-radius: var(--radius-sm); background: var(--color-raised); color: var(--color-text); font: inherit; font-size: 14px; font-variant-numeric: tabular-nums; }
  input:focus-visible { outline: 2px solid var(--color-accent); outline-offset: 1px; }

  /* Archives : lignes fines */
  .archives { margin: 4px 0 0; padding: 0; list-style: none; }
  li { display: flex; align-items: center; justify-content: space-between; gap: 16px; padding: 12px 0; border-top: 1px solid var(--color-line-subtle); }
  li > div { min-width: 0; }
  li strong { font-size: 14px; font-weight: 600; font-variant-numeric: tabular-nums; color: var(--color-text); }
  small { display: block; margin-top: 4px; font-size: 12px; color: var(--color-muted); overflow-wrap: anywhere; }

  /* Mémoire */
  .memory-card { gap: 16px; }
  .memory-header { display: flex; align-items: flex-start; justify-content: space-between; flex-wrap: wrap; gap: 16px; }
  .memory-header > div { flex: 1 1 320px; min-width: 0; }
  .memory-header h3 { margin-bottom: 4px; }
  .memory-grid { display: grid; grid-template-columns: repeat(auto-fit, minmax(160px, 1fr)); gap: 12px; }
  .metric { display: grid; gap: 6px; padding: 14px 16px; border-radius: var(--radius-sm); background: var(--color-raised); }
  .metric.highlight { background: var(--color-success-soft); }
  .metric small { margin: 0; font-size: 11px; font-weight: 600; letter-spacing: 0.08em; text-transform: uppercase; }
  .metric strong { font-size: 22px; font-weight: 700; letter-spacing: -0.02em; font-variant-numeric: tabular-nums; color: var(--color-text); }
  .metric span { font-size: 12px; color: var(--color-muted); }

  @media (max-width: 640px) {
    article { padding: 16px; }
    li { flex-direction: column; align-items: stretch; gap: 10px; }
    li a, .memory-header button { width: 100%; }
    .actions button { flex: 1 1 auto; }
    .memory-grid { grid-template-columns: minmax(0, 1fr) minmax(0, 1fr); }
    input[type='number'] { max-width: none; font-size: 16px; }
  }
  @media (prefers-reduced-motion: reduce) { button, a { transition: none; } }
</style>
