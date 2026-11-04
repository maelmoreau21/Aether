<script lang="ts">
  import { onMount } from 'svelte';
  import { session } from './session';
  let { mode = 'security' }: { mode?: 'security' | 'devices' } = $props();
  interface Device { id: string; device: string; createdAt: number; lastSeenAt: number; expiresAt: number; current: boolean }
  let canCreate=$state(false),login=$state('');
  let devices = $state<Device[]>([]);
  let currentPassword = $state('');
  let newPassword = $state('');
  let confirmation = $state('');
  let error = $state('');
  let busy = $state(false);
  let loading = $state(true);
  async function load() {
    const response = await fetch('/api/v1/account/sessions', { cache: 'no-store' });
    if (!response.ok) throw new Error('Impossible de charger les appareils.');
    devices = (await response.json()).sessions;
  }
  onMount(() => { if (mode !== 'devices') { loading = false; void fetch('/api/v1/account/local-credentials',{cache:'no-store'}).then(r=>r.ok?r.json():{canCreate:false}).then(v=>canCreate=v.canCreate).catch(()=>{}); return; } void load().catch(cause => error = cause.message).finally(() => loading = false); });
  async function mutate(path: string, method: string, body?: unknown) {
    error = ''; busy = true;
    try {
      const response = await fetch(path, { method, headers: { 'content-type': 'application/json', 'x-csrf-token': $session?.csrfToken ?? '' }, body: body === undefined ? undefined : JSON.stringify(body) });
      if (!response.ok) {
        const code = (await response.json()).error;
        throw new Error(({ invalid_current_password: 'Le mot de passe actuel est incorrect.', invalid_password: 'Utilisez 15 à 128 caractères sans caractère de contrôle.', local_credentials_required: 'Le mot de passe est géré par votre fournisseur SSO.', recent_login_required: 'Reconnectez-vous avec le SSO puis réessayez dans les cinq minutes.', sso_local_accounts_blocked: 'La création d’identifiants locaux est désactivée.', local_credentials_conflict: 'Identifiant indisponible ou compte déjà configuré.', try_later: 'Trop de tentatives. Réessayez dans une minute.' } as Record<string,string>)[code] ?? 'Opération impossible. Réessayez.');
      }
      return true;
    } catch (cause) { error = cause instanceof Error ? cause.message : 'Opération impossible.'; return false; }
    finally { busy = false; currentPassword = ''; newPassword = ''; confirmation = ''; }
  }
  async function change(event: SubmitEvent) {
    event.preventDefault();
    if (newPassword !== confirmation) { error = 'Les deux nouveaux mots de passe doivent être identiques.'; return; }
    if (await mutate($session?.user.sso ? '/api/v1/account/local-credentials' : '/api/v1/account/password', $session?.user.sso ? 'POST' : 'PUT', $session?.user.sso ? {login,password:newPassword} : { currentPassword, newPassword })) window.location.reload();
  }
  async function revoke(device?: Device) {
    if (!await mutate('/api/v1/account/sessions' + (device ? '/' + device.id : ''), 'DELETE')) return;
    if (!device || device.current) window.location.reload();
    else await load().catch(cause => error = cause.message);
  }
