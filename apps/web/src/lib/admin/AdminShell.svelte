<script lang="ts">
  import { onMount, tick, type Snippet } from 'svelte';
  import { asset, resolve } from '$app/paths';
  import { serverPresentation } from '../config/presentation';
  import { DEFAULT_NAME } from '../config/brand';
  import ProfileMenu from '../ui/ProfileMenu.svelte';
  import Icon from '../ui/Icon.svelte';
  import { t, locale } from '../i18n';
  import { adminNavigation, adminDestinationLabel } from './navigation';

  let { title, currentHash, administrator, onnavigate, children }: {
    title: string; currentHash: string; administrator: boolean; onnavigate: (hash: string) => void; children: Snippet;
  } = $props();
  let wide = $state(false);
  let menuOpen = $state(false);
  let navigationDialog = $state<HTMLDialogElement>();
  let menuButton = $state<HTMLButtonElement>();
  let content: HTMLElement;
  let sidebar = $state<HTMLElement>();

  function closeMenu(restore = true) {
    navigationDialog?.close(); menuOpen = false;
    if (restore && !wide) menuButton?.focus();
  }
  function openMenu() {
    if (!navigationDialog || wide) return;
    navigationDialog.showModal(); menuOpen = true;
    navigationDialog.querySelector<HTMLAnchorElement>('[aria-current="page"]')?.focus();
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

{#snippet navigation()}
  <nav class="sidebar-sections" aria-label="Sections d’administration">
    {#each adminNavigation as group (group.label)}
      <div class="sidebar-group"><p class="group-label">{$locale === 'en' ? group.englishLabel : group.label}</p>
        {#each group.links as link (link.hash)}
          <a class="sidebar-nav-item" class:active={currentHash === link.hash} href={resolve(`admin/libraries/#${link.hash.slice(1)}`)} aria-current={currentHash === link.hash ? 'page' : undefined} onclick={event => choose(event, link.hash)}>
            <Icon name={link.icon} size={18} /><span>{adminDestinationLabel(link, $t, $locale === 'en')}</span>
          </a>
        {/each}
      </div>
    {/each}
  </nav>
{/snippet}

<a class="skip-admin" href="#admin-main-content" onclick={event => { event.preventDefault(); content?.focus(); }}>Aller au contenu administratif</a>
<header class="admin-header">
  <div class="header-left">
    <a class="admin-brand" href={resolve('/')} aria-label={`${$serverPresentation.serverName || DEFAULT_NAME}, accueil`}><img src={asset('images/brand-mark.svg')} width="28" height="28" alt="" /><span>{$serverPresentation.serverName || DEFAULT_NAME}</span></a>
    <span class="header-context">Administration</span>
    {#if administrator}<button class="mobile-menu-toggle" bind:this={menuButton} type="button" aria-haspopup="dialog" aria-expanded={menuOpen} aria-controls="admin-navigation" onclick={openMenu}><Icon name="menu" size={18} /><span>Menu</span></button>{/if}
  </div>
  <div class="header-right"><a class="back-link" href={resolve('/')} aria-label="Retour à la médiathèque"><Icon name="back" size={18} /><span>Médiathèque</span></a><ProfileMenu /></div>
</header>

<div class="admin-dashboard-container" class:with-sidebar={administrator && wide}>
  {#if administrator}
    {#if wide}
      <aside class="admin-sidebar desktop-sidebar" aria-label="Navigation d’administration" bind:this={sidebar}>
        <p class="sidebar-title">Administration</p>{@render navigation()}
      </aside>
    {:else}
      <dialog class="admin-nav-dialog admin-sidebar" id="admin-navigation" bind:this={navigationDialog} aria-label="Menu d’administration" onclose={() => { menuOpen = false; if (!wide && navigationDialog?.contains(document.activeElement)) menuButton?.focus(); }} onpointerdown={event => { if (event.target === event.currentTarget) { const bounds = event.currentTarget.getBoundingClientRect(); if (event.clientX > bounds.right || event.clientX < bounds.left) closeMenu(); } }}>
        <div class="drawer-heading"><strong>Administration</strong><button aria-label="Fermer le menu" onclick={() => closeMenu()}><Icon name="close" size={20} /></button></div>
        {@render navigation()}
      </dialog>
    {/if}
  {/if}
  <main id="admin-main-content" class="admin-content-area" tabindex="-1" bind:this={content}>
    <h1 class="sr-only">{title} — Administration</h1>
    {@render children()}
  </main>
</div>

<style>
  .admin-header { position:sticky; top:0; z-index:100; display:flex; align-items:center; justify-content:space-between; gap:16px; min-height:72px; padding:10px var(--page-gutter); border-bottom:1px solid var(--color-line); background:var(--color-glass); backdrop-filter:blur(16px) saturate(160%); -webkit-backdrop-filter:blur(16px) saturate(160%); }
  .header-left,.header-right { display:flex; align-items:center; gap:20px; min-width:0; }
  .header-right { flex:none; }
  .admin-brand { display:flex; align-items:center; gap:8px; min-width:0; font-size:18px; font-weight:800; letter-spacing:-0.02em; }
  .admin-brand img { flex:none; }
  .admin-brand span { max-width:180px; overflow:hidden; text-overflow:ellipsis; white-space:nowrap; }
  .header-context { border-left:1px solid var(--color-line-strong); padding-left:20px; font-size:13px; font-weight:500; color:var(--color-muted); }
  .back-link { display:flex; align-items:center; gap:8px; min-height:44px; font-size:13px; font-weight:500; color:var(--color-muted); transition:color var(--transition-fast); }
  .back-link:hover { color:var(--color-text); }
  .mobile-menu-toggle { display:inline-flex; align-items:center; gap:8px; padding:10px 14px; min-height:44px; border:1px solid var(--color-line-strong); border-radius:var(--radius-sm); background:var(--color-raised); color:var(--color-text); font-size:14px; }
  .admin-dashboard-container { width:100%; max-width:none; display:grid; grid-template-columns:minmax(0,1fr); gap:32px; padding:28px var(--page-gutter) 64px; margin:0; }
  .with-sidebar { grid-template-columns:232px minmax(0,1fr); }
  .admin-content-area { display:flex; flex-direction:column; gap:20px; min-width:0; }
  .admin-content-area:focus { outline:none; }
  .desktop-sidebar { position:sticky; top:96px; align-self:start; max-height:calc(100dvh - 112px); overflow-y:auto; padding:0 12px 16px 0; border-right:1px solid var(--color-line); scrollbar-width:thin; }
  .sidebar-title { margin:0 10px 20px; font-size:15px; font-weight:700; }
  .sidebar-sections { display:grid; gap:24px; }
  .sidebar-group { display:grid; gap:3px; }
  .group-label { margin:0; padding:0 10px 6px; color:var(--color-muted); font-size:11px; font-weight:700; text-transform:uppercase; letter-spacing:.06em; }
  .sidebar-nav-item { display:flex; align-items:center; gap:10px; min-height:44px; padding:10px; border-radius:var(--radius-sm); color:var(--color-muted); font-size:13px; line-height:1.5; transition:background var(--transition-fast), color var(--transition-fast); }
  .sidebar-nav-item :global(svg) { flex:none; }
  .sidebar-nav-item span { min-width:0; overflow-wrap:anywhere; }
  .sidebar-nav-item:hover { color:var(--color-text); background:var(--color-raised); }
  .sidebar-nav-item.active { background:var(--color-accent-soft); color:var(--color-accent); font-weight:650; }
  .admin-nav-dialog { position:fixed; inset:0 auto 0 0; width:min(320px,90vw); height:100dvh; max-width:none; max-height:100dvh; margin:0; padding:0; border:0; border-right:1px solid var(--color-line-strong); background:var(--color-glass-raised); backdrop-filter:blur(24px); -webkit-backdrop-filter:blur(24px); color:var(--color-text); }
  .admin-nav-dialog[open] { display:flex; flex-direction:column; }
  .admin-nav-dialog::backdrop { background:rgb(0 0 0 / .65); backdrop-filter:blur(4px); }
  .drawer-heading { display:flex; align-items:center; justify-content:space-between; gap:16px; padding:16px; border-bottom:1px solid var(--color-line); flex:none; }
  .drawer-heading button { display:grid; place-items:center; width:44px; height:44px; border:1px solid var(--color-line); border-radius:var(--radius-full); background:transparent; color:inherit; }
  .admin-nav-dialog .sidebar-sections { padding:24px 16px calc(24px + env(safe-area-inset-bottom,0px)); overflow-y:auto; min-height:0; overscroll-behavior:contain; }
  .skip-admin { position:fixed; top:-100px; left:16px; z-index:120; padding:12px 16px; background:var(--color-text); color:var(--color-canvas); border-radius:var(--radius-sm); }
  .skip-admin:focus { top:12px; }
  .admin-content-area :global(:where(section,article,form,fieldset,header,label,details,.card)) { min-width:0; }
  .admin-content-area :global(:where(h2,h3,p,dd,code,label,button,summary)) { overflow-wrap:anywhere; }
  .admin-content-area :global(:where(input,select,button)) { max-width:100%; }
  @media(min-width:1100px) { .mobile-menu-toggle { display:none; } }
  @media(max-width:1099px) { .header-context { display:none; } .admin-header { min-height:64px; } }
  @media(max-width:640px) { .admin-header { gap:8px; padding:8px 16px; } .header-left,.header-right { gap:8px; } .admin-brand span,.back-link span { display:none; } .back-link { justify-content:center; width:44px; } .admin-dashboard-container { padding:24px 16px 48px; } }
</style>
