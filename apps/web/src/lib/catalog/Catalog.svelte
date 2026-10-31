<script lang="ts">
  import { onMount, tick } from 'svelte';
  import { loadArtworkRotationPolicy } from './artwork-rotation-policy.svelte';
  import { serverPresentation } from '../config/presentation';
  import { ensurePreferences, savePreferences } from '../preferences';
  import { page } from '$app/state';
  import { goto, beforeNavigate, afterNavigate } from '$app/navigation';
  import { cachedCatalog, loadCatalog, invalidateCatalog, catalogRevision, saveCatalogScroll, catalogScroll } from './page-cache';
  import { resolve } from '$app/paths';
  import { DEFAULT_NAME, fetchBrandName } from '../config/brand';
  import type ImmersivePlayer from '../player/ImmersivePlayer.svelte';
  import type { Media, PlayerFactory } from '../player/types';
  import { accessibleLibraries } from './libraries';
  import Navbar from '../ui/Navbar.svelte';
  import { bestVersion } from '../player/version-choice';
  import CatalogHome from './CatalogHome.svelte';
  import CatalogGrid from './CatalogGrid.svelte';
  import CatalogSkeleton from './CatalogSkeleton.svelte';
  import Icon from '../ui/Icon.svelte';
  import {
    request,
    playbackIds,
    fetchMediaDescriptors,
    revalidateMediaDescriptor,
    getCachedMedia,
    type Work
  } from './api';
  import { preloadProgress } from '../player/progress';
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
  let totalFilms = $state<number | null>(null);
  let totalSeries = $state<number | null>(null);
  let moreFilms = $state(false);
  let moreSeries = $state(false);
  let detail = $state<Work | null>(null);
  let selected = $state('');
  let playingEpisode = $state<Work | null>(null);
  let followingEpisode = $state<Work | undefined>();
  let season = $state(1);
  let search = $state('');
  let sort = $state('title-asc');
  let selectedKindFilter = $state<'all' | 'film' | 'series'>('all');
  let immersive = $state<ImmersivePlayer>();
  let ImmersivePlayerComponent = $state<typeof ImmersivePlayer>();
  let createPlayer = $state<PlayerFactory>();
  let playerLoading: Promise<void> | undefined;
  let launchingEpisodeId = $state<string | null>(null);
  let launching = $state(false);
  let episodeLaunchController: AbortController | null = null;

  // Account Preferences (saved in localStorage for persistence)
  let prefAudio = $state('fr');
  let prefSubtitles = $state('forced');

  async function launchEpisode(episode: Work, versionId: string): Promise<void> {
    if (episodeLaunchController) {
      episodeLaunchController.abort();
      episodeLaunchController = null;
    }
    if (!episode.synthetic) preloadProgress(episode.id);
    const controller = new AbortController();
    episodeLaunchController = controller;
    const launchGen = getSessionGeneration();
    launchingEpisodeId = episode.id;
    mediaError = '';

    try {
      const descriptor = (async () => {
        if (versionId !== 'auto') return revalidateMediaDescriptor(versionId, controller.signal);
        const ids = playbackIds(episode);
        const known = ids.map(getCachedMedia).filter((item): item is Media => item !== undefined);
        const probe = document.createElement('video');
        if (known.length === ids.length) {
          const candidate = bestVersion(known, mime => probe.canPlayType(mime));
          if (!candidate) throw new Error('Aucune version disponible.');
          return revalidateMediaDescriptor(candidate.id, controller.signal);
        }
        const fresh = await fetchMediaDescriptors(ids, controller.signal, { revalidate: true });
        const candidate = bestVersion(fresh, mime => probe.canPlayType(mime));
        if (!candidate) throw new Error('Aucune version disponible.');
        return candidate;
      })();
      void descriptor.catch(() => {});
      await loadPlayer();
      await tick();
      if (controller.signal.aborted || launchGen !== getSessionGeneration()) return;
      const openedHere = immersive?.open(descriptor);
      const item = await descriptor;
      if (controller.signal.aborted || launchGen !== getSessionGeneration()) return;
      const cachedForEpisode = playbackIds(episode)
        .map(eid => (eid === item.id ? item : getCachedMedia(eid)))
        .filter((m): m is Media => m !== undefined);
      const otherMedia = media.filter(m => !cachedForEpisode.some(c => c.id === m.id));
      media = [...otherMedia, ...cachedForEpisode];
      playingEpisode = episode;
      await tick();
      if (controller.signal.aborted || launchGen !== getSessionGeneration()) return;
      if (!openedHere) immersive?.changeMedia(item, true);
    } catch (err) {
      if (!controller.signal.aborted && launchGen === getSessionGeneration()) {
        media = media.filter(m => m.id !== versionId && getCachedMedia(m.id) !== undefined);
        mediaError = err instanceof Error ? err.message : 'Chargement de l’épisode impossible.';
        // Revalidation failed (e.g. HTTP 403): do not leave the player open on an unusable source.
        immersive?.close();
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

  beforeNavigate(() => {
    if (!workId) saveCatalogScroll(page.url.pathname + page.url.search, window.scrollY);
  });
  afterNavigate(() => {
    if (workId) return;
    const key = page.url.pathname + page.url.search;
    const generation = getSessionGeneration();
    const y = catalogScroll(key);
    if (y !== undefined) void tick().then(() => requestAnimationFrame(() => {
      if (key === page.url.pathname + page.url.search && generation === getSessionGeneration()) window.scrollTo({top: y, behavior: 'instant'});
    }));
  });

  const workId = $derived(page.url.searchParams.get('work') ?? '');
  const libraryId = $derived(page.url.searchParams.get('library') ?? '');
  const library = $derived($accessibleLibraries.find(item=>item.id===libraryId));
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
    library?.name ?? (section === 'settings'
      ? 'Paramètres'
      : section === 'films'
        ? 'Films'
        : section === 'series'
          ? 'Séries'
          : section === 'search'
            ? 'Recherche'
            : 'Accueil')
  );
  const allowedIds = $derived(detail ? playbackIds(detail) : []);
  const playableMedia = $derived(media.filter(item => allowedIds.includes(item.id)));
  const episodeIds = $derived(playingEpisode ? playbackIds(playingEpisode) : []);
  const playerMedia = $derived(detail?.kind === 'series' ? media.filter(item => episodeIds.includes(item.id)) : playableMedia);

  const nextEpisode = $derived.by(() => {
    if (!playingEpisode || detail?.kind !== 'series') return undefined;
    if (detail.episodesPage) return followingEpisode;
    const episodes = (detail.seasons ?? []).flatMap(s => s.episodes);
    const index = episodes.findIndex(episode => episode.id === playingEpisode?.id);
    return index >= 0 ? episodes[index + 1] : undefined;
  });

  $effect(() => {
    const work = detail;
    const episode = playingEpisode;
    followingEpisode = undefined;
    if (!work?.episodesPage || !episode || work.kind !== 'series') return;
    const controller = new AbortController();
    const generation = getSessionGeneration();
    const params = new URLSearchParams({ episodeAfter: episode.id, episodeLimit: '1' });
    void request<Work>(`/api/v1/catalog/${encodeURIComponent(work.id)}?${params}`, controller.signal).then(result => {
      if (!controller.signal.aborted && generation === getSessionGeneration()) {
        followingEpisode = result.seasons?.flatMap(season => season.episodes)[0];
      }
    }).catch(() => {});
    return () => controller.abort();
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
    if (detail && !detail.synthetic) preloadProgress(detail.id);
    const launchGen = getSessionGeneration();
    launching = true;
    mediaError = '';

    try {
      const descriptor = revalidateMediaDescriptor(targetId, controller.signal);
      // Attach rejection immediately while the optional module download completes.
      void descriptor.catch(() => {});
      await loadPlayer();
      await tick();
      if (controller.signal.aborted || launchGen !== getSessionGeneration()) return;
      immersive?.open(descriptor);
      const item = await descriptor;
      if (controller.signal.aborted || launchGen !== getSessionGeneration()) return;
      media = [...media.filter(m => m.id !== item.id), item];
      selected = item.id;
      await tick();
      if (controller.signal.aborted || launchGen !== getSessionGeneration()) return;
    } catch (err) {
      if (!controller.signal.aborted && launchGen === getSessionGeneration()) {
        media = media.filter(m => m.id !== targetId && getCachedMedia(m.id) !== undefined);
        mediaError = err instanceof Error ? err.message : 'Chargement des sources impossible.';
        immersive?.close();
      }
    } finally {
      if (episodeLaunchController === controller) {
        episodeLaunchController = null;
        launching = false;
      }
    }
  }

  async function handleDirectPlay(work: Work): Promise<void> {
    if (work.kind === 'series') {
      // detailUrl already applies SvelteKit resolve to the route and base.
      // eslint-disable-next-line svelte/no-navigation-without-resolve
      await goto(detailUrl(work));
      return;
    }
    const ids = playbackIds(work);
    const targetId = ids[0];
    if (!targetId) {
      // detailUrl already applies SvelteKit resolve to the route and base.
      // eslint-disable-next-line svelte/no-navigation-without-resolve
      await goto(detailUrl(work));
      return;
    }
    try {
      detail = work;
      selected = targetId;
      await openPlayer();
    } catch {
      // detailUrl already applies SvelteKit resolve to the route and base.
      // eslint-disable-next-line svelte/no-navigation-without-resolve
      await goto(detailUrl(work));
    }
  }

  onMount(() => {
    const controller = new AbortController();
    void loadArtworkRotationPolicy(controller.signal);
    fetchBrandName(controller.signal)
      .then(brandName => {
        name = brandName;
      })
      .catch(() => {
        if (!controller.signal.aborted) {
          mediaError = 'La lecture est temporairement indisponible. Rechargez la page pour réessayer.';
        }
      });

    void ensurePreferences().then(saved => {
      if (controller.signal.aborted || !saved) return;
      const savedAudio = saved.audioLanguage;
      if (savedAudio) prefAudio = savedAudio;
      const savedSub = saved.subtitles;
      if (savedSub) prefSubtitles = savedSub;
    }).catch(() => {});

    const artworkChanged=(event:Event)=>{
      const id=(event as CustomEvent<{id:string}>).detail.id;
      invalidateCatalog(detail?.libraryId);
      if(detail?.id===id)void request<Work>(`/api/v1/catalog/${encodeURIComponent(id)}?episodeLimit=24`,controller.signal).then(work=>{if(!controller.signal.aborted&&detail?.id===id)detail=work;}).catch(()=>{});
    };
    window.addEventListener('media-artwork-updated',artworkChanged);
    return () => {controller.abort();window.removeEventListener('media-artwork-updated',artworkChanged);};
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

  $effect.pre(() => {
    void $catalogRevision;
    const id = workId;
    const q = query;
    const currentLibrary = libraryId;
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

    launching = false;
    pendingCatalogKinds = 0;
    partialCatalogError = '';
    error = '';
    mediaError = '';
    detail = null;
    media = [];
    totalFilms = null;
    totalSeries = null;
    moreFilms = false;
    moreSeries = false;
    playingEpisode = null;

    const kinds = currentSection === 'films' ? ['film'] : currentSection === 'series' ? ['series'] : ['film', 'series'];
    const requests = kinds.map(kind => {
      const url = `/api/v1/catalog?${new URLSearchParams({ ...filters, ...(currentLibrary ? {library: currentLibrary} : {}), kind, q, sort: sorting, offset: String(start), limit: '24', count: 'false' })}`;
      return { kind, url, key: url };
    });
    let cachedKinds = 0;
    if (!id) for (const {kind, key} of requests) {
      const cached = cachedCatalog(key);
      if (!cached) continue;
      cachedKinds++;
      if (kind === 'film') { films = cached.items; totalFilms = cached.total; moreFilms = cached.hasMore ?? (cached.total ?? 0) > start + 24; }
      else { series = cached.items; totalSeries = cached.total; moreSeries = cached.hasMore ?? (cached.total ?? 0) > start + 24; }
    }
    if (cachedKinds === 0) {
      films = [];
      series = [];
    }
    busy = Boolean(id) || cachedKinds === 0;

    async function load() {
      try {
        if (currentSection === 'settings') return;
        if (id) {
          if (currentSection !== 'films' && currentSection !== 'series') {
            throw new Error('Ouvrez ce média depuis sa bibliothèque Films ou Séries.');
          }
          void loadPlayer().catch(() => { /* A playback attempt reports the import error and can retry. */ });
          const work = await request<Work>(`/api/v1/catalog/${encodeURIComponent(id)}?episodeLimit=24&episodeOffset=0`, controller.signal);
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
          pendingCatalogKinds = kinds.length;
          let completedKinds = 0;
          const results = await Promise.allSettled(
            requests.map(async ({kind, key, url}) => {
              try {
                const result = await loadCatalog(key, url);
                if (controller.signal.aborted || effectSessionGen !== getSessionGeneration()) return;
                if (kind === 'film') {
                  films = result.items;
                  totalFilms = result.total;
                  moreFilms = result.hasMore ?? (result.total ?? 0) > start + 24;
                } else {
                  series = result.items;
                  totalSeries = result.total;
                  moreSeries = result.hasMore ?? (result.total ?? 0) > start + 24;
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
          void loadPlayer().catch(() => {});
          const failure = results.find(result => result.status === 'rejected');
          if (failure?.status === 'rejected') {
            if (failure.reason instanceof Error && /accès|session/.test(failure.reason.message)) { films = []; series = []; throw failure.reason; }
            if (completedKinds === 0 && cachedKinds === 0) throw failure.reason;
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

<main id="main-content" tabindex="-1">
  {#if launching}<p role="status">Connexion à la source… <button onclick={()=>{episodeLaunchController?.abort();episodeLaunchController=null;launching=false;}}>Annuler</button></p>{/if}
  {#if !busy && pendingCatalogKinds > 0}<p role="status">Chargement du reste du catalogue…</p>{/if}
  {#if partialCatalogError}<p role="alert">{partialCatalogError}</p>{/if}
  {#if section === 'home' && $serverPresentation.welcomeMessage}<p class="server-welcome">{$serverPresentation.welcomeMessage}</p>{/if}
  {#if section === 'settings'}
    {#await import('./SettingsView.svelte')}
      <CatalogSkeleton label="Chargement des paramètres…" />
    {:then { default: SettingsView }}
    <SettingsView
      bind:prefAudio
      bind:prefSubtitles
      onSavePreference={savePreference}
    />
    {:catch}<p role="alert">Chargement des paramètres impossible. Rechargez la page pour réessayer.</p>{/await}

  {:else if busy}
    <div class="page-heading">
      <span class="eyebrow">{title}</span>
      <h1>Votre médiathèque</h1>
    </div>
    <CatalogSkeleton />

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
      {#await import('./FilmDetail.svelte')}
        <CatalogSkeleton label="Chargement du film…" />
      {:then { default: FilmDetail }}
      <FilmDetail
        work={detail}
        {playableMedia}
        {allowedIds}
        bind:selected
        {mediaError}
        similarCandidates={films}
        onPlay={openPlayer}
      />
      {:catch}<p role="alert">Chargement du film impossible. Rechargez la page pour réessayer.</p>{/await}
    {:else}
      {#await import('./SeriesDetail.svelte')}
        <CatalogSkeleton label="Chargement des épisodes…" />
      {:then { default: SeriesDetail }}
      <SeriesDetail
        work={detail}
        bind:season
        {media}
        {mediaError}
        similarCandidates={series}
        {immersive}
        {launchingEpisodeId}
        onLaunchEpisode={launchEpisode}
        onSelectEpisode={episode => (playingEpisode = episode)}
      />
      {:catch}<p role="alert">Chargement des épisodes impossible. Rechargez la page pour réessayer.</p>{/await}
    {/if}

  {:else if section === 'home'}
    <!-- Refined, Immersive Home View -->
    <CatalogHome
      {films}
      {series}
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
      {moreFilms}
      {moreSeries}
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
    width: 100%;
    max-width: none;
    margin: 0;
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
