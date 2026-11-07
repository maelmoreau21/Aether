<script lang="ts">
  import { onMount } from 'svelte';
  import { resolve } from '$app/paths';
  import { session } from '../auth/session';
  import { applicationRequest, features, loadFeatures, type RemoteServer } from './api';
  import LegacyImport from './LegacyImport.svelte';
  import './applications.css';
  import { t } from '../i18n';
  let { embedded = false }: { embedded?: boolean } = $props();
  interface Profile { id: number; name: string }
  interface Options { rootFolders: { path: string }[]; qualityProfiles: Profile[]; metadataProfiles: Profile[] }
  interface Location { id: string; name: string; kind: 'films' | 'series' | 'music'; nodeId: string; nodeName?: string; root: string }
  const blank = (): Omit<RemoteServer, 'id'> => ({ name: '', kind: 'radarr', baseUrl: '', libraryId: '', rootFolder: '', qualityProfileId: 0, metadataProfileId: undefined, isDefault: false, enabled: false });
  let loading = $state(true);
  let busy = $state(false);
  let error = $state('');
  let message = $state('');
  let servers = $state<RemoteServer[]>([]);
  let locations = $state<Location[]>([]);
  let editing = $state<string | null>(null);
  let form = $state(blank());
  let key = $state('');
  let autoApprove = $state(false);
  let maxPending = $state(20);
  let options = $state<Options | null>(null);
  let confirmingRemoval = $state<string | null>(null);
  const administrator = $derived($session?.user.role === 'administrator');
  const matchingLocations = $derived(locations.filter(location => location.kind === (form.kind === 'radarr' ? 'films' : form.kind === 'sonarr' ? 'series' : 'music')));
  async function refresh() {
    const [list, settings, destinations] = await Promise.all([
      applicationRequest<{ servers: RemoteServer[] }>('/api/v1/admin/acquisition/servers'),
      applicationRequest<{ autoApprove: boolean; maxPendingPerUser: number }>('/api/v1/admin/acquisition/settings'),
      applicationRequest<{ locations: Location[] }>('/api/v1/admin/acquisition/locations')
    ]);
    servers = list.servers; locations = destinations.locations; autoApprove = settings.autoApprove; maxPending = settings.maxPendingPerUser;
  }
  onMount(() => { void (async () => { try { if (!administrator) return; const available = await loadFeatures(true); if (available.requests) await refresh(); } catch (cause) { error = cause instanceof Error ? cause.message : $t('administration_unavailable'); } finally { loading = false; } })(); });
  function edit(server: RemoteServer) { editing = server.id; form = { name: server.name, kind: server.kind, baseUrl: server.baseUrl, libraryId: server.libraryId, rootFolder: server.rootFolder, qualityProfileId: server.qualityProfileId, metadataProfileId: server.metadataProfileId, isDefault: server.isDefault, enabled: server.enabled }; key = ''; options = null; message = ''; }
  function clear() { editing = null; form = blank(); key = ''; options = null; }
  async function save(event: SubmitEvent) {
    event.preventDefault(); busy = true; error = ''; message = '';
    try {
      const saved = await applicationRequest<{ server: RemoteServer }>(editing ? `/api/v1/admin/acquisition/servers/${encodeURIComponent(editing)}` : '/api/v1/admin/acquisition/servers', { method: editing ? 'PUT' : 'POST', body: JSON.stringify({ ...form, ...(key ? { apiKey: key } : {}) }) });
      key = ''; await refresh(); if (saved.server?.id) edit(saved.server); else clear(); message = $t('connection_saved_load_its_folders_and');
    } catch (cause) { error = cause instanceof Error ? cause.message : $t('connection_not_saved'); }
    finally { busy = false; }
  }
  async function test(id: string) {
    busy = true; error = ''; message = '';
    try { await applicationRequest(`/api/v1/admin/acquisition/servers/${encodeURIComponent(id)}/test`, { method: 'POST' }); message = $t('connection_successful'); }
    catch (cause) { error = cause instanceof Error ? cause.message : $t('connection_failed'); }
    finally { busy = false; }
  }
  async function loadOptions() {
    if (!editing) return; busy = true; error = '';
    try { options = await applicationRequest<Options>(`/api/v1/admin/acquisition/servers/${encodeURIComponent(editing)}/options`); }
    catch (cause) { error = cause instanceof Error ? cause.message : $t('profiles_unavailable'); }
    finally { busy = false; }
  }
  async function remove(id: string) {
    busy = true; error = ''; message = '';
    try { await applicationRequest(`/api/v1/admin/acquisition/servers/${encodeURIComponent(id)}`, { method: 'DELETE' }); confirmingRemoval = null; if (editing === id) clear(); await refresh(); message = $t('connection_removed'); }
    catch (cause) { error = cause instanceof Error ? cause.message : $t('deletion_failed'); }
    finally { busy = false; }
  }
  async function saveSettings(event: SubmitEvent) {
    event.preventDefault(); busy = true; error = ''; message = '';
    try { await applicationRequest('/api/v1/admin/acquisition/settings', { method: 'PUT', body: JSON.stringify({ autoApprove, maxPendingPerUser: maxPending }) }); message = $t('approval_rules_saved'); }
    catch (cause) { error = cause instanceof Error ? cause.message : $t('saving_failed'); }
    finally { busy = false; }
  }
