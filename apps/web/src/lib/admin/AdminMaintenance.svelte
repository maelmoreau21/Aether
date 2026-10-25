<script lang="ts">
  import { onMount } from 'svelte';
  import { session } from '../auth/session';
  let { visible = true }: { visible?: boolean } = $props();
  interface Archive { id: string; size: number; createdAt: number }
  interface Schedule { enabled: boolean; intervalMinutes: number; nextDueAt: number; lastStartedAt: number | null; lastCompletedAt: number | null; lastOutcome: string }
  interface Listing { available: boolean; reason: string | null; running: boolean; archives: Archive[]; schedule: Schedule }
  let listing = $state<Listing | null>(null);
  let loading = $state(true);
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
  }
  async function create() {
    if (!$session || creating) return;
    creating = true;
    error = '';
    success = '';
    try {
      const response = await fetch('/api/v1/admin/backups', { method: 'POST', credentials: 'same-origin', cache: 'no-store', headers: { 'x-csrf-token': $session.csrfToken }, signal: AbortSignal.timeout(300000) });
      if (!response.ok) throw new Error(response.status === 409 ? 'Une sauvegarde est déjà en cours ou le dossier serveur est refusé. Actualisez son état.' : `Sauvegarde impossible (HTTP ${response.status}).`);
      if (active) { success = 'Sauvegarde complète créée et chiffrée.'; await refresh(); }
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
  onMount(() => { active = true; return () => { active = false; }; });
  $effect(() => { if (visible) void refresh(); });
</script>

<section id="backups-section" class="maintenance" aria-labelledby="maintenance-heading">
  <header><p class="eyebrow">MAINTENANCE</p><h2 id="maintenance-heading">Sauvegardes et base de données</h2><p>Sauvegardez la base complète et la configuration du serveur dans une archive chiffrée.</p></header>
  <div class="actions"><button onclick={create} disabled={loading || creating || !listing?.available || listing?.running}>{creating || listing?.running ? 'Sauvegarde en cours…' : 'Créer une sauvegarde'}</button><button onclick={refresh} disabled={loading}>Actualiser</button></div>
  {#if error}<p class="error" role="alert">{error}</p>{/if}
  {#if success}<p role="status">{success}</p>{/if}
  {#if loading}<p role="status">Chargement des archives…</p>{:else if listing}
    {#if !listing.available}<div class="notice"><strong>Sauvegarde indisponible</strong><p>{listing.reason ?? "Le service de sauvegarde n'est pas disponible actuellement."}</p></div>{:else}
      <article><h3>Planification automatique</h3>
        <form onsubmit={saveSchedule}>
          <label><input type="checkbox" bind:checked={scheduleEnabled} disabled={saving} /> Activer les sauvegardes automatiques</label>
          <label>Intervalle en minutes <input type="number" min="60" max="10080" step="1" required bind:value={intervalMinutes} disabled={saving} /></label>
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
  .maintenance { display: grid; gap: 1.5rem; }
  header p, article p { color: var(--text-secondary); line-height: 1.65; }
  h2, h3 { color: var(--text-primary); margin: 0 0 .75rem; }
  h2 { font-size: 1.5rem; }
  .eyebrow { font-size: .7rem; letter-spacing: .15em; font-weight: 700; }
  article, .notice { background: var(--bg-surface); border: 1px solid var(--border-subtle); border-radius: 16px; padding: 1.5rem; min-width: 0; }
  .notice { border-inline-start: 3px solid var(--accent-primary); }
  .notice p { color: var(--text-secondary); }
  form { display: grid; gap: 1rem; }
  form label { display: flex; align-items: center; flex-wrap: wrap; gap: .6rem; color: var(--text-primary); }
  input[type="number"] { max-width: 10rem; padding: .6rem; background: var(--bg-main); color: var(--text-primary); border: 1px solid var(--border-subtle); border-radius:var(--radius-md); }
  .actions { display: flex; flex-wrap: wrap; gap: .75rem; }
  button, a { border: 1px solid var(--border-subtle); border-radius:var(--radius-md); padding: .7rem 1rem; background: var(--bg-elevated); color: var(--text-primary); font: inherit; }
  button { cursor: pointer; } button:disabled { cursor: default; opacity: .5; }
  .archives { padding: 0; margin: 0; list-style: none; }
  li { display: flex; align-items: center; justify-content: space-between; gap: 1rem; padding: 1rem 0; border-bottom: 1px solid var(--border-subtle); }
  small { display: block; margin-top: .35rem; color: var(--text-secondary); overflow-wrap: anywhere; }
  .error { color: var(--status-error); }
  @media (max-width: 650px) { li { align-items: start; flex-direction: column; } article, .notice { padding: 1.1rem; } }
</style>




