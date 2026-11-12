<script lang="ts">
  import { onMount } from 'svelte';
  import { resolve } from '$app/paths';
  import { session } from '../auth/session';
  import {
    applicationRequest, features, loadFeatures, type RemoteServer,
    loadPolicyOverview, saveDefaultPolicy, saveGroupPolicy, deleteGroupPolicy,
    saveUserPolicy, deleteUserPolicy, loadUserEffectivePolicy,
    type PolicyOverview, type PolicyRule, type MyPolicy
  } from './api';
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
  let policyOverview = $state<PolicyOverview | null>(null);
  let defaultRule = $state<PolicyRule>({
    film: { allow: true, autoApprove: false },
    series: { allow: true, autoApprove: false },
    music: { allow: true, autoApprove: false }
  });
  let editingGroup = $state<{ id: string; name: string; rule: PolicyRule } | null>(null);
  let editingUser = $state<{ id: string; rule: PolicyRule } | null>(null);
  let inspectModal = $state<{ id: string; effective: MyPolicy } | null>(null);
  let newUserId = $state('');

  const administrator = $derived($session?.user.role === 'administrator');
  const matchingLocations = $derived(locations.filter(location => location.kind === (form.kind === 'radarr' ? 'films' : form.kind === 'sonarr' ? 'series' : 'music')));
  async function refresh() {
    const [list, settings, destinations, overview] = await Promise.all([
      applicationRequest<{ servers: RemoteServer[] }>('/api/v1/admin/acquisition/servers'),
      applicationRequest<{ autoApprove: boolean; maxPendingPerUser: number }>('/api/v1/admin/acquisition/settings'),
      applicationRequest<{ locations: Location[] }>('/api/v1/admin/acquisition/locations'),
      loadPolicyOverview().catch(() => null)
    ]);
    servers = list.servers; locations = destinations.locations; autoApprove = settings.autoApprove; maxPending = settings.maxPendingPerUser;
    if (overview) {
      policyOverview = overview;
      defaultRule = {
        film: { allow: overview.default.film?.allow ?? true, autoApprove: overview.default.film?.autoApprove ?? autoApprove, quota: overview.default.film?.quota },
        series: { allow: overview.default.series?.allow ?? true, autoApprove: overview.default.series?.autoApprove ?? autoApprove, quota: overview.default.series?.quota },
        music: { allow: overview.default.music?.allow ?? true, autoApprove: overview.default.music?.autoApprove ?? autoApprove, quota: overview.default.music?.quota }
      };
    }
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

  async function saveDefaultRules(event: SubmitEvent) {
    event.preventDefault(); busy = true; error = ''; message = '';
    try {
      await saveDefaultPolicy(defaultRule);
      await refresh();
      message = $t('rq_policy_saved');
    } catch (cause) {
      error = cause instanceof Error ? cause.message : $t('saving_failed');
    } finally {
      busy = false;
    }
  }

  function startEditGroup(group: { id: string; name: string; rule: PolicyRule | null }) {
    editingGroup = {
      id: group.id,
      name: group.name,
      rule: group.rule ? JSON.parse(JSON.stringify(group.rule)) : { film: {}, series: {}, music: {} }
    };
  }

  async function saveGroupRule() {
    if (!editingGroup) return;
    busy = true; error = ''; message = '';
    try {
      await saveGroupPolicy(editingGroup.id, editingGroup.rule);
      editingGroup = null;
      await refresh();
      message = $t('rq_policy_saved');
    } catch (cause) {
      error = cause instanceof Error ? cause.message : $t('saving_failed');
    } finally {
      busy = false;
    }
  }

  async function resetGroupRule(id: string) {
    busy = true; error = ''; message = '';
    try {
      await deleteGroupPolicy(id);
      if (editingGroup?.id === id) editingGroup = null;
      await refresh();
      message = $t('rq_policy_saved');
    } catch (cause) {
      error = cause instanceof Error ? cause.message : $t('saving_failed');
    } finally {
      busy = false;
    }
  }

  async function addUserOverride(event: SubmitEvent) {
    event.preventDefault();
    if (!newUserId.trim()) return;
    busy = true; error = ''; message = '';
    try {
      await saveUserPolicy(newUserId.trim(), { film: {}, series: {}, music: {} });
      newUserId = '';
      await refresh();
      message = $t('rq_policy_saved');
    } catch (cause) {
      error = cause instanceof Error ? cause.message : $t('saving_failed');
    } finally {
      busy = false;
    }
  }

  function startEditUser(user: { id: string; rule: PolicyRule }) {
    editingUser = {
      id: user.id,
      rule: JSON.parse(JSON.stringify(user.rule))
    };
  }

  async function saveUserRule() {
    if (!editingUser) return;
    busy = true; error = ''; message = '';
    try {
      await saveUserPolicy(editingUser.id, editingUser.rule);
      editingUser = null;
      await refresh();
      message = $t('rq_policy_saved');
    } catch (cause) {
      error = cause instanceof Error ? cause.message : $t('saving_failed');
    } finally {
      busy = false;
    }
  }

  async function removeUserRule(id: string) {
    busy = true; error = ''; message = '';
    try {
      await deleteUserPolicy(id);
      if (editingUser?.id === id) editingUser = null;
      await refresh();
      message = $t('rq_policy_saved');
    } catch (cause) {
      error = cause instanceof Error ? cause.message : $t('saving_failed');
    } finally {
      busy = false;
    }
  }

  async function inspectUser(id: string) {
    busy = true; error = '';
    try {
      const data = await loadUserEffectivePolicy(id);
      inspectModal = { id, effective: data.effective };
    } catch (cause) {
      error = cause instanceof Error ? cause.message : $t('loading_impossible');
    } finally {
      busy = false;
    }
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

      <!-- Requests Permissions and Quotas Section -->
      <section class="panel stack">
        <div class="heading compact">
          <div>
            <h2>{$t('rq_policy_title')}</h2>
            <p class="muted">{$t('rq_policy_subtitle')}</p>
          </div>
        </div>

        <!-- Server Default Rules -->
        <form class="stack default-policy-box" onsubmit={saveDefaultRules}>
          <h3>{$t('rq_policy_default_heading')}</h3>
          <p class="muted small">{$t('rq_policy_default_desc')}</p>

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

        <!-- Group Rules -->
        {#if policyOverview?.groups?.length}
          <div class="stack">
            <h3>{$t('rq_policy_groups_heading')}</h3>
            <ul class="records">
              {#each policyOverview.groups as grp (grp.id)}
                <li class="record">
                  <div class="record-header">
                    <div>
                      <h4>{grp.name}</h4>
                      <small class="badge tone-{grp.rule ? 'info' : 'neutral'}">
                        {grp.rule ? $t('rq_policy_group_custom') : $t('rq_policy_group_inherited')}
                      </small>
                    </div>
                    <div class="row">
                      <button disabled={busy} onclick={() => startEditGroup(grp)}>{$t('rq_policy_edit_group')}</button>
                      {#if grp.rule}
                        <button disabled={busy} onclick={() => resetGroupRule(grp.id)}>{$t('rq_policy_reset')}</button>
                      {/if}
                    </div>
                  </div>

                  {#if editingGroup?.id === grp.id}
                    <div class="detail stack">
                      <div class="policy-classes-grid">
                        {#each ['film', 'series', 'music'] as const as mediaClass (mediaClass)}
                          {@const gRule = editingGroup.rule[mediaClass] ?? {}}
                          <div class="policy-card">
                            <h5>{$t(mediaClass === 'film' ? 'kind_movie' : mediaClass === 'series' ? 'kind_series' : 'audio')}</h5>
                            <label class="check">
                              <input type="checkbox" checked={gRule.allow ?? true} onchange={e => gRule.allow = (e.target as HTMLInputElement).checked} />
                              <span>{$t('rq_policy_allow')}</span>
                            </label>
                            <label>
                              <span>{$t('rq_policy_auto')}</span>
                              <select value={gRule.autoApprove ?? false} onchange={e => gRule.autoApprove = (e.target as HTMLSelectElement).value === 'true'}>
                                <option value={false}>{$t('rq_policy_auto_review')}</option>
                                <option value={true}>{$t('rq_policy_auto_direct')}</option>
                              </select>
                            </label>
                            <div class="quota-field">
                              <span class="small muted">{$t('rq_policy_quota')}</span>
                              <div class="row">
                                <input type="number" min="0" max="1000" placeholder={$t('rq_policy_quota_none')}
                                  value={gRule.quota?.limit ?? ''}
                                  oninput={e => {
                                    const val = Number((e.target as HTMLInputElement).value);
                                    if (!val) gRule.quota = undefined;
                                    else gRule.quota = { limit: val, days: gRule.quota?.days ?? 7 };
                                  }}
                                />
                                {#if gRule.quota?.limit}
                                  <input type="number" min="1" max="365"
                                    value={gRule.quota.days}
                                    oninput={e => {
                                      if (gRule.quota) gRule.quota.days = Math.max(1, Number((e.target as HTMLInputElement).value) || 7);
                                    }}
                                  />
                                {/if}
                              </div>
                            </div>
                          </div>
                        {/each}
                      </div>
                      <div class="row">
                        <button class="primary" disabled={busy} onclick={saveGroupRule}>{$t('save')}</button>
                        <button disabled={busy} onclick={() => editingGroup = null}>{$t('cancel')}</button>
                      </div>
                    </div>
                  {/if}
                </li>
              {/each}
            </ul>
          </div>
        {/if}

        <!-- User Overrides -->
        <div class="stack">
          <h3>{$t('rq_policy_users_heading')}</h3>
          <form class="row" onsubmit={addUserOverride}>
            <label class="grow">
              <span class="sr-only">{$t('rq_policy_user_id')}</span>
              <input required placeholder={$t('rq_policy_add_user_override')} bind:value={newUserId} />
            </label>
            <button class="primary" disabled={busy || !newUserId.trim()}>{$t('add')}</button>
          </form>

          {#if policyOverview?.users?.length}
            <ul class="records">
              {#each policyOverview.users as usr (usr.id)}
                <li class="record">
                  <div class="record-header">
                    <div>
                      <h4>{usr.label ?? usr.id}</h4>
                      <small class="muted">{usr.id}</small>
                    </div>
                    <div class="row">
                      <button disabled={busy} onclick={() => inspectUser(usr.id)}>{$t('rq_policy_effective_btn')}</button>
                      <button disabled={busy} onclick={() => startEditUser(usr)}>{$t('edit')}</button>
                      <button class="danger" disabled={busy} onclick={() => removeUserRule(usr.id)}>{$t('remove')}</button>
                    </div>
                  </div>

                  {#if editingUser?.id === usr.id}
                    <div class="detail stack">
                      <div class="policy-classes-grid">
                        {#each ['film', 'series', 'music'] as const as mediaClass (mediaClass)}
                          {@const uRule = editingUser.rule[mediaClass] ?? {}}
                          <div class="policy-card">
                            <h5>{$t(mediaClass === 'film' ? 'kind_movie' : mediaClass === 'series' ? 'kind_series' : 'audio')}</h5>
                            <label class="check">
                              <input type="checkbox" checked={uRule.allow ?? true} onchange={e => uRule.allow = (e.target as HTMLInputElement).checked} />
                              <span>{$t('rq_policy_allow')}</span>
                            </label>
                            <label>
                              <span>{$t('rq_policy_auto')}</span>
                              <select value={uRule.autoApprove ?? false} onchange={e => uRule.autoApprove = (e.target as HTMLSelectElement).value === 'true'}>
                                <option value={false}>{$t('rq_policy_auto_review')}</option>
                                <option value={true}>{$t('rq_policy_auto_direct')}</option>
                              </select>
                            </label>
                            <div class="quota-field">
                              <span class="small muted">{$t('rq_policy_quota')}</span>
                              <div class="row">
                                <input type="number" min="0" max="1000" placeholder={$t('rq_policy_quota_none')}
                                  value={uRule.quota?.limit ?? ''}
                                  oninput={e => {
                                    const val = Number((e.target as HTMLInputElement).value);
                                    if (!val) uRule.quota = undefined;
                                    else uRule.quota = { limit: val, days: uRule.quota?.days ?? 7 };
                                  }}
                                />
                                {#if uRule.quota?.limit}
                                  <input type="number" min="1" max="365"
                                    value={uRule.quota.days}
                                    oninput={e => {
                                      if (uRule.quota) uRule.quota.days = Math.max(1, Number((e.target as HTMLInputElement).value) || 7);
                                    }}
                                  />
                                {/if}
                              </div>
                            </div>
                          </div>
                        {/each}
                      </div>
                      <div class="row">
                        <button class="primary" disabled={busy} onclick={saveUserRule}>{$t('save')}</button>
                        <button disabled={busy} onclick={() => editingUser = null}>{$t('cancel')}</button>
                      </div>
                    </div>
                  {/if}
                </li>
              {/each}
            </ul>
          {/if}
        </div>
      </section>
      {/if}
    </div>
  {/if}
</div>

{#if inspectModal}
  <dialog class="application-dialog" open onclose={() => inspectModal = null}>
    <div class="stack">
      <header>
        <h2>{$t('rq_policy_effective_modal_title')}</h2>
        <p class="muted small">{inspectModal.id}</p>
      </header>
      {#if inspectModal.effective.restricted}
        <p class="feedback error">{$t('rq_policy_user_restricted')}</p>
      {/if}
      <div class="policy-classes-grid">
        {#each ['film', 'series', 'music'] as const as mediaClass (mediaClass)}
          {@const eff = inspectModal.effective.classes[mediaClass]}
          <div class="policy-card">
            <h4>{$t(mediaClass === 'film' ? 'kind_movie' : mediaClass === 'series' ? 'kind_series' : 'audio')}</h4>
            <div class="badges">
              <span class="badge tone-{eff.allow ? 'ok' : 'err'}">
                {eff.allow ? $t('rq_policy_badge_allowed') : $t('rq_policy_badge_denied')}
              </span>
              <span class="badge tone-{eff.autoApprove ? 'ok' : 'info'}">
                {eff.autoApprove ? $t('rq_policy_badge_auto') : $t('rq_policy_badge_review')}
              </span>
            </div>
            {#if eff.quota}
              <p class="muted small">
                {$t('rq_quota_remaining_label', { remaining: eff.quota.remaining, limit: eff.quota.limit, days: eff.quota.days })}
              </p>
            {:else}
              <p class="muted small">{$t('rq_policy_quota_none')}</p>
            {/if}
          </div>
        {/each}
      </div>
      <div class="row end">
        <button class="primary" onclick={() => inspectModal = null}>{$t('close')}</button>
      </div>
    </div>
  </dialog>
{/if}
