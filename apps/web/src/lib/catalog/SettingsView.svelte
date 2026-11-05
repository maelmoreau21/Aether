<script lang="ts">
  import { onMount } from 'svelte';
  import { preferences, ensurePreferences, savePreferences } from '../preferences';
  import { resolve } from '$app/paths';
  import { session } from '../auth/session';
  import { features } from '../applications/api';
  import ProfileAvatar from '../ui/ProfileAvatar.svelte';
  import AppearanceCustomization from './AppearanceCustomization.svelte';
  import RecommendationSettings from './RecommendationSettings.svelte';
  import NotificationSettings from './NotificationSettings.svelte';
  import AvatarEditor from './AvatarEditor.svelte';
  import OidcSettings from '../auth/OidcSettings.svelte';
  import AccountSecurity from '../auth/AccountSecurity.svelte';
  import Icon from '../ui/Icon.svelte';
  import { t, locale, setLocale } from '../i18n';
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
  const pages = [
    { id: 'account', label: 'Profil', icon: 'profile', group: 'Moi' },
    { id: 'playback', label: 'Lecture et sous-titres', icon: 'play', group: 'Expérience' },
    { id: 'display', label: 'Apparence et accueil', icon: 'image', group: 'Expérience' },
    { id: 'security', label: 'Mot de passe et SSO', icon: 'shield', group: 'Sécurité' },
    { id: 'devices', label: 'Appareils et sessions', icon: 'server', group: 'Sécurité' },
    { id: 'about', label: 'À propos', icon: 'info', group: 'Sécurité' }
  ] as const;
  const pageGroups = ['Moi', 'Expérience', 'Sécurité'] as const;
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
    const sync = () => { const hash = window.location.hash.slice(1); const id = hash === 'appearance' ? 'account' : hash === 'sso' ? 'security' : hash; current = pages.some(page => page.id === id) ? id : 'account'; };
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
      else artworkError = 'Impossible d’enregistrer vos préférences. Réessayez.';
    } catch { artworkError = 'Impossible d’enregistrer vos préférences. Réessayez.'; }
    finally { artworkSaving = false; }
  }
</script>

