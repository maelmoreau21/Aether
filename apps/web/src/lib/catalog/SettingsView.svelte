<script lang="ts">
  import { resolve } from '$app/paths';
  import { session } from '../auth/session';
  import OidcSettings from '../auth/OidcSettings.svelte';
  import Icon from '../ui/Icon.svelte';
  import { t, locale, setLocale } from '../i18n';
  import { themeMode, setTheme, type ThemeMode } from '../theme';

  let {
    prefAudio = $bindable('fr'),
    prefSubtitles = $bindable('forced'),
    onSavePreference
  }: {
    prefAudio: string;
    prefSubtitles: string;
    onSavePreference: (key: string, value: string) => void;
  } = $props();

  const displayName = $derived($session?.user.displayName || $session?.user.login || 'Utilisateur');
  const hasDistinctName = $derived(
    Boolean($session?.user.displayName && $session?.user.displayName !== $session?.user.login)
  );
</script>

<div class="settings-view">
  <div class="page-heading">
    <span class="eyebrow">{$t('settings_eyebrow')}</span>
    <h1>{$t('settings_title')}</h1>
    <p class="heading-sub">{$t('settings_subtitle')}</p>
  </div>

  <div class="settings-grid">
    <!-- User Account Info Card -->
    <section class="card profile-card" aria-labelledby="profile-heading">
      <h2 id="profile-heading" class="card-title">{$t('settings_user_account')}</h2>
      <div class="profile-header-row">
        <div class="profile-avatar-large" aria-hidden="true">
          <Icon name="profile" size={32} />
        </div>
        <div>
          <h3>{displayName}</h3>
          {#if hasDistinctName}
            <p class="profile-handle">@{$session?.user.login}</p>
          {/if}
          <span class="role-badge" class:admin-badge={$session?.user.role === 'administrator'}>
            {$session?.user.role === 'administrator' ? $t('administrator') : $t('user')}
          </span>
        </div>
      </div>

      <div class="profile-info-list">
        <div class="info-row">
          <span class="label">{$t('settings_unique_id')}</span>
          <span class="val"><code>{$session?.user.id ?? 'Inconnu'}</code></span>
        </div>
        <div class="info-row">
          <span class="label">{$t('settings_auth_type')}</span>
          <span class="val">
            {$session?.user.sso ?? $session?.user.login.startsWith('sso-') ? $t('sso_account') : $t('auth_local_sub')}
          </span>
        </div>
        <div class="info-row">
          <span class="label">{$t('settings_session_security')}</span>
          <span class="val">{$t('settings_session_note')}</span>
        </div>
      </div>

      {#if $session?.user.role === 'administrator'}
        <div class="admin-shortcut">
          <a class="admin-link" href={resolve('admin/libraries/')}>
            <Icon name="settings" size={16} />
            <span>{$t('admin_dashboard')}</span>
          </a>
        </div>
      {/if}
    </section>

    <!-- OIDC Settings -->
    <section class="card oidc-card" aria-labelledby="sso-settings-heading">
      <h2 id="sso-settings-heading" class="card-title">Liaison de compte SSO (OpenID Connect)</h2>
      <OidcSettings />
    </section>

    <!-- Playback Preferences Card -->
    <section class="card preferences-card" aria-labelledby="pref-heading">
      <h2 id="pref-heading" class="card-title">{$t('settings_playback_prefs')}</h2>
      <p class="muted">
        {$t('settings_playback_note')}
      </p>

      <div class="pref-fields">
        <label>
          <span><Icon name="sun" size={14} /> {$t('theme')}</span>
          <select
            value={$themeMode}
            onchange={(e) => setTheme(e.currentTarget.value as ThemeMode)}
          >
            <option value="dark">{$t('theme_dark')}</option>
            <option value="light">{$t('theme_light')}</option>
            <option value="system">{$t('theme_system')}</option>
          </select>
        </label>

        <label>
          <span><Icon name="globe" size={14} /> {$t('language')}</span>
          <select
            value={$locale}
            onchange={(e) => setLocale(e.currentTarget.value as 'fr' | 'en')}
          >
            <option value="fr">{$t('language_fr')}</option>
            <option value="en">{$t('language_en')}</option>
          </select>
        </label>

        <label>
          <span>{$t('settings_pref_audio')}</span>
          <select
            bind:value={prefAudio}
            onchange={() => onSavePreference('media_pref_audio', prefAudio)}
          >
            <option value="fr">Français (par défaut)</option>
            <option value="original">Version originale (si disponible)</option>
            <option value="en">English</option>
          </select>
        </label>

        <label>
          <span>{$t('settings_pref_subtitles')}</span>
          <select
            bind:value={prefSubtitles}
            onchange={() => onSavePreference('media_pref_subtitles', prefSubtitles)}
          >
            <option value="forced">{$t('settings_sub_forced')}</option>
            <option value="always">{$t('settings_sub_all')}</option>
            <option value="off">{$t('settings_sub_none')}</option>
          </select>
        </label>
      </div>

      <p class="pref-note">
        ✓ {$t('settings_saved_alert')}
      </p>
    </section>
  </div>
</div>

<style>
  .settings-view {
    display: flex;
    flex-direction: column;
    gap: 28px;
    padding-top: 12px;
  }

  .page-heading {
    display: flex;
    flex-direction: column;
    gap: 8px;
  }

  h1 {
    font-family: var(--font-display);
    font-size: clamp(30px, 3.8vw, 44px);
    font-weight: 750;
    line-height: 1.15;
    letter-spacing: -0.04em;
    color: var(--color-text-bright);
    margin: 0;
  }

  .heading-sub {
    color: var(--color-muted);
    font-size: 14.5px;
    margin: 0;
  }

  .settings-grid {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(min(100%, 360px), 1fr));
    gap: 24px;
  }

  .card {
    min-width: 0;
    overflow-wrap: anywhere;
    padding: clamp(16px, 3vw, 28px);
    border-radius: var(--radius-lg);
    background: var(--color-surface);
    border: 1px solid var(--color-line);
    box-shadow: var(--shadow-sm);
    display: flex;
    flex-direction: column;
    gap: 20px;
  }

  .card-title {
    font-size: 18px;
    font-weight: 700;
    letter-spacing: -0.02em;
    color: var(--color-text-bright);
    margin: 0;
    padding-bottom: 12px;
    border-bottom: 1px solid var(--color-line);
  }

  .profile-header-row {
    display: flex;
    align-items: center;
    gap: 16px;
  }

  .profile-avatar-large {
    flex-shrink: 0;
    display: grid;
    place-items: center;
    width: 60px;
    height: 60px;
    border-radius: 50%;
    background: var(--color-accent-soft);
    color: var(--color-accent);
    border: 1px solid var(--color-line);
  }

  h3 {
    font-size: 18px;
    font-weight: 650;
    margin: 0 0 4px;
    color: var(--color-text-bright);
  }

  .role-badge {
    display: inline-block;
    font-size: 11px;
    font-weight: 700;
    padding: 3px 8px;
    border-radius: var(--radius-xs);
    background: rgba(255, 255, 255, 0.08);
    color: var(--color-muted);
  }

  .admin-badge {
    background: var(--color-accent-soft);
    color: var(--color-accent);
    border: 1px solid var(--color-line);
  }

  .profile-info-list {
    display: flex;
    flex-direction: column;
    gap: 14px;
  }

  .info-row {
    display: flex;
    flex-direction: column;
    gap: 4px;
  }

  .label {
    font-size: 11.5px;
    font-weight: 600;
    color: var(--color-muted);
  }

  .val {
    font-size: 13.5px;
    color: var(--color-text);
  }

  .val code {
    font-family: monospace;
    font-size: 12px;
    padding: 2px 6px;
    border-radius: var(--radius-xs);
    background: var(--color-raised);
    border: 1px solid var(--color-line);
    color: var(--color-accent-hover);
  }

  .admin-shortcut {
    padding-top: 8px;
  }

  .admin-link {
    display: inline-flex;
    align-items: center;
    gap: 10px;
    padding: 10px 18px;
    border-radius: var(--radius-sm);
    background: var(--brand-gradient);
    color: var(--color-on-accent);
    font-size: 13px;
    font-weight: 700;
    text-decoration: none;
    transition: all var(--transition-fast);
  }

  .admin-link:hover {
    background: var(--brand-gradient);
    box-shadow: var(--shadow-sm);
    transform: translateY(-1px);
  }

  .muted {
    font-size: 13px;
    line-height: 1.55;
    color: var(--color-muted);
    margin: 0;
  }

  .pref-fields {
    display: flex;
    flex-direction: column;
    gap: 16px;
  }

  .pref-fields label {
    display: flex;
    flex-direction: column;
    gap: 6px;
    font-size: 12.5px;
    font-weight: 600;
    color: var(--color-text);
  }

  .pref-note {
    font-size: 12px;
    color: var(--color-accent);
    margin: 0;
  }
</style>
