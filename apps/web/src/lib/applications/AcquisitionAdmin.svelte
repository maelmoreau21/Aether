<script lang="ts">
  import { onMount } from 'svelte';
  import { resolve } from '$app/paths';
  import { session } from '../auth/session';
  import {
    applicationRequest, features, loadFeatures,
    type ArrTag, type QualityProfile, type RemoteServer, type ServerOptions, type ServerSettings, type ServerSystemStatus, type ServerTestResult,
    loadPolicyOverview, saveDefaultPolicy,
    type PolicyRule
  } from './api';
  import './applications.css';
  import { t } from '../i18n';

  let { embedded = false }: { embedded?: boolean } = $props();
  interface Location { id: string; name: string; kind: 'films' | 'series' | 'music'; nodeId: string; nodeName?: string; root: string }

  const defaultSettings = (): ServerSettings => ({
    externalUrl: null,
    tags: [],
    availableTags: [],
    searchOnAdd: true,
    minimumAvailability: 'released',
    seriesType: 'standard',
    seasonFolder: true,
    languageProfileId: null,
    animeQualityProfileId: null,
    animeRootFolder: null,
    animeLanguageProfileId: null,
    animeTags: [],
    monitorNewItems: 'none',
    lastStatus: null
  });

  const blank = (): Omit<RemoteServer, 'id'> & { settings: ServerSettings } => ({
    name: '',
    kind: 'radarr',
    baseUrl: '',
    libraryId: '',
    rootFolder: '',
    qualityProfileId: 0,
    qualityProfiles: [],
    metadataProfileId: undefined,
    settings: defaultSettings(),
    isDefault: false,
    enabled: true
  });

  let activeTab = $state<'services' | 'general'>('services');
  let loading = $state(true);
  let busy = $state(false);
  let testing = $state(false);
  let error = $state('');
  let message = $state('');
  let servers = $state<RemoteServer[]>([]);
  let locations = $state<Location[]>([]);
  let editing = $state<string | null>(null);
  let form = $state(blank());
  let key = $state('');
  let autoApprove = $state(false);
  let maxPending = $state(20);
  let options = $state<ServerOptions | null>(null);
  let connectionTested = $state(false);
  let connectionStatus = $state<ServerSystemStatus | null>(null);
  let connectionError = $state('');
  let showAnimeSettings = $state(false);
  let confirmingRemoval = $state<string | null>(null);

  let defaultRule = $state<PolicyRule>({
    film: { allow: true, autoApprove: false },
    series: { allow: true, autoApprove: false },
    music: { allow: true, autoApprove: false }
  });

  const administrator = $derived($session?.user.role === 'administrator');
  const matchingLocations = $derived(locations.filter(location => location.kind === (form.kind === 'radarr' ? 'films' : form.kind === 'sonarr' ? 'series' : 'music')));
  const settingsUnlocked = $derived(Boolean(editing) || connectionTested);
  const availableTags = $derived<ArrTag[]>(options?.tags ?? form.settings.availableTags ?? []);

  function formatBytes(bytes: number | null | undefined): string {
    if (bytes == null || !Number.isFinite(bytes) || bytes < 0) return '';
    const units = ['o', 'Ko', 'Mo', 'Go', 'To', 'Po'];
    let value = bytes;
    let idx = 0;
    while (value >= 1024 && idx < units.length - 1) {
      value /= 1024;
      idx += 1;
    }
    return `${value >= 10 || idx === 0 ? value.toFixed(0) : value.toFixed(2)} ${units[idx]}`;
  }

  function mergeSettings(raw?: Partial<ServerSettings> | null): ServerSettings {
    const base = defaultSettings();
    if (!raw) return base;
    return {
      ...base,
      ...raw,
      tags: [...(raw.tags ?? [])],
      availableTags: [...(raw.availableTags ?? [])],
      animeTags: [...(raw.animeTags ?? [])]
    };
  }

  async function refresh() {
    const [list, settings, destinations, overview] = await Promise.all([
      applicationRequest<{ servers: RemoteServer[] }>('/api/v1/admin/acquisition/servers'),
      applicationRequest<{ autoApprove: boolean; maxPendingPerUser: number }>('/api/v1/admin/acquisition/settings'),
      applicationRequest<{ locations: Location[] }>('/api/v1/admin/acquisition/locations'),
      loadPolicyOverview().catch(() => null)
    ]);
    servers = list.servers;
    locations = destinations.locations;
    autoApprove = settings.autoApprove;
    maxPending = settings.maxPendingPerUser;
    if (overview) {
      defaultRule = {
        film: { allow: overview.default.film?.allow ?? true, autoApprove: overview.default.film?.autoApprove ?? autoApprove, quota: overview.default.film?.quota },
        series: { allow: overview.default.series?.allow ?? true, autoApprove: overview.default.series?.autoApprove ?? autoApprove, quota: overview.default.series?.quota },
        music: { allow: overview.default.music?.allow ?? true, autoApprove: overview.default.music?.autoApprove ?? autoApprove, quota: overview.default.music?.quota }
      };
    }
  }

  onMount(() => {
    void (async () => {
      try {
        if (!administrator) return;
        const available = await loadFeatures(true);
        if (available.requests) await refresh();
      } catch (cause) {
        error = cause instanceof Error ? cause.message : $t('administration_unavailable');
      } finally {
        loading = false;
      }
    })();
  });

  function edit(server: RemoteServer) {
    editing = server.id;
    const merged = mergeSettings(server.settings);
    form = {
      name: server.name,
      kind: server.kind,
      baseUrl: server.baseUrl,
      libraryId: server.libraryId,
      rootFolder: server.rootFolder,
      qualityProfileId: server.qualityProfileId,
      qualityProfiles: [...(server.qualityProfiles ?? [])],
      metadataProfileId: server.metadataProfileId,
      settings: merged,
      isDefault: server.isDefault,
      enabled: server.enabled
    };
    key = '';
    options = null;
    connectionTested = false;
    connectionStatus = merged.lastStatus ?? null;
    connectionError = '';
    showAnimeSettings = Boolean(
      merged.animeQualityProfileId || merged.animeRootFolder || merged.animeLanguageProfileId || merged.animeTags.length > 0
    );
    message = '';
    activeTab = 'services';
  }

  function clear() {
    editing = null;
    form = blank();
    key = '';
    options = null;
    connectionTested = false;
    connectionStatus = null;
    connectionError = '';
    showAnimeSettings = false;
  }

  function toggleTag(listKey: 'tags' | 'animeTags', id: number) {
    const current = form.settings[listKey];
    if (current.includes(id)) {
      form.settings[listKey] = current.filter(item => item !== id);
    } else {
      form.settings[listKey] = [...current, id];
    }
  }

  function tagLabel(server: RemoteServer, id: number): string {
    return server.settings?.availableTags?.find(t => t.id === id)?.label ?? `#${id}`;
  }

  function ensureDefaultInProfiles() {
    if (!form.qualityProfileId) return;
    const list = [...(form.qualityProfiles ?? [])];
    const source = options?.qualityProfiles.find(p => p.id === form.qualityProfileId);
    if (!list.some(p => p.id === form.qualityProfileId)) {
      if (source) {
        list.unshift({ id: source.id, name: source.name });
      }
    } else if (source) {
      const idx = list.findIndex(p => p.id === form.qualityProfileId);
      if (idx >= 0) list[idx] = { id: source.id, name: source.name };
    }
    form.qualityProfiles = list;
  }

  function toggleQualityProfile(profile: QualityProfile, checked: boolean) {
    if (profile.id === form.qualityProfileId) return;
    const current = [...(form.qualityProfiles ?? [])];
    if (checked) {
      if (!current.some(p => p.id === profile.id)) {
        current.push({ id: profile.id, name: profile.name });
      }
      ensureDefaultInProfiles();
    } else {
      form.qualityProfiles = current.filter(p => p.id !== profile.id);
      return;
    }
    form.qualityProfiles = current;
  }

  function profileEnabled(id: number): boolean {
    if (id === form.qualityProfileId) return true;
    return (form.qualityProfiles ?? []).some(p => p.id === id);
  }

  function defaultProfileLabel(server: RemoteServer): string {
    const found = server.qualityProfiles?.find(p => p.id === server.qualityProfileId);
    return found ? found.name : `#${server.qualityProfileId}`;
  }

  function applyDiscoveredOptions(discovered: ServerOptions, system?: ServerSystemStatus | null) {
    options = discovered;
    connectionTested = true;
    connectionStatus = system ?? null;
    connectionError = '';
    form.settings = {
      ...form.settings,
      availableTags: [...(discovered.tags ?? [])],
      lastStatus: system ?? form.settings.lastStatus
    };
    if (!form.libraryId && matchingLocations.length === 1) {
      form.libraryId = matchingLocations[0].id;
    }
    if (!form.rootFolder && discovered.rootFolders.length > 0) {
      const accessible = discovered.rootFolders.find(f => f.accessible !== false) ?? discovered.rootFolders[0];
      form.rootFolder = accessible.path;
    }
    if ((!form.qualityProfileId || form.qualityProfileId <= 0) && discovered.qualityProfiles.length > 0) {
      form.qualityProfileId = discovered.qualityProfiles[0].id;
    }
    if (form.qualityProfileId > 0) {
      ensureDefaultInProfiles();
    }
    if (form.kind === 'lidarr' && !form.metadataProfileId && discovered.metadataProfiles.length > 0) {
      form.metadataProfileId = discovered.metadataProfiles[0].id;
    }
    if (form.kind === 'sonarr' && !form.settings.languageProfileId && (discovered.languageProfiles?.length ?? 0) > 0) {
      form.settings.languageProfileId = discovered.languageProfiles![0].id;
    }
  }

  async function testConnection() {
    testing = true;
    connectionError = '';
    error = '';
    message = '';
    try {
      const result = await applicationRequest<ServerTestResult>('/api/v1/admin/acquisition/servers/test', {
        method: 'POST',
        body: JSON.stringify({
          ...(editing ? { id: editing } : {}),
          kind: form.kind,
          baseUrl: form.baseUrl,
          ...(key ? { apiKey: key } : {})
        })
      });
      const system: ServerSystemStatus = result.system ?? {
        appName: result.appName,
        version: result.version,
        instanceName: result.instanceName
      };
      if (result.options) {
        applyDiscoveredOptions(result.options, system);
      } else {
        connectionTested = true;
        connectionStatus = system;
      }
      message = $t('connection_successful');
    } catch (cause) {
      connectionTested = false;
      connectionStatus = null;
      connectionError = cause instanceof Error ? cause.message : $t('connection_failed');
    } finally {
      testing = false;
    }
  }

  async function save(event: SubmitEvent) {
    event.preventDefault();
    busy = true;
    error = '';
    message = '';
    try {
      ensureDefaultInProfiles();
      const saved = await applicationRequest<{ server: RemoteServer }>(
        editing ? `/api/v1/admin/acquisition/servers/${encodeURIComponent(editing)}` : '/api/v1/admin/acquisition/servers',
        { method: editing ? 'PUT' : 'POST', body: JSON.stringify({ ...form, ...(key ? { apiKey: key } : {}) }) }
      );
      key = '';
      await refresh();
      if (saved.server?.id) edit(saved.server);
      else clear();
      message = $t('connection_saved_load_its_folders_and');
    } catch (cause) {
      error = cause instanceof Error ? cause.message : $t('connection_not_saved');
    } finally {
      busy = false;
    }
  }

  async function test(id: string) {
    busy = true;
    error = '';
    message = '';
    try {
      const result = await applicationRequest<ServerTestResult>(`/api/v1/admin/acquisition/servers/${encodeURIComponent(id)}/test`, { method: 'POST' });
      const app = result.system?.appName ?? result.appName;
      const version = result.system?.version ?? result.version;
      message = app && version ? $t('arr_connected_banner', { app, version }) : $t('connection_successful');
      if (editing === id && result.options) {
        applyDiscoveredOptions(result.options, result.system ?? { appName: app, version });
      }
    } catch (cause) {
      error = cause instanceof Error ? cause.message : $t('connection_failed');
    } finally {
      busy = false;
    }
  }

  async function remove(id: string) {
    busy = true;
    error = '';
    message = '';
    try {
      await applicationRequest(`/api/v1/admin/acquisition/servers/${encodeURIComponent(id)}`, { method: 'DELETE' });
      confirmingRemoval = null;
      if (editing === id) clear();
      await refresh();
      message = $t('connection_removed');
    } catch (cause) {
      error = cause instanceof Error ? cause.message : $t('deletion_failed');
    } finally {
      busy = false;
    }
  }

  async function saveGeneralAndDefault(event: SubmitEvent) {
    event.preventDefault();
    busy = true;
    error = '';
    message = '';
    try {
      await Promise.all([
        applicationRequest('/api/v1/admin/acquisition/settings', {
          method: 'PUT',
          body: JSON.stringify({ autoApprove, maxPendingPerUser: maxPending })
        }),
        saveDefaultPolicy(defaultRule)
      ]);
      await refresh();
      message = $t('rq_policy_saved');
    } catch (cause) {
      error = cause instanceof Error ? cause.message : $t('saving_failed');
    } finally {
      busy = false;
    }
  }
