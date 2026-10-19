<script lang="ts">
  import '../app.css';
  import '../lib/styles/tokens.css';
  import { onMount } from 'svelte';
  import { initTheme, setTheme } from '../lib/theme';
  import { setLocale } from '../lib/i18n';
  import { hydratePreferences, preferenceSyncError } from '../lib/preferences';
  import { getSessionGeneration } from '../lib/auth/session';
  import { page } from '$app/state';
  import { goto } from '$app/navigation';
  import { resolve } from '$app/paths';
  import { readSession } from '../lib/auth/session';

  onMount(() => {
    initTheme();
  });
  let { children } = $props();
  let verified = $state(false);
  let error = $state('');
  const isLogin = $derived(page.url.pathname.replace(/\/$/, '').endsWith('/login'));
  $effect(() => {
    const loginPage = isLogin;
    let cancelled = false;
    verified = false; error = '';
    readSession().then(async current => {
      if (cancelled) return;
      if (!current && !loginPage) { void goto(resolve('login/'), { replaceState: true }); return; }
      if (current && loginPage) { void goto(resolve('/'), { replaceState: true }); return; }
      if (current) {
        const generation = getSessionGeneration();
        const saved = await hydratePreferences();
        if (cancelled || generation !== getSessionGeneration() || !saved) return;
        setTheme(saved.theme ?? 'system', false);
        setLocale(saved.locale ?? (navigator.language.toLowerCase().startsWith('en') ? 'en' : 'fr'), false);
      }
      verified = true;
    }).catch(cause => { if (!cancelled) error = cause instanceof Error ? cause.message : 'Le serveur est indisponible. Réessayez pour vérifier votre session.'; });
    return () => { cancelled = true; };
  });
</script>
{#if verified}
  {#if $preferenceSyncError}<p class="preference-error" role="alert">{$preferenceSyncError}</p>{/if}
  {@render children()}
{:else if error}
  <main class="session-status"><p role="alert">{error}</p><button onclick={() => window.location.reload()}>Réessayer</button></main>
{:else}
  <main class="session-status" role="status">Vérification de la session…</main>
{/if}
<style>.session-status { max-width:600px; margin:15vh auto; padding:32px; color:var(--color-muted); line-height:1.8; }.session-status button { color:var(--color-accent); background:var(--color-surface); border:1px solid var(--color-line); padding:12px 20px; border-radius:8px; }</style>
