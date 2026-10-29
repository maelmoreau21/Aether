<script lang="ts">
  import { resolve } from '$app/paths';
  import Icon from './Icon.svelte';
  import ProfileAvatar from './ProfileAvatar.svelte';
  import { goto } from '$app/navigation';
  import { session, logout } from '../auth/session';
  import { t, locale, setLocale } from '../i18n';
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
          <button
            type="button"
            class="lang-pill"
            class:active={$locale === 'fr'}
            onclick={() => setLocale('fr')}
          >
            FR
          </button>
          <button
            type="button"
            class="lang-pill"
            class:active={$locale === 'en'}
            onclick={() => setLocale('en')}
          >
            EN
          </button>
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
    border: 1px solid transparent;
    border-radius: 24px;
    padding: 4px 9px 4px 4px;
    background: transparent;
    color: var(--color-text);
    cursor: pointer;
  }
  .profile-trigger:hover,
  .profile-trigger[aria-expanded='true'] {
    background: var(--color-surface);
    border-color: var(--color-line);
  }
  .profile-trigger:active {
    background: var(--color-accent-soft);
  }
  .profile-caption {
    font-size: 12px;
    color: var(--color-text);
    white-space: nowrap;
    max-width: 150px;
    overflow: hidden;
    text-overflow: ellipsis;
    font-weight: 500;
  }
  .profile-chevron {
    display: inline-flex;
    color: var(--color-muted);
    transform: rotate(90deg);
    transition: transform 0.18s;
  }
  .profile-chevron.expanded {
    transform: rotate(-90deg);
  }
  .profile-popover {
    position: fixed;
    inset: 76px var(--page-gutter) auto auto;
    margin: 0;
    width: 270px;
    max-width: calc(100vw - 32px);
    max-height: calc(100dvh - 96px);
    overflow-y: auto;
    padding: 10px;
    border: 1px solid var(--color-line);
    border-radius: 14px;
    color: var(--color-text);
    background: var(--color-raised);
    box-shadow: 0 18px 50px rgba(0, 0, 0, 0.6);
  }
  .profile-popover:popover-open {
    animation: reveal 0.14s ease-out;
  }
  .profile-heading {
    display: flex;
    gap: 12px;
    align-items: center;
    padding: 10px 8px 14px;
    margin-bottom: 6px;
    border-bottom: 1px solid var(--color-line);
  }
  .user-details {
    min-width: 0;
    overflow-wrap: anywhere;
  }
  strong {
    font-size: 13px;
    font-weight: 600;
    color: var(--color-text);
    display: block;
    line-height: 1.3;
  }
  .user-handle {
    margin: 2px 0 0;
    color: var(--color-muted);
    font-size: 11px;
    font-family: monospace;
  }
  .user-role-badge {
    margin: 4px 0 0;
    display: inline-block;
    font-size: 10px;
    text-transform: uppercase;
    letter-spacing: 0.05em;
    color: var(--color-accent);
    background: var(--color-accent-soft);
    padding: 2px 6px;
    border-radius: 4px;
  }
  nav {
    display: grid;
    gap: 4px;
  }
  .profile-entry {
    display: flex;
    align-items: center;
    gap: 12px;
    width: 100%;
    min-height: 44px;
    padding: 10px 12px;
    border: 0;
    border-radius:var(--radius-md);
    background: transparent;
    color: var(--color-text);
    font-size: 12px;
    text-align: left;
    text-decoration: none;
    cursor: pointer;
  }
  .profile-entry > span {
    flex: 1;
  }
  .profile-entry:disabled {
    opacity: 0.6;
    cursor: default;
  }
  .profile-entry:hover:not(:disabled),
  .profile-entry:focus-visible {
    background: var(--color-accent-soft);
    color: var(--color-accent-hover);
  }
  .signout-entry {
    color: var(--color-danger);
    border-top: 1px solid var(--color-line);
    margin-top: 4px;
    padding-top: 12px;
  }
  .signout-entry:hover:not(:disabled) {
    background: var(--color-danger-soft);
    color: var(--color-danger);
  }
  .lang-switch-section {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 8px 12px;
    border-top: 1px solid var(--color-line);
    margin-top: 4px;
  }
  .lang-title {
    display: flex;
    align-items: center;
    gap: 6px;
    font-size: 11px;
    color: var(--color-muted);
  }
  .lang-pills {
    display: flex;
    gap: 4px;
    background: var(--color-surface);
    padding: 2px;
    border-radius: 6px;
    border: 1px solid var(--color-line);
  }
  .lang-pill {
    padding: 3px 8px;
    font-size: 10px;
    font-weight: 600;
    border-radius: 4px;
    border: none;
    background: transparent;
    color: var(--color-muted);
    cursor: pointer;
    transition: all 0.15s;
  }
  .lang-pill.active {
    background: var(--color-accent);
    color: var(--color-on-accent);
  }
  .menu-notice {
    padding: 4px 12px;
    line-height: 1.7;
    font-size: 11px;
  }
  .error {
    color: var(--color-danger);
  }
  @keyframes reveal {
    from {
      opacity: 0;
      transform: translateY(-4px);
    }
    to {
      opacity: 1;
      transform: translateY(0);
    }
  }
  @media (max-width: 1100px) {
    .profile-caption {
      display: none;
    }
  }
  @media (max-width: 700px) {
    .profile-popover {
      top: 64px;
      right: 20px;
    }
    .profile-trigger {
      padding-right: 6px;
    }
  }
</style>
