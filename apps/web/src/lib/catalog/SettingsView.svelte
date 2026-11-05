<script lang="ts">
  import { onMount } from 'svelte';
  import { preferences, ensurePreferences, savePreferences } from '../preferences';
  import { resolve } from '$app/paths';
  import { goto } from '$app/navigation';
  import { session } from '../auth/session';
  import { features } from '../applications/api';
  import ProfileAvatar from '../ui/ProfileAvatar.svelte';
  import AppearanceCustomization from './AppearanceCustomization.svelte';
  import RecommendationSettings from './RecommendationSettings.svelte';
  import NotificationSettings from './NotificationSettings.svelte';
  import AvatarEditor from './AvatarEditor.svelte';
  import OidcSettings from '../auth/OidcSettings.svelte';
  import AccountSecurity from '../auth/AccountSecurity.svelte';
  import Requests from '../applications/Requests.svelte';
  import Reports from '../applications/Reports.svelte';
  import Statistics from '../statistics/Statistics.svelte';
  import DownloadsPanel from './DownloadsPanel.svelte';
  import Icon from '../ui/Icon.svelte';
  import type { IconName } from '../ui/Icon.svelte';
  import { t, locale, localeOptions, setLocale, type TranslationKey, type SupportedLocale } from '../i18n';
  import { themeMode, setTheme, getTvMode, setTvMode, type ThemeMode, type TvMode } from '../theme';
  import { DEFAULT_SUBTITLE_STYLE, sanitizeSubtitleStyle, subtitleStyleToCss, type SubtitleStyle } from '../player/subtitle-style';
  import { fetchServerConfig, type AppServerConfig } from '../config/brand';
  import { defaultArtworkRotationPolicy, setArtworkRotationPolicy, type ArtworkRotationPolicy } from './artwork-rotation-policy.svelte';
  import { playbackPreferences, trackPreferences, SKIP_SECONDS } from '../player/playback-preferences';
  import { invalidateLatestCatalog } from './page-cache';

  let serverConfig = $state<AppServerConfig | null>(null);

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
  let skipIntroEnabled = $state(preferences().skipIntroEnabled !== false);
  let skipCreditsEnabled = $state(preferences().skipCreditsEnabled !== false);
  let autoSkipIntro = $state(preferences().autoSkipIntro === true);
  let autoSkipCredits = $state(preferences().autoSkipCredits === true);
  let offsetsEdited = false;
  let playbackDefaults = $state(playbackPreferences(preferences()));
  let playbackEdited = false;
  let rememberAudioSelections = $state(trackPreferences(preferences()).rememberAudioSelections);
  let rememberSubtitleSelections = $state(trackPreferences(preferences()).rememberSubtitleSelections);
  let playDefaultAudioTrack = $state(trackPreferences(preferences()).playDefaultAudioTrack);
  let selectionEdited = false;
  let hidePlayedInLatest = $state(preferences().hidePlayedInLatest === true);
  let latestEdited = false;
  function saveTrackDefaults() {
    selectionEdited = true;
    void savePreferences({ rememberAudioSelections, rememberSubtitleSelections, playDefaultAudioTrack });
  }
  async function saveLatestPreference() {
    latestEdited = true;
    if (await savePreferences({ hidePlayedInLatest })) invalidateLatestCatalog();
  }
  function savePlaybackDefaults() {
    playbackEdited = true;
    savePreferences({ ...playbackDefaults });
  }
  let artworkRotation = $state<ArtworkRotationPolicy>({ ...defaultArtworkRotationPolicy, ...preferences().artworkRotation });
  let artworkEdited = false;
  let artworkError = $state('');
  let artworkSaving = $state(false);

  let subtitleStyle = $state<Required<SubtitleStyle>>({
    ...DEFAULT_SUBTITLE_STYLE,
    ...sanitizeSubtitleStyle(preferences().subtitleStyle)
  });
  const previewSubtitleCss = $derived(subtitleStyleToCss(subtitleStyle));

  function updateSubtitleStyle<K extends keyof Required<SubtitleStyle>>(key: K, value: Required<SubtitleStyle>[K]) {
    subtitleStyle = { ...subtitleStyle, [key]: value };
    savePreferences({ subtitleStyle: { ...subtitleStyle } });
  }
  // `feature`: entry listed only while that optional extension is active. The
  // panel itself still opens from its hash so a link never lands on the wrong page.
  const pages: readonly { id: string; labelKey: TranslationKey; icon: IconName; groupKey: TranslationKey; feature?: 'requests' | 'reports' }[] = [
    { id: 'account', labelKey: 'settings_nav_profile', icon: 'profile', groupKey: 'settings_group_me' },
    { id: 'requests', labelKey: 'settings_requests', icon: 'sparkles', groupKey: 'settings_group_me', feature: 'requests' },
    { id: 'reports', labelKey: 'settings_reports', icon: 'warning', groupKey: 'settings_group_me', feature: 'reports' },
    { id: 'statistics', labelKey: 'settings_statistics', icon: 'clock', groupKey: 'settings_group_me' },
    { id: 'downloads', labelKey: 'downloads', icon: 'download', groupKey: 'settings_group_me' },
    { id: 'playback', labelKey: 'settings_nav_playback', icon: 'play', groupKey: 'settings_group_experience' },
    { id: 'display', labelKey: 'settings_nav_display', icon: 'image', groupKey: 'settings_group_experience' },
    { id: 'security', labelKey: 'settings_nav_security', icon: 'shield', groupKey: 'settings_group_security' },
    { id: 'devices', labelKey: 'settings_nav_devices', icon: 'server', groupKey: 'settings_group_security' },
    { id: 'about', labelKey: 'settings_nav_about', icon: 'info', groupKey: 'settings_group_security' }
  ] as const;
  const pageGroups: readonly TranslationKey[] = ['settings_group_me', 'settings_group_experience', 'settings_group_security'];
  let tvMode = $state<TvMode>('auto');
  onMount(() => {
    tvMode = getTvMode();
    let disposed = false;
    void fetchServerConfig().then(cfg => { if (!disposed) serverConfig = cfg; }).catch(() => {});
    void ensurePreferences().then(saved => {
      if (disposed || !saved) return;
      if (!playbackEdited) playbackDefaults = playbackPreferences(saved);
      if (!selectionEdited) {
        const policy = trackPreferences(saved);
        rememberAudioSelections = policy.rememberAudioSelections;
        rememberSubtitleSelections = policy.rememberSubtitleSelections;
        playDefaultAudioTrack = policy.playDefaultAudioTrack;
      }
      if (!latestEdited) hidePlayedInLatest = saved.hidePlayedInLatest === true;
      if (!offsetsEdited) {
        audioOffset = saved.offsets?.audio ?? 0;
        subtitleOffset = saved.offsets?.subtitle ?? 0;
      }
      if (saved.subtitleStyle) {
        subtitleStyle = { ...DEFAULT_SUBTITLE_STYLE, ...sanitizeSubtitleStyle(saved.subtitleStyle) };
      }
      skipIntroEnabled = saved.skipIntroEnabled !== false;
      skipCreditsEnabled = saved.skipCreditsEnabled !== false;
      autoSkipIntro = saved.autoSkipIntro === true;
      autoSkipCredits = saved.autoSkipCredits === true;
      if (!artworkEdited) artworkRotation = { ...defaultArtworkRotationPolicy, ...saved.artworkRotation };
    }).catch(() => {});
    const sync = () => {
      const hash = window.location.hash.slice(1);
      // The embedded requests panel links administrators to the acquisition screen.
      if (hash === 'acquisition-section') { void goto(resolve('admin/libraries/#acquisition-section'), { replaceState: true }); return; }
      const id = hash === 'appearance' ? 'account' : hash === 'sso' ? 'security' : hash; current = pages.some(page => page.id === id) ? id : 'account';
    };
    sync(); window.addEventListener('hashchange', sync);
    return () => { disposed = true; window.removeEventListener('hashchange', sync); };
  });
  function saveOffsets(event: SubmitEvent) {
    event.preventDefault();
    offsetsEdited = true;
    if ([audioOffset, subtitleOffset].every(value => Number.isFinite(value) && Math.abs(value) <= 30)) {
      savePreferences({ offsets: { audio: audioOffset, subtitle: subtitleOffset } });
    }
  }
  async function saveArtworkRotation(event: SubmitEvent) {
    event.preventDefault();
    if (artworkSaving) return;
    artworkEdited = true; artworkError = ''; artworkSaving = true;
    const value = { ...artworkRotation };
    try {
      if (await savePreferences({ artworkRotation: value })) setArtworkRotationPolicy(value);
      else artworkError = $t('save_preferences_failed');
    } catch { artworkError = $t('save_preferences_failed'); }
    finally { artworkSaving = false; }
  }
