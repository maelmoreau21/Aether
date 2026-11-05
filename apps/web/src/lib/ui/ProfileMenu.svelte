<script lang="ts">
  import { resolve } from '$app/paths';
  import Icon from './Icon.svelte';
  import ProfileAvatar from './ProfileAvatar.svelte';
  import { goto } from '$app/navigation';
  import { session, logout } from '../auth/session';
  import { t, locale, localeOptions, setLocale } from '../i18n';
  import { themeMode, setTheme } from '../theme';

  let panel: HTMLDivElement;
  let expanded = $state(false);
  let busy = $state(false);
  let error = $state('');

  const displayName = $derived($session?.user.displayName || $session?.user.login || 'Compte');
  const hasDistinctName = $derived(
    Boolean($session?.user.displayName && $session?.user.displayName !== $session?.user.login)
  );

  async function signOut() {
    if (!$session || busy) return;
    busy = true;
    error = '';
    try {
      await logout($session);
      panel.hidePopover();
      await goto(resolve('login/'), { replaceState: true });
    } catch (failure) {
      error = failure instanceof Error ? failure.message : 'Déconnexion impossible. Réessayez.';
    } finally {
      busy = false;
    }
  }

  function toggled(event: ToggleEvent) {
    expanded = event.newState === 'open';
    if (expanded) panel.querySelector<HTMLAnchorElement>('a[href]')?.focus();
  }

  function focusLeft(event: FocusEvent) {
    if (event.relatedTarget instanceof Node && !panel.parentElement?.contains(event.relatedTarget)) {
      panel.hidePopover();
    }
  }

  function navigate(event: MouseEvent, destination: 'settings/' | 'admin/libraries/#overview') {
    if (event.defaultPrevented || event.button !== 0 || event.metaKey || event.ctrlKey || event.shiftKey || event.altKey) return;
    event.preventDefault();
    panel.hidePopover();
    void goto(resolve(destination));
  }
</script>

