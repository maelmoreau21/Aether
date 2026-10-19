<script lang="ts">
  import type { Library } from './types';
  import { session, readSession, type AuthUser } from '../auth/session';
  import Icon from '../ui/Icon.svelte';
  import { locale, t } from '../i18n';

  interface Props {
    users: Account[];
    libraries: Library[];
    currentUserId?: string;
    groups?: { id: string; name: string; userIds: string[]; libraryIds: string[] }[];
    onchanged?: () => void | Promise<void>;
  }

  const {
    users,
    libraries,
    currentUserId, groups = [], onchanged
  }: Props = $props();
  type Account = AuthUser & { disabledAt?: number | null };
  let accounts = $state<Account[] | null>(null);
  const allUsers = $derived(accounts ?? users);
  let busy = $state(false);
  let message = $state('');
  let failed = $state(false);
  let editing = $state<Account | null>(null);
  let login = $state('');
  let displayName = $state('');
  let role = $state<'administrator' | 'user'>('user');
  let disabled = $state(false);
  let password = $state('');
  let newPassword = $state('');
  let libraryIds = $state<string[]>([]);
  async function refresh() {
    const response = await fetch('/api/v1/admin/users', { credentials: 'same-origin', cache: 'no-store', signal: AbortSignal.timeout(10000) });
    if (!response.ok) throw new Error('Chargement des comptes impossible.');
    accounts = (await response.json()).users;
  }
  function edit(user: Account | null) {
    editing = user; login = user?.login ?? ''; displayName = user?.displayName ?? '';
    role = user?.role ?? 'user'; disabled = user?.disabledAt != null; password = ''; newPassword = '';
    libraryIds = user ? libraries.filter(l => l.userIds.includes(user.id)).map(l => l.id) : [];
  }
  async function mutate(path: string, method: string, body: object) {
    if (!$session) throw new Error('Session expirée. Reconnectez-vous.');
    const response = await fetch(path, { method, credentials: 'same-origin', cache: 'no-store', headers: { 'content-type': 'application/json', 'x-csrf-token': $session.csrfToken }, body: JSON.stringify(body), signal: AbortSignal.timeout(15000) });
    if (!response.ok) {
      const code = (await response.json().catch(() => ({}))).error;
      const errors: Record<string, string> = { login_exists: 'Cet identifiant existe déjà.', self_protection: 'Vous ne pouvez pas désactiver ou rétrograder votre propre compte.', last_administrator: 'Le dernier administrateur actif doit être conservé.', invalid_password: 'Le mot de passe doit contenir de 15 à 128 caractères sans caractère de contrôle.', local_credentials_required: 'Ce compte est géré par le fournisseur SSO.', invalid_login: 'Identifiant : 3 à 64 caractères minuscules, chiffres, points, tirets ou underscores.', invalid_display_name: 'Nom : 128 caractères maximum, sans caractère de contrôle.' };
      throw new Error(errors[code] ?? 'Opération refusée ou serveur indisponible.');
    }
  }
  async function save(resetPassword = false) {
    busy = true; message = ''; failed = false;
    try {
      if (resetPassword && editing) {
        await mutate(`/api/v1/admin/users/${encodeURIComponent(editing.id)}/password`, 'PUT', { password: newPassword });
        newPassword = '';
        message = 'Mot de passe remplacé. Toutes les sessions du compte ont été révoquées.';
        if (editing.id === currentUserId) { await readSession(); return; }
      } else if (editing) {
        await mutate(`/api/v1/admin/users/${encodeURIComponent(editing.id)}`, 'PUT', {
          role,
          disabled,
          displayName: displayName.trim() || null,
          libraryIds: role === 'administrator' ? [] : libraryIds
        });
        message = 'Compte enregistré. Les permissions et changements de sécurité sont appliqués.';
      } else {
        await mutate('/api/v1/admin/users', 'POST', {
          login,
          password,
          role,
          displayName: displayName.trim() || null,
          libraryIds: role === 'administrator' ? [] : libraryIds
        });
        password = '';
        message = 'Compte créé avec ses autorisations d’accès aux bibliothèques.';
      }
      await refresh(); await onchanged?.(); edit(null);
    } catch (error) { failed = true; message = error instanceof Error ? error.message : 'Opération impossible.'; }
    finally { busy = false; }
  }
  let search = $state('');
  let roleFilter = $state('all');
  let authFilter = $state('all');
  let accessFilter = $state('all');
  const copy = $derived($locale === 'en' ? {
    search: 'Search accounts', placeholder: 'Name, login or user ID…', role: 'Role', all: 'All',
    authentication: 'Authentication', access: 'Library access', withAccess: 'With access', withoutAccess: 'Without access',
    results: 'matching accounts', empty: 'No accounts match these filters.', reset: 'Clear filters',
    manage: 'Manage user groups', disabled: 'Disabled', unknown: 'Not specified',
    limit: 'Disabling an account preserves its data. Security changes revoke its sessions. Library permissions are assigned directly here or via user groups.',
    identifier: 'User ID',
  } : {
    search: 'Rechercher un compte', placeholder: 'Nom, identifiant ou ID utilisateur…', role: 'Rôle', all: 'Tous',
    authentication: 'Authentification', access: 'Accès aux bibliothèques', withAccess: 'Avec accès', withoutAccess: 'Sans accès',
    results: 'comptes correspondants', empty: 'Aucun compte ne correspond à ces filtres.', reset: 'Effacer les filtres',
    manage: 'Gérer les groupes d’utilisateurs', disabled: 'Désactivée', unknown: 'Non précisée',
    limit: 'Les comptes sont conservés lors de leur désactivation. Les changements de rôle, état et mot de passe révoquent leurs sessions. Les accès aux bibliothèques se configurent directement ici ou via les groupes d’utilisateurs.',
    identifier: 'ID utilisateur',
  });
  const visibleUsers = $derived(allUsers.filter(user => {
    const query = search.trim().toLocaleLowerCase();
    const matchesSearch = !query || [user.login, user.displayName ?? '', user.id].some(value => value.toLocaleLowerCase().includes(query));
    const hasAccess = user.disabledAt == null && libraries.some(library => library.enabled && (user.role === 'administrator' || (library.userIds.includes(user.id) || groups.some(group => group.userIds.includes(user.id) && group.libraryIds.includes(library.id)))));
    return matchesSearch
      && (roleFilter === 'all' || user.role === roleFilter)
      && (authFilter === 'all' || (authFilter === 'sso' ? user.sso === true : user.sso === false))
      && (accessFilter === 'all' || (accessFilter === 'with' ? hasAccess : !hasAccess));
  }));
  function resetFilters() {
    search = '';
    roleFilter = 'all';
    authFilter = 'all';
    accessFilter = 'all';
  }
