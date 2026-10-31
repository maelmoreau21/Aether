<script module lang="ts">
  export const navLinks = [
    { key: 'home', path: '/', label: 'Accueil', symbol: '⌂' },
    { key: 'lists', path: 'lists/', label: 'Playlists', symbol: '☷' },
    { key: 'downloads', path: 'downloads/', label: 'Téléchargements', symbol: '↓' }
  ] as const;
</script>

<script lang="ts">
  import { preloadCatalog } from '../catalog/page-cache';
  import { onMount, tick } from 'svelte';
  import { page } from '$app/state';
  import { goto } from '$app/navigation';
  import { session } from '../auth/session';
  import { accessibleLibraries, loadLibraries } from '../catalog/libraries';
  import { asset, resolve } from '$app/paths';
  import Icon from './Icon.svelte';
  import ProfileMenu from './ProfileMenu.svelte';
  import { serverPresentation } from '../config/presentation';

  let { section = 'home', name = 'Aether' }: {
    section?: 'home' | 'films' | 'series' | 'music' | 'lists' | 'downloads' | 'search' | 'settings' | 'admin';
    name?: string;
  } = $props();

  let libraryError = $state('');
  let loadingLibraries = $state(false);
  let libraryDialog: HTMLDialogElement;
  let libraryMenuOpen = $state(false);
  let opener: HTMLElement | null = null;
  let isScrolled = $state(false);
  let searchExpanded = $state(false);
  let headerSearchQuery = $state('');
  let headerSearchInput = $state<HTMLInputElement>();

  const librarySection = $derived(['films', 'series', 'music'].includes(section));
  const currentLibrary = $derived($accessibleLibraries.find(library => library.id === page.url.searchParams.get('library')));

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
    const refresh = () => { if (document.visibilityState === 'visible') void refreshLibraries(true); };
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
      <img src={asset('images/brand-mark.svg')} width="32" height="32" alt="" />
      <span>{$serverPresentation.serverName || name}</span>
    </a>

    <nav class="desktop-nav" aria-label="Navigation principale">
      <a href={resolve('/')} aria-current={section === 'home' ? 'page' : undefined}>Accueil</a>
      {#each $accessibleLibraries as library (library.id)}
        <a
          class="library-nav-link"
          href={resolve(`${library.kind}/?library=${encodeURIComponent(library.id)}` as 'films/')}
          aria-current={currentLibrary?.id === library.id && librarySection ? 'page' : undefined}
          onpointerenter={() => { if (library.kind !== 'music') preloadCatalog(library.kind, library.id); }}
          onfocus={() => { if (library.kind !== 'music') preloadCatalog(library.kind, library.id); }}
        >
          {#if library.kind === 'music'}
            <span class="lib-kind-glyph" aria-hidden="true">♫</span>
          {:else}
            <Icon name={library.kind === 'films' ? 'film' : 'tv'} size={15} />
          {/if}
          <span>{library.name}</span>
        </a>
      {:else}
        <a href={resolve('films/')} aria-current={section === 'films' ? 'page' : undefined}>
          <Icon name="film" size={15} /><span>Films</span>
        </a>
        <a href={resolve('series/')} aria-current={section === 'series' ? 'page' : undefined}>
          <Icon name="tv" size={15} /><span>Séries</span>
        </a>
      {/each}
      {#if $accessibleLibraries.length > 6}
        <button class="all-libraries-btn" class:active={librarySection} aria-haspopup="dialog" aria-expanded={libraryMenuOpen} aria-controls="library-navigation" title="Toutes les bibliothèques" aria-label="Toutes les bibliothèques" onclick={openLibraries}>
          <Icon name="chevron" size={14} />
        </button>
      {/if}
    </nav>

    <div class="header-actions">
      {#if searchExpanded}
        <form class="header-search-form" onsubmit={submitQuickSearch}>
          <Icon name="search" size={16} />
          <input
            bind:this={headerSearchInput}
            type="search"
            bind:value={headerSearchQuery}
            placeholder="Titres, genres, artistes…"
            aria-label="Recherche rapide"
            onkeydown={e => { if (e.key === 'Escape') searchExpanded = false; }}
            onblur={() => { if (!headerSearchQuery.trim()) searchExpanded = false; }}
          />
        </form>
      {/if}
      <a
        class="quick-search-btn action-icon-btn"
        class:active={section === 'search' || searchExpanded}
        href={resolve('search/')}
        onclick={toggleQuickSearch}
        title="Rechercher"
        aria-label="Rechercher"
        aria-current={section === 'search' ? 'page' : undefined}
      >
        <Icon name="search" size={19} />
      </a>
      <a
        class="action-icon-btn"
        class:active={section === 'lists'}
        href={resolve('lists/')}
        title="Playlists"
        aria-label="Playlists"
        aria-current={section === 'lists' ? 'page' : undefined}
      >
        <Icon name="playlist" size={19} />
      </a>
      <a
        class="action-icon-btn"
        class:active={section === 'downloads'}
        href={resolve('downloads/')}
        title="Téléchargements"
        aria-label="Téléchargements"
        aria-current={section === 'downloads' ? 'page' : undefined}
      >
        <Icon name="download" size={19} />
      </a>
      <ProfileMenu />
    </div>
  </div>
</header>

<nav class="mobile-nav" aria-label="Navigation principale mobile">
  <a href={resolve('/')} aria-current={section === 'home' ? 'page' : undefined}><span aria-hidden="true">⌂</span><span>Accueil</span></a>
  <button class:active={librarySection} aria-haspopup="dialog" aria-expanded={libraryMenuOpen} aria-controls="library-navigation" onclick={openLibraries}><Icon name="film" size={21} /><span>Bibliothèques</span></button>
  <a href={resolve('search/')} aria-current={section === 'search' ? 'page' : undefined}><Icon name="search" size={21} /><span>Recherche</span></a>
  <a href={resolve('lists/')} aria-current={section === 'lists' ? 'page' : undefined}><span aria-hidden="true">☷</span><span>Playlists</span></a>
</nav>

<dialog id="library-navigation" class="library-dialog" bind:this={libraryDialog} aria-labelledby="library-navigation-title" onclose={() => { libraryMenuOpen = false; opener?.focus(); }}>
  <header class="dialog-heading">
    <div>
      <p>Votre médiathèque</p>
      <h2 id="library-navigation-title">Bibliothèques</h2>
    </div>
    <button class="close-button" aria-label="Fermer les bibliothèques" onclick={closeLibraries}><Icon name="close" /></button>
  </header>
  <nav class="library-list" aria-label="Choisir une bibliothèque">
    {#each $accessibleLibraries as library (library.id)}
      <a
        href={resolve(`${library.kind}/?library=${encodeURIComponent(library.id)}` as 'films/')}
        onclick={closeLibraries}
        aria-current={currentLibrary?.id === library.id && librarySection ? 'page' : undefined}
        onpointerenter={() => { if (library.kind !== 'music') preloadCatalog(library.kind, library.id); }}
        onfocus={() => { if (library.kind !== 'music') preloadCatalog(library.kind, library.id); }}
      >
        <span class="library-icon" aria-hidden="true">
          {#if library.kind === 'music'}♫{:else}<Icon name={library.kind === 'films' ? 'film' : 'tv'} size={22} />{/if}
        </span>
        <span class="library-name">
          <strong>{library.name}</strong>
          <small>{library.kind === 'music' ? 'Musique' : library.kind === 'films' ? 'Films' : 'Séries'}</small>
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
  <a class="downloads-link" href={resolve('downloads/')} onclick={closeLibraries}>
    <span aria-hidden="true">↓</span> Téléchargements sur cet appareil <Icon name="chevron" size={16} />
  </a>
</dialog>

<style>
  .site-header {
    position: sticky;
    top: 0;
    z-index: 40;
    background: linear-gradient(180deg, rgba(11, 12, 16, 0.85) 0%, rgba(11, 12, 16, 0.45) 70%, transparent 100%);
    border-bottom: 1px solid transparent;
    transition: background var(--transition-normal), border-color var(--transition-normal), backdrop-filter var(--transition-normal), box-shadow var(--transition-normal);
  }

  :global([data-theme="light"]) .site-header:not(.scrolled) {
    background: linear-gradient(180deg, rgba(227, 222, 214, 0.92) 0%, rgba(227, 222, 214, 0.6) 100%);
  }

  .site-header.scrolled {
    background: var(--color-glass);
    backdrop-filter: blur(16px) saturate(180%);
    -webkit-backdrop-filter: blur(16px) saturate(180%);
    border-bottom-color: var(--color-line);
    box-shadow: var(--shadow-sm);
  }

  .header-inner {
    display: flex;
    align-items: center;
    gap: 24px;
    min-height: 72px;
    width: 100%;
    max-width: none;
    padding: 0 var(--page-gutter);
    margin: 0;
  }

  .brand {
    display: flex;
    align-items: center;
    gap: 10px;
    min-width: 0;
    flex-shrink: 0;
    font-family: var(--font-display);
    font-size: 23px;
    font-weight: 800;
    letter-spacing: -0.04em;
    color: var(--color-text-bright);
  }

  .brand img {
    flex: none;
    filter: drop-shadow(0 2px 8px rgba(249, 115, 22, 0.3));
  }

  .brand span {
    max-width: 220px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .desktop-nav {
    display: flex;
    align-items: center;
    gap: 6px;
    flex: 1 1 auto;
    min-width: 0;
    overflow-x: auto;
    scrollbar-width: none;
  }

  .desktop-nav::-webkit-scrollbar {
    display: none;
  }

  .desktop-nav a,
  .desktop-nav button {
    display: inline-flex;
    align-items: center;
    gap: 8px;
    min-height: 42px;
    padding: 8px 15px;
    border: 1px solid transparent;
    border-radius: var(--radius-pill);
    background: transparent;
    color: var(--color-muted);
    font-size: 14px;
    font-weight: 550;
    white-space: nowrap;
    flex-shrink: 0;
  }

  .library-nav-link span {
    max-width: 220px;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .lib-kind-glyph {
    font-size: 14px;
    line-height: 1;
  }

  .all-libraries-btn {
    padding: 8px 10px !important;
  }

  .desktop-nav button :global(svg) {
    transform: rotate(90deg);
    transition: transform var(--transition-fast);
  }

  .desktop-nav button[aria-expanded='true'] :global(svg) {
    transform: rotate(-90deg);
  }

  .desktop-nav :is(a, button):hover {
    background: rgba(255, 255, 255, 0.07);
    color: var(--color-text-bright);
  }

  :global([data-theme="light"]) .desktop-nav :is(a, button):hover {
    background: rgba(33, 30, 27, 0.07);
  }

  [aria-current='page'],
  .desktop-nav .active {
    color: var(--color-text-bright);
    font-weight: 700;
  }

  .desktop-nav [aria-current='page'],
  .desktop-nav .active {
    background: var(--color-accent-soft);
    border-color: rgba(249, 115, 22, 0.35);
    color: var(--color-text-bright);
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
    height: 40px;
    padding: 0 14px;
    border-radius: var(--radius-pill);
    background: var(--color-glass-raised);
    backdrop-filter: blur(16px);
    -webkit-backdrop-filter: blur(16px);
    border: 1px solid var(--color-accent);
    color: var(--color-muted);
    box-shadow: 0 4px 16px rgba(0, 0, 0, 0.35);
    animation: expandSearch 180ms cubic-bezier(0.16, 1, 0.3, 1);
  }

  @keyframes expandSearch {
    from {
      opacity: 0;
      width: 120px;
    }
    to {
      opacity: 1;
      width: 240px;
    }
  }

  .header-search-form input {
    width: 185px;
    min-height: 32px;
    height: 32px;
    border: 0;
    padding: 0;
    background: transparent;
    color: var(--color-text-bright);
    font-size: 13.5px;
    box-shadow: none !important;
  }

  .header-search-form input:focus {
    outline: none;
    box-shadow: none !important;
  }

  .action-icon-btn,
  .quick-search-btn,
  .close-button {
    display: grid;
    place-items: center;
    width: 42px;
    height: 42px;
    border: 1px solid var(--color-line);
    border-radius: var(--radius-full);
    color: var(--color-muted);
    background: rgba(255, 255, 255, 0.04);
    transition: background var(--transition-fast), border-color var(--transition-fast), color var(--transition-fast), transform var(--transition-fast);
  }

  :global([data-theme="light"]) .action-icon-btn,
  :global([data-theme="light"]) .quick-search-btn {
    background: rgba(33, 30, 27, 0.04);
  }

  .action-icon-btn:hover,
  .quick-search-btn:hover,
  .close-button:hover {
    background: var(--color-raised);
    border-color: var(--color-line-strong);
    color: var(--color-text-bright);
    transform: translateY(-1px);
  }

  .action-icon-btn.active,
  .action-icon-btn[aria-current='page'],
  .quick-search-btn.active {
    background: var(--color-accent-soft);
    border-color: rgba(249, 115, 22, 0.4);
    color: var(--color-accent);
  }

  .mobile-nav {
    display: none;
  }

  .library-dialog {
    width: min(500px, calc(100% - 32px));
    max-width: none;
    max-height: calc(100dvh - 32px);
    padding: 24px;
    border: 1px solid var(--color-line-strong);
    border-radius: var(--radius-xl);
    color: var(--color-text);
    background: var(--color-glass-raised);
    backdrop-filter: blur(24px);
    -webkit-backdrop-filter: blur(24px);
    box-shadow: var(--shadow-lg);
    overscroll-behavior: contain;
  }

  .library-dialog[open] {
    display: flex;
    flex-direction: column;
  }

  :global(body:has(.library-dialog[open])) {
    overflow: hidden;
  }

  .library-dialog::backdrop {
    background: rgba(0, 0, 0, 0.72);
    backdrop-filter: blur(6px);
  }

  .dialog-heading {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 16px;
    margin-bottom: 20px;
    flex: none;
  }

  .dialog-heading p {
    margin: 0 0 4px;
    font-size: 11px;
    font-weight: 700;
    text-transform: uppercase;
    letter-spacing: 0.08em;
    color: var(--color-accent);
  }

  h2 {
    margin: 0;
    font-family: var(--font-display);
    font-size: 26px;
    letter-spacing: -0.03em;
  }

  .library-list {
    display: flex;
    flex-direction: column;
    gap: 6px;
    overflow-y: auto;
    min-height: 0;
    overscroll-behavior: contain;
  }

  .library-list a {
    display: flex;
    align-items: center;
    gap: 14px;
    min-height: 72px;
    padding: 12px 14px;
    border-radius: var(--radius-md);
    border: 1px solid transparent;
    flex: none;
    transition: background var(--transition-fast), border-color var(--transition-fast), transform var(--transition-fast);
  }

  .library-list a:hover,
  .library-list [aria-current='page'] {
    background: var(--color-accent-soft);
    border-color: rgba(249, 115, 22, 0.35);
  }

  .library-icon {
    display: grid;
    place-items: center;
    width: 46px;
    height: 46px;
    flex: none;
    border-radius: var(--radius-md);
    background: var(--color-raised);
    border: 1px solid var(--color-line);
    font-size: 22px;
    color: var(--color-text-bright);
  }

  .library-name {
    flex: 1;
    min-width: 0;
    overflow-wrap: anywhere;
  }

  .library-name strong {
    display: block;
    font-size: 15px;
    color: var(--color-text-bright);
  }

  small {
    display: block;
    margin-top: 4px;
    color: var(--color-muted);
    font-size: 12px;
    font-weight: 400;
  }

  .downloads-link {
    display: flex;
    align-items: center;
    gap: 12px;
    min-height: 56px;
    margin-top: 16px;
    padding-top: 16px;
    border-top: 1px solid var(--color-line);
    font-size: 14px;
    font-weight: 500;
    flex: none;
  }

  .downloads-link:hover {
    color: var(--color-accent-hover);
  }

  .downloads-link :global(svg) {
    margin-left: auto;
    flex: none;
  }

  .retry {
    min-height: 44px;
    padding: 10px 16px;
    border: 1px solid var(--color-line);
    border-radius: var(--radius-sm);
    background: var(--color-raised);
    color: inherit;
  }

  [role='alert'] {
    color: var(--color-danger);
  }

  @media (min-width: 1920px) {
    .header-inner {
      min-height: 82px;
      max-width: none;
    }
    .brand {
      font-size: 26px;
    }
    .desktop-nav a,
    .desktop-nav button {
      font-size: 15px;
      padding: 10px 18px;
    }
  }

  @media (max-width: 1100px) {
    .header-inner {
      gap: 20px;
    }
    .desktop-nav {
      gap: 2px;
    }
    .desktop-nav a,
    .desktop-nav button {
      padding-inline: 10px;
      font-size: 13px;
    }
    .brand span {
      max-width: 120px;
    }
    .header-search-form {
      display: none;
    }
  }

  @media (max-width: 900px) {
    .desktop-nav,
    .quick-search-btn,
    .header-search-form {
      display: none;
    }
    .header-inner {
      min-height: 64px;
      gap: 16px;
    }
    .brand {
      font-size: 21px;
    }
    .brand span {
      max-width: calc(100vw - 150px);
    }
    .mobile-nav {
      display: grid;
      grid-template-columns: repeat(4, minmax(0, 1fr));
      position: fixed;
      inset: auto 0 0;
      z-index: 110;
      background: var(--color-glass-raised);
      backdrop-filter: blur(20px);
      -webkit-backdrop-filter: blur(20px);
      border-top: 1px solid var(--color-line);
      padding: 4px 8px calc(4px + env(safe-area-inset-bottom, 0px));
    }
    .mobile-nav a,
    .mobile-nav button {
      display: flex;
      flex-direction: column;
      align-items: center;
      justify-content: center;
      gap: 4px;
      min-height: 56px;
      min-width: 0;
      border: 0;
      border-radius: var(--radius-sm);
      padding: 4px 0;
      background: transparent;
      color: var(--color-muted);
      font-size: 11px;
      font-weight: 500;
    }
    .mobile-nav a > span:first-child {
      font-size: 22px;
      line-height: 21px;
    }
    .mobile-nav [aria-current='page'],
    .mobile-nav .active {
      color: var(--color-text-bright);
      background: var(--color-accent-soft);
    }
    .library-dialog {
      margin: auto 0 0;
      width: 100%;
      max-height: 85dvh;
      border-radius: var(--radius-xl) var(--radius-xl) 0 0;
      padding: 20px 16px calc(20px + env(safe-area-inset-bottom, 0px));
    }
    :global(body:has(.mobile-nav)) {
      padding-bottom: calc(64px + env(safe-area-inset-bottom, 0px));
    }
    :global(body.music-playing:has(.mobile-nav)) {
      padding-bottom: calc(var(--music-player-height, 110px) + 88px + env(safe-area-inset-bottom, 0px));
    }
    :global(body:has(.mobile-nav) .music-player) {
      bottom: calc(64px + env(safe-area-inset-bottom, 0px));
      padding-bottom: 0;
    }
  }
</style>
