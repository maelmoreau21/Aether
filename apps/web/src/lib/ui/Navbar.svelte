<script module lang="ts">
  export const navLinks = [
    { key: 'home', path: '/', labelKey: 'home', symbol: '⌂' },
    { key: 'lists', path: 'lists/', labelKey: 'lists', symbol: '☷' }
  ] as const;
</script>

<script lang="ts">
  import { preloadCatalog } from '../catalog/page-cache';
  import { nativeApp } from '../native/app-bridge';
  // The Android application keeps offline downloads in a native list.
  const app = nativeApp();
  import { onMount, tick } from 'svelte';
  import { goto } from '$app/navigation';
  import { session } from '../auth/session';
  import { accessibleLibraries, loadLibraries, type AccessibleLibrary } from '../catalog/libraries';
  import { resolve } from '$app/paths';
  import Icon from './Icon.svelte';
  import ProfileMenu from './ProfileMenu.svelte';
  import WatchPartyModal from '../syncplay/WatchPartyModal.svelte';
  import { serverPresentation, watchTogetherEnabled } from '../config/presentation';
  import { loadFeatures } from '../applications/api';
  import { t } from '../i18n';
  import { startWatchParty } from '../syncplay/launch';

  type Kind = AccessibleLibrary['kind'];

  let { section = 'home', name = 'Aether' }: {
    section?: 'home' | 'films' | 'series' | 'music' | 'lists' | 'downloads' | 'search' | 'settings' | 'admin' | 'requests' | 'reports';
    name?: string;
  } = $props();

  const kindLabels: Record<Kind, 'films' | 'series' | 'music'> = { films: 'films', series: 'series', music: 'music' };

  let watchPartyOpen = $state(false);
  let pendingInvitesCount = $state(0);

  onMount(() => {
    void loadFeatures().catch(() => {});
  });
  $effect(() => { if ($watchTogetherEnabled) void loadInvitesCount(); else { pendingInvitesCount = 0; watchPartyOpen = false; } });

  async function loadInvitesCount() {
    try {
      const res = await fetch('/api/v1/parties/invites');
      if (res.ok) {
        const data = await res.json();
        pendingInvitesCount = (data.invites ?? []).length;
      }
    } catch {}
  }
  let isScrolled = $state(false);
  let searchExpanded = $state(false);
  let headerSearchQuery = $state('');
  let headerSearchInput = $state<HTMLInputElement>();

  // Until libraries load, show the video destinations so the bar does not jump.
  const kinds = $derived(
    $accessibleLibraries.length
      ? (['films', 'series', 'music'] as const).filter(kind => $accessibleLibraries.some(library => library.kind === kind))
      : (['films', 'series'] as const)
  );

  function kindHref(kind: Kind): string {
    const libraries = $accessibleLibraries.filter(library => library.kind === kind);
    return libraries.length === 1
      ? resolve(`${kind}/?library=${encodeURIComponent(libraries[0].id)}` as 'films/')
      : resolve(`${kind}/` as 'films/');
  }

  function preloadKind(kind: Kind) {
    if (kind === 'music') return;
    const libraries = $accessibleLibraries.filter(library => library.kind === kind);
    preloadCatalog(kind, libraries.length === 1 ? libraries[0].id : undefined);
  }

  // En cas d'échec, la navigation garde ses destinations vidéo par défaut.
  async function refreshLibraries(force = false) {
    try { await loadLibraries(force); } catch { /* repli : films et séries */ }
  }

  $effect(() => { if ($session?.user.id) void refreshLibraries(); });

  onMount(() => {
    const updateScroll = () => { isScrolled = window.scrollY > 12; };
    updateScroll();
    window.addEventListener('scroll', updateScroll, { passive: true });
    const refresh = () => { if (document.visibilityState === 'visible') { void refreshLibraries(true); void loadFeatures(true).catch(() => {}); } };
    window.addEventListener('focus', refresh);
    return () => {
      window.removeEventListener('scroll', updateScroll);
      window.removeEventListener('focus', refresh);
    };
  });

  async function toggleQuickSearch(event: MouseEvent) {
    if (event.metaKey || event.ctrlKey || event.shiftKey || event.altKey || section === 'search') return;
    if (window.innerWidth <= 1024) return;
    event.preventDefault();
    searchExpanded = !searchExpanded;
    if (searchExpanded) {
      await tick();
      headerSearchInput?.focus();
    }
  }

  function submitQuickSearch(event: SubmitEvent) {
    event.preventDefault();
    const trimmed = headerSearchQuery.trim();
    searchExpanded = false;
    const target = trimmed ? resolve(`search/?q=${encodeURIComponent(trimmed)}` as 'search/') : resolve('search/');
    void goto(target);
  }