<div class="profile-access">
  <button
    class="profile-trigger"
    popovertarget="profile-menu"
    aria-label="Menu profil"
    aria-expanded={expanded}
    aria-controls="profile-menu"
  >
    <ProfileAvatar />
    <span class="profile-caption">{displayName}</span>
    <span class="profile-chevron" class:expanded><Icon name="chevron" size={12} /></span>
  </button>

  <div
    id="profile-menu"
    class="profile-popover"
    popover="auto"
    bind:this={panel}
    ontoggle={toggled}
    onfocusout={focusLeft}
  >
    <div class="profile-heading">
      <ProfileAvatar />
      <div class="user-details">
        <strong>{displayName}</strong>
        {#if hasDistinctName}
          <p class="user-handle">@{$session?.user.login}</p>
        {/if}
        <p class="user-role-badge">
          {$session?.user.role === 'administrator' ? $t('administrator') : $t('user')}
        </p>
      </div>
    </div>

    <nav aria-label="Menu profil">
      <a class="profile-entry" href={resolve('settings/')} onclick={(event) => navigate(event, 'settings/')}>
        <Icon name="profile" size={17} />
        <span>{$t('profile_settings')}</span>
        <Icon name="chevron" size={13} />
      </a>

      {#if $session?.user.role === 'administrator'}
        <a class="profile-entry" href={resolve('admin/libraries/#overview')} onclick={(event) => navigate(event, 'admin/libraries/#overview')}>
          <Icon name="settings" size={17} />
          <span>{$t('admin_dashboard')}</span>
          <Icon name="chevron" size={13} />
        </a>
      {/if}

      <!-- Theme mode selector toggle -->
      <div class="lang-switch-section">
        <span class="lang-title"><Icon name="sun" size={15} /> {$t('theme')}</span>
        <div class="lang-pills">
          <button
            type="button"
            class="lang-pill"
            class:active={$themeMode === 'light'}
            onclick={() => setTheme('light')}
            title={$t('theme_light')}
          >
            {$locale === 'fr' ? 'Clair' : 'Light'}
          </button>
          <button
            type="button"
            class="lang-pill"
            class:active={$themeMode === 'dark'}
            onclick={() => setTheme('dark')}
            title={$t('theme_dark')}
          >
            {$locale === 'fr' ? 'Sombre' : 'Dark'}
          </button>
        </div>
      </div>

      <!-- Language selector toggle -->
      <div class="lang-switch-section">
        <span class="lang-title"><Icon name="globe" size={15} /> {$t('language')}</span>
        <div class="lang-pills">
          {#each localeOptions as option (option.code)}
            <button type="button" class="lang-pill" class:active={$locale === option.code} onclick={() => setLocale(option.code)}>{option.label}</button>
          {/each}
        </div>
      </div>

      <button class="profile-entry signout-entry" disabled={busy} onclick={signOut}>
        <span>{busy ? $t('loading') : $t('logout')}</span>
      </button>

      {#if error}
        <p class="menu-notice error" role="alert">{error}</p>
      {/if}
    </nav>
  </div>
</div>

<style>
  .profile-access {
    margin-left: auto;
    flex-shrink: 0;
  }
  .profile-trigger {
    display: flex;
    align-items: center;
    gap: 10px;
    min-height: 44px;
    border: 0;
    border-radius: var(--r-pill);
    padding: 5px 12px 5px 5px;
    background: var(--pn);
    color: var(--tx);
    font: inherit;
    cursor: pointer;
    transition: background-color var(--t-fast) ease, transform var(--t-fast) var(--ease);
  }
  .profile-trigger:hover,
  .profile-trigger[aria-expanded='true'] {
    background: var(--pn-2);
  }
  .profile-trigger:active {
    transform: scale(0.98);
  }
  .profile-trigger:focus-visible,
  .profile-entry:focus-visible,
  .lang-pill:focus-visible {
    outline: 2px solid var(--ac);
    outline-offset: 3px;
  }
  .profile-caption {
    font-size: 14px;
    color: var(--tx);
    white-space: nowrap;
    max-width: 150px;
    overflow: hidden;
    text-overflow: ellipsis;
    font-weight: 600;
  }
  .profile-chevron {
    display: inline-flex;
    color: var(--mu);
    transform: rotate(90deg);
    transition: transform var(--t-med) var(--ease);
  }
  .profile-chevron.expanded {
    transform: rotate(-90deg);
  }
  .profile-popover {
    position: fixed;
    inset: 76px var(--gutter) auto auto;
    margin: 0;
    width: 300px;
    max-width: calc(100vw - 32px);
    max-height: calc(100dvh - 96px);
    overflow-y: auto;
    padding: 6px;
    border: 1px solid var(--line);
    border-radius: var(--r-md);
    color: var(--tx);
    background: var(--bg-3);
    box-shadow: var(--shadow);
  }
  .profile-popover:popover-open {
    animation: profile-menu-reveal var(--t-fast) var(--ease);
  }
  .profile-heading {
    display: flex;
    gap: 12px;
    align-items: center;
    padding: 10px 10px 14px;
    margin-bottom: 6px;
    border-bottom: 1px solid var(--line);
  }
  .user-details {
    min-width: 0;
    overflow-wrap: anywhere;
  }
  strong {
    font-size: 15px;
    font-weight: 700;
    color: var(--tx);
    display: block;
    line-height: 1.3;
  }
  .user-handle {
    margin: 2px 0 0;
    color: var(--mu);
    font-size: 13px;
    font-family: var(--font-mono);
  }
  .user-role-badge {
    margin: 6px 0 0;
    display: inline-flex;
    align-items: center;
    height: 22px;
    padding: 0 8px;
    font-size: 12px;
    font-weight: 700;
    color: var(--mu);
    background: var(--pn-2);
    border-radius: var(--r-xs);
  }
  nav {
    display: grid;
    gap: 2px;
  }
  .profile-entry {
    display: flex;
    align-items: center;
    gap: 12px;
    width: 100%;
    min-height: 44px;
    padding: 0 12px;
    border: 0;
    border-radius: 10px;
    background: transparent;
    color: var(--tx);
    font: inherit;
    font-size: 14px;
    font-weight: 600;
    text-align: left;
    text-decoration: none;
    cursor: pointer;
    transition: background-color var(--t-fast) ease;
  }
  .profile-entry > span {
    flex: 1;
  }
  .profile-entry > :global(svg:last-child:not(:first-child)) {
    color: var(--mu);
  }
  .profile-entry:disabled {
    opacity: 0.45;
    cursor: default;
  }
  .profile-entry:hover:not(:disabled) {
    background: var(--pn);
  }
  .signout-entry {
    color: var(--err);
    margin-top: 7px;
    position: relative;
  }
  .signout-entry::before {
    content: '';
    position: absolute;
    inset: -4px 6px auto;
    height: 1px;
    background: var(--line);
  }
  .signout-entry:hover:not(:disabled) {
    background: var(--err-soft);
    color: var(--err);
  }
  .lang-switch-section {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
    min-height: 52px;
    padding: 4px 6px 4px 12px;
    border-top: 1px solid var(--line);
    margin-top: 4px;
  }
  .lang-title {
    display: flex;
    align-items: center;
    gap: 8px;
    font-size: 14px;
    font-weight: 600;
    color: var(--mu);
  }
  .lang-pills {
    display: inline-flex;
    gap: 4px;
    background: var(--pn);
    padding: 4px;
    border-radius: var(--r-pill);
  }
  .lang-pill {
    min-height: 36px;
    min-width: 44px;
    padding: 0 12px;
    font: inherit;
    font-size: 13px;
    font-weight: 700;
    border-radius: var(--r-pill);
    border: 0;
    background: transparent;
    color: var(--mu);
    cursor: pointer;
    transition: background-color var(--t-fast) ease, color var(--t-fast) ease;
  }
  .lang-pill:hover {
    color: var(--tx);
  }
  .lang-pill.active,
  .lang-pill.active:hover {
    background: var(--tx);
    color: var(--bg);
  }
  .menu-notice {
    margin: 4px 0 0;
    padding: 8px 12px;
    line-height: 1.5;
    font-size: 13px;
    font-weight: 600;
    border-radius: 10px;
  }
  .error {
    color: var(--err);
    background: var(--err-soft);
  }
  @keyframes profile-menu-reveal {
    from {
      opacity: 0;
      transform: translateY(-4px);
    }
    to {
      opacity: 1;
      transform: translateY(0);
    }
  }
  @media (prefers-reduced-motion: reduce) {
    .profile-popover:popover-open { animation: none; }
  }
  @media (max-width: 1100px) {
    .profile-caption {
      display: none;
    }
    .profile-trigger {
      padding-right: 10px;
    }
  }
  @media (max-width: 760px) {
    .profile-popover {
      top: 64px;
      right: var(--gutter);
    }
    .profile-trigger {
      padding-right: 8px;
    }
  }
</style>
