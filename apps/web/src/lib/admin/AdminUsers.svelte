<script lang="ts">
  import ParentalPolicy from './ParentalPolicy.svelte';
  import UserMediaPolicy from './UserMediaPolicy.svelte';
  import type { Library } from './types';
  import { session, readSession, type AuthUser } from '../auth/session';
  import Icon from '../ui/Icon.svelte';
  import { modalFocus } from './modal-focus';
  import { locale, t } from '../i18n';
  import { activityLabel as formatActivity, filterAndSortUsers, groupMembershipUpdates, userMutationErrorMessage } from './user-list';

  interface Props {
    users: Account[];
    libraries: Library[];
    currentUserId?: string;
    groups?: { id: string; name: string; isDefault?: boolean; userIds: string[]; libraryIds: string[] }[];
    onchanged?: () => void | Promise<void>;
  }

  const {
    users,
    libraries,
    currentUserId,
    groups = [],
    onchanged
  }: Props = $props();

  type Account = AuthUser & { disabledAt?: number | null; lastSeenAt?: number | null; hasAvatar?: boolean };

  let failedAvatars = $state<string[]>([]);
  let accounts = $state<Account[] | null>(null);
  const allUsers = $derived(accounts ?? users);

  let selectedUserIds = $state<string[]>([]);
  let editing = $state<Account | null>(null);
  let editorOpen = $state(false);
  let activeTab = $state<'info' | 'groups' | 'permissions' | 'parental' | 'media' | 'password'>('info');

  let busy = $state(false);
  let message = $state('');
  let failed = $state(false);

  // Form states
  let login = $state('');
  let displayName = $state('');
  let role = $state<'administrator' | 'user'>('user');
  let disabled = $state(false);
  let password = $state('');
  let newPassword = $state('');
  let libraryIds = $state<string[]>([]);
  let assignedGroupIds = $state<string[]>([]);

  // Search & Filter
  let search = $state('');
  let roleFilter = $state('all');
  let statusFilter = $state('all');
  let sortField = $state<'name' | 'role' | 'status' | 'lastSeen'>('name');
  let sortAsc = $state(true);

  function activityLabel(timestamp: number | null | undefined) {
    return formatActivity(timestamp, $locale);
  }

  async function refresh() {
    const response = await fetch('/api/v1/admin/users', { credentials: 'same-origin', cache: 'no-store', signal: AbortSignal.timeout(10000) });
    if (!response.ok) throw new Error('Chargement des comptes impossible.');
    accounts = (await response.json()).users;
    failedAvatars = [];
  }

  function edit(user: Account | null) {
    editorOpen = true;
    activeTab = 'info';
    editing = user;
    login = user?.login ?? '';
    displayName = user?.displayName ?? '';
    role = user?.role ?? 'user';
    disabled = user?.disabledAt != null;
    password = '';
    newPassword = '';
    libraryIds = user ? libraries.filter(l => l.userIds.includes(user.id)).map(l => l.id) : [];
    assignedGroupIds = user ? groups.filter(g => !g.isDefault && g.userIds.includes(user.id)).map(g => g.id) : [];
    message = '';
    failed = false;
  }

  async function mutate(path: string, method: string, body: object) {
    if (!$session) throw new Error('Session expirée. Reconnectez-vous.');
    const response = await fetch(path, {
      method,
      credentials: 'same-origin',
      cache: 'no-store',
      headers: { 'content-type': 'application/json', 'x-csrf-token': $session.csrfToken },
      body: JSON.stringify(body),
      signal: AbortSignal.timeout(15000)
    });
    if (!response.ok) {
      const code = (await response.json().catch(() => ({}))).error;
      throw new Error(userMutationErrorMessage(code));
    }
  }

  async function syncUserGroups(userId: string, targetGroupIds: string[]) {
    if (!$session) return;
    const updatePromises = groupMembershipUpdates(groups, userId, targetGroupIds).map(({ group, userIds }) =>
      fetch(`/api/v1/admin/groups/${encodeURIComponent(group.id)}`, {
        method: 'PUT',
        credentials: 'same-origin',
        cache: 'no-store',
        headers: { 'content-type': 'application/json', 'x-csrf-token': $session.csrfToken },
        body: JSON.stringify({
          name: group.name,
          isDefault: group.isDefault,
          userIds,
          libraryIds: group.libraryIds
        })
      })
    );
    await Promise.all(updatePromises);
  }

  async function save(resetPassword = false) {
    busy = true;
    message = '';
    failed = false;
    try {
      if (resetPassword && editing) {
        await mutate(`/api/v1/admin/users/${encodeURIComponent(editing.id)}/password`, 'PUT', { password: newPassword });
        newPassword = '';
        message = 'Mot de passe remplacé. Toutes les sessions du compte ont été révoquées.';
        if (editing.id === currentUserId) {
          await readSession();
          return;
        }
      } else if (editing) {
        await mutate(`/api/v1/admin/users/${encodeURIComponent(editing.id)}`, 'PUT', {
          role,
          disabled,
          displayName: displayName.trim() || null,
          libraryIds: role === 'administrator' ? [] : libraryIds
        });
        await syncUserGroups(editing.id, assignedGroupIds);
        message = 'Compte enregistré avec succès.';
      } else {
        await mutate('/api/v1/admin/users', 'POST', {
          login,
          password,
          role,
          displayName: displayName.trim() || null,
          libraryIds: role === 'administrator' ? [] : libraryIds
        });
        password = '';
        message = 'Compte créé avec succès.';
      }
      await refresh();
      await onchanged?.();
      editorOpen = false;
      editing = null;
    } catch (err) {
      failed = true;
      message = err instanceof Error ? err.message : 'Opération impossible.';
    } finally {
      busy = false;
    }
  }

  async function toggleDisableSelected(disableTarget: boolean) {
    if (selectedUserIds.length === 0 || busy) return;
    busy = true;
    message = '';
    failed = false;
    try {
      for (const id of selectedUserIds) {
        const u = allUsers.find(item => item.id === id);
        if (!u || u.id === currentUserId) continue;
        await mutate(`/api/v1/admin/users/${encodeURIComponent(u.id)}`, 'PUT', {
          role: u.role,
          disabled: disableTarget,
          displayName: u.displayName,
          libraryIds: libraries.filter(l => l.userIds.includes(u.id)).map(l => l.id)
        });
      }
      message = disableTarget ? 'Comptes sélectionnés désactivés.' : 'Comptes sélectionnés activés.';
      selectedUserIds = [];
      await refresh();
      await onchanged?.();
    } catch (err) {
      failed = true;
      message = err instanceof Error ? err.message : 'Erreur lors de la mise à jour des comptes.';
    } finally {
      busy = false;
    }
  }

  const filteredUsers = $derived(
    filterAndSortUsers(allUsers, { search, roleFilter, statusFilter, sortField, sortAsc })
  );

  const allSelected = $derived(filteredUsers.length > 0 && filteredUsers.every(u => selectedUserIds.includes(u.id)));

  function toggleSelectAll() {
    if (allSelected) {
      selectedUserIds = [];
    } else {
      selectedUserIds = filteredUsers.map(u => u.id);
    }
  }

  function toggleSelectUser(id: string) {
    if (selectedUserIds.includes(id)) {
      selectedUserIds = selectedUserIds.filter(item => item !== id);
    } else {
      selectedUserIds = [...selectedUserIds, id];
    }
  }

  function handleSort(field: 'name' | 'role' | 'status' | 'lastSeen') {
    if (sortField === field) {
      sortAsc = !sortAsc;
    } else {
      sortField = field;
      sortAsc = true;
    }
  }