</script>
<div class="application-page">
  <header class="heading"><div><p class="muted">{$t('administration')}</p>{#if embedded}<h2>{$t('admin_acquisition')}</h2>{:else}<h1>{$t('requests_and_acquisitions')}</h1>{/if}</div><a class="button-link" href={embedded ? '#requests-section' : resolve('requests/')}>{$t('view_requests')}</a></header>
  {#if error}<p role="alert" class="feedback error">{error}</p>{/if}{#if message}<p role="status" class="feedback success">{message}</p>{/if}
  {#if !administrator}<p class="panel">{$t('this_page_is_reserved_for_administrators')}</p>{:else if loading}<p role="status">{$t('loading_settings_short')}</p>{:else if !$features.requests && !$features.reports}<p class="panel">{$t('install_and_enable_requests_or_reports')}</p>{:else}
    <div class="stack">
      {#if $features.requests}
      <section class="panel"><h2>{$t('approval')}</h2><form class="stack" onsubmit={saveSettings}><label>{$t('handling_of_new_requests')}<select bind:value={autoApprove}><option value={false}>{$t('approval_by_an_administrator')}</option><option value={true}>{$t('automatic_approval')}</option></select></label><label>{$t('maximum_pending_requests_per_user')}<input type="number" min="1" max="1000" required bind:value={maxPending} /></label><div><button class="primary" disabled={busy}>{$t('save_rules')}</button></div></form></section>
      <section><div class="heading"><h2>{$t('acquisition_servers')}</h2><button disabled={busy} onclick={clear}>{$t('new_connection')}</button></div><p class="muted">{$t('each_request_uses_a_chosen_destination')}</p><ul class="records">{#each servers as server (server.id)}<li class="record"><div class="record-header"><div><h3>{server.name}</h3><small>{server.kind} · {server.enabled ? 'Activé' : 'Désactivé'}{server.isDefault ? $t('applications_default') : ''}</small><p class="muted">{server.baseUrl}</p></div><div class="row"><button disabled={busy} onclick={() => edit(server)}>{$t('edit')}</button><button disabled={busy} onclick={() => test(server.id)}>{$t('test')}</button><button class="danger" disabled={busy} onclick={() => confirmingRemoval = server.id}>{$t('remove')}</button></div></div>{#if confirmingRemoval === server.id}<div class="detail"><p>{$t('remove_this_connection_existing_requests')}</p><div class="row"><button class="danger" disabled={busy} onclick={() => remove(server.id)}>{$t('confirm_removal')}</button><button onclick={() => confirmingRemoval = null}>{$t('keep')}</button></div></div>{/if}</li>{:else}<li class="empty">{$t('no_servers_configured')}</li>{/each}</ul></section>
      <section class="panel stack"><h2>{editing ? $t('edit_connection') : $t('add_a_connection')}</h2><form class="stack" onsubmit={save}>
        <div class="fields"><label>{$t('name')}<input required maxlength="100" bind:value={form.name} /></label><label>{$t('service')}<select bind:value={form.kind} onchange={() => { form.libraryId = ''; options = null; }}><option value="radarr">{$t('radarr_films')}</option><option value="sonarr">{$t('sonarr_series')}</option><option value="lidarr">{$t('lidarr_music')}</option></select></label><label>{$t('server_address')}<input required type="url" maxlength="2048" bind:value={form.baseUrl} placeholder="http://serveur:7878" /></label><label>{$t('api_key_label')}{editing ? $t('leave_empty_to_keep') : ''}<input type="password" autocomplete="new-password" required={!editing} maxlength="512" bind:value={key} /></label><label>{$t('destination_location')}<select required bind:value={form.libraryId}><option value="">{$t('choose_a_library_folder')}</option>{#each matchingLocations as location (location.id)}<option value={location.id}>{location.name} · {location.nodeName ?? location.nodeId} · {location.root}</option>{/each}</select></label></div>
        <p class="muted">{$t('choosing_an_http_address_explicitly_allows')}</p>
        {#if editing}<div><button type="button" disabled={busy} onclick={loadOptions}>{$t('load_folders_and_profiles')}</button></div>{/if}
        <div class="fields"><label>{$t('remote_folder')}{#if options}<select required bind:value={form.rootFolder}><option value="">{$t('choose_a_folder')}</option>{#each options.rootFolders as folder (folder.path)}<option value={folder.path}>{folder.path}</option>{/each}</select>{:else}<input bind:value={form.rootFolder} maxlength="1024" placeholder={$t('choose_after_the_first_save')} />{/if}</label><label>{$t('quality_profile')}{#if options}<select bind:value={form.qualityProfileId}><option value={0}>{$t('choose_a_profile')}</option>{#each options.qualityProfiles as profile (profile.id)}<option value={profile.id}>{profile.name}</option>{/each}</select>{:else}<input type="number" min="0" bind:value={form.qualityProfileId} />{/if}</label>{#if form.kind === 'lidarr'}<label>{$t('metadata_profile')}{#if options}<select bind:value={form.metadataProfileId}><option value={undefined}>{$t('choose_a_profile')}</option>{#each options.metadataProfiles as profile (profile.id)}<option value={profile.id}>{profile.name}</option>{/each}</select>{:else}<input type="number" min="1" bind:value={form.metadataProfileId} />{/if}</label>{/if}</div>
        <label class="check"><input type="checkbox" bind:checked={form.isDefault} />{$t('default_destination_for_this_media_type')}</label><label class="check"><input type="checkbox" bind:checked={form.enabled} />{$t('connection_enabled')}</label><div class="row"><button class="primary" disabled={busy}>{$t('save')}</button>{#if editing}<button type="button" disabled={busy} onclick={clear}>{$t('finish_editing')}</button>{/if}</div>
      </form></section>
      {/if}
      <LegacyImport {servers} />
    </div>
  {/if}
</div>
