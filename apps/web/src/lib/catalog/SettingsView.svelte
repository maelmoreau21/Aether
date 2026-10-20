<script lang="ts">
  import { onMount } from 'svelte';
  import { preferences, savePreferences } from '../preferences';
  import { resolve } from '$app/paths';
  import { session } from '../auth/session';
  import ProfileAvatar from '../ui/ProfileAvatar.svelte';
  import AvatarEditor from './AvatarEditor.svelte';
  import OidcSettings from '../auth/OidcSettings.svelte';
  import AccountSecurity from '../auth/AccountSecurity.svelte';
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

  let current = $state('account');
  let audioOffset = $state(preferences().offsets?.audio ?? 0);
  let subtitleOffset = $state(preferences().offsets?.subtitle ?? 0);
  const pages = [
    { id: 'account', label: 'Compte', icon: 'profile' },
    { id: 'appearance', label: 'Apparence', icon: 'sun' },
    { id: 'playback', label: 'Lecture', icon: 'play' },
    { id: 'sso', label: 'Connexion SSO', icon: 'shield' }
  ] as const;
  onMount(() => {
    const sync = () => { const id = window.location.hash.slice(1); current = pages.some(page => page.id === id) ? id : 'account'; };
    sync(); window.addEventListener('hashchange', sync);
    return () => window.removeEventListener('hashchange', sync);
  });
  function saveOffsets(event: SubmitEvent) {
    event.preventDefault();
    if ([audioOffset, subtitleOffset].every(value => Number.isFinite(value) && Math.abs(value) <= 30)) {
      savePreferences({ offsets: { audio: audioOffset, subtitle: subtitleOffset } });
    }
  }
</script>

