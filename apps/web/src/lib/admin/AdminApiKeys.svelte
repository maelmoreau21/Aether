<script lang="ts">
  import { onMount } from 'svelte';
  import { adminRequest } from './request';
  import type { AuthUser } from '../auth/session';
  import type { Library } from './types';
  let { users, libraries }: { users: AuthUser[]; libraries: Library[] } = $props();
  interface Key { id: string; name: string; userId: string; expiresAt: number; revokedAt: number | null; libraryIds: string[] }
  let keys = $state<Key[]>([]); let name = $state(''); let userId = $state(''); let libraryIds = $state<string[]>([]);
  let days = $state(30); let token = $state(''); let busy = $state(false); let error = $state(''); let loaded = $state(false);
  async function load() { try { keys = (await adminRequest<{ keys: Key[] }>('/api/v1/admin/api-keys')).keys; loaded = true; } catch (e) { error = e instanceof Error ? e.message : 'Chargement impossible.'; } }
  onMount(() => { void load(); });
  async function create(event: SubmitEvent) {
    event.preventDefault(); if (busy) return; busy = true; error = ''; token = '';
    try { const result = await adminRequest<{ token: string }>('/api/v1/admin/api-keys', { method:'POST', body:JSON.stringify({ name, userId, libraryIds, expiresInDays: days }) }); token = result.token; name = ''; libraryIds = []; await load(); }
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
  <p>Accès en lecture au catalogue pour vos intégrations. Chaque clé expire et reste limitée aux bibliothèques sélectionnées et aux droits actuels de son propriétaire.</p>
  <p>Portée <code>catalog:read</code> · endpoint <code>GET /api/v1/integrations/catalog</code> · authentification <code>Authorization: Bearer …</code>. Aucun accès aux sauvegardes, à l’administration ou aux flux vidéo.</p>
  {#if error}<p role="alert">{error}</p><button onclick={load}>Réessayer</button>{/if}
  {#if token}<aside role="status"><strong>Copiez cette clé maintenant : elle ne sera plus affichée.</strong><textarea readonly aria-label="Nouvelle clé API" value={token}></textarea><button onclick={() => token = ''}>Masquer la clé</button></aside>{/if}
  {#if !loaded}<p role="status">Chargement des clés…</p>{/if}
  <div class="keys">{#each keys as key (key.id)}<article><div><h3>{key.name}</h3><p>{users.find(u => u.id === key.userId)?.login ?? 'Compte désactivé'} · {key.libraryIds.length} bibliothèque(s) · expire le {new Date(key.expiresAt * 1000).toLocaleDateString()}</p><span>{key.revokedAt ? 'Révoquée' : key.expiresAt * 1000 <= Date.now() ? 'Expirée' : 'Active'}</span></div><button disabled={busy || !!key.revokedAt} onclick={() => revoke(key)}>Révoquer</button></article>{:else}{#if loaded}<p>Aucune clé API.</p>{/if}{/each}</div>
  <form onsubmit={create}>
    <h3>Créer une clé</h3>
    <label>Nom <input required maxlength="80" bind:value={name} disabled={busy} /></label>
    <label>Propriétaire <select required bind:value={userId} disabled={busy}><option value="">Choisir un compte</option>{#each users.filter(user => user.disabledAt == null) as user (user.id)}<option value={user.id}>{user.displayName || user.login}</option>{/each}</select></label>
    <label>Expiration en jours <input type="number" min="1" max="365" required bind:value={days} disabled={busy} /></label>
    <fieldset disabled={busy}><legend>Bibliothèques (droits vérifiés par le serveur)</legend>{#each libraries.filter(l => l.enabled) as library (library.id)}<label><input type="checkbox" value={library.id} bind:group={libraryIds} /> {library.name}</label>{/each}</fieldset>
    <button type="submit" disabled={busy || !libraryIds.length}>{busy ? 'Création…' : 'Créer la clé'}</button>
  </form>
</section>

<style>
  h2,h3 { margin:0 0 .8rem; } p { color:var(--color-muted); line-height:1.6; } code { overflow-wrap:anywhere; } article,form,aside { padding:1.4rem; border:1px solid var(--color-line); border-radius:var(--radius-md); background:var(--color-surface); }
  .keys { display:grid; gap:.8rem; margin:1.4rem 0; } article { display:flex; align-items:center; justify-content:space-between; gap:1rem; } article p { margin:.3rem 0; } label { display:block; margin:.8rem 0; }
  input:not([type='checkbox']),select,textarea { display:block; width:100%; max-width:480px; padding:.7rem; background:var(--color-raised); border:1px solid var(--color-line); border-radius:6px; color:var(--color-text); }
  textarea { margin:1rem 0; min-height:100px; } fieldset { border:1px solid var(--color-line); margin:1rem 0; max-height:300px; overflow:auto; }
  button { padding:.65rem .9rem; border:1px solid var(--color-line); border-radius:6px; color:var(--color-text); background:var(--color-raised); cursor:pointer; } button:disabled { opacity:.6; cursor:not-allowed; }
  @media(max-width:640px) { article { flex-direction:column; align-items:flex-start; } }
</style>