</script>
<section>
{#if mode === 'security'}<h3>Connexion par mot de passe</h3>
{#if $session?.user.sso}<p>Votre mot de passe est géré par votre fournisseur SSO.</p>{/if}
{#if !$session?.user.sso || canCreate}
<form onsubmit={change}>
{#if $session?.user.sso}<p>Reconnectez-vous avec le SSO avant de créer vos identifiants locaux (moins de cinq minutes).</p><label>Identifiant local<input required minlength="3" maxlength="64" pattern="[a-z0-9._-]+" autocomplete="username" bind:value={login} disabled={busy} /></label>{:else}
  <label>Mot de passe actuel<input type="password" autocomplete="current-password" required maxlength="512" bind:value={currentPassword} disabled={busy} /></label>
  {/if}
  <label>Nouveau mot de passe<input type="password" autocomplete="new-password" required minlength="15" maxlength="128" bind:value={newPassword} disabled={busy} /></label>
  <label>Confirmer le nouveau mot de passe<input type="password" autocomplete="new-password" required minlength="15" maxlength="128" bind:value={confirmation} disabled={busy} /></label>
  {#if !$session?.user.sso}<p>Toutes vos sessions seront déconnectées après le changement.</p>{/if}<button disabled={busy}>{$session?.user.sso ? 'Créer mes identifiants locaux' : 'Changer le mot de passe'}</button>
</form>{/if}
{:else}<h3>Vos sessions</h3><p>Votre connexion reste active pendant 7 jours sans activité, dans la limite de 30 jours après la connexion.</p><button disabled={busy || loading} onclick={() => { void load().catch(cause => error = cause.message); }}>Actualiser la liste</button>{#if loading}<p role="status">Chargement des appareils…</p>{/if}<p>Chaque entrée représente une session du navigateur, pas une identification matérielle vérifiée.</p>
{#each devices as device (device.id)}<article><strong>{device.device}</strong>{#if device.current}<span class="current-device">Session actuelle</span>{/if}<dl><div><dt>Connexion</dt><dd>{new Date(device.createdAt * 1000).toLocaleString()}</dd></div><div><dt>Dernière activité</dt><dd>{new Date(device.lastSeenAt * 1000).toLocaleString()}</dd></div><div><dt>Expiration</dt><dd>{new Date(device.expiresAt * 1000).toLocaleString()}</dd></div></dl><button disabled={busy} onclick={() => revoke(device)}>Déconnecter{device.current ? ' cette session' : ''}</button></article>{/each}
<button disabled={busy || devices.length === 0} onclick={() => revoke()}>Déconnecter tous les appareils</button>
{/if}
{#if error}<p role="alert">{error}</p>{/if}</section>
<style>
  section { display: grid; gap: 12px; margin-top: 24px; min-width: 0; }
  h3 { margin: 0; font-size: 17px; font-weight: 700; color: var(--tx); }
  p { margin: 0; color: var(--mu); font-size: 14px; line-height: 1.6; }
  form { display: grid; gap: 16px; }
  label { display: grid; gap: 8px; font-size: 14px; font-weight: 700; color: var(--tx); }
  input { width: 100%; max-width: 520px; height: 46px; min-height: 46px; box-sizing: border-box; padding: 0 14px; border: 1px solid transparent; border-radius: var(--r-sm); background: var(--pn); color: var(--tx); font: inherit; font-size: 15px; font-weight: 500; box-shadow: none; transition: border-color var(--t-fast) ease, background-color var(--t-fast) ease; }
  input:hover:not(:disabled) { background: var(--pn-2); }
  input:focus { outline: none; border-color: var(--ac); background: var(--bg-3); }
  input:disabled { opacity: 0.45; }
  button { justify-self: start; display: inline-flex; align-items: center; justify-content: center; gap: 8px; min-height: 44px; padding: 0 18px; border: 0; border-radius: var(--r-md); background: var(--pn-2); color: var(--tx); font: inherit; font-size: 14px; font-weight: 700; cursor: pointer; transition: background-color var(--t-fast) ease, opacity var(--t-fast) ease, transform var(--t-fast) var(--ease); }
  button:hover:not(:disabled) { background: var(--pn-3); }
  button:active:not(:disabled) { transform: scale(0.98); }
  button:disabled { opacity: 0.45; cursor: default; }
  button:focus-visible, input:focus-visible { outline: 2px solid var(--ac); outline-offset: 3px; }
  form button { height: 48px; padding: 0 22px; background: var(--btn); color: var(--btn-tx); }
  form button:hover:not(:disabled) { background: var(--btn); opacity: 0.9; }
  article button, section > button:last-of-type:not(:first-of-type) { background: var(--err-soft); color: var(--err); }
  article button:hover:not(:disabled), section > button:last-of-type:not(:first-of-type):hover:not(:disabled) { background: var(--err-soft); opacity: 0.85; }
  article { display: grid; gap: 8px; margin: 0; padding: 16px 0; border-top: 1px solid var(--line); overflow-wrap: anywhere; }
  article strong { font-size: 15px; font-weight: 700; color: var(--tx); }
  article + button { margin-top: 4px; }
  .current-device { justify-self: start; display: inline-flex; align-items: center; height: 24px; padding: 0 9px; border-radius: var(--r-xs); background: var(--ac-soft); color: var(--ac); font-size: 12px; font-weight: 700; }
  dl { display: grid; grid-template-columns: repeat(auto-fit, minmax(160px, 1fr)); gap: 12px 16px; margin: 4px 0; }
  dt { color: var(--mu); font-size: 13px; font-weight: 600; }
  dd { margin: 4px 0 0; font-size: 14px; font-weight: 600; color: var(--tx); font-variant-numeric: tabular-nums; }
  [role='alert'] { padding: 12px 14px; border-radius: var(--r-sm); background: var(--err-soft); color: var(--err); font-weight: 600; }
  [role='status'] { color: var(--mu); }
</style>
