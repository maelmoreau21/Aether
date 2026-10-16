<script lang="ts">
  import '../lib/styles/tokens.css';
  import { page } from '$app/state';
  import { goto } from '$app/navigation';
  import { resolve } from '$app/paths';
  import { readSession } from '../lib/auth/session';
  let { children } = $props();
  let verified = $state(false);
  let error = $state('');
  const isLogin = $derived(page.url.pathname.replace(/\/$/, '').endsWith('/login'));
  $effect(() => {
    const loginPage = isLogin;
    let cancelled = false;
    verified = false; error = '';
    readSession().then(current => {
      if (cancelled) return;
      if (!current && !loginPage) { void goto(resolve('login/'), { replaceState: true }); return; }
      if (current && loginPage) { void goto(resolve('/'), { replaceState: true }); return; }
      verified = true;
    }).catch(() => { if (!cancelled) error = 'Le serveur est indisponible. Réessayez pour vérifier votre session.'; });
    return () => { cancelled = true; };
  });
</script>
{#if verified}
  {@render children()}
{:else if error}
  <main class="session-status"><p role="alert">{error}</p><button onclick={() => window.location.reload()}>Réessayer</button></main>
{:else}
  <main class="session-status" role="status">Vérification de la session…</main>
{/if}
<style>.session-status { max-width:600px; margin:15vh auto; padding:32px; color:var(--color-muted); line-height:1.8; }.session-status button { color:var(--color-accent); background:var(--color-surface); border:1px solid var(--color-line); padding:12px 20px; border-radius:8px; }</style>
