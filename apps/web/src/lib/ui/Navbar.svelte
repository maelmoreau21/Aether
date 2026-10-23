<script module lang="ts">
  export const navLinks = [
    { key: 'home', path: '/', label: 'Accueil', symbol: '⌂' },
    { key: 'lists', path: 'lists/', label: 'Playlists', symbol: '☷' },
    { key: 'downloads', path: 'downloads/', label: 'Téléchargements', symbol: '↓' }
  ] as const;
</script>

<script lang="ts">
  import { onMount } from 'svelte';
  import { page } from '$app/state';
  import { session } from '../auth/session';
  import { accessibleLibraries, loadLibraries } from '../catalog/libraries';
  import { resolve } from '$app/paths';
  import Icon from './Icon.svelte';
  import ProfileMenu from './ProfileMenu.svelte';
  import { serverPresentation } from '../config/presentation';

  let {
    section = 'home',
    name = 'Aether'
  }: {
    section?: 'home' | 'films' | 'series' | 'music' | 'lists' | 'downloads' | 'search' | 'settings' | 'admin';
    name?: string;
  } = $props();

  const base = $derived(resolve('/').replace(/\/$/, ''));
  let scrolled = $state(false);
  let libraryError = $state('');
  $effect(()=>{const id=$session?.user.id;if(id)void loadLibraries().then(()=>libraryError='').catch(()=>libraryError='Bibliothèques indisponibles');});
  onMount(()=>{const refresh=()=>{if(document.visibilityState==='visible')void loadLibraries(true).then(()=>libraryError='').catch(()=>{});};window.addEventListener('focus',refresh);return()=>window.removeEventListener('focus',refresh);});

  function handleScroll() {
    scrolled = window.scrollY > 20;
  }
</script>

<svelte:window onscroll={handleScroll} />

