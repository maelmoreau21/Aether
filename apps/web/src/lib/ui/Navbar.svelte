<script module lang="ts">
  export const navLinks = [
    { key: 'home', path: '/', label: 'Accueil', symbol: '⌂' },
    { key: 'lists', path: 'lists/', label: 'Playlists', symbol: '☷' },
    { key: 'downloads', path: 'downloads/', label: 'Téléchargements', symbol: '↓' }
  ] as const;
</script>

<script lang="ts">
  import { preloadCatalog } from '../catalog/page-cache';
  import { onMount } from 'svelte';
  import { page } from '$app/state';
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
    const refresh = () => { if (document.visibilityState === 'visible') void refreshLibraries(true); };
    window.addEventListener('focus', refresh);
    return () => window.removeEventListener('focus', refresh);
  });
  function openLibraries(event: MouseEvent) {
    opener = event.currentTarget as HTMLElement;
    libraryDialog.showModal();
    libraryMenuOpen = true;
  }
  function closeLibraries() { libraryDialog.close(); }
</script>

<header class="site-header">
  <div class="header-inner">
    <a class="brand" href={resolve('/')} onpointerenter={() => preloadCatalog('home')} onfocus={() => preloadCatalog('home')} aria-label={`${$serverPresentation.serverName || name}, accueil`}>
      <img src={asset('images/brand-mark.svg')} width="32" height="32" alt="" />
      <span>{$serverPresentation.serverName || name}</span>
    </a>
    <nav class="desktop-nav" aria-label="Navigation principale">
      <a href={resolve('/')} aria-current={section === 'home' ? 'page' : undefined}>Accueil</a>
      <button class:active={librarySection} aria-haspopup="dialog" aria-expanded={libraryMenuOpen} aria-controls="library-navigation" onclick={openLibraries}>
        <span>{currentLibrary?.name || 'Bibliothèques'}</span><Icon name="chevron" size={14} />
      </button>
      <a href={resolve('lists/')} aria-current={section === 'lists' ? 'page' : undefined}>Playlists</a>
      <a href={resolve('downloads/')} aria-current={section === 'downloads' ? 'page' : undefined}>Téléchargements</a>
    </nav>
    <div class="header-actions">
      <a class="quick-search-btn" href={resolve('search/')} aria-label="Rechercher" aria-current={section === 'search' ? 'page' : undefined}><Icon name="search" size={20} /></a>
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
  <header class="dialog-heading"><div><p>Votre médiathèque</p><h2 id="library-navigation-title">Bibliothèques</h2></div><button class="close-button" aria-label="Fermer les bibliothèques" onclick={closeLibraries}><Icon name="close" /></button></header>
  <nav class="library-list" aria-label="Choisir une bibliothèque">
    {#each $accessibleLibraries as library (library.id)}
      <a href={resolve(`${library.kind}/?library=${encodeURIComponent(library.id)}` as 'films/')} onclick={closeLibraries} aria-current={currentLibrary?.id === library.id && librarySection ? 'page' : undefined} onpointerenter={() => { if (library.kind !== 'music') preloadCatalog(library.kind, library.id); }} onfocus={() => { if (library.kind !== 'music') preloadCatalog(library.kind, library.id); }}>
        <span class="library-icon" aria-hidden="true">{#if library.kind === 'music'}♫{:else}<Icon name={library.kind === 'films' ? 'film' : 'tv'} size={22} />{/if}</span>
        <span class="library-name"><strong>{library.name}</strong><small>{library.kind === 'music' ? 'Musique' : library.kind === 'films' ? 'Films' : 'Séries'}</small></span><Icon name="chevron" size={16} />
      </a>
    {/each}
  </nav>
  {#if libraryError}<p role="alert">{libraryError}</p><button class="retry" disabled={loadingLibraries} onclick={() => refreshLibraries(true)}>Réessayer</button>
  {:else if loadingLibraries}<p role="status">Chargement des bibliothèques…</p>
  {:else if !$accessibleLibraries.length}<p>Aucune bibliothèque accessible avec ce compte.</p>{/if}
  <a class="downloads-link" href={resolve('downloads/')} onclick={closeLibraries}><span aria-hidden="true">↓</span> Téléchargements sur cet appareil <Icon name="chevron" size={16} /></a>
</dialog>

<style>
  .site-header { position:sticky; top:0; z-index:40; background:var(--color-canvas); border-bottom:1px solid var(--color-line); }
  .header-inner { display:flex; align-items:center; gap:32px; min-height:72px; max-width:1600px; padding:0 var(--page-gutter); margin:auto; }
  .brand { display:flex; align-items:center; gap:10px; min-width:0; flex-shrink:1; font-size:23px; font-weight:750; letter-spacing:-.04em; }
  .brand img { flex:none; }
  .brand span { max-width:200px; overflow:hidden; text-overflow:ellipsis; white-space:nowrap; }
  .desktop-nav { display:flex; align-items:center; gap:8px; min-width:0; }
  .desktop-nav a,.desktop-nav button { display:flex; align-items:center; gap:8px; min-height:44px; padding:10px 12px; border:0; border-radius:var(--radius-sm); background:transparent; color:var(--color-muted); font-size:14px; white-space:nowrap; }
  .desktop-nav button span { max-width:160px; overflow:hidden; text-overflow:ellipsis; }
  .desktop-nav button :global(svg) { transform:rotate(90deg); }
  .desktop-nav :is(a,button):hover { background:var(--color-surface); color:var(--color-text); }
  [aria-current='page'],.desktop-nav .active { color:var(--color-text); font-weight:650; }
  .desktop-nav [aria-current='page'],.desktop-nav .active { background:var(--color-accent-soft); }
  .header-actions { display:flex; align-items:center; gap:12px; margin-left:auto; flex:none; }
  .quick-search-btn,.close-button { display:grid; place-items:center; width:44px; height:44px; border:1px solid var(--color-line); border-radius:var(--radius-full); color:var(--color-text); background:transparent; }
  .quick-search-btn:hover,.close-button:hover { background:var(--color-raised); }
  .mobile-nav { display:none; }
  .library-dialog { width:min(480px,calc(100% - 32px)); max-width:none; max-height:calc(100dvh - 32px); padding:24px; border:1px solid var(--color-line); border-radius:var(--radius-xl); color:var(--color-text); background:var(--color-surface); overscroll-behavior:contain; }
  .library-dialog[open] { display:flex; flex-direction:column; }
  :global(body:has(.library-dialog[open])) { overflow:hidden; }
  .library-dialog::backdrop { background:rgb(0 0 0 / .6); }
  .dialog-heading { display:flex; align-items:center; justify-content:space-between; gap:16px; margin-bottom:20px; flex:none; }
  .dialog-heading p { margin:0 0 6px; font-size:12px; color:var(--color-muted); }
  h2 { margin:0; font-size:26px; letter-spacing:-.03em; }
  .library-list { display:flex; flex-direction:column; gap:4px; overflow-y:auto; min-height:0; overscroll-behavior:contain; }
  .library-list a { display:flex; align-items:center; gap:14px; min-height:72px; padding:12px; border-radius:var(--radius-md); flex:none; }
  .library-list a:hover,.library-list [aria-current='page'] { background:var(--color-accent-soft); }
  .library-icon { display:grid; place-items:center; width:44px; height:44px; flex:none; border-radius:var(--radius-sm); background:var(--color-raised); font-size:24px; }
  .library-name { flex:1; min-width:0; overflow-wrap:anywhere; }
  .library-name strong { display:block; font-size:15px; }
  small { display:block; margin-top:4px; color:var(--color-muted); font-size:12px; font-weight:400; }
  .downloads-link { display:flex; align-items:center; gap:12px; min-height:56px; margin-top:16px; padding-top:16px; border-top:1px solid var(--color-line); font-size:14px; flex:none; }
  .downloads-link :global(svg) { margin-left:auto; flex:none; }
  .retry { min-height:44px; padding:10px 16px; border:1px solid var(--color-line); border-radius:var(--radius-sm); background:var(--color-raised); color:inherit; }
  [role='alert'] { color:var(--color-danger); }
  @media(max-width:1100px) { .header-inner { gap:20px; } .desktop-nav { gap:2px; } .desktop-nav a,.desktop-nav button { padding-inline:10px; font-size:13px; } .brand span { max-width:120px; } }
  @media(max-width:900px) {
    .desktop-nav,.quick-search-btn { display:none; }
    .header-inner { min-height:64px; gap:16px; }
    .brand { font-size:21px; } .brand span { max-width:calc(100vw - 150px); }
    .mobile-nav { display:grid; grid-template-columns:repeat(4,minmax(0,1fr)); position:fixed; inset:auto 0 0; z-index:110; background:var(--color-surface); border-top:1px solid var(--color-line); padding:4px 8px calc(4px + env(safe-area-inset-bottom,0px)); }
    .mobile-nav a,.mobile-nav button { display:flex; flex-direction:column; align-items:center; justify-content:center; gap:4px; min-height:56px; min-width:0; border:0; border-radius:var(--radius-sm); padding:4px 0; background:transparent; color:var(--color-muted); font-size:11px; }
    .mobile-nav a>span:first-child { font-size:23px; line-height:21px; }
    .mobile-nav [aria-current='page'],.mobile-nav .active { color:var(--color-accent); background:var(--color-accent-soft); }
    .library-dialog { margin:auto 0 0; width:100%; max-height:85dvh; border-radius:var(--radius-xl) var(--radius-xl) 0 0; padding:20px 16px calc(20px + env(safe-area-inset-bottom,0px)); }
    :global(body:has(.mobile-nav)) { padding-bottom:calc(64px + env(safe-area-inset-bottom,0px)); }
    :global(body.music-playing:has(.mobile-nav)) { padding-bottom:calc(244px + env(safe-area-inset-bottom,0px)); }
    :global(body:has(.mobile-nav) .music-player) { bottom:calc(64px + env(safe-area-inset-bottom,0px)); padding-bottom:0; }
  }
</style>
