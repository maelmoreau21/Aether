<script lang="ts">
  import { onMount } from 'svelte';
  import { adminRequest } from './request';
  import type { AuthUser } from '../auth/session';
  import type { Library } from './types';
  let { users, libraries }: { users: AuthUser[]; libraries: Library[] } = $props();
  interface Key { id: string; name: string; userId: string; expiresAt: number | null; revokedAt: number | null; libraryIds: string[] }
  let keys = $state<Key[]>([]); let name = $state(''); let userId = $state(''); let libraryIds = $state<string[]>([]);
  let days = $state(30); let neverExpires = $state(false); let token = $state(''); let busy = $state(false); let error = $state(''); let loaded = $state(false);
  async function load() { try { keys = (await adminRequest<{ keys: Key[] }>('/api/v1/admin/api-keys')).keys; loaded = true; } catch (e) { error = e instanceof Error ? e.message : 'Chargement impossible.'; } }
  onMount(() => { void load(); });
  async function create(event: SubmitEvent) {
    event.preventDefault(); if (busy) return; busy = true; error = ''; token = '';
    try { const result = await adminRequest<{ token: string }>('/api/v1/admin/api-keys', { method:'POST', body:JSON.stringify({ name, userId, libraryIds, expiresInDays: neverExpires ? null : days, neverExpires }) }); token = result.token; name = ''; libraryIds = []; await load(); }
    catch (e) { error = e instanceof Error ? e.message : 'Création impossible.'; } finally { busy = false; }
  }
  async function revoke(key: Key) {
    if (busy || !window.confirm(`Révoquer définitivement « ${key.name} » ?`)) return; busy = true; error = '';
    try { await adminRequest(`/api/v1/admin/api-keys/${encodeURIComponent(key.id)}`, { method: 'DELETE' }); token = ''; await load(); }
    catch (e) { error = e instanceof Error ? e.message : 'Révocation impossible.'; } finally { busy = false; }
  }
</script>

