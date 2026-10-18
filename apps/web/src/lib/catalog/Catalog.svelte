<script module lang="ts">
  const links = [
    { key: 'home', path: '/', label: 'Accueil', symbol: '⌂' },
    { key: 'films', path: 'films/', label: 'Films', symbol: '▤' },
    { key: 'series', path: 'series/', label: 'Séries', symbol: '▥' },
    { key: 'search', path: 'search/', label: 'Recherche', symbol: '⌕' }
  ] as const;
</script>

<script lang="ts">
  import { onMount, tick } from 'svelte';
  import { page } from '$app/state';
  import { resolve } from '$app/paths';
  import { DEFAULT_NAME, fetchBrandName } from '../config/brand';
  import type ImmersivePlayer from '../player/ImmersivePlayer.svelte';
  import type { Media, PlayerFactory } from '../player/types';
  import Icon from '../ui/Icon.svelte';
  import ProfileMenu from '../ui/ProfileMenu.svelte';
  import Poster from './Poster.svelte';
  import OidcSettings from '../auth/OidcSettings.svelte';
  import EpisodePlayer from './EpisodePlayer.svelte';
  import {
    videoResolutionBadge,
    containerLabel,
    colorProfileLabel,
    languageLabel
  } from '../player/track-labels';
  import {
    request,
    playbackIds,
    sourceLabel,
    fetchMediaDescriptors,
    revalidateMediaDescriptor,
    getCachedMedia,
    type Work,
    type CatalogPage
  } from './api';
  import { session, getSessionGeneration } from '../auth/session';

  let { section = 'home' }: { section?: 'home' | 'films' | 'series' | 'search' | 'settings' } = $props();
  let name = $state(DEFAULT_NAME);
  let media = $state<Media[]>([]);
  let mediaError = $state('');
  let catalogueNotice = $state('');
  let busy = $state(true);
  let error = $state('');
  let films = $state<Work[]>([]);
  let series = $state<Work[]>([]);
  let totalFilms = $state(0);
  let totalSeries = $state(0);
  let detail = $state<Work | null>(null);
  let selected = $state('');
  let playingEpisode = $state<Work | null>(null);
  let season = $state(1);
  let search = $state('');
  let sort = $state('title-asc');
  let selectedKindFilter = $state<'all' | 'film' | 'series'>('all');
  let immersive = $state<ImmersivePlayer>();
  let ImmersivePlayerComponent = $state<typeof ImmersivePlayer>();
  let createPlayer = $state<PlayerFactory>();
  let playerLoading = false;
  let launchingEpisodeId = $state<string | null>(null);
  let episodeLaunchController: AbortController | null = null;

  // Account Preferences (saved in localStorage for persistence)
  let prefAudio = $state('fr');
  let prefSubtitles = $state('forced');

  async function launchEpisode(episode: Work, versionId: string): Promise<void> {
    if (episodeLaunchController) {
      episodeLaunchController.abort();
      episodeLaunchController = null;
    }
    const controller = new AbortController();
    episodeLaunchController = controller;
    const launchGen = getSessionGeneration();
    launchingEpisodeId = episode.id;
    mediaError = '';

    try {
      const item = await revalidateMediaDescriptor(versionId, controller.signal);
      if (controller.signal.aborted || launchGen !== getSessionGeneration()) return;
      const cachedForEpisode = playbackIds(episode)
        .map(eid => (eid === item.id ? item : getCachedMedia(eid)))
        .filter((m): m is Media => m !== undefined);
      const otherMedia = media.filter(m => !cachedForEpisode.some(c => c.id === m.id));
      media = [...otherMedia, ...cachedForEpisode];
      playingEpisode = episode;
      await tick();
      if (controller.signal.aborted || launchGen !== getSessionGeneration()) return;
      immersive?.open(item);
    } catch (err) {
      if (!controller.signal.aborted && launchGen === getSessionGeneration()) {
        media = media.filter(m => m.id !== versionId && getCachedMedia(m.id) !== undefined);
        mediaError = err instanceof Error ? err.message : 'Chargement de l’épisode impossible.';
      }
    } finally {
      if (episodeLaunchController === controller) {
        launchingEpisodeId = null;
        episodeLaunchController = null;
      }
    }
  }

  async function loadPlayer(): Promise<void> {
    if (ImmersivePlayerComponent && createPlayer) return;
    if (playerLoading) return;
    playerLoading = true;
    try {
      const [{ default: comp }, { NativePlayer }] = await Promise.all([
        import('../player/ImmersivePlayer.svelte'),
        import('../player/native')
      ]);
      ImmersivePlayerComponent = comp;
      createPlayer = (video, onError) => new NativePlayer(video, onError);
    } finally {
      playerLoading = false;
    }
  }

  const base = $derived(resolve('/').replace(/\/$/, ''));
  const workId = $derived(page.url.searchParams.get('work') ?? '');
  const query = $derived(page.url.searchParams.get('q') ?? '');
  const order = $derived(page.url.searchParams.get('sort') === 'title-desc' ? 'title-desc' : 'title-asc');
  const offset = $derived(Math.max(0, Number(page.url.searchParams.get('offset')) || 0));
  const title = $derived(section === 'settings' ? 'Paramètres' : links.find(link => link.key === section)?.label ?? 'Accueil');
  const allowedIds = $derived(detail ? playbackIds(detail) : []);
  const playableMedia = $derived(media.filter(item => allowedIds.includes(item.id)));
  const episodeIds = $derived(playingEpisode ? playbackIds(playingEpisode) : []);
  const playerMedia = $derived(detail?.kind === 'series' ? media.filter(item => episodeIds.includes(item.id)) : playableMedia);
  const activeSeason = $derived(detail?.seasons?.find(item => item.number === season));

  // Current selected media for film technical inspector
  const currentSelectedMedia = $derived(playableMedia.find(item => item.id === selected) ?? playableMedia[0]);
  const currentVideoTrack = $derived(
    currentSelectedMedia?.sources[0]?.tracks?.find(t => t.codec_type === 'video')
  );
  const currentAudioTracks = $derived(
    currentSelectedMedia?.sources.flatMap(s => s.tracks?.filter(t => t.codec_type === 'audio') ?? []) ?? []
  );

  function detailUrl(work: Work) {
    return work.kind === 'series'
      ? resolve(`series/?work=${encodeURIComponent(work.id)}`)
      : resolve(`films/?work=${encodeURIComponent(work.id)}`);
  }

  async function openPlayer(): Promise<void> {
    const current = playableMedia.find(item => item.id === selected) ?? playableMedia[0];
    const targetId = current?.id ?? (allowedIds.includes(selected) ? selected : allowedIds[0]);
    if (!targetId) return;
    if (episodeLaunchController) {
      episodeLaunchController.abort();
      episodeLaunchController = null;
    }
    const controller = new AbortController();
    episodeLaunchController = controller;
    const launchGen = getSessionGeneration();
    mediaError = '';

    try {
      const item = await revalidateMediaDescriptor(targetId, controller.signal);
      if (controller.signal.aborted || launchGen !== getSessionGeneration()) return;
      media = [...media.filter(m => m.id !== item.id), item];
      selected = item.id;
      immersive?.open(item);
    } catch (err) {
      if (!controller.signal.aborted && launchGen === getSessionGeneration()) {
        media = media.filter(m => m.id !== targetId && getCachedMedia(m.id) !== undefined);
        mediaError = err instanceof Error ? err.message : 'Chargement des sources impossible.';
      }
    } finally {
      if (episodeLaunchController === controller) {
        episodeLaunchController = null;
      }
    }
  }

  onMount(() => {
    const controller = new AbortController();
    fetchBrandName(controller.signal)
      .then(brandName => { name = brandName; })
      .catch(() => {
        if (!controller.signal.aborted) {
          mediaError = 'La lecture est temporairement indisponible. Rechargez la page pour réessayer.';
        }
      });

    try {
      const savedAudio = localStorage.getItem('media_pref_audio');
      if (savedAudio) prefAudio = savedAudio;
      const savedSub = localStorage.getItem('media_pref_subtitles');
      if (savedSub) prefSubtitles = savedSub;
    } catch {
      // localStorage may be disabled in private browsing
    }

    return () => controller.abort();
  });

  function savePreference(key: string, value: string) {
    try {
      localStorage.setItem(key, value);
    } catch {
      // Ignore
    }
  }

  $effect(() => {
    const id = workId;
    const q = query;
    const sorting = order;
    const start = offset;
    const currentSection = section;
    const activeSessionKey = $session ? `${$session.user.id}:${$session.csrfToken}` : 'anonymous';
    void activeSessionKey;
    const effectSessionGen = getSessionGeneration();
    const controller = new AbortController();

    search = q;
    sort = sorting;
    busy = true;
    error = '';
    mediaError = '';
    detail = null;
    media = [];
    films = [];
    series = [];
    totalFilms = 0;
    totalSeries = 0;
    playingEpisode = null;

    async function load() {
      try {
        if (currentSection === 'settings') return;
        if (id) {
          if (currentSection !== 'films' && currentSection !== 'series') {
            throw new Error('Ouvrez ce média depuis sa bibliothèque Films ou Séries.');
          }
          const playerPromise = loadPlayer();
          const work = await request<Work>(`/api/v1/catalog/${encodeURIComponent(id)}`, controller.signal);
          if (controller.signal.aborted || effectSessionGen !== getSessionGeneration()) return;
          if (
            (currentSection === 'films' && work.kind !== 'film') ||
            (currentSection === 'series' && work.kind !== 'series')
          ) {
            throw new Error('Ce média appartient à une autre bibliothèque.');
          }
          await playerPromise;
          if (controller.signal.aborted || effectSessionGen !== getSessionGeneration()) return;
          detail = work;
          season = work.seasons?.[0]?.number ?? 1;

          if (work.kind === 'film') {
            const ids = playbackIds(work);
            selected = ids[0] ?? '';
            try {
              const items = await fetchMediaDescriptors(ids, controller.signal);
              if (!controller.signal.aborted && effectSessionGen === getSessionGeneration()) media = items;
            } catch (failure) {
              if (!controller.signal.aborted && effectSessionGen === getSessionGeneration()) {
                mediaError = failure instanceof Error ? failure.message : 'Chargement des sources impossible.';
              }
            }
          } else {
            selected = '';
            const allEpisodeIds = (work.seasons ?? []).flatMap(s => s.episodes.flatMap(playbackIds));
            media = allEpisodeIds.map(eid => getCachedMedia(eid)).filter((m): m is Media => m !== undefined);
          }
        } else {
          const kinds =
            currentSection === 'films'
              ? ['film']
              : currentSection === 'series'
                ? ['series']
                : ['film', 'series'];
          const results = await Promise.all(
            kinds.map(kind =>
              request<CatalogPage>(
                `/api/v1/catalog?${new URLSearchParams({
                  kind,
                  q,
                  sort: sorting,
                  offset: String(start),
                  limit: '24'
                })}`,
                controller.signal
              )
            )
          );
          if (controller.signal.aborted || effectSessionGen !== getSessionGeneration()) return;
          films = [];
          series = [];
          totalFilms = 0;
          totalSeries = 0;
          catalogueNotice = [...new Set(results.map(result => result.notice).filter(Boolean))].join(' ');
          results.forEach((result, index) => {
            if (kinds[index] === 'film') {
              films = result.items;
              totalFilms = result.total;
            } else {
              series = result.items;
              totalSeries = result.total;
            }
          });
        }
      } catch (e) {
        if (!controller.signal.aborted && effectSessionGen === getSessionGeneration()) {
          error = e instanceof Error ? e.message : 'Connexion au catalogue impossible.';
        }
      } finally {
        if (!controller.signal.aborted && effectSessionGen === getSessionGeneration()) {
          busy = false;
        }
      }
    }

    void load();
    return () => {
      controller.abort();
      if (episodeLaunchController) {
        episodeLaunchController.abort();
        episodeLaunchController = null;
      }
    };
  });
