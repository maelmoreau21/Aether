<script lang="ts">
  import { onMount, tick } from 'svelte';
  import { resolve } from '$app/paths';
  import { session } from '../auth/session';
  import {
    applicationRequest, dateLabel, errorMessage, features, groupCount, kindLabels, loadFeatures, parseSelection,
    rejectReasonLabels, rejectReasons, requestGroups, requestPayload, requestStep, requestSteps, seasonRanges,
    statusLabels, statusTone, workHref, type MediaRequest, type RequestGroup, type RequestKind, type RequestSelection,
    type SearchResult
  } from './api';
  import './applications.css';
  import { t } from '../i18n';

  let { embedded = false }: { embedded?: boolean } = $props();
  type Destination = { id: string; name: string; kind: string; isDefault?: boolean };

  let loading = $state(true);
  let busy = $state(false);
  let error = $state('');
  let message = $state('');
  let destinations = $state<Destination[]>([]);

  // Search
  let query = $state('');
  let kind = $state<RequestKind>('film');
  let serverId = $state('');
  let searching = $state(false);
  let searched = $state(false);
  let results = $state<SearchResult[]>([]);
  let searchErrors = $state<string[]>([]);

  // Request dialog
  let dialog = $state<HTMLDialogElement>();
  let selected = $state<SearchResult | null>(null);
  let seriesScope = $state<'all' | 'pick'>('all');
  let chosenSeasons = $state<number[]>([]);
  let seasonsText = $state('');
  let episodesText = $state('');
  let monitorFuture = $state(false);

  // List
  let requests = $state<MediaRequest[]>([]);
  let counts = $state<Record<string, number>>({});
  let nextCursor = $state<string | null>(null);
  let group = $state<RequestGroup>('all');
  let kindFilter = $state('');
  let mineOnly = $state(false);
  let detail = $state<MediaRequest | null>(null);
  let actionDestination = $state('');
  let picked = $state<string[]>([]);
  let rejecting = $state<{ ids: string[]; reason: string } | null>(null);
  let brokenArt = $state<string[]>([]);

  const isAdmin = $derived($session?.user.role === 'administrator');
  const kinds = Object.keys(kindLabels) as RequestKind[];
  const service = (value: string | undefined) => value === 'film' ? 'radarr' : value === 'series' ? 'sonarr' : 'lidarr';
  const destinationsFor = (value: string | undefined) => destinations.filter(server => server.kind === service(value));
  const pendingIds = $derived(requests.filter(item => item.status === 'pending').map(item => item.id));
  const allPicked = $derived(pendingIds.length > 0 && pendingIds.every(id => picked.includes(id)));
  const groupLabels: Record<RequestGroup, string> = $derived({
    all: $t('rq_group_all'), active: $t('rq_group_active'), pending: $t('awaiting_approval'),
    done: $t('rq_group_done'), attention: $t('rq_group_attention'), closed: $t('rq_group_closed')
  });
  const stepLabels: Record<(typeof requestSteps)[number], string> = $derived({
    pending: $t('rq_step_requested'), approved: $t('approved'), submitted: $t('sent'),
    acquiring: $t('being_acquired'), available: $t('rq_step_available')
  });

  function listPath(append: boolean) {
    const params = new URLSearchParams();
    if (group !== 'all') params.set('status', group);
    if (kindFilter) params.set('kind', kindFilter);
    if (isAdmin && mineOnly) params.set('mine', 'true');
    if (append && nextCursor) params.set('before', nextCursor);
    const suffix = params.toString();
    return `/api/v1/requests${suffix ? `?${suffix}` : ''}`;
  }
  async function refresh(append = false) {
    const data = await applicationRequest<{ requests: MediaRequest[]; nextCursor?: string | null; counts?: Record<string, number> }>(listPath(append));
    requests = append ? [...requests, ...data.requests.filter(item => !requests.some(existing => existing.id === item.id))] : data.requests;
    nextCursor = data.nextCursor ?? null;
    counts = data.counts ?? counts;
    picked = picked.filter(id => requests.some(item => item.id === id && item.status === 'pending'));
  }
  async function run(task: () => Promise<void>, fallback: string) {
    busy = true; error = '';
    try { await task(); }
    catch (cause) { error = cause instanceof Error ? cause.message : fallback; }
    finally { busy = false; }
  }
  onMount(() => {
    void (async () => {
      try {
        const available = await loadFeatures(true);
        if (available.requests) {
          await refresh();
          destinations = (await applicationRequest<{ servers: Destination[] }>('/api/v1/requests/destinations')).servers;
        }
      } catch (cause) { error = cause instanceof Error ? cause.message : $t('loading_impossible'); }
      finally { loading = false; }
    })();
  });

  function setKind(value: RequestKind) {
    if (kind === value) return;
    kind = value; serverId = ''; results = []; searched = false; searchErrors = [];
  }
  async function search(event: SubmitEvent) {
    event.preventDefault();
    if (searching || query.trim().length < 2) return;
    searching = true; error = ''; message = ''; results = []; searchErrors = []; searched = false;
    try {
      const params = new URLSearchParams({ q: query.trim(), kind, ...(serverId ? { serverId } : {}) });
      const data = await applicationRequest<{ results: SearchResult[]; errors?: { message?: string; error?: string }[] }>(`/api/v1/requests/search?${params}`);
      results = data.results; searched = true;
      searchErrors = (data.errors ?? []).map(item => item.message ?? (item.error ? errorMessage(item.error) : $t('request_search_error')));
    } catch (cause) { error = cause instanceof Error ? cause.message : $t('loading_impossible'); }
    finally { searching = false; }
  }

  /** A film already in a library, or anything already requested, is not offered again. */
  function blocked(result: SearchResult): string {
    if (result.kind === 'film' && result.inLibrary) return $t('rq_already_available');
    if (result.request && result.kind !== 'series') return $t('rq_already_requested_short');
    return '';
  }
  async function choose(result: SearchResult) {
    selected = result; seriesScope = 'all'; chosenSeasons = []; seasonsText = ''; episodesText = '';
    monitorFuture = result.kind === 'artist'; message = '';
    await tick();
    dialog?.showModal();
  }
  function closeDialog() { if (dialog?.open) dialog.close(); }
  function selection(): RequestSelection {
    if (selected?.kind !== 'series') return { monitorFuture: selected?.kind === 'artist' ? monitorFuture : false };
    if (seriesScope === 'all') return { seasons: [], episodes: [], monitorFuture };
    const typed = parseSelection(selected.seasons?.length ? '' : seasonsText, episodesText);
    const seasons = [...new Set([...chosenSeasons, ...typed.seasons])].sort((a, b) => a - b);
    return { seasons, episodes: typed.episodes.filter(item => !seasons.includes(item.season)), monitorFuture };
  }
  const summary = $derived.by(() => {
    if (!selected) return '';
    try { return scopeLabel(selected.kind, selection()); } catch { return ''; }
  });
  const canSubmit = $derived.by(() => {
    if (!selected || busy) return false;
    if (selected.kind === 'artist') return monitorFuture;
    if (selected.kind === 'series' && seriesScope === 'pick') return chosenSeasons.length > 0 || seasonsText.trim() !== '' || episodesText.trim() !== '';
    return true;
  });
  async function submit(event: SubmitEvent) {
    event.preventDefault();
    if (!selected || !canSubmit) return;
    const target = selected;
    await run(async () => {
      const created = await applicationRequest<MediaRequest>('/api/v1/requests', { method: 'POST', body: JSON.stringify(requestPayload(target, { ...selection(), serverId: serverId || undefined })) });
      closeDialog();
      message = $t('rq_request_created', { title: created.title ?? target.title, status: statusLabels[created.status] ?? created.status });
      results = results.map(item => item === target ? { ...item, request: { status: created.status, mine: true, id: created.id } } : item);
      await refresh();
    }, $t('request_impossible'));
  }

  function scopeLabel(value: string, scope: RequestSelection | undefined): string {
    if (value === 'series') {
      const parts: string[] = [];
      if (scope?.seasons?.length) parts.push($t(scope.seasons.length > 1 ? 'rq_scope_seasons' : 'rq_scope_season', { list: seasonRanges(scope.seasons) }));
      if (scope?.episodes?.length) parts.push($t('rq_scope_episodes', { count: scope.episodes.length }));
      if (!parts.length) parts.push($t('rq_scope_whole_series'));
      if (scope?.monitorFuture) parts.push($t('rq_scope_future_episodes'));
      return parts.join(' · ');
    }
    if (value === 'artist') {
      const parts: string[] = [];
      if (scope?.albumIds?.length) parts.push($t('rq_scope_albums', { count: scope.albumIds.length }));
      if (scope?.monitorFuture) parts.push($t('rq_scope_future_releases'));
      return parts.join(' · ');
    }
    return '';
  }

  async function setGroup(value: RequestGroup) {
    group = value; detail = null;
    await run(() => refresh(), $t('loading_impossible'));
  }
  async function act(id: string, value: string, extra: Record<string, string> = {}) {
    await applicationRequest(`/api/v1/requests/${encodeURIComponent(id)}/action`, { method: 'POST', body: JSON.stringify({ action: value, ...extra }) });
  }
  async function action(id: string, value: string, extra: Record<string, string> = {}) {
    message = '';
    await run(async () => {
      await act(id, value, extra);
      await refresh();
      if (detail?.id === id) detail = await applicationRequest<MediaRequest>(`/api/v1/requests/${encodeURIComponent(id)}`);
    }, $t('action_impossible'));
  }
  /** Bulk actions run one by one; a failure stops the batch and names what is left. */
  async function bulk(ids: string[], value: 'approve' | 'reject', extra: Record<string, string> = {}) {
    message = '';
    let done = 0;
    await run(async () => {
      try { for (const id of ids) { await act(id, value, extra); done++; } }
      finally {
        await refresh().catch(() => undefined);
        message = $t(value === 'approve' ? 'rq_bulk_approved' : 'rq_bulk_rejected', { count: done });
      }
    }, $t('action_impossible'));
    rejecting = null;
  }
  async function confirmReject() {
    if (!rejecting) return;
    const { ids, reason } = rejecting;
    if (ids.length === 1) { await action(ids[0], 'reject', { reason }); rejecting = null; }
    else await bulk(ids, 'reject', { reason });
  }
  function togglePick(id: string) { picked = picked.includes(id) ? picked.filter(other => other !== id) : [...picked, id]; }
  function toggleAll() { picked = allPicked ? [] : [...pendingIds]; }
  async function show(id: string) {
    if (detail?.id === id) { detail = null; return; }
    await run(async () => {
      detail = await applicationRequest<MediaRequest>(`/api/v1/requests/${encodeURIComponent(id)}`);
      actionDestination = detail.serverId ?? '';
    }, $t('detail_unavailable'));
  }
  function art(workId: string | null | undefined) {
    return workId && !brokenArt.includes(workId) ? `/api/v1/works/${encodeURIComponent(workId)}/poster?width=160` : '';
  }
  function initial(title: string) { return title.trim().charAt(0).toUpperCase() || '?'; }
  function metaLine(result: SearchResult): string {
    const parts = [kindLabels[result.kind] ?? result.kind];
    if (result.kind === 'track' || result.kind === 'album') { if (result.artist) parts.push(result.artist); if (result.album) parts.push(result.album); }
    if (result.genres?.length) parts.push(result.genres.join(', '));
    if (result.runtime) parts.push($t('rq_runtime', { minutes: result.runtime }));
    if (result.network) parts.push(result.network);
    const regular = result.seasons?.filter(season => season.number > 0).length ?? 0;
    if (regular) parts.push($t(regular > 1 ? 'rq_season_count' : 'rq_season_count_one', { count: regular }));
    return parts.join(' · ');
  }