</script>

<div class="settings-view">
  <h1>{$t('settings_title')}</h1>
  <div class="settings-layout">
    <nav class="settings-nav" aria-label={$t('settings_title')}>
      {#each pageGroups as group (group)}
        <p class="nav-group">{$t(group)}</p>
        {#each pages.filter(page => page.groupKey === group && (!page.feature || $features[page.feature])) as page (page.id)}
          <a href={resolve(`settings/#${page.id}`)} onclick={() => current = page.id} class:active={current === page.id} aria-current={current === page.id ? 'page' : undefined}><Icon name={page.icon} size={18} />{$t(page.labelKey)}</a>
        {/each}
      {/each}
      {#if $session?.user.role === 'administrator'}<a class="server-link" href={resolve('admin/libraries/#overview')}><Icon name="server" size={20} />{$t('settings_server_admin')}</a>{/if}
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


    </section>
<section class="card"><h2>{$t('interface_preferences')}</h2>      <div class="pref-fields">
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
            onchange={(e) => setLocale(e.currentTarget.value as SupportedLocale)}
          >
            {#each localeOptions as option (option.code)}<option value={option.code}>{option.label}</option>{/each}
          </select>
        </label>

        <label>
          <span><Icon name="tv" size={14} /> {$t('tv_mode')}</span>
          <select value={tvMode} onchange={(e) => { tvMode = e.currentTarget.value as TvMode; setTvMode(tvMode); }}>
            <option value="auto">{$t('tv_mode_auto')}</option>
            <option value="on">{$t('tv_mode_on')}</option>
            <option value="off">{$t('tv_mode_off')}</option>
          </select>
        </label>

      </div>
</section>

      <section class="card profile-card" aria-label="Informations du compte">
        <div class="profile-info-list">
          <div class="info-row">
            <span class="label">{$t('settings_unique_id')}</span>
            <span class="val"><code>{$session?.user.id ?? $t('unknown_value')}</code></span>
          </div>
          <div class="info-row">
            <span class="label">{$t('settings_auth_type')}</span>
            <span class="val">
              {$session?.user.sso === true ? $t('sso_account') : $session?.user.sso === false ? $t('auth_local_sub') : $t('not_communicated')}
            </span>
          </div>
        </div>
      </section>
      </div>
            <div hidden={current !== 'display'}>
        <AppearanceCustomization />
        <RecommendationSettings />
        <section class="card" aria-labelledby="home-preferences-heading">
          <h2 id="home-preferences-heading">{$t('home_preferences')}</h2>
          <label class="checkbox-row"><input type="checkbox" bind:checked={hidePlayedInLatest} onchange={() => void saveLatestPreference()} /> {$t('hide_watched_new')}</label>
        </section>
        <form class="card offsets" onsubmit={saveArtworkRotation}>
          <h2>{$t('artwork_rotation')}</h2>
          <p class="muted">{$t('artwork_rotation_note')}</p>
          <label><span>{$t('enable_artwork_rotation')}</span><input type="checkbox" bind:checked={artworkRotation.enabled} onchange={() => artworkEdited = true} /></label>

          <label>{$t('artwork_language')}
            <select bind:value={artworkRotation.languageMode} onchange={() => artworkEdited = true}>
              <option value="interface">{$t('interface_language')}</option>
              <option value="original">{$t('original_work_language')}</option>
            </select>
          </label>
          <p class="muted">{$t('artwork_fallback_note')}</p>
          <button type="submit" disabled={artworkSaving}>{artworkSaving ? $t('loading') : $t('save_display')}</button>
          {#if artworkError}<p role="alert">{artworkError}</p>{/if}
        </form>
      </div>
      <!-- Data panels mount only while shown: nothing is fetched for hidden pages. -->
      <div class="embedded-panel" hidden={current !== 'requests'}>{#if current === 'requests'}<h2>{$t('settings_requests')}</h2><Requests embedded />{/if}</div>
      <div class="embedded-panel" hidden={current !== 'reports'}>{#if current === 'reports'}<h2>{$t('settings_reports')}</h2><Reports embedded />{/if}</div>
      <div class="embedded-panel" hidden={current !== 'statistics'}>{#if current === 'statistics'}<Statistics embedded />{/if}</div>
      <div hidden={current !== 'downloads'}>{#if current === 'downloads'}<section class="card"><DownloadsPanel embedded /></section>{/if}</div>
      <div hidden={current !== 'security'}><section class="card"><h2>{$t('security')}</h2><AccountSecurity mode="security" /><h3>{$t('sso_connection')}</h3><OidcSettings embedded /></section><section class="card"><NotificationSettings /></section></div>
      <div hidden={current !== 'devices'}><section class="card"><h2>{$t('settings_nav_devices')}</h2><AccountSecurity mode="devices" /></section></div>
      <div hidden={current !== 'playback'}><section class="card" aria-labelledby="playback-heading"><h2 id="playback-heading">{$t('playback')}</h2><p class="muted">{$t('playback_account_note')}</p>
      <div class="pref-fields">
        <label>
          <span>{$t('settings_pref_audio')}</span>
          <select
            bind:value={prefAudio}
            onchange={() => onSavePreference('media_pref_audio', prefAudio)}
          >
            <option value="fr">{$t('audio_default')}</option>
            <option value="original">{$t('original_version')}</option>
            <option value="en">{$t('language_en')}</option>
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


        <section class="offsets" aria-label={$t('track_selection')}>
          <h3>{$t('track_selection')}</h3>
          <label class="checkbox-row"><input type="checkbox" bind:checked={playDefaultAudioTrack} onchange={saveTrackDefaults} /> {$t('prefer_default_audio')}</label>
          <p class="muted">{$t('default_track_note')}</p>
          <label class="checkbox-row"><input type="checkbox" bind:checked={rememberAudioSelections} onchange={saveTrackDefaults} /> {$t('remember_audio')}</label>
          <label class="checkbox-row"><input type="checkbox" bind:checked={rememberSubtitleSelections} onchange={saveTrackDefaults} /> {$t('remember_subtitles')}</label>
          <p class="muted">{$t('manual_choices_note')}</p>
        </section>

        <section class="offsets" aria-label={$t('playback_controls')}>
          <h3>{$t('playback_controls')}</h3>
          <label class="checkbox-row"><input type="checkbox" bind:checked={playbackDefaults.autoplayNextEpisode} onchange={savePlaybackDefaults} /> {$t('autoplay_next')}</label>
          <p class="muted">{$t('autoplay_note')}</p>
          <label>{$t('back_by')}<select bind:value={playbackDefaults.skipBackSeconds} onchange={savePlaybackDefaults}>{#each SKIP_SECONDS as seconds (seconds)}<option value={seconds}>{seconds} {$t('seconds')}</option>{/each}</select></label>
          <label>{$t('forward_by')}<select bind:value={playbackDefaults.skipForwardSeconds} onchange={savePlaybackDefaults}>{#each SKIP_SECONDS as seconds (seconds)}<option value={seconds}>{seconds} {$t('seconds')}</option>{/each}</select></label>
          <p class="muted">{$t('player_duration_note')}</p>
        </section>

        <section class="offsets" aria-label={$t('intros_credits')}>
          <h3>{$t('intros_credits')}</h3>
          <label class="checkbox-row"><input type="checkbox" bind:checked={skipIntroEnabled} onchange={() => savePreferences({ skipIntroEnabled })} /> {$t('suggest_skip_intro')}</label>
          <label class="checkbox-row"><input type="checkbox" bind:checked={skipCreditsEnabled} onchange={() => savePreferences({ skipCreditsEnabled })} /> {$t('suggest_skip_credits')}</label>
          <label class="checkbox-row"><input type="checkbox" bind:checked={autoSkipIntro} onchange={() => savePreferences({ autoSkipIntro })} /> {$t('auto_skip_intro')}</label>
          <label class="checkbox-row"><input type="checkbox" bind:checked={autoSkipCredits} onchange={() => savePreferences({ autoSkipCredits })} /> {$t('auto_skip_credits')}</label>
        </section>

        <form class="offsets" onsubmit={saveOffsets}>
          <h3>{$t('synchronization')}</h3><p class="muted">{$t('offset_note')}</p>
          <label>{$t('audio_offset_seconds_label')}<input type="number" min="-30" max="30" step="0.1" required bind:value={audioOffset} oninput={() => { offsetsEdited = true; }} /></label>
          <label>{$t('subtitle_offset_seconds_label')}<input type="number" min="-30" max="30" step="0.1" required bind:value={subtitleOffset} oninput={() => { offsetsEdited = true; }} /></label>
          <button type="submit">{$t('save_offsets')}</button>
        </form>

        <section class="subtitles-appearance" aria-labelledby="subtitles-style-heading">
          <h3 id="subtitles-style-heading">{$t('subtitle_style')}</h3>
          <p class="muted">{$t('subtitle_style_note')}</p>

          <div class="subtitle-preview-box" aria-hidden="true">
            <span
              class="subtitle-preview-text"
              style:color={previewSubtitleCss.color}
              style:font-size={previewSubtitleCss.fontSize}
              style:font-family={previewSubtitleCss.fontFamily}
              style:background-color={previewSubtitleCss.backgroundColor}
              style:text-shadow={previewSubtitleCss.textShadow}
            >
              {$t('subtitle_example')}
            </span>
          </div>

          <div class="pref-fields">
            <label>
              <span>{$t('font_size')}</span>
              <select
                value={subtitleStyle.fontSize}
                onchange={(e) => updateSubtitleStyle('fontSize', e.currentTarget.value as 'small' | 'medium' | 'large')}
              >
                <option value="small">{$t('small')}</option>
                <option value="medium">{$t('medium_default')}</option>
                <option value="large">{$t('large')}</option>
              </select>
            </label>

            <label>
              <span>{$t('font_family')}</span>
              <select
                value={subtitleStyle.fontFamily}
                onchange={(e) => updateSubtitleStyle('fontFamily', e.currentTarget.value as 'sans-serif' | 'serif' | 'monospace')}
              >
                <option value="sans-serif">{$t('sans_serif')}</option>
                <option value="serif">{$t('serif')}</option>
                <option value="monospace">{$t('monospace')}</option>
              </select>
            </label>

            <div class="color-row">
              <label>
                <span>{$t('text_color')}</span>
                <div class="color-picker-wrap">
                  <input
                    type="color"
                    value={subtitleStyle.textColor}
                    onchange={(e) => updateSubtitleStyle('textColor', e.currentTarget.value)}
                  />
                  <span>{subtitleStyle.textColor.toUpperCase()}</span>
                </div>
              </label>

              <label>
                <span>{$t('background_color')}</span>
                <div class="color-picker-wrap">
                  <input
                    type="color"
                    value={subtitleStyle.backgroundColor}
                    onchange={(e) => updateSubtitleStyle('backgroundColor', e.currentTarget.value)}
                  />
                  <span>{subtitleStyle.backgroundColor.toUpperCase()}</span>
                </div>
              </label>
            </div>

            <label>
              <span>{$t('background_opacity', { value: subtitleStyle.backgroundOpacity })}</span>
              <div class="slider-wrap">
                <input
                  type="range"
                  min="0"
                  max="100"
                  step="5"
                  value={subtitleStyle.backgroundOpacity}
                  oninput={(e) => updateSubtitleStyle('backgroundOpacity', Number(e.currentTarget.value))}
                />
              </div>
              <small class="muted">{$t('no_opaque_background')}</small>
            </label>

            <label>
              <span>{$t('text_outline_shadow')}</span>
              <select
                value={subtitleStyle.textShadow}
                onchange={(e) => updateSubtitleStyle('textShadow', e.currentTarget.value as 'none' | 'outline' | 'drop-shadow')}
              >
                <option value="drop-shadow">{$t('drop_shadow_default')}</option>
                <option value="outline">{$t('strong_black_outline')}</option>
                <option value="none">{$t('no_outline_shadow')}</option>
              </select>
            </label>
          </div>
        </section>
      </section></div>
      <div hidden={current !== 'about'}>
        <section class="card" aria-labelledby="about-heading">
          <h2 id="about-heading" class="card-title">À propos de {serverConfig?.name || 'Aether'}</h2>
          <div class="profile-info-list">
            <div class="info-row">
              <span class="label">Version</span>
              <span class="val">
                <strong>{serverConfig?.version || '0.1.0'}</strong>
                {#if serverConfig?.channel}
                  <span class="role-badge" class:admin-badge={serverConfig.channel === 'stable'}>
                    {serverConfig.channel}
                  </span>
                {/if}
              </span>
            </div>
            {#if serverConfig?.commit}
              <div class="info-row">
                <span class="label">Commit Git</span>
                <span class="val"><code>{serverConfig.commit}</code></span>
              </div>
            {/if}
            {#if serverConfig?.build_date}
              <div class="info-row">
                <span class="label">Date de compilation</span>
                <span class="val">{serverConfig.build_date}</span>
              </div>
            {/if}
            {#if serverConfig?.release_url}
              <div class="info-row">
                <span class="label">Notes de version</span>
                <span class="val">
                  <!-- eslint-disable-next-line svelte/no-navigation-without-resolve -->
                  <a href={serverConfig.release_url} target="_blank" rel="noopener noreferrer">
                    Consulter la release {serverConfig.version} sur GitHub
                  </a>
                </span>
              </div>
            {/if}
            <div class="info-row">
              <span class="label">Mode d'exécution</span>
              <span class="val"><code>{serverConfig?.mode || 'standalone'}</code></span>
            </div>
          </div>
        </section>
      </div>
    </div>
  </div>
</div>
<style>
  .settings-view { padding: 20px 0 48px; }
  h1 { font-size: clamp(32px, 4vw, 44px); font-weight: 800; letter-spacing: -0.03em; line-height: 1.02; margin: 0 0 28px; }
  .settings-layout { display: grid; grid-template-columns: 260px minmax(0, 1fr); gap: 32px; align-items: start; }
  .settings-nav { display: flex; flex-direction: column; gap: 2px; position: sticky; top: 88px; }
  .nav-group { margin: 18px 0 6px; padding: 0 12px; font-size: 12px; font-weight: 700; letter-spacing: 0.07em; text-transform: uppercase; color: var(--mu); }
  .nav-group:first-child { margin-top: 0; }
  .settings-nav a {
    display: flex;
    align-items: center;
    gap: 12px;
    min-height: 42px;
    padding: 0 12px;
    border-radius: 12px;
    color: var(--mu);
    font-size: 14px;
    font-weight: 600;
    transition: background-color var(--t-fast) ease, color var(--t-fast) ease;
  }
  .settings-nav a:hover { background: var(--pn); color: var(--tx); }
  .settings-nav a.active { background: var(--pn-2); color: var(--tx); }
  .server-link { margin-top: 16px; border-top: 1px solid var(--line); border-radius: 0 0 12px 12px; padding-top: 4px; }
  .settings-content { min-width: 0; display: flex; flex-direction: column; }
  .embedded-panel :global(.application-page) { max-width: none; padding: 0; }
  .embedded-panel > h2 { margin-bottom: 4px; }
  .card {
    min-width: 0;
    overflow-wrap: anywhere;
    padding: 24px;
    background: var(--bg-2);
    border: 1px solid var(--line);
    border-radius: var(--r-lg);
    margin-bottom: 20px;
  }
  h2 { font-size: 24px; font-weight: 800; letter-spacing: -0.02em; margin: 0 0 18px; }
  h3 { margin: 0; font-size: 17px; font-weight: 700; }
  .profile-header-row { display: flex; align-items: center; gap: 20px; margin-bottom: 24px; }
  .profile-handle { margin: 4px 0; color: var(--mu); }
  .role-badge { display: inline-flex; align-items: center; height: 24px; margin-top: 8px; padding: 0 9px; font-size: 12px; font-weight: 700; border-radius: var(--r-xs); background: var(--pn-2); }
  .admin-badge { background: var(--ac-soft); color: var(--ac); }
  .profile-info-list { display: grid; }
  .info-row { display: grid; grid-template-columns: 180px minmax(0, 1fr); gap: 16px; padding: 16px 0; border-bottom: 1px solid var(--line); font-size: 14px; }
  .info-row:first-child { padding-top: 0; }
  .info-row:last-child { border-bottom: none; padding-bottom: 0; }
  .label { color: var(--mu); font-weight: 600; }
  .val { font-weight: 600; }
  .val code { overflow-wrap: anywhere; font-family: var(--font-mono); font-size: 13px; }
  .muted { color: var(--mu); font-size: 14px; line-height: 1.6; }
  .pref-fields, .offsets { display: grid; gap: 20px; margin-top: 8px; }
  label { display: grid; gap: 8px; font-size: 14px; font-weight: 700; }
  .checkbox-row { display: flex; align-items: center; gap: 12px; min-height: 44px; font-weight: 600; }
  .checkbox-row input { flex: 0 0 auto; width: 20px; height: 20px; margin: 0; accent-color: var(--ac); }
  input[type='checkbox'] { accent-color: var(--ac); width: 20px; height: 20px; }
  label > span { display: flex; align-items: center; gap: 8px; }
  select, input:not([type='checkbox']):not([type='range']):not([type='color']) {
    min-width: 0;
    width: 100%;
    max-width: 520px;
    height: 46px;
    min-height: 46px;
    padding: 0 14px;
    border: 1px solid transparent;
    border-radius: var(--r-sm);
    background-color: var(--pn);
    color: var(--tx);
    font-weight: 500;
    box-shadow: none;
    transition: border-color var(--t-fast) ease, background-color var(--t-fast) ease;
  }
  select { padding-right: 40px; }
  select:hover, input:not([type='checkbox']):not([type='range']):not([type='color']):hover { background-color: var(--pn-2); }
  select:focus, input:focus { outline: none; border-color: var(--ac); background-color: var(--bg-3); }
  button {
    justify-self: start;
    height: 48px;
    padding: 0 22px;
    border: 0;
    border-radius: var(--r-md);
    background: var(--btn);
    color: var(--btn-tx);
    font-weight: 700;
    cursor: pointer;
    transition: opacity var(--t-fast) ease, transform var(--t-fast) var(--ease);
  }
  button:hover { opacity: 0.9; }
  button:active { transform: scale(0.98); }
  .offsets { padding-top: 0; }
  .offsets p { margin: 0; }
  .subtitles-appearance { margin-top: 28px; padding-top: 24px; border-top: 1px solid var(--line); }
  .subtitles-appearance h3 { margin-bottom: 8px; }
  /* Aperçu des sous-titres : posé sur une image sombre neutre, comme dans le lecteur */
  .subtitle-preview-box {
    margin: 16px 0;
    padding: 36px 20px;
    background: #11151f;
    border-radius: var(--r-md);
    display: flex;
    align-items: flex-end;
    justify-content: center;
    text-align: center;
    min-height: 160px;
    overflow: hidden;
  }
  .subtitle-preview-text {
    padding: 4px 10px;
    border-radius: 4px;
    display: inline-block;
    max-width: 90%;
    word-break: break-word;
    transition: all var(--t-fast) ease-out;
  }
  .color-row { display: grid; grid-template-columns: repeat(auto-fit, minmax(200px, 1fr)); gap: 20px; }
  .color-picker-wrap { display: flex; align-items: center; gap: 12px; }
  .color-picker-wrap input[type='color'] { width: 48px; height: 44px; padding: 2px 4px; border-radius: 10px; border: 1px solid var(--line); background: transparent; cursor: pointer; }
  .slider-wrap input[type='range'] { width: 100%; max-width: 520px; accent-color: var(--ac); }
  @media (max-width: 1023px) {
    .settings-layout { grid-template-columns: minmax(0, 1fr); gap: 20px; }
    .settings-nav { position: static; flex-direction: row; gap: 6px; overflow-x: auto; scrollbar-width: none; padding-bottom: 4px; }
    .nav-group { display: none; }
    .settings-nav a { flex: none; white-space: nowrap; background: var(--pn); min-height: 40px; }
    .server-link { margin-top: 0; border-top: 0; border-radius: 12px; padding-top: 0; }
  }
  @media (max-width: 640px) {
    .card { padding: 16px; border-radius: var(--r-md); }
    .info-row { grid-template-columns: minmax(0, 1fr); gap: 6px; }
  }
</style>
