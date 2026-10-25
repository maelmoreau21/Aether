<script lang="ts">
  import { onMount } from 'svelte';
  import { session } from './session';
  interface Device { id: string; device: string; createdAt: number; lastSeenAt: number; expiresAt: number; current: boolean }
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
  onMount(() => { void load().catch(cause => error = cause.message).finally(() => loading = false); });
  async function mutate(path: string, method: string, body?: unknown) {
    error = ''; busy = true;
    try {
      const response = await fetch(path, { method, headers: { 'content-type': 'application/json', 'x-csrf-token': $session?.csrfToken ?? '' }, body: body === undefined ? undefined : JSON.stringify(body) });
      if (!response.ok) {
        const code = (await response.json()).error;
        throw new Error(({ invalid_current_password: 'Le mot de passe actuel est incorrect.', invalid_password: 'Utilisez 15 à 128 caractères sans caractère de contrôle.', local_credentials_required: 'Le mot de passe est géré par votre fournisseur SSO.', try_later: 'Trop de tentatives. Réessayez dans une minute.' } as Record<string,string>)[code] ?? 'Opération impossible. Réessayez.');
      }
      return true;
    } catch (cause) { error = cause instanceof Error ? cause.message : 'Opération impossible.'; return false; }
    finally { busy = false; currentPassword = ''; newPassword = ''; confirmation = ''; }
  }
  async function change(event: SubmitEvent) {
    event.preventDefault();
    if (newPassword !== confirmation) { error = 'Les deux nouveaux mots de passe doivent être identiques.'; return; }
    if (await mutate('/api/v1/account/password', 'PUT', { currentPassword, newPassword })) window.location.reload();
  }
  async function revoke(device?: Device) {
    if (!await mutate('/api/v1/account/sessions' + (device ? '/' + device.id : ''), 'DELETE')) return;
    if (!device || device.current) window.location.reload();
    else await load().catch(cause => error = cause.message);
  }
</script>
<section aria-labelledby="password-title"><h3 id="password-title">Mot de passe personnel</h3>
{#if $session?.user.sso}<p>Votre mot de passe est géré par votre fournisseur SSO.</p>{:else}
<form onsubmit={change}>
  <label>Mot de passe actuel<input type="password" autocomplete="current-password" required maxlength="512" bind:value={currentPassword} disabled={busy} /></label>
  <label>Nouveau mot de passe<input type="password" autocomplete="new-password" required minlength="15" maxlength="128" bind:value={newPassword} disabled={busy} /></label>
  <label>Confirmer le nouveau mot de passe<input type="password" autocomplete="new-password" required minlength="15" maxlength="128" bind:value={confirmation} disabled={busy} /></label>
  <p>Toutes vos sessions seront déconnectées après le changement.</p><button disabled={busy}>Changer le mot de passe</button>
</form>{/if}
<h3>Appareils connectés</h3><p>Votre connexion reste active pendant 7 jours sans activité, dans la limite de 30 jours après la connexion.</p><button disabled={busy || loading} onclick={() => { void load().catch(cause => error = cause.message); }}>Actualiser la liste</button>{#if loading}<p role="status">Chargement des appareils…</p>{/if}<p>Chaque entrée représente une session du navigateur, pas une identification matérielle vérifiée.</p>
{#each devices as device (device.id)}<article><strong>{device.device}</strong>{#if device.current} · Cet appareil{/if}<p>Connexion : {new Date(device.createdAt * 1000).toLocaleString()}</p><p>Expiration : {new Date(device.expiresAt * 1000).toLocaleString()}</p><p>Dernière activité : {new Date(device.lastSeenAt * 1000).toLocaleString()}</p><button disabled={busy} onclick={() => revoke(device)}>Déconnecter{device.current ? ' cette session' : ''}</button></article>{/each}
<button disabled={busy || devices.length === 0} onclick={() => revoke()}>Déconnecter tous les appareils</button>
{#if error}<p role="alert">{error}</p>{/if}</section>
<style>section { margin-top:24px; } form { display:grid; gap:12px; } label { display:grid; gap:6px; } input { width:100%; box-sizing:border-box; padding:10px; background:var(--color-surface); color:var(--color-text); border:1px solid var(--color-line); border-radius:8px; } button { padding:10px 14px; background:var(--color-surface); color:var(--color-text); border:1px solid var(--color-line); border-radius:8px; cursor:pointer; } article { margin:12px 0; padding:12px; border:1px solid var(--color-line); border-radius:8px; overflow-wrap:anywhere; } p { color:var(--color-muted); } </style>
