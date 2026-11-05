<script lang="ts">
  import { onMount } from 'svelte';
  import { resolve } from '$app/paths';
  import { session } from '../auth/session';
  import { applicationRequest, dateLabel, errorMessage, features, kindLabels, loadFeatures, parseSelection, requestPayload, statusLabels, type MediaRequest, type RequestKind, type SearchResult } from './api';
  import './applications.css';
  import { t } from '../i18n';
  let { embedded = false }: { embedded?: boolean } = $props();
  let loading = $state(true);
  let busy = $state(false);
  let error = $state('');
  let message = $state('');
  let requests = $state<MediaRequest[]>([]);
  let nextCursor = $state<string | null>(null);
  let results = $state<SearchResult[]>([]);
  let searchErrors = $state<string[]>([]);
  let query = $state('');
  let kind = $state<RequestKind>('film');
  let searched = $state(false);
  let selected = $state<SearchResult | null>(null);
  let detail = $state<MediaRequest | null>(null);
  let actionDestination = $state('');
  let seasons = $state('');
  let episodes = $state('');
  let monitorFuture = $state(false);
  let filter = $state('all');
  let serverId = $state('');
  let destinations = $state<{ id: string; name: string; kind: string }[]>([]);
  const isAdmin = $derived($session?.user.role === 'administrator');
  const visible = $derived(requests.filter(item => filter === 'all' || item.status === filter));
  async function refresh(append = false) {
    const data = await applicationRequest<{ requests: MediaRequest[]; nextCursor?: string | null }>(`/api/v1/requests${append && nextCursor ? `?before=${encodeURIComponent(nextCursor)}` : ''}`);
    requests = append ? [...requests, ...data.requests.filter(item => !requests.some(existing => existing.id === item.id))] : data.requests;
    nextCursor = data.nextCursor ?? null;
  }
  onMount(() => {
    void (async () => { try { const available = await loadFeatures(true); if (available.requests) { await refresh(); destinations = (await applicationRequest<{ servers: typeof destinations }>('/api/v1/requests/destinations')).servers; } } catch (cause) { error = String(cause instanceof Error ? cause.message : cause); } finally { loading = false; } })();
  });
  async function search(event: SubmitEvent) {
    event.preventDefault(); if (busy || query.trim().length < 2) return; busy = true; error = ''; message = ''; selected = null; results = []; searchErrors = []; searched = false;
    try {
      const data = await applicationRequest<{ results: SearchResult[]; errors?: { message?: string; error?: string; serverId?: string }[] }>(`/api/v1/requests/search?${new URLSearchParams({ q: query.trim(), kind, ...(serverId ? { serverId } : {}) })}`);
      results = data.results; searched = true;
      searchErrors = (data.errors ?? []).map(item => item.message ?? item.error ?? $t('request_search_error'));
    } catch (cause) { error = cause instanceof Error ? cause.message : $t('loading_impossible'); }
    finally { busy = false; }
  }
  function choose(result: SearchResult) { selected = result; seasons = ''; episodes = ''; monitorFuture = false; message = ''; }
  async function submit(event: SubmitEvent) {
    event.preventDefault(); if (!selected) return; busy = true; error = ''; message = '';
    try {
      const selection = selected.kind === 'series' ? parseSelection(seasons, episodes) : { seasons: [], episodes: [] };
      const created = await applicationRequest<MediaRequest>('/api/v1/requests', { method: 'POST', body: JSON.stringify(requestPayload(selected, { ...selection, monitorFuture })) });
      selected = null; message = `${$t('request_saved')}${created.status ? ` · ${statusLabels[created.status] ?? created.status}` : ''}.`;
      await refresh();
    } catch (cause) { error = cause instanceof Error ? cause.message : $t('request_impossible'); }
    finally { busy = false; }
  }
  async function action(id: string, value: string, destination?: string) {
    busy = true; error = ''; message = '';
    try { await applicationRequest(`/api/v1/requests/${encodeURIComponent(id)}/action`, { method: 'POST', body: JSON.stringify({ action: value, ...(destination ? { serverId: destination } : {}) }) }); await refresh(); if (detail?.id === id) detail = await applicationRequest<MediaRequest>(`/api/v1/requests/${encodeURIComponent(id)}`); }
    catch (cause) { error = cause instanceof Error ? cause.message : $t('action_impossible'); }
    finally { busy = false; }
  }
  async function show(id: string) {
    busy = true; error = '';
    try { detail = detail?.id === id ? null : await applicationRequest<MediaRequest>(`/api/v1/requests/${encodeURIComponent(id)}`); actionDestination = detail?.serverId ?? ''; }
    catch (cause) { error = cause instanceof Error ? cause.message : $t('detail_unavailable'); }
    finally { busy = false; }
  }