<section id="api-keys-section" aria-labelledby="api-keys-heading">
  <h2 id="api-keys-heading">Clés API</h2>
  <p>Accès en lecture au catalogue pour vos intégrations. Choisissez une durée ou « Sans expiration ». Chaque clé reste révocable et limitée aux bibliothèques sélectionnées et aux droits actuels de son propriétaire.</p>
  <p>Portée <code>catalog:read</code> · endpoint <code>GET /api/v1/integrations/catalog</code> · authentification <code>Authorization: Bearer …</code>. Aucun accès aux sauvegardes, à l’administration ou aux flux vidéo.</p>
  {#if error}<p role="alert">{error}</p><button onclick={load}>Réessayer</button>{/if}
  {#if token}<aside role="status"><strong>Copiez cette clé maintenant : elle ne sera plus affichée.</strong><textarea readonly aria-label="Nouvelle clé API" value={token}></textarea><button onclick={() => token = ''}>Masquer la clé</button></aside>{/if}
  {#if !loaded}<p role="status">Chargement des clés…</p>{/if}
  <div class="keys">{#each keys as key (key.id)}<article><div><h3>{key.name}</h3><p>{users.find(u => u.id === key.userId)?.login ?? 'Compte désactivé'} · {key.libraryIds.length} bibliothèque(s) · {key.expiresAt === null ? 'Sans expiration' : `expire le ${new Date(key.expiresAt * 1000).toLocaleDateString()}`}</p><span>{key.revokedAt ? 'Révoquée' : key.expiresAt !== null && key.expiresAt * 1000 <= Date.now() ? 'Expirée' : 'Active'}</span></div><button disabled={busy || !!key.revokedAt} onclick={() => revoke(key)}>Révoquer</button></article>{:else}{#if loaded}<p>Aucune clé API.</p>{/if}{/each}</div>
  <form onsubmit={create}>
    <h3>Créer une clé</h3>
    <label>Nom <input required maxlength="80" bind:value={name} disabled={busy} /></label>
    <label>Propriétaire <select required bind:value={userId} disabled={busy}><option value="">Choisir un compte</option>{#each users.filter(user => user.disabledAt == null) as user (user.id)}<option value={user.id}>{user.displayName || user.login}</option>{/each}</select></label>
    <label><input type="checkbox" bind:checked={neverExpires} disabled={busy} /> Sans expiration (jusqu’à révocation)</label>
    <p>Une clé sans expiration reste révocable. Ce choix ne supprime ni les limites de portée ni celles d’un service externe.</p>
    <label>Expiration en jours <input type="number" min="1" max="365" required={!neverExpires} bind:value={days} disabled={busy || neverExpires} /></label>
    <fieldset disabled={busy}><legend>Bibliothèques (droits vérifiés par le serveur)</legend>{#each libraries.filter(l => l.enabled) as library (library.id)}<label><input type="checkbox" value={library.id} bind:group={libraryIds} /> {library.name}</label>{/each}</fieldset>
    <button type="submit" disabled={busy || !libraryIds.length}>{busy ? 'Création…' : 'Créer la clé'}</button>
  </form>
</section>

<style>
section { display: grid; gap: 16px; min-width: 0; font-family: var(--font); color: var(--tx); }
  h2 { margin: 0; font-size: 24px; font-weight: 800; letter-spacing: -0.02em; line-height: 1.2; }
  h3 { margin: 0; font-size: 17px; font-weight: 700; }
  p { max-width: 760px; margin: 0; color: var(--mu); font-size: 14px; line-height: 1.6; }
  code { overflow-wrap: anywhere; font-family: var(--font-mono); font-size: 13px; color: var(--tx); }
  p[role='alert'] { max-width: none; padding: 12px 16px; border-radius: var(--r-md); background: var(--err-soft); color: var(--tx); }
  p[role='status'] { color: var(--mu); }

  /* Clé tout juste créée */
  aside { display: grid; gap: 12px; padding: 20px; border-radius: var(--r-lg); background: var(--warn-soft); }
  aside strong { font-size: 15px; font-weight: 700; }
  textarea {
    box-sizing: border-box;
    width: 100%;
    min-height: 100px;
    padding: 12px 14px;
    border: 1px solid transparent;
    border-radius: var(--r-sm);
    background: var(--bg-3);
    color: var(--tx);
    font-family: var(--font-mono);
    font-size: 13px;
    resize: vertical;
  }
  textarea:focus { outline: none; border-color: var(--ac); }

  /* Liste des clés */
  .keys { padding: 4px 24px; border: 1px solid var(--line); border-radius: var(--r-lg); background: var(--bg-2); }
  .keys:empty { display: none; }
  .keys > p { padding: 20px 0; }
  article { display: flex; align-items: center; justify-content: space-between; gap: 16px; min-height: 60px; padding: 14px 0; border-bottom: 1px solid var(--line); }
  article:last-child { border-bottom: 0; }
  article > div { min-width: 0; }
  article h3 { font-size: 15px; overflow-wrap: anywhere; }
  article p { margin: 4px 0 8px; font-size: 13px; }
  article span { display: inline-flex; align-items: center; height: 24px; padding: 0 9px; border-radius: var(--r-xs); background: var(--pn-2); color: var(--tx); font-size: 12px; font-weight: 700; }

  /* Formulaire de création */
  form { display: grid; gap: 16px; padding: 24px; border: 1px solid var(--line); border-radius: var(--r-lg); background: var(--bg-2); }
  form p { font-size: 13px; line-height: 1.5; margin-top: -8px; }
  label { display: grid; gap: 8px; min-width: 0; font-size: 14px; font-weight: 700; }
  label:has(> input[type='checkbox']) { display: flex; align-items: center; gap: 12px; min-height: 44px; font-weight: 600; cursor: pointer; }
  input[type='checkbox'] { flex: none; width: 20px; height: 20px; margin: 0; accent-color: var(--ac); }
  input:not([type='checkbox']), select {
    box-sizing: border-box;
    width: 100%;
    max-width: 480px;
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
  select { padding-right: 40px; }
  input:not([type='checkbox']):hover, select:hover { background-color: var(--pn-2); }
  input:not([type='checkbox']):focus, select:focus { outline: none; border-color: var(--ac); background-color: var(--bg-3); }
  input:disabled, select:disabled { opacity: 0.45; cursor: not-allowed; }
  fieldset { display: grid; gap: 0; max-height: 300px; min-width: 0; margin: 0; padding: 4px 16px; overflow: auto; border: 1px solid var(--line); border-radius: var(--r-md); }
  fieldset label { min-height: 48px; border-bottom: 1px solid var(--line); }
  fieldset label:last-child { border-bottom: 0; }
  legend { padding: 0 6px; font-size: 14px; font-weight: 700; }

  /* Boutons */
  button {
    justify-self: start;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    flex: none;
    height: 44px;
    padding: 0 18px;
    border: 0;
    border-radius: var(--r-md);
    background: var(--pn-2);
    color: var(--tx);
    font: inherit;
    font-size: 14px;
    font-weight: 700;
    cursor: pointer;
    transition: background-color var(--t-fast) var(--ease), opacity var(--t-fast) var(--ease), transform var(--t-fast) var(--ease);
  }
  button:hover:not(:disabled) { background: var(--pn-3); }
  button:active:not(:disabled) { transform: scale(0.98); }
  button:focus-visible { outline: 2px solid var(--ac); outline-offset: 3px; }
  button:disabled { opacity: 0.45; cursor: not-allowed; }
  article button { background: var(--err-soft); color: var(--err); }
  article button:hover:not(:disabled) { background: var(--pn-2); }
  button[type='submit'] { height: 48px; padding: 0 22px; background: var(--btn); color: var(--btn-tx); }
  button[type='submit']:hover:not(:disabled) { background: var(--btn); opacity: 0.9; }

  @media (max-width: 640px) {
    .keys { padding: 4px 16px; border-radius: var(--r-md); }
    form { padding: 16px; border-radius: var(--r-md); }
    article { flex-direction: column; align-items: flex-start; }
    input:not([type='checkbox']), select { max-width: none; }
    button[type='submit'] { width: 100%; justify-self: stretch; }
  }
  @media (prefers-reduced-motion: reduce) {
    button, input, select { transition: none; }
  }
</style>