</script>

<svelte:head>
  <title>{detail?.title ?? title} · {name}</title>
  <meta name="description" content="Films et séries en haute fidélité. Bibliothèques locales et lecture directe." />
</svelte:head>

<a class="skip-link" href="#main-content">Aller au contenu</a>

<header class="site-header">
  <a class="brand" href={resolve('/')} aria-label={`${name}, accueil`}>
    <img src={`${base}/images/brand-mark.svg`} width="25" height="30" alt="" />
    <span>{name}</span>
  </a>
  <nav aria-label="Navigation principale">
    {#each links as link (link.key)}
      <a href={resolve(link.path)} aria-current={section === link.key ? 'page' : undefined}>
        <span class:search-icon={link.key === 'search'} class:nav-symbol={link.key !== 'search'} aria-hidden="true">
          {#if link.key === 'search'}
            <Icon name="search" size={19} />
          {:else}
            {link.symbol}
          {/if}
        </span>
        {link.label}
      </a>
    {/each}
  </nav>
  <ProfileMenu />
</header>

<main id="main-content" tabindex="-1">
  {#if section === 'settings'}
    <!-- Profile & Settings View -->
    <div class="page-heading">
      <p class="eyebrow">VOTRE ESPACE PERSONNEL</p>
      <h1>Profil & Paramètres</h1>
      <p>Gérez votre identité, vos accès SSO et vos préférences de lecture.</p>
    </div>

    <div class="settings-grid">
      <!-- User Account Info Card -->
      <section class="card profile-card" aria-labelledby="profile-heading">
        <h2 id="profile-heading" class="card-title">Compte utilisateur</h2>
        <div class="profile-header-row">
          <div class="profile-avatar-large" aria-hidden="true">
            <Icon name="profile" size={32} />
          </div>
          <div>
            <h3>{$session?.user.login ?? 'Utilisateur'}</h3>
            <span class="role-badge" class:admin-badge={$session?.user.role === 'administrator'}>
              {$session?.user.role === 'administrator' ? 'Administrateur de l’instance' : 'Utilisateur standard'}
            </span>
          </div>
        </div>

        <div class="profile-info-list">
          <div>
            <span class="label">Identifiant unique (UUID)</span>
            <span class="val"><code>{$session?.user.id ?? 'Inconnu'}</code></span>
          </div>
          <div>
            <span class="label">Type d’authentification</span>
            <span class="val">
              {$session?.user.login.startsWith('sso-') ? 'SSO Authentik (OpenID Connect)' : 'Compte local (hash Argon2id)'}
            </span>
          </div>
          <div>
            <span class="label">Sécurité de session</span>
            <span class="val">Cookie <code>HttpOnly</code> strict, rotation CSRF et expiration glissante.</span>
          </div>
        </div>

        {#if $session?.user.role === 'administrator'}
          <div class="admin-shortcut">
            <a class="primary admin-link" href={resolve('admin/libraries/')}>
              <Icon name="settings" size={16} />
              Ouvrir la console Administrateur
            </a>
          </div>
        {/if}
      </section>

      <!-- OIDC Settings -->
      <section class="card oidc-card" aria-labelledby="sso-settings-heading">
        <h2 id="sso-settings-heading" class="card-title">Liaison Authentik (SSO)</h2>
        <OidcSettings />
      </section>

      <!-- Playback Preferences Card -->
      <section class="card preferences-card" aria-labelledby="pref-heading">
        <h2 id="pref-heading" class="card-title">Préférences de lecture</h2>
        <p class="muted">
          Configurez vos préférences pour la sélection automatique des pistes audio et des sous-titres lors du lancement des médias.
        </p>

        <div class="pref-fields">
          <label>
            <span>Langue audio par défaut</span>
            <select
              bind:value={prefAudio}
              onchange={() => savePreference('media_pref_audio', prefAudio)}
            >
              <option value="fr">Français (par défaut)</option>
              <option value="original">Version originale (si disponible)</option>
              <option value="en">Anglais</option>
            </select>
          </label>

          <label>
            <span>Affichage des sous-titres</span>
            <select
              bind:value={prefSubtitles}
              onchange={() => savePreference('media_pref_subtitles', prefSubtitles)}
            >
              <option value="forced">Sous-titres forcés uniquement (recommandé)</option>
              <option value="always">Toujours afficher les sous-titres</option>
              <option value="off">Désactivés par défaut</option>
            </select>
          </label>
        </div>
        <p class="pref-note">
          ✓ Vos préférences sont enregistrées sur cet appareil et appliquées automatiquement au lecteur.
        </p>
      </section>
    </div>

  {:else if busy}
    <div class="page-heading">
      <p class="eyebrow">{title}</p>
      <h1>Votre médiathèque</h1>
    </div>
    <div class="empty loading-empty" role="status">
      <div class="spinner" aria-hidden="true"></div>
      Chargement du catalogue…
    </div>

  {:else if error}
    <div class="page-heading">
      <h1>{title}</h1>
    </div>
    <div class="empty error" role="alert">
      <h2>Le catalogue ne répond pas</h2>
      <p>{error}</p>
      <a
        class="secondary"
        href={resolve(
          section === 'series'
            ? `series/?${page.url.searchParams}`
            : section === 'films'
              ? `films/?${page.url.searchParams}`
              : section === 'search'
                ? `search/?${page.url.searchParams}`
                : '/'
        )}
        data-sveltekit-reload
      >
        Réessayer
      </a>
    </div>

  {:else if detail}
    <!-- Complete Media Detail Page (Films & Séries) -->
    <a class="back-link" href={resolve(section === 'series' ? 'series/' : 'films/')}>
      ← {section === 'series' ? 'Toutes les séries' : 'Tous les films'}
    </a>

    <section class="detail">
      <div class="detail-poster">
        <Poster work={detail} showMetadata={false} eager />
      </div>

      <div class="detail-copy">
        <div class="detail-tags">
          <span class="badge kind-tag">{detail.kind === 'series' ? 'SÉRIE' : 'FILM'}</span>
          {#if detail.synthetic}
            <span class="badge demo-tag">DÉMONSTRATION</span>
          {/if}
          {#if detail.year}
            <span class="meta-tag">{detail.year}</span>
          {/if}
          {#if detail.rating}
            <span class="badge rating-tag">⭐ {detail.rating.toFixed(1)} / 10</span>
          {/if}
          {#if detail.mpaa}
            <span class="badge mpaa-tag">{detail.mpaa}</span>
          {/if}
          {#if detail.kind === 'series'}
            <span class="meta-tag">
              {detail.seasonCount ?? 0} {(detail.seasonCount ?? 0) > 1 ? 'saisons' : 'saison'} · {detail.episodeCount ?? 0} {(detail.episodeCount ?? 0) > 1 ? 'épisodes' : 'épisode'}
            </span>
          {/if}
        </div>

        <h1>{detail.title}</h1>

        {#if detail.originalTitle && detail.originalTitle !== detail.title}
          <p class="original-title">Titre original : <em>{detail.originalTitle}</em></p>
        {/if}

        {#if detail.genres?.length}
          <div class="genres-row">
            {#each detail.genres as g (g)}
              <span class="genre-pill">{g}</span>
            {/each}
          </div>
        {/if}

        {#if detail.outline && detail.outline !== detail.description && detail.outline !== detail.plot}
          <blockquote class="outline-hook">« {detail.outline} »</blockquote>
        {/if}

        <p class="synopsis">{detail.plot || detail.description || 'Synopsis non renseigné.'}</p>

        <!-- Offline Node Notice -->
        {#if detail.available === false}
          <div class="node-offline-box" role="status">
            <Icon name="alert" size={20} />
            <div>
              <strong>Média temporairement indisponible</strong>
              <p>Le Media Node propriétaire (<code>{detail.nodeId ?? 'distant'}</code>) est actuellement hors ligne. L’œuvre reste indexée au catalogue et sera immédiatement lisible dès le retour du nœud.</p>
            </div>
          </div>
        {/if}

        <!-- Film Actions: Versions & Playback -->
        {#if detail.kind === 'film'}
          <div class="film-actions">
            {#if playableMedia.length > 1}
              <div class="versions-section">
                <span class="versions-label">Versions disponibles :</span>
                <div class="versions" aria-label="Versions disponibles">
                  {#each playableMedia as item (item.id)}
                    <button
                      class="version-choice"
                      aria-pressed={selected === item.id}
                      aria-label={`Sélectionner la version ${sourceLabel(item)}`}
                      onclick={() => (selected = item.id)}
                    >
                      {sourceLabel(item)}
                    </button>
                  {/each}
                </div>
              </div>
            {/if}

            <div class="play-action-row">
              <button
                class="hero-cta"
                disabled={detail.available === false || (!playableMedia.length && !allowedIds.length)}
                onclick={openPlayer}
              >
                <Icon name="play" size={20} />
                {detail.available === false ? 'Node indisponible' : 'Lancer le film'}
              </button>
              {#if !playableMedia.length && !allowedIds.length}
                <p class="unavailable">Aucune version disponible pour la lecture.</p>
              {/if}
            </div>
          </div>

          <!-- Real Technical Specifications Inspector for Selected Film -->
          {#if currentSelectedMedia}
            <div class="tech-specs-card">
              <h4>Informations techniques réelles (flux analysés)</h4>
              <div class="specs-grid">
                <div>
                  <span class="spec-label">🎬 Vidéo</span>
                  <span class="spec-val">
                    {currentVideoTrack?.codec_name?.toUpperCase() ?? 'Codec natif'}
                    {#if currentVideoTrack?.width && currentVideoTrack?.height}
                      · {currentVideoTrack.width} × {currentVideoTrack.height}
                    {/if}
                    {#if videoResolutionBadge(currentVideoTrack?.width, currentVideoTrack?.height)}
                      <span class="tech-badge">{videoResolutionBadge(currentVideoTrack?.width, currentVideoTrack?.height)}</span>
                    {/if}
                    {#if colorProfileLabel(currentVideoTrack?.pix_fmt)}
                      · {colorProfileLabel(currentVideoTrack?.pix_fmt)}
                    {/if}
                  </span>
                </div>
                <div>
                  <span class="spec-label">🔊 Audio</span>
                  <span class="spec-val">
                    {#if currentAudioTracks.length > 0}
                      {#each currentAudioTracks as at, idx (idx)}
                        <span class="audio-track-spec">
                          {at.codec_name?.toUpperCase() ?? 'Audio'} ({at.channel_layout ?? `${at.channels}ch`})
                        </span>
                      {/each}
                    {:else}
                      Piste audio intégrée
                    {/if}
                  </span>
                </div>
                <div>
                  <span class="spec-label">📦 Conteneur & Langues</span>
                  <span class="spec-val">
                    {containerLabel(currentSelectedMedia.sources[0]?.mime)} ·
                    {currentSelectedMedia.sources.map(s => languageLabel(s.language)).join(', ')}
                  </span>
                </div>
                {#if currentSelectedMedia.subtitles.length > 0}
                  <div>
                    <span class="spec-label">💬 Sous-titres</span>
                    <span class="spec-val">
                      {currentSelectedMedia.subtitles.map(s => s.label).join(' · ')}
                    </span>
                  </div>
                {/if}
              </div>
            </div>
          {/if}
        {/if}

        {#if mediaError}
          <p class="error" role="alert">{mediaError}</p>
        {/if}

        <!-- Metadata Provenance Footer -->
        <div class="metadata-footer">
          <span class="prov-tag">
            Provenance : {detail.metadataProvenance === 'nfo' ? 'Fichier NFO local' : 'Indexation automatique'}
          </span>
          {#if detail.premiered}
            <span class="prov-tag">Date de première diffusion : {detail.premiered}</span>
          {/if}
          <span class="prov-tag">Nœud source : <code>{detail.nodeId ?? 'local'}</code></span>
        </div>

        {#if detail.synthetic}
          <p class="detail-note">Fiche de démonstration synthétique pour la validation du lecteur.</p>
        {/if}
      </div>
    </section>

    <!-- Seasons & Episodes Shelf for Séries -->
    {#if detail.kind === 'series'}
      <section class="shelf series-shelf">
        <div class="section-heading">
          <h2>Épisodes</h2>
          <label class="season-selector-label">
            <span>Sélectionner la saison :</span>
            <select aria-label="Saison" bind:value={season}>
              {#each detail.seasons ?? [] as item (item.id)}
                <option value={item.number}>
                  {item.title || (item.number === 0 ? 'Hors-série' : `Saison ${item.number}`)} ({item.episodes.length} épisodes)
                </option>
              {/each}
            </select>
          </label>
        </div>

        <div class="episodes-list">
          {#each activeSeason?.episodes ?? [] as episode (episode.id)}
            <article class="episode">
              <span class="episode-number">{String(episode.number).padStart(2, '0')}</span>
              <div class="episode-content">
                <div class="episode-title-row">
                  <h3>{episode.title}</h3>
                  {#if episode.available === false}
                    <span class="badge warn-badge">Node indisponible</span>
                  {/if}
                </div>
                <p class="episode-desc">{episode.description || 'Pas de synopsis pour cet épisode.'}</p>
                <EpisodePlayer
                  {episode}
                  {media}
                  {immersive}
                  isLaunching={launchingEpisodeId === episode.id}
                  onlaunch={launchEpisode}
                  onselect={() => (playingEpisode = episode)}
                />
              </div>
            </article>
          {:else}
            <div class="empty">Aucun épisode disponible pour cette saison.</div>
          {/each}
        </div>
      </section>
    {/if}

  {:else}
    <!-- Home & Catalog Listing Views -->
    {#if section === 'home'}
      <section class="home-intro">
        <div>
          <p class="eyebrow">LE CINÉMA, CHEZ VOUS</p>
          <h1>Une envie de cinéma.<br /><em>Une place pour chaque histoire.</em></h1>
          <p class="hero-text">
            Retrouvez vos films préférés et explorez vos séries en lecture directe haute fidélité.<br />
            Sans transcodage forcé, vos médias sont diffusés dans leur format d’origine.
          </p>
          <div class="home-actions">
            <a class="primary" href={resolve('films/')}>Explorer les films <Icon name="arrow" size={16} /></a>
            <a class="secondary" href={resolve('series/')}>Voir les séries</a>
          </div>
        </div>
        <div class="intro-mark" aria-hidden="true">
          a<span>VOTRE MÉDIATHÈQUE</span>
        </div>
      </section>

      <!-- Slot: Reprendre la lecture -->
      <section class="shelf resume-shelf">
        <div class="section-heading">
          <h2>Reprendre la lecture</h2>
        </div>
        <div class="empty compact-slot">
          <div class="slot-icon" aria-hidden="true">⏱️</div>
          <div>
            <strong>Historique et reprise de séance</strong>
            <p>La reprise automatique et l'historique de lecture apparaîtront ici dès que vos séances seront enregistrées.</p>
          </div>
        </div>
      </section>

    {:else}
      <!-- Filter Bar for Films, Series and Search -->
      <div class="page-heading">
        <p class="eyebrow">CATALOGUE AETHER</p>
        <h1>{title}</h1>
        <p>
          {section === 'films'
            ? 'Tous vos films indexés, avec leurs différentes versions physiques et pistes audio.'
            : section === 'series'
              ? 'Toutes vos séries, organisées par saisons et épisodes.'
              : 'Recherchez une œuvre dans l’ensemble de vos bibliothèques.'}
        </p>
      </div>

      <form class="filters" action={`${base}/${section}/`}>
        <div class="search-field">
          <label for="catalog-search">Rechercher dans le catalogue</label>
          <div class="search-input-wrapper">
            <input
              id="catalog-search"
              type="search"
              name="q"
              bind:value={search}
              placeholder="Titre de film ou série…"
              maxlength="200"
            />
            {#if search}
              <button
                type="button"
                class="clear-search-btn"
                aria-label="Effacer la recherche"
                onclick={() => (search = '')}
              >
                ×
              </button>
            {/if}
          </div>
        </div>

        {#if section === 'search'}
          <div class="kind-filter-group">
            <label for="kind-select">Type</label>
            <select id="kind-select" bind:value={selectedKindFilter}>
              <option value="all">Tous les médias</option>
              <option value="film">Films uniquement</option>
              <option value="series">Séries uniquement</option>
            </select>
          </div>
        {/if}

        <div class="sort-field">
          <label for="catalog-sort">Trier par</label>
          <select id="catalog-sort" name="sort" bind:value={sort}>
            <option value="title-asc">Titre · A à Z</option>
            <option value="title-desc">Titre · Z à A</option>
          </select>
        </div>

        <button class="secondary submit-btn" type="submit">
          Appliquer
        </button>
      </form>
    {/if}

    <!-- Films Shelf -->
    {#if section !== 'series' && (selectedKindFilter === 'all' || selectedKindFilter === 'film')}
      <section class="shelf">
        <div class="section-heading">
          <h2>
            {section === 'home'
              ? 'Films récemment ajoutés'
              : section === 'search'
                ? 'Films correspondants'
                : 'Tous les films'}
          </h2>
          {#if section === 'home'}
            <a href={resolve('films/')}>Tout voir →</a>
          {:else}
            <span class="count-badge">{totalFilms} film{totalFilms > 1 ? 's' : ''}</span>
          {/if}
        </div>

        <div class="media-grid">
          {#each films as work (work.id)}
            <Poster {work} href={detailUrl(work)} />
          {:else}
            <div class="empty">
              {query ? 'Aucun film ne correspond à votre recherche.' : 'Aucun film dans cette bibliothèque.'}
            </div>
          {/each}
        </div>
      </section>
    {/if}

    <!-- Series Shelf -->
    {#if section !== 'films' && (selectedKindFilter === 'all' || selectedKindFilter === 'series')}
      <section class="shelf">
        <div class="section-heading">
          <h2>
            {section === 'home'
              ? 'Séries récemment ajoutées'
              : section === 'search'
                ? 'Séries correspondantes'
                : 'Toutes les séries'}
          </h2>
          {#if section === 'home'}
            <a href={resolve('series/')}>Tout voir →</a>
          {:else}
            <span class="count-badge">{totalSeries} série{totalSeries > 1 ? 's' : ''}</span>
          {/if}
        </div>

        <div class="media-grid">
          {#each series as work (work.id)}
            <Poster {work} href={detailUrl(work)} />
          {:else}
            <div class="empty">
              {query ? 'Aucune série ne correspond à votre recherche.' : 'Aucune série dans cette bibliothèque.'}
            </div>
          {/each}
        </div>
      </section>
    {/if}

    <!-- Prepared Slots on Home -->
    {#if section === 'home'}
      <section class="shelf">
        <div class="section-heading">
          <h2>Continuer les séries en cours</h2>
        </div>
        <div class="empty compact-slot">
          <div class="slot-icon" aria-hidden="true">🎬</div>
          <div>
            <strong>Progression des épisodes</strong>
            <p>Vos prochains épisodes à regarder apparaîtront automatiquement ici au fur et à mesure de votre progression.</p>
          </div>
        </div>
      </section>

      <section class="shelf">
        <div class="section-heading">
          <h2>Favoris & Listes d’envies</h2>
        </div>
        <div class="empty compact-slot">
          <div class="slot-icon" aria-hidden="true">⭐</div>
          <div>
            <strong>Votre sélection personnalisée</strong>
            <p>Enregistrez vos coups de cœur et composez votre liste de visionnage pour vos futures séances.</p>
          </div>
        </div>
      </section>
    {/if}

    <!-- Pagination -->
    {#if section !== 'home' && (offset > 0 || totalFilms > offset + 24 || totalSeries > offset + 24)}
      <nav class="pagination" aria-label="Pages du catalogue">
        {#if offset > 0}
          <a
            class="secondary"
            href={resolve(
              `${section === 'films' ? 'films/' : section === 'series' ? 'series/' : 'search/'}?${new URLSearchParams({
                q: query,
                sort: order,
                offset: String(Math.max(0, offset - 24))
              })}`
            )}
          >
            ← Page précédente
          </a>
        {/if}
        <span class="page-indicator">Page {Math.floor(offset / 24) + 1}</span>
        {#if totalFilms > offset + 24 || totalSeries > offset + 24}
          <a
            class="secondary"
            href={resolve(
              `${section === 'films' ? 'films/' : section === 'series' ? 'series/' : 'search/'}?${new URLSearchParams({
                q: query,
                sort: order,
                offset: String(offset + 24)
              })}`
            )}
          >
            Page suivante →
          </a>
        {/if}
      </nav>
    {/if}
  {/if}
</main>

<footer>
  <span>{name} <span class="footer-dot">·</span> Votre cinéma, en direct de vos serveurs.</span>
  <p>
    {detail?.synthetic
      ? 'Fiche de démonstration · médias de test synthétiques.'
      : catalogueNotice || 'Bibliothèques locales indexées · Fichiers médias en lecture seule.'}
  </p>
</footer>

{#if detail && ImmersivePlayerComponent && createPlayer}
  <ImmersivePlayerComponent bind:this={immersive} catalogue={playerMedia} {createPlayer} />
{/if}

<style>
  .skip-link {
    position: fixed;
    top: -80px;
    left: 20px;
    z-index: 50;
    background: var(--color-text);
    color: var(--color-canvas);
    padding: 14px;
  }
  .skip-link:focus {
    top: 10px;
  }

  .site-header {
    display: flex;
    align-items: center;
    gap: 60px;
    padding: 0 var(--page-gutter);
    min-height: 88px;
    border-bottom: 1px solid var(--color-line);
  }
  .brand {
    display: flex;
    gap: 9px;
    align-items: center;
  }
  .brand span {
    font-size: 30px;
    letter-spacing: -0.065em;
    font-weight: 650;
  }

  nav {
    display: flex;
    gap: 30px;
  }
  nav a {
    display: flex;
    align-items: center;
    gap: 7px;
    font-size: 13px;
    color: var(--color-muted);
    padding: 32px 0;
    border-bottom: 2px solid transparent;
  }
  nav a[aria-current='page'] {
    color: var(--color-text);
    border-color: var(--color-accent);
  }
  nav a:hover {
    color: white;
  }
  .nav-symbol {
    display: none;
  }
  .search-icon {
    display: inline-flex;
    align-items: center;
    justify-content: center;
  }

  main {
    max-width: 1600px;
    margin: auto;
    padding: 0 var(--page-gutter) 80px;
    min-height: 70vh;
  }

  h1 {
    font-size: clamp(34px, 4.5vw, 64px);
    line-height: 1.12;
    letter-spacing: -0.055em;
    font-weight: 500;
    margin: 16px 0 22px;
  }
  h2 {
    font-size: clamp(21px, 2vw, 28px);
    font-weight: 500;
    letter-spacing: -0.035em;
    margin: 0;
  }
  h3 {
    font-size: 16px;
    font-weight: 600;
  }
  h4 {
    font-size: 14px;
    font-weight: 600;
    margin: 0 0 12px;
  }
  p {
    line-height: 1.8;
    color: var(--color-muted);
  }

  .page-heading {
    padding: 50px 0 24px;
  }
  .page-heading p {
    margin: 0;
  }

  .home-intro {
    display: flex;
    justify-content: space-between;
    align-items: center;
    gap: 30px;
    padding: 70px 0 60px;
    border-bottom: 1px solid var(--color-line);
  }
  .home-intro h1 {
    font-size: clamp(36px, 4.5vw, 66px);
  }
  .home-intro h1 em {
    color: #c9b8ef;
    font-family: Georgia, serif;
    font-weight: 400;
  }
  .hero-text {
    font-size: 14px;
    max-width: 680px;
  }
  .intro-mark {
    color: #b4a1f83b;
    font-size: 180px;
    letter-spacing: -0.12em;
    padding: 0 50px;
    font-family: Georgia, serif;
  }
  .intro-mark span {
    display: block;
    font-family: var(--font-sans);
    font-size: 8px;
    color: var(--color-subtle);
    letter-spacing: 0.2em;
    white-space: nowrap;
  }

  .home-actions {
    display: flex;
    gap: 14px;
    margin-top: 28px;
    flex-wrap: wrap;
  }

  .primary, .hero-cta, .secondary {
    display: inline-flex;
    gap: 10px;
    align-items: center;
    justify-content: center;
    padding: 13px 22px;
    border-radius: 6px;
    font-size: 13px;
    min-height: 46px;
    border: 1px solid var(--color-line);
  }
  .primary, .hero-cta {
    background: var(--color-text);
    color: #16121d;
    font-weight: 600;
  }
  .primary:hover:not(:disabled), .hero-cta:hover:not(:disabled) {
    background: white;
    transform: translateY(-2px);
  }
  .secondary {
    background: var(--color-surface);
    color: var(--color-text);
  }
  .secondary:hover:not(:disabled) {
    border-color: var(--color-accent);
  }

  .shelf {
    margin-top: 48px;
  }
  .section-heading {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 16px;
    margin-bottom: 22px;
  }
  .section-heading > a {
    font-size: 13px;
    color: var(--color-accent);
  }
  .count-badge {
    font-size: 12px;
    color: var(--color-muted);
    background: var(--color-surface);
    padding: 4px 10px;
    border-radius: 99px;
    border: 1px solid var(--color-line);
  }

  .compact-slot {
    display: flex;
    align-items: center;
    gap: 18px;
    padding: 20px 24px;
    border-radius: var(--radius-md);
    background: var(--color-surface);
    border: 1px solid var(--color-line);
  }
  .slot-icon {
    font-size: 28px;
  }
  .compact-slot strong {
    display: block;
    color: var(--color-text);
    font-size: 14px;
    margin-bottom: 4px;
  }
  .compact-slot p {
    margin: 0;
    font-size: 13px;
    color: var(--color-muted);
  }

  .media-grid {
    display: grid;
    grid-template-columns: repeat(6, minmax(0, 1fr));
    gap: 22px;
  }
  .empty {
    grid-column: 1 / -1;
    padding: 36px;
    background: var(--color-surface);
    border: 1px solid var(--color-line);
    border-radius: 10px;
    color: var(--color-muted);
    font-size: 13px;
    line-height: 1.8;
  }
  .loading-empty {
    display: flex;
    align-items: center;
    gap: 14px;
  }

  /* Filters */
  .filters {
    display: flex;
    gap: 18px;
    align-items: flex-end;
    flex-wrap: wrap;
    padding: 22px;
    background: var(--color-surface);
    border: 1px solid var(--color-line);
    border-radius: var(--radius-md);
    margin-bottom: 32px;
  }
  .search-field {
    flex: 2;
    min-width: 220px;
  }
  .search-input-wrapper {
    position: relative;
    display: flex;
    align-items: center;
  }
  .clear-search-btn {
    position: absolute;
    right: 12px;
    background: transparent;
    border: 0;
    color: var(--color-muted);
    font-size: 18px;
    cursor: pointer;
    padding: 4px;
  }
  .kind-filter-group {
    flex: 1;
    min-width: 160px;
  }
  .sort-field {
    flex: 1;
    min-width: 160px;
  }
  .submit-btn {
    align-self: flex-end;
  }

  label {
    display: grid;
    gap: 8px;
    font-size: 12px;
    color: var(--color-muted);
  }
  input[type='search'], select {
    background: var(--color-raised);
    color: var(--color-text);
    border: 1px solid var(--color-line);
    border-radius: 6px;
    min-height: 46px;
    padding: 10px 14px;
    width: 100%;
    font-size: 13px;
  }

  .pagination {
    margin-top: 40px;
    display: flex;
    align-items: center;
    justify-content: center;
    gap: 16px;
  }
  .page-indicator {
    font-size: 13px;
    color: var(--color-muted);
  }

  /* Media Detail Page */
  .back-link {
    display: inline-block;
    margin: 28px 0 16px;
    color: var(--color-muted);
    font-size: 13px;
  }
  .back-link:hover {
    color: var(--color-text);
  }

  .detail {
    display: grid;
    grid-template-columns: 260px minmax(0, 1fr);
    gap: 50px;
    padding: 24px 0 48px;
    border-bottom: 1px solid var(--color-line);
  }
  .detail-poster {
    width: 100%;
  }
  .detail-copy {
    align-self: start;
    max-width: 860px;
  }
  .detail-tags {
    display: flex;
    align-items: center;
    gap: 8px;
    flex-wrap: wrap;
    margin-bottom: 12px;
  }
  .badge {
    font-size: 11px;
    font-weight: 600;
    padding: 3px 8px;
    border-radius: 4px;
    border: 1px solid var(--color-line);
  }
  .kind-tag {
    background: var(--color-accent-soft);
    color: var(--color-accent);
    border-color: #b4a1f833;
  }
  .demo-tag {
    background: #2b200e;
    color: #f5c97b;
    border-color: #75541e;
  }
  .rating-tag {
    background: #14281c;
    color: #9be7b4;
    border-color: #2e6b44;
  }
  .mpaa-tag {
    background: var(--color-raised);
    color: #d1cadf;
  }
  .meta-tag {
    font-size: 13px;
    color: var(--color-muted);
    padding: 0 4px;
  }

  .original-title {
    font-size: 14px;
    color: var(--color-muted);
    margin: -8px 0 16px;
  }

  .genres-row {
    display: flex;
    gap: 8px;
    flex-wrap: wrap;
    margin-bottom: 18px;
  }
  .genre-pill {
    font-size: 12px;
    background: var(--color-surface);
    border: 1px solid var(--color-line);
    padding: 4px 12px;
    border-radius: 99px;
    color: #ded9e8;
  }

  .outline-hook {
    margin: 0 0 16px;
    padding: 12px 18px;
    border-left: 3px solid var(--color-accent);
    background: var(--color-accent-soft);
    font-size: 14px;
    font-style: italic;
    color: #e5def5;
    border-radius: 0 6px 6px 0;
  }

  .synopsis {
    font-size: 15px;
    line-height: 1.8;
    color: #d1cbdc;
    margin-bottom: 24px;
  }

  .node-offline-box {
    display: flex;
    gap: 14px;
    align-items: flex-start;
    padding: 16px 20px;
    background: #2b1f11;
    border: 1px solid #75541e;
    border-radius: var(--radius-sm);
    color: #f5c97b;
    margin-bottom: 24px;
    font-size: 13px;
  }
  .node-offline-box strong {
    display: block;
    margin-bottom: 4px;
  }
  .node-offline-box p {
    margin: 0;
    color: #f5c97bcc;
  }

  .film-actions {
    margin: 28px 0;
  }
  .versions-section {
    margin-bottom: 16px;
  }
  .versions-label {
    display: block;
    font-size: 12px;
    color: var(--color-muted);
    margin-bottom: 8px;
  }
  .versions {
    display: flex;
    gap: 10px;
    flex-wrap: wrap;
  }
  .version-choice {
    background: var(--color-surface);
    color: var(--color-muted);
    border: 1px solid var(--color-line);
    border-radius: 6px;
    min-height: 40px;
    padding: 8px 18px;
    font-size: 13px;
    cursor: pointer;
  }
  .version-choice[aria-pressed='true'] {
    color: var(--color-accent);
    border-color: var(--color-accent);
    background: var(--color-accent-soft);
    font-weight: 600;
  }
  .play-action-row {
    display: flex;
    align-items: center;
    gap: 16px;
    margin-top: 14px;
  }

  /* Technical Specs Inspector */
  .tech-specs-card {
    background: var(--color-surface);
    border: 1px solid var(--color-line);
    border-radius: var(--radius-md);
    padding: 20px 24px;
    margin: 28px 0;
  }
  .specs-grid {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(220px, 1fr));
    gap: 16px;
    font-size: 13px;
  }
  .spec-label {
    display: block;
    font-size: 11px;
    font-weight: 600;
    color: var(--color-muted);
    text-transform: uppercase;
    letter-spacing: 0.05em;
    margin-bottom: 4px;
  }
  .spec-val {
    color: var(--color-text);
    line-height: 1.5;
  }
  .tech-badge {
    background: var(--color-accent-soft);
    color: var(--color-accent);
    font-size: 10px;
    padding: 2px 6px;
    border-radius: 4px;
    margin-left: 4px;
  }
  .audio-track-spec {
    display: inline-block;
    margin-right: 8px;
  }

  .metadata-footer {
    display: flex;
    gap: 16px;
    flex-wrap: wrap;
    margin-top: 24px;
    padding-top: 16px;
    border-top: 1px solid var(--color-line);
    font-size: 12px;
    color: var(--color-subtle);
  }

  /* Séries Episodes Shelf */
  .series-shelf {
    margin-top: 40px;
  }
  .season-selector-label {
    display: flex;
    align-items: center;
    gap: 12px;
  }
  .season-selector-label select {
    min-width: 200px;
  }
  .episodes-list {
    display: grid;
    gap: 16px;
  }
  .episode {
    display: flex;
    gap: 24px;
    padding: 24px;
    background: var(--color-surface);
    border: 1px solid var(--color-line);
    border-radius: var(--radius-md);
  }
  .episode-number {
    color: var(--color-accent);
    font-size: 32px;
    font-weight: 300;
    line-height: 1;
    margin-top: 6px;
  }
  .episode-content {
    flex: 1;
    min-width: 0;
  }
  .episode-title-row {
    display: flex;
    align-items: center;
    gap: 12px;
    margin-bottom: 6px;
  }
  .episode-title-row h3 {
    margin: 0;
    font-size: 17px;
  }
  .episode-desc {
    font-size: 13px;
    margin-bottom: 16px;
  }

  /* Profile & Settings Page */
  .settings-grid {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(340px, 1fr));
    gap: 28px;
    margin-top: 20px;
  }
  .card {
    background: var(--color-surface);
    border: 1px solid var(--color-line);
    border-radius: var(--radius-md);
    padding: 28px;
  }
  .card-title {
    font-size: 18px;
    font-weight: 600;
    margin: 0 0 20px;
    padding-bottom: 10px;
    border-bottom: 1px solid var(--color-line);
  }
  .profile-header-row {
    display: flex;
    align-items: center;
    gap: 16px;
    margin-bottom: 24px;
  }
  .profile-avatar-large {
    width: 60px;
    height: 60px;
    border-radius: 50%;
    background: var(--color-accent-soft);
    border: 1px solid #b4a1f84d;
    color: var(--color-accent);
    display: flex;
    align-items: center;
    justify-content: center;
  }
  .profile-info-list {
    display: grid;
    gap: 16px;
    font-size: 13px;
  }
  .profile-info-list .label {
    display: block;
    color: var(--color-muted);
    font-size: 11px;
    text-transform: uppercase;
    letter-spacing: 0.05em;
    margin-bottom: 3px;
  }
  .profile-info-list .val {
    color: var(--color-text);
  }
  .admin-shortcut {
    margin-top: 24px;
    padding-top: 16px;
    border-top: 1px solid var(--color-line);
  }
  .admin-link {
    width: 100%;
  }

  .pref-fields {
    display: grid;
    gap: 18px;
    margin: 20px 0;
  }
  .pref-note {
    font-size: 12px;
    color: #9be7b4;
    margin: 12px 0 0;
  }

  footer {
    border-top: 1px solid var(--color-line);
    padding: 26px var(--page-gutter);
    display: flex;
    justify-content: space-between;
    align-items: center;
    gap: 24px;
    font-size: 12px;
    color: #c4becf;
  }
  footer p {
    margin: 0;
    text-align: right;
    font-size: 11px;
  }
  .footer-dot {
    color: var(--color-accent);
    padding: 0 8px;
  }

  .spinner {
    width: 20px;
    height: 20px;
    border: 2px solid var(--color-line);
    border-top-color: var(--color-accent);
    border-radius: 50%;
    animation: spin 0.8s linear infinite;
  }
  @keyframes spin {
    to { transform: rotate(360deg); }
  }

  @media(max-width: 1100px) {
    .site-header { gap: 32px; }
    nav { gap: 22px; }
    .media-grid { grid-template-columns: repeat(4, minmax(0, 1fr)); }
    .intro-mark { padding: 0 10px; font-size: 140px; }
  }
  @media(max-width: 800px) {
    .detail {
      grid-template-columns: 1fr;
      gap: 24px;
    }
    .detail-poster {
      width: 160px;
    }
  }
  @media(max-width: 700px) {
    .site-header { min-height: 70px; }
    .site-header nav {
      position: fixed;
      bottom: 0;
      left: 0;
      right: 0;
      z-index: 10;
      background: #101017;
      border-top: 1px solid var(--color-line);
      justify-content: space-around;
      gap: 0;
      padding: 0 6px env(safe-area-inset-bottom);
    }
    .site-header nav a {
      padding: 10px 2px;
      display: flex;
      flex-direction: column;
      align-items: center;
      gap: 4px;
      font-size: 10px;
      min-width: 56px;
      border-bottom: 0;
      border-top: 2px solid transparent;
    }
    .site-header nav a[aria-current='page'] {
      border-top-color: var(--color-accent);
      color: var(--color-accent);
    }
    .nav-symbol { display: block; font-size: 21px; height: 24px; }
    .search-icon { height: 24px; }
    .home-intro { padding: 44px 0 36px; }
    .intro-mark { display: none; }
    .media-grid { grid-template-columns: repeat(2, minmax(0, 1fr)); gap: 20px 14px; }
    .home-actions { flex-wrap: wrap; }
    .shelf { margin-top: 32px; }
    .page-heading { padding-top: 36px; }
    .filters { gap: 14px; padding: 16px; }
    .search-field { width: 100%; flex-basis: 100%; }
    .episode { padding: 18px; gap: 16px; }
    footer {
      flex-direction: column;
      align-items: flex-start;
      padding-bottom: calc(110px + env(safe-area-inset-bottom, 0px));
    }
    footer p { text-align: left; }
    .section-heading { align-items: start; }
    main {
      padding-bottom: calc(64px + env(safe-area-inset-bottom, 0px));
      scroll-padding-bottom: calc(84px + env(safe-area-inset-bottom, 0px));
    }
  }
</style>
