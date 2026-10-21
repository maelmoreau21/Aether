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
    currentUserId, groups = [], onchanged
  }: Props = $props();
  type Account = AuthUser & { disabledAt?: number | null; lastSeenAt?: number | null; hasAvatar?: boolean };
  let failedAvatars = $state<string[]>([]);
  function activityLabel(timestamp: number | null | undefined) {
    if (timestamp == null) return 'Aucune session enregistrée';
    return 'Dernière activité : ' + new Date(timestamp * 1000).toLocaleString($locale === 'en' ? 'en-GB' : 'fr-FR');
  }
  let accounts = $state<Account[] | null>(null);
  const allUsers = $derived(accounts ?? users);
  let busy = $state(false);
  let message = $state('');
  let failed = $state(false);
  let editing = $state<Account | null>(null);
  let editorOpen = $state(false);
  let activeTab = $state<'profile' | 'access' | 'parental' | 'password' | 'media'>('profile');
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
    failedAvatars = [];
  }
  function edit(user: Account | null) {
    editorOpen = true; activeTab = 'profile';
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
      await refresh(); await onchanged?.(); editorOpen = false; editing = null; password = ''; newPassword = '';
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
    const hasAccess = user.disabledAt == null && libraries.some(library => library.enabled && (user.role === 'administrator' || (library.userIds.includes(user.id) || groups.some(group => (group.isDefault || group.userIds.includes(user.id)) && group.libraryIds.includes(library.id)))));
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
    <div><h2 id="users-heading">{$t('users_heading')}</h2><p>{$t('users_subtitle')}</p></div>
    <button type="button" class="btn btn-primary" disabled={busy} onclick={() => edit(null)}>+ Créer un compte local</button>
  </div>

  {#if message}<p class="notice" role={failed ? 'alert' : 'status'}>{message}</p>{/if}
  {#if editorOpen}
    <div class="account-detail">
      <div class="detail-heading">
        <h3>{editing ? editing.displayName || editing.login : 'Créer un compte local'}</h3>
        <button type="button" class="btn btn-secondary" disabled={busy} onclick={() => { editorOpen = false; password = ''; newPassword = ''; }}>Retour aux utilisateurs</button>
      </div>
      {#if editing}
        <nav class="account-tabs" aria-label="Paramètres du compte">
          <button type="button" aria-current={activeTab === 'profile' ? 'page' : undefined} onclick={() => activeTab = 'profile'}>Profil</button>
          <button type="button" aria-current={activeTab === 'access' ? 'page' : undefined} onclick={() => activeTab = 'access'}>Accès</button>
          <button type="button" aria-current={activeTab === 'parental' ? 'page' : undefined} onclick={() => activeTab = 'parental'}>Contrôle parental</button>
          <button type="button" aria-current={activeTab === 'media' ? 'page' : undefined} onclick={() => activeTab = 'media'}>Permissions médias</button>
          <button type="button" aria-current={activeTab === 'password' ? 'page' : undefined} onclick={() => activeTab = 'password'}>Mot de passe</button>
        </nav>
      {/if}
      {#if activeTab === 'profile' || activeTab === 'access'}
        <form class="account-editor" onsubmit={(event) => { event.preventDefault(); void save(); }}>
          <fieldset class="editor-fields" disabled={busy} hidden={activeTab !== 'profile'}>
            <legend>Profil du compte</legend>
            {#if !editing}<label>Identifiant<input required minlength="3" maxlength="64" pattern="[a-z0-9._-]+" autocomplete="off" bind:value={login} /></label>
            {:else}<p>Identifiant : <strong>{editing.login}</strong> · {editing.sso ? 'Connexion SSO' : 'Compte local'}</p>{/if}
            <label>Nom affiché<input maxlength="128" bind:value={displayName} /></label>
            <label>Rôle<select bind:value={role} disabled={editing?.id === currentUserId}><option value="user">Utilisateur</option><option value="administrator">Administrateur</option></select></label>
            {#if editing}
              <label class="check"><input type="checkbox" bind:checked={disabled} disabled={editing.id === currentUserId} /> Compte désactivé</label>
              <p class="muted-note">La désactivation conserve les données et déconnecte le compte. Votre propre compte administrateur est protégé.</p>
              {#if editing.sso}<p class="muted-note">Le rôle SSO peut être réappliqué par le fournisseur à la prochaine connexion.</p>{/if}
            {:else}<label>Mot de passe (15 à 128 caractères)<input type="password" required minlength="15" maxlength="128" autocomplete="new-password" bind:value={password} /></label>{/if}
          </fieldset>
          {#if activeTab === 'access' || !editing}
            <fieldset class="user-libraries-fieldset" disabled={busy}>
              <legend>Accès aux bibliothèques</legend>
              {#if role === 'administrator'}
                <p>Les administrateurs ont accès à toutes les bibliothèques actives.</p>
              {:else}
                <p>Choisissez les bibliothèques accessibles à ce compte. Les accès accordés par un groupe s’ajoutent à cette sélection.</p>
                {#if libraries.length === 0}<p class="muted-note">Aucune bibliothèque configurée.</p>
                {:else}
                  <label class="check select-all"><input type="checkbox" checked={libraries.every(library => libraryIds.includes(library.id))} onchange={(event) => libraryIds = event.currentTarget.checked ? libraries.map(library => library.id) : []} />Sélectionner toutes les bibliothèques actuelles</label>
                  <div class="user-libraries-grid">
                    {#each libraries as library (library.id)}
                      {@const inherited = editing ? groups.filter(group => (group.isDefault || group.userIds.includes(editing?.id ?? '')) && group.libraryIds.includes(library.id)) : []}
                      <div class="library-choice">
                        <label class="check"><input type="checkbox" value={library.id} bind:group={libraryIds} aria-label={'Bibliothèque autorisée : ' + library.name} /><span>{library.name}{library.enabled ? '' : ' (désactivée)'}</span></label>
                        {#if inherited.length}<span class="inherited">Également autorisée par : {inherited.map(group => group.name).join(', ')}</span>{/if}
                      </div>
                    {/each}
                  </div>
                  <p class="muted-note">Les futures bibliothèques ne seront pas ajoutées automatiquement. Décocher une bibliothèque ne retire pas un accès accordé par un groupe.</p>
                {/if}
              {/if}
            </fieldset>
          {/if}
          <div class="editor-actions"><button type="submit" class="btn btn-primary" disabled={busy}>{busy ? 'Enregistrement…' : editing ? 'Enregistrer' : 'Créer le compte'}</button><button type="button" class="btn btn-secondary" disabled={busy} onclick={() => { editorOpen = false; password = ''; newPassword = ''; }}>Annuler</button></div>
        </form>
      {:else if editing && activeTab === 'media'}
        <UserMediaPolicy userId={editing.id} />
      {:else if editing && activeTab === 'parental'}
        <ParentalPolicy userId={editing.id} />
      {:else if editing && activeTab === 'password'}
        {#if editing.sso}<p>Le mot de passe de ce compte est géré par votre fournisseur SSO.</p>
        {:else}<form class="account-editor" onsubmit={(event) => { event.preventDefault(); void save(true); }}>
          <h4>Remplacer le mot de passe local</h4><p>Toutes les sessions de ce compte seront déconnectées.</p>
          <label>Nouveau mot de passe<input type="password" required minlength="15" maxlength="128" autocomplete="new-password" bind:value={newPassword} /></label>
          <button type="submit" class="btn btn-primary" disabled={busy}>Remplacer et révoquer les sessions</button>
        </form>{/if}
      {/if}
    </div>
  {:else}
    <div class="directory-toolbar"><span>{allUsers.length} comptes · {allUsers.filter(user => user.role === 'administrator').length} administrateur(s)</span><button type="button" class="btn btn-secondary" disabled={busy} onclick={() => refresh().catch(() => { failed = true; message = 'Chargement impossible.'; })}>Actualiser les comptes et leurs états</button></div>
    <div class="account-filters">
      <label class="search-filter">{copy.search}<input type="search" bind:value={search} placeholder={copy.placeholder} /></label>
      <label>{copy.role}<select bind:value={roleFilter}><option value="all">{copy.all}</option><option value="administrator">{$t('administrator')}</option><option value="user">{$t('user')}</option></select></label>
      <label>{copy.authentication}<select bind:value={authFilter}><option value="all">{copy.all}</option><option value="local">{$t('local_account')}</option><option value="sso">{$t('sso_badge')}</option></select></label>
      <label>{copy.access}<select bind:value={accessFilter}><option value="all">{copy.all}</option><option value="with">{copy.withAccess}</option><option value="without">{copy.withoutAccess}</option></select></label>
    </div>
    <div class="directory-summary"><span role="status">{visibleUsers.length} / {allUsers.length} {copy.results}</span>{#if search || roleFilter !== 'all' || authFilter !== 'all' || accessFilter !== 'all'}<button type="button" onclick={resetFilters}>{copy.reset}</button>{/if}</div>
    <div class="users-grid">
      {#each visibleUsers as user (user.id)}
        {@const userLibraries = libraries.filter(library => library.enabled && (user.role === 'administrator' || library.userIds.includes(user.id) || groups.some(group => (group.isDefault || group.userIds.includes(user.id)) && group.libraryIds.includes(library.id))))}
        <article class="user-card" class:disabled-account={user.disabledAt != null}>
          <button type="button" class="user-card-open" disabled={busy} onclick={() => edit(user)} aria-label={'Modifier ' + (user.displayName || user.login)}>
            <span class="user-avatar" aria-hidden="true">{#if user.hasAvatar && !failedAvatars.includes(user.id)}<img src={`/api/v1/admin/users/${encodeURIComponent(user.id)}/avatar`} alt="" width="96" height="96" loading="lazy" decoding="async" onerror={() => failedAvatars = [...failedAvatars, user.id]} />{:else}{(user.displayName || user.login).slice(0, 2).toLocaleUpperCase()}{/if}</span>
            <span class="user-name">{user.displayName || user.login}</span>
            <span class="user-login">{user.login}{user.id === currentUserId ? ' · ' + $t('you') : ''}</span>
            <span class="card-badges"><span class="badge" class:admin-role={user.role === 'administrator'}>{user.role === 'administrator' ? $t('administrator') : $t('user')}</span><span class="badge">{user.sso == null ? copy.unknown : user.sso ? $t('sso_badge') : $t('local_account')}</span></span>
            <span class="access-summary">{user.disabledAt != null ? 'Compte désactivé' : user.role === 'administrator' ? $t('all_libs_granted') : `${userLibraries.length} bibliothèque(s) accessible(s)`}</span>
            <span class="access-summary">{activityLabel(user.lastSeenAt)}</span>
            <span class="edit-label">Modifier le compte →</span>
          </button>
          <details class="account-id"><summary>{copy.identifier}</summary><code>{user.id}</code></details>
        </article>
      {:else}<p class="empty-directory">{copy.empty}</p>{/each}
    </div>
  {/if}
  <div class="security-footer-note"><Icon name="shield" size={16} /><span>{copy.limit}<a href="#groups-section">{copy.manage}</a></span></div>
</section>

<style>
  .dashboard-section { margin-bottom:48px;scroll-margin-top:24px; }
  .section-header,.detail-heading,.directory-toolbar,.directory-summary,.editor-actions { display:flex;align-items:center;justify-content:space-between;gap:16px;flex-wrap:wrap; }
  .section-header { margin-bottom:24px;padding-bottom:16px;border-bottom:1px solid var(--color-line); }
  h2,h3,h4,p { margin:0; }
  h2 { font-size:22px; } h3 { font-size:20px; } h4 { font-size:16px; }
  .section-header p,.muted-note,.directory-toolbar,.directory-summary { color:var(--color-muted);font-size:13px; }
  .section-header p { margin-top:6px; }
  .account-detail { padding:24px;background:var(--color-surface);border:1px solid var(--color-line);border-radius:var(--radius-md); }
  .account-tabs { display:flex;gap:8px;overflow-x:auto;border-bottom:1px solid var(--color-line);margin:20px 0; }
  .account-tabs button { border:0;border-bottom:3px solid transparent;background:transparent;color:var(--color-muted);padding:12px;white-space:nowrap;font:inherit;cursor:pointer; }
  .account-tabs button[aria-current="page"] { border-bottom-color:var(--color-accent);color:var(--color-accent); }
  .btn { display:inline-flex;align-items:center;justify-content:center;min-height:42px;padding:10px 16px;border:1px solid var(--color-line);border-radius:var(--radius-sm);font:inherit;cursor:pointer; }
  .btn-primary { background:var(--color-accent);color:var(--color-on-accent);border-color:var(--color-accent); }
  .btn-secondary { background:var(--color-surface);color:var(--color-text); }
  .btn:disabled { opacity:.6;cursor:wait; }
  .account-editor { display:grid;gap:20px;max-width:780px;margin-top:20px; }
  .editor-fields { display:grid;gap:18px;border:0;padding:0;margin:0; }
  .editor-fields[hidden] { display:none; }
  .editor-fields legend { margin-bottom:16px;font-weight:600; }
  .account-editor label,.account-filters label { display:grid;gap:7px;font-size:13px; }
  input:not([type="checkbox"]),select { width:100%;min-height:42px;padding:8px 10px;border:1px solid var(--color-line);border-radius:var(--radius-sm);background:var(--color-raised);color:var(--color-text);font:inherit;box-sizing:border-box; }
  input[type="checkbox"] { width:20px;height:20px;accent-color:var(--color-accent);flex-shrink:0; }
  .account-editor .check { display:flex;align-items:center;gap:10px;cursor:pointer; }
  .user-libraries-fieldset { border:1px solid var(--color-line);border-radius:var(--radius-sm);padding:18px;margin:0; }
  .user-libraries-fieldset legend { padding:0 6px;font-weight:600; }
  .user-libraries-fieldset p { margin:8px 0 16px;font-size:13px;line-height:1.6; }
  .user-libraries-grid { display:grid;gap:1px;background:var(--color-line);border:1px solid var(--color-line);border-radius:var(--radius-sm);overflow:hidden; }
  .library-choice { padding:14px;background:var(--color-raised); }
  .inherited { display:block;color:var(--color-muted);font-size:12px;margin:6px 0 0 30px; }
  .select-all { margin-bottom:16px; }
  .directory-toolbar { margin-bottom:20px; }
  .account-filters { display:flex;flex-wrap:wrap;gap:14px;margin-bottom:14px; }
  .account-filters label { flex:1 1 140px;color:var(--color-muted); }
  .account-filters .search-filter { flex-grow:2; }
  .directory-summary { margin-bottom:20px; }
  .directory-summary button { border:0;background:transparent;color:var(--color-accent);cursor:pointer;font:inherit; }
  .users-grid { display:grid;grid-template-columns:repeat(auto-fill,minmax(190px,1fr));gap:18px; }
  .user-card { min-width:0;overflow:hidden;border:1px solid var(--color-line);border-radius:var(--radius-md);background:var(--color-surface); }
  .user-card-open { display:flex;flex-direction:column;align-items:flex-start;gap:10px;width:100%;padding:20px;border:0;background:transparent;color:var(--color-text);text-align:left;cursor:pointer;font:inherit; }
  .user-card:hover { border-color:var(--color-accent); }
  .user-avatar { display:grid;place-items:center;align-self:center;width:96px;height:96px;border-radius:50%;margin:4px 0 12px;background:var(--color-accent-soft);color:var(--color-accent);font-size:32px;font-weight:650; }
  .user-avatar { overflow:hidden; }
  .user-avatar img { width:100%;height:100%;object-fit:cover; }
  .user-name { font-size:17px;font-weight:600;overflow-wrap:anywhere; }
  .user-login,.access-summary { color:var(--color-muted);font-size:12px;overflow-wrap:anywhere; }
  .card-badges { display:flex;flex-wrap:wrap;gap:6px; }
  .badge { font-size:11px;padding:4px 8px;border-radius:99px;background:var(--color-raised);color:var(--color-muted); }
  .admin-role { color:var(--color-accent);background:var(--color-accent-soft); }
  .disabled-account .user-avatar { color:var(--color-muted);background:var(--color-raised); }
  .edit-label { color:var(--color-accent);font-size:12px;margin-top:4px; }
  .account-id { margin:0 20px 16px;color:var(--color-muted);font-size:11px; }
  .account-id summary { cursor:pointer; }
  .account-id code { display:block;margin-top:8px;overflow-wrap:anywhere; }
  .empty-directory { padding:32px;color:var(--color-muted); }
  .security-footer-note { display:flex;align-items:flex-start;gap:12px;margin-top:24px;padding:16px;background:var(--color-raised);border-radius:var(--radius-sm);font-size:12px;color:var(--color-muted);line-height:1.6; }
  .security-footer-note a { display:block;margin-top:8px;color:var(--color-accent); }
  .notice { margin-bottom:20px;padding:14px;background:var(--color-raised);border-radius:var(--radius-sm); }
  :is(input,select,button,a,summary):focus-visible { outline:2px solid var(--color-accent);outline-offset:3px; }
  @media(max-width:600px) { .account-detail { padding:16px; }.users-grid { grid-template-columns:repeat(auto-fill,minmax(145px,1fr));gap:12px; }.user-card-open { padding:14px; }.user-avatar { width:76px;height:76px;font-size:26px; }.account-tabs { gap:0; }.account-tabs button { padding:10px 8px;font-size:13px; } }
</style>