</script>

<div class="application-page">
  <header class="heading">
    <div>
      <p class="muted">{$t('administration')}</p>
      {#if embedded}<h2>{$t('admin_acquisition')}</h2>{:else}<h1>{$t('requests_and_acquisitions')}</h1>{/if}
    </div>
    <a class="button-link" href={embedded ? '#requests-section' : resolve('requests/')}>{$t('view_requests')}</a>
  </header>

  {#if error}<p role="alert" class="feedback error">{error}</p>{/if}
  {#if message}<p role="status" class="feedback success">{message}</p>{/if}

  {#if !administrator}
    <p class="panel">{$t('this_page_is_reserved_for_administrators')}</p>
  {:else if loading}
    <p role="status">{$t('loading_settings_short')}</p>
  {:else if !$features.requests && !$features.reports}
    <p class="panel">{$t('install_and_enable_requests_or_reports')}</p>
  {:else if $features.requests}
    <div class="seerr-tabs" role="tablist">
      <button type="button" role="tab" class="seerr-tab" class:active={activeTab === 'services'} aria-selected={activeTab === 'services'} onclick={() => activeTab = 'services'}>
        {$t('rq_tab_services')}
      </button>
      <button type="button" role="tab" class="seerr-tab" class:active={activeTab === 'general'} aria-selected={activeTab === 'general'} onclick={() => activeTab = 'general'}>
        {$t('rq_tab_general')}
      </button>
    </div>

    {#if activeTab === 'services'}
      <div class="stack">
        <section>
          <div class="heading">
            <h2>{$t('acquisition_servers')}</h2>
            <button disabled={busy} onclick={clear}>{$t('new_connection')}</button>
          </div>
          <p class="muted">{$t('each_request_uses_a_chosen_destination')}</p>
          <ul class="records">
            {#each servers as server (server.id)}
              <li class="record">
                <div class="record-header">
                  <div>
                    <h3>{server.name}</h3>
                    <small>
                      {server.kind}
                      {#if server.settings?.lastStatus?.version} · v{server.settings.lastStatus.version}{/if}
                      · {server.enabled ? $t('active_state') : $t('disabled_state')}{server.isDefault ? $t('applications_default') : ''}
                    </small>
                    <p class="muted small">
                      {server.baseUrl}
                      {#if server.rootFolder} · <code>{server.rootFolder}</code>{/if}
                      · {$t('default_quality_profile')}: <strong>{defaultProfileLabel(server)}</strong>
                      {#if (server.qualityProfiles?.length ?? 0) > 1}
                        ({$t('quality_profiles_count', { count: server.qualityProfiles?.length ?? 0 })})
                      {/if}
                    </p>
                    {#if server.settings?.tags?.length}
                      <div class="tag-pills">
                        {#each server.settings.tags as tagId (tagId)}
                          <span class="badge tone-info">{tagLabel(server, tagId)}</span>
                        {/each}
                      </div>
                    {/if}
                  </div>
                  <div class="row">
                    {#if server.settings?.externalUrl}
                      <!-- eslint-disable-next-line svelte/no-navigation-without-resolve -->
                      <a class="button-link" href={server.settings.externalUrl} target="_blank" rel="noopener noreferrer">{$t('arr_open_service', { service: server.kind })}</a>
                    {/if}
                    <button disabled={busy} onclick={() => edit(server)}>{$t('edit')}</button>
                    <button disabled={busy} onclick={() => test(server.id)}>{$t('test')}</button>
                    <button class="danger" disabled={busy} onclick={() => confirmingRemoval = server.id}>{$t('remove')}</button>
                  </div>
                </div>
                {#if confirmingRemoval === server.id}
                  <div class="detail">
                    <p>{$t('remove_this_connection_existing_requests')}</p>
                    <div class="row">
                      <button class="danger" disabled={busy} onclick={() => remove(server.id)}>{$t('confirm_removal')}</button>
                      <button onclick={() => confirmingRemoval = null}>{$t('keep')}</button>
                    </div>
                  </div>
                {/if}
              </li>
            {:else}
              <li class="empty">{$t('no_servers_configured')}</li>
            {/each}
          </ul>
        </section>

        <section class="panel stack">
          <h2>{editing ? $t('edit_connection') : $t('add_a_connection')}</h2>
          <form class="stack" onsubmit={save}>
            <div class="arr-step">
              <h3>{$t('arr_step_connection')}</h3>
              <div class="fields">
                <label>{$t('name')}
                  <input required maxlength="100" bind:value={form.name} placeholder={form.kind === 'radarr' ? 'Radarr' : form.kind === 'sonarr' ? 'Sonarr' : 'Lidarr'} />
                </label>
                <label>{$t('service')}
                  <select bind:value={form.kind} onchange={() => { form.libraryId = ''; form.qualityProfiles = []; form.rootFolder = ''; form.qualityProfileId = 0; form.metadataProfileId = undefined; options = null; connectionTested = false; connectionStatus = null; connectionError = ''; }}>
                    <option value="radarr">{$t('radarr_films')}</option>
                    <option value="sonarr">{$t('sonarr_series')}</option>
                    <option value="lidarr">{$t('lidarr_music')}</option>
                  </select>
                </label>
                <label>{$t('server_address')}
                  <input required type="url" maxlength="2048" bind:value={form.baseUrl} placeholder={form.kind === 'radarr' ? 'http://serveur:7878' : form.kind === 'sonarr' ? 'http://serveur:8989' : 'http://serveur:8686'} />
                </label>
                <label>{$t('api_key_label')}{editing ? $t('leave_empty_to_keep') : ''}
                  <input type="password" autocomplete="new-password" required={!editing} maxlength="512" bind:value={key} />
                </label>
              </div>
              <p class="muted small">{$t('choosing_an_http_address_explicitly_allows')}</p>

              <div class="row">
                <button type="button" class="secondary" disabled={busy || testing || !form.baseUrl || (!editing && !key)} onclick={testConnection}>
                  {testing ? $t('arr_testing_connection') : editing ? $t('arr_test_and_sync') : $t('arr_test_connection')}
                </button>
              </div>

              {#if connectionError}
                <p role="alert" class="feedback error">{connectionError}</p>
              {/if}
              {#if connectionStatus}
                <div class="arr-status-banner">
                  <strong>✓ {$t('arr_connected_banner', { app: connectionStatus.appName ?? form.kind, version: connectionStatus.version ? `v${connectionStatus.version}` : '' })}</strong>
                  {#if connectionStatus.instanceName}<span class="muted"> · {connectionStatus.instanceName}</span>{/if}
                  {#if options}<p class="muted small">{$t('arr_connected_hint')}</p>{/if}
                </div>
              {/if}
            </div>

            <hr class="seerr-divider" />

            <div class="arr-step" class:arr-step-locked={!settingsUnlocked}>
              <h3>{$t('arr_step_settings')}</h3>
              {#if !settingsUnlocked}
                <p class="muted">{$t('arr_connect_first_hint')}</p>
              {:else}
                <div class="fields">
                  <label>{$t('destination_location')}
                    <select required bind:value={form.libraryId}>
                      <option value="">{$t('choose_a_library_folder')}</option>
                      {#each matchingLocations as location (location.id)}
                        <option value={location.id}>{location.name} · {location.nodeName ?? location.nodeId} · {location.root}</option>
                      {/each}
                    </select>
                  </label>

                  <label>{$t('remote_folder')}
                    {#if options}
                      <select required={form.enabled} bind:value={form.rootFolder}>
                        <option value="">{$t('choose_a_folder')}</option>
                        {#each options.rootFolders as folder (folder.path)}
                          <option value={folder.path}>
                            {folder.path}{folder.freeSpace != null ? ` (${$t('arr_free_space', { size: formatBytes(folder.freeSpace) })})` : ''}{folder.accessible === false ? ` · ${$t('arr_folder_inaccessible')}` : ''}
                          </option>
                        {/each}
                      </select>
                    {:else}
                      <input required={form.enabled} bind:value={form.rootFolder} maxlength="1024" />
                    {/if}
                  </label>

                  <label>{$t('default_quality_profile')}
                    {#if options}
                      <select required={form.enabled} bind:value={form.qualityProfileId} onchange={ensureDefaultInProfiles}>
                        <option value={0}>{$t('choose_a_profile')}</option>
                        {#each options.qualityProfiles as profile (profile.id)}
                          <option value={profile.id}>{profile.name}</option>
                        {/each}
                      </select>
                    {:else}
                      <input type="number" min="0" bind:value={form.qualityProfileId} />
                    {/if}
                  </label>

                  {#if form.kind === 'lidarr'}
                    <label>{$t('metadata_profile')}
                      {#if options}
                        <select required={form.enabled} bind:value={form.metadataProfileId}>
                          <option value={undefined}>{$t('choose_a_profile')}</option>
                          {#each options.metadataProfiles as profile (profile.id)}
                            <option value={profile.id}>{profile.name}</option>
                          {/each}
                        </select>
                      {:else}
                        <input type="number" min="1" bind:value={form.metadataProfileId} />
                      {/if}
                    </label>
                    <label>{$t('arr_monitor_new_items')}
                      <select bind:value={form.settings.monitorNewItems}>
                        <option value="none">{$t('arr_monitor_none')}</option>
                        <option value="all">{$t('arr_monitor_all')}</option>
                        <option value="new">{$t('arr_monitor_new')}</option>
                      </select>
                    </label>
                  {/if}

                  {#if form.kind === 'radarr'}
                    <label>{$t('arr_minimum_availability')}
                      <select bind:value={form.settings.minimumAvailability}>
                        <option value="announced">{$t('arr_min_avail_announced')}</option>
                        <option value="inCinemas">{$t('arr_min_avail_in_cinemas')}</option>
                        <option value="released">{$t('arr_min_avail_released')}</option>
                        <option value="preDB">{$t('arr_min_avail_predb')}</option>
                      </select>
                    </label>
                  {/if}

                  {#if form.kind === 'sonarr'}
                    <label>{$t('arr_series_type')}
                      <select bind:value={form.settings.seriesType}>
                        <option value="standard">{$t('arr_series_type_standard')}</option>
                        <option value="daily">{$t('arr_series_type_daily')}</option>
                        <option value="anime">{$t('arr_series_type_anime')}</option>
                      </select>
                    </label>
                    {#if options?.languageProfiles?.length || form.settings.languageProfileId}
                      <label>{$t('arr_language_profile')}
                        {#if options?.languageProfiles?.length}
                          <select bind:value={form.settings.languageProfileId}>
                            <option value={null}>{$t('arr_language_profile_none')}</option>
                            {#each options.languageProfiles as profile (profile.id)}
                              <option value={profile.id}>{profile.name}</option>
                            {/each}
                          </select>
                        {:else}
                          <input type="number" min="1" bind:value={form.settings.languageProfileId} />
                        {/if}
                      </label>
                    {/if}
                  {/if}

                  <label>{$t('arr_external_url')}
                    <input type="url" maxlength="2048" bind:value={form.settings.externalUrl} placeholder="https://arr.exemple.com" />
                  </label>
                </div>

                {#if options?.qualityProfiles?.length || (form.qualityProfiles?.length ?? 0) > 0}
                  <div class="quality-profiles-box">
                    <h4>{$t('available_quality_profiles')}</h4>
                    <p class="muted small">{$t('available_quality_profiles_hint')}</p>
                    <div class="quality-profiles-grid">
                      {#each (options?.qualityProfiles ?? form.qualityProfiles ?? []) as profile (profile.id)}
                        {@const isDef = profile.id === form.qualityProfileId}
                        <label class="quality-chip" class:is-default={isDef}>
                          <input
                            type="checkbox"
                            checked={profileEnabled(profile.id)}
                            disabled={busy || isDef}
                            onchange={e => toggleQualityProfile(profile, e.currentTarget.checked)}
                          />
                          <span>{profile.name}</span>
                          {#if isDef}
                            <small class="badge tone-info">{$t('quality_profile_default_badge')}</small>
                          {/if}
                        </label>
                      {/each}
                    </div>
                  </div>
                {/if}

                <div class="quality-profiles-box">
                  <h4>{$t('arr_tags')}</h4>
                  {#if availableTags.length > 0}
                    <div class="quality-profiles-grid">
                      {#each availableTags as tag (tag.id)}
                        <label class="quality-chip" class:is-default={form.settings.tags.includes(tag.id)}>
                          <input
                            type="checkbox"
                            checked={form.settings.tags.includes(tag.id)}
                            disabled={busy}
                            onchange={() => toggleTag('tags', tag.id)}
                          />
                          <span>{tag.label}</span>
                        </label>
                      {/each}
                    </div>
                  {:else}
                    <p class="muted small">{$t('arr_tags_none')}</p>
                  {/if}
                </div>

                {#if form.kind === 'sonarr'}
                  <label class="check">
                    <input type="checkbox" bind:checked={form.settings.seasonFolder} />
                    {$t('arr_season_folders')}
                  </label>

                  <div class="quality-profiles-box">
                    <div class="record-header">
                      <div>
                        <h4>{$t('arr_anime_section')}</h4>
                        <p class="muted small">{$t('arr_anime_section_hint')}</p>
                      </div>
                      <button type="button" onclick={() => showAnimeSettings = !showAnimeSettings}>
                        {showAnimeSettings ? '−' : '+'}
                      </button>
                    </div>
                    {#if showAnimeSettings}
                      <div class="fields">
                        <label>{$t('arr_anime_root_folder')}
                          {#if options}
                            <select bind:value={form.settings.animeRootFolder}>
                              <option value={null}>{$t('arr_use_default_setting')}</option>
                              {#each options.rootFolders as folder (folder.path)}
                                <option value={folder.path}>{folder.path}</option>
                              {/each}
                            </select>
                          {:else}
                            <input bind:value={form.settings.animeRootFolder} maxlength="1024" placeholder={$t('arr_use_default_setting')} />
                          {/if}
                        </label>
                        <label>{$t('arr_anime_quality_profile')}
                          {#if options}
                            <select bind:value={form.settings.animeQualityProfileId}>
                              <option value={null}>{$t('arr_use_default_setting')}</option>
                              {#each options.qualityProfiles as profile (profile.id)}
                                <option value={profile.id}>{profile.name}</option>
                              {/each}
                            </select>
                          {:else}
                            <input type="number" min="1" bind:value={form.settings.animeQualityProfileId} />
                          {/if}
                        </label>
                        {#if options?.languageProfiles?.length || form.settings.animeLanguageProfileId}
                          <label>{$t('arr_anime_language_profile')}
                            {#if options?.languageProfiles?.length}
                              <select bind:value={form.settings.animeLanguageProfileId}>
                                <option value={null}>{$t('arr_use_default_setting')}</option>
                                {#each options.languageProfiles as profile (profile.id)}
                                  <option value={profile.id}>{profile.name}</option>
                                {/each}
                              </select>
                            {:else}
                              <input type="number" min="1" bind:value={form.settings.animeLanguageProfileId} />
                            {/if}
                          </label>
                        {/if}
                      </div>
                      {#if availableTags.length > 0}
                        <span class="small muted">{$t('arr_anime_tags')}</span>
                        <div class="quality-profiles-grid">
                          {#each availableTags as tag (tag.id)}
                            <label class="quality-chip" class:is-default={form.settings.animeTags.includes(tag.id)}>
                              <input
                                type="checkbox"
                                checked={form.settings.animeTags.includes(tag.id)}
                                disabled={busy}
                                onchange={() => toggleTag('animeTags', tag.id)}
                              />
                              <span>{tag.label}</span>
                            </label>
                          {/each}
                        </div>
                      {/if}
                    {/if}
                  </div>
                {/if}

                <label class="check"><input type="checkbox" bind:checked={form.settings.searchOnAdd} />{$t('arr_search_on_add')}</label>
                <label class="check"><input type="checkbox" bind:checked={form.isDefault} />{$t('default_destination_for_this_media_type')}</label>
                <label class="check"><input type="checkbox" bind:checked={form.enabled} />{$t('connection_enabled')}</label>
                <div class="row">
                  <button class="primary" disabled={busy}>{$t('save')}</button>
                  {#if editing}<button type="button" disabled={busy} onclick={clear}>{$t('finish_editing')}</button>{/if}
                </div>
              {/if}
            </div>
          </form>
        </section>
      </div>
    {:else}
      <div class="stack">
        <form class="panel stack" onsubmit={saveGeneralAndDefault}>
          <div>
            <h2>{$t('approval')}</h2>
            <p class="muted">{$t('rq_policy_subtitle')}</p>
          </div>
          <div class="fields">
            <label>{$t('handling_of_new_requests')}
              <select bind:value={autoApprove}>
                <option value={false}>{$t('approval_by_an_administrator')}</option>
                <option value={true}>{$t('automatic_approval')}</option>
              </select>
            </label>
            <label>{$t('maximum_pending_requests_per_user')}
              <input type="number" min="1" max="1000" required bind:value={maxPending} />
            </label>
          </div>

          <hr class="seerr-divider" />

          <div>
            <h3>{$t('rq_policy_default_heading')}</h3>
            <p class="muted small">{$t('rq_policy_default_desc')}</p>
          </div>

          <div class="policy-classes-grid">
            {#each ['film', 'series', 'music'] as const as mediaClass (mediaClass)}
              {@const classRule = defaultRule[mediaClass] ?? { allow: true, autoApprove: false }}
              <div class="policy-card">
                <h4>{$t(mediaClass === 'film' ? 'kind_movie' : mediaClass === 'series' ? 'kind_series' : 'audio')}</h4>
                <label class="check">
                  <input type="checkbox" bind:checked={classRule.allow} />
                  <span>{$t('rq_policy_allow')}</span>
                </label>
                <label>
                  <span>{$t('rq_policy_auto')}</span>
                  <select bind:value={classRule.autoApprove}>
                    <option value={false}>{$t('rq_policy_auto_review')}</option>
                    <option value={true}>{$t('rq_policy_auto_direct')}</option>
                  </select>
                </label>
                <div class="quota-field">
                  <span class="small muted">{$t('rq_policy_quota')} · {$t('rq_policy_quota_sliding')}</span>
                  <div class="row">
                    <label class="inline">
                      <span class="sr-only">{$t('rq_policy_quota_limit_label')}</span>
                      <input type="number" min="0" max="1000" placeholder={$t('rq_policy_quota_none')}
                        value={classRule.quota?.limit ?? ''}
                        oninput={e => {
                          const val = Number((e.target as HTMLInputElement).value);
                          if (!val) classRule.quota = undefined;
                          else classRule.quota = { limit: val, days: classRule.quota?.days ?? 7 };
                        }}
                      />
                    </label>
                    {#if classRule.quota?.limit}
                      <label class="inline">
                        <span class="sr-only">{$t('rq_policy_quota_days_label')}</span>
                        <input type="number" min="1" max="365"
                          value={classRule.quota.days}
                          oninput={e => {
                            if (classRule.quota) classRule.quota.days = Math.max(1, Number((e.target as HTMLInputElement).value) || 7);
                          }}
                        />
                      </label>
                    {/if}
                  </div>
                </div>
              </div>
            {/each}
          </div>

          <div>
            <button class="primary" disabled={busy}>{$t('rq_policy_save_default')}</button>
          </div>
        </form>

        <section class="panel stack">
          <div>
            <h3>{$t('rq_policy_moved_title')}</h3>
            <p class="muted">{$t('rq_policy_moved_desc')}</p>
          </div>
          <div class="row">
            <a class="button-link" href={embedded ? '#groups-section' : resolve(`admin/libraries/#groups-section`)}>{$t('rq_open_groups')}</a>
            <a class="button-link" href={embedded ? '#users-section' : resolve(`admin/libraries/#users-section`)}>{$t('rq_open_users')}</a>
          </div>
        </section>
      </div>
    {/if}
  {/if}
</div>

<style>
  .seerr-tabs {
    display: flex;
    gap: 0.5rem;
    border-bottom: 1px solid var(--border-subtle, rgba(255, 255, 255, 0.1));
    margin-bottom: 1.25rem;
  }
  .seerr-tab {
    background: none;
    border: none;
    border-bottom: 2px solid transparent;
    padding: 0.65rem 1rem;
    cursor: pointer;
    font-weight: 500;
    opacity: 0.75;
    color: inherit;
  }
  .seerr-tab.active {
    opacity: 1;
    border-bottom-color: var(--accent, #5ea2ff);
  }
  .seerr-divider {
    border: none;
    border-top: 1px solid var(--border-subtle, rgba(255, 255, 255, 0.08));
    margin: 0.5rem 0;
  }
  .quality-profiles-box {
    border: 1px solid var(--border-subtle, rgba(255, 255, 255, 0.1));
    border-radius: 8px;
    padding: 0.85rem 1rem;
    display: flex;
    flex-direction: column;
    gap: 0.5rem;
  }
  .quality-profiles-box h4 {
    margin: 0;
  }
  .quality-profiles-grid {
    display: flex;
    flex-wrap: wrap;
    gap: 0.5rem;
  }
  .quality-chip {
    display: inline-flex;
    align-items: center;
    gap: 0.45rem;
    padding: 0.4rem 0.7rem;
    border-radius: 6px;
    border: 1px solid var(--border-subtle, rgba(255, 255, 255, 0.12));
    cursor: pointer;
    font-size: 0.88rem;
  }
  .quality-chip.is-default {
    border-color: var(--accent, #5ea2ff);
  }
  .arr-step {
    display: flex;
    flex-direction: column;
    gap: 0.75rem;
  }
  .arr-step h3 {
    margin: 0;
  }
  .arr-step-locked {
    opacity: 0.65;
  }
  .arr-status-banner {
    border: 1px solid var(--ok, #4ade80);
    background: rgba(74, 222, 128, 0.08);
    border-radius: 8px;
    padding: 0.75rem 1rem;
  }
  .arr-status-banner p {
    margin: 0.25rem 0 0;
  }
  .tag-pills {
    display: flex;
    flex-wrap: wrap;
    gap: 0.35rem;
    margin-top: 0.35rem;
  }
</style>
