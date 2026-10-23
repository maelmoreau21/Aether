<script lang="ts">
  import '../app.css';
  import '../lib/styles/tokens.css';
  import { onMount } from 'svelte';
  import { loadServerPresentation } from '../lib/config/presentation';
  import { initTheme, setTheme } from '../lib/theme';
  import { setLocale } from '../lib/i18n';
  import { hydratePreferences, preferenceSyncError } from '../lib/preferences';
  import { getSessionGeneration } from '../lib/auth/session';
  import { page } from '$app/state';
  import { goto } from '$app/navigation';
  import { asset, resolve } from '$app/paths';
  import { readSession } from '../lib/auth/session';
  import MusicPlayer from '../lib/music/MusicPlayer.svelte';

  onMount(() => {
    initTheme();
    const refresh=()=>{if(document.visibilityState==='visible'&&!isDownloads)void hydratePreferences().then(saved=>{if(saved){setTheme(saved.theme??'system',false);setLocale(saved.locale??'fr',false);}}).catch(()=>{});};
    window.addEventListener('focus',refresh);
    return()=>window.removeEventListener('focus',refresh);
  });
  let { children } = $props();
  let verified = $state(false);
  let error = $state('');
  const isLogin = $derived(page.url.pathname.replace(/\/$/, '').endsWith('/login'));
  const isDownloads = $derived(page.url.pathname.replace(/\/$/, '').endsWith('/downloads'));
  $effect(() => {
    const loginPage = isLogin;
    let cancelled = false;
    verified = false; error = '';
    if (isDownloads) { verified=true; return; }
    readSession().then(async current => {
      if (cancelled) return;
      if (!current && !loginPage) { void goto(resolve('login/'), { replaceState: true }); return; }
      if (current && loginPage) { void goto(resolve('/'), { replaceState: true }); return; }
      if (current) {
        const generation = getSessionGeneration();
        const loadingPreferences = hydratePreferences();
        // Catalogue requests and navigation can start while optional settings load.
        verified = true;
        void loadServerPresentation();
        void loadingPreferences.then(saved => {
          if (cancelled || generation !== getSessionGeneration() || !saved) return;
          setTheme(saved.theme ?? 'system', false);
          setLocale(saved.locale ?? (navigator.language.toLowerCase().startsWith('en') ? 'en' : 'fr'), false);
        }).catch(() => {
          if (!cancelled && generation === getSessionGeneration()) preferenceSyncError.set('Vos préférences sont temporairement indisponibles.');
        });
      }
      verified = true;
    }).catch(cause => { if (!cancelled) error = cause instanceof Error ? cause.message : 'Le serveur est indisponible. Réessayez pour vérifier votre session.'; });
    return () => { cancelled = true; };
  });
</script>
{#if verified}
  {#if $preferenceSyncError}<p class="preference-error" role="alert">{$preferenceSyncError}</p>{/if}
  {@render children()}
  {#if !isLogin && !isDownloads}<MusicPlayer />{/if}
{:else if error}
  <main class="session-status"><p role="alert">{error}</p><button onclick={() => window.location.reload()}>Réessayer</button></main>
{:else}
  <main class="session-status session-loading" role="status"><img src={asset('images/logo-loading.svg')} alt="" width="56" height="56" /><span>Ouverture de votre médiathèque…</span></main>
{/if}
<style>.session-loading { min-height:100dvh; max-width:none !important; margin:0 !important; display:flex; flex-direction:column; align-items:center; justify-content:center; gap:24px; box-sizing:border-box; background:var(--color-canvas); color:var(--color-muted) !important; } .session-status { max-width:600px; margin:15vh auto; padding:32px; color:var(--color-muted); line-height:1.8; }.session-status button { color:var(--color-accent); background:var(--color-surface); border:1px solid var(--color-line); padding:12px 20px; border-radius:var(--radius-md); }</style>
