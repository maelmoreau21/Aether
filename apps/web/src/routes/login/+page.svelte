<script lang="ts">
  import { goto } from '$app/navigation';
  import { resolve } from '$app/paths';
  import { DEFAULT_NAME, fetchBrandName } from '../../lib/config/brand';
  import { login } from '../../lib/auth/session';
  let name = $state(DEFAULT_NAME);
  let identifier = $state('');
  let password = $state('');
  let ssoEnabled = $state(false);
  let busy = $state(false);
  let error = $state('');
  async function sso() {
    busy = true; error = '';
    try {
      const response = await fetch('/api/v1/auth/oidc/start', { method: 'POST', credentials: 'same-origin' });
      if (!response.ok) throw new Error('Le SSO est indisponible. Réessayez ou utilisez votre compte local.');
      const data: { url: string } = await response.json();
      window.location.assign(data.url);
    } catch (failure) { error = failure instanceof Error ? failure.message : 'Connexion SSO indisponible.'; busy = false; }
  }
  async function submit(event: SubmitEvent) {
    event.preventDefault();
    if (busy) return;
    busy = true; error = '';
    try { await login(identifier, password); password = ''; await goto(resolve('/'), { replaceState: true }); }
    catch (failure) { error = failure instanceof Error ? failure.message : 'La connexion est indisponible. Réessayez.'; }
    finally { password = ''; busy = false; }
  }
  $effect(() => {
    const controller = new AbortController();
    fetch('/api/v1/auth/oidc/status', { signal: controller.signal }).then(r => r.ok ? r.json() : { enabled: false }).then(data => { ssoEnabled = data.enabled === true; }).catch(() => {});
    fetchBrandName(controller.signal).then(brandName => { name = brandName; });
    return () => controller.abort();
  });
</script>
<svelte:head><title>Connexion · {name}</title><meta name="description" content="Connectez-vous à votre médiathèque." /></svelte:head>
<main>
  <div class="brand"><img src={`${resolve('/').replace(/\/$/, '')}/images/brand-mark.svg`} alt="" width="32" height="38" /><span>{name}</span></div>
  <section aria-labelledby="login-title">
    <p class="eyebrow">VOTRE CINÉMA, CHEZ VOUS</p>
    <h1 id="login-title">Bienvenue chez vous.</h1>
    <p class="intro">Connectez-vous pour retrouver votre médiathèque.</p>
    <form onsubmit={submit} aria-busy={busy}>
      <label for="login">Identifiant<input id="login" name="username" autocomplete="username" autocapitalize="none" spellcheck="false" maxlength="64" required bind:value={identifier} disabled={busy} /></label>
      <label for="password">Mot de passe<input id="password" name="password" type="password" autocomplete="current-password" maxlength="512" required bind:value={password} disabled={busy} /></label>
      {#if error}<p class="error" role="alert">{error}</p>{/if}
      <button type="submit" disabled={busy}>{busy ? 'Connexion en cours…' : 'Se connecter'}<span aria-hidden="true">→</span></button>
    </form>
    {#if ssoEnabled}<button type="button" onclick={sso} disabled={busy}>Se connecter avec SSO</button>{/if}
    <p class="help">Utilisez le compte local créé lors de l’installation.</p>
  </section>
  <footer>Votre cinéma, autrement.</footer>
</main>
<style>
  main { min-height:100svh; display:flex; flex-direction:column; align-items:center; padding:48px 24px 26px; }.brand { display:flex; align-items:center; gap:12px; font-size:34px; font-weight:650; letter-spacing:-.06em; }section { width:100%; max-width:430px; margin:auto; padding:60px 0; }h1 { font-size:clamp(30px,6vw,38px); font-weight:500; letter-spacing:-.05em; margin:18px 0 14px; }.intro,.help { color:var(--color-muted); font-size:13px; line-height:1.7; }form { display:grid; gap:24px; margin-top:36px; }label { display:grid; gap:10px; font-size:12px; color:var(--color-text); }input { width:100%; min-height:50px; border:1px solid #ffffff20; border-radius:8px; background:var(--color-surface); color:var(--color-text); padding:12px 15px; }input:focus { border-color:var(--color-accent); }button { display:flex; justify-content:space-between; align-items:center; min-height:50px; padding:14px 18px; border:0; border-radius:8px; background:var(--color-accent); color:#171123; font-size:13px; font-weight:650; }button:hover:not(:disabled) { background:var(--color-accent-hover); }.error { margin:0; color:var(--color-danger); font-size:13px; line-height:1.7; }.help { margin:24px 0 0; font-size:11px; }footer { color:var(--color-subtle); font-size:11px; }@media(max-width:500px) { main { padding-top:32px; }section { padding:45px 0; } }
</style>
