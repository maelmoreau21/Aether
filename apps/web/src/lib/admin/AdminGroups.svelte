<script lang="ts">
  import { adminRequest, type UserGroup } from './request';
  import type { AuthUser } from '../auth/session';
  import type { Library } from './types';
  let { groups, users, libraries, onchanged }: { groups: UserGroup[]; users: AuthUser[]; libraries: Library[]; onchanged: () => Promise<void> } = $props();
  let name = $state(''); let isDefault = $state(false); let userIds = $state<string[]>([]); let libraryIds = $state<string[]>([]);
  let editing = $state<string | null>(null); let busy = $state(false); let error = $state(''); let notice = $state('');
  function reset() { editing = null; name = ''; isDefault = false; userIds = []; libraryIds = []; }
  function edit(group: UserGroup) { editing = group.id; name = group.name; isDefault = !!group.isDefault; userIds = [...group.userIds]; libraryIds = [...group.libraryIds]; }
  async function save(event: SubmitEvent) {
    event.preventDefault(); if (busy) return; busy = true; error = ''; notice = '';
    try {
      await adminRequest(editing ? `/api/v1/admin/groups/${encodeURIComponent(editing)}` : '/api/v1/admin/groups', { method: editing ? 'PUT' : 'POST', body: JSON.stringify({ name: name.trim(), isDefault, userIds, libraryIds }) });
      reset(); await onchanged(); notice = 'Groupe enregistré. Les droits sont appliqués aux prochaines requêtes.';
    } catch (e) { error = e instanceof Error ? e.message : 'Enregistrement impossible.'; } finally { busy = false; }
  }
  async function remove(group: UserGroup) {
    if (busy || !window.confirm(`Supprimer le groupe « ${group.name} » et ses accès hérités ? Les comptes seront conservés.`)) return;
    busy = true; error = ''; notice = '';
    try { await adminRequest(`/api/v1/admin/groups/${encodeURIComponent(group.id)}`, { method: 'DELETE' }); if (editing === group.id) reset(); await onchanged(); notice = 'Groupe supprimé. Les autres permissions sont conservées.'; }
    catch (e) { error = e instanceof Error ? e.message : 'Suppression impossible.'; } finally { busy = false; }
  }
</script>

<section id="groups-section" aria-labelledby="groups-heading">
  <h2 id="groups-heading">Groupes d’utilisateurs</h2>
  <p>Accordez des bibliothèques à plusieurs comptes. Les droits individuels et ceux des groupes s’ajoutent ; un groupe ne change jamais le rôle d’un compte.</p>
  {#if error}<p role="alert">{error}</p>{/if}
  {#if notice}<p role="status">{notice}</p>{/if}
  <div class="groups">
    {#each groups as group (group.id)}
      <article><div><h3>{group.name}{#if group.isDefault}<span class="default-badge">Par défaut</span>{/if}</h3><p>{group.isDefault ? 'Tous les utilisateurs (par défaut)' : `${group.userIds.length} membre(s)`} · {group.libraryIds.length} bibliothèque(s)</p></div>
        <div class="actions"><button disabled={busy} onclick={() => edit(group)}>Modifier</button><button disabled={busy} onclick={() => remove(group)}>Supprimer</button></div>
      </article>
    {:else}<p>Aucun groupe local. Créez votre premier groupe ci-dessous.</p>{/each}
  </div>
  <form onsubmit={save}>
    <h3>{editing ? 'Modifier le groupe' : 'Créer un groupe'}</h3>
    <label>Nom <input required maxlength="80" bind:value={name} disabled={busy} /></label>
    <label class="check-label"><input type="checkbox" bind:checked={isDefault} disabled={busy} /> <span><strong>Groupe par défaut</strong> — tous les utilisateurs (existants et futurs) font automatiquement partie de ce groupe et accèdent à ses bibliothèques.</span></label>
    <div class="choices">
      <fieldset disabled={busy || isDefault}><legend>Membres {isDefault ? '(automatique : tous)' : ''}</legend>{#each users as user (user.id)}<label><input type="checkbox" value={user.id} bind:group={userIds} disabled={isDefault} aria-label={"Membre : " + (user.displayName || user.login)} /> {user.displayName || user.login}</label>{/each}</fieldset>
      <fieldset disabled={busy}><legend>Bibliothèques autorisées</legend>{#each libraries as library (library.id)}<label><input type="checkbox" value={library.id} bind:group={libraryIds} aria-label={"Bibliothèque autorisée : " + library.name} /> {library.name}{library.enabled ? '' : ' (désactivée)'}</label>{/each}</fieldset>
    </div>
    <div class="actions"><button disabled={busy} type="submit">{busy ? 'Enregistrement…' : 'Enregistrer'}</button>{#if editing}<button disabled={busy} type="button" onclick={reset}>Annuler</button>{/if}</div>
  </form>
  <p>Ces groupes locaux sont distincts des groupes OIDC gérés chez votre fournisseur SSO.</p>
</section>

<style>
  h2,h3 { margin:0 0 .8rem; } p { color:var(--color-muted); line-height:1.6; } .groups { display:grid; gap:.8rem; margin:1.5rem 0; }
  article,form { padding:1.4rem; border:1px solid var(--color-line); border-radius:var(--radius-md); background:var(--color-surface); }
  article { display:flex; align-items:center; justify-content:space-between; gap:1rem; } article p { margin:0; } .actions { display:flex; gap:.6rem; flex-wrap:wrap; }
  label { display:block; margin:.6rem 0; } input:not([type='checkbox']) { display:block; width:100%; max-width:400px; padding:.7rem; background:var(--color-raised); border:1px solid var(--color-line); border-radius:6px; color:var(--color-text); }
  .choices { display:grid; grid-template-columns:1fr 1fr; gap:1rem; margin:1rem 0; } fieldset { min-width:0; border:1px solid var(--color-line); max-height:320px; overflow:auto; }
  button { padding:.65rem .9rem; border:1px solid var(--color-line); border-radius:6px; color:var(--color-text); background:var(--color-raised); cursor:pointer; } button:disabled { opacity:.6; cursor:wait; }
  .default-badge { display:inline-block; font-size:11px; padding:2px 8px; border-radius:999px; background:var(--color-primary, #6366f1); color:#fff; font-weight:600; vertical-align:middle; margin-left:8px; }
  .check-label { display:flex; align-items:flex-start; gap:8px; margin:10px 0; font-size:14px; cursor:pointer; }
  .check-label input { width:auto; margin-top:3px; }
  @media(max-width:640px) { .choices { grid-template-columns:1fr; } article { align-items:flex-start; flex-direction:column; } }
</style>
