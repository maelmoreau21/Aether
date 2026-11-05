<script lang="ts">
  import { onMount, untrack } from 'svelte';
  import { session } from '../auth/session';
  import { invalidateCatalog } from '../catalog/page-cache';
  let { libraryId }: { libraryId: string } = $props();
  type Job = { id: string; status: string; processed: number; downloaded: number; duplicates: number; skipped: number; errors: number; lastError: string; cursorWork: string };
  let job = $state<Job | null>(null), error = $state(''), busy = $state(false);
  let languages = $state('fr, en'), includeNeutral = $state(true), maxImages = $state(12), maxWorks = $state(1000);
  const active = $derived(job && ['queued', 'running', 'paused'].includes(job.status));
  let alive = false, generation = 0;
  async function refresh() {
    const requestedLibrary = libraryId, request = generation;
    if (!requestedLibrary) { job = null; return; }
    try {
      const response = await fetch(`/api/v1/admin/artwork/jobs?libraryId=${encodeURIComponent(requestedLibrary)}`, { cache: 'no-store', signal: AbortSignal.timeout(8000) });
      if (!response.ok) throw new Error('Progression indisponible.');
      const jobs: Job[] = await response.json();
      if (alive && requestedLibrary === libraryId && request === generation) {
        const previous = job?.downloaded;
        job = jobs.find(value => ['queued', 'running', 'paused'].includes(value.status)) ?? jobs[0] ?? null;
        if (job?.downloaded !== previous) invalidateCatalog(requestedLibrary);
      }
    } catch (cause) { if (alive && request === generation) error = cause instanceof Error ? cause.message : 'Erreur'; }
  }
  $effect(() => { void libraryId; untrack(() => { generation++; job = null; if (alive) void refresh(); }); });
  onMount(() => { alive = true; void refresh(); const timer = setInterval(() => void refresh(), 5000); return () => { alive = false; clearInterval(timer); }; });
  async function submit(action?: 'pause' | 'resume' | 'cancel') {
    if (busy || !libraryId) return;
    busy = true; error = '';
    try {
      const response = await fetch(action ? `/api/v1/admin/artwork/jobs/${encodeURIComponent(job?.id ?? '')}` : '/api/v1/admin/artwork/jobs', {
        method: action ? 'PUT' : 'POST', headers: { 'content-type': 'application/json', 'x-csrf-token': $session?.csrfToken ?? '' },
        body: JSON.stringify(action ? { action } : { libraryId, after: job?.status === 'completed' ? job.cursorWork : undefined, languages: [...new Set(languages.split(',').map(v => v.trim()).filter(Boolean))], includeNeutral, maxImages, maxWorks }),
        signal: AbortSignal.timeout(10000)
      });
      if (!response.ok) throw new Error(`Collecte non enregistrée (HTTP ${response.status}).`);
      await refresh();
    } catch (cause) { error = cause instanceof Error ? cause.message : 'Erreur'; }
    finally { busy = false; }
  }
</script>
<section aria-label="Collecte durable des images">
  <h3>Collecter les images</h3>
  <p>La collecte continue après la fermeture de cette page et reprend après un redémarrage. La langue originale connue est incluse. Une image reste fixe uniquement si elle est épinglée.</p>
  <form onsubmit={event => { event.preventDefault(); void submit(); }}>
    <fieldset disabled={busy || !!active}>
      <label>Langues recherchées<input bind:value={languages} maxlength="100" required placeholder="fr, en" /></label>
      <label>Maximum d’images par œuvre<input type="number" min="1" max="12" bind:value={maxImages} required /></label>
      <label>Budget d’œuvres par collecte<input type="number" min="1" max="100000" bind:value={maxWorks} required /></label>
      <label><input type="checkbox" bind:checked={includeNeutral} /> Inclure les images sans langue</label>
    </fieldset>
    {#if !active}<button type="submit" disabled={busy || !libraryId}>{job?.status === 'completed' ? 'Continuer la collecte' : 'Lancer la collecte'}</button>{/if}
  </form>
  {#if job}
    <p role="status">{({ queued: 'En attente', running: 'En cours', paused: 'En pause', completed: 'Collecte terminée', cancelled: 'Annulée' })[job.status] ?? job.status} · {job.processed} œuvres examinées · {job.downloaded} images ajoutées · {job.duplicates} similaires écartées · {job.skipped} ignorées · {job.errors} erreurs</p>
    {#if active}<div class="actions">
      <button type="button" disabled={busy} onclick={() => void submit(job?.status === 'paused' ? 'resume' : 'pause')}>{job.status === 'paused' ? 'Reprendre la collecte' : 'Mettre en pause'}</button>
      <button type="button" disabled={busy} onclick={() => void submit('cancel')}>Annuler la collecte</button>
    </div>{/if}
    {#if job.lastError}<p>Dernier résultat : {job.lastError}</p>{/if}
  {/if}
  {#if error}<p role="alert">{error}</p>{/if}
</section>
<style>
  h3 { margin-top: 0; font-size: 17px; }
  p { color: var(--mu); font-size: 14px; line-height: 1.6; }
  fieldset { display: grid; gap: 12px; border: 0; padding: 0; margin: 16px 0; }
  label { display: grid; gap: 6px; }
  label:has(input[type='checkbox']) { display: flex; align-items: center; }
  input[type='checkbox'] { width: 18px; height: 18px; }
  .actions { display: flex; gap: 12px; flex-wrap: wrap; }
</style>
