<script lang="ts">
  import { onMount } from 'svelte';
  import { adminRequest } from './request';
  import type { AuthUser } from '../auth/session';
  import type { Library } from './types';
  import { t, fmt } from '../i18n';
  let { users, libraries }: { users: AuthUser[]; libraries: Library[] } = $props();
  interface Key { id: string; name: string; userId: string; expiresAt: number | null; revokedAt: number | null; libraryIds: string[] }
  let keys = $state<Key[]>([]); let name = $state(''); let userId = $state(''); let libraryIds = $state<string[]>([]);
  let days = $state(30); let neverExpires = $state(false); let token = $state(''); let busy = $state(false); let error = $state(''); let loaded = $state(false);
  async function load() { try { keys = (await adminRequest<{ keys: Key[] }>('/api/v1/admin/api-keys')).keys; loaded = true; } catch (e) { error = e instanceof Error ? e.message : $t('loading_impossible'); } }
  onMount(() => { void load(); });
  async function create(event: SubmitEvent) {
    event.preventDefault(); if (busy) return; busy = true; error = ''; token = '';
    try { const result = await adminRequest<{ token: string }>('/api/v1/admin/api-keys', { method:'POST', body:JSON.stringify({ name, userId, libraryIds, expiresInDays: neverExpires ? null : days, neverExpires }) }); token = result.token; name = ''; libraryIds = []; await load(); }
    catch (e) { error = e instanceof Error ? e.message : $t('creation_failed'); } finally { busy = false; }
  }
  async function revoke(key: Key) {
    if (busy || !window.confirm(`Révoquer définitivement « ${key.name} » ?`)) return; busy = true; error = '';
    try { await adminRequest(`/api/v1/admin/api-keys/${encodeURIComponent(key.id)}`, { method: 'DELETE' }); token = ''; await load(); }
    catch (e) { error = e instanceof Error ? e.message : $t('revocation_failed'); } finally { busy = false; }
  }
</script>

