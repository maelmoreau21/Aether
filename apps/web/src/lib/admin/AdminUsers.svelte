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

<section id="users-section" class="dsm-container" aria-labelledby="users-heading">
  <!-- DSM Header -->
  <div class="dsm-header">
    <div class="dsm-title-group">
      <div class="dsm-icon-badge">
        <Icon name="users" size={22} />
      </div>
      <div>
        <h2 id="users-heading">{$t('users_heading')}</h2>
        <p class="dsm-subtitle">Gérez les comptes d'accès locaux et attribuez leurs privilèges de médiathèque.</p>
      </div>
    </div>
  </div>

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
            <h3 id="modal-user-title">{editing ? (editing.displayName || editing.login) : 'Créer un utilisateur'}</h3>
          </div>
          <button type="button" class="btn-close" aria-label="Fermer" onclick={() => { editorOpen = false; }}>
            <Icon name="close" size={18} />
          </button>
        </div>

        {#if editing}
          <div class="dsm-tabs" role="tablist">
            <button type="button" role="tab" class="dsm-tab" class:active={activeTab === 'info'} onclick={() => activeTab = 'info'}>
              <Icon name="info" size={15} /> Informations
            </button>
            <button type="button" role="tab" class="dsm-tab" class:active={activeTab === 'groups'} onclick={() => activeTab = 'groups'}>
              <Icon name="users" size={15} /> Groupes
            </button>
            <button type="button" role="tab" class="dsm-tab" class:active={activeTab === 'permissions'} onclick={() => activeTab = 'permissions'}>
              <Icon name="film" size={15} /> Permissions
            </button>
            <button type="button" role="tab" class="dsm-tab" class:active={activeTab === 'parental'} onclick={() => activeTab = 'parental'}>
              <Icon name="shield" size={15} /> Contrôle parental
            </button>
            <button type="button" role="tab" class="dsm-tab" class:active={activeTab === 'media'} onclick={() => activeTab = 'media'}>
              <Icon name="settings" size={15} /> Téléchargement
            </button>
            <button type="button" role="tab" class="dsm-tab" class:active={activeTab === 'password'} onclick={() => activeTab = 'password'}>
              <Icon name="settings" size={15} /> Sécurité
            </button>
          </div>
        {/if}

        <div class="dsm-modal-body">
          {#if activeTab === 'info' || !editing}
            <div class="dsm-form-section">
              <h4 class="form-section-title">Informations de base</h4>
              <div class="dsm-grid-form">
                {#if !editing}
                  <label class="form-row">
                    <span class="row-label">Nom d'utilisateur <abbr title="requis" class="required">*</abbr></span>
                    <div class="row-field">
                      <input required minlength="3" maxlength="64" pattern="[a-z0-9._-]+" bind:value={login} placeholder="ex: jean.dupont" autocomplete="off" />
                      <span class="field-hint">3 à 64 lettres minuscules, chiffres, tirets ou points.</span>
                    </div>
                  </label>
                {:else}
                  <div class="form-row">
                    <span class="row-label">Nom d'utilisateur</span>
                    <div class="row-field">
                      <strong class="user-static-login">{editing.login}</strong>
                      <span class="dsm-badge" class:badge-sso={editing.sso} class:badge-local={!editing.sso}>
                        {editing.sso ? 'Fournisseur SSO' : 'Compte local'}
                      </span>
                    </div>
                  </div>
                {/if}

                <label class="form-row">
                  <span class="row-label">Description / Nom affiché</span>
                  <div class="row-field">
                    <input maxlength="128" bind:value={displayName} placeholder="ex: Jean Dupont" />
                  </div>
                </label>

                {#if !editing}
                  <label class="form-row">
                    <span class="row-label">Mot de passe <abbr title="requis" class="required">*</abbr></span>
                    <div class="row-field">
                      <input type="password" required minlength="15" maxlength="128" bind:value={password} autocomplete="new-password" placeholder="Minimum 15 caractères" />
                      <span class="field-hint">Algorithme robuste Argon2id (15 à 128 caractères).</span>
                    </div>
                  </label>
                {/if}

                <label class="form-row">
                  <span class="row-label">Privilège</span>
                  <div class="row-field">
                    <select bind:value={role} disabled={editing?.id === currentUserId}>
                      <option value="user">Utilisateur standard</option>
                      <option value="administrator">Administrateur</option>
                    </select>
                    {#if editing?.id === currentUserId}
                      <span class="field-hint">Vous ne pouvez pas modifier votre propre rôle.</span>
                    {/if}
                  </div>
                </label>

                {#if editing}
                  <div class="form-row">
                    <span class="row-label">Statut du compte</span>
                    <div class="row-field">
                      <label class="dsm-checkbox-label">
                        <input type="checkbox" bind:checked={disabled} disabled={editing.id === currentUserId} />
                        <span>Désactiver ce compte</span>
                      </label>
                      <span class="field-hint">Un compte désactivé conserve ses préférences mais ne peut plus se connecter.</span>
                    </div>
                  </div>
                {/if}
              </div>
            </div>
          {:else if editing && activeTab === 'groups'}
            <div class="dsm-form-section">
              <h4 class="form-section-title">Appartenance aux groupes</h4>
              <p class="section-desc">Sélectionnez les groupes d'utilisateurs auxquels ce compte doit être affecté :</p>

              <div class="dsm-table-container">
                <table class="dsm-table">
                  <thead>
                    <tr>
                      <th class="col-check"></th>
                      <th>Groupe</th>
                      <th>Type</th>
                      <th>Bibliothèques accessibles</th>
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
        <input type="search" bind:value={search} placeholder="Rechercher…" />
      </div>

      <select bind:value={roleFilter} class="filter-select" aria-label="Filtrer par rôle">
        <option value="all">Tous les rôles</option>
        <option value="administrator">Administrateurs</option>
        <option value="user">Utilisateurs standard</option>
      </select>

      <select bind:value={statusFilter} class="filter-select" aria-label="Filtrer par statut">
        <option value="all">Tous les statuts</option>
        <option value="active">Actifs uniquement</option>
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
.dsm-container { display: flex; flex-direction: column; gap: 16px; margin-bottom: 40px; font-family: var(--font); color: var(--tx); }

  /* En-tête de section */
  .dsm-header { display: flex; align-items: center; justify-content: space-between; gap: 16px; }
  .dsm-title-group { display: flex; align-items: center; gap: 14px; min-width: 0; }
  .dsm-icon-badge { display: grid; place-items: center; flex: none; width: 44px; height: 44px; border-radius: 14px; background: var(--pn); color: var(--tx); }
  h2 { margin: 0; font-size: 24px; font-weight: 800; letter-spacing: -0.02em; line-height: 1.2; }
  .dsm-subtitle { margin: 4px 0 0; font-size: 14px; line-height: 1.5; color: var(--mu); }

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
    border-radius: var(--r-md);
    background: transparent;
    color: var(--tx);
    font: inherit;
    font-size: 14px;
    font-weight: 700;
    white-space: nowrap;
    cursor: pointer;
    transition: background-color var(--t-fast) var(--ease), opacity var(--t-fast) var(--ease), transform var(--t-fast) var(--ease);
  }
  .btn:active:not(:disabled) { transform: scale(0.98); }
  .btn:focus-visible, .btn-close:focus-visible, .dsm-tab:focus-visible, .sortable:focus-visible { outline: 2px solid var(--ac); outline-offset: 3px; }
  .btn-primary { background: var(--btn); color: var(--btn-tx); }
  .btn-primary:hover:not(:disabled) { opacity: 0.9; }
  .btn-default { background: var(--pn-2); color: var(--tx); }
  .btn-default:hover:not(:disabled) { background: var(--pn-3); }
  .btn-warning { background: var(--err-soft); color: var(--err); }
  .btn-warning:hover:not(:disabled) { background: var(--pn-2); }
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
    border-radius: var(--r-sm);
    background: var(--pn);
    color: var(--mu);
    transition: border-color var(--t-fast) var(--ease), background-color var(--t-fast) var(--ease);
  }
  .search-box:focus-within { border-color: var(--ac); background: var(--bg-3); }
  .search-box input { width: 180px; min-width: 0; height: 100%; min-height: 0; box-shadow: none; padding: 0; border: 0; background: transparent; color: var(--tx); font: inherit; font-size: 14px; outline: none; }
  .search-box input::placeholder { color: var(--mu-2); }
  .filter-select {
    height: 44px;
    padding: 0 36px 0 14px;
    border: 1px solid transparent;
    border-radius: var(--r-sm);
    background-color: var(--pn);
    color: var(--tx);
    font: inherit;
    font-size: 14px;
    font-weight: 600;
    cursor: pointer;
    transition: border-color var(--t-fast) var(--ease), background-color var(--t-fast) var(--ease);
  }
  .filter-select:hover { background-color: var(--pn-2); }
  .filter-select:focus { outline: none; border-color: var(--ac); background-color: var(--bg-3); }

  /* Tableaux */
  .dsm-table-panel { overflow-x: auto; border: 1px solid var(--line); border-radius: var(--r-lg); background: var(--bg-2); }
  .dsm-table-container { overflow-x: auto; border: 1px solid var(--line); border-radius: var(--r-md); }
  .dsm-table { width: 100%; border-collapse: collapse; font-size: 14px; text-align: left; }
  .main-table { min-width: 860px; }
  .dsm-table th {
    height: 48px;
    padding: 0 16px;
    border-bottom: 1px solid var(--line);
    background: transparent;
    color: var(--mu);
    font-size: 12px;
    font-weight: 700;
    letter-spacing: 0.06em;
    text-transform: uppercase;
    white-space: nowrap;
    user-select: none;
  }
  .dsm-table th.sortable { cursor: pointer; transition: color var(--t-fast) var(--ease); }
  .dsm-table th.sortable:hover { color: var(--tx); }
  .dsm-table td { height: 60px; padding: 8px 16px; border-bottom: 1px solid var(--line); vertical-align: middle; }
  .dsm-table tbody tr:last-child td { border-bottom: 0; }
  .dsm-table tbody tr { transition: background-color var(--t-fast) var(--ease); }
  .dsm-table tbody tr:hover { background: var(--pn); }
  .dsm-table tbody tr.selected { background: var(--ac-soft); }
  .dsm-table tbody tr.row-disabled { color: var(--mu); }
  .dsm-table input[type='checkbox'] { width: 20px; height: 20px; margin: 0; accent-color: var(--ac); cursor: pointer; vertical-align: middle; }
  .dsm-table input[type='checkbox']:disabled { cursor: not-allowed; opacity: 0.45; }

  .user-cell { display: flex; align-items: center; gap: 12px; }
  .dsm-avatar { display: grid; place-items: center; flex: none; width: 36px; height: 36px; overflow: hidden; border-radius: 50%; background: var(--pn-2); color: var(--tx); font-size: 12px; font-weight: 800; }
  .dsm-avatar img { width: 100%; height: 100%; object-fit: cover; }
  .user-login-text { display: inline-flex; align-items: center; gap: 8px; font-weight: 700; }
  .user-display-name { color: var(--mu); }

  /* Badges */
  .self-pill, .dsm-pill, .dsm-badge, .lib-kind-pill, .group-mini-tag {
    display: inline-flex;
    align-items: center;
    height: 24px;
    padding: 0 9px;
    border-radius: var(--r-xs);
    background: var(--pn-2);
    color: var(--tx);
    font-size: 12px;
    font-weight: 700;
    white-space: nowrap;
  }
  .self-pill { background: var(--ac-soft); color: var(--ac); }
  .pill-admin { background: var(--ac-soft); color: var(--ac); }
  .pill-user { background: var(--pn-2); color: var(--mu); }
  .pill-default { background: var(--info-soft); color: var(--info); }
  .dsm-badge { margin-left: 8px; }
  .badge-local { background: var(--pn-2); color: var(--mu); }
  .badge-sso { background: var(--info-soft); color: var(--info); }
  .lib-kind-pill { text-transform: capitalize; }
  .groups-tag-list { display: flex; flex-wrap: wrap; gap: 6px; }
  .group-mini-tag { font-weight: 600; }
  .default-tag { background: var(--info-soft); color: var(--info); }

  .status-indicator { display: inline-flex; align-items: center; gap: 8px; font-size: 14px; font-weight: 600; }
  .status-indicator .dot { width: 8px; height: 8px; border-radius: 50%; }
  .status-on .dot { background: var(--ok); }
  .status-off .dot { background: var(--err); }

  .text-muted { color: var(--mu); font-size: 13px; }
  .dsm-tag-muted { margin-left: 6px; color: var(--mu); font-size: 13px; }
  .empty-cell { height: auto; padding: 40px 16px; color: var(--mu); text-align: center; }
  .dsm-statusbar { display: flex; align-items: center; justify-content: space-between; gap: 12px; flex-wrap: wrap; padding: 0 4px; color: var(--mu); font-size: 13px; }
  .dsm-statusbar strong { color: var(--tx); }

  /* Dialogue d'édition */
  .dsm-modal-backdrop {
    position: fixed;
    inset: 0;
    z-index: 1000;
    display: grid;
    place-items: center;
    padding: 24px;
    background: rgb(6 8 14 / 0.55);
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
    border: 1px solid var(--line);
    border-radius: var(--r-xl);
    background: var(--bg-3);
    color: var(--tx);
    box-shadow: var(--shadow);
  }
  .dsm-modal-header { display: flex; align-items: center; justify-content: space-between; gap: 16px; padding: 20px 20px 12px 28px; }
  .modal-title-wrap { display: flex; align-items: center; gap: 12px; min-width: 0; color: var(--mu); }
  .modal-title-wrap h3 { margin: 0; color: var(--tx); font-size: 20px; font-weight: 800; letter-spacing: -0.02em; overflow-wrap: anywhere; }
  .btn-close { display: grid; place-items: center; flex: none; width: 44px; height: 44px; padding: 0; border: 0; border-radius: 14px; background: var(--pn); color: var(--tx); cursor: pointer; transition: background-color var(--t-fast) var(--ease); }
  .btn-close:hover { background: var(--pn-2); }

  .dsm-tabs { display: flex; gap: 20px; margin: 0 28px; border-bottom: 1px solid var(--line); overflow-x: auto; scrollbar-width: none; }
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
    color: var(--mu);
    font: inherit;
    font-size: 14px;
    font-weight: 700;
    white-space: nowrap;
    cursor: pointer;
    transition: color var(--t-fast) var(--ease);
  }
  .dsm-tab:hover { color: var(--tx); }
  .dsm-tab.active { color: var(--tx); border-bottom-color: var(--ac); }

  .dsm-modal-body { flex: 1; min-height: 0; padding: 24px 28px; overflow-y: auto; }
  .form-section-title { margin: 0 0 16px; font-size: 17px; font-weight: 700; }
  .section-desc { margin: -8px 0 16px; color: var(--mu); font-size: 13px; line-height: 1.5; }

  .dsm-grid-form { display: grid; }
  .form-row { display: grid; grid-template-columns: 200px minmax(0, 1fr); align-items: start; gap: 16px; padding: 16px 0; border-bottom: 1px solid var(--line); font-size: 14px; }
  .form-row:first-child { padding-top: 0; }
  .form-row:last-child { border-bottom: 0; padding-bottom: 0; }
  .row-label { padding-top: 12px; color: var(--tx); font-weight: 700; }
  .required { color: var(--err); text-decoration: none; }
  .row-field { display: flex; flex-direction: column; align-items: flex-start; gap: 8px; min-width: 0; }
  .row-field input:not([type='checkbox']), .row-field select {
    box-sizing: border-box;
    width: 100%;
    height: 46px;
    padding: 0 14px;
    border: 1px solid transparent;
    border-radius: var(--r-sm);
    background-color: var(--pn);
    color: var(--tx);
    font: inherit;
    font-size: 14px;
    font-weight: 500;
    box-shadow: none;
    transition: border-color var(--t-fast) var(--ease), background-color var(--t-fast) var(--ease);
  }
  .row-field select { padding-right: 40px; }
  .row-field input:not([type='checkbox']):hover, .row-field select:hover { background-color: var(--pn-2); }
  .row-field input:not([type='checkbox']):focus, .row-field select:focus { outline: none; border-color: var(--ac); background-color: var(--bg-3); }
  .row-field input:disabled, .row-field select:disabled { opacity: 0.45; cursor: not-allowed; }
  .user-static-login { display: inline-flex; align-items: center; min-height: 46px; font-weight: 700; }
  .row-field:has(.user-static-login) { flex-direction: row; flex-wrap: wrap; align-items: center; }
  .field-hint { color: var(--mu); font-size: 13px; line-height: 1.5; }
  .dsm-checkbox-label { display: flex; align-items: center; gap: 12px; min-height: 46px; font-weight: 600; cursor: pointer; }
  .dsm-checkbox-label input { width: 20px; height: 20px; margin: 0; accent-color: var(--ac); }

  .dsm-info-box { display: flex; align-items: flex-start; gap: 12px; padding: 14px 16px; border-radius: var(--r-md); background: var(--info-soft); color: var(--tx); font-size: 14px; line-height: 1.5; }
  .dsm-info-box :global(svg) { flex: none; margin-top: 1px; color: var(--info); }

  .inherited-badge { color: var(--tx); font-size: 13px; font-weight: 600; }
  .direct-badge { color: var(--mu); font-size: 13px; }

  .dsm-modal-footer { display: flex; align-items: center; justify-content: flex-end; gap: 8px; padding: 16px 28px 24px; border-top: 1px solid var(--line); }

  /* Messages */
  .dsm-alert { display: flex; align-items: center; gap: 12px; min-height: 52px; padding: 12px 16px; border-radius: var(--r-md); color: var(--tx); font-size: 14px; }
  .alert-success { background: var(--ok-soft); }
  .alert-success :global(svg) { color: var(--ok); }
  .alert-error { background: var(--err-soft); }
  .alert-error :global(svg) { color: var(--err); }

  .col-check { width: 52px; }
  .col-check-center { width: 52px; text-align: center; }
  .cell-center { text-align: center; }
  .mt-10 { margin-top: 4px; }

  @media (max-width: 820px) {
    .dsm-toolbar { align-items: stretch; flex-direction: column; }
    .toolbar-right { width: 100%; }
    .search-box { flex: 1 1 220px; }
    .search-box input { width: 100%; }
    .filter-select { flex: 1 1 160px; }
  }
  @media (max-width: 640px) {
    .dsm-icon-badge { display: none; }
    .toolbar-left .btn { flex: 1 1 auto; }
    .btn-group { flex: 1 1 100%; }
    .btn-group .btn { flex: 1; }
    .dsm-modal-backdrop { place-items: end stretch; padding: 0; }
    .dsm-modal { max-width: none; max-height: 92dvh; border-radius: var(--r-xl) var(--r-xl) 0 0; }
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