<header class="site-header" class:scrolled>
  <div class="header-inner">
    <div class="header-brand-group">
      <a class="brand" href={resolve('/')} aria-label={`${$serverPresentation.serverName || name}, accueil`}>
        <div class="brand-logo-wrapper">
          <img src={`${base}/images/brand-mark.svg`} width="32" height="32" alt="" />
        </div>
        <span class="brand-text">{$serverPresentation.serverName || name}</span>
      </a>

      <nav class="main-nav" aria-label="Navigation principale">
        {#each navLinks.filter(link=>link.key==='home') as link (link.key)}
          <a
            href={resolve(link.path)}
            class="nav-link"
            aria-label={link.label}
            class:active={section === link.key}
            aria-current={section === link.key ? 'page' : undefined}
          >
            <b class="nav-symbol" aria-hidden="true">{link.symbol}</b><span>{link.label}</span>
          </a>
        {/each}
        {#each $accessibleLibraries as library (library.id)}
          <a class="nav-link library-link" href={resolve(`${library.kind}/?library=${encodeURIComponent(library.id)}` as 'films/')} class:active={page.url.searchParams.get('library')===library.id} aria-current={page.url.searchParams.get('library')===library.id?'page':undefined}>
            {#if library.kind==='music'}<b aria-hidden="true">♫</b>{:else}<Icon name={library.kind==='films'?'film':'tv'} size={16} />{/if}<span>{library.name}</span>
          </a>
        {/each}
        {#each navLinks.filter(link=>link.key!=='home') as link (link.key)}<a class="nav-link" href={resolve(link.path)} class:active={section===link.key} aria-label={link.label}><b class="nav-symbol" aria-hidden="true">{link.symbol}</b><span>{link.label}</span></a>{/each}
        {#if libraryError}<button class="nav-link" onclick={()=>loadLibraries(true).then(()=>libraryError='').catch(()=>{})}>{libraryError} · Réessayer</button>{/if}
      </nav>
    </div>

    <div class="header-actions">
      <a
        href={resolve('search/')}
        class="quick-search-btn"
        class:active={section === 'search'}
        aria-current={section === 'search' ? 'page' : undefined}
        aria-label="Recherche rapide"
        title="Rechercher dans le catalogue"
      >
        <Icon name="search" size={18} />
      </a>
      <ProfileMenu />
    </div>
  </div>
</header>

<style>
  .site-header {
    position: sticky;
    top: 0;
    z-index: 40;
    width: 100%;
    background: var(--color-glass);
    backdrop-filter: blur(20px);
    -webkit-backdrop-filter: blur(20px);
    border-bottom: 1px solid var(--color-line);
    transition: background var(--transition-normal), border-color var(--transition-normal), box-shadow var(--transition-normal);
  }

  .site-header.scrolled {
    background: var(--color-surface);
    box-shadow: var(--shadow-sm);
    border-bottom-color: var(--color-line-strong);
  }

  .header-inner {
    max-width: 1600px;
    margin: 0 auto;
    padding: 0 var(--page-gutter);
    min-height: 72px;
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 32px;
  }

  .header-brand-group {
    display: flex;
    align-items: center;
    gap: 40px;
  }

  .brand {
    display: flex;
    align-items: center;
    gap: 10px;
    text-decoration: none;
    color: var(--color-text-bright);
    transition: transform var(--transition-fast), opacity var(--transition-fast);
  }

  .brand:hover {
    opacity: 0.95;
    transform: translateY(-1px);
  }

  .brand-logo-wrapper {
    flex-shrink: 0;
    display: grid;
    place-items: center;
    filter: none;
    transition: filter var(--transition-fast);
  }

  .brand:hover .brand-logo-wrapper {
    filter: none;
  }

  .brand-text {
    max-width: 200px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-size: 24px;
    font-weight: 750;
    letter-spacing: -0.04em;
    color: var(--color-text);
  }

  .main-nav {
    display: flex;
    align-items: center;
    gap: 6px;
    overflow-x:auto;
    min-width:0;
  }
  .nav-symbol { font-size:16px;font-weight:400; }
  .header-brand-group { min-width:0; }
  .nav-link { flex-shrink:0;white-space:nowrap; }

  .nav-link {
    display: inline-flex;
    align-items: center;
    gap: 8px;
    padding: 12px 14px;
    border-radius: var(--radius-sm);
    color: var(--color-muted);
    font-size: 13.5px;
    font-weight: 500;
    transition: all var(--transition-fast);
  }

  .nav-link:hover {
    color: var(--color-text-bright);
    background: var(--color-raised);
  }

  .nav-link.active,
  .nav-link[aria-current='page'] {
    color: var(--color-text-bright);
    font-weight: 600;
    background: var(--color-accent-soft);
    border: 1px solid var(--color-line);
    box-shadow: var(--shadow-sm);
  }

  .header-actions {
    display: flex;
    align-items: center;
    gap: 16px;
  }

  .quick-search-btn {
    display: grid;
    place-items: center;
    width: 44px;
    height: 44px;
    border-radius: 50%;
    color: var(--color-muted);
    background: var(--color-surface);
    border: 1px solid var(--color-line);
    transition: all var(--transition-fast);
  }

  .quick-search-btn:hover,
  .quick-search-btn.active {
    color: var(--color-text-bright);
    background: var(--color-raised);
    border-color: var(--color-line);
    box-shadow: 0 0 12px var(--color-accent-glow);
  }

  @media (max-width: 820px) {
    .header-inner {
      min-height: 64px;
      gap: 16px;
    }

    .header-brand-group {
      gap: 16px;
    }

    .brand-text {
    max-width: 200px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
      font-size: 20px;
    }

    .nav-link {
      padding: 6px 10px;
      font-size: 12.5px;
      gap: 6px;
    }
  }

  @media (max-width: 640px) {
    .header-inner { gap: 8px; padding-inline: 16px; }
    .header-brand-group { gap: 8px; }
    .brand { gap: 6px; }
    .brand-text {
    max-width: 200px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap; display: none; }
    .main-nav { gap: 2px; }
    .nav-link:not(.library-link) span { display: none; }
    .library-link { border-radius:var(--radius-sm); }
    .library-link span { max-width:160px;overflow:hidden;text-overflow:ellipsis; }

    .nav-link {
      padding: 8px;
      border-radius: 50%;
    }

    .quick-search-btn {
      display: inline-flex;
      width: 44px;
      height: 44px;
      flex-shrink: 0;
    }
  }
</style>