<div class="settings-view">
  <h1>Paramètres</h1>
  <div class="settings-layout">
    <nav class="settings-nav" aria-label="Paramètres personnels">
      {#each pages as page (page.id)}
        <a href={resolve(`settings/#${page.id}`)} onclick={() => current = page.id} class:active={current === page.id} aria-current={current === page.id ? 'page' : undefined}><Icon name={page.icon} size={20} />{page.label}</a>
      {/each}
      {#if $session?.user.role === 'administrator'}<a class="server-link" href={resolve('admin/libraries/#overview')}><Icon name="server" size={20} />Administration du serveur</a>{/if}
    </nav>
    <div class="settings-content">
      <div hidden={current !== 'account'}>
    <section class="card profile-card" aria-labelledby="profile-heading">
      <h2 id="profile-heading" class="card-title">{$t('settings_user_account')}</h2>
      <div class="profile-header-row">
        <ProfileAvatar size={64} />
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

      <AvatarEditor />
      <div class="profile-info-list">
        <div class="info-row">
          <span class="label">{$t('settings_unique_id')}</span>
          <span class="val"><code>{$session?.user.id ?? 'Inconnu'}</code></span>
        </div>
        <div class="info-row">
          <span class="label">{$t('settings_auth_type')}</span>
          <span class="val">
            {$session?.user.sso === true ? $t('sso_account') : $session?.user.sso === false ? $t('auth_local_sub') : 'Non communiqué'}
          </span>
        </div>
      </div>

      <AccountSecurity />
    </section>
      </div>
      <div hidden={current !== 'appearance'}><section class="card" aria-labelledby="appearance-heading"><h2 id="appearance-heading">Apparence</h2><p class="muted">Personnalisez le thème et la langue de l’interface.</p>
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

      </div>
      </section></div>
      <div hidden={current !== 'playback'}><section class="card" aria-labelledby="playback-heading"><h2 id="playback-heading">Lecture</h2><p class="muted">Ces préférences s’appliquent à votre compte.</p>
      <div class="pref-fields">
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


        <form class="offsets" onsubmit={saveOffsets}>
          <h3>Synchronisation</h3><p class="muted">Décalages par défaut, de −30 à +30 secondes. Un décalage positif retarde la piste. Le décalage audio concerne les pistes externes.</p>
          <label>Décalage audio (secondes)<input type="number" min="-30" max="30" step="0.1" required bind:value={audioOffset} /></label>
          <label>Décalage des sous-titres (secondes)<input type="number" min="-30" max="30" step="0.1" required bind:value={subtitleOffset} /></label>
          <button type="submit">Enregistrer les décalages</button>
        </form>
      </section></div>
      <div hidden={current !== 'sso'}>
    <section class="card oidc-card" aria-labelledby="sso-settings-heading">
      <h2 id="sso-settings-heading" class="card-title">Connexion SSO</h2>
      <OidcSettings embedded />
    </section>

      </div>
      <p class="save-note">Les modifications sont enregistrées dans votre compte. Un échec d’enregistrement est signalé en haut de la page.</p>
    </div>
  </div>
</div>
<style>
  .settings-view { padding-top: 24px; }
  h1 { font-size: 28px; font-weight: 400; margin: 0 0 32px; }
  .settings-layout { display: grid; grid-template-columns: 220px minmax(0, 1fr); gap: 40px; align-items: start; }
  .settings-nav { display: grid; gap: 4px; }
  .settings-nav a { display: flex; align-items: center; gap: 14px; padding: 14px; color: var(--text-secondary); font-size: 14px; min-height: 48px; }
  .settings-nav a:hover, .settings-nav a.active { background: var(--bg-elevated); color: var(--text-primary); }
  .settings-nav a.active { border-left: 3px solid var(--accent-primary); }
  .server-link { margin-top: 20px; border-top: 1px solid var(--border-subtle); }
  .settings-content { min-width: 0; max-width: 860px; }
  .card { min-width: 0; overflow-wrap: anywhere; padding: 0; }
  h2 { font-size: 24px; font-weight: 400; margin: 0 0 24px; }
  h3 { margin: 0; font-size: 18px; font-weight: 500; }
  .profile-header-row { display: flex; align-items: center; gap: 20px; margin-bottom: 28px; }
  .profile-handle { margin: 6px 0; color: var(--text-secondary); }
  .role-badge { display: inline-block; margin-top: 8px; padding: 4px 8px; font-size: 12px; background: var(--bg-elevated); }
  .admin-badge { background: var(--color-accent-soft); color: var(--accent-primary); }
  .profile-info-list { display: grid; gap: 0; }
  .info-row { display: grid; grid-template-columns: 180px minmax(0, 1fr); gap: 16px; padding: 18px 0; border-bottom: 1px solid var(--border-subtle); font-size: 14px; }
  .label { color: var(--text-secondary); }
  .val code { overflow-wrap: anywhere; }
  .muted, .save-note { color: var(--text-secondary); font-size: 14px; line-height: 1.6; }
  .save-note { margin-top: 32px; padding-top: 20px; border-top: 1px solid var(--border-subtle); font-size: 12px; }
  .pref-fields, .offsets { display: grid; gap: 24px; margin-top: 24px; }
  label { display: grid; gap: 10px; font-size: 14px; }
  label > span { display: flex; align-items: center; gap: 8px; }
  select, input { min-width: 0; width: 100%; max-width: 520px; min-height: 44px; border: 1px solid var(--border-subtle); border-radius: 4px; background-color: var(--bg-surface); color: var(--text-primary); }
  input { padding: 12px; }
  button { justify-self: start; min-height: 44px; padding: 12px 20px; border: 0; border-radius: 4px; background: var(--brand-gradient); color: var(--color-on-accent); }
  .offsets { padding-top: 28px; border-top: 1px solid var(--border-subtle); }
  .offsets p { margin: 0; }
  @media (max-width: 760px) {
    .settings-layout { grid-template-columns: minmax(0, 1fr); gap: 28px; }
    .settings-nav { grid-template-columns: repeat(2, minmax(0, 1fr)); gap: 4px; }
    .settings-nav a { font-size: 13px; padding: 12px; gap: 10px; }
    .server-link { grid-column: 1 / -1; margin-top: 4px; }
    .info-row { grid-template-columns: minmax(0, 1fr); gap: 8px; }
  }
</style>
