<script lang="ts">
  import type { Library, LibraryOptions } from './types';
  import { adminRequest, type UserGroup } from './request';
  let { library, groups, options = $bindable(), onchanged }: {
    library: Library | null; groups: UserGroup[]; options: LibraryOptions; onchanged: () => Promise<void>;
  } = $props();

  let groupIds = $state<string[]>([]);
  let busy = $state(false), message = $state(''), failed = $state(false);

  // Re-read the server state whenever the edited library or the account data changes.
  $effect(() => {
    const id = library?.id;
    groupIds = id ? groups.filter(group => group.libraryIds.includes(id)).map(group => group.id) : [];
    message = '';
  });

  const initialGroups = $derived(library ? groups.filter(group => group.libraryIds.includes(library.id)).map(group => group.id) : []);
  const dirty = $derived(groupIds.length !== initialGroups.length || groupIds.some(id => !initialGroups.includes(id)));

  /** Called by the library editor's « Enregistrer » before the library itself is saved. */
  export async function saveGroups(): Promise<boolean> {
    if (!library || busy || !dirty) return true;
    const id = library.id;
    busy = true; message = ''; failed = false;
    try {
      for (const group of groups) {
        if (group.systemRole === 'administrator') continue;
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
      await onchanged();
      return true;
    } catch (error) {
      failed = true;
      message = error instanceof Error ? error.message : 'Enregistrement des accès impossible.';
      return false;
    } finally { busy = false; }
  }
</script>

<h3>Accès à la bibliothèque</h3>
<p class="lead">Choisissez les groupes qui voient cette bibliothèque. Les changements s’appliquent avec « Enregistrer ».</p>
{#if !library}
  <p class="notice">Enregistrez d’abord la bibliothèque, puis rouvrez-la pour attribuer les accès.</p>
{:else}
  <fieldset disabled={busy}>
    <legend>Groupes</legend>
    {#each groups as group (group.id)}
      {#if group.systemRole === 'administrator'}
        <label class="check"><input type="checkbox" bind:checked={() => options.adminAccess !== false, value => options.adminAccess = value} /><span>{group.name}<small>{group.userIds.length} membre(s) · {options.adminAccess === false ? 'aucun accès aux médias (sauf via un groupe coché) · la gestion reste possible' : 'accès à tous les médias de la bibliothèque'}</small></span></label>
      {:else}
        <label class="check"><input type="checkbox" value={group.id} bind:group={groupIds} /><span>{group.name}<small>{group.userIds.length} membre(s){group.isDefault ? ' · groupe par défaut' : ''}</small></span></label>
      {/if}
    {:else}<p class="hint">Aucun groupe. Créez-en dans Groupes.</p>{/each}
  </fieldset>
  {#if message}<p role={failed ? 'alert' : 'status'} class:failed>{message}</p>{/if}
{/if}
<p class="hint">Les médias restent en lecture seule : ajouter un dossier n’accorde aucun droit supplémentaire.</p>

<style>
  h3 { margin: 0; font-size: 17px; font-weight: 700; }
  p { margin: 8px 0 0; color: var(--mu); font-size: 14px; line-height: 1.6; }
  p.failed { color: var(--color-danger, #e5484d); }
  .hint { font-size: 13px; }
  .notice { margin-top: 16px; padding: 12px 14px; border-radius: var(--r-sm); background: var(--color-surface); }
  fieldset { min-width: 0; margin: 18px 0 0; padding: 0 0 12px; border: 0; border-bottom: 1px solid var(--color-line-subtle); }
  legend { padding: 0; margin-bottom: 4px; color: var(--mu); font-size: 13px; font-weight: 700; letter-spacing: 0.04em; text-transform: uppercase; }
  .check { display: flex; align-items: flex-start; gap: 12px; min-height: 44px; padding: 8px 0; font-size: 14px; font-weight: 600; cursor: pointer; }
  .check:has(input:disabled) { cursor: not-allowed; opacity: 0.7; }
  small { display: block; margin-top: 2px; color: var(--mu); font-size: 13px; font-weight: 400; }
  input[type='checkbox'] { flex: none; width: 20px; height: 20px; margin: 1px 0 0; accent-color: var(--ac); }
  input:focus-visible { outline: 2px solid var(--ac); outline-offset: 3px; }
</style>
