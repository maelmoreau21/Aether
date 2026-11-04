<script lang="ts">
  import { onMount } from 'svelte';
  import { resolve } from '$app/paths';
  import { session } from '../auth/session';
  import { applicationRequest, dateLabel, features, kindLabels, loadFeatures, parseSelection, statusLabels, type MediaRequest, type RequestKind, type SearchResult } from './api';
  import './applications.css';
  let loading = $state(true);
  let busy = $state(false);
  let error = $state('');
  let message = $state('');
  let requests = $state<MediaRequest[]>([]);
  let results = $state<SearchResult[]>([]);
  let searchErrors = $state<string[]>([]);
  let query = $state('');
  let kind = $state<RequestKind>('film');
  let searched = $state(false);
  let selected = $state<SearchResult | null>(null);
  let detail = $state<MediaRequest | null>(null);
  let seasons = $state('');
  let episodes = $state('');
  let monitorFuture = $state(false);
  let filter = $state('all');
  let serverId = $state('');
  let destinations = $state<{ id: string; name: string; kind: string }[]>([]);
  const isAdmin = $derived($session?.user.role === 'administrator');
  const visible = $derived(requests.filter(item => filter === 'all' || item.status === filter));
  async function refresh() {
    requests = (await applicationRequest<{ requests: MediaRequest[] }>('/api/v1/requests')).requests;
  }
  onMount(() => {
    void (async () => { try { const available = await loadFeatures(true); if (available.requests) { await refresh(); destinations = (await applicationRequest<{ servers: typeof destinations }>('/api/v1/requests/destinations')).servers; } } catch (cause) { error = String(cause instanceof Error ? cause.message : cause); } finally { loading = false; } })();
  });
  async function search(event: SubmitEvent) {
    event.preventDefault(); busy = true; error = ''; message = ''; selected = null; results = []; searchErrors = []; searched = false;
    try {
      const data = await applicationRequest<{ results: SearchResult[]; errors?: { message?: string; error?: string; serverId?: string }[] }>(`/api/v1/requests/search?${new URLSearchParams({ q: query.trim(), kind, ...(serverId ? { serverId } : {}) })}`);
      results = data.results; searched = true;
      searchErrors = (data.errors ?? []).map(item => item.message ?? item.error ?? 'Une source de recherche est indisponible.');
    } catch (cause) { error = cause instanceof Error ? cause.message : 'Recherche impossible.'; }
    finally { busy = false; }
  }
  function choose(result: SearchResult) { selected = result; seasons = ''; episodes = ''; monitorFuture = false; message = ''; }
  async function submit(event: SubmitEvent) {
    event.preventDefault(); if (!selected) return; busy = true; error = ''; message = '';
    try {
      const selection = selected.kind === 'series' ? parseSelection(seasons, episodes) : { seasons: [], episodes: [] };
      const created = await applicationRequest<MediaRequest>('/api/v1/requests', { method: 'POST', body: JSON.stringify({ ...selected, ...selection, monitorFuture }) });
      selected = null; message = `Demande enregistrée${created.status ? ` · ${statusLabels[created.status] ?? created.status}` : ''}.`;
      await refresh();
    } catch (cause) { error = cause instanceof Error ? cause.message : 'Demande impossible.'; }
    finally { busy = false; }
  }
  async function action(id: string, value: string) {
    busy = true; error = ''; message = '';
    try { await applicationRequest(`/api/v1/requests/${encodeURIComponent(id)}/action`, { method: 'POST', body: JSON.stringify({ action: value }) }); await refresh(); if (detail?.id === id) detail = await applicationRequest<MediaRequest>(`/api/v1/requests/${encodeURIComponent(id)}`); }
    catch (cause) { error = cause instanceof Error ? cause.message : 'Action impossible.'; }
    finally { busy = false; }
  }
  async function show(id: string) {
    busy = true; error = '';
    try { detail = detail?.id === id ? null : await applicationRequest<MediaRequest>(`/api/v1/requests/${encodeURIComponent(id)}`); }
    catch (cause) { error = cause instanceof Error ? cause.message : 'Détail indisponible.'; }
    finally { busy = false; }
  }
