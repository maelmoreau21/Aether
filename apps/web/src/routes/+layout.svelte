<script lang="ts">
  import '@fontsource-variable/manrope';
  import '../app.css';
  import '../lib/styles/halo.css';
  import '../lib/styles/tokens.css';
  import { onMount } from 'svelte';
  import { loadServerPresentation } from '../lib/config/presentation';
  import { initTheme, setTheme } from '../lib/theme';
  import { detectLocale, setLocale, t } from '../lib/i18n';
  import { hydratePreferences, preferenceSyncError, preferenceState } from '../lib/preferences';
  import { getSessionGeneration } from '../lib/auth/session';
  import { page } from '$app/state';
  import { goto } from '$app/navigation';
  import { asset, resolve } from '$app/paths';
  import { readSession } from '../lib/auth/session';
  import { preloadCatalog } from '../lib/catalog/page-cache';
  import MusicPlayer from '../lib/music/MusicPlayer.svelte';
  import RemotePanel from '../lib/remote/RemotePanel.svelte';
  import { normalizeDeviceCode, PENDING_DEVICE_CODE } from '../lib/auth/quick-connect';
  import { rememberPendingParty } from '../lib/syncplay/launch';
  import { installTvNavigation } from '../lib/tv/navigation';
  import { parkAccount, shouldAskWhoIsWatching } from '../lib/auth/accounts';

  onMount(() => {
    initTheme();
    const refresh=()=>{if(document.visibilityState==='visible'&&!isDownloads)void hydratePreferences().then(saved=>{if(saved){setTheme(saved.theme??'system',false);setLocale(saved.locale??'fr',false);}}).catch(()=>{});};
    window.addEventListener('focus',refresh);
    const uninstallTv=installTvNavigation();
    return()=>{window.removeEventListener('focus',refresh);uninstallTv();};
  });
  $effect(() => {
    const appearance = $preferenceState;
    if (typeof document === 'undefined') return;
    document.documentElement.dataset.cardSize = appearance.cardSize ?? 'medium';
    document.documentElement.dataset.cardDensity = appearance.cardDensity ?? 'comfortable';
    document.documentElement.dataset.accent = appearance.accentColor ?? 'orange';
  });
  let { children } = $props();
  let verified = $state(false);
  let error = $state('');
  const isLogin = $derived(page.url.pathname.replace(/\/$/, '').endsWith('/login'));
  const isSetup = $derived(page.url.pathname.replace(/\/$/, '').endsWith('/setup'));
  const isDownloads = $derived(page.url.pathname.replace(/\/$/, '').endsWith('/downloads'));
  const isConnect = $derived(page.url.pathname.replace(/\/$/, '').endsWith('/connect'));
  const isAccounts = $derived(page.url.pathname.replace(/\/$/, '').endsWith('/accounts'));
  $effect(() => {
    const loginPage = isLogin;
    const setupPage = isSetup;
    const connectPage = isConnect;
    const accountsPage = isAccounts;
    let cancelled = false;
    verified = false; error = '';
    if (isDownloads) { verified=true; return; }
    readSession().then(async current => {
      if (cancelled) return;
      // Signed out on a shared device: the accounts set aside are offered there.
      if (!current && accountsPage) { verified = true; return; }
      if (!current && !loginPage && !setupPage) {
        if (connectPage) {
          const code = normalizeDeviceCode(window.location.hash.slice(1));
          if (code) sessionStorage.setItem(PENDING_DEVICE_CODE, code);
        }
        rememberPendingParty(window.location.search);
        const bootstrap = await fetch('/api/v1/auth/bootstrap/status', { credentials: 'same-origin', cache: 'no-store' })
          .then(r => r.ok ? r.json() as Promise<{ needsBootstrap?: boolean }> : null)
          .catch(() => null);
        if (cancelled) return;
        void goto(resolve(bootstrap?.needsBootstrap ? 'setup/' : 'login/'), { replaceState: true }); return;
      }
      if (current && !connectPage && !setupPage && normalizeDeviceCode(sessionStorage.getItem(PENDING_DEVICE_CODE) ?? '')) {
        void goto(resolve('connect/'), { replaceState: true }); return;
      }
      if (current && loginPage) { void goto(resolve('/'), { replaceState: true }); return; }
      // Several accounts on this device and none chosen since the app opened: the active
      // one is set aside, so coming back to it asks for its PIN like the others.
      if (current && !accountsPage && !connectPage && !setupPage && shouldAskWhoIsWatching()) {
        await parkAccount(current.csrfToken).catch(() => undefined);
        if (!cancelled) window.location.replace(resolve('accounts/'));
        return;
      }
      if (current) {
        const generation = getSessionGeneration();
        const loadingPreferences = hydratePreferences();
        // Catalogue requests and navigation can start while optional settings load.
        preloadCatalog('home');
        verified = true;
        void loadServerPresentation();
        void loadingPreferences.then(saved => {
          if (cancelled || generation !== getSessionGeneration() || !saved) return;
          setTheme(saved.theme ?? 'system', false);
          setLocale(saved.locale ?? detectLocale(navigator.language), false);
        }).catch(() => {
          if (!cancelled && generation === getSessionGeneration()) preferenceSyncError.set('Vos préférences sont temporairement indisponibles.');
        });
      }
      verified = true;
    }).catch(cause => { if (!cancelled) error = cause instanceof Error ? cause.message : $t('the_server_is_unavailable_try_again'); });
    return () => { cancelled = true; };
  });
</script>
{#if verified}
  {#if $preferenceSyncError}<p class="preference-error" role="alert">{$preferenceSyncError}</p>{/if}
  {@render children()}
  {#if !isLogin && !isSetup && !isDownloads}<MusicPlayer /><RemotePanel />{/if}
{:else if error}
  <main class="session-status"><p role="alert">{error}</p><button onclick={() => window.location.reload()}>{$t('request_retry')}</button></main>
{:else}
  <main class="session-status session-loading" role="status"><img src={asset('images/logo-loading.svg')} alt="" width="56" height="56" /><span>{$t('opening_your_library')}</span></main>
{/if}
<style>.session-loading { min-height:100dvh; max-width:none !important; margin:0 !important; display:flex; flex-direction:column; align-items:center; justify-content:center; gap:24px; box-sizing:border-box; background:var(--color-canvas); color:var(--color-muted) !important; } .session-status { max-width:600px; margin:15vh auto; padding:32px; color:var(--color-muted); line-height:1.8; }.session-status button { color:var(--color-accent); background:var(--color-surface); border:1px solid var(--color-line); padding:12px 20px; border-radius:var(--radius-md); }</style>