</script>

<section id="users-section" class="dsm-container" aria-label={$t('users_heading')}>

  {#if message}
    <div class="dsm-alert" class:alert-error={failed} class:alert-success={!failed} role="status">
      <Icon name={failed ? 'alert' : 'check'} size={18} />
      <span>{message}</span>
    </div>
  {/if}

  {#if editorOpen}
    <!-- DSM Dialog / Modal Panel -->
    <div class="dsm-modal-backdrop" use:modalFocus={() => { editorOpen = false; }}>
      <div class="dsm-modal" role="dialog" aria-modal="true" aria-labelledby="modal-user-title">
        <div class="dsm-modal-header">
          <div class="modal-title-wrap">
            <Icon name="profile" size={20} />
            <h3 id="modal-user-title">{editing ? (editing.displayName || editing.login) : $t('create_user')}</h3>
          </div>
          <button type="button" class="btn-close" aria-label={$t('close')} onclick={() => { editorOpen = false; }}>
            <Icon name="close" size={18} />
          </button>
        </div>

        {#if editing}
          <div class="dsm-tabs" role="tablist">
            <button type="button" role="tab" class="dsm-tab" class:active={activeTab === 'info'} onclick={() => activeTab = 'info'}>
              <Icon name="info" size={15} /> {$t('information')}
            </button>
            <button type="button" role="tab" class="dsm-tab" class:active={activeTab === 'groups'} onclick={() => activeTab = 'groups'}>
              <Icon name="users" size={15} /> {$t('groups')}
            </button>
            <button type="button" role="tab" class="dsm-tab" class:active={activeTab === 'permissions'} onclick={() => activeTab = 'permissions'}>
              <Icon name="film" size={15} /> {$t('permissions')}
            </button>
            <button type="button" role="tab" class="dsm-tab" class:active={activeTab === 'parental'} onclick={() => activeTab = 'parental'}>
              <Icon name="shield" size={15} /> {$t('parental_control')}
            </button>
            <button type="button" role="tab" class="dsm-tab" class:active={activeTab === 'media'} onclick={() => activeTab = 'media'}>
              <Icon name="settings" size={15} /> {$t('downloads')}
            </button>
            <button type="button" role="tab" class="dsm-tab" class:active={activeTab === 'password'} onclick={() => activeTab = 'password'}>
              <Icon name="settings" size={15} /> {$t('security')}
            </button>
          </div>
        {/if}

        <div class="dsm-modal-body">
          {#if activeTab === 'info' || !editing}
            <div class="dsm-form-section">
              <h4 class="form-section-title">{$t('basic_information')}</h4>
              <div class="dsm-grid-form">
                {#if !editing}
                  <label class="form-row">
                    <span class="row-label">{$t('username')} <abbr title={$t('required')} class="required">*</abbr></span>
                    <div class="row-field">
                      <input required minlength="3" maxlength="64" pattern="[a-z0-9._-]+" bind:value={login} placeholder="ex: jean.dupont" autocomplete="off" />
                      <span class="field-hint">{$t('username_hint')}</span>
                    </div>
                  </label>
                {:else}
                  <div class="form-row">
                    <span class="row-label">{$t('username')}</span>
                    <div class="row-field">
                      <strong class="user-static-login">{editing.login}</strong>
                      <span class="dsm-badge" class:badge-sso={editing.sso} class:badge-local={!editing.sso}>
                        {editing.sso ? $t('sso_provider') : $t('local_account')}
                      </span>
                    </div>
                  </div>
                {/if}

                <label class="form-row">
                  <span class="row-label">{$t('description_display_name')}</span>
                  <div class="row-field">
                    <input maxlength="128" bind:value={displayName} placeholder="ex: Jean Dupont" />
                  </div>
                </label>

                {#if !editing}
                  <label class="form-row">
                    <span class="row-label">{$t('password')} <abbr title={$t('required')} class="required">*</abbr></span>
                    <div class="row-field">
                      <input type="password" required minlength="15" maxlength="128" bind:value={password} autocomplete="new-password" placeholder={$t('password_minimum')} />
                      <span class="field-hint">{$t('password_hint')}</span>
                    </div>
                  </label>
                {/if}

                <label class="form-row">
                  <span class="row-label">{$t('privilege')}</span>
                  <div class="row-field">
                    <select bind:value={role} disabled={editing?.id === currentUserId}>
                      <option value="user">{$t('standard_user')}</option>
                      <option value="administrator">{$t('administrator')}</option>
                    </select>
                    {#if editing?.id === currentUserId}
                      <span class="field-hint">{$t('cannot_change_own_role')}</span>
                    {/if}
                  </div>
                </label>

                {#if editing}
                  <div class="form-row">
                      <span class="row-label">{$t('account_status')}</span>
                    <div class="row-field">
                      <label class="dsm-checkbox-label">
                        <input type="checkbox" bind:checked={disabled} disabled={editing.id === currentUserId} />
                        <span>{$t('disable_account')}</span>
                      </label>
                      <span class="field-hint">{$t('disabled_account_hint')}</span>
                    </div>
                  </div>
                {/if}
              </div>
            </div>
          {:else if editing && activeTab === 'groups'}
            <div class="dsm-form-section">
              <h4 class="form-section-title">{$t('group_membership')}</h4>
              <p class="section-desc">{$t('group_membership_hint')}</p>

              <div class="dsm-table-container">
                <table class="dsm-table">
                  <thead>
                    <tr>
                      <th class="col-check"></th>
                      <th>{$t('group')}</th>
                      <th>{$t('type')}</th>
                      <th>{$t('accessible_libraries')}</th>
                    </tr>
                  </thead>
                  <tbody>
                    {#each groups as group (group.id)}
                      {@const isDefault = !!group.isDefault}
                      <tr>
                        <td>
                          <input
                            type="checkbox"
                            disabled={isDefault}
                            checked={isDefault || assignedGroupIds.includes(group.id)}
                            onchange={(e) => {
                              if (isDefault) return;
                              if (e.currentTarget.checked) {
                                assignedGroupIds = [...assignedGroupIds, group.id];
                              } else {
                                assignedGroupIds = assignedGroupIds.filter(id => id !== group.id);
                              }
                            }}
                          />
                        </td>
                        <td>
                          <strong>{group.name}</strong>
                        </td>
                        <td>
                          {#if isDefault}
                            <span class="dsm-pill pill-default">Automatique (tous)</span>
                          {:else}
                            <span class="dsm-pill">Personnalisé</span>
                          {/if}
                        </td>
                        <td>
                          {group.libraryIds.length} bibliothèque(s)
                        </td>
                      </tr>
                    {:else}
                      <tr>
                        <td colspan="4" class="empty-cell">Aucun groupe configuré</td>
                      </tr>
                    {/each}
                  </tbody>
                </table>
              </div>
            </div>
          {:else if activeTab === 'permissions'}
            <div class="dsm-form-section">
              <h4 class="form-section-title">Permissions sur les bibliothèques</h4>
              {#if role === 'administrator'}
                <div class="dsm-info-box">
                  <Icon name="shield" size={18} />
                  <span>En tant qu'<strong>Administrateur</strong>, ce compte a automatiquement accès à toutes les bibliothèques multimédias.</span>
                </div>
              {:else}
                <p class="section-desc">Cochez les bibliothèques autorisées individuellement pour ce compte :</p>
                <div class="dsm-table-container">
                  <table class="dsm-table">
                    <thead>
                      <tr>
                        <th class="col-check">
                          <input
                            type="checkbox"
                            checked={libraries.length > 0 && libraries.every(l => libraryIds.includes(l.id))}
                            onchange={(e) => libraryIds = e.currentTarget.checked ? libraries.map(l => l.id) : []}
                          />
                        </th>
                        <th>Nom de la bibliothèque</th>
                        <th>Type</th>
                        <th>Statut héritage</th>
                      </tr>
                    </thead>
                    <tbody>
                      {#each libraries as lib (lib.id)}
                        {@const inherited = editing ? groups.filter(g => (g.isDefault || g.userIds.includes(editing?.id ?? '')) && g.libraryIds.includes(lib.id)) : []}
                        <tr>
                          <td>
                            <input
                              type="checkbox"
                              value={lib.id}
                              bind:group={libraryIds}
                            />
                          </td>
                          <td>
                            <strong>{lib.name}</strong>
                            {#if !lib.enabled}<span class="dsm-tag-muted">(Désactivée)</span>{/if}
                          </td>
                          <td>
                            <span class="lib-kind-pill">{lib.kind}</span>
                          </td>
                          <td>
                            {#if inherited.length > 0}
                              <span class="inherited-badge">
                                Inclus par : {inherited.map(g => g.name).join(', ')}
                              </span>
                            {:else}
                              <span class="direct-badge">Droit direct uniquement</span>
                            {/if}
                          </td>
                        </tr>
                      {:else}
                        <tr>
                          <td colspan="4" class="empty-cell">Aucune bibliothèque disponible</td>
                        </tr>
                      {/each}
                    </tbody>
                  </table>
                </div>
              {/if}
            </div>
          {:else if editing && activeTab === 'parental'}
            <ParentalPolicy userId={editing.id} />
          {:else if editing && activeTab === 'media'}
            <UserMediaPolicy userId={editing.id} />
          {:else if editing && activeTab === 'password'}
            <div class="dsm-form-section">
              <h4 class="form-section-title">Sécurité du compte & Mot de passe</h4>
              {#if editing.sso}
                <div class="dsm-info-box">
                  <Icon name="info" size={18} />
                  <span>Ce compte s'authentifie via le fournisseur OpenID Connect. Le mot de passe ne peut pas être modifié localement.</span>
                </div>
              {:else}
                <p class="section-desc">Changer le mot de passe local. Toutes les sessions actives de cet utilisateur seront immédiatement révoquées.</p>
                <div class="dsm-grid-form">
                  <label class="form-row">
                    <span class="row-label">Nouveau mot de passe</span>
                    <div class="row-field">
                      <input type="password" required minlength="15" maxlength="128" bind:value={newPassword} placeholder="15 caractères minimum" />
                      <button type="button" class="btn btn-warning mt-10" disabled={busy || newPassword.length < 15} onclick={() => save(true)}>
                        Appliquer et révoquer les sessions
                      </button>
                    </div>
                  </label>
                </div>
              {/if}
            </div>
          {/if}
        </div>

        <div class="dsm-modal-footer">
          <button type="button" class="btn btn-default" disabled={busy} onclick={() => { editorOpen = false; }}>
            Annuler
          </button>
          {#if activeTab !== 'password'}
            <button type="button" class="btn btn-primary" disabled={busy} onclick={() => save(false)}>
              {busy ? 'Enregistrement…' : 'Appliquer'}
            </button>
          {/if}
        </div>
      </div>
    </div>
  {/if}

  <!-- DSM Action Toolbar -->
  <div class="dsm-toolbar">
    <div class="toolbar-left">
      <button type="button" class="btn btn-primary" disabled={busy} onclick={() => edit(null)}>
        <Icon name="profile" size={16} />
        <span>Créer</span>
      </button>

      <button
        type="button"
        class="btn btn-default"
        disabled={busy || selectedUserIds.length !== 1}
        onclick={() => {
          const u = allUsers.find(item => item.id === selectedUserIds[0]);
          if (u) edit(u);
        }}
      >
        <Icon name="edit" size={16} />
        <span>Modifier</span>
      </button>

      <div class="btn-group">
        <button
          type="button"
          class="btn btn-default"
          disabled={busy || selectedUserIds.length === 0}
          title="Activer les comptes sélectionnés"
          onclick={() => toggleDisableSelected(false)}
        >
          <span>Activer</span>
        </button>
        <button
          type="button"
          class="btn btn-default"
          disabled={busy || selectedUserIds.length === 0}
          title="Désactiver les comptes sélectionnés"
          onclick={() => toggleDisableSelected(true)}
        >
          <span>Désactiver</span>
        </button>
      </div>

      <button type="button" class="btn btn-default" disabled={busy} onclick={() => refresh().catch(() => {})}>
        <Icon name="refresh" size={16} />
        <span>Actualiser</span>
      </button>
    </div>

    <div class="toolbar-right">
      <div class="search-box">
        <Icon name="search" size={16} />
        <input type="search" bind:value={search} placeholder="Rechercher…" aria-label="Rechercher un utilisateur" />
      </div>

      <select bind:value={roleFilter} class="filter-select" aria-label="Filtrer par rôle">
        <option value="all">Tous les rôles</option>
        <option value="administrator">Administrateurs</option>
        <option value="user">Utilisateurs</option>
      </select>

      <select bind:value={statusFilter} class="filter-select" aria-label="Filtrer par statut">
        <option value="all">Tous les statuts</option>
        <option value="active">Actifs</option>
        <option value="disabled">Désactivés</option>
      </select>
    </div>
  </div>

  <!-- DSM Table View -->
  <div class="dsm-table-panel">
    <table class="dsm-table main-table">
      <thead>
        <tr>
          <th class="col-check-center">
            <input type="checkbox" checked={allSelected} onchange={toggleSelectAll} aria-label="Sélectionner tous les utilisateurs" />
          </th>
          <th class="sortable" onclick={() => handleSort('name')}>
            Nom d'utilisateur {sortField === 'name' ? (sortAsc ? '▲' : '▼') : ''}
          </th>
          <th>Description</th>
          <th class="sortable" onclick={() => handleSort('role')}>
            Rôle {sortField === 'role' ? (sortAsc ? '▲' : '▼') : ''}
          </th>
          <th class="sortable" onclick={() => handleSort('status')}>
            Statut {sortField === 'status' ? (sortAsc ? '▲' : '▼') : ''}
          </th>
          <th>Groupes</th>
          <th class="sortable" onclick={() => handleSort('lastSeen')}>
            Dernière activité {sortField === 'lastSeen' ? (sortAsc ? '▲' : '▼') : ''}
          </th>
        </tr>
      </thead>
      <tbody>
        {#each filteredUsers as user (user.id)}
          {@const userGroups = groups.filter(g => g.isDefault || g.userIds.includes(user.id))}
          {@const isSelected = selectedUserIds.includes(user.id)}
          <tr
            class:selected={isSelected}
            class:row-disabled={user.disabledAt != null}
            ondblclick={() => edit(user)}
          >
            <td class="cell-center">
              <input
                type="checkbox"
                checked={isSelected}
                onchange={() => toggleSelectUser(user.id)}
                aria-label={`Sélectionner ${user.login}`}
              />
            </td>
            <td>
              <div class="user-cell">
                <span class="dsm-avatar">
                  {#if user.hasAvatar && !failedAvatars.includes(user.id)}
                    <img src={`/api/v1/admin/users/${encodeURIComponent(user.id)}/avatar`} alt="" width="26" height="26" loading="lazy" onerror={() => failedAvatars = [...failedAvatars, user.id]} />
                  {:else}
                    {(user.displayName || user.login).slice(0, 2).toUpperCase()}
                  {/if}
                </span>
                <span class="user-login-text">
                  {user.login}
                  {#if user.id === currentUserId}
                    <span class="self-pill">{$t('you')}</span>
                  {/if}
                </span>
              </div>
            </td>
            <td>
              <span class="user-display-name">{user.displayName || '—'}</span>
            </td>
            <td>
              {#if user.role === 'administrator'}
                <span class="dsm-pill pill-admin">Administrateur</span>
              {:else}
                <span class="dsm-pill pill-user">Utilisateur</span>
              {/if}
            </td>
            <td>
              {#if user.disabledAt != null}
                <span class="status-indicator status-off">
                  <span class="dot"></span> Désactivé
                </span>
              {:else}
                <span class="status-indicator status-on">
                  <span class="dot"></span> Normal
                </span>
              {/if}
            </td>
            <td>
              <div class="groups-tag-list">
                {#each userGroups as g (g.id)}
                  <span class="group-mini-tag" class:default-tag={g.isDefault}>{g.name}</span>
                {:else}
                  <span class="text-muted">—</span>
                {/each}
              </div>
            </td>
            <td class="text-muted">
              {activityLabel(user.lastSeenAt)}
            </td>
          </tr>
        {:else}
          <tr>
            <td colspan="7" class="empty-cell">
              Aucun utilisateur ne correspond aux critères de recherche.
            </td>
          </tr>
        {/each}
      </tbody>
    </table>
  </div>

  <div class="dsm-statusbar">
    <span>Total : <strong>{filteredUsers.length}</strong> utilisateur(s) sur {allUsers.length}</span>
    <span>{selectedUserIds.length} sélectionné(s)</span>
  </div>
</section>

<style>
.dsm-container { display: flex; flex-direction: column; gap: 16px; margin-bottom: 40px; font-family: var(--font-sans); color: var(--color-text); }

  /* En-tête de section */

  /* Barre d'outils */
  .dsm-toolbar { display: flex; align-items: center; justify-content: space-between; gap: 12px; flex-wrap: wrap; }
  .toolbar-left, .toolbar-right { display: flex; align-items: center; gap: 8px; flex-wrap: wrap; min-width: 0; }

  /* Boutons */
  .btn {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    gap: 8px;
    height: 44px;
    padding: 0 16px;
    border: 0;
    border-radius: var(--radius-md);
    background: transparent;
    color: var(--color-text);
    font: inherit;
    font-size: 14px;
    font-weight: 700;
    white-space: nowrap;
    cursor: pointer;
    transition: background-color var(--transition-fast), opacity var(--transition-fast), transform var(--transition-fast);
  }
  .btn:active:not(:disabled) { transform: scale(0.98); }
  .btn:focus-visible, .btn-close:focus-visible, .dsm-tab:focus-visible, .sortable:focus-visible { outline: 2px solid var(--color-accent); outline-offset: 3px; }
  .btn-primary { background: var(--color-accent); color: var(--color-on-accent); }
  .btn-primary:hover:not(:disabled) { background: var(--color-accent-hover); }
  .btn-default { background: transparent; border: 1px solid var(--color-line); color: var(--color-text); }
  .btn-default:hover:not(:disabled) { background: var(--color-raised); }
  .btn-warning { background: transparent; border: 1px solid var(--color-line); color: var(--color-danger); }
  .btn-warning:hover:not(:disabled) { background: var(--color-danger-soft); }
  .btn:disabled { opacity: 0.45; cursor: not-allowed; }
  .btn-group { display: inline-flex; gap: 8px; }

  /* Recherche et filtres */
  .search-box {
    display: flex;
    align-items: center;
    gap: 8px;
    height: 44px;
    padding: 0 14px;
    border: 1px solid transparent;
    border-radius: var(--radius-sm);
    background: var(--color-raised);
    color: var(--color-muted);
    transition: border-color var(--transition-fast), background-color var(--transition-fast);
  }
  .search-box:focus-within { border-color: var(--color-accent); background: var(--color-surface-elevated); }
  .search-box input { width: 180px; min-width: 0; height: 100%; min-height: 0; box-shadow: none; padding: 0; border: 0; background: transparent; color: var(--color-text); font: inherit; font-size: 14px; outline: none; }
  .search-box input::placeholder { color: var(--color-subtle); }
  .filter-select {
    width: auto;
    min-width: 0;
    height: 44px;
    padding: 0 36px 0 14px;
    border: 1px solid transparent;
    border-radius: var(--radius-sm);
    background-color: var(--color-raised);
    color: var(--color-text);
    font: inherit;
    font-size: 14px;
    font-weight: 600;
    cursor: pointer;
    transition: border-color var(--transition-fast), background-color var(--transition-fast);
  }
  .filter-select:hover { background-color: var(--color-raised-hover); }
  .filter-select:focus { outline: none; border-color: var(--color-accent); background-color: var(--color-surface-elevated); }

  /* Tableaux */
  .dsm-table-panel { overflow-x: auto; padding: 4px 16px; border-radius: var(--radius-md); background: var(--color-surface); }
  .dsm-table-container { overflow-x: auto; border-radius: var(--radius-md); background: var(--color-surface); }
  .dsm-table { width: 100%; border-collapse: collapse; font-size: 14px; text-align: left; }
  .main-table { min-width: 860px; }
  .dsm-table th {
    height: 48px;
    padding: 0 16px;
    border-bottom: 1px solid var(--color-line-subtle);
    background: transparent;
    color: var(--color-muted);
    font-size: 12px;
    font-weight: 500;
    white-space: nowrap;
    user-select: none;
  }
  .dsm-table th.sortable { cursor: pointer; transition: color var(--transition-fast); }
  .dsm-table th.sortable:hover { color: var(--color-text); }
  .dsm-table td { height: 60px; padding: 8px 16px; border-bottom: 1px solid var(--color-line-subtle); vertical-align: middle; }
  .dsm-table tbody tr:last-child td { border-bottom: 0; }
  .dsm-table tbody tr { transition: background-color var(--transition-fast); }
  .dsm-table tbody tr:hover { background: var(--color-surface-hover); }
  .dsm-table tbody tr.selected { background: var(--color-accent-soft); }
  .dsm-table tbody tr.row-disabled { color: var(--color-muted); }
  .dsm-table input[type='checkbox'] { width: 20px; height: 20px; margin: 0; accent-color: var(--color-accent); cursor: pointer; vertical-align: middle; }
  .dsm-table input[type='checkbox']:disabled { cursor: not-allowed; opacity: 0.45; }

  .user-cell { display: flex; align-items: center; gap: 12px; }
  .dsm-avatar { display: grid; place-items: center; flex: none; width: 36px; height: 36px; overflow: hidden; border-radius: 50%; background: var(--color-raised-hover); color: var(--color-text); font-size: 12px; font-weight: 700; }
  .dsm-avatar img { width: 100%; height: 100%; object-fit: cover; }
  .user-login-text { display: inline-flex; align-items: center; gap: 8px; font-weight: 700; }
  .user-display-name { color: var(--color-muted); }

  /* Badges */
  .self-pill, .dsm-pill, .dsm-badge, .lib-kind-pill, .group-mini-tag {
    display: inline-flex;
    align-items: center;
    height: 24px;
    padding: 0 9px;
    border-radius: var(--radius-xs);
    background: var(--color-raised-hover);
    color: var(--color-text);
    font-size: 12px;
    font-weight: 700;
    white-space: nowrap;
  }
  .self-pill { background: var(--color-accent-soft); color: var(--color-accent); }
  .pill-admin { background: var(--color-accent-soft); color: var(--color-accent); }
  .pill-user { background: var(--color-raised-hover); color: var(--color-muted); }
  .pill-default { background: var(--color-raised); color: var(--color-info); }
  .dsm-badge { margin-left: 8px; }
  .badge-local { background: var(--color-raised-hover); color: var(--color-muted); }
  .badge-sso { background: var(--color-raised); color: var(--color-info); }
  .lib-kind-pill { text-transform: capitalize; }
  .groups-tag-list { display: flex; flex-wrap: wrap; gap: 6px; }
  .group-mini-tag { font-weight: 600; }
  .default-tag { background: var(--color-raised); color: var(--color-info); }

  .status-indicator { display: inline-flex; align-items: center; gap: 8px; font-size: 14px; font-weight: 600; }
  .status-indicator .dot { width: 8px; height: 8px; border-radius: 50%; }
  .status-on .dot { background: var(--color-success); }
  .status-off .dot { background: var(--color-danger); }

  .text-muted { color: var(--color-muted); font-size: 13px; }
  .dsm-tag-muted { margin-left: 6px; color: var(--color-muted); font-size: 13px; }
  .empty-cell { height: auto; padding: 40px 16px; color: var(--color-muted); text-align: center; }
  .dsm-statusbar { display: flex; align-items: center; justify-content: space-between; gap: 12px; flex-wrap: wrap; padding: 0 4px; color: var(--color-muted); font-size: 13px; }
  .dsm-statusbar strong { color: var(--color-text); }

  /* Dialogue d'édition */
  .dsm-modal-backdrop {
    position: fixed;
    inset: 0;
    z-index: 1000;
    display: grid;
    place-items: center;
    padding: 24px;
    background: color-mix(in srgb, var(--color-canvas) 70%, transparent);
    backdrop-filter: blur(4px);
    -webkit-backdrop-filter: blur(4px);
  }
  .dsm-modal {
    display: flex;
    flex-direction: column;
    width: 100%;
    max-width: 760px;
    max-height: min(88vh, 900px);
    overflow: hidden;
    border-radius: var(--radius-lg);
    background: var(--color-surface);
    color: var(--color-text);
    box-shadow: var(--shadow-sm);
  }
  .dsm-modal-header { display: flex; align-items: center; justify-content: space-between; gap: 16px; padding: 20px 20px 12px 28px; }
  .modal-title-wrap { display: flex; align-items: center; gap: 12px; min-width: 0; color: var(--color-muted); }
  .modal-title-wrap h3 { margin: 0; color: var(--color-text); font-size: 20px; font-weight: 700; letter-spacing: -0.02em; overflow-wrap: anywhere; }
  .btn-close { display: grid; place-items: center; flex: none; width: 44px; height: 44px; padding: 0; border: 0; border-radius: var(--radius-md); background: transparent; color: var(--color-text); cursor: pointer; transition: background-color var(--transition-fast); }
  .btn-close:hover { background: var(--color-raised); }

  .dsm-tabs { display: flex; gap: 20px; margin: 0 28px; border-bottom: 1px solid var(--color-line-subtle); overflow-x: auto; scrollbar-width: none; }
  .dsm-tab {
    display: inline-flex;
    align-items: center;
    gap: 8px;
    height: 46px;
    margin-bottom: -1px;
    padding: 0;
    border: 0;
    border-bottom: 2px solid transparent;
    background: transparent;
    color: var(--color-muted);
    font: inherit;
    font-size: 14px;
    font-weight: 700;
    white-space: nowrap;
    cursor: pointer;
    transition: color var(--transition-fast);
  }
  .dsm-tab:hover { color: var(--color-text); }
  .dsm-tab.active { color: var(--color-text); border-bottom-color: var(--color-accent); }

  .dsm-modal-body { flex: 1; min-height: 0; padding: 24px 28px; overflow-y: auto; }
  .form-section-title { margin: 0 0 16px; font-size: 17px; font-weight: 700; }
  .section-desc { margin: -8px 0 16px; color: var(--color-muted); font-size: 13px; line-height: 1.5; }

  .dsm-grid-form { display: grid; }
  .form-row { display: grid; grid-template-columns: 200px minmax(0, 1fr); align-items: start; gap: 16px; padding: 16px 0; border-bottom: 1px solid var(--color-line-subtle); font-size: 14px; }
  .form-row:first-child { padding-top: 0; }
  .form-row:last-child { border-bottom: 0; padding-bottom: 0; }
  .row-label { padding-top: 12px; color: var(--color-text); font-weight: 700; }
  .required { color: var(--color-danger); text-decoration: none; }
  .row-field { display: flex; flex-direction: column; align-items: flex-start; gap: 8px; min-width: 0; }
  .row-field input:not([type='checkbox']), .row-field select {
    box-sizing: border-box;
    width: 100%;
    height: 46px;
    padding: 0 14px;
    border: 1px solid var(--color-line-subtle);
    border-radius: var(--radius-sm);
    background-color: var(--color-canvas);
    color: var(--color-text);
    font: inherit;
    font-size: 14px;
    font-weight: 500;
    box-shadow: none;
    transition: border-color var(--transition-fast), background-color var(--transition-fast);
  }
  .row-field select { padding-right: 40px; }
  .row-field input:not([type='checkbox']):hover, .row-field select:hover { background-color: var(--color-raised-hover); }
  .row-field input:not([type='checkbox']):focus, .row-field select:focus { outline: none; border-color: var(--color-accent); background-color: var(--color-surface-elevated); }
  .row-field input:disabled, .row-field select:disabled { opacity: 0.45; cursor: not-allowed; }
  .user-static-login { display: inline-flex; align-items: center; min-height: 46px; font-weight: 700; }
  .row-field:has(.user-static-login) { flex-direction: row; flex-wrap: wrap; align-items: center; }
  .field-hint { color: var(--color-muted); font-size: 13px; line-height: 1.5; }
  .dsm-checkbox-label { display: flex; align-items: center; gap: 12px; min-height: 46px; font-weight: 600; cursor: pointer; }
  .dsm-checkbox-label input { width: 20px; height: 20px; margin: 0; accent-color: var(--color-accent); }

  .dsm-info-box { display: flex; align-items: flex-start; gap: 12px; padding: 14px 16px; border-radius: var(--radius-md); background: var(--color-raised); color: var(--color-text); font-size: 14px; line-height: 1.5; }
  .dsm-info-box :global(svg) { flex: none; margin-top: 1px; color: var(--color-info); }

  .inherited-badge { color: var(--color-text); font-size: 13px; font-weight: 600; }
  .direct-badge { color: var(--color-muted); font-size: 13px; }

  .dsm-modal-footer { display: flex; align-items: center; justify-content: flex-end; gap: 8px; padding: 16px 28px 24px; border-top: 1px solid var(--color-line-subtle); }

  /* Messages */
  .dsm-alert { display: flex; align-items: center; gap: 12px; min-height: 52px; padding: 12px 16px; border-radius: var(--radius-md); color: var(--color-text); font-size: 14px; }
  .alert-success { background: var(--color-success-soft); }
  .alert-success :global(svg) { color: var(--color-success); }
  .alert-error { background: var(--color-danger-soft); }
  .alert-error :global(svg) { color: var(--color-danger); }

  .col-check { width: 52px; }
  .col-check-center { width: 52px; text-align: center; }
  .cell-center { text-align: center; }
  .mt-10 { margin-top: 4px; }

  @media (max-width: 820px) {
    .dsm-toolbar { align-items: stretch; flex-direction: column; }
    .toolbar-right { width: 100%; }
    .search-box { flex: 1 1 100%; }
    .search-box input { width: 100%; }
    .filter-select { flex: 1 1 calc(50% - 4px); }
  }
  @media (max-width: 640px) {
    .toolbar-left .btn { flex: 1 1 auto; }
    .btn-group { flex: 1 1 100%; }
    .btn-group .btn { flex: 1; }
    .dsm-modal-backdrop { place-items: end stretch; padding: 0; }
    .dsm-modal { max-width: none; max-height: 92dvh; border-radius: var(--radius-lg) var(--radius-lg) 0 0; }
    .dsm-modal-header { padding: 16px 12px 8px 16px; }
    .dsm-tabs { margin: 0 16px; }
    .dsm-modal-body { padding: 20px 16px; }
    .dsm-modal-footer { padding: 12px 16px calc(16px + env(safe-area-inset-bottom, 0px)); }
    .dsm-modal-footer .btn { flex: 1; }
    .form-row { grid-template-columns: minmax(0, 1fr); gap: 8px; }
    .row-label { padding-top: 0; }
  }
  @media (prefers-reduced-motion: reduce) {
    .btn, .dsm-tab, .btn-close, .dsm-table tbody tr, .search-box, .filter-select { transition: none; }
  }
</style>
