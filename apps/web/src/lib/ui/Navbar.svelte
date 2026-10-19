<script module lang="ts">
  export const navLinks = [
    { key: 'home', path: '/', label: 'Accueil', symbol: '⌂' },
    { key: 'films', path: 'films/', label: 'Films', symbol: '▤' },
    { key: 'series', path: 'series/', label: 'Séries', symbol: '▥' },
    { key: 'search', path: 'search/', label: 'Recherche', symbol: '⌕' }
  ] as const;
</script>

<script lang="ts">
  import { resolve } from '$app/paths';
  import Icon from './Icon.svelte';
  import ProfileMenu from './ProfileMenu.svelte';
  import { t } from '../i18n';

  let {
    section = 'home',
    name = 'Aether'
  }: {
    section?: 'home' | 'films' | 'series' | 'search' | 'settings' | 'admin';
    name?: string;
  } = $props();

  const base = $derived(resolve('/').replace(/\/$/, ''));
  let scrolled = $state(false);

  function handleScroll() {
    scrolled = window.scrollY > 20;
  }
</script>

<svelte:window onscroll={handleScroll} />

<header class="site-header" class:scrolled>
  <div class="header-inner">
    <div class="header-brand-group">
      <a class="brand" href={resolve('/')} aria-label={`${name}, accueil`}>
        <div class="brand-logo-wrapper">
          <img src={`${base}/images/brand-mark.svg`} width="32" height="32" alt="" />
        </div>
        <span class="brand-text">{name}</span>
      </a>

      <nav class="main-nav" aria-label="Navigation principale">
        {#each navLinks as link (link.key)}
          <a
            href={resolve(link.path)}
            class="nav-link"
            class:active={section === link.key}
            aria-current={section === link.key ? 'page' : undefined}
          >
            {#if link.key === 'search'}
              <Icon name="search" size={16} />
            {:else if link.key === 'home'}
              <Icon name="sparkles" size={16} />
            {:else if link.key === 'films'}
              <Icon name="film" size={16} />
            {:else if link.key === 'series'}
              <Icon name="tv" size={16} />
            {/if}
            <span>{$t(link.key)}</span>
          </a>
        {/each}
      </nav>
    </div>

    <div class="header-actions">
      <a
        href={resolve('search/')}
        class="quick-search-btn"
        class:active={section === 'search'}
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
    filter: drop-shadow(0 0 12px var(--color-accent-glow));
    transition: filter var(--transition-fast);
  }

  .brand:hover .brand-logo-wrapper {
    filter: drop-shadow(0 0 18px var(--color-accent-glow));
  }

  .brand-text {
    font-size: 24px;
    font-weight: 750;
    letter-spacing: -0.04em;
    color: var(--color-text);
  }

  .main-nav {
    display: flex;
    align-items: center;
    gap: 6px;
  }

  .nav-link {
    display: inline-flex;
    align-items: center;
    gap: 8px;
    padding: 8px 14px;
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
    width: 38px;
    height: 38px;
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
    .brand-text { display: none; }
    .main-nav { gap: 2px; }
    .nav-link span {
      display: none;
    }

    .nav-link {
      padding: 8px;
      border-radius: 50%;
    }

    .quick-search-btn {
      display: none;
    }
  }
</style>