</script>
<div class="application-page">
  <header class="heading"><div><p class="muted">Votre médiathèque, à la demande</p><h1>Demandes</h1><p class="muted">Films, séries et musique : recherchez un contenu et suivez son arrivée.</p></div>{#if isAdmin && $features.requests}<a class="button-link" href={resolve('requests/admin/')}>Gérer les demandes</a>{/if}</header>
  {#if error}<p role="alert" class="feedback error">{error}</p>{/if}
  {#if message}<p role="status" class="feedback success">{message}</p>{/if}
  {#if loading}<p class="empty" role="status">Chargement…</p>
  {:else if !$features.requests}<p class="panel">L’extension officielle Demandes doit être installée et activée par un administrateur.</p>
  {:else}
    <section class="panel stack" aria-labelledby="request-search-title"><h2 id="request-search-title">Trouver un contenu</h2>
      <form class="stack" onsubmit={search}><div class="fields"><label>Type<select bind:value={kind} onchange={() => { serverId = ''; selected = null; results = []; searched = false; }}>{#each Object.entries(kindLabels) as [value, label] (value)}<option {value}>{label}</option>{/each}</select></label><label>Votre recherche<input type="search" required minlength="2" maxlength="200" bind:value={query} placeholder="Titre, artiste…" /></label><label>Destination<select bind:value={serverId}><option value="">Destination par défaut</option>{#each destinations.filter(server => server.kind === (kind === 'film' ? 'radarr' : kind === 'series' ? 'sonarr' : 'lidarr')) as server (server.id)}<option value={server.id}>{server.name}</option>{/each}</select></label></div><div><button class="primary" disabled={busy || query.trim().length < 2}>{busy ? 'Recherche…' : 'Rechercher'}</button></div></form>
      {#if kind === 'track'}<p class="muted">Si le titre ne peut pas être acquis dans Lidarr, la demande sera signalée comme échouée à l’administrateur. Aucun album ne sera acquis à sa place.</p>{/if}
      {#each searchErrors as failure, itemIndex (itemIndex)}<p class="feedback error">{failure}</p>{/each}
      {#if results.length}<ul class="records" aria-label="Résultats de recherche">{#each results as result, index (`${result.serverId ?? ''}:${result.externalId}:${index}`)}<li class="record"><div class="record-header"><div><h3>{result.title}{result.year ? ` (${result.year})` : ''}</h3><small>{kindLabels[result.kind] ?? result.kind}</small>{#if result.summary}<p>{result.summary}</p>{/if}</div><button disabled={busy} onclick={() => choose(result)}>Demander</button></div></li>{/each}</ul>{:else if searched}<p class="empty">Aucun résultat pour cette recherche.</p>{/if}
      {#if selected}<form class="panel stack" onsubmit={submit}><h3>Demander « {selected.title} »</h3>
        {#if selected.kind === 'series'}<div class="fields"><label>Saisons (facultatif)<input bind:value={seasons} maxlength="500" placeholder="1, 2" /></label><label>Épisodes (facultatif)<input bind:value={episodes} maxlength="1000" placeholder="1:3, 1:4" /></label></div><p class="muted">Sans sélection, la demande porte sur les saisons actuelles. Les épisodes se notent saison:épisode.</p><label class="check"><input type="checkbox" bind:checked={monitorFuture} />Surveiller également les épisodes à venir</label>{/if}
        {#if selected.kind === 'artist'}<label class="check"><input type="checkbox" bind:checked={monitorFuture} />Suivre les prochaines sorties de cet artiste</label><p class="muted">Pour un album précis, utilisez la recherche Album. Cette demande n’active pas le téléchargement de toute la discographie.</p>{/if}
        <div class="row"><button class="primary" disabled={busy || selected.kind === 'artist' && !monitorFuture}>Confirmer la demande</button><button type="button" disabled={busy} onclick={() => selected = null}>Annuler</button></div>
      </form>{/if}
    </section>
    <section aria-labelledby="request-list-title"><div class="heading"><h2 id="request-list-title">{isAdmin ? 'Toutes les demandes' : 'Mes demandes'}</h2><div class="row"><label>État<select bind:value={filter}><option value="all">Tous les états</option>{#each [...new Set(requests.map(item => item.status))] as status (status)}<option value={status}>{statusLabels[status] ?? status}</option>{/each}</select></label><button disabled={busy} onclick={() => { busy = true; void refresh().catch(cause => error = String(cause instanceof Error ? cause.message : cause)).finally(() => busy = false); }}>Actualiser</button></div></div>
      <ul class="records">{#each visible as item (item.id)}<li class="record"><div class="record-header"><div><h3>{item.title}</h3><small>{kindLabels[item.kind] ?? item.kind} · {dateLabel(item.createdAt)}</small></div><span class="badge">{statusLabels[item.status] ?? item.status}</span></div>{#if item.error || item.errorCode}<p class="error">{item.error ?? item.errorCode}</p>{/if}<div class="row"><button disabled={busy} onclick={() => show(item.id)}>{detail?.id === item.id ? 'Masquer' : 'Voir le suivi'}</button>{#if isAdmin && item.status === 'pending'}<button class="primary" disabled={busy} onclick={() => action(item.id, 'approve')}>Approuver</button><button disabled={busy} onclick={() => action(item.id, 'reject')}>Refuser</button>{/if}{#if isAdmin && item.status === 'failed'}<button disabled={busy} onclick={() => action(item.id, 'retry')}>Réessayer</button>{/if}{#if ['pending', 'approved', 'queued'].includes(item.status)}<button disabled={busy} onclick={() => action(item.id, 'cancel')}>Annuler la demande</button>{/if}</div>
        {#if detail?.id === item.id}<div class="detail"><p class="muted">Dernière modification : {dateLabel(detail.updatedAt ?? detail.createdAt)}</p>{#if detail.seasons?.length}<p>Saisons : {detail.seasons.join(', ')}</p>{/if}{#if detail.episodes?.length}<p>Épisodes : {detail.episodes.map(episode => `${episode.season}:${episode.episode}`).join(', ')}</p>{/if}{#each detail.history ?? [] as entry, itemIndex (itemIndex)}<p>{dateLabel(entry.createdAt)} · {statusLabels[entry.status ?? ''] ?? entry.action ?? entry.status}{entry.message ? ` · ${entry.message}` : ''}</p>{/each}</div>{/if}
      </li>{:else}<li class="empty">Aucune demande dans cette sélection.</li>{/each}</ul>
    </section>
  {/if}
</div>
