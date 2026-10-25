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
<style>dl { display:grid;grid-template-columns:repeat(auto-fit,minmax(160px,1fr));gap:16px;margin:12px 0; } dt { color:var(--text-secondary);font-size:12px; } dd { margin:6px 0 0;font-size:14px; } article button { justify-self:start; } button { min-height:44px; } .current-device { color:var(--accent-primary);font-size:12px; } section { margin-top:24px; } form { display:grid; gap:12px; } label { display:grid; gap:6px; } input { width:100%; box-sizing:border-box; padding:10px; background:var(--color-surface); color:var(--color-text); border:1px solid var(--color-line); border-radius:8px; } button { padding:10px 14px; background:var(--color-surface); color:var(--color-text); border:1px solid var(--color-line); border-radius:8px; cursor:pointer; } article { display:grid;gap:6px;background:var(--bg-elevated); margin:12px 0; padding:12px; border:1px solid var(--color-line); border-radius:8px; overflow-wrap:anywhere; } p { color:var(--color-muted); } </style>
