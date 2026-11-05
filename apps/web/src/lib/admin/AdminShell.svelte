<script lang="ts">
  import { onMount, tick, type Snippet } from 'svelte';
  import { resolve } from '$app/paths';
  import { serverPresentation } from '../config/presentation';
  import { DEFAULT_NAME } from '../config/brand';
  import ProfileMenu from '../ui/ProfileMenu.svelte';
  import Icon from '../ui/Icon.svelte';
  import { t, locale } from '../i18n';
  import { adminNavigation, adminDestinationLabel, adminEntryFor, normalizeAdminSearch, searchAdmin, type AdminSearchResult } from './navigation';
  import { extensionPins, toggleExtensionPin } from './extension-pins';
  import { features, loadFeatures } from '../applications/api';

  let { title, currentHash, administrator, onnavigate, children }: {
    title: string; currentHash: string; administrator: boolean; onnavigate: (hash: string) => void; children: Snippet;
  } = $props();
  let wide = $state(false);
  let menuOpen = $state(false);
  let navigationDialog = $state<HTMLDialogElement>();
  let menuButton = $state<HTMLButtonElement>();
  let content: HTMLElement;
  let sidebar = $state<HTMLElement>();
  const currentEntry = $derived(adminEntryFor(currentHash));
  let query = $state('');
  const searching = $derived(query.trim().length > 0);
  const results = $derived(searchAdmin(query).filter(result => !result.entry.extensionId || (result.entry.extensionId === 'media-requests' ? $features.requests : $features.reports)));

  /** Section shown after the matched setting; skipped when it would only repeat the match. */
  function resultContext(result: AdminSearchResult): string {
    const english = $locale === 'en';
    const match = normalizeAdminSearch(result.match);
    const section = adminDestinationLabel(result.entry, $t, english);
    if (match !== normalizeAdminSearch(result.entry.label) && match !== normalizeAdminSearch(section)) return section;
    return result.destination !== result.entry && match !== normalizeAdminSearch(result.destination.label) ? adminDestinationLabel(result.destination, $t, english) : '';
  }
  function searchKeydown(event: KeyboardEvent) {
    if (event.key !== 'Escape' || !query) return;
    event.preventDefault(); event.stopPropagation();
    query = '';
  }
  function chooseResult(event: MouseEvent, hash: string) {
    void choose(event, hash);
    if (event.defaultPrevented) query = '';
  }

  function closeMenu(restore = true) {
    navigationDialog?.close(); menuOpen = false;
    if (restore && !wide) menuButton?.focus();
  }
  function openMenu() {
    if (!navigationDialog || wide) return;
    navigationDialog.showModal(); menuOpen = true;
    navigationDialog.querySelector<HTMLElement>(searching ? 'input[type="search"]' : '[aria-current="page"]')?.focus();
  }
  async function choose(event: MouseEvent, hash: string) {
    if (event.defaultPrevented || event.button !== 0 || event.metaKey || event.ctrlKey || event.shiftKey || event.altKey) return;
    event.preventDefault();
    closeMenu(false);
    onnavigate(hash);
    if (window.location.hash !== hash) window.location.hash = hash;
    await tick();
    content?.focus({ preventScroll: true });
    window.scrollTo({ top: 0, behavior: 'instant' });
  }
  onMount(() => {
    if (administrator) void loadFeatures().catch(() => {});
    const media = matchMedia('(min-width: 1100px)');
    const resize = () => {
      const wasNavigating = menuOpen || Boolean(sidebar?.contains(document.activeElement));
      navigationDialog?.close(); menuOpen = false; wide = media.matches;
      if (wasNavigating) void tick().then(() => {
        if (wide) sidebar?.querySelector<HTMLAnchorElement>('[aria-current="page"]')?.focus();
        else menuButton?.focus();
      });
    };
    resize(); media.addEventListener('change', resize);
    return () => media.removeEventListener('change', resize);
  });
  $effect(() => {
    if (!menuOpen) return;
    const previous = document.body.style.overflow;
    document.body.style.overflow = 'hidden';
    return () => { document.body.style.overflow = previous; };
  });
</script>

