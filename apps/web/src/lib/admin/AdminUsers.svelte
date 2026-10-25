<script lang="ts">
  import ParentalPolicy from './ParentalPolicy.svelte';
  import UserMediaPolicy from './UserMediaPolicy.svelte';
  import type { Library } from './types';
  import { session, readSession, type AuthUser } from '../auth/session';
  import Icon from '../ui/Icon.svelte';
  import { locale, t } from '../i18n';

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
    if (timestamp == null) return 'Jamais';
    return new Date(timestamp * 1000).toLocaleString($locale === 'en' ? 'en-GB' : 'fr-FR', {
      dateStyle: 'short',
      timeStyle: 'short'
    });
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
      const errors: Record<string, string> = {
        login_exists: 'Cet identifiant existe déjà.',
        self_protection: 'Vous ne pouvez pas désactiver ou rétrograder votre propre compte.',
        last_administrator: 'Le dernier administrateur actif doit être conservé.',
        invalid_password: 'Le mot de passe doit contenir de 15 à 128 caractères sans caractère de contrôle.',
        local_credentials_required: 'Ce compte est géré par le fournisseur SSO.',
        invalid_login: 'Identifiant : 3 à 64 caractères minuscules, chiffres, points, tirets ou underscores.',
        invalid_display_name: 'Nom : 128 caractères maximum, sans caractère de contrôle.'
      };
      throw new Error(errors[code] ?? 'Opération refusée ou serveur indisponible.');
    }
  }

  async function syncUserGroups(userId: string, targetGroupIds: string[]) {
    if (!$session) return;
    const updatePromises: Promise<unknown>[] = [];
    for (const group of groups) {
      if (group.isDefault) continue;
      const isMember = group.userIds.includes(userId);
      const shouldBeMember = targetGroupIds.includes(group.id);
      if (isMember !== shouldBeMember) {
        const nextUserIds = shouldBeMember
          ? [...group.userIds, userId]
          : group.userIds.filter(id => id !== userId);
        updatePromises.push(
          fetch(`/api/v1/admin/groups/${encodeURIComponent(group.id)}`, {
            method: 'PUT',
            credentials: 'same-origin',
            cache: 'no-store',
            headers: { 'content-type': 'application/json', 'x-csrf-token': $session.csrfToken },
            body: JSON.stringify({
              name: group.name,
              isDefault: group.isDefault,
              userIds: nextUserIds,
              libraryIds: group.libraryIds
            })
          })
        );
      }
    }
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

  const filteredUsers = $derived.by(() => {
    return allUsers.filter(user => {
      const q = search.trim().toLowerCase();
      const matchSearch = !q || [user.login, user.displayName ?? '', user.id].some(v => v.toLowerCase().includes(q));
      const matchRole = roleFilter === 'all' || user.role === roleFilter;
      const matchStatus = statusFilter === 'all' || (statusFilter === 'disabled' ? user.disabledAt != null : user.disabledAt == null);
      return matchSearch && matchRole && matchStatus;
    }).sort((a, b) => {
      let cmp = 0;
      if (sortField === 'name') {
        cmp = (a.displayName || a.login).localeCompare(b.displayName || b.login);
      } else if (sortField === 'role') {
        cmp = a.role.localeCompare(b.role);
      } else if (sortField === 'status') {
        const sA = a.disabledAt != null ? 1 : 0;
        const sB = b.disabledAt != null ? 1 : 0;
        cmp = sA - sB;
      } else if (sortField === 'lastSeen') {
        cmp = (a.lastSeenAt ?? 0) - (b.lastSeenAt ?? 0);
      }
      return sortAsc ? cmp : -cmp;
    });
  });

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
    <div class="dsm-modal-backdrop" onclick={(e) => { if (e.target === e.currentTarget) { editorOpen = false; } }}>
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
                      <th style="width: 48px;"></th>
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
                        <th style="width: 48px;">
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
                      <button type="button" class="btn btn-warning" style="margin-top: 10px;" disabled={busy || newPassword.length < 15} onclick={() => save(true)}>
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
          <th style="width: 40px; text-align: center;">
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
            <td style="text-align: center;">
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
  .dsm-container {
    display: flex;
    flex-direction: column;
    gap: 14px;
    margin-bottom: 40px;
  }

  .dsm-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding-bottom: 12px;
    border-bottom: 1px solid var(--color-line);
  }

  .dsm-title-group {
    display: flex;
    align-items: center;
    gap: 14px;
  }

  .dsm-icon-badge {
    display: grid;
    place-items: center;
    width: 42px;
    height: 42px;
    background: var(--color-accent-soft);
    color: var(--color-accent);
    border-radius: var(--radius-md);
  }

  h2 {
    margin: 0;
    font-size: 20px;
    font-weight: 700;
  }

  .dsm-subtitle {
    margin: 2px 0 0;
    font-size: 13px;
    color: var(--color-muted);
  }

  /* Toolbar */
  .dsm-toolbar {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
    flex-wrap: wrap;
    background: var(--color-surface);
    border: 1px solid var(--color-line);
    border-radius: var(--radius-sm);
    padding: 8px 12px;
  }

  .toolbar-left, .toolbar-right {
    display: flex;
    align-items: center;
    gap: 8px;
    flex-wrap: wrap;
  }

  .btn {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    height: 32px;
    padding: 0 12px;
    border-radius: var(--radius-sm);
    font-size: 13px;
    font-weight: 500;
    border: 1px solid transparent;
    cursor: pointer;
    background: transparent;
    transition: all 0.15s ease;
  }

  .btn-primary {
    background: var(--color-accent);
    color: var(--color-on-accent);
  }
  .btn-primary:hover:not(:disabled) {
    filter: brightness(1.1);
  }

  .btn-default {
    background: var(--color-raised);
    border-color: var(--color-line);
    color: var(--color-text);
  }
  .btn-default:hover:not(:disabled) {
    background: var(--color-surface);
    border-color: var(--color-accent);
  }

  .btn-warning {
    background: #d97706;
    color: #fff;
  }

  .btn:disabled {
    opacity: 0.5;
    cursor: not-allowed;
  }

  .btn-group {
    display: inline-flex;
  }
  .btn-group .btn {
    border-radius: 0;
  }
  .btn-group .btn:first-child {
    border-top-left-radius: var(--radius-sm);
    border-bottom-left-radius: var(--radius-sm);
  }
  .btn-group .btn:last-child {
    border-top-right-radius: var(--radius-sm);
    border-bottom-right-radius: var(--radius-sm);
    border-left: 0;
  }

  .search-box {
    display: flex;
    align-items: center;
    gap: 6px;
    background: var(--color-raised);
    border: 1px solid var(--color-line);
    border-radius: var(--radius-sm);
    padding: 0 8px;
    height: 32px;
    color: var(--color-muted);
  }

  .search-box input {
    border: none;
    background: transparent;
    color: var(--color-text);
    outline: none;
    font-size: 13px;
    width: 140px;
  }

  .filter-select {
    height: 32px;
    padding: 0 8px;
    background: var(--color-raised);
    border: 1px solid var(--color-line);
    border-radius: var(--radius-sm);
    color: var(--color-text);
    font-size: 12px;
  }

  /* Table */
  .dsm-table-panel {
    background: var(--color-surface);
    border: 1px solid var(--color-line);
    border-radius: var(--radius-sm);
    overflow-x: auto;
  }

  .dsm-table {
    width: 100%;
    border-collapse: collapse;
    font-size: 13px;
    text-align: left;
  }

  .dsm-table th {
    background: var(--color-raised);
    border-bottom: 1px solid var(--color-line);
    padding: 8px 12px;
    font-weight: 600;
    color: var(--color-muted);
    user-select: none;
    white-space: nowrap;
  }

  .dsm-table th.sortable {
    cursor: pointer;
  }
  .dsm-table th.sortable:hover {
    color: var(--color-text);
  }

  .dsm-table td {
    padding: 8px 12px;
    border-bottom: 1px solid var(--color-line);
    vertical-align: middle;
  }

  .dsm-table tbody tr:hover {
    background: var(--color-raised);
  }

  .dsm-table tbody tr.selected {
    background: var(--color-accent-soft);
  }

  .dsm-table tbody tr.row-disabled {
    opacity: 0.7;
  }

  .user-cell {
    display: flex;
    align-items: center;
    gap: 10px;
  }

  .dsm-avatar {
    display: grid;
    place-items: center;
    width: 26px;
    height: 26px;
    border-radius: 50%;
    background: var(--color-accent-soft);
    color: var(--color-accent);
    font-size: 11px;
    font-weight: 700;
    overflow: hidden;
    flex-shrink: 0;
  }
  .dsm-avatar img {
    width: 100%;
    height: 100%;
    object-fit: cover;
  }

  .user-login-text {
    font-weight: 600;
    display: inline-flex;
    align-items: center;
    gap: 6px;
  }

  .self-pill {
    font-size: 10px;
    padding: 1px 6px;
    border-radius: 99px;
    background: var(--color-accent-soft);
    color: var(--color-accent);
  }

  .dsm-pill {
    font-size: 11px;
    font-weight: 600;
    padding: 2px 8px;
    border-radius: 99px;
  }

  .pill-admin {
    background: var(--color-accent-soft);
    color: var(--color-accent);
  }

  .pill-user {
    background: var(--color-raised);
    color: var(--color-muted);
  }

  .pill-default {
    background: #0284c726;
    color: #38bdf8;
  }

  .status-indicator {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    font-size: 12px;
  }
  .status-indicator .dot {
    width: 8px;
    height: 8px;
    border-radius: 50%;
  }
  .status-on .dot {
    background: #22c55e;
  }
  .status-off .dot {
    background: #ef4444;
  }

  .groups-tag-list {
    display: flex;
    flex-wrap: wrap;
    gap: 4px;
  }

  .group-mini-tag {
    font-size: 11px;
    background: var(--color-raised);
    border: 1px solid var(--color-line);
    padding: 1px 6px;
    border-radius: var(--radius-sm);
  }

  .default-tag {
    border-color: var(--color-accent);
    color: var(--color-accent);
  }

  .text-muted {
    color: var(--color-muted);
    font-size: 12px;
  }

  .empty-cell {
    text-align: center;
    padding: 30px;
    color: var(--color-muted);
  }

  .dsm-statusbar {
    display: flex;
    align-items: center;
    justify-content: space-between;
    font-size: 12px;
    color: var(--color-muted);
    padding: 0 4px;
  }

  /* Modal */
  .dsm-modal-backdrop {
    position: fixed;
    inset: 0;
    background: rgba(0, 0, 0, 0.6);
    display: grid;
    place-items: center;
    z-index: 1000;
    padding: 20px;
    backdrop-filter: blur(2px);
  }

  .dsm-modal {
    background: var(--color-surface);
    border: 1px solid var(--color-line);
    border-radius: var(--radius-md);
    width: 100%;
    max-width: 680px;
    max-height: 85vh;
    display: flex;
    flex-direction: column;
    box-shadow: 0 10px 30px rgba(0, 0, 0, 0.4);
    overflow: hidden;
  }

  .dsm-modal-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 14px 20px;
    background: var(--color-raised);
    border-bottom: 1px solid var(--color-line);
  }

  .modal-title-wrap {
    display: flex;
    align-items: center;
    gap: 10px;
  }

  .modal-title-wrap h3 {
    margin: 0;
    font-size: 16px;
    font-weight: 700;
  }

  .btn-close {
    background: transparent;
    border: none;
    color: var(--color-muted);
    cursor: pointer;
    display: grid;
    place-items: center;
    padding: 4px;
    border-radius: var(--radius-sm);
  }
  .btn-close:hover {
    color: var(--color-text);
    background: var(--color-line);
  }

  .dsm-tabs {
    display: flex;
    gap: 2px;
    padding: 0 16px;
    background: var(--color-raised);
    border-bottom: 1px solid var(--color-line);
    overflow-x: auto;
  }

  .dsm-tab {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    padding: 10px 14px;
    background: transparent;
    border: none;
    border-bottom: 2px solid transparent;
    color: var(--color-muted);
    font-size: 13px;
    font-weight: 500;
    cursor: pointer;
    white-space: nowrap;
  }
  .dsm-tab:hover {
    color: var(--color-text);
  }
  .dsm-tab.active {
    color: var(--color-accent);
    border-bottom-color: var(--color-accent);
    font-weight: 600;
  }

  .dsm-modal-body {
    padding: 20px;
    overflow-y: auto;
    flex: 1;
  }

  .form-section-title {
    margin: 0 0 12px;
    font-size: 14px;
    font-weight: 600;
  }

  .section-desc {
    font-size: 13px;
    color: var(--color-muted);
    margin: 0 0 14px;
  }

  .dsm-grid-form {
    display: grid;
    gap: 16px;
  }

  .form-row {
    display: grid;
    grid-template-columns: 180px 1fr;
    align-items: start;
    gap: 16px;
    font-size: 13px;
  }

  .row-label {
    padding-top: 8px;
    color: var(--color-muted);
    font-weight: 500;
  }

  .row-field {
    display: flex;
    flex-direction: column;
    gap: 6px;
  }

  .row-field input:not([type="checkbox"]), .row-field select {
    width: 100%;
    height: 34px;
    padding: 0 10px;
    background: var(--color-raised);
    border: 1px solid var(--color-line);
    border-radius: var(--radius-sm);
    color: var(--color-text);
    font-size: 13px;
    box-sizing: border-box;
  }

  .field-hint {
    font-size: 11px;
    color: var(--color-muted);
  }

  .dsm-checkbox-label {
    display: flex;
    align-items: center;
    gap: 8px;
    cursor: pointer;
    padding-top: 6px;
  }

  .dsm-badge {
    display: inline-block;
    padding: 2px 8px;
    border-radius: 99px;
    font-size: 11px;
    margin-left: 8px;
  }
  .badge-local {
    background: var(--color-raised);
    color: var(--color-muted);
  }
  .badge-sso {
    background: #0284c726;
    color: #38bdf8;
  }

  .dsm-info-box {
    display: flex;
    align-items: center;
    gap: 12px;
    background: var(--color-raised);
    border: 1px solid var(--color-line);
    padding: 12px 16px;
    border-radius: var(--radius-sm);
    font-size: 13px;
    color: var(--color-text);
  }

  .inherited-badge {
    font-size: 11px;
    color: var(--color-accent);
  }
  .direct-badge {
    font-size: 11px;
    color: var(--color-muted);
  }
  .lib-kind-pill {
    text-transform: capitalize;
    font-size: 11px;
    background: var(--color-raised);
    padding: 2px 6px;
    border-radius: var(--radius-sm);
  }

  .dsm-modal-footer {
    display: flex;
    align-items: center;
    justify-content: flex-end;
    gap: 10px;
    padding: 14px 20px;
    background: var(--color-raised);
    border-top: 1px solid var(--color-line);
  }

  .dsm-alert {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 10px 14px;
    border-radius: var(--radius-sm);
    font-size: 13px;
  }
  .alert-success {
    background: var(--color-success-soft);
    border: 1px solid var(--color-success);
    color: var(--color-success);
  }
  .alert-error {
    background: var(--color-danger-soft);
    border: 1px solid var(--color-danger);
    color: var(--color-danger);
  }

  @media(max-width: 640px) {
    .form-row {
      grid-template-columns: 1fr;
      gap: 6px;
    }
  }
</style>
