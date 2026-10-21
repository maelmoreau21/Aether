<script lang="ts">
  import { onMount, tick } from 'svelte';
  import { serverPresentation } from '../config/presentation';
  import { preferences, savePreferences } from '../preferences';
  import { page } from '$app/state';
  import { resolve } from '$app/paths';
  import { DEFAULT_NAME, fetchBrandName } from '../config/brand';
  import type ImmersivePlayer from '../player/ImmersivePlayer.svelte';
  import type { Media, PlayerFactory } from '../player/types';
  import Navbar from '../ui/Navbar.svelte';
  import CatalogFilters from './CatalogFilters.svelte';
  import { bestVersion } from '../player/version-choice';
  import CatalogHome from './CatalogHome.svelte';
  import CatalogGrid from './CatalogGrid.svelte';
  import FilmDetail from './FilmDetail.svelte';
  import SeriesDetail from './SeriesDetail.svelte';
  import SettingsView from './SettingsView.svelte';
  import Icon from '../ui/Icon.svelte';
  import {
    request,
    playbackIds,
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
  let busy = $state(true);
  let pendingCatalogKinds = $state(0);
  let partialCatalogError = $state('');
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
  let playerLoading: Promise<void> | undefined;
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
      const [, item] = await Promise.all([loadPlayer(), revalidateMediaDescriptor(versionId, controller.signal)]);
      if (controller.signal.aborted || launchGen !== getSessionGeneration()) return;
      const cachedForEpisode = playbackIds(episode)
        .map(eid => (eid === item.id ? item : getCachedMedia(eid)))
        .filter((m): m is Media => m !== undefined);
      const otherMedia = media.filter(m => !cachedForEpisode.some(c => c.id === m.id));
      media = [...otherMedia, ...cachedForEpisode];
      playingEpisode = episode;
      await tick();
      if (controller.signal.aborted || launchGen !== getSessionGeneration()) return;
      immersive?.changeMedia(item);
    } catch (err) {
      if (!controller.signal.aborted && launchGen === getSessionGeneration()) {
        media = media.filter(m => m.id !== versionId && getCachedMedia(m.id) !== undefined);
        mediaError = err instanceof Error ? err.message : 'Chargement de l’épisode impossible.';
        immersive?.reportError(mediaError);
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
    if (playerLoading) return playerLoading;
    playerLoading = (async () => {
      const [{ default: comp }, { NativePlayer }] = await Promise.all([
        import('../player/ImmersivePlayer.svelte'),
        import('../player/native')
      ]);
      ImmersivePlayerComponent = comp;
      createPlayer = (video, onError) => new NativePlayer(video, onError);
    })();
    try { await playerLoading; }
    finally { playerLoading = undefined; }
  }

  const workId = $derived(page.url.searchParams.get('work') ?? '');
  const query = $derived(page.url.searchParams.get('q') ?? '');
  const order = $derived(
    ['title-asc', 'title-desc', 'year-asc', 'year-desc', 'added-asc', 'added-desc'].includes(
      page.url.searchParams.get('sort') ?? ''
    )
      ? page.url.searchParams.get('sort')!
      : section === 'home'
        ? 'added-desc'
        : 'title-asc'
  );
  const offset = $derived(Math.max(0, Number(page.url.searchParams.get('offset')) || 0));
  const title = $derived(
    section === 'settings'
      ? 'Paramètres'
      : section === 'films'
        ? 'Films'
        : section === 'series'
          ? 'Séries'
          : section === 'search'
            ? 'Recherche'
            : 'Accueil'
  );
  const allowedIds = $derived(detail ? playbackIds(detail) : []);
  const playableMedia = $derived(media.filter(item => allowedIds.includes(item.id)));
  const episodeIds = $derived(playingEpisode ? playbackIds(playingEpisode) : []);
  const playerMedia = $derived(detail?.kind === 'series' ? media.filter(item => episodeIds.includes(item.id)) : playableMedia);

  const nextEpisode = $derived.by(() => {
    if (!playingEpisode || detail?.kind !== 'series') return undefined;
    const episodes = (detail.seasons ?? []).flatMap(s => s.episodes);
    const index = episodes.findIndex(episode => episode.id === playingEpisode?.id);
    return index >= 0 ? episodes[index + 1] : undefined;
  });

  function playNext() {
    if (!nextEpisode) return;
    const next = nextEpisode;
    const id = playbackIds(next)[0];
    if (!id) {
      mediaError = 'Épisode suivant sans source disponible.';
      return;
    }
    void launchEpisode(next, id);
  }

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
      const [, item] = await Promise.all([loadPlayer(), revalidateMediaDescriptor(targetId, controller.signal)]);
      if (controller.signal.aborted || launchGen !== getSessionGeneration()) return;
      media = [...media.filter(m => m.id !== item.id), item];
      selected = item.id;
      await tick();
      if (controller.signal.aborted || launchGen !== getSessionGeneration()) return;
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

  async function handleDirectPlay(work: Work): Promise<void> {
    if (work.kind === 'series') {
      window.location.href = detailUrl(work);
      return;
    }
    const ids = playbackIds(work);
    const targetId = ids[0];
    if (!targetId) {
      window.location.href = detailUrl(work);
      return;
    }
    try {
      const [, item] = await Promise.all([loadPlayer(), revalidateMediaDescriptor(targetId)]);
      detail = work;
      media = [item];
      selected = item.id;
      await tick();
      immersive?.open(item);
    } catch {
      window.location.href = detailUrl(work);
    }
  }

  onMount(() => {
    const controller = new AbortController();
    fetchBrandName(controller.signal)
      .then(brandName => {
        name = brandName;
      })
      .catch(() => {
        if (!controller.signal.aborted) {
          mediaError = 'La lecture est temporairement indisponible. Rechargez la page pour réessayer.';
        }
      });

    try {
      const savedAudio = preferences().audioLanguage;
      if (savedAudio) prefAudio = savedAudio;
      const savedSub = preferences().subtitles;
      if (savedSub) prefSubtitles = savedSub;
    } catch {
      // localStorage may be disabled in private browsing
    }

    return () => controller.abort();
  });

  function savePreference(key: string, value: string) {
    if (key === 'media_pref_audio') savePreferences({ audioLanguage: value });
    if (key === 'media_pref_subtitles' && (value === 'forced' || value === 'always' || value === 'off')) savePreferences({ subtitles: value });
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
    const filters=Object.fromEntries(['genre','year','quality','codec','language'].flatMap(key=>{const value=page.url.searchParams.get(key);return value?[[key,value]]:[];}));
    const currentSection = section;
    const activeSessionKey = $session ? `${$session.user.id}:${$session.csrfToken}` : 'anonymous';
    void activeSessionKey;
    const effectSessionGen = getSessionGeneration();
    const controller = new AbortController();

    search = q;
    sort = sorting;
    busy = true;
    pendingCatalogKinds = 0;
    partialCatalogError = '';
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
          void loadPlayer().catch(() => { /* A playback attempt reports the import error and can retry. */ });
          const work = await request<Work>(`/api/v1/catalog/${encodeURIComponent(id)}`, controller.signal);
          if (controller.signal.aborted || effectSessionGen !== getSessionGeneration()) return;
          if (
            (currentSection === 'films' && work.kind !== 'film') ||
            (currentSection === 'series' && work.kind !== 'series')
          ) {
            throw new Error('Ce média appartient à une autre bibliothèque.');
          }
          detail = work;
          busy = false;
          season = work.seasons?.[0]?.number ?? 1;

          if (work.kind === 'film') {
            const ids = playbackIds(work);
            selected = ids[0] ?? '';
            try {
              const items = await fetchMediaDescriptors(ids, controller.signal);
              if (!controller.signal.aborted && effectSessionGen === getSessionGeneration()) {
                media = items;
                const probe=document.createElement('video');
                selected=bestVersion(items,mime=>probe.canPlayType(mime))?.id??selected;
              }
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
          pendingCatalogKinds = kinds.length;
          let completedKinds = 0;
          const results = await Promise.allSettled(
            kinds.map(async kind => {
              try {
                const result = await request<CatalogPage>(
                  `/api/v1/catalog?${new URLSearchParams({
                    ...filters,
                    kind,
                    q,
                    sort: sorting,
                    offset: String(start),
                    limit: '24'
                  })}`,
                  controller.signal
                );
                if (controller.signal.aborted || effectSessionGen !== getSessionGeneration()) return;
                if (kind === 'film') {
                  films = result.items;
                  totalFilms = result.total;
                } else {
                  series = result.items;
                  totalSeries = result.total;
                }
                completedKinds += 1;
                // Render the ready shelf and start its images immediately.
                busy = false;
              } finally {
                if (!controller.signal.aborted && effectSessionGen === getSessionGeneration()) pendingCatalogKinds -= 1;
              }
            })
          );
          if (controller.signal.aborted || effectSessionGen !== getSessionGeneration()) return;
          const failure = results.find(result => result.status === 'rejected');
          if (failure?.status === 'rejected') {
            if (completedKinds === 0) throw failure.reason;
            partialCatalogError = 'Une partie du catalogue est indisponible. Rechargez la page pour réessayer.';
          }
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

<Navbar {section} {name} />
{#if !workId && section!=='settings'}<CatalogFilters />{/if}

<main id="main-content" tabindex="-1">
  {#if !busy && pendingCatalogKinds > 0}<p role="status">Chargement du reste du catalogue…</p>{/if}
  {#if partialCatalogError}<p role="alert">{partialCatalogError}</p>{/if}
  {#if section === 'home' && $serverPresentation.welcomeMessage}<p class="server-welcome">{$serverPresentation.welcomeMessage}</p>{/if}
  {#if section === 'settings'}
    <SettingsView
      bind:prefAudio
      bind:prefSubtitles
      onSavePreference={savePreference}
    />

  {:else if busy}
    <div class="page-heading">
      <span class="eyebrow">{title}</span>
      <h1>Votre médiathèque</h1>
    </div>
    <div class="empty loading-empty" role="status">
      <img
        src={`${resolve('/').replace(/\/$/, '')}/images/logo-loading.svg`}
        alt=""
        class="logo-spinner"
        width="48"
        height="48"
      />
      <span>Chargement du catalogue…</span>
    </div>

  {:else if error}
    <div class="page-heading">
      <h1>{title}</h1>
    </div>
    <div class="empty error" role="alert">
      <Icon name="alert" size={32} />
      <h2>Le catalogue ne répond pas</h2>
      <p>{error}</p>
      <a
        class="retry-action-btn"
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
    {#if detail.kind === 'film'}
      <FilmDetail
        work={detail}
        {playableMedia}
        {allowedIds}
        bind:selected
        {mediaError}
        onPlay={openPlayer}
      />
    {:else}
      <SeriesDetail
        work={detail}
        bind:season
        {media}
        {mediaError}
        {immersive}
        {launchingEpisodeId}
        onLaunchEpisode={launchEpisode}
        onSelectEpisode={episode => (playingEpisode = episode)}
      />
    {/if}

  {:else if section === 'home'}
    <!-- Refined, Immersive Home View -->
    <CatalogHome
      {films}
      {series}
      {totalFilms}
      {totalSeries}
      featured={films[0] ?? series[0] ?? null}
      onPlay={handleDirectPlay}
      {detailUrl}
    />

  {:else}
    <!-- Films, Series and Search Grid View -->
    <CatalogGrid
      {section}
      {films}
      {series}
      {totalFilms}
      {totalSeries}
      bind:search
      bind:sort
      bind:selectedKindFilter
      {query}
      {offset}
      {title}
      {detailUrl}
    />
  {/if}
</main>

{#if detail && ImmersivePlayerComponent && createPlayer}
  <ImmersivePlayerComponent
    bind:this={immersive}
    catalogue={playerMedia}
    {createPlayer}
    nextTitle={nextEpisode?.title}
    onNext={nextEpisode ? playNext : undefined}
  />
{/if}

<style>
  .server-welcome { color: var(--text-secondary); white-space: pre-wrap; overflow-wrap: anywhere; padding-block: 16px; }
  .skip-link {
    position: fixed;
    top: -80px;
    left: 20px;
    z-index: 50;
    background: var(--color-text-bright);
    color: var(--color-canvas);
    padding: 12px 18px;
    border-radius: var(--radius-sm);
    font-weight: 700;
    transition: top var(--transition-fast);
  }

  .skip-link:focus {
    top: 14px;
  }

  main {
    max-width: 1600px;
    margin: 0 auto;
    padding: 0 var(--page-gutter) 80px;
    min-height: 75vh;
  }

  .page-heading {
    display: flex;
    flex-direction: column;
    gap: 8px;
    padding-top: 16px;
  }

  h1 {
    font-family: var(--font-display);
    font-size: clamp(32px, 4vw, 52px);
    font-weight: 750;
    line-height: 1.15;
    letter-spacing: -0.04em;
    color: var(--color-text-bright);
    margin: 8px 0 16px;
  }

  /* Empty / Loading States */
  .empty {
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 16px;
    padding: 80px 24px;
    border-radius: var(--radius-lg);
    background: var(--color-surface);
    border: 1px solid var(--color-line);
    text-align: center;
    margin: 32px 0;
  }

  .loading-empty {
    color: var(--color-muted);
    font-size: 14.5px;
  }

  .logo-spinner {
    width: 48px;
    height: 48px;
    display: block;
    margin: 0 auto 12px;
  }

  .error {
    border-color: rgba(248, 113, 113, 0.3);
    color: var(--color-danger);
  }

  .error h2 {
    font-size: 20px;
    font-weight: 700;
    margin: 0;
    color: var(--color-text-bright);
  }

  .error p {
    margin: 0;
    color: var(--color-muted);
    font-size: 14px;
    max-width: 500px;
  }

  .retry-action-btn {
    display: inline-flex;
    align-items: center;
    padding: 10px 22px;
    border-radius: var(--radius-sm);
    background: var(--color-raised);
    border: 1px solid var(--color-line);
    color: var(--color-text-bright);
    font-size: 13px;
    font-weight: 600;
    margin-top: 8px;
    transition: all var(--transition-fast);
  }

  .retry-action-btn:hover {
    background: var(--color-accent-soft);
    border-color: var(--color-accent);
    color: var(--color-accent-hover);
    transform: translateY(-1px);
  }

</style>