<section id="api-keys-section" aria-labelledby="api-keys-heading">
  <h2 id="api-keys-heading">{$t('admin_api_keys')}</h2>
  <p>{$t('read_only_catalogue_access_for_your')}</p>
  <p>{$t('scope')} <code>catalog:read</code> {$t('endpoint')} <code>GET /api/v1/integrations/catalog</code> {$t('authentication')} <code>Authorization: Bearer …</code>{$t('no_access_to_backups_administration_or')}</p>
  {#if error}<p role="alert">{error}</p><button onclick={load}>{$t('request_retry')}</button>{/if}
  {#if token}<aside role="status"><strong>{$t('copy_this_key_now_it_will')}</strong><textarea readonly aria-label={$t('new_api_key')} value={token}></textarea><button onclick={() => token = ''}>{$t('hide_key')}</button></aside>{/if}
  {#if !loaded}<p role="status">{$t('loading_keys')}</p>{/if}
  <div class="keys">{#each keys as key (key.id)}<article><div><h3>{key.name}</h3><p>{users.find(u => u.id === key.userId)?.login ?? $t('account_disabled')} · {$t('library_count', { count: key.libraryIds.length })} · {key.expiresAt === null ? $t('no_expiry') : $t('key_expires_on', { date: $fmt.date(key.expiresAt) })}</p><span>{key.revokedAt ? $t('key_revoked') : key.expiresAt !== null && key.expiresAt * 1000 <= Date.now() ? $t('key_expired') : $t('active_state')}</span></div><button disabled={busy || !!key.revokedAt} onclick={() => revoke(key)}>{$t('revoke')}</button></article>{:else}{#if loaded}<p>{$t('no_api_keys')}</p>{/if}{/each}</div>
  <form onsubmit={create}>
    <h3>{$t('create_a_key')}</h3>
    <label>{$t('name')} <input required maxlength="80" bind:value={name} disabled={busy} /></label>
    <label>{$t('owner')} <select required bind:value={userId} disabled={busy}><option value="">{$t('choose_an_account')}</option>{#each users.filter(user => user.disabledAt == null) as user (user.id)}<option value={user.id}>{user.displayName || user.login}</option>{/each}</select></label>
    <label><input type="checkbox" bind:checked={neverExpires} disabled={busy} /> {$t('no_expiry_until_revoked')}</label>
    <p>{$t('a_key_without_expiry_can_still')}</p>
    <label>{$t('expiry_in_days')} <input type="number" min="1" max="365" required={!neverExpires} bind:value={days} disabled={busy || neverExpires} /></label>
    <fieldset disabled={busy}><legend>{$t('libraries_rights_checked_by_the_server')}</legend>{#each libraries.filter(l => l.enabled) as library (library.id)}<label><input type="checkbox" value={library.id} bind:group={libraryIds} /> {library.name}</label>{/each}</fieldset>
    <button type="submit" disabled={busy || !libraryIds.length}>{busy ? 'Création…' : $t('create_the_key')}</button>
  </form>
</section>

<style>
  section { display: grid; gap: 20px; min-width: 0; max-width: 760px; font-family: var(--font-sans); color: var(--color-text); }
  h2 { margin: 0; font-size: 26px; font-weight: 700; letter-spacing: -0.02em; line-height: 1.2; }
  h3 { margin: 0; font-size: 16px; font-weight: 600; }
  p { margin: 0; font-size: 13px; line-height: 1.6; color: var(--color-muted); }
  code { overflow-wrap: anywhere; font-family: ui-monospace, monospace; font-size: 12px; color: var(--color-text); }
  p[role='alert'] { padding: 12px 16px; border-radius: var(--radius-sm); background: var(--color-danger-soft); color: var(--color-text); }

  /* Clé tout juste créée */
  aside { display: grid; gap: 12px; padding: 20px; border-radius: var(--radius-md); background: var(--color-warning-soft); }
  aside strong { font-size: 14px; font-weight: 600; }
  textarea { box-sizing: border-box; width: 100%; min-height: 90px; padding: 10px 12px; border: 1px solid var(--color-line-subtle); border-radius: var(--radius-sm); background: var(--color-raised); color: var(--color-text); font-family: ui-monospace, monospace; font-size: 12px; resize: vertical; }
  textarea:focus-visible { outline: 2px solid var(--color-accent); outline-offset: 1px; }

  /* Liste des clés : lignes fines sur surface plate */
  .keys { padding: 0 20px; border-radius: var(--radius-md); background: var(--color-surface); }
  .keys:empty { display: none; }
  .keys > p { padding: 16px 0; }
  article { display: flex; align-items: center; justify-content: space-between; gap: 16px; padding: 14px 0; border-top: 1px solid var(--color-line-subtle); }
  article:first-child { border-top: 0; }
  article > div { min-width: 0; }
  article h3 { font-size: 15px; overflow-wrap: anywhere; }
  article p { margin: 4px 0; }
  article span { font-size: 12px; font-weight: 600; color: var(--color-accent); }

  /* Formulaire de création : lignes label + champ */
  form { display: grid; padding: 4px 20px 20px; border-radius: var(--radius-md); background: var(--color-surface); }
  form > h3 { padding: 16px 0 8px; }
  form > p { padding: 0 0 8px; }
  label { display: grid; gap: 8px; min-width: 0; padding: 14px 0; border-top: 1px solid var(--color-line-subtle); font-size: 14px; font-weight: 600; }
  label:has(> input[type='checkbox']) { display: flex; align-items: center; gap: 12px; min-height: 44px; cursor: pointer; }
  input[type='checkbox'] { flex: none; width: 20px; height: 20px; margin: 0; accent-color: var(--color-accent); }
  input:not([type='checkbox']), select { box-sizing: border-box; width: 100%; max-width: 480px; height: 42px; padding: 0 12px; border: 1px solid var(--color-line-subtle); border-radius: var(--radius-sm); background: var(--color-raised); color: var(--color-text); font: inherit; font-size: 14px; font-weight: 400; }
  input:not([type='checkbox']):focus-visible, select:focus-visible { outline: 2px solid var(--color-accent); outline-offset: 1px; }
  input:disabled, select:disabled { opacity: 0.55; cursor: not-allowed; }
  fieldset { display: grid; max-height: 300px; min-width: 0; margin: 0; padding: 14px 0 0; overflow: auto; border: 0; border-top: 1px solid var(--color-line-subtle); }
  fieldset label { min-height: 40px; padding: 6px 0; border-top: 0; font-weight: 400; }
  legend { padding: 0; font-size: 14px; font-weight: 600; }

  /* Boutons : primaire = création, le reste en ghost */
  button { justify-self: start; display: inline-flex; align-items: center; justify-content: center; width: fit-content; min-height: 40px; padding: 8px 18px; border: 1px solid var(--color-line); border-radius: var(--radius-sm); background: transparent; color: var(--color-text); font: inherit; font-size: 14px; font-weight: 500; cursor: pointer; transition: background-color var(--transition-fast); }
  button:hover:not(:disabled) { background: var(--color-raised); }
  button:focus-visible { outline: 2px solid var(--color-accent); outline-offset: 2px; }
  button:disabled { opacity: 0.55; cursor: not-allowed; }
  article button { color: var(--status-error); }
  button[type='submit'] { margin-top: 16px; border: 0; background: var(--color-play); color: var(--color-on-play); font-weight: 600; }
  button[type='submit']:hover:not(:disabled) { background: var(--color-play-hover); }

  @media (max-width: 640px) {
    .keys, form { padding-inline: 16px; }
    article { flex-direction: column; align-items: flex-start; }
    input:not([type='checkbox']), select { max-width: none; font-size: 16px; }
    button[type='submit'] { width: 100%; justify-self: stretch; }
  }
  @media (prefers-reduced-motion: reduce) { button { transition: none; } }
</style>
