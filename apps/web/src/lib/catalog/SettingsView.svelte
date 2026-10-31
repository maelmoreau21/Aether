<script lang="ts">
  import { onMount } from 'svelte';
  import { preferences, ensurePreferences, savePreferences } from '../preferences';
  import { resolve } from '$app/paths';
  import { session } from '../auth/session';
  import ProfileAvatar from '../ui/ProfileAvatar.svelte';
  import AvatarEditor from './AvatarEditor.svelte';
  import OidcSettings from '../auth/OidcSettings.svelte';
  import AccountSecurity from '../auth/AccountSecurity.svelte';
  import Icon from '../ui/Icon.svelte';
  import { t, locale, setLocale } from '../i18n';
  import { themeMode, setTheme, type ThemeMode } from '../theme';
  import { DEFAULT_SUBTITLE_STYLE, sanitizeSubtitleStyle, subtitleStyleToCss, type SubtitleStyle } from '../player/subtitle-style';
  import { fetchServerConfig, type AppServerConfig } from '../config/brand';
  import { defaultArtworkRotationPolicy, setArtworkRotationPolicy, type ArtworkRotationPolicy } from './artwork-rotation-policy.svelte';
  import { accessibleLibraries, loadLibraries } from './libraries';

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
  let offsetsEdited = false;
  let artworkRotation = $state<ArtworkRotationPolicy>({ ...defaultArtworkRotationPolicy, ...preferences().artworkRotation });
  let artworkEdited = false;

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
    { id: 'account', label: 'Compte', icon: 'profile' },
    { id: 'display', label: 'Affichage & Affiches', icon: 'image' },
    { id: 'security', label: 'Sécurité', icon: 'shield' },
    { id: 'devices', label: 'Appareils', icon: 'server' },
    { id: 'playback', label: 'Lecture', icon: 'play' },
    { id: 'about', label: 'À propos', icon: 'info' }
  ] as const;
  onMount(() => {
    let disposed = false;
    void loadLibraries();
    void fetchServerConfig().then(cfg => { if (!disposed) serverConfig = cfg; }).catch(() => {});
    void ensurePreferences().then(saved => {
      if (disposed || !saved) return;
      if (!offsetsEdited) {
        audioOffset = saved.offsets?.audio ?? 0;
        subtitleOffset = saved.offsets?.subtitle ?? 0;
      }
      if (saved.subtitleStyle) {
        subtitleStyle = { ...DEFAULT_SUBTITLE_STYLE, ...sanitizeSubtitleStyle(saved.subtitleStyle) };
      }
      skipIntroEnabled = saved.skipIntroEnabled !== false;
      skipCreditsEnabled = saved.skipCreditsEnabled !== false;
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
  function saveArtworkRotation(event: SubmitEvent) {
    event.preventDefault();
    artworkEdited = true;
    const value = { ...artworkRotation };
    savePreferences({ artworkRotation: value });
    setArtworkRotationPolicy(value);
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
        <form class="card offsets" onsubmit={saveArtworkRotation}>
          <h2>Rotation des affiches</h2>
          <p class="muted">La rotation utilise les affiches déjà présentes en local. Les images choisies manuellement restent prioritaires.</p>
          <label><span>Activer la rotation automatique</span><input type="checkbox" bind:checked={artworkRotation.enabled} onchange={() => artworkEdited = true} /></label>

          <fieldset class="library-filter-fieldset">
            <legend>Bibliothèques concernées</legend>
            <p class="hint">Cochez les bibliothèques sur lesquelles activer la rotation des affiches.</p>
            <div class="library-checks">
              {#each ($accessibleLibraries.filter(l => l.kind !== 'music')) as lib (lib.id)}
                <label class="lib-checkbox-label">
                  <input type="checkbox"
                    checked={artworkRotation.enabledLibraries ? artworkRotation.enabledLibraries.includes(lib.id) : (lib.kind === 'films' ? artworkRotation.enableMovies !== false : artworkRotation.enableSeries !== false)}
                    onchange={(e) => {
                      artworkEdited = true;
                      const defaults = $accessibleLibraries.filter(l => l.kind !== 'music' && (l.kind === 'films' ? artworkRotation.enableMovies !== false : artworkRotation.enableSeries !== false)).map(l => l.id);
                      let curr = artworkRotation.enabledLibraries ? [...artworkRotation.enabledLibraries] : defaults;
                      if (e.currentTarget.checked) {
                        if (!curr.includes(lib.id)) curr.push(lib.id);
                      } else {
                        curr = curr.filter(id => id !== lib.id);
                      }
                      artworkRotation.enabledLibraries = curr;
                    }}
                  />
                  <span><strong>{lib.name}</strong> <small>({lib.kind === 'films' ? 'Films' : 'Séries'})</small></span>
                </label>
              {:else}
                <p class="muted">Aucune bibliothèque vidéo disponible.</p>
              {/each}
            </div>
          </fieldset>

          <label>Ordre de rotation<select bind:value={artworkRotation.rotationMode} onchange={() => artworkEdited = true}><option value="random">Aléatoire</option><option value="sequential">Séquentiel</option></select></label>
          <label>Intervalle minimal en heures (0 : nouvelle visite)<input type="number" min="0" max="720" step="1" required bind:value={artworkRotation.minHoursBetweenSwitches} oninput={() => artworkEdited = true} /></label>
          <label>Nombre maximal d'affiches en réserve<input type="number" min="1" max="24" step="1" required bind:value={artworkRotation.poolSize} oninput={() => artworkEdited = true} /></label>
          <label>Langue préférée des affiches<input type="text" pattern="[A-Za-z0-9-]+" minlength="2" maxlength="35" required bind:value={artworkRotation.preferredLanguage} oninput={() => artworkEdited = true} /></label>
          <label>Langue de repli<input type="text" pattern="[A-Za-z0-9-]+" minlength="2" maxlength="35" required bind:value={artworkRotation.fallbackLanguage} oninput={() => artworkEdited = true} /></label>
          <label><span>Inclure les affiches sans langue taguée</span><input type="checkbox" bind:checked={artworkRotation.includeUnknownLanguage} onchange={() => artworkEdited = true} /></label>
          <label><span>Accepter les autres langues en repli si indisponible</span><input type="checkbox" bind:checked={artworkRotation.allowAnyLanguageFallback} onchange={() => artworkEdited = true} /></label>
          <button type="submit">Enregistrer l'affichage</button>
        </form>
      </div>
      <div hidden={current !== 'security'}><section class="card"><h2>Sécurité</h2><AccountSecurity mode="security" /><h3>Connexion SSO</h3><OidcSettings embedded /></section></div>
      <div hidden={current !== 'devices'}><section class="card"><h2>Appareils</h2><AccountSecurity mode="devices" /></section></div>
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


        <section class="offsets" aria-label="Sauts des génériques">
          <h3>Intros et génériques</h3>
          <label><input type="checkbox" bind:checked={skipIntroEnabled} onchange={() => savePreferences({ skipIntroEnabled })} /> Proposer « Passer l'intro »</label>
          <label><input type="checkbox" bind:checked={skipCreditsEnabled} onchange={() => savePreferences({ skipCreditsEnabled })} /> Proposer « Passer le générique »</label>
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
  .settings-view { padding-top: 24px; }
  h1 { font-size: clamp(24px, 2.8vw, 32px); font-weight: 800; letter-spacing: -0.02em; margin: 0 0 32px; }
  .settings-layout { display: grid; grid-template-columns: 230px minmax(0, 1fr); gap: 40px; align-items: start; }
  .settings-nav { display: grid; gap: 6px; }
  .settings-nav a {
    display: flex;
    align-items: center;
    gap: 14px;
    padding: 12px 16px;
    color: var(--text-secondary);
    font-size: 14px;
    font-weight: 500;
    min-height: 48px;
    border-radius: var(--radius-sm);
    border-left: 3px solid transparent;
    transition: background var(--transition-fast), color var(--transition-fast), border-color var(--transition-fast);
  }
  .settings-nav a:hover, .settings-nav a.active { background: var(--bg-elevated); color: var(--text-primary); }
  .settings-nav a.active { border-left: 3px solid var(--accent-primary); font-weight: 700; }
  .server-link { margin-top: 20px; border-top: 1px solid var(--border-subtle); padding-top: 16px; }
  .settings-content { min-width: 0; max-width: 860px; }
  .card {
    min-width: 0;
    overflow-wrap: anywhere;
    padding: 24px;
    background: var(--bg-surface);
    border: 1px solid var(--border-subtle);
    border-radius: var(--radius-md);
    margin-bottom: 24px;
  }
  h2 { font-size: 20px; font-weight: 700; letter-spacing: -0.015em; margin: 0 0 20px; }
  h3 { margin: 0; font-size: 17px; font-weight: 650; }
  .profile-header-row { display: flex; align-items: center; gap: 20px; margin-bottom: 28px; }
  .profile-handle { margin: 6px 0; color: var(--text-secondary); }
  .role-badge { display: inline-block; margin-top: 8px; padding: 4px 10px; font-size: 11px; font-weight: 700; text-transform: uppercase; letter-spacing: 0.04em; border-radius: var(--radius-full); background: var(--bg-elevated); }
  .admin-badge { background: var(--color-accent-soft); color: var(--accent-primary); }
  .profile-info-list { display: grid; gap: 0; }
  .info-row { display: grid; grid-template-columns: 180px minmax(0, 1fr); gap: 16px; padding: 18px 0; border-bottom: 1px solid var(--border-subtle); font-size: 14px; }
  .info-row:last-child { border-bottom: none; padding-bottom: 0; }
  .label { color: var(--text-secondary); }
  .val code { overflow-wrap: anywhere; }
  .muted { color: var(--text-secondary); font-size: 14px; line-height: 1.6; }
  .pref-fields, .offsets { display: grid; gap: 24px; margin-top: 24px; }
  label { display: grid; gap: 10px; font-size: 14px; font-weight: 500; }
  label > span { display: flex; align-items: center; gap: 8px; }
  select, input:not([type="checkbox"]):not([type="range"]):not([type="color"]) {
    min-width: 0;
    width: 100%;
    max-width: 520px;
    min-height: 44px;
    padding: 10px 14px;
    border: 1px solid var(--border-strong);
    border-radius: var(--radius-sm);
    background-color: var(--bg-elevated);
    color: var(--text-primary);
    transition: border-color var(--transition-fast);
  }
  select:focus, input:focus { border-color: var(--accent-primary); }
  button {
    justify-self: start;
    min-height: 44px;
    padding: 12px 22px;
    border: 0;
    border-radius: var(--radius-sm);
    background: var(--color-play);
    color: var(--color-on-accent);
    font-weight: 700;
    cursor: pointer;
    box-shadow: var(--shadow-glow);
    transition: background var(--transition-fast), transform var(--transition-fast);
  }
  button:hover { background: var(--color-play-hover); transform: translateY(-1px); }
  .offsets { padding-top: 28px; border-top: 1px solid var(--border-subtle); }
  .offsets p { margin: 0; }
  .subtitles-appearance { margin-top: 36px; padding-top: 28px; border-top: 1px solid var(--border-subtle); }
  .subtitles-appearance h3 { margin-bottom: 8px; }
  .subtitle-preview-box {
    margin: 20px 0;
    padding: 36px 20px;
    background: linear-gradient(135deg, #18181B 0%, #0B0C10 100%);
    border: 1px solid var(--border-subtle);
    border-radius: var(--radius-md);
    display: flex;
    align-items: center;
    justify-content: center;
    text-align: center;
    min-height: 120px;
    overflow: hidden;
  }
  .subtitle-preview-text {
    padding: 4px 10px;
    border-radius: 4px;
    display: inline-block;
    max-width: 90%;
    word-break: break-word;
    transition: all 0.15s ease-out;
  }
  .color-row {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(200px, 1fr));
    gap: 20px;
  }
  .color-picker-wrap {
    display: flex;
    align-items: center;
    gap: 12px;
  }
  .color-picker-wrap input[type="color"] {
    width: 48px;
    height: 44px;
    padding: 2px 4px;
    border-radius: 4px;
    border: 1px solid var(--border-subtle);
    background: transparent;
    cursor: pointer;
  }
  .slider-wrap input[type="range"] {
    width: 100%;
    max-width: 520px;
    accent-color: var(--color-play);
  }
  @media (max-width: 760px) {
    .settings-layout { grid-template-columns: minmax(0, 1fr); gap: 24px; }
    .settings-nav { grid-template-columns: repeat(2, minmax(0, 1fr)); gap: 6px; }
    .settings-nav a { font-size: 13px; padding: 12px; gap: 10px; }
    .server-link { grid-column: 1 / -1; margin-top: 4px; }
    .card { padding: 18px; }
    .info-row { grid-template-columns: minmax(0, 1fr); gap: 8px; }
  }
</style>