</script>

<div class="application-page requests-page">
  <header class="heading">
    <div>
      <p class="muted eyebrow">{$t('requests_subtitle')}</p>
      {#if !embedded}<h1>{$t('settings_requests')}</h1>{/if}
      <p class="muted">{$t('requests_hint')}</p>
    </div>
    {#if isAdmin && $features.requests}<a class="button-link" href={embedded ? '#acquisition-section' : resolve('requests/admin/')}>{$t('manage_requests')}</a>{/if}
  </header>
  {#if error}<p role="alert" class="feedback error">{error}</p>{/if}
  {#if message}<p role="status" class="feedback success">{message}</p>{/if}

  {#if loading}<p class="empty" role="status">{$t('loading_settings_short')}</p>
  {:else if !$features.requests}<p class="panel">{$t('rq_unavailable')}</p>
  {:else}
    <section class="panel stack" aria-labelledby="request-search-title">
      <h2 id="request-search-title">{$t('request_search')}</h2>
      <div class="segmented" role="radiogroup" aria-label={$t('request_type')}>
        {#each kinds as value (value)}
          <label class:active={kind === value}><input type="radio" name="request-kind" {value} checked={kind === value} disabled={searching} onchange={() => setKind(value)} />{kindLabels[value]}</label>
        {/each}
      </div>
      <form class="search-row" onsubmit={search} role="search">
        <label class="grow"><span class="sr-only">{$t('your_search')}</span>
          <input type="search" required minlength="2" maxlength="200" bind:value={query} disabled={searching} placeholder={$t('rq_search_placeholder', { kind: (kindLabels[kind] ?? '').toLowerCase() })} />
        </label>
        {#if destinationsFor(kind).length > 1}
          <label><span class="sr-only">{$t('destination')}</span>
            <select bind:value={serverId} disabled={searching} onchange={() => { results = []; searched = false; }}>
              <option value="">{$t('default_destination')}</option>
              {#each destinationsFor(kind) as server (server.id)}<option value={server.id}>{server.name}</option>{/each}
            </select>
          </label>
        {/if}
        <button type="submit" class="primary" disabled={searching} aria-busy={searching}>{searching ? $t('searching') : $t('search_action')}</button>
      </form>
      {#if kind === 'track'}<p class="muted small">{$t('request_failed_lidarr')}</p>{/if}
      {#if kind === 'artist'}<p class="muted small">{$t('artist_album_note')}</p>{/if}
      {#each searchErrors as failure, itemIndex (itemIndex)}<p class="feedback error">{failure}</p>{/each}

      {#if results.length}
        <ul class="result-grid" aria-label={$t('results')}>
          {#each results as result, index (`${result.serverId ?? ''}:${result.externalId}:${index}`)}
            {@const reason = blocked(result)}
            <li class="result-card">
              <div class="art" aria-hidden="true">
                {#if art(result.inLibrary?.workId)}<img src={art(result.inLibrary?.workId)} alt="" loading="lazy" onerror={() => brokenArt = [...brokenArt, result.inLibrary?.workId ?? '']} />{:else}<span>{initial(result.title)}</span>{/if}
              </div>
              <div class="result-body">
                <h3>{result.title}{#if result.year}<span class="year">{result.year}</span>{/if}</h3>
                <p class="meta">{metaLine(result)}</p>
                {#if result.summary}<p class="summary">{result.summary}</p>{/if}
                <div class="badges">
                  {#if result.inLibrary}
                    {@const href = workHref(result.kind, result.inLibrary.workId)}
                    {#if href}<a class="badge tone-ok" href={resolve(href)}>{$t('rq_in_library')}</a>{:else}<span class="badge tone-ok">{$t('rq_in_library')}</span>{/if}
                  {/if}
                  {#if result.request}<span class="badge tone-{statusTone(result.request.status)}">{result.request.mine ? $t('rq_requested_by_you') : $t('rq_already_requested')} · {statusLabels[result.request.status] ?? result.request.status}</span>{/if}
                  {#if result.onServer && !result.inLibrary && !result.request}<span class="badge">{$t('rq_on_server')}</span>{/if}
                </div>
              </div>
              <div class="result-action">
                <button class:primary={!reason} disabled={busy || !!reason} onclick={() => choose(result)}>{reason || $t('request_action')}</button>
              </div>
            </li>
          {/each}
        </ul>
      {:else if searched}<p class="empty">{$t('no_request_results')}</p>{/if}
    </section>

    <section class="stack" aria-labelledby="request-list-title">
      <div class="heading compact">
        <h2 id="request-list-title">{isAdmin && !mineOnly ? $t('all_requests') : $t('my_requests')}</h2>
        <div class="row">
          <label class="inline">{$t('request_type')}
            <select bind:value={kindFilter} disabled={busy} onchange={() => run(() => refresh(), $t('loading_impossible'))}>
              <option value="">{$t('rq_all_kinds')}</option>
              {#each kinds as value (value)}<option {value}>{kindLabels[value]}</option>{/each}
            </select>
          </label>
          {#if isAdmin}<label class="check"><input type="checkbox" bind:checked={mineOnly} disabled={busy} onchange={() => run(() => refresh(), $t('loading_impossible'))} />{$t('rq_only_mine')}</label>{/if}
          <button disabled={busy} onclick={() => run(() => refresh(), $t('loading_impossible'))}>{$t('refresh')}</button>
        </div>
      </div>
      <div class="chips" role="group" aria-label={$t('status')}>
        {#each requestGroups as value (value)}
          <button class="chip" aria-pressed={group === value} disabled={busy} onclick={() => setGroup(value)}>{groupLabels[value]}<span class="count">{groupCount(counts, value)}</span></button>
        {/each}
      </div>

      {#if isAdmin && pendingIds.length}
        <div class="bulk-bar" role="group" aria-label={$t('rq_bulk_actions')}>
          <label class="check"><input type="checkbox" checked={allPicked} onchange={toggleAll} disabled={busy} />{$t('rq_select_pending', { count: pendingIds.length })}</label>
          {#if picked.length}
            <span class="muted">{$t('rq_selected_count', { count: picked.length })}</span>
            <button class="primary" disabled={busy} onclick={() => bulk([...picked], 'approve')}>{$t('approve')}</button>
            <button disabled={busy} onclick={() => rejecting = { ids: [...picked], reason: 'other' }}>{$t('reject')}</button>
          {/if}
        </div>
        {#if rejecting && rejecting.ids.length > 1}
          {@render rejectForm()}
        {/if}
      {/if}

      <ul class="records">
        {#each requests as item (item.id)}
          {@const step = requestStep(item.status)}
          {@const scope = scopeLabel(item.kind, item.selection)}
          <li class="record request-record">
            <div class="record-main">
              {#if isAdmin && item.status === 'pending'}<input class="pick" type="checkbox" aria-label={$t('rq_select_request', { title: item.title })} checked={picked.includes(item.id)} onchange={() => togglePick(item.id)} />{/if}
              <div class="art small" aria-hidden="true">
                {#if art(item.workId)}<img src={art(item.workId)} alt="" loading="lazy" onerror={() => brokenArt = [...brokenArt, item.workId ?? '']} />{:else}<span>{initial(item.title)}</span>{/if}
              </div>
              <div class="record-text">
                <h3>{item.title}</h3>
                <small>{kindLabels[item.kind] ?? item.kind} · {dateLabel(item.createdAt)}{#if isAdmin && item.authorLabel && !item.own} · {$t('rq_requested_by', { name: item.authorLabel })}{/if}</small>
                {#if scope}<p class="muted small">{scope}</p>{/if}
                {#if item.status === 'rejected' && item.errorCode}<p class="note">{$t('rq_reject_reason_label')} : {rejectReasonLabels[item.errorCode] ?? errorMessage(item.errorCode)}</p>
                {:else if item.error || item.errorCode}<p class="error small">{item.error ?? errorMessage(item.errorCode)}</p>{/if}
              </div>
              <span class="badge tone-{statusTone(item.status)}">{statusLabels[item.status] ?? item.status}</span>
            </div>
            {#if step >= 0}
              <ol class="steps" aria-label={$t('rq_progress')}>
                {#each requestSteps as value, index (value)}<li class:done={index <= step} aria-current={index === step ? 'step' : undefined}>{stepLabels[value]}</li>{/each}
              </ol>
            {/if}
            <div class="row actions">
              <button disabled={busy} aria-expanded={detail?.id === item.id} onclick={() => show(item.id)}>{detail?.id === item.id ? $t('hide') : $t('view_tracking')}</button>
              {#if item.workId}
                {@const href = workHref(item.kind, item.workId)}
                {#if href}<a class="button-link" href={resolve(href)}>{$t('rq_open_in_library')}</a>{/if}
              {/if}
              {#if isAdmin && item.status === 'pending'}
                <button class="primary" disabled={busy} onclick={() => action(item.id, 'approve')}>{$t('approve')}</button>
                <button disabled={busy} onclick={() => rejecting = { ids: [item.id], reason: 'other' }}>{$t('reject')}</button>
              {/if}
              {#if isAdmin && ['failed', 'suspended'].includes(item.status)}<button disabled={busy} onclick={() => action(item.id, 'retry')}>{item.status === 'suspended' ? $t('resume_request') : $t('request_retry')}</button>{/if}
              {#if ['pending', 'approved'].includes(item.status) && (item.own || isAdmin)}<button class="danger" disabled={busy} onclick={() => action(item.id, 'cancel')}>{$t('cancel_request')}</button>{/if}
            </div>
            {#if rejecting && rejecting.ids.length === 1 && rejecting.ids[0] === item.id}{@render rejectForm()}{/if}
            {#if detail?.id === item.id}
              <div class="detail stack">
                {#if isAdmin && ['pending', 'suspended', 'failed'].includes(detail.status) && destinationsFor(detail.kind).length > 1}
                  <div class="row">
                    <label class="inline">{$t('request_destination')}
                      <select bind:value={actionDestination}>
                        <option value="">{$t('default_destination')}</option>
                        {#each destinationsFor(detail.kind) as server (server.id)}<option value={server.id}>{server.name}</option>{/each}
                      </select>
                    </label>
                    <button disabled={busy} onclick={() => action(item.id, detail?.status === 'pending' ? 'approve' : 'retry', actionDestination ? { serverId: actionDestination } : {})}>{detail.status === 'pending' ? $t('approve_destination') : $t('resume_destination')}</button>
                  </div>
                {/if}
                {#if detail.sourceStatus}<p class="muted">{$t('previous_app_status')} : {detail.sourceStatus}</p>{/if}
                <ol class="timeline">
                  {#each detail.events ?? [] as entry, itemIndex (itemIndex)}
                    <li class="tone-{statusTone(entry.status ?? '')}">
                      <span class="when">{dateLabel(entry.createdAt)}</span>
                      <span>{statusLabels[entry.status ?? ''] ?? entry.action ?? entry.status}{#if entry.actorLabel} · {entry.actorLabel}{/if}{#if entry.code} · {rejectReasonLabels[entry.code] ?? errorMessage(entry.code)}{/if}</span>
                    </li>
                  {:else}<li>{$t('last_modified', { date: dateLabel(detail.updatedAt ?? detail.createdAt) })}</li>{/each}
                </ol>
              </div>
            {/if}
          </li>
        {:else}<li class="empty">{$t('no_requests_selection')}</li>{/each}
      </ul>
      {#if nextCursor}<button disabled={busy} onclick={() => run(() => refresh(true), $t('loading_impossible'))}>{$t('load_previous_requests')}</button>{/if}
    </section>
  {/if}
</div>

{#snippet rejectForm()}
  {#if rejecting}
    <form class="inline-form" onsubmit={(event) => { event.preventDefault(); void confirmReject(); }}>
      <label class="inline">{$t('rq_reject_reason_label')}
        <select bind:value={rejecting.reason}>{#each rejectReasons as value (value)}<option {value}>{rejectReasonLabels[value]}</option>{/each}</select>
      </label>
      <button class="primary" disabled={busy}>{rejecting.ids.length > 1 ? $t('rq_reject_selected', { count: rejecting.ids.length }) : $t('reject')}</button>
      <button type="button" disabled={busy} onclick={() => rejecting = null}>{$t('cancel')}</button>
    </form>
  {/if}
{/snippet}

<dialog bind:this={dialog} class="application-dialog" aria-labelledby="request-dialog-title" onclose={() => selected = null}>
  {#if selected}
    <form class="stack" onsubmit={submit}>
      <header>
        <h2 id="request-dialog-title">{$t('confirm_request_for', { title: selected.title })}</h2>
        <p class="muted small">{metaLine(selected)}{#if selected.year} · {selected.year}{/if}</p>
      </header>
      {#if error}<p role="alert" class="feedback error">{error}</p>{/if}
      {#if selected.inLibrary}<p class="feedback">{$t('rq_dialog_in_library')}</p>{/if}
      {#if selected.kind === 'series'}
        <fieldset class="stack">
          <legend>{$t('rq_seasons_legend')}</legend>
          <label class="check"><input type="radio" name="series-scope" value="all" bind:group={seriesScope} />{$t('rq_all_seasons')}</label>
          <label class="check"><input type="radio" name="series-scope" value="pick" bind:group={seriesScope} />{$t('rq_pick_seasons')}</label>
          {#if seriesScope === 'pick'}
            {#if selected.seasons?.length}
              <div class="season-grid">
                {#each selected.seasons as season (season.number)}
                  <label class="season-chip">
                    <input type="checkbox" value={season.number} bind:group={chosenSeasons} />
                    <span>{season.number === 0 ? $t('rq_specials') : $t('rq_season_n', { number: season.number })}</span>
                    {#if season.episodes}<small>{$t('rq_episode_count', { count: season.episodes })}</small>{/if}
                  </label>
                {/each}
              </div>
            {:else}
              <label>{$t('seasons_optional')}<input bind:value={seasonsText} maxlength="500" placeholder="1, 2" /></label>
            {/if}
            <details>
              <summary>{$t('rq_specific_episodes')}</summary>
              <label>{$t('episodes_optional')}<input bind:value={episodesText} maxlength="1000" placeholder="1:3, 1:4" /></label>
              <p class="muted small">{$t('rq_episodes_hint')}</p>
            </details>
          {/if}
        </fieldset>
        <label class="check"><input type="checkbox" bind:checked={monitorFuture} />{$t('monitor_future_episodes')}</label>
      {/if}
      {#if selected.kind === 'artist'}
        <label class="check"><input type="checkbox" bind:checked={monitorFuture} />{$t('monitor_artist_releases')}</label>
        <p class="muted small">{$t('artist_album_note')}</p>
      {/if}
      {#if summary}<p class="scope-summary">{summary}</p>{/if}
      <footer class="row end">
        <button type="button" onclick={closeDialog}>{$t('cancel')}</button>
        <button class="primary" disabled={!canSubmit} aria-busy={busy}>{$t('confirm_request')}</button>
      </footer>
    </form>
  {/if}
</dialog>
