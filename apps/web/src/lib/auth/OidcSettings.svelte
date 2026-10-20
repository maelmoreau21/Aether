<script lang="ts">
  import { onMount } from 'svelte';
  import { session } from './session';
  let { embedded = false }: { embedded?: boolean } = $props();
  let enabled = $state(false);
  let config = $state<{ issuer: string; clientId: string; redirectUri: string; userGroups: string[] } | null>(null);
  let message = $state('');
  let password = $state('');
  let busy = $state(false);
  onMount(() => {
    const controller = new AbortController();
    fetch('/api/v1/auth/oidc/status', { signal: controller.signal }).then(r => r.json()).then(s => { enabled = s.enabled === true; }).catch(() => { message = 'État SSO indisponible.'; });
    if ($session?.user.role === 'administrator') {
      fetch('/api/v1/admin/oidc', { signal: controller.signal }).then(r => { if (!r.ok) throw new Error(); return r.json(); }).then(s => { config = s; }).catch(() => { message = 'Configuration SSO indisponible.'; });
    }
    return () => controller.abort();
  });
  async function link(event: SubmitEvent) {
    event.preventDefault(); busy = true; message = '';
    try {
      const response = await fetch('/api/v1/auth/oidc/link', { method: 'POST', headers: { 'content-type': 'application/json', 'x-csrf-token': $session?.csrfToken ?? '' }, body: JSON.stringify({ password }) });
      if (!response.ok) throw new Error('Liaison refusée. Vérifiez votre mot de passe local et la configuration SSO.');
      const data: { url: string } = await response.json();
      window.location.assign(data.url);
    } catch (failure) { message = failure instanceof Error ? failure.message : 'Liaison indisponible.'; busy = false; }
    finally { password = ''; }
  }
</script>
<section class:embedded aria-label={embedded ? 'Liaison SSO' : undefined} aria-labelledby={embedded ? undefined : 'oidc-title'}>
  {#if !embedded}<h2 id="oidc-title">Connexion SSO</h2>{/if}
  <p>{enabled ? 'SSO activé. La connexion locale reste disponible.' : 'SSO désactivé.'}</p>
  {#if config && enabled}
    <dl><dt>Issuer</dt><dd>{config.issuer}</dd><dt>Client ID</dt><dd>{config.clientId}</dd><dt>URI de redirection exacte</dt><dd>{config.redirectUri}</dd><dt>Groupes autorisés (rôle user)</dt><dd>{config.userGroups.join(', ')}</dd></dl>
    <p>Configuration par variables MEDIA_OIDC_*. Les rôles administrateur sont gérés localement. Conservez un administrateur local de secours.</p>
  {/if}
  {#if enabled}
    <form onsubmit={link}>
      <p>Pour lier votre compte local à votre identité SSO, confirmez votre mot de passe local puis authentifiez-vous chez le fournisseur.</p>
      <label>Mot de passe local <input type="password" autocomplete="current-password" required maxlength="512" bind:value={password} disabled={busy} /></label>
      <button type="submit" disabled={busy}>{busy ? 'Connexion…' : 'Lier mon compte local au SSO'}</button>
    </form>
  {/if}
  {#if message}<p role="alert">{message}</p>{/if}
</section>
<style>
  section { margin-bottom:24px; max-width:700px; padding:30px; background:var(--color-surface); border:1px solid var(--color-line); border-radius:12px; }
  section.embedded { background: transparent; padding: 0; border: 0; border-radius: 0; margin: 0; }
  h2 { margin-top:0; }p,dl,label { font-size:14px; line-height:1.7; }dd { margin:0 0 10px; overflow-wrap:anywhere; }form { display:grid; gap:16px; }label { display:grid; gap:8px; }input,button { padding:12px; border-radius:6px; border:1px solid var(--color-line); background:var(--color-raised); color:var(--color-text); }
</style>