<div class="settings-view">
  <h1>Paramètres</h1>
  <div class="settings-layout">
    <nav class="settings-nav" aria-label="Paramètres personnels">
      {#each pageGroups as group (group)}
        <p class="nav-group">{group}</p>
        {#each pages.filter(page => page.group === group) as page (page.id)}
          <a href={resolve(`settings/#${page.id}`)} onclick={() => current = page.id} class:active={current === page.id} aria-current={current === page.id ? 'page' : undefined}><Icon name={page.icon} size={18} />{page.label}</a>
        {/each}
        {#if group === 'Moi'}
          {#if $features.requests}<a href={resolve('requests/')}><Icon name="download" size={18} />Demandes</a>{/if}
          {#if $features.reports}<a href={resolve('reports/')}><Icon name="alert" size={18} />Signalements</a>{/if}
          <a href={resolve('statistics/')}><Icon name="clock" size={18} />Mes statistiques</a>
          <a href={resolve('downloads/')}><Icon name="download" size={18} />Téléchargements</a>
        {/if}
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


    </section>
<section class="card"><h2>Préférences de l’interface</h2>      <div class="pref-fields">
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
          <span><Icon name="tv" size={14} /> Mode TV sur cet appareil</span>
          <select value={tvMode} onchange={(e) => { tvMode = e.currentTarget.value as TvMode; setTvMode(tvMode); }}>
            <option value="auto">Automatique (téléviseurs détectés)</option>
            <option value="on">Activé</option>
            <option value="off">Désactivé</option>
          </select>
        </label>

      </div>
</section>

      <section class="card profile-card" aria-label="Informations du compte">
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
      </section>
      </div>
            <div hidden={current !== 'display'}>
        <AppearanceCustomization />
        <RecommendationSettings />
        <section class="card" aria-labelledby="home-preferences-heading">
          <h2 id="home-preferences-heading">Accueil</h2>
          <label class="checkbox-row"><input type="checkbox" bind:checked={hidePlayedInLatest} onchange={() => void saveLatestPreference()} /> Masquer les contenus vus dans les nouveautés</label>
        </section>
        <form class="card offsets" onsubmit={saveArtworkRotation}>
          <h2>Rotation des affiches</h2>
          <p class="muted">La rotation utilise les affiches déjà présentes en local. Les images choisies manuellement restent prioritaires.</p>
          <label><span>Activer la rotation automatique</span><input type="checkbox" bind:checked={artworkRotation.enabled} onchange={() => artworkEdited = true} /></label>

          <label>Langue des affiches
            <select bind:value={artworkRotation.languageMode} onchange={() => artworkEdited = true}>
              <option value="interface">Langue de l’interface</option>
              <option value="original">Langue originale de l’œuvre</option>
            </select>
          </label>
          <p class="muted">Si la langue originale ou une affiche correspondante est inconnue, la langue de l’interface sert de repli.</p>
          <button type="submit" disabled={artworkSaving}>{artworkSaving ? 'Enregistrement…' : "Enregistrer l’affichage"}</button>
          {#if artworkError}<p role="alert">{artworkError}</p>{/if}
        </form>
      </div>
      <div hidden={current !== 'security'}><section class="card"><h2>Sécurité</h2><AccountSecurity mode="security" /><h3>Connexion SSO</h3><OidcSettings embedded /></section><section class="card"><NotificationSettings /></section></div>
      <div hidden={current !== 'devices'}><section class="card"><h2>Appareils et sessions</h2><AccountSecurity mode="devices" /></section></div>
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


        <section class="offsets" aria-label="Choix des pistes">
          <h3>Choix des pistes</h3>
          <label class="checkbox-row"><input type="checkbox" bind:checked={playDefaultAudioTrack} onchange={saveTrackDefaults} /> Privilégier la piste audio par défaut du fichier</label>
          <p class="muted">Si aucune piste par défaut n’est déclarée et sélectionnable, votre langue préférée reste utilisée.</p>
          <label class="checkbox-row"><input type="checkbox" bind:checked={rememberAudioSelections} onchange={saveTrackDefaults} /> Mémoriser mes choix audio</label>
          <label class="checkbox-row"><input type="checkbox" bind:checked={rememberSubtitleSelections} onchange={saveTrackDefaults} /> Mémoriser mes choix de sous-titres</label>
          <p class="muted">Vos choix manuels restent prioritaires pendant la séance. Sans mémorisation, les nouvelles séances utilisent vos réglages audio et sous-titres ci-dessus. Aucun ancien choix n’est effacé.</p>
        </section>

        <section class="offsets" aria-label="Commandes de lecture">
          <h3>Commandes de lecture</h3>
          <label class="checkbox-row"><input type="checkbox" bind:checked={playbackDefaults.autoplayNextEpisode} onchange={savePlaybackDefaults} /> Enchaîner automatiquement l’épisode suivant</label>
          <p class="muted">S’applique aux épisodes disposant d’un suivant. Les séances synchronisées conservent leurs commandes communes.</p>
          <label>Reculer de<select bind:value={playbackDefaults.skipBackSeconds} onchange={savePlaybackDefaults}>{#each SKIP_SECONDS as seconds (seconds)}<option value={seconds}>{seconds} secondes</option>{/each}</select></label>
          <label>Avancer de<select bind:value={playbackDefaults.skipForwardSeconds} onchange={savePlaybackDefaults}>{#each SKIP_SECONDS as seconds (seconds)}<option value={seconds}>{seconds} secondes</option>{/each}</select></label>
          <p class="muted">Les boutons du lecteur et les flèches gauche/droite utilisent ces durées.</p>
        </section>

        <section class="offsets" aria-label="Sauts des génériques">
          <h3>Intros et génériques</h3>
          <label class="checkbox-row"><input type="checkbox" bind:checked={skipIntroEnabled} onchange={() => savePreferences({ skipIntroEnabled })} /> Proposer « Passer l'intro »</label>
          <label class="checkbox-row"><input type="checkbox" bind:checked={skipCreditsEnabled} onchange={() => savePreferences({ skipCreditsEnabled })} /> Proposer « Passer le générique »</label>
          <label class="checkbox-row"><input type="checkbox" bind:checked={autoSkipIntro} onchange={() => savePreferences({ autoSkipIntro })} /> Sauter automatiquement l'introduction</label>
          <label class="checkbox-row"><input type="checkbox" bind:checked={autoSkipCredits} onchange={() => savePreferences({ autoSkipCredits })} /> Sauter automatiquement le générique de fin</label>
        </section>

        <form class="offsets" onsubmit={saveOffsets}>
          <h3>Synchronisation</h3><p class="muted">Décalages par défaut, de −30 à +30 secondes. Un décalage positif retarde la piste. Le décalage audio concerne les pistes externes.</p>
          <label>Décalage audio (secondes)<input type="number" min="-30" max="30" step="0.1" required bind:value={audioOffset} oninput={() => { offsetsEdited = true; }} /></label>
          <label>Décalage des sous-titres (secondes)<input type="number" min="-30" max="30" step="0.1" required bind:value={subtitleOffset} oninput={() => { offsetsEdited = true; }} /></label>
          <button type="submit">Enregistrer les décalages</button>
        </form>

        <section class="subtitles-appearance" aria-labelledby="subtitles-style-heading">
          <h3 id="subtitles-style-heading">Style des sous-titres</h3>
          <p class="muted">Personnalisez l’affichage des sous-titres dans le lecteur vidéo.</p>

          <div class="subtitle-preview-box" aria-hidden="true">
            <span
              class="subtitle-preview-text"
              style:color={previewSubtitleCss.color}
              style:font-size={previewSubtitleCss.fontSize}
              style:font-family={previewSubtitleCss.fontFamily}
              style:background-color={previewSubtitleCss.backgroundColor}
              style:text-shadow={previewSubtitleCss.textShadow}
            >
              Exemple de sous-titre affiché
            </span>
          </div>

          <div class="pref-fields">
            <label>
              <span>Taille de la police</span>
              <select
                value={subtitleStyle.fontSize}
                onchange={(e) => updateSubtitleStyle('fontSize', e.currentTarget.value as 'small' | 'medium' | 'large')}
              >
                <option value="small">Petite</option>
                <option value="medium">Moyenne (par défaut)</option>
                <option value="large">Grande</option>
              </select>
            </label>

            <label>
              <span>Police d’écriture</span>
              <select
                value={subtitleStyle.fontFamily}
                onchange={(e) => updateSubtitleStyle('fontFamily', e.currentTarget.value as 'sans-serif' | 'serif' | 'monospace')}
              >
                <option value="sans-serif">Sans empattement (Moderne / Sans-serif)</option>
                <option value="serif">Avec empattement (Classique / Serif)</option>
                <option value="monospace">Chasse fixe (Monospace)</option>
              </select>
            </label>

            <div class="color-row">
              <label>
                <span>Couleur du texte</span>
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
                <span>Couleur d’arrière-plan</span>
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
              <span>Opacité de l’arrière-plan ({subtitleStyle.backgroundOpacity} %)</span>
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
              <small class="muted">À 0 %, aucun fond opaque ne vient masquer la vidéo.</small>
            </label>

            <label>
              <span>Contour et ombre du texte</span>
              <select
                value={subtitleStyle.textShadow}
                onchange={(e) => updateSubtitleStyle('textShadow', e.currentTarget.value as 'none' | 'outline' | 'drop-shadow')}
              >
                <option value="drop-shadow">Ombre portée nette (par défaut)</option>
                <option value="outline">Contour noir renforcé</option>
                <option value="none">Aucun contour / ombre</option>
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