</script>

<section id="users-section" class="dashboard-section" aria-labelledby="users-heading">
  <div class="section-header">
    <div class="header-icon-wrap">
      <Icon name="users" size={18} />
    </div>
    <div>
      <h2 id="users-heading" class="section-title">{$t('users_heading')}</h2>
      <p class="section-subtitle">{$t('users_subtitle')}</p>
    </div>
  </div>

  <div class="card">
    <div class="card-header">
      <div class="card-title-group">
        <h3>{$t('registered_accounts')} ({allUsers.length})</h3>
        <span class="muted-note">{$t('strict_roles_note')}</span>
      </div>
      <div class="badge-role-stats">
        <span class="role-pill admin-pill">
          {allUsers.filter(u => u.role === 'administrator').length} {$t('administrator')}
        </span>
        <span class="role-pill user-pill">
          {allUsers.filter(u => u.role === 'user').length} {$t('user')}(s)
        </span>
      </div>
    </div>

    <form class="account-editor" onsubmit={(event) => { event.preventDefault(); void save(); }}>
      <h3>{editing ? `Modifier ${editing.login}` : 'Créer un compte local'}</h3>
      {#if !editing}<label>Identifiant<input required minlength="3" maxlength="64" pattern="[a-z0-9._-]+" autocomplete="off" bind:value={login} /></label>{/if}
      <label>Nom affiché<input maxlength="128" bind:value={displayName} /></label>
      <label>Rôle<select bind:value={role} disabled={editing?.id === currentUserId}><option value="user">Utilisateur</option><option value="administrator">Administrateur</option></select></label>
      {#if editing}<label><input type="checkbox" bind:checked={disabled} disabled={editing.id === currentUserId} /> Compte désactivé</label>
      {:else}<label>Mot de passe (15 à 128 caractères)<input type="password" required minlength="15" maxlength="128" autocomplete="new-password" bind:value={password} /></label>{/if}
      {#if role === 'user'}
        <fieldset class="user-libraries-fieldset" disabled={busy}>
          <legend>Bibliothèques autorisées</legend>
          {#if libraries.length === 0}
            <p class="muted-note">Aucune bibliothèque configurée.</p>
          {:else}
            <div class="user-libraries-grid">
              {#each libraries as library (library.id)}
                <label class="check">
                  <input
                    type="checkbox"
                    value={library.id}
                    bind:group={libraryIds}
                    aria-label={"Bibliothèque autorisée : " + library.name}
                  />
                  <span>{library.name}{library.enabled ? '' : ' (désactivée)'}</span>
                </label>
              {/each}
            </div>
          {/if}
        </fieldset>
      {/if}
      <button type="submit" class="btn btn-primary" disabled={busy}>{busy ? 'Enregistrement…' : editing ? 'Enregistrer' : 'Créer le compte'}</button>
      {#if editing}<button type="button" class="btn btn-secondary" disabled={busy} onclick={() => edit(null)}>Annuler</button>{/if}
    </form>
    {#if editing && !editing.sso}
      <form class="account-editor" onsubmit={(event) => { event.preventDefault(); void save(true); }}>
        <h3>Remplacer le mot de passe local</h3>
        <p>Toutes les sessions de ce compte seront déconnectées.</p>
        <label>Nouveau mot de passe<input type="password" required minlength="15" maxlength="128" autocomplete="new-password" bind:value={newPassword} /></label>
        <button type="submit" class="btn btn-secondary" disabled={busy}>Remplacer et révoquer les sessions</button>
      </form>
    {/if}
    {#if message}<p role={failed ? 'alert' : 'status'}>{message}</p>{/if}
    <button type="button" class="btn btn-secondary" disabled={busy} onclick={() => refresh().catch(() => { failed = true; message = 'Chargement impossible.'; })}>Actualiser les comptes et leurs états</button>
    <div class="account-filters">
      <label class="search-filter">{copy.search}
        <input type="search" bind:value={search} placeholder={copy.placeholder} />
      </label>
      <label>{copy.role}
        <select bind:value={roleFilter}>
          <option value="all">{copy.all}</option>
          <option value="administrator">{$t('administrator')}</option>
          <option value="user">{$t('user')}</option>
        </select>
      </label>
      <label>{copy.authentication}
        <select bind:value={authFilter}>
          <option value="all">{copy.all}</option>
          <option value="local">{$t('local_account')}</option>
          <option value="sso">{$t('sso_badge')}</option>
        </select>
      </label>
      <label>{copy.access}
        <select bind:value={accessFilter}>
          <option value="all">{copy.all}</option>
          <option value="with">{copy.withAccess}</option>
          <option value="without">{copy.withoutAccess}</option>
        </select>
      </label>
    </div>
    <div class="directory-summary">
      <span role="status">{visibleUsers.length} / {allUsers.length} {copy.results}</span>
      {#if search || roleFilter !== 'all' || authFilter !== 'all' || accessFilter !== 'all'}
        <button type="button" onclick={resetFilters}>{copy.reset}</button>
      {/if}
    </div>
    <div class="users-table-wrapper">
      <table class="admin-table">
        <thead>
          <tr>
            <th>{$t('col_user_login')}</th>
            <th>{$t('col_user_name')}</th>
            <th>{$t('col_auth_type')}</th>
            <th>{$t('col_role')}</th>
            <th>{$t('col_allowed_libs')}</th><th>Gestion</th>
          </tr>
        </thead>
        <tbody>
          {#each visibleUsers as user (user.id)}
            {@const isSso = user.sso === true}
            {@const userLibraries = libraries.filter(lib => lib.userIds.includes(user.id) || groups.some(group => group.userIds.includes(user.id) && group.libraryIds.includes(lib.id)))}
            <tr>
              <td>
                <div class="user-login-cell">
                  <strong>{user.login}</strong>
                  {#if user.id === currentUserId}
                    <span class="current-user-badge">{$t('you')}</span>
                  {/if}
                </div>
                <details class="account-id"><summary>{copy.identifier}</summary><code>{user.id}</code></details>
              </td>
              <td>
                <span class="user-display-name">
                  {user.displayName || user.login}
                </span>
              </td>
              <td>
                <span class="badge" data-status={isSso ? 'disconnected' : 'connected'}>
                  {user.sso == null ? copy.unknown : isSso ? $t('sso_badge') : $t('local_account')}
                </span>
              </td>
              <td>
                <span class="role-badge" class:admin-role={user.role === 'administrator'}>
                  {user.role === 'administrator' ? $t('administrator') : $t('user')}
                </span>
              </td>
              <td>
                {#if user.role === 'administrator'}
                  <span class="all-access-badge">{$t('all_libs_granted')}</span>
                {:else if userLibraries.length > 0}
                  <div class="lib-badges-list">
                    {#each userLibraries as lib (lib.id)}
                      <span class="badge kind-badge" class:disabled-library={!lib.enabled}>{lib.name}{!lib.userIds.includes(user.id) ? ' � groupe' : ''}{!lib.enabled ? ` · ${copy.disabled}` : ''}</span>
                    {/each}
                  </div>
                {:else}
                  <span class="muted">{$t('no_libs_granted')}</span>
                {/if}
              </td>
              <td><button type="button" class="btn btn-secondary" disabled={busy} onclick={() => edit(user)}>Modifier</button>{#if user.disabledAt != null}<span class="badge">Désactivé</span>{/if}</td>
            </tr>
          {:else}
            <tr><td colspan="6" class="empty-directory">{copy.empty}</td></tr>
          {/each}
        </tbody>
      </table>
    </div>

    <div class="security-footer-note">
      <Icon name="shield" size={16} />
      <span>
        {copy.limit}
        <a href="#groups-section">{copy.manage}</a>
      </span>
    </div>
  </div>
</section>

<style>
  .account-editor { display: flex; flex-wrap: wrap; gap: 16px; align-items: end; padding: 18px; margin-bottom: 20px; background: var(--color-raised); border-radius: var(--radius-sm); }
  .account-editor h3, .account-editor p { flex-basis: 100%; margin: 0; }
  .account-editor label { display: grid; gap: 7px; flex: 1 1 180px; font-size: 13px; }
  .account-editor input:not([type="checkbox"]), .account-editor select { min-height: 40px; padding: 8px; border: 1px solid var(--color-line); border-radius: var(--radius-sm); background: var(--color-surface); color: var(--color-text); font: inherit; }
  .account-filters { display: flex; flex-wrap: wrap; gap: 14px; margin-bottom: 14px; }
  .account-filters label { display: grid; gap: 7px; flex: 1 1 150px; font-size: 12px; color: var(--color-muted); }
  .account-filters .search-filter { flex-grow: 2; }
  .account-filters input, .account-filters select { width: 100%; min-height: 40px; padding: 8px 10px; border: 1px solid var(--color-line); border-radius: var(--radius-sm); background: var(--color-raised); color: var(--color-text); font: inherit; }
  .directory-summary { display: flex; flex-wrap: wrap; justify-content: space-between; gap: 12px; margin-bottom: 14px; font-size: 12px; color: var(--color-muted); }
  .directory-summary button { border: 0; background: transparent; color: var(--color-accent); cursor: pointer; font: inherit; }
  .account-id { margin-top: 7px; color: var(--color-muted); font-size: 11px; }
  .account-id summary { cursor: pointer; }
  .account-id code { display: block; margin-top: 5px; overflow-wrap: anywhere; }
  .admin-table .empty-directory { padding: 32px 14px; text-align: center; color: var(--color-muted); }
  .disabled-library { opacity: .65; }
  .security-footer-note a { display: block; margin-top: 8px; color: var(--color-accent); }
  :is(input, select, button, a, summary):focus-visible { outline: 2px solid var(--color-accent); outline-offset: 3px; }

  .dashboard-section {
    margin-bottom: 48px;
    scroll-margin-top: 24px;
  }

  .section-header {
    display: flex;
    align-items: center;
    gap: 14px;
    margin-bottom: 24px;
    padding-bottom: 14px;
    border-bottom: 1px solid var(--color-line);
  }

  .header-icon-wrap {
    width: 36px;
    height: 36px;
    border-radius: var(--radius-sm);
    background: var(--color-accent-soft);
    color: var(--color-accent);
    display: flex;
    align-items: center;
    justify-content: center;
    flex-shrink: 0;
  }

  .section-title {
    font-size: 20px;
    font-weight: 650;
    letter-spacing: -0.02em;
    margin: 0;
    color: var(--color-text);
  }

  .section-subtitle {
    margin: 4px 0 0;
    font-size: 13px;
    color: var(--color-muted);
  }

  .card {
    background: var(--color-surface);
    border: 1px solid var(--color-line);
    border-radius: var(--radius-md);
    padding: 24px;
  }

  .card-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 16px;
    margin-bottom: 20px;
    padding-bottom: 12px;
    border-bottom: 1px solid var(--color-line);
    flex-wrap: wrap;
  }

  .card-title-group h3 {
    margin: 0;
    font-size: 16px;
    font-weight: 600;
  }

  .badge-role-stats {
    display: flex;
    gap: 8px;
  }

  .role-pill {
    font-size: 11px;
    font-weight: 600;
    padding: 3px 10px;
    border-radius: 99px;
  }

  .admin-pill {
    background: var(--color-accent-soft);
    color: var(--color-accent);
  }

  .user-pill {
    background: var(--color-raised);
    color: var(--color-muted);
  }

  .users-table-wrapper {
    overflow-x: auto;
  }

  .admin-table {
    width: 100%;
    border-collapse: collapse;
    font-size: 13px;
    text-align: left;
  }

  .admin-table th {
    padding: 12px 14px;
    border-bottom: 1px solid var(--color-line);
    color: var(--color-muted);
    font-weight: 500;
    font-size: 12px;
  }

  .admin-table td {
    padding: 14px;
    border-bottom: 1px solid var(--color-line);
    vertical-align: middle;
  }

  .user-login-cell {
    display: flex;
    align-items: center;
    gap: 8px;
  }

  .current-user-badge {
    font-size: 10px;
    font-weight: 600;
    background: var(--color-accent-soft);
    color: var(--color-accent);
    padding: 2px 6px;
    border-radius: 4px;
  }

  .role-badge {
    font-size: 11px;
    padding: 3px 8px;
    border-radius: 4px;
    background: var(--color-raised);
    color: var(--color-muted);
  }

  .admin-role {
    background: var(--color-accent-soft);
    color: var(--color-accent);
    font-weight: 600;
  }

  .all-access-badge {
    color: var(--status-success);
    font-size: 12px;
    font-weight: 500;
  }

  .lib-badges-list {
    display: flex;
    gap: 6px;
    flex-wrap: wrap;
  }

  .badge {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    font-size: 11px;
    font-weight: 500;
    padding: 2px 8px;
    border-radius: 99px;
    border: 1px solid var(--color-line);
    vertical-align: middle;
  }

  .badge[data-status='connected'] {
    color: var(--status-success);
    border-color: var(--color-line);
    background: var(--color-raised);
  }

  .badge[data-status='disconnected'] {
    color: var(--text-secondary);
    border-color: var(--color-line);
    background: var(--color-raised);
  }

  .kind-badge {
    background: var(--color-raised);
    color: var(--color-text);
  }

  .security-footer-note {
    display: flex;
    align-items: center;
    gap: 12px;
    margin-top: 20px;
    padding: 12px 16px;
    background: var(--color-raised);
    border-radius: var(--radius-sm);
    font-size: 12px;
    color: var(--color-muted);
  }

  .muted-note {
    font-size: 12px;
    color: var(--color-muted);
  }

  .muted {
    color: var(--color-subtle);
    font-size: 12px;
  }
  .user-libraries-fieldset {
    flex-basis: 100%;
    border: 1px solid var(--color-line);
    border-radius: var(--radius-sm);
    padding: 12px 16px;
    margin-top: 4px;
    background: var(--color-surface);
  }
  .user-libraries-fieldset legend {
    font-size: 13px;
    font-weight: 600;
    color: var(--color-text);
    padding: 0 4px;
  }
  .user-libraries-grid {
    display: flex;
    flex-wrap: wrap;
    gap: 12px;
    margin-top: 6px;
  }
  .user-libraries-grid .check {
    display: flex;
    align-items: center;
    gap: 6px;
    margin: 0;
    cursor: pointer;
    font-size: 13px;
  }
</style>