</script>

<header class="site-header" class:scrolled={isScrolled}>
  <div class="header-inner">
    <a class="brand" href={resolve('/')} onpointerenter={() => preloadCatalog('home')} onfocus={() => preloadCatalog('home')} aria-label={$t('brand_home_label', { name: $serverPresentation.serverName || name })}>
      <svg class="brand-mark" viewBox="0 0 256 256" width="28" height="28" aria-hidden="true">
        <defs><linearGradient id="halo-brand" x1="0" y1="0" x2="1" y2="1"><stop offset="0" class="logo-stop-a" /><stop offset="1" class="logo-stop-b" /></linearGradient></defs>
        <path d="M64 192 V48 L216 128 L116 180" fill="none" stroke="url(#halo-brand)" stroke-width="30" stroke-linecap="round" stroke-linejoin="round" />
      </svg>
      <span>{$serverPresentation.serverName || name}</span>
    </a>

    <nav class="desktop-nav" aria-label={$t('main_navigation')}>
      <a href={resolve('/')} aria-current={section === 'home' ? 'page' : undefined}>{$t('home')}</a>
      {#each kinds as kind (kind)}
        <!-- eslint-disable-next-line svelte/no-navigation-without-resolve -- kindHref() resolves the URL. -->
        <a href={kindHref(kind)} aria-current={section === kind ? 'page' : undefined} onpointerenter={() => preloadKind(kind)} onfocus={() => preloadKind(kind)}>{$t(kindLabels[kind])}</a>
      {/each}
      <a href={resolve('lists/')} aria-current={section === 'lists' ? 'page' : undefined}>{$t('lists')}</a>
    </nav>

    <div class="header-actions">
      {#if searchExpanded}
        <form class="header-search-form" onsubmit={submitQuickSearch}>
          <Icon name="search" size={16} />
          <input
            bind:this={headerSearchInput}
            type="search"
            bind:value={headerSearchQuery}
            placeholder={$t('search_placeholder')}
              aria-label={$t('quick_search')}
            onkeydown={e => { if (e.key === 'Escape') searchExpanded = false; }}
            onblur={() => { if (!headerSearchQuery.trim()) searchExpanded = false; }}
          />
        </form>
      {/if}
      <a
        class="icon-btn"
        class:active={section === 'search' || searchExpanded}
        href={resolve('search/')}
        onclick={toggleQuickSearch}
        title={$t('search_action')}
        aria-label={$t('search_action')}
        aria-current={section === 'search' ? 'page' : undefined}
      >
        <Icon name="search" size={19} />
      </a>
      {#if $watchTogetherEnabled}
      <button
        type="button"
        class="icon-btn watch-party-btn"
        title={$t('watch_party_sessions')}
        aria-label={$t('watch_party_sessions')}
        onclick={() => (watchPartyOpen = true)}
      >
        <Icon name="users" size={19} />
        {#if pendingInvitesCount > 0}
          <span class="invite-badge">{pendingInvitesCount}</span>
        {/if}
      </button>
      {/if}
      {#if app?.downloads}
        <button type="button" class="icon-btn" title={$t('native_downloads')} aria-label={$t('native_downloads')} onclick={() => app.downloads?.()}>
          <Icon name="download" size={19} />
        </button>
      {/if}
      <ProfileMenu />
    </div>
  </div>
</header>

<nav class="mobile-nav" aria-label={$t('mobile_navigation')}>
  <a href={resolve('/')} aria-current={section === 'home' ? 'page' : undefined}>
    <svg width="22" height="22" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" aria-hidden="true"><path d="M4 11 12 4l8 7v9h-5v-6H9v6H4z" /></svg>
    <span>{$t('home')}</span>
  </a>
  {#each kinds as kind (kind)}
    <!-- eslint-disable-next-line svelte/no-navigation-without-resolve -- kindHref() resolves the URL. -->
    <a href={kindHref(kind)} aria-current={section === kind ? 'page' : undefined}>
      {#if kind === 'films'}
        <svg width="22" height="22" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" aria-hidden="true"><rect x="3" y="5" width="18" height="14" rx="3" /><path d="M3 9h18" /></svg>
      {:else if kind === 'series'}
        <svg width="22" height="22" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" aria-hidden="true"><rect x="3" y="6" width="18" height="13" rx="3" /><path d="m8 2 4 4 4-4" /></svg>
      {:else}
        <svg width="22" height="22" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" aria-hidden="true"><path d="M9 18V6l11-2v12" /><circle cx="6.5" cy="18" r="2.5" /><circle cx="17.5" cy="16" r="2.5" /></svg>
      {/if}
      <span>{$t(kindLabels[kind])}</span>
    </a>
  {/each}
  <a href={resolve('lists/')} aria-current={section === 'lists' ? 'page' : undefined}>
    <svg width="22" height="22" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" aria-hidden="true"><path d="M5 4h14v17l-7-4-7 4z" /></svg>
    <span>{$t('lists')}</span>
  </a>
</nav>

{#if watchPartyOpen && $watchTogetherEnabled}
  <WatchPartyModal
    onClose={() => (watchPartyOpen = false)}
    onJoinParty={(partyId, mediaId) => {
      watchPartyOpen = false;
      startWatchParty({ partyId, mediaId });
    }}
  />
{/if}

<style>
  .site-header {
    position: sticky;
    top: 0;
    z-index: 40;
    background: transparent;
    border-bottom: 1px solid transparent;
    transition: background-color var(--t-med) ease, border-color var(--t-med) ease;
  }

  .site-header.scrolled {
    background: var(--glass);
    backdrop-filter: blur(18px);
    -webkit-backdrop-filter: blur(18px);
    border-bottom-color: var(--line);
  }

  .header-inner {
    display: flex;
    align-items: center;
    gap: 24px;
    min-height: calc(72px * var(--ui-scale, 1));
    width: 100%;
    max-width: var(--maxw);
    margin: 0 auto;
    padding: 0 var(--gutter);
  }

  .brand {
    display: flex;
    align-items: center;
    gap: 10px;
    min-width: 0;
    flex-shrink: 0;
    font-size: calc(20px * var(--ui-scale, 1));
    font-weight: 800;
    letter-spacing: -0.03em;
    color: var(--tx);
  }

  .brand-mark { flex: none; width: calc(28px * var(--ui-scale, 1)); height: calc(28px * var(--ui-scale, 1)); }

  .brand span {
    max-width: 200px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .desktop-nav {
    display: flex;
    align-items: center;
    gap: 4px;
    padding: 4px;
    border-radius: var(--r-pill);
    background: var(--pn);
    min-width: 0;
    overflow-x: auto;
    scrollbar-width: none;
  }

  .desktop-nav::-webkit-scrollbar { display: none; }

  .desktop-nav a {
    display: inline-flex;
    align-items: center;
    height: calc(38px * var(--ui-scale, 1));
    padding: 0 calc(16px * var(--ui-scale, 1));
    border-radius: var(--r-pill);
    color: var(--mu);
    font-size: calc(14px * var(--ui-scale, 1));
    font-weight: 600;
    white-space: nowrap;
    flex-shrink: 0;
    transition: background-color var(--t-med) var(--ease), color var(--t-med) var(--ease);
  }

  .desktop-nav a:hover { color: var(--tx); }

  .desktop-nav a[aria-current='page'] {
    background: var(--tx);
    color: var(--bg);
  }

  .header-actions {
    display: flex;
    align-items: center;
    gap: 8px;
    margin-left: auto;
    flex: none;
  }

  .header-search-form {
    display: flex;
    align-items: center;
    gap: 8px;
    height: 44px;
    width: 260px;
    padding: 0 14px;
    border-radius: var(--r-md);
    background: var(--pn-2);
    color: var(--mu);
    animation: expand-search var(--t-med) var(--ease);
  }

  @keyframes expand-search {
    from { opacity: 0; width: 160px; }
    to { opacity: 1; width: 260px; }
  }

  .header-search-form input {
    flex: 1;
    min-width: 0;
    height: 32px;
    min-height: 32px;
    border: 0;
    padding: 0;
    background: transparent;
    color: var(--tx);
    font-size: 14px;
    box-shadow: none !important;
  }

  .header-search-form input:focus { outline: none; }

  .icon-btn {
    position: relative;
    display: grid;
    place-items: center;
    width: calc(44px * var(--ui-scale, 1));
    height: calc(44px * var(--ui-scale, 1));
    flex: none;
    border: 0;
    border-radius: 14px;
    background: var(--pn);
    color: var(--tx);
    transition: background-color var(--t-fast) ease, color var(--t-fast) ease;
  }

  .icon-btn:hover { background: var(--pn-2); }

  .icon-btn.active,
  .icon-btn[aria-current='page'] {
    background: var(--ac-soft);
    color: var(--ac);
  }

  .invite-badge {
    position: absolute;
    top: 4px;
    right: 4px;
    min-width: 16px;
    height: 16px;
    padding: 0 4px;
    border-radius: 8px;
    background: var(--ac);
    color: var(--on-ac);
    font-size: 10px;
    font-weight: 800;
    display: grid;
    place-items: center;
  }

  .mobile-nav { display: none; }

  @media (max-width: 1023px) {
    .header-inner { gap: 16px; }
    .desktop-nav a { padding: 0 12px; }
    .brand span { max-width: 120px; }
    .header-search-form { display: none; }
  }

  @media (max-width: 760px) {
    .desktop-nav,
    .header-search-form { display: none; }
    .header-inner { min-height: 60px; gap: 8px; }
    .brand { font-size: 19px; }
    .brand span { max-width: calc(100vw - 230px); }
    .mobile-nav {
      display: flex;
      position: fixed;
      inset: auto 0 0;
      z-index: 110;
      background: var(--glass);
      backdrop-filter: blur(18px);
      -webkit-backdrop-filter: blur(18px);
      border-top: 1px solid var(--line);
      padding: 4px 8px calc(4px + env(safe-area-inset-bottom, 0px));
    }
    .mobile-nav a {
      flex: 1;
      display: flex;
      flex-direction: column;
      align-items: center;
      justify-content: center;
      gap: 4px;
      min-height: 58px;
      min-width: 0;
      color: var(--mu);
      font-size: 11px;
      font-weight: 700;
    }
    .mobile-nav [aria-current='page'] { color: var(--tx); }
    :global(body:has(.mobile-nav)) {
      padding-bottom: calc(66px + env(safe-area-inset-bottom, 0px));
    }
    :global(body.music-playing:has(.mobile-nav)) {
      padding-bottom: calc(var(--music-player-height, 110px) + 90px + env(safe-area-inset-bottom, 0px));
    }
    :global(body:has(.mobile-nav) .music-player) {
      bottom: calc(66px + env(safe-area-inset-bottom, 0px));
      padding-bottom: 0;
    }
  }

  /* La CSP interdit les attributs style : couleurs du dégradé du logo ici. */
  .logo-stop-a { stop-color: var(--logo-a); }
  .logo-stop-b { stop-color: var(--logo-b); }
</style>
