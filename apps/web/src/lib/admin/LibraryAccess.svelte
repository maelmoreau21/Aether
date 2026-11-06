<script lang="ts">
  import type { AuthUser } from '../auth/session';
  import type { Library } from './types';
  import { adminRequest, type UserGroup } from './request';
  let { library, libraries, users, groups, onchanged }: {
    library: Library | null; libraries: Library[]; users: AuthUser[]; groups: UserGroup[]; onchanged: () => Promise<void>;
  } = $props();

  const members = $derived(users.filter(user => user.role !== 'administrator'));
  const administrators = $derived(users.filter(user => user.role === 'administrator'));
  let groupIds = $state<string[]>([]);
  let userIds = $state<string[]>([]);
  let busy = $state(false), message = $state(''), failed = $state(false);

  // Re-read the server state whenever the edited library or the account data changes.
  $effect(() => {
    const id = library?.id;
    groupIds = id ? groups.filter(group => group.libraryIds.includes(id)).map(group => group.id) : [];
    userIds = id ? [...(library?.userIds ?? [])] : [];
    message = '';
  });

  const initialGroups = $derived(library ? groups.filter(group => group.libraryIds.includes(library.id)).map(group => group.id) : []);
  const initialUsers = $derived(library?.userIds ?? []);
  const dirty = $derived(
    groupIds.length !== initialGroups.length || groupIds.some(id => !initialGroups.includes(id)) ||
    userIds.length !== initialUsers.length || userIds.some(id => !initialUsers.includes(id))
  );
  const viaGroups = $derived(new Set(groups.filter(group => groupIds.includes(group.id)).flatMap(group => group.userIds)));

  async function save() {
    if (!library || busy) return;
    const id = library.id;
    busy = true; message = ''; failed = false;
    try {
      for (const group of groups) {
        const had = group.libraryIds.includes(id), wants = groupIds.includes(group.id);
        if (had === wants) continue;
        await adminRequest(`/api/v1/admin/groups/${encodeURIComponent(group.id)}`, {
          method: 'PUT',
          body: JSON.stringify({
            name: group.name,
            isDefault: group.isDefault ?? false,
            userIds: group.systemRole ? [] : group.userIds,
            libraryIds: wants ? [...group.libraryIds, id] : group.libraryIds.filter(item => item !== id),
            ...(group.allowedKinds ? { allowedKinds: group.allowedKinds } : {})
          })
        });
      }
      for (const user of members) {
        const had = initialUsers.includes(user.id), wants = userIds.includes(user.id);
        if (had === wants) continue;
        const current = libraries.filter(item => item.userIds.includes(user.id)).map(item => item.id);
        await adminRequest(`/api/v1/admin/users/${encodeURIComponent(user.id)}`, {
          method: 'PUT',
          body: JSON.stringify({
            role: user.role,
            disabled: user.disabledAt != null,
            displayName: user.displayName ?? null,
            libraryIds: wants ? [...current, id] : current.filter(item => item !== id)
          })
        });
      }
      await onchanged();
      message = 'Accès enregistrés.';
    } catch (error) {
      failed = true;
      message = error instanceof Error ? error.message : 'Enregistrement des accès impossible.';
    } finally { busy = false; }
  }
</script>

<h3>Accès à la bibliothèque</h3>
<p class="lead">Choisissez les groupes et les comptes qui voient cette bibliothèque. Les administrateurs y ont toujours accès.</p>
{#if !library}
  <p class="notice">Enregistrez d’abord la bibliothèque, puis rouvrez-la pour attribuer les accès.</p>
{:else}
  <fieldset disabled={busy}>
    <legend>Groupes</legend>
    {#each groups as group (group.id)}
      <label class="check"><input type="checkbox" value={group.id} bind:group={groupIds} /><span>{group.name}<small>{group.userIds.length} membre(s){group.isDefault ? ' · groupe par défaut' : ''}</small></span></label>
    {:else}<p class="hint">Aucun groupe. Créez-en dans Groupes.</p>{/each}
  </fieldset>
  <fieldset disabled={busy}>
    <legend>Comptes</legend>
    {#each members as user (user.id)}
      <label class="check"><input type="checkbox" value={user.id} bind:group={userIds} /><span>{user.displayName || user.login}<small>{user.login}{user.disabledAt != null ? ' · désactivé' : ''}{viaGroups.has(user.id) && !userIds.includes(user.id) ? ' · accès via un groupe' : ''}</small></span></label>
    {:else}<p class="hint">Aucun compte utilisateur.</p>{/each}
    {#if administrators.length}<p class="hint">Accès complet : {administrators.map(user => user.displayName || user.login).join(', ')}</p>{/if}
  </fieldset>
  <div class="actions">
    {#if message}<p role={failed ? 'alert' : 'status'} class:failed>{message}</p>{/if}
    <button type="button" class="primary" disabled={busy || !dirty} onclick={save}>{busy ? 'Enregistrement…' : 'Enregistrer les accès'}</button>
  </div>
{/if}
<p class="hint">Les médias restent en lecture seule : ajouter un dossier n’accorde aucun droit supplémentaire.</p>

<style>
  h3 { margin: 0; font-size: 17px; font-weight: 700; }
  p { margin: 8px 0 0; color: var(--mu); font-size: 14px; line-height: 1.6; }
  .hint { font-size: 13px; }
  .notice { margin-top: 16px; padding: 12px 14px; border-radius: var(--r-sm); background: var(--color-surface); }
  fieldset { min-width: 0; margin: 18px 0 0; padding: 0 0 12px; border: 0; border-bottom: 1px solid var(--color-line-subtle); }
  legend { padding: 0; margin-bottom: 4px; color: var(--mu); font-size: 13px; font-weight: 700; letter-spacing: 0.04em; text-transform: uppercase; }
  .check { display: flex; align-items: flex-start; gap: 12px; min-height: 44px; padding: 8px 0; font-size: 14px; font-weight: 600; cursor: pointer; }
  .check:has(input:disabled) { cursor: not-allowed; opacity: 0.7; }
  small { display: block; margin-top: 2px; color: var(--mu); font-size: 13px; font-weight: 400; }
  input[type='checkbox'] { flex: none; width: 20px; height: 20px; margin: 1px 0 0; accent-color: var(--ac); }
  .actions { display: flex; flex-wrap: wrap; align-items: center; justify-content: flex-end; gap: 12px; margin-top: 16px; }
  .actions p { margin: 0 auto 0 0; }
  .actions p.failed { color: var(--color-danger, #e5484d); }
  button { min-height: 44px; padding: 0 18px; border: 1px solid var(--color-line); border-radius: var(--r-md); background: transparent; color: var(--tx); font: inherit; font-size: 14px; font-weight: 600; cursor: pointer; }
  button.primary { border-color: var(--ac); background: var(--ac); color: var(--on-ac); }
  button:disabled { opacity: 0.5; cursor: not-allowed; }
  button:focus-visible, input:focus-visible { outline: 2px solid var(--ac); outline-offset: 3px; }
</style>
