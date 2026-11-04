<script lang="ts">
  import { onMount } from 'svelte';
  import { applicationRequest, dateLabel, features, type RemoteServer } from './api';
  interface ImportJob { id: string; sourceUrl: string; status: string; phase: string; processed: number; errors: number; updatedAt: number; lastError?: string; failures?: { phase: string; sourceId: string; error: string }[] }
  interface Preview { id: string; sourceUrl: string; users: { id: string; name: string }[]; servers: { id: string; name: string; kind: string }[]; counts: { requests: number; reports: number }; warnings: string[] }
  let { servers }: { servers: RemoteServer[] } = $props();
  let baseUrl = $state('');
  let apiKey = $state('');
  let preview = $state<Preview | null>(null);
  let userMappings = $state<Record<string, string>>({});
  let serverMappings = $state<Record<string, string>>({});
  let users = $state<{ id: string; login: string; displayName?: string }[]>([]);
  let jobs = $state<ImportJob[]>([]);
  let busy = $state(false);
  let error = $state('');
  let acknowledged = $state(false);
  const labels: Record<string, string> = { preview: 'Aperçu', ready: 'Prêt', pending: 'À démarrer', running: 'En cours', completed: 'Terminé', completed_with_errors: 'Terminé avec des erreurs', failed: 'Échoué', paused: 'En pause', requests: 'Demandes', reports: 'Signalements', comments: 'Commentaires', done: 'Terminé' };
  async function refresh() { jobs = (await applicationRequest<{ imports: ImportJob[] }>('/api/v1/admin/imports')).imports; }
  onMount(() => {
    void refresh().catch(cause => error = cause instanceof Error ? cause.message : 'Imports indisponibles.');
    const interval = setInterval(() => { if (jobs.some(job => job.status === 'running') && !busy) void refresh().catch(() => {}); }, 5000);
    return () => clearInterval(interval);
  });
  async function analyze(event: SubmitEvent) {
    event.preventDefault(); busy = true; error = ''; preview = null; acknowledged = false;
    try {
      const result = await applicationRequest<Preview>('/api/v1/admin/imports/preview', { method: 'POST', body: JSON.stringify({ baseUrl, apiKey }) });
      apiKey = ''; preview = result; userMappings = {}; serverMappings = {};
      users = (await applicationRequest<{ users: { id: string; login: string; displayName?: string }[] }>('/api/v1/admin/users')).users;
      await refresh();
    } catch (cause) { error = cause instanceof Error ? cause.message : 'Analyse impossible.'; }
    finally { busy = false; }
  }
  async function start() {
    if (!preview || !acknowledged) return; busy = true; error = '';
    try {
      await applicationRequest(`/api/v1/admin/imports/${encodeURIComponent(preview.id)}/start`, { method: 'POST', body: JSON.stringify({ userMappings: Object.fromEntries(Object.entries(userMappings).filter(([, id]) => id)), serverMappings: Object.fromEntries(Object.entries(serverMappings).filter(([, id]) => id)) }) });
      preview = null; await refresh();
    } catch (cause) { error = cause instanceof Error ? cause.message : 'Import impossible.'; }
    finally { busy = false; }
  }
  async function progress(id: string, action: 'run' | 'pause') {
    busy = true; error = '';
    try { await applicationRequest(`/api/v1/admin/imports/${encodeURIComponent(id)}/${action}`, { method: 'POST' }); await refresh(); }
    catch (cause) { error = cause instanceof Error ? cause.message : 'Action impossible.'; }
    finally { busy = false; }
  }
</script>
<section class="panel stack" aria-labelledby="import-title"><h2 id="import-title">Importer les dossiers existants</h2><p class="muted">Import en lecture seule des demandes, signalements et commentaires de votre application précédente. Les mots de passe et privilèges ne sont pas transférés. Aucune acquisition ni notification n’est déclenchée par l’import.</p>
  <form class="stack" onsubmit={analyze}><div class="fields"><label>Adresse de l’application précédente<input type="url" required bind:value={baseUrl} maxlength="2048" placeholder="http://serveur:5055" /></label><label>Clé API administrateur de la source<input type="password" required autocomplete="new-password" bind:value={apiKey} maxlength="512" /></label></div><div><button disabled={busy}>{busy ? 'Analyse…' : 'Analyser sans importer'}</button></div></form>
  {#if error}<p class="feedback error" role="alert">{error}</p>{/if}
  {#if preview}<div class="detail stack"><h3>Aperçu de {preview.sourceUrl}</h3><p>{preview.counts.requests} demande(s) · {preview.counts.reports} signalement(s)</p>{#each preview.warnings as warning, itemIndex (itemIndex)}<p class="feedback">{warning}</p>{/each}
    {#if !$features.reports && preview.counts.reports}<p class="feedback">Activez également Signalements pour reprendre les dossiers de problèmes.</p>{/if}
    <h3>Associer les utilisateurs</h3><p class="muted">Les auteurs sans correspondance seront conservés comme auteurs historiques, sans compte créé.</p><div class="fields">{#each preview.users as source (source.id)}<label>{source.name}<select bind:value={userMappings[source.id]}><option value="">Auteur historique</option>{#each users as user (user.id)}<option value={user.id}>{user.displayName || user.login}</option>{/each}</select></label>{/each}</div>
    {#if preview.servers.length}<h3>Associer les serveurs</h3><div class="fields">{#each preview.servers as source (source.id)}<label>{source.name} · {source.kind}<select bind:value={serverMappings[source.id]}><option value="">Sans correspondance</option>{#each servers.filter(server => server.kind === source.kind) as target (target.id)}<option value={target.id}>{target.name}</option>{/each}</select></label>{/each}</div>{/if}
    <label class="check"><input type="checkbox" bind:checked={acknowledged} />J’ai vérifié les correspondances. Les demandes importées resteront suspendues jusqu’à leur reprise explicite.</label><div><button class="primary" disabled={busy || !acknowledged || (!$features.requests && preview.counts.requests > 0) || (!$features.reports && preview.counts.reports > 0)} onclick={start}>Importer les dossiers</button></div>
  </div>{/if}
  <div class="heading"><h3>Historique des imports</h3><button disabled={busy} onclick={() => void refresh().catch(cause => error = cause instanceof Error ? cause.message : 'Actualisation impossible.')}>Actualiser</button></div><ul class="records">{#each jobs as job (job.id)}<li class="record"><div class="record-header"><div><h3>{job.sourceUrl}</h3><small>{dateLabel(job.updatedAt)} · {labels[job.phase] ?? job.phase}</small></div><span class="badge">{labels[job.status] ?? job.status}</span></div><p>{job.processed} élément(s) traité(s) · {job.errors} erreur(s)</p>{#if job.lastError}<p class="error">{job.lastError}</p>{/if}{#if job.failures?.length}<details><summary>Éléments à vérifier</summary>{#each job.failures as failure, index (index)}<p class="text">{labels[failure.phase] ?? failure.phase} · {failure.sourceId} : {failure.error}</p>{/each}</details>{/if}<div class="row">{#if job.status === 'running'}<button disabled={busy} onclick={() => progress(job.id, 'pause')}>Mettre en pause</button>{/if}{#if ['running','paused','failed'].includes(job.status)}<button disabled={busy} onclick={() => progress(job.id, 'run')}>Traiter le lot suivant</button>{/if}</div></li>{:else}<li class="muted">Aucun import enregistré.</li>{/each}</ul>
</section>