</script>
<div class="application-page">
  <header class="heading"><div><p class="muted">{$t('requests_subtitle')}</p>{#if !embedded}<h1>{$t('settings_requests')}</h1>{/if}<p class="muted">{$t('requests_hint')}</p></div>{#if isAdmin && $features.requests}<a class="button-link" href={embedded ? '#acquisition-section' : resolve('requests/admin/')}>{$t('manage_requests')}</a>{/if}</header>
  {#if error}<p role="alert" class="feedback error">{error}</p>{/if}
  {#if message}<p role="status" class="feedback success">{message}</p>{/if}
  {#if loading}<p class="empty" role="status">Chargement…</p>
  {:else if !$features.requests}<p class="panel">{$t('reports_unavailable')}</p>
  {:else}
    <section class="panel stack" aria-labelledby="request-search-title"><h2 id="request-search-title">{$t('request_search')}</h2>
      <form class="stack" onsubmit={search}><div class="fields"><label>{$t('request_type')}<select bind:value={kind} disabled={busy} onchange={() => { serverId = ''; selected = null; results = []; searched = false; }}>{#each Object.entries(kindLabels) as [value, label] (value)}<option {value}>{label}</option>{/each}</select></label><label>{$t('your_search')}<input type="search" required minlength="2" maxlength="200" bind:value={query} disabled={busy} placeholder={$t('search_placeholder')} /></label><label>{$t('destination')}<select bind:value={serverId} disabled={busy} onchange={() => { selected = null; results = []; searched = false; }}><option value="">{$t('default_destination')}</option>{#each destinations.filter(server => server.kind === (kind === 'film' ? 'radarr' : kind === 'series' ? 'sonarr' : 'lidarr')) as server (server.id)}<option value={server.id}>{server.name}</option>{/each}</select></label></div><div><button type="submit" class="primary" disabled={busy} aria-busy={busy}>{busy ? $t('searching') : $t('search_action')}</button></div></form>
      {#if kind === 'track'}<p class="muted">{$t('request_failed_lidarr')}</p>{/if}
      {#each searchErrors as failure, itemIndex (itemIndex)}<p class="feedback error">{failure}</p>{/each}
      {#if results.length}<ul class="records" aria-label={$t('results')}>{#each results as result, index (`${result.serverId ?? ''}:${result.externalId}:${index}`)}<li class="record"><div class="record-header"><div><h3>{result.title}{result.year ? ` (${result.year})` : ''}</h3><small>{kindLabels[result.kind] ?? result.kind}</small>{#if result.summary}<p>{result.summary}</p>{/if}</div><button disabled={busy} onclick={() => choose(result)}>{$t('request_action')}</button></div></li>{/each}</ul>{:else if searched}<p class="empty">{$t('no_request_results')}</p>{/if}
      {#if selected}<form class="panel stack" onsubmit={submit}><h3>{$t('confirm_request_for', { title: selected.title })}</h3>
        {#if selected.kind === 'series'}<div class="fields"><label>{$t('seasons_optional')}<input bind:value={seasons} maxlength="500" placeholder="1, 2" /></label><label>{$t('episodes_optional')}<input bind:value={episodes} maxlength="1000" placeholder="1:3, 1:4" /></label></div><p class="muted">{$t('current_seasons_note')}</p><label class="check"><input type="checkbox" bind:checked={monitorFuture} />{$t('monitor_future_episodes')}</label>{/if}
        {#if selected.kind === 'artist'}<label class="check"><input type="checkbox" bind:checked={monitorFuture} />{$t('monitor_artist_releases')}</label><p class="muted">{$t('artist_album_note')}</p>{/if}
        <div class="row"><button class="primary" disabled={busy || selected.kind === 'artist' && !monitorFuture}>{$t('confirm_request')}</button><button type="button" disabled={busy} onclick={() => selected = null}>{$t('cancel')}</button></div>
      </form>{/if}
    </section>
    <section aria-labelledby="request-list-title"><div class="heading"><h2 id="request-list-title">{isAdmin ? $t('all_requests') : $t('my_requests')}</h2><div class="row"><label>{$t('status')}<select bind:value={filter}><option value="all">{$t('all_statuses')}</option>{#each [...new Set(requests.map(item => item.status))] as status (status)}<option value={status}>{statusLabels[status] ?? status}</option>{/each}</select></label><button disabled={busy} onclick={() => { busy = true; void refresh().catch(cause => error = String(cause instanceof Error ? cause.message : cause)).finally(() => busy = false); }}>{$t('refresh')}</button></div></div>
      <ul class="records">{#each visible as item (item.id)}<li class="record"><div class="record-header"><div><h3>{item.title}</h3><small>{kindLabels[item.kind] ?? item.kind} · {dateLabel(item.createdAt)}</small></div><span class="badge">{statusLabels[item.status] ?? item.status}</span></div>{#if item.error || item.errorCode}<p class="error">{item.error ?? errorMessage(item.errorCode)}</p>{/if}<div class="row"><button disabled={busy} onclick={() => show(item.id)}>{detail?.id === item.id ? $t('hide') : $t('view_tracking')}</button>{#if isAdmin && item.status === 'pending'}<button class="primary" disabled={busy} onclick={() => action(item.id, 'approve')}>{$t('approve')}</button><button disabled={busy} onclick={() => action(item.id, 'reject')}>{$t('reject')}</button>{/if}{#if isAdmin && ['failed','suspended'].includes(item.status)}<button disabled={busy} onclick={() => action(item.id, 'retry')}>{item.status === 'suspended' ? $t('resume_request') : $t('request_retry')}</button>{/if}{#if ['pending', 'approved', 'queued'].includes(item.status)}<button disabled={busy} onclick={() => action(item.id, 'cancel')}>{$t('cancel_request')}</button>{/if}</div>
        {#if detail?.id === item.id}<div class="detail"><p class="muted">{$t('last_modified', { date: dateLabel(detail.updatedAt ?? detail.createdAt) })}</p>{#if isAdmin && ['pending','suspended','failed'].includes(detail.status)}<div class="row"><label>{$t('request_destination')}<select bind:value={actionDestination}><option value="">{$t('default_destination')}</option>{#each destinations.filter(server => server.kind === (detail?.kind === 'film' ? 'radarr' : detail?.kind === 'series' ? 'sonarr' : 'lidarr')) as server (server.id)}<option value={server.id}>{server.name}</option>{/each}</select></label><button disabled={busy} onclick={() => action(item.id, detail?.status === 'pending' ? 'approve' : 'retry', actionDestination)}>{detail.status === 'pending' ? $t('approve_destination') : $t('resume_destination')}</button></div>{/if}{#if detail.selection?.seasons?.length}<p>{$t('seasons')}: {detail.selection.seasons.join(', ')}</p>{/if}{#if detail.selection?.episodes?.length}<p>{$t('episodes')}: {detail.selection.episodes.map(episode => `${episode.season}:${episode.episode}`).join(', ')}</p>{/if}{#if detail.sourceStatus}<p>{$t('previous_app_status')}: {detail.sourceStatus}</p>{/if}{#each detail.events ?? [] as entry, itemIndex (itemIndex)}<p>{dateLabel(entry.createdAt)} · {statusLabels[entry.status ?? ''] ?? entry.action ?? entry.status}{entry.code ? ` · ${errorMessage(entry.code)}` : ''}</p>{/each}</div>{/if}
      </li>{:else}<li class="empty">{$t('no_requests_selection')}</li>{/each}</ul>
      {#if nextCursor}<button disabled={busy} onclick={() => { busy = true; void refresh(true).catch(cause => error = cause instanceof Error ? cause.message : $t('loading_impossible')).finally(() => busy = false); }}>{$t('load_previous_requests')}</button>{/if}
    </section>
  {/if}
</div>
