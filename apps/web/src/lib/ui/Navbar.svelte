<script module lang="ts">
  export const navLinks = [
    { key: 'home', path: '/', label: 'Accueil', symbol: '⌂' },
    { key: 'lists', path: 'lists/', label: 'Mes listes', symbol: '☷' },
    { key: 'downloads', path: 'downloads/', label: 'Téléchargements', symbol: '↓' }
  ] as const;
</script>

<script lang="ts">
  import { preloadCatalog } from '../catalog/page-cache';
  import { onMount, tick } from 'svelte';
  import { page } from '$app/state';
  import { goto } from '$app/navigation';
  import { session } from '../auth/session';
  import { accessibleLibraries, loadLibraries, type AccessibleLibrary } from '../catalog/libraries';
  import { resolve } from '$app/paths';
  import Icon from './Icon.svelte';
  import ProfileMenu from './ProfileMenu.svelte';
  import WatchPartyModal from '../syncplay/WatchPartyModal.svelte';
  import { serverPresentation } from '../config/presentation';
  import { features, loadFeatures } from '../applications/api';

  type Kind = AccessibleLibrary['kind'];

  let { section = 'home', name = 'Aether' }: {
    section?: 'home' | 'films' | 'series' | 'music' | 'lists' | 'downloads' | 'search' | 'settings' | 'admin' | 'requests' | 'reports';
    name?: string;
  } = $props();

  const kindLabels: Record<Kind, string> = { films: 'Films', series: 'Séries', music: 'Musique' };

  let libraryError = $state('');
  let loadingLibraries = $state(false);
  let libraryDialog: HTMLDialogElement;
  let libraryMenuOpen = $state(false);
  let watchPartyOpen = $state(false);
  let pendingInvitesCount = $state(0);

  onMount(() => {
    void loadInvitesCount();
    void loadFeatures().catch(() => {});
  });

  async function loadInvitesCount() {
    try {
      const res = await fetch('/api/v1/parties/invites');
      if (res.ok) {
        const data = await res.json();
        pendingInvitesCount = (data.invites ?? []).length;
      }
    } catch {}
  }
  let opener: HTMLElement | null = null;
  let isScrolled = $state(false);
  let searchExpanded = $state(false);
  let headerSearchQuery = $state('');
  let headerSearchInput = $state<HTMLInputElement>();

  const currentLibrary = $derived($accessibleLibraries.find(library => library.id === page.url.searchParams.get('library')));
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

  async function refreshLibraries(force = false) {
    loadingLibraries = true;
    try { await loadLibraries(force); libraryError = ''; }
    catch { libraryError = 'Impossible de charger vos bibliothèques.'; }
    finally { loadingLibraries = false; }
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

  function openLibraries(event: MouseEvent) {
    opener = event.currentTarget as HTMLElement;
    libraryDialog.showModal();
    libraryMenuOpen = true;
  }

  function closeLibraries() { libraryDialog.close(); }

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
    <a class="brand" href={resolve('/')} onpointerenter={() => preloadCatalog('home')} onfocus={() => preloadCatalog('home')} aria-label={`${$serverPresentation.serverName || name}, accueil`}>
      <svg class="brand-mark" viewBox="0 0 256 256" width="28" height="28" aria-hidden="true">
        <defs><linearGradient id="halo-brand" x1="0" y1="0" x2="1" y2="1"><stop offset="0" class="logo-stop-a" /><stop offset="1" class="logo-stop-b" /></linearGradient></defs>
        <path d="M64 192 V48 L216 128 L116 180" fill="none" stroke="url(#halo-brand)" stroke-width="30" stroke-linecap="round" stroke-linejoin="round" />
      </svg>
      <span>{$serverPresentation.serverName || name}</span>
    </a>

    <nav class="desktop-nav" aria-label="Navigation principale">
      <a href={resolve('/')} aria-current={section === 'home' ? 'page' : undefined}>Accueil</a>
      {#each kinds as kind (kind)}
        <!-- eslint-disable-next-line svelte/no-navigation-without-resolve -- kindHref() resolves the URL. -->
        <a href={kindHref(kind)} aria-current={section === kind ? 'page' : undefined} onpointerenter={() => preloadKind(kind)} onfocus={() => preloadKind(kind)}>{kindLabels[kind]}</a>
      {/each}
      <a href={resolve('lists/')} aria-current={section === 'lists' ? 'page' : undefined}>Mes listes</a>
      {#if $features.requests}<a href={resolve('requests/')} aria-current={section === 'requests' ? 'page' : undefined}>Demandes</a>{/if}
    </nav>

    <div class="header-actions">
      {#if searchExpanded}
        <form class="header-search-form" onsubmit={submitQuickSearch}>
          <Icon name="search" size={16} />
          <input
            bind:this={headerSearchInput}
            type="search"
            bind:value={headerSearchQuery}
            placeholder="Films, séries, artistes…"
            aria-label="Recherche rapide"
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
        title="Rechercher"
        aria-label="Rechercher"
        aria-current={section === 'search' ? 'page' : undefined}
      >
        <Icon name="search" size={19} />
      </a>
      <button
        type="button"
        class="icon-btn"
        class:active={libraryMenuOpen || Boolean(currentLibrary)}
        title="Bibliothèques"
        aria-label={currentLibrary ? `Bibliothèque : ${currentLibrary.name}` : 'Bibliothèques'}
        aria-haspopup="dialog"
        aria-expanded={libraryMenuOpen}
        aria-controls="library-navigation"
        onclick={openLibraries}
      >
        <Icon name="film" size={19} />
      </button>
      <button
        type="button"
        class="icon-btn watch-party-btn hide-phone"
        title="Séances Watch Party"
        aria-label="Séances Watch Party"
        onclick={() => (watchPartyOpen = true)}
      >
        <Icon name="users" size={19} />
        {#if pendingInvitesCount > 0}
          <span class="invite-badge">{pendingInvitesCount}</span>
        {/if}
      </button>
      <ProfileMenu />
    </div>
  </div>
</header>

<nav class="mobile-nav" aria-label="Navigation principale mobile">
  <a href={resolve('/')} aria-current={section === 'home' ? 'page' : undefined}>
    <svg width="22" height="22" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" aria-hidden="true"><path d="M4 11 12 4l8 7v9h-5v-6H9v6H4z" /></svg>
    <span>Accueil</span>
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
      <span>{kindLabels[kind]}</span>
    </a>
  {/each}
  <a href={resolve('lists/')} aria-current={section === 'lists' ? 'page' : undefined}>
    <svg width="22" height="22" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" aria-hidden="true"><path d="M5 4h14v17l-7-4-7 4z" /></svg>
    <span>Listes</span>
  </a>
</nav>

<dialog id="library-navigation" class="library-dialog" bind:this={libraryDialog} aria-labelledby="library-navigation-title" onclose={() => { libraryMenuOpen = false; opener?.focus(); }}>
  <header class="dialog-heading">
    <div>
      <p>Votre médiathèque</p>
      <h2 id="library-navigation-title">Bibliothèques</h2>
    </div>
    <button class="icon-btn" aria-label="Fermer les bibliothèques" onclick={closeLibraries}><Icon name="close" /></button>
  </header>
  <nav class="library-list" aria-label="Choisir une bibliothèque">
    {#each $accessibleLibraries as library (library.id)}
      <a
        href={resolve(`${library.kind}/?library=${encodeURIComponent(library.id)}` as 'films/')}
        onclick={closeLibraries}
        aria-current={currentLibrary?.id === library.id ? 'page' : undefined}
        onpointerenter={() => { if (library.kind !== 'music') preloadCatalog(library.kind, library.id); }}
        onfocus={() => { if (library.kind !== 'music') preloadCatalog(library.kind, library.id); }}
      >
        <span class="library-icon" aria-hidden="true">
          {#if library.kind === 'music'}♫{:else}<Icon name={library.kind === 'films' ? 'film' : 'tv'} size={22} />{/if}
        </span>
        <span class="library-name">
          <strong>{library.name}</strong>
          <small>{kindLabels[library.kind]}</small>
        </span>
        <Icon name="chevron" size={16} />
      </a>
    {/each}
  </nav>
  {#if libraryError}
    <p role="alert">{libraryError}</p>
    <button class="retry" disabled={loadingLibraries} onclick={() => refreshLibraries(true)}>Réessayer</button>
  {:else if loadingLibraries}
    <p role="status">Chargement des bibliothèques…</p>
  {:else if !$accessibleLibraries.length}
    <p>Aucune bibliothèque accessible avec ce compte.</p>
  {/if}
  <button type="button" class="downloads-link" onclick={() => { closeLibraries(); watchPartyOpen = true; }}>
    <Icon name="users" size={18} /> Séances Watch Party <Icon name="chevron" size={16} />
  </button>
  <a class="downloads-link" href={resolve('downloads/')} onclick={closeLibraries}>
    <Icon name="download" size={18} /> Téléchargements sur cet appareil <Icon name="chevron" size={16} />
  </a>
</dialog>

{#if watchPartyOpen}
  <WatchPartyModal
    onClose={() => (watchPartyOpen = false)}
    onJoinParty={(partyId, mediaId) => {
      watchPartyOpen = false;
      window.dispatchEvent(new CustomEvent('start-watch-party', { detail: { partyId, mediaId } }));
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
    min-height: 72px;
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
    font-size: 20px;
    font-weight: 800;
    letter-spacing: -0.03em;
    color: var(--tx);
  }

  .brand-mark { flex: none; }

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
    height: 38px;
    padding: 0 16px;
    border-radius: var(--r-pill);
    color: var(--mu);
    font-size: 14px;
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
    width: 44px;
    height: 44px;
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

  .library-dialog {
    width: min(500px, calc(100% - 32px));
    max-width: none;
    max-height: calc(100dvh - 32px);
    padding: 24px;
    border: 1px solid var(--line);
    border-radius: var(--r-xl);
    color: var(--tx);
    background: var(--bg-3);
    box-shadow: var(--shadow);
    overscroll-behavior: contain;
  }

  .library-dialog[open] {
    display: flex;
    flex-direction: column;
  }

  :global(body:has(.library-dialog[open])) { overflow: hidden; }

  .library-dialog::backdrop {
    background: rgb(6 8 14 / 0.55);
    backdrop-filter: blur(4px);
  }

  .dialog-heading {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 16px;
    margin-bottom: 16px;
    flex: none;
  }

  .dialog-heading p {
    margin: 0 0 4px;
    font-size: 12px;
    font-weight: 700;
    text-transform: uppercase;
    letter-spacing: 0.07em;
    color: var(--mu);
  }

  h2 {
    margin: 0;
    font-size: 24px;
    font-weight: 800;
    letter-spacing: -0.02em;
  }

  .library-list {
    display: flex;
    flex-direction: column;
    gap: 4px;
    overflow-y: auto;
    min-height: 0;
    overscroll-behavior: contain;
  }

  .library-list a {
    display: flex;
    align-items: center;
    gap: 14px;
    min-height: 64px;
    padding: 10px 12px;
    border-radius: var(--r-md);
    flex: none;
    transition: background-color var(--t-fast) ease;
  }

  .library-list a:hover { background: var(--pn); }
  .library-list [aria-current='page'] { background: var(--ac-soft); }

  .library-icon {
    display: grid;
    place-items: center;
    width: 44px;
    height: 44px;
    flex: none;
    border-radius: 14px;
    background: var(--pn-2);
    font-size: 20px;
  }

  .library-name {
    flex: 1;
    min-width: 0;
    overflow-wrap: anywhere;
  }

  .library-name strong {
    display: block;
    font-size: 15px;
    font-weight: 700;
  }

  small {
    display: block;
    margin-top: 2px;
    color: var(--mu);
    font-size: 13px;
  }

  .downloads-link {
    display: flex;
    align-items: center;
    gap: 12px;
    width: 100%;
    min-height: 52px;
    padding: 0 12px;
    border: 0;
    border-top: 1px solid var(--line);
    background: none;
    color: var(--tx);
    font-size: 14px;
    font-weight: 600;
    text-align: left;
    flex: none;
  }

  .downloads-link:first-of-type { margin-top: 12px; }
  .downloads-link:hover { background: var(--pn); }
  .downloads-link > :global(svg:last-child) { margin-left: auto; flex: none; }

  .retry {
    min-height: 44px;
    padding: 0 16px;
    border: 0;
    border-radius: var(--r-sm);
    background: var(--pn-2);
    color: inherit;
    font-weight: 700;
  }

  [role='alert'] { color: var(--err); }

  @media (max-width: 1023px) {
    .header-inner { gap: 16px; }
    .desktop-nav a { padding: 0 12px; }
    .brand span { max-width: 120px; }
    .header-search-form { display: none; }
  }

  @media (max-width: 760px) {
    .desktop-nav,
    .header-search-form,
    .hide-phone { display: none; }
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
    .library-dialog {
      margin: auto 0 0;
      width: 100%;
      max-height: 85dvh;
      border-radius: var(--r-xl) var(--r-xl) 0 0;
      padding: 20px 16px calc(20px + env(safe-area-inset-bottom, 0px));
    }
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