{#snippet navigation(place: 'side' | 'menu')}
  <div class="settings-search" role="search">
    <label class="h-sr" for={`admin-settings-search-${place}`}>Rechercher un réglage</label>
    <div class="search-field">
      <Icon name="search" size={18} />
      <input id={`admin-settings-search-${place}`} type="search" placeholder="Rechercher un réglage…" autocomplete="off" spellcheck="false" bind:value={query} onkeydown={searchKeydown} aria-controls={searching ? `admin-settings-results-${place}` : undefined} />
    </div>
    <p class="h-sr" aria-live="polite">{searching ? (results.length ? `${results.length} ${results.length > 1 ? 'réglages trouvés' : 'réglage trouvé'}` : 'Aucun réglage ne correspond.') : ''}</p>
  </div>
  {#if searching}
    <nav class="sidebar-sections search-results" id={`admin-settings-results-${place}`} aria-label="Résultats de la recherche de réglages">
      {#if results.length}
        <ul>
          {#each results as result (`${result.hash}:${result.match}`)}
            {@const context = resultContext(result)}
            <li>
              <a class="sidebar-nav-item search-result" href={resolve(`admin/libraries/#${result.hash.slice(1)}`)} onclick={event => chooseResult(event, result.hash)}>
                <Icon name={result.destination.icon} size={18} /><span><strong>{result.match}</strong>{#if context}<span class="result-context"> — {context}</span>{/if}</span>
              </a>
            </li>
          {/each}
        </ul>
      {:else}
        <p class="search-empty">Aucun réglage ne correspond.</p>
      {/if}
    </nav>
  {:else}
  <nav class="sidebar-sections" aria-label="Sections d’administration">
    {#each adminNavigation as group (group.label)}
      <div class="sidebar-group"><p class="group-label">{$locale === 'en' ? group.englishLabel : group.label}</p>
        {#each group.links as link (link.hash)}
          {#if !link.extensionId || ($extensionPins.includes(link.extensionId) && (link.extensionId === 'media-requests' ? $features.requests : $features.reports))}
          {@const active = currentEntry?.hash === link.hash}
          <a class="sidebar-nav-item" class:active href={resolve(`admin/libraries/#${link.hash.slice(1)}`)} aria-current={active ? 'page' : undefined} onclick={event => choose(event, link.hash)}>
            <Icon name={link.icon} size={18} /><span>{adminDestinationLabel(link, $t, $locale === 'en')}</span>
          </a>
          {/if}
        {/each}
      </div>
    {/each}
  </nav>
  {/if}
{/snippet}

<a class="skip-admin" href="#admin-main-content" onclick={event => { event.preventDefault(); content?.focus(); }}>Aller au contenu administratif</a>
<header class="admin-header">
  <div class="header-left">
    <a class="admin-brand" href={resolve('/')} aria-label={`${$serverPresentation.serverName || DEFAULT_NAME}, accueil`}>
      <svg viewBox="0 0 256 256" width="28" height="28" aria-hidden="true">
        <defs><linearGradient id="halo-admin-brand" x1="0" y1="0" x2="1" y2="1"><stop offset="0" class="logo-stop-a" /><stop offset="1" class="logo-stop-b" /></linearGradient></defs>
        <path d="M64 192 V48 L216 128 L116 180" fill="none" stroke="url(#halo-admin-brand)" stroke-width="30" stroke-linecap="round" stroke-linejoin="round" />
      </svg>
      <span>{$serverPresentation.serverName || DEFAULT_NAME}</span>
    </a>
    <span class="header-context">Administration</span>
    {#if administrator}<button class="mobile-menu-toggle" bind:this={menuButton} type="button" aria-haspopup="dialog" aria-expanded={menuOpen} aria-controls="admin-navigation" onclick={openMenu}><Icon name="menu" size={18} /><span>{currentEntry ? adminDestinationLabel(currentEntry, $t, $locale === 'en') : 'Menu'}</span></button>{/if}
  </div>
  <div class="header-right"><a class="back-link" href={resolve('/')} aria-label="Retour à la médiathèque"><Icon name="back" size={18} /><span>Médiathèque</span></a><ProfileMenu /></div>
</header>

<div class="admin-dashboard-container" class:with-sidebar={administrator && wide}>
  {#if administrator}
    {#if wide}
      <aside class="admin-sidebar desktop-sidebar" aria-label="Navigation d’administration" bind:this={sidebar}>
        {@render navigation('side')}
      </aside>
    {:else}
      <dialog class="admin-nav-dialog admin-sidebar" id="admin-navigation" bind:this={navigationDialog} aria-label="Menu d’administration" onclose={() => { menuOpen = false; if (!wide && navigationDialog?.contains(document.activeElement)) menuButton?.focus(); }} onpointerdown={event => { if (event.target === event.currentTarget) { const bounds = event.currentTarget.getBoundingClientRect(); if (event.clientX > bounds.right || event.clientX < bounds.left) closeMenu(); } }}>
        <div class="drawer-heading"><strong>Administration</strong><button aria-label="Fermer le menu" onclick={() => closeMenu()}><Icon name="close" size={20} /></button></div>
        {@render navigation('menu')}
      </dialog>
    {/if}
  {/if}
  <main id="admin-main-content" class="admin-content-area" tabindex="-1" bind:this={content}>
    {#if administrator && currentEntry}
      <header class="section-title">
        <div class="section-heading"><h1>{adminDestinationLabel(currentEntry, $t, $locale === 'en')}</h1>
          {#if currentEntry.extensionId && (currentEntry.extensionId === 'media-requests' ? $features.requests : $features.reports)}
            <button class="pin-extension" aria-pressed={$extensionPins.includes(currentEntry.extensionId)} onclick={() => toggleExtensionPin(currentEntry.extensionId!)}><Icon name="star" size={17} />{$extensionPins.includes(currentEntry.extensionId) ? 'Désépingler' : 'Épingler'}</button>
          {/if}
        </div>
        {#if currentEntry.tabs}
          <nav class="section-tabs" aria-label={`Sous-sections : ${adminDestinationLabel(currentEntry, $t, $locale === 'en')}`}>
            {#each currentEntry.tabs as tab (tab.hash)}
              <a href={resolve(`admin/libraries/#${tab.hash.slice(1)}`)} aria-current={currentHash === tab.hash ? 'page' : undefined} onclick={event => choose(event, tab.hash)}>{adminDestinationLabel(tab, $t, $locale === 'en')}</a>
            {/each}
          </nav>
        {/if}
      </header>
    {:else}
      <h1 class="h-sr">{title} — Administration</h1>
    {/if}
    {@render children()}
  </main>
</div>

<style>
  .admin-header { position: sticky; top: 0; z-index: 100; display: flex; align-items: center; justify-content: space-between; gap: 16px; min-height: 72px; padding: 10px var(--gutter); border-bottom: 1px solid var(--line); background: var(--glass); backdrop-filter: blur(18px); -webkit-backdrop-filter: blur(18px); }
  .header-left, .header-right { display: flex; align-items: center; gap: 16px; min-width: 0; }
  .header-right { flex: none; gap: 8px; }
  .admin-brand { display: flex; align-items: center; gap: 10px; min-width: 0; font-size: 20px; font-weight: 800; letter-spacing: -0.03em; color: var(--tx); }
  .admin-brand svg { flex: none; }
  .admin-brand span { max-width: 180px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .header-context { display: inline-flex; align-items: center; height: 24px; padding: 0 9px; border-radius: var(--r-xs); background: var(--pn-2); font-size: 12px; font-weight: 700; color: var(--tx); }
  .back-link { display: flex; align-items: center; gap: 8px; height: 40px; padding: 0 14px; border-radius: var(--r-sm); font-size: 14px; font-weight: 600; color: var(--mu); transition: background-color var(--t-fast) ease, color var(--t-fast) ease; }
  .back-link:hover { color: var(--tx); background: var(--pn); }
  .mobile-menu-toggle { display: inline-flex; align-items: center; gap: 8px; padding: 0 14px; height: 44px; border: 0; border-radius: var(--r-sm); background: var(--pn-2); color: var(--tx); font-size: 14px; font-weight: 700; max-width: 52vw; }
  .mobile-menu-toggle span { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .admin-dashboard-container { width: 100%; max-width: none; box-sizing: border-box; margin: 0; display: grid; grid-template-columns: minmax(0, 1fr); gap: 32px; padding: 24px var(--gutter) 64px; }
  .with-sidebar { grid-template-columns: 260px minmax(0, 1fr); }
  .admin-content-area { display: flex; flex-direction: column; gap: 20px; min-width: 0; width: 100%; max-width: 1080px; }
  .admin-content-area:focus { outline: none; }
  .section-title { display: flex; flex-direction: column; gap: 14px; }
  .section-heading { display: flex; align-items: center; justify-content: space-between; flex-wrap: wrap; gap: 12px; }
  .pin-extension { display: inline-flex; align-items: center; gap: 8px; min-height: 42px; padding: 0 14px; border: 0; border-radius: var(--r-sm); background: var(--pn); color: var(--tx); font: inherit; cursor: pointer; }
  .section-title h1 { margin: 0; font-size: clamp(30px, 3.6vw, 44px); font-weight: 800; letter-spacing: -0.03em; line-height: 1.02; }
  .section-tabs { display: flex; gap: 24px; border-bottom: 1px solid var(--line); overflow-x: auto; scrollbar-width: none; }
  .section-tabs a { display: flex; align-items: center; height: 46px; border-bottom: 2px solid transparent; margin-bottom: -1px; color: var(--mu); font-size: 14px; font-weight: 700; white-space: nowrap; transition: color var(--t-fast) ease; }
  .section-tabs a:hover { color: var(--tx); }
  .section-tabs a[aria-current='page'] { color: var(--tx); border-bottom-color: var(--ac); }
  .desktop-sidebar { position: sticky; top: 96px; align-self: start; max-height: calc(100dvh - 112px); overflow-y: auto; scrollbar-width: thin; }
  .sidebar-sections { display: grid; gap: 20px; }
  .sidebar-group { display: grid; gap: 2px; }
  .group-label { margin: 0; padding: 0 12px 6px; color: var(--mu); font-size: 12px; font-weight: 700; text-transform: uppercase; letter-spacing: 0.07em; }
  .sidebar-nav-item { display: flex; align-items: center; gap: 12px; min-height: 42px; padding: 0 12px; border-radius: 12px; color: var(--mu); font-size: 14px; font-weight: 600; line-height: 1.3; transition: background-color var(--t-fast) ease, color var(--t-fast) ease; }
  .sidebar-nav-item :global(svg) { flex: none; }
  .sidebar-nav-item span { min-width: 0; overflow-wrap: anywhere; }
  .sidebar-nav-item:hover { color: var(--tx); background: var(--pn); }
  .sidebar-nav-item.active { background: var(--pn-2); color: var(--tx); }
  .settings-search { flex: none; margin-bottom: 20px; }
  .search-field { display: flex; align-items: center; gap: 10px; height: 42px; padding: 0 12px; border-radius: var(--r-sm); background: var(--pn); color: var(--mu); transition: background-color var(--t-fast) ease; }
  .search-field:hover { background: var(--pn-2); }
  .search-field:focus-within { outline: 2px solid var(--ac); outline-offset: 2px; background: var(--pn); }
  .search-field :global(svg) { flex: none; }
  .search-field input { flex: 1; min-width: 0; height: 100%; min-height: 0; box-shadow: none; padding: 0; border: 0; background: transparent; color: var(--tx); font: inherit; font-size: 14px; font-weight: 500; outline: none; }
  .search-field input::placeholder { color: var(--mu); opacity: 1; }
  .search-results ul { display: grid; gap: 2px; margin: 0; padding: 0; list-style: none; }
  .search-result { min-height: 44px; padding-block: 8px; }
  .search-result strong { color: var(--tx); font-weight: 700; }
  .search-result .result-context { color: var(--mu); }
  .search-empty { margin: 0; padding: 8px 12px; color: var(--mu); font-size: 14px; }
  .sidebar-nav-item:focus-visible { outline: 2px solid var(--ac); outline-offset: 2px; }
  .admin-nav-dialog .settings-search { margin: 0; padding: 16px 12px 0; }
  .admin-nav-dialog { position: fixed; inset: 0 auto 0 0; width: min(320px, 90vw); height: 100dvh; max-width: none; max-height: 100dvh; margin: 0; padding: 0; border: 0; border-right: 1px solid var(--line); background: var(--bg-3); color: var(--tx); }
  .admin-nav-dialog[open] { display: flex; flex-direction: column; }
  .admin-nav-dialog::backdrop { background: rgb(6 8 14 / 0.55); backdrop-filter: blur(4px); }
  .drawer-heading { display: flex; align-items: center; justify-content: space-between; gap: 16px; padding: 16px; border-bottom: 1px solid var(--line); flex: none; font-size: 18px; }
  .drawer-heading button { display: grid; place-items: center; width: 44px; height: 44px; border: 0; border-radius: 14px; background: var(--pn); color: inherit; }
  .admin-nav-dialog .sidebar-sections { padding: 20px 12px calc(24px + env(safe-area-inset-bottom, 0px)); overflow-y: auto; min-height: 0; overscroll-behavior: contain; }
  .skip-admin { position: fixed; top: -100px; left: 16px; z-index: 120; padding: 12px 16px; background: var(--btn); color: var(--btn-tx); border-radius: var(--r-sm); font-weight: 700; }
  .skip-admin:focus { top: 12px; }
  .admin-content-area :global(:where(section, article, form, fieldset, header, label, details, .card)) { min-width: 0; }
  .admin-content-area :global(:where(h2, h3, p, dd, code, label, button, summary)) { overflow-wrap: anywhere; }
  .admin-content-area :global(:where(input, select, button)) { max-width: 100%; }
  @media (min-width: 1100px) { .mobile-menu-toggle { display: none; } }
  @media (max-width: 1099px) { .header-context { display: none; } .admin-header { min-height: 64px; } }
  @media (max-width: 640px) { .admin-header { gap: 8px; padding: 8px 16px; } .header-left { gap: 8px; } .admin-brand span, .back-link span { display: none; } .back-link { justify-content: center; width: 44px; padding: 0; } .admin-dashboard-container { padding: 16px 16px 48px; } .search-field input { font-size: 16px; } }

  /* La CSP interdit les attributs style : couleurs du dégradé du logo ici. */
  .logo-stop-a { stop-color: var(--logo-a); }
  .logo-stop-b { stop-color: var(--logo-b); }
</style>
